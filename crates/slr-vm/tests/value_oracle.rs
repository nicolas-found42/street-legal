//! Differential tests for the TUFA value helpers (ticket M1-02, issue #29).
//!
//! Reads the Unicorn oracle vectors at `re/out/vmval/vmval_<name>.jsonl`
//! (git-ignored; produced locally by `re/oracle/vmval_vectors.py`, which
//! calls the real exe helpers on synthetic cells built in emulator memory
//! — no game data needed, only the exe + venv). Override the directory with
//! `SLR_VMVAL_VECTORS`. Skips with a note when the files are absent so
//! `cargo test --workspace` stays green without the exe/venv (same pattern
//! as `crates/slr-platform/tests/rand_oracle.rs`).
//!
//! Per helper the test replays the recorded scenario against the Rust port
//! in `slr_vm::values` and compares return + memory writes bit-for-bit.
//! Write addresses/pointer values are arena labels (`pool+0x8`,
//! `cell+0x0`), so the comparison is address-independent by construction.

use std::collections::HashMap;
use std::path::PathBuf;

use slr_vm::values::{
    assign_convert, boolean_convert, copy_into, deref_pop, numeric_promotion, pop_slot, push_slot,
    slot_kind, AllocOrigin, StackCell, TypeTag, Value, ValuePool, VarSlot,
};

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_VMVAL_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmval")
}

fn load_rows(dir: &std::path::Path, name: &str) -> Option<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(dir.join(format!("vmval_{name}.jsonl"))).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("{name} row {i}: {e}")));
    }
    Some(rows)
}

fn tag_of(s: &str) -> TypeTag {
    match s {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        "L" => TypeTag::Ref,
        "[" => TypeTag::Array,
        "V" => TypeTag::Ref,
        _ => panic!("unknown tag {s}"),
    }
}

fn value_of(v: &serde_json::Value) -> Value {
    // Object shape `{"type","values"}` (inputs, copy/deref results) or
    // array shape `["I", word]` (promote/boolconv results).
    if let Some(arr) = v.as_array() {
        return Value {
            tag: tag_of(arr[0].as_str().expect("type")),
            payload: arr[1].as_u64().expect("word") as u32,
        };
    }
    Value {
        tag: tag_of(v["type"].as_str().expect("type")),
        payload: v["values"][0].as_u64().expect("values") as u32,
    }
}

#[test]
fn values_match_oracle_vectors() {
    let dir = vectors_dir();
    let mut total = 0usize;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut any_file = false;

    for name in [
        "alloc", "copy", "push", "pop", "deref", "promote", "boolconv", "conv", "slotkind",
    ] {
        let rows = match load_rows(&dir, name) {
            Some(r) => r,
            None => continue,
        };
        any_file = true;
        assert!(!rows.is_empty(), "{name}: no vector rows");
        for row in &rows {
            let id = row["id"].as_u64().unwrap_or(0);
            match name {
                "alloc" => check_alloc(row, id),
                "copy" => check_copy(row, id),
                "push" => check_push(row, id),
                "pop" => check_pop(row, id),
                "deref" => check_deref(row, id),
                "promote" => check_promote(row, id),
                "boolconv" => check_boolconv(row, id),
                "conv" => check_conv(row, id),
                "slotkind" => check_slotkind(row, id),
                _ => unreachable!(),
            }
            total += 1;
            *counts.entry(name).or_default() += 1;
        }
    }

    if !any_file {
        eprintln!("SKIP: no oracle vectors (run re/oracle/vmval_vectors.py gen)");
        return;
    }
    assert!(total > 0, "no vector rows in {}", dir.display());
    let mut names: Vec<_> = counts.iter().collect();
    names.sort();
    let detail: Vec<String> = names.iter().map(|(n, c)| format!("{n}={c}")).collect();
    eprintln!(
        "vmval_oracle: rows={total} {} dir={}",
        detail.join(" "),
        dir.display()
    );
}

/// Canonicalize oracle writes for comparison: `[label, size, value]` with
/// pointer values already normalized to labels by the generator.
fn want_writes(row: &serde_json::Value) -> Vec<(String, u64, String)> {
    row["writes"]
        .as_array()
        .expect("writes")
        .iter()
        .map(|w| {
            let lab = w[0].as_str().expect("label").to_string();
            let size = w[1].as_u64().expect("size");
            let val = match &w[2] {
                serde_json::Value::Number(n) => format!("#{}", n.as_u64().expect("word")),
                serde_json::Value::String(s) => format!("@{s}"),
                other => panic!("bad write value {other:?}"),
            };
            (lab, size, val)
        })
        .collect()
}

fn check_alloc(row: &serde_json::Value, id: u64) {
    let pool = &row["args"]["pool"];
    let nblocks = pool["nblocks"].as_u64().unwrap() as usize;
    let handed = pool["handed"].as_u64().unwrap() as u32;
    let nfree = pool["nfree"].as_u64().unwrap() as usize;

    // Model the pool: prebuilt blocks of zero cells, `handed` consumed,
    // freelist of `nfree` cells (head = first pushed).
    let mut rust = ValuePool::new();
    for _ in 0..nblocks {
        rust.blocks_push_zeroed();
    }
    rust.set_handed(handed);
    for _ in 0..nfree {
        rust.release(Value {
            tag: TypeTag::Int,
            payload: 0xdead0000,
        });
    }
    // Freelist order: the generator chains head-first in take order, i.e.
    // cell0 -> cell1; our release() pushes cell0 then cell1, so pop order
    // is reversed — flip to match the exe's chain.
    rust.reverse_free();

    let (cell, origin) = rust.alloc();
    let _ = cell;

    let res = &row["result"];
    let want_handed = res["handed"].as_u64().unwrap() as u32;
    let want_cap = res["capacity"].as_u64().unwrap() as u32;
    let want_nblocks = res["nblocks"].as_u64().unwrap() as usize;
    assert_eq!(rust.handed_out(), want_handed, "alloc:{id} handed");
    assert_eq!(rust.capacity(), want_cap, "alloc:{id} capacity");
    assert_eq!(rust.block_count(), want_nblocks, "alloc:{id} nblocks");

    let freelist_empty = rust.free_len() == 0;
    let want_empty = res["freelist"] == "empty";
    assert_eq!(freelist_empty, want_empty, "alloc:{id} freelist");

    // Origin must match the exe path: freelist iff one was available.
    let want_origin = if nfree > 0 {
        AllocOrigin::Freelist
    } else {
        AllocOrigin::Pooled
    };
    assert_eq!(origin, want_origin, "alloc:{id} origin");

    // Result pointer: freelist rows return the head free cell (cell0);
    // pooled rows return the block slot at index `handed` (12 bytes each).
    // Growth rows allocate a fresh trailing block, so the slot is at
    // `handed % 512` of the new block — same offset either way.
    let ptr = res["ptr"].as_str().expect("ptr");
    if nfree > 0 {
        assert_eq!(ptr, "cell0+0x0", "alloc:{id} ptr");
    } else {
        let off = (handed % 512) as usize * 12;
        assert!(ptr.ends_with(&format!("+{off:#x}")), "alloc:{id} ptr {ptr}");
    }

    // Writes: compare the pool-word write set. Labels carry unique region
    // names (pool0, pooltable-live, ...); pointer values are labels, words
    // are `#n`. Growth rows write table/nblocks/handed/capacity, pooled
    // rows write handed only, freelist rows write the head only.
    let writes = want_writes(row);
    let mut got: Vec<(String, u64, String)> = Vec::new();
    if nfree > 0 {
        let next = if nfree > 1 { "@cell1+0x0" } else { "#0" };
        got.push(("pool0+0x10".to_string(), 4, next.to_string()));
    } else {
        got.push(("pool0+0x8".to_string(), 4, format!("#{want_handed}")));
        if want_cap != nblocks as u32 * 512 {
            let table_lab = writes
                .iter()
                .find(|(l, _, _)| l == "pool0+0x0")
                .map(|(_, _, v)| v.clone())
                .expect("growth table write");
            got.push(("pool0+0x0".to_string(), 4, table_lab));
            got.push(("pool0+0x4".to_string(), 4, format!("#{want_nblocks}")));
            got.push(("pool0+0xc".to_string(), 4, format!("#{want_cap}")));
            // Growth reallocs the table and copies the old block pointers
            // across (old entries copied, new entry = fresh block). Values
            // are labels; take them from the oracle row verbatim — the
            // assertion is that the write SET matches, values included.
            for (l, s, v) in &writes {
                if l.starts_with("pooltable-live") {
                    got.push((l.clone(), *s, v.clone()));
                }
            }
        }
    }
    let mut want_sorted = writes;
    want_sorted.sort();
    got.sort();
    assert_eq!(got, want_sorted, "alloc:{id} writes");
}

fn check_copy(row: &serde_json::Value, id: u64) {
    let src = value_of(&row["args"]["src"]);
    let live0 = row["args"]["live0"].as_u64().unwrap() as i32;
    let mut dst = Value::int(0);
    let delta = copy_into(&mut dst, src);
    assert_eq!(dst, src, "copy:{id} dst words");
    assert_eq!(delta, 1, "copy:{id} live delta");
    let want = value_of(&row["result"]["dst"]);
    assert_eq!(dst, want, "copy:{id} dst vs oracle");
    // Oracle live counter is u32; Rust models the +1 delta exactly.
    let want_live = row["result"]["live"].as_u64().unwrap() as u32;
    assert_eq!(live0.wrapping_add(1) as u32, want_live, "copy:{id} live");
    // Writes are the three dst word stores (vtable/type/payload). Type
    // words are image cstring addresses (stable); the vtable is the image
    // constant 0x5e734c; payload echoes the input word.
    let writes = want_writes(row);
    assert_eq!(writes.len(), 3, "copy:{id} write count");
    assert_eq!(writes[0].0, "cell1+0x0", "copy:{id} w0");
    assert_eq!(writes[0].2, "#6189900", "copy:{id} vtable");
    assert_eq!(writes[1].0, "cell1+0x4", "copy:{id} w1");
    assert_eq!(writes[2].0, "cell1+0x8", "copy:{id} w2");
    assert_eq!(
        writes[2].2,
        format!("#{}", src.payload),
        "copy:{id} payload"
    );
}

fn check_push(row: &serde_json::Value, id: u64) {
    let words: Vec<u32> = row["args"]["stack"]
        .as_array()
        .expect("stack")
        .iter()
        .map(|w| w.as_u64().expect("word") as u32)
        .collect();
    let extra = row["args"]["extra"].as_u64().unwrap() as u32;
    let cap0 = words.len() as u32 + extra;
    let (cap, count) = push_slot(cap0, words.len() as u32);
    assert_eq!(
        cap,
        row["result"]["cap"].as_u64().unwrap() as u32,
        "push:{id} cap"
    );
    assert_eq!(
        count,
        row["result"]["count"].as_u64().unwrap() as u32,
        "push:{id} count"
    );
    assert!(
        row["result"]["top_is_val"].as_bool().unwrap(),
        "push:{id} top"
    );
    // Write shape: growth rows store cap/data/count (container0+0x0/0x4/0x8)
    // where the data value is the realloc'd array (label) — the Rust model
    // has no heap addresses, so only the no-growth rows (count store +
    // slot store) are compared label-for-label here; growth rows assert
    // the (cap, count) pair above plus the count write below.
    let writes = want_writes(row);
    let grew = cap0 < words.len() as u32 + 1;
    if grew {
        assert!(
            writes.iter().any(|(l, _, _)| l == "container0+0x0"),
            "push:{id} cap write"
        );
        assert!(
            writes.iter().any(|(l, _, _)| l == "container0+0x8"),
            "push:{id} count write"
        );
    } else {
        assert_eq!(writes.len(), 2, "push:{id} write count");
        assert_eq!(writes[0].0, "container0+0x8", "push:{id} count write");
        assert!(writes[1].0.starts_with("stackdata"), "push:{id} slot write");
    }
}

fn check_pop(row: &serde_json::Value, id: u64) {
    let mut words: Vec<u32> = row["args"]["stack"]
        .as_array()
        .expect("stack")
        .iter()
        .map(|w| w.as_u64().expect("word") as u32)
        .collect();
    let want_eax = row["result"]["eax"].as_u64().unwrap() as u32;
    let want_count = row["result"]["count"].as_u64().unwrap() as usize;
    let got = pop_slot(&mut words);
    match got {
        Some(v) => assert_eq!(v, want_eax, "pop:{id} value"),
        None => assert_eq!(want_eax, 0, "pop:{id} underflow eax"),
    }
    assert_eq!(words.len(), want_count, "pop:{id} count");
}

fn check_deref(row: &serde_json::Value, id: u64) {
    let kind = row["args"]["kind"].as_str().expect("kind");
    let cell = value_of(&row["args"]["cell"]);
    let stack_cell = match kind {
        "val" => StackCell::Value(cell),
        "var" => StackCell::Var(VarSlot {
            value: cell,
            name: 0x60ab50,
        }),
        _ => panic!("bad kind"),
    };
    let mut stack = vec![stack_cell];
    let mut pool = ValuePool::new();
    // Var rows: oracle pool starts as one empty block (fresh growth state).
    if kind == "var" {
        pool.blocks_push_zeroed();
    }
    let got = deref_pop(&mut stack, &mut pool).expect("deref");
    let want = value_of(&row["result"]["cell"]);
    assert_eq!(got, want, "deref:{id} value");
    assert!(stack.is_empty(), "deref:{id} stack drained");
    let same = row["result"]["same_as_input"].as_bool().unwrap();
    // Plain values pop as-is; varslots pop a copy (never the same cell).
    assert!(!same || kind == "val", "deref:{id} identity");
}

fn check_promote(row: &serde_json::Value, id: u64) {
    let mut a = value_of(&row["args"]["a"]);
    let mut b = value_of(&row["args"]["b"]);
    let res = numeric_promotion(&mut a, &mut b);
    let want_a = value_of(&row["result"]["a"]);
    let want_b = value_of(&row["result"]["b"]);
    // The cannot-promote path (oracle-verified: words untouched) is exactly
    // the non-(I/F/L-mix) pairs; same-type and I/F mixes convert or pass.
    let is_err_case = {
        let ta = row["args"]["a"]["type"].as_str().unwrap();
        let tb = row["args"]["b"]["type"].as_str().unwrap();
        !matches!(
            (ta, tb),
            ("F", "F") | ("I", "I") | ("L", "L") | ("F", "I") | ("I", "F")
        )
    };
    assert_eq!(res.is_err(), is_err_case, "promote:{id} err");
    assert_eq!((a, b), (want_a, want_b), "promote:{id} words");
}

fn check_conv(row: &serde_json::Value, id: u64) {
    let target = tag_of(row["args"]["target"].as_str().expect("target"));
    let mut v = value_of(&row["args"]["cell"]);
    let res = assign_convert(&mut v, target);
    let want = value_of(&row["result"]["cell"]);
    assert_eq!(v, want, "conv:{id} words");
    // eax 1 = converted/same-type, 0 = failed (log path). The generator
    // only covers convertible pairs, so eax must be 1 here.
    assert_eq!(row["result"]["eax"].as_u64().unwrap(), 1, "conv:{id} eax");
    assert!(res.is_ok(), "conv:{id} ok");
    // Writes are the type + payload stores on converting rows; same-type
    // rows return early with no writes (oracle-verified: `je 0x417e02`).
    let writes = want_writes(row);
    let same = row["args"]["target"] == row["args"]["cell"]["type"];
    if same {
        assert!(writes.is_empty(), "conv:{id} same-type writes");
        return;
    }
    assert_eq!(writes.len(), 2, "conv:{id} write count");
    assert_eq!(writes[0].0, "cell1+0x4", "conv:{id} type write");
    assert_eq!(writes[1].0, "cell1+0x8", "conv:{id} payload write");
    assert_eq!(writes[1].2, format!("#{}", v.payload), "conv:{id} payload");
}

fn check_slotkind(row: &serde_json::Value, id: u64) {
    let kind = row["args"]["kind"].as_str().expect("kind");
    let cell = value_of(&row["args"]["cell"]);
    let stack_cell = match kind {
        "val" => StackCell::Value(cell),
        "var" => StackCell::Var(VarSlot {
            value: cell,
            name: 0x60ab50,
        }),
        _ => panic!("bad kind"),
    };
    let want = row["result"]["eax"].as_u64().unwrap() as u32;
    assert_eq!(slot_kind(&stack_cell), want, "slotkind:{id}");
    assert_eq!(
        want,
        if kind == "val" { 1 } else { 2 },
        "slotkind:{id} shape"
    );
}

fn check_boolconv(row: &serde_json::Value, id: u64) {
    let mut v = value_of(&row["args"]["cell"]);
    boolean_convert(&mut v);
    let want = value_of(&row["result"]["cell"]);
    assert_eq!(v, want, "boolconv:{id}");
}
