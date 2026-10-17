//! Platform seams: window, `Clock`, `Rng` (exact per-thread MSVC `rand`),
//! replay driver (plan M0-13).
//!
//! The `rand` port below mirrors the exe's CRT helpers bit for bit:
//! `srand` stores its seed at per-thread-data offset `0x14`, and `rand`
//! advances that word with the classic MSVC linear congruential generator.
//! Later milestones plug the game loop into [`Replay`] (fixed seed, fixed
//! time step, scripted input) so every run is a deterministic replay.

mod clock;
mod replay;
mod rng;

pub use clock::{Clock, FixedClock, WallClock};
pub use replay::{InputFrame, Replay, ReplayConfig};
pub use rng::{current_thread_rng, seed_current_thread, MsvcRand};
