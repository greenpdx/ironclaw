//! Browser automation via Chrome DevTools Protocol.
//!
//! Provides a [`BrowserSession`] that lazily launches a headless Chrome
//! instance and exposes navigation, screenshot, click, type, extract,
//! eval, and content retrieval operations.
//!
//! Gated behind `--features browser`.

mod error;
mod session;
mod tools;

pub use self::error::BrowserError;
pub use self::session::BrowserSession;
pub use self::tools::{
    BrowserClickTool, BrowserContentTool, BrowserEvalTool, BrowserExtractTool,
    BrowserNavigateTool, BrowserScreenshotTool, BrowserTypeTool,
};
