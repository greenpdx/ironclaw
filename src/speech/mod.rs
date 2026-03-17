//! Local speech-to-text via whisper.cpp.
//!
//! Provides microphone capture, voice activity detection (VAD), and
//! transcription using the `whisper-rs` bindings. Transcribed text is
//! injected as [`IncomingMessage`]s into the channel manager.
//!
//! Gated behind `--features speech`.

mod local_stt;
mod vad;

pub use self::local_stt::{LocalSttEngine, LocalSttError};
pub use self::vad::{VadConfig, VadState};
