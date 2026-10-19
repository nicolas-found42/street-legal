//! Differential tests for the 27 TUFA operator/assignment helpers (M1-03,
//! issue #30).
//!
//! Reads the Unicorn oracle vectors at `re/out/vm/vm_<name>.jsonl`
//! (git-ignored; produced locally by `re/oracle/vm_vectors.py gen`, which
//! calls the real exe helpers on synthetic value cells in emulator memory,
//! no game data needed). Override the directory with `SLR_VM_VECTORS`.
//! Skips with a note when the files are absent so `cargo test --workspace`
//! stays green without the exe/venv (same pattern as `value_oracle.rs`).
//!
//! Per row the test builds the recorded operand cells, calls the ported
//! helper and compares the result cell `(type, payload word)` named by the
//! row's `dst` field bit for bit.

use std::path::PathBuf;

use slr_vm::ops::Alu;
use slr_vm::values::{TypeTag, Value};

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_VM_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vm")
}

fn tag_of(s: &str) -> TypeTag {
    match s {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        "L" => TypeTag::Ref("L;"),
        "[" => TypeTag::Array("["),
        "V" => TypeTag::Ref("V"),
        _ => panic!("unknown tag {s}"),
    }
}

fn cell(v: &serde_json::Value) -> Value {
    Value {
        tag: tag_of(v["type"].as_str().expect("type")),
        payload: v["values"][0].as_u64().expect("word") as u32,
    }
}

/// Helper name (vector file stem) -> (exe address, runner over the operand
/// cells). Operand order is the recorded one: binary helpers get
/// `(LEFT, RIGHT)`, shifts `(count, value)`, unary one cell, assignments
/// `(dst, src)` plus the kind word.
type Runner = fn(&mut Alu, &mut [Value], u32);

fn helpers() -> Vec<(&'static str, u32, Runner)> {
    vec![
        ("mul", 0x41b860, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.mul(&mut l[0], &mut r[0]).unwrap();
        }),
        ("add", 0x41b4d0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.add(&mut l[0], &mut r[0]).unwrap();
        }),
        ("sub", 0x41b760, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.sub(&mut l[0], &mut r[0]).unwrap();
        }),
        ("div", 0x41b970, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.div(&mut l[0], &mut r[0]).unwrap();
        }),
        ("mod", 0x41bbf0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.rem(&mut l[0], &mut r[0]).unwrap();
        }),
        ("bor", 0x41bfb0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.logical_or(&mut l[0], &mut r[0]);
        }),
        ("band", 0x41bea0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.logical_and(&mut l[0], &mut r[0]);
        }),
        ("bitor", 0x41c290, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.bit_or(&mut l[0], &mut r[0]);
        }),
        ("bitxor", 0x41c380, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.bit_xor(&mut l[0], &mut r[0]);
        }),
        ("bitand", 0x41c1a0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.bit_and(&mut l[0], &mut r[0]);
        }),
        ("shr", 0x41aff0, |a, c, _| {
            let (count, value) = c.split_at_mut(1);
            a.shr(&mut count[0], &mut value[0]);
        }),
        ("shl", 0x41b0e0, |a, c, _| {
            let (count, value) = c.split_at_mut(1);
            a.shl(&mut count[0], &mut value[0]);
        }),
        ("eq", 0x41c5c0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.eq(&mut l[0], &mut r[0]);
        }),
        ("ne", 0x41c6c0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.ne(&mut l[0], &mut r[0]);
        }),
        ("ge", 0x41c860, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.ge(&mut l[0], &mut r[0]);
        }),
        ("le", 0x41cb60, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.le(&mut l[0], &mut r[0]);
        }),
        ("gt", 0x41c790, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.gt(&mut l[0], &mut r[0]);
        }),
        ("lt", 0x41c9e0, |a, c, _| {
            let (l, r) = c.split_at_mut(1);
            a.lt(&mut l[0], &mut r[0]);
        }),
        ("neg", 0x41c0c0, |a, c, _| a.bit_not(&mut c[0])),
        ("lnot", 0x41bdb0, |a, c, _| a.logical_not(&mut c[0])),
        ("uminus", 0x41b1d0, |a, c, _| a.negate(&mut c[0])),
        ("inc", 0x41b2d0, |a, c, _| a.increment(&mut c[0])),
        ("dec", 0x41b3d0, |a, c, _| a.decrement(&mut c[0])),
        ("assign23", 0x41acb0, assign_runner),
        ("assign24", 0x41acb0, assign_runner),
        ("assign25", 0x41acb0, assign_runner),
        ("assign26", 0x41acb0, assign_runner),
        ("assign27", 0x41acb0, assign_runner),
        ("assign28", 0x41acb0, assign_runner),
        ("assign2c", 0x41acb0, assign_runner),
        ("assign2d", 0x41acb0, assign_runner),
        ("assign2e", 0x41acb0, assign_runner),
    ]
}

fn assign_runner(a: &mut Alu, c: &mut [Value], kind: u32) {
    let (d, s) = c.split_at_mut(1);
    // The vectors' destination is a plain value cell (not a variable slot).
    a.assign(&mut d[0], false, &mut s[0], kind).unwrap();
}

#[test]
fn operator_helpers_match_oracle_vectors() {
    let dir = vectors_dir();
    let mut total = 0usize;
    let mut files = 0usize;
    let mut detail = Vec::new();
    for (name, addr, run) in helpers() {
        let text = match std::fs::read_to_string(dir.join(format!("vm_{name}.jsonl"))) {
            Ok(t) => t,
            Err(_) => continue,
        };
        files += 1;
        let mut n = 0usize;
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let row: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{name} row {i}: {e}"));
            assert_eq!(
                row["exe_addr"].as_str().unwrap(),
                format!("0x{addr:08x}"),
                "{name}: exe_addr"
            );
            let mut cells: Vec<Value> = Vec::new();
            let mut kind = 0u32;
            for a in row["args"].as_array().unwrap() {
                if let Some(v) = a.get("vmval") {
                    cells.push(cell(v));
                } else {
                    kind = a["int"].as_u64().unwrap() as u32;
                }
            }
            let want = cell(&row["result"]);
            let dst = row["dst"].as_u64().unwrap() as usize;
            let mut alu = Alu::default();
            let before = cells.clone();
            run(&mut alu, &mut cells, kind);
            assert_eq!(
                cells[dst], want,
                "{name} row {i}: operands {before:?} kind {kind:#x}: got {}, oracle {}",
                cells[dst], want
            );
            n += 1;
        }
        assert!(n > 0, "{name}: no vector rows");
        total += n;
        detail.push(format!("{name}={n}"));
    }
    if files == 0 {
        eprintln!("SLR_ORACLE_SKIPPED: operator_helpers_match_oracle_vectors");
        eprintln!("SKIP: no oracle vectors (run re/oracle/vm_vectors.py gen)");
        return;
    }
    assert_eq!(files, 32, "expected 32 helper vector files, found {files}");
    eprintln!(
        "vm_oracle: rows={total} files={files} {} dir={}",
        detail.join(" "),
        dir.display()
    );
}
