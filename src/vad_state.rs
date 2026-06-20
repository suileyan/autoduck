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
