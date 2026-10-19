//! `java.lang.Thread` / `java.lang.Object` natives (M1-05, #32).
//!
//! This module owns the 13 rows the exe's registrar (`FUN_00416b00`) files
//! under `java.lang.Thread` and `java.lang.Object` for the thread/monitor
//! subsystem (`re/out/native_api.tsv`, `0x47c510`..`0x47c970`;
//! `handoff-M1-05.md` (b)). Three rows have ports: **T7**
//! `java.lang.Thread.init` `(Ljava.lang.String;)V` at [`THREAD_INIT`] (the
//! script-side `<init>` of `system/Scripts/lang/Thread.class`), **T8**
//! `java.lang.Thread.start` `()V` at [`THREAD_START`] and **T9**
//! `java.lang.Thread.stop` `()V` at [`THREAD_STOP`].
//!
//! ```text
//! 47c510 push ecx                 ; local L (the marshalled parameter)
//! 47c511 push esi
//! 47c512 mov  esi,[esp+0xc]       ; esi = the CallInfo argument
//! 47c516 lea  eax,[esp+4]
//! 47c51a push eax                 ; &L: where out[1] goes
//! 47c51b lea  ecx,[esp+0x10]
//! 47c51f push ecx                 ; &(arg1 slot): where out[0] goes
//! 47c520 push esi
//! 47c521 call FUN_0045d910        ; out[0] = receiver, out[1] = the String
//! 47c526 push 0x38
//! 47c528 call FUN_0054f560        ; malloc(0x38): the thread object
//! 47c52d add  esp,0x10
//! 47c534 mov  edx,[esp+4]         ; edx = the parameter: the thread's name
//! 47c538 mov  ecx,[esi]           ; ecx = jvm
//! 47c53a push edx                 ; name
//! 47c53b push 2                   ; flags    = 2
//! 47c53d push 0                   ; priority = 0
//! 47c53f push ecx                 ; jvm
//! 47c540 mov  ecx,eax
//! 47c542 call FUN_0041f340        ; build + link at the run-queue tail
//! 47c54b mov  edx,[esp+0xc]       ; edx = the receiver slot
//! 47c54f push eax
//! 47c550 mov  [eax+0x18],edx      ; thread+0x18 = the script object
//! 47c553 mov  eax,[0x62e008]      ; the java.lang.Native handle field id
//! 47c558 mov  ecx,[esp+0x10]
//! 47c55c push eax
//! 47c55d push ecx
//! 47c55e call FUN_0042a9e0        ; receiver's handle field = the thread
//! 47c563 add  esp,0xc
//! 47c566 pop  esi
//! 47c567 pop  ecx
//! 47c568 ret                      ; V: no return cell
//! ```
//!
//! Three steps, in this order:
//!
//! 1. `FUN_0041f340(thread, jvm, 0, 2, name)` — the ctor `slr_vm::threads`
//!    ports as [`ThreadQueues::spawn`]: priority `0` is `arg2` (`+0x28`),
//!    flags `2` is `arg3` (`+0x2c`, [`CREATED`]), and the `String`
//!    parameter is `arg4` (the name copied into `+0x1c`). The ctor links the
//!    thread at the tail of the run queue `[jvm+0x18]`.
//! 2. `[thread+0x18] = receiver` — the thread keeps a back pointer to its
//!    script object. `Thread.start` (`0x47c570`) clears it once the run
//!    returns (`0x47c5e2`), together with the handle field.
//! 3. `FUN_0042a9e0(receiver, [0x62e008], thread)` — store the thread word
//!    into the receiver's *handle field* (`java.lang.Native.ptr`, field id
//!    `DAT_0062e008`), the same call the `java.io.File` natives make (see
//!    `slr-io`'s `IoEvent::SetHandle`). `Thread.start` reads it back with
//!    `FUN_0042ab50(receiver, [0x62e008])` (`0x47c58c`).
//!
//! Port split: [`ThreadQueues`] is the arena plus the two queues, so step 1
//! lands there. Steps 2 and 3 have no home in it — `ThreadState` is the
//! scheduler's view of a thread (no script-object slot) and the handle field
//! belongs to the script object — so [`ThreadHost`] carries both, keyed by
//! the words the natives see: [`ThreadHost::script_object`] is `+0x18`,
//! [`ThreadHost::object`] is the handle field.
//!
//! Name: the exe `strcpy`s the bytes at the parameter's payload word into
//! `thread+0x1c`. The port has no word -> text map yet (the string rows own
//! it), so [`ThreadHost::set_name_source`] is the hook: with no source
//! installed the copy has no bytes and the name stays empty, the same
//! convention the port's other "the exe reads a pointer the port does not
//! have" sites use (they read 0 instead of faulting).
//!
//! Row **T8** ports `java.lang.Thread.start` `()V` at [`THREAD_START`], the
//! exe's `LAB_0047c570` (it takes no parameters, so `FUN_0045d910` marshals
//! the receiver alone):
//!
//! ```text
//! 47c570 mov  ecx,[esp+4]         ; the CallInfo
//! 47c574 push esi
//! 47c575 lea  eax,[esp+8]
//! 47c579 push eax                 ; &(arg1 slot): where out[0] goes
//! 47c57a push ecx
//! 47c57b call FUN_0045d910        ; out[0] = the receiver
//! 47c580 mov  edx,[0x62e008]      ; the java.lang.Native handle field id
//! 47c586 mov  eax,[esp+0x10]      ; the receiver slot
//! 47c58a push edx
//! 47c58b push eax
//! 47c58c call FUN_0042ab50        ; thread = the receiver's handle field
//! 47c591 mov  esi,eax
//! 47c593 add  esp,0x10
//! 47c596 test esi,esi
//! 47c598 je   0x47c5fe            ; no thread: done
//! 47c59a mov  eax,[esi+0x2c]      ; flags
//! 47c59d mov  ecx,eax
//! 47c59f or   ecx,2
//! 47c5a2 je   0x47c5fe            ; dead branch: `flags|2` is never 0
//! 47c5a4 and  eax,0xfffffffd      ; clear bit 1 (CREATED)
//! 47c5a7 push 0
//! 47c5a9 mov  ecx,esi
//! 47c5ab mov  [esi+0x2c],eax
//! 47c5ae call FUN_0041f9f0        ; push 0 arguments on the frame
//! 47c5b3 mov  edx,[esp+8]         ; the receiver slot
//! 47c5b7 push 0x612cfc            ; "java.lang.Runnable"
//! 47c5bc push 0x612d10            ; "target"
//! 47c5c1 push edx
//! 47c5c2 call FUN_0042a690        ; target = the receiver's `target` field
//! 47c5c7 add  esp,0xc
//! 47c5ca push 0x612d18            ; "run"
//! 47c5cf push esi                 ; the thread whose pushed args to use
//! 47c5d0 mov  ecx,eax             ; ecx = the target object
//! 47c5d2 call FUN_00408a30        ; call by name (FUN_004207c0): target.run()
//! 47c5d7 test eax,eax
//! 47c5d9 je   0x47c5fe            ; the call failed: nothing is cleaned up
//! 47c5db mov  ecx,esi
//! 47c5dd call FUN_0041f780        ; die: notify + monitor unlink, set 0x80
//! 47c5e2 mov  [esi+0x18],0        ; thread+0x18 = 0
//! 47c5e9 mov  eax,[0x62e008]
//! 47c5ee mov  ecx,[esp+8]
//! 47c5f2 push 0
//! 47c5f4 push eax
//! 47c5f5 push ecx
//! 47c5f6 call FUN_0042a9e0        ; the receiver's handle field = 0
//! 47c5fb add  esp,0xc
//! 47c5fe pop  esi
//! 47c5ff ret                      ; V: no return cell
//! ```
//!
//! The thread comes out of the receiver's handle field (`FUN_0042ab50`,
//! [`ThreadHost::object`]) — the reverse of [`ThreadHost::init`]'s store — and
//! the rest is one long "on success" arm:
//!
//! 1. Bit 1 [`CREATED`] is cleared. The guard at `0x47c59f` is `or ecx,2` /
//!    `je`: `OR` with the non-zero constant `2` is never zero, so that branch
//!    is dead code and *every* thread the handle lookup found continues.
//!    (What makes a second `start` do nothing is not the flag word but the
//!    success path's cleared handle field, `0x47c5f6`; see `CREATED`.)
//! 2. `FUN_0041f9f0(thread, 0)` pushes an empty argument container on the
//!    thread's current frame — what makes the `run` call take no arguments.
//!    [`ThreadHost`] keeps no frame list (frames are the interpreter's), so
//!    this step is the [`RunSource`] hook's empty argument list.
//! 3. `FUN_0042a690(receiver, "target", "java.lang.Runnable")` reads the
//!    receiver's `target` field, `0` when the field lookup fails or the value
//!    is not a `Runnable` (it logs both cases). That field read is the
//!    [`TargetSource`] hook.
//! 4. `FUN_00408a30(target, thread, "run")` calls `run` by name on the target
//!    (`FUN_004207c0`, with the thread's pushed frame as the argument list).
//!    Its `eax` decides the cleanup, so the [`RunSource`] hook's `bool` is
//!    that `eax != 0` — the [`StartOutcome::RunFailed`] path when it is zero.
//! 5. On success `FUN_0041f780(thread)` marks the thread dead and unlinks it
//!    from its monitor ([`ThreadQueues::die`] reports the monitor object for
//!    S2's `Monitor::unlink`), `+0x18` is cleared (the thread lets its script
//!    object go) and the receiver's handle field is stored as 0.
//!
//! The exe dereferences the target word without a null test, so a missing
//! `target` faults there; the port reads it as "the call cannot be made" and
//! reports the failed call, its usual stand-in for a pointer it does not
//! have.
//!
//! Row **T9** ports `java.lang.Thread.stop` `()V` at [`THREAD_STOP`], the
//! exe's `LAB_0047c600` (no parameters, so `FUN_0045d910` marshals the
//! receiver alone, exactly like `start`):
//!
//! ```text
//! 47c600 mov  ecx,[esp+4]         ; the CallInfo
//! 47c604 push esi
//! 47c605 lea  eax,[esp+8]
//! 47c609 push eax                 ; &(arg1 slot): where out[0] goes
//! 47c60a push ecx
//! 47c60b call FUN_0045d910        ; out[0] = the receiver
//! 47c610 mov  edx,[0x62e008]      ; the java.lang.Native handle field id
//! 47c616 mov  eax,[esp+0x10]      ; the receiver slot
//! 47c61a push edx
//! 47c61b push eax
//! 47c61c call FUN_0042ab50        ; thread = the receiver's handle field
//! 47c621 mov  esi,eax
//! 47c623 add  esp,0x10
//! 47c626 test esi,esi
//! 47c628 je   0x47c64e            ; no thread: done
//! 47c62a mov  ecx,esi
//! 47c62c call FUN_0041f780        ; die: notify + monitor unlink, set 0x80
//! 47c631 mov  dword ptr [esi+0x18],0   ; thread+0x18 = 0
//! 47c638 mov  ecx,[0x62e008]
//! 47c63e mov  edx,[esp+8]         ; the receiver slot
//! 47c642 push 0
//! 47c644 push ecx
//! 47c645 push edx
//! 47c646 call FUN_0042a9e0        ; the receiver's handle field = 0
//! 47c64b add  esp,0xc
//! 47c64e pop  esi
//! 47c64f ret                      ; V: no return cell
//! ```
//!
//! `stop` is `start`'s success cleanup (`0x47c5db`–`0x47c5fb`) without the
//! `run` call in front of it: the thread comes out of the receiver's handle
//! field ([`ThreadHost::object`]), [`ThreadQueues::die`] kills it — a waiting
//! thread is notified back onto the run queue and its monitor object reported
//! for S2's `Monitor::unlink` — then `+0x18` and the handle field are cleared
//! ([`ThreadHost::stop`], [`StopOutcome`]). Nothing else changes: no flag bit
//! is cleared first (the `CREATED` clear is `start`'s alone) and no `run` is
//! called.
//!
//! All thirteen rows have ports after this slice. The `java.lang.Thread` ten
//! are `init`, `start`, `stop`, `sleep`, `suspend`, `resume`, `setDaemon`,
//! `isAlive`, `getPriority` and `setPriority`; the `java.lang.Object` trio is
//! `wait`, `notify`, `notifyAll` (below). No row is a no-op stub: every ported
//! row's body is machine-read (see the per-native sections and the git-ignored
//! `re/out/thread/probe_natives_t10.py` / `probe_object_monitor.py`).
//!
//! `sleep` is the one native that does **not** use its receiver: the exe
//! loads `[CallInfo+4]` (the calling thread) at `0x47c66a` and sleeps that
//! thread, taking the duration from `out[1]`:
//!
//! ```text
//! 47c650 push ecx; push esi
//! 47c652 mov  esi,[esp+0xc]       ; the CallInfo
//! 47c656 lea  eax,[esp+0xc]       ; &out[1] (the F slot)
//! 47c65b lea  ecx,[esp+8]         ; &out[0] (the receiver slot)
//! 47c660 push esi
//! 47c661 call FUN_0045d910        ; marshal
//! 47c666 mov  edx,[esp+0x18]      ; edx = out[1]: the duration
//! 47c66a mov  ecx,[esi+4]         ; ecx = the CALLING thread
//! 47c670 push edx
//! 47c671 call FUN_0041f630        ; sleep(thread, duration)
//! ```
//!
//! `[oracle]` `re/out/thread/probe_natives_t10.py` drives all ten natives
//! with a hand-built `CallInfo` (container `[param, receiver]`, the handle
//! lookup `FUN_0042ab50` hooked to a synthetic thread, clock 5.0):
//! `isAlive` -> `eax` 0 even with a live thread; `getPriority` -> 7
//! (`[thread+0x28]`); `setPriority(I)` with `out[1]` = 0 -> `+0x28` = 0;
//! `setDaemon(I)` with `out[1]` = 0 and a non-zero receiver -> bit `0x100`
//! still clear (the parameter is read, not the receiver); `sleep(F)` with
//! `out[1]` = 2.5 -> `+0x30` = 7.5 and flags |= 8 with **no** handle lookup;
//! `suspend` -> flags |= `0x20`; `resume` on a non-suspended thread ->
//! unchanged.
//!
//! The `java.lang.Object` trio share the object's wait container at
//! `object+0x14` (S2's `Monitor`, one per receiver word). `wait` takes the
//! **calling** thread from `[CallInfo+4]` (`0x47c930`) and appends it to that
//! container after `FUN_0041f650` has moved it to the notify queue; `notify`
//! and `notifyAll` take the object from the receiver `out[0]` only (`0x47c937`
//! / `0x47c97f`) and consult no handle field:
//!
//! ```text
//! 47c925 lea  eax,[esp+8]         ; &out[0] (the receiver/object)
//! 47c92b call FUN_0045d910        ; marshal (one cell: `wait` takes none)
//! 47c930 mov  ecx,[esi+4]         ; ecx = the CALLING thread
//! 47c937 mov  ecx,[esp+0xc]       ; ecx = the object word (out[0])
//! 47c93b call FUN_00408b20        ; wait(object, thread)
//! ```
//!
//! `[oracle]` `re/out/thread/probe_object_monitor.py` has two exe threads
//! wait on one object (container `[a, b]`, both flags |= `0x10`, both `+0x34`
//! = the object), then notifies: `b` — the newest — wakes (`0x10` clears, the
//! container keeps `[a]`), then `notifyAll` drains it and `a` clears too.
//! [`ThreadQueues::die`]'s monitor object is unlinked here
//! ([`ThreadHost::unlink_waiter`], the exe's `FUN_00408c50` half), so a
//! stopped waiter leaves no slot behind.
//!
//! `[oracle]` `re/out/thread/probe_fun_47c510.py` runs the exe's own native
//! with a hand-built `CallInfo` whose container carries two distinguishable
//! `L` cells (the parameter's payload at `b"param-name"`, the receiver's at
//! `b"recv-name"`): `thread+0x28` = 0, `thread+0x2c` = 2,
//! `thread+0x1c` = `"param-name"` (the *parameter* is the name),
//! `thread+0x18` = the *receiver* word, and the hooked `FUN_0042a9e0` is
//! called as `(receiver, [0x62e008], new thread)` — the receiver carries the
//! handle, exactly as [`ThreadHost::init`] does it. `eax` after the call is
//! the word the native happened to leave there (it sets none; the signature
//! is `V`).
//!
//! `[oracle]` `start` has no probe of its own: its two external steps read a
//! script object's `target` field and call `run` on it, which needs the
//! interpreter. Its unit tests drive those two steps through
//! [`ThreadHost::set_target_source`] / [`ThreadHost::set_run_source`] fakes
//! instead, including the failure path (a `run` that reports `eax == 0`).

use std::cell::RefCell;
use std::collections::HashMap;

use slr_vm::host::VmHost;
use slr_vm::monitor::{Monitor, MonitorFault};
use slr_vm::threads::{ThreadFault, ThreadId, ThreadQueues, ThreadState};
use slr_vm::values::Value;

use crate::seam::{self, LangHost, NativeBinding, NativeResult, NativeTable};

/// `java.lang.Thread.init` `(Ljava.lang.String;)V` — the exe's
/// `LAB_0047c510` (T7).
pub const THREAD_INIT: u32 = 0x0047_c510;
/// `java.lang.Thread.start` `()V` — the exe's `LAB_0047c570` (T8).
pub const THREAD_START: u32 = 0x0047_c570;
/// `java.lang.Thread.stop` `()V` (`LAB_0047c600`).
pub const THREAD_STOP: u32 = 0x0047_c600;
/// `java.lang.Thread.sleep` `(F)V` (`LAB_0047c650`).
pub const THREAD_SLEEP: u32 = 0x0047_c650;
/// `java.lang.Thread.suspend` `()V` (`LAB_0047c680`).
pub const THREAD_SUSPEND: u32 = 0x0047_c680;
/// `java.lang.Thread.resume` `()V` (`LAB_0047c6b0`).
pub const THREAD_RESUME: u32 = 0x0047_c6b0;
/// `java.lang.Thread.setDaemon` `(I)V` (`FUN_0047c6e0`).
pub const THREAD_SET_DAEMON: u32 = 0x0047_c6e0;
/// `java.lang.Thread.isAlive` `()I` (`LAB_0047c730`).
pub const THREAD_IS_ALIVE: u32 = 0x0047_c730;
/// `java.lang.Thread.getPriority` `()I` (`LAB_0047c760`).
pub const THREAD_GET_PRIORITY: u32 = 0x0047_c760;
/// `java.lang.Thread.setPriority` `(I)V` (`FUN_0047c790`).
pub const THREAD_SET_PRIORITY: u32 = 0x0047_c790;
/// `java.lang.Object.wait` `()V` (`FUN_0047c920`).
pub const OBJECT_WAIT: u32 = 0x0047_c920;
/// `java.lang.Object.notify` `()V` (`LAB_0047c950`).
pub const OBJECT_NOTIFY: u32 = 0x0047_c950;
/// `java.lang.Object.notifyAll` `()V` (`LAB_0047c970`).
pub const OBJECT_NOTIFY_ALL: u32 = 0x0047_c970;

/// `thread+0x2c` bit 1: created by [`THREAD_INIT`], not started yet.
///
/// The ctor is handed it as `arg3`; `Thread.start` clears it again
/// (`and eax,0xfffffffd` at `0x47c5a4`). The guard in front of that clear —
/// `or ecx,2` / `je 0x47c5fe` at `0x47c59f` — is dead code: `OR` with the
/// non-zero constant `2` can never produce zero, so the branch is never
/// taken and the clear is unconditional (an original quirk, recorded as
/// behaviour). What stops a second `start` from running a finished thread
/// again is the success path's *handle field*, which it stores as 0
/// (`0x47c5f6`): the second lookup finds no thread and exits at `0x47c598`.
pub const CREATED: u32 = 0x02;

/// `thread+0x28` for a fresh thread: the ctor's `arg2`, which
/// `Thread.<init>` pushes as `0`.
pub const INIT_PRIORITY: i32 = 0;

/// The 13 `java.lang.Thread` / `java.lang.Object` rows of the exe's
/// registrar (`FUN_00416b00` calls, `native_api.tsv` lines 138..150).
///
/// Declared into the seam by [`install`] so an address whose row has not
/// landed reports [`crate::seam::DispatchError::NotPorted`], never
/// [`crate::seam::DispatchError::NoImplementation`] (a mistyped address). The class is
/// spelled as the exe's own registry text spells it.
pub const THREAD_NATIVE_BINDINGS: &[NativeBinding] = &[
    NativeBinding {
        class: "java.lang.Thread",
        name: "init",
        sig: "(Ljava.lang.String;)V",
        impl_addr: THREAD_INIT,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "start",
        sig: "()V",
        impl_addr: THREAD_START,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "stop",
        sig: "()V",
        impl_addr: THREAD_STOP,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "isAlive",
        sig: "()I",
        impl_addr: THREAD_IS_ALIVE,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "getPriority",
        sig: "()I",
        impl_addr: THREAD_GET_PRIORITY,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "setPriority",
        sig: "(I)V",
        impl_addr: THREAD_SET_PRIORITY,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "sleep",
        sig: "(F)V",
        impl_addr: THREAD_SLEEP,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "suspend",
        sig: "()V",
        impl_addr: THREAD_SUSPEND,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "resume",
        sig: "()V",
        impl_addr: THREAD_RESUME,
    },
    NativeBinding {
        class: "java.lang.Thread",
        name: "setDaemon",
        sig: "(I)V",
        impl_addr: THREAD_SET_DAEMON,
    },
    NativeBinding {
        class: "java.lang.Object",
        name: "wait",
        sig: "()V",
        impl_addr: OBJECT_WAIT,
    },
    NativeBinding {
        class: "java.lang.Object",
        name: "notify",
        sig: "()V",
        impl_addr: OBJECT_NOTIFY,
    },
    NativeBinding {
        class: "java.lang.Object",
        name: "notifyAll",
        sig: "()V",
        impl_addr: OBJECT_NOTIFY_ALL,
    },
];

/// The port-side state of one script `java.lang.Thread` object: its handle
/// field (`java.lang.Native.ptr`, the exe's `DAT_0062e008`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadObject {
    /// The word stored by `FUN_0042a9e0(object, DAT_0062e008, value)`:
    /// `Some` thread for a live handle, `None` for the cleared field
    /// (`Thread.start`'s cleanup, `0x47c5ee`).
    pub handle: Option<ThreadId>,
}

/// The receiver's `target` field: `FUN_0042a690(receiver, "target",
/// "java.lang.Runnable")`, the object `Thread.start` calls `run` on. `0`
/// when the field lookup fails or the value is not a `Runnable` — the exe
/// logs both and returns 0.
///
/// The field lives on the script object, which the port models only through
/// [`ThreadHost::object`] (the handle field, the one word a thread native
/// stores); the `target` word is written by the script's own `<init>`
/// bytecode, so the port has no writer for it and takes it from here.
pub type TargetSource = fn(u32) -> u32;

/// The `run` invocation behind `Thread.start`: `FUN_0041f9f0(thread, 0)`
/// (an empty argument container on the thread's frame) and
/// `FUN_00408a30(target, thread, "run")` (`FUN_004207c0`, call by name).
///
/// The arguments are the thread's pushed frame, which is empty, so the hook
/// takes none; `true` is the call's non-zero `eax`, the success
/// [`ThreadHost::start`] tests before it cleans up.
pub type RunSource = fn(u32, ThreadId) -> bool;

/// What [`ThreadHost::start`] did, i.e. how far `LAB_0047c570` got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    /// `FUN_0042ab50` found no thread in the receiver's handle field
    /// (`0x47c598`): nothing changed. Includes a second `start` of a thread
    /// whose success path already cleared that field.
    NoThread,
    /// The `run` call's `eax` was 0 (`0x47c5d9`): the flag bit was cleared,
    /// nothing else.
    RunFailed,
    /// The `run` call succeeded: `FUN_0041f780` marked the thread dead,
    /// `+0x18` and the handle field are cleared.
    Finished {
        /// The monitor object the exe's die scanner (`FUN_00408c50`) unlinks
        /// the thread from — S2's `Monitor::unlink`, which owns the wait
        /// container. `None` when the thread was not waiting
        /// ([`ThreadQueues::die`]'s own report).
        monitor: Option<u32>,
    },
}

/// What [`ThreadHost::stop`] did, i.e. how far `LAB_0047c600` got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    /// `FUN_0042ab50` found no thread in the receiver's handle field
    /// (`0x47c628`): nothing changed. Includes a `stop` after a `start` or
    /// `stop` already cleared that field.
    NoThread,
    /// The thread was killed: `FUN_0041f780` marked it dead (a waiting
    /// thread was notified back onto the run queue first), `+0x18` and the
    /// receiver's handle field were cleared.
    Stopped {
        /// The monitor object the exe's die scanner (`FUN_00408c50`) unlinks
        /// the thread from — S2's `Monitor::unlink`, which owns the wait
        /// container. `None` when the thread was not waiting
        /// ([`ThreadQueues::die`]'s own report).
        monitor: Option<u32>,
    },
}

/// Why a thread/monitor native could not do its work.
///
/// Two sources: the monitor half's own failures ([`MonitorFault`] — the exe's
/// four fatal precondition messages plus the port-only empty-notify case) and
/// the port-only missing calling thread. The exe logs through `FUN_004190d0`
/// and keeps running with a half-updated object; the port stops instead,
/// leaves the state untouched and [`ThreadHost::last_fault`] carries the
/// message the exe would have printed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadHostFault {
    /// The monitor or thread half failed; the state is untouched.
    Monitor(MonitorFault),
    /// Port-only: the native needs the *calling* thread — `Thread.sleep`
    /// and `Object.wait` read `[CallInfo+4]` (`0x47c66a`, `0x47c930`), which
    /// the port has no equivalent of (see [`ThreadHost::set_current`]) — and
    /// none is installed. The exe always has one, so this has no exe text.
    MissingCurrentThread,
}

impl ThreadHostFault {
    /// The exe's diagnostic text for this failure, or the port's own note
    /// for a missing calling thread.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::Monitor(fault) => fault.message(),
            Self::MissingCurrentThread => "Thread: no calling thread installed",
        }
    }
}

impl From<MonitorFault> for ThreadHostFault {
    fn from(fault: MonitorFault) -> Self {
        Self::Monitor(fault)
    }
}

impl From<ThreadFault> for ThreadHostFault {
    fn from(fault: ThreadFault) -> Self {
        Self::Monitor(MonitorFault::from(fault))
    }
}

/// The thread natives' runtime: the JVM's thread bookkeeping (M1-05 S1) plus
/// the object-model words a thread native stores, the calling thread the
/// `Object.wait`/`Thread.sleep` pair reads and the seams `Thread.start` needs
/// ([`TargetSource`], [`RunSource`]).
///
/// The exe keeps all of it on the JVM: `[jvm+0x18]`/`[jvm+0x1c]` are the two
/// queues, `[thread+0x18]` is the script object and `DAT_0062e008` is the
/// handle field of the receiver. This struct is that state, per VM.
///
/// Two things the exe reads out of the `CallInfo` have no port counterpart:
/// the *calling* thread (`[CallInfo+4]`) and, by extension, the JVM object.
/// slr-vm's `CallInfo::thread` is always 0 (it is the exe's thread pointer
/// word and the port's [`ThreadId`] is an arena index, so there is no
/// word -> id link), so the harness installs the calling thread with
/// [`Self::set_current`]; a native that needs it reports
/// [`ThreadHostFault::MissingCurrentThread`] when none is installed.
pub struct ThreadHost {
    queues: ThreadQueues,
    monitors: HashMap<u32, Monitor>,
    scripts: HashMap<ThreadId, u32>,
    objects: HashMap<u32, ThreadObject>,
    current: Option<ThreadId>,
    last_fault: Option<ThreadHostFault>,
    name_source: Option<fn(u32) -> String>,
    target_source: Option<TargetSource>,
    run_source: Option<RunSource>,
}

impl ThreadHost {
    /// No threads, both queues empty, no source installed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queues: ThreadQueues::new(),
            monitors: HashMap::new(),
            scripts: HashMap::new(),
            objects: HashMap::new(),
            current: None,
            last_fault: None,
            name_source: None,
            target_source: None,
            run_source: None,
        }
    }

    /// Install the calling thread — the port's stand-in for the exe's
    /// `[CallInfo+4]`, which `Thread.sleep` and `Object.wait` read inside
    /// their `FUN_0041f630`/`FUN_00408b20` calls. `None` clears it.
    pub fn set_current(&mut self, id: Option<ThreadId>) {
        self.current = id;
    }

    /// The calling thread (`[CallInfo+4]`), when one is installed.
    #[must_use]
    pub fn current(&self) -> Option<ThreadId> {
        self.current
    }

    /// The fault the last fault-reporting native hit (`None` after a clean
    /// call). The exe prints through `FUN_004190d0`; the port records.
    #[must_use]
    pub fn last_fault(&self) -> Option<ThreadHostFault> {
        self.last_fault
    }

    /// Record the outcome of the native that just ran (`None` clears it).
    pub fn set_last_fault(&mut self, fault: Option<ThreadHostFault>) {
        self.last_fault = fault;
    }

    /// Install the word -> text source for thread names (see the module
    /// docs): the exe copies the C string at the `L` parameter's payload.
    pub fn set_name_source(&mut self, source: fn(u32) -> String) {
        self.name_source = Some(source);
    }

    /// Install the receiver -> `target` field source `Thread.start` reads
    /// ([`TargetSource`]): with no source installed the read has no word and
    /// the call cannot be made.
    pub fn set_target_source(&mut self, source: TargetSource) {
        self.target_source = Some(source);
    }

    /// Install the `run` call `Thread.start` makes ([`RunSource`]): with no
    /// source installed the call cannot be made and the success cleanup
    /// never runs.
    pub fn set_run_source(&mut self, source: RunSource) {
        self.run_source = Some(source);
    }

    /// The arena and the two queues (M1-05 S1).
    #[must_use]
    pub fn queues(&self) -> &ThreadQueues {
        &self.queues
    }

    /// The mutable arena: tests seed a thread's priority/flags to the words
    /// the oracle row started from, then compare what the native wrote.
    pub fn queues_mut(&mut self) -> &mut ThreadQueues {
        &mut self.queues
    }

    /// The script object's handle field (`DAT_0062e008`), defaulting to a
    /// cleared field for an object no `init` has touched (and for an object
    /// whose thread a successful `start` cleared, `0x47c5f6`).
    #[must_use]
    pub fn object(&self, word: u32) -> ThreadObject {
        self.objects.get(&word).copied().unwrap_or_default()
    }

    /// `thread+0x18`: the script object the thread belongs to — `None` for a
    /// thread id this runtime did not hand out and for the word
    /// [`Self::start`]'s success path clears (`0x47c5e2`).
    #[must_use]
    pub fn script_object(&self, id: ThreadId) -> Option<u32> {
        self.scripts.get(&id).copied()
    }

    /// The bytes `FUN_0041f340` copies into `thread+0x1c`: the text of the
    /// `L` parameter's payload word, or the empty name when no source is
    /// installed for it (see the module docs).
    #[must_use]
    pub fn resolve_name(&self, word: u32) -> String {
        self.name_source
            .map_or_else(String::new, |source| source(word))
    }

    /// exe: LAB_0047c510
    ///
    /// `java.lang.Thread.init(String)`: build the thread object and link it
    /// at the run-queue tail (`FUN_0041f340`, priority [`INIT_PRIORITY`],
    /// flags [`CREATED`], named from `name_word`), record `receiver` in
    /// `thread+0x18` and store the thread in the receiver's handle field.
    ///
    /// `receiver` is the native's receiver slot and `name_word` the payload
    /// of its `L` parameter (0 when the container ran short, where the exe
    /// reads a stale slot).
    pub fn init(&mut self, receiver: u32, name_word: u32) -> ThreadId {
        let name = self.resolve_name(name_word);
        let id = self.queues.spawn(INIT_PRIORITY, CREATED, &name);
        self.scripts.insert(id, receiver);
        self.objects
            .insert(receiver, ThreadObject { handle: Some(id) });
        id
    }

    /// exe: LAB_0047c570
    ///
    /// `java.lang.Thread.start()`: look the thread up through `receiver`'s
    /// handle field (`FUN_0042ab50`), clear [`CREATED`], push an empty
    /// argument list and call `run` on the receiver's `target`; when that call
    /// succeeds the thread is killed (`FUN_0041f780`) and both object-model
    /// words are cleared. See the module docs for the machine code.
    ///
    /// `receiver` is the native's receiver slot (0 for a container that ran
    /// short, where the exe reads a stale slot); a receiver whose handle field
    /// holds no thread is a no-op ([`StartOutcome::NoThread`]). The `target`
    /// read and the `run` call are the [`TargetSource`] / [`RunSource`] seams;
    /// a missing target or a missing `run` source is a call that cannot be
    /// made, which the exe would fault on and the port reports as
    /// [`StartOutcome::RunFailed`].
    pub fn start(&mut self, receiver: u32) -> StartOutcome {
        let Some(id) = self.objects.get(&receiver).and_then(|object| object.handle) else {
            return StartOutcome::NoThread;
        };
        // The exe's `or ecx,2` / `je` guard is dead code; the clear is
        // unconditional (see `CREATED`).
        self.queues.get_mut(id).flags &= !CREATED;
        let target = self.target_source.map_or(0, |source| source(receiver));
        let ran = target != 0 && self.run_source.is_some_and(|run| run(target, id));
        if !ran {
            return StartOutcome::RunFailed;
        }
        let monitor = self.queues.die(id);
        if let Some(object) = monitor {
            self.unlink_waiter(object, id);
        }
        self.scripts.remove(&id);
        self.objects.insert(receiver, ThreadObject { handle: None });
        StartOutcome::Finished { monitor }
    }

    /// exe: LAB_0047c600
    ///
    /// `java.lang.Thread.stop()`: look the thread up through `receiver`'s
    /// handle field (`FUN_0042ab50`), kill it (`FUN_0041f780`) and clear both
    /// object-model words. See the module docs for the machine code.
    ///
    /// This is [`Self::start`]'s success cleanup without the `run` call: no
    /// flag bit is cleared first (the `CREATED` clear is `start`'s alone).
    /// `receiver` is the native's receiver slot (0 for a container that ran
    /// short, where the exe reads a stale slot); a receiver whose handle field
    /// holds no thread is a no-op ([`StopOutcome::NoThread`]).
    pub fn stop(&mut self, receiver: u32) -> StopOutcome {
        let Some(id) = self.objects.get(&receiver).and_then(|object| object.handle) else {
            return StopOutcome::NoThread;
        };
        let monitor = self.queues.die(id);
        if let Some(object) = monitor {
            self.unlink_waiter(object, id);
        }
        self.scripts.remove(&id);
        self.objects.insert(receiver, ThreadObject { handle: None });
        StopOutcome::Stopped { monitor }
    }

    /// The thread in `receiver`'s handle field, `FUN_0042ab50(receiver,
    /// [0x62e008])`: `None` for a receiver no `init` has touched and for one
    /// whose handle a successful `start`/`stop` cleared.
    fn handle(&self, receiver: u32) -> Option<ThreadId> {
        self.objects.get(&receiver).and_then(|object| object.handle)
    }

    /// The calling thread's id, when [`Self::set_current`] installed one and
    /// it is a thread this runtime handed out.
    fn current_id(&self) -> Option<ThreadId> {
        let id = self.current?;
        (id < self.queues.threads().len()).then_some(id)
    }

    /// The calling thread's state (see [`Self::current_id`]).
    fn current_thread(&mut self) -> Option<&mut ThreadState> {
        let id = self.current_id()?;
        Some(self.queues.get_mut(id))
    }

    /// exe: LAB_0047c730
    ///
    /// `java.lang.Thread.isAlive()I`: the exe looks the thread up through
    /// `FUN_0042ab50` and then returns 0 unconditionally (`xor eax,eax` at
    /// `0x47c753`). That is an original quirk of build 606 — a live thread
    /// reports dead — recorded as behaviour, not a bug fix and not an ADR
    /// 0007 divergence. The lookup has no side effect, so `false` it is.
    #[must_use]
    pub fn is_alive(&self, receiver: u32) -> bool {
        self.handle(receiver);
        false
    }

    /// exe: LAB_0047c760
    ///
    /// `java.lang.Thread.getPriority()I`: `[thread+0x28]`, or 0 when the
    /// handle lookup finds no thread.
    #[must_use]
    pub fn get_priority(&self, receiver: u32) -> i32 {
        self.handle(receiver)
            .map_or(0, |id| self.queues.get(id).priority)
    }

    /// exe: FUN_0047c790
    ///
    /// `java.lang.Thread.setPriority(I)V`: store the argument in
    /// `[thread+0x28]`. Nothing is touched when the handle lookup finds no
    /// thread.
    pub fn set_priority(&mut self, receiver: u32, priority: i32) {
        if let Some(id) = self.handle(receiver) {
            self.queues.get_mut(id).priority = priority;
        }
    }

    /// exe: FUN_0047c6e0
    ///
    /// `java.lang.Thread.setDaemon(I)V`: a non-zero argument sets bit `0x100`,
    /// zero clears it, nothing else changes. Nothing is touched when the
    /// handle lookup finds no thread.
    pub fn set_daemon(&mut self, receiver: u32, on: bool) {
        if let Some(id) = self.handle(receiver) {
            self.queues.get_mut(id).set_daemon(on);
        }
    }

    /// exe: FUN_0047c650 -> FUN_0041f630
    ///
    /// `java.lang.Thread.sleep(F)V`: sleep the **calling** thread — the exe
    /// loads `[CallInfo+4]` at `0x47c66a` and ignores its receiver — setting
    /// `[thread+0x30] = clock() + duration` and bit 3. `now` is the port's
    /// `clock()` reading ([`LangHost::read_clock`], the exe's `FUN_005516c0`
    /// call inside `FUN_0041f630`); a second sleep overwrites the deadline.
    ///
    /// No calling thread installed is [`ThreadHostFault::MissingCurrentThread`]
    /// and nothing changes.
    pub fn sleep(&mut self, now: f32, duration: f32) -> Result<(), ThreadHostFault> {
        let thread = self
            .current_thread()
            .ok_or(ThreadHostFault::MissingCurrentThread)?;
        thread.sleep(now, duration);
        Ok(())
    }

    /// exe: FUN_0047c680 -> FUN_0041f570
    ///
    /// `java.lang.Thread.suspend()V`: set bit `0x20`; unless the thread is
    /// waiting, move it from the run queue to the notify queue. A thread that
    /// is already suspended (or waiting) is left exactly where it is. Nothing
    /// is touched when the handle lookup finds no thread.
    pub fn suspend(&mut self, receiver: u32) {
        if let Some(id) = self.handle(receiver) {
            self.queues.suspend(id);
        }
    }

    /// exe: FUN_0047c6b0 -> FUN_0041f5d0
    ///
    /// `java.lang.Thread.resume()V`: clear bit `0x20`; unless the thread is
    /// waiting, move it back to the tail of the run queue. A thread that is
    /// not suspended is left alone. Nothing is touched when the handle
    /// lookup finds no thread.
    pub fn resume(&mut self, receiver: u32) {
        if let Some(id) = self.handle(receiver) {
            self.queues.resume(id);
        }
    }

    /// The waiters on `object`'s monitor (`object+0x14`), in wait order.
    #[must_use]
    pub fn waiters(&self, object: u32) -> Vec<ThreadId> {
        self.monitors
            .get(&object)
            .map_or_else(Vec::new, Monitor::waiters)
    }

    /// exe: FUN_00408c50 (the die scanner's container half)
    ///
    /// Zero the dying thread's slot in `object`'s wait container — the count
    /// is not changed and the exe logs
    /// `Instance::notify(Thread*): inconsistency` when the thread is absent
    /// (the port has no wire for that log, so a false return is dropped).
    /// [`ThreadQueues::die`] performed the notify half already.
    fn unlink_waiter(&mut self, object: u32, id: ThreadId) {
        if let Some(monitor) = self.monitors.get_mut(&object) {
            monitor.unlink(id);
        }
    }

    /// exe: FUN_0047c920
    ///
    /// The native body is `FUN_0047c920`, which forwards to the container
    /// move `FUN_00408b20` (thread half `FUN_0041f650`).
    ///
    /// `java.lang.Object.wait()V`: move the **calling** thread
    /// (`[CallInfo+4]`, `0x47c930` — the native ignores the object's handle
    /// field entirely) onto `receiver`'s wait container: unlink it from the
    /// run queue, set bit `0x10`, append it to the notify queue, record
    /// `receiver` in `+0x34` and append it to the container. A dead thread or
    /// one that is already waiting is a fault and leaves both halves
    /// untouched. No calling thread installed is
    /// [`ThreadHostFault::MissingCurrentThread`].
    pub fn wait(&mut self, receiver: u32) -> Result<(), ThreadHostFault> {
        let id = self
            .current_id()
            .ok_or(ThreadHostFault::MissingCurrentThread)?;
        let monitor = self.monitors.entry(receiver).or_default();
        monitor.wait(&mut self.queues, id, receiver)?;
        Ok(())
    }

    /// exe: LAB_0047c950
    ///
    /// The native body forwards to the container wake `FUN_00408bc0`.
    ///
    /// `java.lang.Object.notify()V`: wake the newest waiter on `receiver`'s
    /// container — pop the last non-zero slot (LIFO, `[oracle]`
    /// `re/out/thread/probe_object_monitor.py`: two waits `a`, `b` wake `b`
    /// first) and clear its bit `0x10` (`FUN_0041f6f0`); a suspended waiter
    /// keeps `0x20` and stays queued for a later resume. An empty container
    /// is [`MonitorFault::EmptyNotify`]: the exe returns silently, the port
    /// surfaces the lost notification.
    pub fn notify(&mut self, receiver: u32) -> Result<ThreadId, ThreadHostFault> {
        let monitor = self.monitors.entry(receiver).or_default();
        monitor.notify(&mut self.queues).map_err(Into::into)
    }

    /// exe: LAB_0047c970
    ///
    /// The native body forwards to the container drain `FUN_00408ce0`.
    ///
    /// `java.lang.Object.notifyAll()V`: wake every waiter on `receiver`'s
    /// container, newest first, until it is empty (`[oracle]`: the drain
    /// leaves the flags of both waiters clear). Notifying nobody is this
    /// native's normal outcome and is not a fault.
    pub fn notify_all(&mut self, receiver: u32) -> Result<Vec<ThreadId>, ThreadHostFault> {
        let monitor = self.monitors.entry(receiver).or_default();
        monitor.notify_all(&mut self.queues).map_err(Into::into)
    }
}

impl Default for ThreadHost {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ThreadHost {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ThreadHost")
            .field("threads", &self.queues.threads().len())
            .field("run_queue", &self.queues.run_order())
            .field("notify_queue", &self.queues.notify_order())
            .field("scripts", &self.scripts.len())
            .field("objects", &self.objects.len())
            .field("monitors", &self.monitors.len())
            .field("current", &self.current)
            .field("last_fault", &self.last_fault)
            .finish()
    }
}

thread_local! {
    /// The per-thread thread runtime (the exe's JVM thread state lives on
    /// the one JVM; the port mirrors the seam's per-thread data block).
    static THREADS: RefCell<ThreadHost> = RefCell::new(ThreadHost::new());
}

/// Run `f` over this thread's [`ThreadHost`].
pub fn with_threads<R>(f: impl FnOnce(&mut ThreadHost) -> R) -> R {
    THREADS.with(|cell| f(&mut cell.borrow_mut()))
}

/// Reset this thread's [`ThreadHost`] to a fresh one.
pub fn reset() {
    THREADS.with(|cell| *cell.borrow_mut() = ThreadHost::new());
}

/// exe: LAB_0047c510 (as a `LangNative`)
///
/// The seam hands the already-marshalled container: `out[0]` is the receiver
/// (`args[count-1]`, as `FUN_0045d910` writes it) and `out[1]` the `L`
/// parameter (`args[count-2]`). A container too short for either reads 0,
/// the port's stand-in for the exe's stale stack slot.
pub fn thread_init(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = args.last().map_or(0, |value| value.payload);
    let name_word = args
        .len()
        .checked_sub(2)
        .map_or(0, |index| args[index].payload);
    with_threads(|threads| threads.init(receiver, name_word));
    None
}

/// exe: LAB_0047c570 (as a `LangNative`)
///
/// The seam hands the already-marshalled container: `start` takes no
/// parameters, so `out[0]` is the receiver alone (`args[count-1]`, as
/// `FUN_0045d910` writes it) and there is no parameter slot. A container too
/// short for it reads 0, the port's stand-in for the exe's stale stack slot.
///
/// The outcome is [`ThreadHost::start`]'s; the native itself is `V`, so it
/// returns no cell.
pub fn thread_start(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = args.last().map_or(0, |value| value.payload);
    with_threads(|threads| {
        threads.start(receiver);
    });
    None
}

/// exe: LAB_0047c600 (as a `LangNative`)
///
/// `stop` takes no parameters, so the seam hands the receiver alone
/// (`out[0]` = `args[count-1]`, as `FUN_0045d910` writes it) and there is no
/// parameter slot. A container too short for it reads 0, the port's stand-in
/// for the exe's stale stack slot.
///
/// The outcome is [`ThreadHost::stop`]'s; the native itself is `V`, so it
/// returns no cell.
pub fn thread_stop(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = args.last().map_or(0, |value| value.payload);
    with_threads(|threads| {
        threads.stop(receiver);
    });
    None
}

/// The container's last cell (`out[0]`, the receiver): 0 when the container
/// ran short, the port's stand-in for the exe's stale stack slot (the exe
/// reads `[esp+0xc]`/`[esp+0x14]` regardless of `count`).
fn receiver_of(args: &[Value]) -> u32 {
    args.last().map_or(0, |value| value.payload)
}

/// The `n`-th declared parameter (`out[n]`): `out[1]` is the container's
/// second-to-last cell, `out[2]` the third-to-last. 0 when the container ran
/// short (again the exe's stale slot).
fn param_of(args: &[Value], n: usize) -> u32 {
    args.len()
        .checked_sub(1 + n)
        .map_or(0, |index| args[index].payload)
}

/// exe: FUN_0047c650 (as a `LangNative`)
///
/// `java.lang.Thread.sleep(F)V`: sleep the calling thread for `out[1]`
/// seconds from the current clock. The receiver `out[0]` is marshalled by
/// the exe but never read (`0x47c66a` loads `[CallInfo+4]` instead); the port
/// takes the calling thread from [`ThreadHost::set_current`]. No calling
/// thread or a container that ran short for the `F` reads 0 either way.
pub fn thread_sleep(host: &mut LangHost, args: &[Value]) -> NativeResult {
    let duration = f32::from_bits(param_of(args, 1));
    let now = host.read_clock();
    with_threads(|threads| {
        let outcome = threads.sleep(now, duration);
        threads.set_last_fault(outcome.err());
    });
    None
}

/// exe: FUN_0047c680 (as a `LangNative`)
///
/// `java.lang.Thread.suspend()V`: the receiver's handle field names the
/// thread and `FUN_0041f570` suspends it.
pub fn thread_suspend(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    with_threads(|threads| threads.suspend(receiver));
    None
}

/// exe: FUN_0047c6b0 (as a `LangNative`)
///
/// `java.lang.Thread.resume()V`: the receiver's handle field names the
/// thread and `FUN_0041f5d0` resumes it.
pub fn thread_resume(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    with_threads(|threads| threads.resume(receiver));
    None
}

/// exe: FUN_0047c6e0 (as a `LangNative`)
///
/// `java.lang.Thread.setDaemon(I)V`: `out[1]` non-zero sets bit `0x100`, zero
/// clears it, on the thread the receiver's handle field names.
pub fn thread_set_daemon(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    let on = param_of(args, 1) != 0;
    with_threads(|threads| threads.set_daemon(receiver, on));
    None
}

/// exe: LAB_0047c730 (as a `LangNative`)
///
/// `java.lang.Thread.isAlive()I`: returns 0 unconditionally — the exe's own
/// quirk (`xor eax,eax` at `0x47c753`), recorded as behaviour, not a bug fix.
pub fn thread_is_alive(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    let alive = with_threads(|threads| threads.is_alive(receiver));
    Some(u32::from(alive))
}

/// exe: LAB_0047c760 (as a `LangNative`)
///
/// `java.lang.Thread.getPriority()I`: `[thread+0x28]` for the thread the
/// receiver's handle field names, 0 when it names none.
pub fn thread_get_priority(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    let priority = with_threads(|threads| threads.get_priority(receiver));
    Some(priority as u32)
}

/// exe: FUN_0047c790 (as a `LangNative`)
///
/// `java.lang.Thread.setPriority(I)V`: store `out[1]` in `[thread+0x28]` for
/// the thread the receiver's handle field names.
pub fn thread_set_priority(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    let priority = param_of(args, 1) as i32;
    with_threads(|threads| threads.set_priority(receiver, priority));
    None
}

/// exe: FUN_0047c920
///
/// `java.lang.Object.wait()V`: the receiver `out[0]` is the object whose
/// `+0x14` container takes the **calling** thread (`[CallInfo+4]`,
/// `0x47c930`); the object's own handle field is not consulted.
pub fn object_wait(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    with_threads(|threads| {
        let outcome = threads.wait(receiver);
        threads.set_last_fault(outcome.err());
    });
    None
}

/// exe: LAB_0047c950
///
/// `java.lang.Object.notify()V`: wake the newest waiter on the receiver's
/// `+0x14` container.
pub fn object_notify(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    with_threads(|threads| {
        let outcome = threads.notify(receiver);
        threads.set_last_fault(outcome.err());
    });
    None
}

/// exe: LAB_0047c970
///
/// `java.lang.Object.notifyAll()V`: drain the receiver's `+0x14` container.
pub fn object_notify_all(_host: &mut LangHost, args: &[Value]) -> NativeResult {
    let receiver = receiver_of(args);
    with_threads(|threads| {
        let outcome = threads.notify_all(receiver);
        threads.set_last_fault(outcome.err());
    });
    None
}

/// Declare the 13 rows and install all thirteen ports ([`THREAD_INIT`],
/// [`THREAD_START`], [`THREAD_STOP`], [`THREAD_SLEEP`], [`THREAD_SUSPEND`],
/// [`THREAD_RESUME`], [`THREAD_SET_DAEMON`], [`THREAD_IS_ALIVE`],
/// [`THREAD_GET_PRIORITY`], [`THREAD_SET_PRIORITY`], [`OBJECT_WAIT`],
/// [`OBJECT_NOTIFY`], [`OBJECT_NOTIFY_ALL`]) in this thread's seam table, then
/// wire the seam into `vm`: the rows go into `VmHost::natives` and the bridge
/// is installed at every ported address.
///
/// Idempotent (the exe's registrar overwrites a record and `declare` does
/// not duplicate a row).
pub fn install(vm: &mut VmHost) {
    // Build the ports in a local table, then let `seam::install` publish them
    // into this thread's seam table (which `bridge` dispatches through). The
    // borrow must not be held across the call: `seam::install` publishes
    // through `with_native_table` itself.
    let mut table = NativeTable::new();
    table.set(THREAD_INIT, thread_init);
    table.set(THREAD_START, thread_start);
    table.set(THREAD_STOP, thread_stop);
    table.set(THREAD_SLEEP, thread_sleep);
    table.set(THREAD_SUSPEND, thread_suspend);
    table.set(THREAD_RESUME, thread_resume);
    table.set(THREAD_SET_DAEMON, thread_set_daemon);
    table.set(THREAD_IS_ALIVE, thread_is_alive);
    table.set(THREAD_GET_PRIORITY, thread_get_priority);
    table.set(THREAD_SET_PRIORITY, thread_set_priority);
    table.set(OBJECT_WAIT, object_wait);
    table.set(OBJECT_NOTIFY, object_notify);
    table.set(OBJECT_NOTIFY_ALL, object_notify_all);
    seam::install(vm, &table);
}

/// `true` when `impl_addr` is one of this module's rows (`test/debug
/// convenience`).
#[must_use]
pub fn is_thread_address(impl_addr: u32) -> bool {
    THREAD_NATIVE_BINDINGS
        .iter()
        .any(|row| row.impl_addr == impl_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::DispatchError;
    use slr_vm::host::CallInfo;
    use slr_vm::interp::flags;
    use slr_vm::threads::{DAEMON, SLEEPING};
    use std::cell::Cell;

    /// The test name source: `w<hex>`. The port has no word -> text map for
    /// script `String`s yet (the string rows own it).
    fn test_name(word: u32) -> String {
        format!("w{word:x}")
    }

    /// A runtime with the test name source installed.
    fn fresh() -> ThreadHost {
        let mut host = ThreadHost::new();
        host.set_name_source(test_name);
        host
    }

    /// The word the fake `FUN_0042a690` reports as the receiver's `target`.
    const RUNNABLE: u32 = 0x3000_0001;

    thread_local! {
        /// What the fake target source returns (0 = no `target` field).
        static TARGET: Cell<u32> = const { Cell::new(RUNNABLE) };
        /// What the fake `run` reports as the call's `eax`.
        static RUN_RESULT: Cell<bool> = const { Cell::new(true) };
        /// Every `(target, thread)` the fake `run` was called with.
        static RUN_CALLS: RefCell<Vec<(u32, ThreadId)>> = const { RefCell::new(Vec::new()) };
    }

    /// The fake `FUN_0042a690(receiver, "target", "java.lang.Runnable")`.
    fn test_target(_receiver: u32) -> u32 {
        TARGET.with(Cell::get)
    }

    /// The fake `FUN_00408a30(target, thread, "run")` (`FUN_004207c0`).
    fn test_run(target: u32, thread: ThreadId) -> bool {
        RUN_CALLS.with(|calls| calls.borrow_mut().push((target, thread)));
        RUN_RESULT.with(Cell::get)
    }

    /// A runtime with the name source and both `start` seams installed, the
    /// fake call log cleared and the fakes reset to "the call succeeds".
    fn starter() -> ThreadHost {
        TARGET.with(|target| target.set(RUNNABLE));
        RUN_RESULT.with(|result| result.set(true));
        RUN_CALLS.with(|calls| calls.borrow_mut().clear());
        let mut host = fresh();
        host.set_target_source(test_target);
        host.set_run_source(test_run);
        host
    }

    /// Every `(target, thread)` the fake `run` saw.
    fn run_calls() -> Vec<(u32, ThreadId)> {
        RUN_CALLS.with(|calls| calls.borrow().clone())
    }

    /// `Thread.<init>` builds a created (flag 2), priority-0 thread named
    /// from the parameter, links it at the run-queue tail, and stores the
    /// two words: the script object in `+0x18`, the thread in the receiver's
    /// handle field.
    #[test]
    fn init_builds_and_links_the_thread() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);

        assert_eq!(id, 0);
        let thread = host.queues().get(id);
        assert_eq!(thread.priority, 0, "the ctor's arg2 (0x47c53d)");
        assert_eq!(thread.flags, CREATED, "the ctor's arg3 (0x47c53b)");
        assert_eq!(thread.name, "w20000001", "the `L` parameter (0x47c534)");
        assert_eq!(thread.sleep_deadline, 0.0);
        assert_eq!(thread.wait_object, 0);
        assert_eq!(host.queues().run_order(), vec![id]);
        assert_eq!(host.queues().run_queue().first(), Some(id));
        assert!(host.queues().notify_queue().is_empty());

        assert_eq!(host.script_object(id), Some(receiver), "thread+0x18");
        assert_eq!(
            host.object(receiver),
            ThreadObject { handle: Some(id) },
            "FUN_0042a9e0(receiver, DAT_0062e008, thread)"
        );
    }

    /// A second `init` appends to the run-queue tail, and a second `init` on
    /// the *same* receiver overwrites its handle field (the exe's field
    /// store is unconditional).
    #[test]
    fn init_appends_and_repoints_the_handle() {
        let mut host = fresh();
        let first = host.init(0x2000_0007, 0x2000_0001);
        let second = host.init(0x2000_0008, 0x2000_0002);
        assert_ne!(first, second);
        assert_eq!(host.queues().run_order(), vec![first, second]);
        assert_eq!(host.queues().run_queue().last(), Some(second));
        assert_eq!(host.script_object(second), Some(0x2000_0008));

        let again = host.init(0x2000_0007, 0x2000_0003);
        assert_eq!(host.object(0x2000_0007).handle, Some(again));
        assert_eq!(host.script_object(again), Some(0x2000_0007));
        assert_eq!(
            host.object(0x2000_0007).handle,
            Some(again),
            "the older thread stays alive, only the handle moved"
        );
    }

    /// Without a name source the copy has no bytes: the name stays empty
    /// (the exe would `strcpy` from the raw payload address).
    #[test]
    fn without_a_name_source_the_name_is_empty() {
        let mut host = ThreadHost::new();
        let id = host.init(0x2000_0007, 0x2000_0001);
        assert_eq!(host.queues().get(id).name, "");
        assert_eq!(host.queues().get(id).flags, CREATED);
        assert_eq!(host.resolve_name(0x2000_0001), "");
    }

    /// The `L` parameter's payload word is what the source resolves; a
    /// container that ran short (or is empty) reads word 0, the port's
    /// stand-in for the exe's stale stack slot, and still builds a thread.
    #[test]
    fn the_name_word_is_the_parameter_payload() {
        assert_eq!(fresh().resolve_name(0xdead_beef), "wdeadbeef");

        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| threads.set_name_source(test_name));

        assert_eq!(
            seam::dispatch(THREAD_INIT, &[Value::reference(0x2000_0001)]),
            Ok(None),
            "one cell only: the receiver, no `L` slot"
        );
        assert_eq!(
            seam::dispatch(THREAD_INIT, &[]),
            Ok(None),
            "empty container: a null receiver word"
        );
        with_threads(|threads| {
            let names: Vec<&str> = threads
                .queues()
                .threads()
                .iter()
                .map(|thread| thread.name.as_str())
                .collect();
            assert_eq!(names, vec!["w0", "w0"]);
            assert_eq!(threads.object(0x2000_0001).handle, Some(0));
            assert_eq!(threads.object(0).handle, Some(1));
        });
        seam::reset();
        reset();
    }

    /// `Thread.start` on a created thread: bit 1 clears, `run` is called on
    /// the receiver's `target` with the thread, and success kills the thread
    /// and clears both object-model words.
    #[test]
    fn start_clears_created_and_finishes_a_successful_run() {
        let receiver = 0x2000_0007;
        let mut host = starter();
        let id = host.init(receiver, 0x2000_0001);
        assert_eq!(host.queues().get(id).flags, CREATED);

        assert_eq!(
            host.start(receiver),
            StartOutcome::Finished { monitor: None },
            "not waiting: no monitor to unlink"
        );
        assert_eq!(
            host.queues().get(id).flags,
            flags::DEAD,
            "bit 1 cleared (0x47c5a4), then FUN_0041f780's 0x80"
        );
        assert_eq!(host.script_object(id), None, "thread+0x18 = 0 (0x47c5e2)");
        assert_eq!(
            host.object(receiver),
            ThreadObject { handle: None },
            "FUN_0042a9e0(receiver, [0x62e008], 0) (0x47c5f6)"
        );
        assert_eq!(
            host.queues().run_order(),
            vec![id],
            "the dead thread keeps its queue slot; the scheduler reaps it"
        );
        assert_eq!(
            run_calls(),
            vec![(RUNNABLE, id)],
            "FUN_00408a30(target, thread, \"run\")"
        );
    }

    /// The failure path: a `run` whose `eax` is 0 (`0x47c5d9`) leaves
    /// everything but the flag bit alone.
    #[test]
    fn start_keeps_the_thread_when_run_fails() {
        let receiver = 0x2000_0007;
        let mut host = starter();
        RUN_RESULT.with(|result| result.set(false));
        let id = host.init(receiver, 0x2000_0001);

        assert_eq!(host.start(receiver), StartOutcome::RunFailed);
        assert_eq!(host.queues().get(id).flags, 0, "bit 1 cleared only");
        assert_eq!(host.script_object(id), Some(receiver), "thread+0x18 kept");
        assert_eq!(host.object(receiver).handle, Some(id), "handle kept");
        assert_eq!(host.queues().run_order(), vec![id]);
        assert_eq!(run_calls(), vec![(RUNNABLE, id)], "the call was made");
    }

    /// No thread in the handle field: `FUN_0042ab50` returning 0 exits at
    /// `0x47c598` before anything is touched and no call is made. A *second*
    /// `start` takes this path, because success cleared the handle field.
    #[test]
    fn start_without_a_thread_does_nothing() {
        let mut host = starter();
        assert_eq!(host.start(0x2000_0007), StartOutcome::NoThread);
        assert!(host.queues().threads().is_empty());
        assert!(run_calls().is_empty(), "no thread: no `run` call");

        let id = host.init(0x2000_0007, 0x2000_0001);
        assert_eq!(
            host.start(0x2000_0007),
            StartOutcome::Finished { monitor: None }
        );
        assert_eq!(host.queues().get(id).flags, flags::DEAD);
        assert_eq!(
            host.start(0x2000_0007),
            StartOutcome::NoThread,
            "the first start cleared the handle, so the second has no thread"
        );
        assert_eq!(
            run_calls(),
            vec![(RUNNABLE, id)],
            "the second start made no call"
        );
        assert_eq!(host.queues().get(id).flags, flags::DEAD, "unchanged");
    }

    /// The exe dereferences the target word without a null test; the port
    /// reads a missing `target` (0) as a call it cannot make.
    #[test]
    fn start_without_a_target_reports_the_failed_call() {
        let receiver = 0x2000_0007;
        let mut host = starter();
        TARGET.with(|target| target.set(0));
        let id = host.init(receiver, 0x2000_0001);

        assert_eq!(host.start(receiver), StartOutcome::RunFailed);
        assert_eq!(host.queues().get(id).flags, 0);
        assert_eq!(host.object(receiver).handle, Some(id));
        assert!(run_calls().is_empty(), "the target read failed first");
    }

    /// Without the two seams the call cannot be made: the flag bit clears and
    /// nothing else changes. The `target` seam alone is not enough either.
    #[test]
    fn start_without_the_seams_fails_the_call() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        assert_eq!(host.start(receiver), StartOutcome::RunFailed);
        assert_eq!(host.queues().get(id).flags, 0);
        assert_eq!(host.object(receiver).handle, Some(id));
        assert_eq!(host.script_object(id), Some(receiver));

        let mut host = fresh();
        host.set_target_source(test_target);
        let id = host.init(receiver, 0x2000_0001);
        assert_eq!(
            host.start(receiver),
            StartOutcome::RunFailed,
            "no `run` source: the call cannot be made"
        );
        assert_eq!(host.queues().get(id).flags, 0);
        assert_eq!(host.object(receiver).handle, Some(id));
    }

    /// A waiting thread is the die scanner's other half: `FUN_0041f780`
    /// notifies it back into the run queue and reports its monitor object for
    /// S2's `Monitor::unlink`.
    #[test]
    fn start_unlinks_a_waiting_thread() {
        const MONITOR: u32 = 0x3000_0005;
        let receiver = 0x2000_0007;
        let mut host = starter();
        let id = host.init(receiver, 0x2000_0001);
        // A waiting thread (`Object.wait` is a later row): the port's own
        // queue API, reached through the private field.
        assert!(host.queues.wait(id, MONITOR).is_ok());
        assert_eq!(host.queues.notify_order(), vec![id]);

        assert_eq!(
            host.start(receiver),
            StartOutcome::Finished {
                monitor: Some(MONITOR)
            }
        );
        assert_eq!(host.queues.notify_order(), vec![], "notified off the queue");
        assert_eq!(host.queues.run_order(), vec![id], "back on the run queue");
        assert_eq!(host.queues.get(id).wait_object, MONITOR, "+0x34 kept");
        assert_eq!(host.queues.get(id).flags, flags::DEAD);
        assert_eq!(host.script_object(id), None);
        assert_eq!(host.object(receiver).handle, None);
    }

    /// `thread_start` through the seam: the receiver is the container's last
    /// cell, the hooks run, and the return is `V`.
    #[test]
    fn start_dispatches_through_the_seam() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.set_name_source(test_name);
            threads.set_target_source(test_target);
            threads.set_run_source(test_run);
            threads.init(0x2000_0007, 0x2000_0001);
        });
        RUN_RESULT.with(|result| result.set(true));
        RUN_CALLS.with(|calls| calls.borrow_mut().clear());

        let args = [Value::reference(0x2000_0007)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.Thread",
            name: "start",
            sig: "()V",
        };
        assert_eq!(seam::bridge(&mut vm, &info), None, "V: no return cell");
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        with_threads(|threads| {
            assert_eq!(threads.queues().threads().len(), 1);
            assert_eq!(threads.queues().get(0).flags, flags::DEAD);
            assert_eq!(threads.script_object(0), None);
            assert_eq!(threads.object(0x2000_0007).handle, None);
        });
        assert_eq!(run_calls(), vec![(RUNNABLE, 0)]);

        let empty: [Value; 0] = [];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &empty,
            class: "java.lang.Thread",
            name: "start",
            sig: "()V",
        };
        assert_eq!(
            seam::bridge(&mut vm, &info),
            None,
            "an empty container is a null receiver: no thread"
        );
        assert_eq!(run_calls(), vec![(RUNNABLE, 0)], "no second call");
        reset();
        seam::reset();
    }

    /// `Thread.stop` on a created (never started) thread: `FUN_0041f780`
    /// marks it dead and it keeps every other flag — including the `CREATED`
    /// bit `start` would have cleared — while both object-model words go.
    #[test]
    fn stop_kills_the_thread_and_keeps_created() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);

        assert_eq!(
            host.stop(receiver),
            StopOutcome::Stopped { monitor: None },
            "not waiting: no monitor to unlink"
        );
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::DEAD,
            "0x80 set, no flag cleared (`start`'s `CREATED` clear is its own)"
        );
        assert_eq!(host.script_object(id), None, "thread+0x18 = 0 (0x47c631)");
        assert_eq!(
            host.object(receiver),
            ThreadObject { handle: None },
            "FUN_0042a9e0(receiver, [0x62e008], 0) (0x47c646)"
        );
        assert_eq!(
            host.queues().run_order(),
            vec![id],
            "the dead thread keeps its queue slot; the scheduler reaps it"
        );

        assert_eq!(
            host.stop(receiver),
            StopOutcome::NoThread,
            "the first stop cleared the handle, so the second has no thread"
        );
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::DEAD,
            "unchanged"
        );
    }

    /// A waiting thread is the die scanner's other half: `FUN_0041f780`
    /// notifies it back into the run queue and reports its monitor object for
    /// S2's `Monitor::unlink` (the `T_Container` slot `FUN_00408c50` clears
    /// without shrinking the container).
    #[test]
    fn stop_unlinks_a_waiting_thread() {
        const MONITOR: u32 = 0x3000_0005;
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        // A waiting thread (`Object.wait` is a later row): the port's own
        // queue API, reached through the private field.
        assert!(host.queues.wait(id, MONITOR).is_ok());
        assert_eq!(host.queues.notify_order(), vec![id]);

        assert_eq!(
            host.stop(receiver),
            StopOutcome::Stopped {
                monitor: Some(MONITOR)
            },
            "the exe's die scanner hands back the +0x34 monitor object"
        );
        assert_eq!(host.queues.notify_order(), vec![], "notified off the queue");
        assert_eq!(host.queues.run_order(), vec![id], "back on the run queue");
        assert_eq!(host.queues.get(id).wait_object, MONITOR, "+0x34 kept");
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::DEAD,
            "0x10 cleared by the notify, 0x80 set"
        );
        assert_eq!(host.script_object(id), None);
        assert_eq!(host.object(receiver).handle, None);
    }

    /// No thread in the handle field: `FUN_0042ab50` returning 0 exits at
    /// `0x47c628` before anything is touched — and ditto for a receiver no
    /// `init` has ever stored a handle on.
    #[test]
    fn stop_without_a_thread_does_nothing() {
        let mut host = fresh();
        assert_eq!(host.stop(0x2000_0007), StopOutcome::NoThread);
        assert!(host.queues().threads().is_empty());
        assert!(host.queues().run_queue().is_empty());
        assert!(host.queues().notify_queue().is_empty());
    }

    /// `thread_stop` through the seam: the receiver is the container's last
    /// cell, the thread dies, and the return is `V`.
    #[test]
    fn stop_dispatches_through_the_seam() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.set_name_source(test_name);
            threads.init(0x2000_0007, 0x2000_0001);
        });

        let args = [Value::reference(0x2000_0007)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.Thread",
            name: "stop",
            sig: "()V",
        };
        assert_eq!(seam::bridge(&mut vm, &info), None, "V: no return cell");
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        with_threads(|threads| {
            assert_eq!(threads.queues().threads().len(), 1);
            assert_eq!(threads.queues().get(0).flags, CREATED | flags::DEAD);
            assert_eq!(threads.script_object(0), None);
            assert_eq!(threads.object(0x2000_0007).handle, None);
        });

        let empty: [Value; 0] = [];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &empty,
            class: "java.lang.Thread",
            name: "stop",
            sig: "()V",
        };
        assert_eq!(
            seam::bridge(&mut vm, &info),
            None,
            "an empty container is a null receiver: no thread"
        );
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        reset();
        seam::reset();
    }

    /// `sleep` marks the *calling* thread (`[CallInfo+4]`, `0x47c66a`), not
    /// the receiver: the receiver's thread stays exactly as it was. A test
    /// that only looked at the receiver would see "nothing happened"; the
    /// marker is on the caller's `+0x30`/bit 3.
    #[test]
    fn sleep_marks_the_calling_thread_only() {
        let mut host = fresh();
        let caller = host.init(0x2000_0007, 0x2000_0001);
        let receiver = host.init(0x2000_0008, 0x2000_0002);
        host.set_current(Some(caller));

        assert_eq!(host.sleep(5.0, 2.5), Ok(()), "FUN_0041f630's clock + arg");
        assert_eq!(
            host.queues().get(caller).sleep_deadline,
            7.5,
            "+0x30 = clock() + duration (0x41f638)"
        );
        assert_eq!(
            host.queues().get(caller).flags,
            CREATED | SLEEPING,
            "bit 3 set (0x41f63f)"
        );
        assert_eq!(
            host.queues().get(receiver).sleep_deadline,
            0.0,
            "the receiver is not the sleeper"
        );
        assert_eq!(host.queues().get(receiver).flags, CREATED);
    }

    /// A second `sleep` overwrites the deadline and re-sets bit 3 (`0x41f642`
    /// stores unconditionally); with no calling thread installed the port
    /// reports the missing caller instead of silently sleeping nobody.
    #[test]
    fn sleep_overwrites_and_reports_a_missing_caller() {
        let mut host = fresh();
        let id = host.init(0x2000_0007, 0x2000_0001);

        assert_eq!(
            host.sleep(1.0, 2.0),
            Err(ThreadHostFault::MissingCurrentThread),
            "the exe always has a caller; the port says so instead of guessing"
        );
        assert_eq!(host.queues().get(id).flags, CREATED, "nothing marked");
        assert_eq!(host.queues().get(id).sleep_deadline, 0.0);

        host.set_current(Some(id));
        assert_eq!(host.sleep(1.0, 2.0), Ok(()));
        assert_eq!(host.queues().get(id).sleep_deadline, 3.0);
        assert_eq!(host.sleep(9.0, 1.0), Ok(()), "a second sleep overwrites");
        assert_eq!(host.queues().get(id).sleep_deadline, 10.0);
        assert_eq!(host.queues().get(id).flags, CREATED | SLEEPING);

        host.set_current(Some(99));
        assert_eq!(
            host.sleep(1.0, 1.0),
            Err(ThreadHostFault::MissingCurrentThread),
            "an id the arena never handed out is no calling thread"
        );
        assert_eq!(host.queues().get(id).sleep_deadline, 10.0, "unchanged");
    }

    /// `suspend` sets `0x20` and moves the thread off the run queue into the
    /// notify queue; `resume` puts it back at the run-queue tail. The queue
    /// movement is the discriminator: a silently-failed suspend would leave
    /// the thread runnable.
    #[test]
    fn suspend_and_resume_move_the_thread_between_the_queues() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);

        host.suspend(receiver);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::NOTIFIED,
            "bit 0x20 (0x41f577)"
        );
        assert_eq!(host.queues().run_order(), vec![], "off the run queue");
        assert_eq!(
            host.queues().notify_order(),
            vec![id],
            "onto the notify queue"
        );

        host.suspend(receiver);
        assert_eq!(
            host.queues().notify_order(),
            vec![id],
            "already 0x20: a second suspend is a no-op (0x41f575)"
        );
        assert_eq!(host.queues().get(id).flags, CREATED | flags::NOTIFIED);

        host.resume(receiver);
        assert_eq!(host.queues().get(id).flags, CREATED, "0x20 cleared");
        assert_eq!(host.queues().notify_order(), vec![]);
        assert_eq!(host.queues().run_order(), vec![id], "back on the run queue");

        host.resume(receiver);
        assert_eq!(
            host.queues().run_order(),
            vec![id],
            "not suspended: a second resume cannot duplicate the entry"
        );
    }

    /// A waiting thread is not moved by suspend/resume: `0x10` guards the
    /// queue half (`0x41f57c`/`0x41f5df`), so only the bit changes.
    #[test]
    fn suspend_keeps_a_waiting_thread_on_the_notify_queue() {
        const MONITOR: u32 = 0x3000_0005;
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        assert!(host.queues.wait(id, MONITOR).is_ok());

        host.suspend(receiver);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::WAITING | flags::NOTIFIED
        );
        assert_eq!(host.queues().notify_order(), vec![id], "one entry only");
        assert_eq!(host.queues().run_order(), vec![]);

        host.resume(receiver);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::WAITING,
            "0x20 cleared, 0x10 kept"
        );
        assert_eq!(
            host.queues().notify_order(),
            vec![id],
            "a waiting thread stays notified-queued"
        );
        assert_eq!(host.queues().run_order(), vec![]);
    }

    /// `setDaemon`: a non-zero argument sets bit `0x100`, zero clears it, and
    /// nothing else moves.
    #[test]
    fn set_daemon_sets_and_clears_only_bit_0x100() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        assert!(!host.queues().get(id).daemon());

        host.set_daemon(receiver, true);
        assert!(host.queues().get(id).daemon());
        assert_eq!(host.queues().get(id).flags, CREATED | DAEMON);

        host.set_daemon(receiver, false);
        assert!(!host.queues().get(id).daemon());
        assert_eq!(host.queues().get(id).flags, CREATED);

        host.set_daemon(0x2000_0009, true);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED,
            "an unknown receiver names no thread"
        );
    }

    /// `isAlive` returns 0 for a live thread — the exe's own quirk
    /// (`xor eax,eax` at `0x47c753`): recorded as behaviour, not a bug fix.
    #[test]
    fn is_alive_returns_false_for_a_live_thread() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED,
            "the thread is created"
        );

        assert!(!host.is_alive(receiver));
        assert!(
            !host.is_alive(0x2000_0009),
            "and false for a receiver with no thread at all"
        );
    }

    /// The priority pair: `getPriority` reads `+0x28` and reports 0 for a
    /// receiver that names no thread; `setPriority` writes `+0x28` and
    /// touches nothing otherwise.
    #[test]
    fn priority_accessors_go_through_the_handle() {
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        assert_eq!(host.get_priority(receiver), INIT_PRIORITY);

        host.set_priority(receiver, -3);
        assert_eq!(host.queues().get(id).priority, -3);
        assert_eq!(host.get_priority(receiver), -3);

        assert_eq!(host.get_priority(0x2000_0009), 0, "no thread: 0");
        host.set_priority(0x2000_0009, 5);
        assert_eq!(
            host.queues().get(id).priority,
            -3,
            "an unknown receiver cannot change a thread"
        );
    }

    /// The seven Thread natives through the seam: `isAlive` returns `Some(0)`,
    /// `getPriority` the stored word, `setPriority`/`setDaemon`/`sleep`/
    /// `suspend`/`resume` return `V`, and the container's *last* cell is the
    /// receiver while the parameter sits before it.
    #[test]
    fn the_thread_natives_dispatch_through_the_seam() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.set_name_source(test_name);
            threads.init(0x2000_0007, 0x2000_0001);
            threads.set_current(Some(0));
        });

        assert_eq!(
            seam::dispatch(THREAD_IS_ALIVE, &[Value::reference(0x2000_0007)]),
            Ok(Some(0)),
            "the exe's unconditional 0"
        );
        assert_eq!(
            seam::dispatch(THREAD_GET_PRIORITY, &[Value::reference(0x2000_0007)]),
            Ok(Some(0))
        );
        assert_eq!(
            seam::dispatch(
                THREAD_SET_PRIORITY,
                &[Value::int(5), Value::reference(0x2000_0007)],
            ),
            Ok(None),
            "V: no return cell"
        );
        assert_eq!(
            seam::dispatch(THREAD_GET_PRIORITY, &[Value::reference(0x2000_0007)]),
            Ok(Some(5)),
            "out[1] (the parameter) is the priority"
        );
        assert_eq!(
            seam::dispatch(
                THREAD_SET_DAEMON,
                &[Value::int(1), Value::reference(0x2000_0007)],
            ),
            Ok(None)
        );
        assert_eq!(
            seam::dispatch(
                THREAD_SLEEP,
                &[Value::float(2.5), Value::reference(0x2000_0009)],
            ),
            Ok(None),
            "the receiver 0x2000_0009 names no thread and is ignored"
        );
        assert_eq!(
            seam::dispatch(THREAD_SUSPEND, &[Value::reference(0x2000_0007)]),
            Ok(None)
        );
        assert_eq!(
            seam::dispatch(THREAD_RESUME, &[Value::reference(0x2000_0007)]),
            Ok(None)
        );
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        with_threads(|threads| {
            assert_eq!(threads.queues().get(0).priority, 5);
            assert!(threads.queues().get(0).daemon());
            assert_eq!(
                threads.queues().get(0).sleep_deadline,
                2.5,
                "clock 0.0 (no source) + 2.5"
            );
            assert_eq!(
                threads.queues().get(0).flags,
                CREATED | DAEMON | SLEEPING,
                "suspend then resume cancelled 0x20 again"
            );
            assert_eq!(threads.queues().run_order(), vec![0]);
            assert_eq!(threads.last_fault(), None);
        });
        reset();
        seam::reset();
    }

    /// A short container reads 0 for the missing cells (the exe's stale
    /// slot); `sleep` still sleeps the caller with a 0 duration and
    /// `setDaemon` clears the bit, exactly like the exe reading `[esp+0x18]`.
    #[test]
    fn short_containers_read_stale_slots_as_zero() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.init(0x2000_0007, 0x2000_0001);
            threads.set_current(Some(0));
            threads.set_daemon(0x2000_0007, true);
        });
        assert_eq!(
            seam::dispatch(THREAD_SET_DAEMON, &[Value::reference(0x2000_0007)]),
            Ok(None),
            "one cell only: the parameter reads 0"
        );
        assert_eq!(
            seam::dispatch(THREAD_SLEEP, &[Value::reference(0x2000_0007)]),
            Ok(None),
            "and so does the F"
        );
        with_threads(|threads| {
            assert!(!threads.queues().get(0).daemon(), "0 cleared it");
            assert_eq!(threads.queues().get(0).sleep_deadline, 0.0);
            assert_eq!(threads.queues().get(0).flags, CREATED | SLEEPING);
        });
        reset();
        seam::reset();
    }

    /// `Object.wait` moves the *calling* thread onto the receiver's
    /// container: off the run queue, bit `0x10`, onto the notify queue,
    /// `+0x34` = the object and one waiter in the container. The receiver is
    /// the object — an object word, not a thread handle — so no `init` is
    /// needed for it.
    #[test]
    fn wait_moves_the_calling_thread_onto_the_object() {
        const OBJECT: u32 = 0x3000_0005;
        let mut host = fresh();
        let caller = host.init(0x2000_0007, 0x2000_0001);
        let idle = host.init(0x2000_0008, 0x2000_0002);
        host.set_current(Some(caller));

        assert_eq!(host.wait(OBJECT), Ok(()));
        assert_eq!(
            host.queues().get(caller).flags,
            CREATED | flags::WAITING,
            "bit 0x10 (0x41f6a0)"
        );
        assert_eq!(host.queues().get(caller).wait_object, OBJECT, "+0x34");
        assert_eq!(host.queues().notify_order(), vec![caller]);
        assert_eq!(host.queues().run_order(), vec![idle], "off the run queue");
        assert_eq!(host.waiters(OBJECT), vec![caller], "object+0x14 container");
        assert_eq!(host.queues().get(idle).flags, CREATED, "the idle thread");
    }

    /// No calling thread installed: the port reports the missing caller
    /// instead of silently leaving the thread runnable, and touches nothing.
    /// A second `wait` on an already-waiting caller is the exe's own
    /// `Thread::wait: already waiting` fatal path.
    #[test]
    fn wait_reports_a_missing_caller_and_a_double_wait() {
        const OBJECT: u32 = 0x3000_0005;
        let mut host = fresh();
        let caller = host.init(0x2000_0007, 0x2000_0001);

        assert_eq!(
            host.wait(OBJECT),
            Err(ThreadHostFault::MissingCurrentThread)
        );
        assert_eq!(host.queues().get(caller).flags, CREATED);
        assert_eq!(host.waiters(OBJECT), vec![]);
        assert_eq!(host.queues().run_order(), vec![caller]);

        host.set_current(Some(caller));
        assert_eq!(host.wait(OBJECT), Ok(()));
        assert_eq!(
            host.wait(OBJECT),
            Err(ThreadHostFault::Monitor(MonitorFault::Thread(
                ThreadFault::WaitAlreadyWaiting
            ))),
            "the exe's `Thread::wait: already waiting` (0x60c1ec)"
        );
        assert_eq!(host.waiters(OBJECT), vec![caller], "one entry only");
        assert_eq!(host.queues().notify_order(), vec![caller]);
    }

    /// `notify` wakes the newest waiter (LIFO) and leaves the older one
    /// waiting; `notifyAll` then drains the container. The `[oracle]`
    /// sequence is waits `a, b` -> notify wakes `b` -> notifyAll wakes `a`.
    #[test]
    fn notify_is_lifo_and_notify_all_drains() {
        const OBJECT: u32 = 0x3000_0005;
        let mut host = fresh();
        let a = host.init(0x2000_0007, 0x2000_0001);
        let b = host.init(0x2000_0008, 0x2000_0002);
        host.set_current(Some(a));
        assert_eq!(host.wait(OBJECT), Ok(()));
        host.set_current(Some(b));
        assert_eq!(host.wait(OBJECT), Ok(()));
        assert_eq!(host.waiters(OBJECT), vec![a, b], "append order");
        assert_eq!(host.queues().notify_order(), vec![a, b]);

        assert_eq!(host.notify(OBJECT), Ok(b), "the newest waiter");
        assert_eq!(host.queues().get(b).flags, CREATED, "0x10 cleared");
        assert_eq!(
            host.queues().get(a).flags,
            CREATED | flags::WAITING,
            "a is still waiting"
        );
        assert_eq!(host.waiters(OBJECT), vec![a], "the slot is consumed");
        assert_eq!(host.queues().run_order(), vec![b], "b is runnable again");

        assert_eq!(host.notify_all(OBJECT), Ok(vec![a]));
        assert_eq!(host.waiters(OBJECT), vec![]);
        assert_eq!(host.queues().get(a).flags, CREATED);
        assert_eq!(host.queues().run_order(), vec![b, a]);
    }

    /// A notify on a container nobody is waiting on is the exe's silent
    /// return; the port surfaces the lost notification as the same condition
    /// the die scanner logs, and `notifyAll` on an empty container is not a
    /// fault at all.
    #[test]
    fn notify_without_waiters_surfaces_the_lost_notification() {
        const OBJECT: u32 = 0x3000_0005;
        let mut host = fresh();
        host.init(0x2000_0007, 0x2000_0001);

        assert_eq!(
            host.notify(OBJECT),
            Err(ThreadHostFault::Monitor(MonitorFault::EmptyNotify)),
            "Instance::notify(Thread*): inconsistency"
        );
        assert_eq!(
            host.notify_all(OBJECT),
            Ok(vec![]),
            "draining nobody is fine"
        );
        assert_eq!(host.waiters(OBJECT), vec![]);
    }

    /// A suspended waiter keeps `0x20`: the notify clears `0x10` but the
    /// thread stays on the notify queue until a later resume.
    #[test]
    fn notify_of_a_suspended_waiter_leaves_it_queued() {
        const OBJECT: u32 = 0x3000_0005;
        let mut host = fresh();
        let receiver = 0x2000_0007;
        let id = host.init(receiver, 0x2000_0001);
        host.set_current(Some(id));
        assert_eq!(host.wait(OBJECT), Ok(()));
        host.suspend(receiver);
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::WAITING | flags::NOTIFIED
        );

        assert_eq!(host.notify(OBJECT), Ok(id));
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::NOTIFIED,
            "0x10 cleared, 0x20 kept (0x41f72e)"
        );
        assert_eq!(
            host.queues().notify_order(),
            vec![id],
            "still queued for the resume"
        );
        assert_eq!(host.queues().run_order(), vec![]);

        host.resume(receiver);
        assert_eq!(host.queues().run_order(), vec![id]);
    }

    /// A dying waiter is unlinked from the object's container — the die
    /// scanner's other half (`FUN_00408c50`), the exact case T8/T9 handed to
    /// S2. Without it the container would keep a slot naming a dead thread.
    #[test]
    fn stop_unlinks_the_waiter_from_the_container() {
        const OBJECT: u32 = 0x3000_0005;
        let receiver = 0x2000_0007;
        let mut host = fresh();
        let id = host.init(receiver, 0x2000_0001);
        host.set_current(Some(id));
        assert_eq!(host.wait(OBJECT), Ok(()));
        assert_eq!(host.waiters(OBJECT), vec![id]);

        assert_eq!(
            host.stop(receiver),
            StopOutcome::Stopped {
                monitor: Some(OBJECT)
            }
        );
        assert_eq!(host.waiters(OBJECT), vec![], "the slot was zeroed");
        assert_eq!(
            host.queues().get(id).flags,
            CREATED | flags::DEAD,
            "the die notify cleared 0x10, then 0x80 was set"
        );
        assert_eq!(host.queues().run_order(), vec![id], "back on the run queue");
        assert_eq!(
            host.notify(OBJECT),
            Err(ThreadHostFault::Monitor(MonitorFault::EmptyNotify)),
            "the container's count still names the zeroed hole (0x408bd2)"
        );
    }

    /// The three Object natives through the seam: `wait` moves the container's
    /// caller, `notify` is LIFO and `notifyAll` drains; the receiver is the
    /// object word itself and no handle lookup is involved.
    #[test]
    fn the_object_natives_dispatch_through_the_seam() {
        const OBJECT: u32 = 0x3000_0005;
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.set_name_source(test_name);
            threads.init(0x2000_0007, 0x2000_0001);
            threads.init(0x2000_0008, 0x2000_0002);
            threads.set_current(Some(0));
        });

        assert_eq!(
            seam::dispatch(OBJECT_WAIT, &[Value::reference(OBJECT)]),
            Ok(None),
            "V: no return cell"
        );
        with_threads(|threads| {
            assert_eq!(threads.waiters(OBJECT), vec![0]);
            threads.set_current(Some(1));
        });
        assert_eq!(
            seam::dispatch(OBJECT_WAIT, &[Value::reference(OBJECT)]),
            Ok(None)
        );
        assert_eq!(
            seam::dispatch(OBJECT_NOTIFY, &[Value::reference(OBJECT)]),
            Ok(None)
        );
        assert_eq!(
            seam::dispatch(OBJECT_NOTIFY_ALL, &[Value::reference(OBJECT)]),
            Ok(None)
        );
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        with_threads(|threads| {
            assert_eq!(threads.waiters(OBJECT), vec![], "drained");
            assert_eq!(threads.queues().get(0).flags, CREATED);
            assert_eq!(threads.queues().get(1).flags, CREATED);
            assert_eq!(threads.queues().run_order(), vec![1, 0], "LIFO then a");
            assert_eq!(threads.last_fault(), None);
        });
        reset();
        seam::reset();
    }

    /// `wait` with no calling thread through the seam records the port's
    /// missing-caller fault instead of silently doing nothing.
    #[test]
    fn the_object_wait_reports_its_fault_through_the_seam() {
        const OBJECT: u32 = 0x3000_0005;
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_threads(|threads| {
            threads.init(0x2000_0007, 0x2000_0001);
        });

        assert_eq!(
            seam::dispatch(OBJECT_WAIT, &[Value::reference(OBJECT)]),
            Ok(None),
            "V: the fault is the only report"
        );
        with_threads(|threads| {
            assert_eq!(
                threads.last_fault(),
                Some(ThreadHostFault::MissingCurrentThread)
            );
            assert_eq!(threads.waiters(OBJECT), vec![]);
            assert_eq!(threads.queues().get(0).flags, CREATED);
        });
        assert_eq!(
            seam::dispatch(OBJECT_WAIT, &[Value::reference(OBJECT)]),
            Ok(None)
        );
        with_threads(|threads| threads.set_current(Some(0)));
        assert_eq!(
            seam::dispatch(OBJECT_WAIT, &[Value::reference(OBJECT)]),
            Ok(None)
        );
        with_threads(|threads| {
            assert_eq!(threads.last_fault(), None, "cleared on success");
            assert_eq!(threads.waiters(OBJECT), vec![0]);
        });
        reset();
        seam::reset();
    }

    /// The 13 registrar rows, with the exe's own addresses, spellings and no
    /// duplicates; every row has a port.
    #[test]
    fn the_family_is_the_thirteen_rows() {
        assert_eq!(THREAD_NATIVE_BINDINGS.len(), 13);
        for (i, row) in THREAD_NATIVE_BINDINGS.iter().enumerate() {
            assert!(is_thread_address(row.impl_addr));
            assert!(
                !THREAD_NATIVE_BINDINGS[..i]
                    .iter()
                    .any(|b| b.impl_addr == row.impl_addr),
                "duplicate address {:08x}",
                row.impl_addr
            );
        }
        assert_eq!(THREAD_INIT, 0x0047_c510);
        assert_eq!(OBJECT_NOTIFY_ALL, 0x0047_c970);
    }

    /// `install` registers every row and installs the bridge at all thirteen
    /// addresses; the bridge then dispatches a port end to end.
    #[test]
    fn install_registers_the_rows_and_dispatches_the_port() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);

        for row in THREAD_NATIVE_BINDINGS {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
            assert!(
                vm.native_impls.contains_key(&row.impl_addr),
                "every row now has a port: {:#010x}",
                row.impl_addr
            );
        }

        with_threads(|threads| threads.set_name_source(test_name));
        let args = [Value::reference(0x2000_0001), Value::reference(0x2000_0007)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.Thread",
            name: "init",
            sig: "(Ljava.lang.String;)V",
        };
        assert_eq!(seam::bridge(&mut vm, &info), None, "V: no return cell");
        seam::with_lang_host(|host| assert_eq!(host.last_error, None));
        with_threads(|threads| {
            assert_eq!(threads.queues().run_order(), vec![0]);
            assert_eq!(threads.queues().get(0).name, "w20000001");
            assert_eq!(threads.queues().get(0).flags, CREATED);
            assert_eq!(threads.script_object(0), Some(0x2000_0007));
            assert_eq!(threads.object(0x2000_0007).handle, Some(0));
        });

        install(&mut vm);
        with_threads(|threads| assert_eq!(threads.queues().threads().len(), 1));
        reset();
        seam::reset();
    }

    /// Every thread/object row has a port now, so none of them is
    /// `NotPorted`; an address that is not a row at all stays
    /// `NoImplementation`.
    #[test]
    fn no_thread_row_is_left_unported() {
        seam::reset();
        reset();
        let mut vm = VmHost::new();
        install(&mut vm);
        for row in THREAD_NATIVE_BINDINGS {
            assert!(
                !matches!(
                    seam::dispatch(row.impl_addr, &[]),
                    Err(DispatchError::NotPorted { .. })
                ),
                "{:#010x} must have a port",
                row.impl_addr
            );
        }
        assert_eq!(
            seam::dispatch(THREAD_IS_ALIVE, &[]),
            Ok(Some(0)),
            "a ported row dispatches its port"
        );
        assert_eq!(
            seam::dispatch(0xdead_beef, &[]),
            Err(DispatchError::NoImplementation {
                impl_addr: 0xdead_beef
            })
        );
        assert_eq!(
            seam::dispatch(THREAD_INIT, &[]),
            Ok(None),
            "the ported row still dispatches"
        );
        seam::reset();
        reset();
    }
}
