//! ResourceRef and GameRef flag vectors from the real executable.
mod support;
use serde_json::{json, Value};
use slr_resource::node as n;

fn bool_at(a: &Value, key: &str) -> bool {
    a[key].as_bool().unwrap()
}

#[test]
fn resource_native_effects_match_oracle_vectors() {
    let dir = std::env::var_os("SLR_RESOURCE_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/res")
        });
    let mut checked = 0;
    let mut files = 0;
    let mut excluded = 0;
    for name in [
        "newNative",
        "deleteNative",
        "setI",
        "setRef",
        "load",
        "unload",
        "destroy",
        "type",
        "id",
        "getParentID",
        "getParent",
        "getWTRoot",
        "getFirstChild",
        "getNextChild",
        "makeTexture",
        "makeSound",
        "cache",
        "precache",
        "duplicate",
        "scaleMesh",
        "getFlags",
        "setFlags",
        "clearFlags",
        "setActiveCollision",
    ] {
        let Ok(text) = std::fs::read_to_string(dir.join(format!("res_resource_{name}.jsonl")))
        else {
            continue;
        };
        files += 1;
        assert!(!text.trim().is_empty(), "empty {name} vectors");
        for line in text.lines().filter(|l| !l.is_empty()) {
            let row: Value = serde_json::from_str(line).unwrap();
            let a = &row["args"];
            let binding = n::RESOURCE_REF_BINDINGS
                .iter()
                .chain(n::GAME_REF_BINDINGS)
                .find(|b| {
                    b.impl_addr
                        == u32::from_str_radix(
                            row["exe_addr"].as_str().unwrap().trim_start_matches("0x"),
                            16,
                        )
                        .unwrap()
                })
                .unwrap();
            let mut host = n::ResHost::new();
            let terminal = host.add_container(n::Container::new(0, 0));
            let target = host.add_container(n::Container::new(1, 0x55));
            host.container_mut(target).unwrap().next_sibling =
                if bool_at(a, "terminal") { 0 } else { terminal };
            let mut source = n::Container::new(
                a["mode"].as_i64().unwrap() as i32,
                a["id"].as_u64().unwrap() as u32,
            );
            source.flags = a["flags"].as_u64().unwrap() as u32;
            source.parent = if bool_at(a, "target") { target } else { 0 };
            source.wt_root = source.parent;
            source.next_sibling = if bool_at(a, "target") {
                target
            } else {
                terminal
            };
            source.first_child = if bool_at(a, "target") {
                target
            } else {
                terminal
            };
            let source = host.add_container(source);
            let node = host.add_node_for_test(
                a["id"].as_u64().unwrap() as u32,
                if bool_at(a, "bound") { source } else { 0 },
            );
            if bool_at(a, "node") {
                host.bind_object(7, node);
            }
            // Keep the source head consistent when there is no receiver node.
            if !bool_at(a, "node") {
                host.container_mut(source).unwrap().head = 0;
            }
            let other = host.add_node_for_test(0x55, 0);
            host.node_mut(other).unwrap().container = if bool_at(a, "target") { target } else { 0 };
            host.bind_id(0xbeef, if bool_at(a, "target") { target } else { 0 });
            let text_word = host.intern("paint.dds");
            host.set_gate_refused(bool_at(a, "gate"));
            host.set_ground_precache(a["precache"].as_i64().unwrap() as i32);
            host.set_created_result(if bool_at(a, "target") { target } else { 0 });
            let arg = if bool_at(a, "arg") { other } else { 0 };
            let words = match name {
                "setI" => vec![7, 0xbeef],
                "setRef" | "duplicate" => vec![7, arg],
                "makeTexture" | "makeSound" => {
                    vec![7, arg, if bool_at(a, "text") { text_word } else { 0 }]
                }
                "setFlags" | "clearFlags" => vec![7, a["mask"].as_u64().unwrap() as u32],
                "scaleMesh" => vec![7, 0x3fc00000, 0xc0200000, 0x80000000],
                _ => vec![7],
            };
            if let Some(fault) = row["result"]["fault"].as_str() {
                // ADR 0007 excludes only these eight recorded build-606
                // triggers. A different null-node fault must still fail.
                let (expected_id, prefix) = match name {
                    "getFlags" => (1, "unmapped read of 4 bytes at 0xc (eip=0x47df63,"),
                    "setFlags" => (1, "unmapped read of 4 bytes at 0xc (eip=0x486ca5,"),
                    "clearFlags" => (1, "unmapped read of 4 bytes at 0xc (eip=0x486cf5,"),
                    "getParent" => (1, "unmapped read of 4 bytes at 0x8 (eip=0x48aaf0,"),
                    "getWTRoot" => (1, "unmapped read of 4 bytes at 0x8 (eip=0x48aa70,"),
                    "getFirstChild" => (1, "unmapped read of 4 bytes at 0xc (eip=0x48d241,"),
                    "getNextChild" => (1, "unmapped read of 4 bytes at 0xc (eip=0x48d311,"),
                    "getParentID" => (7, "unmapped read of 4 bytes at 0x50 (eip=0x48ab8a,"),
                    _ => panic!("unexpected executable fault: {name} {row}"),
                };
                let parent = name == "getParentID";
                let expected_args = json!({
                    "node":parent,"bound":true,"id":9,"mode":3,
                    "flags":0x12345678,"mask":0x123,"gate":false,
                    "precache":1,"target":!parent,"arg":true,
                    "text":true,"terminal":false
                });
                assert_eq!(row["id"], expected_id, "unexpected fault scenario {name}");
                assert_eq!(*a, expected_args, "unexpected fault inputs {name}");
                assert_eq!(binding.name, name, "unexpected fault native {name}");
                assert_eq!(row["name"], name, "unexpected fault row name");
                assert_eq!(row["sig"], binding.sig, "unexpected fault descriptor");
                assert!(
                    fault.starts_with(prefix),
                    "unexpected fault location: {fault}"
                );
                // The differential comparison excludes this original crash,
                // but the port's corrected behavior is still exercised.
                let got = support::invoke(&mut host, binding, &words);
                let corrected = if binding.sig.ends_with(")V") {
                    None
                } else {
                    Some(0)
                };
                assert_eq!(got, corrected, "{name}: corrected crash behavior");
                assert!(
                    host.events().is_empty(),
                    "{name}: unexpected corrected effects"
                );
                assert_eq!(host.container(source).unwrap().flags, 0x12345678);
                support::assert_record(&host, binding, &words, got);
                excluded += 1;
                continue;
            }
            let got = support::invoke(&mut host, binding, &words);
            let w = &row["result"];
            let state = host.node(host.object_node(7));
            let mut result = json!({
                "node":state.is_some(),"id":state.map(|n|n.id),"bound":state.map(|n|n.container!=0),
                "flags":host.container(source).unwrap().flags,
                "source_head":host.container(source).unwrap().head!=0,
                "target_head":host.container(target).unwrap().head!=0,
            });
            if binding.sig.ends_with(")I") {
                result["ret"] = json!(got);
            } else if binding.sig.ends_with(';') {
                result["ret"] = json!(got.unwrap_or(0) != 0);
            } else {
                assert_eq!(got, None);
            }
            assert_eq!(result, *w, "{name} row {} result", row["id"]);
            support::assert_record(&host, binding, &words, got);
            // The argument ref is supplied outside the target's native handle
            // list, matching the independent raw argument in the oracle.
            let events: Vec<Value> = host
                .events()
                .iter()
                .map(|event| match event {
                    n::ResourceEvent::Bind { node: called, id } => {
                        assert_eq!(*called, node);
                        json!(["lookup", id, 0, 0])
                    }
                    n::ResourceEvent::Materialise { container } => {
                        assert_eq!(*container, source);
                        json!(["materialise"])
                    }

                    n::ResourceEvent::Fetch { container } => {
                        assert_eq!(*container, source);
                        json!(["fetch"])
                    }
                    n::ResourceEvent::Gate { container, mask } => {
                        assert_eq!(*container, source);
                        json!(["gate", mask, 0, 0])
                    }
                    n::ResourceEvent::Release {
                        node: called,
                        destroy,
                    } => {
                        assert_eq!(*called, node);
                        json!(["release", "node", destroy])
                    }
                    n::ResourceEvent::Cache {
                        node: called,
                        low,
                        high,
                    } => {
                        assert_eq!(*called, node);
                        json!(["cache", "node", low, high])
                    }
                    n::ResourceEvent::Duplicate {
                        node: called,
                        source: argument,
                    } => {
                        assert_eq!(*called, node);
                        assert_eq!(*argument, other);
                        json!(["duplicate", "node", 0x777])
                    }
                    n::ResourceEvent::ScaleMesh {
                        node: called,
                        scale,
                    } => {
                        assert_eq!(*called, node);
                        json!(["scale", "node", scale[0], scale[1], scale[2]])
                    }
                    n::ResourceEvent::Make {
                        node: called,
                        resource,
                        text,
                        sound,
                    } => {
                        assert_eq!(*called, node);
                        assert_eq!(*resource, other);
                        assert_eq!(*sound, name == "makeSound");
                        json!(["make", sound, 0x777, text, "paint.dds".len() + 0x1d])
                    }
                    n::ResourceEvent::Wrap { node: wrapped } => {
                        assert_eq!(host.node(*wrapped).unwrap().container, target);
                        assert_eq!(host.node(*wrapped).unwrap().id, 0x55);
                        assert_eq!(host.object_node(got.unwrap()), *wrapped);
                        json!(["wrap", "target"])
                    }
                    n::ResourceEvent::Recolour {
                        container,
                        node: called,
                    } => {
                        assert_eq!(*container, source);
                        assert_eq!(*called, node);
                        json!(["recolour"])
                    }
                    _ => panic!("unexpected resource engine call {event:?}"),
                })
                .collect();
            assert_eq!(events, want_events(&row), "{name} events row {}", row["id"]);
            checked += 1;
        }
    }
    if files == 0 {
        eprintln!("SLR_ORACLE_SKIPPED: resource_native_effects_match_oracle_vectors");
    } else {
        assert_eq!(files, 24, "incomplete resource vectors");
        assert!(checked >= 264);
        assert_eq!(excluded, 8);
    }
    eprintln!("replayed {checked} resource rows across {files} files; {excluded} original bug triggers excluded");
}
fn want_events(row: &Value) -> Vec<Value> {
    row["events"].as_array().unwrap().clone()
}
