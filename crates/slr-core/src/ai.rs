//! AI driver: pure-pursuit steering along the track centreline (optionally in a lane) and a
//! curvature-limited speed profile with look-ahead braking.

use crate::track::Track;
use crate::vehicle::{Controls, Vehicle};
use crate::G;

#[derive(Debug, Clone)]
pub struct Driver {
    /// 0..1: fraction of the car's grip the driver is willing to use.
    pub skill: f32,
    /// Lateral offset from the centreline in metres, positive to the right.
    pub lane: f32,
    pub drag: bool,
    pub launched: bool,
    /// Speed the driver is currently aiming for (m/s), exposed for debugging and HUDs.
    pub target_speed: f32,
}

impl Driver {
    pub fn new(skill: f32, lane: f32, drag: bool) -> Self {
        Self {
            skill: skill.clamp(0.1, 1.0),
            lane,
            drag,
            launched: false,
            target_speed: 0.0,
        }
    }

    pub fn drive(&mut self, v: &Vehicle, track: &Track, go: bool) -> Controls {
        if !go {
            return Controls {
                brake: 1.0,
                ..Default::default()
            };
        }
        let pr = track.project(v.pos);
        let speed = v.speed().max(0.1);
        let lookahead = (6.0 + 0.55 * speed).min(60.0);
        let (target_pt, _) = track.at(pr.s + lookahead);
        let (_, t_here) = track.at(pr.s + lookahead);
        let right = [t_here[1], -t_here[0]];
        let aim = [
            target_pt[0] + right[0] * self.lane,
            target_pt[1] + right[1] * self.lane,
        ];
        let to = [aim[0] - v.pos[0], aim[1] - v.pos[1]];
        let desired_heading = Track::heading_of(to);
        let mut err = desired_heading - v.heading;
        while err > std::f32::consts::PI {
            err -= 2.0 * std::f32::consts::PI;
        }
        while err < -std::f32::consts::PI {
            err += 2.0 * std::f32::consts::PI;
        }
        // Pure pursuit: wheel angle (CCW positive) from the heading error.
        let delta = (2.0 * v.wheelbase * err.sin() / lookahead.max(1.0)).atan();
        let body_v = v.body_velocity();
        // Counter-steer against sideslip so the AI recovers from slides.
        let slip_damp = 0.35 * (body_v[0] / (body_v[1].abs() + 4.0)).atan();
        let max_delta = v.max_steer / (1.0 + (speed / 28.0).powi(2));
        let steer = (-(delta - slip_damp) / max_delta).clamp(-1.0, 1.0);

        // Speed profile.
        let mu = 0.78 * self.skill;
        let a_brake = 0.7 * G * (0.6 + 0.4 * self.skill);
        let mut target = if self.drag { 1000.0 } else { 90.0 };
        let mut d = 0.0;
        while d <= 160.0 {
            let k = track.curvature(pr.s + d, 14.0).abs().max(1e-4);
            let v_corner = (mu * G / k).sqrt();
            let allowed = (v_corner * v_corner + 2.0 * a_brake * d).sqrt();
            target = f32::min(target, allowed);
            d += 8.0;
        }
        self.target_speed = target;
        let mut c = Controls {
            steer,
            ..Default::default()
        };
        let diff = target - speed;
        if diff > 0.0 {
            c.throttle = (diff * 0.4 + 0.15).clamp(0.0, 1.0)
                * if self.drag { 1.0 } else { self.skill.max(0.6) };
        } else {
            // Trail-braking limit: the friction circle leaves less brake when cornering hard.
            let lat = v.lateral_g().abs();
            c.brake = (-diff * 0.25).clamp(0.0, 1.0) * (1.0 - (lat / 1.0).powi(2)).clamp(0.0, 1.0);
        }
        // Traction control: back off when the driven wheels spin.
        let spin = v
            .wheels
            .iter()
            .filter(|w| w.driven)
            .map(|w| w.slip_ratio)
            .fold(0.0, f32::max);
        if spin > 0.22 {
            c.throttle *= (1.0 - (spin - 0.22) * 4.0).clamp(0.3, 1.0);
        }
        c
    }
}
