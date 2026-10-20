//! Logical callers of the shared original performance-counter clock.
//!
//! Existing interpreter/scheduler APIs keep their `FnMut() -> X87` provider.
//! A provider can inspect [`current_site`] to validate a recorded input stream.
//! Metadata scopes do not sample, transform, store or advance the clock.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};

use slr_x87::{Precision, Rounding, X87};

/// The original caller whose clock request is currently being evaluated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClockSite {
    /// A caller that has not supplied clock metadata.
    #[default]
    Unspecified,
    /// `Thread::run` computes its deadline.
    ThreadEnter,
    /// `Thread::run` checks its budget at the loop tail.
    ThreadPoll,
    /// `Thread::run` computes its leftover time.
    ThreadExit,
    /// The scheduler driver computes its deadline.
    DriveEnter,
    /// The scheduler driver checks its deadline at the loop top.
    DriveLoop,
    /// The scheduler driver checks a sleeping thread's wake deadline.
    DriveSleepProbe,
    /// The scheduler tick samples time before the callback block.
    TickEnter,
    /// The scheduler tick samples elapsed time after the callback block.
    TickAfterCallbacks,
    /// Original build606 clock caller at `0x41f638`.
    ThreadSleep,
    /// Original build606 clock caller at `0x428413`.
    TimeScaleOffset,
    /// Original build606 clock caller at `0x428a5b`.
    FramePhysicsEpoch,
    /// Original build606 clock caller at `0x428a77`.
    FramePreviousTime,
    /// Original build606 clock caller at `0x428ad5`.
    FrameAnimationDelta,
    /// Original build606 clock caller at `0x47bfe5`.
    SystemCurrentTime,
    /// Original build606 clock caller at `0x47c7d5`.
    MathRandomize,
}

impl ClockSite {
    /// Original build606 return address following the `FUN_005516c0` call.
    /// This is a call-site identity, not an address to execute in the port.
    #[must_use]
    pub const fn original_caller(self) -> Option<u32> {
        match self {
            Self::Unspecified => None,
            Self::ThreadEnter => Some(0x0042_1066),
            Self::ThreadPoll => Some(0x0042_390d),
            Self::ThreadExit => Some(0x0042_3958),
            Self::DriveEnter => Some(0x0041_696a),
            Self::DriveLoop => Some(0x0041_69ac),
            Self::DriveSleepProbe => Some(0x0041_6a0e),
            Self::TickEnter => Some(0x0041_8d2d),
            Self::TickAfterCallbacks => Some(0x0041_8e67),
            Self::ThreadSleep => Some(0x0041f638),
            Self::TimeScaleOffset => Some(0x00428413),
            Self::FramePhysicsEpoch => Some(0x00428a5b),
            Self::FramePreviousTime => Some(0x00428a77),
            Self::FrameAnimationDelta => Some(0x00428ad5),
            Self::SystemCurrentTime => Some(0x0047bfe5),
            Self::MathRandomize => Some(0x0047c7d5),
        }
    }

    /// Identify a supported original caller; unknown sites remain explicit.
    #[must_use]
    pub const fn from_original_caller(caller: u32) -> Option<Self> {
        match caller {
            0x0042_1066 => Some(Self::ThreadEnter),
            0x0042_390d => Some(Self::ThreadPoll),
            0x0042_3958 => Some(Self::ThreadExit),
            0x0041_696a => Some(Self::DriveEnter),
            0x0041_69ac => Some(Self::DriveLoop),
            0x0041_6a0e => Some(Self::DriveSleepProbe),
            0x0041_8d2d => Some(Self::TickEnter),
            0x0041_8e67 => Some(Self::TickAfterCallbacks),
            0x0041f638 => Some(Self::ThreadSleep),
            0x00428413 => Some(Self::TimeScaleOffset),
            0x00428a5b => Some(Self::FramePhysicsEpoch),
            0x00428a77 => Some(Self::FramePreviousTime),
            0x00428ad5 => Some(Self::FrameAnimationDelta),
            0x0047bfe5 => Some(Self::SystemCurrentTime),
            0x0047c7d5 => Some(Self::MathRandomize),
            _ => None,
        }
    }
}

/// The actual port interpreter thread and current script location. This is
/// diagnostic request metadata, never copied from an oracle capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadContext {
    pub id: u64,
    pub name: Option<String>,
    pub class: Option<String>,
    pub method: Option<String>,
    pub node_index: Option<u64>,
}
static NEXT_EPHEMERAL: AtomicU64 = AtomicU64::new(1);
/// Temporary interpreter identities cannot collide with boot queue identities.
#[must_use]
pub fn ephemeral_identity() -> u64 {
    let id = NEXT_EPHEMERAL.fetch_add(1, Ordering::Relaxed);
    assert!(id < 1 << 63, "clock thread identity exhausted");
    id | (1 << 63)
}
thread_local! {
    static THREAD_CONTEXT: RefCell<Vec<ThreadContext>> = const { RefCell::new(Vec::new()) };
    static DRIVER_CURSOR: Cell<Option<usize>> = const { Cell::new(None) };
    static CONTROLS: Cell<(Precision,Rounding)> = const { Cell::new((Precision::F32,Rounding::NearestEven)) };
    static SITE: Cell<ClockSite> = const { Cell::new(ClockSite::Unspecified) };
}

/// The requesting site, visible only while its provider is being called.
#[must_use]
pub fn current_site() -> ClockSite {
    SITE.with(Cell::get)
}

#[must_use]
pub fn current_thread_context() -> Option<ThreadContext> {
    THREAD_CONTEXT.with(|value| value.borrow().last().cloned())
}
#[must_use]
pub fn current_thread_contexts() -> Vec<ThreadContext> {
    THREAD_CONTEXT.with(|value| value.borrow().clone())
}
#[must_use]
pub fn current_driver_cursor() -> Option<usize> {
    DRIVER_CURSOR.with(Cell::get)
}
#[must_use]
pub fn current_controls() -> (Precision, Rounding) {
    CONTROLS.with(Cell::get)
}
struct RestoreCursor(Option<usize>);
impl Drop for RestoreCursor {
    fn drop(&mut self) {
        DRIVER_CURSOR.with(|v| v.set(self.0));
    }
}
pub fn with_driver_cursor<R>(cursor: Option<usize>, operation: impl FnOnce() -> R) -> R {
    let _restore = RestoreCursor(DRIVER_CURSOR.with(|v| v.replace(cursor)));
    operation()
}
struct RestoreControls((Precision, Rounding));
impl Drop for RestoreControls {
    fn drop(&mut self) {
        CONTROLS.with(|v| v.set(self.0));
    }
}
pub fn with_interpreter_context<R>(
    context: ThreadContext,
    precision: Precision,
    operation: impl FnOnce() -> R,
) -> R {
    let _restore =
        RestoreControls(CONTROLS.with(|v| v.replace((precision, Rounding::NearestEven))));
    with_thread_context(context, operation)
}
struct RestoreThread;
impl Drop for RestoreThread {
    fn drop(&mut self) {
        THREAD_CONTEXT.with(|value| {
            value.borrow_mut().pop();
        });
    }
}
/// Scope real interpreter metadata across native dispatch, restoring the caller
/// after nested interpreter invocations and after unwinding.
pub fn with_thread_context<R>(context: ThreadContext, operation: impl FnOnce() -> R) -> R {
    THREAD_CONTEXT.with(|value| value.borrow_mut().push(context));
    let _restore = RestoreThread;
    operation()
}

struct Restore(ClockSite);

impl Drop for Restore {
    fn drop(&mut self) {
        SITE.with(|site| site.set(self.0));
    }
}

/// Evaluate an operation with scoped request metadata. Nested scopes and
/// unwinding restore the previous site, including a provider that panics.
pub fn with_site<R>(site: ClockSite, operation: impl FnOnce() -> R) -> R {
    let _restore = Restore(SITE.with(|current| current.replace(site)));
    operation()
}

/// Sample the existing provider exactly once, without changing its value.
pub fn read(clock: &mut dyn FnMut() -> X87, site: ClockSite) -> X87 {
    let value = with_site(site, clock);
    if crate::diagnostics::enabled() {
        crate::diagnostics::emit(crate::diagnostics::EventKind::Clock {
            site: format!("{site:?}"),
            caller: site.original_caller(),
            result_le: value.to_bytes(),
        });
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        interp::{BodyId, Frame, Host, Pc, Thread},
        sched::{CallbackRun, FinalizerQueues, SchedExit, Scheduler},
        threads::{ThreadQueues, SLEEPING},
    };
    use slr_fmt_tufa::Node;

    #[test]
    fn interpreter_metadata_tracks_real_nodes_and_nested_native_dispatch() {
        use crate::interp::{ExternOp, Flow, VmError};
        struct MetadataHost {
            body: Vec<Node>,
            native_context: Option<ThreadContext>,
        }
        impl Host for MetadataHost {
            fn body(&self, _: BodyId) -> &[Node] {
                &self.body
            }
            fn body_label(&self, _: BodyId) -> String {
                "java.test.Metadata.run()V".into()
            }
            fn extern_op(&mut self, _: &mut Thread, _: ExternOp) -> Result<Flow, VmError> {
                self.native_context = current_thread_context();
                let parent = current_thread_context().unwrap();
                let mut nested = parent.clone();
                nested.id += 1;
                with_thread_context(nested.clone(), || {
                    assert_eq!(current_thread_context(), Some(nested.clone()));
                    let contexts = current_thread_contexts();
                    assert_eq!(contexts.last(), Some(&nested));
                    assert_eq!(contexts.get(contexts.len() - 2), Some(&parent));
                });
                assert_eq!(current_thread_context(), Some(parent));
                Ok(Flow::Advance)
            }
        }
        let mut host = MetadataHost {
            body: vec![
                Node {
                    opword: 0x0024,
                    operand: 0,
                    imm: 0,
                },
                Node {
                    opword: 0x0010,
                    operand: 0,
                    imm: 0,
                },
            ],
            native_context: None,
        };
        let mut thread = Thread::new();
        let identity = thread.clock_identity;
        assert_eq!(thread.clone().clock_identity, identity);
        assert_ne!(Thread::new().clock_identity, identity);
        thread.clock_name = Some("honest thread".into());
        let pc = Some(Pc {
            body: BodyId(0),
            idx: 0,
        });
        thread.pc = pc;
        thread.frames.push(Frame::new(pc, 0));
        let mut reads = Vec::new();
        thread
            .run(&mut host, 10.0, &mut || {
                reads.push((current_site(), current_thread_context().unwrap()));
                X87::from_f64(0.0)
            })
            .unwrap();
        assert_eq!(
            reads.iter().map(|(_, c)| c.node_index).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(1)]
        );
        for (_, context) in &reads {
            assert_eq!(context.id, identity);
            assert_eq!(context.class.as_deref(), Some("java.test.Metadata"));
            assert_eq!(context.method.as_deref(), Some("run"));
            assert_eq!(context.name.as_deref(), Some("honest thread"));
        }
        assert_eq!(host.native_context.unwrap().node_index, Some(0));
        assert_eq!(current_thread_context(), None);
        let context = reads[0].1.clone();
        let _ =
            std::panic::catch_unwind(|| with_thread_context(context, || panic!("metadata unwind")));
        assert_eq!(current_thread_context(), None);
    }

    #[test]
    fn provider_values_and_scopes_survive_nested_reads_and_unwinding() {
        assert_eq!(current_site(), ClockSite::Unspecified);
        let value = X87::from_f64(1.25);
        let mut calls = 0;
        with_site(ClockSite::DriveLoop, || {
            assert_eq!(
                read(
                    &mut || {
                        calls += 1;
                        assert_eq!(current_site(), ClockSite::ThreadPoll);
                        value
                    },
                    ClockSite::ThreadPoll
                ),
                value
            );
            assert_eq!(current_site(), ClockSite::DriveLoop);
            let result = std::panic::catch_unwind(|| {
                with_site(ClockSite::ThreadExit, || panic!("provider stopped"));
            });
            assert!(result.is_err());
            assert_eq!(current_site(), ClockSite::DriveLoop);
        });
        assert_eq!(calls, 1);
        assert_eq!(current_site(), ClockSite::Unspecified);
    }

    #[test]
    fn interpreter_exposes_entry_poll_and_exit_without_extra_reads() {
        struct Body(Vec<Node>);
        impl Host for Body {
            fn body(&self, _: BodyId) -> &[Node] {
                &self.0
            }
        }
        let mut host = Body(vec![
            Node {
                opword: 0x1007,
                operand: 1,
                imm: 0x1c,
            },
            Node {
                opword: 0x100b,
                operand: 1,
                imm: 7,
            },
            Node {
                opword: 0x0010,
                operand: 1,
                imm: 0,
            },
        ]);
        let pc = Some(Pc {
            body: BodyId(0),
            idx: 0,
        });
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(Frame::new(pc, 0));
        let mut sites = Vec::new();
        let result = thread
            .run(&mut host, 10.0, &mut || {
                sites.push(current_site());
                X87::from_f64((sites.len() - 1) as f64)
            })
            .unwrap();
        assert_eq!(
            sites,
            [
                ClockSite::ThreadEnter,
                ClockSite::ThreadPoll,
                ClockSite::ThreadExit
            ]
        );
        assert_eq!(result, X87::from_f64(8.0));
        assert_eq!(current_site(), ClockSite::Unspecified);
    }

    #[test]
    fn tick_and_sleep_probe_preserve_the_existing_shared_read_sequence() {
        let mut queues = ThreadQueues::new();
        let sleeping = queues.spawn(0, SLEEPING, "sleeper");
        queues.get_mut(sleeping).sleep_deadline = 100.0;
        let mut scheduler = Scheduler::new();
        let mut sites = Vec::new();
        let mut clock = || {
            sites.push(current_site());
            X87::from_f64((sites.len() - 1) as f64)
        };
        let exit = scheduler.tick(
            &mut queues,
            &mut FinalizerQueues::new(),
            10.0,
            &mut clock,
            &mut |_| panic!("no finalizer"),
            &mut || CallbackRun::Consumed,
            &mut |_, _, _, _| panic!("sleeper cannot run"),
        );
        assert_eq!(exit, SchedExit::TimeUp);
        assert_eq!(
            sites,
            [
                ClockSite::TickEnter,
                ClockSite::TickAfterCallbacks,
                ClockSite::DriveEnter,
                ClockSite::DriveLoop,
                ClockSite::DriveSleepProbe,
                ClockSite::DriveLoop
            ]
        );
        assert_eq!(current_site(), ClockSite::Unspecified);
    }
}
