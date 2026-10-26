//! Script threads and the per-frame scheduler pump for the boot driver.
//!
//! The exe keeps one flag word and one frame list per thread. The port splits
//! them: `slr_lang::thread` owns the queued [`ThreadState`] (priority, flags,
//! sleep deadline, monitor wait) and `slr_vm::interp::Thread` owns the frames.
//! This module is the glue.
//!
//! * The queue arena lives in the thread natives' `ThreadHost` between ticks.
//!   While the scheduler walks a pass it holds the arena itself, so the
//!   runner swaps the arena back into the host for the duration of each
//!   script run and out again afterwards (natives such as `Thread.init`,
//!   `Thread.sleep` and `Object.wait` mutate it mid-run).
//! * The interpreter threads wait in [`INTERP`] keyed by queue id. A thread is
//!   absent while it runs.
//! * [`hook`] mirrors the flag word between the running interpreter thread and
//!   its queue entry around every native call, so a native's sleep/wait/stop
//!   is visible to the loop-tail check exactly as one shared word would be.
//! * `Thread.start` (`LAB_0047c570`) calls `target.run()` by name on the new
//!   thread. A body that enters a frame leaves a pending continuation which a
//!   later scheduler pass executes; a null body returns immediately and the
//!   thread dies (original probe `re/oracle/thread_start_continuation_probe.py`).
//!
//! Scenario inputs, declared rather than observed: the simulated clock (a
//! monotone millisecond counter advanced by a fixed step on every read) and
//! the creation threads' queue identity (`FUN_0053a5e0`: `THRD-CREATE`,
//! priority 10, flags 0). The original frame loop's other per-frame work
//! (input poll, object-list step, animation callbacks) is not run here.
use slr_boot::clock_replay::{Replay, RequestContext, ThreadContext};
use slr_lang::thread::with_threads;
use slr_vm::{
    host::{CallInfo, VmHost},
    interp::{Flow, Thread},
    sched::{CallbackRun, FinalizerQueues, SchedExit, Scheduler},
    threads::{ThreadId, ThreadQueues},
    values::Value,
    vmclass::{ClassId, ClassRef},
};
use slr_x87::X87;
use std::path::Path;
use std::{cell::Cell, cell::RefCell, collections::HashMap};

thread_local! {
    static INTERP: RefCell<HashMap<ThreadId, Thread>> = RefCell::new(HashMap::new());
    static LAST_DRIVER_CURSOR: Cell<Option<ThreadId>> = const {Cell::new(None)};
    static CURRENT: Cell<Option<ThreadId>> = const { Cell::new(None) };
    static CLOCK: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
    static REPLAY: RefCell<Option<Replay>> = const {RefCell::new(None)};
    static FRAME_TIMER: RefCell<Option<(slr_boot::frame_timer::FrameTimerReplay,usize)>> = const {RefCell::new(None)};
    static REPLAYING: Cell<bool> = const {Cell::new(false)};
    static REPLAY_ERROR: RefCell<Option<String>> = const {RefCell::new(None)};
    static QUEUE_CONTEXTS: RefCell<HashMap<ThreadId,slr_vm::clock::ThreadContext>> = RefCell::new(HashMap::new());
}

/// Configure strict replay from the earliest captured external clock request.
pub fn configure_replay(path: &Path) -> Result<(), String> {
    let mut replay = Replay::load(path).map_err(|e| e.to_string())?;
    replay.select_start().map_err(|e| e.to_string())?;
    let timer = slr_boot::frame_timer::FrameTimerReplay::load_companion(path)?;
    FRAME_TIMER.with(|v| *v.borrow_mut() = Some(timer));
    REPLAY.with(|v| *v.borrow_mut() = Some(replay));
    REPLAYING.with(|v| v.set(true));
    REPLAY_ERROR.with(|v| *v.borrow_mut() = None);
    Ok(())
}
pub fn begin_replay_after_game_init() -> Result<(), String> {
    REPLAY.with(|value| {
        if let Some(replay) = value.borrow_mut().as_mut() {
            replay.check_after_game_init().map_err(|e| e.to_string())?;
            REPLAYING.with(|v| v.set(true));
        }
        Ok(())
    })
}
pub fn replaying() -> bool {
    REPLAYING.with(Cell::get)
}
pub fn replay_error() -> Option<String> {
    REPLAY_ERROR.with(|v| v.borrow().clone())
}
pub fn finish_accepted() -> Result<(), String> {
    if let Some(error) = replay_error() {
        return Err(error);
    }
    FRAME_TIMER.with(|v| {
        if let Some((timer, accepted)) = v.borrow().as_ref() {
            if timer.consumed() != *accepted {
                return Err(format!(
                    "accepted frame timer boundary incomplete: consumed {} expected {accepted}",
                    timer.consumed()
                ));
            }
        }
        Ok(())
    })?;
    REPLAY.with(|v| {
        v.borrow()
            .as_ref()
            .map_or(Ok(()), |r| r.finish_accepted().map_err(|e| e.to_string()))
    })
}
/// Original Timer7 elapsed seconds at frame end; frames are one-based.
/// None selects the separately declared legacy fixed-frame scenario.
pub fn replayed_frame_dt(frame: u64) -> Result<Option<f32>, String> {
    if !replaying() {
        return Ok(None);
    }
    if let Some(error) = replay_error() {
        return Err(error);
    }
    let (precision, rounding) = slr_vm::clock::current_controls();
    let result = FRAME_TIMER.with(|v| {
        v.borrow_mut()
            .as_mut()
            .ok_or("frame timer companion absent".to_owned())?
            .0
            .next(frame, precision, rounding)
    });
    if let Err(error) = &result {
        REPLAY_ERROR.with(|v| *v.borrow_mut() = Some(error.clone()));
    }
    result.map(Some)
}

/// The caller has performed the actual scoped native thread destructor.
/// This permits a later original allocation to reuse that thread's address.
pub fn retire_clock_thread(id: ThreadId) {
    REPLAY.with(|value| {
        if let Some(replay) = value.borrow_mut().as_mut() {
            replay.retire_thread(queue_identity(id));
        }
    });
    QUEUE_CONTEXTS.with(|value| {
        value.borrow_mut().remove(&id);
    });
}

fn queue_identity(id: ThreadId) -> u64 {
    id as u64 + 1
}
fn assign_clock_metadata(thread: &mut Thread, id: ThreadId, name: &str) {
    thread.clock_identity = queue_identity(id);
    thread.clock_name = Some(name.to_owned());
}
fn cache_clock_thread(vm: &VmHost, id: ThreadId, thread: &Thread) {
    QUEUE_CONTEXTS.with(|v| {
        v.borrow_mut().insert(id, thread.clock_context(vm));
    });
}
fn refresh_clock_queues(vm: &VmHost, queues: &ThreadQueues) {
    let mut ids = queues.run_order();
    ids.extend(queues.notify_order());
    for id in ids {
        let state = INTERP.with(|v| v.borrow().get(&id).map(|t| t.clock_context(vm)));
        let context = state.unwrap_or_else(|| slr_vm::clock::ThreadContext {
            id: queue_identity(id),
            name: Some(queues.get(id).name.clone()),
            class: None,
            method: None,
            node_index: None,
        });
        QUEUE_CONTEXTS.with(|v| {
            v.borrow_mut().insert(id, context);
        });
    }
}
fn request_context() -> Result<RequestContext, String> {
    let contexts = slr_vm::clock::current_thread_contexts();
    let convert = |v: slr_vm::clock::ThreadContext| ThreadContext {
        id: v.id,
        name: v.name,
        class: v.class,
        method: v.method,
        node_index: v.node_index,
    };
    let cursor = slr_vm::clock::current_driver_cursor()
        .map(|id| {
            contexts
                .iter()
                .rev()
                .find(|v| v.id == queue_identity(id))
                .cloned()
                .or_else(|| QUEUE_CONTEXTS.with(|v| v.borrow().get(&id).cloned()))
                .map(convert)
                .ok_or_else(|| format!("clock driver cursor {id} has no runtime metadata"))
        })
        .transpose()?;
    Ok(RequestContext {
        thread: contexts.last().cloned().map(convert),
        driver_cursor: cursor,
    })
}
fn diagnostic_event(category: &str, fields: Vec<(String, String)>) -> Option<u64> {
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: category.to_owned(),
        fields,
    })
}
fn context_json(c: &RequestContext) -> serde_json::Value {
    let thread = |t: &ThreadContext| serde_json::json!({"id":t.id,"name":t.name,"class":t.class,"method":t.method,"node_index":t.node_index});
    serde_json::json!({"thread":c.thread.as_ref().map(thread),"driver_cursor":c.driver_cursor.as_ref().map(thread)})
}
fn diagnostic_queues(category: &str, tick: u64, queues: &ThreadQueues) {
    if !slr_vm::diagnostics::enabled() {
        return;
    }
    let entries = |ids: Vec<ThreadId>| {
        ids.into_iter().map(|id|{let t=queues.get(id);serde_json::json!({"id":queue_identity(id),"name":t.name,"flags":t.flags,"priority":t.priority,"sleep_deadline_bits":t.sleep_deadline.to_bits()})}).collect::<Vec<_>>()
    };
    diagnostic_event(
        category,
        vec![
            ("tick".into(), tick.to_string()),
            (
                "ready".into(),
                serde_json::Value::Array(entries(queues.run_order())).to_string(),
            ),
            (
                "waiting".into(),
                serde_json::Value::Array(entries(queues.notify_order())).to_string(),
            ),
            (
                "driver_cursor".into(),
                format!("{:?}", LAST_DRIVER_CURSOR.with(Cell::get)),
            ),
        ],
    );
}
/// Parked interpreter stacks/locals/cells read directly from the current port.
pub fn diagnostic_threads(vm: &VmHost) -> Vec<slr_vm::diagnostics::FailureSnapshot> {
    INTERP.with(|threads| {
        let mut snapshots = threads
            .borrow()
            .values()
            .map(|thread| {
                let mut snapshot = slr_vm::diagnostics::failure_snapshot(vm, thread);
                snapshot.driver_cursor = LAST_DRIVER_CURSOR.with(Cell::get);
                snapshot
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|snapshot| snapshot.thread);
        snapshots
    })
}

fn replay_clock() -> Result<X87, String> {
    if let Some(error) = replay_error() {
        return Err(error);
    }
    let context = request_context()?;
    let (precision, rounding) = slr_vm::clock::current_controls();
    let site = slr_vm::clock::current_site();
    let diagnostic = slr_vm::diagnostics::enabled();
    let sequence = if diagnostic {
        let pending = REPLAY.with(|v| v.borrow().as_ref().and_then(Replay::pending));
        let mut fields = vec![
            ("site".into(), format!("{site:?}")),
            ("actual_context".into(), context_json(&context).to_string()),
            ("precision".into(), format!("{precision:?}")),
            ("rounding".into(), format!("{rounding:?}")),
        ];
        if let Some(p) = pending {
            fields.extend([
                ("request_ordinal".into(), p.ordinal.to_string()),
                ("expected_site".into(), format!("{:?}", p.site)),
                ("source_counter".into(), p.counter.to_string()),
                ("source_frequency".into(), p.frequency.to_string()),
                ("source_baseline".into(), p.baseline.to_string()),
                (
                    "source_entry_icount".into(),
                    p.source_entry_icount.to_string(),
                ),
                (
                    "source_completed_icount".into(),
                    p.source_completed_icount.to_string(),
                ),
            ]);
        }
        diagnostic_event("clock-replay-request", fields)
    } else {
        None
    };
    let result = REPLAY.with(|v| {
        v.borrow_mut()
            .as_mut()
            .ok_or("clock replay absent".to_owned())?
            .read(site, &context, precision, rounding)
            .map_err(|e| e.to_string())
    });
    if diagnostic {
        let position = REPLAY.with(|v| v.borrow().as_ref().map_or(0, Replay::consumed));
        let mut fields = vec![("consumed".into(), position.to_string())];
        match &result {
            Ok(value) => fields.push((
                "st0_le".into(),
                value
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            )),
            Err(error) => fields.push(("error".into(), error.clone())),
        };
        slr_vm::diagnostics::with_context(
            slr_vm::diagnostics::Context {
                parent_sequence: sequence,
                ..Default::default()
            },
            || {
                diagnostic_event("clock-replay-result", fields);
            },
        );
    }
    result
}

/// Declare the simulated clock: it starts at `0.0` ms and advances `step` ms
/// on every read.
pub fn configure_clock(step: f64) {
    REPLAY.with(|v| *v.borrow_mut() = None);
    FRAME_TIMER.with(|v| *v.borrow_mut() = None);
    REPLAYING.with(|v| v.set(false));
    REPLAY_ERROR.with(|v| *v.borrow_mut() = None);
    QUEUE_CONTEXTS.with(|v| v.borrow_mut().clear());
    CLOCK.with(|c| c.set((0.0, step)));
}

/// One read of the simulated clock (`FUN_005516c0`'s stand-in).
pub fn clock_ms() -> X87 {
    if replaying() {
        return match replay_clock() {
            Ok(value) => {
                let (precision, rounding) = slr_vm::clock::current_controls();
                CLOCK.with(|v| {
                    let (_, step) = v.get();
                    v.set((value.to_f32(precision, rounding) as f64, step));
                });
                value
            }
            Err(error) => {
                REPLAY_ERROR.with(|v| {
                    if v.borrow().is_none() {
                        *v.borrow_mut() = Some(error);
                    }
                });
                X87::from_f64(f64::NAN)
            }
        };
    }
    CLOCK.with(|c| {
        let (now, step) = c.get();
        c.set((now + step, step));
        X87::from_f64(now)
    })
}

/// The simulated clock as the language host's `f32` reading.
#[must_use]
pub fn clock_f32() -> f32 {
    let (precision, rounding) = slr_vm::clock::current_controls();
    clock_ms().to_f32(precision, rounding)
}

/// Advance the simulated clock by one frame interval (milliseconds).
pub fn advance_ms(ms: f64) {
    if replaying() {
        return;
    }
    CLOCK.with(|c| {
        let (now, step) = c.get();
        c.set((now + ms, step));
    });
}

/// Current simulated time without advancing it.
#[must_use]
pub fn now_ms() -> f64 {
    CLOCK.with(|c| c.get().0)
}

/// Read-only acceptance observer: a blocked menu construction must not count
/// as idle merely because GameLogic has already stored its new state.
pub fn pending_menu_call(vm: &VmHost) -> bool {
    INTERP.with(|threads| {
        threads.borrow().values().any(|thread| {
            thread.frames.iter().any(|frame| {
                vm.loader
                    .context_id(frame.class_ctx)
                    .and_then(|id| vm.loader.class_name(id))
                    .is_some_and(|name| {
                        matches!(
                            name,
                            "java.game.MainMenu"
                                | "java.game.MainMenuDialog"
                                | "java.game.OptionsDialog"
                        )
                    })
            })
        })
    })
}

fn mirror(thread: &mut Thread, before: bool) {
    let Some(id) = CURRENT.with(Cell::get) else {
        return;
    };
    // Field initializers execute on a distinct transient interpreter thread.
    // Its native calls must not overwrite the parked creation/callback queue
    // entry, nor inherit that entry's flags/name when the native returns.
    if thread.clock_identity != queue_identity(id) {
        return;
    }
    with_threads(|host| {
        if before {
            host.set_current(Some(id));
            host.queues_mut().get_mut(id).flags = thread.flags;
        } else {
            thread.flags = host.queues().get(id).flags;
            thread.clock_name = Some(host.queues().get(id).name.clone());
        }
    });
}

/// Installed as `VmHost::native_thread_hook`.
pub fn hook(thread: &mut Thread, before: bool) {
    mirror(thread, before);
}

fn object_class(vm: &VmHost, word: u32) -> Option<ClassId> {
    match vm.objects.get(word.checked_sub(0x1000)? as usize)?.class {
        ClassRef::Loaded(id) => Some(id),
        ClassRef::Array(_) => None,
    }
}

/// `java.lang.Thread.start()V` (`LAB_0047c570`).
pub fn thread_start(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let receiver = info.args.last().map_or(0, |v| v.payload);
    let id = with_threads(|h| h.begin_start(receiver))?;
    let target = match vm.read_field(receiver, "target") {
        Ok(value) if value.payload != 0 => value.payload,
        other => {
            vm.native_failure = Some(format!(
                "Thread.start: receiver {receiver:#x} has no readable target ({other:?}); the exe dereferences it unchecked"
            ));
            return None;
        }
    };
    let Some(class) = object_class(vm, target) else {
        vm.native_failure = Some(format!(
            "Thread.start: target {target:#x} is not a loaded object"
        ));
        return None;
    };
    let mut thread = vm.new_thread();
    thread.alu.precision = vm.runtime_precision;
    let name = with_threads(|h| h.queues().get(id).name.clone());
    assign_clock_metadata(&mut thread, id, &name);
    match vm.enter_method(&mut thread, class, target, "run", &[]) {
        Ok(Flow::Advance) => {
            with_threads(|h| h.finish_start(receiver, id, true));
        }
        Ok(Flow::Stay) => {
            cache_clock_thread(vm, id, &thread);
            INTERP.with(|m| m.borrow_mut().insert(id, thread));
            with_threads(|h| h.finish_start(receiver, id, false));
        }
        Err(error) => {
            vm.native_failure = Some(format!("Thread.start: target.run dispatch {error:?}"));
        }
    }
    None
}

/// Run a script entry point to completion on a fresh queued thread, as the
/// original creation bridge does (`THRD-CREATE`, priority 10, flags 0, then an
/// immediate unbudgeted `Thread::run`). Returns the interpreter thread so a
/// failure can describe its frames. A thread that blocks stays resumable.
#[allow(clippy::result_large_err)]
pub fn run_created(
    vm: &mut VmHost,
    name: &str,
    class: ClassId,
    receiver: u32,
    method: &str,
    args: &[Value],
) -> Result<(), (Thread, slr_vm::interp::VmError)> {
    let id = with_threads(|h| h.queues_mut().spawn(10, 0, name));
    let mut thread = vm.new_thread();
    thread.record_origin(10, 0, name);
    thread.alu.precision = vm.runtime_precision;
    assign_clock_metadata(&mut thread, id, name);
    let flow = match vm.enter_method(&mut thread, class, receiver, method, args) {
        Ok(flow) => flow,
        Err(error) => return Err((thread, error)),
    };
    let outer = CURRENT.with(|c| c.replace(Some(id)));
    let result = if flow == Flow::Stay {
        let before = with_threads(|h| h.queues().get(id).flags);
        thread.flags |= before;
        thread.run(vm, 0.0, &mut clock_ms).map(|_| ())
    } else {
        Ok(())
    };
    CURRENT.with(|c| c.set(outer));
    match result {
        Ok(()) => {
            with_threads(|h| h.queues_mut().get_mut(id).flags = thread.flags);
            if thread.flags & (slr_vm::interp::flags::STOPPED | slr_vm::interp::flags::DEAD) == 0
                && flow == Flow::Stay
            {
                INTERP.with(|m| m.borrow_mut().insert(id, thread));
            }
            Ok(())
        }
        Err(error) => Err((thread, error)),
    }
}

/// Original425a90→4255e0 callback wrapper. The third stack argument requests
/// a return value: it sets thread flag1 and makes destruction immediate. Void
/// callbacks use flag0 and retain a blocked continuation in the run queue.
#[allow(clippy::result_large_err)]
pub fn run_callback(
    vm: &mut VmHost,
    resource_name: &str,
    class: ClassId,
    receiver: u32,
    method: &str,
    args: &[Value],
    request_return: bool,
) -> Result<Option<Value>, (Thread, slr_vm::interp::VmError)> {
    // Original554900 appends into the wrapper's64-byte buffer, includingNUL.
    let mut name = format!("THRD-RUNVMI {resource_name}.{method}");
    let mut end = name.len().min(63);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    let flags = u32::from(request_return);
    let id = with_threads(|host| host.queues_mut().spawn(10, flags, &name));
    let mut thread = vm.new_thread();
    thread.record_origin(10, flags, &name);
    thread.alu.precision = vm.runtime_precision;
    thread.flags = flags;
    assign_clock_metadata(&mut thread, id, &name);
    let outer = CURRENT.with(|current| current.replace(Some(id)));
    with_threads(|host| host.set_current(Some(id)));
    let mut dispatched = None;
    let result = (|| {
        let flow = vm.enter_method(&mut thread, class, receiver, method, args)?;
        dispatched = Some(flow);
        if flow == Flow::Stay {
            thread.run(vm, 0.0, &mut clock_ms)?;
        }
        if request_return || flow == Flow::Advance {
            let cell = thread.pop_deref_with(vm)?;
            let value = cell.value();
            thread.release(cell);
            Ok(Some(value))
        } else {
            Ok(None)
        }
    })();
    CURRENT.with(|current| current.set(outer));
    match result {
        Ok(value) => {
            if !request_return && dispatched == Some(Flow::Stay) {
                with_threads(|host| {
                    host.queues_mut().get_mut(id).flags = thread.flags;
                    host.set_current(outer);
                });
                if thread.flags & (slr_vm::interp::flags::STOPPED | slr_vm::interp::flags::DEAD)
                    == 0
                {
                    cache_clock_thread(vm, id, &thread);
                    INTERP.with(|threads| {
                        threads.borrow_mut().insert(id, thread);
                    });
                }
            } else {
                with_threads(|host| {
                    host.dispose_scoped(id);
                    host.set_current(outer);
                });
                retire_clock_thread(id);
            }
            Ok(value)
        }
        Err(error) => {
            with_threads(|host| {
                host.dispose_scoped(id);
                host.set_current(outer);
            });
            retire_clock_thread(id);
            Err((thread, error))
        }
    }
}

/// Original408560's scoped priority10,flags1 THRD-FIN. Its immediate
/// destructor unlinks the temporary thread even if finalize blocked.
#[allow(clippy::result_large_err)]
pub fn run_finalizer(
    vm: &mut VmHost,
    receiver: u32,
) -> Result<(), (Thread, slr_vm::interp::VmError)> {
    let id = with_threads(|host| host.queues_mut().spawn(10, 1, "THRD-FIN"));
    let mut thread = vm.new_thread();
    thread.record_origin(10, 1, "THRD-FIN");
    thread.alu.precision = vm.runtime_precision;
    assign_clock_metadata(&mut thread, id, "THRD-FIN");
    thread.flags = 1;
    let outer = CURRENT.with(|current| current.replace(Some(id)));
    with_threads(|host| host.set_current(Some(id)));
    let class = receiver
        .checked_sub(0x1000)
        .and_then(|index| vm.objects.get(index as usize))
        .and_then(|object| match object.class {
            ClassRef::Loaded(class) => Some(class),
            ClassRef::Array(_) => vm.loader.lookup("java.lang.Object"),
        });
    let result = match class {
        Some(class) => match vm.enter_method(&mut thread, class, receiver, "finalize", &[]) {
            Ok(Flow::Stay) => thread.run(vm, 0.0, &mut clock_ms).map(|_| ()),
            Ok(Flow::Advance) => Ok(()),
            Err(error) => Err(error),
        },
        None => Err(slr_vm::interp::VmError::NoFrame),
    };
    CURRENT.with(|current| current.set(outer));
    with_threads(|host| {
        host.dispose_scoped(id);
        host.set_current(outer);
    });
    retire_clock_thread(id);
    result.map_err(|error| (thread, error))
}

///418290 walks ready then waiting threads; each thread's script object comes
/// before its current-to-bottom frame cells. Detached constructor contexts are
/// excluded just as the original queue walk excludes them.
pub(crate) fn gc_thread_roots(vm: &mut VmHost) -> Result<Vec<u32>, String> {
    let ids = with_threads(|host| {
        let mut ids = host.queues().run_order();
        ids.extend(host.queues().notify_order());
        ids
    });
    let mut roots = Vec::new();
    for id in ids {
        if let Some(word) = with_threads(|host| host.script_object(id)) {
            roots.push(word);
        }
        INTERP.with(|threads| {
            if let Some(thread) = threads.borrow().get(&id) {
                roots.extend(
                    vm.gc_thread_roots(thread)
                        .map_err(|error| format!("GC frame roots:{error:?}"))?,
                );
            }
            Ok::<_, String>(())
        })?;
    }
    Ok(roots)
}

/// Failure of one scheduled run: the interpreter thread and its error.
pub struct RunFailure {
    pub id: ThreadId,
    pub name: String,
    pub thread: Thread,
    pub error: slr_vm::interp::VmError,
}

/// The scheduler driver state (`FUN_00418d10`'s object).
pub struct Pump {
    pub scheduler: Scheduler,
    pub finals: FinalizerQueues,
    pub ticks: u64,
    pub runs: u64,
}

fn gc_failure(error: String) -> Box<RunFailure> {
    Box::new(RunFailure {
        id: 0,
        name: error,
        thread: Thread::new(),
        error: slr_vm::interp::VmError::Unsupported(0x0fff),
    })
}

impl Pump {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
            finals: FinalizerQueues::new(),
            ticks: 0,
            runs: 0,
        }
    }

    /// One `FUN_00418d10(jvm, 10.0f)` pass. The first script error stops the
    /// remaining runs of the pass and is returned with its thread.
    pub fn tick(&mut self, vm: &mut VmHost) -> Result<SchedExit, Box<RunFailure>> {
        self.ticks += 1;
        with_threads(|h| diagnostic_queues("scheduler-tick-enter", self.ticks, h.queues()));
        self.scheduler.begin_tick();
        crate::gc_runtime::retain_queued(vm, &mut self.finals);
        let mut drain = self.scheduler.begin_fairness(&self.finals);
        loop {
            crate::gc_runtime::retain_queued(vm, &mut self.finals);
            let Some(word) = drain.next(&mut self.finals) else {
                break;
            };
            crate::gc_runtime::finalize(vm, &mut self.finals, word).map_err(gc_failure)?;
        }
        drain.finish(&mut self.finals);
        self.scheduler.callback_ticks = vm.gc.allocation_counter();
        let mut gc_error = None;
        self.scheduler
            .callbacks_phase(&mut clock_ms, &mut || match crate::gc_runtime::step(
                vm,
                &mut self.finals,
            ) {
                Ok(result) => result,
                Err(error) => {
                    gc_error = Some(error);
                    CallbackRun::Retained
                }
            });
        if let Some(error) = gc_error {
            return Err(gc_failure(error));
        }
        let mut queues = with_threads(|h| std::mem::replace(h.queues_mut(), ThreadQueues::new()));
        refresh_clock_queues(vm, &queues);
        let mut failure: Option<Box<RunFailure>> = None;
        let mut runs = 0;
        let mut reap_error = None;
        let exit = {
            // The driver invokes runner and destructor sequentially; both
            // require exclusive access to the same VM at their actual points.
            let vm = RefCell::new(vm);
            let mut clock = clock_ms;
            let mut run =
                |q: &mut ThreadQueues, id: ThreadId, slice: f32, clock: &mut dyn FnMut() -> X87| {
                    if failure.is_some() {
                        return;
                    }
                    let mut host = vm.borrow_mut();
                    let vm = &mut **host;
                    let name = q.get(id).name.clone();
                    let Some(mut thread) = INTERP.with(|m| m.borrow_mut().remove(&id)) else {
                        failure = Some(Box::new(RunFailure {
                            id,
                            name,
                            thread: Thread::new(),
                            error: slr_vm::interp::VmError::EmptyThread,
                        }));
                        return;
                    };
                    assign_clock_metadata(&mut thread, id, &name);
                    runs += 1;
                    with_threads(|h| {
                        std::mem::swap(h.queues_mut(), q);
                        h.set_current(Some(id));
                        thread.flags = h.queues().get(id).flags;
                    });
                    CURRENT.with(|c| c.set(Some(id)));
                    thread.alu.precision = vm.runtime_precision;
                    let result = thread.run(vm, slice, clock);
                    CURRENT.with(|c| c.set(None));
                    with_threads(|h| {
                        h.queues_mut().get_mut(id).flags = thread.flags;
                        h.set_current(None);
                        std::mem::swap(h.queues_mut(), q);
                    });
                    cache_clock_thread(vm, id, &thread);
                    match result {
                        Ok(_) => {
                            if thread.flags & slr_vm::interp::flags::DEAD == 0 {
                                INTERP.with(|m| m.borrow_mut().insert(id, thread));
                            }
                        }
                        Err(error) => {
                            failure = Some(Box::new(RunFailure {
                                id,
                                name,
                                thread,
                                error,
                            }));
                        }
                    }
                    refresh_clock_queues(vm, q);
                };
            let mut reap = |queues: &mut ThreadQueues, id: ThreadId| {
                let script = with_threads(|host| {
                    std::mem::swap(host.queues_mut(), queues);
                    let script = host.destroy_thread(id);
                    std::mem::swap(host.queues_mut(), queues);
                    script
                });
                INTERP.with(|threads| {
                    threads.borrow_mut().remove(&id);
                });
                retire_clock_thread(id);
                if let Some(script) = script {
                    let mut host = vm.borrow_mut();
                    if let Err(error) = host.write_field(script, "ptr", Value::int(0)) {
                        reap_error = Some(format!("Thread destructor Native.ptr: {error:?}"));
                    }
                }
            };
            self.scheduler
                .drive_with_reaper(&mut queues, 10.0, 0, &mut clock, &mut run, &mut reap)
        };
        self.runs += runs;
        LAST_DRIVER_CURSOR.with(|v| v.set(self.scheduler.cursor()));
        diagnostic_queues("scheduler-tick-return", self.ticks, &queues);
        with_threads(|h| *h.queues_mut() = queues);
        if let Some(error) = reap_error {
            return Err(gc_failure(error));
        }
        if let Some(failure) = failure {
            return Err(failure);
        }
        Ok(exit)
    }
}

impl Default for Pump {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod clock_provider_tests {
    use super::*;
    use slr_vm::clock::{with_site, ClockSite};
    #[test]
    fn shared_raw_input_provider_stops_on_wrong_role_and_legacy_begin_is_noop() {
        slr_vm::diagnostics::configure("clock-provider-test".into(), 64, None);
        configure_clock(0.01);
        begin_replay_after_game_init().unwrap();
        assert!(!replaying());
        assert_eq!(clock_f32(), 0.0);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl");
        configure_replay(&path).unwrap();
        assert!(replaying());
        assert!(begin_replay_after_game_init()
            .unwrap_err()
            .contains("position 0 expected 6207"));
        with_site(ClockSite::ThreadEnter, clock_ms);
        let error = replay_error().unwrap();
        assert!(error.contains("FPU control mismatch"));
        with_site(ClockSite::TickEnter, clock_ms);
        assert_eq!(replay_error(), Some(error));
        assert!(finish_accepted().is_err());
        let events = slr_vm::diagnostics::recent();
        assert!(events.iter().any(|event|matches!(&event.kind,slr_vm::diagnostics::EventKind::External {category,fields} if category=="clock-replay-request" && fields.iter().any(|(key,_)|key=="source_counter"))));
        assert!(events.iter().any(|event|matches!(&event.kind,slr_vm::diagnostics::EventKind::External {category,fields} if category=="clock-replay-result" && fields.iter().any(|(key,_)|key=="error")) && event.context.parent_sequence.is_some()));
        slr_vm::diagnostics::configure(String::new(), 0, None);
        configure_clock(0.0);
        assert!(!replaying());
        assert!(replay_error().is_none());
    }
}

#[cfg(test)]
mod callback_wrapper_tests {
    use super::*;

    fn stock_vm(leaf: &str) -> VmHost {
        let mut vm = VmHost::new();
        vm.start_machine_recording().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock");
        for file in [
            "system/Scripts/lang/Object.class",
            "system/Scripts/lang/Native.class",
            leaf,
        ] {
            vm.loader
                .load_file_named(&std::fs::read(root.join(file)).unwrap())
                .unwrap();
        }
        vm.native_thread_hook = Some(hook);
        vm
    }

    #[test]
    fn stock_script_returns_scalar_and_destroys_protected_callback_thread() {
        slr_lang::thread::reset();
        configure_clock(0.0);
        slr_vm::diagnostics::configure("callback-scalar".into(), 128, None);
        let mut vm = VmHost::new();
        vm.start_machine_recording().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock");
        vm.loader
            .load_file_named(&std::fs::read(root.join("system/Scripts/lang/Object.class")).unwrap())
            .unwrap();
        let object = vm.alloc_object("java.lang.Object").unwrap();
        let class = vm.loader.lookup("java.lang.Object").unwrap();
        let original = with_threads(|host| host.queues().run_order());
        let result = run_callback(
            &mut vm,
            "Config",
            class,
            object,
            "equals",
            &[Value::reference(object)],
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.tag.name(), "I");
        let machine = vm.machine_trace().pending(usize::MAX);
        let origin = machine.iter().find(|entry| matches!(&entry.event,
            slr_vm::machine::Event::ThreadOrigin { priority: 10, flags: 1, name, .. }
            if name == "THRD-RUNVMI Config.equals")).unwrap();
        let slr_vm::machine::Event::ThreadOrigin { identity, .. } = origin.event else { unreachable!() };
        let first_frame = machine.iter().find(|entry| matches!(entry.event,
            slr_vm::machine::Event::Frame { thread, .. } if thread == identity)).unwrap();
        assert!(origin.sequence < first_frame.sequence);
        with_threads(|host| {
            assert_eq!(host.queues().run_order(), original);
            assert!(host.queues().notify_order().is_empty());
            assert_eq!(host.current(), None);
        });
        let events = slr_vm::diagnostics::recent();
        assert!(events.iter().any(|event| matches!(
            event.kind,
            slr_vm::diagnostics::EventKind::ThreadRun { .. }
        ) && event.context.thread_name.as_deref()
            == Some("THRD-RUNVMI Config.equals")));
        let state = with_threads(|host| host.queues().threads().last().unwrap().clone());
        assert_ne!(state.flags & 1, 0);
        assert_ne!(state.flags & slr_vm::interp::flags::DEAD, 0);
        slr_vm::diagnostics::configure(String::new(), 0, None);
    }

    #[test]
    fn void_callback_that_waits_retains_real_continuation_and_unprotected_flags() {
        fn close(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
            with_threads(|host| host.wait(0x1234).unwrap());
            None
        }
        slr_lang::thread::reset();
        configure_clock(0.0);
        let mut vm = stock_vm("system/Scripts/io/File.class");
        slr_io::file::install(&mut vm);
        vm.native_impls.insert(slr_io::file::FILE_CLOSE, close);
        let object = vm.alloc_object("java.io.File").unwrap();
        let class = vm.loader.lookup("java.io.File").unwrap();
        assert!(
            run_callback(&mut vm, "file", class, object, "finalize", &[], false)
                .unwrap()
                .is_none()
        );
        let machine = vm.machine_trace().pending(usize::MAX);
        assert!(machine.iter().any(|entry| matches!(&entry.event,
            slr_vm::machine::Event::ThreadOrigin { priority: 10, flags: 0, name, .. }
            if name == "THRD-RUNVMI file.finalize")));
        let id = with_threads(|host| {
            let ids = host.queues().notify_order();
            assert_eq!(ids.len(), 1);
            let id = ids[0];
            assert_eq!(host.queues().get(id).flags & 1, 0);
            assert_eq!(host.queues().get(id).name, "THRD-RUNVMI file.finalize");
            assert_eq!(host.waiters(0x1234), vec![id]);
            id
        });
        assert!(INTERP.with(|threads| threads.borrow().contains_key(&id)));
        INTERP.with(|threads| threads.borrow_mut().remove(&id));
        with_threads(|host| host.dispose_scoped(id));
    }
}

#[cfg(test)]
mod mirror_identity_tests {
    use super::*;

    #[test]
    fn nested_initializer_native_hook_preserves_both_threads_flags_and_names() {
        slr_lang::thread::reset();
        let id = with_threads(|host| host.queues_mut().spawn(10, 0x10, "THRD-CREATE"));
        let mut initializer = Thread::new();
        initializer.clock_name = Some("THRD-CLASSVAR-INI".into());
        initializer.flags = 1;
        CURRENT.with(|current| current.set(Some(id)));
        hook(&mut initializer, true);
        hook(&mut initializer, false);
        assert_eq!(initializer.flags, 1);
        assert_eq!(initializer.clock_name.as_deref(), Some("THRD-CLASSVAR-INI"));
        with_threads(|host| {
            assert_eq!(host.queues().get(id).flags, 0x10);
            assert_eq!(host.queues().get(id).name, "THRD-CREATE");
        });
        let mut queued = Thread::new();
        assign_clock_metadata(&mut queued, id, "old name");
        hook(&mut queued, false);
        assert_eq!(queued.flags, 0x10);
        assert_eq!(queued.clock_name.as_deref(), Some("THRD-CREATE"));
        CURRENT.with(|current| current.set(None));
        slr_lang::thread::reset();
    }
}

#[cfg(test)]
mod driver_destructor_tests {
    use super::*;

    #[test]
    fn pump_destruction_clears_vm_handle_roots_stacks_and_clock_cache() {
        slr_lang::thread::reset();
        configure_clock(0.01);
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(slr_vm::vmclass::Class {
            kind: 0,
            name: "java.lang.Thread".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: vec![slr_vm::vmclass::Field {
                class: "java.lang.Thread".into(),
                name: "ptr".into(),
                ty: "I".into(),
                is_static: false,
            }],
        });
        let object = vm.alloc_object("java.lang.Thread").unwrap();
        vm.write_field(object, "ptr", Value::int(0x1234)).unwrap();
        let id = with_threads(|host| {
            let id = host.init(object, 0);
            host.queues_mut().get_mut(id).flags = slr_vm::interp::flags::DEAD;
            id
        });
        let mut thread = Thread::new();
        assign_clock_metadata(&mut thread, id, "done");
        cache_clock_thread(&vm, id, &thread);
        INTERP.with(|threads| threads.borrow_mut().insert(id, thread));
        crate::gc_runtime::configure_roots(object, object, None).unwrap();
        let mut pump = Pump::new();
        match pump.tick(&mut vm) {
            Ok(exit) => assert_eq!(exit, SchedExit::TimeUp),
            Err(failure) => panic!("{}: {:?}", failure.name, failure.error),
        }
        assert_eq!(pump.scheduler.reaped(), &[id]);
        assert_eq!(vm.read_field(object, "ptr").unwrap().payload, 0);
        with_threads(|host| {
            assert_eq!(host.object(object).handle, None);
            assert_eq!(host.script_object(id), None);
            assert!(host.queues().run_order().is_empty());
        });
        assert!(!INTERP.with(|threads| threads.borrow().contains_key(&id)));
        assert!(!QUEUE_CONTEXTS.with(|contexts| contexts.borrow().contains_key(&id)));
        slr_lang::thread::reset();
    }
}
