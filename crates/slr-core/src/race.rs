//! A race: countdown, racers (player + AI) on a track, lap/finish tracking, barrier and
//! car-to-car collisions, and results.

use crate::ai::Driver;
use crate::track::Track;
use crate::vehicle::{Controls, Vehicle};
use serde::{Deserialize, Serialize};

pub const COUNTDOWN: f32 = 3.0;
pub const DRAG_LENGTH: f32 = 402.3;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RaceKind {
    Drag,
    Circuit { laps: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Countdown,
    Racing,
    Finished,
}

pub struct Racer {
    pub name: String,
    pub vehicle: Vehicle,
    pub driver: Option<Driver>,
    pub is_player: bool,
    /// Distance covered along the track since the start line (can be negative behind it).
    pub progress: f32,
    last_s: f32,
    pub lap: u32,
    pub lap_start: f32,
    pub lap_times: Vec<f32>,
    pub finish_time: Option<f32>,
    pub penalty: f32,
    pub false_start: bool,
    stuck_timer: f32,
}

impl Racer {
    pub fn best_lap(&self) -> Option<f32> {
        self.lap_times
            .iter()
            .cloned()
            .fold(None, |a, t| Some(a.map_or(t, |x: f32| x.min(t))))
    }
}

pub struct Race {
    pub kind: RaceKind,
    pub track: Track,
    pub racers: Vec<Racer>,
    pub time: f32,
    pub phase: Phase,
    pub countdown: f32,
    /// Hit-by-barrier events this frame (for audio / effects).
    pub impacts: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Standing {
    pub name: String,
    pub is_player: bool,
    pub time: Option<f32>,
    pub progress: f32,
}

impl Race {
    pub fn new(kind: RaceKind, track: Track) -> Race {
        Race {
            kind,
            track,
            racers: Vec::new(),
            time: 0.0,
            phase: Phase::Countdown,
            countdown: COUNTDOWN,
            impacts: Vec::new(),
        }
    }

    /// Add a racer on grid slot `slot` (0 = pole). Drag races use two lanes.
    pub fn add_racer(
        &mut self,
        name: &str,
        mut vehicle: Vehicle,
        ai_skill: Option<f32>,
        slot: usize,
    ) {
        let (behind, lane) = match self.kind {
            RaceKind::Drag => (0.0, if slot.is_multiple_of(2) { -3.5 } else { 3.5 }),
            RaceKind::Circuit { .. } => (
                8.0 + 9.0 * (slot / 2) as f32,
                if slot.is_multiple_of(2) { -2.5 } else { 2.5 },
            ),
        };
        let (pos, heading) = self.track.grid_slot(behind, lane);
        vehicle.place(pos, heading);
        let drag = matches!(self.kind, RaceKind::Drag);
        let driver = ai_skill.map(|s| Driver::new(s, lane, drag));
        let last_s = self.track.project(pos).s;
        self.racers.push(Racer {
            name: name.into(),
            vehicle,
            is_player: ai_skill.is_none(),
            driver,
            progress: -behind,
            last_s,
            lap: 0,
            lap_start: 0.0,
            lap_times: vec![],
            finish_time: None,
            penalty: 0.0,
            false_start: false,
            stuck_timer: 0.0,
        });
    }

    pub fn player(&self) -> Option<&Racer> {
        self.racers.iter().find(|r| r.is_player)
    }

    pub fn total_distance(&self) -> f32 {
        match self.kind {
            RaceKind::Drag => DRAG_LENGTH,
            RaceKind::Circuit { laps } => laps as f32 * self.track.length(),
        }
    }

    pub fn step(&mut self, player: &Controls, dt: f32) {
        self.impacts.clear();
        match self.phase {
            Phase::Countdown => {
                self.countdown -= dt;
                if self.countdown <= 0.0 {
                    self.phase = Phase::Racing;
                    self.time = 0.0;
                }
            }
            Phase::Racing => self.time += dt,
            Phase::Finished => {}
        }
        let go = self.phase != Phase::Countdown;
        let total = self.total_distance();
        let closed = self.track.closed;
        let track_len = self.track.length();
        for r in &mut self.racers {
            let mut c = if r.is_player {
                *player
            } else {
                match r.driver.as_mut() {
                    Some(d) => d.drive(&r.vehicle, &self.track, go),
                    None => Controls::default(),
                }
            };
            if !go || r.finish_time.is_some() {
                // Held on the grid / coasting after the flag.
                if r.finish_time.is_some() {
                    c.throttle = 0.0;
                    c.brake = 0.4;
                } else {
                    c.throttle = if r.is_player { c.throttle } else { 0.0 };
                    c.brake = 1.0;
                    c.handbrake = true;
                }
            }
            if !go
                && r.is_player
                && r.vehicle.speed() > 2.0
                && !r.false_start
                && matches!(self.kind, RaceKind::Drag)
            {
                r.false_start = true;
                r.penalty += 1.0;
            }
            if !go {
                // Cars wait on the grid in first gear.
                r.vehicle.gear = 1;
            }
            r.vehicle.step(&c, dt);
            // Wedged against a barrier: AI cars are put back on the road after a few seconds.
            if go && !r.is_player && r.finish_time.is_none() && r.vehicle.speed() < 1.5 {
                r.stuck_timer += dt;
            } else {
                r.stuck_timer = 0.0;
            }
            if r.stuck_timer > 3.0 {
                r.stuck_timer = 0.0;
                let pr = self.track.project(r.vehicle.pos);
                let (p, t) = self.track.at(pr.s);
                r.vehicle.place(p, Track::heading_of(t));
                r.penalty += 2.0;
            }

            // Progress tracking with wrap handling on closed circuits.
            let pr = self.track.project(r.vehicle.pos);
            let mut ds = pr.s - r.last_s;
            if closed {
                if ds < -track_len / 2.0 {
                    ds += track_len;
                } else if ds > track_len / 2.0 {
                    ds -= track_len;
                }
            }
            r.last_s = pr.s;
            r.progress += ds;
            if let RaceKind::Circuit { .. } = self.kind {
                let lap = (r.progress / track_len).floor().max(0.0) as u32;
                if lap > r.lap && go {
                    r.lap_times.push(self.time - r.lap_start);
                    r.lap_start = self.time;
                    r.lap = lap;
                }
            }
            if go && r.finish_time.is_none() && r.progress >= total {
                r.finish_time = Some(self.time + r.penalty);
            }
        }
        self.resolve_collisions();
        let player_done = match self.player() {
            Some(p) => p.finish_time.is_some(),
            None => self.racers.iter().all(|r| r.finish_time.is_some()),
        };
        if self.phase == Phase::Racing && player_done {
            self.phase = Phase::Finished;
        }
        // Cut-off: everyone but the player finishing 25 s after the player ends the race.
    }

    fn resolve_collisions(&mut self) {
        let half_len = 2.2;
        let half_wid = 0.9;
        for ri in 0..self.racers.len() {
            let v = &self.racers[ri].vehicle;
            let f = v.forward();
            let r = v.right();
            let corners = [
                (half_len, -half_wid, 0usize),
                (half_len, half_wid, 0),
                (-half_len, -half_wid, 1),
                (-half_len, half_wid, 1),
            ];
            let mut worst: Option<([f32; 2], f32, usize)> = None;
            for (fl, rl, side) in corners {
                let p = [
                    v.pos[0] + f[0] * fl + r[0] * rl,
                    v.pos[1] + f[1] * fl + r[1] * rl,
                ];
                if let Some((n, depth)) = self.track.wall_contact(p) {
                    if worst.is_none_or(|w| depth > w.1) {
                        worst = Some((n, depth, side));
                    }
                }
            }
            if let Some((n, depth, side)) = worst {
                let speed = self.racers[ri].vehicle.collide(n, depth, side);
                if speed > 0.5 {
                    self.impacts.push(speed);
                }
            }
        }
        // Car vs car: treat as discs.
        for i in 0..self.racers.len() {
            for j in (i + 1)..self.racers.len() {
                let (a, b) = {
                    let (l, r) = self.racers.split_at_mut(j);
                    (&mut l[i].vehicle, &mut r[0].vehicle)
                };
                let d = [b.pos[0] - a.pos[0], b.pos[1] - a.pos[1]];
                let dist = d[0].hypot(d[1]);
                let min = 2.3;
                if dist < min && dist > 1e-4 {
                    let n = [d[0] / dist, d[1] / dist];
                    let push = (min - dist) / 2.0;
                    a.pos = [a.pos[0] - n[0] * push, a.pos[1] - n[1] * push];
                    b.pos = [b.pos[0] + n[0] * push, b.pos[1] + n[1] * push];
                    let rel = (b.vel[0] - a.vel[0]) * n[0] + (b.vel[1] - a.vel[1]) * n[1];
                    if rel < 0.0 {
                        let j_imp = -(1.0 + 0.2) * rel / (1.0 / a.mass + 1.0 / b.mass);
                        a.vel = [
                            a.vel[0] - j_imp * n[0] / a.mass,
                            a.vel[1] - j_imp * n[1] / a.mass,
                        ];
                        b.vel = [
                            b.vel[0] + j_imp * n[0] / b.mass,
                            b.vel[1] + j_imp * n[1] / b.mass,
                        ];
                        a.cond.apply_impact(-rel * 0.5, 0);
                        b.cond.apply_impact(-rel * 0.5, 1);
                        self.impacts.push(-rel);
                    }
                }
            }
        }
    }

    pub fn standings(&self) -> Vec<Standing> {
        let mut s: Vec<Standing> = self
            .racers
            .iter()
            .map(|r| Standing {
                name: r.name.clone(),
                is_player: r.is_player,
                time: r.finish_time,
                progress: r.progress,
            })
            .collect();
        s.sort_by(|a, b| match (a.time, b.time) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap(),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.progress.partial_cmp(&a.progress).unwrap(),
        });
        s
    }

    pub fn player_position(&self) -> usize {
        self.standings()
            .iter()
            .position(|s| s.is_player)
            .map_or(0, |p| p + 1)
    }

    /// Let the remaining AI cars finish (used to settle final standings when the player crosses).
    pub fn simulate_to_end(&mut self, max_seconds: f32) {
        let mut t = 0.0;
        while t < max_seconds && self.racers.iter().any(|r| r.finish_time.is_none()) {
            self.phase = Phase::Racing;
            self.step(&Controls::default(), 1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        self.phase = Phase::Finished;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::vehicle::Setup;

    pub fn car(cat: &Catalog, id: &str, auto: bool) -> Vehicle {
        let m = cat.car(id).unwrap();
        Vehicle::new(
            &m.cfg,
            Setup {
                engine: cat.engine(&m.stock_engine).unwrap().clone(),
                tyre_front: cat.stock_tyre().clone(),
                tyre_rear: cat.stock_tyre().clone(),
                final_drive_scale: 1.0,
                brake_bias: 0.6,
                wing: 0.3,
                lsd: 0.0,
                automatic: auto,
            },
        )
    }

    #[test]
    fn drag_race_ends_with_ordered_results() {
        let cat = Catalog::fallback();
        let mut race = Race::new(RaceKind::Drag, Track::drag_strip(DRAG_LENGTH));
        race.add_racer("You", car(&cat, "Baiern/CoupeSport", true), None, 0);
        race.add_racer("AI", car(&cat, "Baiern/CoupeSport", true), Some(0.9), 1);
        let mut guard = 0;
        while race.phase != Phase::Finished && guard < 60 * 60 {
            race.step(
                &Controls {
                    throttle: 1.0,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            guard += 1;
        }
        assert_eq!(race.phase, Phase::Finished);
        let p = race.player().unwrap();
        let t = p.finish_time.unwrap();
        assert!((10.0..25.0).contains(&t), "quarter mile {t}");
        // Identical cars with a full-throttle driver: the result must be close.
        race.simulate_to_end(30.0);
        let st = race.standings();
        assert!(st.iter().all(|s| s.time.is_some()));
        assert!((st[0].time.unwrap() - st[1].time.unwrap()).abs() < 2.0);
    }

    #[test]
    fn countdown_holds_cars_then_releases() {
        let cat = Catalog::fallback();
        let mut race = Race::new(RaceKind::Drag, Track::drag_strip(DRAG_LENGTH));
        race.add_racer("You", car(&cat, "Baiern/CoupeSport", true), None, 0);
        for _ in 0..60 {
            race.step(
                &Controls {
                    throttle: 1.0,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
        }
        assert_eq!(race.phase, Phase::Countdown);
        assert!(race.racers[0].vehicle.speed() < 1.0);
    }

    #[test]
    fn jumping_the_start_adds_a_penalty() {
        let cat = Catalog::fallback();
        let mut race = Race::new(RaceKind::Drag, Track::drag_strip(DRAG_LENGTH));
        race.add_racer("You", car(&cat, "Baiern/CoupeSport", true), None, 0);
        // Releasing the brake and flooring it during the countdown.
        race.racers[0].vehicle.vel = [0.0, 5.0];
        race.step(
            &Controls {
                throttle: 1.0,
                ..Default::default()
            },
            1.0 / 60.0,
        );
        assert!(race.racers[0].false_start);
        assert!(race.racers[0].penalty >= 1.0);
    }

    #[test]
    fn ai_completes_a_clean_lap_of_the_circuit() {
        let cat = Catalog::fallback();
        let mut race = Race::new(RaceKind::Circuit { laps: 1 }, Track::circuit());
        race.add_racer("AI", car(&cat, "Baiern/DevilSport", true), Some(0.85), 0);
        let mut guard = 0;
        while race.racers[0].finish_time.is_none() && guard < 60 * 400 {
            race.step(&Controls::default(), 1.0 / 60.0);
            guard += 1;
        }
        let r = &race.racers[0];
        let t = r.finish_time.expect("AI should finish a lap");
        assert!((60.0..220.0).contains(&t), "lap time {t}");
        assert!(
            r.vehicle.cond.body_total() < 0.1,
            "AI crashed: {:?}",
            r.vehicle.cond.body
        );
    }

    #[test]
    fn crashing_into_the_wall_damages_the_car() {
        let cat = Catalog::fallback();
        let mut race = Race::new(RaceKind::Drag, Track::drag_strip(DRAG_LENGTH));
        race.add_racer("You", car(&cat, "Baiern/CoupeSport", true), None, 0);
        // Skip the countdown and steer hard right under power.
        race.phase = Phase::Racing;
        race.countdown = 0.0;
        for _ in 0..60 * 12 {
            race.step(
                &Controls {
                    throttle: 1.0,
                    steer: 0.5,
                    ..Default::default()
                },
                1.0 / 60.0,
            );
            if race.racers[0].vehicle.cond.body_total() > 0.0 {
                break;
            }
        }
        assert!(race.racers[0].vehicle.cond.body_total() > 0.0);
        // The wall must hold the car in.
        let pr = race.track.project(race.racers[0].vehicle.pos);
        assert!(pr.lateral.abs() < race.track.wall_distance() + 1.5);
    }
}
