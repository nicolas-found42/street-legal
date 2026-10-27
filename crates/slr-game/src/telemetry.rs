//! Crash-surviving diagnostics for the interactive frontend.
//!
//! Enabled with `--diagnostics DIR`. The streams match the headless boot's
//! (`events.jsonl`, `states.jsonl`, `file-io.jsonl`, `environment.json`,
//! `capture.json`, `capture-end.json`, `crash.json`), so one reader handles
//! both. The frontend has no native call log: the VM does not run here, so
//! `runtime.calllog` stays an empty, valid header.
//!
//! Checkpoints are periodic rather than per-frame: a frame is 16 ms of work
//! and a `states.jsonl` line is a few hundred bytes, so `--diagnostics-interval`
//! (default 30 frames) keeps a long session inspectable without becoming the
//! dominant cost. Every state transition is recorded immediately regardless.
use crate::app::{App, Screen};
use serde_json::{json, Value};
use slr_core::race::Phase;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

thread_local! {
    /// The last checkpoint, so a panic hook can publish it without touching
    /// the app (which may be mid-borrow or mid-unwind).
    static LAST: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// The frontend's diagnostic session.
pub struct Diagnostics {
    pub enabled: bool,
    /// Where the bundle lives; exposed for the end-of-run summary.
    pub directory: PathBuf,
    /// Frames between periodic checkpoints; 0 checkpoints only on transitions.
    pub interval: u64,
    last_frame: u64,
    /// UTC start, recorded when the capture opened.
    started: String,
    screen: String,
    /// Sessions started this run (the checkpoint's session index).
    pub sessions: u64,
    finished: bool,
}

impl Diagnostics {
    #[must_use]
    pub fn disabled() -> Diagnostics {
        Diagnostics {
            enabled: false,
            directory: PathBuf::new(),
            interval: 30,
            last_frame: 0,
            started: String::new(),
            screen: String::new(),
            sessions: 0,
            finished: false,
        }
    }

    /// Open `directory` as a fresh run. Fails when the path already holds a
    /// capture: a diagnostic run never overwrites evidence.
    pub fn start(directory: &Path, interval: u64) -> Result<Diagnostics, String> {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("DiagnosticsBoundary: {}: {error}", directory.display()))?;
        let run_id = directory.file_name().map_or_else(
            || format!("frontend-{}", std::process::id()),
            |name| name.to_string_lossy().into_owned(),
        );
        // One JSON event line per frame is the useful default for a frontend.
        slr_telemetry::configure(directory, &run_id, 8192, false, true, true)?;
        slr_telemetry::install_panic_hook(crash_context);
        slr_telemetry::detail(
            "frontend-start",
            vec![
                ("run_id".into(), run_id),
                ("interval".into(), interval.to_string()),
            ],
        );
        // A manifest exists from the first frame, so a crash leaves a
        // discoverable bundle rather than an anonymous directory.
        slr_telemetry::write_manifest(serde_json::json!({
            "embedding": "slr-game",
            "status": "running",
            "started": slr_telemetry::utc_now(),
            "configuration": {"diagnostics_interval": interval},
        }))?;
        Ok(Diagnostics {
            enabled: true,
            directory: directory.to_owned(),
            interval,
            last_frame: 0,
            started: slr_telemetry::utc_now(),
            screen: String::new(),
            sessions: 0,
            finished: false,
        })
    }

    /// Record a named occurrence (`save`, `load`, `session-start`, ...).
    pub fn event(&self, category: &str, fields: Vec<(&str, String)>) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        slr_telemetry::detail(
            category,
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    /// Publish a prepared snapshot (the caller owns the app borrow).
    pub fn checkpoint(&mut self, value: &Value) {
        if !self.enabled {
            return;
        }
        LAST.with(|last| *last.borrow_mut() = Some(value.clone()));
        if let Err(error) = slr_telemetry::write_state(value) {
            slr_telemetry::log::eprintln(
                slr_telemetry::log::Level::Error,
                "frontend.diagnostics",
                &format!("street-legal: diagnostics: {error}"),
            );
        }
        self.last_frame = frame_of(value);
    }

    /// Called at the end of every frame with that frame's snapshot.
    pub fn tick(&mut self, frame: u64, snapshot: &Value) {
        if !self.enabled {
            return;
        }
        let screen = snapshot["screen"].as_str().unwrap_or_default().to_owned();
        if screen != self.screen {
            let previous = std::mem::replace(&mut self.screen, screen.clone());
            self.event(
                "screen",
                vec![
                    ("from", previous),
                    ("to", screen),
                    ("frame", frame.to_string()),
                ],
            );
            self.checkpoint(snapshot);
            return;
        }
        if self.interval > 0 && frame.saturating_sub(self.last_frame) >= self.interval {
            self.checkpoint(snapshot);
        }
    }

    /// Count a new session and publish its starting state. `frame` is the
    /// app's frame number; the app cannot be borrowed here because the caller
    /// already holds it mutably.
    pub fn session_started(&mut self, frame: u64) {
        self.sessions += 1;
        self.event(
            "session-start",
            vec![
                ("session", self.sessions.to_string()),
                ("frame", frame.to_string()),
            ],
        );
    }

    pub fn autotest(&self, passes: &[String], failures: &[String]) {
        if !self.enabled {
            return;
        }
        let value = json!({
            "schema_version": 1,
            "run_id": slr_telemetry::run_id(),
            "wall_ns": slr_telemetry::monotonic_ns(),
            "passes": passes,
            "failures": failures,
            "passed": passes.len(),
            "failed": failures.len(),
        });
        if let Err(error) = slr_telemetry::write_artifact("autotest.json", &value) {
            slr_telemetry::log::eprintln(
                slr_telemetry::log::Level::Error,
                "frontend.diagnostics",
                &format!("street-legal: diagnostics: {error}"),
            );
        }
        self.event(
            "autotest",
            vec![
                ("passed", passes.len().to_string()),
                ("failed", failures.len().to_string()),
            ],
        );
    }

    /// Record one file operation the frontend performed (saves are the only
    /// files it writes), so the bundle answers "which save, when, how big".
    pub fn file(&self, op: &str, path: &Path, detail: Value) {
        if self.enabled {
            slr_telemetry::file_io(op, &path.display().to_string(), detail);
        }
    }

    /// Report the bundle's location once, at the end of the run.
    pub fn announce(&self) {
        if self.enabled {
            slr_telemetry::log::eprintln(
                slr_telemetry::log::Level::Info,
                "frontend.diagnostics",
                &format!("street-legal: diagnostics {}", self.directory.display()),
            );
        }
    }

    /// Close the streams, then publish the manifest and the completion marker.
    ///
    /// `status` is `completed` or `failed`; the archive tools read the same
    /// fields the headless runner writes, so a frontend bundle can be listed,
    /// queried and pruned alongside a boot bundle.
    pub fn finish(&mut self, snapshot: Value, configuration: Value, failure: Option<&str>) {
        if !self.enabled || self.finished {
            return;
        }
        self.finished = true;
        let mut value = serde_json::json!({
            "embedding": "slr-game",
            "status": if failure.is_some() { "failed" } else { "completed" },
            "error": failure,
            "sessions": self.sessions,
            "started": self.started,
            "finished": slr_telemetry::utc_now(),
            "configuration": configuration,
            "final": snapshot,
        });
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "comparisons".into(),
                serde_json::json!({"filtered": {"equal": null}, "unfiltered": {"equal": null}}),
            );
        }
        // Finish first: it writes `capture-end.json`, which the manifest must
        // then inventory. The manifest is therefore always the last artifact.
        if let Err(error) = slr_telemetry::finish(failure) {
            slr_telemetry::log::eprintln(
                slr_telemetry::log::Level::Error,
                "frontend.diagnostics",
                &format!("street-legal: diagnostics: {error}"),
            );
            return;
        }
        if let Err(error) = slr_telemetry::write_manifest(value) {
            slr_telemetry::log::eprintln(
                slr_telemetry::log::Level::Error,
                "frontend.diagnostics",
                &format!("street-legal: diagnostics: {error}"),
            );
        }
    }
}

fn frame_of(value: &Value) -> u64 {
    value["frame"].as_u64().unwrap_or(0)
}

fn screen_name(app: &App) -> String {
    match app.screen {
        Screen::Title => "title".into(),
        Screen::Garage => "garage".into(),
        Screen::Session(_) => "session".into(),
    }
}

/// What a panicking process can still report: the last published checkpoint
/// plus the shared causal context.
fn crash_context() -> Value {
    let mut value = slr_telemetry::crash_context();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "last_checkpoint".into(),
            LAST.with(|last| last.borrow().clone().unwrap_or(Value::Null)),
        );
    }
    value
}

impl App {
    /// The state a checkpoint publishes: what the player sees, what the
    /// simulation holds, and what the last frame did.
    pub(crate) fn snapshot(&self, phase: &str, session: u64) -> Value {
        let state = self.state.as_ref().map(|state| {
            let car = state.car();
            json!({
                "money_bits": state.money.to_bits(),
                "reputation": state.reputation,
                "day": state.day,
                "races": state.races,
                "wins": state.wins,
                "seed": state.seed,
                "car": {
                    "model": car.model,
                    "paint": car.paint,
                    "tune_stage": car.tune_stage,
                    "odometer_km_bits": car.cond.odometer_km.to_bits(),
                    "engine_bits": car.cond.engine.to_bits(),
                    "tyre_wear_bits": car.cond.tyre_wear.map(f32::to_bits),
                    "brake_wear_bits": car.cond.brake_wear.to_bits(),
                    "body_bits": car.cond.body.map(f32::to_bits),
                },
            })
        });
        let race = match &self.screen {
            Screen::Session(session) => Some(json!({
                "phase": match session.race.phase {
                    Phase::Countdown => "countdown",
                    Phase::Racing => "racing",
                    Phase::Finished => "finished",
                },
                "time_bits": session.race.time.to_bits(),
                "elapsed_bits": session.elapsed.to_bits(),
                "paused": session.paused,
                "free_run": session.free_run,
                "event": session.event.map(|event| event.id),
                "settled": session.settled,
                "outcome": session.outcome.as_ref().map(|outcome| format!("{outcome:?}")),
                "racers": session.race.racers.iter().map(|racer| json!({
                    "name": racer.name,
                    "player": racer.is_player,
                    "pos_bits": racer.vehicle.pos.map(f32::to_bits),
                    "vel_bits": racer.vehicle.vel.map(f32::to_bits),
                    "heading_bits": racer.vehicle.heading.to_bits(),
                    "speed_bits": racer.vehicle.speed().to_bits(),
                    "gear": racer.vehicle.gear,
                    "rpm_bits": racer.vehicle.rpm.to_bits(),
                    "lap": racer.lap,
                    "progress_bits": racer.progress.to_bits(),
                    "finish_time_bits": racer.finish_time.map(f32::to_bits),
                    "penalty_bits": racer.penalty.to_bits(),
                    "false_start": racer.false_start,
                })).collect::<Vec<_>>(),
            })),
            _ => None,
        };
        // The checkpoint joins the timeline by a real event identity, so a
        // reader can validate that every state line is anchored.
        let sequence =
            slr_telemetry::detail("state-checkpoint", vec![("phase".into(), phase.to_owned())]);
        let frame = json!({
            "schema_version": 1,
            "phase": phase,
            "sequence": sequence,
            "provenance": slr_telemetry::provenance(),
            "frame": self.frame_no,
            "screen": screen_name(self),
            "session_index": session,
            "reference_loaded": self.reference_loaded,
            "save_path": self.save_path().display().to_string(),
            "game": state,
            "race": race,
            "wall_ns": slr_telemetry::monotonic_ns(),
        });
        LAST.with(|last| *last.borrow_mut() = Some(frame.clone()));
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Args, Screen};
    use std::path::PathBuf;

    fn app() -> App {
        App::new(
            Args {
                reference: PathBuf::from("reference/raw"),
                save_dir: PathBuf::from("saves"),
                autotest: false,
                shots: PathBuf::from("target/autotest"),
                time_scale: 1.0,
                diagnostics: None,
                diagnostics_interval: 30,
            },
            slr_core::catalog::Catalog::fallback(),
            false,
            None,
        )
    }

    #[test]
    fn a_disabled_session_writes_nothing_and_answers_nothing() {
        let mut app = app();
        assert!(!app.diagnostics.enabled);
        assert_eq!(app.diagnostics.event("x", vec![]), None);
        app.diagnostics.session_started(0);
        app.finish_diagnostics(None);
        assert_eq!(app.diagnostics.sessions, 1);
    }

    #[test]
    fn the_checkpoint_carries_floats_as_exact_bits() {
        let mut app = app();
        // `App::new_game` reads macroquad's clock, which needs a live context;
        // the state it produces is a plain `GameState`.
        app.state = Some(slr_core::state::GameState::new_game(&app.cat, 7));
        app.screen = Screen::Garage;
        let value = app.snapshot("test", 1);
        assert_eq!(value["screen"], "garage");
        assert_eq!(value["game"]["day"], 1);
        // Money is published as bits so a NaN or -0.0 would survive JSON.
        assert!(value["game"]["money_bits"].as_u64().is_some());
        assert!(value["race"].is_null());
        app.screen = Screen::Title;
        assert_eq!(screen_name(&app), "title");
    }

    #[test]
    fn every_save_reaches_the_audit_stream() {
        let directory = std::env::temp_dir().join(format!(
            "slr-game-save-{}-{}",
            std::process::id(),
            slr_telemetry::monotonic_ns()
        ));
        let mut app = app();
        app.args.save_dir = directory.join("saves");
        std::fs::create_dir_all(&app.args.save_dir).unwrap();
        app.args.diagnostics = Some(directory.join("bundle"));
        app.start_diagnostics().unwrap();
        app.state = Some(slr_core::state::GameState::new_game(&app.cat, 7));
        // Both the player's save and the automatic race save are audited; a
        // save that bypasses the traced path is invisible to the archive.
        app.save();
        app.write_save("race-settled").unwrap().unwrap();
        app.finish_diagnostics(None);
        let stream = std::fs::read_to_string(directory.join("bundle/file-io.jsonl")).unwrap();
        assert_eq!(stream.lines().count(), 2, "{stream}");
        assert!(stream.contains("\"op\":\"save\""), "{stream}");
        assert!(stream.contains("\"reason\":\"race-settled\""), "{stream}");
        // The bytes recorded are the bytes on disk.
        let saved = std::fs::metadata(app.save_path()).unwrap().len();
        assert!(stream.contains(&format!("\"bytes\":{saved}")), "{stream}");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn frames_are_the_serialized_identity_not_the_event_sequence() {
        let mut app = app();
        app.frame_no = 42;
        let value = app.snapshot("frame", 3);
        assert_eq!(frame_of(&value), 42);
        assert_eq!(value["session_index"], 3);
        // With diagnostics disabled there is no timeline to join, so the
        // sequence is explicitly null rather than a made-up number.
        assert!(value["sequence"].is_null());
    }
}
