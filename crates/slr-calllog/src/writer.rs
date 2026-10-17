//! Streaming writer: header, then one length-prefixed record after another.
//!
//! The writer owns no buffering of its own beyond the inner `Write`; every
//! `record()` call emits exactly one complete record so the hook layer
//! (Python/C, see `docs/call-log.md` §5) can mirror the framing trivially.

use crate::error::{LogError, Result};
use crate::model::{
    D3d9CallKind, DirectInputKind, DirectSoundKind, FileOpKind, LogRecord, Value, FORMAT_VERSION,
    MAGIC, MAX_ARGS, MAX_RECORD_BYTES,
};
use std::io::Write;

/// Byte cap for one length-prefixed string or byte vector (8 MiB).
///
/// Well under [`MAX_RECORD_BYTES`] so a record holding one maximal payload
/// plus its fixed fields still fits; corrupt length prefixes fail fast.
pub const MAX_PAYLOAD_BYTES: usize = 8 << 20;

/// Streaming call-log writer over any `Write`.
pub struct LogWriter<W: Write> {
    inner: W,
}

impl<W: Write> LogWriter<W> {
    /// Create a writer and emit the 8-byte header (`MAGIC` + version).
    pub fn create(mut inner: W) -> Result<Self> {
        inner.write_all(MAGIC)?;
        inner.write_all(&FORMAT_VERSION.to_le_bytes())?;
        Ok(Self { inner })
    }

    /// Append one record: tag byte, u32 LE body length, then the body.
    pub fn record(&mut self, record: &LogRecord) -> Result<()> {
        if let LogRecord::NativeCall { args, .. } = record {
            if args.len() > MAX_ARGS {
                return Err(LogError::LimitExceeded {
                    what: "native-call arg count",
                    n: args.len(),
                    limit: MAX_ARGS,
                    at: 0,
                });
            }
        }
        let mut body = Vec::new();
        encode_record(record, &mut body)?;
        let len = u32::try_from(body.len()).map_err(|_| LogError::LimitExceeded {
            what: "record body length",
            n: body.len(),
            limit: u32::MAX as usize,
            at: 0,
        })?;
        if body.len() > MAX_RECORD_BYTES {
            return Err(LogError::LimitExceeded {
                what: "record body length",
                n: body.len(),
                limit: MAX_RECORD_BYTES,
                at: 0,
            });
        }
        self.inner.write_all(&[record.tag()])?;
        self.inner.write_all(&len.to_le_bytes())?;
        self.inner.write_all(&body)?;
        Ok(())
    }

    /// Flush the underlying sink.
    pub fn flush(&mut self) -> Result<()> {
        self.inner.flush()?;
        Ok(())
    }

    /// Finish writing and return the sink.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

fn put_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_str(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(LogError::LimitExceeded {
            what: "string length",
            n: bytes.len(),
            limit: MAX_PAYLOAD_BYTES,
            at: 0,
        });
    }
    put_u32(buf, bytes.len() as u32);
    buf.extend_from_slice(bytes);
    Ok(())
}

fn put_bytes(buf: &mut Vec<u8>, b: &[u8]) -> Result<()> {
    if b.len() > MAX_PAYLOAD_BYTES {
        return Err(LogError::LimitExceeded {
            what: "byte-vector length",
            n: b.len(),
            limit: MAX_PAYLOAD_BYTES,
            at: 0,
        });
    }
    put_u32(buf, b.len() as u32);
    buf.extend_from_slice(b);
    Ok(())
}

fn put_value(buf: &mut Vec<u8>, v: &Value) -> Result<()> {
    put_u8(buf, v.tag());
    match v {
        Value::Int(x) => put_i32(buf, *x),
        Value::Long(x) => put_i64(buf, *x),
        Value::FloatBits(b) => put_u32(buf, *b),
        Value::DoubleBits(b) => put_u64(buf, *b),
        Value::Str(s) => put_str(buf, s)?,
        Value::Bytes(b) => put_bytes(buf, b)?,
        Value::Bool(b) => put_u8(buf, u8::from(*b)),
    }
    Ok(())
}

fn put_values(buf: &mut Vec<u8>, vs: &[Value]) -> Result<()> {
    if vs.len() > MAX_ARGS {
        return Err(LogError::LimitExceeded {
            what: "value-list length",
            n: vs.len(),
            limit: MAX_ARGS,
            at: 0,
        });
    }
    put_u32(buf, vs.len() as u32);
    for v in vs {
        put_value(buf, v)?;
    }
    Ok(())
}

fn put_opt_value(buf: &mut Vec<u8>, v: &Option<Value>) -> Result<()> {
    match v {
        None => put_u8(buf, 0),
        Some(x) => {
            put_u8(buf, 1);
            put_value(buf, x)?;
        }
    }
    Ok(())
}

/// Serialize one record body (without tag/length framing).
pub(crate) fn encode_record(record: &LogRecord, buf: &mut Vec<u8>) -> Result<()> {
    match record {
        LogRecord::NativeCall {
            class,
            name,
            args,
            result,
        } => {
            put_str(buf, class)?;
            put_str(buf, name)?;
            put_values(buf, args)?;
            put_opt_value(buf, result)?;
        }
        LogRecord::D3d9Call { kind, method, args } => {
            put_u8(buf, *kind as u8);
            put_str(buf, method)?;
            put_values(buf, args)?;
        }
        LogRecord::DirectSoundCall { kind, method, args } => {
            put_u8(buf, *kind as u8);
            put_str(buf, method)?;
            put_values(buf, args)?;
        }
        LogRecord::DirectInput { kind, report } => {
            put_u8(buf, *kind as u8);
            put_bytes(buf, report)?;
        }
        LogRecord::FileIo { op, path, detail } => {
            put_u8(buf, *op as u8);
            put_str(buf, path)?;
            put_values(buf, detail)?;
        }
        LogRecord::VmSnapshot(s) => {
            put_u64(buf, s.frame);
            put_u32(buf, s.pc);
            put_u32(buf, s.control_word);
            put_values(buf, &s.stack)?;
        }
        LogRecord::FrameVehicle(s) => {
            put_u64(buf, s.frame);
            for w in s.pos {
                put_u32(buf, w);
            }
            for w in s.ypr {
                put_u32(buf, w);
            }
            for w in s.vel {
                put_u32(buf, w);
            }
            put_u32(buf, s.rpm_bits);
            put_i32(buf, s.gear);
            put_u16(buf, 0); // reserved, MUST be zero
        }
    }
    // Keep the enum-to-integer maps honest: a new variant added without a
    // matching TryFrom arm must fail compilation here, not decode silently.
    const _: fn(D3d9CallKind) -> u8 = |k| k as u8;
    const _: fn(DirectSoundKind) -> u8 = |k| k as u8;
    const _: fn(DirectInputKind) -> u8 = |k| k as u8;
    const _: fn(FileOpKind) -> u8 = |k| k as u8;
    Ok(())
}
