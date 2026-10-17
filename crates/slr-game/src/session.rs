//! A driving session: free test drive on the circuit or a race event, with HUD and results.

use crate::render3d::{CarRender, ChaseCam, Light, MeshSink, World};
use crate::ui::*;
use macroquad::prelude::*;
use slr_core::ai::Driver;
use slr_core::race::{Phase, Race, RaceKind};
use slr_core::state::{EventDef, RaceOutcome};
use slr_core::track::Track;
use slr_core::vehicle::Controls;

pub struct RaceSession {
    pub race: Race,
    pub event: Option<&'static EventDef>,
    pub world: World,
    pub cam: ChaseCam,
    pub light: Light,
    pub night: bool,
    accum: f32,
    pub paused: bool,
    pub outcome: Option<RaceOutcome>,
    pub renders: Vec<CarRender>,
    pub spins: Vec<[f32; 4]>,
    pub paints: Vec<[u8; 3]>,
    steer_in: f32,
    thr_in: f32,
    brk_in: f32,
    pub free_run: bool,
    pub finish_sim: f32,
    pub settled: bool,
    pub toast: String,
    pub toast_t: f32,
    pub elapsed: f32,
    /// Replaces keyboard input with an AI pilot (used by the automated self-test).
    pub autopilot: Option<Driver>,
    pub impact_flash: f32,
    pub best_lap_seen: Option<f32>,
}

impl RaceSession {
    pub fn new(
        race: Race,
        event: Option<&'static EventDef>,
        renders: Vec<CarRender>,
        paints: Vec<[u8; 3]>,
        night: bool,
        free_run: bool,
    ) -> RaceSession {
        let drag = matches!(race.kind, RaceKind::Drag);
        let world = World::build(&race.track, night, drag);
        let n = race.racers.len();
        RaceSession {
            race,
            event,
            world,
            cam: ChaseCam::new(),
            light: if night { Light::night() } else { Light::day() },
            night,
            accum: 0.0,
            paused: false,
            outcome: None,
            renders,
            spins: vec![[0.0; 4]; n],
            paints,
            steer_in: 0.0,
            thr_in: 0.0,
            brk_in: 0.0,
            free_run,
            finish_sim: 0.0,
            settled: false,
            toast: String::new(),
            toast_t: 0.0,
            elapsed: 0.0,
            autopilot: None,
            impact_flash: 0.0,
            best_lap_seen: None,
        }
    }

    fn read_controls(&mut self, dt: f32) -> Controls {
        if let Some(d) = self.autopilot.as_mut() {
            let go = self.race.phase != Phase::Countdown;
            let p = &self.race.racers[0];
            let mut c = d.drive(&p.vehicle, &self.race.track, go);
            c.throttle = c.throttle.max(0.0);
            return c;
        }
        let key = |a: KeyCode, b: KeyCode| is_key_down(a) || is_key_down(b);
        let thr_target = if key(KeyCode::W, KeyCode::Up) {
            1.0
        } else {
            0.0
        };
        let brk_target = if key(KeyCode::S, KeyCode::Down) {
            1.0
        } else {
            0.0
        };
        let steer_target = (if key(KeyCode::D, KeyCode::Right) {
            1.0
        } else {
            0.0
        }) - (if key(KeyCode::A, KeyCode::Left) {
            1.0
        } else {
            0.0
        });
        let rate = |cur: f32, target: f32, up: f32, down: f32| {
            let r = if target.abs() > cur.abs() || target.signum() != cur.signum() && target != 0.0
            {
                up
            } else {
                down
            };
            cur + (target - cur).clamp(-r * dt, r * dt)
        };
        self.thr_in = rate(self.thr_in, thr_target, 5.0, 8.0);
        self.brk_in = rate(self.brk_in, brk_target, 6.0, 10.0);
        // Steering eases out at speed so a keyboard is drivable.
        let speed = self.race.racers[0].vehicle.speed();
        let up = 3.2 / (1.0 + speed / 25.0);
        self.steer_in = rate(self.steer_in, steer_target, up.max(0.9), 5.0);
        let mut shift = 0;
        if is_key_pressed(KeyCode::E) {
            shift += 1;
        }
        if is_key_pressed(KeyCode::Q) {
            shift -= 1;
        }
        Controls {
            throttle: self.thr_in,
            brake: self.brk_in,
            steer: self.steer_in,
            handbrake: is_key_down(KeyCode::Space),
            shift,
        }
    }

    pub fn respawn_player(&mut self) {
        let track = &self.race.track;
        let r = &mut self.race.racers[0];
        let pr = track.project(r.vehicle.pos);
        let (p, t) = track.at(pr.s);
        r.vehicle.place(p, Track::heading_of(t));
        r.penalty += if self.free_run { 0.0 } else { 5.0 };
        self.toast = if self.free_run {
            "Car reset".into()
        } else {
            "Car reset (+5 s penalty)".into()
        };
        self.toast_t = 2.5;
    }

    pub fn update(&mut self, dt: f32) {
        if is_key_pressed(KeyCode::C) {
            self.cam.mode = (self.cam.mode + 1) % ChaseCam::MODES;
        }
        if is_key_pressed(KeyCode::R) && self.race.phase == Phase::Racing && self.outcome.is_none()
        {
            self.respawn_player();
        }
        if is_key_pressed(KeyCode::Escape) && self.outcome.is_none() {
            self.paused = !self.paused;
        }
        self.toast_t = (self.toast_t - dt).max(0.0);
        self.impact_flash = (self.impact_flash - dt * 2.0).max(0.0);
        if self.paused {
            return;
        }
        self.accum += dt.min(0.1);
        let step = 1.0 / 60.0;
        let mut n = 0;
        while self.accum >= step && n < 6 {
            self.accum -= step;
            n += 1;
            let c = self.read_controls(step);
            self.race.step(&c, step);
            self.elapsed += step;
            for &imp in &self.race.impacts {
                self.impact_flash = (self.impact_flash + imp / 20.0).min(1.0);
            }
            for (i, r) in self.race.racers.iter().enumerate() {
                for w in 0..4 {
                    self.spins[i][w] += r.vehicle.wheels[w].omega * step;
                }
            }
        }
        // Keep the world running a while after the player finishes so the AI can finish too.
        if self.race.phase == Phase::Finished && self.outcome.is_none() && !self.free_run {
            let unfinished = self.race.racers.iter().any(|r| r.finish_time.is_none());
            if unfinished && self.finish_sim < 120.0 {
                for _ in 0..30 {
                    let c = Controls {
                        brake: 0.4,
                        ..Default::default()
                    };
                    self.race.step(&c, step);
                    self.finish_sim += step;
                    if self.race.racers.iter().all(|r| r.finish_time.is_some()) {
                        break;
                    }
                }
            }
        }
        if let Some(p) = self.race.player() {
            if let Some(b) = p.best_lap() {
                if self.best_lap_seen.is_none_or(|x| b < x - 1e-3) {
                    self.best_lap_seen = Some(b);
                }
            }
        }
    }

    pub fn finished_waiting(&self) -> bool {
        self.race.phase == Phase::Finished
            && !self.free_run
            && self
                .race
                .racers
                .iter()
                .all(|r| r.finish_time.is_some() || self.finish_sim >= 120.0)
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let dt = get_frame_time();
        let player = &self.race.racers[0];
        let cam = self.cam.camera(
            player.vehicle.pos,
            player.vehicle.heading,
            player.vehicle.speed(),
            dt,
        );
        let sky = if self.night {
            Color::new(0.02, 0.03, 0.08, 1.0)
        } else {
            Color::new(0.52, 0.72, 0.92, 1.0)
        };
        clear_background(sky);
        set_camera(&cam);
        self.world.sink.draw();
        let mut sink = MeshSink::new();
        for (i, r) in self.race.racers.iter().enumerate() {
            let v = &r.vehicle;
            let braking = v.accel_body[1] < -2.5 || v.forward_speed() < -0.3;
            self.renders[i].emit(
                &mut sink,
                v.pos,
                v.heading,
                v.steer_angle,
                self.spins[i],
                self.paints[i],
                self.light,
                braking,
                v.cond.body_total(),
            );
            if self.night {
                // Headlight glow on the ground ahead of each car.
                let f = v.forward();
                let rgt = v.right();
                for side in [-0.6f32, 0.6] {
                    let p = [
                        v.pos[0] + f[0] * 2.4 + rgt[0] * side,
                        v.pos[1] + f[1] * 2.4 + rgt[1] * side,
                    ];
                    sink.cuboid(
                        crate::render3d::to_render(p[0], p[1], 0.7),
                        [0.12, 0.08, 0.05],
                        Color::new(1.0, 0.97, 0.8, 1.0),
                    );
                }
                let far = [v.pos[0] + f[0] * 16.0, v.pos[1] + f[1] * 16.0];
                let near = [v.pos[0] + f[0] * 3.0, v.pos[1] + f[1] * 3.0];
                let w0 = 1.0;
                let w1 = 7.0;
                let cone = Color::new(0.5, 0.5, 0.38, 1.0);
                sink.quad(
                    crate::render3d::to_render(near[0] - rgt[0] * w0, near[1] - rgt[1] * w0, 0.04),
                    crate::render3d::to_render(near[0] + rgt[0] * w0, near[1] + rgt[1] * w0, 0.04),
                    crate::render3d::to_render(far[0] + rgt[0] * w1, far[1] + rgt[1] * w1, 0.04),
                    crate::render3d::to_render(far[0] - rgt[0] * w1, far[1] - rgt[1] * w1, 0.04),
                    cone,
                );
            }
        }
        sink.draw();
        set_default_camera();
        self.draw_hud(ui);
    }

    fn draw_hud(&mut self, ui: &mut Ui) {
        let p = &self.race.racers[0];
        let v = &p.vehicle;
        // Damage flash.
        if self.impact_flash > 0.0 {
            ui.rect(
                0.0,
                0.0,
                VW,
                VH,
                Color::new(0.9, 0.1, 0.05, 0.25 * self.impact_flash),
            );
        }
        // Speed / gear / rpm cluster.
        let (cx, cy) = (1110.0, 600.0);
        let r = 100.0;
        ui.circle(cx, cy, r + 6.0, Color::new(0.05, 0.06, 0.08, 0.85));
        let redline = v.setup.engine.redline_rpm;
        let max_rpm = (redline / 1000.0).ceil() * 1000.0 + 500.0;
        let a0 = 2.4f32;
        let a1 = 2.4 + 4.5;
        let seg = 48;
        for k in 0..seg {
            let t0 = k as f32 / seg as f32;
            let t1 = (k + 1) as f32 / seg as f32;
            let rpm0 = t0 * max_rpm;
            let col = if rpm0 > redline {
                ACCENT
            } else if rpm0 < v.rpm {
                TEXT
            } else {
                Color::new(0.3, 0.33, 0.38, 1.0)
            };
            let (x0, y0) = (
                cx + (a0 + (a1 - a0) * t0).cos() * (r - 8.0),
                cy + (a0 + (a1 - a0) * t0).sin() * (r - 8.0),
            );
            let (x1, y1) = (
                cx + (a0 + (a1 - a0) * t1).cos() * (r - 8.0),
                cy + (a0 + (a1 - a0) * t1).sin() * (r - 8.0),
            );
            ui.line(x0, y0, x1, y1, 7.0, col);
        }
        for k in 0..=(max_rpm / 1000.0) as i32 {
            let t = k as f32 * 1000.0 / max_rpm;
            let ang = a0 + (a1 - a0) * t;
            let (x, y) = (
                cx + ang.cos() * (r - 26.0),
                cy + ang.sin() * (r - 26.0) + 5.0,
            );
            ui.text_centered(&k.to_string(), x, y, 14.0, DIM);
        }
        ui.text_centered(&format!("{:.0}", v.kph()), cx, cy + 22.0, 46.0, TEXT);
        ui.text_centered("km/h", cx, cy + 44.0, 16.0, DIM);
        let gear = match v.gear {
            g if g < 0 => "R".to_string(),
            0 => "N".to_string(),
            g => g.to_string(),
        };
        ui.text_centered(
            &gear,
            cx,
            cy - 42.0,
            40.0,
            if v.limiter { ACCENT } else { TEXT },
        );
        ui.text_centered(
            if v.setup.automatic { "AUTO" } else { "MANUAL" },
            cx,
            cy - 20.0,
            14.0,
            DIM,
        );

        // Condition panel.
        ui.rect(20.0, 560.0, 250.0, 140.0, Color::new(0.05, 0.06, 0.08, 0.8));
        ui.text("ENGINE", 32.0, 586.0, 16.0, DIM);
        ui.bar(
            110.0,
            572.0,
            140.0,
            14.0,
            v.cond.engine,
            condition_colour(v.cond.engine),
        );
        let tyre = 1.0 - v.cond.tyre_wear.iter().sum::<f32>() / 4.0;
        ui.text("TYRES", 32.0, 610.0, 16.0, DIM);
        ui.bar(110.0, 596.0, 140.0, 14.0, tyre, condition_colour(tyre));
        ui.text("BODY", 32.0, 634.0, 16.0, DIM);
        ui.bar(
            110.0,
            620.0,
            140.0,
            14.0,
            1.0 - v.cond.body_total(),
            condition_colour(1.0 - v.cond.body_total()),
        );
        ui.text("BRAKES", 32.0, 658.0, 16.0, DIM);
        ui.bar(
            110.0,
            644.0,
            140.0,
            14.0,
            1.0 - v.cond.brake_wear,
            condition_colour(1.0 - v.cond.brake_wear),
        );
        ui.text(
            &format!(
                "{:.2} g lat   {:.2} g long",
                v.lateral_g().abs(),
                v.longitudinal_g()
            ),
            32.0,
            686.0,
            15.0,
            DIM,
        );

        // Race info.
        ui.rect(20.0, 20.0, 260.0, 120.0, Color::new(0.05, 0.06, 0.08, 0.8));
        match self.race.kind {
            RaceKind::Drag => {
                ui.text(
                    &format!("Drag race   {:.0} m", self.race.total_distance()),
                    32.0,
                    46.0,
                    20.0,
                    TEXT,
                );
                ui.text(
                    &format!("{:.0} m", p.progress.max(0.0)),
                    32.0,
                    76.0,
                    26.0,
                    TEXT,
                );
            }
            RaceKind::Circuit { laps } => {
                let lap_txt = if self.free_run {
                    format!("Lap {}", p.lap + 1)
                } else {
                    format!("Lap {}/{}", (p.lap + 1).min(laps), laps)
                };
                ui.text(&lap_txt, 32.0, 46.0, 24.0, TEXT);
                if let Some(b) = p.best_lap() {
                    ui.text(&format!("Best {}", fmt_time(b)), 32.0, 104.0, 18.0, WARN);
                }
            }
        }
        let t = if self.race.phase == Phase::Countdown {
            0.0
        } else {
            self.race.time
        };
        ui.text(
            &format!(
                "Time {}",
                fmt_time(t + p.penalty * if p.finish_time.is_some() { 1.0 } else { 0.0 })
            ),
            32.0,
            130.0 - 24.0,
            18.0,
            DIM,
        );
        if !self.free_run {
            ui.text(
                &format!(
                    "P{}/{}",
                    self.race.player_position(),
                    self.race.racers.len()
                ),
                190.0,
                76.0,
                30.0,
                if self.race.player_position() == 1 {
                    GOOD
                } else {
                    WARN
                },
            );
        }
        // Minimap.
        self.draw_minimap(ui);
        // Countdown / banners.
        match self.race.phase {
            Phase::Countdown => {
                let n = self.race.countdown.ceil().max(1.0) as i32;
                ui.text_centered(&n.to_string(), VW / 2.0, 260.0, 140.0, TEXT);
                ui.text_centered(
                    "Hold throttle at the line. Moving early costs a second.",
                    VW / 2.0,
                    320.0,
                    22.0,
                    DIM,
                );
            }
            Phase::Racing if self.race.time < 1.2 => {
                ui.text_centered("GO!", VW / 2.0, 260.0, 140.0, GOOD)
            }
            _ => {}
        }
        if p.false_start {
            ui.text_centered("FALSE START  +1.0 s", VW / 2.0, 120.0, 30.0, ACCENT);
        }
        if self.toast_t > 0.0 {
            ui.text_centered(&self.toast, VW / 2.0, 160.0, 26.0, WARN);
        }
        if self.elapsed < 8.0 {
            ui.text_centered("W/S throttle & brake   A/D steer   Space handbrake   Q/E shift (manual)   C camera   R reset   Esc pause", VW / 2.0, 700.0, 18.0, DIM);
        }
        if let Some(d) = self.race.racers.get(1).filter(|_| !self.free_run) {
            let gap = d.progress - p.progress;
            let txt = if gap >= 0.0 {
                format!("{} +{:.0} m", d.name, gap)
            } else {
                format!("{} {:.0} m", d.name, gap)
            };
            ui.text_right(&txt, 1260.0, 340.0, 18.0, DIM);
        }
        // Finish banner.
        if self.race.phase == Phase::Finished && self.outcome.is_none() && !self.free_run {
            ui.text_centered("FINISH", VW / 2.0, 250.0, 100.0, TEXT);
            ui.text_centered("waiting for the others...", VW / 2.0, 300.0, 22.0, DIM);
        }
    }

    fn draw_minimap(&self, ui: &Ui) {
        let (x0, y0, w, h) = (1040.0, 20.0, 220.0, 220.0);
        ui.rect(x0, y0, w, h, Color::new(0.05, 0.06, 0.08, 0.75));
        let pts = &self.race.track.points;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
        let sc = (w - 30.0).min(h - 30.0) / span;
        let map = |p: [f32; 2]| {
            (
                x0 + 15.0 + (p[0] - lo[0]) * sc + ((w - 30.0) - (hi[0] - lo[0]) * sc) / 2.0,
                y0 + h - 15.0 - (p[1] - lo[1]) * sc - ((h - 30.0) - (hi[1] - lo[1]) * sc) / 2.0,
            )
        };
        let n = pts.len();
        let step = (n / 120).max(1);
        let mut i = 0;
        while i + step < n + if self.race.track.closed { step } else { 0 } {
            let a = map(pts[i % n]);
            let b = map(pts[(i + step) % n]);
            if i + step < n || self.race.track.closed {
                ui.line(a.0, a.1, b.0, b.1, 3.0, DIM);
            }
            i += step;
        }
        for (k, r) in self.race.racers.iter().enumerate() {
            let m = map(r.vehicle.pos);
            ui.circle(
                m.0,
                m.1,
                if k == 0 { 5.0 } else { 4.0 },
                if k == 0 { ACCENT } else { WARN },
            );
        }
    }
}
