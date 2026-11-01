//! DirectSound-equivalent audio (M6-01..M6-03, obligations G7/S6/P28).
//!
//! `stream` and `diff` are the canonical call-stream schema and its strict
//! first-divergence comparator (docs/audio.md). `device` is the software
//! DirectSound device the ported sound library calls.

pub mod device;
pub mod diff;
pub mod stream;
