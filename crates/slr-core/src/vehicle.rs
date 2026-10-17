//! Planar vehicle dynamics: four tyres with load transfer, an engine + gearbox + open/LSD
//! drivetrain, aerodynamic drag/downforce, brakes and condition-driven degradation.
//!
//! Conventions: world x east, y north; heading 0 faces +y and grows counter-clockwise.
//! Body frame: x to the right, y forward (the shipped cfg uses -z as forward; wheel
//! positions are converted on load). Steering input > 0 turns right.

use crate::condition::Condition;
use crate::engine::EngineSpec;
use crate::tyre::{combined_slip_force, TyreSpec};
use crate::G;
use serde::{Deserialize, Serialize};
use slr_formats::cfg::VehicleCfg;

pub const SUBSTEP: f32 = 1.0 / 360.0;
const AIR_DENSITY: f32 = 1.2;
const CG_HEIGHT: f32 = 0.5;
const DRIVELINE_EFFICIENCY: f32 = 0.9;

/// Player-adjustable configuration: what the garage edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub engine: EngineSpec,
    pub tyre_front: TyreSpec,
    pub tyre_rear: TyreSpec,
    /// Multiplies the model's stock `rearendratio`.
    pub final_drive_scale: f32,
    /// Fraction of brake torque on the front axle.
    pub brake_bias: f32,
    /// 0..1 rear wing angle.
    pub wing: f32,
    /// Limited-slip differential lock, 0 = open.
    pub lsd: f32,
    pub automatic: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Controls {
    pub throttle: f32,
    pub brake: f32,
    /// -1 left .. +1 right.
    pub steer: f32,
    pub handbrake: bool,
    /// Manual gearbox shift requests for this frame: +1 up, -1 down.
    pub shift: i8,
}

#[derive(Debug, Clone)]
pub struct Wheel {
    /// Body-frame position (x right, y forward).
    pub pos: [f32; 2],
    pub radius: f32,
    pub inertia: f32,
    pub steered: bool,
    pub driven: bool,
    pub handbrake: bool,
    pub front: bool,
    pub omega: f32,
    pub fz: f32,
    pub slip_ratio: f32,
    pub slip_angle: f32,
}

#[derive(Debug, Clone)]
pub struct Vehicle {
    pub setup: Setup,
    pub cond: Condition,
    pub wheels: Vec<Wheel>,
    pub mass: f32,
    pub yaw_inertia: f32,
    pub wheelbase: f32,
    pub track: f32,
    /// Distance CG -> front axle and CG -> rear axle (m).
    pub a: f32,
    pub b: f32,
    pub gear_ratios: Vec<f32>,
    pub reverse_ratio: f32,
    pub final_drive: f32,
    pub max_steer: f32,
    pub brake_torque_ref: f32,
    pub cd_area: f32,
    // dynamic state
    pub pos: [f32; 2],
    pub heading: f32,
    pub vel: [f32; 2],
    pub yaw_rate: f32,
    pub steer_angle: f32,
    pub gear: i8,
    pub rpm: f32,
    pub shift_timer: f32,
    pub reverse_timer: f32,
    pub accel_body: [f32; 2],
    pub limiter: bool,
    pub impact_count: u32,
}

impl Vehicle {
    pub fn new(cfg: &VehicleCfg, setup: Setup) -> Vehicle {
        let mut wheels: Vec<Wheel> = cfg
            .wheels
            .iter()
            .map(|w| {
                let front = -w.position[2] > 0.0;
                let tyre = if front {
                    &setup.tyre_front
                } else {
                    &setup.tyre_rear
                };
                let r = tyre.radius_m();
                let m = tyre.mass_kg() + w.wheel_mass;
                Wheel {
                    pos: [w.position[0], -w.position[2]],
                    radius: r,
                    inertia: 0.5 * m * r * r * 1.1,
                    steered: w.steered(),
                    driven: w.driven(),
                    handbrake: w.handbraked(),
                    front,
                    omega: 0.0,
                    fz: 0.0,
                    slip_ratio: 0.0,
                    slip_angle: 0.0,
                }
            })
            .collect();
        // Order wheels FL, FR, RL, RR so per-wheel condition indexes are stable.
        wheels.sort_by(|p, q| {
            let kp = (!p.front as u8, p.pos[0] > 0.0);
            let kq = (!q.front as u8, q.pos[0] > 0.0);
            kp.cmp(&kq)
        });
        let wheel_mass: f32 = wheels
            .iter()
            .map(|w| w.inertia * 2.0 / (w.radius * w.radius))
            .sum();
        let mass = cfg.body_mass + setup.engine.mass_kg + 140.0 + 75.0 + wheel_mass;
        let front_y = wheels
            .iter()
            .filter(|w| w.front)
            .map(|w| w.pos[1])
            .sum::<f32>()
            / 2.0;
        let rear_y = wheels
            .iter()
            .filter(|w| !w.front)
            .map(|w| w.pos[1])
            .sum::<f32>()
            / 2.0;
        let wheelbase = front_y - rear_y;
        let track = (wheels[1].pos[0] - wheels[0].pos[0]).abs();
        // Front-engined layout: 55% of the weight on the front axle.
        let a = (1.0 - 0.55) * wheelbase;
        let b = wheelbase - a;
        // Re-express wheel positions relative to the centre of gravity (the cfg origin is not it).
        let cg_y = front_y - a;
        for w in &mut wheels {
            w.pos[1] -= cg_y;
        }
        let final_drive = cfg.rearend_ratio * setup.final_drive_scale;
        Vehicle {
            cond: Condition::new(),
            wheels,
            mass,
            yaw_inertia: mass * (0.32 * wheelbase).powi(2) * 2.0,
            wheelbase,
            track,
            a,
            b,
            gear_ratios: cfg.gear_ratios.clone(),
            reverse_ratio: cfg.reverse_ratio,
            final_drive,
            max_steer: cfg.maxsteer[0],
            brake_torque_ref: cfg.brakepower[0],
            cd_area: 0.65,
            setup,
            pos: [0.0, 0.0],
            heading: 0.0,
            vel: [0.0, 0.0],
            yaw_rate: 0.0,
            steer_angle: 0.0,
            gear: 1,
            rpm: 900.0,
            shift_timer: 0.0,
            reverse_timer: 0.0,
            accel_body: [0.0, 0.0],
            limiter: false,
            impact_count: 0,
        }
    }

    pub fn place(&mut self, pos: [f32; 2], heading: f32) {
        self.pos = pos;
        self.heading = heading;
        self.vel = [0.0; 2];
        self.yaw_rate = 0.0;
        self.steer_angle = 0.0;
        self.gear = 1;
        self.rpm = self.setup.engine.idle_rpm;
        for w in &mut self.wheels {
            w.omega = 0.0;
        }
    }

    pub fn forward(&self) -> [f32; 2] {
        [-self.heading.sin(), self.heading.cos()]
    }

    pub fn right(&self) -> [f32; 2] {
        [self.heading.cos(), self.heading.sin()]
    }

    /// Velocity in the body frame: (right, forward).
    pub fn body_velocity(&self) -> [f32; 2] {
        let (r, f) = (self.right(), self.forward());
        [
            self.vel[0] * r[0] + self.vel[1] * r[1],
            self.vel[0] * f[0] + self.vel[1] * f[1],
        ]
    }

    pub fn speed(&self) -> f32 {
        self.vel[0].hypot(self.vel[1])
    }

    pub fn forward_speed(&self) -> f32 {
        self.body_velocity()[1]
    }

    pub fn current_ratio(&self) -> f32 {
        match self.gear {
            0 => 0.0,
            g if g < 0 => self.reverse_ratio * self.final_drive,
            g => self.gear_ratios.get(g as usize - 1).copied().unwrap_or(1.0) * self.final_drive,
        }
    }

    pub fn gear_count(&self) -> i8 {
        self.gear_ratios.len() as i8
    }

    fn wheel_load_static(&self) -> (f32, f32) {
        let w = self.mass * G;
        (w * self.b / self.wheelbase, w * self.a / self.wheelbase)
    }

    /// Advance the simulation by `dt`, internally substepping at [`SUBSTEP`].
    pub fn step(&mut self, c: &Controls, dt: f32) {
        let n = ((dt / SUBSTEP).ceil() as usize).max(1);
        let h = dt / n as f32;
        let mut ctl = *c;
        self.process_gearbox_inputs(&mut ctl, dt);
        for _ in 0..n {
            self.substep(&ctl, h);
        }
        self.cond.odometer_km += self.speed() * dt / 1000.0;
    }

    fn process_gearbox_inputs(&mut self, c: &mut Controls, dt: f32) {
        self.shift_timer = (self.shift_timer - dt).max(0.0);
        let vf = self.forward_speed();
        if self.setup.automatic {
            if self.gear < 0 {
                // In reverse the brake pedal drives and the throttle pedal brakes.
                if c.throttle > 0.5 && vf > -0.5 {
                    self.gear = 1;
                }
            } else {
                if vf.abs() < 1.0 && c.brake > 0.5 && c.throttle < 0.1 {
                    self.reverse_timer += dt;
                    if self.reverse_timer > 0.35 {
                        self.gear = -1;
                        self.reverse_timer = 0.0;
                    }
                } else {
                    self.reverse_timer = 0.0;
                }
                if self.shift_timer <= 0.0 && self.gear >= 1 {
                    let redline = self.setup.engine.redline_rpm;
                    let ratio_of = |g: i8| self.gear_ratios[g as usize - 1];
                    if self.rpm > redline * 0.9 && self.gear < self.gear_count() {
                        self.gear += 1;
                        self.shift_timer = 0.35;
                    } else if self.gear > 1 {
                        // Only drop a gear when the engine would not be over-revved afterwards.
                        let rpm_after = self.rpm * ratio_of(self.gear - 1) / ratio_of(self.gear);
                        let kickdown = c.throttle > 0.9 && self.rpm < redline * 0.45;
                        if (self.rpm < redline * 0.33 || kickdown) && rpm_after < redline * 0.82 {
                            self.gear -= 1;
                            self.shift_timer = 0.35;
                        }
                    }
                }
            }
        } else if c.shift != 0 && self.shift_timer <= 0.0 {
            let next = self.gear + c.shift;
            let allowed = -1..=self.gear_count();
            if allowed.contains(&next) {
                self.gear = next;
                self.shift_timer = 0.25;
            }
        }
        if self.gear < 0 {
            std::mem::swap(&mut c.throttle, &mut c.brake);
        }
    }

    fn speed_limited_steer(&self, v: f32) -> f32 {
        self.max_steer / (1.0 + (v / 28.0).powi(2))
    }

    fn substep(&mut self, c: &Controls, dt: f32) {
        let engine = self.setup.engine.clone();
        let speed = self.speed();
        // Steering with rate limit, speed-sensitive lock and damage pull.
        let target =
            -c.steer.clamp(-1.0, 1.0) * self.speed_limited_steer(speed) + self.cond.steer_pull();
        let rate = 3.2 * dt;
        self.steer_angle += (target - self.steer_angle).clamp(-rate, rate);

        let body_v = self.body_velocity();
        let (sin_d, cos_d) = self.steer_angle.sin_cos();

        // ---- vertical loads with (filtered) load transfer ----
        let (fz_f_static, fz_r_static) = self.wheel_load_static();
        let v2 = speed * speed;
        let downforce = 0.5 * AIR_DENSITY * (0.15 + 0.9 * self.setup.wing) * v2;
        let (ax, ay) = (self.accel_body[0], self.accel_body[1]);
        let long_t = self.mass * ay * CG_HEIGHT / self.wheelbase;
        let fz_front = (fz_f_static + 0.4 * downforce - long_t).max(0.0);
        let fz_rear = (fz_r_static + 0.6 * downforce + long_t).max(0.0);
        let lat_t = self.mass * ax * CG_HEIGHT / self.track;
        let load = |front: bool, left: bool| -> f32 {
            let axle = if front { fz_front } else { fz_rear };
            let share = if front { 0.55 } else { 0.45 };
            let t = lat_t * share;
            (axle / 2.0 + if left { t } else { -t }).max(0.0)
        };
        for w in &mut self.wheels {
            w.fz = load(w.front, w.pos[0] < 0.0);
        }

        // ---- drivetrain ----
        let ratio = self.current_ratio();
        let throttle = c.throttle.clamp(0.0, 1.0);
        let driven: Vec<usize> = (0..self.wheels.len())
            .filter(|&i| self.wheels[i].driven)
            .collect();
        let avg_omega = if driven.is_empty() {
            0.0
        } else {
            driven.iter().map(|&i| self.wheels[i].omega).sum::<f32>() / driven.len() as f32
        };
        let rpm_in = avg_omega * ratio * 30.0 / std::f32::consts::PI;
        let in_gear = ratio != 0.0 && self.shift_timer < 0.15;
        let clutch_floor =
            engine.idle_rpm + throttle * (engine.peak_torque_rpm * 0.55 - engine.idle_rpm).max(0.0);
        let locked = in_gear && rpm_in >= clutch_floor;
        if !in_gear {
            // Free-revving engine: drift towards a throttle-dependent speed.
            let target_rpm = engine.idle_rpm + throttle * (engine.redline_rpm - engine.idle_rpm);
            self.rpm += (target_rpm - self.rpm) * (6.0 * dt).min(1.0);
        } else if locked {
            self.rpm = rpm_in.max(engine.idle_rpm * 0.6);
        } else {
            self.rpm += (clutch_floor - self.rpm) * (20.0 * dt).min(1.0);
        }
        self.limiter = self.rpm >= engine.redline_rpm;
        if self.rpm >= engine.redline_rpm * 0.995 && throttle > 0.5 {
            self.cond.apply_overrev(dt);
        }
        let t_engine = engine.net_torque(self.rpm, throttle, self.cond.engine);
        let clutch_cap = 1.6 * engine.peak_torque_nm * engine.boost;
        let t_in = if !in_gear {
            0.0
        } else if locked {
            t_engine.clamp(-clutch_cap, clutch_cap)
        } else {
            // A slipping clutch can only push; the idle governor keeps the engine from stalling.
            t_engine.clamp(0.0, clutch_cap)
        };
        let t_total = t_in * ratio * DRIVELINE_EFFICIENCY;
        let eff_inertia_extra = if locked && !driven.is_empty() {
            engine.inertia * ratio * ratio / driven.len() as f32
        } else {
            0.0
        };

        // ---- per-wheel tyre forces ----
        let drive_split = driven.len().max(1) as f32;
        let mut f_body = [0.0f32; 2];
        let mut yaw_moment = 0.0;
        let brake_in = c.brake.clamp(0.0, 1.0) * self.cond.brake_scale();
        let mut torques = vec![0.0f32; self.wheels.len()];
        // Differential torque split between driven wheels (LSD biases to the slower wheel).
        if driven.len() == 2 {
            let (i, j) = (driven[0], driven[1]);
            let dw = self.wheels[i].omega - self.wheels[j].omega;
            let bias = (self.setup.lsd * (dw / 8.0).clamp(-1.0, 1.0)).clamp(-0.5, 0.5);
            torques[i] = t_total * (0.5 - bias);
            torques[j] = t_total * (0.5 + bias);
        } else {
            for &i in &driven {
                torques[i] = t_total / drive_split;
            }
        }
        let mut wear_inc = [0.0f32; 4];
        for (i, w) in self.wheels.iter_mut().enumerate() {
            let (wsin, wcos) = if w.steered {
                (sin_d, cos_d)
            } else {
                (0.0, 1.0)
            };
            let wvx = body_v[0] - self.yaw_rate * w.pos[1];
            let wvy = body_v[1] + self.yaw_rate * w.pos[0];
            let v_long = -wsin * wvx + wcos * wvy;
            let v_lat = wcos * wvx + wsin * wvy;
            let mu = self.setup.tyre_front.mu();
            let mu = if w.front {
                mu
            } else {
                self.setup.tyre_rear.mu()
            } * self.cond.grip_scale(i);
            let v_ref = v_long.abs().max(3.0);
            let force = |omega: f32| -> (f32, f32) {
                let kappa = (omega * w.radius - v_long) / v_ref;
                let alpha = (v_lat / v_ref).atan();
                let (fx, fy) = combined_slip_force(kappa, alpha);
                (fx * mu * w.fz, fy * mu * w.fz)
            };
            let (fx, fy) = force(w.omega);
            let (fx2, _) = force(w.omega + 0.5);
            let dfx_domega = (fx2 - fx) / 0.5;
            w.slip_ratio = (w.omega * w.radius - v_long) / v_ref;
            w.slip_angle = (v_lat / v_ref).atan();

            // Wheel spin: semi-implicit with the tyre's local stiffness for stability.
            let mut t = torques[i] - fx * w.radius;
            let i_eff = w.inertia + if w.driven { eff_inertia_extra } else { 0.0 };
            let denom = i_eff + dt * w.radius * dfx_domega.max(0.0);
            let mut omega = w.omega + dt * t / denom;
            // Brakes: opposing torque clamped so they stop the wheel rather than reverse it.
            let axle_share = if w.front {
                self.setup.brake_bias
            } else {
                1.0 - self.setup.brake_bias
            };
            let mut bt = brake_in * self.brake_torque_ref * 2.0 * axle_share;
            if w.handbrake && c.handbrake {
                bt = bt.max(self.brake_torque_ref * 1.6);
            }
            let dw = dt * bt / i_eff;
            if omega.abs() <= dw {
                omega = 0.0;
            } else {
                omega -= dw * omega.signum();
            }
            t = 0.0;
            let _ = t;
            w.omega = omega;
            // Brake force from sticking wheels is carried by the tyre force above next step.

            // Forces back to the body frame.
            let fbx = -wsin * fx + wcos * fy;
            let fby = wcos * fx + wsin * fy;
            f_body[0] += fbx;
            f_body[1] += fby;
            yaw_moment += w.pos[0] * fby - w.pos[1] * fbx;
            wear_inc[i] = (fx.abs() * (w.omega * w.radius - v_long).abs() + fy.abs() * v_lat.abs())
                * dt
                * 4.0e-8;
        }
        for (wear, inc) in self.cond.tyre_wear.iter_mut().zip(wear_inc.iter()).take(4) {
            *wear = (*wear + inc).min(1.0);
        }
        self.cond.brake_wear = (self.cond.brake_wear + brake_in * speed * dt * 1.5e-6).min(1.0);

        // ---- aero drag, rolling resistance ----
        let drag_area = self.cd_area + self.cond.extra_drag_area() + 0.12 * self.setup.wing;
        let drag = 0.5 * AIR_DENSITY * drag_area * speed;
        let rolling = 0.013 * self.mass * G * (speed / 0.5).tanh() / speed.max(1e-3);
        let (fwd, rgt) = (self.forward(), self.right());
        let f_world = [
            f_body[0] * rgt[0] + f_body[1] * fwd[0] - (drag + rolling) * self.vel[0],
            f_body[0] * rgt[1] + f_body[1] * fwd[1] - (drag + rolling) * self.vel[1],
        ];
        // Filtered body accelerations drive load transfer next step.
        let a_new = [
            (f_body[0] - (drag + rolling) * body_v[0]) / self.mass,
            (f_body[1] - (drag + rolling) * body_v[1]) / self.mass,
        ];
        for (acc, new) in self.accel_body.iter_mut().zip(a_new.iter()) {
            *acc += (new - *acc) * (dt * 18.0).min(1.0);
        }
        self.vel[0] += f_world[0] / self.mass * dt;
        self.vel[1] += f_world[1] / self.mass * dt;
        self.yaw_rate += yaw_moment / self.yaw_inertia * dt;
        // Small yaw damping stands in for suspension and tyre carcass effects.
        self.yaw_rate *= 1.0 - 0.15 * dt;
        self.pos[0] += self.vel[0] * dt;
        self.pos[1] += self.vel[1] * dt;
        self.heading += self.yaw_rate * dt;
    }

    /// Resolve a collision: `normal` points away from the obstacle, `side` is the body side hit
    /// (0 front, 1 rear, 2 left, 3 right). Returns the impact speed along the normal.
    pub fn collide(&mut self, normal: [f32; 2], penetration: f32, side: usize) -> f32 {
        self.pos[0] += normal[0] * penetration;
        self.pos[1] += normal[1] * penetration;
        let vn = self.vel[0] * normal[0] + self.vel[1] * normal[1];
        if vn < 0.0 {
            let restitution = 0.25;
            self.vel[0] -= (1.0 + restitution) * vn * normal[0];
            self.vel[1] -= (1.0 + restitution) * vn * normal[1];
            // Scrubbing speed off along the wall and some spin.
            self.vel[0] *= 0.97;
            self.vel[1] *= 0.97;
            self.yaw_rate *= 0.8;
            self.cond.apply_impact(-vn, side);
            self.impact_count += 1;
            return -vn;
        }
        0.0
    }

    /// Kilometres per hour.
    pub fn kph(&self) -> f32 {
        self.speed() * 3.6
    }

    pub fn lateral_g(&self) -> f32 {
        self.accel_body[0] / G
    }

    pub fn longitudinal_g(&self) -> f32 {
        self.accel_body[1] / G
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn coupe(auto: bool) -> Vehicle {
        let cat = Catalog::fallback();
        let car = cat.car("Baiern/CoupeSport").unwrap();
        let setup = Setup {
            engine: cat.engine(&car.stock_engine).unwrap().clone(),
            tyre_front: cat.stock_tyre().clone(),
            tyre_rear: cat.stock_tyre().clone(),
            final_drive_scale: 1.0,
            brake_bias: 0.6,
            wing: 0.3,
            lsd: 0.0,
            automatic: auto,
        };
        let mut v = Vehicle::new(&car.cfg, setup);
        v.place([0.0, 0.0], 0.0);
        v
    }

    fn run(v: &mut Vehicle, c: Controls, seconds: f32) {
        let steps = (seconds / (1.0 / 60.0)) as usize;
        for _ in 0..steps {
            v.step(&c, 1.0 / 60.0);
        }
    }

    #[test]
    fn mass_and_balance_are_sane() {
        let v = coupe(true);
        assert!((1100.0..1500.0).contains(&v.mass), "{}", v.mass);
        assert!((v.wheelbase - 2.736).abs() < 0.01, "{}", v.wheelbase);
        assert_eq!(v.wheels.len(), 4);
        assert!(v.wheels[0].front && v.wheels[0].pos[0] < 0.0);
        assert!(!v.wheels[3].front && v.wheels[3].pos[0] > 0.0);
    }

    #[test]
    fn sitting_still_stays_still() {
        let mut v = coupe(true);
        run(&mut v, Controls::default(), 3.0);
        assert!(v.speed() < 0.05, "{}", v.speed());
    }

    #[test]
    fn full_throttle_accelerates_forward_and_in_a_straight_line() {
        let mut v = coupe(true);
        run(
            &mut v,
            Controls {
                throttle: 1.0,
                ..Default::default()
            },
            6.0,
        );
        assert!(v.kph() > 60.0, "{}", v.kph());
        assert!(v.forward_speed() > 0.0);
        assert!(v.pos[0].abs() < 1.0, "drifted {}", v.pos[0]);
        assert!(v.gear >= 2);
    }

    #[test]
    fn zero_to_hundred_is_in_a_believable_range() {
        let mut v = coupe(true);
        let mut t = 0.0;
        let c = Controls {
            throttle: 1.0,
            ..Default::default()
        };
        while v.kph() < 100.0 && t < 30.0 {
            v.step(&c, 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        assert!((4.0..14.0).contains(&t), "0-100 took {t}");
    }

    #[test]
    fn braking_stops_the_car_shorter_than_coasting() {
        let mut a = coupe(true);
        run(
            &mut a,
            Controls {
                throttle: 1.0,
                ..Default::default()
            },
            6.0,
        );
        let mut b = a.clone();
        run(
            &mut a,
            Controls {
                brake: 1.0,
                ..Default::default()
            },
            3.0,
        );
        run(&mut b, Controls::default(), 3.0);
        assert!(
            a.speed() < b.speed() * 0.5,
            "{} vs {}",
            a.speed(),
            b.speed()
        );
    }

    #[test]
    fn steering_right_turns_clockwise() {
        let mut v = coupe(true);
        run(
            &mut v,
            Controls {
                throttle: 0.6,
                ..Default::default()
            },
            4.0,
        );
        let h0 = v.heading;
        run(
            &mut v,
            Controls {
                throttle: 0.3,
                steer: 1.0,
                ..Default::default()
            },
            2.0,
        );
        assert!(v.heading < h0 - 0.2, "heading {} -> {}", h0, v.heading);
        assert!(v.pos[0] > 0.0, "should drift right of the start line");
    }

    #[test]
    fn lateral_grip_is_around_one_g() {
        let mut v = coupe(true);
        run(
            &mut v,
            Controls {
                throttle: 0.7,
                ..Default::default()
            },
            5.0,
        );
        let mut peak = 0.0f32;
        for _ in 0..180 {
            v.step(
                &Controls {
                    throttle: 0.3,
                    steer: 0.35,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            peak = peak.max(v.lateral_g().abs());
        }
        assert!((0.5..1.6).contains(&peak), "peak lateral g {peak}");
    }

    #[test]
    fn worn_tyres_corner_slower_than_new_ones() {
        let steady = |wear: f32| {
            let mut v = coupe(true);
            v.cond.tyre_wear = [wear; 4];
            run(
                &mut v,
                Controls {
                    throttle: 0.7,
                    ..Default::default()
                },
                5.0,
            );
            let mut peak = 0.0f32;
            for _ in 0..180 {
                v.step(
                    &Controls {
                        throttle: 0.3,
                        steer: 0.5,
                        ..Default::default()
                    },
                    1.0 / 60.0,
                );
                peak = peak.max(v.lateral_g().abs());
            }
            peak
        };
        assert!(steady(0.95) < steady(0.0));
    }

    #[test]
    fn engine_damage_slows_acceleration() {
        let time = |health: f32| {
            let mut v = coupe(true);
            v.cond.engine = health;
            let mut t = 0.0;
            while v.kph() < 100.0 && t < 40.0 {
                v.step(
                    &Controls {
                        throttle: 1.0,
                        ..Default::default()
                    },
                    1.0 / 60.0,
                );
                t += 1.0 / 60.0;
            }
            t
        };
        assert!(time(0.2) > time(1.0));
    }

    #[test]
    fn reverse_engages_from_standstill_when_braking_in_auto() {
        let mut v = coupe(true);
        run(
            &mut v,
            Controls {
                brake: 1.0,
                ..Default::default()
            },
            3.0,
        );
        assert!(v.gear < 0);
        assert!(v.forward_speed() < -0.5, "{}", v.forward_speed());
    }

    #[test]
    fn manual_gearbox_obeys_shift_requests() {
        let mut v = coupe(false);
        v.step(
            &Controls {
                shift: 1,
                ..Default::default()
            },
            1.0 / 60.0,
        );
        assert_eq!(v.gear, 2);
        run(&mut v, Controls::default(), 0.5);
        v.step(
            &Controls {
                shift: -1,
                ..Default::default()
            },
            1.0 / 60.0,
        );
        assert_eq!(v.gear, 1);
    }

    #[test]
    fn collision_reflects_velocity_and_damages() {
        let mut v = coupe(true);
        v.vel = [0.0, 20.0];
        let hit = v.collide([0.0, -1.0], 0.1, 0);
        assert!(hit > 19.0);
        assert!(v.vel[1] < 0.0);
        assert!(v.cond.body[0] > 0.1);
    }
}
