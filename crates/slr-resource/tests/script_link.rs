//! Original creation bridge's pre-constructor native link projection.
mod support;
use serde_json::{json, Value};
use slr_resource::node as n;
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value as Cell,
};

fn call(vm: &mut VmHost, class: &str, name: &str, sig: &str, args: &[Cell]) -> Option<u32> {
    n::bridge(
        vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            class,
            name,
            sig,
            args,
        },
    )
}
#[test]
fn created_script_link_original_rows() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/boot/native-link.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: created_script_link_original_rows");
        return;
    };
    let mut count = 0;
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let id = row["link_state"]["id"].as_u64().unwrap() as u32;
        let mut vm = VmHost::new();
        support::resource_classes(&mut vm);
        n::install(&mut vm);
        n::reset_res();
        let script = vm.alloc_object(n::GAME_REF_CLASS).unwrap();
        assert_eq!(vm.read_field(script, "ptr").unwrap().payload, 0);
        let link = n::link_created_script(&mut vm, script, id).unwrap();
        assert_eq!(vm.read_field(script, "ptr").unwrap().payload, link.node);
        let constructor = n::with_res(|h| {
            let node = h.node(link.node).unwrap();
            let container = h.container(link.container).unwrap();
            json!({"prev":node.prev,"id":node.id,"owner":if node.container==link.container {"native-node"}else{"wrong-owner"},"owner_head_matches":container.head==link.node,"temporary_successor":node.next!=0 && h.node(node.next).is_some_and(|next|next.prev==link.node && next.container==link.container)})
        });
        assert_eq!(constructor, row["constructor_link_state"]);
        let before = n::with_res(|h| {
            (
                h.node_count(),
                h.events().len(),
                h.node(link.node).unwrap().clone(),
            )
        });
        assert_eq!(n::link_created_script(&mut vm, script, id).unwrap(), link);
        let mut observations = Vec::new();
        for observed in row["native_observations"].as_array().unwrap() {
            let name = observed["name"].as_str().unwrap();
            let signature = observed["signature"].as_str().unwrap();
            let args = match name {
                "newNative" | "id" => vec![Cell::reference(script)],
                "set" => vec![Cell::int(id as i32), Cell::reference(script)],
                _ => panic!("unexpected original observation {name}"),
            };
            let allocated = n::with_res(|h| h.node_count());
            let result = call(&mut vm, n::RESOURCE_REF_CLASS, name, signature, &args);
            observations.push(json!({"name":name,"signature":signature,"id":result,"no_allocation":n::with_res(|h|h.node_count()==allocated),"same_link":vm.read_field(script,"ptr").unwrap().payload==link.node}));
            n::with_res(|h| {
                assert_eq!(
                    (
                        h.node_count(),
                        h.events().len(),
                        h.node(link.node).unwrap().clone()
                    ),
                    before
                )
            });
        }
        assert_eq!(
            observations,
            row["native_observations"].as_array().unwrap().to_owned()
        );
        assert_eq!(
            call(
                &mut vm,
                n::RESOURCE_REF_CLASS,
                "type",
                "()I",
                &[Cell::reference(script)]
            ),
            Some(1)
        );
        assert_eq!(
            call(
                &mut vm,
                n::GAME_REF_CLASS,
                "getScriptInstance",
                "()Ljava.lang.Object;",
                &[Cell::reference(script)]
            ),
            Some(script)
        );
        n::finish_created_script(link).unwrap();
        let snapshot = n::with_res(|h| {
            let node = h.node(link.node).unwrap();
            let container = h.container(link.container).unwrap();
            json!({"prev":node.prev,"next":node.next,"id":node.id,"owner":if node.container==link.container {"native-node"}else{"wrong-owner"},"owner_head_matches":container.head==link.node,"script_backref_matches":container.instance.as_ref().unwrap().script==script})
        });
        assert_eq!(snapshot, row["link_state"]);
        assert_eq!(n::link_created_script(&mut vm, script, id).unwrap(), link);
        n::with_res(|h| assert_eq!(h.node_count(), 1));
        let other = vm.alloc_object(n::GAME_REF_CLASS).unwrap();
        assert!(n::link_created_script(&mut vm, other, id).is_err());
        assert_eq!(vm.read_field(other, "ptr").unwrap().payload, 0);
        count += 1;
    }
    assert_eq!(count, 2);
}
