//! Exact port of the exe's MSVC `rand`/`srand` (VC6 CRT).
//!
//! The exe keeps one 32-bit state word per thread at per-thread-data
//! offset `0x14` (see `FUN_005dd8c0`, which initializes it to 1, and
//! `_getptd` = `FUN_005dd8d3`, which hands out the per-thread block).
//! [`MsvcRand`] owns one such word explicitly; the thread-local accessors
//! below mirror `_getptd` by giving each OS thread its own word.

/// One MSVC `rand` state: the `u32` word the exe keeps at `[ptd+0x14]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsvcRand {
    state: u32,
}

impl MsvcRand {
    /// Fresh state exactly as the CRT leaves it for a new thread: 1
    /// (see `FUN_005dd8c0`: `mov dword ptr [eax+0x14], 1`).
    pub fn new() -> Self {
        Self { state: 1 }
    }

    /// exe: FUN_005d73fb
    ///
    /// `srand(seed)`: stores the seed word, with no other effect.
    pub fn seeded(seed: u32) -> Self {
        Self { state: seed }
    }

    /// exe: FUN_005d73fb
    ///
    /// `srand(seed)` on an existing state: stores the seed word.
    pub fn reseed(&mut self, seed: u32) {
        self.state = seed;
    }

    /// exe: FUN_005d7408
    ///
    /// `rand()`: `state = 0x343FD * state + 0x269EC3` (wrapping `u32`),
    /// returning bits 16..=30 of the new state (`(state >> 16) & 0x7fff`).
    pub fn next_u15(&mut self) -> u16 {
        self.state = self.state.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        ((self.state >> 16) & 0x7FFF) as u16
    }

    /// The raw state word (for snapshots and tests).
    pub fn state(&self) -> u32 {
        self.state
    }
}

impl Default for MsvcRand {
    fn default() -> Self {
        Self::new()
    }
}

std::thread_local! {
    static THREAD_RNG: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// Per-thread `rand` state behind the `_getptd` semantics: each OS thread
/// owns its word (initial 1), independent of every other thread's.
fn with_thread_state<R>(f: impl FnOnce(&mut MsvcRand) -> R) -> R {
    THREAD_RNG.with(|cell| {
        let mut rng = MsvcRand { state: cell.get() };
        let out = f(&mut rng);
        cell.set(rng.state);
        out
    })
}

/// exe: FUN_005dd8d3
///
/// `srand(seed)` on the calling thread's word.
pub fn seed_current_thread(seed: u32) {
    with_thread_state(|rng| rng.reseed(seed));
}

/// exe: FUN_005dd8d3
///
/// `rand()` on the calling thread's word.
pub fn current_thread_rng() -> u16 {
    with_thread_state(|rng| rng.next_u15())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First draws after `srand(1)`, computed from the LCG formula itself
    /// (`state = state * 0x343FD + 0x269EC3` wrapping, output
    /// `(state >> 16) & 0x7FFF`): a pure unit test of [`MsvcRand`] with no
    /// oracle provenance (the Unicorn differential lives in
    /// `tests/rand_oracle.rs`).
    #[test]
    fn first_draws_match_lcg_formula() {
        let mut state: u32 = 1;
        let mut rng = MsvcRand::seeded(1);
        for _ in 0..8 {
            state = state.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
            assert_eq!(rng.next_u15(), ((state >> 16) & 0x7FFF) as u16);
        }
    }

    #[test]
    fn new_thread_state_is_one() {
        assert_eq!(MsvcRand::new().state(), 1);
    }

    #[test]
    fn reseeding_restarts_the_sequence() {
        let mut rng = MsvcRand::seeded(42);
        let a: Vec<u16> = (0..4).map(|_| rng.next_u15()).collect();
        rng.reseed(42);
        let b: Vec<u16> = (0..4).map(|_| rng.next_u15()).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn threads_do_not_share_state() {
        seed_current_thread(7);
        let before = current_thread_rng();
        let other = std::thread::spawn(|| {
            // Fresh OS thread: untouched initial state 1, not 7's successor.
            current_thread_rng()
        })
        .join()
        .unwrap();
        let mut fresh = MsvcRand::new();
        assert_eq!(other, fresh.next_u15());
        // Our own word advanced exactly once.
        let mut again = MsvcRand::seeded(7);
        assert_eq!(before, again.next_u15());
    }
}
