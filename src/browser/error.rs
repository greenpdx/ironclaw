//! Browser-specific error types.

use std::time::Duration;

/// Errors from browser automation operations.
#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("Failed to launch browser: {0}")]
    LaunchFailed(String),

    #[error("Navigation to {url} failed: {reason}")]
    NavigationFailed { url: String, reason: String },

    #[error("Element not found: {selector}")]
    ElementNotFound { selector: String },

    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    #[error("JavaScript error: {0}")]
    JavaScriptError(String),

    #[error("Operation timed out after {0:?}: {1}")]
    Timeout(Duration, String),

    #[error("Invalid URL: {url}: {reason}")]
    InvalidUrl { url: String, reason: String },

    #[error("Browser session is closed")]
    SessionClosed,
}
