//! Public interpreter field/lvalue behavior, independent of native internals.
use slr_fmt_tufa::Node;
use slr_vm::host::{Object, VmHost};
use slr_vm::interp::{Frame, Pc, Thread};
use slr_vm::values::{TypeTag, Value};
use slr_vm::vmclass::{Class, Field};
use slr_x87::X87;

fn node(opword: u16, imm: u32) -> Node {
    Node {
        opword,
        operand: 1,
        imm,
    }
}
fn class(name: &str, base: Option<&str>, fields: &[(&str, &str, bool)]) -> Class {
    Class {
        kind: 0,
        name: name.into(),
        base: base.map(str::to_owned),
        base_id: None,
        interfaces: vec![],
        methods: vec![],
        bodies: vec![],
        fields: fields
            .iter()
            .map(|(field, ty, is_static)| Field {
                class: name.into(),
                name: (*field).into(),
                ty: (*ty).into(),
                is_static: *is_static,
            })
            .collect(),
    }
}
#[test]
fn implicit_named_field_is_a_persistent_typed_lvalue() {
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "Fields",
        None,
        &[("first", "I", false), ("second", "F", false)],
    ));
    let fields_id = vm.loader.lookup("Fields").unwrap();
    vm.objects.push(Object {
        class: slr_vm::vmclass::ClassRef::Loaded(fields_id),
        fields: vec![Value::int(0x111), Value::float(0.0)],
    });
    // Original420ff0/001d returns the requested variable and pops no receiver.
    // Stack is RHS before lvalue; assigning I0xabc to F yields bits0x452bc000.
    let body = vm
        .add_body(
            "Fields",
            vec![
                node(0x1027, 0xabc),
                node(0x001d, 0),
                node(0x1008, 0x23),
                node(0x0010, 0),
            ],
            vec![slr_fmt_tufa::PoolEntry::Str("second".into())],
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0x1000);
    frame.class_ctx = vm.class_context("Fields").unwrap();
    thread.frames.push(frame);
    thread.pc = pc;
    thread
        .run(&mut vm, 0.0, &mut || X87::from_f64(0.0))
        .unwrap();
    assert_eq!(
        vm.objects[0].fields,
        vec![
            Value::int(0x111),
            Value {
                tag: TypeTag::Float,
                payload: 1160495104
            }
        ]
    );
    assert!(thread.frames[0].stack.is_empty());
    assert_eq!(
        thread.live, 0,
        "one literal created/released; borrowed field owns no temporary"
    );
}

#[test]
fn object_layout_is_base_first_typed_and_excludes_static_fields() {
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "Base",
        None,
        &[("staticBase", "I", true), ("ptr", "I", false)],
    ));
    vm.loader.insert_test_class(class(
        "Leaf",
        Some("Base"),
        &[
            ("staticLeaf", "I", true),
            ("amount", "F", false),
            ("target", "Ljava.lang.Thread;", false),
        ],
    ));
    let word = vm.alloc_object("Leaf").unwrap();
    assert_eq!(
        vm.objects[(word - 0x1000) as usize].fields,
        vec![
            Value::int(0),
            Value::float(0.),
            Value::reference_named(0, "Ljava.lang.Thread;")
        ]
    );
    assert_eq!(vm.instance_field_slot("Leaf", "ptr").unwrap(), 0);
    assert_eq!(vm.instance_field_slot("Leaf", "amount").unwrap(), 1);
    assert_eq!(vm.instance_field_slot("Leaf", "target").unwrap(), 2);
    assert!(vm.instance_field_slot("Leaf", "staticLeaf").is_err());
}

#[test]
fn explicit_member_prefers_class_static_cell_and_keeps_aliases_live() {
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "Fields",
        None,
        &[("same", "F", true), ("same", "I", false)],
    ));
    let word = vm.alloc_object("Fields").unwrap();
    vm.objects[0].fields[0] = Value::int(0x222);
    let body = vm
        .add_body(
            "Fields",
            vec![
                node(0x1001, 0),
                node(0x401c, 0),
                node(0x1007, 0x19),
                node(0x0018, 0),
                node(0x1001, 0),
                node(0x401c, 0),
                node(0x0010, 0),
            ],
            vec![slr_fmt_tufa::PoolEntry::Str("same".into())],
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, word);
    frame.class_ctx = vm.class_context("Fields").unwrap();
    let receiver = thread.new_var(Value::reference(word), 0);
    frame.locals.push(receiver);
    thread.frames.push(frame);
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    let value = thread.pop_deref_with(&mut vm).unwrap();
    assert_eq!(value, Value::float(1.));
    assert_eq!(vm.objects[0].fields, vec![Value::int(0x222)]);
    assert_eq!(
        thread.live, 2,
        "receiver variable plus last dereferenced result; field lvalues do not allocate"
    );
}

#[test]
fn pool_field_reference_resolves_the_declaring_hierarchy_and_keeps_receiver_implicit() {
    use slr_fmt_tufa::PoolEntry as P;
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "Base",
        None,
        &[("first", "I", false), ("inherited", "F", false)],
    ));
    vm.loader
        .insert_test_class(class("Leaf", Some("Base"), &[("last", "I", false)]));
    let word = vm.alloc_object("Leaf").unwrap();
    let pool = vec![
        P::Str("Leaf".into()),
        P::Class(0),
        P::Str("inherited".into()),
        P::Str("F".into()),
        P::Type7 { a: 2, b: 3 },
        P::Type5 { a: 1, b: 4 },
    ];
    let body = vm
        .add_body(
            "Leaf",
            vec![
                node(0x1027, 9),
                node(0x101b, 5),
                node(0x1008, 0x23),
                node(0x101b, 5),
                node(0x0010, 0),
            ],
            pool,
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, word);
    frame.class_ctx = vm.class_context("Leaf").unwrap();
    thread.frames.push(frame);
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    assert_eq!(thread.pop_deref_with(&mut vm).unwrap(), Value::float(9.));
    assert_eq!(
        vm.objects[0].fields,
        vec![Value::int(0), Value::float(9.), Value::int(0)]
    );
    assert_eq!(thread.live, 1);
}

fn stock_vm(test: &str, classes: &[&str]) -> Option<VmHost> {
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    if !stock.join("system/Scripts/lang/Object.class").exists() {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return None;
    }
    let mut vm = VmHost::new();
    for class in classes {
        vm.loader
            .load_file(&std::fs::read(stock.join(format!("system/Scripts/{class}.class"))).unwrap())
            .unwrap();
    }
    Some(vm)
}

#[test]
fn stock_static_initializers_produce_real_values_or_explicit_unsupported_errors() {
    let Some(mut vm) = stock_vm(
        "stock_static_initializers_produce_real_values_or_explicit_unsupported_errors",
        &[
            "lang/Object",
            "lang/Native",
            "util/resource/ResourceRef",
            "util/resource/GameRef",
            "util/MessagePort",
        ],
    ) else {
        return;
    };
    let resource = vm.alloc_object("java.util.resource.ResourceRef").unwrap();
    assert_eq!(
        vm.read_field(resource, "RESTYPE_RENDER_OBJECT").unwrap(),
        Value::int(14)
    );
    let objects_before = vm.objects.len();
    assert!(
        matches!(
            vm.alloc_object("java.util.resource.GameRef"),
            Err(slr_vm::host::HostError::UnsupportedInitializer(_))
        ),
        "class initialization requires an explicit resource resolver before allocating GameRef"
    );
    assert_eq!(vm.objects.len(), objects_before);
    assert!(
        matches!(
            vm.alloc_object("java.util.MessagePort"),
            Err(slr_vm::host::HostError::UnsupportedInitializer(_))
        ),
        "new Vector initializer requires loaded Vector metadata; it must not leave fifo null as a success"
    );
}

fn stock_method_replay(group: &str, name: &str, sig: &str, test: &str, count: usize) {
    use slr_vm::interp::{ScriptMethod, Slot};
    let dir = std::env::var_os("SLR_VMFIELD_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmfield")
        });
    let Ok(text) = std::fs::read_to_string(dir.join(format!("vmfield_{group}.jsonl"))) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return;
    };
    let mut checked = 0;
    for line in text.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let Some(mut vm) = stock_vm(test, &["lang/Object", "lang/Vector3"]) else {
            return;
        };
        let word = vm.alloc_object("java.lang.Vector3").unwrap();
        for (field, value) in vm.objects[0]
            .fields
            .iter_mut()
            .zip(row["input"]["values"].as_array().unwrap())
        {
            *field = Value {
                tag: TypeTag::Float,
                payload: value.as_u64().unwrap() as u32,
            };
        }
        let argument = if let Some(values) = row["input"]["other"].as_array() {
            let argument = vm.alloc_object("java.lang.Vector3").unwrap();
            for (field, value) in vm.objects[(argument - 0x1000) as usize]
                .fields
                .iter_mut()
                .zip(values)
            {
                *field = Value {
                    tag: TypeTag::Float,
                    payload: value.as_u64().unwrap() as u32,
                };
            }
            Value::reference_named(argument, "Ljava.lang.Vector3;")
        } else {
            Value {
                tag: TypeTag::Float,
                payload: row["input"]["factor"].as_u64().unwrap() as u32,
            }
        };
        let (metadata, owner) = vm
            .loader
            .resolve_method("java.lang.Vector3", name, sig)
            .unwrap();
        let method = ScriptMethod {
            body: vm.loader.body_id(&owner, metadata.body as usize).unwrap(),
            class_ctx: vm.class_context(&owner).unwrap(),
            signature: 0,
            is_static: false,
            params: vec![argument.tag],
        };
        use slr_vm::interp::Host;
        assert_eq!(
            vm.body(method.body).len(),
            row["result"]["nodes"].as_u64().unwrap() as usize
        );
        vm.loader.insert_test_class(class("Caller", None, &[]));
        let caller = vm
            .add_body("Caller", vec![node(0x24, 0), node(0x10, 0)], vec![])
            .unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut thread = Thread::new();
        thread.frames.push(Frame::new(pc, 0));
        thread.pc = pc;
        thread.push_new(argument).unwrap();
        thread.call_script_method(&method, word).unwrap();
        thread.live = 0; // Original fixture starts its already-allocated-cell counter at zero.
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let values: Vec<_> = vm.objects[0]
            .fields
            .iter()
            .map(|value| value.payload)
            .collect();
        let types: Vec<_> = vm.objects[0]
            .fields
            .iter()
            .map(|value| value.tag.name())
            .collect();
        let Some(Slot::Value(result)) = thread.frames[0].stack.last() else {
            panic!("original returned a value cell")
        };
        let mut actual = serde_json::json!({"values":values,"types":types,"returned_receiver":result.payload==word,"return_type":result.tag.name(),"return_kind":"value","live":thread.live,"nodes":vm.body(method.body).len()});
        if row["input"]["other"].is_array() {
            actual["other_values"] = serde_json::json!(vm.objects
                [(argument.payload - 0x1000) as usize]
                .fields
                .iter()
                .map(|value| value.payload)
                .collect::<Vec<_>>());
        }
        assert_eq!(row["result"], actual);
        checked += 1;
    }
    assert_eq!(checked, count, "empty or partial stock method vectors");
}

#[test]
fn unchanged_stock_vector_mul_matches_original_interpreter() {
    stock_method_replay(
        "stock_mul",
        "mul",
        "(F)Ljava.lang.Vector3;",
        "unchanged_stock_vector_mul_matches_original_interpreter",
        4,
    );
}

#[test]
fn unchanged_stock_vector_add_matches_original_field_chains() {
    stock_method_replay(
        "stock_add",
        "add",
        "(Ljava.lang.Vector3;)Ljava.lang.Vector3;",
        "unchanged_stock_vector_add_matches_original_field_chains",
        2,
    );
}

#[test]
fn inline_field_chain_resolves_a_local_object_then_its_field_as_one_lvalue() {
    use slr_fmt_tufa::PoolEntry as P;
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "Fields",
        None,
        &[("first", "I", false), ("second", "F", false)],
    ));
    let receiver = vm.alloc_object("Fields").unwrap();
    let argument = vm.alloc_object("Fields").unwrap();
    vm.objects[0].fields[1] = Value::float(1.);
    vm.objects[1].fields[1] = Value::float(7.);
    let pool = vec![
        P::Str("Fields".into()),
        P::Class(0),
        P::Str("second".into()),
        P::Str("F".into()),
        P::Type7 { a: 2, b: 3 },
        P::Type5 { a: 1, b: 4 },
    ];
    let body = vm
        .add_body(
            "Fields",
            vec![
                node(0x1011, 2),
                node(0x1001, 1),
                node(0x101b, 5),
                node(0x101b, 5),
                node(0x1008, 0x27),
                node(0x0010, 0),
            ],
            pool,
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, receiver);
    frame.class_ctx = vm.class_context("Fields").unwrap();
    frame
        .locals
        .push(thread.new_var(Value::reference(receiver), 0));
    frame
        .locals
        .push(thread.new_var(Value::reference(argument), 0));
    thread.frames.push(frame);
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    assert_eq!(vm.objects[0].fields[1], Value::float(8.));
    assert_eq!(vm.objects[1].fields[1], Value::float(7.));
    assert_eq!(thread.live, 2);
}

#[test]
fn array_elements_are_typed_persistent_lvalues() {
    use slr_vm::interp::{HeapSlot, Host, Slot};
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class("Code", None, &[]));
    let word = vm.alloc_array("F", 3).unwrap();
    vm.write_cell(HeapSlot::Array { word, index: 0 }, Value::float(3.))
        .unwrap();
    vm.write_cell(HeapSlot::Array { word, index: 1 }, Value::float(4.))
        .unwrap();
    vm.write_cell(HeapSlot::Array { word, index: 2 }, Value::float(9.))
        .unwrap();
    let body = vm
        .add_body(
            "Code",
            vec![
                node(0x1027, 1),
                node(0x1001, 0),
                node(0x20, 0),
                node(0x1007, 0x19),
                node(0x18, 0),
                node(0x1027, 1),
                node(0x1001, 0),
                node(0x20, 0),
                node(0x28, 0),
                node(0x10, 0),
            ],
            vec![],
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0);
    frame
        .locals
        .push(thread.new_var(Value::reference_named(word, "[F"), 0));
    thread.frames.push(frame);
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    assert_eq!(thread.frames[0].stack, vec![Slot::Value(Value::float(5.))]);
    assert_eq!(
        [0, 1, 2].map(|index| vm.read_cell(HeapSlot::Array { word, index }).unwrap()),
        [Value::float(3.), Value::float(5.), Value::float(9.)]
    );
    assert_eq!(thread.live, 2);
}

#[test]
fn declared_local_type_comes_from_the_following_pool_node() {
    use slr_fmt_tufa::PoolEntry as P;
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class("Code", None, &[]));
    let body = vm
        .add_body(
            "Code",
            vec![
                node(0x1002, 0),
                node(0x402e, 1),
                node(0x1006, 0),
                node(0x002c, 0),
                node(0x1027, 9),
                node(0x002d, 0),
                node(0x1001, 0),
                node(0x28, 0),
                node(0x10, 0),
            ],
            vec![P::Str("F".into()), P::Str("named".into())],
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    thread.frames.push(Frame::new(pc, 0));
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    let local = thread.vars.get(thread.frames[0].locals[0]);
    assert_eq!(local.value, Value::float(9.));
    assert_eq!(vm.variable_name(local.name), Some("named"));
    assert_eq!(thread.live, 2);
}

#[test]
fn reference_field_assignment_preserves_the_target_descriptor_and_casts_the_source() {
    use slr_vm::interp::Slot;
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class("Base", None, &[]));
    vm.loader
        .insert_test_class(class("Child", Some("Base"), &[]));
    vm.loader
        .insert_test_class(class("Holder", None, &[("target", "LBase;", false)]));
    let holder = vm.alloc_object("Holder").unwrap();
    let child = vm.alloc_object("Child").unwrap();
    let body = vm
        .add_body(
            "Holder",
            vec![node(0x1d, 0), node(0x1007, 0x23), node(0x10, 0)],
            vec![slr_fmt_tufa::PoolEntry::Str("target".into())],
        )
        .unwrap();
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    thread.frames.push(Frame::new(pc, holder));
    thread.pc = pc;
    thread
        .push(Slot::Value(Value::reference_named(child, "LChild;")))
        .unwrap();
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    assert_eq!(
        vm.objects[0].fields[0],
        Value::reference_named(child, "LBase;")
    );
    assert_eq!(
        thread.frames[0].stack,
        vec![Slot::Value(Value::reference_named(child, "LBase;"))]
    );
    assert!(thread.alu.diagnostics.is_empty());
}

#[test]
fn runtime_reference_class_uses_the_authoritative_object_in_both_word_namespaces() {
    use slr_vm::values::LoaderConvert;
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class("Child", None, &[]));
    let object = vm.alloc_object("Child").unwrap();
    let child_id = vm.loader.lookup("Child").unwrap();
    assert_eq!(
        vm.reference_class(Value::reference_named(object, "LBase;")),
        Some(slr_vm::vmclass::ClassRef::Loaded(child_id))
    );
    // Existing String native allocation has a separate word base into the same
    // objects vector. Identity lookup does not certify its #110 allocation path.
    let string = 0x2000_0000 + vm.objects.len() as u32;
    let string_id = vm.loader.lookup("java.lang.String").unwrap_or_else(|| {
        vm.loader
            .insert_test_class(class("java.lang.String", None, &[]))
    });
    vm.objects.push(Object {
        class: slr_vm::vmclass::ClassRef::Loaded(string_id),
        fields: vec![],
    });
    assert_eq!(
        vm.reference_class(Value::reference_named(string, "Ljava.lang.Object;")),
        Some(slr_vm::vmclass::ClassRef::Loaded(string_id))
    );
    assert_eq!(vm.reference_class(Value::reference(0x5555_5555)), None);
}

#[test]
fn loaded_stock_interface_names_follow_actual_pool_indices() {
    let Some(vm) = stock_vm(
        "loaded_stock_interface_names_follow_actual_pool_indices",
        &["lang/GameType"],
    ) else {
        return;
    };
    assert_eq!(
        vm.loader.find("java.lang.GameType").unwrap().interfaces,
        vec!["java.lang.Runnable"]
    );
}

#[test]
fn embedding_bodies_preserve_their_class_pool_and_reject_conflicts_atomically() {
    use slr_fmt_tufa::PoolEntry as P;
    use slr_vm::interp::{Host, Slot};
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(
        "C",
        None,
        &[("first", "I", false), ("second", "I", false)],
    ));
    let word = vm.alloc_object("C").unwrap();
    vm.objects[0].fields = vec![Value::int(11), Value::int(22)];
    let pool = vec![
        P::Str("first".into()),
        P::Float(f32::from_bits(0x7fc0_0001)),
    ];
    let body = vm
        .add_body("C", vec![node(0x1d, 0), node(0x10, 0)], pool.clone())
        .unwrap();
    assert!(vm
        .add_body("C", vec![node(0x1027, 99)], vec![P::Str("second".into())])
        .is_err());
    assert_eq!(vm.loader.find("C").unwrap().bodies.len(), 1);
    assert_eq!(vm.loader.pool_text("C", 0).unwrap(), "first");
    let other = vm
        .add_body("C", vec![node(0x10, 0)], pool.clone())
        .expect("bit-identical NaN pool remains reusable");
    assert_eq!(vm.loader.body_id("C", 1), Some(other));
    let mut changed = pool;
    changed[1] = P::Float(f32::from_bits(0x7fc0_0002));
    assert!(vm.add_body("C", vec![], changed).is_err());
    vm.loader.insert_test_class(class("Zero", None, &[]));
    vm.add_body("Zero", vec![], vec![P::Float(0.)]).unwrap();
    assert!(vm.add_body("Zero", vec![], vec![P::Float(-0.)]).is_err());
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    thread.frames.push(Frame::new(pc, word));
    thread.pc = pc;
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    let Slot::Heap(cell) = thread.frames[0].stack[0] else {
        panic!("field must remain a heap lvalue")
    };
    assert_eq!(vm.read_cell(cell).unwrap(), Value::int(11));
}

#[test]
fn issued_body_ids_survive_identical_class_installation_and_reject_replacement() {
    let mut vm = VmHost::new();
    let mut original = class("C", None, &[]);
    original.bodies = vec![vec![node(0x1027, 11)]];
    vm.loader.insert_test_class(original.clone());
    let body = vm.loader.body_id("C", 0).unwrap();
    vm.loader.insert_test_class(original.clone());
    let mut replacement = original;
    replacement.bodies[0][0].imm = 99;
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm
        .loader
        .insert_test_class(replacement)))
    .is_err());
    assert_eq!(vm.loader.body(body).unwrap()[0].imm, 11);
}
