//! Differential tests for the TUFA interpreter loop (M1-03, issue #30).
//!
//! Replays `re/out/thread/thread_run.jsonl`, produced by
//! `re/oracle/thread_vectors.py gen`: the REAL exe `Thread::run`
//! (`FUN_00420ff0`) executed under Unicorn on hand-built programs (the
//! exe's own Thread and frame constructors, node arrays, locals and
//! operand stacks in emulator memory; no JVM boot, no game data). Override
//! the directory with `SLR_THREAD_VECTORS`. Skips with a note when the file
//! is absent so `cargo test --workspace` stays green without the exe/venv.
//!
//! Per row the test rebuilds the same initial thread in the port, runs it
//! with the recorded budget and clock readings and compares the final
//! machine state bit for bit: outcome (stopped / fatal script error /
//! integer trap), thread flags, current frame, pc, the live-cell counter,
//! the raw 10-byte x87 leftover-time return value, and for every live frame
//! its operand stack, locals and saved pc.

use std::path::PathBuf;

use slr_fmt_tufa::Node;
use slr_vm::interp::{BodyId, Frame, Host, Pc, Slot, Thread, VmError};
use slr_vm::values::{TypeTag, Value};
use slr_x87::X87;

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_THREAD_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/thread")
}

fn tag_of(s: &str) -> TypeTag {
    match s {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        "L" => TypeTag::Ref("L;"),
        "V" => TypeTag::Ref("V"),
        "[" => TypeTag::Array("["),
        _ => panic!("unknown tag {s}"),
    }
}

fn tag_char(t: TypeTag) -> String {
    (t.byte() as char).to_string()
}

struct Bodies(Vec<Vec<Node>>);

impl Host for Bodies {
    fn body(&self, id: BodyId) -> &[Node] {
        &self.0[id.0 as usize]
    }
}

fn value_of(c: &serde_json::Value) -> Value {
    Value {
        tag: tag_of(c["t"].as_str().unwrap()),
        payload: c["w"].as_u64().unwrap() as u32,
    }
}

fn build(row: &serde_json::Value) -> (Thread, Bodies) {
    let mut th = Thread::new();
    let mut bodies = Vec::new();
    for (fi, f) in row["frames"].as_array().unwrap().iter().enumerate() {
        let nodes: Vec<Node> = f["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| Node {
                opword: n[0].as_u64().unwrap() as u16,
                operand: n[1].as_u64().unwrap() as u16,
                imm: n[2].as_u64().unwrap() as u32,
            })
            .collect();
        bodies.push(nodes);
        let entry = f["entry"].as_u64().unwrap() as usize;
        let null_pc = f.get("null_pc").is_some();
        let mut frame = Frame::new(
            (!null_pc).then_some(Pc {
                body: BodyId(fi as u32),
                idx: entry,
            }),
            0,
        );
        for c in f["locals"].as_array().unwrap() {
            assert_eq!(c["k"], "var");
            let id = th.vars.alloc(value_of(c), 0);
            frame.locals.push(id);
        }
        for c in f["stack"].as_array().unwrap() {
            frame.stack.push(match c["k"].as_str().unwrap() {
                "v" => Slot::Value(th.allocate_value(value_of(c))),
                "lv" => Slot::Var(frame.locals[c["i"].as_u64().unwrap() as usize]),
                "null" => Slot::Null,
                k => panic!("stack cell kind {k}"),
            });
        }
        th.frames.push(frame);
    }
    // Original supplied fixture cells were initialized with counter zero.
    th.set_live_counter(0);
    // run() refuses an empty thread and reloads pc from the frame.
    th.pc = Some(Pc {
        body: BodyId(0),
        idx: 0,
    });
    (th, Bodies(bodies))
}

fn cell_json(th: &Thread, frame: &Frame, s: &Slot) -> serde_json::Value {
    match s {
        Slot::Heap(_) => panic!("operator-only oracle unexpectedly produced a heap lvalue"),
        Slot::Null => serde_json::json!({"k": "null"}),
        Slot::Value(v) => {
            serde_json::json!({"k": "v", "t": tag_char(v.tag), "w": v.payload})
        }
        Slot::Var(id) => match frame.locals.iter().position(|l| l == id) {
            Some(i) => serde_json::json!({"k": "lv", "i": i}),
            None => {
                let v = th.vars.get(*id).value;
                serde_json::json!({"k": "var", "t": tag_char(v.tag), "w": v.payload})
            }
        },
    }
}

fn pc_json(pc: Option<Pc>) -> serde_json::Value {
    match pc {
        Some(p) => serde_json::json!([p.body.0, p.idx]),
        None => serde_json::Value::Null,
    }
}

#[test]
fn run_loop_matches_oracle_vectors() {
    let path = vectors_dir().join("thread_run.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SLR_ORACLE_SKIPPED: run_loop_matches_oracle_vectors");
            eprintln!("SKIP: no oracle vectors (run re/oracle/thread_vectors.py gen)");
            return;
        }
    };
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut total = 0usize;
    let mut divergent = 0usize;
    let mut compared_abnormal = std::collections::BTreeMap::<String, usize>::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let name = row["name"].as_str().unwrap().to_string();
        let want = &row["result"];
        let (mut th, mut host) = build(&row);
        let budget = row["budget"].as_f64().unwrap() as f32;
        let mut clock: Vec<f64> = row["clock"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        clock.reverse();
        let mut read_clock = move || X87::from_f64(clock.pop().unwrap_or(0.0));
        let outcome = th.run(&mut host, budget, &mut read_clock);
        let want_outcome = want["outcome"].as_str().unwrap();
        let got_outcome = match &outcome {
            Ok(_) => "stopped",
            Err(VmError::IntegerTrap { .. }) => "trap",
            Err(e) if e.is_script_fatal() => "fatal",
            Err(VmError::NullSlot | VmError::DanglingVar | VmError::NoFrame) => "crash",
            Err(e) => panic!("row {i} {name}: unexpected port error {e:?}"),
        };
        if row.get("known_divergence").is_some() {
            // Cases where the exe goes through the class loader (see the
            // `known_divergence` text in the row): the oracle crashes, the
            // port deliberately carries on. Pin both sides, do not compare.
            assert_eq!(want_outcome, "crash", "row {i} {name}: oracle outcome");
            assert_eq!(got_outcome, "stopped", "row {i} {name}: port outcome");
            divergent += 1;
            continue;
        }
        assert_eq!(got_outcome, want_outcome, "row {i} {name}: outcome");
        *counts.entry(got_outcome.to_string()).or_default() += 1;
        total += 1;
        let stopped = got_outcome == "stopped";
        let diffs = state_diffs(&th, want, stopped);
        if stopped {
            let hex: String = outcome
                .unwrap()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(
                Some(hex.as_str()),
                want["leftover"].as_str(),
                "row {i} {name}: leftover time"
            );
            assert!(diffs.is_empty(), "row {i} {name}: {diffs:?}");
        } else {
            *compared_abnormal
                .entry(got_outcome.to_string())
                .or_default() += 1;
            assert!(diffs.is_empty(), "row {i} {name}: {diffs:?}");
        }
    }
    assert!(total > 0, "no rows in {}", path.display());
    eprintln!(
        "thread_oracle: rows={total} outcomes={counts:?} abnormal_compared={compared_abnormal:?} known_divergence={divergent}"
    );
}

/// Differences between the port's thread and the recorded machine state.
/// Normally-stopped rows compare everything; for a fatal error, trap or
/// crash the exe state is the one at the moment the error path was entered
/// (the harness stops there), so the saved pcs, which `run`'s exit path
/// writes, and the RUNNING bit (the port clears it when it stops on an
/// error) are not comparable.
fn state_diffs(th: &Thread, want: &serde_json::Value, stopped: bool) -> Vec<String> {
    let mut d = Vec::new();
    let mut flags = u64::from(th.flags);
    let mut want_flags = want["flags"].as_u64().unwrap();
    if !stopped {
        flags &= !4;
        want_flags &= !4;
    }
    if flags != want_flags {
        d.push(format!("flags {flags:#x} vs {want_flags:#x}"));
    }
    if th.frames.len() as i64 - 1 != want["frame"].as_i64().unwrap() {
        d.push("current frame".into());
    }
    if pc_json(th.pc) != want["pc"] {
        d.push(format!("pc {:?} vs {}", pc_json(th.pc), want["pc"]));
    }
    if i64::from(th.live()) != want["live"].as_i64().unwrap() {
        d.push(format!("live {} vs {}", th.live(), want["live"]));
    }
    let want_frames = want["frames"].as_array().unwrap();
    if th.frames.len() != want_frames.len() {
        d.push("frame count".into());
        return d;
    }
    for (fi, (f, w)) in th.frames.iter().zip(want_frames).enumerate() {
        let locals: Vec<_> = f
            .locals
            .iter()
            .map(|id| {
                let v = th.vars.get(*id).value;
                serde_json::json!({"k": "var", "t": tag_char(v.tag), "w": v.payload})
            })
            .collect();
        if serde_json::Value::Array(locals) != w["locals"] {
            d.push(format!("frame {fi} locals"));
        }
        let stack: Vec<_> = f.stack.iter().map(|s| cell_json(th, f, s)).collect();
        if serde_json::Value::Array(stack) != w["stack"] {
            d.push(format!("frame {fi} stack"));
        }
        if stopped && f.saved_pc.map(|p| p.idx as u64) != w["saved_pc"].as_u64() {
            d.push(format!("frame {fi} saved pc"));
        }
    }
    d
}
