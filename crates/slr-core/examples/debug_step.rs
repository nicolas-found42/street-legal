use slr_core::catalog::Catalog;
use slr_core::vehicle::*;
fn main() {
    let cat = Catalog::fallback();
    let m = cat.car("Baiern/DevilSport").unwrap();
    for &(speed, steer) in &[(20.0f32, 0.06f32), (40.0, 0.04), (40.0, 0.12)] {
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
        v.vel = [0.0, speed];
        for w in &mut v.wheels {
            w.omega = speed / w.radius;
        }
        v.gear = 4;
        v.rpm = 4000.0;
        println!("--- speed {speed} steer {steer}");
        for i in 0..60 * 6 {
            let st = if i < 60 { 0.0 } else { steer };
            // hold speed with a P throttle
            let thr = ((speed - v.speed()) * 0.3 + 0.12).clamp(0.0, 1.0);
            v.step(
                &Controls {
                    throttle: thr,
                    steer: st,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            if i % 30 == 0 {
                println!(
                    "t={:4.1} yaw={:6.3} ay={:5.2}g lat={:6.2} aF={:6.3} aR={:6.3} spd={:5.1} g={}",
                    i as f32 / 60.0,
                    v.yaw_rate,
                    v.lateral_g(),
                    v.pos[0],
                    v.wheels[0].slip_angle,
                    v.wheels[2].slip_angle,
                    v.speed(),
                    v.gear
                );
            }
        }
    }
}
