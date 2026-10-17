//! Minimal clock seam (plan M0-13).
//!
//! What M1 needs: deterministic replay runs on a fixed time step, while
//! real wall-clock stays available behind the same trait. Keep it boring:
//! seconds as `f64`, one method, two implementations.

/// Game time source: seconds since an arbitrary epoch.
pub trait Clock {
    /// Current time in seconds.
    fn now(&self) -> f64;
}

/// Deterministic clock for replay: starts at 0 and advances by the fixed
/// step each tick. Every run with the same step count reads the same times.
#[derive(Debug, Clone)]
pub struct FixedClock {
    t: f64,
    dt: f64,
}

impl FixedClock {
    /// Fixed-step clock starting at `t = 0`.
    pub fn new(dt: f64) -> Self {
        Self { t: 0.0, dt }
    }

    /// Advance one fixed step.
    pub fn tick(&mut self) {
        self.t += self.dt;
    }

    /// The fixed step this clock advances by.
    pub fn dt(&self) -> f64 {
        self.dt
    }
}

impl Clock for FixedClock {
    fn now(&self) -> f64 {
        self.t
    }
}

/// Real wall clock, for non-replay runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_advances_by_dt() {
        let mut c = FixedClock::new(1.0 / 60.0);
        assert_eq!(c.now(), 0.0);
        c.tick();
        c.tick();
        assert!((c.now() - 2.0 / 60.0).abs() < 1e-12);
    }

    #[test]
    fn wall_clock_reads_nonnegative() {
        assert!(WallClock.now() >= 0.0);
    }
}
