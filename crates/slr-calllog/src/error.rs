//! [`LogError`]: every failure mode of the call-log reader and writer.

use thiserror::Error;

/// Every way reading or writing a call log can fail.
///
/// Unknown record kinds, value tags and file ops are hard errors, never
/// silent skips: a log that contains something this reader does not
/// understand must be rejected loudly so a format extension can never be
/// mistaken for a passing comparison. See `docs/call-log.md`.
#[derive(Debug, Error)]
pub enum LogError {
    /// First six bytes are not `SLRLOG`.
    #[error("bad magic at offset {offset}: expected \"SLRLOG\", found {found:?}")]
    BadMagic { offset: usize, found: Vec<u8> },
    /// Header version this reader does not implement.
    #[error("unsupported format version {found}; this reader supports version {supported}")]
    UnsupportedVersion { found: u16, supported: u16 },
    /// Fewer bytes than a complete value needs.
    #[error("truncated log at offset {at}: need {need} byte(s) for {what}")]
    Truncated {
        at: usize,
        need: usize,
        what: &'static str,
    },
    /// Record tag byte this reader does not implement.
    #[error("unknown record kind {tag:#04x} at offset {at}")]
    UnknownRecordKind { tag: u8, at: usize },
    /// Argument/result tag byte this reader does not implement.
    #[error("unknown value tag {tag:#04x} at offset {at}")]
    UnknownValueTag { tag: u8, at: usize },
    /// File-operation byte this reader does not implement.
    #[error("unknown file-op {op:#04x} at offset {at}")]
    UnknownFileOp { op: u8, at: usize },
    /// A length-prefixed string is not valid UTF-8.
    #[error("invalid UTF-8 in {what} at offset {at}")]
    InvalidUtf8 { what: &'static str, at: usize },
    /// A bool payload byte is neither 0 nor 1.
    #[error("invalid bool byte {byte:#04x} at offset {at}")]
    InvalidBool { byte: u8, at: usize },
    /// A count or byte length exceeds the documented sanity cap.
    #[error("{what} of {n} at offset {at} exceeds the limit of {limit}")]
    LimitExceeded {
        what: &'static str,
        n: usize,
        limit: usize,
        at: usize,
    },
    /// Underlying I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Fallible call-log result.
pub type Result<T, E = LogError> = std::result::Result<T, E>;
