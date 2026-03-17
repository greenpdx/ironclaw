use std::path::PathBuf;

use crate::config::helpers::{optional_env, parse_bool_env, parse_optional_env};
use crate::error::ConfigError;
use crate::settings::Settings;

/// Local speech-to-text configuration.
#[derive(Debug, Clone)]
pub struct SpeechConfig {
    /// Whether local STT is enabled.
    pub enabled: bool,
    /// Path to the whisper GGML model file.
    pub model_path: PathBuf,
    /// Audio sample rate in Hz (default: 16000).
    pub sample_rate: u32,
    /// Silence detection threshold (0.0–1.0, default: 0.01).
    pub silence_threshold: f32,
    /// Minimum speech duration in frames before triggering (default: 3).
    pub min_speech_frames: usize,
    /// Trailing silence frames before finalizing (default: 30).
    pub trailing_silence_frames: usize,
}

impl Default for SpeechConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_path: default_model_path(),
            sample_rate: 16_000,
            silence_threshold: 0.01,
            min_speech_frames: 3,
            trailing_silence_frames: 30,
        }
    }
}

/// Default model path: ~/.ironclaw/models/ggml-base.en.bin
fn default_model_path() -> PathBuf {
    crate::bootstrap::ironclaw_base_dir()
        .join("models")
        .join("ggml-base.en.bin")
}

impl SpeechConfig {
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        let enabled = parse_bool_env(
            "SPEECH_ENABLED",
            settings.speech.as_ref().is_some_and(|s| s.enabled),
        )?;

        let model_path = optional_env("SPEECH_MODEL_PATH")?
            .map(PathBuf::from)
            .unwrap_or_else(default_model_path);

        let sample_rate = parse_optional_env("SPEECH_SAMPLE_RATE", 16_000)?;
        let silence_threshold: f32 = optional_env("SPEECH_SILENCE_THRESHOLD")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01);
        let min_speech_frames = parse_optional_env("SPEECH_MIN_SPEECH_FRAMES", 3)?;
        let trailing_silence_frames =
            parse_optional_env("SPEECH_TRAILING_SILENCE_FRAMES", 30)?;

        Ok(Self {
            enabled,
            model_path,
            sample_rate,
            silence_threshold,
            min_speech_frames,
            trailing_silence_frames,
        })
    }

    /// Create a `LocalSttConfig` for the speech engine.
    #[cfg(feature = "speech")]
    pub fn to_stt_config(&self) -> crate::speech::local_stt::LocalSttConfig {
        crate::speech::local_stt::LocalSttConfig {
            model_path: self.model_path.clone(),
            sample_rate: self.sample_rate,
            vad: crate::speech::VadConfig {
                silence_threshold: self.silence_threshold,
                min_speech_frames: self.min_speech_frames,
                trailing_silence_frames: self.trailing_silence_frames,
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::helpers::ENV_MUTEX;

    #[test]
    fn default_config_is_disabled() {
        let config = SpeechConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.sample_rate, 16_000);
        assert!((config.silence_threshold - 0.01).abs() < f32::EPSILON);
        assert_eq!(config.min_speech_frames, 3);
        assert_eq!(config.trailing_silence_frames, 30);
        assert!(config.model_path.ends_with("ggml-base.en.bin"));
    }

    #[test]
    fn resolve_defaults_without_env_vars() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let settings = Settings::default();
        let config = SpeechConfig::resolve(&settings).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.sample_rate, 16_000);
    }
}
