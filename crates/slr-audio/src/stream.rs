//! The canonical DirectSound call stream (`slr-dsound-stream/1`, docs/audio.md).
//!
//! One JSON object per line: a header (provenance) followed by events. The
//! Python recorder (`re/headless/dsound_trace.py`) and the port's software
//! device emit the same shape, so one comparator judges both.

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

/// Schema tag in every stream header.
pub const SCHEMA: &str = "slr-dsound-stream/1";

/// One `IDirectSound*` call: compared fields first, evidence-only `meta` last.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Ordinal within the stream (compared implicitly: position is the key).
    pub i: u64,
    /// `Interface.Method`, or `DirectSoundCreate`.
    pub call: String,
    /// Canonical object name (`ds`, `buf#3`, `listener`, `3d#3`, `api`).
    pub this: String,
    /// Decoded arguments (an object; floats are `f32:0x........` strings).
    pub args: Value,
    /// HRESULT, or the reference count for `AddRef`/`Release`.
    pub ret: u64,
    /// Decoded out values; empty for a failed call.
    pub out: Value,
    /// Evidence for humans; never compared.
    pub meta: Value,
}

impl Event {
    pub fn to_json(&self) -> Value {
        json!({
            "i": self.i, "call": self.call, "this": self.this, "args": self.args,
            "ret": self.ret, "out": self.out, "meta": self.meta,
        })
    }

    pub fn from_json(value: &Value) -> Result<Event, StreamError> {
        let object = value
            .as_object()
            .ok_or_else(|| StreamError::Shape("event is not an object".into()))?;
        let text = |key: &str| -> Result<String, StreamError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    StreamError::Shape(format!("event field `{key}` missing or not a string"))
                })
        };
        let number = |key: &str| -> Result<u64, StreamError> {
            object.get(key).and_then(Value::as_u64).ok_or_else(|| {
                StreamError::Shape(format!(
                    "event field `{key}` missing or not an unsigned integer"
                ))
            })
        };
        let tree = |key: &str| -> Result<Value, StreamError> {
            match object.get(key) {
                Some(v) if v.is_object() => Ok(v.clone()),
                _ => Err(StreamError::Shape(format!(
                    "event field `{key}` missing or not an object"
                ))),
            }
        };
        for key in object.keys() {
            if !["i", "call", "this", "args", "ret", "out", "meta"].contains(&key.as_str()) {
                return Err(StreamError::Shape(format!("unknown event field `{key}`")));
            }
        }
        Ok(Event {
            i: number("i")?,
            call: text("call")?,
            this: text("this")?,
            args: tree("args")?,
            ret: number("ret")?,
            out: tree("out")?,
            meta: tree("meta")?,
        })
    }
}

/// A parsed stream: the header object and its events, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub header: Map<String, Value>,
    pub events: Vec<Event>,
}

#[derive(Debug)]
pub enum StreamError {
    Io(String),
    Json {
        line: usize,
        message: String,
    },
    Shape(String),
    Schema(String),
    Ordinal {
        line: usize,
        expected: u64,
        found: u64,
    },
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamError::Io(m) => write!(f, "io: {m}"),
            StreamError::Json { line, message } => {
                write!(f, "line {line}: invalid JSON: {message}")
            }
            StreamError::Shape(m) => write!(f, "{m}"),
            StreamError::Schema(m) => write!(f, "schema: {m}"),
            StreamError::Ordinal {
                line,
                expected,
                found,
            } => write!(f, "line {line}: event ordinal {found}, expected {expected}"),
        }
    }
}

impl std::error::Error for StreamError {}

impl Stream {
    pub fn new(header: Map<String, Value>) -> Stream {
        let mut header = header;
        header.insert("schema".into(), Value::String(SCHEMA.into()));
        Stream {
            header,
            events: Vec::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Stream, StreamError> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let (_, first) = lines
            .next()
            .ok_or_else(|| StreamError::Schema("empty stream (no header)".into()))?;
        let header: Value = serde_json::from_str(first).map_err(|e| StreamError::Json {
            line: 1,
            message: e.to_string(),
        })?;
        let header = match header {
            Value::Object(map) => map,
            _ => return Err(StreamError::Schema("header is not an object".into())),
        };
        if header.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
            return Err(StreamError::Schema(format!(
                "header schema is not `{SCHEMA}`"
            )));
        }
        let mut events = Vec::new();
        for (index, line) in lines {
            let value: Value = serde_json::from_str(line).map_err(|e| StreamError::Json {
                line: index + 1,
                message: e.to_string(),
            })?;
            let event = Event::from_json(&value)?;
            if event.i != events.len() as u64 {
                return Err(StreamError::Ordinal {
                    line: index + 1,
                    expected: events.len() as u64,
                    found: event.i,
                });
            }
            events.push(event);
        }
        Ok(Stream { header, events })
    }

    pub fn read(path: &Path) -> Result<Stream, StreamError> {
        let text = fs::read_to_string(path)
            .map_err(|e| StreamError::Io(format!("{}: {e}", path.display())))?;
        Stream::parse(&text)
    }

    pub fn render(&self) -> String {
        let mut out =
            serde_json::to_string(&Value::Object(self.header.clone())).expect("header serialises");
        out.push('\n');
        for event in &self.events {
            out.push_str(&serde_json::to_string(&event.to_json()).expect("event serialises"));
            out.push('\n');
        }
        out
    }

    pub fn write(&self, path: &Path) -> Result<(), StreamError> {
        fs::write(path, self.render())
            .map_err(|e| StreamError::Io(format!("{}: {e}", path.display())))
    }
}

/// `f32:0x........` for an exact single-precision bit pattern.
pub fn f32_word(bits: u32) -> String {
    format!("f32:0x{bits:08x}")
}
