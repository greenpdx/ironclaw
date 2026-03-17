//! Local speech-to-text engine using whisper.cpp.
//!
//! Captures audio from the default microphone using CPAL, buffers speech
//! segments via VAD, and transcribes them using the Whisper model.

use std::path::PathBuf;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{HeapRb, traits::{Consumer, Producer, Split}};
use tokio::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::vad::{VadConfig, VadState};

/// Errors from the local STT engine.
#[derive(Debug, thiserror::Error)]
pub enum LocalSttError {
    #[error("Failed to load whisper model from {path}: {reason}")]
    ModelLoad { path: String, reason: String },

    #[error("Audio device error: {0}")]
    AudioDevice(String),

    #[error("Transcription failed: {0}")]
    TranscriptionFailed(String),
}

/// Configuration for the local STT engine.
#[derive(Debug, Clone)]
pub struct LocalSttConfig {
    /// Path to the GGML whisper model file.
    pub model_path: PathBuf,
    /// Audio sample rate (must match model expectations; default: 16000).
    pub sample_rate: u32,
    /// VAD configuration.
    pub vad: VadConfig,
    /// Size of the audio ring buffer in samples.
    pub buffer_size: usize,
    /// Frame size for VAD processing (samples per frame).
    pub frame_size: usize,
}

impl Default for LocalSttConfig {
    fn default() -> Self {
        Self {
            model_path: default_model_path(),
            sample_rate: 16_000,
            vad: VadConfig::default(),
            buffer_size: 16_000 * 30, // 30 seconds at 16kHz
            frame_size: 1600,         // 100ms at 16kHz
        }
    }
}

/// Default model path: ~/.ironclaw/models/ggml-base.en.bin
fn default_model_path() -> PathBuf {
    crate::bootstrap::ironclaw_base_dir()
        .join("models")
        .join("ggml-base.en.bin")
}

/// Local speech-to-text engine.
///
/// Captures audio from the microphone, detects speech via VAD,
/// and transcribes complete utterances via whisper.cpp.
///
/// Transcriptions are sent through an `mpsc::Sender<String>`.
pub struct LocalSttEngine {
    config: LocalSttConfig,
}

impl LocalSttEngine {
    /// Create a new engine with the given configuration.
    pub fn new(config: LocalSttConfig) -> Self {
        Self { config }
    }

    /// Start the capture-and-transcribe loop.
    ///
    /// Returns a receiver that yields transcribed text strings.
    /// The engine runs in background tasks until the receiver is dropped.
    pub fn start(&self) -> Result<mpsc::Receiver<String>, LocalSttError> {
        let (tx, rx) = mpsc::channel(16);

        // Load the whisper model
        let ctx = WhisperContext::new_with_params(
            self.config.model_path.to_str().unwrap_or(""),
            WhisperContextParameters::default(),
        )
        .map_err(|e| LocalSttError::ModelLoad {
            path: self.config.model_path.display().to_string(),
            reason: e.to_string(),
        })?;

        let ctx = Arc::new(ctx);

        // Set up the ring buffer for audio samples
        let rb = HeapRb::<f32>::new(self.config.buffer_size);
        let (mut producer, mut consumer) = rb.split();

        // Set up CPAL audio capture
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| LocalSttError::AudioDevice("no default input device".to_string()))?;

        let desired_config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        tracing::info!(
            device = device.name().unwrap_or_default(),
            sample_rate = self.config.sample_rate,
            model = %self.config.model_path.display(),
            "Starting local STT engine"
        );

        let stream = device
            .build_input_stream(
                &desired_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Push samples into ring buffer; drop oldest if full
                    for &sample in data {
                        let _ = producer.try_push(sample);
                    }
                },
                |err| {
                    tracing::error!("Audio capture error: {}", err);
                },
                None,
            )
            .map_err(|e| LocalSttError::AudioDevice(e.to_string()))?;

        stream
            .play()
            .map_err(|e| LocalSttError::AudioDevice(e.to_string()))?;

        // Spawn the VAD + transcription loop
        let frame_size = self.config.frame_size;
        let vad_config = self.config.vad.clone();
        tokio::spawn(async move {
            // Keep stream alive by moving it into the task
            let _stream = stream;

            let mut vad = VadState::new(vad_config);
            let mut speech_buffer: Vec<f32> = Vec::new();
            let mut frame = vec![0.0f32; frame_size];

            loop {
                // Wait a bit for audio to accumulate
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                // Drain available samples from ring buffer
                loop {
                    let read = consumer.pop_slice(&mut frame);
                    if read == 0 {
                        break;
                    }

                    let chunk = &frame[..read];
                    let is_speech = vad.process_frame(chunk);

                    if vad.is_in_speech() || is_speech {
                        speech_buffer.extend_from_slice(chunk);
                    }

                    if vad.is_speech_complete() && !speech_buffer.is_empty() {
                        let audio = std::mem::take(&mut speech_buffer);
                        let ctx = Arc::clone(&ctx);
                        let tx = tx.clone();

                        // Transcribe on a blocking thread (whisper is CPU-intensive)
                        tokio::task::spawn_blocking(move || {
                            match transcribe(&ctx, &audio) {
                                Ok(text) if !text.is_empty() => {
                                    tracing::info!(
                                        text_len = text.len(),
                                        audio_secs = audio.len() as f32 / 16000.0,
                                        "Transcribed speech"
                                    );
                                    let _ = tx.blocking_send(text);
                                }
                                Ok(_) => {
                                    tracing::debug!("Transcription produced empty text, skipping");
                                }
                                Err(e) => {
                                    tracing::error!("Transcription error: {}", e);
                                }
                            }
                        });

                        vad.reset();
                    }
                }

                // Check if receiver has been dropped
                if tx.is_closed() {
                    tracing::info!("STT receiver dropped, stopping capture");
                    break;
                }
            }
        });

        Ok(rx)
    }
}

/// Transcribe an audio buffer using whisper.
fn transcribe(ctx: &WhisperContext, audio: &[f32]) -> Result<String, LocalSttError> {
    let mut state = ctx
        .create_state()
        .map_err(|e| LocalSttError::TranscriptionFailed(e.to_string()))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(2);
    params.set_language(Some("en"));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, audio)
        .map_err(|e| LocalSttError::TranscriptionFailed(e.to_string()))?;

    let num_segments = state.full_n_segments().map_err(|e| {
        LocalSttError::TranscriptionFailed(format!("failed to get segments: {e}"))
    })?;

    let mut text = String::new();
    for i in 0..num_segments {
        if let Ok(segment_text) = state.full_get_segment_text(i) {
            text.push_str(segment_text.trim());
            text.push(' ');
        }
    }

    Ok(text.trim().to_string())
}
