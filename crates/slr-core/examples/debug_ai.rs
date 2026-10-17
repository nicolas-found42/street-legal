use slr_core::catalog::Catalog;
use slr_core::race::*;
use slr_core::track::Track;
use slr_core::vehicle::*;
fn main() {
    let cat = Catalog::fallback();
    for (id, skill) in [("Baiern/DevilSport", 0.85f32), ("Baiern/CoupeSport", 0.5)] {
        let m = cat.car(id).unwrap();
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
        race.add_racer("AI", v, Some(skill), 0);
        for _ in 0..60 * 400 {
            race.step(&Controls::default(), 1.0 / 60.0);
            if race.racers[0].finish_time.is_some() {
                break;
            }
        }
        let r = &race.racers[0];
        println!(
            "{id} skill {skill}: laps {:?} finish {:?} body {:.2} penalty {}",
            r.lap_times,
            r.finish_time,
            r.vehicle.cond.body_total(),
            r.penalty
        );
    }
}
