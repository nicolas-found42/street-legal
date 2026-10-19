//! Data-free tests of the interpreter loop, the frame/call protocol and the
//! host seam, on synthetic programs. Expected values are worked examples
//! (small integers, exactly representable floats), not recomputations of
//! the code under test. The exe-backed checks of the same loop live in
//! `thread_oracle.rs` and `vm_oracle.rs`.

use std::cell::Cell;

use slr_fmt_tufa::Node;
use slr_vm::interp::{
    BodyId, ExternKind, ExternOp, Flow, Frame, Host, Pc, ScriptMethod, Slot, Thread, VmError,
};
use slr_vm::values::{TypeTag, Value};
use slr_x87::X87;

fn n(opword: u16, imm: u32) -> Node {
    Node {
        opword,
        operand: 1,
        imm,
    }
}

fn lit_i(v: i32) -> [Node; 2] {
    [n(0x1007, 0x1c), n(0x100b, v as u32)]
}

fn lit_f(v: f32) -> [Node; 2] {
    [n(0x1007, 0x1c), n(0x200a, v.to_bits())]
}

const STOP: Node = Node {
    opword: 0x0010,
    operand: 1,
    imm: 0,
};

#[derive(Default)]
struct Plain {
    bodies: Vec<Vec<Node>>,
}

impl Host for Plain {
    fn body(&self, id: BodyId) -> &[Node] {
        &self.bodies[id.0 as usize]
    }
}

fn entry(body: u32, idx: usize) -> Option<Pc> {
    Some(Pc {
        body: BodyId(body),
        idx,
    })
}

fn run_one(nodes: Vec<Node>, locals: &[Value]) -> (Thread, Result<X87, VmError>) {
    let mut th = Thread::new();
    let mut f = Frame::new(entry(0, 0), 0);
    for v in locals {
        let id = th.new_var(*v, 0);
        f.locals.push(id);
    }
    th.frames.push(f);
    th.pc = entry(0, 0);
    let mut host = Plain {
        bodies: vec![nodes],
    };
    let r = th.run(&mut host, 0.0, &mut || X87::from_f64(0.0));
    (th, r)
}

fn stack_values(th: &Thread) -> Vec<Value> {
    th.frames
        .last()
        .unwrap()
        .stack
        .iter()
        .map(|s| match s {
            Slot::Value(v) => *v,
            other => panic!("expected a value, found {other:?}"),
        })
        .collect()
}

#[test]
fn integer_expression_evaluates_left_to_right() {
    // (7 - 3) * 2
    let mut p = Vec::new();
    p.extend(lit_i(7));
    p.extend(lit_i(3));
    p.push(n(0x1007, 0xf));
    p.extend(lit_i(2));
    p.push(n(0x1007, 0x13));
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    assert_eq!(stack_values(&th), vec![Value::int(8)]);
    assert_eq!(th.live, 1, "only the result cell is alive");
    assert_eq!(th.flags, 0x40, "stopped, no longer running");
}

#[test]
fn mixed_int_float_promotes_to_float() {
    let mut p = Vec::new();
    p.extend(lit_i(1));
    p.extend(lit_f(0.5));
    p.push(n(0x1007, 0x10));
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    assert_eq!(stack_values(&th), vec![Value::float(1.5)]);
}

#[test]
fn comparison_yields_an_int_truth_value() {
    let mut p = Vec::new();
    p.extend(lit_i(2));
    p.extend(lit_i(5));
    p.push(n(0x1007, 0xb)); // 2 < 5
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    assert_eq!(stack_values(&th), vec![Value::int(1)]);
}

#[test]
fn compound_assignment_writes_through_the_lvalue() {
    // x += 3 as an expression (value below, lvalue on top), then x++ (old
    // value pushed).
    let mut p = Vec::new();
    p.extend(lit_i(3));
    p.push(n(0x1001, 0));
    p.push(n(0x1007, 0x27));
    p.push(n(0x1001, 0));
    p.push(n(0x1007, 0x17));
    p.push(STOP);
    let (th, r) = run_one(p, &[Value::int(5)]);
    r.unwrap();
    let id = th.frames[0].locals[0];
    assert_eq!(th.vars.get(id).value, Value::int(9));
    assert_eq!(stack_values(&th), vec![Value::int(3), Value::int(8)]);
}

#[test]
fn assignment_to_a_plain_value_is_a_script_error() {
    let mut p = Vec::new();
    p.extend(lit_i(1));
    p.extend(lit_i(2));
    p.push(n(0x1007, 0x23));
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    assert_eq!(r.unwrap_err(), VmError::NotLvalue);
    assert_eq!(th.flags & 0x04, 0, "an errored thread is not left running");
}

#[test]
fn conditional_jump_counts_the_jump_node_itself() {
    // false condition: jump by 3 lands after the skipped pair.
    let mut p = Vec::new();
    p.extend(lit_i(0));
    p.push(n(0x1015, 3));
    p.push(n(0x1027, 111)); // skipped
    p.push(n(0x0018, 0)); // skipped
    p.push(n(0x1027, 222));
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    assert_eq!(stack_values(&th), vec![Value::int(222)]);
}

#[test]
fn short_circuit_or_skips_the_right_operand_on_true() {
    // 1 || (push 7)
    let mut p = Vec::new();
    p.extend(lit_i(5));
    p.push(n(0x0004, 0));
    p.push(n(0x1027, 7)); // skipped
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    // the tested value stays on the stack as a boolean 0/1
    assert_eq!(stack_values(&th), vec![Value::int(5)]);
}

#[test]
fn bad_opcodes_are_fatal_and_traps_are_reported() {
    let (_, r) = run_one(vec![n(0x0000, 0)], &[]);
    assert_eq!(r.unwrap_err(), VmError::FatalOpcode(0));
    let mut p = Vec::new();
    p.extend(lit_i(5));
    p.extend(lit_i(0));
    p.push(n(0x1007, 0x12));
    p.push(STOP);
    let (_, r) = run_one(p, &[]);
    assert!(matches!(r.unwrap_err(), VmError::IntegerTrap { .. }));
    // float division by zero does not trap
    let mut p = Vec::new();
    p.extend(lit_f(1.0));
    p.extend(lit_f(0.0));
    p.push(n(0x1007, 0x12));
    p.push(STOP);
    let (th, r) = run_one(p, &[]);
    r.unwrap();
    assert_eq!(stack_values(&th), vec![Value::float(f32::INFINITY)]);
}

#[test]
fn return_with_value_resumes_the_caller_after_the_call_node() {
    let caller = vec![n(0x0029, 0), n(0x1027, 1), STOP];
    let callee = vec![n(0x1027, 5), n(0x002b, 0)];
    let mut th = Thread::new();
    th.frames.push(Frame::new(entry(0, 0), 0)); // suspended at node 0
    th.frames.push(Frame::new(entry(1, 0), 0));
    th.pc = entry(1, 0);
    let mut host = Plain {
        bodies: vec![caller, callee],
    };
    th.run(&mut host, 0.0, &mut || X87::from_f64(0.0)).unwrap();
    assert_eq!(th.frames.len(), 1, "callee frame popped");
    assert_eq!(stack_values(&th), vec![Value::int(5), Value::int(1)]);
}

#[test]
fn returning_into_a_null_pc_stops_the_thread() {
    let callee = vec![n(0x002a, 0)];
    let mut th = Thread::new();
    th.frames.push(Frame::new(None, 0));
    th.frames.push(Frame::new(entry(0, 0), 0));
    th.pc = entry(0, 0);
    let mut host = Plain {
        bodies: vec![callee],
    };
    th.run(&mut host, 0.0, &mut || X87::from_f64(0.0)).unwrap();
    assert_eq!(th.flags & 0x40, 0x40);
    assert_eq!(th.pc, None);
}

#[test]
fn clock_is_read_at_entry_every_256_steps_and_at_exit() {
    let calls = Cell::new(0u32);
    let mut p = Vec::new();
    for _ in 0..300 {
        p.push(n(0x1027, 1));
        p.push(n(0x1008, 0x1b));
    }
    p.push(STOP);
    let mut th = Thread::new();
    th.frames.push(Frame::new(entry(0, 0), 0));
    th.pc = entry(0, 0);
    let mut host = Plain { bodies: vec![p] };
    // budget 5.0: expires only when the clock reads 5.0 or more
    let mut clock = || {
        calls.set(calls.get() + 1);
        X87::from_f64(if calls.get() >= 4 { 99.0 } else { 0.0 })
    };
    th.run(&mut host, 5.0, &mut clock).unwrap();
    // Clock reads: entry (1), checks after steps 1, 257 and 513 (2..4;
    // the fourth read returns 99.0, past the deadline), exit (5).
    assert_eq!(calls.get(), 5);
    assert_eq!(th.flags & 0x40, 0, "ran out of time, did not stop");
    assert_eq!(th.pc.unwrap().idx, 513);
}

#[derive(Default)]
struct Recording {
    bodies: Vec<Vec<Node>>,
    seen: Vec<(ExternKind, bool)>,
    stores: Vec<Value>,
}

impl Host for Recording {
    fn body(&self, id: BodyId) -> &[Node] {
        &self.bodies[id.0 as usize]
    }
    fn extern_op(&mut self, th: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        self.seen.push((op.kind, op.discard));
        match op.kind {
            ExternKind::New => {
                th.push_new(Value::reference(0x1234))?;
                Ok(Flow::Advance)
            }
            _ => Err(VmError::Unsupported(op.node.opword)),
        }
    }
    fn on_reference_store(&mut self, v: Value) {
        self.stores.push(v);
    }
}

#[test]
fn class_dependent_ops_go_through_the_host() {
    let p = vec![n(0x1007, 0x1e), n(0x1006, 0), STOP];
    let mut th = Thread::new();
    th.frames.push(Frame::new(entry(0, 0), 0));
    th.pc = entry(0, 0);
    let mut host = Recording {
        bodies: vec![p],
        ..Default::default()
    };
    th.run(&mut host, 0.0, &mut || X87::from_f64(0.0)).unwrap();
    assert_eq!(host.seen, vec![(ExternKind::New, false)]);
    assert_eq!(stack_values(&th), vec![Value::reference(0x1234)]);
    // The default host refuses what it does not implement.
    let (_, r) = run_one(vec![n(0x4025, 0), STOP], &[]);
    assert_eq!(r.unwrap_err(), VmError::Unsupported(0x4025));
}

#[test]
fn reference_stores_reach_the_write_barrier_hook() {
    let p = vec![
        n(0x1007, 0x1c),
        n(0x000c, 0),
        n(0x1001, 0),
        n(0x1007, 0x23),
        STOP,
    ];
    let mut th = Thread::new();
    let id = th.new_var(Value::reference(0), 0);
    let mut f = Frame::new(entry(0, 0), 0);
    f.locals.push(id);
    th.frames.push(f);
    th.pc = entry(0, 0);
    let mut host = Recording {
        bodies: vec![p],
        ..Default::default()
    };
    th.run(&mut host, 0.0, &mut || X87::from_f64(0.0)).unwrap();
    assert_eq!(host.stores, vec![Value::reference(0)]);
}

#[test]
fn script_method_call_builds_this_and_pops_arguments_last_first() {
    let mut th = Thread::new();
    let mut caller = Frame::new(entry(0, 0), 0);
    caller.stack = vec![
        Slot::Value(Value::int(10)),    // argument 0 (int)
        Slot::Value(Value::float(2.5)), // argument 1 (float)
    ];
    th.live = 2;
    th.frames.push(caller);
    th.pc = entry(0, 4);
    let m = ScriptMethod {
        body: BodyId(1),
        class_ctx: 0xc1a55,
        signature: 0x516,
        is_static: false,
        params: vec![TypeTag::Int, TypeTag::Float],
    };
    th.call_script_method(&m, 0x77).unwrap();
    assert_eq!(th.frames.len(), 2);
    assert_eq!(th.frames[0].saved_pc, entry(0, 4), "caller pc saved");
    assert!(th.frames[0].stack.is_empty(), "arguments consumed");
    assert_eq!(th.pc, entry(1, 0), "resumes at the callee entry");
    let callee = &th.frames[1];
    assert_eq!(
        (callee.receiver, callee.class_ctx, callee.signature),
        (0x77, 0xc1a55, 0x516)
    );
    // locals: this, then the parameters LAST first
    let vals: Vec<Value> = callee
        .locals
        .iter()
        .map(|id| th.vars.get(*id).value)
        .collect();
    assert_eq!(
        vals,
        vec![Value::reference(0x77), Value::float(2.5), Value::int(10)]
    );
    // two argument cells released, three variable slots created
    assert_eq!(th.live, 3);
}

#[test]
fn call_converts_arguments_to_the_declared_parameter_type() {
    let mut th = Thread::new();
    let mut caller = Frame::new(entry(0, 0), 0);
    caller.stack = vec![Slot::Value(Value::int(3))];
    th.frames.push(caller);
    th.pc = entry(0, 0);
    let m = ScriptMethod {
        body: BodyId(1),
        class_ctx: 0,
        signature: 0,
        is_static: true,
        params: vec![TypeTag::Float],
    };
    th.call_script_method(&m, 0).unwrap();
    let id = th.frames[1].locals[0];
    assert_eq!(th.vars.get(id).value, Value::float(3.0));
}

// ---- review fixes (m1-03-fixes) -----------------------------------------

#[test]
fn variable_slot_arena_stays_bounded_over_a_call_loop() {
    // Every script call creates `this` plus one slot per parameter and the
    // return frees them; the arena must reuse the freed slots (the exe's
    // variable pool recycles through a free list).
    let mut th = Thread::new();
    th.frames.push(Frame::new(entry(0, 0), 0));
    th.pc = entry(0, 0);
    let m = ScriptMethod {
        body: BodyId(1),
        class_ctx: 0,
        signature: 0,
        is_static: false,
        params: vec![TypeTag::Int, TypeTag::Float],
    };
    for i in 0..2000 {
        th.frames[0].stack = vec![Slot::Value(Value::int(i)), Slot::Value(Value::float(1.5))];
        th.live += 2;
        th.pc = entry(0, 0);
        th.call_script_method(&m, 0x77).unwrap();
        th.pop_frame().unwrap();
    }
    assert_eq!(th.frames.len(), 1);
    assert!(
        th.vars.slot_count() <= 3,
        "arena grew to {} slots",
        th.vars.slot_count()
    );
    assert_eq!(th.live, 0, "every cell released again");
}

#[test]
fn a_stale_variable_handle_is_not_revived_by_slot_reuse() {
    let mut th = Thread::new();
    let mut f = Frame::new(entry(0, 0), 0);
    let old = th.new_var(Value::int(1), 0);
    f.locals.push(old);
    th.frames.push(f);
    th.pop_frame().unwrap(); // frees the slot
    let fresh = th.new_var(Value::int(2), 0); // reuses the storage
    assert!(th.vars.try_get(old).is_none(), "old handle must stay dead");
    assert_eq!(th.vars.try_get(fresh).map(|v| v.value), Some(Value::int(2)));
    assert_eq!(th.vars.slot_count(), 1);
}

#[test]
fn instance_method_without_receiver_is_a_fatal_script_error() {
    // FUN_0041fbc0 checks this before touching any state.
    let mut th = Thread::new();
    let mut caller = Frame::new(entry(0, 0), 0);
    caller.stack = vec![Slot::Value(Value::int(1))];
    th.frames.push(caller);
    th.pc = entry(0, 3);
    let m = ScriptMethod {
        body: BodyId(1),
        class_ctx: 0,
        signature: 0,
        is_static: false,
        params: vec![TypeTag::Int],
    };
    let err = th.call_script_method(&m, 0).unwrap_err();
    assert_eq!(err, VmError::InstanceMethodWithoutReceiver);
    assert!(err.is_script_fatal());
    assert_eq!(th.frames.len(), 1, "no frame was created");
    assert_eq!(th.frames[0].stack.len(), 1, "arguments untouched");
    assert_eq!(th.frames[0].saved_pc, entry(0, 0), "caller pc not saved");
    // a static method with receiver 0 is fine
    let s = ScriptMethod {
        is_static: true,
        ..m
    };
    th.call_script_method(&s, 0).unwrap();
}

// ---- reference operators reach the host, also as compound assignments ----

use slr_vm::interp::RefOperator;
use slr_vm::ops::{Diagnostic, LogKind};

/// A fake string model: a reference payload `p` "concatenated" with an int
/// `n` becomes `p * 10 + n`.
#[derive(Default)]
struct FakeStrings {
    bodies: Vec<Vec<Node>>,
    seen: Vec<(RefOperator, Value, Value)>,
}

impl Host for FakeStrings {
    fn body(&self, id: BodyId) -> &[Node] {
        &self.bodies[id.0 as usize]
    }
    fn operator_on_refs(
        &mut self,
        op: RefOperator,
        left: &mut Value,
        right: &mut Value,
    ) -> Option<Result<(), VmError>> {
        self.seen.push((op, *left, *right));
        left.payload = left.payload.wrapping_mul(10).wrapping_add(right.payload);
        Some(Ok(()))
    }
}

fn run_with<H: Host>(host: &mut H, locals: &[Value]) -> Thread {
    let mut th = Thread::new();
    let mut f = Frame::new(entry(0, 0), 0);
    for v in locals {
        let id = th.new_var(*v, 0);
        f.locals.push(id);
    }
    th.frames.push(f);
    th.pc = entry(0, 0);
    th.run(host, 0.0, &mut || X87::from_f64(0.0)).unwrap();
    th
}

fn fake(nodes: Vec<Node>) -> FakeStrings {
    FakeStrings {
        bodies: vec![nodes],
        ..Default::default()
    }
}

#[test]
fn string_plus_assign_reaches_the_host_with_the_variable_as_left() {
    // s += 3: [value][lvalue] 0x1007/0x27
    let mut p = Vec::new();
    p.extend(lit_i(3));
    p.push(n(0x1001, 0));
    p.push(n(0x1007, 0x27));
    p.push(STOP);
    let mut host = fake(p);
    let th = run_with(&mut host, &[Value::reference(7)]);
    assert_eq!(
        host.seen,
        vec![(RefOperator::AddAssign, Value::reference(7), Value::int(3))]
    );
    let id = th.frames[0].locals[0];
    assert_eq!(th.vars.get(id).value, Value::reference(73), "written back");
    assert_eq!(stack_values(&th), vec![Value::int(3)], "value pushed back");
}

#[test]
fn string_minus_assign_negates_the_operand_then_concatenates() {
    // The exe's 0x28 is uminus(src) followed by the same helper as 0x27.
    let mut p = Vec::new();
    p.extend(lit_i(3));
    p.push(n(0x1001, 0));
    p.push(n(0x1007, 0x28));
    p.push(STOP);
    let mut host = fake(p);
    let th = run_with(&mut host, &[Value::reference(7)]);
    assert_eq!(
        host.seen,
        vec![(RefOperator::AddAssign, Value::reference(7), Value::int(-3))]
    );
    let id = th.frames[0].locals[0];
    assert_eq!(th.vars.get(id).value, Value::reference(67));
}

#[test]
fn string_plus_and_minus_expressions_reach_the_host_as_add() {
    for (sub, right) in [(0x10u32, 3), (0xf, -3)] {
        let mut p = vec![n(0x1007, 0x1c), n(0x000c, 0)];
        p.extend(lit_i(3));
        p.push(n(0x1007, sub));
        p.push(STOP);
        let mut host = fake(p);
        let th = run_with(&mut host, &[]);
        assert_eq!(
            host.seen,
            vec![(RefOperator::Add, Value::reference(0), Value::int(right))],
            "sub-op {sub:#x}"
        );
        assert_eq!(stack_values(&th), vec![Value::reference(right as u32)]);
    }
}

#[test]
fn other_compound_kinds_on_references_never_reach_the_host() {
    // 0x23 (plain store), 0x24/0x25/0x26 and the bitwise kinds do not call
    // the exe's string-aware `+` helper (FUN_0041b4d0).
    for kind in [0x23u32, 0x24, 0x25, 0x26, 0x2c, 0x2d, 0x2e] {
        let mut p = Vec::new();
        p.extend(lit_i(3));
        p.push(n(0x1001, 0));
        p.push(n(0x1007, kind));
        p.push(STOP);
        let mut host = fake(p);
        let _ = run_with(&mut host, &[Value::reference(7)]);
        assert!(host.seen.is_empty(), "kind {kind:#x}");
    }
}

#[test]
fn without_a_string_model_reference_plus_logs_and_leaves_the_variable() {
    let mut p = Vec::new();
    p.extend(lit_i(3));
    p.push(n(0x1001, 0));
    p.push(n(0x1007, 0x27));
    p.push(STOP);
    let mut host = Plain { bodies: vec![p] };
    let th = run_with(&mut host, &[Value::reference(7)]);
    let id = th.frames[0].locals[0];
    assert_eq!(th.vars.get(id).value, Value::reference(7));
    assert!(th.alu.diagnostics.iter().any(|d| matches!(
        d,
        Diagnostic::Logged {
            kind: LogKind::UnsupportedClassType,
            ..
        }
    )));
}
