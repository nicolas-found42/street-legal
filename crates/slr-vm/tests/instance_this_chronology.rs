//! Bounded original instance-this construction chronology, not ABI acceptance.
//! Numeric LocalHost supplies receiver typing and controlled name identity0.
//! Original literal name 'this'/0x60c37c is retained and checked separately;
//! this test does not assert that semantic0 is an original null name pointer.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::Node;
use slr_vm::{
    interp::{BodyId, Frame, Host, Pc, ScriptMethod, Thread, ThreadPools, VmError},
    machine::{Event, FramePhase},
    values::{PoolEvent, PoolKind, TypeTag, Value},
};

fn digest(path: &std::path::Path) -> String {
    let output = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}
struct LocalHost;
impl Host for LocalHost {
    fn body(&self, _: BodyId) -> &[Node] {
        &[]
    }
    fn receiver_value(&mut self, word: u32) -> Result<Value, VmError> {
        Ok(Value::reference_named(word, "LLeaf;"))
    }
}
fn tag(name: &str) -> TypeTag {
    match name {
        "I" => TypeTag::Int,
        "F" => TypeTag::Float,
        _ => panic!("inventory type"),
    }
}

#[test]
fn instance_this_selects_callee_constructs_zero_attaches_then_stores_receiver_like_original() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture: Json = serde_json::from_slice(
        &std::fs::read(root.join("re/out/vmexec/vm_instance_this_chronology.json"))
            .expect("run vm_instance_this_chronology_probe.py"),
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
        "vm_instance_this_chronology_probe.py",
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
    assert_eq!(cases.len(), 6, "exact original inventory");
    let parameters = [vec![], vec!["I"], vec!["I", "F"]];
    for (index, row) in cases.iter().enumerate() {
        let types = &parameters[index % 3];
        assert_eq!(
            row["input"],
            json!({"receiver_variant":index / 3,"parameters":types})
        );
        let original = row["events"].as_array().unwrap();
        assert_eq!(original.len(), 5);
        assert_eq!(
            original
                .iter()
                .map(|e| e["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "select_callee",
                "constructor_enter",
                "typed_zero_constructed",
                "append_this",
                "store_receiver"
            ]
        );
        assert_eq!(
            original
                .iter()
                .map(|e| e["pc"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0x4202a1, 0x423ec0, 0x4202c7, 0x4202f8, 0x4202fb]
        );
        let callee = &original[0]["selected"];
        let variable = &original[1]["variable"];
        assert_ne!(&original[0]["previous"], callee);
        for observation in &original[1..] {
            assert_eq!(&observation["current_frame"], callee);
            assert_eq!(observation["registers"].as_object().unwrap().len(), 8);
        }
        assert_eq!(original[1]["name_address"], 0x60c37c);
        assert_eq!(original[2]["words"][0], 0x5e7344);
        assert_eq!(original[2]["words"][2], 0);
        assert_eq!(original[2]["words"][3], 0x60c37c);
        assert_eq!(original[2]["descriptor"], "LLeaf;");
        assert_eq!(original[2]["name"], "this");
        assert_eq!(original[2]["identity"], json!({"kind":"variable","slot":0}));
        assert_eq!(original[3]["previous_count"], 0);
        assert_eq!(original[3]["count"], 1);
        assert_eq!(&original[3]["attached"], variable);
        assert_eq!(original[3]["payload"], 0);
        assert_eq!(original[4]["before"], 0);
        assert_eq!(original[4]["after"], row["receiver"]);
        assert_eq!(original[4]["locals"], json!([variable]));
        assert_eq!(original[4]["attached"], true);
        assert_eq!(row["final_words"][2], row["receiver"]);
        assert_eq!(row["return_code"], 0);

        let pools = ThreadPools::default();
        pools.machine_trace().start().unwrap();
        let mut thread = Thread::with_pools(pools);
        let pc = Some(Pc {
            body: BodyId(0),
            idx: 0,
        });
        thread.push_frame(Frame::new(pc, 0)).unwrap();
        thread.pc = pc;
        for ty in types {
            thread
                .push_new(if *ty == "I" {
                    Value::int(11)
                } else {
                    Value::float(2.5)
                })
                .unwrap();
        }
        let receiver = if index / 3 == 0 { 0x1234 } else { 0xabcd_9876 };
        thread
            .call_script_method_with(
                &mut LocalHost,
                &ScriptMethod {
                    body: BodyId(1),
                    class_ctx: 7,
                    signature: 9,
                    is_static: false,
                    params: types.iter().map(|ty| tag(ty)).collect(),
                },
                receiver,
            )
            .unwrap();
        let frame = thread.frames.last().unwrap();
        let activation = frame.machine_identity();
        let this = frame.locals[0];
        assert_eq!(
            thread.vars.get(this).value,
            Value::reference_named(receiver, "LLeaf;")
        );
        let events = thread.machine_trace().pending(usize::MAX);
        let created = events
            .iter()
            .position(|e| {
                matches!(e.event,
            Event::Frame { phase:FramePhase::Create, frame, .. } if frame.identity==activation)
            })
            .unwrap();
        let allocated = events
            .iter()
            .position(|e| {
                matches!(
                    e.event,
                    Event::Pool(PoolEvent::Allocate {
                        kind: PoolKind::Variable,
                        slot: 0,
                        ..
                    })
                )
            })
            .unwrap();
        let Event::Pool(PoolEvent::Allocate { cell, .. }) = events[allocated].event else {
            unreachable!()
        };
        assert_eq!(
            cell.value,
            Value::reference_named(0, "LLeaf;"),
            "case {index}: original423ec0 constructs this as typed zero before attachment"
        );
        let selected = events
            .iter()
            .position(|e| {
                matches!(e.event,
            Event::CurrentFrame { frame, .. } if frame==activation)
            })
            .expect("original4202a1 selects callee before this allocation");
        let appended = events.iter().position(|e|matches!(e.event,
            Event::Local { frame, append:true,index:0,variable,.. } if frame==activation && variable==this)).unwrap();
        let stored = events.iter().position(|e|matches!(e.event,
            Event::VariableStore { identity, before,after } if identity==this
                && before==Value::reference_named(0,"LLeaf;") && after==Value::reference_named(receiver,"LLeaf;")))
            .expect("original4202fb receiver store follows local attachment");
        assert!(
            created < selected && selected < allocated && allocated < appended && appended < stored,
            "case {index}: retain original causal this construction order"
        );
    }
}
