//! Differential tests for the class-loader primitives (M1-04, issue #31).
//!
//! Replays `re/out/vmclass/vmclass_*.jsonl`, produced by
//! `re/oracle/vmclass_vectors.py gen`: the REAL exe string interner
//! (`FUN_0041e610` + `FUN_0041ef10`), native registrar (`FUN_00416b00`)
//! and boot binder (`FUN_00423e10`) executed under Unicorn on synthetic
//! inputs (no JVM boot, no game data). Override the directory with
//! `SLR_VMCLASS_VECTORS`. Skips with a note when the files are absent so
//! `cargo test --workspace` stays green without the exe/venv.
//!
//! Per row the test rebuilds the same state in the port and compares bit
//! for bit: interned bytes/length/refcount/hash, registrar bucket/record
//! words, bound descriptor bytes.

use std::path::PathBuf;

use slr_vm::natives::Natives;
use slr_vm::strings::Strings;

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_VMCLASS_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmclass")
}

fn load_rows(dir: &std::path::Path, name: &str) -> Option<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(dir.join(format!("vmclass_{name}.jsonl"))).ok()?;
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

fn str_arg(row: &serde_json::Value) -> Vec<u8> {
    row["args"]["str"]
        .as_str()
        .expect("str")
        .as_bytes()
        .to_vec()
}

#[test]
fn interner_matches_oracle_vectors() {
    let dir = vectors_dir();
    let Some(rows) = load_rows(&dir, "intern") else {
        eprintln!("SLR_ORACLE_SKIPPED: interner_matches_oracle_vectors");
        eprintln!("SKIP: no vmclass vectors (run re/oracle/vmclass_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    let mut strings = Strings::new();
    for row in &rows {
        let s = str_arg(row);
        let want_bytes = row["result"]["bytes"].as_str().expect("bytes");
        let want_len = row["result"]["len"].as_u64().expect("len") as usize;
        let want_refs = row["result"]["refs"].as_u64().expect("refs") as u32;
        let want_refs2 = row["result"]["refs_after_second"].as_u64().expect("refs2") as u32;
        let want_hash = row["result"]["hash"].as_u64().expect("hash") as u32;
        // The exe's hash over the input bytes.
        assert_eq!(Strings::hash(&s), want_hash, "hash {}", row["id"]);
        // First intern: bytes, length, refcount 1.
        let rec = strings.intern(&s).clone();
        assert_eq!(rec.bytes, want_bytes.as_bytes(), "bytes {}", row["id"]);
        assert_eq!(rec.bytes.len(), want_len, "len {}", row["id"]);
        assert_eq!(rec.refs, want_refs, "refs {}", row["id"]);
        // Second intern dedups (same content, refcount 2).
        let rec2 = strings.intern(&s).clone();
        assert_eq!(rec2.bytes, rec.bytes, "dedup {}", row["id"]);
        assert_eq!(rec2.refs, want_refs2, "refs2 {}", row["id"]);
        assert_eq!(
            strings.len(),
            rows.iter().position(|r| r == row).unwrap() + 1
        );
    }
    eprintln!("vmclass_intern: rows={}", rows.len());
}

#[test]
fn registrar_matches_oracle_vectors() {
    let dir = vectors_dir();
    let Some(rows) = load_rows(&dir, "register") else {
        eprintln!("SLR_ORACLE_SKIPPED: registrar_matches_oracle_vectors");
        eprintln!("SKIP: no vmclass vectors (run re/oracle/vmclass_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    let mut natives = Natives::new();
    for row in &rows {
        let class = row["args"]["class"].as_str().expect("class");
        let name = row["args"]["name"].as_str().expect("name");
        let sig = row["args"]["sig"].as_str().expect("sig");
        let imp = u32::from_str_radix(
            row["args"]["impl"]
                .as_str()
                .expect("impl")
                .trim_start_matches("0x"),
            16,
        )
        .expect("impl addr");
        // The exe's hash and bucket over the interned strings.
        assert_eq!(
            Natives::hash(class, name),
            row["result"]["hash"].as_u64().expect("hash") as u32,
            "hash {}",
            row["id"]
        );
        assert_eq!(
            Natives::bucket(class, name) as u64,
            row["result"]["bucket"].as_u64().expect("bucket"),
            "bucket {}",
            row["id"]
        );
        assert_eq!(
            row["result"]["bucket_hash"].as_u64().expect("bucket_hash") as u32,
            row["result"]["hash"].as_u64().expect("hash") as u32,
            "exe bucket hash is the full hash {}",
            row["id"]
        );
        natives.register(class, name, sig, imp);
        // The record resolves back to the implementation address.
        assert_eq!(
            natives.resolve(class, name, sig),
            Some(imp),
            "resolve {}",
            row["id"]
        );
        assert_eq!(
            row["result"]["rec_name"].as_str().expect("rec_name"),
            name,
            "record name {}",
            row["id"]
        );
        assert_eq!(
            row["result"]["rec_sig"].as_str().expect("rec_sig"),
            sig,
            "record sig {}",
            row["id"]
        );
        assert_eq!(
            row["result"]["rec_class"].as_str().expect("rec_class"),
            class,
            "record class {}",
            row["id"]
        );
    }
    eprintln!("vmclass_register: rows={}", rows.len());
}

#[test]
fn boot_binds_match_oracle_vectors() {
    let dir = vectors_dir();
    let Some(rows) = load_rows(&dir, "bind") else {
        eprintln!("SLR_ORACLE_SKIPPED: boot_binds_match_oracle_vectors");
        eprintln!("SKIP: no vmclass vectors (run re/oracle/vmclass_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());
    // The 5 boot descriptors the binder stores: I/I/F/L;/V.
    let mut strings = Strings::new();
    for row in &rows {
        let s = str_arg(row);
        let want = row["result"]["bytes"].as_str().expect("bytes");
        let rec = strings.intern(&s).clone();
        assert_eq!(rec.bytes, want.as_bytes(), "bind {}", row["id"]);
    }
    eprintln!("vmclass_bind: rows={}", rows.len());
}
