//! Request-driven class loading (`FUN_00410890`/`FUN_00410630`/
//! `FUN_0041d290`/`FUN_00410430`): replay the 13 path/directory/file regen
//! rows plus the Dialog-first/YesNoDialog-first end-to-end requests through
//! the public [`slr_vm::vmclass::Loader::request`] seam.
//!
//! Fixture inputs are copied from the recovery-archive rows into ignored
//! `re/out/vmclass110/` (existing oracle-test convention): the paths and
//! directories regen files land next to the already-present files regen
//! file. Rows replay against the real `stock/` tree (override with
//! `SLR_STOCK`; the suite skips with a note when absent so
//! `cargo test --workspace` stays green without the data).
//!
//! Covered original behavior:
//! - `410890` normalizes a well-formed `L...;` descriptor and checks
//!   `411610` FIRST: a hit performs no compile check, no file read, no
//!   registration (asserted via the read seam's counter).
//! - Miss: `410630` asks `41d290` for a directory via the `JVM+0x7c`
//!   package root, takes the LAST dotted component as the basename, and
//!   requests `directory + "\\" + basename + ".class"` through `410430`
//!   (real concatenation; the read seam supplies exact stock bytes;
//!   missing reads return null with the original log, NO fallback
//!   filename, no second directory after a failed read).
//! - Directory selection is exact-case, first matching entry wins; the
//!   second entry wins only if it alone lists the basename; a stale first
//!   entry (read fails) does NOT fall back; absent/lowercase/uppercase
//!   package requests read nothing and return null with the original
//!   diagnostics.
//! - Real `411b20` parses unchanged stock bytes and real `403a90`
//!   registers every module; the post-load result follows the ordinary
//!   cache lookup (winner after registration), not "first parsed class".

use std::path::PathBuf;

use slr_vm::vmclass::{FileReader, Loader};

fn stock_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    p.is_dir().then_some(p)
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmclass110")
}

fn load_rows(name: &str) -> Option<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(out_dir().join(name)).ok()?;
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

/// A loader whose package cache mirrors the probe's controlled stock
/// nested-dialog folder: one directory entry listing every `.class`
/// basename under `system/Scripts/render/osd/dialog`, in walk order.
fn dialog_loader(root: &std::path::Path) -> Loader {
    let mut loader = Loader::new();
    loader.boot();
    loader.set_reader(FileReader::stock(root.to_path_buf()));
    // The real `system/Scripts/render/osd/dialog` directory backs the
    // logical `system\Scripts\render\osd\dialog` requests: seed the
    // package cache from the walk, then narrow to the dialog directory
    // the rows address.
    loader.build_package_cache(root);
    loader
}

/// Basenames the probe's prebuilt cache lists for the dialog directory.
fn dialog_basenames(root: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("system/Scripts/render/osd/dialog"))
        .expect("dialog dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "class"))
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn request_paths_replay_path_rows() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: request_paths_replay_path_rows");
        return;
    };
    let Some(rows) = load_rows("slr-vm110-class-selection-paths-regen.jsonl") else {
        eprintln!("SLR_ORACLE_SKIPPED: request_paths_replay_path_rows");
        return;
    };
    assert_eq!(rows.len(), 5, "five path rows");
    let basenames = dialog_basenames(&root);
    for (i, row) in rows.iter().enumerate() {
        let order: Vec<&str> = row["order"]
            .as_array()
            .expect("order")
            .iter()
            .map(|v| v.as_str().expect("order entry"))
            .collect();
        let observations = row["observations"].as_array().expect("observations");
        assert_eq!(observations.len(), order.len(), "row {i}: one observation per request");
        let mut loader = dialog_loader(&root);
        for (observation, requested_suffix) in observations.iter().zip(order.iter()) {
            let requested = observation["requested"].as_str().expect("requested");
            // The descriptor-first row requests with `L...;` form.
            if requested.starts_with('L') {
                assert!(
                    requested_suffix.starts_with('L'),
                    "row {i}: descriptor request matches order"
                );
            }
            let before = loader.len();
            let selected = loader
                .request(requested)
                .unwrap_or_else(|e| panic!("row {i} request {requested}: {e:?}"));
            let events = observation["events"].as_array().expect("events");
            let new_registrations = observation["new_class_registrations"]
                .as_u64()
                .expect("new_class_registrations")
                as usize;
            assert_eq!(
                loader.len() - before,
                new_registrations,
                "row {i} request {requested}: registrations"
            );
            if new_registrations > 0 {
                // Miss: exactly one compile check + one file read for the
                // `directory\basename.class` construction.
                assert_eq!(events.len(), 2, "row {i} request {requested}: miss events");
                let compile = &events[0]["compile_check"];
                assert!(compile.is_array(), "row {i}: compile_check first");
                let read = events[1]["read_request"].as_str().expect("read_request");
                let bare = requested
                    .strip_prefix('L')
                    .and_then(|s| s.strip_suffix(';'))
                    .unwrap_or(requested);
                let basename = bare.rsplit('.').next().expect("basename");
                assert!(
                    read.ends_with(&format!("{basename}.class")),
                    "row {i} request {requested}: read {read}"
                );
                assert!(
                    basenames.iter().any(|b| b == basename),
                    "row {i} request {requested}: basename listed"
                );
                let id = selected.unwrap_or_else(|| panic!("row {i} request {requested}: selected"));
                assert_eq!(
                    loader.class_name(id),
                    Some(bare),
                    "row {i} request {requested}: post-load cache winner"
                );
            } else {
                // Hit: zero compile/file events, zero registrations, and
                // the selected record is the ordinary cache lookup.
                assert!(
                    events.is_empty(),
                    "row {i} request {requested}: hit has no events"
                );
                let bare = requested
                    .strip_prefix('L')
                    .and_then(|s| s.strip_suffix(';'))
                    .unwrap_or(requested);
                let id = selected.unwrap_or_else(|| panic!("row {i} request {requested}: hit"));
                assert_eq!(loader.class_name(id), Some(bare));
                assert_eq!(loader.lookup(bare), Some(id));
            }
        }
    }
    eprintln!("request_paths: rows={}", rows.len());
}
