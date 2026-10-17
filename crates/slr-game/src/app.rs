//! Application state machine: title screen, garage (parts, tuning, mechanic, painter, car lot,
//! events), driving sessions, results, and persistence.

use crate::render3d::{CarRender, Light, MeshSink};
use crate::session::RaceSession;
use crate::ui::*;
use macroquad::prelude::*;
use slr_core::catalog::Catalog;
use slr_core::perf::{self, Perf};
use slr_core::race::{Phase, Race, RaceKind};
use slr_core::state::{Axle, GameState, Listing, EVENTS, PAINTS};
use slr_core::track::Track;
use slr_core::vehicle::Vehicle;
use slr_core::visual::{load_car_visual, CarVisual, Material, VisualPart, VisualVertex};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub struct Args {
    pub reference: PathBuf,
    pub save_dir: PathBuf,
    pub autotest: bool,
    pub shots: PathBuf,
    pub time_scale: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Parts,
    Tuning,
    Mechanic,
    Painter,
    Cars,
    Events,
}

const TABS: [(Tab, &str); 7] = [
    (Tab::Overview, "Garage"),
    (Tab::Parts, "Parts"),
    (Tab::Tuning, "Tuning"),
    (Tab::Mechanic, "Mechanic"),
    (Tab::Painter, "Painter"),
    (Tab::Cars, "Cars"),
    (Tab::Events, "Events"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PartsView {
    Engine,
    Tyres,
    Upgrades,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CarsView {
    Showroom,
    Used,
    Mine,
}

pub enum Screen {
    Title,
    Garage,
    Session(Box<RaceSession>),
}

pub struct App {
    pub args: Args,
    pub cat: Catalog,
    pub state: Option<GameState>,
    pub screen: Screen,
    pub tab: Tab,
    pub ui: Ui,
    pub quit: bool,
    pub toast: String,
    pub toast_t: f32,
    visuals: HashMap<String, Rc<CarVisual>>,
    perf_cache: Option<(String, Perf)>,
    angle: f32,
    scroll: f32,
    parts_view: PartsView,
    cars_view: CarsView,
    axle: Axle,
    used_lot: Vec<Listing>,
    used_day: u32,
    pub reference_loaded: bool,
    pub frame_no: u64,
    stash: f32,
}

fn fallback_visual() -> CarVisual {
    // A plain box body, used when the reference install (and its panel meshes) is missing.
    let (hx, hy, hz) = (0.85f32, 0.35f32, 2.1f32);
    let c = [
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ];
    let faces: [([usize; 4], [f32; 3]); 6] = [
        ([0, 1, 2, 3], [0.0, 0.0, -1.0]),
        ([5, 4, 7, 6], [0.0, 0.0, 1.0]),
        ([4, 0, 3, 7], [-1.0, 0.0, 0.0]),
        ([1, 5, 6, 2], [1.0, 0.0, 0.0]),
        ([3, 2, 6, 7], [0.0, 1.0, 0.0]),
        ([4, 5, 1, 0], [0.0, -1.0, 0.0]),
    ];
    let mut part = VisualPart {
        name: "box".into(),
        material: Material::Paint,
        vertices: vec![],
        indices: vec![],
    };
    for (idx, n) in faces {
        let base = part.vertices.len() as u32;
        for i in idx {
            part.vertices.push(VisualVertex {
                pos: [c[i][0], c[i][1] + 0.55, c[i][2]],
                normal: n,
            });
        }
        part.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    CarVisual { parts: vec![part] }
}

impl App {
    pub fn new(args: Args, cat: Catalog, reference_loaded: bool, font: Option<Font>) -> App {
        App {
            args,
            cat,
            state: None,
            screen: Screen::Title,
            tab: Tab::Overview,
            ui: Ui::new(font),
            quit: false,
            toast: String::new(),
            toast_t: 0.0,
            visuals: HashMap::new(),
            perf_cache: None,
            angle: 0.6,
            scroll: 0.0,
            parts_view: PartsView::Engine,
            cars_view: CarsView::Showroom,
            axle: Axle::Both,
            used_lot: vec![],
            used_day: 0,
            reference_loaded,
            frame_no: 0,
            stash: 0.0,
        }
    }

    pub fn autotest_stash_money(&mut self) {
        self.stash = self.state.as_ref().map_or(0.0, |g| g.money);
    }

    pub fn stashed_money(&self) -> f32 {
        self.stash
    }

    pub fn save_path(&self) -> PathBuf {
        self.args.save_dir.join("slot1.json")
    }

    pub fn say(&mut self, s: impl Into<String>) {
        self.toast = s.into();
        self.toast_t = 3.0;
    }

    pub fn save(&mut self) {
        if let Some(gs) = &self.state {
            match gs.save(&self.save_path()) {
                Ok(()) => self.say("Game saved"),
                Err(e) => self.say(format!("Save failed: {e}")),
            }
        }
    }

    fn visual_for(&mut self, model: &str) -> Rc<CarVisual> {
        if let Some(v) = self.visuals.get(model) {
            return v.clone();
        }
        let loaded = self.cat.car(model).and_then(|m| {
            let dir = m.mesh_dir.as_ref()?.parent()?.to_path_buf();
            let stem = m.id.split('/').nth(1)?.to_string();
            load_car_visual(&dir, &stem)
        });
        let v = Rc::new(loaded.unwrap_or_else(fallback_visual));
        self.visuals.insert(model.to_string(), v.clone());
        v
    }

    pub fn render_for(
        &mut self,
        model: &str,
        front: &slr_core::tyre::TyreSpec,
        rear: &slr_core::tyre::TyreSpec,
    ) -> CarRender {
        let visual = self.visual_for(model);
        let cfg = &self.cat.car(model).expect("model in catalog").cfg;
        let radii = [
            front.radius_m(),
            front.radius_m(),
            rear.radius_m(),
            rear.radius_m(),
        ];
        let widths = [
            front.width_mm / 1000.0,
            front.width_mm / 1000.0,
            rear.width_mm / 1000.0,
            rear.width_mm / 1000.0,
        ];
        CarRender::new(visual, &cfg.wheels, radii, widths)
    }

    pub fn new_game(&mut self) {
        let seed = (get_time() * 1000.0) as u64 ^ 0xA5A5_1234;
        self.state = Some(GameState::new_game(&self.cat, seed));
        self.screen = Screen::Garage;
        self.tab = Tab::Overview;
        self.perf_cache = None;
        self.say("New game started: a tired used car and $2,500");
    }

    pub fn continue_game(&mut self) {
        match GameState::load(&self.save_path()) {
            Ok(gs) => {
                // A save made with a different catalog may reference unknown cars.
                if gs.cars.iter().all(|c| self.cat.car(&c.model).is_some()) {
                    self.state = Some(gs);
                    self.screen = Screen::Garage;
                    self.perf_cache = None;
                    self.say("Game loaded");
                } else {
                    self.say("Save refers to cars missing from the content");
                }
            }
            Err(e) => self.say(format!("Could not load: {e}")),
        }
    }

    // ---- driving sessions ----

    fn player_vehicle(&self) -> Option<Vehicle> {
        let gs = self.state.as_ref()?;
        gs.vehicle_for(&self.cat, gs.active).ok()
    }

    pub fn start_test_drive(&mut self) {
        let Some(v) = self.player_vehicle() else {
            return;
        };
        let (model, setup, paint) = {
            let c = self.state.as_ref().unwrap().car();
            (c.model.clone(), c.setup.clone(), c.paint)
        };
        let mut race = Race::new(RaceKind::Circuit { laps: 999 }, Track::circuit());
        race.add_racer("You", v, None, 0);
        race.phase = Phase::Racing;
        race.countdown = 0.0;
        let r = self.render_for(&model, &setup.tyre_front, &setup.tyre_rear);
        let mut s = RaceSession::new(race, None, vec![r], vec![PAINTS[paint].1], false, true);
        s.light = Light::day();
        self.screen = Screen::Session(Box::new(s));
    }

    pub fn start_event(&mut self, id: &str) {
        let Some(gs) = self.state.as_mut() else {
            return;
        };
        let ev = match gs.event(id) {
            Some(e) => e,
            None => return,
        };
        match gs.start_event(&self.cat, id) {
            Ok(race) => {
                let (model, setup, paint) = {
                    let c = gs.car();
                    (c.model.clone(), c.setup.clone(), c.paint)
                };
                let opp_model = if self.cat.car(ev.opponent).is_some() {
                    ev.opponent.to_string()
                } else {
                    self.cat.cars[0].id.clone()
                };
                let stock = self.cat.stock_tyre().clone();
                let r0 = self.render_for(&model, &setup.tyre_front, &setup.tyre_rear);
                let r1 = self.render_for(&opp_model, &stock, &stock);
                let opp_paint =
                    (opp_model.bytes().map(|b| b as usize).sum::<usize>()) % PAINTS.len();
                let s = RaceSession::new(
                    race,
                    Some(ev),
                    vec![r0, r1],
                    vec![PAINTS[paint].1, PAINTS[opp_paint].1],
                    ev.night,
                    false,
                );
                self.screen = Screen::Session(Box::new(s));
            }
            Err(e) => self.say(e.to_string()),
        }
    }

    fn finish_session(&mut self, forfeit: bool) {
        let Screen::Session(s) = std::mem::replace(&mut self.screen, Screen::Garage) else {
            return;
        };
        if let Some(gs) = self.state.as_mut() {
            let active = gs.active;
            if !s.settled {
                // Quitting early: wear and damage still carry over, the entry fee is gone.
                gs.cars[active].cond = s.race.racers[0].vehicle.cond.clone();
                if forfeit && s.event.is_some() {
                    gs.races += 1;
                    gs.day += 1;
                }
            }
            let _ = gs.save(&self.args.save_dir.join("slot1.json"));
        }
        self.perf_cache = None;
        self.tab = Tab::Overview;
    }

    // ---- per frame ----

    pub fn frame(&mut self) {
        self.frame_no += 1;
        self.ui.begin();
        self.toast_t = (self.toast_t - get_frame_time()).max(0.0);
        let screen = std::mem::replace(&mut self.screen, Screen::Title);
        match screen {
            Screen::Title => {
                self.screen = Screen::Title;
                self.draw_title();
            }
            Screen::Garage => {
                self.screen = Screen::Garage;
                self.draw_garage();
            }
            Screen::Session(mut s) => {
                let dt = get_frame_time() * self.args.time_scale;
                s.update(dt);
                s.draw(&mut self.ui);
                let mut next: Option<bool> = None;
                self.draw_session_overlays(&mut s, &mut next);
                self.screen = Screen::Session(s);
                if let Some(forfeit) = next {
                    self.finish_session(forfeit);
                }
            }
        }
        if self.toast_t > 0.0 && !matches!(self.screen, Screen::Session(_)) {
            let w = self.ui.text_width(&self.toast, 22.0) + 40.0;
            self.ui.rect(
                VW / 2.0 - w / 2.0,
                640.0,
                w,
                40.0,
                Color::new(0.0, 0.0, 0.0, 0.8),
            );
            self.ui
                .text_centered(&self.toast.clone(), VW / 2.0, 668.0, 22.0, WARN);
        }
        self.ui.end();
    }

    fn draw_title(&mut self) {
        clear_background(Color::new(0.05, 0.06, 0.08, 1.0));
        let ui = &mut self.ui;
        ui.rect(0.0, 0.0, VW, 6.0, ACCENT);
        ui.text_centered("STREET LEGAL RACING", VW / 2.0, 190.0, 72.0, TEXT);
        ui.text_centered("REDLINE", VW / 2.0, 260.0, 72.0, ACCENT);
        ui.text_centered(
            "a Rust recreation, built from the shipped game data",
            VW / 2.0,
            305.0,
            22.0,
            DIM,
        );
        let has_save = self.args.save_dir.join("slot1.json").exists();
        let mut action = 0;
        if ui.button(VW / 2.0 - 160.0, 360.0, 320.0, 52.0, "Continue", has_save) {
            action = 1;
        }
        if ui.button(VW / 2.0 - 160.0, 424.0, 320.0, 52.0, "New Game", true) {
            action = 2;
        }
        if ui.button(VW / 2.0 - 160.0, 488.0, 320.0, 52.0, "Quit", true) {
            action = 3;
        }
        let info = if self.reference_loaded {
            format!(
                "Content: {} cars, {} engines, {} tyres from {}",
                self.cat.cars.len(),
                self.cat.engines.len(),
                self.cat.tyres.len(),
                self.args.reference.display()
            )
        } else {
            "Reference install not found: running on the built-in two-car fallback content"
                .to_string()
        };
        ui.text_centered(&info, VW / 2.0, 610.0, 18.0, DIM);
        ui.text_centered(
            "Data: Street Legal Racing: Redline build 606 (v2.2.1)",
            VW / 2.0,
            636.0,
            16.0,
            DIM,
        );
        match action {
            1 => self.continue_game(),
            2 => self.new_game(),
            3 => self.quit = true,
            _ => {}
        }
    }

    // ---- garage ----

    fn perf(&mut self) -> Perf {
        let Some(gs) = self.state.as_ref() else {
            return Perf {
                zero_to_100_s: None,
                quarter_mile_s: None,
                trap_kph: 0.0,
                top_kph: 0.0,
                peak_kw: 0.0,
                mass_kg: 0.0,
            };
        };
        let c = gs.car();
        let key = format!(
            "{}|{:?}|{:.2}|{:.2}",
            c.model,
            c.setup,
            c.cond.engine,
            c.cond.body_total()
        );
        if let Some((k, p)) = &self.perf_cache {
            if *k == key {
                return *p;
            }
        }
        let p = gs
            .vehicle_for(&self.cat, gs.active)
            .map(|v| perf::measure(&v))
            .unwrap();
        self.perf_cache = Some((key, p));
        p
    }

    fn draw_garage_scene(&mut self) {
        let dt = get_frame_time();
        self.angle += dt * 0.35;
        let (model, setup, paint, body_damage) = {
            let c = self.state.as_ref().unwrap().car();
            (
                c.model.clone(),
                c.setup.clone(),
                c.paint,
                c.cond.body_total(),
            )
        };
        let render = self.render_for(&model, &setup.tyre_front, &setup.tyre_rear);
        let light = Light {
            ambient: 0.5,
            sun: 0.7,
            tint: [1.0, 1.0, 1.0],
        };
        clear_background(Color::new(0.09, 0.10, 0.13, 1.0));
        let r = 7.2;
        let cam = Camera3D {
            // Shift the camera sideways so the car sits in the left half, clear of the panel.
            position: vec3(
                self.angle.sin() * r + self.angle.cos() * 1.9,
                2.3,
                self.angle.cos() * r - self.angle.sin() * 1.9,
            ),
            target: vec3(self.angle.cos() * 1.9, 0.7, -self.angle.sin() * 1.9),
            up: vec3(0.0, 1.0, 0.0),
            fovy: 0.62,
            projection: Projection::Perspective,
            ..Default::default()
        };
        set_camera(&cam);
        let mut sink = MeshSink::new();
        // Floor with a grid of tiles.
        for ix in -6..6 {
            for iz in -6..6 {
                let k = if (ix + iz) % 2 == 0 { 0.2 } else { 0.16 };
                let col = Color::new(k, k + 0.01, k + 0.03, 1.0);
                let (x, z) = (ix as f32 * 2.0, iz as f32 * 2.0);
                sink.quad(
                    [x, 0.0, z],
                    [x + 2.0, 0.0, z],
                    [x + 2.0, 0.0, z + 2.0],
                    [x, 0.0, z + 2.0],
                    col,
                );
            }
        }
        // Heading 0 faces -z in render space, so spin the heading with the camera.
        render.emit(
            &mut sink,
            [0.0, 0.0],
            0.0,
            0.0,
            [0.0; 4],
            PAINTS[paint].1,
            light,
            false,
            body_damage,
        );
        sink.draw();
        set_default_camera();
    }

    fn header(&mut self) {
        let gs = self.state.as_ref().unwrap();
        let (money_s, rep, day) = (money(gs.money), gs.reputation, gs.day);
        let ui = &mut self.ui;
        ui.rect(0.0, 0.0, 640.0, 64.0, Color::new(0.04, 0.05, 0.07, 0.8));
        ui.text("GARAGE", 24.0, 42.0, 34.0, TEXT);
        ui.text_right(
            &format!("{money_s}    Rep {rep}    Day {day}"),
            620.0,
            40.0,
            24.0,
            GOOD,
        );
    }

    fn draw_garage(&mut self) {
        if self.state.is_none() {
            self.screen = Screen::Title;
            return;
        }
        self.draw_garage_scene();
        self.header();
        // Car title under the preview.
        {
            let gs = self.state.as_ref().unwrap();
            let name = self
                .cat
                .car(&gs.car().model)
                .map(|m| format!("{} {}", m.maker, m.name))
                .unwrap_or_default();
            self.ui
                .rect(0.0, 640.0, 640.0, 80.0, Color::new(0.04, 0.05, 0.07, 0.8));
            self.ui.text(&name, 24.0, 684.0, 34.0, TEXT);
            self.ui.text_right(
                &format!("{} km", gs.car().cond.odometer_km.round()),
                620.0,
                684.0,
                20.0,
                DIM,
            );
        }
        let ui = &mut self.ui;
        ui.rect(650.0, 14.0, 616.0, 692.0, PANEL);
        // Tab bar.
        let mut new_tab = self.tab;
        for (i, (t, label)) in TABS.iter().enumerate() {
            let x = 660.0 + i as f32 * 86.0;
            let sel = *t == self.tab;
            let hover = ui.hovered(x, 22.0, 82.0, 36.0);
            ui.rect(
                x,
                22.0,
                82.0,
                36.0,
                if sel {
                    ACCENT
                } else if hover {
                    PANEL2
                } else {
                    Color::new(0.14, 0.16, 0.19, 1.0)
                },
            );
            ui.text_centered(label, x + 41.0, 46.0, 18.0, TEXT);
            if hover && ui.clicked {
                new_tab = *t;
            }
        }
        for (i, k) in [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
        ]
        .iter()
        .enumerate()
        {
            if is_key_pressed(*k) {
                new_tab = TABS[i].0;
            }
        }
        if new_tab != self.tab {
            self.tab = new_tab;
            self.scroll = 0.0;
            self.ui.reset_selection();
        }
        match self.tab {
            Tab::Overview => self.tab_overview(),
            Tab::Parts => self.tab_parts(),
            Tab::Tuning => self.tab_tuning(),
            Tab::Mechanic => self.tab_mechanic(),
            Tab::Painter => self.tab_painter(),
            Tab::Cars => self.tab_cars(),
            Tab::Events => self.tab_events(),
        }
    }

    fn drivetrain(&self) -> &'static str {
        let gs = self.state.as_ref().unwrap();
        let Some(m) = self.cat.car(&gs.car().model) else {
            return "?";
        };
        let f = m
            .cfg
            .wheels
            .iter()
            .filter(|w| w.driven() && w.position[2] < 0.0)
            .count();
        let r = m
            .cfg
            .wheels
            .iter()
            .filter(|w| w.driven() && w.position[2] > 0.0)
            .count();
        match (f > 0, r > 0) {
            (true, true) => "AWD",
            (true, false) => "FWD",
            _ => "RWD",
        }
    }

    fn tab_overview(&mut self) {
        let perf = self.perf();
        let dt = self.drivetrain();
        let gs = self.state.as_ref().unwrap();
        let c = gs.car();
        let cond = c.cond.clone();
        let eng = c.setup.engine.clone();
        let (tf, tr) = (c.setup.tyre_front.label(), c.setup.tyre_rear.label());
        let ratio_txt = self
            .cat
            .car(&c.model)
            .map(|m| {
                m.cfg
                    .gear_ratios
                    .iter()
                    .map(|r| format!("{r:.2}"))
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let fd = self
            .cat
            .car(&c.model)
            .map(|m| m.cfg.rearend_ratio * c.setup.final_drive_scale)
            .unwrap_or(0.0);
        let tune = c.tune_stage;
        let auto = c.setup.automatic;
        let ui = &mut self.ui;
        let mut y = 100.0;
        ui.text("SPECIFICATION", 670.0, y, 18.0, ACCENT);
        y += 30.0;
        let rows = [
            (
                "Engine",
                format!("{}  {:.1} L", eng.display, eng.displacement_l),
            ),
            (
                "Power / torque",
                format!(
                    "{:.0} kW ({:.0} hp)  {:.0} Nm",
                    perf.peak_kw * eng.boost.max(1.0) / eng.boost.max(1.0),
                    perf.peak_kw * 1.341,
                    eng.peak_torque_nm * eng.boost
                ),
            ),
            (
                "Drivetrain",
                format!("{dt}   {}", if auto { "automatic" } else { "manual" }),
            ),
            ("Gears", ratio_txt),
            ("Final drive", format!("{fd:.2}")),
            ("Tyres F / R", format!("{tf}  /  {tr}")),
            (
                "Mass",
                format!("{:.0} kg   tune stage {}", perf.mass_kg, tune),
            ),
        ];
        for (k, v) in rows {
            ui.text(k, 670.0, y, 19.0, DIM);
            ui.text(&v, 850.0, y, 19.0, TEXT);
            y += 28.0;
        }
        y += 10.0;
        ui.text("PERFORMANCE (simulated)", 670.0, y, 18.0, ACCENT);
        y += 30.0;
        let z = perf
            .zero_to_100_s
            .map_or("-".to_string(), |t| format!("{t:.1} s"));
        let q = perf.quarter_mile_s.map_or("-".to_string(), |t| {
            format!("{t:.1} s @ {:.0} km/h", perf.trap_kph)
        });
        for (k, v) in [
            ("0-100 km/h", z),
            ("402 m", q),
            ("Top speed", format!("{:.0} km/h", perf.top_kph)),
        ] {
            ui.text(k, 670.0, y, 19.0, DIM);
            ui.text(&v, 850.0, y, 19.0, TEXT);
            y += 28.0;
        }
        y += 10.0;
        ui.text("CONDITION", 670.0, y, 18.0, ACCENT);
        y += 14.0;
        let tyre = 1.0 - cond.tyre_wear.iter().sum::<f32>() / 4.0;
        for (k, f) in [
            ("Engine", cond.engine),
            ("Tyres", tyre),
            ("Bodywork", 1.0 - cond.body_total()),
            ("Brakes", 1.0 - cond.brake_wear),
        ] {
            y += 26.0;
            ui.text(k, 670.0, y, 19.0, DIM);
            ui.bar(850.0, y - 14.0, 330.0, 16.0, f, condition_colour(f));
            ui.text_right(&format!("{:.0}%", f * 100.0), 1250.0, y, 17.0, TEXT);
        }
        let by = 600.0;
        let mut act = 0;
        if ui.button(670.0, by, 270.0, 46.0, "Test drive (circuit)", true) {
            act = 1;
        }
        if ui.button(950.0, by, 150.0, 46.0, "Save", true) {
            act = 2;
        }
        if ui.button(1110.0, by, 140.0, 46.0, "Title", true) {
            act = 3;
        }
        if ui.button(
            670.0,
            by + 54.0,
            580.0,
            40.0,
            "Events: go racing (tab 7)",
            true,
        ) {
            self.tab = Tab::Events;
        }
        match act {
            1 => self.start_test_drive(),
            2 => self.save(),
            3 => {
                self.save();
                self.screen = Screen::Title;
            }
            _ => {}
        }
    }

    fn tab_parts(&mut self) {
        let mut pv = self.parts_view;
        {
            let ui = &mut self.ui;
            for (i, (v, l)) in [
                (PartsView::Engine, "Engines"),
                (PartsView::Tyres, "Tyres"),
                (PartsView::Upgrades, "Upgrades"),
            ]
            .iter()
            .enumerate()
            {
                let x = 670.0 + i as f32 * 130.0;
                let hover = ui.hovered(x, 70.0, 124.0, 30.0);
                ui.rect(
                    x,
                    70.0,
                    124.0,
                    30.0,
                    if *v == pv {
                        PANEL2
                    } else {
                        Color::new(0.1, 0.11, 0.13, 1.0)
                    },
                );
                if *v == pv {
                    ui.rect(x, 98.0, 124.0, 3.0, ACCENT);
                }
                ui.text_centered(l, x + 62.0, 92.0, 18.0, TEXT);
                if hover && ui.clicked {
                    pv = *v;
                }
            }
        }
        if pv != self.parts_view {
            self.parts_view = pv;
            self.scroll = 0.0;
            self.ui.reset_selection();
        }
        match self.parts_view {
            PartsView::Engine => self.parts_engines(),
            PartsView::Tyres => self.parts_tyres(),
            PartsView::Upgrades => self.parts_upgrades(),
        }
    }

    fn scroll_list(&mut self, count: usize, rows_visible: usize) -> usize {
        let max = count.saturating_sub(rows_visible) as f32;
        self.scroll = (self.scroll - self.ui.wheel * 0.05 * 1.0).clamp(0.0, max);
        self.scroll as usize
    }

    fn parts_engines(&mut self) {
        let engines = self.cat.engines.clone();
        let first = self.scroll_list(engines.len(), 9);
        let mut install: Option<String> = None;
        let gs = self.state.as_ref().unwrap();
        let current = gs.car().setup.engine.id.clone();
        let boost = gs.car().setup.engine.boost;
        let money_now = gs.money;
        let quotes: Vec<f32> = engines
            .iter()
            .map(|e| {
                gs.engine_swap_cost(&self.cat, gs.active, &e.id)
                    .unwrap_or(0.0)
            })
            .collect();
        let ui = &mut self.ui;
        ui.text(
            "Swap the engine. A new engine is mint condition and keeps your tune stage.",
            670.0,
            126.0,
            16.0,
            DIM,
        );
        for (row, (e, q)) in engines
            .iter()
            .zip(quotes.iter())
            .enumerate()
            .skip(first)
            .take(9)
        {
            let y = 140.0 + (row - first) as f32 * 52.0;
            let is_cur = e.id == current;
            ui.rect(
                660.0,
                y,
                596.0,
                48.0,
                if is_cur {
                    Color::new(0.14, 0.2, 0.16, 1.0)
                } else {
                    PANEL2
                },
            );
            ui.text(&e.display, 672.0, y + 22.0, 20.0, TEXT);
            ui.text(
                &format!(
                    "{:.1} L   {:.0} kW   {:.0} Nm   redline {:.0}",
                    e.displacement_l,
                    e.peak_power_kw() * boost,
                    e.peak_torque_nm * boost,
                    e.redline_rpm
                ),
                672.0,
                y + 41.0,
                15.0,
                DIM,
            );
            let label = if is_cur {
                "Installed".to_string()
            } else {
                format!("Install {}", money(*q))
            };
            if ui.button(
                1086.0,
                y + 6.0,
                160.0,
                36.0,
                &label,
                !is_cur && money_now >= *q,
            ) {
                install = Some(e.id.clone());
            }
        }
        if let Some(id) = install {
            let idx = self.state.as_ref().unwrap().active;
            let cat = self.cat.clone();
            match self.state.as_mut().unwrap().install_engine(&cat, idx, &id) {
                Ok(c) => self.say(format!("Engine installed for {}", money(c))),
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn parts_tyres(&mut self) {
        let tyres = self.cat.tyres.clone();
        let first = self.scroll_list(tyres.len(), 8);
        let mut buy: Option<String> = None;
        let mut axle = self.axle;
        let gs = self.state.as_ref().unwrap();
        let (cf, cr) = (
            gs.car().setup.tyre_front.id.clone(),
            gs.car().setup.tyre_rear.id.clone(),
        );
        let money_now = gs.money;
        let ui = &mut self.ui;
        ui.text(
            "Fit new tyres (they start unworn). Wider tyres grip more and weigh more.",
            670.0,
            126.0,
            16.0,
            DIM,
        );
        for (i, (a, l)) in [
            (Axle::Front, "Front"),
            (Axle::Rear, "Rear"),
            (Axle::Both, "Both"),
        ]
        .iter()
        .enumerate()
        {
            let x = 670.0 + i as f32 * 100.0;
            let hover = ui.hovered(x, 138.0, 94.0, 28.0);
            ui.rect(
                x,
                138.0,
                94.0,
                28.0,
                if *a == axle {
                    ACCENT
                } else if hover {
                    PANEL2
                } else {
                    Color::new(0.14, 0.16, 0.19, 1.0)
                },
            );
            ui.text_centered(l, x + 47.0, 158.0, 18.0, TEXT);
            if hover && ui.clicked {
                axle = *a;
            }
        }
        for (row, t) in tyres.iter().enumerate().skip(first).take(8) {
            let y = 176.0 + (row - first) as f32 * 46.0;
            let price = t.price() * if axle == Axle::Both { 4.0 } else { 2.0 };
            let on = (t.id == cf && axle != Axle::Rear) || (t.id == cr && axle != Axle::Front);
            ui.rect(
                660.0,
                y,
                596.0,
                42.0,
                if on {
                    Color::new(0.14, 0.2, 0.16, 1.0)
                } else {
                    PANEL2
                },
            );
            ui.text(
                &format!("{}   rim {:.1}\"", t.label(), t.rim_width_in),
                672.0,
                y + 19.0,
                19.0,
                TEXT,
            );
            ui.text(
                &format!(
                    "radius {:.0} mm   grip {:.2}   {:.1} kg",
                    t.radius_m() * 1000.0,
                    t.mu(),
                    t.mass_kg()
                ),
                672.0,
                y + 36.0,
                14.0,
                DIM,
            );
            if ui.button(
                1086.0,
                y + 3.0,
                160.0,
                36.0,
                &format!("Fit {}", money(price.round())),
                money_now >= price.round(),
            ) {
                buy = Some(t.id.clone());
            }
        }
        self.axle = axle;
        if let Some(id) = buy {
            let idx = self.state.as_ref().unwrap().active;
            let cat = self.cat.clone();
            match self
                .state
                .as_mut()
                .unwrap()
                .install_tyres(&cat, idx, &id, axle)
            {
                Ok(c) => self.say(format!("Tyres fitted for {}", money(c))),
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn parts_upgrades(&mut self) {
        let gs = self.state.as_ref().unwrap();
        let stage = gs.car().tune_stage;
        let has_lsd = gs.car().has_lsd;
        let money_now = gs.money;
        let ui = &mut self.ui;
        let mut set_stage: Option<u8> = None;
        let mut lsd = false;
        ui.text(
            "Engine tuning stages raise the torque curve.",
            670.0,
            126.0,
            16.0,
            DIM,
        );
        let labels = ["Stock", "Stage 1  +8%", "Stage 2  +17%", "Stage 3  +28%"];
        for (i, l) in labels.iter().enumerate() {
            let y = 146.0 + i as f32 * 56.0;
            let cost = if i == 0 {
                0.0
            } else {
                GameState::TUNE_COSTS[i - 1]
                    - if stage > 0 {
                        GameState::TUNE_COSTS[stage as usize - 1] * 0.5
                    } else {
                        0.0
                    }
            };
            ui.rect(
                660.0,
                y,
                596.0,
                50.0,
                if stage as usize == i {
                    Color::new(0.14, 0.2, 0.16, 1.0)
                } else {
                    PANEL2
                },
            );
            ui.text(l, 672.0, y + 31.0, 22.0, TEXT);
            let text = if stage as usize == i {
                "Installed".to_string()
            } else if i as u8 > stage {
                format!("Install {}", money(cost.round()))
            } else {
                "Downgrade (free)".to_string()
            };
            if ui.button(
                1056.0,
                y + 7.0,
                190.0,
                36.0,
                &text,
                stage as usize != i && (cost <= money_now),
            ) {
                set_stage = Some(i as u8);
            }
        }
        let y = 146.0 + 4.0 * 56.0 + 14.0;
        ui.rect(
            660.0,
            y,
            596.0,
            50.0,
            if has_lsd {
                Color::new(0.14, 0.2, 0.16, 1.0)
            } else {
                PANEL2
            },
        );
        ui.text("Limited-slip differential", 672.0, y + 31.0, 22.0, TEXT);
        if ui.button(
            1056.0,
            y + 7.0,
            190.0,
            36.0,
            if has_lsd { "Installed" } else { "Install $650" },
            !has_lsd && money_now >= 650.0,
        ) {
            lsd = true;
        }
        let idx = self.state.as_ref().unwrap().active;
        if let Some(s) = set_stage {
            match self.state.as_mut().unwrap().set_tune_stage(idx, s) {
                Ok(c) => self.say(format!("Tune applied ({})", money(c))),
                Err(e) => self.say(e.to_string()),
            }
        }
        if lsd {
            match self.state.as_mut().unwrap().buy_lsd(idx) {
                Ok(c) => self.say(format!("Differential installed ({})", money(c))),
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn tab_tuning(&mut self) {
        let perf = self.perf();
        let gs = self.state.as_mut().unwrap();
        let setup = gs.car().setup.clone();
        let has_lsd = gs.car().has_lsd;
        let ui = &mut self.ui;
        ui.text(
            "Tuning is free and takes effect immediately.",
            670.0,
            100.0,
            16.0,
            DIM,
        );
        let w = 580.0;
        let fd = ui.slider(
            670.0,
            120.0,
            w,
            "Final drive",
            setup.final_drive_scale,
            0.7,
            1.4,
            &|v| format!("{:.2}x", v),
            true,
        );
        let bb = ui.slider(
            670.0,
            160.0,
            w,
            "Brake bias (front)",
            setup.brake_bias,
            0.3,
            0.8,
            &|v| format!("{:.0}%", v * 100.0),
            true,
        );
        let wing = ui.slider(
            670.0,
            200.0,
            w,
            "Rear wing",
            setup.wing,
            0.0,
            1.0,
            &|v| format!("{:.0}%", v * 100.0),
            true,
        );
        let lsd = ui.slider(
            670.0,
            240.0,
            w,
            "Diff lock",
            setup.lsd,
            0.0,
            1.0,
            &|v| format!("{:.0}%", v * 100.0),
            has_lsd,
        );
        if !has_lsd {
            ui.text(
                "(install a limited-slip differential in Parts > Upgrades)",
                680.0,
                282.0,
                14.0,
                DIM,
            );
        }
        let toggle = ui.button(
            670.0,
            296.0,
            280.0,
            40.0,
            if setup.automatic {
                "Gearbox: automatic"
            } else {
                "Gearbox: manual (Q/E)"
            },
            true,
        );
        let reset = ui.button(970.0, 296.0, 280.0, 40.0, "Reset tuning", true);
        gs.set_final_drive(fd).ok();
        gs.set_brake_bias(bb);
        gs.set_wing(wing);
        if has_lsd {
            gs.set_lsd(lsd).ok();
        }
        if toggle {
            gs.set_automatic(!setup.automatic);
        }
        if reset {
            gs.set_final_drive(1.0).ok();
            gs.set_brake_bias(0.6);
            gs.set_wing(0.3);
            if has_lsd {
                gs.set_lsd(0.4).ok();
            }
        }
        let ui = &mut self.ui;
        ui.text("EFFECT", 670.0, 380.0, 18.0, ACCENT);
        let z = perf
            .zero_to_100_s
            .map_or("-".to_string(), |t| format!("{t:.1} s"));
        let q = perf.quarter_mile_s.map_or("-".to_string(), |t| {
            format!("{t:.1} s @ {:.0} km/h", perf.trap_kph)
        });
        for (i, (k, v)) in [
            ("0-100 km/h", z),
            ("402 m", q),
            ("Top speed", format!("{:.0} km/h", perf.top_kph)),
        ]
        .iter()
        .enumerate()
        {
            ui.text(k, 670.0, 412.0 + i as f32 * 30.0, 20.0, DIM);
            ui.text(v, 850.0, 412.0 + i as f32 * 30.0, 20.0, TEXT);
        }
        ui.text("Shorter final drive: quicker launch, lower top speed. More wing: grip at speed, more drag.", 670.0, 540.0, 15.0, DIM);
        ui.text(
            "Brake bias moves braking force between axles; too far forward and the fronts lock.",
            670.0,
            562.0,
            15.0,
            DIM,
        );
    }

    fn tab_mechanic(&mut self) {
        let gs = self.state.as_ref().unwrap();
        let idx = gs.active;
        let c = gs.car().clone();
        let repair = gs.repair_quote(idx).unwrap_or(0.0);
        let tyres = gs.tyre_replacement_quote(idx).unwrap_or(0.0);
        let money_now = gs.money;
        let ui = &mut self.ui;
        ui.text("Mechanic", 670.0, 106.0, 26.0, TEXT);
        ui.text("Repairs restore the engine, bodywork and brakes. Worn tyres are replaced like for like.", 670.0, 132.0, 16.0, DIM);
        let mut y = 176.0;
        ui.text("ENGINE", 670.0, y, 18.0, DIM);
        ui.bar(
            800.0,
            y - 14.0,
            360.0,
            16.0,
            c.cond.engine,
            condition_colour(c.cond.engine),
        );
        y += 34.0;
        for (i, n) in ["Front", "Rear", "Left", "Right"].iter().enumerate() {
            ui.text(&format!("BODY {n}"), 670.0, y, 18.0, DIM);
            ui.bar(
                800.0,
                y - 14.0,
                360.0,
                16.0,
                1.0 - c.cond.body[i],
                condition_colour(1.0 - c.cond.body[i]),
            );
            y += 30.0;
        }
        ui.text("BRAKES", 670.0, y, 18.0, DIM);
        ui.bar(
            800.0,
            y - 14.0,
            360.0,
            16.0,
            1.0 - c.cond.brake_wear,
            condition_colour(1.0 - c.cond.brake_wear),
        );
        y += 34.0;
        for (i, n) in ["FL", "FR", "RL", "RR"].iter().enumerate() {
            ui.text(&format!("TYRE {n}"), 670.0, y, 18.0, DIM);
            ui.bar(
                800.0,
                y - 14.0,
                360.0,
                16.0,
                1.0 - c.cond.tyre_wear[i],
                condition_colour(1.0 - c.cond.tyre_wear[i]),
            );
            y += 30.0;
        }
        let r = ui.button(
            670.0,
            520.0,
            580.0,
            46.0,
            &format!("Repair engine, body and brakes   {}", money(repair)),
            repair > 0.0 && money_now >= repair,
        );
        let t = ui.button(
            670.0,
            574.0,
            580.0,
            46.0,
            &format!("Replace worn tyres   {}", money(tyres)),
            tyres > 0.0 && money_now >= tyres,
        );
        if repair > money_now {
            ui.text(
                "Not enough money for the full repair. Race for prize money first.",
                670.0,
                645.0,
                16.0,
                WARN,
            );
        }
        let gs = self.state.as_mut().unwrap();
        let res = if r {
            Some(gs.repair(idx).map(|c| format!("Repaired for {}", money(c))))
        } else if t {
            Some(
                gs.replace_worn_tyres(idx)
                    .map(|c| format!("Tyres replaced for {}", money(c))),
            )
        } else {
            None
        };
        if let Some(res) = res {
            match res {
                Ok(s) => self.say(s),
                Err(e) => self.say(e.to_string()),
            }
            self.perf_cache = None;
        }
    }

    fn tab_painter(&mut self) {
        let gs = self.state.as_ref().unwrap();
        let cur = gs.car().paint;
        let money_now = gs.money;
        let idx = gs.active;
        let ui = &mut self.ui;
        ui.text("Painter", 670.0, 106.0, 26.0, TEXT);
        ui.text(
            "A respray costs $150. Pick a colour and watch the car on the left.",
            670.0,
            132.0,
            16.0,
            DIM,
        );
        let mut pick: Option<usize> = None;
        for (i, (name, rgb)) in PAINTS.iter().enumerate() {
            let (col, row) = (i % 4, i / 4);
            let x = 670.0 + col as f32 * 146.0;
            let y = 170.0 + row as f32 * 120.0;
            let hover = ui.hovered(x, y, 136.0, 110.0);
            ui.rect(x, y, 136.0, 110.0, if hover { ACCENT } else { PANEL2 });
            ui.rect(
                x + 8.0,
                y + 8.0,
                120.0,
                70.0,
                Color::from_rgba(rgb[0], rgb[1], rgb[2], 255),
            );
            ui.text_centered(name, x + 68.0, y + 100.0, 18.0, TEXT);
            if i == cur {
                ui.outline(x, y, 136.0, 110.0, 3.0, GOOD);
            }
            if hover && ui.clicked {
                pick = Some(i);
            }
        }
        if money_now < 150.0 {
            ui.text("Not enough money for a respray.", 670.0, 560.0, 18.0, WARN);
        }
        if let Some(p) = pick {
            match self.state.as_mut().unwrap().paint(idx, p) {
                Ok(c) => self.say(if c > 0.0 {
                    format!("Resprayed for {}", money(c))
                } else {
                    "Already that colour".into()
                }),
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn tab_cars(&mut self) {
        let mut view = self.cars_view;
        {
            let ui = &mut self.ui;
            for (i, (v, l)) in [
                (CarsView::Showroom, "Showroom"),
                (CarsView::Used, "Used lot"),
                (CarsView::Mine, "My cars"),
            ]
            .iter()
            .enumerate()
            {
                let x = 670.0 + i as f32 * 130.0;
                let hover = ui.hovered(x, 70.0, 124.0, 30.0);
                ui.rect(
                    x,
                    70.0,
                    124.0,
                    30.0,
                    if *v == view {
                        PANEL2
                    } else {
                        Color::new(0.1, 0.11, 0.13, 1.0)
                    },
                );
                if *v == view {
                    ui.rect(x, 98.0, 124.0, 3.0, ACCENT);
                }
                ui.text_centered(l, x + 62.0, 92.0, 18.0, TEXT);
                if hover && ui.clicked {
                    view = *v;
                }
            }
        }
        if view != self.cars_view {
            self.cars_view = view;
            self.scroll = 0.0;
            self.ui.reset_selection();
        }
        let gs = self.state.as_ref().unwrap();
        if self.used_day != gs.day || self.used_lot.is_empty() {
            self.used_lot = gs.used_lot(&self.cat);
            self.used_day = gs.day;
        }
        match self.cars_view {
            CarsView::Showroom => self.cars_showroom(),
            CarsView::Used => self.cars_used(),
            CarsView::Mine => self.cars_mine(),
        }
    }

    fn cars_showroom(&mut self) {
        let cars = self.cat.cars.clone();
        let first = self.scroll_list(cars.len(), 9);
        let money_now = self.state.as_ref().unwrap().money;
        let mut buy: Option<String> = None;
        let engines = self.cat.engines.clone();
        let ui = &mut self.ui;
        for (row, m) in cars.iter().enumerate().skip(first).take(9) {
            let y = 116.0 + (row - first) as f32 * 52.0;
            ui.rect(660.0, y, 596.0, 48.0, PANEL2);
            ui.text(
                &format!("{} {}", m.maker, m.name),
                672.0,
                y + 22.0,
                20.0,
                TEXT,
            );
            let kw = engines
                .iter()
                .find(|e| e.id == m.stock_engine)
                .map(|e| e.peak_power_kw())
                .unwrap_or(0.0);
            ui.text(
                &format!("{:.0} kW   {:.0} kg shell", kw, m.cfg.body_mass),
                672.0,
                y + 41.0,
                15.0,
                DIM,
            );
            if ui.button(
                1066.0,
                y + 6.0,
                180.0,
                36.0,
                &format!("Buy {}", money(m.base_price)),
                money_now >= m.base_price,
            ) {
                buy = Some(m.id.clone());
            }
        }
        if let Some(id) = buy {
            let cat = self.cat.clone();
            match self.state.as_mut().unwrap().buy_new(&cat, &id) {
                Ok(_) => {
                    self.say("New car bought and made active");
                    self.perf_cache = None;
                }
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn cars_used(&mut self) {
        let lot = self.used_lot.clone();
        let money_now = self.state.as_ref().unwrap().money;
        let mut buy: Option<usize> = None;
        let ui = &mut self.ui;
        ui.text(
            "Today's used cars. Condition varies; check the engine and tyres.",
            670.0,
            126.0,
            16.0,
            DIM,
        );
        for (row, l) in lot.iter().enumerate() {
            let y = 140.0 + row as f32 * 76.0;
            ui.rect(660.0, y, 596.0, 70.0, PANEL2);
            ui.text(
                &l.model.replace(['/', '_'], " "),
                672.0,
                y + 24.0,
                20.0,
                TEXT,
            );
            let tyre = 1.0 - l.condition.tyre_wear.iter().sum::<f32>() / 4.0;
            ui.text(
                &format!("{:.0} km", l.condition.odometer_km),
                672.0,
                y + 46.0,
                15.0,
                DIM,
            );
            for (k, (n, f)) in [
                ("eng", l.condition.engine),
                ("tyr", tyre),
                ("body", 1.0 - l.condition.body_total()),
            ]
            .iter()
            .enumerate()
            {
                let x = 760.0 + k as f32 * 100.0;
                ui.text(n, x, y + 46.0, 14.0, DIM);
                ui.bar(x + 34.0, y + 35.0, 56.0, 12.0, *f, condition_colour(*f));
            }
            if ui.button(
                1066.0,
                y + 17.0,
                180.0,
                36.0,
                &format!("Buy {}", money(l.price)),
                money_now >= l.price,
            ) {
                buy = Some(row);
            }
        }
        if let Some(i) = buy {
            let cat = self.cat.clone();
            let listing = self.used_lot[i].clone();
            match self.state.as_mut().unwrap().buy_used(&cat, &listing) {
                Ok(_) => {
                    self.used_lot.remove(i);
                    self.say("Used car bought and made active");
                    self.perf_cache = None;
                }
                Err(e) => self.say(e.to_string()),
            }
        }
    }

    fn cars_mine(&mut self) {
        let cat = self.cat.clone();
        let gs = self.state.as_ref().unwrap();
        let cars: Vec<(String, f32, bool)> = gs
            .cars
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    c.model.clone(),
                    gs.resale_value(&cat, i).unwrap_or(0.0),
                    i == gs.active,
                )
            })
            .collect();
        let ui = &mut self.ui;
        let mut select = None;
        let mut sell = None;
        for (i, (model, value, active)) in cars.iter().enumerate().take(8) {
            let y = 116.0 + i as f32 * 60.0;
            ui.rect(
                660.0,
                y,
                596.0,
                54.0,
                if *active {
                    Color::new(0.14, 0.2, 0.16, 1.0)
                } else {
                    PANEL2
                },
            );
            ui.text(&model.replace(['/', '_'], " "), 672.0, y + 24.0, 20.0, TEXT);
            ui.text(
                &format!("worth {}", money(*value)),
                672.0,
                y + 44.0,
                15.0,
                DIM,
            );
            if ui.button(
                930.0,
                y + 9.0,
                150.0,
                36.0,
                if *active { "Active" } else { "Use this car" },
                !*active,
            ) {
                select = Some(i);
            }
            if ui.button(1090.0, y + 9.0, 156.0, 36.0, "Sell", cars.len() > 1) {
                sell = Some(i);
            }
        }
        let gs = self.state.as_mut().unwrap();
        if let Some(i) = select {
            gs.active = i;
            self.perf_cache = None;
        }
        if let Some(i) = sell {
            match gs.sell(&cat, i) {
                Ok(v) => self.say(format!("Sold for {}", money(v))),
                Err(e) => self.say(e.to_string()),
            }
            self.perf_cache = None;
        }
    }

    fn tab_events(&mut self) {
        let gs = self.state.as_ref().unwrap();
        let rep = gs.reputation;
        let money_now = gs.money;
        let best = gs.best_times.clone();
        let eng_id = gs.car().setup.engine.id.clone();
        let wear_warn = gs.car().cond.repair_cost() > 600.0;
        let ui = &mut self.ui;
        ui.text(&format!("Reputation {rep}. Win to earn prize money and reputation; second place pays a little."), 670.0, 100.0, 16.0, DIM);
        let mut go: Option<&'static str> = None;
        for (i, e) in EVENTS.iter().enumerate() {
            let y = 116.0 + i as f32 * 88.0;
            let locked = e.rep_required > rep;
            ui.rect(660.0, y, 596.0, 82.0, PANEL2);
            ui.text(
                e.name,
                672.0,
                y + 26.0,
                22.0,
                if locked { DIM } else { TEXT },
            );
            let kind = match e.kind {
                RaceKind::Drag => "402 m drag".to_string(),
                RaceKind::Circuit { laps } => format!("{laps} laps, {:.1} km", laps as f32 * 2.9),
            };
            ui.text(
                &format!(
                    "{kind}{}   vs {}",
                    if e.night { "   night" } else { "" },
                    e.opponent.replace(['/', '_'], " ")
                ),
                672.0,
                y + 48.0,
                15.0,
                DIM,
            );
            ui.text(
                &format!(
                    "fee {}   prize {}   +{} rep",
                    money(e.entry_fee),
                    money(e.prize),
                    e.rep_gain
                ),
                672.0,
                y + 68.0,
                15.0,
                if locked { DIM } else { GOOD },
            );
            if let Some(t) = best.get(&format!("{}:{}", e.id, eng_id)) {
                ui.text(
                    &format!("best {}", fmt_time(*t)),
                    940.0,
                    y + 26.0,
                    15.0,
                    WARN,
                );
            }
            let label = if locked {
                format!("Needs {} rep", e.rep_required)
            } else {
                "Race".to_string()
            };
            if ui.button(
                1096.0,
                y + 20.0,
                150.0,
                42.0,
                &label,
                !locked && money_now >= e.entry_fee,
            ) {
                go = Some(e.id);
            }
        }
        if wear_warn {
            ui.text(
                "Your car is badly worn: visit the mechanic first.",
                670.0,
                690.0,
                16.0,
                WARN,
            );
        }
        if let Some(id) = go {
            self.start_event(id);
        }
    }

    // ---- session overlays: pause + results ----

    fn draw_session_overlays(&mut self, s: &mut RaceSession, next: &mut Option<bool>) {
        let ui = &mut self.ui;
        if s.paused {
            ui.rect(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.6));
            ui.text_centered("PAUSED", VW / 2.0, 250.0, 60.0, TEXT);
            if ui.button(VW / 2.0 - 150.0, 290.0, 300.0, 48.0, "Resume", true) {
                s.paused = false;
            }
            let label = if s.event.is_some() {
                "Forfeit and leave"
            } else {
                "Back to garage"
            };
            if ui.button(VW / 2.0 - 150.0, 350.0, 300.0, 48.0, label, true) {
                *next = Some(true);
            }
        }
        // Results.
        if s.finished_waiting() && s.outcome.is_none() && !s.settled {
            let ev = s.event.unwrap();
            let gs = self.state.as_mut().unwrap();
            // Make sure unfinished AI are ranked by progress before settling.
            let out = gs.settle_race(&s.race, ev);
            s.outcome = Some(out);
            s.settled = true;
            let _ = gs.save(&self.args.save_dir.join("slot1.json"));
        }
        if let Some(out) = &s.outcome {
            let ui = &mut self.ui;
            ui.rect(0.0, 0.0, VW, VH, Color::new(0.0, 0.0, 0.0, 0.55));
            ui.rect(VW / 2.0 - 280.0, 150.0, 560.0, 420.0, PANEL);
            ui.rect(
                VW / 2.0 - 280.0,
                150.0,
                560.0,
                6.0,
                if out.won { GOOD } else { ACCENT },
            );
            ui.text_centered(
                if out.won { "VICTORY" } else { "DEFEAT" },
                VW / 2.0,
                220.0,
                64.0,
                if out.won { GOOD } else { ACCENT },
            );
            ui.text_centered(
                &format!("Finished P{} of {}", out.position, s.race.racers.len()),
                VW / 2.0,
                262.0,
                26.0,
                TEXT,
            );
            let mut y = 310.0;
            let rows = [
                ("Time", out.time.map(fmt_time).unwrap_or_else(|| "-".into())),
                ("Entry fee", money(-out.entry_fee)),
                ("Prize", money(out.prize)),
                ("Reputation", format!("+{}", out.rep_gain)),
                ("Repairs needed", money(out.repair_estimate.round())),
            ];
            for (k, v) in rows {
                ui.text(k, VW / 2.0 - 220.0, y, 22.0, DIM);
                ui.text_right(&v, VW / 2.0 + 220.0, y, 22.0, TEXT);
                y += 36.0;
            }
            if out.new_record {
                ui.text_centered("New personal best!", VW / 2.0, y + 6.0, 22.0, WARN);
            }
            if ui.button(VW / 2.0 - 150.0, 500.0, 300.0, 50.0, "Back to garage", true) {
                *next = Some(false);
            }
        }
    }
}
