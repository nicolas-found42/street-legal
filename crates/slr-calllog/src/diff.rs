//! First-divergence diff: walk two logs in lockstep and report precisely.
//!
//! The comparison is strict and positional: record `N` of the left log is
//! compared with record `N` of the right log, field by field, and the walk
//! stops at the first difference. There is no resynchronization heuristic —
//! a dropped or reordered record IS the divergence and is reported at its
//! ordinal. Length and count mismatches are reported cleanly once the common
//! prefix compares equal.

use crate::model::{LogRecord, Value};
use std::fmt;

/// One scalar field difference: dotted path, both sides rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDiff {
    /// Dotted path from the record, e.g. `args[1]` or `pos[0]`.
    pub path: String,
    /// Left (`expected`) side, rendered.
    pub expected: String,
    /// Right (`got`) side, rendered.
    pub got: String,
}

impl FieldDiff {
    fn new(path: impl Into<String>, expected: String, got: String) -> Self {
        Self {
            path: path.into(),
            expected,
            got,
        }
    }
}

/// The first divergence between two logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Divergence {
    /// Same ordinal, different record kind on each side.
    KindMismatch {
        ordinal: u64,
        expected: &'static str,
        got: &'static str,
    },
    /// Same ordinal and kind, but at least one field differs. Only the
    /// first differing field is reported; `fields[0]` is always present.
    FieldMismatch {
        ordinal: u64,
        kind: &'static str,
        fields: Vec<FieldDiff>,
    },
    /// Common prefix is equal, but one log holds more records.
    LengthMismatch {
        /// Records compared before one side ran out.
        common: u64,
        expected_len: Option<u64>,
        got_len: Option<u64>,
    },
}

/// The outcome of [`first_divergence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOutcome {
    /// Logs are identical record-for-record.
    Ok { records: u64 },
    /// Logs differ; the details of the first divergence.
    Diverged(Divergence),
}

impl fmt::Display for DiffOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffOutcome::Ok { records } => write!(f, "OK: {records} records identical"),
            DiffOutcome::Diverged(d) => write!(f, "{d}"),
        }
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Divergence::KindMismatch {
                ordinal,
                expected,
                got,
            } => write!(
                f,
                "DIVERGENCE at record #{ordinal}: kind mismatch: expected {expected}, got {got}"
            ),
            Divergence::FieldMismatch {
                ordinal,
                kind,
                fields,
            } => {
                write!(f, "DIVERGENCE at record #{ordinal} ({kind}):")?;
                for fd in fields {
                    write!(
                        f,
                        "\n  field {}: expected {}, got {}",
                        fd.path, fd.expected, fd.got
                    )?;
                }
                Ok(())
            }
            Divergence::LengthMismatch {
                common,
                expected_len,
                got_len,
            } => {
                let exp = expected_len.map_or("?".to_owned(), |n| n.to_string());
                let got = got_len.map_or("?".to_owned(), |n| n.to_string());
                write!(
                    f,
                    "DIVERGENCE after record #{common}: length mismatch: expected {exp} records, got {got} records"
                )
            }
        }
    }
}

/// Render one value the way diff reports show it.
///
/// Floats print as hex bit patterns (`f32:0x3f800000`), never as decimals:
/// two floats that print the same decimally can still differ in the last
/// ulp, and this tool exists to catch exactly that.
#[must_use]
pub fn render_value(v: &Value) -> String {
    match v {
        Value::Int(x) => format!("int:{x}"),
        Value::Long(x) => format!("long:{x}"),
        Value::FloatBits(b) => format!("f32:{b:#010x}"),
        Value::DoubleBits(b) => format!("f64:{b:#018x}"),
        Value::Str(s) => format!("str:{s:?}"),
        Value::Bytes(b) => format!("bytes:{}", hexpreview(b)),
        Value::Bool(b) => format!("bool:{b}"),
    }
}

fn hexpreview(b: &[u8]) -> String {
    const SHOW: usize = 32;
    let mut s = format!("len={} ", b.len());
    s.push_str(
        &b.iter()
            .take(SHOW)
            .map(|x| format!("{x:02x}"))
            .collect::<String>(),
    );
    if b.len() > SHOW {
        s.push_str("...");
    }
    s
}

fn diff_values(path: &str, e: &Value, g: &Value) -> Option<FieldDiff> {
    if std::mem::discriminant(e) != std::mem::discriminant(g) {
        return Some(FieldDiff::new(path, render_value(e), render_value(g)));
    }
    if e != g {
        return Some(FieldDiff::new(path, render_value(e), render_value(g)));
    }
    None
}

fn diff_value_lists(base: &str, e: &[Value], g: &[Value]) -> Option<FieldDiff> {
    if e.len() != g.len() {
        return Some(FieldDiff::new(
            format!("{base}.len"),
            e.len().to_string(),
            g.len().to_string(),
        ));
    }
    for (i, (a, b)) in e.iter().zip(g.iter()).enumerate() {
        if let Some(d) = diff_values(&format!("{base}[{i}]"), a, b) {
            return Some(d);
        }
    }
    None
}

fn diff_f32word(path: String, e: u32, g: u32) -> Option<FieldDiff> {
    if e != g {
        Some(FieldDiff::new(
            path,
            format!("f32:{e:#010x}"),
            format!("f32:{g:#010x}"),
        ))
    } else {
        None
    }
}

/// Compare two records at one ordinal.
///
/// Returns `None` when they are identical, otherwise the first divergence.
/// Records of different kinds diverge as [`Divergence::KindMismatch`];
/// same-kind records with any field difference diverge as
/// [`Divergence::FieldMismatch`] carrying exactly one [`FieldDiff`].
#[must_use]
pub fn diff_records(ordinal: u64, expected: &LogRecord, got: &LogRecord) -> Option<Divergence> {
    if std::mem::discriminant(expected) != std::mem::discriminant(got) {
        return Some(Divergence::KindMismatch {
            ordinal,
            expected: expected.kind_name(),
            got: got.kind_name(),
        });
    }
    let kind = expected.kind_name();
    let field = match (expected, got) {
        (
            LogRecord::NativeCall {
                class: ec,
                name: en,
                args: ea,
                result: er,
            },
            LogRecord::NativeCall {
                class: gc,
                name: gn,
                args: ga,
                result: gr,
            },
        ) => {
            if ec != gc {
                Some(FieldDiff::new(
                    "class",
                    format!("{ec:?}"),
                    format!("{gc:?}"),
                ))
            } else if en != gn {
                Some(FieldDiff::new("name", format!("{en:?}"), format!("{gn:?}")))
            } else if let Some(d) = diff_value_lists("args", ea, ga) {
                Some(d)
            } else {
                match (er, gr) {
                    (None, None) => None,
                    (Some(_), None) => Some(FieldDiff::new(
                        "result",
                        render_value(er.as_ref().unwrap()),
                        "void".to_owned(),
                    )),
                    (None, Some(_)) => Some(FieldDiff::new(
                        "result",
                        "void".to_owned(),
                        render_value(gr.as_ref().unwrap()),
                    )),
                    (Some(a), Some(b)) => diff_values("result", a, b),
                }
            }
        }
        (
            LogRecord::D3d9Call {
                kind: ek,
                method: em,
                args: ea,
            },
            LogRecord::D3d9Call {
                kind: gk,
                method: gm,
                args: ga,
            },
        ) => {
            if ek != gk {
                Some(FieldDiff::new("kind", format!("{ek:?}"), format!("{gk:?}")))
            } else if em != gm {
                Some(FieldDiff::new(
                    "method",
                    format!("{em:?}"),
                    format!("{gm:?}"),
                ))
            } else {
                diff_value_lists("args", ea, ga)
            }
        }
        (
            LogRecord::DirectSoundCall {
                kind: ek,
                method: em,
                args: ea,
            },
            LogRecord::DirectSoundCall {
                kind: gk,
                method: gm,
                args: ga,
            },
        ) => {
            if ek != gk {
                Some(FieldDiff::new("kind", format!("{ek:?}"), format!("{gk:?}")))
            } else if em != gm {
                Some(FieldDiff::new(
                    "method",
                    format!("{em:?}"),
                    format!("{gm:?}"),
                ))
            } else {
                diff_value_lists("args", ea, ga)
            }
        }
        (
            LogRecord::DirectInput {
                kind: ek,
                report: er,
            },
            LogRecord::DirectInput {
                kind: gk,
                report: gr,
            },
        ) => {
            if ek != gk {
                Some(FieldDiff::new("kind", format!("{ek:?}"), format!("{gk:?}")))
            } else if er != gr {
                Some(FieldDiff::new("report", hexpreview(er), hexpreview(gr)))
            } else {
                None
            }
        }
        (
            LogRecord::FileIo {
                op: eo,
                path: ep,
                detail: ed,
            },
            LogRecord::FileIo {
                op: go,
                path: gp,
                detail: gd,
            },
        ) => {
            if eo != go {
                Some(FieldDiff::new("op", format!("{eo:?}"), format!("{go:?}")))
            } else if ep != gp {
                Some(FieldDiff::new("path", format!("{ep:?}"), format!("{gp:?}")))
            } else {
                diff_value_lists("detail", ed, gd)
            }
        }
        (LogRecord::VmSnapshot(e), LogRecord::VmSnapshot(g)) => {
            if e.frame != g.frame {
                Some(FieldDiff::new(
                    "frame",
                    e.frame.to_string(),
                    g.frame.to_string(),
                ))
            } else if e.pc != g.pc {
                Some(FieldDiff::new(
                    "pc",
                    format!("{:#010x}", e.pc),
                    format!("{:#010x}", g.pc),
                ))
            } else if e.control_word != g.control_word {
                Some(FieldDiff::new(
                    "control_word",
                    format!("{:#010x}", e.control_word),
                    format!("{:#010x}", g.control_word),
                ))
            } else {
                diff_value_lists("stack", &e.stack, &g.stack)
            }
        }
        (LogRecord::FrameVehicle(e), LogRecord::FrameVehicle(g)) => {
            if e.frame != g.frame {
                Some(FieldDiff::new(
                    "frame",
                    e.frame.to_string(),
                    g.frame.to_string(),
                ))
            } else if let Some(d) = ["pos", "ypr", "vel"]
                .iter()
                .zip([e.pos, e.ypr, e.vel].iter().zip([g.pos, g.ypr, g.vel]))
                .find_map(|(base, (a, b))| {
                    a.iter()
                        .zip(b.iter())
                        .enumerate()
                        .find_map(|(i, (x, y))| diff_f32word(format!("{base}[{i}]"), *x, *y))
                })
            {
                Some(d)
            } else if e.rpm_bits != g.rpm_bits {
                diff_f32word("rpm".to_owned(), e.rpm_bits, g.rpm_bits)
            } else if e.gear != g.gear {
                Some(FieldDiff::new(
                    "gear",
                    e.gear.to_string(),
                    g.gear.to_string(),
                ))
            } else {
                None
            }
        }
        // Same discriminant pairs are all covered above; different-kind
        // pairs returned early. This arm is unreachable by construction.
        _ => None,
    };
    field.map(|f| Divergence::FieldMismatch {
        ordinal,
        kind,
        fields: vec![f],
    })
}

/// Walk two record slices in lockstep and report the first divergence.
///
/// `expected` is the oracle side, `got` the port side. A common equal
/// prefix followed by exhaustion on one side is
/// [`Divergence::LengthMismatch`] with the known lengths filled in.
#[must_use]
pub fn first_divergence(expected: &[LogRecord], got: &[LogRecord]) -> DiffOutcome {
    let common = expected.len().min(got.len());
    for (i, (e, g)) in expected.iter().zip(got.iter()).enumerate() {
        if let Some(d) = diff_records(i as u64, e, g) {
            return DiffOutcome::Diverged(d);
        }
    }
    if expected.len() == got.len() {
        DiffOutcome::Ok {
            records: common as u64,
        }
    } else {
        DiffOutcome::Diverged(Divergence::LengthMismatch {
            common: common as u64,
            expected_len: Some(expected.len() as u64),
            got_len: Some(got.len() as u64),
        })
    }
}
