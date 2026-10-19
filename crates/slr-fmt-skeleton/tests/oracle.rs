//! Differential tests against the Unicorn function oracle
//! (`re/oracle/skel_vectors.py gen [--cw 0x27f]`, vectors in git-ignored
//! `re/out/skel/`).
//!
//! The rows hold the corpus file bytes (stock + modded) and synthetic edge
//! cases, run through the exe's own readers `FUN_00531e90` (skeleton text)
//! and `FUN_00532e70` (`.matrixdata`) in Unicorn. The scanner `FUN_00553890`
//! accumulates in x87 registers, so its output depends on the precision
//! control: vectors are recorded at the device-thread word `0x007F` (PC24, the
//! production model, ADR 0007) and at the worker-thread word `0x027F` (PC53);
//! the port is compared at the matching `Precision`.
//!
//! Covered exe functions: `FUN_00531e90` (skeleton reader), `FUN_00530cd0`
//! (parent-before-child thread), `FUN_00553890` (`%f`/`%d` scanner) and
//! `FUN_00532e70` (`.matrixdata` reader).

use serde_json::Value;
use slr_fmt_skeleton::{parse_animation, parse_skeleton_with};
use slr_x87::Precision;
use std::path::PathBuf;

fn vectors(kind: &str, suffix: &str) -> Option<Vec<Value>> {
    let dir = std::env::var_os("SLR_SKEL_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/skel"));
    let text = std::fs::read_to_string(dir.join(format!("skel_{kind}{suffix}.jsonl"))).ok()?;
    Some(
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
    )
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn u32s(v: &Value) -> Vec<u32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u32)
        .collect()
}

fn bits(f: &[f32]) -> Vec<u32> {
    f.iter().map(|x| x.to_bits()).collect()
}

/// Precision control of the row's recorded control word (bits 8-9).
fn precision(row: &Value) -> Precision {
    let cw = u32::from_str_radix(row["cw"].as_str().unwrap().trim_start_matches("0x"), 16).unwrap();
    match (cw >> 8) & 3 {
        0 => Precision::F32,
        2 => Precision::F64,
        _ => Precision::Ext,
    }
}

fn skeleton_diff(row: &Value) -> Result<(), String> {
    let pc = precision(row);
    let data = hex(row["input_hex"].as_str().unwrap());
    let res = &row["result"];
    let sk = parse_skeleton_with(&data, pc).map_err(|e| format!("port error: {e}"))?;
    let want = res["list"].as_array().unwrap();
    if sk.bones.len() as u64 != res["bones"].as_u64().unwrap() {
        return Err(format!("bone count {} vs {}", sk.bones.len(), res["bones"]));
    }
    if i64::from(sk.head) != res["head"].as_i64().unwrap() {
        return Err(format!("head {} vs {}", sk.head, res["head"]));
    }
    for (i, (b, w)) in sk.bones.iter().zip(want).enumerate() {
        let name_ok = b.name == w["name"].as_str().unwrap();
        let ok = name_ok
            && i64::from(b.parent) == w["parent"].as_i64().unwrap()
            && i64::from(b.next) == w["next"].as_i64().unwrap()
            && bits(&b.matrix) == u32s(&w["matrix"])
            && bits(&b.min) == u32s(&w["min"])
            && bits(&b.max) == u32s(&w["max"]);
        if !ok {
            return Err(format!("bone {i} ({:?}) differs from oracle {w}", b.name));
        }
    }
    Ok(())
}

#[test]
fn skeleton_reader_matches_oracle() {
    let mut ran = false;
    for suffix in ["", "_cw027f"] {
        let Some(rows) = vectors("skeleton", suffix) else {
            continue;
        };
        ran = true;
        assert!(!rows.is_empty());
        let (mut corpus, mut synth) = (0, 0);
        for row in &rows {
            if let Err(e) = skeleton_diff(row) {
                panic!("{} (cw {}): {e}", row["source"], row["cw"]);
            }
            if row["source"].as_str().unwrap().starts_with("synthetic:") {
                synth += 1;
            } else {
                corpus += 1;
            }
        }
        eprintln!(
            "skeleton oracle cw={}: rows={} corpus={corpus} synthetic={synth}",
            rows[0]["cw"],
            rows.len()
        );
    }
    if !ran {
        eprintln!("SLR_ORACLE_SKIPPED: skeleton_reader_matches_oracle");
        eprintln!("SKIP: no oracle vectors (run re/oracle/skel_vectors.py gen)");
    }
}

#[test]
fn animation_reader_matches_oracle() {
    let Some(rows) = vectors("matrixdata", "") else {
        eprintln!("SLR_ORACLE_SKIPPED: animation_reader_matches_oracle");
        eprintln!("SKIP: no oracle vectors (run re/oracle/skel_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    for row in &rows {
        let src = row["source"].as_str().unwrap();
        let data = hex(row["input_hex"].as_str().unwrap());
        let res = &row["result"];
        let a = parse_animation(&data).unwrap_or_else(|e| panic!("{src}: port error {e}"));
        assert_eq!(a.bone_count as u64, res["bones"].as_u64().unwrap(), "{src}");
        assert_eq!(a.key_count as u64, res["keys"].as_u64().unwrap(), "{src}");
        assert_eq!(bits(&a.times), u32s(&res["times"]), "{src}: times");
        let want_keys = res["key_list"].as_array().unwrap();
        assert_eq!(a.keys.len(), want_keys.len(), "{src}");
        for (i, (k, w)) in a.keys.iter().zip(want_keys).enumerate() {
            let mut got = bits(&k.rotation);
            got.extend(bits(&k.direction));
            assert_eq!(got, u32s(w), "{src}: key {i}");
        }
        let names: Vec<&str> = res["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(a.bone_names, names, "{src}: names");
    }
    eprintln!("animation oracle: rows={}", rows.len());
}

/// Guards the guard: the PC24 and PC53 oracle runs must actually differ, and
/// the port at the wrong precision must be caught, or a precision-blind test
/// would pass vacuously.
#[test]
fn precision_control_is_observable() {
    let (Some(a), Some(b)) = (vectors("skeleton", ""), vectors("skeleton", "_cw027f")) else {
        eprintln!("SLR_ORACLE_SKIPPED: precision_control_is_observable");
        eprintln!("SKIP: need both PC24 and PC53 vectors (skel_vectors.py gen; gen --cw 0x27f)");
        return;
    };
    assert_eq!(a.len(), b.len());
    let oracle_diff = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x["result"] != y["result"])
        .count();
    assert!(
        oracle_diff > 0,
        "oracle PC24 and PC53 results are identical"
    );
    // The port at the other precision disagrees with the recorded vectors.
    let mut wrong = 0;
    for row in &a {
        let mut r = row.clone();
        r["cw"] = Value::from("0x027f");
        if skeleton_diff(&r).is_err() {
            wrong += 1;
        }
    }
    assert!(
        wrong > 0,
        "port output does not depend on precision control"
    );
    eprintln!(
        "precision control observable: {oracle_diff} of {} rows differ PC24 vs PC53 in the oracle; \
         port at PC53 vs PC24 vectors: {wrong} rows differ",
        a.len()
    );
}
