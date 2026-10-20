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
use std::{cell::Cell, cell::RefCell, collections::HashMap};

thread_local! {
    static INTERP: RefCell<HashMap<ThreadId, Thread>> = RefCell::new(HashMap::new());
    static CURRENT: Cell<Option<ThreadId>> = const { Cell::new(None) };
    static CLOCK: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
}

/// Declare the simulated clock: it starts at `0.0` ms and advances `step` ms
/// on every read.
pub fn configure_clock(step: f64) {
    CLOCK.with(|c| c.set((0.0, step)));
}

/// One read of the simulated clock (`FUN_005516c0`'s stand-in).
pub fn clock_ms() -> X87 {
    CLOCK.with(|c| {
        let (now, step) = c.get();
        c.set((now + step, step));
        X87::from_f64(now)
    })
}

/// The simulated clock as the language host's `f32` reading.
#[must_use]
pub fn clock_f32() -> f32 {
    CLOCK.with(|c| {
        let (now, step) = c.get();
        c.set((now + step, step));
        now as f32
    })
}

/// Advance the simulated clock by one frame interval (milliseconds).
pub fn advance_ms(ms: f64) {
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

fn mirror(thread: &mut Thread, before: bool) {
    let Some(id) = CURRENT.with(Cell::get) else {
        return;
    };
    with_threads(|host| {
        if before {
            host.set_current(Some(id));
            host.queues_mut().get_mut(id).flags = thread.flags;
        } else {
            thread.flags = host.queues().get(id).flags;
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
    let mut thread = Thread::new();
    match vm.enter_method(&mut thread, class, target, "run", &[]) {
        Ok(Flow::Advance) => {
            with_threads(|h| h.finish_start(receiver, id, true));
        }
        Ok(Flow::Stay) => {
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
    let mut thread = Thread::new();
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
        let mut queues = with_threads(|h| std::mem::replace(h.queues_mut(), ThreadQueues::new()));
        let mut failure: Option<Box<RunFailure>> = None;
        let mut runs = 0;
        let mut callback_stop = false;
        let exit = {
            let mut clock = clock_ms;
            let mut run =
                |q: &mut ThreadQueues, id: ThreadId, slice: f32, clock: &mut dyn FnMut() -> X87| {
                    if failure.is_some() {
                        return;
                    }
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
                    runs += 1;
                    with_threads(|h| {
                        std::mem::swap(h.queues_mut(), q);
                        h.set_current(Some(id));
                        thread.flags = h.queues().get(id).flags;
                    });
                    CURRENT.with(|c| c.set(Some(id)));
                    let result = thread.run(vm, slice, clock);
                    CURRENT.with(|c| c.set(None));
                    with_threads(|h| {
                        h.queues_mut().get_mut(id).flags = thread.flags;
                        h.set_current(None);
                        std::mem::swap(h.queues_mut(), q);
                    });
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
                };
            let mut finalize = |_| {};
            let mut callbacks = || {
                // The callback block walks the script-registered record list
                // through `FUN_00408500`; no such record can exist before the
                // callback natives are wired, so reaching it is a boundary.
                callback_stop = true;
                CallbackRun::Consumed
            };
            self.scheduler.tick(
                &mut queues,
                &mut self.finals,
                10.0,
                &mut clock,
                &mut finalize,
                &mut callbacks,
                &mut run,
            )
        };
        self.runs += runs;
        with_threads(|h| *h.queues_mut() = queues);
        if let Some(failure) = failure {
            return Err(failure);
        }
        if callback_stop {
            return Err(Box::new(RunFailure {
                id: 0,
                name: "scheduler callback block".into(),
                thread: Thread::new(),
                error: slr_vm::interp::VmError::Unsupported(0x0fff),
            }));
        }
        Ok(exit)
    }
}

impl Default for Pump {
    fn default() -> Self {
        Self::new()
    }
}
