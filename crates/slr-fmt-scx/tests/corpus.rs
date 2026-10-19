//! Corpus tests for the `INVO` container: every shipped `.scx`/`.scy` parses
//! with zero leftover bytes and range-checked indices.
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

/// Every byte is accounted for: the parser consumes the whole file, and the
/// geometry ranges are re-derived from the file so no trailing or skipped
/// bytes can hide.
fn check_zero_leftover(path: &Path) -> (usize, usize) {
    let data = std::fs::read(path).unwrap();
    let scx = slr_fmt_scx::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match scx {
        slr_fmt_scx::Scx::V4 { .. } => (0, 0),
        slr_fmt_scx::Scx::V3 { groups, trailing } => {
            let mut off = 8usize;
            let mut verts = 0usize;
            let mut tris = 0usize;
            for g in &groups {
                off += g.material_size as usize;
                let nv = g.mesh.positions.len();
                assert_eq!(g.mesh.normals.len(), nv, "{} normals", path.display());
                assert_eq!(g.mesh.uvs.len(), nv, "{} uvs", path.display());
                assert_eq!(g.mesh.extra.len(), nv, "{} extra", path.display());
                assert!(
                    g.mesh.extra.iter().all(|x| x.len() + 32 == g.stride),
                    "{} stride {}",
                    path.display(),
                    g.stride
                );
                assert!(
                    g.mesh.indices.len() % 3 == 0,
                    "{} face words",
                    path.display()
                );
                off += 4 + nv * g.stride;
                let nf = g.mesh.indices.len() / 3;
                off += 4 + nf * 12;
                verts += nv;
                tris += nf;
            }
            off += trailing.map(|_| 4).unwrap_or(0);
            assert_eq!(
                off,
                data.len(),
                "{}: parser consumed {off} of {} bytes",
                path.display(),
                data.len()
            );
            (verts, tris)
        }
    }
}

fn check_profile(name: &str, root: &Path) {
    let files = walk(root, &["scx", "scy"]);
    assert!(
        !files.is_empty(),
        "{name}: no scx/scy files under {}",
        root.display()
    );
    let (mut v3, mut v4, mut scy, mut verts, mut tris) = (0, 0, 0, 0, 0);
    let mut names = std::collections::BTreeSet::new();
    for f in &files {
        let is_scy = f
            .extension()
            .and_then(|e| e.to_str())
            .unwrap()
            .eq_ignore_ascii_case("scy");
        let (v, t) = check_zero_leftover(f);
        let data = std::fs::read(f).unwrap();
        match slr_fmt_scx::parse(&data).unwrap() {
            slr_fmt_scx::Scx::V3 { groups, .. } => {
                v3 += 1;
                if is_scy {
                    scy += 1;
                    // Collision-surface files carry no triangles; every group
                    // is an empty, named surface-type record.
                    for g in &groups {
                        assert!(g.mesh.positions.is_empty(), "{} scy verts", f.display());
                        assert!(g.mesh.indices.is_empty(), "{} scy faces", f.display());
                        names.insert(g.name());
                    }
                }
            }
            slr_fmt_scx::Scx::V4 { .. } => {
                assert!(!is_scy, "{} v4 scy?", f.display());
                v4 += 1;
            }
        }
        verts += v;
        tris += t;
    }
    eprintln!(
        "{name}: files={} v3={v3} v4={v4} scy={scy} v3verts={verts} v3tris={tris}",
        files.len()
    );
    eprintln!("{name}: scy surface names: {names:?}");
}

#[test]
fn stock_scx_v3_and_scy_decode_exactly() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: stock_scx_v3_and_scy_decode_exactly");
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    check_profile("stock", &root);
}

#[test]
fn modded_scx_v3_and_scy_decode_exactly() {
    let Some(root) = modded_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: modded_scx_v3_and_scy_decode_exactly");
        eprintln!("SKIP: no reference dir (set SLR_REFERENCE)");
        return;
    };
    check_profile("modded", &root);
}
