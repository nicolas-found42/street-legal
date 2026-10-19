//! Monitor objects: the wait container and the `Object.wait` / `notify` /
//! `notifyAll` protocol (M1-05 S2, issue #32).
//!
//! Machine model (`[static]` machine-code reads of the exe; `[oracle]`
//! Unicorn probes of the exe's own functions):
//!
//! - An object's wait container lives at `object+0x14` and is the exe's
//!   `T_Container`: `+0` capacity, `+4` data pointer, `+8` count
//!   (`[static]`, `FUN_00408b20` at `0x408b30`). `Object.wait`
//!   (`0x408b20`) allocates a zeroed 12-byte container when the slot is
//!   null and then appends the thread; [`Container`] models the absent and
//!   the empty container identically because every ported operation only
//!   distinguishes "count is zero".
//! - Append (`FUN_00408b20` at `0x408b63`): when `count + 1 > capacity`,
//!   grow to `capacity + count + 1` (realloc; free + null when that is 0),
//!   clamp `count` to the new capacity when it shrank, then store at
//!   `data[count]` and increment. So the capacities walk
//!   `0 -> 1 -> 3 -> 7 -> 15 ...` — the same rule as the operand-stack
//!   container ([`crate::values::push_slot`]).
//! - Pop-last (`FUN_00408bc0` at `0x408bd2`): if `count == 0` the exe logs
//!   `T_Container: underflow` (`0x607b24`) and returns 0; otherwise
//!   `count -= 1`, read `data[count]`, zero that slot, and return the
//!   thread (0 when the slot was already zero).
//! - `Object.notify` (`0x408bc0`, `FUN_00408bc0`): when the container is
//!   null or empty it returns **silently** (`0x408bc8`/`0x408bd0` jump to
//!   the epilogue). Otherwise it pops from the end, skipping slots that
//!   were already zeroed (the destructor's unlink leaves such holes) for
//!   at most `count` iterations, and calls `FUN_0041f6f0` on the first
//!   non-zero thread. So notifications are LIFO (`[oracle]`: waits
//!   `a, b, c` are woken `c, b, a`).
//! - `Object.notifyAll` (`0x408ce0`, `FUN_00408ce0`): silent when the
//!   container is null or empty, else pops for at most `count` iterations
//!   and notifies every non-zero slot, newest first.
//! - The die scanner (`FUN_00408c50`, called by `FUN_0041f780`): walks the
//!   container from `count-1` down, compares `data[i]` (0 when out of
//!   capacity) with the dying thread, and on a match zeroes that slot and
//!   calls `FUN_0041f6f0`; `count` is **not** changed. When the thread is
//!   absent it logs `! Instance::notify(Thread*): inconsistency`
//!   (`0x607af8`) and does not notify. [`Monitor::unlink`] is that
//!   container half; the notify half is performed by
//!   [`crate::threads::ThreadQueues::die`] (see its documentation for the
//!   ordering difference).
//! - The four fatal precondition checks (`Thread::wait: thread already
//!   died` / `already waiting`, `Thread::notify: thread already died` /
//!   `not waiting`) are in `FUN_0041f650`/`FUN_0041f6f0`, i.e. the thread
//!   half in [`crate::threads`]. The exe logs through `FUN_004190d0` and
//!   continues with a half-updated object; this module stops instead and
//!   reports the fault, like the rest of the crate.
//!
//! Error model. Every failure is a [`MonitorFault`]: the four thread-side
//! fatal paths ([`ThreadFault`]) and, port-only, [`MonitorFault::EmptyNotify`]
//! for a notify that woke nobody — the exe returns silently, but a lost
//! notification is exactly the `Instance::notify(Thread*): inconsistency`
//! condition the die scanner treats as a fatal log, so the port surfaces it
//! instead of swallowing it. `notify`/`notifyAll` pop the slot before the
//! thread-side check, because the exe zeroes the slot first
//! (`0x408be9`) and only then calls `FUN_0041f6f0`; a failed notify
//! therefore leaves the waiter removed, exactly like the exe.

use crate::threads::{ThreadFault, ThreadId, ThreadQueues};

/// The exe's `T_Container` as used for an object's wait list:
/// `[+0] capacity, [+4] data, [+8] count`.
///
/// `slots.len()` mirrors the capacity; a slot is `None` when the exe's
/// word is 0 (empty, or zeroed by a notify pop / the die scanner).
#[derive(Clone, Debug, Default)]
pub struct Container {
    /// `container+0`: allocated slots.
    cap: u32,
    /// `container+4`: the data words, one thread id per slot.
    slots: Vec<Option<ThreadId>>,
    /// `container+8`: number of occupied-or-holed slots in use.
    count: u32,
}

impl Container {
    /// A zeroed container (the shape `Object.wait` allocates).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `container+0`: capacity.
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.cap
    }

    /// `container+8`: count (includes slots zeroed by a pop or unlink —
    /// only the die scanner and the pops move it, and pops move it down).
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Is the container empty (the `count == 0` test of notify)?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// `data[i]`, or `None` when `i` is outside the capacity (the exe's
    /// `capacity > i` guard) or the word is 0.
    #[must_use]
    pub fn slot(&self, index: u32) -> Option<ThreadId> {
        if index >= self.cap {
            return None;
        }
        self.slots.get(index as usize).copied().flatten()
    }

    /// Non-zero slots in container order (append order, holes skipped).
    #[must_use]
    pub fn threads(&self) -> Vec<ThreadId> {
        (0..self.count).filter_map(|i| self.slot(i)).collect()
    }

    /// exe: FUN_00408b20 (the append at `0x408b63`)
    ///
    /// Append at `data[count]`, growing to `capacity + count + 1` first
    /// when the container is full. A capacity wrap (only possible with
    /// 2^31 waiters) faults on the store like the exe's null `data`.
    pub fn append(&mut self, thread: ThreadId) {
        if self.count.wrapping_add(1) > self.cap {
            let new = self.cap.wrapping_add(self.count).wrapping_add(1);
            self.slots.resize(new as usize, None);
            if new < self.count {
                self.count = new;
            }
            self.cap = new;
        }
        self.slots[self.count as usize] = Some(thread);
        self.count += 1;
    }

    /// exe: FUN_00408bc0 (the pop at `0x408bd2`)
    ///
    /// Pop the last slot: `count -= 1`, take and zero `data[count]`. Returns
    /// `None` when the container is empty (`T_Container: underflow`,
    /// `0x607b24`) or when the slot was already zero; both are a 0 return
    /// in the exe.
    pub fn pop_last(&mut self) -> Option<ThreadId> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        self.slots[self.count as usize].take()
    }

    /// Zero one slot without touching the count (the die scanner's clear).
    fn clear(&mut self, index: u32) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            *slot = None;
        }
    }
}

/// Why a monitor operation failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonitorFault {
    /// Port-only: a notify (or notifyAll) found no waiter at all. The exe's
    /// `0x408bc0`/`0x408ce0` return silently in that case; the same loss of
    /// a notification is what `FUN_00408c50` logs as
    /// `Instance::notify(Thread*): inconsistency`.
    EmptyNotify,
    /// One of the four fatal precondition failures of the thread half.
    Thread(ThreadFault),
}

impl MonitorFault {
    /// The exe's diagnostic text for this failure.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::EmptyNotify => "Instance::notify(Thread*): inconsistency",
            Self::Thread(fault) => fault.message(),
        }
    }
}

impl From<ThreadFault> for MonitorFault {
    fn from(fault: ThreadFault) -> Self {
        Self::Thread(fault)
    }
}

/// One object's monitor state: `object+0x14`, the wait container.
#[derive(Clone, Debug, Default)]
pub struct Monitor {
    container: Container,
}

impl Monitor {
    /// A monitor with no waiters (the container is allocated lazily in the
    /// exe; an absent container behaves exactly like an empty one here).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The wait container (`object+0x14`).
    #[must_use]
    pub fn container(&self) -> &Container {
        &self.container
    }

    /// Threads currently waiting on this monitor, in wait order.
    #[must_use]
    pub fn waiters(&self) -> Vec<ThreadId> {
        self.container.threads()
    }

    /// exe: FUN_00408b20 (`Object.wait`), thread half FUN_0041f650
    ///
    /// Move the calling thread onto this monitor: unlink it from the run
    /// queue, set `0x10`, append it to the notify queue, record the monitor
    /// object in `+0x34`, then append it here. A dead thread or one that is
    /// already waiting is a fatal error and leaves both the thread and the
    /// container untouched (the exe logs and appends a half-updated thread
    /// anyway).
    pub fn wait(
        &mut self,
        queues: &mut ThreadQueues,
        thread: ThreadId,
        object: u32,
    ) -> Result<(), MonitorFault> {
        queues.wait(thread, object)?;
        self.container.append(thread);
        Ok(())
    }

    /// exe: FUN_00408bc0 (`Object.notify`)
    ///
    /// Wake the newest waiter: pop from the end, skipping slots already
    /// zeroed by [`Self::unlink`] (at most `count` probes), clear its
    /// `0x10` and return it to the run queue — a suspended waiter keeps
    /// `0x20` and stays in the notify queue for a later resume. No waiter
    /// at all is [`MonitorFault::EmptyNotify`].
    pub fn notify(&mut self, queues: &mut ThreadQueues) -> Result<ThreadId, MonitorFault> {
        let budget = self.container.count();
        if budget == 0 {
            return Err(MonitorFault::EmptyNotify);
        }
        for _ in 0..budget {
            if let Some(thread) = self.container.pop_last() {
                queues.notify(thread)?;
                return Ok(thread);
            }
        }
        Err(MonitorFault::EmptyNotify)
    }

    /// exe: FUN_00408ce0 (`Object.notifyAll`)
    ///
    /// Wake every waiter, newest first. The exe's silent empty-container
    /// return becomes `Ok` with no threads, since notifying nobody is the
    /// normal outcome of `notifyAll` here; a thread-side fatal path stops
    /// the drain with that fault, like the rest of the crate stops instead
    /// of logging and continuing.
    pub fn notify_all(&mut self, queues: &mut ThreadQueues) -> Result<Vec<ThreadId>, MonitorFault> {
        let mut woken = Vec::new();
        loop {
            match self.notify(queues) {
                Ok(thread) => woken.push(thread),
                Err(MonitorFault::EmptyNotify) => return Ok(woken),
                Err(fault) => return Err(fault),
            }
        }
    }

    /// exe: FUN_00408c50
    ///
    /// The die scanner's container half: zero the thread's slot, scanning
    /// from the end, without changing the count. Returns false — the exe's
    /// `Instance::notify(Thread*): inconsistency` case — when the thread is
    /// not in the container. The notify half is
    /// [`ThreadQueues::die`](crate::threads::ThreadQueues::die), which the
    /// exe runs only on a match.
    pub fn unlink(&mut self, thread: ThreadId) -> bool {
        let mut index = self.container.count();
        while index > 0 {
            index -= 1;
            if self.container.slot(index) == Some(thread) {
                self.container.clear(index);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::flags;

    const OBJ: u32 = 0x0030_0000;

    /// A fresh monitor with `a`, `b`, `c` waiting on it in that order.
    fn three_waiters() -> (Monitor, ThreadQueues, [ThreadId; 3]) {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        let b = queues.spawn(5, 0, "b");
        let c = queues.spawn(-3, 0, "c");
        let mut monitor = Monitor::new();
        for id in [a, b, c] {
            monitor.wait(&mut queues, id, OBJ).expect("wait");
        }
        (monitor, queues, [a, b, c])
    }

    #[test]
    fn container_grows_like_the_exe() {
        let mut container = Container::new();
        assert_eq!((container.capacity(), container.count()), (0, 0));
        assert!(container.is_empty());
        for (expected_cap, thread) in [(1, 10), (3, 11), (3, 12), (7, 13)] {
            container.append(thread);
            assert_eq!(container.capacity(), expected_cap);
        }
        assert_eq!(container.count(), 4);
        assert_eq!(container.threads(), vec![10, 11, 12, 13]);
        assert_eq!(container.slot(0), Some(10));
        assert_eq!(container.slot(3), Some(13));
        // Outside the capacity the exe's guard reads 0.
        assert_eq!(container.slot(4), None);
        assert_eq!(container.slot(7), None);
    }

    #[test]
    fn pop_last_shrinks_the_count_and_zeroes_the_slot() {
        let mut container = Container::new();
        container.append(1);
        container.append(2);
        assert_eq!(container.pop_last(), Some(2));
        assert_eq!(container.count(), 1);
        assert_eq!(container.slot(1), None);
        assert_eq!(container.pop_last(), Some(1));
        assert!(container.is_empty());
        // Empty: the exe's T_Container underflow path returns 0.
        assert_eq!(container.pop_last(), None);
        assert_eq!(container.count(), 0);
    }

    #[test]
    fn wait_appends_in_order_and_notify_is_lifo() {
        let (mut monitor, mut queues, [a, b, c]) = three_waiters();
        assert_eq!(monitor.waiters(), vec![a, b, c]);
        assert_eq!(monitor.container().count(), 3);
        assert!(queues.run_order().is_empty());
        assert_eq!(queues.notify_order(), vec![a, b, c]);
        for id in [a, b, c] {
            assert_eq!(queues.get(id).wait_object, OBJ);
            assert_eq!(queues.get(id).flags, flags::WAITING);
        }

        assert_eq!(monitor.notify(&mut queues), Ok(c));
        assert_eq!(monitor.notify(&mut queues), Ok(b));
        assert_eq!(monitor.notify(&mut queues), Ok(a));
        assert_eq!(queues.run_order(), vec![c, b, a]);
        assert!(queues.notify_order().is_empty());
        assert_eq!(monitor.waiters(), vec![]);
        for id in [a, b, c] {
            assert_eq!(queues.get(id).flags, 0);
        }
    }

    #[test]
    fn notify_without_waiters_is_an_error() {
        // The exe returns silently; the port reports the lost notification.
        let mut queues = ThreadQueues::new();
        let mut monitor = Monitor::new();
        assert_eq!(monitor.notify(&mut queues), Err(MonitorFault::EmptyNotify));
        assert_eq!(monitor.notify(&mut queues), Err(MonitorFault::EmptyNotify));
        // notifyAll on an empty monitor is simply nothing to wake.
        assert_eq!(monitor.notify_all(&mut queues), Ok(vec![]));
    }

    #[test]
    fn waiting_twice_is_a_fatal_error() {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        let mut monitor = Monitor::new();
        monitor.wait(&mut queues, a, OBJ).expect("first wait");
        assert_eq!(
            monitor.wait(&mut queues, a, OBJ),
            Err(MonitorFault::Thread(ThreadFault::WaitAlreadyWaiting))
        );
        // Container and queue links unchanged by the failed second wait.
        assert_eq!(monitor.waiters(), vec![a]);
        assert_eq!(monitor.container().count(), 1);
        assert_eq!(queues.notify_order(), vec![a]);
        assert_eq!(queues.get(a).wait_object, OBJ);
    }

    #[test]
    fn waiting_dead_is_a_fatal_error() {
        let mut queues = ThreadQueues::new();
        let a = queues.spawn(0, 0, "a");
        queues.get_mut(a).flags |= flags::DEAD;
        let mut monitor = Monitor::new();
        assert_eq!(
            monitor.wait(&mut queues, a, OBJ),
            Err(MonitorFault::Thread(ThreadFault::WaitDied))
        );
        assert!(monitor.container().is_empty());
        assert_eq!(queues.run_order(), vec![a]);
    }

    #[test]
    fn notify_of_a_suspended_waiter_keeps_bit_0x20() {
        // Wait order a, b, c: notify pops the last, so suspend c to make it
        // the thread that gets notified.
        let (mut monitor, mut queues, [a, b, c]) = three_waiters();
        queues.suspend(c);
        assert_eq!(queues.get(c).flags, flags::WAITING | flags::NOTIFIED);

        assert_eq!(monitor.notify(&mut queues), Ok(c));
        // 0x10 cleared, 0x20 kept, still in the notify queue.
        assert_eq!(queues.get(c).flags, flags::NOTIFIED);
        assert_eq!(queues.run_order(), vec![]);
        assert_eq!(queues.notify_order(), vec![a, b, c]);
        assert_eq!(monitor.waiters(), vec![a, b]);
        assert_eq!(queues.get(a).flags, flags::WAITING);
        assert_eq!(queues.get(b).flags, flags::WAITING);

        // The later resume does the move (FUN_0041f5d0).
        queues.resume(c);
        assert_eq!(queues.get(c).flags, 0);
        assert_eq!(queues.run_order(), vec![c]);
        assert_eq!(queues.notify_order(), vec![a, b]);
    }

    #[test]
    fn notify_all_drains_newest_first() {
        let (mut monitor, mut queues, [a, b, c]) = three_waiters();
        assert_eq!(monitor.notify_all(&mut queues), Ok(vec![c, b, a]));
        assert!(monitor.container().is_empty());
        assert_eq!(queues.run_order(), vec![c, b, a]);
        assert!(queues.notify_order().is_empty());
        assert_eq!(queues.get(a).flags, 0);
    }

    #[test]
    fn notify_skips_holes_left_by_unlink() {
        let (mut monitor, mut queues, [a, b, c]) = three_waiters();
        assert!(monitor.unlink(b));
        // The die scanner zeroes the slot but keeps the count.
        assert_eq!(monitor.container().count(), 3);
        assert_eq!(monitor.container().slot(1), None);
        assert_eq!(monitor.waiters(), vec![a, c]);

        assert_eq!(monitor.notify(&mut queues), Ok(c));
        // One probe hits the hole, the next reaches `a`.
        assert_eq!(monitor.notify(&mut queues), Ok(a));
        assert_eq!(monitor.notify(&mut queues), Err(MonitorFault::EmptyNotify));
        assert!(monitor.container().is_empty());
        assert_eq!(queues.run_order(), vec![c, a]);
    }

    #[test]
    fn unlink_reports_an_absent_thread() {
        let (mut monitor, _queues, [a, b, c]) = three_waiters();
        assert!(monitor.unlink(c));
        assert!(monitor.unlink(a));
        // A second scan of an already cleared thread is the exe's
        // "inconsistency" case.
        assert!(!monitor.unlink(a));
        assert!(monitor.unlink(b));
        assert!(!monitor.unlink(b));
        // Count untouched: the container keeps its length.
        assert_eq!(monitor.container().count(), 3);
        assert!(monitor.waiters().is_empty());
    }

    #[test]
    fn notify_of_a_dead_waiter_reports_the_fault_after_the_pop() {
        // The exe zeroes the slot, then FUN_0041f6f0 logs and continues; the
        // port keeps the pop and returns the fault. `c` waits last, so it is
        // the one notify pops.
        let (mut monitor, mut queues, [_, _, c]) = three_waiters();
        queues.get_mut(c).flags |= flags::DEAD;
        assert_eq!(
            monitor.notify(&mut queues),
            Err(MonitorFault::Thread(ThreadFault::NotifyDied))
        );
        assert_eq!(monitor.container().count(), 2);
        assert_eq!(monitor.waiters().len(), 2);
        assert_eq!(queues.get(c).flags, flags::WAITING | flags::DEAD);
    }

    #[test]
    fn fault_messages_match_the_exe() {
        assert_eq!(
            MonitorFault::EmptyNotify.message(),
            "Instance::notify(Thread*): inconsistency"
        );
        assert_eq!(
            MonitorFault::Thread(ThreadFault::NotifyNotWaiting).message(),
            "Thread::notify: not waiting"
        );
        assert_eq!(
            MonitorFault::from(ThreadFault::WaitAlreadyWaiting).message(),
            "Thread::wait: already waiting"
        );
    }
}
