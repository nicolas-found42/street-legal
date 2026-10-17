//! Engine torque model.
//!
//! The shipped game computes torque from a per-part thermodynamic model scripted in `TUFA`
//! classes (bore, valve diameter, cam timing...). That model is not yet executable here, so
//! each stock engine is described by a peak torque / peak rpm / redline triple chosen to
//! match the displacement encoded in the part names, and shaped with a generic curve.
//! See `docs/fidelity.md` (Mechanical simulation).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSpec {
    /// Part alias as it appears in `parts/engines/*/scripts/<alias>_block.cfg`.
    pub id: String,
    pub display: String,
    pub displacement_l: f32,
    pub peak_torque_nm: f32,
    pub peak_torque_rpm: f32,
    pub redline_rpm: f32,
    pub idle_rpm: f32,
    pub mass_kg: f32,
    /// Crank + flywheel inertia (kg m^2).
    pub inertia: f32,
    /// Forced induction / tuning multiplier on the torque curve.
    pub boost: f32,
}

impl EngineSpec {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        display: &str,
        litres: f32,
        nm: f32,
        nm_rpm: f32,
        redline: f32,
        mass: f32,
    ) -> Self {
        Self {
            id: id.into(),
            display: display.into(),
            displacement_l: litres,
            peak_torque_nm: nm,
            peak_torque_rpm: nm_rpm,
            redline_rpm: redline,
            idle_rpm: 900.0,
            mass_kg: mass,
            inertia: 0.18 + 0.05 * litres,
            boost: 1.0,
        }
    }

    /// Full-throttle torque (Nm) at `rpm`, before wear.
    pub fn full_throttle_torque(&self, rpm: f32) -> f32 {
        let x = (rpm / self.peak_torque_rpm).max(0.0);
        let shape = if x < 1.0 {
            // Rises from ~55% at idle-ish rpm to peak.
            0.55 + 0.45 * (1.0 - (1.0 - x).powi(2))
        } else {
            // Falls after peak, steeper past the redline.
            (1.0 - 0.55 * (x - 1.0).powf(1.5)).max(0.15)
        };
        self.peak_torque_nm * self.boost * shape * if rpm < 400.0 { rpm / 400.0 } else { 1.0 }
    }

    /// Net torque at the crank for a throttle position, including friction/engine braking.
    pub fn net_torque(&self, rpm: f32, throttle: f32, health: f32) -> f32 {
        if rpm >= self.redline_rpm {
            return -self.friction_torque(rpm);
        }
        let drive = self.full_throttle_torque(rpm)
            * throttle.clamp(0.0, 1.0)
            * (0.6 + 0.4 * health.clamp(0.0, 1.0));
        drive - self.friction_torque(rpm) * (1.0 - 0.6 * throttle.clamp(0.0, 1.0))
    }

    pub fn friction_torque(&self, rpm: f32) -> f32 {
        self.peak_torque_nm * 0.12 * (0.3 + 0.7 * rpm / self.redline_rpm)
    }

    /// Peak power in kW over the rpm range.
    pub fn peak_power_kw(&self) -> f32 {
        (0..=140)
            .map(|i| {
                let rpm = self.idle_rpm + (self.redline_rpm - self.idle_rpm) * i as f32 / 140.0;
                self.full_throttle_torque(rpm) * rpm * std::f32::consts::PI / 30.0 / 1000.0
            })
            .fold(0.0, f32::max)
    }
}

/// Stock engine table for the 14 engine blocks shipped in `parts/engines`.
pub fn stock_engines() -> Vec<EngineSpec> {
    vec![
        EngineSpec::new(
            "Baiern_Kraftwerk_2_5",
            "Kraftwerk 2.5",
            2.5,
            250.0,
            4300.0,
            6800.0,
            150.0,
        ),
        EngineSpec::new(
            "Baiern_Devils_6SFi_3_6",
            "Devil's 6SFi 3.6",
            3.6,
            380.0,
            4800.0,
            7000.0,
            185.0,
        ),
        EngineSpec::new(
            "Duhen_D15V",
            "Duhen D15V 1.5",
            1.5,
            145.0,
            5200.0,
            7600.0,
            105.0,
        ),
        EngineSpec::new(
            "Duhen_D18V",
            "Duhen D18V 1.8",
            1.8,
            175.0,
            5400.0,
            7800.0,
            112.0,
        ),
        EngineSpec::new(
            "Duhen_D22V",
            "Duhen D22V 2.2",
            2.2,
            215.0,
            5100.0,
            7200.0,
            125.0,
        ),
        EngineSpec::new(
            "Einvagen_110_GT",
            "Einvagen 110 GT",
            2.0,
            210.0,
            4600.0,
            6600.0,
            140.0,
        ),
        EngineSpec::new(
            "Einvagen_PerPro_GTA",
            "Einvagen PerPro GTA",
            2.8,
            300.0,
            4500.0,
            6800.0,
            160.0,
        ),
        EngineSpec::new(
            "Emer_StreetGT",
            "Emer Street GT",
            3.0,
            340.0,
            5000.0,
            7000.0,
            160.0,
        ),
        EngineSpec::new(
            "Emer_MotorSport_GT2",
            "Emer MotorSport GT2",
            3.2,
            400.0,
            5600.0,
            8200.0,
            150.0,
        ),
        EngineSpec::new(
            "Ishima_IshiBox",
            "Ishima IshiBox",
            1.3,
            115.0,
            4200.0,
            6400.0,
            95.0,
        ),
        EngineSpec::new("MC_block", "MC V8", 4.6, 500.0, 3800.0, 6200.0, 230.0),
        EngineSpec::new(
            "SuperDuty_5_4",
            "SuperDuty 5.4",
            5.4,
            560.0,
            3800.0,
            6000.0,
            270.0,
        ),
        EngineSpec::new(
            "Shimutshibu_RC_B94S200",
            "Shimutshibu RC B94",
            2.0,
            330.0,
            4800.0,
            7500.0,
            135.0,
        ),
        EngineSpec::new(
            "104er_engine_block",
            "104er Flat Block",
            3.8,
            440.0,
            4400.0,
            6600.0,
            190.0,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torque_peaks_at_peak_rpm_and_falls_after() {
        let e = EngineSpec::new("t", "t", 2.5, 250.0, 4000.0, 7000.0, 150.0);
        assert!((e.full_throttle_torque(4000.0) - 250.0).abs() < 0.01);
        assert!(e.full_throttle_torque(6500.0) < e.full_throttle_torque(4000.0));
        assert!(e.full_throttle_torque(1500.0) < e.full_throttle_torque(4000.0));
    }

    #[test]
    fn rev_limiter_cuts_drive() {
        let e = EngineSpec::new("t", "t", 2.5, 250.0, 4000.0, 7000.0, 150.0);
        assert!(e.net_torque(7100.0, 1.0, 1.0) < 0.0);
    }

    #[test]
    fn damaged_engine_makes_less_power() {
        let e = EngineSpec::new("t", "t", 2.5, 250.0, 4000.0, 7000.0, 150.0);
        assert!(e.net_torque(4000.0, 1.0, 0.2) < e.net_torque(4000.0, 1.0, 1.0));
    }

    #[test]
    fn power_is_plausible() {
        for e in stock_engines() {
            let kw = e.peak_power_kw();
            assert!((60.0..600.0).contains(&kw), "{} {kw}", e.id);
        }
    }
}
