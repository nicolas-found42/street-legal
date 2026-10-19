//! Original420ff0 field/array lvalue vectors through the public interpreter.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::{Node, PoolEntry as P};
use slr_vm::host::VmHost;
use slr_vm::interp::{ArrayError, Frame, HeapSlot, Host, Pc, Slot, Thread, VmError};
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
fn value(ty: &str, word: u32) -> Value {
    Value {
        tag: TypeTag::intern(ty),
        payload: word,
    }
}
fn field(name: &str, ty: &str, is_static: bool) -> Field {
    Field {
        class: "Fields".into(),
        name: name.into(),
        ty: ty.into(),
        is_static,
    }
}
fn rows(group: &str, test: &str) -> Option<Vec<Json>> {
    let path = std::env::var_os("SLR_VMFIELD_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmfield")
        })
        .join(format!("vmfield_{group}.jsonl"));
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return None;
    };
    Some(
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}
fn jvalue(value: Value) -> Json {
    json!({"type":value.tag.name(),"word":value.payload})
}
fn stack(vm: &mut VmHost, thread: &Thread, word: u32) -> Vec<Json> {
    thread.frames[0]
        .stack
        .iter()
        .map(|slot| match slot {
            Slot::Null => json!({"kind":"null"}),
            Slot::Value(value) => {
                let mut v = jvalue(*value);
                v["kind"] = json!("value");
                v
            }
            Slot::Heap(location) => {
                let identity = match *location {
                    HeapSlot::Object { word: target, slot } => {
                        assert_eq!(target, word);
                        json!(["instance", slot])
                    }
                    HeapSlot::Static { context, slot } => {
                        assert_eq!(Some(context), vm.class_context("Fields"));
                        json!(["static", slot])
                    }
                    HeapSlot::Array {
                        word: target,
                        index,
                    } => {
                        assert_eq!(target, word);
                        json!(["array", index])
                    }
                };
                let mut v = jvalue(vm.read_cell(*location).unwrap());
                v["kind"] = json!("lvalue");
                v["location"] = identity;
                v
            }
            Slot::Var(_) => {
                panic!("field getter returned a local alias instead of the actual host-owned cell")
            }
        })
        .collect()
}
fn enter(vm: &VmHost, body: slr_vm::interp::BodyId, receiver: u32) -> Thread {
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, receiver);
    frame.class_ctx = vm.class_context("Fields").unwrap();
    thread.frames.push(frame);
    thread.pc = pc;
    thread
}
#[test]
fn field_lvalues_match_original_interpreter() {
    let Some(rows) = rows("fields", "field_lvalues_match_original_interpreter") else {
        return;
    };
    assert_eq!(rows.len(), 27, "empty or partial field vectors");
    for row in rows {
        let input = &row["input"];
        let ty = input["type"].as_str().unwrap();
        let collision = input["collision"].as_bool().unwrap();
        let write = input["write"].as_bool().unwrap();
        let is_static = input["static"].as_bool().unwrap();
        let access = input["access"].as_str().unwrap();
        let mut vm = VmHost::new();
        let mut metadata = vec![field("shared", "I", true)];
        if collision {
            metadata.push(field("second", ty, true));
        }
        metadata.extend([field("first", "I", false), field("second", ty, false)]);
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: "Fields".into(),
            base: None,
            base_id: None,
            interfaces: vec![],
            methods: vec![],
            fields: metadata,
            bodies: vec![],
        });
        let word = vm.alloc_object("Fields").unwrap();
        vm.objects[0].fields = vec![value("I", 273), value(ty, 546)];
        vm.write_field(word, "shared", value("I", 819)).unwrap();
        if collision {
            vm.write_field(word, "second", value(ty, 1092)).unwrap();
        }
        let name = if is_static { "shared" } else { "second" };
        let pool = vec![
            P::Str("Fields".into()),
            P::Class(0),
            P::Str(name.into()),
            P::Str(if is_static { "I" } else { ty }.into()),
            P::Type7 { a: 2, b: 3 },
            P::Type5 { a: 1, b: 4 },
        ];
        let mut nodes = vec![node(
            match access {
                "index" => 0x101b,
                "implicit" => 0x1d,
                "member" => 0x401c,
                _ => panic!(),
            },
            if access == "index" { 5 } else { 2 },
        )];
        if write {
            nodes.push(node(0x1008, 0x23));
        }
        nodes.push(node(0x10, 0));
        let body = vm.add_body("Fields", nodes, pool).unwrap();
        let mut thread = enter(&vm, body, if is_static { 0 } else { word });
        if write {
            thread
                .push(Slot::Value(if ty == "L;" {
                    value("L;", 0)
                } else {
                    Value::int(2748)
                }))
                .unwrap();
        }
        if access == "member" {
            let receiver = value("L;", word);
            let slot = if input["receiver_kind"] == "var" {
                Slot::Var(thread.new_var(receiver, 0))
            } else {
                Slot::Value(receiver)
            };
            thread.push(slot).unwrap();
        }
        thread.live = 0;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let instance: Vec<_> = vm.objects[0].fields.iter().copied().map(jvalue).collect();
        let context = vm.class_context("Fields").unwrap();
        let statics: Vec<_> = (0..if collision { 2 } else { 1 })
            .map(|slot| jvalue(vm.read_cell(HeapSlot::Static { context, slot }).unwrap()))
            .collect();
        let result = json!({"stack":stack(&mut vm,&thread,word),"instance":instance,"static":statics,"live":thread.live});
        assert_eq!(result, row["result"], "field row {}", row["id"]);
    }
}
#[test]
fn array_lvalues_match_original_interpreter() {
    let Some(rows) = rows("arrays", "array_lvalues_match_original_interpreter") else {
        return;
    };
    assert_eq!(rows.len(), 12, "empty or partial array vectors");
    for row in rows {
        let input = &row["input"];
        let ty = input["type"].as_str().unwrap();
        let write = input["write"].as_bool().unwrap();
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: "Fields".into(),
            base: None,
            base_id: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        let word = vm.alloc_array(ty, 3).unwrap();
        for (index, w) in [273, 546, 819].into_iter().enumerate() {
            vm.write_cell(HeapSlot::Array { word, index }, value(ty, w))
                .unwrap();
        }
        let mut nodes = vec![node(0x20, 0)];
        if write {
            nodes.push(node(0x1008, 0x23));
        }
        nodes.push(node(0x10, 0));
        let body = vm.add_body("Fields", nodes, vec![]).unwrap();
        let mut thread = enter(&vm, body, 0);
        if write {
            thread
                .push(Slot::Value(if ty == "L;" {
                    value("L;", 0)
                } else {
                    Value::int(2748)
                }))
                .unwrap();
        }
        thread
            .push(Slot::Value(value(
                input["index_type"].as_str().unwrap(),
                input["index"].as_i64().unwrap() as u32,
            )))
            .unwrap();
        let receiver = value(
            input["tag"].as_str().unwrap(),
            if input["null"].as_bool().unwrap() {
                0
            } else {
                word
            },
        );
        let slot = if input["receiver_kind"] == "var" {
            Slot::Var(thread.new_var(receiver, 0))
        } else {
            Slot::Value(receiver)
        };
        thread.push(slot).unwrap();
        thread.live = 0;
        let outcome = match thread.run(&mut vm, 0., &mut || X87::from_f64(0.)) {
            Ok(_) => "ok",
            Err(VmError::ArrayAccess(error)) => match error {
                ArrayError::IndexType => "index_type",
                ArrayError::NegativeIndex => "negative",
                ArrayError::NotArray => "not_array",
                ArrayError::NullArray => "null_array",
            },
            Err(error) => panic!("unexpected {error:?}"),
        };
        let elements: Vec<_> = (0..3)
            .map(|index| jvalue(vm.read_cell(HeapSlot::Array { word, index }).unwrap()))
            .collect();
        let result = json!({"outcome":outcome,"stack":stack(&mut vm,&thread,word),"elements":elements,"live":thread.live});
        assert_eq!(result, row["result"], "array row {}", row["id"]);
    }
}

fn test_class(name: &str, base: Option<&str>, fields: Vec<Field>) -> Class {
    Class {
        kind: 0,
        name: name.into(),
        base: base.map(str::to_owned),
        base_id: None,
        interfaces: vec![],
        methods: vec![],
        bodies: vec![],
        fields,
    }
}
#[test]
fn reference_field_stores_match_original_conversion_and_assignment() {
    use slr_vm::values::loader_convert;
    let Some(rows) = rows(
        "reference_store",
        "reference_field_stores_match_original_conversion_and_assignment",
    ) else {
        return;
    };
    assert_eq!(rows.len(), 7, "empty or partial conversion vectors");
    for row in rows {
        let input = &row["input"];
        let mut vm = VmHost::new();
        for (name, base) in [("Base", None), ("Child", Some("Base")), ("Other", None)] {
            vm.loader.insert_test_class(test_class(name, base, vec![]));
        }
        let source = format!("L{};", input["source"].as_str().unwrap());
        let target = format!("L{};", input["target"].as_str().unwrap());
        let mut metadata = field("target", &target, false);
        metadata.class = "Holder".into();
        vm.loader
            .insert_test_class(test_class("Holder", None, vec![metadata]));
        let holder = vm.alloc_object("Holder").unwrap();
        let object = vm.alloc_object(input["actual"].as_str().unwrap()).unwrap();
        let payload = if input["null"] == true { 0 } else { object };
        let mut converted = value(&source, payload);
        let success = loader_convert(&mut vm, &mut converted, &target).is_ok();
        let body = vm
            .add_body(
                "Holder",
                vec![node(0x1d, 0), node(0x1007, 0x23), node(0x10, 0)],
                vec![P::Str("target".into())],
            )
            .unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        thread.frames.push(Frame::new(pc, holder));
        thread.pc = pc;
        thread.push(Slot::Value(value(&source, payload))).unwrap();
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let Slot::Value(source_after) = thread.frames[0].stack[0] else {
            panic!("assignment must return its source value")
        };
        let stored = vm.objects[(holder - 0x1000) as usize].fields[0];
        let actual = json!({"converted":success,"converted_type":converted.tag.name(),"converted_payload_preserved":converted.payload==payload,"stored_type":stored.tag.name(),"source_type":source_after.tag.name(),"stored_payload_preserved":stored.payload==payload,"diagnostics":thread.alu.diagnostics.len()});
        assert_eq!(actual, row["result"], "row {}", row["id"]);
    }
}
#[test]
fn indexed_static_keeps_the_encoded_reference_class_like_original() {
    let Some(rows) = rows(
        "inherited_static",
        "indexed_static_keeps_the_encoded_reference_class_like_original",
    ) else {
        return;
    };
    assert_eq!(rows.len(), 2, "empty or partial inherited static vectors");
    for row in rows {
        let mut vm = VmHost::new();
        for (name, base, field_name) in [
            ("Base", None, "baseStatic"),
            ("Leaf", Some("Base"), "leafStatic"),
        ] {
            let mut f = field(field_name, "I", true);
            f.class = name.into();
            vm.loader.insert_test_class(test_class(name, base, vec![f]));
        }
        for (name, field_name, value) in [("Base", "baseStatic", 11), ("Leaf", "leafStatic", 22)] {
            let obj = vm.alloc_object(name).unwrap();
            vm.write_field(obj, field_name, Value::int(value)).unwrap();
        }
        vm.loader
            .insert_test_class(test_class("Code", None, vec![]));
        let reference = row["input"]["refclass"].as_str().unwrap();
        let body = vm
            .add_body(
                "Code",
                vec![node(0x101b, 5), node(0x10, 0)],
                vec![
                    P::Str(reference.into()),
                    P::Class(0),
                    P::Str("baseStatic".into()),
                    P::Str("I".into()),
                    P::Type7 { a: 2, b: 3 },
                    P::Type5 { a: 1, b: 4 },
                ],
            )
            .unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        thread.frames.push(Frame::new(pc, 0));
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let Slot::Heap(HeapSlot::Static { context, slot }) = thread.frames[0].stack[0] else {
            panic!("expected static lvalue")
        };
        let actual = json!({"value":jvalue(vm.read_cell(HeapSlot::Static{context,slot}).unwrap()),"identity":vm.loader.context_name(context).unwrap()});
        assert_eq!(
            actual,
            json!({"value":row["result"]["value"],"identity":row["result"]["identity"]}),
            "row {}",
            row["id"]
        );
    }
}
#[test]
fn typed_declarations_match_original_interpreter() {
    let Some(rows) = rows(
        "declarations",
        "typed_declarations_match_original_interpreter",
    ) else {
        return;
    };
    assert_eq!(rows.len(), 8, "empty or partial declaration vectors");
    for row in rows {
        let input = &row["input"];
        let ty = input["type"].as_str().unwrap();
        let named = input["named"] == true;
        let push = input["push"] == true;
        let mut vm = VmHost::new();
        vm.loader
            .insert_test_class(test_class("Code", None, vec![]));
        let mut nodes = vec![node(0x1002, 0)];
        if named {
            nodes.push(node(0x402e, 1));
        }
        nodes.push(node(0x1006, 0));
        if push {
            nodes.push(node(0x2c, 0));
        }
        nodes.push(node(0x10, 0));
        let body = vm
            .add_body(
                "Code",
                nodes,
                vec![P::Str(ty.into()), P::Str("named".into())],
            )
            .unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        thread.frames.push(Frame::new(pc, 0));
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let id = thread.frames[0].locals[0];
        let local = thread.vars.get(id);
        let actual = json!({"local":jvalue(local.value),"name":vm.variable_name(local.name),"stack_is_local":thread.frames[0].stack==if push{vec![Slot::Var(id)]}else{vec![]},"live":thread.live});
        assert_eq!(actual, row["result"], "row {}", row["id"]);
    }
}

#[test]
fn inherited_instance_storage_matches_original_constructor_layout() {
    let Some(rows) = rows(
        "layout",
        "inherited_instance_storage_matches_original_constructor_layout",
    ) else {
        return;
    };
    assert_eq!(rows.len(), 2, "empty or partial layout vectors");
    for row in rows {
        let mut vm = VmHost::new();
        for (name, base, key) in [("Base", None, "base"), ("Leaf", Some("Base"), "leaf")] {
            let mut fields = vec![];
            for spec in row["input"][key].as_array().unwrap() {
                let mut f = field(spec[0].as_str().unwrap(), spec[1].as_str().unwrap(), false);
                f.class = name.into();
                fields.push(f);
            }
            let mut stat = field("excludedStatic", "I", true);
            stat.class = name.into();
            fields.push(stat);
            vm.loader.insert_test_class(test_class(name, base, fields));
        }
        let word = vm.alloc_object("Leaf").unwrap();
        let layout = vm.instance_fields("Leaf").unwrap();
        let observed:Vec<_>=layout.iter().zip(&vm.objects[(word-0x1000) as usize].fields).map(|(field,value)|json!({"name":field.name,"type":value.tag.name(),"word":value.payload})).collect();
        assert_eq!(
            observed,
            row["result"]["fields"].as_array().unwrap().to_vec()
        );
        let names: Vec<_> = layout.iter().map(|f| P::Str(f.name.clone())).collect();
        let mut nodes: Vec<_> = (0..names.len())
            .map(|index| node(0x1d, index as u32))
            .collect();
        nodes.push(node(0x10, 0));
        let body = vm.add_body("Leaf", nodes, names).unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        thread.frames.push(Frame::new(pc, word));
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        assert_eq!(
            thread.frames[0].stack,
            (0..observed.len())
                .map(|slot| Slot::Heap(HeapSlot::Object { word, slot }))
                .collect::<Vec<_>>()
        );
        // Original global object-allocation counter is retained in the receipt;
        // Thread.live counts interpreter temporaries, not the host's heap owner.
        assert_eq!(thread.live, 0);
    }
}
