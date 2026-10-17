use slr_core::catalog::Catalog;
use slr_core::race::*;
use slr_core::track::Track;
use slr_core::vehicle::*;
fn main() {
    let cat = Catalog::fallback();
    let m = cat.car("Baiern/DevilSport").unwrap();
    let v = Vehicle::new(
        &m.cfg,
        Setup {
            engine: cat.engine(&m.stock_engine).unwrap().clone(),
            tyre_front: cat.stock_tyre().clone(),
            tyre_rear: cat.stock_tyre().clone(),
            final_drive_scale: 1.0,
            brake_bias: 0.6,
            wing: 0.3,
            lsd: 0.0,
            automatic: true,
        },
    );
    let mut race = Race::new(RaceKind::Circuit { laps: 2 }, Track::circuit());
    race.add_racer("AI", v, Some(0.85), 0);
    println!(
        "len {} total {}",
        race.track.length(),
        race.total_distance()
    );
    for i in 0..60 * 8 {
        race.step(&Controls::default(), 1.0 / 60.0);
        if i % 30 == 0 {
            let r = &race.racers[0];
            println!(
                "i={i} phase={:?} time={:.2} prog={:.1} pos={:?} fin={:?}",
                race.phase, race.time, r.progress, r.vehicle.pos, r.finish_time
            );
        }
    }
}
