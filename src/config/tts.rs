use secrecy::SecretString;

use crate::config::helpers::{optional_env, parse_bool_env, parse_string_env};
use crate::error::ConfigError;
use crate::settings::Settings;

/// TTS pipeline configuration.
#[derive(Debug, Clone)]
pub struct TtsConfig {
    /// Whether TTS is enabled.
    pub enabled: bool,
    /// Provider: "openai" or "elevenlabs".
    pub provider: String,
    /// API key (falls back to OPENAI_API_KEY for OpenAI provider).
    pub api_key: Option<SecretString>,
    /// Model to use (default: "tts-1" for OpenAI).
    pub model: String,
    /// Voice to use (default: "alloy" for OpenAI).
    pub voice: String,
    /// Base URL override for the TTS API.
    pub base_url: Option<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "openai".to_string(),
            api_key: None,
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            base_url: None,
        }
    }
}

impl TtsConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let enabled = parse_bool_env(
            "TTS_ENABLED",
            settings.tts.as_ref().is_some_and(|t| t.enabled),
        )?;

        let provider = parse_string_env("TTS_PROVIDER", "openai")?;

        // TTS_API_KEY takes priority; fall back to OPENAI_API_KEY for OpenAI provider
        let api_key = optional_env("TTS_API_KEY")?
            .or_else(|| {
                if provider == "openai" {
                    optional_env("OPENAI_API_KEY").ok().flatten()
                } else {
                    None
                }
            })
            .map(SecretString::from);

        let model = parse_string_env("TTS_MODEL", "tts-1")?;
        let voice = parse_string_env("TTS_VOICE", "alloy")?;
        let base_url = optional_env("TTS_BASE_URL")?;

        Ok(Self {
            enabled,
            provider,
            api_key,
            model,
            voice,
            base_url,
        })
    }

    /// Create the TTS provider if enabled and configured.
    pub fn create_provider(&self) -> Option<Box<dyn crate::tts::TtsProvider>> {
        if !self.enabled {
            return None;
        }

        let api_key = self.api_key.as_ref()?;

        match self.provider.as_str() {
            "openai" => {
                tracing::info!(model = %self.model, voice = %self.voice, "TTS enabled via OpenAI");
                let mut provider = crate::tts::OpenAiTtsProvider::new(api_key.clone())
                    .with_model(&self.model)
                    .with_voice(&self.voice);
                if let Some(ref base_url) = self.base_url {
                    provider = provider.with_base_url(base_url);
                }
                Some(Box::new(provider))
            }
            "elevenlabs" => {
                tracing::info!(voice = %self.voice, "TTS enabled via ElevenLabs");
                let mut provider = crate::tts::ElevenLabsProvider::new(api_key.clone())
                    .with_voice_id(&self.voice);
                if let Some(ref base_url) = self.base_url {
                    provider = provider.with_base_url(base_url);
                }
                Some(Box::new(provider))
            }
            other => {
                tracing::warn!(provider = %other, "Unknown TTS provider, TTS disabled");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::helpers::ENV_MUTEX;

    #[test]
    fn default_config_is_disabled() {
        let config = TtsConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "tts-1");
        assert_eq!(config.voice, "alloy");
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
    }

    #[test]
    fn create_provider_returns_none_when_disabled() {
        let config = TtsConfig::default();
        assert!(config.create_provider().is_none());
    }

    #[test]
    fn create_provider_returns_none_without_api_key() {
        let config = TtsConfig {
            enabled: true,
            ..TtsConfig::default()
        };
        assert!(config.create_provider().is_none());
    }

    #[test]
    fn create_provider_returns_openai_provider() {
        let config = TtsConfig {
            enabled: true,
            api_key: Some(SecretString::from("test-key".to_string())),
            ..TtsConfig::default()
        };
        let provider = config.create_provider();
        assert!(provider.is_some());
        assert_eq!(provider.as_ref().map(|p| p.name()), Some("openai"));
    }

    #[test]
    fn create_provider_returns_elevenlabs_provider() {
        let config = TtsConfig {
            enabled: true,
            provider: "elevenlabs".to_string(),
            api_key: Some(SecretString::from("test-key".to_string())),
            ..TtsConfig::default()
        };
        let provider = config.create_provider();
        assert!(provider.is_some());
        assert_eq!(provider.as_ref().map(|p| p.name()), Some("elevenlabs"));
    }

    #[test]
    fn create_provider_returns_none_for_unknown_provider() {
        let config = TtsConfig {
            enabled: true,
            provider: "unknown".to_string(),
            api_key: Some(SecretString::from("test-key".to_string())),
            ..TtsConfig::default()
        };
        assert!(config.create_provider().is_none());
    }

    #[test]
    fn resolve_defaults_without_env_vars() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let settings = Settings::default();
        let config = TtsConfig::resolve(&settings).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "tts-1");
        assert_eq!(config.voice, "alloy");
    }
}
