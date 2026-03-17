//! Browser automation tools.
//!
//! Seven built-in tools that share a single `Arc<BrowserSession>`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use super::BrowserSession;
use crate::context::JobContext;
use crate::tools::{
    ApprovalRequirement, Tool, ToolDomain, ToolError, ToolOutput, ToolRateLimitConfig, require_str,
};

// ---------------------------------------------------------------------------
// browser_navigate
// ---------------------------------------------------------------------------

/// Navigate the browser to a URL.
pub struct BrowserNavigateTool {
    session: Arc<BrowserSession>,
}

impl BrowserNavigateTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn description(&self) -> &str {
        "Navigate the browser to a URL. Returns the final URL and page title."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to (http or https only)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let url = require_str(&params, "url")?;

        let (final_url, title) = self
            .session
            .navigate(url)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "url": final_url,
                "title": title,
            }),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        Some(ToolRateLimitConfig::new(20, 200))
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
}

// ---------------------------------------------------------------------------
// browser_screenshot
// ---------------------------------------------------------------------------

/// Take a screenshot of the current page or a specific element.
pub struct BrowserScreenshotTool {
    session: Arc<BrowserSession>,
}

impl BrowserScreenshotTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "Take a PNG screenshot of the current browser page, or a specific element by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector to screenshot a specific element"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let selector = params.get("selector").and_then(|v| v.as_str());

        let png_data = self
            .session
            .screenshot(selector)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        let encoded = base64::engine::general_purpose::STANDARD.encode(&png_data);

        Ok(ToolOutput::success(
            serde_json::json!({
                "image_base64": encoded,
                "format": "png",
                "size_bytes": png_data.len(),
            }),
            start.elapsed(),
        ))
    }

    fn requires_sanitization(&self) -> bool {
        false // Screenshot is binary data, not untrusted text
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
}

// ---------------------------------------------------------------------------
// browser_click
// ---------------------------------------------------------------------------

/// Click an element by CSS selector.
pub struct BrowserClickTool {
    session: Arc<BrowserSession>,
}

impl BrowserClickTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }

    fn description(&self) -> &str {
        "Click an element on the current browser page by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector of the element to click"
                }
            },
            "required": ["selector"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let selector = require_str(&params, "selector")?;

        self.session
            .click(selector)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        Ok(ToolOutput::success(
            serde_json::json!({"success": true}),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        Some(ToolRateLimitConfig::new(30, 300))
    }
}

// ---------------------------------------------------------------------------
// browser_type
// ---------------------------------------------------------------------------

/// Type text into an element by CSS selector.
pub struct BrowserTypeTool {
    session: Arc<BrowserSession>,
}

impl BrowserTypeTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }

    fn description(&self) -> &str {
        "Type text into an input element on the current browser page by CSS selector."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector of the input element"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type into the element"
                }
            },
            "required": ["selector", "text"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let selector = require_str(&params, "selector")?;
        let text = require_str(&params, "text")?;

        self.session
            .type_text(selector, text)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        Ok(ToolOutput::success(
            serde_json::json!({"success": true}),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn sensitive_params(&self) -> &[&str] {
        &["text"]
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        Some(ToolRateLimitConfig::new(30, 300))
    }
}

// ---------------------------------------------------------------------------
// browser_extract
// ---------------------------------------------------------------------------

/// Extract text and attributes from elements by CSS selector.
pub struct BrowserExtractTool {
    session: Arc<BrowserSession>,
}

impl BrowserExtractTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserExtractTool {
    fn name(&self) -> &str {
        "browser_extract"
    }

    fn description(&self) -> &str {
        "Extract text content (and optionally an attribute value) from elements matching a CSS selector on the current page."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector to match elements"
                },
                "attribute": {
                    "type": "string",
                    "description": "Optional HTML attribute to extract (e.g. 'href', 'src')"
                }
            },
            "required": ["selector"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let selector = require_str(&params, "selector")?;
        let attribute = params.get("attribute").and_then(|v| v.as_str());

        let elements = self
            .session
            .extract(selector, attribute)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "elements": elements,
                "count": elements.len(),
            }),
            start.elapsed(),
        ))
    }

    fn requires_sanitization(&self) -> bool {
        true // External web content
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }
}

// ---------------------------------------------------------------------------
// browser_eval
// ---------------------------------------------------------------------------

/// Evaluate arbitrary JavaScript on the current page.
pub struct BrowserEvalTool {
    session: Arc<BrowserSession>,
}

impl BrowserEvalTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserEvalTool {
    fn name(&self) -> &str {
        "browser_eval"
    }

    fn description(&self) -> &str {
        "Evaluate a JavaScript expression on the current browser page and return the result."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "JavaScript expression to evaluate"
                }
            },
            "required": ["script"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let script = require_str(&params, "script")?;

        let result = self
            .session
            .evaluate_js(script)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        Ok(ToolOutput::success(
            serde_json::json!({"result": result}),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
}

// ---------------------------------------------------------------------------
// browser_content
// ---------------------------------------------------------------------------

/// Get the full page content as HTML or plain text.
pub struct BrowserContentTool {
    session: Arc<BrowserSession>,
}

impl BrowserContentTool {
    pub fn new(session: Arc<BrowserSession>) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tool for BrowserContentTool {
    fn name(&self) -> &str {
        "browser_content"
    }

    fn description(&self) -> &str {
        "Get the full content of the current browser page as HTML or plain text."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "description": "Output format: 'html' or 'text' (default: 'text')",
                    "enum": ["html", "text"]
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let as_text = params
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            != "html";

        let content = self
            .session
            .get_content(as_text)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        // Truncate to prevent huge pages from overwhelming context
        let max_len = 100_000;
        let truncated = if content.len() > max_len {
            let mut end = max_len;
            // Ensure we don't split a multi-byte UTF-8 character
            while !content.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            format!("{}...\n[truncated at {} bytes]", &content[..end], max_len)
        } else {
            content
        };

        Ok(ToolOutput::text(truncated, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true // External web content
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_tool_schema;

    // Schema validation tests for all 7 tools (don't require a browser)

    fn dummy_session() -> Arc<BrowserSession> {
        Arc::new(BrowserSession::new(
            crate::config::browser::BrowserSessionConfig::default(),
        ))
    }

    #[test]
    fn browser_navigate_schema_valid() {
        let tool = BrowserNavigateTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_navigate");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_screenshot_schema_valid() {
        let tool = BrowserScreenshotTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_screenshot");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_click_schema_valid() {
        let tool = BrowserClickTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_click");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_type_schema_valid() {
        let tool = BrowserTypeTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_type");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_extract_schema_valid() {
        let tool = BrowserExtractTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_extract");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_eval_schema_valid() {
        let tool = BrowserEvalTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_eval");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_content_schema_valid() {
        let tool = BrowserContentTool::new(dummy_session());
        let errors = validate_tool_schema(&tool.parameters_schema(), "browser_content");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }

    #[test]
    fn browser_navigate_metadata() {
        let tool = BrowserNavigateTool::new(dummy_session());
        assert_eq!(tool.name(), "browser_navigate");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::UnlessAutoApproved
        );
        assert!(tool.requires_sanitization());
        assert_eq!(tool.domain(), ToolDomain::Orchestrator);
        assert!(tool.rate_limit_config().is_some());
    }

    #[test]
    fn browser_screenshot_metadata() {
        let tool = BrowserScreenshotTool::new(dummy_session());
        assert_eq!(tool.name(), "browser_screenshot");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::Never
        );
        assert!(!tool.requires_sanitization());
    }

    #[test]
    fn browser_type_has_sensitive_params() {
        let tool = BrowserTypeTool::new(dummy_session());
        assert_eq!(tool.sensitive_params(), &["text"]);
    }

    #[test]
    fn browser_eval_requires_always_approval() {
        let tool = BrowserEvalTool::new(dummy_session());
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::Always
        );
    }

    #[test]
    fn browser_extract_requires_sanitization() {
        let tool = BrowserExtractTool::new(dummy_session());
        assert!(tool.requires_sanitization());
    }

    #[test]
    fn browser_content_requires_sanitization() {
        let tool = BrowserContentTool::new(dummy_session());
        assert!(tool.requires_sanitization());
    }

    #[tokio::test]
    async fn browser_navigate_missing_url() {
        let tool = BrowserNavigateTool::new(dummy_session());
        let ctx = JobContext::default();
        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing 'url'"));
    }

    #[tokio::test]
    async fn browser_click_missing_selector() {
        let tool = BrowserClickTool::new(dummy_session());
        let ctx = JobContext::default();
        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing 'selector'"));
    }

    #[tokio::test]
    async fn browser_type_missing_params() {
        let tool = BrowserTypeTool::new(dummy_session());
        let ctx = JobContext::default();
        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    #[tokio::test]
    async fn browser_eval_missing_script() {
        let tool = BrowserEvalTool::new(dummy_session());
        let ctx = JobContext::default();
        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing 'script'"));
    }

    #[tokio::test]
    async fn browser_extract_missing_selector() {
        let tool = BrowserExtractTool::new(dummy_session());
        let ctx = JobContext::default();
        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing 'selector'"));
    }
}
