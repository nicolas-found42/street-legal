//! Wear and damage state of a car, and the performance penalties it causes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Engine health, 1.0 = new.
    pub engine: f32,
    /// Tyre wear per wheel (FL, FR, RL, RR), 0.0 = new, 1.0 = bald.
    pub tyre_wear: [f32; 4],
    /// Brake wear, 0.0 = new.
    pub brake_wear: f32,
    /// Body damage per side (front, rear, left, right), 0.0 = pristine, 1.0 = wrecked.
    pub body: [f32; 4],
    pub odometer_km: f32,
}

impl Default for Condition {
    fn default() -> Self {
        Self::new()
    }
}

impl Condition {
    pub fn new() -> Self {
        Self {
            engine: 1.0,
            tyre_wear: [0.0; 4],
            brake_wear: 0.0,
            body: [0.0; 4],
            odometer_km: 0.0,
        }
    }

    pub fn body_total(&self) -> f32 {
        self.body.iter().sum::<f32>() / 4.0
    }

    pub fn grip_scale(&self, wheel: usize) -> f32 {
        1.0 - 0.45 * self.tyre_wear[wheel].clamp(0.0, 1.0).powf(1.3)
    }

    pub fn brake_scale(&self) -> f32 {
        1.0 - 0.5 * self.brake_wear.clamp(0.0, 1.0)
    }

    /// Extra drag coefficient area from bodywork damage (m^2).
    pub fn extra_drag_area(&self) -> f32 {
        0.25 * self.body_total()
    }

    /// Steering pull from asymmetric side damage, in radians of constant steering offset.
    pub fn steer_pull(&self) -> f32 {
        (self.body[3] - self.body[2]) * 0.05
    }

    pub fn apply_impact(&mut self, speed: f32, side: usize) {
        let d = ((speed - 3.0).max(0.0) / 40.0).powf(1.2);
        self.body[side.min(3)] = (self.body[side.min(3)] + d).min(1.0);
        // Hard hits knock the front tyres' alignment and the engine mounts.
        if side == 0 {
            self.engine = (self.engine - d * 0.25).max(0.05);
        }
    }

    pub fn apply_overrev(&mut self, dt: f32) {
        self.engine = (self.engine - 0.02 * dt).max(0.05);
    }

    pub fn repair_cost(&self) -> f32 {
        self.body.iter().sum::<f32>() * 900.0
            + (1.0 - self.engine) * 1800.0
            + self.brake_wear * 250.0
    }

    pub fn tyre_cost(&self, per_tyre: f32) -> f32 {
        self.tyre_wear.iter().filter(|w| **w > 0.02).count() as f32 * per_tyre
    }

    pub fn repair_all(&mut self) {
        self.engine = 1.0;
        self.body = [0.0; 4];
        self.brake_wear = 0.0;
    }

    pub fn replace_tyres(&mut self) {
        self.tyre_wear = [0.0; 4];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_car_has_no_penalties() {
        let c = Condition::new();
        assert_eq!(c.grip_scale(0), 1.0);
        assert_eq!(c.brake_scale(), 1.0);
        assert_eq!(c.repair_cost(), 0.0);
    }

    #[test]
    fn crash_damages_and_costs_money() {
        let mut c = Condition::new();
        c.apply_impact(25.0, 0);
        assert!(c.body[0] > 0.2);
        assert!(c.engine < 1.0);
        assert!(c.repair_cost() > 100.0);
        c.repair_all();
        assert_eq!(c, Condition::new());
    }

    #[test]
    fn low_speed_bump_is_free() {
        let mut c = Condition::new();
        c.apply_impact(2.0, 2);
        assert_eq!(c.body_total(), 0.0);
    }

    #[test]
    fn worn_tyres_grip_less() {
        let mut c = Condition::new();
        c.tyre_wear[2] = 0.9;
        assert!(c.grip_scale(2) < c.grip_scale(0));
    }
}
