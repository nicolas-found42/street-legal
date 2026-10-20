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
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
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
        assert_eq!(
            observations.len(),
            order.len(),
            "row {i}: one observation per request"
        );
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
                .expect("new_class_registrations") as usize;
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
                let id =
                    selected.unwrap_or_else(|| panic!("row {i} request {requested}: selected"));
                assert_eq!(
                    loader.class_name(id),
                    Some(bare),
                    "row {i} request {requested}: post-load cache winner"
                );
                // First-parsed killer: serve the BUNDLE bytes under the
                // standalone Congrats logical filename on a fresh loader.
                // The miss then loads 11 modules with modules[0] =
                // Dialog while requesting Congrats; a first-parsed
                // return hands back Dialog, the original returns the
                // ordinary cache lookup (Congrats).
                if requested.ends_with("CongratsDialog") {
                    let bundle =
                        std::fs::read(root.join("system/Scripts/render/osd/dialog/Dialog.class"))
                            .expect("stock bundle");
                    let mut fresh = dialog_loader(&root);
                    let mut reader = FileReader::default();
                    reader.serve(
                        "system\\scripts\\render\\osd\\dialog\\CongratsDialog.class",
                        bundle,
                    );
                    fresh.set_reader(reader);
                    let id = fresh
                        .request(requested)
                        .unwrap_or_else(|e| panic!("forced-bundle miss: {e:?}"))
                        .expect("forced bundle loads");
                    assert_eq!(
                        fresh.class_name(id),
                        Some(bare),
                        "forced-bundle miss returns Congrats, not modules[0]"
                    );
                    assert_eq!(
                        fresh.lookup(bare),
                        Some(id),
                        "forced-bundle miss equals the cache winner"
                    );
                    assert_eq!(fresh.len(), 11, "forced-bundle miss registers 11");
                }
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

/// The probe's two-entry directory cache, rebuilt over controlled
/// basenames: `first\dialogs` and `second\dialogs` list caller-chosen
/// names. The stock-read seam serves the real dialog bytes for the
/// constructed logical filename of `YesNoDialog` only when the test
/// allows it; every other read fails like the original missing file.
fn controlled_loader(first: &[&str], second: &[&str], yesno_bytes: Option<Vec<u8>>) -> Loader {
    let mut loader = Loader::new();
    loader.boot();
    let mut reader = FileReader::default();
    if let Some(bytes) = yesno_bytes {
        reader.serve("first\\dialogs\\YesNoDialog.class", bytes.clone());
        reader.serve("second\\dialogs\\YesNoDialog.class", bytes);
    }
    loader.set_reader(reader);
    // Bypass the stock walk: the rows address a controlled two-entry
    // cache, not the real tree. Entries are injected through the same
    // seam the walk fills, in probe order.
    loader.set_package_cache(vec![
        (
            "first\\dialogs".to_owned(),
            first.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ),
        (
            "second\\dialogs".to_owned(),
            second.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ),
    ]);
    loader
}

#[test]
fn request_directories_replay_directory_rows() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: request_directories_replay_directory_rows");
        return;
    };
    let Some(rows) = load_rows("slr-vm110-class-selection-directories-regen.jsonl") else {
        eprintln!("SLR_ORACLE_SKIPPED: request_directories_replay_directory_rows");
        return;
    };
    assert_eq!(rows.len(), 6, "six directory rows");
    let yesno = std::fs::read(root.join("system/Scripts/render/osd/dialog/YesNoDialog.class"))
        .expect("stock YesNoDialog.class");
    // Row 0: both entries list the basename — first wins.
    assert_eq!(rows[0]["case"].as_str(), Some("both-have-name"));
    // Row 1: only the second lists it — second wins.
    // Rows 2-5: stale/absent/case-mismatch — null with diagnostics.
    let result1 = rows[1]["result"].clone();
    assert_eq!(rows[1]["case"].as_str(), Some("only-second-has-name"));
    assert!(
        result1["selected"].is_object(),
        "row 1 selects the second directory's file"
    );
    for (i, row) in rows.iter().enumerate().skip(2) {
        assert!(
            row["result"]["selected"].is_null(),
            "row {i} ({}): null",
            row["case"].as_str().unwrap_or("?")
        );
        assert!(
            !row["result"]["diagnostics"]
                .as_array()
                .expect("diagnostics")
                .is_empty(),
            "row {i}: original diagnostics"
        );
    }
    // Behavioral core (the assertions that pin the port, not the JSON):
    // first-wins.
    let mut loader = controlled_loader(&["YesNoDialog"], &["YesNoDialog"], Some(yesno.clone()));
    assert_eq!(
        loader
            .request_filename("java.render.osd.dialog.YesNoDialog")
            .as_deref(),
        Some("first\\dialogs\\YesNoDialog.class"),
        "both list it: first entry wins"
    );
    assert!(loader
        .request("java.render.osd.dialog.YesNoDialog")
        .unwrap()
        .is_some());
    assert!(loader.take_diagnostics().is_empty(), "success logs nothing");
    // Second-wins only when alone.
    let mut loader = controlled_loader(&["OtherDialog"], &["YesNoDialog"], Some(yesno.clone()));
    assert_eq!(
        loader
            .request_filename("java.render.osd.dialog.YesNoDialog")
            .as_deref(),
        Some("second\\dialogs\\YesNoDialog.class"),
        "only second lists it: second wins"
    );
    assert!(loader
        .request("java.render.osd.dialog.YesNoDialog")
        .unwrap()
        .is_some());
    // Stale first entry: single failed read, no fallback, null + logs.
    let mut loader = controlled_loader(&["YesNoDialog"], &["YesNoDialog"], None);
    assert_eq!(
        loader
            .request("java.render.osd.dialog.YesNoDialog")
            .unwrap(),
        None,
        "stale first entry: no fallback to second"
    );
    let diagnostics = loader.take_diagnostics();
    assert_eq!(
        diagnostics.len(),
        2,
        "loadClass-failed + getClass-not-found"
    );
    assert!(
        diagnostics[0].contains("failed to load first\\dialogs\\YesNoDialog.class"),
        "original loadClass log: {}",
        diagnostics[0]
    );
    assert!(
        diagnostics[1].contains("java.render.osd.dialog.YesNoDialog")
            && diagnostics[1].contains("not found"),
        "original getClass log: {}",
        diagnostics[1]
    );
    // Absent / lowercase-basename / uppercase-package: rows 3-5 carry
    // `Class not found` diagnostics with ZERO events — no read at all.
    // The uppercase package (`java.Render...`) never reaches the
    // basename match: 41d290 walks the package tree from the `JVM+0x7c`
    // root, and `Render` matches no package node, so selection stops
    // before any directory (and any read) is attempted.
    for requested in [
        "java.render.osd.dialog.NoSuchDialog",
        "java.render.osd.dialog.yesnodialog",
        "java.Render.osd.dialog.YesNoDialog",
    ] {
        let mut loader = controlled_loader(&["YesNoDialog"], &["YesNoDialog"], Some(yesno.clone()));
        assert_eq!(
            loader.request(requested).unwrap(),
            None,
            "{requested}: no selection"
        );
        assert!(
            loader.request_filename(requested).is_none(),
            "{requested}: no filename, hence no read"
        );
        let diagnostics = loader.take_diagnostics();
        assert_eq!(diagnostics.len(), 2, "{requested}: two missing-class logs");
        assert!(
            diagnostics[0].contains("Class not found") && diagnostics[0].contains(requested),
            "original missing-class log: {}",
            diagnostics[0]
        );
    }
    eprintln!("request_directories: rows={}", rows.len());
}

#[test]
fn request_files_replay_file_rows() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: request_files_replay_file_rows");
        return;
    };
    let Some(rows) = load_rows("slr-vm110-class-selection-files-regen.jsonl") else {
        eprintln!("SLR_ORACLE_SKIPPED: request_files_replay_file_rows");
        return;
    };
    assert_eq!(rows.len(), 2, "two file rows");
    for (i, row) in rows.iter().enumerate() {
        let order: Vec<&str> = row["order"]
            .as_array()
            .expect("order")
            .iter()
            .map(|v| v.as_str().expect("order entry"))
            .collect();
        assert_eq!(order.len(), 2, "row {i}: two files");
        // End-to-end through the public seam: request Dialog first vs
        // YesNoDialog first, then assert the row's final cache winners.
        let mut loader = dialog_loader(&root);
        let dialog_first = order[0] == "Dialog.class";
        if dialog_first {
            let dialog = loader.request("java.render.osd.dialog.Dialog").unwrap();
            assert!(dialog.is_some(), "row {i}: bundle loads");
            assert_eq!(loader.len(), 11, "row {i}: 11 modules registered");
            let yesno = loader
                .request("java.render.osd.dialog.YesNoDialog")
                .unwrap();
            assert!(yesno.is_some(), "row {i}: YesNo resolves from the bundle");
            assert_eq!(loader.len(), 11, "row {i}: hit registers nothing");
        } else {
            let yesno = loader
                .request("java.render.osd.dialog.YesNoDialog")
                .unwrap();
            assert!(yesno.is_some(), "row {i}: standalone loads");
            assert_eq!(loader.len(), 1, "row {i}: 1 module registered");
            let dialog = loader.request("java.render.osd.dialog.Dialog").unwrap();
            // The bundle's own first module is Dialog — NOT YesNo. A
            // first-parsed-class return would hand back the Dialog
            // record here; the original returns the ordinary cache
            // lookup for the requested name instead.
            assert_eq!(
                dialog.map(|id| loader.class_name(id)),
                Some(Some("java.render.osd.dialog.Dialog")),
                "row {i}: bundle request returns Dialog (its cache winner)"
            );
            assert_eq!(loader.len(), 12, "row {i}: 11 more records");
            // Post-load winner is the ordinary cache lookup: the
            // standalone YesNo stays selected (first-wins bucket
            // topology), not the bundle's duplicate. The identity
            // comparison above IS the pin: `winner == yesno`
            // (standalone record) fails under any first-parsed-class
            // return, which hands back the bundle's duplicate instead.
            // The Dialog-first branch covers the symmetric case: its
            // YesNo hit resolves inside the just-loaded bundle.
            let winner = loader
                .request("java.render.osd.dialog.YesNoDialog")
                .unwrap();
            assert_eq!(winner, yesno, "row {i}: cache winner, not first-parsed");
            // Pool-length discriminator: the standalone YesNo owns 15
            // pool entries, the bundle's duplicate 16. A
            // first-matching-module return hands back the bundle
            // duplicate (16), failing this even when identities
            // accidentally coincide.
            assert_eq!(
                loader.pool_of(winner.unwrap()).map(<[_]>::len),
                Some(15),
                "row {i}: winner owns the standalone pool"
            );
            // Dialog-identity discriminator: the bundle request returns
            // the Dialog record (its own cache winner), never the
            // bundle's YesNo. A first-matching-module return passes
            // this; a first-parsed-class return fails it — together
            // the two discriminators pin the ordinary-lookup return.
            assert_eq!(
                dialog.map(|id| loader.class_name(id)),
                Some(Some("java.render.osd.dialog.Dialog")),
                "row {i}: Dialog request returns Dialog"
            );
        }
        let finals = row["final"].as_array().expect("final");
        for entry in finals {
            let name = entry["name"].as_str().expect("name");
            let selected = entry["selected"].as_object().expect("selected");
            let registration = selected["registration"].as_u64().expect("registration");
            let id = loader
                .lookup(name)
                .unwrap_or_else(|| panic!("row {i}: {name} selected"));
            // The registration index counts within its own file's module
            // list; behaviorally: the selected record's name matches and
            // the record is one of the registered modules.
            assert_eq!(loader.class_name(id), Some(name), "row {i}: {name}");
            let _ = registration;
        }
    }
    eprintln!("request_files: rows={}", rows.len());
}

#[test]
fn request_hit_performs_no_read_or_registration() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: request_hit_performs_no_read_or_registration");
        return;
    };
    let mut loader = dialog_loader(&root);
    let first = loader
        .request("java.render.osd.dialog.Dialog")
        .unwrap()
        .expect("miss loads");
    let _ = first;
    assert_eq!(loader.len(), 11);
    // Swap in a counting reader that fails every read: a hit must never
    // reach it.
    let mut reader = FileReader::default();
    reader.fail("system\\scripts\\render\\osd\\dialog\\Dialog.class");
    loader.set_reader(reader);
    let before = loader.len();
    for requested in [
        "java.render.osd.dialog.Dialog",
        "Ljava.render.osd.dialog.Dialog;",
        "java.render.osd.dialog.YesNoDialog",
    ] {
        let hit = loader.request(requested).unwrap();
        assert!(hit.is_some(), "{requested}: hit");
        assert_eq!(loader.len(), before, "{requested}: no registration");
        assert!(loader.take_diagnostics().is_empty(), "{requested}: no logs");
    }
}
