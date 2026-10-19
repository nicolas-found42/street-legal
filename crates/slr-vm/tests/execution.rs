//! Original VM execution behavior at the public Thread/VmHost boundary.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::{Node, PoolEntry};
use slr_vm::host::VmHost;
use slr_vm::interp::{Frame, Pc, ScriptMethod, Slot, Thread};
use slr_vm::values::{TypeTag, Value};
use slr_vm::vmclass::{Class, Method};
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
fn rows(group: &str, test: &str) -> Option<Vec<Json>> {
    let dir = std::env::var_os("SLR_VMEXEC_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/vmexec")
        });
    let Ok(text) = std::fs::read_to_string(dir.join(format!("vmexec_{group}.jsonl"))) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return None;
    };
    Some(
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}
#[test]
fn non_member_resource_literal_returns_integer_zero_like_original() {
    let Some(records) = rows(
        "resource_invalid",
        "non_member_resource_literal_returns_integer_zero_like_original",
    ) else {
        return;
    };
    assert_eq!(
        records.len(),
        1,
        "empty or partial original literal vectors"
    );
    for row in records {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        let body = vm
            .add_body(
                "Code",
                vec![node(0x1007, 0x1c), node(0x100e, 1), node(0x10, 0)],
                vec![PoolEntry::Str("unused".into()), PoolEntry::Int(27)],
            )
            .unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        thread.frames.push(frame);
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let actual: Vec<_> = thread.frames[0]
            .stack
            .iter()
            .map(|slot| {
                let Slot::Value(value) = slot else {
                    panic!("literal did not produce a value")
                };
                json!({"type":value.tag.name(),"payload":value.payload})
            })
            .collect();
        assert_eq!(actual, row["result"]["stack"].as_array().unwrap().to_vec());
        assert_eq!(thread.frames[0].stack, vec![Slot::Value(Value::int(0))]);
    }
}

thread_local! {static RESOURCE_CALLS:std::cell::RefCell<Vec<(Option<String>,u32)>>=const {std::cell::RefCell::new(vec![])};}
fn controlled_resource_resolver(
    _vm: &mut VmHost,
    package: Option<&str>,
    id: u32,
) -> Result<u32, slr_vm::interp::VmError> {
    RESOURCE_CALLS.with(|calls| calls.borrow_mut().push((package.map(str::to_owned), id)));
    Ok(0x1234_0000 | id)
}
#[test]
fn resource_members_route_package_and_raw_id_to_the_resource_world_like_original() {
    let Some(records) = rows(
        "resource",
        "resource_members_route_package_and_raw_id_to_the_resource_world_like_original",
    ) else {
        return;
    };
    assert_eq!(
        records.len(),
        2,
        "empty or partial original resource vectors"
    );
    for row in records {
        RESOURCE_CALLS.with(|calls| calls.borrow_mut().clear());
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        vm.resource_resolver = Some(controlled_resource_resolver);
        let package = if row["input"]["package"] == false {
            u32::MAX
        } else {
            0
        };
        let body = vm
            .add_body(
                "Code",
                vec![node(0x1007, 0x1c), node(0x100e, 1), node(0x10, 0)],
                vec![
                    PoolEntry::Str("system.rpk".into()),
                    PoolEntry::Member {
                        name: package,
                        extra: 27,
                    },
                ],
            )
            .unwrap();
        let mut thread = Thread::new();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        thread.frames.push(frame);
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let actual: Vec<_> = thread.frames[0]
            .stack
            .iter()
            .map(|slot| {
                let Slot::Value(value) = slot else {
                    panic!("resource literal did not produce a value")
                };
                json!({"type":value.tag.name(),"payload":value.payload})
            })
            .collect();
        assert_eq!(actual, row["result"]["stack"].as_array().unwrap().to_vec());
        let calls = RESOURCE_CALLS.with(|calls| {
            calls
                .borrow()
                .iter()
                .map(|(package, id)| json!({"package":package,"local_id":id}))
                .collect::<Vec<_>>()
        });
        let expected = row["result"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| json!({"package":call["package"],"local_id":call["local_id"]}))
            .collect::<Vec<_>>();
        assert_eq!(calls, expected);
    }
}

#[test]
fn script_frames_preserve_defining_owner_runtime_receiver_and_reverse_arguments_like_original() {
    let Some(records) = rows("frames", "script_frames_preserve_defining_owner_runtime_receiver_and_reverse_arguments_like_original") else { return };
    assert_eq!(records.len(), 4, "empty or partial original frame vectors");
    for row in records {
        let is_static = row["input"]["static"].as_bool().unwrap();
        let arguments = row["input"]["arguments"].as_bool().unwrap();
        let mut vm = VmHost::new();
        for class in [
            empty_class("Caller", None),
            empty_class("Base", None),
            empty_class("Leaf", Some("Base")),
        ] {
            vm.loader.insert_test_class(class);
        }
        let caller = vm
            .add_body("Caller", vec![node(0x24, 0), node(0x10, 0)], vec![])
            .unwrap();
        let callee = vm
            .add_body(
                "Base",
                vec![node(0x1007, 0x1c), node(0x100b, 77), node(0x2b, 0)],
                vec![],
            )
            .unwrap();
        let word = vm.alloc_object("Leaf").unwrap();
        let receiver = if is_static { 0 } else { word };
        let saved = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut thread = Thread::new();
        thread.frames.push(Frame::new(saved, 0));
        thread.pc = saved;
        if arguments {
            thread.push_new(Value::int(11)).unwrap();
            thread.push_new(Value::float(2.5)).unwrap();
        }
        let method = ScriptMethod {
            body: callee,
            class_ctx: vm.class_context("Base").unwrap(),
            signature: 0,
            is_static,
            params: if arguments {
                vec![TypeTag::Int, TypeTag::Float]
            } else {
                vec![]
            },
        };
        thread
            .call_script_method_with(&mut vm, &method, receiver)
            .unwrap();
        let frame = thread.frames.last().unwrap();
        let locals=frame.locals.iter().map(|id| {let value=thread.vars.get(*id).value;json!({"type":value.tag.name(),"payload":if !is_static && value.payload==word {json!("receiver")} else {json!(value.payload)}})}).collect::<Vec<_>>();
        let before = json!({"return_code":0,"new_frame":thread.frames.len()==2,"defining_class_is_base":frame.class_ctx==vm.class_context("Base").unwrap(),"frame_receiver_matches":frame.receiver==receiver,"caller_resume_saved":thread.frames[0].saved_pc==saved,"locals":locals});
        assert_eq!(before, row["result"]["before_run"]);
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let stack = thread.frames[0]
            .stack
            .iter()
            .map(|slot| {
                let Slot::Value(value) = slot else {
                    panic!("return lvalue")
                };
                json!({"type":value.tag.name(),"payload":value.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!({"back_to_caller":thread.frames.len()==1,"stack":stack}),
            row["result"]["after_run"]
        );
        assert_eq!(
            thread.live, 1,
            "callee parameters must be released on return"
        );
    }
}

#[test]
fn loaded_stock_static_native_methods_retain_the_native_dispatch_bit() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../stock/system/Scripts/lang/System.class");
    let Ok(data) = std::fs::read(path) else {
        eprintln!(
            "SLR_ORACLE_SKIPPED: loaded_stock_static_native_methods_retain_the_native_dispatch_bit"
        );
        return;
    };
    let mut vm = VmHost::new();
    vm.loader.load_file(&data).unwrap();
    let (method, _) = vm
        .loader
        .resolve_method("java.lang.System", "currentTime", "()F")
        .unwrap();
    assert!(method.is_native, "0x49 is both static and native");
    assert!(method.static_vector);
    assert_eq!(method.flags, 0x49);
}

#[test]
fn loaded_stock_interface_uses_the_clss_parent_record_instead_of_a_second_string() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../stock/system/Scripts/lang/Runnable.class");
    let Ok(data) = std::fs::read(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: loaded_stock_interface_uses_the_clss_parent_record_instead_of_a_second_string");
        return;
    };
    let mut vm = VmHost::new();
    vm.loader.load_file(&data).unwrap();
    assert_eq!(
        vm.loader.find("java.lang.Runnable").unwrap().base,
        None,
        "CLSS parent UINT_MAX means no parent; the next string is a method name"
    );
}

#[test]
fn inline_method_references_execute_inherited_script_bodies_like_original() {
    let Some(records) = rows(
        "calls",
        "inline_method_references_execute_inherited_script_bodies_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 4, "empty or partial original call vectors");
    for row in records {
        let static_call = row["input"]["static"].as_bool().unwrap();
        let arguments = row["input"]["arguments"].as_bool().unwrap();
        let sig = if arguments { "(IF)I" } else { "()I" };
        let mut vm = VmHost::new();
        let mut base = empty_class("Base", None);
        base.methods.push(Method {
            class: "Base".into(),
            name: "method".into(),
            sig: sig.into(),
            flags: if static_call { 9 } else { 1 },
            static_vector: static_call,
            is_native: false,
            is_abstract: false,
            body: 0,
            extra: 0,
        });
        vm.loader.insert_test_class(base);
        vm.loader
            .insert_test_class(empty_class("Leaf", Some("Base")));
        vm.add_body(
            "Base",
            vec![node(0x1007, 0x1c), node(0x100b, 77), node(0x2b, 0)],
            vec![],
        )
        .unwrap();
        let pool = vec![
            PoolEntry::Str("Base".into()),
            PoolEntry::Class(0),
            PoolEntry::Str("method".into()),
            PoolEntry::Str(sig.into()),
            PoolEntry::Type7 { a: 2, b: 3 },
            PoolEntry::Type5 { a: 1, b: 4 },
        ];
        let caller = vm
            .add_body(
                "Leaf",
                vec![
                    node(0x24, 0),
                    node(0x1011, 1),
                    node(0x101a, 5),
                    node(0x10, 0),
                ],
                pool,
            )
            .unwrap();
        let word = vm.alloc_object("Leaf").unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(pc, if static_call { 0 } else { word });
        frame.class_ctx = vm.class_context("Leaf").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        if arguments {
            thread.push_new(Value::int(11)).unwrap();
            thread.push_new(Value::float(2.5)).unwrap();
        }
        thread
            .push_new(Value::int(if arguments { 2 } else { 0 }))
            .unwrap();
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let stack = thread.frames[0]
            .stack
            .iter()
            .map(|slot| {
                let Slot::Value(v) = slot else {
                    panic!("return lvalue")
                };
                json!({"type":v.tag.name(),"payload":v.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!({"back_to_caller":thread.frames.len()==1,"stack":stack}),
            row["result"]
        );
        assert_eq!(thread.live, 1);
    }
}

thread_local! {static NATIVE_CALL:std::cell::RefCell<Option<Json>>=const {std::cell::RefCell::new(None)};}
fn controlled_native(_vm: &mut VmHost, info: &slr_vm::host::CallInfo<'_>) -> Option<u32> {
    NATIVE_CALL.with(|call| *call.borrow_mut()=Some(json!({"class":info.class,"name":info.name,"signature":info.sig,"cells":info.args.iter().map(|v|json!({"type":v.tag.name(),"payload":v.payload})).collect::<Vec<_>>()})));
    Some(if info.sig.ends_with('F') {
        2.5f32.to_bits()
    } else {
        0x11223344
    })
}
#[test]
fn native_method_calls_preserve_defining_receiver_argument_order_and_return_cells_like_original() {
    let Some(records)=rows("native","native_method_calls_preserve_defining_receiver_argument_order_and_return_cells_like_original") else {return};
    assert_eq!(records.len(), 8, "empty or partial original native vectors");
    for row in records {
        let static_call = row["input"]["static"].as_bool().unwrap();
        let ret = row["input"]["ret"].as_str().unwrap();
        let sig = format!("(IF){ret}");
        let mut vm = VmHost::new();
        let mut base = empty_class("Base", None);
        base.methods.push(Method {
            class: "Base".into(),
            name: "method".into(),
            sig: sig.clone(),
            flags: if static_call { 0x49 } else { 0x41 },
            static_vector: static_call,
            is_native: true,
            is_abstract: false,
            body: u32::MAX,
            extra: 0,
        });
        vm.loader.insert_test_class(base);
        vm.loader
            .insert_test_class(empty_class("Leaf", Some("Base")));
        vm.natives.register("Base", "method", &sig, 0x41b860);
        vm.native_impls.insert(0x41b860, controlled_native);
        let pool = vec![
            PoolEntry::Str("Base".into()),
            PoolEntry::Class(0),
            PoolEntry::Str("method".into()),
            PoolEntry::Str(sig),
            PoolEntry::Type7 { a: 2, b: 3 },
            PoolEntry::Type5 { a: 1, b: 4 },
        ];
        let caller = vm
            .add_body(
                "Leaf",
                vec![
                    node(0x24, 0),
                    node(0x1011, 1),
                    node(0x101a, 5),
                    node(0x10, 0),
                ],
                pool,
            )
            .unwrap();
        let word = vm.alloc_object("Leaf").unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(pc, if static_call { 0 } else { word });
        frame.class_ctx = vm.class_context("Leaf").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        thread.push_new(Value::int(11)).unwrap();
        thread.push_new(Value::float(2.5)).unwrap();
        thread.push_new(Value::int(2)).unwrap();
        NATIVE_CALL.with(|call| *call.borrow_mut() = None);
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let mut actual =
            NATIVE_CALL.with(|call| call.borrow().clone().expect("native not dispatched"));
        assert_eq!(actual["class"], "Base");
        assert_eq!(actual["name"], "method");
        if !static_call {
            let cells = actual["cells"].as_array_mut().unwrap();
            let receiver = cells.last_mut().unwrap();
            assert_eq!(receiver["payload"], word);
            receiver["payload"] = json!("receiver");
        }
        let expected = &row["result"]["events"][0]["native_callback"];
        assert_eq!(actual["signature"], expected["signature"]);
        assert_eq!(actual["cells"], expected["cells"]);
        let stack = thread.frames[0]
            .stack
            .iter()
            .map(|slot| {
                let Slot::Value(v) = slot else {
                    panic!("native return lvalue")
                };
                json!({"type":v.tag.name(),"payload":v.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(stack), row["result"]["after_stack"]);
        assert_eq!(thread.live, if ret.is_empty() { 0 } else { 1 });
    }
}

fn declared_parameters(sig: &str) -> Vec<TypeTag> {
    let mut text = sig.strip_prefix('(').unwrap();
    let mut tags = vec![];
    while !text.starts_with(')') {
        let length = if text.starts_with('L') {
            text.find(';').unwrap() + 1
        } else {
            1
        };
        tags.push(TypeTag::intern(&text[..length]));
        text = &text[length..];
    }
    tags
}
fn overload_world() -> VmHost {
    let mut vm = VmHost::new();
    for (name, kind, base, interfaces) in [
        ("Object", 0, None, vec![]),
        ("Base", 0, Some("Object"), vec![]),
        ("Child", 0, Some("Base"), vec![]),
        ("Other", 0, Some("Object"), vec![]),
        ("I0", 1, None, vec![]),
        ("I1", 1, None, vec!["I0"]),
        ("I2", 1, None, vec!["I1"]),
        ("IX", 1, None, vec![]),
        ("Implementer", 0, Some("Base"), vec!["I1"]),
        ("LeafImpl", 0, Some("Implementer"), vec![]),
        ("java.lang.String", 0, Some("Object"), vec![]),
        ("[I", 0, Some("Object"), vec![]),
    ] {
        let mut class = empty_class(name, base);
        class.kind = kind;
        class.interfaces = interfaces.into_iter().map(str::to_owned).collect();
        vm.loader.insert_test_class(class);
    }
    vm
}
#[test]
fn dynamic_method_names_choose_the_same_overload_as_the_original_resolver() {
    let Some(records) = rows(
        "overload",
        "dynamic_method_names_choose_the_same_overload_as_the_original_resolver",
    ) else {
        return;
    };
    assert_eq!(
        records.len(),
        28,
        "empty or partial original overload vectors"
    );
    for row in records {
        let mut vm = overload_world();
        let input = &row["input"];
        let method_name = input["name"].as_str().unwrap_or("m");
        for (owner, base, static_key, instance_key, prefix) in [
            ("Parent", None, "parent_static", "parent_instance", 2000),
            ("Owner", Some("Parent"), "static", "instance", 1000),
        ] {
            let mut class = empty_class(owner, base);
            for (key, static_vector) in [(static_key, true), (instance_key, false)] {
                for (index, spec) in input[key].as_array().into_iter().flatten().enumerate() {
                    let (name, sig) = if let Some(sig) = spec.as_str() {
                        ("m", sig)
                    } else {
                        (spec[0].as_str().unwrap(), spec[1].as_str().unwrap())
                    };
                    let body = class.bodies.len() as u32;
                    class.methods.push(Method {
                        class: owner.into(),
                        name: name.into(),
                        sig: sig.into(),
                        flags: if static_vector { 9 } else { 1 },
                        static_vector,
                        is_native: false,
                        is_abstract: false,
                        body,
                        extra: 0,
                    });
                    class.bodies.push(vec![
                        node(0x1007, 0x1c),
                        node(
                            0x100b,
                            prefix + if static_vector { 100 } else { 0 } + index as u32,
                        ),
                        node(0x2b, 0),
                    ]);
                }
            }
            vm.loader.insert_test_class(class);
        }
        let caller = vm
            .add_body(
                "Owner",
                vec![
                    node(0x24, 0),
                    node(0x1011, 1),
                    node(0x4012, 0),
                    node(0x10, 0),
                ],
                vec![PoolEntry::Str(method_name.into())],
            )
            .unwrap();
        let word = vm.alloc_object("Owner").unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(pc, word);
        frame.class_ctx = vm.class_context("Owner").unwrap();
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        let parameters = declared_parameters(input["actual"].as_str().unwrap());
        let matched = vm
            .loader
            .resolve_overload("Owner", method_name, &parameters)
            .unwrap();
        let original = &row["result"];
        if let Some(matched) = matched {
            assert_eq!(
                format!("0x{:x}", matched.index),
                original["index_hex"],
                "case{}",
                row["id"]
            );
            assert_eq!(matched.defining, original["selected"][0]);
            assert_eq!(matched.method.sig, original["selected"][3]);
            let ambiguity = original["diagnostic_fragments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|text| {
                    text.as_str()
                        .unwrap()
                        .contains("more matching methods found")
                });
            assert_eq!(matched.ambiguous, ambiguity, "case{}", row["id"]);
        } else {
            assert!(original["selected"].is_null());
        }
        for tag in &parameters {
            thread
                .push_new(Value {
                    tag: *tag,
                    payload: if *tag == TypeTag::Int {
                        11
                    } else if *tag == TypeTag::Float {
                        2.5f32.to_bits()
                    } else {
                        0
                    },
                })
                .unwrap();
        }
        thread
            .push_new(Value::int(parameters.len() as i32))
            .unwrap();
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        let selected = &row["result"]["selected"];
        if selected.is_null() {
            assert!(outcome.is_err(), "case{} unexpectedly selected", row["id"]);
            continue;
        }
        outcome.unwrap_or_else(|error| panic!("case{}: {error:?}", row["id"]));
        let expected = if selected[0] == "Owner" { 1000 } else { 2000 }
            + if selected[1] == "static" { 100 } else { 0 }
            + selected[2].as_u64().unwrap() as u32;
        let value = thread.pop_deref().unwrap();
        assert_eq!(value, Value::int(expected as i32), "case{}", row["id"]);
    }
}

#[test]
fn dynamic_overloads_use_runtime_classes_and_preserve_null_argument_types_like_original() {
    let Some(records) = rows(
        "runtime",
        "dynamic_overloads_use_runtime_classes_and_preserve_null_argument_types_like_original",
    ) else {
        return;
    };
    assert_eq!(
        records.len(),
        8,
        "empty or partial original runtime vectors"
    );
    for row in records {
        let input = &row["input"];
        let static_call = input["static"].as_bool().unwrap_or(false);
        let missing_class = input["no_class"].as_bool().unwrap_or(false);
        let null = input["null"].as_bool().unwrap_or(false);
        let mut vm = overload_world();
        vm.loader.insert_test_class(empty_class("Parent", None));
        let mut owner = empty_class("Owner", Some("Parent"));
        for (index, sig) in ["(LBase;)I", "(LChild;)I"].into_iter().enumerate() {
            owner.methods.push(Method {
                class: "Owner".into(),
                name: "m".into(),
                sig: sig.into(),
                flags: if static_call { 9 } else { 1 },
                static_vector: static_call,
                is_native: false,
                is_abstract: false,
                body: index as u32,
                extra: 0,
            });
            owner
                .bodies
                .push(vec![node(0x1027, 77 + index as u32), node(0x2b, 0)]);
        }
        vm.loader.insert_test_class(owner);
        let caller = vm
            .add_body(
                "Owner",
                vec![
                    node(0x24, 0),
                    node(0x1011, 1),
                    node(0x4012, 0),
                    node(0x10, 0),
                ],
                vec![PoolEntry::Str(
                    input["method_name"].as_str().unwrap_or("m").into(),
                )],
            )
            .unwrap();
        let receiver = vm.alloc_object("Owner").unwrap();
        let actual = vm
            .alloc_object(input["actual"].as_str().unwrap_or("Child"))
            .unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(
            pc,
            if static_call || missing_class {
                0
            } else {
                receiver
            },
        );
        frame.class_ctx = if missing_class {
            0
        } else {
            vm.class_context("Owner").unwrap()
        };
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        thread
            .push_new(Value {
                tag: TypeTag::intern(input["declared"].as_str().unwrap_or("LBase;")),
                payload: if null { 0 } else { actual },
            })
            .unwrap();
        thread.push_new(Value::int(1)).unwrap();
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        if row["result"]["outcome"] == "fatal" {
            assert!(outcome.is_err());
            assert_eq!(
                thread.frames[0].stack.len(),
                row["result"]["remaining_arguments"].as_u64().unwrap() as usize
            );
            continue;
        }
        outcome.unwrap();
        let selected = &row["result"]["calls"][0]["selected"];
        let returned = thread.pop_deref().unwrap();
        assert_eq!(
            returned,
            Value::int(77 + selected[2].as_i64().unwrap() as i32)
        );
        let actual_index = usize::try_from(returned.as_i32() - 77).unwrap();
        let actual_method = &vm.loader.find("Owner").unwrap().methods[actual_index];
        assert_eq!(
            json!([
                actual_method.class,
                if actual_method.static_vector {
                    "static"
                } else {
                    "instance"
                },
                actual_index,
                actual_method.sig
            ]),
            *selected
        );
        let ambiguity = row["result"]["diagnostic_fragments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|text| {
                text.as_str()
                    .unwrap()
                    .contains("more matching methods found")
            });
        assert_eq!(
            !vm.method_diagnostics.is_empty(),
            ambiguity,
            "case{}",
            row["id"]
        );
    }
}

#[test]
fn new_expressions_allocate_the_requested_loaded_class_like_original() {
    let Some(records) = rows(
        "new",
        "new_expressions_allocate_the_requested_loaded_class_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 2, "empty or partial original new vectors");
    for row in records {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        vm.loader.insert_test_class(empty_class("Leaf", None));
        let body = vm
            .add_body(
                "Code",
                vec![
                    node(
                        if row["input"]["statement"] == true {
                            0x1008
                        } else {
                            0x1007
                        },
                        0x1e,
                    ),
                    node(0x1006, 0),
                    node(0x10, 0),
                ],
                vec![PoolEntry::Str("Leaf".into())],
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let value = thread.pop_deref().unwrap();
        let object = &vm.objects[(value.payload - 0x1000) as usize];
        let class_name = match &object.class {
            slr_vm::vmclass::ClassRef::Loaded(id) => vm.loader.class_name(*id).unwrap().to_owned(),
            slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
        };
        assert_eq!(
            class_name,
            row["result"]["events"][0]["name"].as_str().unwrap()
        );
        assert_eq!(
            json!([{"type":value.tag.name(),"is_supplied_leaf_instance":class_name=="Leaf"}]),
            row["result"]["stack"]
        );
    }
}

#[test]
fn constructor_opcodes_route_runtime_this_and_super_receivers_like_original() {
    use slr_vm::vmclass::Field;
    let Some(records) = rows(
        "constructors",
        "constructor_opcodes_route_runtime_this_and_super_receivers_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 4);
    for row in records {
        let mut vm = VmHost::new();
        for (name, base, marker) in [("Base", None, 202), ("Leaf", Some("Base"), 101)] {
            let mut class = empty_class(name, base);
            if name == "Base" {
                class.fields.push(Field {
                    class: name.into(),
                    name: "marker".into(),
                    ty: "I".into(),
                    is_static: false,
                });
            }
            class.methods.push(Method {
                class: name.into(),
                name: "<init>".into(),
                sig: "()".into(),
                flags: 1,
                static_vector: false,
                is_native: false,
                is_abstract: false,
                body: 0,
                extra: 0,
            });
            vm.loader.insert_test_class(class);
            vm.add_body(
                name,
                vec![
                    node(0x1027, marker),
                    node(0x001d, 0),
                    node(0x1008, 0x23),
                    node(0x2a, 0),
                ],
                vec![PoolEntry::Str("marker".into())],
            )
            .unwrap();
        }
        let opcode = row["input"]["op"].as_u64().unwrap() as u16;
        let missing = row["input"]["missing_super"].as_bool().unwrap_or(false);
        let caller = vm
            .add_body(
                "Leaf",
                vec![node(opcode, 0), node(0x10, 0)],
                vec![PoolEntry::Str("marker".into())],
            )
            .unwrap();
        let word = vm.alloc_object("Leaf").unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(pc, word);
        frame.class_ctx = vm
            .class_context(if missing { "Base" } else { "Leaf" })
            .unwrap();
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        if opcode == 0x21 {
            thread
                .push_new(Value::reference_named(word, "LBase;"))
                .unwrap();
        }
        thread.push_new(Value::int(0)).unwrap();
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let calls = row["result"]["dispatch"].as_array().unwrap();
        let expected = if calls.is_empty() {
            0
        } else {
            assert_eq!(calls[0]["name"], "<init>");
            assert_eq!(calls[0]["receiver_matches"], true);
            if calls[0]["class"] == "Base" {
                202
            } else {
                101
            }
        };
        assert_eq!(
            vm.objects[(word - 0x1000) as usize].fields[0],
            Value::int(expected)
        );
        assert_eq!(
            thread.frames[0].stack.len(),
            usize::from(opcode == 0x21 || missing)
        );
    }
}

#[test]
fn named_instance_and_super_method_opcodes_route_and_reject_null_like_original() {
    let Some(records) = rows(
        "named_methods",
        "named_instance_and_super_method_opcodes_route_and_reject_null_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 4);
    for row in records {
        let mut vm = VmHost::new();
        for (name, base, returned) in [("Base", None, 202), ("Leaf", Some("Base"), 101)] {
            let mut class = empty_class(name, base);
            class.methods.push(Method {
                class: name.into(),
                name: "method".into(),
                sig: "()I".into(),
                flags: 1,
                static_vector: false,
                is_native: false,
                is_abstract: false,
                body: 0,
                extra: 0,
            });
            vm.loader.insert_test_class(class);
            vm.add_body(
                name,
                vec![node(0x1027, returned), node(0x2b, 0)],
                vec![PoolEntry::Str("method".into())],
            )
            .unwrap();
        }
        let opcode = row["input"]["op"].as_u64().unwrap() as u16;
        let null = row["input"]["null"].as_bool().unwrap_or(false);
        let missing = row["input"]["missing_super"].as_bool().unwrap_or(false);
        let caller = vm
            .add_body(
                "Leaf",
                vec![node(opcode, 0), node(0x10, 0)],
                vec![PoolEntry::Str("method".into())],
            )
            .unwrap();
        let word = vm.alloc_object("Leaf").unwrap();
        let pc = Some(Pc {
            body: caller,
            idx: 0,
        });
        let mut frame = Frame::new(pc, if opcode == 0x4025 { 0 } else { word });
        frame.class_ctx = vm
            .class_context(if missing { "Base" } else { "Leaf" })
            .unwrap();
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        thread.push_new(Value::int(0)).unwrap();
        if opcode == 0x4025 {
            thread
                .push_new(Value::reference_named(
                    if null { 0 } else { word },
                    "LBase;",
                ))
                .unwrap();
        }
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        if row["result"]["outcome"] == "fatal" {
            assert!(outcome.is_err());
            assert_eq!(
                thread.frames[0].stack.len(),
                row["result"]["remaining_stack"].as_u64().unwrap() as usize
            );
            continue;
        }
        outcome.unwrap();
        let target = &row["result"]["dispatch"][0];
        assert_eq!(target["name"], "method");
        assert_eq!(target["receiver_matches"], true);
        assert_eq!(
            thread.pop_deref().unwrap(),
            Value::int(if target["class"] == "Base" { 202 } else { 101 })
        );
    }
}

#[test]
fn new_arrays_create_typed_persistent_element_cells_like_original() {
    use slr_vm::interp::Host;
    let Some(records) = rows(
        "arrays",
        "new_arrays_create_typed_persistent_element_cells_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 7);
    for row in records {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        let descriptor = row["input"]["desc"].as_str().unwrap();
        let pool = vec![PoolEntry::Str(descriptor.into())];
        let body = vm
            .add_body(
                "Code",
                vec![node(0x1007, 0x1f), node(0x1006, 0), node(0x10, 0)],
                pool.clone(),
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        thread
            .push_new(Value::int(row["input"]["length"].as_i64().unwrap() as i32))
            .unwrap();
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        if row["result"]["outcome"] == "fatal" {
            assert!(outcome.is_err(), "case{}", row["id"]);
            assert_eq!(
                thread.frames[0].stack.len(),
                row["result"]["remaining_stack"].as_u64().unwrap() as usize
            );
            continue;
        }
        outcome.unwrap();
        let array = thread.pop_deref().unwrap();
        assert_eq!(array.tag.name(), row["result"]["descriptor"]);
        let object = &vm.objects[(array.payload - 0x1000) as usize];
        let array_class = match &object.class {
            slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
            slr_vm::vmclass::ClassRef::Loaded(id) => vm.loader.class_name(*id).unwrap().to_owned(),
        };
        assert_eq!(array_class, descriptor);
        assert_eq!(
            object.fields[0].payload,
            row["result"]["length"].as_u64().unwrap() as u32
        );
        let item_body = vm
            .add_body("Code", vec![node(0x20, 0), node(0x10, 0)], pool)
            .unwrap();
        for (index, expected) in row["result"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let mut reader = Thread::new();
            let pc = Some(Pc {
                body: item_body,
                idx: 0,
            });
            reader.pc = pc;
            let mut frame = Frame::new(pc, 0);
            frame.class_ctx = vm.class_context("Code").unwrap();
            reader.frames.push(frame);
            reader.push_new(Value::int(index as i32)).unwrap();
            reader.push_new(array).unwrap();
            reader.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
            let slot = *reader.frames[0].stack.last().unwrap();
            let (kind, value) = if let Slot::Heap(cell) = slot {
                ("variable", vm.read_cell(cell).unwrap())
            } else {
                panic!("array item must remain an lvalue")
            };
            assert_eq!(
                json!({"kind":kind,"type":value.tag.name(),"payload":value.payload}),
                *expected
            );
        }
    }
}

#[test]
fn casts_and_instanceof_match_original_runtime_types_nulls_and_string_bytes() {
    use slr_vm::vmclass::Field;
    let Some(records) = rows(
        "casts",
        "casts_and_instanceof_match_original_runtime_types_nulls_and_string_bytes",
    ) else {
        return;
    };
    assert_eq!(records.len(), 21);
    for row in records {
        let input = &row["input"];
        let operation = input["operation"].as_str().unwrap_or("cast");
        let source = input["source"].as_str().unwrap_or("LBase;");
        let target = input["target"].as_str().unwrap_or("LBase;");
        let mut vm = VmHost::new();
        for (name, kind, base, interfaces) in [
            ("Object", 0, None, vec![]),
            ("Base", 0, Some("Object"), vec![]),
            ("Child", 0, Some("Base"), vec![]),
            ("Other", 0, Some("Object"), vec![]),
            ("I0", 1, None, vec![]),
            ("I1", 1, None, vec!["I0"]),
            ("Implementer", 0, Some("Base"), vec!["I1"]),
            ("java.lang.String", 0, Some("Object"), vec![]),
            ("StringSub", 0, Some("java.lang.String"), vec![]),
        ] {
            let mut class = empty_class(name, base);
            class.kind = kind;
            class.interfaces = interfaces.into_iter().map(str::to_owned).collect();
            if name == "java.lang.String" {
                class.fields.push(Field {
                    class: name.into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                });
            }
            vm.loader.insert_test_class(class);
        }
        vm.loader.insert_test_class(empty_class("Code", None));
        let object = vm
            .alloc_object(input["actual"].as_str().unwrap_or("Child"))
            .unwrap();
        let body = vm
            .add_body(
                "Code",
                vec![
                    node(0x1007, if operation == "cast" { 0x21 } else { 0x20 }),
                    node(0x1006, 0),
                    node(0x10, 0),
                ],
                vec![PoolEntry::Str(target.into())],
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.frames.push(frame);
        thread.pc = pc;
        let tag = TypeTag::intern(source);
        let payload = if source.starts_with(['L', '[']) {
            if input["null"] == true {
                0
            } else {
                object
            }
        } else {
            input["payload"].as_u64().unwrap_or(11) as u32
        };
        thread.push_new(Value { tag, payload }).unwrap();
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        if row["result"]["outcome"] == "fatal" {
            assert!(outcome.is_err(), "case{}", row["id"]);
            assert!(thread.frames[0].stack.is_empty());
            continue;
        }
        outcome.unwrap_or_else(|error| panic!("case{}: {error:?}", row["id"]));
        let value = thread.pop_deref().unwrap();
        let expected = &row["result"]["stack"][0];
        assert_eq!(value.tag.name(), expected["type"], "case{}", row["id"]);
        if let Some(text) = row["result"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|event| event["string"].as_str())
        {
            let string_class = match &vm.objects[(value.payload - 0x1000) as usize].class {
                slr_vm::vmclass::ClassRef::Loaded(id) => {
                    vm.loader.class_name(*id).unwrap().to_owned()
                }
                slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
            };
            assert_eq!(string_class, "java.lang.String");
            assert_eq!(
                vm.string_bytes(value.payload),
                Some(text.as_bytes()),
                "case{}",
                row["id"]
            );
        } else if expected["payload"] == "object" {
            assert_eq!(value.payload, object);
        } else {
            assert_eq!(
                json!(value.payload),
                expected["payload"],
                "case{}",
                row["id"]
            );
        }
        assert_eq!(
            thread.live, 1,
            "case{} duplicated a value temporary",
            row["id"]
        );
    }
}

#[test]
fn argument_arrays_preserve_target_lvalues_item_order_and_typed_copies_like_original() {
    use slr_vm::interp::{HeapSlot, Host};
    let Some(records) = rows(
        "arg_arrays",
        "argument_arrays_preserve_target_lvalues_item_order_and_typed_copies_like_original",
    ) else {
        return;
    };
    assert_eq!(records.len(), 8);
    for row in records {
        let input = &row["input"];
        let descriptor = input["desc"].as_str().unwrap_or("[I");
        let items = input["items"].as_array().cloned().unwrap_or_default();
        let has_target = input["target"].as_bool().unwrap_or(true);
        let variable = input["target_kind"].as_str().unwrap_or("var") == "var";
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        let body = vm
            .add_body(
                "Code",
                vec![node(0x101e, items.len() as u32), node(0x10, 0)],
                vec![],
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut thread = Thread::new();
        thread.pc = pc;
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut target_slot = None;
        if has_target {
            let value = Value {
                tag: TypeTag::intern(descriptor),
                payload: 0,
            };
            let slot = if variable {
                let id = thread.new_var(value, 0);
                frame.locals.push(id);
                Slot::Var(id)
            } else {
                Slot::Value(value)
            };
            target_slot = Some(slot);
        }
        thread.frames.push(frame);
        if let Some(slot) = target_slot {
            if let Slot::Value(value) = slot {
                thread.push_new(value).unwrap();
            } else {
                thread.push(slot).unwrap();
            }
        }
        let mut source_variables = vec![];
        for (index, item) in items.iter().enumerate() {
            let value = Value {
                tag: if item["type"] == "L" {
                    TypeTag::intern("L;")
                } else {
                    TypeTag::intern(item["type"].as_str().unwrap())
                },
                payload: item["payload"].as_u64().unwrap() as u32,
            };
            if item["kind"] == "var" {
                let id = thread.new_var(value, 0);
                thread.frames[0].locals.push(id);
                thread.push(Slot::Var(id)).unwrap();
                source_variables.push((index, id));
            } else {
                thread.push_new(value).unwrap();
            }
        }
        let outcome = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        let expected = &row["result"];
        assert_eq!(
            thread.frames[0].stack.len(),
            expected["remaining_stack"].as_u64().unwrap() as usize,
            "case{}",
            row["id"]
        );
        assert_eq!(
            target_slot.is_some() && thread.frames[0].stack.first() == target_slot.as_ref(),
            expected["target_preserved"].as_bool().unwrap()
        );
        if expected["outcome"] == "fatal" {
            assert!(outcome.is_err());
            assert!(vm.objects.is_empty());
            continue;
        }
        outcome.unwrap();
        let Slot::Value(value) = *thread.frames[0].stack.last().unwrap() else {
            panic!("array result must be a value")
        };
        assert_eq!(value.tag.name(), expected["type"]);
        assert_eq!(json!(value.payload == 0), expected["null"]);
        if value.payload != 0 {
            let array_class = match &vm.objects[(value.payload - 0x1000) as usize].class {
                slr_vm::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
                slr_vm::vmclass::ClassRef::Loaded(id) => {
                    vm.loader.class_name(*id).unwrap().to_owned()
                }
            };
            assert_eq!(array_class, descriptor);
            let cells = expected["cells"].as_array().unwrap();
            assert_eq!(
                vm.objects[(value.payload - 0x1000) as usize].fields[0].payload,
                cells.len() as u32
            );
            for (index, expected_cell) in cells.iter().enumerate() {
                let cell = vm
                    .read_cell(HeapSlot::Array {
                        word: value.payload,
                        index,
                    })
                    .unwrap();
                assert_eq!(
                    json!({"type":cell.tag.name(),"payload":cell.payload}),
                    *expected_cell,
                    "case{} element{index}",
                    row["id"]
                );
            }
        }
        let variables = source_variables
            .into_iter()
            .map(|(index, id)| {
                let value = thread.vars.get(id).value;
                json!({"index":index,"type":value.tag.name(),"payload":value.payload})
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(variables), expected["variables_after"]);
    }
}

#[test]
fn a_non_string_pool_literal_returns_the_original_typed_null() {
    let Some(records) = rows(
        "wrong_kind_literal",
        "a_non_string_pool_literal_returns_the_original_typed_null",
    ) else {
        return;
    };
    assert_eq!(
        records.len(),
        1,
        "empty or partial wrong-kind original literal vectors"
    );
    for row in records {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        let body = vm
            .add_body(
                "Code",
                vec![node(0x1007, 0x1c), node(0x4009, 1), node(0x10, 0)],
                vec![PoolEntry::Str("Code".into()), PoolEntry::Class(0)],
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let value = thread.pop_deref().unwrap();
        assert_eq!(
            json!([{"type":value.tag.name(),"payload":value.payload}]),
            row["result"]["stack"]
        );
        assert!(
            vm.objects.is_empty(),
            "null fast path requires no String allocation or metadata"
        );
    }
}

#[test]
fn malformed_literal_indices_fail_instead_of_succeeding_as_null() {
    for index in [7u32, u32::MAX] {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(empty_class("Code", None));
        let body = vm
            .add_body(
                "Code",
                vec![node(0x1007, 0x1c), node(0x4009, index), node(0x10, 0)],
                vec![PoolEntry::Str("Code".into())],
            )
            .unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        let result = thread.run(&mut vm, 0., &mut || X87::from_f64(0.));
        if index == u32::MAX {
            result.unwrap();
            assert_eq!(
                thread.pop_deref().unwrap(),
                Value::reference_named(0, "Ljava.lang.String;")
            );
        } else {
            assert!(
                result.is_err(),
                "an absent positive pool index must not be accepted as a null literal"
            );
            assert!(thread.frames[0].stack.is_empty());
        }
        assert!(vm.objects.is_empty());
    }
}
