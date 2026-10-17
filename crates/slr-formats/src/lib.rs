//! Parsers for the shipped file formats of Street Legal Racing: Redline (build 606).
//!
//! Every parser here was derived from the shipped binaries and verified against the
//! reference corpus; see `docs/formats.md` for the evidence behind each layout.

pub mod cfg;
pub mod corpus;
pub mod rpak;
pub mod scx;
pub mod sdat;
pub mod tufa;

mod reader;

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

pub type Result<T> = std::result::Result<T, FormatError>;
