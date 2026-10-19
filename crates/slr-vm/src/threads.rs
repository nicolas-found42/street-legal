//! Thread objects and the scheduler's run/notify queues (ticket M1-05, #32).
//!
//! Machine model (`[static]` reads of the exe; `[oracle]` Unicorn probes of
//! the exe's own functions, vectors local under `re/out`):
//!
//! - `FUN_0041f340` builds the thread object: `+0x28` priority, `+0x2c`
//!   flags, `+0x30` sleep deadline (f32), `+0x1c` the name buffer, `+0x34`
//!   the monitor object a waiting thread waits on. Arguments are
//!   `(jvm, priority, flags, name)` (`[oracle]`: arg 2 lands in `+0x28`,
//!   arg 3 in `+0x2c`); `Thread.<init>` (`0x47c510`) pushes `(jvm, 0, 2,
//!   name)`, i.e. priority 0 and flags 2 (`[static]`). The ctor clears the
//!   queue links `+0x04`/`+0x08`, builds the frame list, and links the
//!   thread at the tail of `[jvm+0x18]` (the run queue).
//! - A queue is the exe's 0x1c-byte list object: `+0` vtable `0x5f0988`,
//!   then two 12-byte sentinel nodes (`vtable 0x5e7414`, `next`, `prev`),
//!   the head at `+4` and the tail at `+0x10`. The only state the ported
//!   functions observe is the head's `next` (`[list+8]`, first thread) and
//!   the tail's `prev` (`[list+0x18]`, last thread), so [`ThreadList`]
//!   stores exactly those two values with `None` for "the sentinel". Both
//!   sentinels point at each other in an empty list; the ctor builds them
//!   at `0x41f3a2`-`0x41f3d4`.
//! - Linking is "splice after the node the tail sentinel points at, then
//!   point the tail sentinel at the new node" (`FUN_0041f340` at
//!   `0x41f4af`, `FUN_0041f5d0`, `FUN_0041f6f0`, `FUN_0041f650`) — an
//!   append. Unlinking is the exe's shared four-store patch
//!   `prev.next = next; next.prev = prev` from the node's own links
//!   (`0x41f581`, repeated in suspend/resume/wait/notify and the
//!   destructor `FUN_0041f4d0`), which also rewrites the sentinels'
//!   pointers when the node was first or last ([`oracle`]:
//!   suspend+resume of the tail thread, then another spawn, keeps the run
//!   chain linear and the anchor at the last thread).
//! - Every queue operation detaches through the thread's *own* links before
//!   inserting into the target queue, which is what lets `wait` move a
//!   suspended thread (already in the notify queue) to the notify tail
//!   ([`oracle`]: notify `[b, c]` becomes `[c, b]` after `wait(b)`).
//!   [`ThreadQueues`] tracks which queue holds each thread so the detach
//!   hits the right list. Thread `+0x04`/`+0x08` are
//!   [`ThreadState::next`]/[`ThreadState::prev`].
//! - `FUN_0041f570` suspend: already `0x20` -> nothing; set `0x20`; waiting
//!   (`0x10`) -> nothing; else move run queue -> notify queue `[jvm+0x1c]`.
//! - `FUN_0041f5d0` resume: `0x20` clear -> nothing; clear `0x20`; waiting
//!   -> nothing; else move notify queue -> run queue (append at the tail).
//! - `FUN_0041f650` wait: dead (`0x80`) or already waiting -> fatal
//!   `0x60c1c8`/`0x60c1ec`; else unlink, set `0x10`, link into the notify
//!   queue, `[t+0x34] = object`. `Object.wait` (`0x47c920`,
//!   `FUN_00408b20`) then appends the thread to the object's wait
//!   container (the `T_Container` of `monitor.rs`, M1-05 S2).
//! - `FUN_0041f6f0` notify: dead / not waiting -> fatal
//!   `0x60c20c`/`0x60c230`; clear `0x10`; with `0x20` set do nothing (the
//!   thread stays in the notify queue until a later resume); else unlink
//!   and append to the run queue.
//! - `FUN_0041f780` die: when waiting, `FUN_00408c50` scans the thread's
//!   monitor container from its end, clears that slot (the container keeps
//!   its length) and logs `Instance::notify(Thread*): inconsistency` when
//!   the thread is absent; it then calls `FUN_0041f6f0` on the thread, so a
//!   dying waiter loses `0x10` and returns to the run queue (or stays in
//!   the notify queue while `0x20` is set). Finally `0x80` is set. The
//!   monitor half is S2's, so [`ThreadQueues::die`] performs the notify and
//!   reports the monitor object to unlink.
//!
//! `daemon` is not a separate field: the exe's set/clear (`0x47c6e0`) is
//! bit `0x100` of `thread+0x2c` only, so [`ThreadState::daemon`] reads and
//! [`ThreadState::set_daemon`] writes that one bit.
//!
//! Error model: the fatal paths return [`ThreadFault`] without modifying the
//! thread. The exe logs through `FUN_004190d0` and keeps running with a
//! half-updated object; the port stops instead, like the rest of the crate.

use crate::interp::flags;

/// `thread+0x2c` bit 3: asleep until [`ThreadState::sleep_deadline`].
///
/// Mirrors the exe's flag word; [`crate::interp::flags`] carries every bit
/// the interpreter loop itself dispatches on, this one is only read by the
/// scheduler (`FUN_00416940`).
pub const SLEEPING: u32 = 0x08;

/// `thread+0x2c` bit `0x100`: daemon thread.
pub const DAEMON: u32 = 0x100;

/// Index of a thread inside a [`ThreadQueues`] arena.
pub type ThreadId = usize;

/// Which queue a thread is linked into (the exe has no such field: a
/// thread's `+0x04`/`+0x08` link it into exactly one list, and every
/// caller knows which one it is moving the thread on).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Queue {
    Run,
    Notify,
}

/// The four fatal precondition failures of the wait/notify protocol.
///
/// Each carries the exe's own message (`FUN_004190d0` argument), so the
/// eventual native-call layer can report it verbatim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadFault {
    /// `0x60c1c8`: `Thread::wait: thread already died`.
    WaitDied,
    /// `0x60c1ec`: `Thread::wait: already waiting`.
    WaitAlreadyWaiting,
    /// `0x60c20c`: `Thread::notify: thread already died`.
    NotifyDied,
    /// `0x60c230`: `Thread::notify: not waiting`.
    NotifyNotWaiting,
}

impl ThreadFault {
    /// The exe's fatal-error text for this failure.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::WaitDied => "Thread::wait: thread already died",
            Self::WaitAlreadyWaiting => "Thread::wait: already waiting",
            Self::NotifyDied => "Thread::notify: thread already died",
            Self::NotifyNotWaiting => "Thread::notify: not waiting",
        }
    }
}

/// One thread's scheduler-visible state: the exe's `FUN_0041f340` object
/// minus the frame list and the object-model handles (which belong to
/// [`crate::interp::Thread`]).
#[derive(Clone, Debug, Default)]
pub struct ThreadState {
    /// `thread+0x28`: scheduling priority (higher runs first).
    pub priority: i32,
    /// `thread+0x2c`: flag word, see [`crate::interp::flags`].
    pub flags: u32,
    /// `thread+0x30`: clock value at which a sleeping thread wakes.
    pub sleep_deadline: f32,
    /// `thread+0x1c`: the thread's name.
    pub name: String,
    /// `thread+0x34`: monitor object the thread waits on (0 = none).
    pub wait_object: u32,
    /// `thread+0x04`: next thread in whichever queue holds this one.
    next: Option<ThreadId>,
    /// `thread+0x08`: previous thread in that queue.
    prev: Option<ThreadId>,
    /// Which queue holds this thread (see [`Queue`]).
    queue: Option<Queue>,
}

impl ThreadState {
    /// A thread with no queue links and a cleared wait object.
    ///
    /// The exe's ctor leaves `+0x30`/`+0x34` as whatever the allocator
    /// handed back; the port clears them (no ported interface exposes that
    /// garbage).
    #[must_use]
    pub fn new(priority: i32, flags: u32, name: &str) -> Self {
        Self {
            priority,
            flags,
            sleep_deadline: 0.0,
            name: name.to_string(),
            wait_object: 0,
            next: None,
            prev: None,
            queue: None,
        }
    }

    /// `thread+0x04`: the next thread in the queue holding this one
    /// (`None` at the queue's end).
    #[must_use]
    pub fn next(&self) -> Option<ThreadId> {
        self.next
    }

    /// `thread+0x08`: the previous thread in the queue holding this one
    /// (`None` at the queue's start).
    #[must_use]
    pub fn prev(&self) -> Option<ThreadId> {
        self.prev
    }

    /// `thread+0x2c & 0x100`: is this a daemon thread?
    #[must_use]
    pub fn daemon(&self) -> bool {
        self.flags & DAEMON != 0
    }

    /// exe: FUN_0047c6e0
    ///
    /// `Thread.setDaemon` sets or clears bit `0x100` and touches nothing
    /// else (a non-zero argument sets, zero clears).
    pub fn set_daemon(&mut self, on: bool) {
        if on {
            self.flags |= DAEMON;
        } else {
            self.flags &= !DAEMON;
        }
    }

    /// exe: FUN_0041f630
    ///
    /// `Thread.sleep(duration)`: `+0x30 = clock() + duration` in f32, and
    /// bit 3 is set unconditionally (a second sleep overwrites the
    /// deadline). The clock is the caller's (the exe calls `FUN_005516c0`
    /// inside; the port keeps the scheduler's clock external).
    pub fn sleep(&mut self, now: f32, duration: f32) {
        self.sleep_deadline = now + duration;
        self.flags |= SLEEPING;
    }
}

/// One of the exe's queue objects (`[jvm+0x18]` run, `[jvm+0x1c]` notify).
///
/// The exe's object is 0x1c bytes: vtable `0x5f0988`, a head sentinel at
/// `+4` and a tail sentinel at `+0x10` (each a 12-byte list node
/// `vtable,next,prev`). Its observable state is exactly `[list+8]` (head
/// sentinel's next = first thread) and `[list+0x18]` (tail sentinel's prev
/// = last thread), which is what this struct stores as `head`/`tail`;
/// `None` corresponds to the sentinel (an empty queue has
/// `[list+8] == tail sentinel`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadList {
    head: Option<ThreadId>,
    tail: Option<ThreadId>,
}

impl ThreadList {
    /// Is the queue empty (the exe's `[head.next+4] == 0` test)?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// The first thread (`[list+8]`), if any.
    #[must_use]
    pub fn first(&self) -> Option<ThreadId> {
        self.head
    }

    /// The last thread (`[list+0x18]`), if any.
    #[must_use]
    pub fn last(&self) -> Option<ThreadId> {
        self.tail
    }

    /// Threads in queue order, stopping early if the links ever cycle.
    pub fn iter<'a>(&self, threads: &'a [ThreadState]) -> ThreadListIter<'a> {
        ThreadListIter {
            threads,
            cursor: self.head,
            remaining: threads.len(),
        }
    }

    /// Number of threads in the queue.
    #[must_use]
    pub fn count(&self, threads: &[ThreadState]) -> usize {
        self.iter(threads).count()
    }

    /// Is `id` currently in this queue?
    #[must_use]
    pub fn contains(&self, threads: &[ThreadState], id: ThreadId) -> bool {
        self.iter(threads).any(|t| t == id)
    }

    /// exe: FUN_0041f340 (the link at `0x41f4af`), FUN_0041f650,
    /// FUN_0041f6f0, FUN_0041f570, FUN_0041f5d0
    ///
    /// Insert after the anchor `[list+0x18]` (the last thread, or the head
    /// sentinel when the queue is empty) and make the new node the anchor.
    /// The caller must have detached the thread first.
    fn append(&mut self, threads: &mut [ThreadState], id: ThreadId, kind: Queue) {
        let anchor = self.tail;
        threads[id].next = None;
        threads[id].prev = anchor;
        threads[id].queue = Some(kind);
        match anchor {
            Some(a) => threads[a].next = Some(id),
            None => self.head = Some(id),
        }
        self.tail = Some(id);
    }

    /// The unlink shared by suspend/resume/wait/notify (and the destructor
    /// `FUN_0041f4d0`): patch both neighbours from the node's own links,
    /// then clear those links. In the exe the head/tail anchors *are* the
    /// sentinels' link fields, so patching `prev.next`/`next.prev` also
    /// moves `[list+8]`/`[list+0x18]`; here `prev`/`next` of `None` mean
    /// the queue's sentinel, which is exactly when the corresponding anchor
    /// has to change. A thread that is not linked into this queue is left
    /// untouched.
    fn unlink(&mut self, threads: &mut [ThreadState], id: ThreadId, kind: Queue) {
        if threads[id].queue != Some(kind) {
            return;
        }
        let (prev, next) = (threads[id].prev, threads[id].next);
        match prev {
            Some(p) => threads[p].next = next,
            None => self.head = next,
        }
        match next {
            Some(n) => threads[n].prev = prev,
            None => self.tail = prev,
        }
        let t = &mut threads[id];
        t.next = None;
        t.prev = None;
        t.queue = None;
    }
}

/// In-order walk of a [`ThreadList`] over a thread arena.
pub struct ThreadListIter<'a> {
    threads: &'a [ThreadState],
    cursor: Option<ThreadId>,
    remaining: usize,
}

impl Iterator for ThreadListIter<'_> {
    type Item = ThreadId;

    fn next(&mut self) -> Option<ThreadId> {
        if self.remaining == 0 {
            self.cursor = None;
            return None;
        }
        let id = self.cursor.take()?;
        let thread = self.threads.get(id)?;
        self.remaining -= 1;
        self.cursor = thread.next;
        Some(id)
    }
}

/// The JVM's thread bookkeeping: the thread arena plus the run queue
/// (`[jvm+0x18]`) and the notify queue (`[jvm+0x1c]`).
///
/// Threads are allocated once and referenced by [`ThreadId`]; the monitors
/// themselves are a separate container (M1-05 S2), so [`Self::wait`] only
/// does the queue half of `Object.wait`.
#[derive(Clone, Debug, Default)]
pub struct ThreadQueues {
    threads: Vec<ThreadState>,
    run: ThreadList,
    notify: ThreadList,
}

impl ThreadQueues {
    /// No threads, both queues empty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// exe: FUN_0041f340
    ///
    /// Create a thread and link it at the tail of the run queue. The exe
    /// also allocates the frame list and the base frame; frames live in
    /// [`crate::interp::Thread`] here.
    pub fn spawn(&mut self, priority: i32, flags: u32, name: &str) -> ThreadId {
        let id = self.threads.len();
        self.threads.push(ThreadState::new(priority, flags, name));
        self.run.append(&mut self.threads, id, Queue::Run);
        id
    }

    /// All threads, by id.
    #[must_use]
    pub fn threads(&self) -> &[ThreadState] {
        &self.threads
    }

    /// One thread. Panics on an id this arena did not hand out.
    #[must_use]
    pub fn get(&self, id: ThreadId) -> &ThreadState {
        &self.threads[id]
    }

    /// Mutable access to one thread's plain fields (priority, flags, ...).
    /// Callers must not touch the queue links indirectly — they have no
    /// public setter, but they can be clobbered wholesale by cloning state
    /// in; use the queue methods for movement.
    pub fn get_mut(&mut self, id: ThreadId) -> &mut ThreadState {
        &mut self.threads[id]
    }

    /// The run queue (`[jvm+0x18]`).
    #[must_use]
    pub fn run_queue(&self) -> &ThreadList {
        &self.run
    }

    /// The notify queue (`[jvm+0x1c]`).
    #[must_use]
    pub fn notify_queue(&self) -> &ThreadList {
        &self.notify
    }

    /// Detach a thread from whichever queue holds it. Every move
    /// (`FUN_00408b20`'s wait, `FUN_0041f5d0`, `FUN_0041f6f0`) starts with
    /// the exe's unconditional `prev.next = next; next.prev = prev` on the
    /// thread's own links, which is only well defined when it is linked;
    /// asking both queues keeps that true without tracking membership.
    fn detach(&mut self, id: ThreadId) {
        self.run.unlink(&mut self.threads, id, Queue::Run);
        self.notify.unlink(&mut self.threads, id, Queue::Notify);
    }

    /// Run-queue thread ids in scheduling order.
    #[must_use]
    pub fn run_order(&self) -> Vec<ThreadId> {
        self.run.iter(&self.threads).collect()
    }

    /// Notify-queue thread ids in queue order.
    #[must_use]
    pub fn notify_order(&self) -> Vec<ThreadId> {
        self.notify.iter(&self.threads).collect()
    }

    /// exe: FUN_0041f570
    ///
    /// Suspend: set bit `0x20`; unless the thread is waiting, move it from
    /// the run queue to the notify queue. A thread that is already
    /// suspended (or waiting) is left exactly where it is.
    pub fn suspend(&mut self, id: ThreadId) {
        let bits = self.threads[id].flags;
        if bits & flags::NOTIFIED != 0 {
            return;
        }
        self.threads[id].flags = bits | flags::NOTIFIED;
        if bits & flags::WAITING != 0 {
            return;
        }
        self.detach(id);
        self.notify.append(&mut self.threads, id, Queue::Notify);
    }

    /// exe: FUN_0041f5d0
    ///
    /// Resume: clear bit `0x20`; unless the thread is waiting, move it back
    /// to the tail of the run queue. A thread that is not suspended is left
    /// alone, so resuming twice cannot duplicate a queue entry.
    pub fn resume(&mut self, id: ThreadId) {
        let bits = self.threads[id].flags;
        if bits & flags::NOTIFIED == 0 {
            return;
        }
        let bits = bits & !flags::NOTIFIED;
        self.threads[id].flags = bits;
        if bits & flags::WAITING != 0 {
            return;
        }
        self.detach(id);
        self.run.append(&mut self.threads, id, Queue::Run);
    }

    /// exe: FUN_0041f650
    ///
    /// `Thread::wait(object)`: unlink from the run queue, set bit `0x10`,
    /// append to the notify queue and record the monitor object in
    /// `+0x34`. The caller (`Object.wait`, `FUN_00408b20`) then appends the
    /// thread to that object's wait container.
    ///
    /// A dead thread or one that is already waiting is a fatal error and
    /// leaves the state untouched.
    pub fn wait(&mut self, id: ThreadId, object: u32) -> Result<(), ThreadFault> {
        let bits = self.threads[id].flags;
        if bits & flags::DEAD != 0 {
            return Err(ThreadFault::WaitDied);
        }
        if bits & flags::WAITING != 0 {
            return Err(ThreadFault::WaitAlreadyWaiting);
        }
        self.detach(id);
        self.threads[id].flags = bits | flags::WAITING;
        self.notify.append(&mut self.threads, id, Queue::Notify);
        self.threads[id].wait_object = object;
        Ok(())
    }

    /// exe: FUN_0041f6f0
    ///
    /// `Thread::notify`: clear bit `0x10`. A suspended thread keeps `0x20`
    /// and stays in the notify queue (a later [`Self::resume`] moves it);
    /// otherwise it is appended to the run queue.
    ///
    /// A dead thread or one that is not waiting is a fatal error and leaves
    /// the state untouched.
    pub fn notify(&mut self, id: ThreadId) -> Result<(), ThreadFault> {
        let bits = self.threads[id].flags;
        if bits & flags::DEAD != 0 {
            return Err(ThreadFault::NotifyDied);
        }
        if bits & flags::WAITING == 0 {
            return Err(ThreadFault::NotifyNotWaiting);
        }
        let bits = bits & !flags::WAITING;
        self.threads[id].flags = bits;
        if bits & flags::NOTIFIED != 0 {
            return Ok(());
        }
        self.detach(id);
        self.run.append(&mut self.threads, id, Queue::Run);
        Ok(())
    }

    /// exe: FUN_0041f780
    ///
    /// Mark the thread dead (bit `0x80`), keeping every other flag. A
    /// waiting thread is first notified (`FUN_00408c50` -> `FUN_0041f6f0`):
    /// `0x10` clears and it moves to the run queue, or stays queued behind
    /// `0x20`. Returns the monitor object the caller must unlink from its
    /// wait container (S2): the exe scans that container and clears the
    /// thread's slot without shrinking it.
    pub fn die(&mut self, id: ThreadId) -> Option<u32> {
        let bits = self.threads[id].flags;
        let waiting = bits & flags::WAITING != 0;
        if waiting {
            // Dead is set only below, so the notify cannot fail.
            debug_assert!(self.notify(id).is_ok());
        }
        self.threads[id].flags |= flags::DEAD;
        waiting.then_some(self.threads[id].wait_object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three runnable threads `a`, `b`, `c` in queue order. `a` carries the
    /// `Thread.<init>` flag 2 (created, not started).
    fn three() -> (ThreadQueues, [ThreadId; 3]) {
        let mut q = ThreadQueues::new();
        let a = q.spawn(0, 2, "a");
        let b = q.spawn(5, 0, "b");
        let c = q.spawn(-3, 0, "c");
        (q, [a, b, c])
    }

    /// Same three threads with a zero flag word, for tests that compare the
    /// whole flag word.
    fn plain() -> (ThreadQueues, [ThreadId; 3]) {
        let mut q = ThreadQueues::new();
        let a = q.spawn(0, 0, "a");
        let b = q.spawn(5, 0, "b");
        let c = q.spawn(-3, 0, "c");
        (q, [a, b, c])
    }

    #[test]
    fn spawn_stores_fields_and_appends_at_tail() {
        let mut q = ThreadQueues::new();
        let a = q.spawn(7, 0x1234, "nm");
        let b = q.spawn(0, 2, "b");
        let t = q.get(a);
        assert_eq!(t.priority, 7);
        assert_eq!(t.flags, 0x1234);
        assert_eq!(t.name, "nm");
        assert_eq!(t.sleep_deadline, 0.0);
        assert_eq!(t.wait_object, 0);
        assert_eq!(q.run_order(), vec![a, b]);
        assert_eq!(q.get(a).next(), Some(b));
        assert_eq!(q.get(a).prev(), None);
        assert_eq!(q.get(b).next(), None);
        assert_eq!(q.get(b).prev(), Some(a));
        assert_eq!(q.run_queue().first(), Some(a));
        assert_eq!(q.run_queue().last(), Some(b));
        assert!(q.notify_queue().is_empty());
    }

    #[test]
    fn suspend_moves_to_notify_queue_and_sets_notified() {
        let (mut q, [a, b, c]) = three();
        q.suspend(b);
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
        assert_eq!(q.get(b).flags, flags::NOTIFIED);
        assert_eq!(q.get(b).next(), None);
        assert_eq!(q.get(b).prev(), None);
        assert_eq!(q.get(a).next(), Some(c));
        assert_eq!(q.get(c).prev(), Some(a));
    }

    #[test]
    fn suspend_twice_does_not_duplicate() {
        let (mut q, [_, b, _]) = three();
        q.suspend(b);
        q.suspend(b);
        assert_eq!(q.notify_order(), vec![b]);
        assert_eq!(q.notify_queue().count(q.threads()), 1);
        assert_eq!(q.get(b).flags, flags::NOTIFIED);
    }

    #[test]
    fn resume_appends_to_run_tail() {
        let (mut q, [a, b, c]) = three();
        q.suspend(b);
        q.resume(b);
        assert_eq!(q.run_order(), vec![a, c, b]);
        assert!(q.notify_order().is_empty());
        assert_eq!(q.get(b).flags, 0);
        assert_eq!(q.run_queue().last(), Some(b));
        assert_eq!(q.get(b).next(), None);
    }

    #[test]
    fn resume_without_suspend_is_a_no_op() {
        let (mut q, [a, b, c]) = three();
        q.resume(b);
        assert_eq!(q.run_order(), vec![a, b, c]);
        assert_eq!(q.get(b).flags, 0);
        assert_eq!(q.run_queue().count(q.threads()), 3);
    }

    #[test]
    fn wait_moves_to_notify_queue_and_records_object() {
        let (mut q, [a, b, c]) = three();
        q.wait(b, 0xdead_beef).expect("wait");
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
        assert_eq!(q.get(b).flags, flags::WAITING);
        assert_eq!(q.get(b).wait_object, 0xdead_beef);
        assert_eq!(q.get(b).next(), None);
        assert_eq!(q.get(b).prev(), None);
    }

    #[test]
    fn waiting_thread_ignores_suspend_and_resume_movement() {
        let (mut q, [a, b, c]) = three();
        q.wait(b, 1).expect("wait");
        q.suspend(b);
        // Suspended while waiting: both bits, still in the notify queue.
        assert_eq!(q.get(b).flags, flags::WAITING | flags::NOTIFIED);
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
        q.resume(b);
        // Resume only clears 0x20; a waiting thread is not moved.
        assert_eq!(q.get(b).flags, flags::WAITING);
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
    }

    #[test]
    fn wait_faults_leave_state_untouched() {
        let (mut q, [a, b, _]) = three();
        q.get_mut(b).flags |= flags::DEAD;
        let before = q.run_order();
        assert_eq!(q.wait(b, 7), Err(ThreadFault::WaitDied));
        assert_eq!(q.get(b).flags, flags::DEAD);
        assert_eq!(q.get(b).wait_object, 0);
        assert_eq!(q.run_order(), before);

        let (mut q, [a2, _, _]) = three();
        q.wait(a2, 7).expect("first wait");
        assert_eq!(q.wait(a2, 8), Err(ThreadFault::WaitAlreadyWaiting));
        assert_eq!(q.get(a2).wait_object, 7);
        assert_eq!(q.notify_order(), vec![a2]);
        let _ = a;
    }

    #[test]
    fn notify_returns_waiter_to_run_tail() {
        let (mut q, [a, b, c]) = three();
        q.wait(b, 9).expect("wait");
        q.notify(b).expect("notify");
        assert_eq!(q.get(b).flags, 0);
        assert_eq!(q.run_order(), vec![a, c, b]);
        assert!(q.notify_order().is_empty());
        assert_eq!(q.get(b).wait_object, 9);
    }

    #[test]
    fn notify_of_a_suspended_waiter_keeps_it_queued() {
        let (mut q, [a, b, c]) = three();
        q.wait(b, 3).expect("wait");
        q.suspend(b);
        q.notify(b).expect("notify");
        // 0x10 cleared, 0x20 kept, queue placement unchanged (FUN_0041f6f0).
        assert_eq!(q.get(b).flags, flags::NOTIFIED);
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
        // The subsequent resume does the move.
        q.resume(b);
        assert_eq!(q.get(b).flags, 0);
        assert_eq!(q.run_order(), vec![a, c, b]);
        assert!(q.notify_order().is_empty());
    }

    #[test]
    fn notify_faults() {
        let (mut q, [a, b, _]) = plain();
        assert_eq!(q.notify(a), Err(ThreadFault::NotifyNotWaiting));
        assert_eq!(q.run_order(), vec![a, b, q.run_order()[2]]);

        q.wait(a, 1).expect("wait");
        q.get_mut(a).flags |= flags::DEAD;
        assert_eq!(q.notify(a), Err(ThreadFault::NotifyDied));
        assert_eq!(q.get(a).flags, flags::WAITING | flags::DEAD);
    }

    #[test]
    fn die_of_a_waiter_notifies_then_sets_dead() {
        // [oracle] die(a) while it waits: notify moves it to the run tail,
        // flags become 0x80, and the monitor object is reported.
        let (mut q, [a, b, c]) = plain();
        q.wait(a, 0x20).expect("wait");
        assert_eq!(q.die(a), Some(0x20));
        assert_eq!(q.get(a).flags, flags::DEAD);
        assert_eq!(q.run_order(), vec![b, c, a]);
        assert!(q.notify_order().is_empty());
    }

    #[test]
    fn die_of_a_suspended_waiter_keeps_it_queued() {
        // [oracle] a waiting thread that is also suspended: die clears
        // 0x10, keeps 0x20, so the notify queue still holds it.
        let (mut q, [a, b, c]) = plain();
        q.wait(a, 0x30).expect("wait");
        q.suspend(a);
        assert_eq!(q.die(a), Some(0x30));
        assert_eq!(q.get(a).flags, flags::DEAD | flags::NOTIFIED);
        assert_eq!(q.notify_order(), vec![a]);
        assert_eq!(q.run_order(), vec![b, c]);
    }

    #[test]
    fn die_of_a_running_thread_just_sets_dead() {
        let (mut q, [a, b, c]) = plain();
        assert_eq!(q.die(b), None);
        assert_eq!(q.get(b).flags, flags::DEAD);
        assert_eq!(q.run_order(), vec![a, b, c]);
    }

    #[test]
    fn sleep_sets_deadline_and_flag() {
        let (mut q, [a, _, _]) = plain();
        q.get_mut(a).sleep(5.0, 2.5);
        assert_eq!(q.get(a).sleep_deadline, 7.5);
        assert_ne!(q.get(a).flags & SLEEPING, 0);
        // A second sleep overwrites the deadline and keeps other flags.
        q.get_mut(a).flags |= flags::RUNNING;
        q.get_mut(a).sleep(10.0, 1.0);
        assert_eq!(q.get(a).sleep_deadline, 11.0);
        assert_eq!(q.get(a).flags, SLEEPING | flags::RUNNING);
    }

    #[test]
    fn daemon_is_flag_bit_0x100() {
        let (mut q, [a, _, _]) = plain();
        q.get_mut(a).set_daemon(true);
        assert!(q.get(a).daemon());
        assert_eq!(q.get(a).flags & DAEMON, DAEMON);
        q.get_mut(a).flags |= flags::STOPPED;
        q.get_mut(a).set_daemon(false);
        assert!(!q.get(a).daemon());
        assert_eq!(q.get(a).flags, flags::STOPPED);
    }

    #[test]
    fn queues_report_membership() {
        let (mut q, [a, b, c]) = three();
        q.suspend(b);
        assert!(q.run_queue().contains(q.threads(), a));
        assert!(!q.run_queue().contains(q.threads(), b));
        assert!(q.notify_queue().contains(q.threads(), b));
        assert_eq!(q.run_queue().count(q.threads()), 2);
        assert_eq!(q.notify_queue().count(q.threads()), 1);
        let _ = c;
    }

    #[test]
    fn wait_of_a_suspended_thread_moves_it_to_the_notify_tail() {
        // [oracle] run [a,b,c]: suspend b, suspend c -> notify [b,c];
        // wait(b) -> notify [c,b] (FUN_0041f650 unlinks from its own links
        // and appends), flags 0x30.
        let (mut q, [a, b, c]) = plain();
        q.suspend(b);
        q.suspend(c);
        assert_eq!(q.notify_order(), vec![b, c]);
        q.wait(b, 0x77).expect("wait");
        assert_eq!(q.run_order(), vec![a]);
        assert_eq!(q.notify_order(), vec![c, b]);
        assert_eq!(q.get(b).flags, flags::WAITING | flags::NOTIFIED);
        assert_eq!(q.get(b).wait_object, 0x77);
        // notify leaves it queued; resume moves it to the run tail.
        q.notify(b).expect("notify");
        assert_eq!(q.notify_order(), vec![c, b]);
        assert_eq!(q.get(b).flags, flags::NOTIFIED);
        q.resume(b);
        assert_eq!(q.run_order(), vec![a, b]);
        assert_eq!(q.notify_order(), vec![c]);
        assert_eq!(q.get(b).flags, 0);
    }

    #[test]
    fn tail_removal_keeps_the_list_linear() {
        // [oracle] suspend+resume of the tail thread, then a spawn: the
        // chain stays a,b,c,d and the tail anchor is the last thread.
        let (mut q, [a, b, c]) = plain();
        q.suspend(c);
        q.resume(c);
        let d = q.spawn(1, 0, "d");
        assert_eq!(q.run_order(), vec![a, b, c, d]);
        assert_eq!(q.run_queue().last(), Some(d));
        assert_eq!(q.get(c).next(), Some(d));
        assert_eq!(q.get(d).prev(), Some(c));
    }

    #[test]
    fn detaching_from_the_wrong_queue_is_a_no_op() {
        let (mut q, [a, b, c]) = plain();
        q.suspend(b);
        // Resuming a thread that is not suspended must not disturb the
        // notify queue it sits in.
        q.resume(a);
        assert_eq!(q.run_order(), vec![a, c]);
        assert_eq!(q.notify_order(), vec![b]);
        assert_eq!(q.get(b).prev(), None);
    }

    #[test]
    fn fault_messages_match_the_exe() {
        assert_eq!(
            ThreadFault::WaitDied.message(),
            "Thread::wait: thread already died"
        );
        assert_eq!(
            ThreadFault::WaitAlreadyWaiting.message(),
            "Thread::wait: already waiting"
        );
        assert_eq!(
            ThreadFault::NotifyDied.message(),
            "Thread::notify: thread already died"
        );
        assert_eq!(
            ThreadFault::NotifyNotWaiting.message(),
            "Thread::notify: not waiting"
        );
    }
}
