//! Text-to-speech pipeline.
//!
//! Provides a [`TtsProvider`] trait for pluggable TTS backends and
//! concrete implementations for OpenAI and ElevenLabs.

mod elevenlabs;
mod openai;

pub use self::elevenlabs::ElevenLabsProvider;
pub use self::openai::OpenAiTtsProvider;

use async_trait::async_trait;

/// Supported audio output formats for TTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOutputFormat {
    Mp3,
    Opus,
    Wav,
    Pcm,
}

impl AudioOutputFormat {
    /// File extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Wav => "wav",
            Self::Pcm => "pcm",
        }
    }

    /// MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/opus",
            Self::Wav => "audio/wav",
            Self::Pcm => "audio/pcm",
        }
    }

    /// Parse from a string, returning `None` for unrecognized formats.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "opus" => Some(Self::Opus),
            "wav" => Some(Self::Wav),
            "pcm" => Some(Self::Pcm),
            _ => None,
        }
    }
}

/// Errors from the TTS pipeline.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("TTS request failed: {0}")]
    RequestFailed(String),

    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),

    #[error("Text is empty")]
    EmptyText,

    #[error("Text too long: {len} chars (max {max})")]
    TextTooLong { len: usize, max: usize },
}

/// Output from a TTS synthesis request.
#[derive(Debug, Clone)]
pub struct TtsOutput {
    /// Raw audio bytes.
    pub audio_data: Vec<u8>,
    /// Format of the audio data.
    pub format: AudioOutputFormat,
}

/// Trait for text-to-speech providers.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Provider name (e.g. "openai", "elevenlabs").
    fn name(&self) -> &str;

    /// Synthesize text into audio.
    async fn synthesize(
        &self,
        text: &str,
        format: AudioOutputFormat,
    ) -> Result<TtsOutput, TtsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_output_format_roundtrip() {
        assert_eq!(
            AudioOutputFormat::from_str_name("mp3"),
            Some(AudioOutputFormat::Mp3)
        );
        assert_eq!(
            AudioOutputFormat::from_str_name("MP3"),
            Some(AudioOutputFormat::Mp3)
        );
        assert_eq!(
            AudioOutputFormat::from_str_name("opus"),
            Some(AudioOutputFormat::Opus)
        );
        assert_eq!(
            AudioOutputFormat::from_str_name("wav"),
            Some(AudioOutputFormat::Wav)
        );
        assert_eq!(
            AudioOutputFormat::from_str_name("pcm"),
            Some(AudioOutputFormat::Pcm)
        );
        assert_eq!(AudioOutputFormat::from_str_name("unknown"), None);
    }

    #[test]
    fn audio_output_format_extension() {
        assert_eq!(AudioOutputFormat::Mp3.extension(), "mp3");
        assert_eq!(AudioOutputFormat::Opus.extension(), "opus");
        assert_eq!(AudioOutputFormat::Wav.extension(), "wav");
        assert_eq!(AudioOutputFormat::Pcm.extension(), "pcm");
    }

    #[test]
    fn audio_output_format_mime_type() {
        assert_eq!(AudioOutputFormat::Mp3.mime_type(), "audio/mpeg");
        assert_eq!(AudioOutputFormat::Opus.mime_type(), "audio/opus");
        assert_eq!(AudioOutputFormat::Wav.mime_type(), "audio/wav");
        assert_eq!(AudioOutputFormat::Pcm.mime_type(), "audio/pcm");
    }
}
