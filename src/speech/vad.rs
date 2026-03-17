//! Voice Activity Detection (VAD).
//!
//! Simple energy-based VAD that classifies audio frames as speech or silence
//! based on RMS amplitude. Designed to be unit-testable without audio hardware.

/// VAD configuration.
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// RMS threshold below which audio is considered silence (0.0–1.0).
    pub silence_threshold: f32,
    /// Minimum consecutive speech frames before triggering.
    pub min_speech_frames: usize,
    /// Number of silence frames after speech before finalizing.
    pub trailing_silence_frames: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            silence_threshold: 0.01,
            min_speech_frames: 3,
            trailing_silence_frames: 30,
        }
    }
}

/// VAD state machine.
///
/// Feed audio frames via [`process_frame`] and check [`is_speech_complete`]
/// to know when a speech segment has ended.
#[derive(Debug)]
pub struct VadState {
    config: VadConfig,
    speech_frames: usize,
    silence_frames_after_speech: usize,
    in_speech: bool,
    speech_complete: bool,
}

impl VadState {
    /// Create a new VAD state with the given config.
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            speech_frames: 0,
            silence_frames_after_speech: 0,
            in_speech: false,
            speech_complete: false,
        }
    }

    /// Process a frame of f32 samples, updating internal state.
    ///
    /// Returns `true` if the frame is classified as speech.
    pub fn process_frame(&mut self, samples: &[f32]) -> bool {
        let rms = compute_rms(samples);
        let is_speech = rms > self.config.silence_threshold;

        if is_speech {
            self.speech_frames += 1;
            self.silence_frames_after_speech = 0;
            self.speech_complete = false;

            if self.speech_frames >= self.config.min_speech_frames {
                self.in_speech = true;
            }
        } else if self.in_speech {
            self.silence_frames_after_speech += 1;
            if self.silence_frames_after_speech >= self.config.trailing_silence_frames {
                self.speech_complete = true;
                self.in_speech = false;
                self.speech_frames = 0;
                self.silence_frames_after_speech = 0;
            }
        } else {
            // Not in speech, silence frame — reset speech counter
            self.speech_frames = 0;
        }

        is_speech
    }

    /// Whether a complete speech segment has been detected
    /// (speech followed by sufficient trailing silence).
    pub fn is_speech_complete(&self) -> bool {
        self.speech_complete
    }

    /// Whether we are currently in a speech segment.
    pub fn is_in_speech(&self) -> bool {
        self.in_speech
    }

    /// Reset the state machine for a new utterance.
    pub fn reset(&mut self) {
        self.speech_frames = 0;
        self.silence_frames_after_speech = 0;
        self.in_speech = false;
        self.speech_complete = false;
    }
}

/// Compute RMS (root mean square) amplitude of a sample buffer.
pub fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_rms_silence() {
        let silence = vec![0.0f32; 100];
        assert_eq!(compute_rms(&silence), 0.0);
    }

    #[test]
    fn compute_rms_empty() {
        assert_eq!(compute_rms(&[]), 0.0);
    }

    #[test]
    fn compute_rms_loud_signal() {
        let loud = vec![0.5f32; 100];
        let rms = compute_rms(&loud);
        assert!((rms - 0.5).abs() < 0.001, "rms={rms}");
    }

    #[test]
    fn compute_rms_mixed_signal() {
        // sin-like pattern has RMS ≈ 1/√2 ≈ 0.707 for amplitude 1.0
        let samples: Vec<f32> = (0..1000)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();
        let rms = compute_rms(&samples);
        assert!(rms > 0.5 && rms < 0.8, "rms={rms}");
    }

    #[test]
    fn vad_detects_speech_then_silence() {
        let config = VadConfig {
            silence_threshold: 0.01,
            min_speech_frames: 2,
            trailing_silence_frames: 3,
        };
        let mut vad = VadState::new(config);

        // Feed speech frames
        let speech = vec![0.1f32; 160];
        assert!(!vad.is_in_speech());
        vad.process_frame(&speech); // 1st speech frame
        assert!(!vad.is_in_speech());
        vad.process_frame(&speech); // 2nd — triggers speech
        assert!(vad.is_in_speech());
        assert!(!vad.is_speech_complete());

        // Feed silence frames
        let silence = vec![0.0f32; 160];
        vad.process_frame(&silence); // 1st silence after speech
        assert!(vad.is_in_speech());
        assert!(!vad.is_speech_complete());

        vad.process_frame(&silence); // 2nd silence
        assert!(!vad.is_speech_complete());

        vad.process_frame(&silence); // 3rd — speech complete
        assert!(vad.is_speech_complete());
        assert!(!vad.is_in_speech());
    }

    #[test]
    fn vad_does_not_trigger_on_pure_silence() {
        let config = VadConfig::default();
        let mut vad = VadState::new(config);

        let silence = vec![0.0f32; 160];
        for _ in 0..100 {
            vad.process_frame(&silence);
        }
        assert!(!vad.is_in_speech());
        assert!(!vad.is_speech_complete());
    }

    #[test]
    fn vad_does_not_trigger_on_brief_noise() {
        let config = VadConfig {
            silence_threshold: 0.01,
            min_speech_frames: 5,
            trailing_silence_frames: 3,
        };
        let mut vad = VadState::new(config);

        // Only 2 speech frames (below min_speech_frames=5)
        let speech = vec![0.1f32; 160];
        let silence = vec![0.0f32; 160];

        vad.process_frame(&speech);
        vad.process_frame(&speech);
        assert!(!vad.is_in_speech());

        // Return to silence
        for _ in 0..10 {
            vad.process_frame(&silence);
        }
        assert!(!vad.is_in_speech());
        assert!(!vad.is_speech_complete());
    }

    #[test]
    fn vad_reset_clears_state() {
        let config = VadConfig {
            silence_threshold: 0.01,
            min_speech_frames: 1,
            trailing_silence_frames: 1,
        };
        let mut vad = VadState::new(config);

        let speech = vec![0.1f32; 160];
        vad.process_frame(&speech);
        assert!(vad.is_in_speech());

        vad.reset();
        assert!(!vad.is_in_speech());
        assert!(!vad.is_speech_complete());
    }

    #[test]
    fn vad_speech_followed_by_more_speech() {
        let config = VadConfig {
            silence_threshold: 0.01,
            min_speech_frames: 2,
            trailing_silence_frames: 5,
        };
        let mut vad = VadState::new(config);

        let speech = vec![0.1f32; 160];
        let silence = vec![0.0f32; 160];

        // Start speech
        vad.process_frame(&speech);
        vad.process_frame(&speech);
        assert!(vad.is_in_speech());

        // Brief silence (not enough to end)
        vad.process_frame(&silence);
        vad.process_frame(&silence);
        assert!(vad.is_in_speech());
        assert!(!vad.is_speech_complete());

        // More speech — resets silence counter
        vad.process_frame(&speech);
        assert!(vad.is_in_speech());
        assert!(!vad.is_speech_complete());
    }
}
