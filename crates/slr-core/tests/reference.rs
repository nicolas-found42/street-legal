//! Tests that need the read-only reference install (skipped when it is absent).
use slr_core::catalog::Catalog;
use slr_core::visual::load_car_visual;
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/raw"));
    p.is_dir().then_some(p)
}

#[test]
fn catalog_loads_24_cars_29_tyres_14_engines() {
    let Some(root) = root() else { return };
    let cat = Catalog::load(&root).unwrap();
    assert_eq!(cat.cars.len(), 24);
    assert_eq!(cat.tyres.len(), 29);
    assert_eq!(cat.engines.len(), 14);
    for c in &cat.cars {
        assert!(cat.engine(&c.stock_engine).is_some(), "{}", c.id);
    }
}

#[test]
fn every_car_assembles_a_plausibly_sized_body() {
    let Some(root) = root() else { return };
    let cat = Catalog::load(&root).unwrap();
    for car in &cat.cars {
        let dir = car.mesh_dir.as_ref().unwrap().parent().unwrap();
        let v = load_car_visual(dir, car.id.split('/').nth(1).unwrap())
            .unwrap_or_else(|| panic!("{} has no visual", car.id));
        let (lo, hi) = v.bounds();
        let (w, h, l) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        assert!(
            (1.4..2.4).contains(&w) && (0.9..1.7).contains(&h) && (3.4..5.5).contains(&l),
            "{}: {w:.2}x{h:.2}x{l:.2}",
            car.id
        );
        assert!(v.triangle_count() > 1000, "{}", car.id);
    }
}

#[test]
fn every_car_in_the_catalog_completes_a_full_state_cycle() {
    // Buy each model, race it against the first drag opponent, settle, save and reload.
    let Some(root) = root() else { return };
    let cat = Catalog::load(&root).unwrap();
    let mut gs = slr_core::state::GameState::new_game(&cat, 7);
    gs.money = 1e7;
    for c in &cat.cars {
        gs.buy_new(&cat, &c.id).unwrap();
        let v = gs.vehicle_for(&cat, gs.active).unwrap();
        let p = slr_core::perf::measure(&v);
        assert!(
            p.zero_to_100_s.is_some() && p.quarter_mile_s.is_some(),
            "{}: {p:?}",
            c.id
        );
    }
    let dir = std::env::temp_dir().join(format!("slr-ref-{}", std::process::id()));
    gs.save(&dir.join("s.json")).unwrap();
    assert_eq!(
        slr_core::state::GameState::load(&dir.join("s.json")).unwrap(),
        gs
    );
    std::fs::remove_dir_all(dir).ok();
}
