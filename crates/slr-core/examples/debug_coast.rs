use slr_core::catalog::Catalog;
use slr_core::vehicle::*;
fn main() {
    let cat = Catalog::fallback();
    let m = cat.car("Baiern/DevilSport").unwrap();
    let mut v = Vehicle::new(
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
    v.place([0.0, 0.0], 0.0);
    for _ in 0..60 * 9 {
        v.step(
            &Controls {
                throttle: 1.0,
                ..Default::default()
            },
            1.0 / 60.0,
        );
    }
    println!("start kph={:.1} gear={} rpm={:.0}", v.kph(), v.gear, v.rpm);
    for i in 0..60 * 8 {
        v.step(&Controls::default(), 1.0 / 60.0);
        if i % 12 == 0 {
            println!(
                "kph={:6.1} g={} rpm={:5.0} w=[{}] k=[{}] x={:.2}",
                v.kph(),
                v.gear,
                v.rpm,
                v.wheels
                    .iter()
                    .map(|w| format!("{:6.1}", w.omega))
                    .collect::<Vec<_>>()
                    .join(","),
                v.wheels
                    .iter()
                    .map(|w| format!("{:5.2}", w.slip_ratio))
                    .collect::<Vec<_>>()
                    .join(","),
                v.pos[0]
            );
        }
    }
}
