//! Pointer-bearing original observations validate opaque cell identities and
//! chronology. Pool slot ordinals are compared; native addresses/heap backing
//! and interpreter machine scratch are deliberately separate dependencies.
use serde_json::{json, Value as Json};
use slr_fmt_tufa::Node;
use slr_vm::{
    host::VmHost,
    interp::{BodyId, Frame, Host, Pc, Slot, Thread, VmError},
    values::{AllocOrigin, PoolEvent, PoolKind, TypeTag, Value},
};

struct Code(Vec<Node>);
impl Host for Code {
    fn body(&self, _: BodyId) -> &[Node] {
        &self.0
    }
}
fn pc() -> Option<Pc> {
    Some(Pc {
        body: BodyId(0),
        idx: 0,
    })
}
fn code(nodes: &Json) -> Code {
    Code(
        nodes
            .as_array()
            .unwrap()
            .iter()
            .map(|n| Node {
                opword: n[0].as_u64().unwrap() as u16,
                operand: n[1].as_u64().unwrap() as u16,
                imm: n[2].as_u64().unwrap() as u32,
            })
            .collect(),
    )
}
fn enter(thread: &mut Thread) {
    thread.frames.push(Frame::new(pc(), 0));
    thread.pc = pc();
}
fn run(thread: &mut Thread, code: &mut Code) -> Result<(), VmError> {
    thread
        .run(code, 0., &mut || slr_x87::X87::from_f64(0.))
        .map(|_| ())
}
fn check_state(thread: &Thread, expected: &Json) {
    let value_pool = thread.pools().values.snapshot();
    let vars = thread.vars.snapshot();
    assert_eq!(json!(value_pool.handed), expected["values"]["words"][2]);
    assert_eq!(json!(value_pool.capacity), expected["values"]["words"][3]);
    assert_eq!(json!(value_pool.free), expected["values"]["free_slots"]);
    assert_eq!(json!(vars.handed), expected["variables"]["words"][2]);
    assert_eq!(json!(vars.capacity), expected["variables"]["words"][3]);
    assert_eq!(json!(vars.free), expected["variables"]["free_slots"]);
    assert_eq!(json!(thread.live()), expected["live"]);
    let actual = thread
        .frames
        .last()
        .map(|frame| {
            frame
                .stack
                .iter()
                .map(|slot| {
                    let (kind, index, value) = match slot {
                        Slot::Value(cell) => ("value", cell.id().slot, cell.value()),
                        Slot::Var(id) => ("variable", id.slot(), thread.vars.get(*id).value),
                        _ => panic!("unexpected borrowed heap/null slot in controlled program"),
                    };
                    assert_eq!(value.tag, TypeTag::Int);
                    json!({"kind":kind,"slot":index,"payload":value.payload})
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let expected_stack = expected["stack"].as_array().unwrap().iter().map(|cell| json!({
        "kind":cell["identity"]["kind"], "slot":cell["identity"]["slot"], "payload":cell["words"][2]
    })).collect::<Vec<_>>();
    assert_eq!(
        actual, expected_stack,
        "exact cell identities in stack order"
    );
}
fn check_events(thread: &Thread, row: &Json) {
    let actual = thread.pools().events().into_iter().map(|event| match event {
        PoolEvent::Allocate { kind, slot, origin, .. } => json!({"action":"allocate", "kind":if kind==PoolKind::Value {"value"}else{"variable"}, "slot":slot, "origin":if origin==AllocOrigin::Pooled {"pooled"}else{"freelist"}}),
        PoolEvent::Release { kind, slot, .. } => json!({"action":"release", "kind":if kind==PoolKind::Value {"value"}else{"variable"}, "slot":slot}),
    }).collect::<Vec<_>>();
    let expected = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e.get("pool_event").is_some())
        .map(|event| {
            let mut result =
                json!({"action":event["pool_event"],"kind":event["kind"],"slot":event["slot"]});
            if let Some(origin) = event.get("origin") {
                result["origin"] = origin.clone();
            }
            result
        })
        .collect::<Vec<_>>();
    assert!(!expected.is_empty(), "missing original pool writes");
    assert_eq!(
        actual, expected,
        "ordered value AND variable allocation/release events"
    );
}
fn release_choice(row: &Json) {
    let mut thread = Thread::new();
    enter(&mut thread);
    thread.pools().observe(true);
    let a = thread.allocate_value(Value::int(7));
    let b = thread.allocate_value(Value::int(7));
    let (aid, bid) = (a.id(), b.id());
    assert_ne!(aid, bid);
    assert_eq!(a.value(), b.value());
    thread.push_owned(a).unwrap();
    thread.push_owned(b).unwrap();
    let second = thread.pop_deref().unwrap();
    let first = thread.pop_deref().unwrap();
    assert_eq!(second.id(), bid);
    assert_eq!(first.id(), aid);
    let (released, survivor) = if row["name"] == "release_first" {
        (first, second)
    } else {
        (second, first)
    };
    let released_id = released.id();
    thread.release(released);
    check_state(&thread, &row["free_before_reuse"]);
    thread.push_owned(survivor).unwrap();
    let replacement = thread.allocate_value(Value::int(7));
    assert_eq!(replacement.id().slot, released_id.slot);
    assert_ne!(replacement.id().generation, released_id.generation);
    thread.push_owned(replacement).unwrap();
    check_state(&thread, &row["final"]);
    check_events(&thread, row);
}
fn opcode(row: &Json) {
    let mut thread = Thread::new();
    enter(&mut thread);
    thread.pools().observe(true);
    for _ in 0..row["synthetic_variable_count"].as_u64().unwrap() {
        let id = thread.new_var(Value::int(7), 0);
        thread.frames[0].locals.push(id);
    }
    let outcome = run(&mut thread, &mut code(&row["nodes"]));
    if row["outcome"] == "fatal" {
        assert_eq!(outcome, Err(VmError::NotLvalue));
    } else {
        outcome.unwrap();
    }
    check_state(&thread, &row["after_run"]);
    if row["destroyed"] == true {
        thread.pop_frame().unwrap();
    }
    check_state(&thread, &row["final"]);
    check_events(&thread, row);
    if row["outcome"] == "fatal" {
        let abandoned = thread.pools().values.snapshot().abandoned;
        assert_eq!(abandoned.len(), 1);
        assert_eq!(abandoned[0].0.slot, 1);
        assert_eq!(abandoned[0].1, Value::int(2));
        assert_eq!(row["final"]["value_cells"][1]["words"][2], 2);
    }
}
fn cross_thread(row: &Json) {
    let vm = VmHost::new();
    let mut first = vm.new_thread();
    let mut second = vm.new_thread();
    enter(&mut first);
    enter(&mut second);
    first.pools().observe(true);
    let variable = first.new_var(Value::int(7), 0);
    first.frames[0].locals.push(variable);
    let mut literal = code(&json!([[0x1027, 1, 7], [0x10, 1, 0]]));
    run(&mut first, &mut literal).unwrap();
    check_state(&first, &row["first"]);
    let Slot::Value(cell) = &first.frames[0].stack[0] else {
        panic!()
    };
    let original_id = cell.id();
    first.frames[0].saved_pc = pc();
    first.pc = pc();
    first.flags = 0;
    run(&mut first, &mut code(&json!([[0x18, 1, 0], [0x10, 1, 0]]))).unwrap();
    check_state(&first, &row["released"]);
    first.pop_frame().unwrap();
    let reused_var = second.new_var(Value::int(7), 0);
    assert_eq!(variable.slot(), reused_var.slot());
    assert_eq!(variable.pool(), reused_var.pool());
    assert_ne!(variable.generation(), reused_var.generation());
    second.frames[0].locals.push(reused_var);
    run(&mut second, &mut literal).unwrap();
    let Slot::Value(cell) = &second.frames[0].stack[0] else {
        panic!()
    };
    assert_eq!(original_id.pool, cell.id().pool);
    assert_eq!(original_id.slot, cell.id().slot);
    assert_ne!(original_id.generation, cell.id().generation);
    check_state(&second, &row["second"]);
    assert_eq!(
        first.live(),
        second.live(),
        "the original counter is global"
    );
    check_events(&second, row);
}
fn no_frame(row: &Json) {
    let mut thread = Thread::new();
    thread.pools().observe(true);
    let cell = thread.allocate_value(Value::int(19));
    assert_eq!(thread.push_owned(cell), Err(VmError::NoFrame));
    check_state(&thread, &row["final"]);
    check_events(&thread, row);
    let snapshot = thread.pools().values.snapshot();
    assert_eq!(snapshot.abandoned.len(), 1);
    assert_eq!(snapshot.abandoned[0].1, Value::int(19));
    assert_eq!(row["fault_pc"], 0x423ba4);
}
fn sha256(path: &std::path::Path) -> String {
    let output = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("macOS shasum");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}
#[test]
fn owned_cell_and_variable_chronology_matches_original_helpers_opcodes_and_shared_threads() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture: Json = serde_json::from_slice(
        &std::fs::read(root.join("re/out/vm-cell-identity-original.json"))
            .expect("run vm_cell_identity_probe.py"),
    )
    .unwrap();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["fpu_cw"], 0x7f);
    assert_eq!(
        fixture["exe_sha256"],
        sha256(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
    );
    let producers = fixture["producer_sources"].as_object().unwrap();
    assert_eq!(
        producers
            .keys()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>(),
        [
            "vm_cell_identity_probe.py",
            "thread_vectors.py",
            "oracle.py"
        ]
        .into_iter()
        .collect()
    );
    for (name, digest) in producers {
        assert_eq!(digest, &sha256(&root.join("re/oracle").join(name)));
    }
    let rows = fixture["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 13);
    let expected_names = [
        "release_first",
        "release_second",
        "equal_distinct_literals",
        "deref_repush_same_cell",
        "equal_distinct_deref_copies",
        "binary_release_then_reuse",
        "frame_destroy_values_and_borrowed_variable",
        "ternary_false_distinct_equal_cells",
        "ternary_true_distinct_equal_cells",
        "switch_miss_preserves_switch_owner",
        "assignment_value_fatal_retains_cell",
        "cross_thread_shared_pool",
        "push_without_frame",
    ];
    assert_eq!(
        rows.iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>(),
        expected_names.into_iter().collect()
    );
    for row in rows {
        match row["name"].as_str().unwrap() {
            "release_first" | "release_second" => release_choice(row),
            "cross_thread_shared_pool" => cross_thread(row),
            "push_without_frame" => no_frame(row),
            _ => opcode(row),
        }
    }
}
