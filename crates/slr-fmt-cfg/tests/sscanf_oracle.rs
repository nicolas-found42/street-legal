//! Differential test: `scan::sscanf` against the exe's own `sscanf` (FUN_00553890, wrapper
//! FUN_00551240), executed in Unicorn (`re/oracle/cfg_handler_vectors.py gen sscanf`).
//!
//! Oracle: **Unicorn function oracle**, control word 0x007F (x87 PC=24, the game's setting);
//! the port runs at `Precision::F32`. Every row compares the return value and the destination
//! cells (trailing 0xAA padding trimmed) bit for bit. Vectors live in git-ignored `re/out/cfg_handlers/`; the test
//! SKIPs when they are absent.

use serde_json::Value;
use slr_fmt_cfg::scan::{sscanf, ScanVal};
use slr_fmt_cfg::Precision;
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_CFG_HANDLER_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/cfg_handlers")
        })
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn cell(v: &Option<ScanVal>) -> Vec<u8> {
    let mut c = vec![0xaau8; 0x440];
    match v {
        None => {}
        Some(ScanVal::F32(f)) => c[..4].copy_from_slice(&f.to_bits().to_le_bytes()),
        Some(ScanVal::I32(i)) => c[..4].copy_from_slice(&i.to_le_bytes()),
        Some(ScanVal::Chr(b)) => c[0] = *b,
        Some(ScanVal::Str(s)) => {
            c[..s.len()].copy_from_slice(s);
            c[s.len()] = 0;
        }
    }
    while c.last() == Some(&0xaa) {
        c.pop();
    }
    c
}

#[test]
fn sscanf_matches_unicorn_vectors() {
    let path = vectors_dir().join("sscanf.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("SKIP: no oracle vectors (run re/oracle/cfg_handler_vectors.py gen sscanf)");
        return;
    };
    let rows: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(!rows.is_empty());
    let mut bad = Vec::new();
    for r in &rows {
        let fmt = r["fmt"].as_str().unwrap();
        let input = unhex(r["input"].as_str().unwrap());
        let want_ret = r["ret"].as_i64().unwrap() as i32;
        let want_cells: Vec<Vec<u8>> = r["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| unhex(c.as_str().unwrap()))
            .collect();
        let got = sscanf(&input, fmt.as_bytes(), Precision::F32);
        let got_cells: Vec<Vec<u8>> = got.vals.iter().map(cell).collect();
        if got.ret != want_ret || got_cells != want_cells {
            bad.push(format!(
                "{}: fmt {:?} input {:?}: ret {} want {}",
                r["id"],
                fmt,
                String::from_utf8_lossy(&input),
                got.ret,
                want_ret
            ));
        }
    }
    eprintln!(
        "sscanf_oracle: rows={} mismatches={}",
        rows.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}
