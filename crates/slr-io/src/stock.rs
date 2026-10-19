//! Startup checksum verification of the clean stock install (plan M7-04).
//!
//! The original refuses to run without its CD (`FUN_00401480`, with `texture.dat` as the
//! disc-bound payload). Neither is ported (ADR 0009, ledgered divergence in `docs/fidelity.md`).
//! Instead the port checks the clean stock install on disk by checksum before it reads any of it.
//!
//! Checksum source: `tools/extract-stock.sh` writes [`MANIFEST_NAME`] inside the git-ignored
//! `stock/` right after it has verified the extracted tree against the MSI (count, per-file sizes,
//! byte-identity with an independent `msiextract` tree, exe hashes). Hashes derived from the
//! original's files are therefore never committed; only the two exe SHA-256 values, which the
//! repository already publishes in full (`tools/extract-stock.sh`; `docs/` shows them abbreviated), are pinned in code
//! ([`STOCK_PINS`]) so a forged or stale manifest cannot vouch for a different exe.
//!
//! Manifest format (UTF-8, one entry per line, tab separated):
//!
//! ```text
//! # slr-stock-manifest v1
//! <sha256 lowercase hex> TAB <size in bytes> TAB <path relative to the stock root, '/' separated>
//! ```
//!
//! Policy: every listed file must exist with the listed size and SHA-256 (missing, truncated or
//! altered files refuse startup). Files not in the manifest are reported in [`Report::extra`] but
//! do not refuse startup (a player may add mods or logs next to a clean install; the manifest
//! guards what the port reads from the clean install).

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Name of the manifest `tools/extract-stock.sh` writes at the root of the stock install.
pub const MANIFEST_NAME: &str = ".slr-manifest";

const HEADER: &str = "# slr-stock-manifest v1";
const REGEN_HINT: &str = "re-create the stock install with `tools/extract-stock.sh --force` \
                          (needs the stock media, see docs/fidelity.md)";
/// How many offending paths a message lists per category.
const LIST_CAP: usize = 10;

/// An exe whose SHA-256 is pinned in code, so a manifest cannot vouch for a different exe.
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    pub path: &'static str,
    pub sha256: &'static str,
}

/// The two stock exes (build 606). These full hashes are already published in
/// `tools/extract-stock.sh`; the `.bak.exe` twin differs only inside the CD check.
pub const STOCK_PINS: [Pin; 2] = [
    Pin {
        path: "StreetLegal_Redline.exe",
        sha256: "16b25cace5313208e5b6f8e149edea5261634f20dc7e45157f8dac0199bfcf24",
    },
    Pin {
        path: "StreetLegal_Redline.bak.exe",
        sha256: "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde",
    },
];

/// Number of files in the clean stock install (MSI File table; also pinned in
/// `tools/extract-stock.sh`). A manifest with fewer entries cannot hide a deleted file.
pub const STOCK_FILE_COUNT: usize = 7858;

/// A verified install.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    /// Files checked (all manifest entries).
    pub files: usize,
    /// Their total size.
    pub bytes: u64,
    /// Files present under the root but absent from the manifest (sorted).
    pub extra: Vec<String>,
}

/// Manifest entries that failed (paths relative to the root, in manifest order).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Problems {
    pub missing: Vec<String>,
    pub size_mismatch: Vec<String>,
    pub hash_mismatch: Vec<String>,
}

#[derive(Debug)]
pub enum StockError {
    NotADirectory(PathBuf),
    ManifestMissing,
    ManifestInvalid { line: usize, reason: String },
    PinMissing(String),
    PinMismatch { path: String },
    WrongEntryCount { expected: usize, found: usize },
    Corrupt(Problems),
    Io(String),
}

impl fmt::Display for StockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StockError::NotADirectory(p) => write!(
                f,
                "stock install not found at {}; {REGEN_HINT}",
                p.display()
            ),
            StockError::ManifestMissing => write!(
                f,
                "stock install has no {MANIFEST_NAME} checksum manifest, so it cannot be verified; {REGEN_HINT}"
            ),
            StockError::ManifestInvalid { line, reason } => write!(
                f,
                "stock checksum manifest is invalid (line {line}: {reason}); {REGEN_HINT}"
            ),
            StockError::PinMissing(path) => write!(
                f,
                "stock checksum manifest does not list {path}; {REGEN_HINT}"
            ),
            StockError::PinMismatch { path } => write!(
                f,
                "stock checksum manifest vouches for a {path} that is not the stock build 606 exe; {REGEN_HINT}"
            ),
            StockError::WrongEntryCount { expected, found } => write!(
                f,
                "stock checksum manifest lists {found} files but the clean install has {expected}; {REGEN_HINT}"
            ),
            StockError::Corrupt(p) => {
                write!(f, "stock install is corrupted or incomplete: ")?;
                let mut parts = Vec::new();
                for (n, what, list) in [
                    (p.missing.len(), "missing", &p.missing),
                    (p.size_mismatch.len(), "wrong size", &p.size_mismatch),
                    (p.hash_mismatch.len(), "checksum mismatch", &p.hash_mismatch),
                ] {
                    if n == 0 {
                        continue;
                    }
                    let shown: Vec<&str> = list.iter().take(LIST_CAP).map(String::as_str).collect();
                    let more = if n > LIST_CAP {
                        format!(", and {} more", n - LIST_CAP)
                    } else {
                        String::new()
                    };
                    parts.push(format!("{n} {what} ({}{more})", shown.join(", ")));
                }
                write!(f, "{}; {REGEN_HINT}", parts.join("; "))
            }
            StockError::Io(e) => write!(f, "cannot read the stock install: {e}"),
        }
    }
}

impl std::error::Error for StockError {}

struct Entry {
    sha256: String,
    size: u64,
    path: String,
}

fn parse_manifest(text: &str) -> Result<Vec<Entry>, StockError> {
    let bad = |line: usize, reason: &str| StockError::ManifestInvalid {
        line,
        reason: reason.to_string(),
    };
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, l)) if l == HEADER => {}
        _ => return Err(bad(1, "missing header")),
    }
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for (i, l) in lines {
        let n = i + 1;
        let mut f = l.split('\t');
        let (Some(sha), Some(size), Some(path), None) = (f.next(), f.next(), f.next(), f.next())
        else {
            return Err(bad(n, "expected <sha256> TAB <size> TAB <path>"));
        };
        if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(bad(n, "checksum is not 64 lowercase hex digits"));
        }
        let size = size
            .parse::<u64>()
            .map_err(|_| bad(n, "size is not a number"))?;
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(bad(n, "path is not a relative path inside the install"));
        }
        if !seen.insert(path.to_string()) {
            return Err(bad(n, "duplicate path"));
        }
        entries.push(Entry {
            sha256: sha.to_string(),
            size,
            path: path.to_string(),
        });
    }
    if entries.is_empty() {
        return Err(bad(1, "no entries"));
    }
    Ok(entries)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk(root, &e.path(), out)?;
        } else if let Ok(rel) = e.path().strip_prefix(root) {
            let rel: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
            out.push(rel.join("/"));
        }
    }
    Ok(())
}

/// Verify the clean stock install at `root` against its manifest and [`STOCK_PINS`].
pub fn verify_stock(root: &Path) -> Result<Report, StockError> {
    verify_with(root, &STOCK_PINS, Some(STOCK_FILE_COUNT))
}

/// [`verify_stock`] with explicit pins and expected entry count (the unit tests use a synthetic
/// tree).
pub fn verify_with(
    root: &Path,
    pins: &[Pin],
    expected_files: Option<usize>,
) -> Result<Report, StockError> {
    if !root.is_dir() {
        return Err(StockError::NotADirectory(root.to_path_buf()));
    }
    let text = match fs::read(root.join(MANIFEST_NAME)) {
        Ok(b) => String::from_utf8(b).map_err(|_| StockError::ManifestInvalid {
            line: 1,
            reason: "not UTF-8".into(),
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(StockError::ManifestMissing)
        }
        Err(e) => return Err(StockError::Io(format!("{MANIFEST_NAME}: {e}"))),
    };
    let entries = parse_manifest(&text)?;
    if let Some(expected) = expected_files {
        if entries.len() != expected {
            return Err(StockError::WrongEntryCount {
                expected,
                found: entries.len(),
            });
        }
    }
    for pin in pins {
        match entries.iter().find(|e| e.path == pin.path) {
            None => return Err(StockError::PinMissing(pin.path.to_string())),
            Some(e) if e.sha256 != pin.sha256 => {
                return Err(StockError::PinMismatch {
                    path: pin.path.to_string(),
                })
            }
            Some(_) => {}
        }
    }

    let mut problems = Problems::default();
    let mut bytes = 0u64;
    for e in &entries {
        let p = root.join(&e.path);
        let meta = match fs::metadata(&p) {
            Ok(m) if m.is_file() => m,
            Ok(_) => {
                problems.missing.push(e.path.clone());
                continue;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                problems.missing.push(e.path.clone());
                continue;
            }
            Err(err) => return Err(StockError::Io(format!("{}: {err}", e.path))),
        };
        if meta.len() != e.size {
            problems.size_mismatch.push(e.path.clone());
            continue;
        }
        match sha256_file(&p) {
            Ok(h) if h == e.sha256 => bytes += e.size,
            Ok(_) => problems.hash_mismatch.push(e.path.clone()),
            Err(err) => return Err(StockError::Io(format!("{}: {err}", e.path))),
        }
    }
    if problems != Problems::default() {
        return Err(StockError::Corrupt(problems));
    }

    let listed: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    let mut all = Vec::new();
    walk(root, root, &mut all).map_err(|e| StockError::Io(e.to_string()))?;
    let mut extra: Vec<String> = all
        .into_iter()
        .filter(|p| p != MANIFEST_NAME && !listed.contains(p.as_str()))
        .collect();
    extra.sort();
    Ok(Report {
        files: entries.len(),
        bytes,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Independent literals (FIPS 180 test vectors), never recomputed by the code under test.
    const SHA_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "slr-io-stock-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Tree(p)
        }
        fn put(&self, rel: &str, data: &[u8]) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        fn manifest(&self, text: &str) {
            self.put(MANIFEST_NAME, text.as_bytes());
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const PIN: [Pin; 1] = [Pin {
        path: "game.exe",
        sha256: SHA_ABC,
    }];

    fn good_manifest() -> String {
        format!("# slr-stock-manifest v1\n{SHA_ABC}\t3\tgame.exe\n{SHA_EMPTY}\t0\tdata/empty.bin\n")
    }

    fn intact() -> Tree {
        let t = Tree::new();
        t.put("game.exe", b"abc");
        t.put("data/empty.bin", b"");
        t.manifest(&good_manifest());
        t
    }

    #[test]
    fn intact_tree_verifies() {
        let t = intact();
        let r = verify_with(&t.0, &PIN, None).unwrap();
        assert_eq!(
            r,
            Report {
                files: 2,
                bytes: 3,
                extra: vec![]
            }
        );
    }

    fn problems(e: StockError) -> Problems {
        match e {
            StockError::Corrupt(p) => p,
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn flipped_byte_in_a_data_file_is_refused() {
        let t = intact();
        t.put("game.exe", b"abd"); // same size, different content
        let p = problems(verify_with(&t.0, &PIN, None).unwrap_err());
        assert_eq!(p.hash_mismatch, vec!["game.exe".to_string()]);
        assert!(p.missing.is_empty() && p.size_mismatch.is_empty());
    }

    #[test]
    fn truncated_file_is_refused_as_size_mismatch() {
        let t = intact();
        t.put("game.exe", b"ab");
        let p = problems(verify_with(&t.0, &PIN, None).unwrap_err());
        assert_eq!(p.size_mismatch, vec!["game.exe".to_string()]);
    }

    #[test]
    fn missing_file_is_refused_as_incomplete() {
        let t = intact();
        fs::remove_file(t.0.join("data/empty.bin")).unwrap();
        let p = problems(verify_with(&t.0, &PIN, None).unwrap_err());
        assert_eq!(p.missing, vec!["data/empty.bin".to_string()]);
    }

    #[test]
    fn extra_files_are_reported_but_not_fatal() {
        let t = intact();
        t.put("save/game/options", b"x");
        let r = verify_with(&t.0, &PIN, None).unwrap();
        assert_eq!(r.extra, vec!["save/game/options".to_string()]);
        assert_eq!(r.files, 2);
    }

    #[test]
    fn missing_manifest_is_refused_with_regeneration_hint() {
        let t = Tree::new();
        t.put("game.exe", b"abc");
        let e = verify_with(&t.0, &PIN, None).unwrap_err();
        assert!(matches!(e, StockError::ManifestMissing));
        assert!(e.to_string().contains("tools/extract-stock.sh"), "{e}");
    }

    #[test]
    fn missing_root_is_refused() {
        let t = Tree::new();
        let e = verify_with(&t.0.join("nope"), &PIN, None).unwrap_err();
        assert!(matches!(e, StockError::NotADirectory(_)));
        assert!(e.to_string().contains("tools/extract-stock.sh"), "{e}");
    }

    #[test]
    fn manifest_cannot_vouch_for_a_different_pinned_exe() {
        let t = Tree::new();
        t.put("game.exe", b"zzz");
        // A forged manifest consistent with the tampered exe: only the pin catches it.
        let forged = "# slr-stock-manifest v1\n\
             cb1ad2119d8fafb69566510ee712661f9f14b83385006ef92aec47f523a38358\t3\tgame.exe\n";
        t.manifest(forged);
        let e = verify_with(&t.0, &PIN, None).unwrap_err();
        assert!(matches!(e, StockError::PinMismatch { .. }), "{e:?}");
    }

    #[test]
    fn manifest_without_a_pinned_exe_is_refused() {
        let t = Tree::new();
        t.put("data/empty.bin", b"");
        t.manifest(&format!(
            "# slr-stock-manifest v1\n{SHA_EMPTY}\t0\tdata/empty.bin\n"
        ));
        let e = verify_with(&t.0, &PIN, None).unwrap_err();
        assert!(matches!(e, StockError::PinMissing(_)), "{e:?}");
    }

    #[test]
    fn malformed_manifests_are_refused() {
        let bad = [
            "no header\n",
            "# slr-stock-manifest v1\nnothex\t3\tgame.exe\n",
            "# slr-stock-manifest v1\n{ABC}\tthree\tgame.exe\n",
            "# slr-stock-manifest v1\n{ABC}\t3\n",
            "# slr-stock-manifest v1\n{ABC}\t3\t../escape\n",
            "# slr-stock-manifest v1\n{ABC}\t3\t/abs\n",
            "# slr-stock-manifest v1\n{ABC}\t3\tgame.exe\n{ABC}\t3\tgame.exe\n",
            "# slr-stock-manifest v1\n",
        ];
        for text in bad {
            let t = Tree::new();
            t.put("game.exe", b"abc");
            t.manifest(&text.replace("{ABC}", SHA_ABC));
            let e = verify_with(&t.0, &PIN, None).unwrap_err();
            assert!(
                matches!(e, StockError::ManifestInvalid { .. }),
                "{text:?}: {e:?}"
            );
        }
    }

    #[test]
    fn message_names_problems_and_caps_the_list() {
        let t = Tree::new();
        let mut m = String::from("# slr-stock-manifest v1\n");
        m += &format!("{SHA_ABC}\t3\tgame.exe\n");
        t.put("game.exe", b"abc");
        for i in 0..40 {
            m += &format!("{SHA_EMPTY}\t0\tgone/{i:02}.bin\n");
        }
        t.manifest(&m);
        let msg = verify_with(&t.0, &PIN, None).unwrap_err().to_string();
        assert!(msg.contains("40 missing"), "{msg}");
        assert!(msg.contains("gone/00.bin"), "{msg}");
        assert!(!msg.contains("gone/39.bin"), "list is capped: {msg}");
        assert!(msg.contains("tools/extract-stock.sh"), "{msg}");
    }

    #[test]
    fn pins_match_the_hashes_published_in_the_extract_script() {
        let script = include_str!("../../../tools/extract-stock.sh");
        assert!(script.contains(&format!("EXPECTED_COUNT={STOCK_FILE_COUNT}")));
        for pin in STOCK_PINS {
            assert!(
                script.contains(&format!("\"{}\"", pin.sha256)),
                "{} pin drifted from tools/extract-stock.sh",
                pin.path
            );
        }
    }

    #[test]
    fn manifest_with_a_deleted_entry_is_refused_by_count() {
        let t = intact();
        // Dropping the second line (and the file) would otherwise go unnoticed.
        t.manifest(&format!(
            "# slr-stock-manifest v1\n{SHA_ABC}\t3\tgame.exe\n"
        ));
        fs::remove_file(t.0.join("data/empty.bin")).unwrap();
        assert!(verify_with(&t.0, &PIN, None).is_ok());
        let e = verify_with(&t.0, &PIN, Some(2)).unwrap_err();
        assert!(
            matches!(
                e,
                StockError::WrongEntryCount {
                    expected: 2,
                    found: 1
                }
            ),
            "{e:?}"
        );
    }
}
