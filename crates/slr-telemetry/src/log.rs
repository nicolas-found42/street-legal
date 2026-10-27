//! Leveled, structured events for the diagnostic and window-session paths.
//!
//! Two entry points, one record shape:
//!
//! * [`event`] records a structured event and prints nothing;
//! * [`eprintln`] prints `text` to stderr **byte for byte as `eprintln!` would**
//!   and records the same event. The runner, tests and comparison tooling parse
//!   some of those lines (`STOP animate frame ...`, `NativeFrameBoundary: ...`),
//!   so the text is the caller's, never reformatted, prefixed or levelled.
//!
//! A record goes to the session journal (`session-*.jsonl`, durable per line, so
//! an interactive window session leaves it under its bundle) as kind `log`, and
//! to the capture's `events.jsonl` as an external `log` event while a capture is
//! still open. Records below the threshold (`SLR_LOG`, default `info`) are not
//! recorded; stderr output is never filtered.
use serde_json::{json, Value};
use std::io::Write;

/// Severity, least to most verbose so `level <= threshold` means "recorded".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Level> {
        match text.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Level::Error),
            "warn" | "warning" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            "trace" => Some(Level::Trace),
            _ => None,
        }
    }
}

/// The verbosity recorded: `SLR_LOG`, else `info`. An unknown value is `info`.
#[must_use]
pub fn threshold() -> Level {
    std::env::var("SLR_LOG")
        .ok()
        .and_then(|value| Level::parse(&value))
        .unwrap_or(Level::Info)
}

/// The journal row and capture event body for one record.
#[must_use]
pub fn record(level: Level, target: &str, message: &str, fields: &[(&str, &str)]) -> Value {
    json!({
        "level": level.as_str(),
        "target": target,
        "message": message,
        "fields": fields.iter().map(|(key, value)| json!([key, value])).collect::<Vec<_>>(),
    })
}

/// Record a structured event (nothing is printed).
pub fn event(level: Level, target: &str, message: &str, fields: &[(&str, &str)]) {
    if level > threshold() {
        return;
    }
    let body = record(level, target, message, fields);
    // Journal writes are best effort: a full disk must not turn a diagnostic
    // line into a failure of the thing being diagnosed.
    let _ = crate::journal::emit("log", body);
    if crate::enabled() && !crate::finished() {
        let mut detail = vec![
            ("level".to_owned(), level.as_str().to_owned()),
            ("target".to_owned(), target.to_owned()),
            ("message".to_owned(), message.to_owned()),
        ];
        detail.extend(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        );
        crate::detail("log", detail);
    }
}

/// Write `text` and a newline to `sink` exactly as `eprintln!("{text}")` does.
///
/// Split out so the byte-for-byte guarantee is testable without a process.
pub fn write_line(sink: &mut impl Write, text: &str) -> std::io::Result<()> {
    sink.write_all(text.as_bytes())?;
    sink.write_all(b"\n")
}

/// Print `text` to stderr unchanged and record it as a structured event.
pub fn eprintln(level: Level, target: &str, text: &str) {
    // `eprintln!` panics when stderr is closed; a closed pipe must not take a
    // diagnostic path down, but the text is otherwise identical.
    let _ = write_line(&mut std::io::stderr().lock(), text);
    event(level, target, text, &[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_text_is_passed_through_byte_for_byte() {
        for text in [
            "STOP animate frame 7: NativeFrameBoundary: player state missing",
            "NativeFrameBoundary: nonempty player animate state",
            "SessionRunnerBoundary: No such file or directory (os error 2)",
            "unicode \u{2713} and trailing space ",
            "",
        ] {
            let mut sink = Vec::new();
            write_line(&mut sink, text).unwrap();
            assert_eq!(sink, format!("{text}\n").into_bytes(), "{text:?}");
        }
    }

    #[test]
    fn levels_order_from_error_to_trace_and_parse() {
        assert!(Level::Error < Level::Warn && Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug && Level::Debug < Level::Trace);
        assert_eq!(Level::parse(" WARN "), Some(Level::Warn));
        assert_eq!(Level::parse("loud"), None);
        assert_eq!(Level::Debug.as_str(), "debug");
    }

    #[test]
    fn record_keeps_level_target_message_and_ordered_fields() {
        let value = record(
            Level::Warn,
            "boot",
            "stalled",
            &[("frame", "9"), ("thread", "main")],
        );
        assert_eq!(value["level"], "warn");
        assert_eq!(value["target"], "boot");
        assert_eq!(value["message"], "stalled");
        assert_eq!(value["fields"], json!([["frame", "9"], ["thread", "main"]]));
    }
}
