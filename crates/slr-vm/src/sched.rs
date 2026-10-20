//! The cooperative scheduler's driver: `FUN_00416940` (ticket M1-05 T3) and
//! the per-frame tick `FUN_00418d10` with its `FUN_00418c20` drain (M1-05
//! T4), issue #32.
//!
//! `FUN_00416940(driver, float budget, int minPriority)` walks the run queue
//! and hands each runnable thread a time slice. The per-frame tick
//! `FUN_00418d10(driver, float budget)` calls it with the tick's own budget
//! and a zero priority floor (from the frame loop `FUN_00428960` at
//! `0x428acb`, guarded by `[system+0xe4] != 0`).
//!
//! The driver object this module models is the exe's JVM (vtable `0x5f0880`,
//! ctor `FUN_0040fb50`, reachable as `[system+0xe4]`): it owns the thread
//! queues ([`crate::threads`]), the scheduler cursor, the tick counter, the
//! frame-time accumulator and three lists `FUN_00418c20` works on.
//!
//! Machine model (`[static]` reads of the exe; addresses are the exe's):
//!
//! - The driver object carries the run queue at `+0x18` (the `[jvm+0x18]`
//!   list of [`crate::threads`]) and the scheduler cursor at `+0x78`, the
//!   thread the next walk resumes at (`null` = start at the head). The
//!   cursor outlives one call, so [`Scheduler`] owns it.
//! - Entry `0x416943-0x416997`: `t0 = clock()` (`FUN_005516c0`),
//!   `deadline = (float)(t0 + budget)`, and `timed = budget != 0.0f` (the
//!   `fcomp` against constant `0x5e73cc` = `0.0`). With a zero budget the
//!   loop never looks at the clock for the deadline at all. `slice_unit`
//!   starts at `0x41200000` = `10.0f` and is replaced by `budget` when
//!   `timed && budget < 10.0f` (constant `0x5e7334`).
//! - Loop top `0x41699f`: with `timed`, read the clock and return 1
//!   ([`SchedExit::TimeUp`]) when `clock >= deadline` (the `fnstsw`/`test
//!   ah,1` pair: `CF` set means `clock < deadline`; an unordered compare —
//!   a NaN deadline — leaves `CF` set and the loop continues).
//! - Load `0x4169bb`: while the cursor is null, load the head of the run
//!   queue (`[[driver+0x18]+8]`, null when that node's own `+4` is null,
//!   i.e. the list is empty). The entry flag `bl` ("a thread has run since
//!   the cursor was last loaded", true at entry) then returns 1 when it is
//!   already clear, and is cleared otherwise.
//! - Check `0x4169de`: a null cursor returns 0 ([`SchedExit::ListEnd`]).
//! - Skips: `flags & 0x82` ([`flags::SCHED_BLOCK`]: dead `0x80` or the
//!   just-created `0x02`) jumps straight to the cursor advance, past the
//!   stopped-to-dead step. `flags & 0x44` ([`flags::STOPPED`] or
//!   [`flags::RUNNING`]) and `priority < minPriority` jump to that step. A
//!   sleeping thread ([`SLEEPING`]) whose `+0x30` is older than the clock
//!   loses the bit (`0x416a09`, `test ah,0x41`: cleared only when
//!   `clock > deadline`, and the flag is re-read at `0x416a1f`), and a
//!   still-set bit skips the run. `0x20` ([`flags::NOTIFIED`]) is *not* a
//!   skip: the exe keeps suspended threads out of the run queue instead
//!   (`FUN_0041f570`), so a suspended thread that is in it does run.
//! - Slice `0x416a28-0x416a6c`: `time_slice(priority) * slice_unit`, with
//!   `priority >= 0 -> 1.0f + ((float)priority * 0.1f) * 99.0f` and
//!   `priority < 0 -> 0.1f + ((float)(priority + 10) * 0.1f) * 0.9f`
//!   (constants `0x5f08f0`/`0x5f08f4`/`0x5f08f8`/`0x5f08ec`/`0x5f08e8`,
//!   the `priority + 10` in 32-bit integer arithmetic before the `fild`).
//! - Run `0x416a70-0x416a8c`: the driver rewrites its own deadline local to
//!   `deadline - slice_unit + slice` first (extended-precision x87, stored
//!   back as f32), then calls `Thread::run` (`FUN_00420ff0`) with the
//!   thread in `ecx` and the slice as its one stack argument, and *discards*
//!   the returned float. The run flag `bl` is set.
//! - Stopped-to-dead `0x416a8e`: `0x40` set and the guard bit `0x01` clear
//!   (`STOPPED_IS_FINAL`) sets `0x80`. The flags are re-read after the
//!   run, so a thread that stopped inside it becomes dead here.
//! - Advance `0x416aa4`: cursor = the next thread (`+4`, null when that
//!   node's own `+4` is null, i.e. at the end of the list). When `0x04` is
//!   clear and `0x80` is set the exe calls vtable slot 0 with 1 — the
//!   deleting destructor `0x423b60`, which is `FUN_0041f4d0` (unlink the
//!   thread from its queue, free the frames) followed by `FUN_0054f5b0`
//!   (free the object).
//!
//! Tick `FUN_00418d10` (`[static]` for the whole body; `[oracle]` probes of
//! the exe's own function with a synthetic driver, a synthetic run queue and
//! hooked `FUN_005516c0`/`FUN_00420ff0`/`FUN_00408560`):
//!
//! - `0x418d18`: `[driver+0x40] += 1` (the tick counter), then `0x418d23`
//!   calls the drain `FUN_00418c20` with the driver in `ecx`.
//! - `0x418d28`: `t0 = clock()` stored as f32; `0x418e62` reads the clock
//!   again for the frame-time accumulator, so everything between the two
//!   reads is what the accumulator measures.
//! - `0x418d31-0x418e1a`: the callback block, entered only while
//!   `[driver+0x34] > [driver+0x38]` (`jle 0x418e62` skips it entirely). It
//!   walks the frame table at `+0x08` (0x200 slots of `0xc`), rebuilds the
//!   argument buffer `+0x20` (`FUN_00418110`/`FUN_00418290` when it is
//!   null, else `FUN_00418970`), walks the `+0x10` record list and calls
//!   `FUN_00408500` on its entries — the VM's callback/timer dispatch, not
//!   the scheduler's (M1-05 S7; the port reaches it through [`Callbacks`]).
//!   `0x418e57` then does `[driver+0x34] -= [driver+0x38]`, but only on the
//!   path that reaches the `+0x10` walk: `FUN_00418970` frees the argument
//!   buffer and nulls `+0x20` when its container is empty (`0x418b26`), and
//!   the `cmp [driver+0x20], 0 / jne 0x418e62` at `0x418e17` skips the walk
//!   and the subtraction while the buffer survives.
//! - `0x418e62-0x418e90`: `elapsed = clock() - t0` (the stored f32),
//!   `+0x24 = (int)(elapsed * 1000.0f * 0.8 + (double)(int)[+0x24] * 0.2)`
//!   with `__ftol` (`0x5d6750`, truncation toward zero) and only the low
//!   dword stored — an 80/20 smoothing of the frame time in milliseconds.
//!   The constants are `0x5f0910` (f32 `1000.0`), `0x5f0908` and `0x5f0900`
//!   (f64 `0.8`, `0.2`). `[oracle]`: an advancing clock with the gate
//!   closed reads `1.0` between the two reads and walks the accumulator
//!   `0, 800, 960, 992`.
//! - `0x418e93`: `FUN_00416940(driver, budget, 0)` — the tick's own float
//!   argument becomes the drive budget and the priority floor is 0 — and
//!   the tick returns that call's `eax`.
//!
//! Drain `FUN_00418c20` (`[static]` + `[oracle]`; executed on every tick,
//! and once per tick it is the whole of the exe's fairness policy):
//!
//! - `0x418c23`: the divisor update runs only while the tick counter's low
//!   byte is zero (`mov al, byte [driver+0x40]` / `jne 0x418c8b`), i.e. on
//!   one tick in 256 — right after `FUN_00418d10` incremented it, so on
//!   ticks 256, 512, ... .
//! - `0x418c2b`: the update needs the `+0x44` list to be non-empty (the
//!   `[first+4] == 0` test at `0x418c31`); an empty list leaves the divisor
//!   alone. Otherwise `[driver+0x3c] = ((count(+0x30) / [driver+0x3c]) +
//!   count(+0x44)) >> 6` (signed `idiv` at `0x418c65`, `sar eax, 6`), then
//!   clamped to `4 .. 0x40` by the store-then-clamp pair
//!   (`0x418c6d-0x418c88`). `[oracle]`: `(65535 / 4 + 1) >> 6` clamps to
//!   `0x40`, `(300 / 8 + 7) >> 6` clamps up to `4`.
//! - `0x418c8c`: the drain pops at most `[driver+0x3c]` entries off the
//!   *front* of the `+0x44` list (`[list+8]`, the head), handing each to
//!   `FUN_00408560` (`0x418cab`), which unlinks it and runs it as a
//!   `THRD-FIN` thread calling `finalize` (`0x607a38`).
//! - `0x418cbb`: when the drain ran dry before the last-but-one slot —
//!   `[driver+0x30] != 0` and the last probe found nothing, which the
//!   `dec ebx` bookkeeping makes exactly `popped < quota - 1`, plus the
//!   `quota == 0` case — the whole `+0x30` list is spliced onto the tail of
//!   the `+0x2c` list (`0x418cc4-0x418d02`) and `+0x30` is left empty.
//!   Popping exactly the quota, or all but one, skips the splice.
//!
//! The three lists hold the exe's *finalizer records*, not threads: each
//! record carries an execution context (`+0xc`), a state word (`+0x18`,
//! its bit 3 is set by `FUN_00408500` when the record is linked into
//! `+0x44`) and an argument container (`+0x10`), and the drain's
//! `FUN_00408560` runs the record's `finalize` method. `FUN_00418c20` never
//! inspects a record beyond moving it between lists, so the port keeps only
//! the record's identity ([`FinalizerId`]) and leaves the payload to the
//! finalizer/GC work; the M1-05 handoff's "notify queue drain" reading of
//! this function (a thread queue move) does not match the machine code —
//! `FUN_00408500` links records into `+0x44`, `FUN_00408560` calls
//! `finalize`, and `System.runFinalization` (`0x47c070`) is that same
//! subsystem's native, itself a no-op in build 606.
//!
//! x87 precision: the driver runs on the game's main thread after
//! `CreateDevice`, so its arithmetic is at the D3D9 24-bit precision
//! (`docs/physics.md` "Control word", ADR 0007) — the same
//! [`SCRIPT_PRECISION`] `Thread::run` uses. Every operation below is done
//! through [`X87`] at that precision, so the slice bits match the exe's.
//!
//! Port notes:
//!
//! - `Thread::run` is behind the [`Runner`] seam, and the clock behind a
//!   `FnMut` seam shared with it, so a test (and the oracle replay of
//!   M1-05 T6) can drive the driver with a fake and keep one clock trace.
//! - The deleting destructor has no equivalent: the port's thread arena
//!   ([`ThreadQueues`]) never removes a thread, and nothing public can
//!   unlink one. The driver therefore records the id in the scheduler's
//!   own reaped list and skips it in every later list walk, which is
//!   exactly what the exe's unlinked list looks like to the two questions
//!   the driver asks of a queue (its head and a node's successor); the
//!   only observer that can tell the difference is
//!   [`ThreadQueues::run_order`], whose result still names the dead thread.
//! - The exe returns 1 from two places (deadline, and reloading an empty
//!   list with no thread run since the previous load) and 0 from one (a null
//!   cursor at the per-thread check). [`SchedExit`] keeps the two causes of
//!   1 apart in its documentation, not in its value.
//! - The tick's callback block is behind the [`Callbacks`] seam and the
//!   drain's per-record call behind [`Finalizer`], so the port never has to
//!   model the frame table, the argument buffer or a finalizer record
//!   before those subsystems exist. The callback seam reports whether the
//!   exe's consuming path ran, because that choice depends on the argument
//!   buffer the seam itself owns.
//! - The exe always splices when the drain runs dry early: its `test eax,eax`
//!   at `0x418cc7` guards a `+0x30` list pointer that only a failed
//!   allocation leaves null (`0x40fe5f`), which the port's [`FinalizerQueues`]
//!   has no equivalent for.
//! - `[driver+0x3c]` cannot reach 0 through the port's API: it starts at 4
//!   (`0x40fdbe`) and every update clamps it up to at least 4, so the exe's
//!   zero-quota drain (`0x418c94` jumping to the splice) cannot happen and
//!   the `idiv` at `0x418c65` can never divide by zero (where the exe would
//!   fault with `#DE`). [`Scheduler::fairness`] still spells that case out,
//!   because it is what the machine code does.

use std::collections::VecDeque;

use slr_x87::{Precision, Rounding, X87};

use crate::clock::{read as read_clock, ClockSite};
use crate::interp::{flags, SCRIPT_PRECISION};
use crate::threads::{ThreadId, ThreadQueues, SLEEPING};

/// The x87 rounding the driver works under; the exe's control word rounds
/// to nearest from process start and never changes (ADR 0007).
const RC: Rounding = Rounding::NearestEven;

/// `0x41200000` / `0x5e7334`: the nominal slice length in clock units.
const SLICE_UNIT: f32 = 10.0;

/// `driver+0x3c`: the finalizer divisor's ctor value (`0x40fdbe`) and the
/// floor and ceiling of its update (`0x418c6d-0x418c88`).
const DIVISOR_INIT: i32 = 4;
/// See [`DIVISOR_INIT`].
const DIVISOR_MIN: i32 = 4;
/// See [`DIVISOR_INIT`].
const DIVISOR_MAX: i32 = 0x40;
/// `0x418c6a`: the right shift between the drain formula and its clamp.
const DIVISOR_SHIFT: u32 = 6;

/// `driver+0x38`: the callback period's ctor value (`0x40fd78`).
const CALLBACK_PERIOD_INIT: i32 = 1000;

/// `0x5f0910`: clock units to milliseconds (f32).
const MS_PER_UNIT: f32 = 1000.0;
/// `0x5f0908`: the new sample's weight (f64).
const MS_WEIGHT_NEW: f64 = 0.8;
/// `0x5f0900`: the previous accumulator's weight (f64).
const MS_WEIGHT_OLD: f64 = 0.2;

/// `0x5f08f8`: the priority weight (`0.10000000149011612`).
const WEIGHT: f32 = 0.1;
/// `0x5f08f4`: the positive-priority scale.
const POSITIVE_SCALE: f32 = 99.0;
/// `0x5f08f0`: the positive-priority base.
const POSITIVE_BASE: f32 = 1.0;
/// `0x5f08ec`: the negative-priority scale.
const NEGATIVE_SCALE: f32 = 0.9;
/// `0x5f08e8`: the negative-priority base (`0.10000000149011612`).
const NEGATIVE_BASE: f32 = 0.1;

/// `thread+0x2c` bit 1.
///
/// The driver reads it at `0x416a98` only, where it keeps a stopped thread
/// (`0x40`) from being turned into a dead one; no other site in the exe
/// tests it. The name says what that one site does with it.
const STOPPED_IS_FINAL: u32 = 0x01;

/// Which of `FUN_00416940`'s exits ended the call (its `eax`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedExit {
    /// The exe's `return 1`. Two ways there: the deadline passed at the loop
    /// top (only with a non-zero budget), or the cursor was reloaded from a
    /// list with nothing left to schedule and no thread had run since the
    /// previous load.
    TimeUp,
    /// The exe's `return 0`: the per-thread check was reached with a null
    /// cursor, i.e. the list was loaded empty while a thread had run since
    /// the previous load.
    ListEnd,
}

/// The scheduler's `Thread::run` seam.
///
/// The driver hands a fake (or the real `Thread::run` caller) the arena, the
/// thread it picked, that thread's slice in clock units, and the clock the
/// driver itself reads, so one trace feeds both. The exe discards the float
/// `FUN_00420ff0` returns, so the seam returns nothing.
pub type Runner<'a> = dyn FnMut(&mut ThreadQueues, ThreadId, f32, &mut dyn FnMut() -> X87) + 'a;

/// One finalizer record's port identity.
///
/// The exe's records are the `+0x44`/`+0x30`/`+0x2c` list nodes: `+0xc` an
/// execution context, `+0x18` a state word, `+0x10` an argument container.
/// `FUN_00418c20` only moves them between lists and hands them to
/// `FUN_00408560`, so the port keeps the record's identity as an opaque
/// handle and leaves its payload to the finalizer work that will create
/// them.
pub type FinalizerId = u32;

/// The three lists `FUN_00418c20` drains and moves records on.
///
/// The exe keeps them as 0x1c-byte sentinel lists in the same JVM object as
/// the thread queues; the port passes them to [`Scheduler::tick`] beside
/// [`ThreadQueues`] the way the exe passes one object. No ported function
/// creates a record yet — the exe's producers are `FUN_00408500` (unlink,
/// set the record's bit 3, append to `+0x44`) and the GC's own queueing —
/// so the pushes exist for the caller that will.
#[derive(Clone, Debug, Default)]
pub struct FinalizerQueues {
    /// `[jvm+0x44]`: the queue the drain pops from, one `FUN_00408560`
    /// (`finalize`) call per record, front first.
    queued: VecDeque<FinalizerId>,
    /// `[jvm+0x30]`: counted for the divisor update, and spliced onto the
    /// `held` tail when the drain runs dry early.
    spare: VecDeque<FinalizerId>,
    /// `[jvm+0x2c]`: the splice destination.
    held: VecDeque<FinalizerId>,
}

impl FinalizerQueues {
    /// Three empty queues.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `[jvm+0x44]` in queue order (the order the drain pops).
    #[must_use]
    pub fn queued(&self) -> Vec<FinalizerId> {
        self.queued.iter().copied().collect()
    }

    /// `[jvm+0x30]` in queue order.
    #[must_use]
    pub fn spare(&self) -> Vec<FinalizerId> {
        self.spare.iter().copied().collect()
    }

    /// `[jvm+0x2c]` in queue order.
    #[must_use]
    pub fn held(&self) -> Vec<FinalizerId> {
        self.held.iter().copied().collect()
    }

    /// Remove revived entries before the next quota-consuming pop.
    pub fn retain_queued(&mut self, keep: impl FnMut(&FinalizerId) -> bool) {
        self.queued.retain(keep);
    }

    /// exe: FUN_00418290
    ///
    /// The append at `0x418553` that `FUN_00408500` makes.
    ///
    /// Append to the `+0x44` queue's tail. The exe's caller has already
    /// unlinked the record and set its bit 3; a port record has neither.
    pub fn push_queued(&mut self, id: FinalizerId) {
        self.queued.push_back(id);
    }

    /// exe: FUN_00408720
    ///
    /// The `+0x30` append (the GC's queueing, no ported caller yet).
    pub fn push_spare(&mut self, id: FinalizerId) {
        self.spare.push_back(id);
    }

    /// exe: FUN_00408780
    ///
    /// The `+0x2c` append (the GC's queueing, no ported caller yet).
    pub fn push_held(&mut self, id: FinalizerId) {
        self.held.push_back(id);
    }
}

/// A drain cursor permits real finalizers to mutate queues between pops.
/// The quota counter and early-empty splice rule match `FUN_00418c20`.
pub struct FinalizerDrain {
    remaining: i32,
    probe_empty: bool,
    done: bool,
}

impl FinalizerDrain {
    /// Pop the next record; callers may remove revived queued strings first.
    pub fn next(&mut self, finals: &mut FinalizerQueues) -> Option<FinalizerId> {
        if self.done {
            return None;
        }
        let Some(id) = finals.queued.pop_front() else {
            self.probe_empty = true;
            self.done = true;
            return None;
        };
        self.probe_empty = false;
        let left = self.remaining;
        self.remaining = self.remaining.wrapping_sub(1);
        self.done = left == 0;
        Some(id)
    }

    /// Finish the original early-empty spare-to-held splice.
    pub fn finish(self, finals: &mut FinalizerQueues) {
        if self.remaining != 0 && self.probe_empty {
            finals.held.append(&mut finals.spare);
        }
    }
}

/// What `FUN_00418d10`'s callback block did, for the accumulator update.
///
/// The block runs while `[+0x34] > [+0x38]`; whether it then consumes the
/// period depends on the argument buffer `[+0x20]`, which the block owns:
/// `FUN_00418970` frees it and nulls the field when its container is empty
/// (`0x418b26`/`0x418b5b`), and the tick subtracts only when the field
/// stayed non-null (`0x418e17`, `0x418e57`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallbackRun {
    /// `[+0x20]` was null throughout (the exe's first-time path at
    /// `0x418d4a`, or a repeat that freed the buffer): the block walked the
    /// object table and the `+0x10` list, and the tick consumes one period
    /// from `[+0x34]`.
    Consumed,
    /// `[+0x20]` was non-null after the block (`0x418e1a`): the exe jumps
    /// straight to the frame-time accumulator and `[+0x34]` keeps its
    /// value.
    Retained,
}

/// The tick's callback seam (`FUN_00418d10`'s `[+0x34] > [+0x38]` block).
///
/// The exe walks the frame table `+0x08` (0x200 slots), rebuilds the
/// argument buffer `+0x20` and walks the `+0x10` record list, calling
/// `FUN_00408500` per entry — the VM's callback/timer dispatch (M1-05 S7),
/// which is not scheduler state. The seam reports what the block did to the
/// accumulator, because only the block can see its argument buffer.
pub type Callbacks<'a> = dyn FnMut() -> CallbackRun + 'a;

/// The drain's per-record seam: the exe's `FUN_00408560`.
///
/// The record is already off the queue when the seam is called (the exe
/// unlinks it inside `FUN_00408560`, `0x4086c4`), so the seam only runs the
/// finalizer the exe expresses as a `THRD-FIN` thread calling `finalize`
/// (`0x607a38`); it returns nothing, like `FUN_00408560`'s callers ignore
/// its `eax`.
pub type Finalizer<'a> = dyn FnMut(FinalizerId) + 'a;

/// The driver object's state: the cursor `[driver+0x78]`, the threads the
/// driver's destructor calls removed, and the tick's own counters.
#[derive(Clone, Debug)]
pub struct Scheduler {
    cursor: Option<ThreadId>,
    reaped: Vec<ThreadId>,
    /// `[driver+0x40]`: the tick counter. Only its low byte is read (by
    /// [`Self::fairness`]), and [`Self::tick`] increments it.
    pub ticks: u32,
    /// `[driver+0x3c]`: the finalizer drain quota, `DIVISOR_INIT` (4) in the
    /// exe's ctor and clamped to `4 ..= 0x40` by every update.
    pub divisor: i32,
    /// `[driver+0x24]`: the smoothed frame time in milliseconds.
    pub elapsed_ms: i32,
    /// `[driver+0x34]`: the callback block's accumulator, compared against
    /// `callback_period` and decremented by it.
    pub callback_ticks: i32,
    /// `[driver+0x38]`: the callback block's period, `CALLBACK_PERIOD_INIT`
    /// (1000) in the exe's ctor.
    pub callback_period: i32,
}

impl Default for Scheduler {
    /// The exe's ctor values: divisor 4, period 1000, everything else zero
    /// (`FUN_0040fb50` at `0x40fdbe`/`0x40fd78`).
    fn default() -> Self {
        Self {
            cursor: None,
            reaped: Vec::new(),
            ticks: 0,
            divisor: DIVISOR_INIT,
            elapsed_ms: 0,
            callback_ticks: 0,
            callback_period: CALLBACK_PERIOD_INIT,
        }
    }
}

impl Scheduler {
    /// A scheduler whose next walk starts at the head of the run queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cursor `[driver+0x78]`: the thread the next [`Self::drive`] call
    /// resumes at, `None` when it starts at the head again.
    #[must_use]
    pub fn cursor(&self) -> Option<ThreadId> {
        self.cursor
    }

    /// The threads the driver's walks reaped, in the order the exe's
    /// deleting destructor `0x423b60` would have freed them.
    ///
    /// The port never removes a thread from its arena (see the module
    /// notes), so this order — and the skip it implies in every later list
    /// walk — is all that is left of that destructor. The oracle replay
    /// (M1-05 T6) compares it against the destructor's own log.
    #[must_use]
    pub fn reaped(&self) -> &[ThreadId] {
        &self.reaped
    }

    /// exe: FUN_00416940
    ///
    /// Run the threads of `queues`' run queue for one budget, highest
    /// priority first only in the sense that threads under `min_priority`
    /// are skipped: the walk is the run queue's own order.
    ///
    /// `budget` is the time the caller grants, `0.0` meaning "no deadline"
    /// (the exe's `FUN_00418d10` passes `10.0`). `min_priority` is the
    /// scheduler's floor. `clock` is `FUN_005516c0` (the frame-delta
    /// accumulated time) and `run` is `Thread::run`; the driver reads the
    /// clock itself at entry, at the top of every pass and for every
    /// sleeping thread, so a shared trace has to be one sequence.
    pub fn drive(
        &mut self,
        queues: &mut ThreadQueues,
        budget: f32,
        min_priority: i32,
        clock: &mut dyn FnMut() -> X87,
        run: &mut Runner<'_>,
    ) -> SchedExit {
        self.drive_with_reaper(queues, budget, min_priority, clock, run, &mut |_, _| {})
    }

    /// Run the original driver with an embedding destructor at the exact
    /// `0x416aa4` reaping point. The cursor has advanced before the destructor;
    /// the callback may unlink the old arena entry without changing traversal.
    #[allow(clippy::too_many_arguments)]
    pub fn drive_with_reaper(
        &mut self,
        queues: &mut ThreadQueues,
        budget: f32,
        min_priority: i32,
        clock: &mut dyn FnMut() -> X87,
        run: &mut Runner<'_>,
        reap: &mut dyn FnMut(&mut ThreadQueues, ThreadId),
    ) -> SchedExit {
        let pc = SCRIPT_PRECISION;
        // 0x416943-0x416974: the entry flag is `budget == 0.0f`, and the
        // deadline is the f32 `clock() + budget`.
        let timed = budget != 0.0;
        let mut deadline = crate::clock::with_driver_cursor(self.cursor, || {
            read_clock(clock, ClockSite::DriveEnter)
        })
        .add(X87::from_f32(budget), pc, RC)
        .to_f32(pc, RC);
        // 0x416978-0x416997: 10.0, shortened to a smaller budget (never
        // touched when the budget is zero).
        let mut slice_unit = SLICE_UNIT;
        if timed && budget < SLICE_UNIT {
            slice_unit = budget;
        }
        // `bl`: a thread has run since the cursor was last loaded.
        let mut ran = true;
        loop {
            // 0x41699f: the deadline, for a non-zero budget. `fcomp` leaves
            // C0 (CF) set for `clock < deadline`, so a clear C0 is the exe's
            // `je` back to the caller.
            if timed {
                let (cc, _) = crate::clock::with_driver_cursor(self.cursor, || {
                    read_clock(clock, ClockSite::DriveLoop)
                })
                .fcom(X87::from_f32(deadline));
                if !cc.c0 {
                    return SchedExit::TimeUp;
                }
            }
            // 0x4169bb: reload the cursor from the head of the run queue.
            if self.cursor.is_none() {
                self.cursor = self.head(queues);
                if !ran {
                    return SchedExit::TimeUp;
                }
                ran = false;
            }
            // 0x4169de: nothing left to schedule.
            let Some(t) = self.cursor else {
                return SchedExit::ListEnd;
            };
            let f = queues.get(t).flags;
            if f & flags::SCHED_BLOCK == 0 {
                // 0x4169f4/0x4169fc: stopped, running and under-priority
                // threads fall through to the stopped-to-dead step.
                if f & (flags::STOPPED | flags::RUNNING) == 0
                    && queues.get(t).priority >= min_priority
                {
                    // 0x416a05-0x416a16: wake a sleeping thread whose
                    // deadline is older than the clock (`test ah,0x41`).
                    if f & SLEEPING != 0 {
                        let (cc, _) = crate::clock::with_driver_cursor(self.cursor, || {
                            read_clock(clock, ClockSite::DriveSleepProbe)
                        })
                        .fcom(X87::from_f32(queues.get(t).sleep_deadline));
                        if !cc.c0 && !cc.c3 {
                            queues.get_mut(t).flags &= !SLEEPING;
                        }
                    }
                    // 0x416a1f: the flag is re-read after the clock call.
                    if queues.get(t).flags & SLEEPING == 0 {
                        let slice = slice_for(queues.get(t).priority, slice_unit, pc);
                        deadline = X87::from_f32(deadline)
                            .sub(X87::from_f32(slice_unit), pc, RC)
                            .add(X87::from_f32(slice), pc, RC)
                            .to_f32(pc, RC);
                        crate::clock::with_driver_cursor(Some(t), || run(queues, t, slice, clock));
                        ran = true;
                    }
                }
                // 0x416a8e: a stopped thread that is not protected dies.
                let f = queues.get(t).flags;
                if f & flags::STOPPED != 0 && f & STOPPED_IS_FINAL == 0 {
                    queues.get_mut(t).flags = f | flags::DEAD;
                }
            }
            // 0x416aa4: step the cursor, then reap the thread the exe's
            // deleting destructor would have freed.
            let f = queues.get(t).flags;
            self.cursor = self.successor(queues, t);
            if f & flags::RUNNING == 0 && f & flags::DEAD != 0 {
                self.reaped.push(t);
                reap(queues, t);
            }
        }
    }

    /// exe: FUN_00418c20
    ///
    /// One tick's finalizer policy: recompute the drain quota
    /// ([`Self::divisor`]) on every 256th tick, then pop up to that many
    /// records off the front of the `queued` queue, running each through
    /// `finalize`, and splice the whole `spare` queue onto the `held` tail
    /// when the drain ran dry before the last-but-one slot.
    ///
    /// The quota update needs `queued` to be non-empty; a run whose quota is
    /// reached exactly, or misses it by one, does not splice.
    pub fn fairness(&mut self, finals: &mut FinalizerQueues, finalize: &mut Finalizer<'_>) {
        let mut drain = self.begin_fairness(finals);
        while let Some(id) = drain.next(finals) {
            finalize(id);
        }
        drain.finish(finals);
    }

    /// Begin the same quota calculation as `fairness`, allowing the embedding
    /// to execute finalizers and update queues between individual pops.
    pub fn begin_fairness(&mut self, finals: &FinalizerQueues) -> FinalizerDrain {
        // 0x418c23: `mov al, byte ptr [esi+0x40]` — only the tick counter's
        // low byte gates the update, and `FUN_00418d10` has already
        // incremented it.
        if self.ticks as u8 == 0 && !finals.queued.is_empty() {
            // 0x418c2e-0x418c65: `count(+0x30) / [+0x3c]`, signed (`idiv`).
            // The divisor is at least 4 from the ctor and from every clamp,
            // so the exe's `#DE` on a zero divisor is unreachable here.
            let incoming = i32::try_from(finals.spare.len()).unwrap_or(i32::MAX);
            let due = i32::try_from(finals.queued.len()).unwrap_or(i32::MAX);
            let quotient = incoming / self.divisor;
            // 0x418c68-0x418c6a: `add eax, edi; sar eax, 6`, wrapping like
            // the exe's 32-bit add.
            let quota = quotient.wrapping_add(due) >> DIVISOR_SHIFT;
            // 0x418c6d-0x418c88: the exe stores the raw value, clamps it up
            // to 4, then re-reads and clamps it down to 0x40; with nothing
            // observing the field in between that is one clamp.
            self.divisor = quota.clamp(DIVISOR_MIN, DIVISOR_MAX);
        }
        FinalizerDrain {
            remaining: self.divisor.wrapping_sub(1),
            probe_empty: true,
            done: self.divisor == 0,
        }
    }

    /// exe: FUN_00418d10
    ///
    /// One frame's tick: bump the tick counter, run the finalizer drain, read
    /// the clock around the callback block, fold the frame time into
    /// [`Self::elapsed_ms`], then drive the scheduler with this tick's own
    /// budget and a zero priority floor.
    ///
    /// `finals` and `queues` are the driver object's lists, `budget` is the
    /// tick's float argument (the frame loop passes `10.0`), `clock` is
    /// `FUN_005516c0` (shared with `drive` in one sequence), `finalize` is
    /// the drain's `FUN_00408560` and `callbacks` the callback block.
    /// Returns what the drive returned, which is also what the exe leaves in
    /// `eax`.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        queues: &mut ThreadQueues,
        finals: &mut FinalizerQueues,
        budget: f32,
        clock: &mut dyn FnMut() -> X87,
        finalize: &mut Finalizer<'_>,
        callbacks: &mut Callbacks<'_>,
        run: &mut Runner<'_>,
    ) -> SchedExit {
        self.begin_tick();
        self.fairness(finals, finalize);
        self.callbacks_phase(clock, callbacks);
        self.drive(queues, budget, 0, clock, run)
    }

    /// Tick counter update, before the finalizer drain and all clock reads.
    pub fn begin_tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// Original clocked callback phase, between the drain and scheduler drive.
    pub fn callbacks_phase(
        &mut self,
        clock: &mut dyn FnMut() -> X87,
        callbacks: &mut Callbacks<'_>,
    ) {
        let pc = SCRIPT_PRECISION;
        // 0x418d28-0x418d2d: `t0` is stored as f32.
        let t0 = read_clock(clock, ClockSite::TickEnter).to_f32(pc, RC);
        // 0x418d31-0x418e1a: the callback block, while `+0x34 > +0x38`.
        if self.callback_ticks > self.callback_period && callbacks() == CallbackRun::Consumed {
            // 0x418e57-0x418e5f: `[+0x34] -= [+0x38]`.
            self.callback_ticks = self.callback_ticks.wrapping_sub(self.callback_period);
        }
        // 0x418e62-0x418e90: the 80/20 smoothing of the frame time.
        let elapsed =
            read_clock(clock, ClockSite::TickAfterCallbacks).sub(X87::from_f32(t0), pc, RC);
        self.elapsed_ms = frame_ms(self.elapsed_ms, elapsed, pc);
    }

    /// exe: FUN_00416940
    ///
    /// The `[[driver+0x18]+8]` load at `0x4169c2-0x4169cf`.
    ///
    /// The run queue's first thread, with the threads already reaped skipped
    /// the way the exe's unlinked list has them skipped.
    fn head(&self, queues: &ThreadQueues) -> Option<ThreadId> {
        queues
            .run_queue()
            .iter(queues.threads())
            .find(|id| !self.reaped.contains(id))
    }

    /// exe: FUN_00416940
    ///
    /// The `[cursor+4]` / `[[cursor+4]+4]` pair at `0x416aa7-0x416ab3`.
    ///
    /// The next thread in the run queue, `None` at the end of the list
    /// (where the exe's `+4` chain runs into the tail sentinel's null).
    fn successor(&self, queues: &ThreadQueues, id: ThreadId) -> Option<ThreadId> {
        let mut next = queues.get(id).next();
        while let Some(n) = next {
            if !self.reaped.contains(&n) {
                return Some(n);
            }
            next = queues.get(n).next();
        }
        None
    }
}

/// exe: FUN_00418d10
///
/// The frame-time accumulator at `0x418e62-0x418e90`.
///
/// `(int)(elapsed * 1000.0f * 0.8 + (double)previous * 0.2)`, truncated
/// toward zero by `__ftol` (`0x5d6750` sets the rounding control to
/// truncate, `fistp` a 64-bit value) with only the low 32 bits stored
/// (`0x418e90`). The f64 constants are loaded exactly and every arithmetic
/// step rounds at `pc`, like the exe's `fmul`/`faddp` chain.
fn frame_ms(previous: i32, elapsed: X87, pc: Precision) -> i32 {
    let sample =
        elapsed
            .mul(X87::from_f32(MS_PER_UNIT), pc, RC)
            .mul(X87::from_f64(MS_WEIGHT_NEW), pc, RC);
    let carried = X87::from_i32(previous).mul(X87::from_f64(MS_WEIGHT_OLD), pc, RC);
    sample.add(carried, pc, RC).to_i64(Rounding::Zero) as i32
}

/// exe: FUN_00416940
///
/// The slice computation at `0x416a28-0x416a6c`.
///
/// `time_slice(priority) * slice_unit`, rounded at the driver's precision
/// after every x87 operation like the exe's `fmul`/`fadd` chain.
fn slice_for(priority: i32, slice_unit: f32, pc: Precision) -> f32 {
    let weighted = if priority >= 0 {
        X87::from_i32(priority)
            .mul(X87::from_f32(WEIGHT), pc, RC)
            .mul(X87::from_f32(POSITIVE_SCALE), pc, RC)
            .add(X87::from_f32(POSITIVE_BASE), pc, RC)
    } else {
        // 0x416a4b: the `+ 10` is 32-bit integer arithmetic first.
        X87::from_i32(priority.wrapping_add(10))
            .mul(X87::from_f32(WEIGHT), pc, RC)
            .mul(X87::from_f32(NEGATIVE_SCALE), pc, RC)
            .add(X87::from_f32(NEGATIVE_BASE), pc, RC)
    };
    weighted
        .mul(X87::from_f32(slice_unit), pc, RC)
        .to_f32(pc, RC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// A clock that reads `1.0`, `2.0`, ... (`FUN_005516c0`), handing back
    /// the read count so a test can pin the driver's clock trace.
    fn ticking() -> (impl FnMut() -> X87, Rc<Cell<u32>>) {
        let reads = Rc::new(Cell::new(0u32));
        let counter = Rc::clone(&reads);
        let clock = move || {
            counter.set(counter.get() + 1);
            X87::from_f32(counter.get() as f32)
        };
        (clock, reads)
    }

    /// `Thread::run` that records `(thread, slice)` and ORs `after` into the
    /// thread's flags — `flags::STOPPED` for a script that returned, `0` for
    /// one that used up its slice.
    fn recording(
        log: &RefCell<Vec<(ThreadId, f32)>>,
        after: u32,
    ) -> impl FnMut(&mut ThreadQueues, ThreadId, f32, &mut dyn FnMut() -> X87) + '_ {
        move |queues, id, slice, _clock| {
            log.borrow_mut().push((id, slice));
            queues.get_mut(id).flags |= after;
        }
    }

    #[test]
    fn interleaved_drain_revival_preserves_quota_and_spare_order() {
        let mut scheduler = Scheduler::new();
        let mut finals = FinalizerQueues::new();
        for word in 1..=5 {
            finals.push_queued(word);
        }
        finals.push_spare(99);
        let mut drain = scheduler.begin_fairness(&finals);
        assert_eq!(drain.next(&mut finals), Some(1));
        // The first finalizer revives the next string before another pop.
        finals.retain_queued(|word| *word != 2);
        finals.push_spare(1);
        for word in [3, 4, 5] {
            assert_eq!(drain.next(&mut finals), Some(word));
        }
        assert_eq!(drain.next(&mut finals), None);
        drain.finish(&mut finals);
        assert_eq!(finals.spare(), vec![99, 1]);
        assert!(finals.held().is_empty());
        let mut drain = scheduler.begin_fairness(&finals);
        assert_eq!(drain.next(&mut finals), None);
        drain.finish(&mut finals);
        assert!(finals.spare().is_empty());
        assert_eq!(finals.held(), vec![99, 1]);
    }

    #[test]
    fn empty_list_without_a_deadline_is_exhausted() {
        let mut queues = ThreadQueues::new();
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        assert_eq!(sched.cursor(), None);
        assert!(log.borrow().is_empty());
        assert!(sched.reaped.is_empty());
        // The clock is read once for `t0`; a zero budget never reads it at
        // the top of the loop.
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn empty_list_with_a_deadline_is_exhausted_while_time_remains() {
        let mut queues = ThreadQueues::new();
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        // t0 = 1.0, deadline = 11.0, next read 2.0 < 11.0.
        assert_eq!(
            sched.drive(&mut queues, 10.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        assert_eq!(sched.cursor(), None);
        assert!(log.borrow().is_empty());
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn deadline_reached_at_the_top_returns_time_up() {
        let mut queues = ThreadQueues::new();
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        // t0 = 1.0, deadline = 1.5, next read 2.0 >= 1.5: no thread is
        // looked at, not even the head load.
        assert_eq!(
            sched.drive(&mut queues, 0.5, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert_eq!(sched.cursor(), None);
        assert!(log.borrow().is_empty());
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn destructor_runs_between_threads_and_can_unlink_without_skipping_successor() {
        let mut queues = ThreadQueues::new();
        let first = queues.spawn(10, 0, "first");
        let second = queues.spawn(10, 0, "second");
        let mut sched = Scheduler::new();
        let log = RefCell::new(Vec::new());
        let mut run = |queues: &mut ThreadQueues, id, _, _: &mut dyn FnMut() -> X87| {
            log.borrow_mut().push(("run", id));
            if id == second {
                assert!(!queues.run_order().contains(&first));
            }
            queues.get_mut(id).flags = flags::STOPPED;
        };
        let mut reap = |queues: &mut ThreadQueues, id| {
            log.borrow_mut().push(("destroy", id));
            assert_ne!(queues.get(id).flags & flags::DEAD, 0);
            queues.remove(id);
        };
        assert_eq!(
            sched.drive_with_reaper(
                &mut queues,
                0.0,
                0,
                &mut || X87::from_i64(0),
                &mut run,
                &mut reap
            ),
            SchedExit::ListEnd
        );
        assert_eq!(
            *log.borrow(),
            vec![
                ("run", first),
                ("destroy", first),
                ("run", second),
                ("destroy", second)
            ]
        );
        assert!(queues.run_order().is_empty());
        assert_eq!(sched.reaped(), &[first, second]);
    }

    #[test]
    fn a_returning_thread_runs_once_and_is_reaped() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        // Priority 0: `1.0f * 10.0f` (`0x41200000`).
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0].0, id);
        assert_eq!(log.borrow()[0].1, 10.0);
        assert_eq!(sched.reaped, vec![id]);
        assert_eq!(sched.cursor(), None);
        assert_eq!(queues.get(id).flags, flags::STOPPED | flags::DEAD);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_thread_that_keeps_running_is_rescheduled_until_the_deadline() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        // t0 = 1.0, deadline = 6.0, slice_unit = 5.0 (the budget). Priority
        // 0 gives a slice of exactly `slice_unit`, so the deadline stays at
        // 6.0 and the pass ends at the fifth top-of-loop read.
        assert_eq!(
            sched.drive(&mut queues, 5.0, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert_eq!(log.borrow().len(), 4);
        assert!(log
            .borrow()
            .iter()
            .all(|(t, s)| *t == id && s.to_bits() == 0x40a0_0000));
        assert!(sched.reaped.is_empty());
        assert_eq!(sched.cursor(), None);
        assert_eq!(queues.get(id).flags, 0);
        assert_eq!(reads.get(), 6);
    }

    #[test]
    fn the_slice_chain_rounds_at_the_drivers_precision() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(1, 0, "a");
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        // Priority 1 at a 24-bit precision: `((1 * 0.1f) * 99.0f) + 1.0f`
        // rounds to `10.900001` before the `* 10.0f`, and the double-rounding
        // of a plain f64 evaluation would give `109.0` (`0x42da0000`).
        assert_eq!(log.borrow()[0].1.to_bits(), 0x42da_0001);
        assert_eq!(sched.reaped, vec![id]);
    }

    #[test]
    fn queue_order_is_the_walk_order_and_min_priority_only_skips() {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        let b = queues.spawn(5, 0, "b");
        let c = queues.spawn(-3, 0, "c");
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        assert_eq!(
            sched.drive(&mut queues, 0.0, -3, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        assert_eq!(
            log.borrow().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            vec![a, b, c]
        );
        // Slice bits per priority at `slice_unit = 10.0`, computed with the
        // exe's own constants (see the module doc) at 24-bit precision:
        // 0 -> 1.0f * 10.0f, 5 -> 50.5f * 10.0f, -3 -> 0.73f * 10.0f.
        assert_eq!(log.borrow()[0].1.to_bits(), 0x4120_0000);
        assert_eq!(log.borrow()[1].1.to_bits(), 0x43fc_8000);
        assert_eq!(log.borrow()[2].1.to_bits(), 0x40e9_999a);
        assert_eq!(sched.reaped, vec![a, b, c]);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn min_priority_skips_and_a_wrap_without_a_run_returns_time_up() {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        let b = queues.spawn(5, 0, "b");
        let c = queues.spawn(-3, 0, "c");
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        // `a` (0) and `c` (-3) are under the floor of 1, so only `b` runs;
        // the walk then wraps to `a`, which is skipped again, and the
        // reload that follows a run-less pass returns 1.
        assert_eq!(
            sched.drive(&mut queues, 0.0, 1, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0].0, b);
        assert_eq!(sched.reaped, vec![b]);
        assert_eq!(sched.cursor(), Some(a));
        // `b` was reaped out of the middle of the list, so `a`'s successor
        // is `c`.
        assert_eq!(queues.get(b).flags, flags::STOPPED | flags::DEAD);
        assert_eq!(queues.get(a).flags, 0);
        assert_eq!(queues.get(c).flags, 0);
        // What the queue itself still holds is the port's one divergence.
        assert_eq!(queues.run_order(), vec![a, b, c]);
    }

    #[test]
    fn a_sleeping_thread_wakes_when_the_clock_passes_its_deadline() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        queues.get_mut(id).sleep_deadline = 0.5;
        queues.get_mut(id).flags |= SLEEPING;
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        // The sleep check reads 2.0 > 0.5, so the bit clears and the thread
        // runs in the same pass.
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0].1.to_bits(), 0x4120_0000);
        assert_eq!(sched.reaped, vec![id]);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn a_sleeping_thread_that_is_not_due_yet_stays_asleep() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        queues.get_mut(id).sleep_deadline = 100.0;
        queues.get_mut(id).flags |= SLEEPING;
        let mut sched = Scheduler::new();
        let (mut clock, reads) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        // 2.0 < 100.0: the thread is skipped, the walk wraps and returns 1
        // with no thread run, leaving the cursor on the thread.
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert!(log.borrow().is_empty());
        assert!(sched.reaped.is_empty());
        assert_eq!(sched.cursor(), Some(id));
        assert_eq!(queues.get(id).flags, SLEEPING);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn a_stopped_thread_is_only_skipped_when_the_guard_bit_is_set() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        queues.get_mut(id).flags = flags::STOPPED | STOPPED_IS_FINAL;
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert!(log.borrow().is_empty());
        // Not made dead, so not reaped either.
        assert_eq!(queues.get(id).flags, flags::STOPPED | STOPPED_IS_FINAL);
        assert!(sched.reaped.is_empty());
        assert_eq!(sched.cursor(), Some(id));
    }

    #[test]
    fn a_dead_thread_that_is_still_running_is_not_reaped() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        // `0x82` is a scheduler block, and `0x04` keeps the advance from
        // reaping it.
        queues.get_mut(id).flags = flags::RUNNING | flags::DEAD;
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert!(log.borrow().is_empty());
        assert!(sched.reaped.is_empty());
        assert_eq!(queues.get(id).flags, flags::RUNNING | flags::DEAD);
        assert_eq!(sched.cursor(), Some(id));
    }

    #[test]
    fn a_created_thread_is_skipped_and_a_dead_one_is_reaped() {
        let mut queues = ThreadQueues::new();
        let created = queues.spawn(0, 0, "new");
        let dead = queues.spawn(0, 0, "dead");
        queues.get_mut(created).flags = 0x02;
        queues.get_mut(dead).flags = flags::DEAD;
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::TimeUp
        );
        assert!(log.borrow().is_empty());
        // The created thread keeps its bit; the dead one is reaped, so the
        // walk after the reaped head sees nothing.
        assert_eq!(queues.get(created).flags, 0x02);
        assert_eq!(sched.reaped, vec![dead]);
        assert_eq!(sched.cursor(), Some(created));
    }

    #[test]
    fn a_suspended_thread_in_the_run_queue_still_runs() {
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        queues.get_mut(id).flags = flags::NOTIFIED;
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, flags::STOPPED);
        // `0x20` is not one of the driver's two skip masks.
        assert_eq!(
            sched.drive(&mut queues, 0.0, 0, &mut clock, &mut run),
            SchedExit::ListEnd
        );
        assert_eq!(log.borrow().len(), 1);
        assert_eq!(log.borrow()[0].0, id);
        assert_eq!(
            queues.get(id).flags,
            flags::NOTIFIED | flags::STOPPED | flags::DEAD
        );
    }

    #[test]
    fn the_cursor_carries_over_between_calls() {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        let b = queues.spawn(1, 0, "b");
        let mut sched = Scheduler::new();
        let (mut clock, _) = ticking();
        let first = RefCell::new(Vec::new());
        {
            // t0 = 1.0, deadline = 2.5, slice_unit = 1.5: `a` runs (its slice
            // of 1.5 leaves the deadline at 2.5), then the top of the next
            // pass reads 3.0 and returns, leaving the cursor on `b`.
            let mut run = recording(&first, 0);
            let exit = sched.drive(&mut queues, 1.5, 0, &mut clock, &mut run);
            assert_eq!(exit, SchedExit::TimeUp);
        }
        assert_eq!(
            first.borrow().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            vec![a]
        );
        assert_eq!(sched.cursor(), Some(b));
        let second = RefCell::new(Vec::new());
        {
            // `a` is under this call's floor, so the resumed walk stops on
            // it after reaping `b`.
            let mut run = recording(&second, flags::STOPPED);
            let exit = sched.drive(&mut queues, 0.0, 1, &mut clock, &mut run);
            assert_eq!(exit, SchedExit::TimeUp);
        }
        // The walk resumed at `b`, ran it, reaped it, then wrapped to `a`
        // and stopped there without running it.
        assert_eq!(
            second.borrow().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            vec![b]
        );
        assert_eq!(sched.reaped, vec![b]);
        assert_eq!(sched.cursor(), Some(a));
    }

    #[test]
    fn a_scheduler_starts_at_the_head() {
        let sched = Scheduler::new();
        assert_eq!(sched.cursor(), None);
        assert!(sched.reaped.is_empty());
    }

    /// A `finalize` recorder — the seam `FUN_00408560` provides.
    fn finalizing(log: &RefCell<Vec<FinalizerId>>) -> impl FnMut(FinalizerId) + '_ {
        move |id| log.borrow_mut().push(id)
    }

    /// A callback block that always reports `result` and counts its calls.
    fn callbacks(result: CallbackRun, calls: &Rc<Cell<u32>>) -> impl FnMut() -> CallbackRun + '_ {
        move || {
            calls.set(calls.get() + 1);
            result
        }
    }

    /// The three finalizer queues from the given contents.
    fn queues_from(
        queued: &[FinalizerId],
        spare: &[FinalizerId],
        held: &[FinalizerId],
    ) -> FinalizerQueues {
        let mut q = FinalizerQueues::new();
        for id in queued {
            q.push_queued(*id);
        }
        for id in spare {
            q.push_spare(*id);
        }
        for id in held {
            q.push_held(*id);
        }
        q
    }

    /// One tick with an empty run queue, an always-`Consumed` callback block
    /// and a fresh `finalize` recorder, returning what the block and the
    /// drain saw.
    fn quiet_tick(
        sched: &mut Scheduler,
        finals: &mut FinalizerQueues,
        clock: &mut dyn FnMut() -> X87,
    ) -> (SchedExit, Vec<FinalizerId>, u32) {
        let mut queues = ThreadQueues::new();
        let drained = RefCell::new(Vec::new());
        let calls = Rc::new(Cell::new(0u32));
        let mut finalize = finalizing(&drained);
        let mut callbacks = callbacks(CallbackRun::Consumed, &calls);
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        let exit = sched.tick(
            &mut queues,
            finals,
            0.0,
            clock,
            &mut finalize,
            &mut callbacks,
            &mut run,
        );
        drop(finalize);
        let drained = drained.into_inner();
        (exit, drained, calls.get())
    }

    #[test]
    fn a_tick_drains_the_queued_records_and_returns_the_drives_exit() {
        let mut sched = Scheduler::new();
        let mut finals = queues_from(&[10, 11, 12, 13, 14], &[20, 21], &[]);
        let (mut clock, reads) = ticking();
        let (exit, drained, calls) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(exit, SchedExit::ListEnd);
        assert_eq!(reads.get(), 3);
        assert_eq!(sched.ticks, 1);
        assert_eq!(sched.divisor, 4);
        // The divisor update is gated on the counter's low byte: 1 is not 0,
        // so the callback block never ran either.
        assert_eq!(calls, 0);
        // The quota is the ctor divisor 4, popped front first; the fifth
        // record stays and the drain was not dry, so nothing is spliced.
        assert_eq!(drained, vec![10, 11, 12, 13]);
        assert_eq!(finals.queued(), vec![14]);
        assert_eq!(finals.spare(), vec![20, 21]);
        assert_eq!(finals.held(), Vec::<FinalizerId>::new());
    }

    #[test]
    fn the_divisor_update_only_runs_when_the_low_byte_of_the_counter_is_zero() {
        // 255 + 1 = 256: the update runs.
        let mut sched = Scheduler::new();
        sched.ticks = 255;
        sched.divisor = 17;
        let mut finals = queues_from(&[1], &[1000], &[]);
        let (mut clock, _) = ticking();
        quiet_tick(&mut sched, &mut finals, &mut clock);
        // (1000 / 17 + 1) >> 6 = 0, clamped up to 4.
        assert_eq!(sched.divisor, 4);

        // 1 + 1 = 2: the low byte is not zero, so the divisor is kept.
        let mut sched = Scheduler::new();
        sched.ticks = 1;
        sched.divisor = 17;
        let mut finals = queues_from(&[1], &[1000], &[]);
        quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(sched.divisor, 17);
    }

    #[test]
    fn the_divisor_update_needs_a_queued_record() {
        let mut sched = Scheduler::new();
        sched.ticks = 255;
        sched.divisor = 17;
        let mut finals = queues_from(&[], &[1000], &[7]);
        let (mut clock, _) = ticking();
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        // `0x418c31`: an empty `+0x44` list skips the update, but the drain
        // still runs with the old divisor and splices (0 < 17 - 1).
        assert_eq!(sched.divisor, 17);
        assert!(drained.is_empty());
        assert_eq!(finals.held(), vec![7, 1000]);
        assert!(finals.spare().is_empty());
    }

    #[test]
    fn the_divisor_is_clamped_to_four_and_to_0x40() {
        let mut sched = Scheduler::new();
        sched.ticks = 255;
        let mut finals = queues_from(&[0; 4160], &[1, 2, 3, 4], &[]);
        let (mut clock, _) = ticking();
        quiet_tick(&mut sched, &mut finals, &mut clock);
        // (4 / 4 + 4160) >> 6 = 65, clamped down to 0x40.
        assert_eq!(sched.divisor, 0x40);

        let mut sched = Scheduler::new();
        sched.ticks = 255;
        let mut finals = queues_from(&[0], &[1], &[]);
        quiet_tick(&mut sched, &mut finals, &mut clock);
        // (1 / 4 + 1) >> 6 = 0, clamped up to 4.
        assert_eq!(sched.divisor, DIVISOR_MIN);
    }

    #[test]
    fn the_drain_uses_the_divisor_the_update_produced() {
        // A quota of 8 without the update: 8 records leave the queue.
        let mut sched = Scheduler::new();
        sched.divisor = 8;
        let ids: Vec<FinalizerId> = (0..100).collect();
        let mut finals = queues_from(&ids, &[], &[]);
        let (mut clock, _) = ticking();
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(drained.len(), 8);
        assert_eq!(finals.queued().len(), 92);

        // The same divisor, but this tick updates it: (0 / 8 + 100) >> 6 = 1
        // clamps to 4, and the drain pops four.
        let mut sched = Scheduler::new();
        sched.ticks = 255;
        sched.divisor = 8;
        let mut finals = queues_from(&ids, &[], &[]);
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(sched.divisor, 4);
        assert_eq!(drained.len(), 4);
        assert_eq!(finals.queued().len(), 96);
    }

    #[test]
    fn a_dry_drain_splices_the_spare_queue_only_before_the_last_slot() {
        // Exactly the quota popped (4 of 4): no splice.
        let mut sched = Scheduler::new();
        let mut finals = queues_from(&[1, 2, 3, 4], &[9], &[]);
        let (mut clock, _) = ticking();
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(drained, vec![1, 2, 3, 4]);
        assert_eq!(finals.spare(), vec![9]);

        // One short of the quota (3 of 4): still no splice.
        let mut finals = queues_from(&[1, 2, 3], &[9], &[]);
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(drained, vec![1, 2, 3]);
        assert_eq!(finals.spare(), vec![9]);

        // Two short of the quota (2 of 4): the whole spare queue moves onto
        // the held tail, in order.
        let mut finals = queues_from(&[1, 2], &[7, 8, 9], &[5, 6]);
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(drained, vec![1, 2]);
        assert!(finals.queued().is_empty());
        assert!(finals.spare().is_empty());
        assert_eq!(finals.held(), vec![5, 6, 7, 8, 9]);
    }

    #[test]
    fn a_zero_quota_pops_nothing_and_still_splices() {
        // `[driver+0x3c]` cannot reach 0 through the port's API (the ctor
        // writes 4 and every update clamps up), but the machine code's
        // `0x418c94` path is spelled out, so pin it: no pop runs and the
        // splice still does ([oracle]: `div = 0` moved the whole `+0x30`
        // list and left the `+0x44` list alone).
        let mut sched = Scheduler::new();
        sched.divisor = 0;
        let mut finals = queues_from(&[1, 2, 3], &[4, 5], &[6]);
        let (mut clock, _) = ticking();
        let (_, drained, _) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert!(drained.is_empty());
        assert_eq!(finals.queued(), vec![1, 2, 3]);
        assert_eq!(sched.divisor, 0);
        assert_eq!(finals.held(), vec![6, 4, 5]);
    }

    #[test]
    fn the_frame_time_accumulator_smooths_eighty_twenty_and_truncates() {
        let mut sched = Scheduler::new();
        // An advancing clock and an empty run queue: every tick reads 1.0
        // between the two frames, so the sample is 800 ms each time.
        let (mut clock, reads) = ticking();
        let mut finals = queues_from(&[], &[], &[]);
        for expected in [800, 960, 992] {
            quiet_tick(&mut sched, &mut finals, &mut clock);
            assert_eq!(sched.elapsed_ms, expected);
        }
        // Two reads for the frame pair plus the drive's entry read.
        assert_eq!(reads.get(), 9);
        assert_eq!(sched.ticks, 3);
    }

    #[test]
    fn the_tick_consumes_one_callback_period_only_when_the_block_reports_it() {
        // The gate is closed at exactly the period.
        let mut sched = Scheduler::new();
        sched.callback_ticks = 1000;
        let mut finals = queues_from(&[], &[], &[]);
        let (mut clock, reads) = ticking();
        let (_, _, calls) = quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(calls, 0);
        assert_eq!(sched.callback_ticks, 1000);
        assert_eq!(sched.elapsed_ms, 800);
        assert_eq!(reads.get(), 3);

        // Above the period, the block runs and the consuming path subtracts
        // the period.
        sched.callback_ticks = 1001;
        quiet_tick(&mut sched, &mut finals, &mut clock);
        assert_eq!(sched.callback_ticks, 1);
        assert_eq!(sched.elapsed_ms, 960);
    }

    #[test]
    fn a_retaining_callback_block_keeps_the_accumulator() {
        let mut sched = Scheduler::new();
        sched.callback_ticks = 5000;
        let mut finals = queues_from(&[], &[], &[]);
        let mut queues = ThreadQueues::new();
        let (mut clock, _) = ticking();
        let drained = RefCell::new(Vec::new());
        let mut finalize = finalizing(&drained);
        let calls = Rc::new(Cell::new(0u32));
        let mut callbacks = callbacks(CallbackRun::Retained, &calls);
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        let exit = sched.tick(
            &mut queues,
            &mut finals,
            0.0,
            &mut clock,
            &mut finalize,
            &mut callbacks,
            &mut run,
        );
        assert_eq!(exit, SchedExit::ListEnd);
        assert_eq!(calls.get(), 1);
        // `0x418e1a`: the retained path jumps past the subtraction.
        assert_eq!(sched.callback_ticks, 5000);
        // The frame pair still brackets the block: 1.0 between the reads.
        assert_eq!(sched.elapsed_ms, 800);
    }

    #[test]
    fn the_tick_drives_with_a_zero_priority_floor_and_its_own_budget() {
        let mut sched = Scheduler::new();
        let mut queues = ThreadQueues::new();
        let id = queues.spawn(0, 0, "a");
        let mut finals = queues_from(&[], &[], &[]);
        let (mut clock, _) = ticking();
        let drained = RefCell::new(Vec::new());
        let mut finalize = finalizing(&drained);
        let calls = Rc::new(Cell::new(0u32));
        let mut callbacks = callbacks(CallbackRun::Consumed, &calls);
        let log = RefCell::new(Vec::new());
        let mut run = recording(&log, 0);
        // `t0` (read 1) = 1.0, deadline = 6.0, `slice_unit` = 5.0 (the
        // budget), drive entry (read 3) = 3.0. Priority 0 gives a slice of
        // exactly `slice_unit`, so the deadline stays at 6.0 and the pass
        // ends at the eighth read — the drive's fixth top-of-loop read.
        let exit = sched.tick(
            &mut queues,
            &mut finals,
            5.0,
            &mut clock,
            &mut finalize,
            &mut callbacks,
            &mut run,
        );
        assert_eq!(exit, SchedExit::TimeUp);
        // A priority floor above 0 would have skipped the only thread.
        assert_eq!(log.borrow().len(), 4);
        assert!(log.borrow().iter().all(|(t, _)| *t == id));
        assert_eq!(log.borrow()[0].1.to_bits(), 0x40a0_0000);
        assert!(sched.reaped.is_empty());
    }
}
