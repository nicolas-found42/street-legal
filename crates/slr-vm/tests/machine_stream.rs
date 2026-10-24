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
fn rejecting_boundary(_info: &CallInfo<'_>) -> Result<(), String> {
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
            } = &entries[2 + 2 * ordinal].event
            else {
                panic!("class must retain its own source module")
            };
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
