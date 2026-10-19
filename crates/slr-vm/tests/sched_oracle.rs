//! Differential replay of the scheduler oracle vectors (M1-05 T6, issue #32).
//!
//! Replays `re/out/sched/sched_vectors.jsonl`, produced by
//! `re/oracle/sched_vectors.py gen`: the REAL exe's cooperative scheduler
//! driver `FUN_00416940` and the per-frame tick `FUN_00418d10` (with its
//! `FUN_00418c20` finalizer drain) executed under Unicorn on a synthetic
//! thread list, a synthetic driver object and hooked `FUN_005516c0` (the
//! clock), `FUN_00420ff0` (`Thread::run`), `0x423b60` (the deleting
//! destructor) and `FUN_00408560` (the finalizer). No JVM boot, no game
//! data. Override the directory with `SLR_SCHED_VECTORS`. Skips with a note
//! when the file is absent so `cargo test --workspace` stays green without
//! the exe/venv.
//!
//! Per row the test rebuilds the same threads, the same driver counters and
//! the same three finalizer queues in the port, feeds the recorded clock
//! readings one by one (`FUN_005516c0` is the only clock the driver has, so
//! one trace has to cover the whole call) and compares bit for bit:
//!
//! - the ordered `Thread::run` trace, each slice as raw f32 bits;
//! - every thread's final flags and sleep deadline (`+0x2c`/`+0x30`);
//! - the scheduler cursor `[driver+0x78]`, the reaped list (the order the
//!   exe's deleting destructor freed threads) and the driver's return;
//! - the number of clock reads;
//! - for tick rows, the tick counter `[+0x40]`, the drain quota `[+0x3c]`,
//!   the smoothed frame time `[+0x24]`, the callback accumulator `[+0x34]`,
//!   the finalizer pops in order and the three queues' lengths afterwards.
//!
//! Rows whose driver never reaches the callback block (`[+0x34] > [+0x38]`
//! is false in every committed row) must not call the callback seam at all.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;

use slr_vm::sched::{CallbackRun, FinalizerQueues, SchedExit, Scheduler};
use slr_vm::threads::{ThreadId, ThreadQueues};
use slr_x87::X87;

/// Rows in the committed harness (`re/oracle/sched_vectors.py`, T5 932f238).
const ROWS: usize = 27;
/// `Thread::run` calls across those rows (`gen` prints "27 rows, 25 thread
/// runs").
const RUNS: usize = 25;
/// `FUN_00416940` (`kind: drive`).
const DRIVE_ADDR: u32 = 0x0041_6940;
/// `FUN_00418d10` (`kind: tick`).
const TICK_ADDR: u32 = 0x0041_8d10;
/// The D3D9 control word every row was run under (`docs/physics.md`, ADR
/// 0007): 24-bit precision, what the port's `SCRIPT_PRECISION` models.
const CW: &str = "0x007f";
/// The record ids the replay pushes onto the `spare` (`[jvm+0x30]`) queue.
/// Only that queue's length is observed, so these ids only have to stay out
/// of the `queued` queue's own id range (the one whose pop order is compared).
const SPARE_BASE: u32 = 0x0001_0000;
/// See [`SPARE_BASE`]; the `held` (`[jvm+0x2c]`) queue.
const HELD_BASE: u32 = 0x0002_0000;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_SCHED_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/sched"))
}

/// A `0x...` word from the vector.
fn unb(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or_else(|e| panic!("hex {s:?}: {e}"))
}

/// A `0x...` word holding f32 bits.
fn f32_of(s: &str) -> f32 {
    f32::from_bits(unb(s))
}

/// One assertion with the row's name for context.
fn check<T: PartialEq + std::fmt::Debug>(name: &str, what: &str, got: T, want: T) {
    assert_eq!(got, want, "{name}: {what} mismatch");
}

/// `FUN_00416940`'s `eax` (the vector's `return`).
fn exit_of(v: u64) -> SchedExit {
    match v {
        0 => SchedExit::ListEnd,
        1 => SchedExit::TimeUp,
        other => panic!("unexpected scheduler return {other}"),
    }
}

/// The clock readings one call consumed, in order (`FUN_005516c0`).
struct Trace {
    values: VecDeque<f32>,
    reads: usize,
}

impl Trace {
    /// The row's own recorded readings.
    fn new(row: &serde_json::Value) -> Self {
        let values = row["clock_trace"]
            .as_array()
            .expect("clock_trace")
            .iter()
            .map(|v| f32_of(v.as_str().expect("clock_trace entry")))
            .collect();
        Self { values, reads: 0 }
    }

    /// Readings for a walk the vectors do not record (the cursor set-up).
    fn from_values(values: Vec<f32>) -> Self {
        Self {
            values: values.into(),
            reads: 0,
        }
    }

    /// One `FUN_005516c0` read. The exe's clock advanced exactly as the
    /// vector recorded, so reading past the trace is a divergence.
    fn read(&mut self) -> X87 {
        let v = self
            .values
            .pop_front()
            .unwrap_or_else(|| panic!("clock read past the recorded trace"));
        self.reads += 1;
        X87::from_f32(v)
    }
}

/// The row's threads in run-queue order. The port's `spawn` order is the
/// vector's list order, so a `ThreadId` is the vector's thread index.
fn build_queues(row: &serde_json::Value) -> (ThreadQueues, Vec<u32>) {
    let specs = row["threads"].as_array().expect("threads");
    let mut queues = ThreadQueues::new();
    let mut masks = Vec::with_capacity(specs.len());
    for (i, spec) in specs.iter().enumerate() {
        let id = queues.spawn(
            spec["priority"].as_i64().expect("priority") as i32,
            unb(spec["flags"].as_str().expect("flags")),
            "oracle",
        );
        assert_eq!(id, i, "spawn order is the vector's thread order");
        queues.get_mut(id).sleep_deadline =
            f32_of(spec["sleep_deadline"].as_str().expect("sleep_deadline"));
        masks.push(unb(spec["run_mask"].as_str().expect("run_mask")));
    }
    (queues, masks)
}

/// Replay one row and compare every recorded field.
fn replay(row: &serde_json::Value) {
    let name = row["name"].as_str().expect("name");
    let expect = &row["expect"];
    assert_eq!(row["cw"].as_str().expect("cw"), CW, "{name}: control word");
    let is_tick = match row["kind"].as_str().expect("kind") {
        "tick" => {
            assert_eq!(
                unb(row["exe_addr"].as_str().expect("exe_addr")),
                TICK_ADDR,
                "{name}: tick address"
            );
            true
        }
        "drive" => {
            assert_eq!(
                unb(row["exe_addr"].as_str().expect("exe_addr")),
                DRIVE_ADDR,
                "{name}: drive address"
            );
            false
        }
        other => panic!("{name}: unknown row kind {other}"),
    };
    let (mut queues, masks) = build_queues(row);
    let mut sched = Scheduler::new();
    let runs = RefCell::new(Vec::<(usize, u32)>::new());
    let mut run =
        |queues: &mut ThreadQueues, id: ThreadId, slice: f32, _clock: &mut dyn FnMut() -> X87| {
            runs.borrow_mut().push((id, slice.to_bits()));
            queues.get_mut(id).flags |= masks[id];
        };
    let budget = f32_of(row["budget"].as_str().expect("budget"));
    let min_priority = row["min_priority"].as_i64().expect("min_priority") as i32;

    // `[driver+0x78]` has no public setter, so a row that starts mid-list is
    // brought to that cursor through the driver's own moves: a timed walk
    // whose priority floor admits no thread advances one node per pass and
    // runs out of time as soon as the cursor stands on the recorded thread.
    if let Some(k) = row["cursor_in"].as_u64() {
        let k = k as usize;
        if k > 0 {
            let mut values = vec![0.0f32; k + 1];
            values.push(10.0);
            let mut prep = Trace::from_values(values);
            let prep_exit = {
                let mut clock = || prep.read();
                sched.drive(&mut queues, 5.0, i32::MAX, &mut clock, &mut run)
            };
            assert_eq!(prep_exit, SchedExit::TimeUp, "{name}: cursor set-up");
            assert_eq!(sched.cursor(), Some(k), "{name}: cursor set-up");
            assert!(
                runs.borrow().is_empty(),
                "{name}: cursor set-up ran a thread"
            );
            assert!(
                sched.reaped().is_empty(),
                "{name}: cursor set-up reaped a thread"
            );
        }
    }

    // The tick's own driver state and its three finalizer queues.
    let mut finals = FinalizerQueues::new();
    let pops = RefCell::new(Vec::<u32>::new());
    let callback_calls = Cell::new(0u32);
    if is_tick {
        let f = &row["finalizers"];
        sched.ticks = f["ticks"].as_u64().expect("ticks") as u32;
        sched.divisor = f["divisor"].as_i64().expect("divisor") as i32;
        sched.callback_ticks = f["callback_ticks"].as_i64().expect("callback_ticks") as i32;
        sched.callback_period = f["callback_period"].as_i64().expect("callback_period") as i32;
        for i in 0..f["queued"].as_u64().expect("queued") {
            finals.push_queued(i as u32);
        }
        for i in 0..f["spare"].as_u64().expect("spare") {
            finals.push_spare(SPARE_BASE + i as u32);
        }
        for i in 0..f["held"].as_u64().expect("held") {
            finals.push_held(HELD_BASE + i as u32);
        }
    }

    let mut trace = Trace::new(row);
    let exit = {
        let mut clock = || trace.read();
        if is_tick {
            let mut finalize = |id: u32| pops.borrow_mut().push(id);
            let mut callbacks = || {
                callback_calls.set(callback_calls.get() + 1);
                CallbackRun::Retained
            };
            sched.tick(
                &mut queues,
                &mut finals,
                budget,
                &mut clock,
                &mut finalize,
                &mut callbacks,
                &mut run,
            )
        } else {
            sched.drive(&mut queues, budget, min_priority, &mut clock, &mut run)
        }
    };

    // ---------------------------------------------------------- compare
    check(
        name,
        "return",
        exit,
        exit_of(expect["return"].as_u64().expect("return")),
    );
    let want_runs: Vec<(usize, u32)> = expect["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .map(|r| {
            (
                r[0].as_u64().expect("run thread") as usize,
                unb(r[1].as_str().expect("run slice")),
            )
        })
        .collect();
    check(
        name,
        "run trace (order + slice bits)",
        runs.borrow().clone(),
        want_runs,
    );
    let flags: Vec<u32> = (0..masks.len()).map(|i| queues.get(i).flags).collect();
    let want_flags: Vec<u32> = expect["flags_after"]
        .as_array()
        .expect("flags_after")
        .iter()
        .map(|v| unb(v.as_str().expect("flags_after entry")))
        .collect();
    check(name, "thread flags", flags, want_flags);
    let sleep: Vec<u32> = (0..masks.len())
        .map(|i| queues.get(i).sleep_deadline.to_bits())
        .collect();
    let want_sleep: Vec<u32> = expect["sleep_after"]
        .as_array()
        .expect("sleep_after")
        .iter()
        .map(|v| unb(v.as_str().expect("sleep_after entry")))
        .collect();
    check(name, "sleep deadlines", sleep, want_sleep);
    check(
        name,
        "cursor",
        sched.cursor(),
        expect["cursor_after"].as_u64().map(|v| v as usize),
    );
    let want_reaped: Vec<usize> = expect["reaped"]
        .as_array()
        .expect("reaped")
        .iter()
        .map(|v| v.as_u64().expect("reaped entry") as usize)
        .collect();
    check(name, "reaped order", sched.reaped(), want_reaped.as_slice());
    check(
        name,
        "clock reads",
        trace.reads,
        expect["clock_reads"].as_u64().expect("clock_reads") as usize,
    );
    assert!(
        trace.values.is_empty(),
        "{name}: the call read fewer clock values than the vector recorded"
    );

    if is_tick {
        check(
            name,
            "tick counter",
            sched.ticks,
            expect["ticks"].as_u64().expect("ticks") as u32,
        );
        check(
            name,
            "drain quota",
            sched.divisor,
            expect["divisor"].as_i64().expect("divisor") as i32,
        );
        check(
            name,
            "elapsed ms",
            sched.elapsed_ms,
            expect["elapsed_ms"].as_i64().expect("elapsed_ms") as i32,
        );
        check(
            name,
            "callback accumulator",
            sched.callback_ticks,
            expect["callback_ticks_after"]
                .as_i64()
                .expect("callback_ticks_after") as i32,
        );
        let want_pops: Vec<u32> = expect["finalizer_pops"]
            .as_array()
            .expect("finalizer_pops")
            .iter()
            .map(|v| v.as_u64().expect("finalizer pop") as u32)
            .collect();
        check(name, "finalizer pops", pops.borrow().clone(), want_pops);
        let after = &expect["finalizer_after"];
        check(
            name,
            "queued queue length",
            finals.queued().len(),
            after["queued"].as_u64().expect("queued") as usize,
        );
        check(
            name,
            "spare queue length",
            finals.spare().len(),
            after["spare"].as_u64().expect("spare") as usize,
        );
        check(
            name,
            "held queue length",
            finals.held().len(),
            after["held"].as_u64().expect("held") as usize,
        );
        check(name, "callback block calls", callback_calls.get(), 0);
    }
}

#[test]
fn scheduler_matches_oracle_vectors() {
    let path = vectors_dir().join("sched_vectors.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("SKIP: no oracle vectors (run re/oracle/sched_vectors.py gen)");
            return;
        }
    };
    let rows: Vec<serde_json::Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
        .collect();

    let mut thread_runs = 0;
    for row in &rows {
        thread_runs += row["expect"]["runs"].as_array().map_or(0, Vec::len);
        replay(row);
    }

    // The committed harness produces exactly these; a mismatch means the
    // generator changed and this evidence line must be refreshed.
    assert_eq!(rows.len(), ROWS, "row count changed in {}", path.display());
    assert_eq!(
        thread_runs,
        RUNS,
        "thread-run count changed in {}",
        path.display()
    );
    eprintln!(
        "sched_oracle: {} rows, {} thread runs replayed bit for bit from {}",
        rows.len(),
        thread_runs,
        path.display()
    );
}
