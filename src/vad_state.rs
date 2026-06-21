/// VAD (Voice Activity Detection) debounce state machine module.
///
/// Provides hysteresis-based debouncing to avoid rapid state toggling
/// caused by transient noise or brief pauses.

/// Represents the current voice activity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Silent,
    Speaking,
}

/// A debounce state machine for voice activity detection.
///
/// Requires a configurable number of consecutive voice/silence frames
/// before transitioning, preventing spurious state changes.
pub struct VadStateMachine {
    state: VoiceState,
    consecutive_voice: u32,
    consecutive_silence: u32,
    attack_frames: u32,
    release_frames: u32,
}

impl VadStateMachine {
    /// Creates a new `VadStateMachine` with the given debounce thresholds.
    ///
    /// - `attack_frames`: number of consecutive voice frames needed to enter `Speaking`
    /// - `release_frames`: number of consecutive silence frames needed to enter `Silent`
    pub fn new(attack_frames: u32, release_frames: u32) -> Self {
        Self {
            state: VoiceState::Silent,
            consecutive_voice: 0,
            consecutive_silence: 0,
            attack_frames,
            release_frames,
        }
    }

    pub fn set_attack_frames(&mut self, frames: u32) {
        self.attack_frames = frames;
    }

    pub fn set_release_frames(&mut self, frames: u32) {
        self.release_frames = frames;
    }

    /// Updates the state machine with a new VAD score and threshold.
    ///
    /// Returns `Some(VoiceState)` when a state transition occurs,
    /// or `None` if the state remains unchanged.
    pub fn update(&mut self, score: f32, threshold: f32) -> Option<VoiceState> {
        if score >= threshold {
            self.consecutive_voice += 1;
            self.consecutive_silence = 0;
        } else {
            self.consecutive_silence += 1;
            self.consecutive_voice = 0;
        }

        match self.state {
            VoiceState::Silent => {
                if self.consecutive_voice >= self.attack_frames {
                    self.state = VoiceState::Speaking;
                    Some(VoiceState::Speaking)
                } else {
                    None
                }
            }
            VoiceState::Speaking => {
                if self.consecutive_silence >= self.release_frames {
                    self.state = VoiceState::Silent;
                    Some(VoiceState::Silent)
                } else {
                    None
                }
            }
        }
    }
}

/// Tracks the noise floor using exponential moving average (EMA) of RMS values.
/// Only updates during Silent state to avoid treating voice as noise.
pub struct NoiseFloorTracker {
    /// Current noise floor estimate (RMS)
    noise_floor: f32,
    /// EMA alpha (smoothing factor), typically 0.001-0.01
    alpha: f32,
}

impl NoiseFloorTracker {
    pub fn new(alpha: f32) -> Self {
        Self {
            noise_floor: 0.0,
            alpha,
        }
    }

    /// Update the noise floor with a new RMS value.
    /// Should only be called when the current state is Silent.
    pub fn update(&mut self, rms: f32) {
        if self.noise_floor <= 0.0 {
            // First measurement
            self.noise_floor = rms;
        } else {
            // EMA update
            self.noise_floor = self.alpha * rms + (1.0 - self.alpha) * self.noise_floor;
        }
    }

    /// Get the effective threshold, which is the maximum of the user threshold
    /// and the noise floor multiplied by the multiplier.
    pub fn effective_threshold(&self, user_threshold: f32, multiplier: f32) -> f32 {
        let noise_threshold = self.noise_floor * multiplier;
        user_threshold.max(noise_threshold)
    }
}

/// Compute spectral flatness of a frame.
/// Returns a value in [0, 1] where 1 = white noise (flat spectrum), 0 = pure tone.
/// Uses FFT to compute power spectrum, then geometric mean / arithmetic mean.
pub fn spectral_flatness(frame: &[f32]) -> f32 {
    use rustfft::{FftPlanner, num_complex::Complex};

    let len = frame.len();
    if len == 0 {
        return 0.0;
    }

    // Apply Hann window to reduce spectral leakage
    let windowed: Vec<f32> = frame
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos());
            s * w
        })
        .collect();

    // Prepare FFT input (zero-pad to next power of 2 if needed)
    let fft_len = windowed.len().next_power_of_two();
    let mut fft_input: Vec<Complex<f32>> =
        windowed.iter().map(|&s| Complex::new(s, 0.0)).collect();
    fft_input.resize(fft_len, Complex::new(0.0, 0.0));

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_len);
    fft.process(&mut fft_input);

    // Compute power spectrum (only first half - positive frequencies)
    let half_len = fft_len / 2;
    let power: Vec<f32> = fft_input[..half_len]
        .iter()
        .map(|c| c.norm_sqr() / fft_len as f32)
        .collect();

    if power.is_empty() {
        return 0.0;
    }

    // Arithmetic mean of power spectrum
    let arith_mean: f32 = power.iter().sum::<f32>() / power.len() as f32;
    if arith_mean <= f32::EPSILON {
        return 0.0;
    }

    // Geometric mean of power spectrum (use log to avoid overflow)
    let log_sum: f32 = power
        .iter()
        .map(|&p| {
            if p > f32::EPSILON {
                p.ln()
            } else {
                f32::MIN_POSITIVE.ln()
            }
        })
        .sum();
    let geo_mean = (log_sum / power.len() as f32).exp();

    // Spectral flatness = geo_mean / arith_mean
    (geo_mean / arith_mean).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_noise_no_trigger() {
        let mut sm = VadStateMachine::new(6, 30);
        // 4 voice frames — less than attack_frames=6
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.state, VoiceState::Silent);
    }

    #[test]
    fn test_attack_triggers_speaking() {
        let mut sm = VadStateMachine::new(6, 30);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), None);
        assert_eq!(sm.update(0.9, 0.5), Some(VoiceState::Speaking));
        assert_eq!(sm.state, VoiceState::Speaking);
    }

    #[test]
    fn test_short_pause_no_recovery() {
        let mut sm = VadStateMachine::new(6, 30);
        // Enter Speaking first
        for _ in 0..6 {
            sm.update(0.9, 0.5);
        }
        assert_eq!(sm.state, VoiceState::Speaking);
        // 10 silence frames — less than release_frames=30
        for _ in 0..10 {
            assert_eq!(sm.update(0.1, 0.5), None);
        }
        assert_eq!(sm.state, VoiceState::Speaking);
    }

    #[test]
    fn test_release_triggers_silent() {
        let mut sm = VadStateMachine::new(6, 30);
        // Enter Speaking first
        for _ in 0..6 {
            sm.update(0.9, 0.5);
        }
        assert_eq!(sm.state, VoiceState::Speaking);
        // 29 silence frames — not enough
        for _ in 0..29 {
            assert_eq!(sm.update(0.1, 0.5), None);
        }
        // 30th silence frame triggers transition
        assert_eq!(sm.update(0.1, 0.5), Some(VoiceState::Silent));
        assert_eq!(sm.state, VoiceState::Silent);
    }

    #[test]
    fn test_full_cycle() {
        let mut sm = VadStateMachine::new(6, 30);

        // Start in Silent
        assert_eq!(sm.state, VoiceState::Silent);

        // Attack: 6 voice frames -> Speaking
        for _ in 0..5 {
            assert_eq!(sm.update(0.9, 0.5), None);
        }
        assert_eq!(sm.update(0.9, 0.5), Some(VoiceState::Speaking));
        assert_eq!(sm.state, VoiceState::Speaking);

        // Release: 30 silence frames -> Silent
        for _ in 0..29 {
            assert_eq!(sm.update(0.1, 0.5), None);
        }
        assert_eq!(sm.update(0.1, 0.5), Some(VoiceState::Silent));
        assert_eq!(sm.state, VoiceState::Silent);
    }
}
