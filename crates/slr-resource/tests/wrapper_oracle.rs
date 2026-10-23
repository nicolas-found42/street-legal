//! Wrapper class construction and native-node roundtrip through the VM heap.
mod support;
use serde_json::Value;
use slr_resource::node as n;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value as VmValue;

fn call(vm: &mut VmHost, class: &str, name: &str, sig: &str, receiver: u32) -> Option<u32> {
    n::bridge(
        vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &[VmValue::reference(receiver)],
            class,
            name,
            sig,
        },
    )
}
fn state(ty: i32) -> n::ResHost {
    let mut host = n::ResHost::new();
    let terminal = host.add_container(n::Container::new(0, 0));
    let mut target = n::Container::new(ty, 0x55);
    target.next_sibling = terminal;
    let target = host.add_container(target);
    let mut source = n::Container::new(3, 9);
    source.parent = target;
    source.wt_root = target;
    source.first_child = target;
    source.next_sibling = target;
    let source = host.add_container(source);
    let node = host.add_node_for_test(9, source);
    host.bind_object(7, node);
    host
}
fn ptr_slot(vm: &VmHost, class: &slr_vm::vmclass::ClassRef) -> usize {
    use slr_vm::vmclass::ClassRef;
    let id = match class {
        ClassRef::Loaded(id) => *id,
        ClassRef::Array(_) => panic!("resource object is not an array"),
    };
    vm.instance_field_slot_id(id, "ptr").unwrap()
}

#[test]
fn resource_wrappers_are_real_typed_vm_objects() {
    let dir = std::env::var_os("SLR_WRAPPER_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/res")
        });
    let Ok(text) = std::fs::read_to_string(dir.join("res_wrapper_factory.jsonl")) else {
        eprintln!("SLR_ORACLE_SKIPPED: resource_wrappers_are_real_typed_vm_objects");
        return;
    };
    let mut checked = 0;
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let ty = if row["args"]["bound"].as_bool().unwrap() {
            row["args"]["type"].as_u64().unwrap() as i32
        } else {
            0
        };
        let expected_class = row["result"]["events"][0][1].as_str().unwrap();
        assert_eq!(row["result"]["exists"], true);
        assert_eq!(
            row["result"]["events"][1],
            serde_json::json!(["construct", 0x7001, 0])
        );
        assert_eq!(
            row["result"]["stores"],
            serde_json::json!([[true, 3, true]])
        );
        for name in ["getParent", "getWTRoot", "getFirstChild", "getNextChild"] {
            let mut vm = VmHost::new();
            support::resource_classes(&mut vm);
            n::install(&mut vm);
            n::with_res(|h| *h = state(ty));
            let word = call(
                &mut vm,
                n::RESOURCE_REF_CLASS,
                name,
                "()Ljava.util.resource.ResourceRef;",
                7,
            )
            .unwrap();
            let object = vm
                .objects
                .get(word.checked_sub(0x1000).expect("private wrapper handle") as usize)
                .expect("wrapper not in VM heap");
            assert_eq!(
                object.class_name(&vm.loader),
                expected_class,
                "{name} row {}",
                row["id"]
            );
            assert_eq!(
                row["result"]["events"],
                serde_json::json!([
                    ["load", object.class_name(&vm.loader)],
                    ["construct", 0x7001, 0]
                ])
            );
            let node = object.fields[ptr_slot(&vm, &object.class)].payload;
            assert_ne!(node, 0);
            n::with_res(|h| {
                assert_eq!(h.object_node(word), node);
                assert_eq!(h.node(node).unwrap().id, 0x55);
                h.bind_object(word, 0);
            });
            let (method, owner) = vm
                .loader
                .resolve_method(object.class_name(&vm.loader), "id", "()I")
                .expect("inherited native method is resolvable");
            let owner = owner.to_owned();
            assert!(method.is_native);
            assert_eq!(
                call(&mut vm, &owner, "id", "()I", word),
                Some(0x55),
                "ptr field must restore the node association"
            );
            assert_eq!(
                call(&mut vm, n::RESOURCE_REF_CLASS, "type", "()I", word),
                Some(ty as u32)
            );
            n::with_res(|h| assert_eq!(h.object_node(word), node));
            if name == "getParent" {
                assert_eq!(
                    call(&mut vm, n::RESOURCE_REF_CLASS, "deleteNative", "()V", word),
                    None
                );
                let slot = ptr_slot(&vm, &vm.objects[(word - 0x1000) as usize].class);
                assert_eq!(vm.objects[(word - 0x1000) as usize].fields[slot].payload, 0);
                assert_eq!(
                    call(&mut vm, n::RESOURCE_REF_CLASS, "newNative", "()V", word),
                    None
                );
                let fresh = vm.objects[(word - 0x1000) as usize].fields[slot].payload;
                assert_ne!(fresh, 0);
                assert_ne!(fresh, node);
                assert_eq!(
                    call(&mut vm, n::RESOURCE_REF_CLASS, "id", "()I", word),
                    Some(0)
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 56);
}

#[test]
fn wrapper_getters_reject_missing_class_or_obj_metadata() {
    for name in ["getParent", "getWTRoot", "getFirstChild", "getNextChild"] {
        for missing_obj in [false, true] {
            let mut vm = VmHost::new();
            n::install(&mut vm);
            if missing_obj {
                // Build an invalid hierarchy from the start; installed classes
                // cannot be replaced once their body/field identities exist.
                for (class, base) in [
                    ("java.lang.Object", None),
                    ("java.lang.Native", Some("java.lang.Object")),
                    (n::RESOURCE_REF_CLASS, Some("java.lang.Native")),
                    (n::GAME_REF_CLASS, Some(n::RESOURCE_REF_CLASS)),
                    ("java.util.resource.RenderRef", Some(n::RESOURCE_REF_CLASS)),
                ] {
                    vm.loader.insert_test_class(slr_vm::vmclass::Class {
                        kind: 0,
                        name: class.into(),
                        base: base.map(str::to_owned),
                        interfaces: vec![],
                        methods: vec![],
                        fields: vec![],
                        bodies: vec![],
                    });
                }
            }
            n::with_res(|h| *h = state(1));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call(
                    &mut vm,
                    n::RESOURCE_REF_CLASS,
                    name,
                    "()Ljava.util.resource.ResourceRef;",
                    7,
                )
            }));
            let error =
                result.expect_err("wrapper getter returned a private handle without metadata");
            let message = error
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| error.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("");
            assert!(
                message.contains("resource wrapper requires loaded class hierarchy and ptr field"),
                "{message}"
            );
            assert!(vm.objects.is_empty());
        }
    }
}

#[test]
fn wrappers_use_the_stock_inherited_native_ptr() {
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    if !stock.join("system/Scripts/lang/Native.class").exists() {
        eprintln!("SLR_ORACLE_SKIPPED: wrappers_use_the_stock_inherited_native_ptr");
        return;
    }
    for (ty, expected) in [
        (1, n::GAME_REF_CLASS),
        (8, n::GAME_REF_CLASS),
        (3, "java.util.resource.RenderRef"),
        (14, "java.util.resource.RenderRef"),
        (2, n::RESOURCE_REF_CLASS),
    ] {
        let mut vm = VmHost::new();
        for path in [
            "lang/Object",
            "lang/Native",
            "util/resource/ResourceRef",
            "util/resource/GameRef",
            "util/resource/RenderRef",
        ] {
            let bytes = std::fs::read(stock.join(format!("system/Scripts/{path}.class"))).unwrap();
            vm.loader.load_file(&bytes).unwrap();
        }
        vm.resource_resolver = Some(initializer_rid_boundary);
        n::install(&mut vm);
        n::with_res(|h| *h = state(ty));
        let word = call(
            &mut vm,
            n::RESOURCE_REF_CLASS,
            "getParent",
            "()Ljava.util.resource.ResourceRef;",
            7,
        )
        .unwrap();
        let object = &vm.objects[(word - 0x1000) as usize];
        assert_eq!(object.class_name(&vm.loader), expected);
        let slot = ptr_slot(&vm, &object.class);
        let node = object.fields[slot].payload;
        assert_ne!(node, 0);
        n::with_res(|h| h.bind_object(word, 0));
        for (method, sig, result) in [("id", "()I", 0x55), ("type", "()I", ty as u32)] {
            let (metadata, owner) = vm.loader.resolve_method(expected, method, sig).unwrap();
            assert!(metadata.is_native);
            assert_eq!(call(&mut vm, &owner, method, sig, word), Some(result));
        }
        // A relative getter on the returned object must also recover ptr from
        // the actual inherited field, even after clearing the side-table entry.
        n::with_res(|h| h.bind_object(word, 0));
        assert_eq!(
            call(
                &mut vm,
                n::RESOURCE_REF_CLASS,
                "getWTRoot",
                "()Ljava.util.resource.ResourceRef;",
                word
            ),
            Some(0)
        );
        assert_eq!(
            vm.objects[(word - 0x1000) as usize].fields[slot].payload,
            node
        );
        n::with_res(|h| {
            let target = h.node(node).unwrap().container;
            let ancestor = h.add_container(n::Container::new(3, 0x66));
            h.container_mut(target).unwrap().parent = ancestor;
            h.bind_object(word, 0);
        });
        let ancestor = call(
            &mut vm,
            n::RESOURCE_REF_CLASS,
            "getParent",
            "()Ljava.util.resource.ResourceRef;",
            word,
        )
        .unwrap();
        assert_eq!(
            vm.objects[(ancestor - 0x1000) as usize].class_name(&vm.loader),
            "java.util.resource.RenderRef"
        );
        assert_eq!(
            call(&mut vm, n::RESOURCE_REF_CLASS, "id", "()I", ancestor),
            Some(0x66)
        );
    }
}

// Loaded-stock wrapper tests supply the resource-world boundary explicitly.
// These controlled IDs exercise initialization/native storage, not package
// loading or production resource-ID fidelity.
thread_local! {static INITIALIZER_RIDS:std::cell::RefCell<Vec<(String,u32)>>=const {std::cell::RefCell::new(Vec::new())};}
fn initializer_rid_boundary(
    _vm: &mut VmHost,
    package: Option<&str>,
    id: u32,
) -> Result<u32, slr_vm::interp::VmError> {
    assert!(
        package.is_some(),
        "stock initializer must preserve its package"
    );
    INITIALIZER_RIDS.with(|calls| calls.borrow_mut().push((package.unwrap().to_owned(), id)));
    Ok(0x6800_0000 | id)
}

fn stock_vm(test: &str) -> Option<VmHost> {
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    if !stock.join("system/Scripts/lang/Native.class").exists() {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return None;
    }
    let mut vm = VmHost::new();
    for path in [
        "lang/Object",
        "lang/Native",
        "util/resource/ResourceRef",
        "util/resource/GameRef",
        "util/resource/RenderRef",
    ] {
        vm.loader
            .load_file(&std::fs::read(stock.join(format!("system/Scripts/{path}.class"))).unwrap())
            .unwrap();
    }
    vm.resource_resolver = Some(initializer_rid_boundary);
    n::install(&mut vm);
    Some(vm)
}

#[test]
fn fresh_stock_resources_initialize_inherited_ptr_on_new_native() {
    let Some(mut vm) = stock_vm("fresh_stock_resources_initialize_inherited_ptr_on_new_native")
    else {
        return;
    };
    n::reset_res();
    INITIALIZER_RIDS.with(|calls| calls.borrow_mut().clear());
    for class in [
        n::RESOURCE_REF_CLASS,
        n::GAME_REF_CLASS,
        "java.util.resource.RenderRef",
    ] {
        let word = vm.alloc_object(class).unwrap();
        if class == n::GAME_REF_CLASS {
            let rid = vm.read_field(word, "RID_DUMMY").unwrap();
            assert!(INITIALIZER_RIDS.with(|calls| calls
                .borrow()
                .iter()
                .any(|(_, id)| rid.payload == (0x6800_0000 | id))));
        }
        let (_, owner) = vm.loader.resolve_method(class, "newNative", "()V").unwrap();
        assert_eq!(call(&mut vm, &owner, "newNative", "()V", word), None);
        let slot = ptr_slot(&vm, &vm.loader.resolve_descriptor(class).unwrap());
        let node = vm.objects[(word - 0x1000) as usize].fields[slot].payload;
        assert_ne!(node, 0);
        n::with_res(|h| {
            assert!(h.node(node).is_some());
            h.bind_object(word, 0);
        });
        assert_eq!(
            call(&mut vm, n::RESOURCE_REF_CLASS, "id", "()I", word),
            Some(0)
        );
        assert_eq!(call(&mut vm, &owner, "newNative", "()V", word), None);
        assert_eq!(
            vm.objects[(word - 0x1000) as usize].fields[slot].payload,
            node
        );
        assert_eq!(
            call(&mut vm, n::RESOURCE_REF_CLASS, "deleteNative", "()V", word),
            None
        );
        assert_eq!(vm.objects[(word - 0x1000) as usize].fields[slot].payload, 0);
    }
}

#[test]
fn stock_resource_arguments_read_native_ptr_without_private_bindings() {
    let Some(template) =
        stock_vm("stock_resource_arguments_read_native_ptr_without_private_bindings")
    else {
        return;
    };
    drop(template);
    for addr in [
        n::SET_REF,
        n::DUPLICATE,
        n::MAKE_TEXTURE,
        n::MAKE_SOUND,
        n::CREATE,
        n::CREATE_NATIVE,
        n::SET_PARENT,
        n::QUEUE_EVENT,
    ] {
        // Independently allocate receiver and arguments, as an ordinary VM constructor does.
        let mut vm =
            stock_vm("stock_resource_arguments_read_native_ptr_without_private_bindings").unwrap();
        n::reset_res();
        let mut objects = vec![];
        let mut nodes = vec![];
        for class in [
            n::GAME_REF_CLASS,
            n::GAME_REF_CLASS,
            "java.util.resource.RenderRef",
        ] {
            let word = vm.alloc_object(class).unwrap();
            call(&mut vm, n::RESOURCE_REF_CLASS, "newNative", "()V", word);
            let slot = ptr_slot(&vm, &vm.loader.resolve_descriptor(class).unwrap());
            nodes.push(vm.objects[(word - 0x1000) as usize].fields[slot].payload);
            objects.push(word);
        }
        let target = n::with_res(|h| {
            let source = h.add_container(n::Container::new(1, 9));
            let target = h.add_container(n::Container::new(3, 0x55));
            h.node_mut(nodes[0]).unwrap().container = source;
            h.node_mut(nodes[0]).unwrap().id = 9;
            h.node_mut(nodes[1]).unwrap().container = target;
            h.node_mut(nodes[1]).unwrap().id = 0x55;
            h.node_mut(nodes[2]).unwrap().container = source;
            h.node_mut(nodes[2]).unwrap().id = 9;
            h.set_created_result(target);
            for object in &objects {
                h.bind_object(*object, 0);
            }
            target
        });
        let binding = n::RESOURCE_REF_BINDINGS
            .iter()
            .chain(n::GAME_REF_BINDINGS)
            .find(|b| b.impl_addr == addr)
            .unwrap();
        let text_word = n::with_res(|h| h.intern("paint.dds"));
        let words = if matches!(addr, n::CREATE | n::CREATE_NATIVE) {
            vec![objects[1], objects[2], 0, 0]
        } else if addr == n::QUEUE_EVENT {
            vec![objects[1], 0x456, 0]
        } else if matches!(addr, n::MAKE_TEXTURE | n::MAKE_SOUND) {
            vec![objects[1], text_word]
        } else {
            vec![objects[1]]
        };
        let mut args: Vec<_> = n::param_types(binding.sig)
            .into_iter()
            .zip(words.iter().copied())
            .map(|(ty, word)| {
                if ty == b'I' {
                    VmValue::int(word as i32)
                } else {
                    VmValue::reference(word)
                }
            })
            .rev()
            .collect();
        args.push(VmValue::reference(objects[0]));
        n::bridge(
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: binding.class,
                name: binding.name,
                sig: binding.sig,
            },
        );
        n::with_res(|h| {
            assert_eq!(h.object_node(objects[0]), nodes[0]);
            if addr == n::SET_REF {
                assert_eq!(h.node(nodes[0]).unwrap().container, target);
            } else {
                let event = h.events().last().unwrap();
                match event {
                    n::ResourceEvent::Duplicate { node, source } => {
                        assert_eq!((*node, *source), (nodes[0], nodes[1]));
                    }
                    n::ResourceEvent::Make { node, resource, .. } => {
                        assert_eq!((*node, *resource), (nodes[0], nodes[1]));
                    }
                    n::ResourceEvent::Parent { node, parent } => {
                        assert_eq!((*node, *parent), (nodes[0], nodes[1]));
                    }
                    n::ResourceEvent::Queue {
                        node,
                        resource,
                        event,
                        ..
                    } => {
                        assert_eq!((*node, *resource, *event), (nodes[0], nodes[1], 0x456));
                    }
                    n::ResourceEvent::Create { args, .. } => {
                        assert_eq!(args[0], nodes[1]);
                        assert_eq!(args[1], nodes[2]);
                    }
                    n::ResourceEvent::CreatedInstance { .. } => {
                        let n::ResourceEvent::Create { args, .. } = &h.events()[0] else {
                            panic!("missing create")
                        };
                        assert_eq!(args[0], nodes[1]);
                        assert_eq!(args[1], nodes[2]);
                    }
                    _ => panic!("unexpected {event:?}"),
                }
            }
            let mut logged = vec![objects[0]];
            logged.extend_from_slice(&words);
            support::assert_record(
                h,
                binding,
                &logged,
                if addr == n::CREATE { Some(0) } else { None },
            );
        });
        // The original receiver guard precedes argument dereferences. A null
        // receiver must not inspect an uninitialized heap argument's ptr.
        let uninitialized = vm.alloc_object(n::RESOURCE_REF_CLASS).unwrap();
        let mut guarded_args = args.clone();
        let last = guarded_args.len() - 1;
        guarded_args[last] = VmValue::reference(0);
        guarded_args[last - 1] = VmValue::reference(uninitialized);
        n::bridge(
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &guarded_args,
                class: binding.class,
                name: binding.name,
                sig: binding.sig,
            },
        );
    }
}

#[test]
fn stock_interpreter_ptr_write_is_observed_by_installed_resource_natives() {
    use slr_fmt_tufa::Node;
    use slr_vm::host::collect_args_with;
    use slr_vm::interp::{Frame, Pc, Thread};
    use slr_vm::values::TypeTag;
    let Some(mut vm) =
        stock_vm("stock_interpreter_ptr_write_is_observed_by_installed_resource_natives")
    else {
        return;
    };
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    vm.loader
        .load_file(&std::fs::read(stock.join("system/Scripts/lang/Runnable.class")).unwrap())
        .unwrap();
    vm.loader
        .load_file(&std::fs::read(stock.join("system/Scripts/lang/GameType.class")).unwrap())
        .unwrap();
    let object = vm.alloc_object("java.lang.GameType").unwrap();
    let heap = &vm.objects[(object - 0x1000) as usize];
    assert_eq!(
        heap.fields.len(),
        4,
        "only inherited ptr + three GameType instance fields"
    );
    assert_eq!(heap.fields[2].tag, TypeTag::Ref("Ljava.lang.Thread;"));
    assert_eq!(heap.fields[3].tag, TypeTag::Ref("Ljava.util.MessagePort;"));
    let heap_id = match &heap.class {
        slr_vm::vmclass::ClassRef::Loaded(id) => *id,
        slr_vm::vmclass::ClassRef::Array(_) => panic!("heap object is not an array"),
    };
    assert_eq!(vm.instance_field_slot_id(heap_id, "ptr").unwrap(), 0);
    n::reset_res();
    let native = n::with_res(|host| {
        let container = host.add_container(n::Container::new(1, 0x55));
        host.add_node_for_test(0x55, container)
    });
    let pool = vm.loader.pool("java.lang.Native").unwrap().to_vec();
    let name = pool
        .iter()
        .position(|entry| matches!(entry,slr_fmt_tufa::PoolEntry::Str(name) if name=="ptr"))
        .unwrap() as u32;
    let node = |opword, imm| Node {
        opword,
        operand: 1,
        imm,
    };
    let body = vm
        .add_body(
            "java.lang.Native",
            vec![
                node(0x1027, native),
                node(0x1d, name),
                node(0x1008, 0x23),
                node(0x10, 0),
            ],
            pool.clone(),
        )
        .unwrap();
    let pc = Some(Pc { body, idx: 0 });
    let mut thread = Thread::new();
    let mut frame = Frame::new(pc, object);
    frame.class_ctx = vm.class_context("java.lang.Native").unwrap();
    thread.frames.push(frame);
    thread.pc = pc;
    thread
        .run(&mut vm, 0., &mut || slr_x87::X87::from_f64(0.))
        .unwrap();
    assert_eq!(
        vm.objects[(object - 0x1000) as usize].fields[0].payload,
        native
    );
    assert_eq!(
        vm.objects[(object - 0x1000) as usize].fields[1],
        VmValue::int(0)
    );
    n::with_res(|host| host.bind_object(object, 0));
    let (_, owner) = vm
        .loader
        .resolve_method("java.lang.GameType", "id", "()I")
        .unwrap();
    assert_eq!(call(&mut vm, &owner, "id", "()I", object), Some(0x55));
    assert_eq!(
        call(&mut vm, n::RESOURCE_REF_CLASS, "type", "()I", object),
        Some(1)
    );
    // The native argument ABI must dereference a heap-backed field too.
    let body = vm
        .add_body(
            "java.lang.Native",
            vec![node(0x1d, name), node(0x10, 0)],
            pool,
        )
        .unwrap();
    let pc = Some(Pc { body, idx: 0 });
    let mut thread = Thread::new();
    let mut frame = Frame::new(pc, object);
    frame.class_ctx = vm.class_context("java.lang.Native").unwrap();
    thread.frames.push(frame);
    thread.pc = pc;
    thread.push_new(VmValue::reference(object)).unwrap();
    thread
        .run(&mut vm, 0., &mut || slr_x87::X87::from_f64(0.))
        .unwrap();
    assert_eq!(
        collect_args_with(&mut thread, "(I)V", &mut vm).unwrap(),
        vec![VmValue::int(native as i32), VmValue::reference(object)]
    );
    assert_eq!(thread.live(), 0);
}
