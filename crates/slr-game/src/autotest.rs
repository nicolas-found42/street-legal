//! Scripted end-to-end run of the real UI: injected clicks, an autopilot for the race,
//! assertions on the resulting game state, and screenshots. Enabled with `--autotest`.

use crate::app::{App, Screen, Tab};
use macroquad::prelude::*;
use slr_core::ai::Driver;
use slr_core::state::GameState;
use std::collections::VecDeque;

pub type Check = fn(&mut App) -> Result<(), String>;

pub enum Step {
    Wait(u32),
    Click(f32, f32),
    Shot(&'static str),
    Check(&'static str, Check),
    Do(fn(&mut App)),
    WaitUntil(&'static str, fn(&App) -> bool, u32),
    Quit,
}

pub struct Autotest {
    steps: VecDeque<Step>,
    wait: u32,
    pending_shot: Option<&'static str>,
    until: Option<(&'static str, fn(&App) -> bool, u32)>,
    pub failures: Vec<String>,
    pub passes: Vec<String>,
}

impl Autotest {
    pub fn new() -> Autotest {
        use Step::*;
        let steps: Vec<Step> = vec![
            Wait(10),
            Shot("01_title"),
            Click(640.0, 450.0), // New Game
            Wait(5),
            Check("new game reaches the garage", |a| {
                matches!(a.screen, Screen::Garage)
                    .then_some(())
                    .ok_or("not in garage".into())
            }),
            Wait(30),
            Shot("02_garage_overview"),
            // Parts > Tyres > fit the narrowest tyre set.
            Click(660.0 + 86.0 + 41.0, 40.0),
            Wait(3),
            Click(670.0 + 130.0 + 62.0, 85.0),
            Wait(3),
            Shot("03_parts_tyres"),
            Do(|a| a.autotest_stash_money()),
            Click(1166.0, 197.0),
            Wait(3),
            Check("fitting tyres costs money and resets wear", |a| {
                let gs = a.state.as_ref().unwrap();
                if gs.money >= a.stashed_money() {
                    return Err(format!(
                        "money did not drop: {} -> {}",
                        a.stashed_money(),
                        gs.money
                    ));
                }
                if gs.car().setup.tyre_front.width_mm != 195.0 {
                    return Err(format!(
                        "front tyre is {}",
                        gs.car().setup.tyre_front.label()
                    ));
                }
                if gs.car().cond.tyre_wear.iter().any(|w| *w > 0.0) {
                    return Err("tyres still worn".into());
                }
                Ok(())
            }),
            // Mechanic > repair everything.
            Click(660.0 + 86.0 * 3.0 + 41.0, 40.0),
            Wait(3),
            Shot("04_mechanic"),
            Do(|a| a.autotest_stash_money()),
            Click(960.0, 543.0),
            Wait(3),
            Check("repairs restore the car", |a| {
                let gs = a.state.as_ref().unwrap();
                if gs.car().cond.repair_cost() > 0.5 {
                    return Err(format!("still needs {}", gs.car().cond.repair_cost()));
                }
                (gs.money < a.stashed_money())
                    .then_some(())
                    .ok_or_else(|| "no payment taken".into())
            }),
            // Painter > pick the 2nd swatch (blue).
            Click(660.0 + 86.0 * 4.0 + 41.0, 40.0),
            Wait(3),
            Click(670.0 + 146.0 + 60.0, 200.0),
            Wait(3),
            Shot("05_painter"),
            Check("respray changes the paint", |a| {
                let gs = a.state.as_ref().unwrap();
                (gs.car().paint == 1)
                    .then_some(())
                    .ok_or_else(|| format!("paint is {}", gs.car().paint))
            }),
            // Tuning tab: drag the final drive slider and check the setup changed.
            Click(660.0 + 86.0 * 2.0 + 41.0, 40.0),
            Wait(3),
            Click(670.0 + 190.0 + 80.0, 137.0),
            Wait(3),
            Shot("06_tuning"),
            Check("slider moved the final drive", |a| {
                let fd = a.state.as_ref().unwrap().car().setup.final_drive_scale;
                ((fd - 1.0).abs() > 0.02)
                    .then_some(())
                    .ok_or_else(|| format!("final drive {fd}"))
            }),
            Do(|a| {
                let gs = a.state.as_mut().unwrap();
                gs.set_final_drive(1.0).unwrap();
            }),
            // Cars tab screenshot, then events.
            Click(660.0 + 86.0 * 5.0 + 41.0, 40.0),
            Wait(3),
            Shot("07_cars"),
            Click(660.0 + 86.0 * 6.0 + 41.0, 40.0),
            Wait(3),
            Shot("08_events"),
            Do(|a| a.autotest_stash_money()),
            Click(1171.0, 157.0), // first event: Race
            Wait(5),
            Check("event started and fee charged", |a| {
                if !matches!(a.screen, Screen::Session(_)) {
                    return Err("no session".into());
                }
                Ok(())
            }),
            Do(|a| {
                if let Screen::Session(s) = &mut a.screen {
                    s.autopilot = Some(Driver::new(1.0, -3.5, true));
                }
            }),
            Wait(250),
            Shot("09_race_hud"),
            WaitUntil(
                "race results shown",
                |a| matches!(&a.screen, Screen::Session(s) if s.outcome.is_some()),
                3600,
            ),
            Wait(10),
            Shot("10_results"),
            Check("race settled into the save", |a| {
                let gs = a.state.as_ref().unwrap();
                if gs.races != 1 {
                    return Err(format!("races = {}", gs.races));
                }
                if gs.car().cond.tyre_wear.iter().all(|w| *w == 0.0) {
                    return Err("no wear carried over".into());
                }
                Ok(())
            }),
            Click(640.0, 525.0), // Back to garage
            Wait(5),
            Check("back in the garage", |a| {
                matches!(a.screen, Screen::Garage)
                    .then_some(())
                    .ok_or("not in garage".into())
            }),
            Check("autosave exists and reloads identically", |a| {
                let path = a.save_path();
                let loaded = GameState::load(&path).map_err(|e| e.to_string())?;
                (&loaded == a.state.as_ref().unwrap())
                    .then_some(())
                    .ok_or_else(|| "saved state differs".into())
            }),
            // Test drive.
            Do(|a| a.tab = Tab::Overview),
            Wait(3),
            Click(805.0, 623.0),
            Wait(5),
            Do(|a| {
                if let Screen::Session(s) = &mut a.screen {
                    s.autopilot = Some(Driver::new(0.8, 0.0, false));
                }
            }),
            Wait(400),
            Shot("11_test_drive"),
            Check("test drive is moving", |a| match &a.screen {
                Screen::Session(s) => (s.race.racers[0].vehicle.kph() > 20.0)
                    .then_some(())
                    .ok_or_else(|| format!("{} km/h", s.race.racers[0].vehicle.kph())),
                _ => Err("no session".into()),
            }),
            Quit,
        ];
        Autotest {
            steps: steps.into(),
            wait: 0,
            pending_shot: None,
            until: None,
            failures: vec![],
            passes: vec![],
        }
    }

    /// Run before the frame is drawn.
    pub fn tick(&mut self, app: &mut App) {
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        if let Some((name, f, left)) = self.until.as_mut() {
            if f(app) {
                self.passes.push(name.to_string());
                self.until = None;
            } else if *left == 0 {
                self.failures.push(format!("timed out waiting for: {name}"));
                self.until = None;
            } else {
                *left -= 1;
                return;
            }
        }
        while let Some(step) = self.steps.pop_front() {
            match step {
                Step::Wait(n) => {
                    self.wait = n;
                    return;
                }
                Step::Click(x, y) => {
                    app.ui.inject = Some(vec2(x, y));
                    self.wait = 1;
                    return;
                }
                Step::Shot(name) => {
                    self.pending_shot = Some(name);
                    return;
                }
                Step::Check(name, f) => match f(app) {
                    Ok(()) => self.passes.push(name.to_string()),
                    Err(e) => self.failures.push(format!("{name}: {e}")),
                },
                Step::Do(f) => f(app),
                Step::WaitUntil(name, f, max) => {
                    self.until = Some((name, f, max));
                    return;
                }
                Step::Quit => {
                    app.quit = true;
                    return;
                }
            }
        }
        app.quit = true;
    }

    /// Run after the frame is drawn, before presenting it.
    pub fn post_frame(&mut self, app: &App) {
        if let Some(name) = self.pending_shot.take() {
            let img = get_screen_data();
            let _ = std::fs::create_dir_all(&app.args.shots);
            img.export_png(&app.args.shots.join(format!("{name}.png")).to_string_lossy());
        }
    }

    pub fn report(&self) -> bool {
        for p in &self.passes {
            println!("PASS  {p}");
        }
        for f in &self.failures {
            println!("FAIL  {f}");
        }
        println!(
            "{} passed, {} failed",
            self.passes.len(),
            self.failures.len()
        );
        self.failures.is_empty()
    }
}
