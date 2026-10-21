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

// ---------------------------------------------------------------------
// `open_write` (0x54caf0) + the raw find layer (0x54cf50/0x54d1c0/0x54d240).
//
// Replays `re/out/boot/filelayer2.jsonl`, produced by
// `re/oracle/boot_filelayer2_vectors.py gen`: the same harness as
// `the_file_layer_rows_replay` (Win32/CRT boundary hooked, exe slot/find
// table code real), extended with a queue-backed synthetic find directory
// and a failing `CreateFileA` arm so the create helper's
// `GetLastError`/`CreateDirectoryA` loop runs. The exe's CRT handle word is
// the declared shim boundary and is not compared; the returned slot, the
// slot words (name/id/in-use/mode), the find slot records and the
// create/mkdir event sequence are.

fn boot_dir() -> std::path::PathBuf {
    std::env::var_os("SLR_BOOT_VECTORS").map_or_else(
        || {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/boot")
                .to_path_buf()
        },
        std::path::PathBuf::from,
    )
}

fn boot_rows() -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(boot_dir().join("filelayer2.jsonl")).ok()?;
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

fn scratch2(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("slr-boot2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The exe's slot words as `(name, id, in_use, mode)`.
fn want_slot(row: &Json, index: usize) -> (String, u32, u8, u8) {
    let s = &row["result"]["slots"][index];
    (
        s["name"].as_str().expect("name").to_string(),
        s["id"].as_u64().expect("id") as u32,
        s["in_use"].as_u64().expect("in_use") as u8,
        s["mode"].as_u64().expect("mode") as u8,
    )
}

fn got_slot(layer: &FileLayer, slot: u32) -> (String, u32, u8, u8) {
    let e = layer.entry(slot).expect("slot");
    (
        String::from_utf8_lossy(&e.name)
            .trim_end_matches('\0')
            .to_string(),
        e.id,
        u8::from(e.in_use),
        e.mode,
    )
}

/// Create the row's synthetic find directory under `root`: one `dir`
/// segment per pattern component plus every entry (a directory when its
/// attr bit is 1).
fn plant_entries(root: &std::path::Path, entries: &[(String, u32)]) {
    for (name, attr) in entries {
        let host = root.join(name);
        if attr & 0x10 != 0 {
            std::fs::create_dir_all(&host).expect("entry dir");
        } else {
            if let Some(parent) = host.parent() {
                std::fs::create_dir_all(parent).expect("entry parent");
            }
            std::fs::write(&host, b"").expect("entry file");
        }
    }
}

fn row_entries(row: &Json) -> Vec<(String, u32)> {
    row["args"]["world"]["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    (
                        e[0].as_str().expect("entry name").to_string(),
                        e[1].as_u64().expect("attr") as u32,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The exe's delivered record as `Some((name, attr))` / `None`. `find_advance`
/// records the whole delivery sequence in `record0`, so its first element is
/// the record `open_search` returned.
fn record0(row: &Json, key: &str) -> Option<(String, u32)> {
    let v = &row["result"][key];
    let v = if v.is_array() { &v[0] } else { v };
    if v.is_null() {
        return None;
    }
    Some((
        v["name"].as_str().expect("record name").to_string(),
        v["attr"].as_u64().expect("record attr") as u32,
    ))
}

/// The port's record 0 for a live handle: `(name, attr bit)`.
fn port_record(host: &mut slr_io::findfile::FindHost, handle: i32) -> Option<(String, u32)> {
    let slot = host.slot(handle)?;
    let entry = slot.entries.first()?;
    Some((entry.name.clone(), u32::from(entry.is_dir)))
}

#[test]
fn the_open_write_and_findfile_rows_replay() {
    let Some(rows) = boot_rows() else {
        eprintln!("SLR_ORACLE_SKIPPED: the_open_write_and_findfile_rows_replay");
        eprintln!(
            "SKIP: no re/out/boot/filelayer2.jsonl (run re/oracle/boot_filelayer2_vectors.py gen)"
        );
        return;
    };
    assert!(!rows.is_empty());
    let mut checked = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id").to_string();
        // Every row observed the exe answer with a word (None only on a
        // harness fault, which would make the row unusable).
        assert!(
            row["result"]["error"].is_null(),
            "{id}: probe fault {}",
            row["result"]["error"]
        );
        match row["class"].as_str().expect("class") {
            "fileio" => {
                let root = scratch2(&id);
                // The oracle's CRT boundary serves every `create_or_open` from
                // its buffer, so the exe's `open_read` row pins slot
                // allocation, not file existence. The replay must plant the
                // file the row opens; otherwise the port's real filesystem
                // open fails and answers 0 instead of the recorded slot.
                let host = root.join("stock/system/y.dat");
                std::fs::create_dir_all(host.parent().unwrap()).expect("fixture dir");
                std::fs::write(&host, b"slr").expect("fixture file");
                let mut layer = FileLayer::new(root.clone());
                let want = row["result"]["eax"].as_u64().expect("eax") as u32;
                let got = match id.as_str() {
                    "open_write_zero" => layer.open_write("/stock/system/y.dat"),
                    "open_write_closed_same_name" => {
                        assert_eq!(layer.open_read("/stock/system/y.dat"), 1);
                        layer.close(1);
                        layer.open_write("/stock/system/y.dat")
                    }
                    "open_write_in_use_same_name" => {
                        assert_eq!(layer.open_read("/stock/system/y.dat"), 1);
                        layer.open_write("/stock/system/y.dat")
                    }
                    "open_write_full_table" => {
                        for i in 0..31 {
                            let p = root.join(format!("stock/system/f{i:02}.dat"));
                            std::fs::write(&p, b"x").expect("filler file");
                            assert_eq!(
                                layer.open_read(&format!("/stock/system/f{i:02}.dat")),
                                (i + 1) as u32,
                                "{id}: filler slot"
                            );
                        }
                        layer.open_write("/stock/new/y.dat")
                    }
                    "open_write_mkdir" => layer.open_write("/stock/miss/y.dat"),
                    other => panic!("{id}: unhandled open_write row {other}"),
                };
                assert_eq!(got, want, "{id}: open_write result");
                // The slot words the exe kept, and the file the create
                // helper materialised (CREATE_ALWAYS disposition 2).
                let slot_word = 1 + row["result"]["slots"]
                    .as_array()
                    .expect("slots")
                    .iter()
                    .position(|s| s["in_use"].as_u64() == Some(1))
                    .filter(|_| want > 0)
                    .unwrap_or(0);
                if want > 0 {
                    assert_eq!(
                        got_slot(&layer, slot_word as u32),
                        want_slot(row, slot_word - 1),
                        "{id}: slot {slot_word} words"
                    );
                }
                let target = row["result"]["creates"]
                    .as_array()
                    .and_then(|c| c.first())
                    .map(|c| c[1].as_str().expect("create name").to_string());
                if let Some(name) = target {
                    // The oracle records the CRT-facing DOS path; on this host the
                    // root is a real directory tree, so separators map to '/'.
                    let host = root.join(name.trim_start_matches('\\').replace('\\', "/"));
                    assert!(host.is_file(), "{id}: created file {}", host.display());
                }
                if id == "open_write_mkdir" {
                    for d in ["stock", "stock/miss"] {
                        assert!(root.join(d).is_dir(), "{id}: dir {d}");
                    }
                }
            }
            "findfile" => {
                let root = scratch2(&id);
                let entries = row_entries(row);
                // The oracle's FindFirstFileA boundary serves a queue regardless of
                // the pattern's directory, so the row pins the exe's slot/sort/
                // delivery logic. On a real tree the entries must live in the
                // directory each search actually names.
                let search_dirs: Vec<String> = match id.as_str() {
                    "find_open_full_table" => (0..33).map(|i| format!("stock/cars{i}")).collect(),
                    _ => vec!["stock/cars".to_string()],
                };
                for dir in &search_dirs {
                    plant_entries(&root.join(dir), &entries);
                }
                let mut host = slr_io::findfile::FindHost::new(root);
                match id.as_str() {
                    "find_open_one"
                    | "find_open_sorted"
                    | "find_open_nomatch"
                    | "find_open_empty_pattern" => {
                        let pattern = match id.as_str() {
                            "find_open_one" => "/stock/cars/*.car",
                            "find_open_sorted" => "/stock/cars/*",
                            "find_open_nomatch" => "/stock/none/*.car",
                            _ => "",
                        };
                        let want = row["result"]["eax"].as_u64().expect("eax") as u32;
                        let got = host.open_search(pattern);
                        assert_eq!(got, want as i32, "{id}: open_search");
                        assert_eq!(
                            port_record(&mut host, got).or(None),
                            record0(row, "record0"),
                            "{id}: delivered record 0"
                        );
                    }
                    "find_two_slots_and_reuse" => {
                        let wants: Vec<i32> = row["result"]["eax"]
                            .as_array()
                            .expect("eax list")
                            .iter()
                            .map(|v| v.as_i64().expect("word") as i32)
                            .collect();
                        assert_eq!(host.open_search("/stock/cars/*.car"), wants[0]);
                        assert_eq!(host.open_search("/stock/cars/*"), wants[1]);
                        assert!(host.free(wants[0]), "{id}: free");
                        assert_eq!(host.open_search("/stock/cars/*.car"), wants[2]);
                    }
                    "find_open_full_table" => {
                        let wants: Vec<i32> = row["result"]["eax"]
                            .as_array()
                            .expect("eax list")
                            .iter()
                            .map(|v| v.as_i64().expect("word") as i32)
                            .collect();
                        assert_eq!(wants.len(), 33);
                        for (i, want) in wants.iter().enumerate() {
                            let got = host.open_search(&format!("/stock/cars{i}/*.car"));
                            assert_eq!(got, *want, "{id}: open {i}");
                        }
                    }
                    "find_advance" => {
                        let handle = host.open_search("/stock/cars/*");
                        assert_eq!(handle, row["result"]["eax"].as_i64().expect("eax") as i32);
                        // `record0` holds the whole delivery sequence, index 0 being
                        // the record `open_search` already returned.
                        let seq: Vec<Option<(String, u32)>> = row["result"]["record0"]
                            .as_array()
                            .expect("seq")
                            .iter()
                            .map(|rec| {
                                (!rec.is_null()).then(|| {
                                    (
                                        rec["name"].as_str().expect("name").to_string(),
                                        rec["attr"].as_u64().expect("attr") as u32,
                                    )
                                })
                            })
                            .collect();
                        for (i, want) in seq.iter().enumerate().skip(1) {
                            let got = host.advance(handle).map(|e| (e.name, u32::from(e.is_dir)));
                            assert_eq!(got.as_ref(), want.as_ref(), "{id}: advance {i}");
                        }
                        assert!(host.advance(handle).is_none(), "{id}: exhausted advance");
                    }
                    "find_advance_bad" => {
                        let wants: Vec<i32> = row["result"]["eax"]
                            .as_array()
                            .expect("eax list")
                            .iter()
                            .map(|v| v.as_i64().expect("word") as i32)
                            .collect();
                        // A free slot, an out-of-range index and a negative
                        // handle all answer the exe's -1: the port's None.
                        for handle in [3, 0x20, -1] {
                            assert!(host.advance(handle).is_none(), "{id}: advance {handle}");
                            let _ = wants;
                        }
                    }
                    "find_free" => {
                        let handle = host.open_search("/stock/cars/*.car");
                        assert_eq!(handle, row["result"]["eax"].as_i64().expect("eax") as i32);
                        // exe [0, 0, -1, -1]: valid-index frees succeed twice,
                        // out-of-range and negative fail.
                        assert!(host.free(handle), "{id}: free 1");
                        assert!(host.free(handle), "{id}: free 2");
                        assert!(!host.free(0x20), "{id}: free oob");
                        assert!(!host.free(-1), "{id}: free negative");
                    }
                    "find_advance_after_free" => {
                        let handle = host.open_search("/stock/cars/*.car");
                        assert!(host.free(handle), "{id}: free");
                        assert!(host.advance(handle).is_none(), "{id}: advance");
                    }
                    other => panic!("{id}: unhandled findfile row {other}"),
                }
            }
            other => panic!("{id}: unhandled class {other}"),
        }
        checked += 1;
    }
    eprintln!("boot2: replayed {checked} rows");
    assert_eq!(checked, rows.len());
}
