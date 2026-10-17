//! Headline performance figures from a headless run of the vehicle simulation.

use crate::vehicle::{Controls, Vehicle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perf {
    pub zero_to_100_s: Option<f32>,
    pub quarter_mile_s: Option<f32>,
    pub trap_kph: f32,
    pub top_kph: f32,
    pub peak_kw: f32,
    pub mass_kg: f32,
}

/// Full-throttle run on a straight: 0-100 km/h, 402 m time and trap speed, top speed reached.
pub fn measure(template: &Vehicle) -> Perf {
    let mut v = template.clone();
    v.setup.automatic = true;
    v.place([0.0, 0.0], 0.0);
    let mut t = 0.0;
    let dt = 1.0 / 60.0;
    let (mut z100, mut q, mut trap, mut top) = (None, None, 0.0f32, 0.0f32);
    let c = Controls {
        throttle: 1.0,
        ..Default::default()
    };
    while t < 45.0 {
        v.step(&c, dt);
        t += dt;
        top = top.max(v.kph());
        if z100.is_none() && v.kph() >= 100.0 {
            z100 = Some(t);
        }
        if q.is_none() && v.pos[1] >= 402.3 {
            q = Some(t);
            trap = v.kph();
        }
    }
    Perf {
        zero_to_100_s: z100,
        quarter_mile_s: q,
        trap_kph: trap,
        top_kph: top,
        peak_kw: template.setup.engine.peak_power_kw()
            * template.cond.engine.clamp(0.0, 1.0).max(0.7),
        mass_kg: template.mass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::vehicle::Setup;

    fn coupe(boost: f32, final_drive: f32) -> Vehicle {
        let cat = Catalog::fallback();
        let m = cat.car("Baiern/CoupeSport").unwrap();
        let mut e = cat.engine(&m.stock_engine).unwrap().clone();
        e.boost = boost;
        Vehicle::new(
            &m.cfg,
            Setup {
                engine: e,
                tyre_front: cat.stock_tyre().clone(),
                tyre_rear: cat.stock_tyre().clone(),
                final_drive_scale: final_drive,
                brake_bias: 0.6,
                wing: 0.3,
                lsd: 0.0,
                automatic: true,
            },
        )
    }

    #[test]
    fn more_power_is_quicker() {
        let a = measure(&coupe(1.0, 1.0));
        let b = measure(&coupe(1.28, 1.0));
        assert!(b.zero_to_100_s.unwrap() < a.zero_to_100_s.unwrap());
        assert!(b.quarter_mile_s.unwrap() < a.quarter_mile_s.unwrap());
    }

    #[test]
    fn taller_final_drive_raises_top_speed_and_lowers_acceleration() {
        let short = measure(&coupe(1.0, 1.25));
        let tall = measure(&coupe(1.0, 0.8));
        assert!(
            tall.top_kph > short.top_kph,
            "{} vs {}",
            tall.top_kph,
            short.top_kph
        );
        assert!(tall.zero_to_100_s.unwrap() > short.zero_to_100_s.unwrap());
    }
}
