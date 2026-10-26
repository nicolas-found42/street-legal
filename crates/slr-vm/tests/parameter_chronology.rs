//! Original41fbc0 typed parameter variables must exist before41ab60 converts.
//! Passive original observations pin allocation, local attachment and final
//! values; live machine events expose the corresponding port chronology.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::Node;
use slr_vm::{
    interp::{BodyId, Frame, Host, Pc, ScriptMethod, Thread, ThreadPools, VmError},
    machine::{Event, FramePhase},
    ops::Alu,
    values::{PoolEvent, PoolKind, TypeTag, Value},
};

fn digest(path: &std::path::Path) -> String {
    let result = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("macOS shasum");
    assert!(result.status.success());
    String::from_utf8(result.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}
fn value(value: Value) -> Json {
    json!({"type":value.tag.name(),"payload":value.payload})
}
fn tag(name: &str) -> TypeTag {
    match name {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        _ => panic!("unsupported chronology input type {name}"),
    }
}
struct ObserveStore {
    pools: ThreadPools,
    stores: Vec<Json>,
}
impl Host for ObserveStore {
    fn body(&self, _: BodyId) -> &[Node] {
        &[]
    }
    fn receiver_value(&mut self, word: u32) -> Result<Value, VmError> {
        Ok(Value::reference_named(word, "LLeaf;"))
    }
    fn store_value(&mut self, alu: &mut Alu, destination: &mut Value, source: &mut Value) {
        let events = self.pools.events();
        let allocated = events.iter().rev().find_map(|event| match event {
            PoolEvent::Allocate {
                kind: PoolKind::Variable,
                slot,
                cell,
                ..
            } => Some(json!({"slot":slot,"value":value(cell.value)})),
            _ => None,
        });
        let entries = self.pools.machine_trace().pending(usize::MAX);
        let created = entries
            .iter()
            .rev()
            .find_map(|entry| match entry.event {
                Event::Frame {
                    phase: FramePhase::Create,
                    frame,
                    ..
                } => Some(frame.identity),
                _ => None,
            })
            .unwrap();
        let attached = entries.iter().rev().find_map(|entry| match entry.event {
            Event::Local {
                frame,
                append: true,
                index,
                variable,
                ..
            } if frame == created => Some(json!({"index":index,"slot":variable.slot()})),
            _ => None,
        });
        self.stores.push(json!({
            "destination":value(*destination),
            "source":value(*source),
            "variable_handed":self.pools.vars.snapshot().handed,
            "allocated_variable":allocated,
            "attached_local":attached,
        }));
        alu.store(destination, source);
    }
}

#[test]
fn parameters_allocate_typed_zero_variables_before_original_assignment_conversion() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture: Json = serde_json::from_slice(
        &std::fs::read(root.join("re/out/vmexec/vm_parameter_chronology.json"))
            .expect("run re/oracle/vm_parameter_chronology_probe.py"),
    )
    .unwrap();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["cw"], 127);
    assert_eq!(fixture["unicorn"], "2.1.4");
    assert_eq!(
        fixture["executable_sha256"],
        digest(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
    );
    let producers = fixture["producers"].as_object().unwrap();
    assert_eq!(producers.len(), 6);
    for name in [
        "vm_parameter_chronology_probe.py",
        "vmexec_vectors.py",
        "thread_vectors.py",
        "oracle.py",
        "vm_cell_identity_probe.py",
        "vmclass_vectors.py",
    ] {
        let relative = format!("re/oracle/{name}");
        assert_eq!(
            producers.get(&relative).unwrap(),
            &digest(&root.join(relative))
        );
    }
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 6, "exact original chronology case inventory");
    let inputs = [
        vec![("I", 11u32, "I"), ("F", 0x40200000, "F")],
        vec![("I", 0xfffffff9, "F"), ("F", 0xc0600000, "I")],
        vec![
            ("F", 0x3fc00000, "I"),
            ("I", 16777217, "F"),
            ("F", 0x80000000, "F"),
        ],
    ];
    for (index, row) in cases.iter().enumerate() {
        let is_static = index < 3;
        let arguments = &inputs[index % 3];
        assert_eq!(
            row["input"],
            json!({"static":is_static,"arguments":arguments.iter().map(|(s,p,t)|json!({"source":s,"payload":p,"target":t})).collect::<Vec<_>>()})
        );
        let original = row["stores"].as_array().unwrap();
        assert_eq!(original.len(), arguments.len());
        let pools = ThreadPools::default();
        pools.machine_trace().start().unwrap();
        let mut thread = Thread::with_pools(pools);
        let pc = Some(Pc {
            body: BodyId(0),
            idx: 0,
        });
        thread.push_frame(Frame::new(pc, 0));
        thread.pc = pc;
        for (source, payload, _) in arguments {
            thread
                .push_new(Value {
                    tag: tag(source),
                    payload: *payload,
                })
                .unwrap();
        }
        let pools = thread.pools();
        pools.observe(true);
        let mut host = ObserveStore {
            pools,
            stores: vec![],
        };
        let method = ScriptMethod {
            body: BodyId(1),
            class_ctx: 7,
            signature: 9,
            is_static,
            params: arguments.iter().map(|(_, _, target)| tag(target)).collect(),
        };
        thread
            .call_script_method_with(&mut host, &method, if is_static { 0 } else { 0x1234 })
            .unwrap();
        let expected = original.iter().enumerate().map(|(ordinal, store)| {
            let count = ordinal + 1 + usize::from(!is_static);
            assert_eq!(store["destination_is_last_local"],true);
            assert_eq!(store["local_count"], count);
            assert_eq!(store["variable_handed"], count);
            assert_eq!(store["destination_slot"],count-1);
            assert_eq!(store["destination"]["payload"],0);
            assert_eq!(store["local_addresses"].as_array().unwrap().len(),count);
            assert_eq!(store["local_addresses"][count-1],store["destination_address"]);
            json!({"destination":store["destination"],"source":store["source"],"variable_handed":store["variable_handed"],"allocated_variable":{"slot":store["destination_slot"],"value":store["destination"]},"attached_local":{"index":count-1,"slot":store["destination_slot"]}})
        }).collect::<Vec<_>>();
        assert_eq!(host.stores, expected, "case {index}: allocation must precede41ab60, with typed zero in the real variable pool");
        let locals = thread
            .frames
            .last()
            .unwrap()
            .locals
            .iter()
            .enumerate()
            .map(|(local, id)| {
                let v = thread.vars.get(*id).value;
                if !is_static && local == 0 {
                    assert_eq!(v, Value::reference_named(0x1234, "LLeaf;"));
                    json!({"type":"LLeaf;","payload":"receiver"})
                } else {
                    value(v)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!(locals),
            row["final_locals"],
            "case {index}: final parameter conversion/order"
        );
        assert_eq!(row["return_code"], 0);
    }
}
