//! Prints headline performance numbers for every car in the reference install.
use slr_core::catalog::Catalog;
use slr_core::vehicle::{Controls, Setup, Vehicle};

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "reference/raw".into());
    let cat = Catalog::load(std::path::Path::new(&root)).expect("catalog");
    println!(
        "{:<34} {:>6} {:>6} {:>6} {:>7} {:>7} {:>6}",
        "car", "kg", "kW", "0-100", "400m s", "trap", "latG"
    );
    for car in &cat.cars {
        let eng = cat.engine(&car.stock_engine).unwrap().clone();
        let setup = Setup {
            engine: eng.clone(),
            tyre_front: cat.stock_tyre().clone(),
            tyre_rear: cat.stock_tyre().clone(),
            final_drive_scale: 1.0,
            brake_bias: 0.6,
            wing: 0.3,
            lsd: 0.0,
            automatic: true,
        };
        let mut v = Vehicle::new(&car.cfg, setup.clone());
        v.place([0.0, 0.0], 0.0);
        let (mut t, mut t100, mut t400, mut trap) = (0.0, f32::NAN, f32::NAN, 0.0);
        while t < 40.0 && t400.is_nan() {
            v.step(
                &Controls {
                    throttle: 1.0,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            t += 1.0 / 60.0;
            if t100.is_nan() && v.kph() >= 100.0 {
                t100 = t;
            }
            if v.pos[1] >= 402.3 {
                t400 = t;
                trap = v.kph();
            }
        }
        let mut w = Vehicle::new(&car.cfg, setup);
        w.place([0.0, 0.0], 0.0);
        for _ in 0..300 {
            w.step(
                &Controls {
                    throttle: 0.7,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
        }
        let mut g = 0.0f32;
        for _ in 0..180 {
            w.step(
                &Controls {
                    throttle: 0.3,
                    steer: 0.4,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            g = g.max(w.lateral_g().abs());
        }
        println!(
            "{:<34} {:>6.0} {:>6.0} {:>6.1} {:>7.1} {:>7.0} {:>6.2}",
            car.id,
            v.mass,
            eng.peak_power_kw(),
            t100,
            t400,
            trap,
            g
        );
    }
}
