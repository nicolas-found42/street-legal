//! Corpus test (needs data): the real clean stock install must verify against the manifest that
//! `tools/extract-stock.sh` generated with `shasum`, so the Rust verifier and the script agree on
//! every one of the 7,858 files. Skips (passes) when `SLR_STOCK`/`stock/` or its manifest is absent.

use std::path::PathBuf;

#[test]
fn real_stock_install_verifies() {
    let root = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    if !root.is_dir() {
        eprintln!("SKIP: no stock install at {}", root.display());
        return;
    }
    if !root.join(slr_io::stock::MANIFEST_NAME).is_file() {
        eprintln!(
            "SKIP: {} has no {} (re-run tools/extract-stock.sh --force)",
            root.display(),
            slr_io::stock::MANIFEST_NAME
        );
        return;
    }
    let report = slr_io::stock::verify_stock(&root).unwrap_or_else(|e| panic!("{e}"));
    eprintln!(
        "stock: {} files, {} bytes verified, {} extra",
        report.files,
        report.bytes,
        report.extra.len()
    );
    assert_eq!(report.files, 7858);
}
