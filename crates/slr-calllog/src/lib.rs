//! Call-log trace format (M0-10): one versioned schema for native method calls,
//! D3D9 / DirectSound / DirectInput calls, file I/O, VM state snapshots, and
//! per-frame vehicle state, plus a streaming reader/writer and a [`Recorder`]
//! backend trait shared by the hook layer (M0-11) and headless backends (M1-12).
//!
//! The canonical byte-level spec lives in `docs/call-log.md`; this module is
//! the normative implementation of it.

pub mod diff;
pub mod error;
pub mod model;
pub mod reader;
pub mod recorder;
pub mod writer;

pub use diff::{first_divergence, DiffOutcome, Divergence, FieldDiff};
pub use error::{LogError, Result};
pub use model::{
    D3d9CallKind, DirectInputKind, DirectSoundKind, FileOpKind, FrameVehicleState, LogRecord,
    Value, VmSnapshot, D3D9_CALL_KINDS, D3D9_CALL_KIND_MAX, DIRECT_INPUT_KINDS,
    DIRECT_INPUT_KIND_MAX, DIRECT_SOUND_KINDS, DIRECT_SOUND_KIND_MAX, FORMAT_VERSION, MAGIC,
    MAX_ARGS, MAX_RECORD_BYTES, VALUE_TAG_MAX,
};
pub use reader::LogReader;
pub use recorder::{MemRecorder, NullRecorder, Recorder};
pub use writer::LogWriter;
