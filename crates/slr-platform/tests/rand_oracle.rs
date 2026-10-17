//! Oracle acceptance for the exact MSVC `rand` port (ticket M0-13).
//!
//! Reads the Unicorn oracle vectors at `re/out/oracle/rand_msvc.jsonl`
//! (git-ignored; produced locally by `re/oracle/rand_vectors.py`, which
//! calls the real exe `srand`/`_rand` per draw with a hooked `_getptd`).
//! Override with `SLR_RAND_VECTORS`. Skips with a note when the file is
//! absent so `cargo test --workspace` stays green without the venv/exe.

use std::path::PathBuf;

fn vectors_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_RAND_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/oracle/rand_msvc.jsonl")
}

#[test]
fn rand_matches_oracle_vectors() {
    let path = vectors_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SKIP: no oracle vectors (run re/oracle/rand_vectors.py)");
            return;
        }
    };
    let mut rows = 0usize;
    let mut draws = 0usize;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}"));
        let seed = v["seed"].as_u64().expect("row needs seed") as u32;
        let thread = v["thread"].as_u64().expect("row needs thread");
        let want: Vec<u16> = v["draws"]
            .as_array()
            .expect("row needs draws")
            .iter()
            .map(|d| d.as_u64().expect("draw") as u16)
            .collect();
        assert!(!want.is_empty(), "row {i}: no draws");
        let mut rng = slr_platform::MsvcRand::seeded(seed);
        for (n, w) in want.iter().enumerate() {
            let got = rng.next_u15();
            assert_eq!(got, *w, "seed {seed:#x} thread {thread} draw {n}");
        }
        rows += 1;
        draws += want.len();
    }
    assert!(rows > 0, "no vector rows in {}", path.display());
    eprintln!(
        "rand_oracle: rows={rows} draws={draws} file={}",
        path.display()
    );
}
