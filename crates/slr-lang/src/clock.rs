//! The original performance-counter clock, independent of a platform provider.
//!
//! `0x5516c0` queries frequency and current counter on every read, ignores both
//! BOOL results, then uses signed `FILD m64`, `FSUB`, `FDIV` and `FMUL 1000.0f`.
//! Integer loads are exact before arithmetic precision applies. The established
//! start counter is never advanced here. A provider may leave its destination
//! unchanged on failure, so the old qword remains observable.
//!
//! `initialize` queries the initial frequency and start counter, always returning
//! zero. It leaves the last sampled `now` word alone. This module does not select
//! a real macOS provider or replace the language host's supplied-reading seam.

use slr_x87::{Precision, Rounding, X87};

/// The two Win32 output-parameter queries used by the clock.
pub trait PerformanceCounters {
    /// Query the counter frequency into the persistent destination qword.
    fn query_frequency(&mut self, destination: &mut i64) -> bool;
    /// Query the current count into the persistent destination qword.
    fn query_counter(&mut self, destination: &mut i64) -> bool;
}

/// Persistent qwords used by the original helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterState {
    pub frequency: i64,
    pub start: i64,
    pub now: i64,
}

/// A clock that owns its persistent frequency, start and current counter words.
#[derive(Clone, Copy, Debug)]
pub struct CounterClock {
    state: CounterState,
}

impl CounterClock {
    /// Query the initial frequency and start count, preserving the old `now`.
    ///
    /// As in the original initializer, both BOOL results are ignored and the
    /// integer return is always zero. Failed queries may retain old words.
    ///
    /// exe: FUN_00551690
    pub fn initialize(&mut self, source: &mut impl PerformanceCounters) -> u32 {
        let _ = source.query_frequency(&mut self.state.frequency);
        let _ = source.query_counter(&mut self.state.start);
        0
    }

    /// Install the initialized or restored original qword state.
    pub fn from_state(state: CounterState) -> Self {
        Self { state }
    }

    /// Observe the persistent output words and the unchanged start count.
    pub fn state(&self) -> CounterState {
        self.state
    }

    /// Query both platform counters and return the original x87 register value.
    ///
    /// The caller supplies its FPU arithmetic controls; the game configuration
    /// is [`Precision::F32`] and [`Rounding::NearestEven`]. No f32 or f64 store
    /// is introduced at this helper's return boundary.
    ///
    /// exe: FUN_005516c0
    pub fn read_milliseconds(
        &mut self,
        source: &mut impl PerformanceCounters,
        precision: Precision,
        rounding: Rounding,
    ) -> X87 {
        let _ = source.query_frequency(&mut self.state.frequency);
        let _ = source.query_counter(&mut self.state.now);
        X87::from_i64(self.state.now)
            .sub(X87::from_i64(self.state.start), precision, rounding)
            .div(X87::from_i64(self.state.frequency), precision, rounding)
            .mul(X87::from_f32(1000.0), precision, rounding)
    }
}
