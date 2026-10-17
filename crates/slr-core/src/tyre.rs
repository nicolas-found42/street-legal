//! Tyre catalog (parsed from `tyre_<width>_<aspect>_<rim>_<rim width>` part names) and the
//! slip-vector friction model.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TyreSpec {
    pub id: String,
    pub width_mm: f32,
    pub aspect: f32,
    pub rim_in: f32,
    pub rim_width_in: f32,
}

impl TyreSpec {
    /// Parse `tyre_205_50_16_8_0` (the trailing `_LOD_CATALOG_GARAGE` is stripped by callers).
    pub fn from_part_name(name: &str) -> Option<Self> {
        let n = name.strip_prefix("tyre_")?;
        let n = n.strip_suffix("_LOD_CATALOG_GARAGE").unwrap_or(n);
        let p: Vec<&str> = n.split('_').collect();
        if p.len() != 5 {
            return None;
        }
        Some(Self {
            id: format!("tyre_{n}"),
            width_mm: p[0].parse().ok()?,
            aspect: p[1].parse().ok()?,
            rim_in: p[2].parse().ok()?,
            rim_width_in: format!("{}.{}", p[3], p[4]).parse().ok()?,
        })
    }

    pub fn label(&self) -> String {
        format!(
            "{:.0}/{:.0} R{:.0}",
            self.width_mm, self.aspect, self.rim_in
        )
    }

    pub fn radius_m(&self) -> f32 {
        (self.rim_in * 25.4 / 2.0 + self.width_mm * self.aspect / 100.0) / 1000.0
    }

    /// Peak friction coefficient: wider contact patches grip more, with diminishing returns.
    pub fn mu(&self) -> f32 {
        (1.05 * (self.width_mm / 205.0).powf(0.28)).clamp(0.8, 1.45)
    }

    pub fn mass_kg(&self) -> f32 {
        7.0 + self.width_mm * self.aspect / 100.0 * 0.012 + self.rim_in * 0.45
    }

    pub fn price(&self) -> f32 {
        40.0 + self.width_mm * 0.9 + self.rim_in * 12.0
    }
}

/// Peak slip ratio and slip angle at which the friction curve peaks.
pub const SLIP_RATIO_PEAK: f32 = 0.12;
pub const SLIP_ANGLE_PEAK: f32 = 0.14;

/// Normalised friction curve of the slip-vector magnitude `sigma` (1.0 = peak slip).
pub fn friction_curve(sigma: f32) -> f32 {
    const C: f32 = 1.4;
    const B: f32 = 2.1;
    (C * (B * sigma).atan()).sin().max(0.0)
}

/// Force on the contact patch in the wheel frame given slip ratio and slip angle.
/// Returns `(longitudinal, lateral)` as fractions of `mu * Fz` (signed).
pub fn combined_slip_force(slip_ratio: f32, slip_angle: f32) -> (f32, f32) {
    let sx = slip_ratio / SLIP_RATIO_PEAK;
    let sy = slip_angle / SLIP_ANGLE_PEAK;
    let sigma = sx.hypot(sy);
    if sigma < 1e-6 {
        return (0.0, 0.0);
    }
    let f = friction_curve(sigma);
    (f * sx / sigma, -f * sy / sigma)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        let t = TyreSpec::from_part_name("tyre_205_50_16_8_0_LOD_CATALOG_GARAGE").unwrap();
        assert_eq!(
            (t.width_mm, t.aspect, t.rim_in, t.rim_width_in),
            (205.0, 50.0, 16.0, 8.0)
        );
        assert!((t.radius_m() - 0.3053).abs() < 0.001);
        assert!(TyreSpec::from_part_name("wheel_x").is_none());
    }

    #[test]
    fn wider_tyres_grip_more() {
        let a = TyreSpec::from_part_name("tyre_195_50_16_7_5").unwrap();
        let b = TyreSpec::from_part_name("tyre_295_20_19_13_0").unwrap();
        assert!(b.mu() > a.mu());
    }

    #[test]
    fn curve_peaks_near_one_and_drops_when_sliding() {
        assert!(friction_curve(1.0) > 0.95);
        assert!(friction_curve(5.0) < friction_curve(1.0));
        assert!(friction_curve(5.0) > 0.5);
    }

    #[test]
    fn force_opposes_slip() {
        let (fx, fy) = combined_slip_force(0.05, 0.05);
        assert!(fx > 0.0);
        assert!(fy < 0.0);
        let (fx, fy) = combined_slip_force(0.0, 0.0);
        assert_eq!((fx, fy), (0.0, 0.0));
    }

    #[test]
    fn friction_circle_is_respected() {
        let (fx, fy) = combined_slip_force(0.3, 0.3);
        assert!(fx.hypot(fy) <= 1.0001);
    }
}
