//! D3D9-equivalent call streams for obligations G6/S5 (ADR 0009 tier D).
//!
//! The raw stream is the `slr-calllog` D3D9 record (tag 0x01) that the headless
//! original's `d3d9_device.py` writes. The port's translator writes the same
//! record kind through [`emit`]. [`canon`] turns either raw log into the
//! canonical per-frame stream (frames split at `Present`, every draw carrying
//! its full effective non-default state), [`compare`] reports the first
//! divergence per frame and [`inventory`] lists the D3D9 features a log uses.
//! The format is specified in `docs/render.md`.
pub mod canon;
pub mod compare;
pub mod defaults;
pub mod emit;
pub mod inventory;
pub mod state;
pub mod taxonomy;

/// Schema tag written into every canonical document and comparison report.
pub const CANON_SCHEMA: &str = "slr-d3d9-canon-1";

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("call log: {0}")]
    Log(#[from] slr_calllog::LogError),
    #[error("record #{ordinal}: unknown D3D9 method {method:?} (the taxonomy fails closed)")]
    UnknownMethod { ordinal: u64, method: String },
    #[error("record #{ordinal}: unsupported D3D9 method {method:?}: {why}")]
    Unsupported {
        ordinal: u64,
        method: String,
        why: &'static str,
    },
    #[error("record #{ordinal}: {method} argument {index}: {why}")]
    BadArgument {
        ordinal: u64,
        method: String,
        index: usize,
        why: &'static str,
    },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, StreamError>;
