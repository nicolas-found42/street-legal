//! Differential test: TUFA string/`+` helpers vs the exe (issue #105, slice L3).
//!
//! Replays the Unicorn oracle vectors at `re/out/vmstr/vmstr_<suite>.jsonl`
//! (git-ignored; produced locally by
//! `re/oracle/vmstr_vectors.py gen --exe reference/raw/StreetLegal_Redline.bak.exe`,
//! one row per call of the real exe function on hand-built cells in emulator
//! memory — no JVM boot). Override the directory with `SLR_VMSTR_VECTORS`.
//! Skips with a note when the files are absent so `cargo test --workspace`
//! stays green without the exe/venv (CI, fresh worktrees) — same pattern as
//! `crates/slr-vm/tests/value_oracle.rs`.
//!
//! Suites:
//!
//! - `vmstr_add` (7 rows, `FUN_0041b4d0` sub-op `0x10` `+`, I/F paths): the
//!   exe runs at control word `0x007f` (PC24, [`Precision::F32`]); the port
//!   is [`Alu::new`] at that precision. Both result cells must match the
//!   recorded `(type, payload)` words bit-for-bit (`f32` sums go through
//!   x87 `fadd`, exactly like the exe's `fld/fadd/fstp`) and the run must log
//!   nothing.
//! - `vmstr_strcheck` (5 rows, `FUN_00417a70`, the `+` string gate): the
//!   harness calls the gate alone, so the recorded cells are the *untouched*
//!   operands (it returns early for both-non-String and, per the open concat
//!   leg, for both-String). The port's port of that gate is the host's
//!   string dispatch, [`Host::operator_on_refs`]: for every row it must
//!   report "not handled here" and leave both cells byte-identical, and for
//!   the non-String rows the plain helper that follows the gate must take
//!   the numeric path with no diagnostic. The one String/String row falls
//!   through to [`Alu::add`]'s reference arm, which logs exactly one
//!   `unsupported class type` (the documented divergence: the exe
//!   concatenates through the string pool, which needs booted String
//!   objects; see `re/oracle/vmstr_vectors.py` blockers).
//! - `vmstr_concat` (2 rows, `FUN_0041b4d0` String leg, null payloads): the
//!   exe's `FUN_004174a0(null) == 0` path keeps the `String` type and stores
//!   payload 0. The `bound` vs `heap` row only changes the type cell's
//!   address; the port's interned tags compare by spelling, so both rows
//!   must behave identically (left cell unchanged + one `unsupported class
//!   type` log).
//! - `vmstr_conv` (9 rows, `FUN_00417da0`): [`assign_convert`] must return
//!   `Ok` (the exe's `eax == 1`) and produce the recorded cell for I→F,
//!   truncating F→I, the `same`-target identity rows, and the null
//!   `String`→`Object` descriptor retarget. Rows 5 and 7 are
//!   **pointer-identity artifacts**: the harness hands the exe a second copy
//!   of the *same spelling*, so the exe compares pointers, returns 0 and
//!   leaves the cell; the port's tags are interned by spelling and compare
//!   equal, so it returns `Ok`. Those rows are skipped with a named note and
//!   never compared on `eax`.
//!
//! Write addresses are never compared: the vectors record payload words and
//! type spellings only.

use std::path::{Path, PathBuf};

use slr_vm::interp::{Host, RefOperator};
use slr_vm::ops::{Alu, Diagnostic, LogKind};
use slr_vm::values::{assign_convert, TypeTag, Value};
use slr_x87::Precision;

/// x87 control word the harness set for every row (PC bits 00 = 24-bit).
const CW: &str = "0x007f";
/// `Ljava.lang.Object;` descriptor (only row 8 of `conv` uses it).
const OBJ: &str = "Ljava.lang.Object;";
/// The `+` sub-op address the `add`/`concat` reference arms log under.
const ADD_ADDR: u32 = 0x0041_b4d0;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_VMSTR_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmstr"))
}

/// One suite's rows, or `None` when the file is absent (skip signal).
fn rows(dir: &Path, suite: &str) -> Option<Vec<serde_json::Value>> {
    let path = dir.join(format!("vmstr_{suite}.jsonl"));
    let text = std::fs::read_to_string(&path).ok()?;
    Some(
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
            .collect(),
    )
}

/// Type tag from the vector's recorded type spelling.
fn tag_of(name: &str) -> TypeTag {
    match name {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        "Ljava.lang.String;" => TypeTag::Ref(TypeTag::STRING),
        ok if ok == OBJ => TypeTag::Ref(OBJ),
        other => panic!("unhandled vector type spelling {other:?}"),
    }
}

/// The recorded `(type, payload)` cells, as `(tag, word)` pairs.
fn want_cells(row: &serde_json::Value) -> Vec<(TypeTag, u32)> {
    row["result"]["cells"]
        .as_array()
        .expect("result.cells")
        .iter()
        .map(|c| {
            (
                tag_of(c["type"].as_str().expect("cell type")),
                c["values"][0].as_u64().expect("cell word") as u32,
            )
        })
        .collect()
}

/// The row's `args` that carry a `vmval` (the `concat` rows also carry a
/// trailing `{"kind": ...}` marker), as `(type spelling, payload)`.
fn vmvals(row: &serde_json::Value) -> Vec<(String, u32)> {
    row["args"]
        .as_array()
        .expect("args")
        .iter()
        .filter_map(|a| {
            a.get("vmval").map(|v| {
                (
                    v["type"].as_str().expect("vmval type").to_string(),
                    v["values"][0].as_u64().expect("vmval word") as u32,
                )
            })
        })
        .collect()
}

fn value_of((name, word): &(String, u32)) -> Value {
    Value {
        tag: tag_of(name),
        payload: *word,
    }
}

/// `+` rows (`FUN_0041b4d0`): both result cells and no diagnostic.
fn replay_add(row: &serde_json::Value, ctx: &str) -> usize {
    assert_eq!(row["cw"].as_str(), Some(CW), "{ctx}: fpu control word");
    let ops = vmvals(row);
    assert_eq!(ops.len(), 2, "{ctx}: two operands");
    let (mut l, mut r) = (value_of(&ops[0]), value_of(&ops[1]));
    let mut alu = Alu::new(Precision::F32);
    alu.add(&mut l, &mut r)
        .expect("integer/float + cannot trap");
    assert!(
        alu.diagnostics.is_empty(),
        "{ctx}: unexpected diagnostics {:?}",
        alu.diagnostics
    );
    let want = want_cells(row);
    assert_eq!(want.len(), 2, "{ctx}: two result cells");
    assert_eq!((l.tag, l.payload), want[0], "{ctx}: left cell");
    assert_eq!((r.tag, r.payload), want[1], "{ctx}: right cell");
    1
}

/// Gate rows (`FUN_00417a70`): the port's host dispatch must not divert and
/// must leave both cells untouched, matching the recorded operands.
fn replay_strcheck(host: &mut slr_vm::host::VmHost, row: &serde_json::Value, ctx: &str) -> usize {
    let ops = vmvals(row);
    assert_eq!(ops.len(), 2, "{ctx}: two operands");
    let (mut l, mut r) = (value_of(&ops[0]), value_of(&ops[1]));
    let before = (l, r);
    let handled = host.operator_on_refs(RefOperator::Add, &mut l, &mut r);
    assert!(
        handled.is_none(),
        "{ctx}: gate is non-mutating for these operands"
    );
    assert_eq!((l, r), before, "{ctx}: gate left the cells untouched");

    let want = want_cells(row);
    assert_eq!(want.len(), 2, "{ctx}: two result cells");
    assert_eq!((l.tag, l.payload), want[0], "{ctx}: left cell");
    assert_eq!((r.tag, r.payload), want[1], "{ctx}: right cell");

    // The helper the exe calls when the gate returns early (numeric
    // operands) or, for the String/String row, the port's reference arm.
    let (mut l2, mut r2) = before;
    let mut alu = Alu::new(Precision::F32);
    alu.add(&mut l2, &mut r2).expect("no trap on this path");
    let strings = l.tag.name() == TypeTag::STRING && r.tag.name() == TypeTag::STRING;
    if strings {
        assert_eq!(
            alu.diagnostics,
            vec![Diagnostic::Logged {
                helper: ADD_ADDR,
                kind: LogKind::UnsupportedClassType,
                tag: b'L',
            }],
            "{ctx}: both-String falls through to one unsupported-class-type log"
        );
        assert_eq!((l2, r2), before, "{ctx}: both-String cells unchanged");
    } else {
        assert!(
            alu.diagnostics.is_empty(),
            "{ctx}: non-String operands must take the numeric path, got {:?}",
            alu.diagnostics
        );
    }
    1
}

/// Concatenation rows (`FUN_0041b4d0` String leg, null payloads).
fn replay_concat(row: &serde_json::Value, ctx: &str) -> usize {
    let ops = vmvals(row);
    assert_eq!(ops.len(), 2, "{ctx}: two operands");
    for (name, _) in &ops {
        assert_eq!(
            name, "Ljava.lang.String;",
            "{ctx}: concat rows are String/String"
        );
    }
    let (mut l, mut r) = (value_of(&ops[0]), value_of(&ops[1]));
    let mut alu = Alu::new(Precision::F32);
    alu.add(&mut l, &mut r)
        .expect("no trap on the reference path");
    assert_eq!(
        alu.diagnostics,
        vec![Diagnostic::Logged {
            helper: ADD_ADDR,
            kind: LogKind::UnsupportedClassType,
            tag: b'L',
        }],
        "{ctx}: concat is open in the port, logged exactly once"
    );
    let want = want_cells(row);
    assert_eq!(want.len(), 1, "{ctx}: the left cell is the result");
    assert_eq!((l.tag, l.payload), want[0], "{ctx}: left cell");
    1
}

/// Conversion rows (`FUN_00417da0`).
fn replay_conv(row: &serde_json::Value, ctx: &str) -> (usize, usize) {
    let ops = vmvals(row);
    assert_eq!(ops.len(), 1, "{ctx}: one source cell");
    let target = row["args"][1]["target"].as_str().expect("target");
    let want = want_cells(row);
    assert_eq!(want.len(), 1, "{ctx}: one result cell");
    let mut v = value_of(&ops[0]);

    if target != "same" && target == ops[0].0 {
        // Pointer-identity artifact: the harness gave the exe a same-spelling
        // but distinct type pointer, so `pcVar3 == pcVar1` is false and the
        // exe returns 0 without touching the cell. The port's interned tags
        // compare by spelling, so this is not replayable — do not compare.
        assert_eq!(row["result"]["eax"].as_u64(), Some(0), "{ctx}: exe eax");
        eprintln!(
            "vmstr_oracle: {ctx}: SKIP pointer-identity artifact \
             (target {target:?} is a second copy of the same spelling; \
             the exe compares pointers, the port compares interned names)"
        );
        return (0, 1);
    }

    assert_eq!(
        row["result"]["eax"].as_u64(),
        Some(1),
        "{ctx}: the exe reports success for the replayed rows"
    );
    let tgt = if target == "same" {
        v.tag
    } else {
        tag_of(target)
    };
    let before = (v.tag, v.payload);
    assign_convert(&mut v, tgt).expect("replayed rows convert");
    assert_eq!((v.tag, v.payload), want[0], "{ctx}: converted cell");
    if target == "same" {
        assert_eq!(
            (v.tag, v.payload),
            before,
            "{ctx}: identity leaves the cell"
        );
    }
    (1, 0)
}

#[test]
fn string_helpers_match_oracle_vectors() {
    let dir = vectors_dir();
    let mut suites: Vec<(&str, Vec<serde_json::Value>)> = Vec::new();
    for suite in ["add", "strcheck", "concat", "conv"] {
        match rows(&dir, suite) {
            Some(r) => suites.push((suite, r)),
            None => {
                eprintln!(
                    "vmstr_oracle: SKIP — {} absent; run \
                     `~/slr-oracle-venv/bin/python re/oracle/vmstr_vectors.py gen --exe \
                     /Users/nicolas/Documents/github/games/street-legal/reference/raw/StreetLegal_Redline.bak.exe`",
                    dir.join(format!("vmstr_{suite}.jsonl")).display()
                );
                return;
            }
        }
    }

    let mut host = slr_vm::host::VmHost::new();
    let (mut n_add, mut n_strcheck, mut n_concat, mut n_conv, mut n_skipped) = (0, 0, 0, 0, 0);

    for row in &suites[0].1 {
        let ctx = format!("vmstr_add[{}]", row["id"]);
        n_add += replay_add(row, &ctx);
    }
    for row in &suites[1].1 {
        let ctx = format!("vmstr_strcheck[{}]", row["id"]);
        n_strcheck += replay_strcheck(&mut host, row, &ctx);
    }
    for row in &suites[2].1 {
        let ctx = format!("vmstr_concat[{}]", row["id"]);
        n_concat += replay_concat(row, &ctx);
    }
    for row in &suites[3].1 {
        let ctx = format!("vmstr_conv[{}]", row["id"]);
        let (ok, skip) = replay_conv(row, &ctx);
        n_conv += ok;
        n_skipped += skip;
    }

    // The committed harness produces exactly this many rows; a mismatch means
    // the generator changed and the evidence line must be refreshed.
    assert_eq!(
        (n_add, n_strcheck, n_concat, n_conv, n_skipped),
        (7, 5, 2, 7, 2),
        "per-suite row counts changed in {}",
        dir.display()
    );
    eprintln!(
        "vmstr_oracle: {} rows replayed (add {n_add}, strcheck {n_strcheck}, concat {n_concat}, \
         conv {n_conv}, skipped {n_skipped} identity artifacts) from {}",
        n_add + n_strcheck + n_concat + n_conv + n_skipped,
        dir.display()
    );
}
