//! Text-to-speech tool.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use crate::context::JobContext;
use crate::tools::tool::{
    ApprovalRequirement, Tool, ToolDomain, ToolError, ToolOutput, ToolRateLimitConfig, require_str,
};
use crate::tts::{AudioOutputFormat, TtsProvider};

/// Built-in tool for text-to-speech synthesis.
///
/// Synthesizes text into audio using the configured TTS provider.
/// Returns base64-encoded audio data.
pub struct SpeakTool {
    provider: Arc<dyn TtsProvider>,
}

impl SpeakTool {
    /// Create a new speak tool with the given TTS provider.
    pub fn new(provider: Arc<dyn TtsProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Tool for SpeakTool {
    fn name(&self) -> &str {
        "speak"
    }

    fn description(&self) -> &str {
        "Synthesize text into audio using text-to-speech. Returns base64-encoded audio data."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to synthesize into speech"
                },
                "format": {
                    "type": "string",
                    "description": "Audio output format: mp3 (default), opus, wav, or pcm",
                    "enum": ["mp3", "opus", "wav", "pcm"]
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let text = require_str(&params, "text")?;

        let format = params
            .get("format")
            .and_then(|v| v.as_str())
            .and_then(AudioOutputFormat::from_str_name)
            .unwrap_or(AudioOutputFormat::Mp3);

        let output = self
            .provider
            .synthesize(text, format)
            .await
            .map_err(|e| ToolError::ExternalService(e.to_string()))?;

        let encoded = base64::engine::general_purpose::STANDARD.encode(&output.audio_data);

        Ok(ToolOutput::success(
            serde_json::json!({
                "audio_base64": encoded,
                "format": output.format.extension(),
                "mime_type": output.format.mime_type(),
                "size_bytes": output.audio_data.len(),
            }),
            start.elapsed(),
        ))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        false // Output is audio data, not untrusted text
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Orchestrator
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        Some(ToolRateLimitConfig::new(10, 100))
    }

    fn execution_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::{TtsError, TtsOutput};

    struct MockTtsProvider {
        result: Result<TtsOutput, TtsError>,
    }

    #[async_trait]
    impl TtsProvider for MockTtsProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn synthesize(
            &self,
            _text: &str,
            format: AudioOutputFormat,
        ) -> Result<TtsOutput, TtsError> {
            match &self.result {
                Ok(output) => Ok(TtsOutput {
                    audio_data: output.audio_data.clone(),
                    format,
                }),
                Err(_) => Err(TtsError::RequestFailed("mock error".into())),
            }
        }
    }

    fn mock_provider(audio_data: Vec<u8>) -> Arc<dyn TtsProvider> {
        Arc::new(MockTtsProvider {
            result: Ok(TtsOutput {
                audio_data,
                format: AudioOutputFormat::Mp3,
            }),
        })
    }

    fn failing_provider() -> Arc<dyn TtsProvider> {
        Arc::new(MockTtsProvider {
            result: Err(TtsError::RequestFailed("mock error".into())),
        })
    }

    #[tokio::test]
    async fn speak_tool_synthesizes_text() {
        let tool = SpeakTool::new(mock_provider(vec![0xFF, 0xFB, 0x90]));
        let ctx = JobContext::default();

        let result = tool
            .execute(serde_json::json!({"text": "Hello world"}), &ctx)
            .await
            .unwrap();

        let audio_b64 = result.result["audio_base64"].as_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(audio_b64)
            .unwrap();
        assert_eq!(decoded, vec![0xFF, 0xFB, 0x90]);
        assert_eq!(result.result["format"], "mp3");
        assert_eq!(result.result["mime_type"], "audio/mpeg");
        assert_eq!(result.result["size_bytes"], 3);
    }

    #[tokio::test]
    async fn speak_tool_respects_format_param() {
        let tool = SpeakTool::new(mock_provider(vec![1, 2, 3]));
        let ctx = JobContext::default();

        let result = tool
            .execute(
                serde_json::json!({"text": "Hello", "format": "opus"}),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result.result["format"], "opus");
        assert_eq!(result.result["mime_type"], "audio/opus");
    }

    #[tokio::test]
    async fn speak_tool_missing_text_param() {
        let tool = SpeakTool::new(mock_provider(vec![]));
        let ctx = JobContext::default();

        let err = tool.execute(serde_json::json!({}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("missing 'text'"));
    }

    #[tokio::test]
    async fn speak_tool_handles_provider_error() {
        let tool = SpeakTool::new(failing_provider());
        let ctx = JobContext::default();

        let err = tool
            .execute(serde_json::json!({"text": "Hello"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("mock error"));
    }

    #[test]
    fn speak_tool_metadata() {
        let tool = SpeakTool::new(mock_provider(vec![]));
        assert_eq!(tool.name(), "speak");
        assert!(!tool.description().is_empty());
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::UnlessAutoApproved
        );
        assert!(!tool.requires_sanitization());
        assert_eq!(tool.domain(), ToolDomain::Orchestrator);
        assert!(tool.rate_limit_config().is_some());
    }

    #[test]
    fn speak_tool_schema_is_valid() {
        let tool = SpeakTool::new(mock_provider(vec![]));
        let schema = tool.parameters_schema();
        let errors = crate::tools::tool::validate_tool_schema(&schema, "speak");
        assert!(errors.is_empty(), "schema errors: {errors:?}");
    }
}
