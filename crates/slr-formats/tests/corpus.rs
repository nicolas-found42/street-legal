//! Corpus-wide verification, two profiles.
//!
//! - Stock (default): the clean install in `stock/` (override with `SLR_STOCK`).
//!   This is the source of truth for stock parity.
//! - Modded compatibility: the modded install in `reference/raw`
//!   (override with `SLR_REFERENCE`).
//!
//! Each profile's tests skip with a note when its directory is absent, so
//! `cargo test --workspace` stays green on machines without the data
//! (CI, fresh worktrees).

use slr_formats::{cfg, corpus, rpak, traffic, tufa};
use std::path::{Path, PathBuf};

/// Expected corpus counts for one profile, derived by running the parsers.
struct Counts {
    /// Total files walked (`.DS_Store` excluded).
    files: usize,
    rpk: usize,
    scx: usize,
    scx_v3: usize,
    scy: usize,
    scx_v3_groups: usize,
    scx_v3_verts: usize,
    scx_v3_tris: usize,
    class_files: usize,
    cfg: usize,
    sdat: usize,
    /// All TUFA modules, including the older-version ones the exe cannot read.
    tufa_modules: usize,
    /// Version 0x11ae1 modules (the ones `pool`/`methods`/`trees` decode).
    modules: usize,
    pool_entries: usize,
    methods: usize,
    fields: usize,
    bodies: usize,
    nodes: usize,
    /// Older-version (0x108f5) modules the stock exe cannot decode.
    unsupported: usize,
    /// `.trc` traffic graph files (all parse with zero leftover bytes).
    trc: usize,
    /// Lane records across all `.trc` files.
    trc_lanes: usize,
    /// Lane items across all `.trc` files.
    trc_lane_items: usize,
}

// Derived 2026-10-17 by running the parsers over the clean stock install.
const STOCK: Counts = Counts {
    files: 7858,
    rpk: 61,
    scx: 2921,
    scx_v3: 390,
    scy: 13,
    scx_v3_groups: 961,
    scx_v3_verts: 178473,
    scx_v3_tris: 70247,
    class_files: 1452,
    cfg: 1012,
    sdat: 9,
    tufa_modules: 1502,
    modules: 1491,
    pool_entries: 94703,
    methods: 3414,
    fields: 2730,
    bodies: 5262,
    nodes: 269034,
    unsupported: 11,
    trc: 1,
    trc_lanes: 1473,
    trc_lane_items: 4864,
};

// The modded install keeps the previously asserted counts.
const MODDED: Counts = Counts {
    files: 8411,
    rpk: 80,
    scx: 3028,
    scx_v3: 441,
    scy: 21,
    scx_v3_groups: 1102,
    scx_v3_verts: 288510,
    scx_v3_tris: 107663,
    class_files: 1501,
    cfg: 1053,
    sdat: 11,
    tufa_modules: 1557,
    modules: 1546,
    pool_entries: 102711,
    methods: 3591,
    fields: 3531,
    bodies: 5816,
    nodes: 320522,
    unsupported: 11,
    trc: 1,
    trc_lanes: 1473,
    trc_lane_items: 4864,
};

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

/// Roots of the profiles present on this machine, stock first.
fn present() -> Vec<(&'static str, PathBuf)> {
    [("stock", stock_root()), ("modded", modded_root())]
        .into_iter()
        .filter_map(|(name, root)| root.map(|p| (name, p)))
        .collect()
}

fn check_verify(name: &str, root: &Path, c: &Counts) {
    let rep = corpus::verify(root).unwrap();
    eprintln!("{name}: {rep:#?}");
    assert!(
        rep.failures.is_empty(),
        "{name} failures: {:#?}",
        rep.failures
    );
    assert_eq!(rep.files, c.files, "{name} file count");
    assert_eq!(rep.by_ext["rpk"], c.rpk, "{name} rpk files");
    assert_eq!(rep.parsed["rpk"], c.rpk, "{name} parsed rpk");
    assert_eq!(rep.by_ext["scx"], c.scx, "{name} scx files");
    assert_eq!(
        rep.scx_v3 + rep.scx_v4,
        c.scx + c.scy,
        "{name} scx versions"
    );
    assert_eq!(rep.scx_v3, c.scx_v3 + c.scy, "{name} scx v3");
    assert_eq!(rep.scy, c.scy, "{name} scy files");
    assert_eq!(
        rep.parsed.get("scy").copied().unwrap_or(0),
        c.scy,
        "{name} parsed scy"
    );
    assert_eq!(rep.scx_v3_groups, c.scx_v3_groups, "{name} scx v3 groups");
    assert_eq!(rep.scx_v3_verts, c.scx_v3_verts, "{name} scx v3 verts");
    assert_eq!(rep.scx_v3_tris, c.scx_v3_tris, "{name} scx v3 tris");
    assert_eq!(rep.by_ext["class"], c.class_files, "{name} class files");
    assert_eq!(rep.parsed["class"], c.class_files, "{name} parsed class");
    assert_eq!(rep.parsed["cfg"], c.cfg, "{name} parsed cfg");
    assert_eq!(rep.parsed["sdat"], c.sdat, "{name} parsed sdat");
    assert_eq!(rep.tufa_modules, c.tufa_modules, "{name} tufa modules");
    assert_eq!(rep.trc_files, c.trc, "{name} trc files");
    assert_eq!(rep.trc_lanes, c.trc_lanes, "{name} trc lanes");
    assert_eq!(
        rep.trc_lane_items, c.trc_lane_items,
        "{name} trc lane items"
    );
}

fn check_trc(name: &str, root: &Path, c: &Counts) {
    let (mut files, mut lanes, mut items) = (0, 0, 0);
    for path in corpus::walk(root).unwrap() {
        if path.extension().and_then(|e| e.to_str()) != Some("trc") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let trc =
            traffic::parse(&data).unwrap_or_else(|e| panic!("{name} {}: {e}", path.display()));
        // Zero leftover bytes: section 1 plus every lane record tile the file.
        let mut pos = 32 + trc.section1.len();
        for l in &trc.lanes {
            assert_eq!(
                l.offset,
                pos,
                "{name} {} lane at {:#x}",
                path.display(),
                l.offset
            );
            pos += 4 + l.items.len() * traffic::ITEM_LEN + l.trailer.len();
        }
        assert_eq!(pos, data.len(), "{name} {} leftover bytes", path.display());
        files += 1;
        lanes += trc.lanes.len();
        items += trc.lanes.iter().map(|l| l.items.len()).sum::<usize>();
    }
    eprintln!("{name} trc files {files} lanes {lanes} items {items}");
    assert_eq!(files, c.trc, "{name} trc files");
    assert_eq!(lanes, c.trc_lanes, "{name} trc lanes");
    assert_eq!(items, c.trc_lane_items, "{name} trc lane items");
}

fn check_tufa_trees(name: &str, root: &Path, c: &Counts) {
    let (mut classes, mut bodies, mut nodes) = (0, 0, 0);
    let (mut unsupported, mut mod_incompatible) = (0, 0);
    for path in corpus::walk(root).unwrap() {
        if path.extension().and_then(|e| e.to_str()) != Some("class") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        for m in tufa::parse(&data).unwrap() {
            match m.trees(&data) {
                Ok(t) => {
                    classes += 1;
                    bodies += t.len();
                    nodes += t.iter().map(Vec::len).sum::<usize>();
                }
                Err(slr_formats::FormatError::UnsupportedVersion(_)) => unsupported += 1,
                Err(e) => {
                    let p = path.to_string_lossy();
                    if p.contains("MW_Mod") || p.contains("DestructionTools") {
                        mod_incompatible += 1; // compiled by a newer compiler than the stock exe's
                    } else {
                        panic!("{name} {}: {e}", path.display());
                    }
                }
            }
        }
    }
    eprintln!("{name} classes {classes} bodies {bodies} nodes {nodes} unsupported {unsupported} mod_incompatible {mod_incompatible}");
    // The only older-version (0x108f5) modules are the 11 in
    // system/Scripts/render/osd/Dialog.class, whose encoding the exe cannot read.
    assert_eq!((classes, bodies, nodes), (c.modules, c.bodies, c.nodes));
    assert_eq!((unsupported, mod_incompatible), (c.unsupported, 0));
}

fn check_tufa_pool(name: &str, root: &Path, c: &Counts) {
    let (mut modules, mut entries) = (0usize, 0usize);
    let mut by_type = [0usize; 8];
    for path in corpus::walk(root).unwrap() {
        if path.extension().and_then(|e| e.to_str()) != Some("class") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        for m in tufa::parse(&data).unwrap() {
            if m.version != 0x11ae1 {
                continue;
            }
            let pool = m
                .pool(&data)
                .unwrap_or_else(|e| panic!("{name} {}: {e}", path.display()));
            modules += 1;
            entries += pool.len();
            for e in &pool {
                let t = match e {
                    tufa::PoolEntry::Str(_) => 0,
                    tufa::PoolEntry::Int(_) => 1,
                    tufa::PoolEntry::Float(_) => 2,
                    tufa::PoolEntry::Member { .. } => 3,
                    tufa::PoolEntry::Class(_) => 4,
                    tufa::PoolEntry::Type5 { .. } => 5,
                    tufa::PoolEntry::Type7 { .. } => 7,
                };
                by_type[t] += 1;
            }
        }
    }
    eprintln!("{name} pool modules {modules} entries {entries} by_type {by_type:?}");
    assert_eq!(modules, c.modules, "{name} pool modules");
    assert_eq!(entries, c.pool_entries, "{name} pool entries");
}

fn check_tufa_methods_fields(name: &str, root: &Path, c: &Counts) {
    let (mut mods, mut methods, mut fields) = (0usize, 0usize, 0usize);
    for path in corpus::walk(root).unwrap() {
        if path.extension().and_then(|e| e.to_str()) != Some("class") {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        for m in tufa::parse(&data).unwrap() {
            if m.version != 0x11ae1 {
                continue;
            }
            mods += 1;
            if let Some(a) = m
                .methods(&data)
                .unwrap_or_else(|e| panic!("{name} {}: {e}", path.display()))
            {
                methods += a[0].len() + a[1].len();
            }
            if let Some(a) = m
                .fields(&data)
                .unwrap_or_else(|e| panic!("{name} {}: {e}", path.display()))
            {
                fields += a[0].len() + a[1].len();
            }
            m.class_rec(&data)
                .unwrap_or_else(|e| panic!("{name} {}: {e}", path.display()));
        }
    }
    eprintln!("{name} modules {mods} methods {methods} fields {fields}");
    assert_eq!(mods, c.modules, "{name} method/field modules");
    assert_eq!(methods, c.methods, "{name} methods");
    assert_eq!(fields, c.fields, "{name} fields");
}

#[test]
fn stock_install_parses_cleanly() {
    let Some(root) = stock_root() else {
        eprintln!("stock/ not present; skipping");
        return;
    };
    check_verify("stock", &root, &STOCK);
}

#[test]
fn modded_install_parses_cleanly() {
    let Some(root) = modded_root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    check_verify("modded", &root, &MODDED);
}

#[test]
fn stock_tufa_trees_decode_exactly() {
    let Some(root) = stock_root() else {
        eprintln!("stock/ not present; skipping");
        return;
    };
    check_tufa_trees("stock", &root, &STOCK);
}

#[test]
fn modded_tufa_trees_decode_exactly() {
    let Some(root) = modded_root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    check_tufa_trees("modded", &root, &MODDED);
}

#[test]
fn stock_tufa_constant_pool_parses_exactly() {
    let Some(root) = stock_root() else {
        eprintln!("stock/ not present; skipping");
        return;
    };
    check_tufa_pool("stock", &root, &STOCK);
}

#[test]
fn modded_tufa_constant_pool_parses_exactly() {
    let Some(root) = modded_root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    check_tufa_pool("modded", &root, &MODDED);
}

#[test]
fn stock_tufa_method_field_and_class_chunk_parses_exactly() {
    let Some(root) = stock_root() else {
        eprintln!("stock/ not present; skipping");
        return;
    };
    check_tufa_methods_fields("stock", &root, &STOCK);
}

#[test]
fn modded_tufa_method_field_and_class_chunk_parses_exactly() {
    let Some(root) = modded_root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    check_tufa_methods_fields("modded", &root, &MODDED);
}

#[test]
fn cars_rpk_tree_is_sane() {
    let profiles = present();
    if profiles.is_empty() {
        eprintln!("no corpus present; skipping");
        return;
    }
    for (name, root) in &profiles {
        let pk = rpak::Package::open(root.join("cars.rpk")).unwrap();
        assert_eq!(pk.links[0].name, "system.rpk", "{name}");
        assert_eq!(pk.entries.len(), 110, "{name}");
        let liberty = pk.entries.iter().find(|e| e.alias == "Liberty").unwrap();
        assert!(!pk.descriptor(liberty).is_empty(), "{name}");
    }
}

#[test]
fn engine_block_class_names_its_base() {
    let profiles = present();
    if profiles.is_empty() {
        eprintln!("no corpus present; skipping");
        return;
    }
    for (name, root) in &profiles {
        let data =
            std::fs::read(root.join("parts/engines/Baiern_Emer/scripts/Emer_StreetGT_block.class"))
                .unwrap();
        let mods = tufa::parse(&data).unwrap();
        assert_eq!(mods[0].version, 0x11ae1, "{name}");
        assert_eq!(
            mods[0].class_name(),
            Some("java.game.parts.engines.Baiern_Emer.Emer_StreetGT_block"),
            "{name}"
        );
        assert_eq!(
            mods[0].base_class(),
            Some("java.game.parts.enginepart.block.block_inline.Block_Inline_OHC"),
            "{name}"
        );
    }
}

#[test]
fn all_24_racer_vehicles_decode() {
    let profiles = present();
    if profiles.is_empty() {
        eprintln!("no corpus present; skipping");
        return;
    }
    for (name, root) in &profiles {
        let mut n = 0;
        for p in corpus::walk(&root.join("cars/racers")).unwrap() {
            let file = p.file_name().unwrap().to_string_lossy().into_owned();
            if file.starts_with("_main_") && file.ends_with(".cfg") {
                let c = cfg::Cfg::open(&p).unwrap();
                let v = cfg::VehicleCfg::from_cfg(&c)
                    .unwrap_or_else(|| panic!("{name} {file} not a vehicle"));
                assert_eq!(v.wheels.len(), 4, "{name} {file}");
                assert!(
                    v.body_mass > 300.0 && v.body_mass < 3000.0,
                    "{name} {file}: {}",
                    v.body_mass
                );
                assert_eq!(
                    v.gear_ratios.len(),
                    5.max(v.gear_ratios.len()),
                    "{name} {file}"
                );
                assert!(
                    v.wheels.iter().filter(|w| w.steered()).count() == 2,
                    "{name} {file}"
                );
                assert!(v.wheels.iter().any(|w| w.driven()), "{name} {file}");
                n += 1;
            }
        }
        assert_eq!(n, 24, "{name} racer count");
    }
}

#[test]
fn baiern_coupesport_matches_the_cfg_numbers() {
    let profiles = present();
    if profiles.is_empty() {
        eprintln!("no corpus present; skipping");
        return;
    }
    for (name, root) in &profiles {
        let c = cfg::Cfg::open(root.join("cars/racers/Baiern_data/scripts/_main_CoupeSport.cfg"))
            .unwrap();
        let v = cfg::VehicleCfg::from_cfg(&c).unwrap();
        assert!((v.body_mass - 838.5).abs() < 0.01, "{name} {}", v.body_mass);
        assert_eq!(v.gear_ratios, vec![4.2, 2.5, 1.3, 1.0, 0.758], "{name}");
        assert_eq!(v.reverse_ratio, -4.4, "{name}");
        assert_eq!(v.rearend_ratio, 4.41, "{name}");
        assert_eq!(v.wheels[0].radius, 0.315, "{name}");
    }
}

#[test]
fn stock_trc_parses_with_zero_leftover() {
    let Some(root) = stock_root() else {
        eprintln!("stock/ not present; skipping");
        return;
    };
    check_trc("stock", &root, &STOCK);
}

#[test]
fn modded_trc_parses_with_zero_leftover() {
    let Some(root) = modded_root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    check_trc("modded", &root, &MODDED);
}
