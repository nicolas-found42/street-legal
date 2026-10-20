//! Controlled original initializer drivers through parsed TUFA and VmHost.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::{Node, PoolEntry};
use slr_vm::host::VmHost;

fn n(opword: u16, imm: u32) -> Node {
    Node {
        opword,
        operand: 1,
        imm,
    }
}
fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}
fn module(
    pool: &[PoolEntry],
    statics: &[[u32; 4]],
    instances: &[[u32; 4]],
    class: &[u32],
    bodies: &[Vec<Node>],
) -> Vec<u8> {
    module_methods(pool, statics, instances, class, bodies, &[])
}
fn module_methods(
    pool: &[PoolEntry],
    statics: &[[u32; 4]],
    instances: &[[u32; 4]],
    class: &[u32],
    bodies: &[Vec<Node>],
    methods: &[[u32; 5]],
) -> Vec<u8> {
    let mut method_bytes = words(&[methods.len() as u32]);
    for method in methods {
        method_bytes.extend(words(method));
    }
    method_bytes.extend(words(&[0]));
    let mut cons = words(&[pool.len() as u32]);
    for entry in pool {
        match entry {
            PoolEntry::Str(text) => {
                cons.extend(words(&[0, text.len() as u32]));
                cons.extend(text.as_bytes());
                cons.push(0);
            }
            PoolEntry::Class(index) => cons.extend(words(&[4, *index])),
            PoolEntry::Member { name, extra } => cons.extend(words(&[3, *name, *extra])),
            PoolEntry::Type5 { a, b } => cons.extend(words(&[5, *a, *b])),
            PoolEntry::Type7 { a, b } => cons.extend(words(&[7, *a, *b])),
            _ => panic!("unsupported controlled fixture pool entry"),
        }
    }
    let mut fields = vec![];
    for records in [statics, instances] {
        fields.extend(words(&[records.len() as u32]));
        for record in records {
            fields.extend(words(record));
        }
    }
    let mut trees = words(&[bodies.len() as u32]);
    for body in bodies {
        trees.extend(words(&[body.len() as u32]));
        for node in body {
            trees.push(node.opword as u8);
            trees.extend(node.operand.to_le_bytes());
            if Node::has_imm(node.opword) {
                trees.extend(node.imm.to_le_bytes());
            }
        }
    }
    let mut data = b"TUFA".to_vec();
    data.extend(words(&[4, 0x11ae1]));
    for (tag, payload) in [
        (b"CONS", cons),
        (b"FILD", fields),
        (b"MTHD", method_bytes),
        (b"CLSS", words(class)),
        (b"TREE", trees),
    ] {
        data.extend(tag);
        data.extend(words(&[payload.len() as u32]));
        data.extend(payload);
    }
    data
}
fn rows(group: &str, test: &str, count: usize) -> Option<Vec<Json>> {
    let dir = std::env::var_os("SLR_VMINIT_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vminit")
        });
    let Ok(text) = std::fs::read_to_string(dir.join(format!("vminit_{group}.jsonl"))) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return None;
    };
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        count,
        "empty or partial initializer oracle suite"
    );
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["id"], index, "duplicate or reordered oracle row");
        assert_eq!(row["name"], group, "wrong oracle suite");
        let expected_input = match group {
            "order" => json!({"mode": (["static","instance"][index])}),
            "visibility" => json!({"static":index<3,"index":index%3}),
            "payloads" => {
                json!({"static":index<6,"kind":(["resource","string","cast","new","self_new","script_call"][index%6])})
            }
            "failure" => json!({"static":index==0}),
            "interfaces" => json!({}),
            "assignment" => {
                json!({"static":index<8,"kind":(["numeric_string","float_string","numeric_other","reference_other","reference_integer","mutate_integer_then_reference","mutate_reference_then_reference","mutate_integer_then_fatal"][index%8])})
            }
            "dependency_kind" => {
                json!({"instance":index>=4,"case":(["valid","interface_class","interface_unknown","base_interface"][index%4])})
            }
            "self_descriptor" => json!({"static":index!=1,"ancestor":index==2}),
            _ => panic!("unregistered initializer suite"),
        };
        assert_eq!(row["input"], expected_input, "oracle input manifest");
        assert_eq!(row["exe_addr"], "0x00403e80/0x00407da0/0x00420ff0");
        assert_eq!(row["oracle"], "unicorn");
        assert_eq!(row["cw"], "0x7f");
        assert_eq!(
            row["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
    }
    Some(rows)
}
fn visibility_file(read: usize, static_fields: bool) -> Vec<u8> {
    let mut pool = vec![
        PoolEntry::Str("ReadClass".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("I".into()),
        PoolEntry::Str("a".into()),
        PoolEntry::Str("b".into()),
        PoolEntry::Str("c".into()),
    ];
    for index in 0..3 {
        let signature = pool.len() as u32;
        pool.push(PoolEntry::Type7 { a: 3 + index, b: 2 });
        pool.push(PoolEntry::Type5 { a: 1, b: signature });
    }
    let fields = (0..3)
        .map(|i| [if static_fields { 0x28 } else { 1 }, 3 + i, 2, i])
        .collect::<Vec<_>>();
    let mut bodies = vec![
        vec![n(0x1007, 0x1c), n(0x100b, 11), n(0x10, 0)],
        vec![n(0x101b, (7 + read * 2) as u32), n(0x10, 0)],
        vec![n(0x1007, 0x1c), n(0x100b, 33), n(0x10, 0)],
    ];
    for index in 0..3 {
        bodies.push(vec![n(0x101b, 7 + index * 2), n(0x10, 0)]);
    }
    module(
        &pool,
        if static_fields { &fields } else { &[] },
        if static_fields { &[] } else { &fields },
        &[u32::MAX, 0, 1, u32::MAX, 0],
        &bodies,
    )
}
#[test]
fn instance_initializers_observe_only_the_installed_field_prefix() {
    let Some(records) = rows(
        "visibility",
        "instance_initializers_observe_only_the_installed_field_prefix",
        6,
    ) else {
        return;
    };
    let records = records
        .into_iter()
        .filter(|row| row["result"]["kind"] == "instance")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    for row in records {
        let read = row["input"]["index"].as_u64().unwrap() as usize;
        let mut vm = VmHost::new();
        vm.loader.load_file(&visibility_file(read, false)).unwrap();
        let outcome = vm.alloc_object("ReadClass");
        if row["result"]["outcome"] == "ok" {
            assert!(outcome.is_ok(), "{outcome:?}");
        } else {
            assert!(
                outcome.is_err(),
                "later field was fabricated as zero: case{read}"
            );
        }
        assert_eq!(vm.objects.len(), 1);
        let object = &vm.objects[0];
        let object_class = match &object.class {
            slr_vm::vmclass::ClassRef::Loaded(id) => vm.loader.class_name(*id).unwrap().to_owned(),
            slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
        };
        assert_eq!(object_class, "ReadClass");
        let actual=object.fields.iter().enumerate().map(|(index,value)|json!({"name":(["a","b","c"][index]),"type":value.tag.name(),"value":value.payload})).collect::<Vec<_>>();
        assert_eq!(json!(actual), row["result"]["instance"], "case{read}");
    }
}

#[test]
fn static_initializers_keep_partial_cells_visible_without_inventing_later_fields() {
    let Some(records) = rows(
        "visibility",
        "static_initializers_keep_partial_cells_visible_without_inventing_later_fields",
        6,
    ) else {
        return;
    };
    let records = records
        .into_iter()
        .filter(|row| row["result"]["kind"] == "static")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    for row in records {
        let read = row["input"]["index"].as_u64().unwrap() as usize;
        let mut vm = VmHost::new();
        vm.loader.load_file(&visibility_file(read, true)).unwrap();
        let outcome = execute_body(&mut vm, "ReadClass", 4);
        if row["result"]["outcome"] == "ok" {
            assert!(outcome.is_ok(), "{outcome:?}");
        } else {
            assert!(
                outcome.is_err(),
                "later static field was fabricated as zero"
            );
        }
        // This explicit embedding reentry compares only the observed owner
        // sentinel/partial storage. It does not execute original unwind.
        let expected = row["result"]["state"]["ReadClass"]["static"]
            .as_array()
            .unwrap();
        let actual = expected
            .iter()
            .map(|field| {
                let name = field["name"].as_str().unwrap();
                let index = ["a", "b", "c"]
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap();
                let value = execute_body(&mut vm, "ReadClass", 3 + index).unwrap();
                json!({"name":name,"type":value.tag.name(),"value":value.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(actual), json!(expected));
        if expected.len() < 3 {
            assert!(execute_body(&mut vm, "ReadClass", 5).is_err());
        }
    }
}

thread_local! {static RIDS:std::cell::RefCell<Vec<Json>>=const {std::cell::RefCell::new(Vec::new())};}
fn resolver(
    _vm: &mut VmHost,
    package: Option<&str>,
    id: u32,
) -> Result<u32, slr_vm::interp::VmError> {
    RIDS.with(|calls| {
        calls
            .borrow_mut()
            .push(json!({"resource":package,"local_id":id}))
    });
    Ok(0x12340000 | id)
}
fn execute_body(
    vm: &mut VmHost,
    class: &str,
    index: usize,
) -> Result<slr_vm::values::Value, slr_vm::interp::VmError> {
    use slr_vm::interp::{Frame, Pc, Thread};
    let body = vm.loader.body_id(class, index).unwrap();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0);
    frame.class_ctx = vm.class_context(class).unwrap();
    let mut thread = Thread::new();
    thread.frames.push(frame);
    thread.pc = pc;
    thread.run(vm, 0., &mut || slr_x87::X87::from_f64(0.))?;
    thread.pop_deref_with(vm)
}
#[test]
fn resource_initializers_use_the_real_literal_boundary_in_both_drivers() {
    let Some(records) = rows(
        "payloads",
        "resource_initializers_use_the_real_literal_boundary_in_both_drivers",
        12,
    ) else {
        return;
    };
    let records = records
        .into_iter()
        .filter(|row| row["result"]["body"] == "resource")
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    for row in records {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let pool = vec![
            PoolEntry::Str("Holder".into()),
            PoolEntry::Class(0),
            PoolEntry::Str("I".into()),
            PoolEntry::Str("value".into()),
            PoolEntry::Str("later".into()),
            PoolEntry::Str("system.rpk".into()),
            PoolEntry::Member { name: 5, extra: 27 },
            PoolEntry::Type7 { a: 3, b: 2 },
            PoolEntry::Type5 { a: 1, b: 7 },
            PoolEntry::Type7 { a: 4, b: 2 },
            PoolEntry::Type5 { a: 1, b: 9 },
        ];
        let fields = vec![
            [if static_fields { 0x28 } else { 1 }, 3, 2, 0],
            [if static_fields { 0x28 } else { 1 }, 4, 2, u32::MAX],
        ];
        let bodies = vec![
            vec![n(0x1007, 0x1c), n(0x100e, 6), n(0x10, 0)],
            vec![n(0x101b, 8), n(0x10, 0)],
            vec![n(0x101b, 10), n(0x10, 0)],
        ];
        let mut vm = VmHost::new();
        vm.loader
            .load_file(&module(
                &pool,
                if static_fields { &fields } else { &[] },
                if static_fields { &[] } else { &fields },
                &[u32::MAX, 0, 1, u32::MAX, 0],
                &bodies,
            ))
            .unwrap();
        vm.resource_resolver = Some(resolver);
        RIDS.with(|calls| calls.borrow_mut().clear());
        let actual = if static_fields {
            [
                execute_body(&mut vm, "Holder", 1).unwrap(),
                execute_body(&mut vm, "Holder", 2).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        let actual=actual.into_iter().enumerate().map(|(index,value)|json!({"name":(["value","later"].get(index).unwrap()),"type":value.tag.name(),"value":value.payload})).collect::<Vec<_>>();
        let expected = if static_fields {
            &row["result"]["state"]["Holder"]["static"]
        } else {
            &row["result"]["instance"]
        };
        assert_eq!(json!(actual), *expected);
        assert_eq!(
            RIDS.with(|calls| json!(*calls.borrow())),
            row["result"]["calls"]
        );
        assert_eq!(
            vm.objects.len() as u64,
            row["result"]["object_count"].as_u64().unwrap()
        );
    }
}

fn order_resolver(
    _vm: &mut VmHost,
    package: Option<&str>,
    id: u32,
) -> Result<u32, slr_vm::interp::VmError> {
    assert_eq!(package, Some("order.rpk"));
    RIDS.with(|calls| calls.borrow_mut().push(json!(id)));
    Ok(id)
}
fn order_module(
    name: &str,
    base: Option<&str>,
    interfaces: &[&str],
    kind: u32,
    statics: &[(&str, &str, Option<u32>)],
    instances: &[(&str, &str, Option<u32>)],
) -> Vec<u8> {
    let mut pool = vec![
        PoolEntry::Str(name.into()),
        PoolEntry::Class(0),
        PoolEntry::Str("order.rpk".into()),
    ];
    let mut class = vec![u32::MAX, kind, 1, u32::MAX, interfaces.len() as u32];
    for (index, dependency) in base
        .into_iter()
        .chain(interfaces.iter().copied())
        .enumerate()
    {
        let name_index = pool.len() as u32;
        pool.push(PoolEntry::Str(dependency.into()));
        let class_index = pool.len() as u32;
        pool.push(PoolEntry::Class(name_index));
        if base.is_some() && index == 0 {
            class[3] = class_index;
        } else {
            class.push(class_index);
        }
    }
    let mut fields = [vec![], vec![]];
    let mut bodies = vec![];
    for (group, entries) in [statics, instances].into_iter().enumerate() {
        for (name, ty, id) in entries {
            let name_index = pool.len() as u32;
            pool.push(PoolEntry::Str((*name).into()));
            let type_index = pool.len() as u32;
            pool.push(PoolEntry::Str((*ty).into()));
            let initializer = if let Some(id) = id {
                let literal = pool.len() as u32;
                pool.push(PoolEntry::Member {
                    name: 2,
                    extra: *id,
                });
                let body = bodies.len() as u32;
                bodies.push(vec![n(0x1007, 0x1c), n(0x100e, literal), n(0x10, 0)]);
                body
            } else {
                u32::MAX
            };
            fields[group].push([
                if group == 0 { 0x28 } else { 1 },
                name_index,
                type_index,
                initializer,
            ]);
        }
    }
    for (name, ty, _) in statics {
        let name_index = pool.len() as u32;
        pool.push(PoolEntry::Str((*name).into()));
        let type_index = pool.len() as u32;
        pool.push(PoolEntry::Str((*ty).into()));
        let sig = pool.len() as u32;
        pool.push(PoolEntry::Type7 {
            a: name_index,
            b: type_index,
        });
        let reference = pool.len() as u32;
        pool.push(PoolEntry::Type5 { a: 1, b: sig });
        bodies.push(vec![n(0x101b, reference), n(0x10, 0)]);
    }
    module(&pool, &fields[0], &fields[1], &class, &bodies)
}
#[test]
fn class_and_instance_initializers_follow_original_dependency_and_field_order() {
    let Some(records) = rows(
        "order",
        "class_and_instance_initializers_follow_original_dependency_and_field_order",
        2,
    ) else {
        return;
    };
    for row in &records {
        let instance = row["input"]["mode"] == "instance";
        let mut vm = VmHost::new();
        for file in [
            order_module(
                "Base",
                None,
                &[],
                0,
                &[("bs", "I", Some(11))],
                &[("bi", "I", Some(12))],
            ),
            order_module("InterfaceOne", None, &[], 1, &[("is", "I", Some(21))], &[]),
            order_module(
                "Leaf",
                Some("Base"),
                &["InterfaceOne"],
                0,
                &[("ls", "I", Some(31)), ("ls2", "F", None)],
                &[("li", "I", Some(32)), ("li2", "F", None)],
            ),
        ] {
            vm.loader.load_file(&file).unwrap();
        }
        vm.resource_resolver = Some(order_resolver);
        RIDS.with(|calls| calls.borrow_mut().clear());
        let word = if instance {
            vm.alloc_object("Leaf").unwrap()
        } else {
            execute_body(&mut vm, "Leaf", 2).unwrap();
            0
        };
        // Literal callbacks make dependency ordering observable at the public seam;
        // expected IDs are the original driver's completed field values.
        let state = &row["result"]["state"];
        let mut expected = vec![
            state["Base"]["static"][0]["value"].clone(),
            state["InterfaceOne"]["static"][0]["value"].clone(),
            state["Leaf"]["static"][0]["value"].clone(),
        ];
        if instance {
            expected.extend([
                row["result"]["instance"][0]["value"].clone(),
                row["result"]["instance"][1]["value"].clone(),
            ]);
        }
        assert_eq!(RIDS.with(|calls| calls.borrow().clone()), expected);
        for field in row["result"]["instance"].as_array().into_iter().flatten() {
            let value = vm
                .read_field(word, field["name"].as_str().unwrap())
                .unwrap();
            assert_eq!(value.tag.name(), field["type"].as_str().unwrap());
            assert_eq!(value.payload, field["value"].as_u64().unwrap() as u32);
        }
        assert_eq!(vm.objects.len(), usize::from(instance));
    }
}

#[test]
fn canonical_self_static_installs_zero_and_skips_its_initializer() {
    let Some(records) = rows(
        "payloads",
        "canonical_self_static_installs_zero_and_skips_its_initializer",
        12,
    ) else {
        return;
    };
    let row = records
        .iter()
        .find(|row| row["input"]["kind"] == "self_new" && row["input"]["static"] == true)
        .unwrap();
    let pool = vec![
        PoolEntry::Str("Holder".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("LHolder;".into()),
        PoolEntry::Str("value".into()),
        PoolEntry::Str("later".into()),
        PoolEntry::Str("I".into()),
        PoolEntry::Type7 { a: 3, b: 2 },
        PoolEntry::Type5 { a: 1, b: 6 },
        PoolEntry::Type7 { a: 4, b: 5 },
        PoolEntry::Type5 { a: 1, b: 8 },
    ];
    let bodies = vec![
        vec![n(0x1007, 0x1e), n(0x1006, 0), n(0x10, 0)],
        vec![n(0x101b, 7), n(0x10, 0)],
        vec![n(0x101b, 9), n(0x10, 0)],
    ];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module(
            &pool,
            &[[0x28, 3, 2, 0], [0x28, 4, 5, u32::MAX]],
            &[],
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &bodies,
        ))
        .unwrap();
    let actual=[execute_body(&mut vm,"Holder",1).unwrap(),execute_body(&mut vm,"Holder",2).unwrap()].into_iter().enumerate().map(|(index,value)|json!({"name":(["value","later"][index]),"type":value.tag.name(),"value":value.payload})).collect::<Vec<_>>();
    assert_eq!(json!(actual), row["result"]["state"]["Holder"]["static"]);
    assert_eq!(
        vm.objects.len(),
        row["result"]["object_count"].as_u64().unwrap() as usize
    );
}

fn payload_vm(kind: &str, static_fields: bool) -> VmHost {
    let ty = match kind {
        "string" => "Ljava.lang.String;",
        "cast" => "F",
        "new" => "LTarget;",
        _ => "I",
    };
    let pool = vec![
        PoolEntry::Str("Holder".into()),
        PoolEntry::Class(0),
        PoolEntry::Str(ty.into()),
        PoolEntry::Str("value".into()),
        PoolEntry::Str("later".into()),
        PoolEntry::Str("I".into()),
        PoolEntry::Str("initializer text".into()),
        PoolEntry::Str("Target".into()),
        PoolEntry::Str("F".into()),
        PoolEntry::Str("make".into()),
        PoolEntry::Str("()I".into()),
        PoolEntry::Type7 { a: 3, b: 2 },
        PoolEntry::Type5 { a: 1, b: 11 },
        PoolEntry::Type7 { a: 4, b: 5 },
        PoolEntry::Type5 { a: 1, b: 13 },
        PoolEntry::Type7 { a: 9, b: 10 },
        PoolEntry::Type5 { a: 1, b: 15 },
    ];
    let expr = match kind {
        "string" => vec![n(0x1007, 0x1c), n(0x4009, 6), n(0x10, 0)],
        "cast" => vec![
            n(0x1007, 0x1c),
            n(0x100b, 11),
            n(0x1007, 0x21),
            n(0x1006, 8),
            n(0x10, 0),
        ],
        "new" => vec![n(0x1007, 0x1e), n(0x1006, 7), n(0x10, 0)],
        "script_call" => vec![
            n(0x1007, 0x1c),
            n(0x100b, 0),
            n(0x24, 0),
            n(0x1011, 1),
            n(0x101a, 16),
            n(0x10, 0),
        ],
        _ => panic!("unknown fixture kind"),
    };
    let bodies = vec![
        expr,
        vec![n(0x1007, 0x1c), n(0x100b, 77), n(0x2b, 0)],
        vec![n(0x101b, 12), n(0x10, 0)],
        vec![n(0x101b, 14), n(0x10, 0)],
    ];
    let fields = [
        [if static_fields { 0x28 } else { 1 }, 3, 2, 0],
        [if static_fields { 0x28 } else { 1 }, 4, 5, u32::MAX],
    ];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module_methods(
            &pool,
            if static_fields { &fields } else { &[] },
            if static_fields { &[] } else { &fields },
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &bodies,
            if kind == "script_call" {
                &[[9, 9, 10, 1, 0]]
            } else {
                &[]
            },
        ))
        .unwrap();
    vm.loader
        .load_file(&order_module("Target", None, &[], 0, &[], &[]))
        .unwrap();
    vm.loader
        .load_file(&order_module(
            "java.lang.String",
            None,
            &[],
            0,
            &[],
            &[("ptr", "I", None)],
        ))
        .unwrap();
    vm
}
#[test]
fn string_initializers_store_real_interned_heap_objects_in_both_drivers() {
    let Some(records) = rows(
        "payloads",
        "string_initializers_store_real_interned_heap_objects_in_both_drivers",
        12,
    ) else {
        return;
    };
    for row in records
        .into_iter()
        .filter(|row| row["input"]["kind"] == "string")
    {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = payload_vm("string", static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 2).unwrap(),
                execute_body(&mut vm, "Holder", 3).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        let expected = if static_fields {
            &row["result"]["state"]["Holder"]["static"]
        } else {
            &row["result"]["instance"]
        };
        assert_eq!(values[0].tag.name(), expected[0]["type"].as_str().unwrap());
        let text = row["result"]["calls"][0]["string"]
            .as_str()
            .unwrap()
            .as_bytes();
        assert_eq!(vm.string_bytes(values[0].payload), Some(text));
        assert_eq!(vm.intern_string(Some(text)).unwrap(), values[0].payload);
        assert_eq!(
            values[1].payload,
            expected[1]["value"].as_u64().unwrap() as u32
        );
        assert_eq!(vm.objects.len(), if static_fields { 1 } else { 2 });
    }
}

#[test]
fn cast_initializers_use_the_declared_float_cell_in_both_drivers() {
    let Some(records) = rows(
        "payloads",
        "cast_initializers_use_the_declared_float_cell_in_both_drivers",
        12,
    ) else {
        return;
    };
    for row in records
        .into_iter()
        .filter(|row| row["input"]["kind"] == "cast")
    {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = payload_vm("cast", static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 2).unwrap(),
                execute_body(&mut vm, "Holder", 3).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        let actual=values.into_iter().enumerate().map(|(index,value)|json!({"name":(["value","later"][index]),"type":value.tag.name(),"value":value.payload})).collect::<Vec<_>>();
        let expected = if static_fields {
            &row["result"]["state"]["Holder"]["static"]
        } else {
            &row["result"]["instance"]
        };
        assert_eq!(json!(actual), *expected);
    }
}

#[test]
fn new_initializers_allocate_real_target_objects_in_both_drivers() {
    let Some(records) = rows(
        "payloads",
        "new_initializers_allocate_real_target_objects_in_both_drivers",
        12,
    ) else {
        return;
    };
    for row in records
        .into_iter()
        .filter(|row| row["input"]["kind"] == "new")
    {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = payload_vm("new", static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 2).unwrap(),
                execute_body(&mut vm, "Holder", 3).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        let expected = if static_fields {
            &row["result"]["state"]["Holder"]["static"]
        } else {
            &row["result"]["instance"]
        };
        assert_eq!(values[0].tag.name(), expected[0]["type"].as_str().unwrap());
        // Original supplies allocation at404e20; compare its requested runtime
        // class, not that seam's arbitrary pointer, against the real Rust heap.
        let expected_class = row["result"]["calls"][1]["new"].as_str().unwrap();
        let actual_class = match &vm.objects[(values[0].payload - 0x1000) as usize].class {
            slr_vm::vmclass::ClassRef::Loaded(id) => vm.loader.class_name(*id).unwrap().to_owned(),
            slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
        };
        assert_eq!(actual_class, expected_class);
        assert_eq!(
            values[1].payload,
            expected[1]["value"].as_u64().unwrap() as u32
        );
        assert_eq!(vm.objects.len(), if static_fields { 1 } else { 2 });
    }
}

#[test]
fn script_call_initializers_return_into_the_declared_field_in_both_drivers() {
    let Some(records) = rows(
        "payloads",
        "script_call_initializers_return_into_the_declared_field_in_both_drivers",
        12,
    ) else {
        return;
    };
    for row in records
        .into_iter()
        .filter(|row| row["input"]["kind"] == "script_call")
    {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = payload_vm("script_call", static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 2).unwrap(),
                execute_body(&mut vm, "Holder", 3).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        let actual=values.into_iter().enumerate().map(|(index,value)|json!({"name":(["value","later"][index]),"type":value.tag.name(),"value":value.payload})).collect::<Vec<_>>();
        let expected = if static_fields {
            &row["result"]["state"]["Holder"]["static"]
        } else {
            &row["result"]["instance"]
        };
        assert_eq!(json!(actual), *expected);
        assert_eq!(vm.objects.len(), if static_fields { 0 } else { 1 });
    }
}

#[test]
fn dependency_kind_diagnostics_continue_initialization_like_original() {
    let Some(records) = rows(
        "dependency_kind",
        "dependency_kind_diagnostics_continue_initialization_like_original",
        8,
    ) else {
        return;
    };
    for row in records {
        let instance = row["input"]["instance"].as_bool().unwrap();
        let mut vm = VmHost::new();
        for file in [
            order_module(
                "Base",
                None,
                &[],
                row["result"]["base_kind"].as_u64().unwrap() as u32,
                &[("base", "I", Some(11))],
                &[],
            ),
            order_module(
                "InterfaceOne",
                None,
                &[],
                row["result"]["interface_kind"].as_u64().unwrap() as u32,
                &[("iface", "I", Some(21))],
                &[],
            ),
            order_module(
                "Leaf",
                Some("Base"),
                &["InterfaceOne"],
                0,
                &[("own", "I", Some(31))],
                &[("item", "I", Some(32))],
            ),
        ] {
            vm.loader.load_file(&file).unwrap();
        }
        vm.resource_resolver = Some(order_resolver);
        RIDS.with(|calls| calls.borrow_mut().clear());
        if instance {
            let word = vm.alloc_object("Leaf").unwrap();
            assert_eq!(
                vm.read_field(word, "item").unwrap().payload,
                row["result"]["instance_fields"][0]["value"]
                    .as_u64()
                    .unwrap() as u32
            );
        } else {
            assert_eq!(execute_body(&mut vm, "Leaf", 2).unwrap().payload, 31);
        }
        let state = &row["result"]["state"];
        let mut expected = vec![
            state["Base"]["static"][0]["value"].clone(),
            state["InterfaceOne"]["static"][0]["value"].clone(),
            state["Leaf"]["static"][0]["value"].clone(),
        ];
        if instance {
            expected.push(row["result"]["instance_fields"][0]["value"].clone());
        }
        assert_eq!(RIDS.with(|calls| calls.borrow().clone()), expected);
        let mismatch = row["result"]["interface_kind"] != 1;
        assert_eq!(
            !row["result"]["logs"].as_array().unwrap().is_empty(),
            mismatch
        );
        assert_eq!(vm.initialization_diagnostics.len(), usize::from(mismatch));
        if mismatch {
            assert_eq!(
                vm.initialization_diagnostics[0],
                slr_vm::host::InitializationDiagnostic::InterfaceExtendsClass {
                    class: "Leaf".into(),
                    interface: "InterfaceOne".into(),
                    kind: row["result"]["interface_kind"].as_u64().unwrap() as u32
                }
            );
        }
    }
}

fn assignment_vm(kind: &str, static_fields: bool) -> VmHost {
    let ty = match kind {
        "numeric_string" | "float_string" => "Ljava.lang.String;",
        "numeric_other" | "reference_other" | "mutate_reference_then_reference" => "LTarget;",
        _ => "I",
    };
    let pool = vec![
        PoolEntry::Str("Holder".into()),
        PoolEntry::Class(0),
        PoolEntry::Str(ty.into()),
        PoolEntry::Str("value".into()),
        PoolEntry::Str("later".into()),
        PoolEntry::Str("I".into()),
        PoolEntry::Str("supplied source".into()),
        PoolEntry::Str("Target".into()),
        PoolEntry::Type7 { a: 3, b: 2 },
        PoolEntry::Type5 { a: 1, b: 8 },
        PoolEntry::Type7 { a: 4, b: 5 },
        PoolEntry::Type5 { a: 1, b: 10 },
    ];
    let mut expr = match kind {
        "mutate_integer_then_reference" | "mutate_integer_then_fatal" => vec![
            n(0x1007, 0x1c),
            n(0x100b, 99),
            n(0x101b, 9),
            n(0x1008, 0x23),
        ],
        "mutate_reference_then_reference" => {
            vec![n(0x1007, 0x1e), n(0x1006, 7), n(0x101b, 9), n(0x1008, 0x23)]
        }
        _ => vec![],
    };
    expr.extend(match kind {
        "float_string" => vec![n(0x1007, 0x1c), n(0x200a, 0x40200000)],
        "numeric_string" | "numeric_other" => vec![n(0x1007, 0x1c), n(0x100b, 11)],
        "mutate_integer_then_fatal" => vec![n(0x0, 0)],
        _ => vec![n(0x1007, 0x1c), n(0x4009, 6)],
    });
    expr.push(n(0x10, 0));
    let bodies = vec![
        expr,
        vec![n(0x101b, 9), n(0x10, 0)],
        vec![n(0x101b, 11), n(0x10, 0)],
    ];
    let fields = [
        [if static_fields { 0x28 } else { 1 }, 3, 2, 0],
        [if static_fields { 0x28 } else { 1 }, 4, 5, u32::MAX],
    ];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module(
            &pool,
            if static_fields { &fields } else { &[] },
            if static_fields { &[] } else { &fields },
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &bodies,
        ))
        .unwrap();
    for class in ["Target", "Other"] {
        vm.loader
            .load_file(&order_module(class, None, &[], 0, &[], &[]))
            .unwrap();
    }
    vm.loader
        .load_file(&order_module(
            "java.lang.String",
            None,
            &[],
            0,
            &[],
            &[("ptr", "I", None)],
        ))
        .unwrap();
    vm
}
#[test]
fn numeric_reference_initializer_conversion_preserves_original_continue_and_copy() {
    let Some(records) = rows(
        "assignment",
        "numeric_reference_initializer_conversion_preserves_original_continue_and_copy",
        16,
    ) else {
        return;
    };
    for row in records.into_iter().filter(|row| {
        matches!(
            row["input"]["kind"].as_str().unwrap(),
            "numeric_string" | "float_string" | "numeric_other"
        )
    }) {
        let kind = row["input"]["kind"].as_str().unwrap();
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = assignment_vm(kind, static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 1).unwrap(),
                execute_body(&mut vm, "Holder", 2).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        assert_eq!(
            !vm.initialization_diagnostics.is_empty(),
            !row["result"]["diagnostic_fragments"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for (index, value) in values.into_iter().enumerate() {
            let expected = &row["result"]["fields"][index];
            assert_eq!(value.tag.name(), expected["type"].as_str().unwrap());
            if expected["normalized_value"] == "java.lang.String" {
                let text = row["result"]["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find_map(|call| call["intern_text"].as_str())
                    .unwrap();
                assert_eq!(vm.string_bytes(value.payload), Some(text.as_bytes()));
            } else {
                assert_eq!(value.payload, expected["value"].as_u64().unwrap() as u32);
            }
        }
    }
}

#[test]
fn invalid_reference_results_overwrite_prior_writes_and_continue_like_original() {
    let Some(records) = rows(
        "assignment",
        "invalid_reference_results_overwrite_prior_writes_and_continue_like_original",
        16,
    ) else {
        return;
    };
    for row in records.into_iter().filter(|row| {
        matches!(
            row["input"]["kind"].as_str().unwrap(),
            "reference_other"
                | "reference_integer"
                | "mutate_integer_then_reference"
                | "mutate_reference_then_reference"
        )
    }) {
        let kind = row["input"]["kind"].as_str().unwrap();
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = assignment_vm(kind, static_fields);
        let values = if static_fields {
            [
                execute_body(&mut vm, "Holder", 1).unwrap(),
                execute_body(&mut vm, "Holder", 2).unwrap(),
            ]
        } else {
            let word = vm.alloc_object("Holder").unwrap();
            [
                vm.read_field(word, "value").unwrap(),
                vm.read_field(word, "later").unwrap(),
            ]
        };
        assert_eq!(
            row["result"]["fields"][0]["normalized_value"],
            "java.lang.String"
        );
        assert_eq!(
            values[0].tag.name(),
            row["result"]["fields"][0]["type"].as_str().unwrap()
        );
        let text = row["result"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|call| call["intern_text"].as_str())
            .unwrap();
        assert_eq!(vm.string_bytes(values[0].payload), Some(text.as_bytes()));
        assert_eq!(
            values[1].payload,
            row["result"]["fields"][1]["value"].as_u64().unwrap() as u32
        );
        assert!(!row["result"]["diagnostic_fragments"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(vm
            .initialization_diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                slr_vm::host::InitializationDiagnostic::Body(
                    slr_vm::ops::Diagnostic::AssignConversion(_)
                )
            )));
    }
}
#[test]
fn fatal_initializer_stops_before_assignment_and_retains_current_side_effect() {
    let Some(records) = rows(
        "assignment",
        "fatal_initializer_stops_before_assignment_and_retains_current_side_effect",
        16,
    ) else {
        return;
    };
    for row in records
        .into_iter()
        .filter(|row| row["input"]["kind"] == "mutate_integer_then_fatal")
    {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut vm = assignment_vm("mutate_integer_then_fatal", static_fields);
        let outcome = if static_fields {
            execute_body(&mut vm, "Holder", 1).map(|_| ())
        } else {
            vm.alloc_object("Holder").map(|_| ()).map_err(Into::into)
        };
        assert!(outcome.is_err());
        // Observe only the explicit stop state. Reentry is an embedding call,
        // not evidence for the original fatal handler's unwind/continuation.
        let value = if static_fields {
            execute_body(&mut vm, "Holder", 1).unwrap()
        } else {
            vm.read_field(0x1000, "value").unwrap()
        };
        assert_eq!(
            value.tag.name(),
            row["result"]["fields"][0]["type"].as_str().unwrap()
        );
        assert_eq!(
            value.payload,
            row["result"]["fields"][0]["value"].as_u64().unwrap() as u32
        );
        let later = if static_fields {
            execute_body(&mut vm, "Holder", 2)
        } else {
            vm.read_field(0x1000, "later").map_err(Into::into)
        };
        assert!(later.is_err());
        assert_eq!(vm.objects.len(), usize::from(!static_fields));
    }
}

#[test]
fn unsupported_later_initializer_preserves_the_already_installed_prefix() {
    // A fatal opcode in a later initializer must fail where reached rather
    // than preflight away earlier writes (the former #110 guard is gone: the
    // interpreter's own fatal-opcode stop is what remains).
    let pool = vec![
        PoolEntry::Str("Partial".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("I".into()),
        PoolEntry::Str("a".into()),
        PoolEntry::Str("b".into()),
        PoolEntry::Str("c".into()),
    ];
    let bodies = vec![
        vec![n(0x1007, 0x1c), n(0x100b, 11), n(0x10, 0)],
        vec![n(0x0000, 0), n(0x10, 0)],
    ];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module(
            &pool,
            &[],
            &[[1, 3, 2, 0], [1, 4, 2, 1], [1, 5, 2, u32::MAX]],
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &bodies,
        ))
        .unwrap();
    assert!(vm.alloc_object("Partial").is_err());
    assert_eq!(vm.objects.len(), 1);
    assert_eq!(vm.read_field(0x1000, "a").unwrap().payload, 11);
    assert_eq!(vm.read_field(0x1000, "b").unwrap().payload, 0);
    assert!(vm.read_field(0x1000, "c").is_err());
}

#[test]
fn inherited_initializer_frames_keep_runtime_receiver_and_shadowed_slots_stable() {
    let mut vm = VmHost::new();
    vm.loader.load_file(&visibility_file(0, false)).unwrap();
    vm.loader
        .load_file(&order_module(
            "Leaf",
            Some("ReadClass"),
            &[],
            0,
            &[("unused", "I", None)],
            &[("a", "F", Some(2))],
        ))
        .unwrap();
    vm.resource_resolver = Some(order_resolver);
    let word = vm.alloc_object("Leaf").unwrap();
    let actual = vm.objects[(word - 0x1000) as usize].fields.clone();
    assert_eq!(
        actual,
        vec![
            slr_vm::values::Value::int(11),
            slr_vm::values::Value::int(11),
            slr_vm::values::Value::int(33),
            slr_vm::values::Value::float(2.)
        ]
    );
    // The base initializer's implicit101b read used the runtime Leaf receiver
    // but its defining ReadClass pool. Named lookup still finds Leaf's shadow.
    assert_eq!(
        vm.read_field(word, "a").unwrap(),
        slr_vm::values::Value::float(2.)
    );
    assert_eq!(
        vm.read_field(word, "b").unwrap(),
        slr_vm::values::Value::int(11)
    );
}
#[test]
fn declared_interface_order_and_shared_dependencies_initialize_once() {
    let Some(records) = rows(
        "interfaces",
        "declared_interface_order_and_shared_dependencies_initialize_once",
        1,
    ) else {
        return;
    };
    let row = &records[0];
    let mut vm = VmHost::new();
    for file in [
        order_module("InterfaceA", None, &[], 1, &[("A", "I", Some(1))], &[]),
        order_module("InterfaceB", None, &[], 1, &[("B", "I", Some(2))], &[]),
        order_module(
            "Declaring",
            None,
            &["InterfaceB", "InterfaceA"],
            0,
            &[("C", "I", Some(3))],
            &[],
        ),
    ] {
        vm.loader.load_file(&file).unwrap();
    }
    vm.resource_resolver = Some(order_resolver);
    RIDS.with(|calls| calls.borrow_mut().clear());
    execute_body(&mut vm, "Declaring", 1).unwrap();
    let expected = row["result"]["add_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| {
            let owner = match name.as_str().unwrap() {
                "B" => "InterfaceB",
                "A" => "InterfaceA",
                _ => "Declaring",
            };
            &row["result"]["state"][owner]["static"][0]["value"]
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(RIDS.with(|calls| calls.borrow().clone()), expected);
    // A second owner shares both interfaces: their installed sentinels prevent
    // duplicate work. This diamond regression is a Rust graph boundary check.
    vm.loader
        .load_file(&order_module(
            "Sibling",
            None,
            &["InterfaceA", "InterfaceB"],
            0,
            &[("D", "I", Some(4))],
            &[],
        ))
        .unwrap();
    execute_body(&mut vm, "Sibling", 1).unwrap();
    assert_eq!(
        RIDS.with(|calls| calls.borrow().clone()),
        vec![json!(2), json!(1), json!(3), json!(4)]
    );
}
#[test]
fn missing_dependency_failure_clears_active_path_without_installing_child() {
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&order_module(
            "Leaf",
            Some("Base"),
            &["InterfaceMissing"],
            0,
            &[("leaf", "I", Some(2))],
            &[],
        ))
        .unwrap();
    vm.loader
        .load_file(&order_module(
            "Base",
            None,
            &[],
            0,
            &[("base", "I", Some(1))],
            &[],
        ))
        .unwrap();
    vm.resource_resolver = Some(order_resolver);
    RIDS.with(|calls| calls.borrow_mut().clear());
    assert!(vm.alloc_object("Leaf").is_err());
    assert!(vm.objects.is_empty());
    assert!(RIDS.with(|calls| calls.borrow().is_empty()));
    vm.loader
        .load_file(&order_module("InterfaceMissing", None, &[], 1, &[], &[]))
        .unwrap();
    vm.alloc_object("Leaf").unwrap();
    assert_eq!(
        RIDS.with(|calls| calls.borrow().clone()),
        vec![json!(1), json!(2)]
    );
}
#[test]
fn malformed_dependency_cycles_fail_explicitly_before_installation() {
    let mut vm = VmHost::new();
    for file in [
        order_module("A", None, &["B"], 1, &[], &[]),
        order_module("B", None, &["A"], 1, &[], &[]),
    ] {
        vm.loader.load_file(&file).unwrap();
    }
    for _ in 0..2 {
        assert_eq!(
            vm.alloc_object("A"),
            Err(slr_vm::host::HostError::UnsupportedInitializer(
                "cyclic initialization dependency".into()
            ))
        );
        assert!(vm.objects.is_empty());
    }
}

#[test]
fn self_descriptor_skip_is_static_and_matches_only_the_defining_owner() {
    let Some(records) = rows(
        "self_descriptor",
        "self_descriptor_skip_is_static_and_matches_only_the_defining_owner",
        3,
    ) else {
        return;
    };
    for row in records {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let ancestor = row["input"]["ancestor"].as_bool().unwrap();
        let pool = vec![
            PoolEntry::Str("Holder".into()),
            PoolEntry::Class(0),
            PoolEntry::Str(if ancestor { "LBase;" } else { "LHolder;" }.into()),
            PoolEntry::Str("value".into()),
            PoolEntry::Str("later".into()),
            PoolEntry::Str("I".into()),
            PoolEntry::Type7 { a: 3, b: 2 },
            PoolEntry::Type5 { a: 1, b: 6 },
            PoolEntry::Type7 { a: 4, b: 5 },
            PoolEntry::Type5 { a: 1, b: 8 },
            PoolEntry::Str("Base".into()),
            PoolEntry::Class(10),
        ];
        let bodies = vec![
            vec![n(0, 0), n(0x10, 0)],
            vec![n(0x101b, 7), n(0x10, 0)],
            vec![n(0x101b, 9), n(0x10, 0)],
        ];
        let fields = [
            [if static_fields { 0x28 } else { 1 }, 3, 2, 0],
            [if static_fields { 0x28 } else { 1 }, 4, 5, u32::MAX],
        ];
        let mut vm = VmHost::new();
        vm.loader
            .load_file(&order_module("Base", None, &[], 0, &[], &[]))
            .unwrap();
        vm.loader
            .load_file(&module(
                &pool,
                if static_fields { &fields } else { &[] },
                if static_fields { &[] } else { &fields },
                &[u32::MAX, 0, 1, if ancestor { 11 } else { u32::MAX }, 0],
                &bodies,
            ))
            .unwrap();
        let outcome = if static_fields {
            execute_body(&mut vm, "Holder", 1).map(|_| ())
        } else {
            vm.alloc_object("Holder").map(|_| ()).map_err(Into::into)
        };
        assert_eq!(outcome.is_ok(), row["result"]["outcome"] == "ok");
        for field in row["result"]["fields"].as_array().unwrap() {
            let name = field["name"].as_str().unwrap();
            let value = if static_fields {
                execute_body(&mut vm, "Holder", if name == "value" { 1 } else { 2 }).unwrap()
            } else {
                vm.read_field(0x1000, name).unwrap()
            };
            assert_eq!(value.tag.name(), field["type"].as_str().unwrap());
            assert_eq!(value.payload, field["value"].as_u64().unwrap() as u32);
        }
        if row["result"]["fields"].as_array().unwrap().len() == 1 {
            assert!(if static_fields {
                execute_body(&mut vm, "Holder", 2).is_err()
            } else {
                vm.read_field(0x1000, "later").is_err()
            });
        }
    }
}

#[test]
fn explicit_initializer_nesting_budget_stops_at_the_current_cell_and_resets() {
    let mut vm = VmHost::new();
    vm.loader.load_file(&visibility_file(0, false)).unwrap();
    vm.initializer_depth_limit = 0;
    assert!(vm.alloc_object("ReadClass").is_err());
    assert_eq!(vm.objects.len(), 1);
    assert_eq!(vm.objects[0].fields, vec![slr_vm::values::Value::int(0)]);
    vm.initializer_depth_limit = 2;
    let word = vm.alloc_object("ReadClass").unwrap();
    assert_eq!(vm.read_field(word, "b").unwrap().payload, 11);
}

#[test]
fn recursive_instance_new_stops_at_the_explicit_embedding_budget() {
    let pool = vec![
        PoolEntry::Str("Recursive".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("LRecursive;".into()),
        PoolEntry::Str("value".into()),
        PoolEntry::Str("later".into()),
        PoolEntry::Str("I".into()),
    ];
    let bodies = vec![vec![n(0x1007, 0x1e), n(0x1006, 0), n(0x10, 0)]];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module(
            &pool,
            &[],
            &[[1, 3, 2, 0], [1, 4, 5, u32::MAX]],
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &bodies,
        ))
        .unwrap();
    vm.initializer_depth_limit = 2;
    assert!(vm.alloc_object("Recursive").is_err());
    assert_eq!(vm.objects.len(), 3);
    for object in &vm.objects {
        assert_eq!(
            object.fields,
            vec![slr_vm::values::Value {
                tag: slr_vm::values::TypeTag::intern("LRecursive;"),
                payload: 0
            }]
        );
    }
    // This checks explicit Rust budget cleanup only. It does not replay the
    // unresolved original recursive self-new allocator or machine stack limit.
    vm.loader.load_file(&visibility_file(0, false)).unwrap();
    vm.alloc_object("ReadClass").unwrap();
}

#[test]
fn fatal_initializer_retains_prior_cell_writes_like_original() {
    let Some(records) = rows(
        "failure",
        "fatal_initializer_retains_prior_cell_writes_like_original",
        2,
    ) else {
        return;
    };
    for row in records {
        let static_fields = row["input"]["static"].as_bool().unwrap();
        let mut pool = vec![
            PoolEntry::Str("FailClass".into()),
            PoolEntry::Class(0),
            PoolEntry::Str("I".into()),
            PoolEntry::Str("a".into()),
            PoolEntry::Str("b".into()),
            PoolEntry::Str("c".into()),
        ];
        for index in 0..3 {
            let sig = pool.len() as u32;
            pool.push(PoolEntry::Type7 { a: 3 + index, b: 2 });
            pool.push(PoolEntry::Type5 { a: 1, b: sig });
        }
        let fields = (0..3)
            .map(|index| [if static_fields { 0x28 } else { 1 }, 3 + index, 2, index])
            .collect::<Vec<_>>();
        let mut bodies = vec![
            vec![n(0x1007, 0x1c), n(0x100b, 11), n(0x10, 0)],
            vec![
                n(0x1007, 0x1c),
                n(0x100b, 99),
                n(0x101b, 7),
                n(0x1008, 0x23),
                n(0, 0),
            ],
            vec![n(0x1007, 0x1c), n(0x100b, 33), n(0x10, 0)],
        ];
        for index in 0..3 {
            bodies.push(vec![n(0x101b, 7 + index * 2), n(0x10, 0)]);
        }
        let mut vm = VmHost::new();
        vm.loader
            .load_file(&module(
                &pool,
                if static_fields { &fields } else { &[] },
                if static_fields { &[] } else { &fields },
                &[u32::MAX, 0, 1, u32::MAX, 0],
                &bodies,
            ))
            .unwrap();
        let outcome = if static_fields {
            execute_body(&mut vm, "FailClass", 4).map(|_| ())
        } else {
            vm.alloc_object("FailClass").map(|_| ()).map_err(Into::into)
        };
        assert!(outcome.is_err());
        let expected = if static_fields {
            &row["result"]["state_at_fatal_stop"]["class"]["FailClass"]["static"]
        } else {
            &row["result"]["state_at_fatal_stop"]["instance"]
        };
        let actual = expected
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let name = field["name"].as_str().unwrap();
                let value = if static_fields {
                    execute_body(&mut vm, "FailClass", 3 + index).unwrap()
                } else {
                    vm.read_field(0x1000, name).unwrap()
                };
                json!({"name":name,"type":value.tag.name(),"value":value.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(actual), *expected);
        assert!(if static_fields {
            execute_body(&mut vm, "FailClass", 5).is_err()
        } else {
            vm.read_field(0x1000, "c").is_err()
        });
        assert_eq!(vm.objects.len(), usize::from(!static_fields));
    }
}

/// Plan step B binding-time policy (403e80/4117a0): dependencies resolve
/// through the CURRENT cache selection until the owner's own owner
/// installs, then the owner retains its resolved IDs on reentry. Replays
/// the four accepted binding rows (uninitialized / initialized /
/// failed-dependency / failed-field) at the public seam.
///
/// `wz` shares bucket 227 with `Base`, so registering it first as an
/// unrelated collision head makes later `Base` duplicates flip live name
/// lookup (first/second/third) instead of keeping the first definition.
fn binding_world() -> (VmHost, slr_vm::vmclass::ClassId) {
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&order_module("wz", None, &[], 1, &[], &[]))
        .unwrap();
    vm.loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap();
    vm.loader
        .load_file(&order_module("InterfaceOne", None, &[], 1, &[], &[]))
        .unwrap();
    let leaf_old = vm
        .loader
        .load_file(&order_module(
            "Leaf",
            Some("Base"),
            &["InterfaceOne"],
            0,
            &[],
            &[],
        ))
        .unwrap()[0]
        .0;
    // Duplicate Base + InterfaceOne: the collision head makes name lookup
    // flip to the new records.
    let base_new = vm
        .loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap()[0]
        .0;
    vm.loader
        .load_file(&order_module("InterfaceOne", None, &[], 1, &[], &[]))
        .unwrap();
    assert_eq!(vm.loader.lookup("Base"), Some(base_new));
    (vm, leaf_old)
}

#[test]
fn uninitialized_leaf_binds_new_dependencies_at_install() {
    let (mut vm, leaf_old) = binding_world();
    assert!(vm.initialized_dependencies(leaf_old).is_none());
    let word = vm.alloc_object_id(leaf_old).unwrap();
    assert_ne!(word, 0);
    let (base, ifaces) = vm.initialized_dependencies(leaf_old).unwrap();
    // Install resolved the CURRENT winners (the new duplicates).
    assert_eq!(base, vm.loader.lookup("Base"));
    assert_eq!(ifaces, vec![vm.loader.lookup("InterfaceOne").unwrap()]);
}

#[test]
fn initialized_leaf_retains_old_dependencies_on_reentry() {
    // Pin the OLD dependencies first: initialize before any duplicate flips
    // selection (head topology: `wz` collides with `Base` in bucket 227).
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&order_module("wz", None, &[], 1, &[], &[]))
        .unwrap();
    let base_old = vm
        .loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap()[0]
        .0;
    vm.loader
        .load_file(&order_module("InterfaceOne", None, &[], 1, &[], &[]))
        .unwrap();
    let leaf = vm
        .loader
        .load_file(&order_module(
            "Leaf",
            Some("Base"),
            &["InterfaceOne"],
            0,
            &[],
            &[],
        ))
        .unwrap()[0]
        .0;
    assert_eq!(vm.loader.lookup("Base"), Some(base_old));
    vm.alloc_object_id(leaf).unwrap();
    let (stored_base, stored_ifaces) = vm.initialized_dependencies(leaf).unwrap();
    assert_eq!(stored_base, Some(base_old));
    // Register Base/InterfaceOne duplicates: selection flips, but reentry
    // retains the OLD ids while direct resolution returns the new records.
    let base_new = vm
        .loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap()[0]
        .0;
    vm.loader
        .load_file(&order_module("InterfaceOne", None, &[], 1, &[], &[]))
        .unwrap();
    assert_eq!(vm.loader.lookup("Base"), Some(base_new));
    assert_ne!(base_new, base_old);
    vm.alloc_object_id(leaf).unwrap();
    let (kept_base, kept_ifaces) = vm.initialized_dependencies(leaf).unwrap();
    // InterfaceOne's bucket has no collision head, so its duplicate keeps
    // selecting the first definition — which is also the stored one.
    assert_eq!((kept_base, kept_ifaces), (stored_base, stored_ifaces));
}

#[test]
fn failed_dependency_retry_selects_new_records() {
    // Leaf with a missing interface fails before owner install; after
    // supplying the interface plus a duplicate Base, retry binds new.
    // The collision head `wz` registers BEFORE `Base` so the later
    // duplicate flips selection (insert-after-head past a foreign head).
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&order_module("wz", None, &[], 1, &[], &[]))
        .unwrap();
    vm.loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap();
    let leaf = vm
        .loader
        .load_file(&order_module(
            "Leaf",
            Some("Base"),
            &["InterfaceOne"],
            0,
            &[],
            &[],
        ))
        .unwrap()[0]
        .0;
    assert!(vm.alloc_object_id(leaf).is_err());
    assert!(vm.initialized_dependencies(leaf).is_none());
    // Supply the missing interface and a NEW Base duplicate; the retry
    // resolves through the CURRENT selection (the new records), since no
    // owner installed before the failure.
    vm.loader
        .load_file(&order_module("InterfaceOne", None, &[], 1, &[], &[]))
        .unwrap();
    let base_new = vm
        .loader
        .load_file(&order_module("Base", None, &[], 1, &[], &[]))
        .unwrap()[0]
        .0;
    assert_eq!(vm.loader.lookup("Base"), Some(base_new));
    vm.alloc_object_id(leaf).unwrap();
    let (stored_base, ifaces) = vm.initialized_dependencies(leaf).unwrap();
    assert_eq!(stored_base, Some(base_new));
    assert_eq!(ifaces, vec![vm.loader.lookup("InterfaceOne").unwrap()]);
}

#[test]
fn static_initializer_failure_preserves_field_name_and_body_cause() {
    let pool = vec![
        PoolEntry::Str("Broken".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("I".into()),
        PoolEntry::Str("bad".into()),
    ];
    let mut vm = VmHost::new();
    vm.loader
        .load_file(&module(
            &pool,
            &[[0x28, 3, 2, 0]],
            &[],
            &[u32::MAX, 0, 1, u32::MAX, 0],
            &[vec![n(0x0000, 0), n(0x10, 0)]],
        ))
        .unwrap();
    assert!(vm.alloc_object("Broken").is_err());
    let cause = vm.initializer_failure.unwrap();
    assert!(cause.contains("Broken.bad"), "{cause}");
    assert!(cause.contains("FatalOpcode"), "{cause}");
}

fn constructor_initializer_module(static_field: bool, arguments: u32, integer: bool) -> Vec<u8> {
    let pool = vec![
        PoolEntry::Str("Holder".into()),
        PoolEntry::Class(0),
        PoolEntry::Str("Target".into()),
        PoolEntry::Class(2),
        PoolEntry::Str("LTarget;".into()),
        PoolEntry::Str("value".into()),
    ];
    let mut body = vec![n(0x1007, 0x1e), n(0x1006, 3)];
    for _ in 0..arguments {
        body.extend([
            n(0x1007, 0x1c),
            n(if integer { 0x100b } else { 0x200a }, 17),
        ]);
    }
    body.extend([n(0x1027, arguments), n(0x21, 0), n(0x10, 0)]);
    let fields = [[if static_field { 0x28 } else { 1 }, 5, 4, 0]];
    module(
        &pool,
        if static_field { &fields } else { &[] },
        if static_field { &[] } else { &fields },
        &[u32::MAX, 0, 1, u32::MAX, 0],
        &[body],
    )
}
fn constructor_target(vm: &mut VmHost, args: bool, real_body: bool) -> slr_vm::vmclass::ClassId {
    use slr_vm::vmclass::{Class, Field, Method};
    let target = vm.loader.insert_test_class(Class {
        kind: 0,
        name: "Target".into(),
        base: None,
        interfaces: vec![],
        fields: if real_body {
            vec![Field {
                class: "Target".into(),
                name: "marker".into(),
                ty: "I".into(),
                is_static: false,
            }]
        } else {
            vec![]
        },
        methods: vec![Method {
            class: "Target".into(),
            name: "<init>".into(),
            sig: if args { "(I)" } else { "()" }.into(),
            flags: 1,
            static_vector: false,
            is_native: false,
            body: 0,
            is_abstract: false,
            extra: 0,
        }],
        bodies: if real_body { vec![] } else { vec![vec![]] },
    });
    if real_body {
        vm.add_body(
            "Target",
            vec![n(0x1001, 1), n(0x1d, 0), n(0x1008, 0x23), n(0x2a, 0)],
            vec![PoolEntry::Str("marker".into())],
        )
        .unwrap();
    }
    target
}
#[test]
fn initializer_constructor_original_rows() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/boot/initializer-constructor.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: initializer_constructor_original_rows");
        return;
    };
    let rows: Vec<Json> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let static_field = row["static"].as_bool().unwrap();
        let arguments = row["arguments"].as_u64().unwrap() as u32;
        let mut vm = VmHost::new();
        let target = constructor_target(&mut vm, arguments != 0, false);
        vm.loader
            .load_file(&constructor_initializer_module(
                static_field,
                arguments,
                true,
            ))
            .unwrap();
        let holder = vm.alloc_object("Holder").unwrap();
        let value = vm.read_field(holder, "value").unwrap();
        assert_eq!(row["outcome"], "ok");
        assert_eq!(value.tag.name(), row["value_type"].as_str().unwrap());
        assert_eq!(vm.constructor_calls.len(), 1);
        let call = &vm.constructor_calls[0];
        assert_eq!(
            json!([{"name":"<init>","signature":call.signature,"target_matches":call.class==target}]),
            row["requests"]
        );
        assert_eq!(
            json!([vm.loader.class_name(call.caller_class.unwrap()).unwrap()]),
            row["body_entries"]
        );
        assert_eq!(
            call.arguments,
            if arguments == 0 {
                vec![]
            } else {
                vec![slr_vm::values::Value::int(17)]
            }
        );
        assert_eq!(
            json!([call
                .arguments
                .iter()
                .map(|value| json!({"type":value.tag.name(),"value":value.payload}))
                .collect::<Vec<_>>()]),
            row["collected_arguments"]
        );
        assert_eq!(json!([call.arguments.len()]), row["collector_counts"]);
        assert_eq!(
            json!([u32::from(!call.entered_script_frame.unwrap())]),
            row["dispatch_returns"]
        );
        assert_eq!(value.payload, call.receiver);
        assert_eq!(row["value_matches_allocated_instance"], true);
        assert_eq!(row["receiver_matches_allocated_instance"], true);
        assert!(
            matches!(vm.objects[(value.payload-0x1000) as usize].class,slr_vm::vmclass::ClassRef::Loaded(id) if id==target)
        );
    }
}
#[test]
fn initializer_constructor_runs_script_body_with_integer_argument_and_retains_object() {
    for static_field in [true, false] {
        let mut vm = VmHost::new();
        constructor_target(&mut vm, true, true);
        vm.loader
            .load_file(&constructor_initializer_module(static_field, 1, true))
            .unwrap();
        let holder = vm.alloc_object("Holder").unwrap();
        let value = vm.read_field(holder, "value").unwrap();
        assert_eq!(vm.read_field(value.payload, "marker").unwrap().payload, 17);
        assert_eq!(value.payload, vm.constructor_calls[0].receiver);
        assert_eq!(
            vm.constructor_calls[0].arguments,
            vec![slr_vm::values::Value::int(17)]
        );
    }
}

#[test]
fn transient_initializers_use_current_embedding_thread_precision() {
    use slr_x87::{Precision, X87};
    use std::cell::RefCell;
    thread_local! {
        static OBSERVED: RefCell<Vec<Precision>> = const { RefCell::new(Vec::new()) };
    }
    fn clock() -> X87 {
        OBSERVED.with(|values| {
            values
                .borrow_mut()
                .push(slr_vm::clock::current_controls().0)
        });
        X87::from_i64(0)
    }
    for precision in [Precision::F64, Precision::F32] {
        let mut vm = VmHost::new();
        vm.runtime_precision = precision;
        vm.runtime_clock = Some(clock);
        vm.loader.load_file(&visibility_file(0, false)).unwrap();
        OBSERVED.with(|values| values.borrow_mut().clear());
        let object = vm.alloc_object("ReadClass").unwrap();
        assert_eq!(vm.objects[(object - 0x1000) as usize].fields[0].payload, 11);
        OBSERVED.with(|values| {
            let values = values.borrow();
            assert!(!values.is_empty());
            assert!(values.iter().all(|value| *value == precision));
        });
    }
}
