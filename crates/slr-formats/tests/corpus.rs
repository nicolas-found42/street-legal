//! Corpus-wide verification against the read-only reference install.
//! Skipped (with a note) when `reference/raw` is absent. Override with `SLR_REFERENCE`.

use slr_formats::{cfg, corpus, rpak, tufa};
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/raw"));
    p.is_dir().then_some(p)
}

#[test]
fn every_parsable_file_in_the_reference_install_parses() {
    let Some(root) = root() else {
        eprintln!("reference/raw not present; skipping");
        return;
    };
    let rep = corpus::verify(&root).unwrap();
    eprintln!("{rep:#?}");
    assert!(rep.failures.is_empty(), "failures: {:#?}", rep.failures);
    assert_eq!(rep.files, 8411);
    assert_eq!(rep.by_ext["rpk"], 80);
    assert_eq!(rep.parsed["rpk"], 80);
    assert_eq!(rep.by_ext["scx"], 3028);
    assert_eq!(rep.scx_v3 + rep.scx_v4, 3028);
    assert_eq!(rep.by_ext["class"], 1501);
    assert_eq!(rep.parsed["class"], 1501);
    assert_eq!(rep.parsed["cfg"], 1053);
    assert_eq!(rep.parsed["sdat"], 11);
}

#[test]
fn cars_rpk_tree_is_sane() {
    let Some(root) = root() else { return };
    let pk = rpak::Package::open(root.join("cars.rpk")).unwrap();
    assert_eq!(pk.links[0].name, "system.rpk");
    assert_eq!(pk.entries.len(), 110);
    let liberty = pk.entries.iter().find(|e| e.alias == "Liberty").unwrap();
    assert!(!pk.descriptor(liberty).is_empty());
}

#[test]
fn engine_block_class_names_its_base() {
    let Some(root) = root() else { return };
    let data =
        std::fs::read(root.join("parts/engines/Baiern_Emer/scripts/Emer_StreetGT_block.class"))
            .unwrap();
    let mods = tufa::parse(&data).unwrap();
    assert_eq!(mods[0].version, 0x11ae1);
    assert_eq!(
        mods[0].class_name(),
        Some("java.game.parts.engines.Baiern_Emer.Emer_StreetGT_block")
    );
    assert_eq!(
        mods[0].base_class(),
        Some("java.game.parts.enginepart.block.block_inline.Block_Inline_OHC")
    );
}

#[test]
fn all_24_racer_vehicles_decode() {
    let Some(root) = root() else { return };
    let mut n = 0;
    for p in corpus::walk(&root.join("cars/racers")).unwrap() {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("_main_") && name.ends_with(".cfg") {
            let c = cfg::Cfg::open(&p).unwrap();
            let v = cfg::VehicleCfg::from_cfg(&c).unwrap_or_else(|| panic!("{name} not a vehicle"));
            assert_eq!(v.wheels.len(), 4, "{name}");
            assert!(
                v.body_mass > 300.0 && v.body_mass < 3000.0,
                "{name}: {}",
                v.body_mass
            );
            assert_eq!(v.gear_ratios.len(), 5.max(v.gear_ratios.len()));
            assert!(
                v.wheels.iter().filter(|w| w.steered()).count() == 2,
                "{name}"
            );
            assert!(v.wheels.iter().any(|w| w.driven()), "{name}");
            n += 1;
        }
    }
    assert_eq!(n, 24);
}

#[test]
fn baiern_coupesport_matches_the_cfg_numbers() {
    let Some(root) = root() else { return };
    let c =
        cfg::Cfg::open(root.join("cars/racers/Baiern_data/scripts/_main_CoupeSport.cfg")).unwrap();
    let v = cfg::VehicleCfg::from_cfg(&c).unwrap();
    assert!((v.body_mass - 838.5).abs() < 0.01, "{}", v.body_mass);
    assert_eq!(v.gear_ratios, vec![4.2, 2.5, 1.3, 1.0, 0.758]);
    assert_eq!(v.reverse_ratio, -4.4);
    assert_eq!(v.rearend_ratio, 4.41);
    assert_eq!(v.wheels[0].radius, 0.315);
}

#[test]
fn every_tufa_tree_decodes_exactly() {
    let Some(root) = root() else { return };
    let (mut classes, mut bodies, mut nodes) = (0, 0, 0);
    let (mut unsupported, mut mod_incompatible) = (0, 0);
    for path in corpus::walk(&root).unwrap() {
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
                        panic!("{}: {e}", path.display());
                    }
                }
            }
        }
    }
    eprintln!("classes {classes} bodies {bodies} nodes {nodes} unsupported {unsupported} mod_incompatible {mod_incompatible}");
    // 1,546 modules decode exactly (5,816 bodies, 320,522 nodes). The only older-version (0x108f5)
    // modules are the 11 in system/Scripts/render/osd/Dialog.class, whose encoding the exe cannot read.
    assert_eq!((classes, bodies, nodes), (1546, 5816, 320_522));
    assert_eq!((unsupported, mod_incompatible), (11, 0));
}

#[test]
fn every_tufa_constant_pool_parses_exactly() {
    let Some(root) = root() else { return };
    let (mut modules, mut entries) = (0usize, 0usize);
    let mut by_type = [0usize; 8];
    for path in corpus::walk(&root).unwrap() {
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
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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
    eprintln!("pool modules {modules} entries {entries} by_type {by_type:?}");
    assert_eq!(modules, 1546);
}

#[test]
fn every_tufa_method_field_and_class_chunk_parses_exactly() {
    let Some(root) = root() else { return };
    let (mut mods, mut methods, mut fields) = (0usize, 0usize, 0usize);
    for path in corpus::walk(&root).unwrap() {
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
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            {
                methods += a[0].len() + a[1].len();
            }
            if let Some(a) = m
                .fields(&data)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            {
                fields += a[0].len() + a[1].len();
            }
            m.class_rec(&data)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
    eprintln!("modules {mods} methods {methods} fields {fields}");
    assert_eq!(mods, 1546);
}
