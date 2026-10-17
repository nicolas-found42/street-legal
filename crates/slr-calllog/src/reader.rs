//! Streaming reader: header validation, then one framed record at a time.
//!
//! The reader reports absolute byte offsets in every error so a corrupt log
//! can be bisected with `dd`. Unknown tags are hard errors, never skips.

use crate::error::{LogError, Result};
use crate::model::{
    D3d9CallKind, DirectInputKind, DirectSoundKind, FileOpKind, FrameVehicleState, LogRecord,
    Value, VmSnapshot, FORMAT_VERSION, MAGIC, MAX_ARGS,
};
use crate::writer::MAX_PAYLOAD_BYTES;
use std::io::{Read, Seek, SeekFrom};

/// Length assigned to the record-tag byte in error offsets.
const TAG_LEN: usize = 1;
/// Length of the u32 LE body-length prefix in error offsets.
const LEN_LEN: usize = 4;

/// Streaming call-log reader over any `Read`.
///
/// Records are borrowed from an internal frame buffer: the returned
/// reference lives as long as the next `next_record` call, which suits the
/// diff tool's lockstep walk without cloning either log.
pub struct LogReader<R: Read> {
    inner: R,
    /// Absolute offset of the next unread byte (== bytes consumed so far).
    offset: usize,
    /// Scratch frame: tag + body of the current record.
    frame: Vec<u8>,
    /// Decoded current record plus the offset its tag byte was read at.
    current: Option<(LogRecord, usize)>,
    /// Number of records returned so far (the ordinal of the next one).
    ordinal: u64,
}

impl<R: Read> LogReader<R> {
    /// Open a reader and validate the 8-byte header.
    pub fn open(mut inner: R) -> Result<Self> {
        let mut magic = [0u8; 6];
        inner.read_exact(&mut magic).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                LogError::Truncated {
                    at: 0,
                    need: 6,
                    what: "file magic",
                }
            } else {
                LogError::Io(e)
            }
        })?;
        if &magic != MAGIC {
            return Err(LogError::BadMagic {
                offset: 0,
                found: magic.to_vec(),
            });
        }
        let mut ver = [0u8; 2];
        inner
            .read_exact(&mut ver)
            .map_err(|_| LogError::Truncated {
                at: 6,
                need: 2,
                what: "format version",
            })?;
        let found = u16::from_le_bytes(ver);
        if found != FORMAT_VERSION {
            return Err(LogError::UnsupportedVersion {
                found,
                supported: FORMAT_VERSION,
            });
        }
        Ok(Self {
            inner,
            offset: 8,
            frame: Vec::new(),
            current: None,
            ordinal: 0,
        })
    }

    /// Ordinal (0-based) of the record the next call will return.
    #[must_use]
    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Read the next record, or `Ok(None)` at clean EOF.
    ///
    /// A truncated tag, length or body is [`LogError::Truncated`]; EOF is
    /// only clean exactly on a record boundary.
    pub fn next_record(&mut self) -> Result<Option<&LogRecord>> {
        let mut tag = [0u8; 1];
        match self.inner.read_exact(&mut tag) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(e) => return Err(LogError::Io(e)),
        }
        let tag_at = self.offset;
        self.offset += TAG_LEN;
        LogRecord::check_tag(tag[0], tag_at)?;

        let mut len = [0u8; 4];
        self.inner
            .read_exact(&mut len)
            .map_err(|_| LogError::Truncated {
                at: self.offset,
                need: LEN_LEN,
                what: "record length",
            })?;
        self.offset += LEN_LEN;
        let body_len = u32::from_le_bytes(len) as usize;
        if body_len > crate::model::MAX_RECORD_BYTES {
            return Err(LogError::LimitExceeded {
                what: "record body length",
                n: body_len,
                limit: crate::model::MAX_RECORD_BYTES,
                at: tag_at,
            });
        }

        self.frame.resize(body_len, 0);
        self.inner
            .read_exact(&mut self.frame)
            .map_err(|_| LogError::Truncated {
                at: self.offset,
                need: body_len,
                what: "record body",
            })?;
        let body_at = self.offset;
        self.offset += body_len;

        let mut cursor = Cursor {
            data: &self.frame,
            pos: 0,
            base: body_at,
        };
        let record = decode_record(tag[0], &mut cursor)?;
        if cursor.pos != body_len {
            // Trailing bytes inside a framed body mean encoder/decoder drift.
            return Err(LogError::Truncated {
                at: body_at + cursor.pos,
                need: body_len - cursor.pos,
                what: "record trailing bytes",
            });
        }
        self.current = Some((record, tag_at));
        self.ordinal += 1;
        Ok(self.current.as_ref().map(|(r, _)| r))
    }

    /// Byte offset where the most recently returned record's tag was read.
    #[must_use]
    pub fn current_offset(&self) -> Option<usize> {
        self.current.as_ref().map(|(_, at)| *at)
    }
}

impl<R: Read + Seek> LogReader<R> {
    /// Absolute offset one past the last byte consumed (for diagnostics).
    pub fn stream_offset(&mut self) -> Result<u64> {
        Ok(self.inner.stream_position()?)
    }

    /// Reposition onto a record boundary previously observed (debugging aid).
    pub fn seek_to(&mut self, pos: u64) -> Result<()> {
        self.inner.seek(SeekFrom::Start(pos))?;
        self.offset = pos as usize;
        self.current = None;
        Ok(())
    }
}

/// Byte cursor over one record body; every error carries an absolute offset.
pub(crate) struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub base: usize,
}

impl Cursor<'_> {
    fn at(&self) -> usize {
        self.base + self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize, what: &'static str) -> Result<&[u8]> {
        if self.data.len().saturating_sub(self.pos) < n {
            return Err(LogError::Truncated {
                at: self.at(),
                need: n,
                what,
            });
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub(crate) fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.bytes(1, what)?[0])
    }

    pub(crate) fn u16(&mut self, what: &'static str) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2, what)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self, what: &'static str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4, what)?.try_into().unwrap()))
    }

    pub(crate) fn i32(&mut self, what: &'static str) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4, what)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self, what: &'static str) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8, what)?.try_into().unwrap()))
    }

    pub(crate) fn i64(&mut self, what: &'static str) -> Result<i64> {
        Ok(i64::from_le_bytes(self.bytes(8, what)?.try_into().unwrap()))
    }

    pub(crate) fn len_prefix(&mut self, what: &'static str) -> Result<usize> {
        let n = self.u32(what)? as usize;
        if n > MAX_PAYLOAD_BYTES {
            return Err(LogError::LimitExceeded {
                what,
                n,
                limit: MAX_PAYLOAD_BYTES,
                at: self.at(),
            });
        }
        Ok(n)
    }

    pub(crate) fn str_data(&mut self, what: &'static str) -> Result<String> {
        let at = self.at();
        let n = self.len_prefix(what)?;
        let raw = self.bytes(n, what)?;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| LogError::InvalidUtf8 { what, at })
    }

    pub(crate) fn byte_vec(&mut self, what: &'static str) -> Result<Vec<u8>> {
        let n = self.len_prefix(what)?;
        Ok(self.bytes(n, what)?.to_vec())
    }

    pub(crate) fn value(&mut self) -> Result<Value> {
        let at = self.at();
        let tag = self.u8("value tag")?;
        match tag {
            0x00 => Ok(Value::Int(self.i32("int payload")?)),
            0x01 => Ok(Value::Long(self.i64("long payload")?)),
            0x02 => Ok(Value::FloatBits(self.u32("float payload")?)),
            0x03 => Ok(Value::DoubleBits(self.u64("double payload")?)),
            0x04 => Ok(Value::Str(self.str_data("string payload")?)),
            0x05 => Ok(Value::Bytes(self.byte_vec("bytes payload")?)),
            0x06 => match self.u8("bool payload")? {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                b => Err(LogError::InvalidBool {
                    byte: b,
                    at: self.at(),
                }),
            },
            other => Err(LogError::UnknownValueTag { tag: other, at }),
        }
    }

    pub(crate) fn value_list(&mut self, what: &'static str) -> Result<Vec<Value>> {
        let at = self.at();
        let n = self.u32(what)? as usize;
        if n > MAX_ARGS {
            return Err(LogError::LimitExceeded {
                what,
                n,
                limit: MAX_ARGS,
                at,
            });
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.value()?);
        }
        Ok(out)
    }

    pub(crate) fn opt_value(&mut self) -> Result<Option<Value>> {
        match self.u8("option discriminant")? {
            0 => Ok(None),
            1 => Ok(self.value().map(Some)?),
            b => Err(LogError::InvalidBool {
                byte: b,
                at: self.at(),
            }),
        }
    }
}

/// Decode one record body (tag already validated by the caller).
pub(crate) fn decode_record(tag: u8, c: &mut Cursor<'_>) -> Result<LogRecord> {
    match tag {
        0x00 => Ok(LogRecord::NativeCall {
            class: c.str_data("native class")?,
            name: c.str_data("native name")?,
            args: c.value_list("native arg count")?,
            result: c.opt_value()?,
        }),
        0x01 => {
            let at = c.at();
            let raw = c.u8("d3d9 kind")?;
            let kind = D3d9CallKind::try_from(raw)
                .map_err(|_| LogError::UnknownRecordKind { tag: raw, at })?;
            Ok(LogRecord::D3d9Call {
                kind,
                method: c.str_data("d3d9 method")?,
                args: c.value_list("d3d9 arg count")?,
            })
        }
        0x02 => {
            let at = c.at();
            let raw = c.u8("direct-sound kind")?;
            let kind = DirectSoundKind::try_from(raw)
                .map_err(|_| LogError::UnknownRecordKind { tag: raw, at })?;
            Ok(LogRecord::DirectSoundCall {
                kind,
                method: c.str_data("direct-sound method")?,
                args: c.value_list("direct-sound arg count")?,
            })
        }
        0x03 => {
            let at = c.at();
            let raw = c.u8("direct-input kind")?;
            let kind = DirectInputKind::try_from(raw)
                .map_err(|_| LogError::UnknownRecordKind { tag: raw, at })?;
            Ok(LogRecord::DirectInput {
                kind,
                report: c.byte_vec("direct-input report")?,
            })
        }
        0x04 => {
            let at = c.at();
            let raw = c.u8("file-op")?;
            let op =
                FileOpKind::try_from(raw).map_err(|_| LogError::UnknownFileOp { op: raw, at })?;
            Ok(LogRecord::FileIo {
                op,
                path: c.str_data("file path")?,
                detail: c.value_list("file detail count")?,
            })
        }
        0x05 => Ok(LogRecord::VmSnapshot(VmSnapshot {
            frame: c.u64("snapshot frame")?,
            pc: c.u32("snapshot pc")?,
            control_word: c.u32("snapshot control word")?,
            stack: c.value_list("snapshot stack depth")?,
        })),
        0x06 => {
            let frame = c.u64("vehicle frame")?;
            let mut pos = [0u32; 3];
            let mut ypr = [0u32; 3];
            let mut vel = [0u32; 3];
            for w in &mut pos {
                *w = c.u32("vehicle position word")?;
            }
            for w in &mut ypr {
                *w = c.u32("vehicle orientation word")?;
            }
            for w in &mut vel {
                *w = c.u32("vehicle velocity word")?;
            }
            let rpm_bits = c.u32("vehicle rpm")?;
            let gear = c.i32("vehicle gear")?;
            let reserved = c.u16("vehicle reserved")?;
            if reserved != 0 {
                // Reserved words keep older readers byte-compatible: a
                // nonzero word is a newer writer, which must fail loudly.
                return Err(LogError::UnknownRecordKind {
                    tag: 0x06,
                    at: c.at(),
                });
            }
            Ok(LogRecord::FrameVehicle(FrameVehicleState {
                frame,
                pos,
                ypr,
                vel,
                rpm_bits,
                gear,
            }))
        }
        other => Err(LogError::UnknownRecordKind {
            tag: other,
            at: c.at(),
        }),
    }
}
