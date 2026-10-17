//! Deterministic session driver (plan M0-13).
//!
//! A replay is a fixed seed, a fixed time step, and a scripted input tape.
//! Later milestones plug the game loop into [`Replay::step`]; the driver
//! itself only guarantees the determinism inputs: same config plus same
//! tape always yields the same per-frame `(t, input, rand)` stream.

use crate::{Clock, FixedClock, MsvcRand};

/// Replay configuration: the two numbers that pin a run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayConfig {
    /// `srand` seed for the session RNG.
    pub seed: u32,
    /// Fixed simulation step in seconds.
    pub dt: f64,
}

impl ReplayConfig {
    /// New replay configuration.
    pub fn new(seed: u32, dt: f64) -> Self {
        Self { seed, dt }
    }
}

/// One frame of scripted input. The button/axis payload stays a plain-word
/// skeleton until the input backend (M6) defines the real layout; the
/// driver only needs the words to be recorded and replayed exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputFrame {
    /// Opaque input words for this frame (buttons, axes, ...).
    pub words: [u32; 4],
}

impl InputFrame {
    /// Blank (no input) frame.
    pub fn blank() -> Self {
        Self::default()
    }
}

/// Deterministic session driver: fixed seed, fixed step, scripted input.
#[derive(Debug, Clone)]
pub struct Replay {
    clock: FixedClock,
    rng: MsvcRand,
    script: Vec<InputFrame>,
    frame: usize,
}

impl Replay {
    /// Start a replay: clock at 0, RNG seeded, tape loaded.
    pub fn new(config: ReplayConfig, script: Vec<InputFrame>) -> Self {
        Self {
            clock: FixedClock::new(config.dt),
            rng: MsvcRand::seeded(config.seed),
            script,
            frame: 0,
        }
    }

    /// Current frame index (0 before the first step).
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// Current session time in seconds.
    pub fn now(&self) -> f64 {
        self.clock.now()
    }

    /// Input for the current frame (blank past the end of the tape).
    pub fn input(&self) -> InputFrame {
        self.script.get(self.frame).copied().unwrap_or_default()
    }

    /// Session RNG: one `rand()` draw.
    pub fn rand(&mut self) -> u16 {
        self.rng.next_u15()
    }

    /// Advance one fixed step: frame + 1, clock + dt.
    pub fn step(&mut self) {
        self.frame += 1;
        self.clock.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_config_same_stream() {
        let cfg = ReplayConfig::new(42, 1.0 / 60.0);
        let script = vec![
            InputFrame {
                words: [1, 2, 3, 4]
            };
            8
        ];
        let mut a = Replay::new(cfg, script.clone());
        let mut b = Replay::new(cfg, script);
        for _ in 0..8 {
            assert_eq!(a.input(), b.input());
            assert_eq!(a.rand(), b.rand());
            assert_eq!(a.now(), b.now());
            a.step();
            b.step();
        }
        assert_eq!(a.frame(), 8);
    }

    #[test]
    fn input_past_tape_end_is_blank() {
        let mut r = Replay::new(ReplayConfig::new(1, 0.01), vec![]);
        assert_eq!(r.input(), InputFrame::blank());
        r.step();
        assert_eq!(r.input(), InputFrame::blank());
    }

    #[test]
    fn step_advances_time_by_dt() {
        let mut r = Replay::new(ReplayConfig::new(1, 0.05), vec![]);
        r.step();
        assert!((r.now() - 0.05).abs() < 1e-12);
    }
}
