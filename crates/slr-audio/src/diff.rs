//! First-divergence comparison of two canonical streams (docs/audio.md).
//!
//! Equality is exact on `call`, `this`, `args`, `ret` and `out`. `meta` is
//! evidence and is skipped by design (decoded samples and exe call sites are
//! not part of the contract). There is no resynchronisation: a dropped or extra
//! call is the divergence, and a common prefix is never a pass.

use std::fmt;

use serde_json::Value;

use crate::stream::{Event, Stream};

#[derive(Clone, Debug, PartialEq)]
pub enum Divergence {
    /// Same ordinal, different content: `path` names the first differing field.
    Field {
        index: usize,
        path: String,
        expected: String,
        got: String,
    },
    /// One stream ended first.
    Length {
        common: usize,
        expected: usize,
        got: usize,
    },
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Divergence::Field { index, path, expected, got } => {
                write!(f, "DIVERGENCE at event #{index}:\n  field {path}: expected {expected}, got {got}")
            }
            Divergence::Length { common, expected, got } => write!(
                f,
                "DIVERGENCE after event #{}: length mismatch: expected {expected} events, got {got} events",
                common.saturating_sub(1)
            ),
        }
    }
}

/// Compare two streams; `None` means every event is identical and none is missing.
pub fn first_divergence(expected: &Stream, got: &Stream) -> Option<Divergence> {
    first_event_divergence(&expected.events, &got.events)
}

pub fn first_event_divergence(expected: &[Event], got: &[Event]) -> Option<Divergence> {
    for (index, (a, b)) in expected.iter().zip(got).enumerate() {
        if let Some((path, ev, gv)) = compare_event(a, b) {
            return Some(Divergence::Field {
                index,
                path,
                expected: ev,
                got: gv,
            });
        }
    }
    if expected.len() != got.len() {
        return Some(Divergence::Length {
            common: expected.len().min(got.len()),
            expected: expected.len(),
            got: got.len(),
        });
    }
    None
}

fn compare_event(a: &Event, b: &Event) -> Option<(String, String, String)> {
    if a.call != b.call {
        return Some(("call".into(), a.call.clone(), b.call.clone()));
    }
    if a.this != b.this {
        return Some(("this".into(), a.this.clone(), b.this.clone()));
    }
    if let Some(d) = value_diff("args", &a.args, &b.args) {
        return Some(d);
    }
    if a.ret != b.ret {
        return Some((
            "ret".into(),
            format!("{:#x}", a.ret),
            format!("{:#x}", b.ret),
        ));
    }
    value_diff("out", &a.out, &b.out)
}

fn value_diff(path: &str, a: &Value, b: &Value) -> Option<(String, String, String)> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = format!("{path}.{key}");
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = value_diff(&child, p, q) {
                            return Some(d);
                        }
                    }
                    (Some(p), None) => return Some((child, p.to_string(), "<absent>".into())),
                    (None, Some(q)) => return Some((child, "<absent>".into(), q.to_string())),
                    (None, None) => unreachable!(),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                if let Some(d) = value_diff(&format!("{path}[{i}]"), p, q) {
                    return Some(d);
                }
            }
            if x.len() != y.len() {
                return Some((
                    format!("{path}.len"),
                    x.len().to_string(),
                    y.len().to_string(),
                ));
            }
            None
        }
        _ => (a != b).then(|| (path.to_string(), a.to_string(), b.to_string())),
    }
}
