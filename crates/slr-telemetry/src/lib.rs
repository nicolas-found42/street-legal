//! Crash-surviving diagnostics shared by the headless boot driver and the
//! interactive frontend.
//!
//! Writers never buffer userspace history: calllogs, JSON event lines and the
//! file-I/O stream go straight to their files, and each stream is synchronized
//! before a frame checkpoint. The `capture-end.json` marker is written only
//! after every stream has been synchronized.
//!
//! The streams are:
//!
//! * `events.jsonl` — the versioned causal timeline from
//!   [`slr_vm::diagnostics`], optionally with a wall-clock field.
//! * `states.jsonl` — whatever checkpoint the embedding chooses to publish;
//!   it must carry `schema_version`, `sequence` and the run provenance.
//! * `runtime.calllog` / `runtime-all.calllog` — the filtered and complete
//!   native call logs (headless boot only).
//! * `file-io.jsonl` — the port's own file layer, one record per open, read,
//!   write, seek, close and directory search.
//! * `environment.json` — the host and build environment the run saw.
//! * `capture.json` / `capture-end.json` — liveness and completion markers.
//! * `failure.json`, `divergence.json`, `crash.json` — targeted artifacts.

pub mod journal;

use serde_json::{json, Value};
use slr_calllog::{LogRecord, LogWriter};
use slr_vm::diagnostics::{self, Event, EventKind, Stats};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Monotonic nanoseconds since the first call in this process.
///
/// This is wall-clock *elapsed* time, deliberately separate from the VM's
/// simulated clock. It is the only field that may differ between two
/// otherwise identical runs.
pub fn monotonic_ns() -> u64 {
    static ORIGIN: std::sync::LazyLock<std::time::Instant> =
        std::sync::LazyLock::new(std::time::Instant::now);
    ORIGIN.elapsed().as_nanos() as u64
}

/// UTC timestamp in the index's `started`/`finished` spelling.
pub fn utc_now() -> String {
    let seconds = unix_ns() / 1_000_000_000;
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (hours, minutes, seconds) = (rest / 3_600, (rest % 3_600) / 60, rest % 60);
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}+00:00")
}

/// Unix epoch nanoseconds, for correlating a run with host logs.
pub fn unix_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

struct State {
    run_id: String,
    output: PathBuf,
    events: File,
    states: File,
    file_io: Option<File>,
    filtered: LogWriter<File>,
    all: LogWriter<File>,
    all_ordinal: u64,
    filtered_ordinal: u64,
    full: bool,
    wall_clock: bool,
    skipped_steps: u64,
    skipped_stores: u64,
    compact: bool,
    error: Option<String>,
    events_written: u64,
    states_written: u64,
    file_io_written: u64,
    last_sequence: Option<u64>,
}

thread_local! {
    /// Per-thread capture. The streams are per-thread files, exactly as the
    /// original's log sink is; the embedding runs one capture per process.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}
/// Set once the runner has published the completion marker.
static FINISHED: AtomicBool = AtomicBool::new(false);

fn create(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("DiagnosticBoundary: {}: {error}", path.display()))
}

fn line(file: &mut File, value: &impl serde::Serialize) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    file.write_all(&bytes).map_err(|error| error.to_string())
}

/// The host, build and input environment of this process.
///
/// Only variable names that describe the diagnostics configuration are
/// retained; unrelated secrets never enter a bundle.
pub fn environment() -> Value {
    const PREFIXES: [&str; 6] = ["SLR_", "RUST_", "WGPU_", "MTL_", "CFG_", "CARGO_"];
    let mut retained: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        .collect();
    retained.sort();
    let executable = std::env::current_exe().ok();
    let binary = executable.as_ref().map(|path| {
        let metadata = std::fs::metadata(path);
        json!({
            "path": path.display().to_string(),
            "bytes": metadata.as_ref().map(|m| m.len()).ok(),
        })
    });
    json!({
        "schema_version": 1,
        "captured_unix_ns": unix_ns(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "pointer_width": usize::BITS,
        "available_parallelism": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "binary": binary,
        "variables": retained,
        "variables_total": std::env::vars().count(),
    })
}

/// Open every stream. `wall_clock` adds `wall_ns` to each event line;
/// `file_io` opens the port file-layer stream (boot does; the frontend may).
pub fn configure(
    output: &Path,
    run_id: &str,
    capacity: usize,
    full: bool,
    wall_clock: bool,
    file_io: bool,
) -> Result<(), String> {
    if FINISHED.load(Ordering::Relaxed) {
        return Err("DiagnosticBoundary: capture already finished".into());
    }
    let state = State {
        run_id: run_id.into(),
        output: output.to_owned(),
        events: create(&output.join("events.jsonl"))?,
        states: create(&output.join("states.jsonl"))?,
        file_io: if file_io {
            Some(create(&output.join("file-io.jsonl"))?)
        } else {
            None
        },
        filtered: LogWriter::create(create(&output.join("runtime.calllog"))?)
            .map_err(|e| e.to_string())?,
        all: LogWriter::create(create(&output.join("runtime-all.calllog"))?)
            .map_err(|e| e.to_string())?,
        all_ordinal: 0,
        filtered_ordinal: 0,
        full,
        wall_clock,
        skipped_steps: 0,
        skipped_stores: 0,
        compact: std::env::var_os("SLR_SESSION_CAPTURE").is_some(),
        error: None,
        events_written: 0,
        states_written: 0,
        file_io_written: 0,
        last_sequence: None,
    };
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    diagnostics::configure(run_id.into(), capacity, Some(event));
    slr_calllog::headless::configure_stream(Some(native), false);
    let mut marker = create(&output.join("capture.json"))?;
    line(
        &mut marker,
        &json!({"schema_version":1,"run_id":run_id,"status":"running",
        "trace_level":if full {"full"} else {"metadata"},"event_ring_capacity":capacity,
        "wall_clock":wall_clock,"file_io":file_io}),
    )?;
    marker.sync_all().map_err(|e| e.to_string())?;
    let mut environment_file = create(&output.join("environment.json"))?;
    let mut bytes = serde_json::to_vec_pretty(&environment()).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    environment_file
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    environment_file.sync_all().map_err(|e| e.to_string())?;
    install_fatal_handler(&output.join("capture.json"));
    detail("capture-start", vec![("run_id".into(), run_id.into())]);
    Ok(())
}

/// True while a capture is open on this thread.
pub fn enabled() -> bool {
    STATE.with(|slot| slot.borrow().is_some())
}

pub fn run_id() -> Option<String> {
    STATE.with(|slot| slot.borrow().as_ref().map(|state| state.run_id.clone()))
}

pub fn output() -> Option<PathBuf> {
    STATE.with(|slot| slot.borrow().as_ref().map(|state| state.output.clone()))
}

/// Emit one named event; answers its sequence so a checkpoint can join the
/// timeline by identity rather than by guesswork.
pub fn detail(category: &str, fields: Vec<(String, String)>) -> Option<u64> {
    diagnostics::emit(EventKind::External {
        category: category.into(),
        fields,
    })
}

/// Append one port file-layer operation. This is deliberately a separate
/// stream from the native call log: the original's file I/O is not reproduced
/// by the headless materialisation, so mixing the two would break comparison.
pub fn file_io(op: &str, path: &str, detail: Value) {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        let Some(file) = state.file_io.as_mut() else {
            return;
        };
        let record = json!({
            "schema_version": 1,
            "run_id": state.run_id,
            "wall_ns": monotonic_ns(),
            "op": op,
            "path": path,
            "detail": detail,
        });
        let result = line(file, &record);
        if let Err(error) = result {
            state.error.get_or_insert(error);
        } else {
            state.file_io_written += 1;
        }
    });
}

/// Append a checkpoint the embedding prepared, then synchronize every stream.
pub fn write_state(value: &Value) -> Result<(), String> {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(());
        };
        line(&mut state.states, value)?;
        state.states_written += 1;
        state.states.sync_data().map_err(|e| e.to_string())?;
        state.all.get_ref().sync_data().map_err(|e| e.to_string())?;
        state
            .filtered
            .get_ref()
            .sync_data()
            .map_err(|e| e.to_string())?;
        if let Some(file) = state.file_io.as_mut() {
            file.sync_data().map_err(|e| e.to_string())?;
        }
        state.events.sync_data().map_err(|e| e.to_string())
    })
}

/// Write a one-shot artifact (`failure.json`, `divergence.json`, `crash.json`).
pub fn write_artifact(name: &str, value: &Value) -> Result<(), String> {
    STATE.with(|slot| {
        let slot = slot.borrow();
        let state = slot.as_ref().ok_or("diagnostics not configured")?;
        let mut file = create(&state.output.join(name))?;
        line(&mut file, value)?;
        file.sync_all().map_err(|e| e.to_string())
    })
}

fn event(event: &Event) {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if !state.full && matches!(event.kind, EventKind::Step { .. }) {
            state.skipped_steps += 1;
            return;
        }
        if state.compact && !state.full && matches!(event.kind, EventKind::Store { .. }) {
            state.skipped_stores += 1;
            return;
        }
        let mut value = match serde_json::to_value(event) {
            Ok(value) => value,
            Err(error) => {
                state.error.get_or_insert(error.to_string());
                return;
            }
        };
        if state.wall_clock {
            if let Some(object) = value.as_object_mut() {
                object.insert("wall_ns".into(), json!(monotonic_ns()));
            }
        }
        let result = line(&mut state.events, &value);
        if let Err(error) = result {
            state.error.get_or_insert(error);
        } else {
            state.events_written += 1;
            state.last_sequence = Some(event.sequence);
        }
    });
}

fn filtered(record: &LogRecord) -> bool {
    !matches!(record,LogRecord::NativeCall{class,..} if class.starts_with("java.render.")
        || class.starts_with("java.sound.") || ["java.io.Input","java.io.MouseCursor","java.io.Controller"].contains(&class.as_str()))
}

/// Append one native call to both call logs; answers the zero-based
/// (all, filtered) ordinals for the correlated event line.
pub fn record_native(record: &LogRecord) -> Option<(u64, Option<u64>)> {
    STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut()?;
        let all_ordinal = state.all_ordinal;
        let selected = filtered(record);
        let filtered_ordinal = selected.then_some(state.filtered_ordinal);
        let result = state
            .all
            .record(record)
            .and_then(|()| state.all.flush())
            .and_then(|()| {
                if selected {
                    state
                        .filtered
                        .record(record)
                        .and_then(|()| state.filtered.flush())
                } else {
                    Ok(())
                }
            });
        if let Err(error) = result {
            state.error.get_or_insert(error.to_string());
        }
        state.all_ordinal += 1;
        if selected {
            state.filtered_ordinal += 1;
        }
        Some((all_ordinal, filtered_ordinal))
    })
}

fn native(record: &LogRecord) {
    let Some((all, filtered)) = record_native(record) else {
        return;
    };
    let native_record = match record {
        LogRecord::NativeCall {
            class,
            name,
            args,
            result,
        } => json!({"class":class,"name":name,
            "arguments":args.iter().map(recorded_value).collect::<Vec<_>>(),
            "result":result.as_ref().map(recorded_value)}),
        _ => json!({"record_kind":record.kind_name()}),
    };
    detail(
        "native-call",
        vec![
            ("all_ordinal".into(), all.to_string()),
            (
                "filtered_ordinal".into(),
                filtered.map_or_else(String::new, |v| v.to_string()),
            ),
            ("native_record".into(), native_record.to_string()),
        ],
    );
}

fn recorded_value(value: &slr_calllog::Value) -> Value {
    use slr_calllog::Value as V;
    match value {
        V::Int(value) => json!({"type":"int","value":value}),
        V::Long(value) => json!({"type":"long","value":value}),
        V::FloatBits(bits) => json!({"type":"float","bits":bits}),
        V::DoubleBits(bits) => json!({"type":"double","bits":bits}),
        V::Str(value) => json!({"type":"string","value":value}),
        V::Bytes(value) => json!({"type":"bytes","value":value}),
        V::Bool(value) => json!({"type":"bool","value":value}),
    }
}

/// Provenance an embedding stamps into every checkpoint.
pub fn provenance() -> Value {
    STATE.with(|slot| {
        slot.borrow().as_ref().map_or(Value::Null, |state| {
            json!({"run_id":state.run_id,"all_records":state.all_ordinal,
                "filtered_records":state.filtered_ordinal})
        })
    })
}

pub fn stats() -> Stats {
    diagnostics::stats()
}

pub fn check() -> Result<(), String> {
    STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|s| s.error.clone())
            .map_or(Ok(()), |e| Err(format!("DiagnosticBoundary: {e}")))
    })
}

/// Synchronize every stream, then write the durable completion marker.
pub fn finish(failure: Option<&str>) -> Result<(), String> {
    check()?;
    detail(
        "capture-end",
        vec![(
            "status".into(),
            if failure.is_some() {
                "failed"
            } else {
                "completed"
            }
            .into(),
        )],
    );
    let result = STATE.with(|slot| {
        let mut slot=slot.borrow_mut();
        let Some(state)=slot.as_mut() else{return Ok(())};
        if let Some(error)=&state.error {return Err(format!("DiagnosticBoundary: {error}"));}
        state.all.flush().map_err(|e|e.to_string())?;
        state.filtered.flush().map_err(|e|e.to_string())?;
        state.all.get_ref().sync_all().map_err(|e|e.to_string())?;
        state.filtered.get_ref().sync_all().map_err(|e|e.to_string())?;
        state.events.sync_all().map_err(|e|e.to_string())?;
        state.states.sync_all().map_err(|e|e.to_string())?;
        if let Some(file)=state.file_io.as_mut(){file.sync_all().map_err(|e|e.to_string())?;}
        let mut marker=create(&state.output.join("capture-end.json"))?;
        line(&mut marker,&json!({"schema_version":1,"run_id":state.run_id,"status":if failure.is_some(){"failed"}else{"completed"},
            "error":failure,"all_records":state.all_ordinal,"filtered_records":state.filtered_ordinal,
            "events_written":state.events_written,"states_written":state.states_written,
            "file_io_written":state.file_io_written,"last_sequence":state.last_sequence,
            "diagnostics":diagnostics::stats(),
            "trace_level":if state.full{"full"}else{"metadata"},"suppressed_steps":state.skipped_steps,
            "suppressed_stores":state.skipped_stores,"compact":state.compact,
            "pending_records":0,"truncated":false}))?;
        marker.sync_all().map_err(|e|e.to_string())
    });
    if result.is_ok() {
        FINISHED.store(true, Ordering::Relaxed);
    }
    result
}

/// Write `failure.json`-style state from a panic hook, then the end marker.
///
/// `snapshot` runs in a panicking thread; it must not panic itself. The
/// embedding supplies whatever state it can still read safely.
pub fn crash(snapshot: impl FnOnce() -> Value) -> Result<(), String> {
    if FINISHED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let value = snapshot();
    write_artifact("crash.json", &value)?;
    finish(Some(
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("panic"),
    ))
}

/// Write the bundle's `manifest.json`, the file every archive tool reads.
///
/// The headless runner writes a richer manifest itself (source identity, input
/// snapshots, comparison policy); this covers embeddings that do not have one,
/// so `list`, `query`, `status` and `prune` treat every bundle the same way.
pub fn write_manifest(extra: Value) -> Result<(), String> {
    let (output, run_id) = STATE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|state| (state.output.clone(), state.run_id.clone()))
            .ok_or("diagnostics not configured")
    })?;
    let artifacts: Vec<Value> = std::fs::read_dir(&output)
        .map_err(|error| format!("DiagnosticBoundary: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter(|entry| entry.file_name() != "manifest.json")
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = std::fs::metadata(&path).ok()?;
            let bytes = std::fs::read(&path).ok()?;
            use sha2::{Digest, Sha256};
            Some(json!({
                "path": entry.file_name().to_string_lossy(),
                "bytes": metadata.len(),
                "sha256": format!("{:x}", Sha256::digest(&bytes)),
            }))
        })
        .collect();
    let mut manifest = extra;
    let Some(object) = manifest.as_object_mut() else {
        return Err("DiagnosticBoundary: manifest must be an object".into());
    };
    object.insert("schema_version".into(), json!(1));
    object.insert("run_id".into(), json!(run_id));
    object.insert("artifacts".into(), json!(artifacts));
    object.insert("environment".into(), environment());
    // Rewritten as the run progresses (running, then completed), so publish it
    // atomically: a reader never sees a truncated manifest.
    let path = output.join("manifest.json");
    let temporary = output.join("manifest.json.tmp");
    let _ = std::fs::remove_file(&temporary);
    let mut bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|error| format!("DiagnosticBoundary: {error}"))?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

/// What a panicking process can still report: the last retained event and the
/// stream counters. Safe while unwinding: no script or app state is touched.
pub fn crash_context() -> Value {
    json!({
        "schema_version": 1,
        "run_id": run_id(),
        "wall_ns": monotonic_ns(),
        "unix_ns": unix_ns(),
        "last_event": diagnostics::recent().last().cloned(),
        "diagnostics": diagnostics::stats(),
    })
}

/// Install a panic hook that publishes a crash artifact before the process
/// aborts. The previous hook still runs, so backtraces are unchanged.
pub fn install_panic_hook(snapshot: fn() -> Value) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if enabled() {
            let mut value = snapshot();
            if let Some(object) = value.as_object_mut() {
                object.insert("message".into(), json!(info.to_string()));
                let thread = std::thread::current();
                object.insert(
                    "thread".into(),
                    json!({"name": thread.name(), "id": format!("{:?}", thread.id())}),
                );
                object.insert(
                    "location".into(),
                    json!(info.location().map(|location| format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    ))),
                );
                object.insert("wall_ns".into(), json!(monotonic_ns()));
            }
            let _ = crash(|| value);
        }
        previous(info);
    }));
}

#[cfg(unix)]
mod fatal {
    use std::{
        os::raw::{c_int, c_void},
        sync::atomic::{AtomicI32, Ordering},
    };

    /// The signal that arrived, so the marker names it.
    static SIGNAL: AtomicI32 = AtomicI32::new(0);

    /// Path of the bundle's `capture.json`; the handler derives its siblings.
    static CAPTURE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

    /// Async-signal-safe subset: `open`, `write`, `ftruncate`, `fsync`, `_exit`.
    ///
    /// Only `write` is guaranteed by POSIX to be async-signal-safe among the
    /// C library calls a handler may make; `open`/`ftruncate`/`fsync` are safe
    /// in practice on the supported libcs and are confined to this handler,
    /// which runs at most once. Formatting, allocation and locks are avoided.
    extern "C" fn handle(signal: c_int) {
        SIGNAL.store(signal, Ordering::SeqCst);
        let path = CAPTURE
            .try_lock()
            .ok()
            .and_then(|guard| guard.clone())
            .map(|capture| capture.with_file_name("crash.json"));
        if let Some(path) = path {
            if let Some(bytes) = path.to_str() {
                let mut text = [0u8; 192];
                let prefix = b"{\"schema_version\":1,\"reason\":\"signal\",\"signal\":";
                let mut length = 0;
                for byte in prefix {
                    text[length] = *byte;
                    length += 1;
                }
                length += copy_unsigned(&mut text[length..], signal as u64);
                for byte in b"}\n" {
                    text[length] = *byte;
                    length += 1;
                }
                unsafe {
                    let fd = libc::open(
                        c_path(bytes).as_ptr(),
                        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                        0o600,
                    );
                    if fd >= 0 {
                        let _ = libc::write(fd, text.as_ptr().cast::<c_void>(), length);
                        let _ = libc::fsync(fd);
                        let _ = libc::close(fd);
                    }
                    // Restore the default action and re-raise so the exit
                    // status and core-dump behaviour match an unhandled signal.
                    libc::signal(signal, libc::SIG_DFL);
                    libc::raise(signal);
                    libc::_exit(128 + signal);
                }
            }
        }
        unsafe { libc::_exit(128 + signal) };
    }

    /// A NUL-terminated copy of `text`, or an empty string if it is too long.
    fn c_path(text: &str) -> [i8; 4096] {
        let mut out = [0i8; 4096];
        if text.len() < out.len() - 1 {
            for (index, byte) in text.bytes().enumerate() {
                out[index] = byte as i8;
            }
        }
        out
    }

    fn copy_unsigned(out: &mut [u8], mut value: u64) -> usize {
        let mut digits = [0u8; 20];
        let mut count = 0;
        loop {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        for index in 0..count {
            out[index] = digits[count - 1 - index];
        }
        count
    }

    /// Arm the handler; `capture` is the open capture marker for this run.
    pub fn install(capture: &std::path::Path) {
        *CAPTURE.lock().unwrap_or_else(|error| error.into_inner()) = Some(capture.to_owned());
        unsafe {
            for signal in [
                libc::SIGSEGV,
                libc::SIGABRT,
                libc::SIGBUS,
                libc::SIGILL,
                libc::SIGFPE,
            ] {
                libc::signal(signal, handle as *const () as libc::sighandler_t);
            }
        }
    }
}

/// Copy `source` to `destination`, sharing blocks where the filesystem can.
///
/// Diagnostics bundles repeat large identical inputs (the stock projection,
/// oracle logs, the binary). APFS, btrfs and XFS can clone them in constant
/// time and space while leaving both paths independently writable; anywhere
/// else this is a plain copy, so callers need no special case.
///
/// Answers `true` when the bytes were shared rather than copied.
pub fn clone_or_copy(source: &Path, destination: &Path) -> std::io::Result<bool> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(source.as_os_str().as_bytes())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let destination = std::ffi::CString::new(destination.as_os_str().as_bytes())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        // SAFETY: both pointers are valid NUL-terminated strings for the call.
        if unsafe { libc::clonefile(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
            return Ok(true);
        }
        // EXDEV, ENOTSUP and friends: fall through to a real copy.
    }
    #[cfg(target_os = "linux")]
    {
        match std::fs::copy(source, destination) {
            Ok(_) => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    std::fs::copy(source, destination)?;
    Ok(false)
}

/// Install the fatal-signal handler for an open capture. `capture` is the
/// bundle's `capture.json`; `crash.json` is written beside it.
#[cfg(unix)]
pub fn install_fatal_handler(capture: &Path) {
    fatal::install(capture);
}

/// Windows has no portable async-signal-safe file API here; panics are still
/// captured by [`install_panic_hook`].
#[cfg(not(unix))]
pub fn install_fatal_handler(_capture: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "slr-telemetry-{}-{}-{name}",
            std::process::id(),
            monotonic_ns()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn structured_native_values_preserve_float_bits_and_byte_strings() {
        let nan = recorded_value(&slr_calllog::Value::FloatBits(0xffc01234));
        assert_eq!(nan["bits"], 0xffc01234u32);
        let negative_zero = recorded_value(&slr_calllog::Value::DoubleBits(0x8000000000000000));
        assert_eq!(negative_zero["bits"], 0x8000000000000000u64);
        assert_eq!(
            recorded_value(&slr_calllog::Value::Bytes(vec![0, 255]))["value"],
            serde_json::json!([0, 255])
        );
    }

    #[test]
    fn environment_retains_only_diagnostic_variables() {
        std::env::set_var("SLR_TELEMETRY_TEST", "1");
        std::env::set_var("UNRELATED_SECRET_FOR_TEST", "do-not-record");
        let value = environment();
        let names: Vec<&str> = value["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[0].as_str().unwrap())
            .collect();
        assert!(names.contains(&"SLR_TELEMETRY_TEST"));
        assert!(!names.contains(&"UNRELATED_SECRET_FOR_TEST"));
        assert_eq!(value["arch"], std::env::consts::ARCH);
        std::env::remove_var("SLR_TELEMETRY_TEST");
        std::env::remove_var("UNRELATED_SECRET_FOR_TEST");
    }

    #[test]
    fn wall_clock_and_file_io_are_separate_streams() {
        let output = scratch("streams");
        configure(&output, "run", 16, false, true, true).unwrap();
        detail("frame-enter", vec![("frame".into(), "1".into())]);
        file_io("open", "saves/slot1.sdat", json!({"mode":"write"}));
        write_state(&json!({"schema_version":1,"sequence":1,"provenance":provenance()})).unwrap();
        finish(None).unwrap();
        let events = std::fs::read_to_string(output.join("events.jsonl")).unwrap();
        assert!(events.lines().all(|line| line.contains("\"wall_ns\"")));
        let io = std::fs::read_to_string(output.join("file-io.jsonl")).unwrap();
        assert!(io.contains("saves/slot1.sdat"));
        assert!(!events.contains("saves/slot1.sdat"));
        let end: Value = serde_json::from_str(
            &std::fs::read_to_string(output.join("capture-end.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(end["file_io_written"], 1);
        assert_eq!(end["status"], "completed");
    }

    #[test]
    fn timestamps_are_monotonic_and_epoch_based() {
        let first = monotonic_ns();
        let epoch = unix_ns();
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(monotonic_ns() > first);
        assert!(epoch > 1_600_000_000_000_000_000);
    }
}
