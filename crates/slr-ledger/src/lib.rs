//! Fidelity ledger generator (plan M0-06).
//!
//! Scans workspace Rust sources for the annotation convention
//!
//! ```text
//! /// exe: <SYMBOL>
//! ```
//!
//! (one symbol per annotation line; an item may carry several lines), reads the
//! local test-results file `re/out/test-results.json` (git-ignored, produced by
//! `cargo run -p slr-ledger -- collect`), and rewrites the generated ledger
//! section of `docs/fidelity.md` between the markers
//!
//! ```text
//! <!-- ledger:start -->
//! <!-- ledger:end -->
//! ```
//!
//! The parity-checklist section of that file stays hand-maintained and is never
//! touched: only the bytes between the two markers are replaced.
//!
//! # Annotation convention (exact)
//!
//! - The annotation is a Rust doc comment whose text, after the `///`, is
//!   `exe:` followed by one symbol: `/// exe: FUN_00411b20`.
//! - The symbol is a `FUN_`-style Ghidra name (e.g. `FUN_00411b20`) or a hex
//!   address (`0x00443f90`, `0x407660`, `0x4195b0`).
//! - The scan is textual: any line matching `^\s*///\s*exe:\s*(\S+)\s*$`
//!   counts. Several such lines may precede one item; the item's port location
//!   is the innermost enclosing `pub fn` / `pub struct` / `pub enum` /
//!   `pub type` / `pub const` / `pub static` / `pub mod` / `pub trait` name when
//!   one is found, else the file path.
//!
//! # Test-results file (exact schema)
//!
//! `re/out/test-results.json` is a JSON array of objects:
//!
//! ```json
//! [
//!   {"test": "stock_tufa_trees_decode_exactly", "crate": "slr-formats",
//!    "exe_symbol": "FUN_0041a5f0", "oracle": "corpus",
//!    "status": "pass", "ran_on": "2026-10-17"}
//! ]
//! ```
//!
//! Fields: `test` (full test name as `cargo test` prints it), `crate` (the
//! tested crate's package name), `exe_symbol` (the exe symbol the test covers,
//! or `null` when the test has no exe counterpart), `oracle` (one of
//! `corpus`, `unicorn`, `wine`, `headless`, `manual`), `status` (`pass` or
//! `fail`), `ran_on` (ISO date `YYYY-MM-DD`).
//!
//! The runner is `cargo run -p slr-ledger -- collect`, which runs the corpus
//! suites with `--format json` semantics (it parses the human-readable test
//! output, which is stable across libtest versions) and writes the file.
//! The ledger only trusts rows whose `status` is `pass`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One exe-address annotation found in a Rust source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The exe symbol (`FUN_00411b20`, `0x00443f90`, ...).
    pub symbol: String,
    /// Workspace-relative file path using `/` separators.
    pub file: String,
    /// 1-based line number of the annotation.
    pub line: usize,
    /// The ported item the annotation documents (`fn Module::trees`, ...),
    /// or the file path when no enclosing item is found.
    pub item: String,
}

/// One row of the local test-results file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TestResult {
    /// Full test name as `cargo test` prints it.
    pub test: String,
    /// Tested crate's package name.
    #[serde(rename = "crate")]
    pub krate: String,
    /// Exe symbol the test covers, or `None`.
    pub exe_symbol: Option<String>,
    /// Which oracle ran: `corpus`, `unicorn`, `wine`, `headless`, `manual`.
    pub oracle: String,
    /// `pass` or `fail`.
    pub status: String,
    /// ISO date `YYYY-MM-DD` the test ran.
    pub ran_on: String,
}

/// One rendered ledger row: an exe symbol, its port location(s), and evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    /// The exe symbol.
    pub symbol: String,
    /// Port locations: `path::item:line` strings, sorted, deduplicated.
    pub locations: Vec<String>,
    /// Passing test names covering this symbol (`crate::test`), sorted.
    pub evidence: Vec<String>,
    /// Oracles behind the evidence, sorted, deduplicated.
    pub oracles: Vec<String>,
    /// Row status: `passing` when at least one passing test covers the
    /// symbol, else `ported (no test yet)`.
    pub status: String,
}

/// Markers delimiting the generated section of `docs/fidelity.md`.
pub const START_MARKER: &str = "<!-- ledger:start -->";
/// Markers delimiting the generated section of `docs/fidelity.md`.
pub const END_MARKER: &str = "<!-- ledger:end -->";

/// Parse annotation lines out of one source text.
///
/// Accepts lines of the form `/// exe: <SYMBOL>` (leading whitespace
/// allowed, exactly one whitespace-separated symbol after the colon).
/// Every other line — including `/// exe:` mentions inside prose with
/// trailing words — is ignored.
pub fn parse_annotations_in_text(text: &str, file: &str) -> Vec<Annotation> {
    let mut out = Vec::new();
    // Track the innermost enclosing item: a stack of (name, depth) where
    // depth is the brace depth at which the item opened.
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut depth: usize = 0;
    // Pending annotations attach to the next item when they precede it.
    let mut pending: Vec<(String, usize)> = Vec::new();
    // Item currently being documented by pending annotations once its line
    // is seen; flushed as annotations when the item line arrives.
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            let rest = rest.trim();
            if let Some(sym) = rest.strip_prefix("exe:") {
                let sym = sym.trim();
                // Exactly one token, non-empty, no trailing prose.
                if !sym.is_empty() && !sym.contains(char::is_whitespace) {
                    pending.push((sym.to_string(), line_no));
                }
                continue;
            }
        }
        if let Some(item) = item_name(raw) {
            let name = item.to_string();
            // Attach pending annotations to this item.
            for (symbol, line) in pending.drain(..) {
                out.push(Annotation {
                    symbol,
                    file: file.to_string(),
                    line,
                    item: name.clone(),
                });
            }
            stack.push((name, depth));
        }
        // Update brace depth from the code portion of the line (strip
        // line comments so braces in comments do not count).
        let code = match raw.find("//") {
            Some(p) => &raw[..p],
            None => raw,
        };
        for ch in code.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    while stack.last().is_some_and(|(_, d)| *d >= depth) {
                        stack.pop();
                    }
                }
                _ => {}
            }
        }
        // Annotations that precede an item (doc-comment position) were
        // flushed above. Annotations *inside* an item body (e.g. on a
        // field) never see a following item line before the next
        // annotation or EOF; attach them to the enclosing item instead.
        // We do this lazily: if pending is non-empty and the next
        // non-comment, non-blank line is not an item, attach to the
        // enclosing scope when we can tell the annotation block ended.
        if !pending.is_empty() {
            let next_is_item_or_doc = {
                let mut lookahead = text.lines().skip(idx + 1);
                loop {
                    match lookahead.next() {
                        None => break false,
                        Some(l) => {
                            let t = l.trim_start();
                            if t.is_empty() || t.starts_with("///") || t.starts_with("//") {
                                continue;
                            }
                            break item_name(l).is_some() || t.starts_with("///");
                        }
                    }
                }
            };
            if !next_is_item_or_doc {
                let owner = stack
                    .last()
                    .map(|(n, _)| n.clone())
                    .unwrap_or_else(|| file.to_string());
                for (symbol, line) in pending.drain(..) {
                    out.push(Annotation {
                        symbol,
                        file: file.to_string(),
                        line,
                        item: owner.clone(),
                    });
                }
            }
        }
    }
    // Trailing annotations (file end) attach to the enclosing item or file.
    let owner: Option<String> = None;
    let _ = owner;
    if !pending.is_empty() {
        let owner = stack
            .last()
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| file.to_string());
        for (symbol, line) in pending.drain(..) {
            out.push(Annotation {
                symbol,
                file: file.to_string(),
                line,
                item: owner.clone(),
            });
        }
    }
    out
}

/// Best-effort name of the item declared on one source line.
///
/// Handles `pub fn name`, `pub struct name`, `pub enum name`,
/// `pub type name`, `pub const name`, `pub static name`, `pub mod name`,
/// `pub trait name`, plus `impl [..] Name [..] {` (reported as `Name`) and
/// `fn name` / `struct name` without `pub`. Attribute lines and anything
/// else return `None`.
fn item_name(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let t = t.strip_prefix("pub")?.trim_start();
    let t = t.strip_prefix("unsafe").map(str::trim_start).unwrap_or(t);
    for kw in [
        "fn", "struct", "enum", "type", "const", "static", "mod", "trait",
    ] {
        if let Some(rest) = t.strip_prefix(kw) {
            let rest = rest.strip_prefix([' ', '\t', '('])?;
            let rest = rest.trim_start();
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(rest.len());
            let mut name = &rest[..end];
            // `fn Module::trees` keeps the path; drop generics/args.
            if let Some(p) = name.find('<') {
                name = &name[..p];
            }
            if name.is_empty() {
                return None;
            }
            return Some(name);
        }
    }
    None
}

/// Scan a workspace root for `/// exe:` annotations.
///
/// Walks `root`, entering every `crates/*/src` directory (plus any crate
/// directory directly under root that holds `src/`), reading every `.rs`
/// file. Returns annotations sorted by (file, line). I/O errors on single
/// files are skipped, not fatal.
pub fn scan_workspace(root: &Path) -> Vec<Annotation> {
    let mut files: Vec<PathBuf> = Vec::new();
    let crates = root.join("crates");
    if let Ok(rd) = std::fs::read_dir(&crates) {
        for e in rd.flatten() {
            let src = e.path().join("src");
            if src.is_dir() {
                collect_rs(&src, &mut files);
            }
        }
    }
    // Also accept crates at the root level (defensive; the workspace keeps
    // them under crates/).
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().and_then(|n| n.to_str()) == Some("crates") {
                continue;
            }
            let src = p.join("src");
            if p.is_dir() && src.is_dir() && p.join("Cargo.toml").is_file() {
                collect_rs(&src, &mut files);
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        out.extend(parse_annotations_in_text(&text, &rel));
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Read the local test-results file. Missing file → empty evidence (the
/// ledger still lists every annotated symbol, as `ported (no test yet)`).
/// A present-but-invalid file is an error.
pub fn read_results(path: &Path) -> Result<Vec<TestResult>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Join annotations with passing test results into ledger rows.
///
/// Symbols sort with `FUN_`-style names first (lexicographic), then hex
/// addresses. Tests whose `status` is not `pass` contribute nothing.
pub fn build_rows(annotations: &[Annotation], results: &[TestResult]) -> Vec<LedgerRow> {
    let mut locs: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for a in annotations {
        locs.entry(a.symbol.as_str())
            .or_default()
            .push(format!("{}:{}:{}", a.file, a.item, a.line));
    }
    let mut by_symbol: BTreeMap<&str, Vec<&TestResult>> = BTreeMap::new();
    for r in results {
        if r.status != "pass" {
            continue;
        }
        if let Some(s) = r.exe_symbol.as_deref() {
            by_symbol.entry(s).or_default().push(r);
        }
    }
    let mut symbols: Vec<&str> = locs.keys().copied().collect();
    symbols.sort_by_key(|a| symbol_key(a));
    symbols
        .into_iter()
        .map(|s| {
            let mut locations = locs[s].clone();
            locations.sort();
            locations.dedup();
            let mut evidence: Vec<String> = Vec::new();
            let mut oracles: Vec<String> = Vec::new();
            if let Some(rs) = by_symbol.get(s) {
                for r in rs {
                    evidence.push(format!("{}::{}", r.krate, r.test));
                    if !oracles.contains(&r.oracle) {
                        oracles.push(r.oracle.clone());
                    }
                }
                evidence.sort();
                evidence.dedup();
                oracles.sort();
            }
            let status = if evidence.is_empty() {
                "ported (no test yet)".to_string()
            } else {
                "passing".to_string()
            };
            LedgerRow {
                symbol: s.to_string(),
                locations,
                evidence,
                oracles,
                status,
            }
        })
        .collect()
}

/// Sort key: `FUN_`-style symbols first, then numeric addresses by value,
/// then anything else lexicographically.
fn symbol_key(s: &str) -> (u8, String) {
    if let Some(hex) = s.strip_prefix("FUN_") {
        return (0, hex.to_uppercase());
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if let Ok(v) = u64::from_str_radix(hex, 16) {
            return (1, format!("{v:016x}"));
        }
    }
    (2, s.to_string())
}

/// Render ledger rows as the markdown section body (generated note plus table;
/// the `## Fidelity ledger` heading lives in the hand-maintained doc).
pub fn render_section(rows: &[LedgerRow]) -> String {
    let mut s = String::new();
    s.push_str("Generated by `slr-ledger` (plan M0-06) from `/// exe:` annotations plus\n");
    s.push_str("`re/out/test-results.json`. Do not edit by hand: rerun\n");
    s.push_str("`cargo run -p slr-ledger -- regen`.\n\n");
    if rows.is_empty() {
        s.push_str("No `/// exe:` annotations found.\n");
        return s;
    }
    s.push_str("| Exe symbol | Port location | Status | Evidence (oracle) |\n");
    s.push_str("|---|---|---|---|\n");
    for r in rows {
        let loc = r
            .locations
            .iter()
            .map(|l| format!("`{l}`"))
            .collect::<Vec<_>>()
            .join("<br>");
        let ev = if r.evidence.is_empty() {
            "—".to_string()
        } else {
            let oracle = if r.oracles.is_empty() {
                String::new()
            } else {
                format!(" ({})", r.oracles.join(", "))
            };
            format!(
                "{}{}",
                r.evidence
                    .iter()
                    .map(|e| format!("`{e}`"))
                    .collect::<Vec<_>>()
                    .join("<br>"),
                oracle
            )
        };
        s.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            r.symbol, loc, r.status, ev
        ));
    }
    s
}

/// Replace the bytes between `START_MARKER` and `END_MARKER` (exclusive)
/// with the rendered section. The markers stay; everything outside them is
/// preserved byte for byte.
///
/// If the markers are absent, the section (wrapped in the markers) is
/// appended at the end of the file.
pub fn splice_section(doc: &str, section: &str) -> String {
    match (doc.find(START_MARKER), doc.find(END_MARKER)) {
        (Some(a), Some(b)) if a < b => {
            let mut out = String::new();
            out.push_str(&doc[..a + START_MARKER.len()]);
            out.push('\n');
            out.push('\n');
            out.push_str(section.trim_end());
            out.push('\n');
            out.push('\n');
            out.push_str(&doc[b..]);
            out
        }
        _ => {
            let mut out = doc.trim_end().to_string();
            out.push_str("\n\n");
            out.push_str(START_MARKER);
            out.push_str("\n\n");
            out.push_str(section.trim_end());
            out.push('\n');
            out.push('\n');
            out.push_str(END_MARKER);
            out.push('\n');
            out
        }
    }
}

/// Default workspace-relative path of the local test-results file.
pub const RESULTS_PATH: &str = "re/out/test-results.json";
/// Default workspace-relative path of the fidelity doc.
pub const FIDELITY_PATH: &str = "docs/fidelity.md";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_annotation_before_fn() {
        let text = "/// Decode trees.\n/// exe: FUN_0041a5f0\npub fn trees(&self) {}\n";
        let a = parse_annotations_in_text(text, "crates/slr-fmt-tufa/src/lib.rs");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].symbol, "FUN_0041a5f0");
        assert_eq!(a[0].line, 2);
    }

    #[test]
    fn ignores_prose_mentions_with_trailing_words() {
        // The SCX convention documents `/// exe:` lines in prose; those
        // lines carry trailing words and must not scan as annotations.
        let text =
            "/// `/// exe:` lines name the loader logic each field ports\npub struct V3Group {\n";
        let a = parse_annotations_in_text(text, "f.rs");
        assert!(a.is_empty(), "{a:?}");
    }

    #[test]
    fn attaches_field_annotations_to_enclosing_struct() {
        let text = "pub struct Mesh {\n    /// exe: FUN_004b5c30\n    pub stride: usize,\n}\n";
        let a = parse_annotations_in_text(text, "f.rs");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].item, "Mesh");
    }

    #[test]
    fn failing_tests_contribute_no_evidence() {
        let ann = vec![Annotation {
            symbol: "FUN_00411b20".into(),
            file: "crates/slr-fmt-tufa/src/lib.rs".into(),
            line: 1,
            item: "parse".into(),
        }];
        let res = vec![TestResult {
            test: "t".into(),
            krate: "slr-formats".into(),
            exe_symbol: Some("FUN_00411b20".into()),
            oracle: "corpus".into(),
            status: "fail".into(),
            ran_on: "2026-10-17".into(),
        }];
        let rows = build_rows(&ann, &res);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "ported (no test yet)");
        assert!(rows[0].evidence.is_empty());
    }

    #[test]
    fn splice_preserves_handwritten_sections() {
        let doc = "# Fidelity\n\n## Parity checklist (stock single-player)\n\n| a |\n\n<!-- ledger:start -->\nOLD\n<!-- ledger:end -->\n";
        let out = splice_section(doc, "NEW\n");
        assert!(out.contains("## Parity checklist (stock single-player)"));
        assert!(out.contains("NEW"));
        assert!(!out.contains("OLD"));
        assert!(out.contains(START_MARKER));
        assert!(out.contains(END_MARKER));
    }

    #[test]
    fn splice_appends_markers_when_absent() {
        let doc = "# Fidelity\n\ntext\n";
        let out = splice_section(doc, "BODY\n");
        assert!(out.starts_with("# Fidelity"));
        assert!(out.contains(START_MARKER));
        assert!(out.contains(END_MARKER));
        assert!(out.contains("BODY"));
    }
}
