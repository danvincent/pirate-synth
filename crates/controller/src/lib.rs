use audio_alsa::AudioCommand;
use crossbeam_channel::Sender;
use engine::{ScaleMode, Wavetable};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Debounce state for a single parameter.
struct Debounce<T> {
    pending: Option<T>,
    deadline: Instant,
    delay: Duration,
}

impl<T: Clone> Debounce<T> {
    fn new(delay_ms: u64) -> Self {
        Self {
            pending: None,
            deadline: Instant::now(),
            delay: Duration::from_millis(delay_ms),
        }
    }

    /// Stage a value. Call `flush` regularly to dispatch when the deadline passes.
    fn stage(&mut self, value: T) {
        self.pending = Some(value);
        self.deadline = Instant::now() + self.delay;
    }

    /// Returns the staged value if the deadline has passed, then clears it.
    fn flush(&mut self) -> Option<T> {
        if self.pending.is_some() && Instant::now() >= self.deadline {
            self.pending.take()
        } else {
            None
        }
    }
}

/// All tuning/scale/transition parameters that may need debouncing.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleParams {
    pub mode: ScaleMode,
    pub spread_percent: f32,
}

/// Central controller for the pirate-synth.
///
/// Owns the `AudioCommand` channel sender and all debounce state.
/// Call [`SynthController::poll`] on every UI tick to flush pending commands.
pub struct SynthController {
    sender: Sender<AudioCommand>,
    cents_debounce: Debounce<f32>,
    scale_debounce: Debounce<ScaleParams>,
    bank_debounce: Debounce<Arc<[Wavetable]>>,
    note_transition_ms: f32,
    current_scale_mode: ScaleMode,
    transition_start: Option<std::time::Instant>,
}

impl SynthController {
    /// Create a new controller from an `AudioCommand` sender.
    /// `debounce_ms` is applied to cents, scale, and bank changes.
    pub fn new(sender: Sender<AudioCommand>, debounce_ms: u64) -> Self {
        Self {
            sender,
            cents_debounce: Debounce::new(debounce_ms),
            scale_debounce: Debounce::new(debounce_ms),
            bank_debounce: Debounce::new(debounce_ms),
            note_transition_ms: 0.0,
            current_scale_mode: ScaleMode::None,
            transition_start: None,
        }
    }

    // ── Immediate commands ────────────────────────────────────────────────

    /// Change the played note (or MIDI pitch). Applies frequency glide if
    /// `note_transition_ms` > 0.
    pub fn set_note_hz(&mut self, frequency_hz: f32) {
        if self.sender.send(AudioCommand::SetFrequencyHz(frequency_hz)).is_err() {
            eprintln!("[controller] audio channel disconnected");
        }
        // Record transition start time for glide progress tracking
        self.transition_start = if self.note_transition_ms > 0.0 {
            Some(std::time::Instant::now())
        } else {
            None
        };
    }

    /// Set the glide duration for all note/scale/cents transitions.
    pub fn set_note_transition_ms(&mut self, ms: f32) {
        self.note_transition_ms = ms.max(0.0);
        if self.sender.send(AudioCommand::SetNoteTransitionMs(self.note_transition_ms)).is_err() {
            eprintln!("[controller] audio channel disconnected");
        }
        // Only set_note_hz() starts a glide. Changing duration alone does not start a glide.
        self.transition_start = None;
    }

    /// Set the wavetable bank crossfade duration in seconds.
    pub fn set_transition_secs(&self, secs: f32) {
        if self.sender.send(AudioCommand::SetTransitionSecs(secs)).is_err() {
            eprintln!("[controller] audio channel disconnected");
        }
    }

    /// Enable or disable all oscillators immediately.
    pub fn set_oscillators_active(&self, active: bool) {
        if self.sender.send(AudioCommand::SetOscillatorsActive(active)).is_err() {
            eprintln!("[controller] audio channel disconnected");
        }
    }

    // ── Debounced commands ────────────────────────────────────────────────

    /// Stage a fine-tune cents change. Dispatched after debounce delay.
    pub fn stage_fine_tune_cents(&mut self, cents: f32) {
        self.cents_debounce.stage(cents);
    }

    /// Stage a scale change. Dispatched after debounce delay.
    pub fn stage_scale(&mut self, mode: ScaleMode, spread_percent: f32) {
        self.scale_debounce.stage(ScaleParams { mode, spread_percent });
    }

    /// Stage a wavetable bank change. Dispatched after debounce delay.
    pub fn stage_bank(&mut self, bank: Arc<[Wavetable]>) {
        self.bank_debounce.stage(bank);
    }

    // ── Tick ──────────────────────────────────────────────────────────────

    /// Flush any pending debounced commands whose deadline has passed.
    /// Call this on every UI poll iteration.
    pub fn poll(&mut self) {
        if let Some(cents) = self.cents_debounce.flush() {
            if self.sender.send(AudioCommand::SetFineTuneCents(cents)).is_err() {
                eprintln!("[controller] audio channel disconnected");
            }
            // Only resend scale when a real scale mode is active
            if self.current_scale_mode != ScaleMode::None {
                if self.sender.send(AudioCommand::SetScale {
                    mode: self.current_scale_mode,
                    spread_percent: cents,
                }).is_err() {
                    eprintln!("[controller] audio channel disconnected");
                }
            }
        }
        if let Some(params) = self.scale_debounce.flush() {
            self.current_scale_mode = params.mode;
            if self.sender.send(AudioCommand::SetScale {
                mode: params.mode,
                spread_percent: params.spread_percent,
            }).is_err() {
                eprintln!("[controller] audio channel disconnected");
            }
        }
        if let Some(bank) = self.bank_debounce.flush() {
            if self.sender.send(AudioCommand::SetWavetableBank(bank)).is_err() {
                eprintln!("[controller] audio channel disconnected");
            }
        }
    }

    /// Current cached `note_transition_ms` value.
    pub fn note_transition_ms(&self) -> f32 {
        self.note_transition_ms
    }

    /// Returns Some(progress 0.0..=1.0) while a glide is in progress, None otherwise.
    pub fn transition_progress(&self) -> Option<f32> {
        let start = self.transition_start?;
        let ms = self.note_transition_ms;
        if ms <= 0.0 {
            return None;
        }
        let elapsed = start.elapsed().as_millis() as f32;
        if elapsed >= ms {
            None
        } else {
            Some((elapsed / ms).clamp(0.0, 1.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel;

    fn make_controller(debounce_ms: u64) -> (SynthController, crossbeam_channel::Receiver<AudioCommand>) {
        let (tx, rx) = crossbeam_channel::bounded(64);
        (SynthController::new(tx, debounce_ms), rx)
    }

    #[test]
    fn set_note_hz_sends_immediately() {
        let (mut ctrl, rx) = make_controller(200);
        ctrl.set_note_hz(440.0);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, AudioCommand::SetFrequencyHz(f) if (f - 440.0).abs() < 0.01));
    }

    #[test]
    fn stage_fine_tune_cents_debounced() {
        let (mut ctrl, rx) = make_controller(0); // 0ms = flush immediately on poll
        ctrl.stage_fine_tune_cents(50.0);
        // Nothing sent yet (poll not called)
        assert!(rx.try_recv().is_err());
        ctrl.poll();
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, AudioCommand::SetFineTuneCents(c) if (c - 50.0).abs() < 0.01));
    }

    #[test]
    fn set_note_transition_ms_sends_command() {
        let (mut ctrl, rx) = make_controller(200);
        ctrl.set_note_transition_ms(500.0);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, AudioCommand::SetNoteTransitionMs(ms) if (ms - 500.0).abs() < 0.01));
    }

    #[test]
    fn debounce_high_delay_does_not_flush_early() {
        let (mut ctrl, rx) = make_controller(1000); // 1 second delay
        ctrl.stage_fine_tune_cents(25.0);
        ctrl.poll(); // too early
        assert!(rx.try_recv().is_err(), "should not flush before debounce delay");
    }

    #[test]
    fn set_oscillators_active_sends_immediately() {
        let (ctrl, rx) = make_controller(200);
        ctrl.set_oscillators_active(true);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, AudioCommand::SetOscillatorsActive(true)));
    }

    #[test]
    fn set_transition_secs_sends_immediately() {
        let (ctrl, rx) = make_controller(200);
        ctrl.set_transition_secs(2.5);
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, AudioCommand::SetTransitionSecs(s) if (s - 2.5).abs() < 0.01));
    }

    #[test]
    fn stage_scale_debounced() {
        let (mut ctrl, rx) = make_controller(0); // 0ms = flush immediately on poll
        ctrl.stage_scale(ScaleMode::Major, 50.0);
        // Nothing sent yet
        assert!(rx.try_recv().is_err());
        ctrl.poll();
        let cmd = rx.try_recv().unwrap();
        assert!(
            matches!(cmd, AudioCommand::SetScale { mode: ScaleMode::Major, spread_percent } if (spread_percent - 50.0).abs() < 0.01),
            "expected SetScale Major 50.0"
        );
    }

    #[test]
    fn stage_bank_debounced() {
        let (mut ctrl, rx) = make_controller(0);
        // Stage bank with a simple test wavetable
        let test_wavetable = Wavetable {
            name: "test".to_string(),
            samples: vec![0.0, 0.5],
        };
        let bank: Arc<[Wavetable]> = Arc::from([test_wavetable]);
        ctrl.stage_bank(bank.clone());
        assert!(rx.try_recv().is_err());
        ctrl.poll();
        let cmd = rx.try_recv().unwrap();
        // Just verify a SetWavetableBank command was sent
        assert!(matches!(cmd, AudioCommand::SetWavetableBank(_)));
    }

    #[test]
    fn debounce_coalesces_multiple_stages_to_latest() {
        let (mut ctrl, rx) = make_controller(0);
        ctrl.stage_fine_tune_cents(10.0);
        ctrl.stage_fine_tune_cents(20.0);
        ctrl.stage_fine_tune_cents(99.0); // latest value
        ctrl.poll();
        let cmd = rx.try_recv().unwrap();
        assert!(
            matches!(cmd, AudioCommand::SetFineTuneCents(c) if (c - 99.0).abs() < 0.01),
            "only the latest staged value should be dispatched"
        );
        // With current_scale_mode == ScaleMode::None (initial state), SetScale is not sent
        assert!(rx.try_recv().is_err(), "only one command should be sent when scale mode is None");
    }

    #[test]
    fn cents_change_also_updates_scale_spread() {
        let (mut ctrl, rx) = make_controller(0);
        // Set scale first so current_scale_mode is non-None
        ctrl.stage_scale(ScaleMode::Major, 0.0);
        ctrl.poll();
        let _ = rx.try_recv(); // consume the SetScale command

        // Now change cents — should emit both SetFineTuneCents and SetScale
        ctrl.stage_fine_tune_cents(50.0);
        ctrl.poll();

        let cmd1 = rx.try_recv().expect("expected SetFineTuneCents");
        let cmd2 = rx.try_recv().expect("expected SetScale with updated spread");
        assert!(matches!(cmd1, AudioCommand::SetFineTuneCents(c) if (c - 50.0).abs() < 0.01));
        assert!(
            matches!(cmd2, AudioCommand::SetScale { mode: ScaleMode::Major, spread_percent } if (spread_percent - 50.0).abs() < 0.01),
            "scale spread should be updated to match new cents value"
        );
    }

    #[test]
    fn transition_progress_none_when_ms_zero() {
        let (mut ctrl, _rx) = make_controller(0);
        ctrl.set_note_transition_ms(0.0);
        ctrl.set_note_hz(440.0);
        assert!(ctrl.transition_progress().is_none());
    }

    #[test]
    fn transition_progress_some_immediately_after_note_set() {
        let (mut ctrl, _rx) = make_controller(0);
        ctrl.set_note_transition_ms(5000.0);
        ctrl.set_note_hz(440.0);
        let p = ctrl.transition_progress();
        assert!(p.is_some(), "expected Some progress");
        assert!(p.unwrap() < 0.1, "progress should be near 0 immediately after note set");
    }

    #[test]
    fn transition_progress_none_after_completion() {
        let (mut ctrl, _rx) = make_controller(0);
        ctrl.set_note_transition_ms(1.0); // 1ms — will complete almost immediately
        ctrl.set_note_hz(440.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(ctrl.transition_progress().is_none(), "should be None after glide completes");
    }
}
