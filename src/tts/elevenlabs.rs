//! ElevenLabs TTS provider.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

use super::{AudioOutputFormat, TtsError, TtsOutput, TtsProvider};

/// Maximum text length for ElevenLabs TTS API.
const MAX_TEXT_LENGTH: usize = 5000;

/// Default voice ID (Rachel).
const DEFAULT_VOICE_ID: &str = "21m00Tcm4TlvDq8ikWAM";

/// ElevenLabs text-to-speech provider.
///
/// Uses the `/v1/text-to-speech/{voice_id}` endpoint.
pub struct ElevenLabsProvider {
    client: reqwest::Client,
    api_key: SecretString,
    voice_id: String,
    model_id: String,
    base_url: String,
}

impl ElevenLabsProvider {
    /// Create a new ElevenLabs TTS provider with the given API key.
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
            voice_id: DEFAULT_VOICE_ID.to_string(),
            model_id: "eleven_monolingual_v1".to_string(),
            base_url: "https://api.elevenlabs.io".to_string(),
        }
    }

    /// Override the voice ID.
    pub fn with_voice_id(mut self, voice_id: impl Into<String>) -> Self {
        self.voice_id = voice_id.into();
        self
    }

    /// Override the model ID (e.g. "eleven_multilingual_v2").
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Override the base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut url = base_url.into();
        while url.ends_with('/') {
            url.pop();
        }
        self.base_url = url;
        self
    }
}

#[async_trait]
impl TtsProvider for ElevenLabsProvider {
    fn name(&self) -> &str {
        "elevenlabs"
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

        let output_format = match format {
            AudioOutputFormat::Mp3 => "mp3_44100_128",
            AudioOutputFormat::Pcm => "pcm_16000",
            // ElevenLabs doesn't natively support wav/opus, use mp3 as fallback
            AudioOutputFormat::Wav | AudioOutputFormat::Opus => "mp3_44100_128",
        };

        // If the requested format isn't natively supported, use mp3
        let actual_format = match format {
            AudioOutputFormat::Wav | AudioOutputFormat::Opus => AudioOutputFormat::Mp3,
            other => other,
        };

        let url = format!(
            "{}/v1/text-to-speech/{}",
            self.base_url, self.voice_id
        );

        let body = serde_json::json!({
            "text": text,
            "model_id": self.model_id,
            "output_format": output_format,
        });

        let response = self
            .client
            .post(&url)
            .header("xi-api-key", self.api_key.expose_secret())
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

        Ok(TtsOutput {
            audio_data,
            format: actual_format,
        })
    }
}
