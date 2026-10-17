//! Corpus tests for SDAT bodies (ticket M2-05).
//!
//! - Stock (default): the clean install's `stock/` tree (override with
//!   `SLR_STOCK`). Expects 9 SDAT files: 8 vehicle saves
//!   (`cars/racers/ROC_cars/ROC_*`, no extension) + `save/game/options`.
//! - Modded: `reference/raw` (override with `SLR_REFERENCE`). Expects 11:
//!   8 vehicle saves + `save/game/options` + `save/controls/active_control_set`
//!   + `DestructionTools/Scripts/Muscle`.
//!
//! Each file parses header+body with zero leftover bytes and re-encodes
//! byte-identically. Tests skip with a note when their directory is absent,
//! so `cargo test --workspace` stays green without the data (CI).

use slr_fmt_sdat::{companion_index, is_jpeg, REF_OPTIONS_FLOATS, STOCK_OPTIONS_FLOATS};
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

fn sdat_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                if let Ok(f) = std::fs::File::open(&p) {
                    let mut magic = [0u8; 4];
                    use std::io::Read as _;
                    let mut f = f;
                    if f.read_exact(&mut magic).is_ok() && &magic == b"SDAT" {
                        out.push(p);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

fn check_file(path: &Path, layout: Option<&[bool]>) {
    let data = std::fs::read(path).unwrap();
    let sdat = slr_fmt_sdat::parse(&data, layout).unwrap_or_else(|e| {
        panic!("{}: parse failed: {e}", path.display());
    });
    let back = slr_fmt_sdat::encode(&sdat);
    assert_eq!(
        back,
        data,
        "{}: re-encode differs ({} vs {} bytes)",
        path.display(),
        back.len(),
        data.len()
    );
}

fn check_tree(root: &Path, layout: Option<&[bool]>, expect: usize, tag: &str) {
    let files = sdat_files(root);
    assert_eq!(files.len(), expect, "{tag}: sdat count");
    for f in &files {
        check_file(f, layout);
    }
    eprintln!(
        "{tag}: {} SDAT files round-trip byte-identical",
        files.len()
    );
}

#[test]
fn stock_sdat_bodies_round_trip() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP stock: no stock dir");
        return;
    };
    check_tree(&root, Some(&STOCK_OPTIONS_FLOATS), 9, "stock");
}

#[test]
fn modded_sdat_bodies_round_trip() {
    let Some(root) = modded_root() else {
        eprintln!("SKIP modded: no reference dir");
        return;
    };
    check_tree(&root, Some(&REF_OPTIONS_FLOATS), 11, "modded");
}

#[test]
fn stock_options_fields_are_sane() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP stock: no stock dir");
        return;
    };
    let data = std::fs::read(root.join("save/game/options")).unwrap();
    let sdat = slr_fmt_sdat::parse(&data, Some(&STOCK_OPTIONS_FLOATS)).unwrap();
    let slr_fmt_sdat::Body::Options(o) = &sdat.body else {
        panic!("options is not Options");
    };
    assert_eq!((o.save_id, o.save_version), (12, 0));
    assert_eq!(o.words.len(), STOCK_OPTIONS_FLOATS.len());
    assert_eq!(o.footer, vec![0, 0, 8]);
    let get_i = |n: usize| match o.words[n] {
        slr_fmt_sdat::Word::I(v) => v,
        slr_fmt_sdat::Word::F(v) => panic!("word {n} is float {v}"),
    };
    let get_f = |n: usize| match o.words[n] {
        slr_fmt_sdat::Word::F(v) => v,
        slr_fmt_sdat::Word::I(v) => panic!("word {n} is int {v}"),
    };
    // saveConfig order without windowed: x, y, depth, gamma, ...
    assert_eq!(get_i(0), 800);
    assert_eq!(get_i(1), 600);
    assert_eq!(get_i(2), 32);
    assert_eq!(get_f(3), 1.0);
    // metric/gps ints, head-move floats at the tail.
    assert_eq!(get_i(28), 1);
    assert_eq!(get_i(29), 0);
    assert_eq!(get_f(34), 1.0);
    assert_eq!(get_f(35), 1.0);
    assert_eq!(get_f(36), 1.0);
}

#[test]
fn controls_body_is_exact() {
    let Some(root) = modded_root() else {
        eprintln!("SKIP modded: no reference dir");
        return;
    };
    let data = std::fs::read(root.join("save/controls/active_control_set")).unwrap();
    let sdat = slr_fmt_sdat::parse(&data, None).unwrap();
    let slr_fmt_sdat::Body::Controls(c) = &sdat.body else {
        panic!("controls is not Controls");
    };
    assert_eq!((c.save_id, c.save_version), (16, 2));
    assert_eq!(
        c.device_names,
        vec!["SysKeyboard".to_string(), "SysMouse".to_string()]
    );
    assert_eq!(c.controls.len(), 78);
    assert_eq!(c.vasp.len(), 4);
    assert_eq!(
        c.vasp.iter().map(|v| v.id).collect::<Vec<_>>(),
        vec![28, 29, 1, 49]
    );
    assert_eq!(c.footer, vec![0, 0, 8]);
    let c0 = &c.controls[0];
    assert_eq!(
        (c0.group, c0.vaxis_id, c0.device_id, c0.axis_id),
        (1, 28, 0, 200)
    );
    assert_eq!(
        (c0.from_min, c0.from_max, c0.to_min, c0.to_max, c0.dead_zone),
        (0.0, 1.0, 0.0, 1.0, 0.0)
    );
}

#[test]
fn vehicle_heads_and_tables_are_sane() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP stock: no stock dir");
        return;
    };
    let dir = root.join("cars/racers/ROC_cars");
    let mut n = 0;
    for f in [
        "ROC_SS_1",
        "ROC_GT2_1",
        "ROC_ST_1",
        "ROC_MC_1",
        "ROC_DS_1",
        "ROC_GT3_1",
        "ROC_SD_1",
        "ROC_DS_2",
    ] {
        let data = std::fs::read(dir.join(f)).unwrap();
        let sdat = slr_fmt_sdat::parse(&data, None).unwrap();
        let slr_fmt_sdat::Body::Vehicle(v) = &sdat.body else {
            panic!("{f} is not Vehicle");
        };
        assert_eq!(v.save_id, 5, "{f} save id");
        assert!(!v.part_bytes.is_empty(), "{f} empty part run");
        assert_eq!(
            v.resources.iter().map(|e| e.index).collect::<Vec<_>>(),
            (1..=v.resources.len() as u32).collect::<Vec<_>>(),
            "{f} resource indices"
        );
        assert!(
            v.resources.iter().all(|e| e.path.ends_with(".rpk")),
            "{f} paths"
        );
        assert!(v.trailer > 0, "{f} trailer");
        n += 1;
    }
    assert_eq!(n, 8);
}

#[test]
fn jpeg_companions_are_jfif() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP stock: no stock dir");
        return;
    };
    let dir = root.join("cars/racers/ROC_cars");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = e.file_name().into_string().unwrap();
        let Some((base, idx)) = companion_index(&name) else {
            continue;
        };
        assert!(!base.is_empty());
        let data = std::fs::read(e.path()).unwrap();
        assert!(is_jpeg(&data), "{name} is not JPEG");
        let _ = idx;
        n += 1;
    }
    assert_eq!(n, 131, "stock companion count");
}

#[test]
fn companion_index_splits_names() {
    assert_eq!(companion_index("ROC_GT2_1.16"), Some(("ROC_GT2_1", 16)));
    assert_eq!(companion_index("Muscle.20"), Some(("Muscle", 20)));
    assert_eq!(companion_index("ROC_GT2_1"), None);
    assert_eq!(companion_index("foo.bar"), None);
    assert_eq!(companion_index("foo."), None);
}
