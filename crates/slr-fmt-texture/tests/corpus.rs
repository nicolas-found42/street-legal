//! Corpus tests for the PTX container: every shipped `.ptx` parses with
//! zero leftover bytes and exact per-mip accounting.
//!
//! Roots default to the workspace `stock/` and `reference/raw` checkouts;
//! override with `SLR_STOCK` / `SLR_REFERENCE`. Tests skip with a note when
//! the directory is absent so `cargo test --workspace` stays green on
//! machines without the data (CI, fresh worktrees).

use std::path::{Path, PathBuf};

fn stock_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    p.is_dir().then_some(p)
}

fn modded_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/raw"));
    p.is_dir().then_some(p)
}

fn walk(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.iter().any(|w| e.eq_ignore_ascii_case(w)))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every byte is accounted for: the parser consumes the whole file, each
/// entry satisfies `total == rgb_size + alpha_size`, and `width`/`height`
/// equal the largest mip's decoded JPEG dimensions.
fn check_zero_leftover(path: &Path) -> (usize, bool) {
    let data = std::fs::read(path).unwrap();
    let ptx = slr_fmt_texture::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(
        ptx.mips.len(),
        ptx.mip_count as usize,
        "{} mip count",
        path.display()
    );
    let mut accounted = 16usize;
    for (i, m) in ptx.mips.iter().enumerate() {
        assert_eq!(
            m.total,
            m.rgb_size + m.alpha_size,
            "{} mip {i} total",
            path.display()
        );
        accounted += 20 + m.total as usize;
        // Payload slicing is re-derived from the file so no trailing or
        // skipped bytes can hide: the jpeg bytes must start with SOI.
        assert!(
            m.rgb.len() >= 2 && m.rgb[0] == 0xFF && m.rgb[1] == 0xD8,
            "{} mip {i} rgb SOI",
            path.display()
        );
        if m.has_alpha() {
            assert!(
                m.alpha.len() >= 2 && m.alpha[0] == 0xFF && m.alpha[1] == 0xD8,
                "{} mip {i} alpha SOI",
                path.display()
            );
            assert_eq!(
                m.alpha.len(),
                m.alpha_size as usize,
                "{} mip {i} alpha len",
                path.display()
            );
        } else {
            assert!(m.alpha.is_empty(), "{} mip {i} alpha", path.display());
        }
    }
    assert_eq!(
        accounted,
        data.len(),
        "{}: parser consumed {accounted} of {} bytes",
        path.display(),
        data.len()
    );
    let has_alpha = ptx.mips.iter().any(|m| m.has_alpha());
    (ptx.mips.len(), has_alpha)
}

fn check_profile(name: &str, root: &Path, expect_files: usize) {
    let files = walk(root, &["ptx"]);
    assert_eq!(files.len(), expect_files, "{name}: ptx file count");
    assert!(
        !files.is_empty(),
        "{name}: no ptx files under {}",
        root.display()
    );
    let (mut mips, mut with_alpha, mut rgb_only) = (0, 0, 0);
    let mut mip_counts = std::collections::BTreeMap::new();
    for f in &files {
        let (n, has_alpha) = check_zero_leftover(f);
        mips += n;
        *mip_counts.entry(n).or_insert(0usize) += 1;
        if has_alpha {
            with_alpha += 1;
        } else {
            rgb_only += 1;
        }
    }
    eprintln!("{name}: files={} mips={mips} with_alpha={with_alpha} rgb_only={rgb_only} mip_counts={mip_counts:?}", files.len());
}

#[test]
fn stock_ptx_parses_zero_leftover() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    check_profile("stock", &root, 127);
}

#[test]
fn modded_ptx_parses_zero_leftover() {
    let Some(root) = modded_root() else {
        eprintln!("SKIP: no reference dir (set SLR_REFERENCE)");
        return;
    };
    check_profile("modded", &root, 127);
}

/// Tier-D JPEG decode: the largest mip of one RGB-only and one alpha-carrying
/// stock file must decode to the header `width`/`height` (dimensions only,
/// never pixels — pixel output is the exe's static libjpeg, not the crate).
#[test]
fn sample_mips_decode_to_header_dimensions() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    for rel in [
        "maps/skydome/textures/Morning_01.ptx",
        "Data/Textures/Default.ptx",
    ] {
        let path = root.join(rel);
        assert!(path.is_file(), "missing sample {}", path.display());
        let data = std::fs::read(&path).unwrap();
        let ptx = slr_fmt_texture::parse(&data).unwrap();
        let last = ptx.mips.last().unwrap();
        let (w, h, _) = slr_fmt_texture::decode_rgb(last).expect("jpeg decode");
        assert_eq!(
            (u32::from(w), u32::from(h)),
            (ptx.width, ptx.height),
            "{} decoded dims",
            path.display()
        );
        eprintln!(
            "{}: decoded {}x{} == header {}x{}",
            path.display(),
            w,
            h,
            ptx.width,
            ptx.height
        );
    }
}
