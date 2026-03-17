//! OpenAI TTS provider.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

use super::{AudioOutputFormat, TtsError, TtsOutput, TtsProvider};

/// Maximum text length for OpenAI TTS API.
const MAX_TEXT_LENGTH: usize = 4096;

/// OpenAI text-to-speech provider.
///
/// Uses the `/v1/audio/speech` endpoint.
pub struct OpenAiTtsProvider {
    client: reqwest::Client,
    api_key: SecretString,
    model: String,
    voice: String,
    base_url: String,
}

impl OpenAiTtsProvider {
    /// Create a new OpenAI TTS provider with the given API key.
    pub fn new(api_key: SecretString) -> Self {
        Self {
            client: match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(
                        "Failed to build HTTP client with timeout, falling back to default: {e}"
                    );
                    reqwest::Client::default()
                }
            },
            api_key,
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            base_url: "https://api.openai.com".to_string(),
        }
    }

    /// Override the base URL (for proxied or compatible endpoints).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        while url.ends_with('/') {
            url.pop();
        }
        self.base_url = url;
        self
    }

    /// Override the model name (e.g. "tts-1-hd").
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the voice (e.g. "nova", "shimmer", "echo", "fable", "onyx").
    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = voice.into();
        self
    }
}

#[async_trait]
impl TtsProvider for OpenAiTtsProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn synthesize(
        &self,
        text: &str,
        format: AudioOutputFormat,
    ) -> Result<TtsOutput, TtsError> {
        if text.is_empty() {
            return Err(TtsError::EmptyText);
        }
        if text.len() > MAX_TEXT_LENGTH {
            return Err(TtsError::TextTooLong {
                len: text.len(),
                max: MAX_TEXT_LENGTH,
            });
        }

        let response_format = match format {
            AudioOutputFormat::Mp3 => "mp3",
            AudioOutputFormat::Opus => "opus",
            AudioOutputFormat::Wav => "wav",
            AudioOutputFormat::Pcm => "pcm",
        };

        let url = format!("{}/v1/audio/speech", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": self.voice,
            "response_format": response_format,
        });

        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| TtsError::RequestFailed(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(TtsError::RequestFailed(format!("HTTP {}: {}", status, body)));
        }

        let audio_data = response
            .bytes()
            .await
            .map_err(|e| TtsError::RequestFailed(e.to_string()))?
            .to_vec();

        Ok(TtsOutput { audio_data, format })
    }
}
