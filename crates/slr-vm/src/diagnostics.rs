//! Opt-in causal diagnostics. Observations never own execution state.
use crate::{
    interp::{Host, Slot, Thread},
    values::Value,
};
use serde::Serialize;
use std::{cell::RefCell, collections::VecDeque};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Context {
    pub frame: Option<u64>,
    pub thread: Option<u64>,
    pub thread_name: Option<String>,
    pub location: Option<ScriptLocation>,
    pub native: Option<NativeIdentity>,
    pub parent_sequence: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ScriptLocation {
    pub body: u32,
    pub node: u64,
    pub line: Option<u16>,
    pub label: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeIdentity {
    pub call_id: u64,
    pub class: String,
    pub name: String,
    pub signature: String,
    pub implementation: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub schema_version: u32,
    pub run_id: String,
    pub sequence: u64,
    pub context: Context,
    pub kind: EventKind,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    External {
        category: String,
        fields: Vec<(String, String)>,
    },
    Step {
        opcode: u16,
        immediate: u32,
    },
    ThreadRun {
        budget_bits: u32,
    },
    ThreadReturn {
        error: Option<String>,
    },
    Clock {
        site: String,
        caller: Option<u32>,
        result_le: [u8; 10],
    },
    NativeEnter {
        args: Vec<ValueSnapshot>,
    },
    NativeReturn {
        result: Option<u32>,
        error: Option<String>,
    },
    Store {
        destination: String,
        before: ValueSnapshot,
        after: ValueSnapshot,
    },
    Failure {
        error: String,
        snapshot: FailureSnapshot,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct ValueSnapshot {
    pub tag: String,
    pub payload: u32,
    pub float_bits: Option<u32>,
}
impl From<Value> for ValueSnapshot {
    fn from(value: Value) -> Self {
        Self {
            tag: value.tag.name().to_owned(),
            payload: value.payload,
            float_bits: (value.tag.name() == "F").then_some(value.payload),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct CellSnapshot {
    pub address: String,
    pub value: Option<ValueSnapshot>,
}
#[derive(Clone, Debug, Serialize)]
pub struct FrameSnapshot {
    pub receiver: u32,
    pub class: u32,
    pub signature: u32,
    pub body: Option<u32>,
    pub node: Option<usize>,
    pub locals: Vec<CellSnapshot>,
    pub stack: Vec<CellSnapshot>,
    pub locals_total: usize,
    pub stack_total: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct ObjectSnapshot {
    pub word: u32,
    pub class: String,
    pub fields: Vec<ValueSnapshot>,
    pub field_names: Vec<String>,
    pub fields_total: usize,
    pub elements: Vec<ValueSnapshot>,
    pub elements_total: usize,
    pub string_bytes: Option<Vec<u8>>,
    pub string_bytes_total: Option<usize>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct FailureSnapshot {
    pub thread: u64,
    pub flags: u32,
    pub live: i32,
    pub body: Option<u32>,
    pub node: Option<usize>,
    pub frames: Vec<FrameSnapshot>,
    pub frames_total: usize,
    pub objects: Vec<ObjectSnapshot>,
    pub objects_total: usize,
    pub driver_cursor: Option<usize>,
}

#[derive(Default)]
struct State {
    run_id: String,
    capacity: usize,
    sequence: u64,
    calls: u64,
    context: Context,
    recent: VecDeque<Event>,
    sink: Option<fn(&Event)>,
    notifying: bool,
    ring_evicted: u64,
    reentrant_not_streamed: u64,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub fn configure(run_id: String, capacity: usize, sink: Option<fn(&Event)>) {
    STATE.with(|s| {
        *s.borrow_mut() = State {
            run_id,
            capacity,
            sink,
            ..State::default()
        }
    });
}
pub fn enabled() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        s.capacity > 0 || s.sink.is_some()
    })
}
pub fn current_context() -> Context {
    STATE.with(|s| s.borrow().context.clone())
}
pub fn set_frame(frame: Option<u64>) {
    STATE.with(|s| s.borrow_mut().context.frame = frame);
}
struct RestoreContext(Context);
impl Drop for RestoreContext {
    fn drop(&mut self) {
        STATE.with(|s| s.borrow_mut().context = self.0.clone());
    }
}
/// Overlay populated fields; restore the complete previous context on unwind.
pub fn with_context<R>(overlay: Context, operation: impl FnOnce() -> R) -> R {
    let previous=STATE.with(|s| { let mut s=s.borrow_mut(); let previous=s.context.clone();
        // A child thread must not inherit an absent name or PC from its caller.
        if overlay.thread.is_some() {
            s.context.thread_name = overlay.thread_name.clone();
            s.context.location = overlay.location.clone();
        }
        macro_rules! overlay { ($($field:ident),*) => { $(if overlay.$field.is_some() { s.context.$field=overlay.$field; })* }; }
        overlay!(frame,thread,thread_name,location,native,parent_sequence); previous });
    let _restore = RestoreContext(previous);
    operation()
}
struct RestoreNotify;
impl Drop for RestoreNotify {
    fn drop(&mut self) {
        STATE.with(|s| s.borrow_mut().notifying = false);
    }
}
/// Sink runs outside storage borrows. Nested events are retained but do not recursively notify.
pub fn emit(kind: EventKind) -> Option<u64> {
    let (event, sink) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.capacity == 0 && s.sink.is_none() {
            return (None, None);
        }
        s.sequence += 1;
        let event = Event {
            schema_version: 1,
            run_id: s.run_id.clone(),
            sequence: s.sequence,
            context: s.context.clone(),
            kind,
        };
        if s.capacity > 0 {
            while s.recent.len() >= s.capacity {
                s.recent.pop_front();
                s.ring_evicted += 1;
            }
            s.recent.push_back(event.clone());
        }
        if s.notifying && s.sink.is_some() {
            s.reentrant_not_streamed += 1;
        }
        let sink = if s.notifying { None } else { s.sink };
        if sink.is_some() {
            s.notifying = true;
        }
        (Some(event), sink)
    });
    let event = event?;
    if let Some(sink) = sink {
        let _restore = RestoreNotify;
        sink(&event);
    }
    Some(event.sequence)
}
pub fn next_call_id() -> u64 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.calls += 1;
        s.calls
    })
}
#[derive(Clone, Debug, Serialize)]
pub struct Stats {
    pub events_total: u64,
    pub ring_retained: usize,
    pub ring_evicted: u64,
    pub reentrant_not_streamed: u64,
    pub capacity: usize,
}
pub fn stats() -> Stats {
    STATE.with(|s| {
        let s = s.borrow();
        Stats {
            events_total: s.sequence,
            ring_retained: s.recent.len(),
            ring_evicted: s.ring_evicted,
            reentrant_not_streamed: s.reentrant_not_streamed,
            capacity: s.capacity,
        }
    })
}
pub fn recent() -> Vec<Event> {
    STATE.with(|s| s.borrow().recent.iter().cloned().collect())
}
pub fn thread_context<H: Host>(thread: &Thread, host: &H) -> Context {
    Context {
        thread: Some(thread.clock_identity),
        thread_name: thread.clock_name.clone(),
        location: thread.pc.map(|pc| ScriptLocation {
            body: pc.body.0,
            node: pc.idx as u64,
            line: Some(thread.line),
            label: host.body_label(pc.body),
        }),
        ..Context::default()
    }
}
pub fn snapshot(host: &crate::host::VmHost) -> Vec<ObjectSnapshot> {
    host.diagnostic_snapshot()
}
pub fn failure_snapshot<H: Host>(host: &H, thread: &Thread) -> FailureSnapshot {
    let cell = |slot: &Slot| {
        let value = match slot {
            Slot::Value(v) => Some(*v),
            Slot::Var(id) => thread.vars.try_get(*id).map(|v| v.value),
            Slot::Heap(slot) => host.diagnostic_cell(*slot),
            _ => None,
        };
        CellSnapshot {
            address: format!("{slot:?}"),
            value: value.map(Into::into),
        }
    };
    FailureSnapshot {
        thread: thread.clock_identity,
        flags: thread.flags,
        live: thread.live,
        body: thread.pc.map(|pc| pc.body.0),
        node: thread.pc.map(|pc| pc.idx),
        frames_total: thread.frames.len(),
        frames: thread
            .frames
            .iter()
            .rev()
            .take(16)
            .map(|frame| FrameSnapshot {
                receiver: frame.receiver,
                class: frame.class_ctx,
                signature: frame.signature,
                body: frame.saved_pc.map(|pc| pc.body.0),
                node: frame.saved_pc.map(|pc| pc.idx),
                locals: frame
                    .locals
                    .iter()
                    .take(64)
                    .map(|id| cell(&Slot::Var(*id)))
                    .collect(),
                stack: frame.stack.iter().rev().take(64).map(cell).collect(),
                locals_total: frame.locals.len(),
                stack_total: frame.stack.len(),
            })
            .collect(),
        objects: host.diagnostic_objects(thread),
        objects_total: host.diagnostic_objects_total(thread),
        driver_cursor: crate::clock::current_driver_cursor(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn external() -> EventKind {
        EventKind::External {
            category: "test".into(),
            fields: vec![],
        }
    }
    #[test]
    fn child_without_name_or_pc_does_not_inherit_parent_script_location() {
        configure("context".into(), 4, None);
        with_context(
            Context {
                thread: Some(1),
                thread_name: Some("parent".into()),
                location: Some(ScriptLocation {
                    body: 4,
                    node: 7,
                    line: Some(9),
                    label: "parent.run()V".into(),
                }),
                ..Default::default()
            },
            || {
                with_context(
                    Context {
                        thread: Some(2),
                        ..Default::default()
                    },
                    || {
                        assert_eq!(current_context().thread, Some(2));
                        assert!(current_context().thread_name.is_none());
                        assert!(current_context().location.is_none());
                    },
                );
                assert_eq!(current_context().location.unwrap().node, 7);
            },
        );
        configure(String::new(), 0, None);
    }
    #[test]
    fn observations_preserve_execution_and_clock_consumption() {
        use crate::interp::{BodyId, Frame, Pc};
        struct H(Vec<slr_fmt_tufa::Node>);
        impl Host for H {
            fn body(&self, _: BodyId) -> &[slr_fmt_tufa::Node] {
                &self.0
            }
        }
        fn run(capacity: usize) -> (Vec<u8>, u32, usize, Vec<Event>) {
            configure("run".into(), capacity, None);
            let mut host = H(vec![slr_fmt_tufa::Node {
                opword: 0x0010,
                operand: 42,
                imm: 0,
            }]);
            let mut thread = Thread::new();
            let pc = Some(Pc {
                body: BodyId(9),
                idx: 0,
            });
            thread.frames.push(Frame::new(pc, 0));
            thread.pc = pc;
            let mut reads = 0;
            let value = thread
                .run(&mut host, 0.0, &mut || {
                    reads += 1;
                    slr_x87::X87::from_f64(reads as f64)
                })
                .unwrap();
            let result = (value.to_bytes().to_vec(), thread.flags, reads, recent());
            configure(String::new(), 0, None);
            result
        }
        let plain = run(0);
        let observed = run(32);
        assert_eq!(
            (&plain.0, plain.1, plain.2),
            (&observed.0, observed.1, observed.2)
        );
        assert_eq!(observed.2, 2);
        assert!(observed.3.iter().any(|event| matches!(
            event.kind,
            EventKind::Step { opcode: 0x0010, .. }
        ) && event.context.location.as_ref().unwrap().line
            == Some(42)));
        assert_eq!(
            observed
                .3
                .iter()
                .filter(|event| matches!(event.kind, EventKind::Clock { .. }))
                .count(),
            2
        );
    }
    #[test]
    fn failure_captures_real_pc_and_nan_without_running_extra_code() {
        use crate::interp::{BodyId, Frame, Pc};
        struct H([slr_fmt_tufa::Node; 1]);
        impl Host for H {
            fn body(&self, _: BodyId) -> &[slr_fmt_tufa::Node] {
                &self.0
            }
        }
        configure("failure".into(), 16, None);
        let mut host = H([slr_fmt_tufa::Node {
            opword: 0,
            operand: 37,
            imm: 0,
        }]);
        let mut thread = Thread::new();
        let pc = Some(Pc {
            body: BodyId(5),
            idx: 0,
        });
        let mut frame = Frame::new(pc, 77);
        frame.stack.push(Slot::Value(Value {
            tag: crate::values::TypeTag::Float,
            payload: 0xffc01234,
        }));
        thread.frames.push(frame);
        thread.pc = pc;
        let mut reads = 0;
        assert!(thread
            .run(&mut host, 0.0, &mut || {
                reads += 1;
                slr_x87::X87::from_f64(0.0)
            })
            .is_err());
        assert_eq!(reads, 1);
        let events = recent();
        let event = events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Failure { .. }))
            .unwrap();
        assert_eq!(event.context.location.as_ref().unwrap().line, Some(37));
        let EventKind::Failure { snapshot, .. } = &event.kind else {
            unreachable!()
        };
        assert_eq!(snapshot.body, Some(5));
        assert_eq!(snapshot.node, Some(0));
        assert_eq!(
            snapshot.frames[0].stack[0]
                .value
                .as_ref()
                .unwrap()
                .float_bits,
            Some(0xffc01234)
        );
        assert_eq!(snapshot.flags & crate::interp::flags::RUNNING, 0);
        configure(String::new(), 0, None);
    }
    #[test]
    fn bounded_sequence_and_reentrant_sink() {
        fn sink(_: &Event) {
            assert!(!recent().is_empty());
            emit(external());
        }
        configure("test".into(), 2, Some(sink));
        assert_eq!(emit(external()), Some(1));
        assert_eq!(
            recent().iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![1, 2]
        );
        emit(external());
        assert_eq!(
            recent().iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert_eq!(stats().ring_evicted, 2);
        assert_eq!(stats().reentrant_not_streamed, 2);
        configure(String::new(), 0, None);
    }
    #[test]
    fn context_restores_on_unwind_and_preserves_float_bits() {
        configure("test".into(), 2, None);
        set_frame(Some(9));
        let _ = std::panic::catch_unwind(|| {
            with_context(
                Context {
                    thread: Some(7),
                    ..Context::default()
                },
                || panic!("unwind"),
            )
        });
        assert_eq!(current_context().thread, None);
        assert_eq!(current_context().frame, Some(9));
        let value = ValueSnapshot::from(Value {
            tag: crate::values::TypeTag::Float,
            payload: 0xffc01234,
        });
        assert_eq!(value.float_bits, Some(0xffc01234));
        configure(String::new(), 0, None);
    }
}
