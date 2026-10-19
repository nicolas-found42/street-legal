//! Differential test for the ported file-layer helpers (M1-09).
//!
//! Replays `re/out/io/filelayer.jsonl`, produced by
//! `re/oracle/filelayer_vectors.py gen`: the REAL exe helpers executed under
//! Unicorn with only the Win32 boundary hooked, so the exe's own path
//! normalisation and slot-allocation loop run. Override the directory with
//! `SLR_FILELAYER_VECTORS`. Skips with a note when the file is absent so
//! `cargo test --workspace` stays green without the exe/venv.
//!
//! # Oracle boundary
//!
//! `0x554860` (the path resolver) and the four Win32 thunks the slot table
//! calls (`0x559090` create/open, `0x558e40` close, `0x559150` size,
//! `0x558d80` read/write) are hooked; everything between them is the exe's
//! real code.
//!
//! # What is compared
//!
//! Every row runs against an *empty* slot table (the probe never opens a slot
//! before the row), so the slot-indexed rows pin the "no such slot" arm — the
//! arm the port must reproduce: `INVALID` (`-1`) for `size`/`seek`/`read`/
//! `write`, and no return for `close`. `open_read` opens into a real
//! temporary directory and must answer slot `1`.
//!
//! `open_write` faults in the probe (`0x558890`, the directory-creating
//! helper, is not modelled) and is skipped with that reason.

use serde_json::Value as Json;
use slr_io::fileio::FileLayer;

fn vectors_dir() -> std::path::PathBuf {
    std::env::var_os("SLR_FILELAYER_VECTORS").map_or_else(
        || {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/io")
                .to_path_buf()
        },
        std::path::PathBuf::from,
    )
}

fn load(name: &str) -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(vectors_dir().join(name)).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}")));
    }
    Some(rows)
}

fn param_at(row: &Json, index: usize) -> &Json {
    &row["args"]["params"][index]
}

fn word_at(row: &Json, index: usize) -> u32 {
    param_at(row, index)[1].as_u64().expect("word") as u32
}

/// A scratch directory each row runs in.
fn scratch() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("slr-filelayer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn the_file_layer_rows_replay() {
    let Some(rows) = load("filelayer.jsonl") else {
        eprintln!("SKIP: no re/out/io/filelayer.jsonl (run re/oracle/filelayer_vectors.py gen)");
        return;
    };
    let root = scratch();
    let mut checked = 0;
    let mut skipped = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let name = row["name"].as_str().expect("name");
        let want = &row["result"];
        if want["error"].as_str().is_some() {
            skipped += 1;
            continue;
        }
        let want_word = want["eax"].as_u64().map(|w| w as u32);

        let got = match name {
            "open_read" => {
                // The probe serves every CRT open from its buffer, so the
                // row pins the slot-allocation answer; the replay gives it a
                // real file to open.
                let path = param_at(row, 0)[1].as_str().expect("path");
                let relative = path.trim_start_matches('/');
                let host = root.join(relative);
                if let Some(parent) = host.parent() {
                    std::fs::create_dir_all(parent).expect("parent");
                }
                std::fs::write(&host, b"slr").expect("fixture");
                let mut layer = FileLayer::new(root.clone());
                Some(layer.open_read(path))
            }
            "close" => {
                let mut layer = FileLayer::new(root.clone());
                layer.close(word_at(row, 0));
                Some(0)
            }
            "size" => Some(FileLayer::new(root.clone()).size(word_at(row, 0))),
            "seek" => Some(FileLayer::new(root.clone()).seek(
                word_at(row, 0),
                word_at(row, 1) as i32,
                word_at(row, 2),
            )),
            "read" => {
                let mut buf = [0u8; 4];
                Some(FileLayer::new(root.clone()).read(word_at(row, 0), &mut buf))
            }
            "write" => Some(FileLayer::new(root.clone()).write(word_at(row, 0), &[0u8; 4])),
            "cmp_ci" => {
                let a = param_at(row, 0)[1].as_str().expect("a");
                let b = param_at(row, 1)[1].as_str().expect("b");
                let folded = word_at(row, 2) != 0;
                let ordering = if folded {
                    slr_io::findfile::cmp_ci(a, b)
                } else {
                    a.as_bytes().cmp(b.as_bytes())
                };
                Some(match ordering {
                    std::cmp::Ordering::Less => u32::MAX,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })
            }
            other => panic!("{id}: unhandled row {other}"),
        };

        assert_eq!(got, want_word, "{id}: result");
        checked += 1;
    }
    eprintln!("replayed {checked} rows, {skipped} skipped");
    assert!(
        checked >= 10,
        "replayed only {checked} rows ({skipped} skipped)"
    );
}
