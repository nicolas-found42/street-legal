//! Structural corpus coverage: each stock TUFA file loads and links in an
//! isolated namespace; the native registry reports 353 bindings. This does
//! not establish one globally linked stock world or choose among conflicting
//! bundled and standalone definitions. Production class selection is #110.
//!
//! Root defaults to the workspace `stock/` checkout; override with
//! `SLR_STOCK`. Skips with a note when the directory is absent so
//! `cargo test --workspace` stays green without the data (CI, fresh
//! worktrees). Local runs read the real 1,452 shipped `.class` files:
//! 1,451 link cleanly; `system/Scripts/render/osd/Dialog.class` is the
//! known exclusion (old compiler version `0x108f5` with pool type 10,
//! which the stock exe itself cannot read — out of scope per the
//! fidelity checklist).

use std::path::{Path, PathBuf};

use slr_vm::natives::Natives;
use slr_vm::vmclass::{LoadError, Loader};

fn stock_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    p.is_dir().then_some(p)
}

fn walk_classes(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("read_dir {}: {e}", d.display()))
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "class") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// `system/Scripts/render/osd/Dialog.class`: old compiler version
/// (`0x108f5`) whose pool uses type 10 (opaque to the `0x11ae1` reader
/// `FUN_00407660`). The stock exe cannot read it either (out of scope
/// per the fidelity checklist), so the corpus counts it as excluded.
const DIALOG_EXCLUSION: &str = "system/Scripts/render/osd/Dialog.class";

fn is_dialog_exclusion(root: &Path, p: &Path) -> bool {
    p.strip_prefix(root)
        .is_ok_and(|rel| rel.to_string_lossy().replace('\\', "/") == DIALOG_EXCLUSION)
}

#[test]
fn stock_class_files_load_and_link_in_isolated_namespaces() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: stock_class_files_load_and_link_in_isolated_namespaces");
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk_classes(&root);
    assert!(
        !files.is_empty(),
        "no .class files under {}",
        root.display()
    );
    let mut loader = Loader::new();
    loader.boot();
    assert!(loader.is_booted());
    assert_eq!(
        loader.packages().len(),
        8,
        "FUN_0040fe70 registers 8 packages"
    );
    let mut linked = 0usize;
    let mut class_modules = 0usize;
    let mut excluded = 0usize;
    let mut failures = Vec::new();
    for f in &files {
        let data = std::fs::read(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        // This is file-format/link coverage. Some shipped compilation units
        // contain dialog classes also present in standalone files; sharing one
        // namespace here silently replaced those earlier definitions. Runtime
        // duplicate-class policy is tested separately and rejects conflicts.
        let mut file_loader = Loader::new();
        match file_loader.load_file(&data) {
            Ok(_) => {
                linked += 1;
                class_modules += file_loader.len();
            }
            Err(e) => {
                if is_dialog_exclusion(&root, f) {
                    excluded += 1;
                } else {
                    failures.push(format!("{}: {e:?}", f.display()));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} unexpected load failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    eprintln!(
        "tufa_corpus: files={} linked={linked} excluded={excluded} class_modules={class_modules}",
        files.len()
    );
    assert_eq!(excluded, 1, "exactly the Dialog.class exclusion");
    assert_eq!(
        files.len(),
        linked + excluded,
        "every non-excluded file links"
    );
}

/// The native table on disk (`re/out/native_api.tsv`, git-ignored, built by
/// `re/build_native_table.py`) loads into the registry: 353 rows, every row
/// resolving back to its implementation address.
#[test]
fn native_table_reports_353_bindings() {
    let dir = std::env::var_os("SLR_NATIVE_TABLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/native_api.tsv")
        });
    let Some(text) = std::fs::read_to_string(&dir).ok() else {
        eprintln!("SLR_ORACLE_SKIPPED: native_table_reports_353_bindings");
        eprintln!("SKIP: no native table (run re/build_native_table.py)");
        return;
    };
    let mut natives = Natives::new();
    let mut rows = 0usize;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 4, "row {i}: 4 TSV columns");
        let (class, name, sig, imp) = (cols[0], cols[1], cols[2], cols[3]);
        let addr: u32 = imp
            .strip_prefix("FUN_")
            .or_else(|| imp.strip_prefix("LAB_"))
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .unwrap_or_else(|| panic!("row {i}: bad impl label {imp}"));
        natives.register(class, name, sig, addr);
        rows += 1;
    }
    assert_eq!(rows, 353, "registry reports 353 bindings");
    assert_eq!(natives.len(), 353);
    // Spot-check the largest groups resolve.
    assert!(natives
        .resolve(
            "java.game.parts.WheelRef",
            "getPos",
            "()Ljava.lang.Vector3;"
        )
        .is_some());
    eprintln!("native_table: rows={rows}");
}

#[test]
fn loader_errors_are_typed() {
    // Bare coverage of the error enum so the corpus failure path above is
    // honest about what it can report.
    let e = LoadError::NotFound("x".into());
    assert_eq!(e, LoadError::NotFound("x".into()));
}
