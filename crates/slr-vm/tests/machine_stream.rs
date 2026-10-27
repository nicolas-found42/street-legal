//! Causal recording contract at live VM boundaries. These checks do not
//! establish original machine-memory or interpreter differential acceptance.
use slr_fmt_tufa::{Node, PoolEntry};
use slr_vm::{
    host::{CallInfo, VmHost},
    interp::{flags, BodyId, Frame, Host, Pc, Thread, VmError},
    machine::{Event, FramePhase, Operand, OperandAction, Trace},
    values::{PoolEvent, Value},
    vmclass::{Class, Method},
};
use slr_x87::X87;

fn node(opword: u16, imm: u32) -> Node {
    Node {
        opword,
        operand: 1,
        imm,
    }
}
fn empty_class(name: &str, base: Option<&str>) -> Class {
    Class {
        kind: 0,
        name: name.into(),
        base: base.map(str::to_owned),
        interfaces: vec![],
        methods: vec![],
        fields: vec![],
        bodies: vec![],
    }
}
fn run(thread: &mut Thread, host: &mut impl Host) -> Result<(), VmError> {
    thread.run(host, 0., &mut || X87::from_f64(0.)).map(|_| ())
}
struct Code(Vec<Node>);
impl Host for Code {
    fn body(&self, _: BodyId) -> &[Node] {
        &self.0
    }
}
fn entered(vm: &VmHost) -> Thread {
    let mut thread = vm.new_thread();
    let pc = Some(Pc {
        body: BodyId(0),
        idx: 0,
    });
    thread.push_frame(Frame::new(pc, 0)).unwrap();
    thread.pc = pc;
    thread
}

#[test]
fn recording_rejects_start_after_unobserved_owner_or_metadata_activity() {
    let vm = VmHost::new();
    let _thread = vm.new_thread();
    assert!(vm.start_machine_recording().is_err());

    let mut vm = VmHost::new();
    vm.loader
        .insert_test_class(empty_class("BeforeRecording", None));
    assert!(vm.start_machine_recording().is_err());

    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    assert!(vm.start_machine_recording().is_err());
}

#[test]
fn pending_is_repeatable_and_acknowledges_only_a_real_prefix_without_ring_loss() {
    let trace = Trace::default();
    trace.start().unwrap();
    for identity in 0..20_000 {
        trace.note(Event::ThreadCreate { identity });
    }
    let prefix = trace.pending(3);
    assert_eq!(prefix, trace.pending(3));
    assert_eq!(
        prefix.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(trace.acknowledge(20_000).is_err());
    assert_eq!(trace.pending(usize::MAX).len(), 20_000);
    trace.acknowledge(1).unwrap();
    assert!(trace.acknowledge(1).is_err());
    let remaining = trace.pending(usize::MAX);
    assert_eq!(remaining.len(), 19_998);
    for (expected, entry) in (2..20_000).zip(remaining) {
        assert_eq!(entry.sequence, expected);
        assert_eq!(entry.event, Event::ThreadCreate { identity: expected });
    }
    trace.acknowledge(19_999).unwrap();
    assert!(trace.pending(usize::MAX).is_empty());
    trace.note(Event::ThreadCreate { identity: 20_000 });
    assert_eq!(trace.pending(1)[0].sequence, 20_000);
}

#[test]
fn foreign_thread_and_unrecorded_direct_frame_are_rejected_before_execution() {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut foreign = Thread::new();
    let snapshot = vm.machine_trace().pending(usize::MAX);
    assert_eq!(run(&mut foreign, &mut vm), Err(VmError::MachineContext));
    assert_eq!(vm.machine_trace().pending(usize::MAX), snapshot);

    let mut owned = vm.new_thread();
    owned.frames.push(Frame::new(
        Some(Pc {
            body: BodyId(0),
            idx: 0,
        }),
        0,
    ));
    let snapshot = vm.machine_trace().pending(usize::MAX);
    assert_eq!(run(&mut owned, &mut vm), Err(VmError::MachineContext));
    assert_eq!(vm.machine_trace().pending(usize::MAX), snapshot);
}

#[test]
fn recording_rejects_prepopulated_fresh_frames_without_inventing_attachments() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut thread = vm.new_thread();
    let mut frame = Frame::new(None, 0);
    frame.stack.push(slr_vm::interp::Slot::Null);
    let before = vm.machine_trace().pending(usize::MAX);
    assert_eq!(thread.push_frame(frame), Err(VmError::MachineContext));
    assert!(thread.frames.is_empty());
    assert_eq!(vm.machine_trace().pending(usize::MAX), before);
}

#[test]
fn direct_script_entry_rejects_a_foreign_recording_before_mutating_the_caller() {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut foreign = Thread::new();
    let pc = Some(Pc {
        body: BodyId(0),
        idx: 0,
    });
    foreign.push_frame(Frame::new(pc, 0)).unwrap();
    foreign.pc = pc;
    let before = vm.machine_trace().pending(usize::MAX);
    let method = slr_vm::interp::ScriptMethod {
        body: BodyId(1),
        class_ctx: 0,
        signature: 0,
        is_static: true,
        params: vec![],
    };
    assert_eq!(
        foreign.call_script_method_with(&mut vm, &method, 0),
        Err(VmError::MachineContext)
    );
    assert_eq!(foreign.frames.len(), 1);
    assert_eq!(foreign.pc, pc);
    assert_eq!(foreign.frames[0].saved_pc, pc);
    assert_eq!(vm.machine_trace().pending(usize::MAX), before);
}

#[test]
fn previously_entered_frames_cannot_move_between_threads_or_recordings() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut source = entered(&vm);
    let mut sibling = vm.new_thread();
    let frame = source.frames.pop().unwrap();
    let before = vm.machine_trace().pending(usize::MAX);
    assert_eq!(sibling.push_frame(frame), Err(VmError::MachineContext));
    assert!(sibling.frames.is_empty());
    assert_eq!(vm.machine_trace().pending(usize::MAX), before);

    let vm2 = VmHost::new();
    vm2.start_machine_recording().unwrap();
    let mut foreign = entered(&vm2);
    let frame = foreign.frames.pop().unwrap();
    assert_eq!(sibling.push_frame(frame), Err(VmError::MachineContext));

    // The legacy public Vec must not bypass validation at execution entry.
    let mut source = entered(&vm);
    sibling.frames.push(source.frames.pop().unwrap());
    sibling.pc = Some(Pc {
        body: BodyId(0),
        idx: 0,
    });
    let before = vm.machine_trace().pending(usize::MAX);
    assert_eq!(
        run(&mut sibling, &mut Code(vec![node(0x10, 0)])),
        Err(VmError::MachineContext)
    );
    assert_eq!(vm.machine_trace().pending(usize::MAX), before);
}

#[test]
fn opcode_exits_record_applied_pc_and_error_flags() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut thread = entered(&vm);
    let mut code = Code(vec![node(0x0029, 0), node(0x0000, 0)]);
    assert_eq!(run(&mut thread, &mut code), Err(VmError::FatalOpcode(0)));
    let events = vm.machine_trace().pending(usize::MAX);
    let exits = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::Opcode {
                enter: false,
                pc,
                next_pc,
                flags,
                error,
                ..
            } => Some((*pc, *next_pc, *flags, error.as_deref())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(exits.len(), 2);
    assert_eq!(exits[0].0.idx, 0);
    assert_eq!(exits[0].1.unwrap().idx, 1);
    assert!(exits[0].3.is_none());
    assert_eq!(exits[1].0.idx, 1);
    assert_eq!(exits[1].1.unwrap().idx, 1);
    assert_eq!(exits[1].2 & flags::RUNNING, 0);
    assert_eq!(exits[1].3, Some("FatalOpcode(0)"));
    assert!(matches!(&events.last().unwrap().event,
        Event::Run { enter: false, flags: stopped, error: Some(error), .. }
        if stopped & flags::RUNNING == 0 && error == "FatalOpcode(0)"));
}

#[test]
fn frame_operands_and_locals_keep_owner_identity_through_destruction() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut thread = entered(&vm);
    let frame = thread.frames[0].machine_identity();
    let variable = thread.new_var(Value::int(8), 0);
    thread.append_local(variable).unwrap();
    let cell = thread.allocate_value(Value::int(7));
    let identity = cell.id();
    thread.push_owned(cell).unwrap();
    let cell = thread.pop_deref().unwrap();
    assert_eq!(cell.id(), identity);
    thread.push_owned(cell).unwrap();
    thread.pop_frame().unwrap();
    let events = vm.machine_trace().pending(usize::MAX);
    let operands = events
        .iter()
        .filter_map(|entry| match entry.event {
            Event::Operand {
                thread: owner,
                frame: activation,
                action,
                slot,
            } => {
                assert_eq!(owner, thread.machine_identity);
                assert_eq!(activation, frame);
                Some((action, slot))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        operands,
        [
            (OperandAction::Push, Operand::Value(identity)),
            (OperandAction::Pop, Operand::Value(identity)),
            (OperandAction::Push, Operand::Value(identity)),
            (OperandAction::Drop, Operand::Value(identity)),
        ]
    );
    let allocation = events.iter().position(|entry| matches!(&entry.event,
        Event::Pool(PoolEvent::Allocate { cell, .. })
        if (cell.pool, cell.slot, cell.generation) == (identity.pool, identity.slot, identity.generation))).unwrap();
    let push = events
        .iter()
        .position(|entry| {
            matches!(
                entry.event,
                Event::Operand {
                    action: OperandAction::Push,
                    ..
                }
            )
        })
        .unwrap();
    assert!(
        allocation < push,
        "an operand cannot precede its allocation lifetime"
    );
    let local_events = events
        .iter()
        .filter_map(|entry| match entry.event {
            Event::Local {
                frame: activation,
                append,
                index,
                variable: actual,
                ..
            } => {
                assert_eq!((activation, index, actual), (frame, 0, variable));
                Some(append)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(local_events, [true, false]);
    let destroy = events
        .iter()
        .position(|entry| {
            matches!(
                entry.event,
                Event::Frame {
                    phase: FramePhase::Destroy,
                    ..
                }
            )
        })
        .unwrap();
    let release = events
        .iter()
        .rposition(|entry| matches!(entry.event, Event::Pool(PoolEvent::Release { .. })))
        .unwrap();
    assert!(release < destroy);
}

fn native(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    Some(0x1122_3344)
}
fn failing_native(vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    vm.native_failure = Some("executed backend failure".into());
    Some(0x5566_7788)
}
fn rejecting_boundary(
    _: &slr_vm::host::NativeContext<'_>,
    _info: &CallInfo<'_>,
) -> Result<(), String> {
    Err("test boundary rejection".into())
}

fn native_host() -> (VmHost, Thread, u32, usize) {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut base = empty_class("Base", None);
    for name in ["unselected", "method"] {
        base.methods.push(Method {
            class: "Base".into(),
            name: name.into(),
            sig: "()I".into(),
            flags: 0x49,
            static_vector: true,
            is_native: true,
            is_abstract: false,
            body: u32::MAX,
            extra: 0,
        });
    }
    vm.loader.insert_test_class(base);
    vm.loader
        .insert_test_class(empty_class("Leaf", Some("Base")));
    vm.natives.register("Base", "method", "()I", 0x41b860);
    vm.native_impls.insert(0x41b860, native);
    let body = vm
        .add_body(
            "Leaf",
            vec![
                node(0x24, 0),
                node(0x1011, 1),
                node(0x101a, 5),
                node(0x10, 0),
            ],
            vec![
                PoolEntry::Str("Base".into()),
                PoolEntry::Class(0),
                PoolEntry::Str("method".into()),
                PoolEntry::Str("()I".into()),
                PoolEntry::Type7 { a: 2, b: 3 },
                PoolEntry::Type5 { a: 1, b: 4 },
            ],
        )
        .unwrap();
    let defining = vm.class_context("Base").unwrap();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0);
    frame.class_ctx = vm.class_context("Leaf").unwrap();
    let mut thread = vm.new_thread();
    thread.push_frame(frame).unwrap();
    thread.pc = pc;
    thread.push_new(Value::int(0)).unwrap();
    (vm, thread, defining, 1)
}

#[test]
fn native_events_retain_selected_record_frame_current_pc_and_actual_result() {
    let (mut vm, mut thread, defining, index) = native_host();
    let frame = thread.frames[0].machine_identity();
    run(&mut thread, &mut vm).unwrap();
    let events = vm.machine_trace().pending(usize::MAX);
    let enters = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::NativeEnter {
                thread: owner,
                frame: activation,
                pc,
                defining_class,
                method_index,
                implementation,
                descriptor,
                cells,
                ..
            } => {
                assert_eq!(
                    (*owner, *activation),
                    (thread.machine_identity, Some(frame))
                );
                assert_eq!((*defining_class, *method_index), (defining, Some(index)));
                assert_eq!(*implementation, 0x41b860);
                assert_eq!(descriptor, "()I");
                assert_eq!(
                    pc.unwrap().idx,
                    2,
                    "method resolution has consumed name components"
                );
                assert_eq!(
                    cells.len(),
                    1,
                    "the actual static receiver owner is retained"
                );
                let receiver = cells[0];
                let allocation = events
                    .iter()
                    .find(|candidate| {
                        matches!(&candidate.event,
                    Event::Pool(PoolEvent::Allocate { cell, .. }) if *cell == receiver)
                    })
                    .unwrap();
                assert!(allocation.sequence < entry.sequence);
                Some(entry.sequence)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(enters.len(), 1);
    let returns = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::NativeReturn {
                executed,
                result,
                error,
                ..
            } => {
                assert!(*executed);
                assert_eq!(*result, Some(0x1122_3344));
                assert!(error.is_none());
                Some(entry.sequence)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(returns.len(), 1);
    assert!(enters[0] < returns[0]);
    let Event::NativeEnter { cells, .. } = &events
        .iter()
        .find(|entry| entry.sequence == enters[0])
        .unwrap()
        .event
    else {
        unreachable!()
    };
    for cell in cells {
        let release = events.iter().find(|entry| matches!(&entry.event,
            Event::Pool(PoolEvent::Release { cell: released, .. })
            if (released.pool, released.slot, released.generation) == (cell.pool, cell.slot, cell.generation))).unwrap();
        assert!(
            returns[0] < release.sequence,
            "arguments remain actual owners until native return"
        );
    }
}

#[test]
fn native_boundary_failure_records_no_executed_native_or_invented_return() {
    let (mut vm, mut thread, _, _) = native_host();
    vm.native_boundary = Some(rejecting_boundary);
    assert_eq!(run(&mut thread, &mut vm), Err(VmError::Unsupported(0x24)));
    let events = vm.machine_trace().pending(usize::MAX);
    let returns = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::NativeReturn {
                executed,
                result,
                error,
                ..
            } => {
                assert!(!executed);
                assert_eq!(*result, None);
                assert!(error
                    .as_deref()
                    .unwrap()
                    .contains("test boundary rejection"));
                Some(())
            }
            _ => None,
        })
        .count();
    assert_eq!(returns, 1);
}

#[test]
fn executed_native_failure_preserves_actual_result_and_backend_reason() {
    let (mut vm, mut thread, _, _) = native_host();
    vm.native_impls.insert(0x41b860, failing_native);
    assert_eq!(run(&mut thread, &mut vm), Err(VmError::Unsupported(0x24)));
    let events = vm.machine_trace().pending(usize::MAX);
    let returns = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::NativeReturn {
                executed,
                result,
                error,
                ..
            } => {
                assert!(*executed);
                assert_eq!(*result, Some(0x5566_7788));
                assert_eq!(error.as_deref(), Some("executed backend failure"));
                Some(())
            }
            _ => None,
        })
        .count();
    assert_eq!(returns, 1);
}

#[test]
fn array_mutability_and_implicit_cell_mutability_are_explicit_unresolved_paths() {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let word = vm.alloc_array("I", 3).unwrap();
    vm.array_cells_mut(word).unwrap()[2] = Value::int(9);
    let thread = vm.new_thread();
    let mut cell = thread.allocate_value(Value::int(1));
    let identity = cell.id();
    cell.payload = 2;
    let events = vm.machine_trace().pending(usize::MAX);
    assert!(events.iter().any(|entry| matches!(&entry.event,
        Event::ObjectAllocate { word: actual, array_descriptor: Some(descriptor), array_length: Some(3), .. }
        if *actual == word && descriptor == "[I")));
    assert!(events.iter().any(|entry| matches!(&entry.event,
        Event::UnresolvedMutation { owner } if owner == &format!("array_cells_mut:{word:#x}"))));
    assert!(events.iter().any(|entry| matches!(&entry.event,
        Event::CellWriteAccess { identity: actual, before } if *actual == identity && *before == Value::int(1))));
    assert!(!events.iter().any(|entry| matches!(&entry.event,
        Event::CellStore { identity: actual, .. } if *actual == identity)));
}

// Minimal authored TUFA modules: no stock-derived fixture or bytecode. File
// identity tests need parsed class records and real file-relative chunk spans.
fn empty_tufa_module(name: &str, version: u32) -> Vec<u8> {
    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|word| word.to_le_bytes()).collect()
    }
    let mut constants = words(&[2, 0, name.len() as u32]);
    constants.extend(name.as_bytes());
    constants.push(0);
    constants.extend(words(&[4, 0]));
    let mut bytes = b"TUFA".to_vec();
    bytes.extend(words(&[4, version]));
    for (tag, payload) in [
        (b"CONS", constants),
        (b"FILD", words(&[0, 0])),
        (b"MTHD", words(&[0, 0])),
        (b"CLSS", words(&[u32::MAX, 0, 1, u32::MAX, 0])),
        (b"TREE", words(&[0])),
    ] {
        bytes.extend(tag);
        bytes.extend(words(&[payload.len() as u32]));
        bytes.extend(payload);
    }
    bytes
}

#[test]
fn file_loads_share_file_identity_across_modules_and_never_deduplicate_equal_bytes() {
    let mut bytes = empty_tufa_module("StreamFirst", 0x11ae1);
    let second_start = bytes.len();
    bytes.extend(empty_tufa_module("StreamSecond", 0x11ae1));
    let modules = slr_fmt_tufa::parse(&bytes).unwrap();
    assert_eq!(modules.len(), 2);
    assert!(modules[1]
        .chunks
        .iter()
        .all(|chunk| chunk.offset > second_start));

    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let loaded = [
        vm.loader.load_file(&bytes).unwrap(),
        vm.loader.load_file(&bytes).unwrap(),
    ];
    let events = vm.machine_trace().pending(usize::MAX);
    assert_eq!(
        events.len(),
        10,
        "each load has FileRead and two ModuleRead/ClassRegister pairs"
    );
    let mut file_ids = vec![];
    let mut module_ids = vec![];
    let mut class_ids = vec![];
    for invocation in 0..2 {
        let entries = &events[invocation * 5..(invocation + 1) * 5];
        let Event::FileRead {
            identity: file,
            bytes: recorded,
        } = &entries[0].event
        else {
            panic!("FileRead must precede this invocation's modules")
        };
        assert_eq!(recorded, &bytes);
        assert!(
            !file_ids.contains(file),
            "equal file contents do not merge load invocations"
        );
        file_ids.push(*file);
        for ordinal in 0..2 {
            let Event::ModuleRead {
                identity,
                file: owner,
                version,
                chunks,
            } = &entries[1 + 2 * ordinal].event
            else {
                panic!("each parsed module must have its own causal identity")
            };
            assert_eq!(owner, file);
            assert!(!module_ids.contains(identity));
            module_ids.push(*identity);
            assert_eq!(*version, modules[ordinal].version);
            assert_eq!(
                chunks,
                &modules[ordinal]
                    .chunks
                    .iter()
                    .map(|chunk| (chunk.tag, chunk.offset, chunk.size))
                    .collect::<Vec<_>>()
            );
            let Event::ClassRegister {
                identity: class,
                module: Some(source),
                bodies,
            } = &entries[2 + 2 * ordinal].event
            else {
                panic!("class must retain its own source module")
            };
            assert_eq!(
                bodies,
                &(0..modules[ordinal].trees(&bytes).unwrap().len())
                    .map(|index| vm
                        .loader
                        .body_id_of(loaded[invocation][ordinal].0, index)
                        .unwrap())
                    .collect::<Vec<_>>()
            );
            assert_eq!(source, identity);
            assert_eq!(*class, loaded[invocation][ordinal].0.get());
            assert_eq!(
                loaded[invocation][ordinal].1,
                ["StreamFirst", "StreamSecond"][ordinal]
            );
            assert!(
                !class_ids.contains(class),
                "parsed repeats retain distinct class records"
            );
            class_ids.push(*class);
        }
    }
    for file in file_ids {
        assert!(
            !module_ids.contains(&file),
            "file and module origins are separate allocation lifetimes"
        );
    }
}

#[test]
fn malformed_file_attempt_and_single_module_load_keep_distinct_file_origins() {
    let bytes = empty_tufa_module("StreamSingle", 0x11ae1);
    let parsed = slr_fmt_tufa::parse(&bytes).unwrap();
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    assert!(vm.loader.load_file(b"bad").is_err());
    let first = vm.loader.load_module(&parsed[0], &bytes).unwrap();
    let name = vm.loader.load_module_named(&parsed[0], &bytes).unwrap();
    assert_eq!(name, first.1);
    let events = vm.machine_trace().pending(usize::MAX);
    assert_eq!(events.len(), 7);
    let files = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::FileRead { identity, bytes } => Some((*identity, bytes.as_slice())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        files.len(),
        3,
        "failed attempts and compatibility calls each retain actual origin"
    );
    assert_eq!(files[0].1, b"bad");
    assert_eq!(files[1].1, bytes);
    assert_eq!(files[2].1, bytes);
    assert_ne!(files[0].0, files[1].0);
    assert_ne!(files[1].0, files[2].0);
    let modules = events
        .iter()
        .filter_map(|entry| match entry.event {
            Event::ModuleRead { file, .. } => Some(file),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        modules,
        [files[1].0, files[2].0],
        "malformed file has no invented parsed module"
    );
}

#[test]
fn boundary_receives_live_selected_context_before_any_backend_hook() {
    fn inspect(
        context: &slr_vm::host::NativeContext<'_>,
        info: &CallInfo<'_>,
    ) -> Result<(), String> {
        assert_eq!(
            context.host.loader.class_name(context.defining_class),
            Some("Base")
        );
        assert_eq!(context.method_index, Some(1));
        assert_eq!(context.implementation, 0x41b860);
        assert_eq!(context.thread.pc.unwrap().idx, 2);
        assert_ne!(context.thread.flags & flags::RUNNING, 0);
        assert_eq!(context.cells, context.host.native_argument_cells().unwrap());
        assert_eq!(context.cells.len(), 1);
        assert_eq!(context.cells[0].value, info.args[0]);
        let events = context.host.machine_trace().pending(usize::MAX);
        assert!(matches!(&events.last().unwrap().event,
            Event::NativeEnter { thread, frame, defining_class, cells, .. }
            if *thread == context.thread.machine_identity
                && *frame == context.thread.frames.last().map(|f| f.machine_identity())
                && *defining_class == context.defining_class.get()
                && cells == context.cells));
        Err("live context inspected".into())
    }
    let (mut vm, mut thread, _, _) = native_host();
    vm.native_boundary = Some(inspect);
    vm.native_thread_hook = Some(|_, _| panic!("hook ran before boundary admitted the call"));
    assert!(run(&mut thread, &mut vm).is_err());
    assert_eq!(
        vm.native_failure.as_deref(),
        Some("Base.method()I: live context inspected")
    );
    assert!(
        vm.native_argument_cells().is_none(),
        "failed scope must release its owners"
    );
}

#[test]
fn explicit_thread_retirement_releases_owners_once_in_destructor_order() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut thread = entered(&vm);
    let frame = thread.frames[0].machine_identity();
    let local = thread.new_var(Value::int(13), 0);
    thread.append_local(local).unwrap();
    thread.push_new(Value::int(29)).unwrap();
    thread.retire().unwrap();
    assert!(thread.frames.is_empty());
    assert!(thread.pc.is_none());
    assert_ne!(thread.flags & flags::DEAD, 0);
    let events = vm.machine_trace().pending(usize::MAX);
    let retired = events
        .iter()
        .position(|e| {
            matches!(e.event,
        Event::ThreadRetire { identity, .. } if identity == thread.machine_identity)
        })
        .unwrap();
    let destroyed = events
        .iter()
        .position(|e| {
            matches!(e.event,
        Event::Frame { phase: FramePhase::Destroy, frame: ref view, .. } if view.identity == frame)
        })
        .unwrap();
    let releases = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| matches!(e.event, Event::Pool(PoolEvent::Release { .. })).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(releases.len(), 2);
    assert!(releases.iter().all(|i| retired < *i && *i < destroyed));
    assert!(!events
        .iter()
        .any(|e| matches!(e.event, Event::CellAbandon { .. })));
    thread.retire().unwrap();
    assert_eq!(
        thread.push_frame(Frame::new(None, 0)),
        Err(VmError::MachineContext)
    );
    assert_eq!(
        run(&mut thread, &mut Code(vec![])),
        Err(VmError::MachineContext)
    );
    assert_eq!(vm.machine_trace().pending(usize::MAX), events);
}

#[test]
fn nested_boundary_uses_inner_thread_and_restores_outer_argument_owners() {
    fn inspect(
        context: &slr_vm::host::NativeContext<'_>,
        info: &CallInfo<'_>,
    ) -> Result<(), String> {
        assert_eq!(context.cells, context.host.native_argument_cells().unwrap());
        assert_eq!(context.cells.len(), 1);
        let events = context.host.machine_trace().pending(usize::MAX);
        let active = events
            .iter()
            .filter_map(|e| match &e.event {
                Event::NativeEnter { thread, cells, .. } => Some((*thread, cells)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(active.last().unwrap().0, context.thread.machine_identity);
        assert_eq!(active.last().unwrap().1, context.cells);
        if info.name == "inner" {
            assert_eq!(active.len(), 2);
            assert_ne!(active[0].0, active[1].0);
            assert_ne!(active[0].1[0].slot, active[1].1[0].slot);
            assert_eq!(active[0].1[0].pool, active[1].1[0].pool);
            assert_eq!(context.method_index, Some(0));
        }
        Ok(())
    }
    fn outer(vm: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
        let owners = vm.native_argument_cells().unwrap().to_vec();
        let class = vm.loader.lookup("Inner").unwrap();
        let mut inner = vm.new_thread();
        vm.enter_method(&mut inner, class, 0, "inner", &[]).unwrap();
        assert_eq!(vm.native_argument_cells().unwrap(), owners);
        let value = inner.pop_deref_with(vm).unwrap();
        let result = value.payload;
        inner.release(value);
        inner.retire().unwrap();
        Some(result)
    }
    let (mut vm, mut thread, _, _) = native_host();
    let mut inner = empty_class("Inner", None);
    inner.methods.push(Method {
        class: "Inner".into(),
        name: "inner".into(),
        sig: "()I".into(),
        flags: 0x49,
        static_vector: true,
        is_native: true,
        is_abstract: false,
        body: u32::MAX,
        extra: 0,
    });
    vm.loader.insert_test_class(inner);
    vm.natives.register("Inner", "inner", "()I", 0x41b870);
    vm.native_impls.insert(0x41b870, native);
    vm.native_impls.insert(0x41b860, outer);
    vm.native_boundary = Some(inspect);
    run(&mut thread, &mut vm).unwrap();
    assert!(vm.native_argument_cells().is_none());
    let value = thread.pop_deref_with(&mut vm).unwrap();
    assert_eq!(value.payload, 0x1122_3344);
    thread.release(value);
    thread.retire().unwrap();
}

#[test]
fn class_install_records_empty_owners_and_pinned_dependency_ids_once() {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    vm.loader.insert_test_class(empty_class("Besa", None));
    vm.loader.insert_test_class(empty_class("Base", None));
    let base = vm.loader.lookup("Base").unwrap().get();
    let mut interface = empty_class("Interface", None);
    interface.kind = 1;
    vm.loader.insert_test_class(interface);
    let interface = vm.loader.lookup("Interface").unwrap().get();
    let mut leaf = empty_class("Leaf", Some("Base"));
    leaf.interfaces.push("Interface".into());
    vm.loader.insert_test_class(leaf);
    let leaf = vm.loader.lookup("Leaf").unwrap().get();
    vm.alloc_object("Leaf").unwrap();
    let first = vm.machine_trace().pending(usize::MAX);
    let installs = first
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::ClassInstall {
                identity,
                base,
                interfaces,
            } => Some((*identity, *base, interfaces.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        installs,
        vec![
            (base, None, vec![]),
            (interface, None, vec![]),
            (leaf, Some(base), vec![interface])
        ]
    );
    let leaf_install = first
        .iter()
        .position(|entry| matches!(entry.event,Event::ClassInstall {identity,..} if identity==leaf))
        .unwrap();
    let object=first.iter().position(|entry|matches!(entry.event,Event::ObjectAllocate {class_context,..} if class_context==leaf)).unwrap();
    assert!(leaf_install < object);
    let dependencies = first
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::ClassDependencies {
                identity,
                base,
                interfaces,
            } => Some((*identity, *base, interfaces.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dependencies,
        vec![
            (leaf, Some(base), vec![interface]),
            (base, None, vec![]),
            (interface, None, vec![])
        ]
    );
    let leaf_dependencies = first
        .iter()
        .position(
            |entry| matches!(entry.event,Event::ClassDependencies {identity,..} if identity==leaf),
        )
        .unwrap();
    let base_install = first
        .iter()
        .position(|entry| matches!(entry.event,Event::ClassInstall {identity,..} if identity==base))
        .unwrap();
    assert!(leaf_dependencies < base_install);

    let mut replacement = empty_class("Base", None);
    replacement.kind = 2;
    vm.loader.insert_test_class(replacement);
    assert_ne!(vm.loader.lookup("Base").unwrap().get(), base);
    vm.alloc_object("Leaf").unwrap();
    let events = vm.machine_trace().pending(usize::MAX);
    assert_eq!(
        events
            .iter()
            .filter(|entry| matches!(entry.event, Event::ClassInstall { .. }))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|entry| matches!(entry.event, Event::ClassDependencies { .. }))
            .count(),
        3
    );
    assert_eq!(
        vm.initialized_dependencies(slr_vm::vmclass::ClassId::from_raw(leaf).unwrap())
            .unwrap()
            .0
            .unwrap()
            .get(),
        base
    );
}

#[test]
fn finalizer_array_clear_retains_elements_before_destruction_and_length_store() {
    use slr_vm::interp::HeapSlot;
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let array = vm.alloc_array("I", 3).unwrap();
    for (index, payload) in [7, 11, 13].into_iter().enumerate() {
        vm.write_array_payload(array, index, payload).unwrap();
    }
    let trace = vm.machine_trace();
    let pending = trace.pending(usize::MAX);
    trace.acknowledge(pending.last().unwrap().sequence).unwrap();
    vm.gc_clear_references(array);
    assert!(vm.array_cells(array).unwrap().is_empty());
    let events = trace
        .pending(usize::MAX)
        .into_iter()
        .map(|entry| entry.event)
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            Event::ArrayClear {
                word: array,
                before: vec![Value::int(7), Value::int(11), Value::int(13)]
            },
            Event::HeapStore {
                slot: HeapSlot::Object {
                    word: array,
                    slot: 0
                },
                before: Value::int(3),
                after: Value::int(0)
            },
        ]
    );
}

#[test]
fn interpreter_arithmetic_and_boolean_edits_record_afterimages_before_reuse() {
    let vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut thread = entered(&vm);
    let mut code = Code(vec![
        node(0x1027, 2),
        node(0x1027, 3),
        node(0x1007, 0x10),
        node(0x1027, 5),
        node(0x1007, 7),
        node(0x1015, 11),
        node(0x1027, 9),
        node(0x1007, 0x15),
        node(5, 0),
        node(0x1027, 99),
        node(0x10, 0),
    ]);
    run(&mut thread, &mut code).unwrap();
    let result = thread.pop_deref_with(&mut code).unwrap();
    assert_eq!(result.value(), Value::int(0));
    thread.release(result);
    thread.retire().unwrap();
    let entries = vm.machine_trace().pending(usize::MAX);
    assert!(!entries
        .iter()
        .any(|entry| matches!(entry.event, Event::CellWriteAccess { .. })));
    let stores = entries
        .iter()
        .filter_map(|entry| match entry.event {
            Event::CellStore { before, after, .. } => Some((before, after)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [
        (Value::int(2), Value::int(5)),
        (Value::int(5), Value::int(1)),
        (Value::int(9), Value::int(0)),
    ] {
        assert!(
            stores.contains(&expected),
            "missing owned mutation {expected:?}: {stores:?}"
        );
    }
    for (i, entry) in entries.iter().enumerate() {
        if let Event::CellStore { identity, .. } = entry.event {
            assert!(entries[i+1..].iter().any(|later|matches!(&later.event,Event::Pool(slr_vm::values::PoolEvent::Release{cell,..})if cell.pool==identity.pool&&cell.slot==identity.slot&&cell.generation==identity.generation)),"store must precede release of its exact owner");
        }
    }
}

#[test]
fn class_registration_pins_empty_and_equal_tree_bodies_to_distinct_record_ids() {
    let mut vm = VmHost::new();
    vm.start_machine_recording().unwrap();
    let mut ids = vec![];
    for kind in [0, 1] {
        let mut class = empty_class("Repeated", None);
        class.kind = kind;
        class.bodies = vec![
            vec![],
            vec![node(0x1027, 7), node(0x10, 0)],
            vec![node(0x1027, 7), node(0x10, 0)],
        ];
        ids.push(vm.loader.insert_test_class(class));
    }
    let events = vm.machine_trace().pending(usize::MAX);
    assert_eq!(events.len(), 2);
    let mut seen = std::collections::HashSet::new();
    for (entry, owner) in events.iter().zip(ids) {
        let Event::ClassRegister {
            identity,
            module: None,
            bodies,
        } = &entry.event
        else {
            panic!("expected source registration")
        };
        assert_eq!(*identity, owner.get());
        assert_eq!(bodies.len(), 3);
        for (index, body) in bodies.iter().enumerate() {
            assert!(seen.insert(*body));
            assert_eq!(vm.loader.body_id_of(owner, index), Some(*body));
            assert_eq!(vm.loader.body_owner(*body), Some(owner));
        }
    }
}

#[test]
fn recorder_and_producer_acknowledge_independently_without_losing_history() {
    let trace = Trace::default();
    trace.start().unwrap();
    let producer = trace.reader().unwrap();
    let diagnostic = trace.reader().unwrap();
    for identity in 0..20000 {
        trace.note(Event::ThreadCreate { identity });
    }
    assert!(
        trace.reader().is_err(),
        "a late reader cannot reconstruct missing owner creation"
    );
    trace.acknowledge(19999).unwrap();
    assert!(trace.pending(usize::MAX).is_empty());
    assert_eq!(producer.pending(usize::MAX).len(), 20000);
    diagnostic.acknowledge(19999).unwrap();
    producer.acknowledge(9999).unwrap();
    assert_eq!(producer.pending(1)[0].sequence, 10000);
    assert!(producer.acknowledge(9999).is_err());
    assert!(producer.acknowledge(20000).is_err());
    assert_eq!(producer.pending(usize::MAX).len(), 10000);
    producer.acknowledge(19999).unwrap();
    trace.note(Event::ThreadCreate { identity: 20000 });
    assert_eq!(producer.pending(1), diagnostic.pending(1));
    assert_eq!(trace.pending(1), diagnostic.pending(1));
    assert_eq!(producer.pending(1)[0].sequence, 20000);
}
