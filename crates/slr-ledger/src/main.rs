//! `slr-ledger` binary: `scan` | `collect` | `regen`.
//!
//! - `scan`: print every `/// exe:` annotation as `file:line symbol item`.
//! - `collect`: run the corpus suites and write `re/out/test-results.json`.
//! - `regen` (default): rewrite the ledger section of `docs/fidelity.md`.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut d = std::env::current_dir().expect("cwd");
    loop {
        if d.join("Cargo.toml").is_file() && d.join("crates").is_dir() {
            return d;
        }
        if !d.pop() {
            panic!("no workspace root above the current directory");
        }
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    let root = workspace_root();
    match arg.as_deref() {
        Some("scan") => {
            for a in slr_ledger::scan_workspace(&root) {
                println!("{}:{} {} {}", a.file, a.line, a.symbol, a.item);
            }
        }
        Some("collect") => collect(&root),
        None | Some("regen") => regen(&root),
        Some(other) => {
            eprintln!("usage: slr-ledger [scan|collect|regen]");
            std::process::exit(if other == "--help" { 0 } else { 2 });
        }
    }
}

fn regen(root: &std::path::Path) {
    let annotations = slr_ledger::scan_workspace(root);
    let results_path = root.join(slr_ledger::RESULTS_PATH);
    let results = slr_ledger::read_results(&results_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let rows = slr_ledger::build_rows(&annotations, &results);
    let section = slr_ledger::render_section(&rows);
    let doc_path = root.join(slr_ledger::FIDELITY_PATH);
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", doc_path.display());
        std::process::exit(1);
    });
    let out = slr_ledger::splice_section(&doc, &section);
    std::fs::write(&doc_path, &out).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", doc_path.display());
        std::process::exit(1);
    });
    println!(
        "ledger: {} symbols, {} evidence rows -> {}",
        rows.len(),
        results.iter().filter(|r| r.status == "pass").count(),
        doc_path.display()
    );
}

/// Corpus suites whose tests feed the ledger, with the exe symbols they
/// cover. Each entry is `(package, test_target, [(test_name, exe_symbols)])`.
/// One passing test row is recorded per symbol, so a corpus test that asserts
/// several chunks at once (e.g. methods + fields + class record) evidences
/// each of its exe symbols. An empty symbol list records the passing test
/// with no exe counterpart (kept for provenance, ignored by the ledger join).
/// One corpus test plus every exe symbol it evidences (empty = no exe counterpart).
type SuiteTest = (&'static str, Vec<&'static str>);
/// One suite: package, test target, and its tests.
type Suite = (&'static str, &'static str, Vec<SuiteTest>);

fn collect_suites() -> Vec<Suite> {
    vec![
        (
            "slr-formats",
            "corpus",
            vec![
                ("stock_install_parses_cleanly", vec![]),
                ("modded_install_parses_cleanly", vec![]),
                (
                    "stock_tufa_trees_decode_exactly",
                    vec!["FUN_0041a5f0", "FUN_0041a5c0"],
                ),
                (
                    "modded_tufa_trees_decode_exactly",
                    vec!["FUN_0041a5f0", "FUN_0041a5c0"],
                ),
                (
                    "stock_tufa_constant_pool_parses_exactly",
                    vec!["FUN_00407660"],
                ),
                (
                    "modded_tufa_constant_pool_parses_exactly",
                    vec!["FUN_00407660"],
                ),
                (
                    "stock_tufa_method_field_and_class_chunk_parses_exactly",
                    vec!["FUN_004195b0", "FUN_00419b60", "FUN_00411b20"],
                ),
                (
                    "modded_tufa_method_field_and_class_chunk_parses_exactly",
                    vec!["FUN_004195b0", "FUN_00419b60", "FUN_00411b20"],
                ),
                ("cars_rpk_tree_is_sane", vec![]),
                ("engine_block_class_names_its_base", vec![]),
                ("all_24_racer_vehicles_decode", vec!["0x00443f90"]),
                (
                    "baiern_coupesport_matches_the_cfg_numbers",
                    vec!["0x00443f90"],
                ),
            ],
        ),
        (
            "slr-fmt-scx",
            "corpus",
            vec![
                ("stock_scx_v3_and_scy_decode_exactly", vec!["FUN_004b5c30"]),
                ("modded_scx_v3_and_scy_decode_exactly", vec!["FUN_004b5c30"]),
            ],
        ),
    ]
}

/// Run the corpus suites, parse the human-readable `cargo test` output
/// (stable across libtest versions, unlike `--format json`), and write
/// `re/out/test-results.json`.
fn collect(root: &std::path::Path) {
    let today = today_iso();
    let stock = std::env::var("SLR_STOCK")
        .unwrap_or_else(|_| "/Users/nicolas/Documents/github/games/street-legal/stock".to_string());
    let reference = std::env::var("SLR_REFERENCE").unwrap_or_else(|_| {
        "/Users/nicolas/Documents/github/games/street-legal/reference/raw".to_string()
    });
    let mut results: Vec<slr_ledger::TestResult> = Vec::new();
    for (package, target, tests) in collect_suites() {
        let output = std::process::Command::new("cargo")
            .args(["test", "-p", package, "--test", target])
            .env("SLR_STOCK", &stock)
            .env("SLR_REFERENCE", &reference)
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| {
                eprintln!("error: cannot run cargo test: {e}");
                std::process::exit(1);
            });
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        let statuses = parse_libtest_statuses(&combined);
        if !output.status.success() {
            eprintln!("error: cargo test -p {package} --test {target} failed:\n{combined}");
            std::process::exit(1);
        }
        for (name, exe_symbols) in &tests {
            let status = statuses.get(*name).cloned().unwrap_or_else(|| {
                eprintln!("error: test {name} did not run in {package} --test {target}");
                std::process::exit(1);
            });
            if exe_symbols.is_empty() {
                results.push(slr_ledger::TestResult {
                    test: name.to_string(),
                    krate: package.to_string(),
                    exe_symbol: None,
                    oracle: "corpus".to_string(),
                    status: status.clone(),
                    ran_on: today.clone(),
                });
            }
            for sym in exe_symbols {
                results.push(slr_ledger::TestResult {
                    test: name.to_string(),
                    krate: package.to_string(),
                    exe_symbol: Some(sym.to_string()),
                    oracle: "corpus".to_string(),
                    status: status.clone(),
                    ran_on: today.clone(),
                });
            }
        }
    }
    let path = root.join(slr_ledger::RESULTS_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: {}: {e}", parent.display());
            std::process::exit(1);
        });
    }
    let json = serde_json::to_string_pretty(&results).expect("serialize results");
    std::fs::write(&path, format!("{json}\n")).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", path.display());
        std::process::exit(1);
    });
    let passed = results.iter().filter(|r| r.status == "pass").count();
    println!(
        "collect: {passed}/{} pass -> {}",
        results.len(),
        path.display()
    );
}

/// Parse `test <name> ... ok|FAILED|ignored` lines from `cargo test` output.
fn parse_libtest_statuses(output: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for line in output.lines() {
        let line = line.trim_start_matches(|c: char| c.is_whitespace());
        // libtest prints `test <name> ... ok`; strip any leading
        // `test ` after harness prefixes like `running` lines are skipped.
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(" ... ") else {
            continue;
        };
        let status = if tail.starts_with("ok") {
            "pass"
        } else if tail.starts_with("FAILED") {
            "fail"
        } else {
            continue; // `ignored`, etc.
        };
        map.insert(name.trim().to_string(), status.to_string());
    }
    map
}

/// Today's date as `YYYY-MM-DD`, via the `date` command (no extra deps).
fn today_iso() -> String {
    std::process::Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 10)
        .unwrap_or_else(|| "1970-01-01".to_string())
}
