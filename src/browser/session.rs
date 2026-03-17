//! Lazy browser session management.

use std::sync::Arc;
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use tokio::sync::Mutex;

use super::BrowserError;
use crate::config::browser::BrowserSessionConfig;

/// Inner state of an active browser session.
struct SessionInner {
    _browser: Arc<Browser>,
    page: Page,
}

/// Lazy browser session that launches Chrome on first use.
///
/// Thread-safe: multiple tools share the same session via `Arc<BrowserSession>`.
/// Operations are serialized by the inner mutex to prevent concurrent page mutations.
pub struct BrowserSession {
    config: BrowserSessionConfig,
    inner: Mutex<Option<SessionInner>>,
}

impl BrowserSession {
    /// Create a new browser session with the given configuration.
    ///
    /// Chrome is not launched until the first operation.
    pub fn new(config: BrowserSessionConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(None),
        }
    }

    /// Ensure a browser page is available, launching Chrome if needed.
    async fn ensure_page(&self) -> Result<tokio::sync::MutexGuard<'_, Option<SessionInner>>, BrowserError> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let inner = self.launch().await?;
            *guard = Some(inner);
        }
        Ok(guard)
    }

    /// Launch a new headless Chrome instance.
    async fn launch(&self) -> Result<SessionInner, BrowserError> {
        let mut builder = BrowserConfig::builder();

        // chromiumoxide is headless by default; with_head() enables the GUI.
        if !self.config.headless {
            builder = builder.with_head();
        }

        builder = builder
            .viewport(
                chromiumoxide::handler::viewport::Viewport {
                    width: self.config.viewport_width,
                    height: self.config.viewport_height,
                    device_scale_factor: None,
                    emulating_mobile: false,
                    is_landscape: false,
                    has_touch: false,
                },
            )
            .arg("--no-sandbox")
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-extensions");

        if let Some(ref path) = self.config.chrome_path {
            builder = builder.chrome_executable(path);
        }

        let config = builder
            .build()
            .map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;

        // Spawn the CDP handler task
        tokio::spawn(async move {
            while handler.next().await.is_some() {}
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| BrowserError::LaunchFailed(format!("failed to create page: {e}")))?;

        tracing::info!("Browser session launched");

        Ok(SessionInner {
            _browser: Arc::new(browser),
            page,
        })
    }

    /// Navigate to a URL. Returns the final URL and page title.
    pub async fn navigate(&self, url: &str) -> Result<(String, String), BrowserError> {
        validate_browser_url(url)?;

        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        inner
            .page
            .goto(url)
            .await
            .map_err(|e| BrowserError::NavigationFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        // Wait for navigation to settle
        tokio::time::sleep(Duration::from_millis(500)).await;

        let final_url = inner
            .page
            .url()
            .await
            .map_err(|e| BrowserError::NavigationFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?
            .map(|u| u.to_string())
            .unwrap_or_else(|| url.to_string());

        let title = inner
            .page
            .get_title()
            .await
            .map_err(|e| BrowserError::NavigationFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?
            .unwrap_or_default();

        Ok((final_url, title))
    }

    /// Take a screenshot of the current page (or a specific element).
    pub async fn screenshot(&self, selector: Option<&str>) -> Result<Vec<u8>, BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        if let Some(sel) = selector {
            let element = inner
                .page
                .find_element(sel)
                .await
                .map_err(|_| BrowserError::ElementNotFound {
                    selector: sel.to_string(),
                })?;
            element
                .screenshot(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
                .await
                .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))
        } else {
            inner
                .page
                .screenshot(
                    chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams::builder()
                        .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
                        .build(),
                )
                .await
                .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))
        }
    }

    /// Click an element by CSS selector.
    pub async fn click(&self, selector: &str) -> Result<(), BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        let element = inner
            .page
            .find_element(selector)
            .await
            .map_err(|_| BrowserError::ElementNotFound {
                selector: selector.to_string(),
            })?;

        element
            .click()
            .await
            .map_err(|e| BrowserError::JavaScriptError(format!("click failed: {e}")))?;

        Ok(())
    }

    /// Type text into an element by CSS selector.
    pub async fn type_text(&self, selector: &str, text: &str) -> Result<(), BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        let element = inner
            .page
            .find_element(selector)
            .await
            .map_err(|_| BrowserError::ElementNotFound {
                selector: selector.to_string(),
            })?;

        element
            .click()
            .await
            .map_err(|e| BrowserError::JavaScriptError(format!("focus failed: {e}")))?;

        element
            .type_str(text)
            .await
            .map_err(|e| BrowserError::JavaScriptError(format!("type failed: {e}")))?;

        Ok(())
    }

    /// Extract text content and optional attribute from elements matching a selector.
    pub async fn extract(
        &self,
        selector: &str,
        attribute: Option<&str>,
    ) -> Result<Vec<ExtractedElement>, BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        let elements = inner
            .page
            .find_elements(selector)
            .await
            .map_err(|_| BrowserError::ElementNotFound {
                selector: selector.to_string(),
            })?;

        let mut results = Vec::new();
        for element in elements {
            let text = element
                .inner_text()
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

            let attr_value = if let Some(attr) = attribute {
                element.attribute(attr).await.ok().flatten()
            } else {
                None
            };

            results.push(ExtractedElement {
                text,
                attribute: attr_value,
            });
        }

        Ok(results)
    }

    /// Evaluate JavaScript on the current page.
    pub async fn evaluate_js(&self, script: &str) -> Result<serde_json::Value, BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        let result = inner
            .page
            .evaluate_expression(script)
            .await
            .map_err(|e| BrowserError::JavaScriptError(e.to_string()))?;

        result
            .into_value()
            .map_err(|e| BrowserError::JavaScriptError(format!("failed to parse result: {e}")))
    }

    /// Get the full page content as HTML or plain text.
    pub async fn get_content(&self, as_text: bool) -> Result<String, BrowserError> {
        let guard = self.ensure_page().await?;
        let inner = guard.as_ref().ok_or(BrowserError::SessionClosed)?;

        let content = inner
            .page
            .content()
            .await
            .map_err(|e| BrowserError::JavaScriptError(format!("failed to get content: {e}")))?;

        if as_text {
            // Strip HTML tags via JS for cleaner text extraction
            let text: String = inner
                .page
                .evaluate_expression(
                    "document.body ? document.body.innerText : document.documentElement.textContent || ''",
                )
                .await
                .map_err(|e| BrowserError::JavaScriptError(e.to_string()))?
                .into_value()
                .unwrap_or_default();
            Ok(text)
        } else {
            Ok(content)
        }
    }
}

/// An extracted element's text and optional attribute value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractedElement {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
}

/// Validate a URL for browser navigation.
///
/// Rejects dangerous schemes and private/localhost addresses.
fn validate_browser_url(url: &str) -> Result<(), BrowserError> {
    let parsed = url::Url::parse(url).map_err(|e| BrowserError::InvalidUrl {
        url: url.to_string(),
        reason: e.to_string(),
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(BrowserError::InvalidUrl {
                url: url.to_string(),
                reason: format!("scheme '{scheme}' is not allowed; use http or https"),
            });
        }
    }

    if let Some(host) = parsed.host_str() {
        let host_lower = host.to_ascii_lowercase();
        if host_lower == "localhost"
            || host_lower.ends_with(".localhost")
            || host_lower == "127.0.0.1"
            || host_lower == "[::1]"
            || host_lower == "0.0.0.0"
        {
            return Err(BrowserError::InvalidUrl {
                url: url.to_string(),
                reason: "localhost/loopback addresses are not allowed".to_string(),
            });
        }

        // Block cloud metadata endpoints
        if host_lower == "169.254.169.254" || host_lower == "metadata.google.internal" {
            return Err(BrowserError::InvalidUrl {
                url: url.to_string(),
                reason: "cloud metadata endpoints are not allowed".to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_browser_url_accepts_https() {
        assert!(validate_browser_url("https://example.com").is_ok());
    }

    #[test]
    fn validate_browser_url_accepts_http() {
        assert!(validate_browser_url("http://example.com").is_ok());
    }

    #[test]
    fn validate_browser_url_rejects_file_scheme() {
        let err = validate_browser_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("scheme 'file' is not allowed"));
    }

    #[test]
    fn validate_browser_url_rejects_javascript_scheme() {
        let err = validate_browser_url("javascript:alert(1)").unwrap_err();
        assert!(
            err.to_string().contains("javascript"),
            "got: {}",
            err
        );
    }

    #[test]
    fn validate_browser_url_rejects_data_scheme() {
        let err = validate_browser_url("data:text/html,<h1>hi</h1>").unwrap_err();
        assert!(err.to_string().contains("scheme 'data' is not allowed"));
    }

    #[test]
    fn validate_browser_url_rejects_localhost() {
        assert!(validate_browser_url("https://localhost/admin").is_err());
        assert!(validate_browser_url("https://localhost:8080").is_err());
        assert!(validate_browser_url("http://sub.localhost/").is_err());
    }

    #[test]
    fn validate_browser_url_rejects_loopback_ip() {
        assert!(validate_browser_url("http://127.0.0.1/").is_err());
        assert!(validate_browser_url("http://0.0.0.0/").is_err());
    }

    #[test]
    fn validate_browser_url_rejects_metadata_endpoint() {
        assert!(validate_browser_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(
            validate_browser_url("http://metadata.google.internal/computeMetadata/v1/").is_err()
        );
    }

    #[test]
    fn validate_browser_url_rejects_invalid_url() {
        assert!(validate_browser_url("not a url").is_err());
    }
}
