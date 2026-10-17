//! The [`Recorder`] backend trait: one interface for the hook layer (M0-11,
//! Python/C) and the headless backends (M1-12).
//!
//! A recorder accepts one record at a time and counts ordinals. The trait is
//! deliberately narrow — seven record kinds in, one error out — so a Python
//! or C emitter can implement the same framing by hand (see
//! `docs/call-log.md` §5) and stay byte-identical to [`LogWriter`].

use crate::error::Result;
use crate::model::{
    D3d9CallKind, DirectInputKind, DirectSoundKind, FileOpKind, FrameVehicleState, LogRecord,
    Value, VmSnapshot,
};

/// A sink for call-log records.
///
/// Implementations MUST preserve call order exactly: the diff tool compares
/// logs record-by-record, so one reordered or dropped record reads as a
/// divergence at that ordinal, not as noise to be skipped over.
pub trait Recorder {
    /// Append one record. Fails on I/O or encoding errors.
    fn push(&mut self, record: LogRecord) -> Result<()>;

    /// Record one native method call.
    fn native_call(
        &mut self,
        class: &str,
        name: &str,
        args: Vec<Value>,
        result: Option<Value>,
    ) -> Result<()> {
        self.push(LogRecord::NativeCall {
            class: class.to_owned(),
            name: name.to_owned(),
            args,
            result,
        })
    }

    /// Record one D3D9 entry-point call.
    fn d3d9_call(&mut self, kind: D3d9CallKind, method: &str, args: Vec<Value>) -> Result<()> {
        self.push(LogRecord::D3d9Call {
            kind,
            method: method.to_owned(),
            args,
        })
    }

    /// Record one DirectSound entry-point call.
    fn direct_sound_call(
        &mut self,
        kind: DirectSoundKind,
        method: &str,
        args: Vec<Value>,
    ) -> Result<()> {
        self.push(LogRecord::DirectSoundCall {
            kind,
            method: method.to_owned(),
            args,
        })
    }

    /// Record one DirectInput poll.
    fn direct_input(&mut self, kind: DirectInputKind, report: Vec<u8>) -> Result<()> {
        self.push(LogRecord::DirectInput { kind, report })
    }

    /// Record one file-I/O operation.
    fn file_io(&mut self, op: FileOpKind, path: &str, detail: Vec<Value>) -> Result<()> {
        self.push(LogRecord::FileIo {
            op,
            path: path.to_owned(),
            detail,
        })
    }

    /// Record one whole-VM snapshot.
    fn vm_snapshot(&mut self, snapshot: VmSnapshot) -> Result<()> {
        self.push(LogRecord::VmSnapshot(snapshot))
    }

    /// Record one frame of vehicle state.
    fn frame_vehicle(&mut self, state: FrameVehicleState) -> Result<()> {
        self.push(LogRecord::FrameVehicle(state))
    }
}

/// A [`Recorder`] that writes straight through to a [`LogWriter`](crate::writer::LogWriter).
pub struct WriterRecorder<W: std::io::Write> {
    writer: crate::writer::LogWriter<W>,
}

impl<W: std::io::Write> WriterRecorder<W> {
    /// Wrap a sink, emitting the log header immediately.
    pub fn create(inner: W) -> Result<Self> {
        Ok(Self {
            writer: crate::writer::LogWriter::create(inner)?,
        })
    }

    /// Flush the underlying sink.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()
    }

    /// Finish and return the sink.
    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
}

impl<W: std::io::Write> Recorder for WriterRecorder<W> {
    fn push(&mut self, record: LogRecord) -> Result<()> {
        self.writer.record(&record)
    }
}

/// A [`Recorder`] that keeps every record in memory, for tests and for
/// backends that serialize a complete run at the end.
#[derive(Debug, Default)]
pub struct MemRecorder {
    records: Vec<LogRecord>,
}

impl MemRecorder {
    /// Create an empty in-memory recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The records pushed so far, in order.
    #[must_use]
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// Ordinal the next pushed record will carry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no record has been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Serialize every buffered record into `inner`, header first.
    pub fn write_to<W: std::io::Write>(&self, inner: W) -> Result<()> {
        let mut w = crate::writer::LogWriter::create(inner)?;
        for r in &self.records {
            w.record(r)?;
        }
        w.flush()?;
        Ok(())
    }
}

impl Recorder for MemRecorder {
    fn push(&mut self, record: LogRecord) -> Result<()> {
        self.records.push(record);
        Ok(())
    }
}

/// A [`Recorder`] that counts records and drops them, for measuring hook
/// overhead without storing or serializing anything.
#[derive(Debug, Default)]
pub struct NullRecorder {
    count: u64,
}

impl NullRecorder {
    /// Create a fresh counting recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records dropped so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl Recorder for NullRecorder {
    fn push(&mut self, _record: LogRecord) -> Result<()> {
        self.count += 1;
        Ok(())
    }
}
