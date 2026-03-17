use crate::config::helpers::{optional_env, parse_bool_env, parse_optional_env};
use crate::error::ConfigError;
use crate::settings::Settings;

/// Browser automation configuration.
#[derive(Debug, Clone, Default)]
pub struct BrowserConfig {
    /// Whether browser automation is enabled.
    pub enabled: bool,
    /// Session config passed to the browser session.
    pub session: BrowserSessionConfig,
}

/// Configuration for a browser session.
#[derive(Debug, Clone)]
pub struct BrowserSessionConfig {
    /// Path to Chrome/Chromium binary (auto-detected if not set).
    pub chrome_path: Option<String>,
    /// Run in headless mode (default: true).
    pub headless: bool,
    /// Viewport width in pixels.
    pub viewport_width: u32,
    /// Viewport height in pixels.
    pub viewport_height: u32,
    /// Navigation timeout in seconds.
    pub navigation_timeout_secs: u64,
}

impl Default for BrowserSessionConfig {
    fn default() -> Self {
        Self {
            chrome_path: None,
            headless: true,
            viewport_width: 1280,
            viewport_height: 720,
            navigation_timeout_secs: 30,
        }
    }
}


impl BrowserConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let enabled = parse_bool_env(
            "BROWSER_ENABLED",
            settings.browser.as_ref().is_some_and(|b| b.enabled),
        )?;

        let chrome_path = optional_env("BROWSER_CHROME_PATH")?;
        let headless = parse_bool_env("BROWSER_HEADLESS", true)?;
        let viewport_width = parse_optional_env("BROWSER_VIEWPORT_WIDTH", 1280)?;
        let viewport_height = parse_optional_env("BROWSER_VIEWPORT_HEIGHT", 720)?;
        let navigation_timeout_secs =
            parse_optional_env("BROWSER_NAVIGATION_TIMEOUT", 30)?;

        Ok(Self {
            enabled,
            session: BrowserSessionConfig {
                chrome_path,
                headless,
                viewport_width,
                viewport_height,
                navigation_timeout_secs,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::helpers::ENV_MUTEX;

    #[test]
    fn default_config_is_disabled() {
        let config = BrowserConfig::default();
        assert!(!config.enabled);
        assert!(config.session.headless);
        assert_eq!(config.session.viewport_width, 1280);
        assert_eq!(config.session.viewport_height, 720);
        assert_eq!(config.session.navigation_timeout_secs, 30);
        assert!(config.session.chrome_path.is_none());
    }

    #[test]
    fn resolve_defaults_without_env_vars() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let settings = Settings::default();
        let config = BrowserConfig::resolve(&settings).unwrap();
        assert!(!config.enabled);
        assert!(config.session.headless);
        assert_eq!(config.session.viewport_width, 1280);
    }

    #[test]
    fn session_config_default() {
        let config = BrowserSessionConfig::default();
        assert!(config.headless);
        assert!(config.chrome_path.is_none());
        assert_eq!(config.viewport_width, 1280);
        assert_eq!(config.viewport_height, 720);
        assert_eq!(config.navigation_timeout_secs, 30);
    }
}
