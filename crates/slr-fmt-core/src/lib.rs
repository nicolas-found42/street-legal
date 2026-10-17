//! Shared primitives for the `slr-fmt-*` format crates: the common error type
//! and the little-endian [`reader`] cursor over the shipped file formats of
//! Street Legal Racing: Redline (build 606). No game logic lives here.

pub mod reader;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("bad magic: expected {expected:?}, found {found:?}")]
    BadMagic {
        expected: &'static str,
        found: String,
    },
    #[error("unsupported version {0:#x}")]
    UnsupportedVersion(u32),
    #[error("truncated data: need {need} bytes at offset {at}, have {have}")]
    Truncated { at: usize, need: usize, have: usize },
    #[error("inconsistent {what}: {detail}")]
    Inconsistent { what: &'static str, detail: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = FormatError> = std::result::Result<T, E>;
