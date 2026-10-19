//! Corpus tests: every shipped `.bon`, `.skl` and `.matrixdata` parses, and
//! the files of one model agree with each other.
//!
//! Two profiles, like `slr-formats`: the clean stock install (`SLR_STOCK`,
//! default `<repo>/stock`, the source of truth) and the modded install
//! (`SLR_REFERENCE`, default `<repo>/reference/raw`, compatibility profile).
//! Each skips with a note when its directory is absent so
//! `cargo test --workspace` stays green without the data.

use slr_fmt_skeleton::{parse_animation, parse_skeleton, Animation, Skeleton};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root(var: &str, default: &str) -> Option<PathBuf> {
    let p = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(default));
    p.is_dir().then_some(p)
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn ext(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[derive(Default, Debug, PartialEq)]
struct Stats {
    bon: usize,
    bon_bones: usize,
    skl: usize,
    skl_bones: usize,
    matrixdata: usize,
    matrixdata_keys: usize,
}

struct Loaded {
    skeletons: BTreeMap<PathBuf, Skeleton>,
    animations: BTreeMap<PathBuf, Animation>,
    stats: Stats,
}

fn load(root: &Path) -> Loaded {
    let mut l = Loaded {
        skeletons: BTreeMap::new(),
        animations: BTreeMap::new(),
        stats: Stats::default(),
    };
    for p in walk(root) {
        let data = || std::fs::read(&p).unwrap();
        match ext(&p).as_str() {
            "bon" | "skl" => {
                let sk = parse_skeleton(&data()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                if ext(&p) == "bon" {
                    l.stats.bon += 1;
                    l.stats.bon_bones += sk.bones.len();
                } else {
                    l.stats.skl += 1;
                    l.stats.skl_bones += sk.bones.len();
                }
                l.skeletons.insert(p, sk);
            }
            "matrixdata" => {
                let a = parse_animation(&data()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                l.stats.matrixdata += 1;
                l.stats.matrixdata_keys += a.keys.len();
                l.animations.insert(p, a);
            }
            _ => {}
        }
    }
    l
}

/// Structural invariants that hold on every shipped file.
fn check_invariants(l: &Loaded) {
    for (p, sk) in &l.skeletons {
        let n = sk.bones.len();
        assert!(n >= 2, "{}: {n} bones", p.display());
        // Every bone is reached exactly once by the parent-before-child thread.
        let order: Vec<usize> = sk.traversal().collect();
        let mut seen = order.clone();
        seen.sort_unstable();
        assert_eq!(seen, (0..n).collect::<Vec<_>>(), "{}: thread", p.display());
        let mut pos = vec![0; n];
        for (k, &i) in order.iter().enumerate() {
            pos[i] = k;
        }
        for (i, b) in sk.bones.iter().enumerate() {
            if let Ok(parent) = usize::try_from(b.parent) {
                assert!(
                    pos[parent] < pos[i],
                    "{}: bone {i} before parent",
                    p.display()
                );
            }
        }
        // Every limit line scanned, and bind translations are finite.
        assert!(
            sk.bones.iter().all(|b| b.limits_explicit),
            "{}",
            p.display()
        );
        assert!(sk
            .bones
            .iter()
            .all(|b| b.matrix.iter().all(|f| f.is_finite())));
        if ext(p) == "bon" {
            // Bind-pose files carry the full +-pi range on every axis.
            let pi = 0x40490fdb;
            assert!(
                sk.bones.iter().all(|b| {
                    b.min.iter().all(|f| f.to_bits() == pi | 0x8000_0000)
                        && b.max.iter().all(|f| f.to_bits() == pi)
                }),
                "{}",
                p.display()
            );
        }
    }
    for (p, a) in &l.animations {
        assert!(
            a.declared_key_counts
                .iter()
                .all(|&c| c as usize == a.key_count),
            "{}",
            p.display()
        );
        assert!(
            a.times.windows(2).all(|w| w[0] < w[1]),
            "{}: times",
            p.display()
        );
        for k in &a.keys {
            let n: f32 = k.rotation.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((n - 1.0).abs() < 1e-5, "{}: |q| = {n}", p.display());
        }
        assert_eq!(a.bone_names.len(), a.bone_count);
    }
    // Files of one model share a stem and agree on the bone list.
    for (ap, a) in &l.animations {
        let stem = ap.with_extension("").to_string_lossy().to_ascii_lowercase();
        let names = |want: &str| {
            l.skeletons.iter().find(|(p, _)| {
                ext(p) == want
                    && p.with_extension("").to_string_lossy().to_ascii_lowercase() == stem
            })
        };
        for e in ["bon", "skl"] {
            if let Some((p, sk)) = names(e) {
                let want: Vec<&str> = sk.bones.iter().map(|b| b.name.as_str()).collect();
                assert_eq!(a.bone_names, want, "{} vs {}", ap.display(), p.display());
            }
        }
    }
}

#[test]
fn stock_profile() {
    let Some(r) = root("SLR_STOCK", "../../stock") else {
        eprintln!("SKIP: stock install not found (SLR_STOCK)");
        return;
    };
    let l = load(&r);
    check_invariants(&l);
    eprintln!("stock: {:?}", l.stats);
    assert_eq!(
        l.stats,
        Stats {
            bon: 19,
            bon_bones: 433,
            skl: 17,
            skl_bones: 429,
            matrixdata: 17,
            matrixdata_keys: 9337,
        }
    );
}

#[test]
fn modded_profile() {
    let Some(r) = root("SLR_REFERENCE", "../../reference/raw") else {
        eprintln!("SKIP: modded install not found (SLR_REFERENCE)");
        return;
    };
    let l = load(&r);
    check_invariants(&l);
    eprintln!("modded: {:?}", l.stats);
    assert_eq!(
        l.stats,
        Stats {
            bon: 21,
            bon_bones: 455,
            skl: 18,
            skl_bones: 449,
            matrixdata: 18,
            matrixdata_keys: 9437,
        }
    );
}
