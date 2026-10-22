//! Creation wrapper link/return protocol, independently executed in build606.
use slr_resource::node::{self as n, Container, GameInstance, ResHost};

#[test]
fn original_creation_wrappers_rebind_to_created_resource_with_live_neighbors() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/boot/game-ref-create-link-original.json");
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/game_ref_create_link_probe.py");
        return;
    };
    let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(data["rows"].as_array().unwrap().len(), 24);
    for row in data["rows"].as_array().unwrap() {
        let mut host = ResHost::new();
        let old = host.add_container(Container::new(1, 7));
        let mut resource = Container::new(1, 11);
        resource.instance = Some(GameInstance {
            script: 0x1234,
            ..Default::default()
        });
        let created = host.add_container(resource);
        let owner = host.add_container(Container::new(1, 77));
        let owner_handle = host.add_bound_node(77, owner);
        host.bind_object(8, owner_handle);
        host.bind_object(n::DEFAULT_RESOURCE_WORD, owner_handle);
        let target = match row["initial"].as_str().unwrap() {
            "old" => old,
            "created" => created,
            "unbound" => 0,
            _ => unreachable!(),
        };
        let id = host.container(target).map_or(0, |c| c.id);
        let handle = host.add_bound_node(id, target);
        let neighbor = host.add_bound_node(id, target);
        // The original scenario has no neighbor link when initially unbound.
        host.bind_object(9, handle);
        host.set_created_result(if row["success"].as_bool().unwrap() {
            created
        } else {
            0
        });
        let native = row["native"].as_bool().unwrap();
        let binding = n::GAME_REF_BINDINGS
            .iter()
            .find(|b| b.impl_addr == if native { n::CREATE_NATIVE } else { n::CREATE })
            .unwrap();
        let result = host.call(
            binding,
            &[
                9,
                if row["default_owner"].as_bool().unwrap() {
                    0
                } else {
                    8
                },
                0x456,
                0x789,
                0xabc,
            ],
        );
        assert_eq!(serde_json::json!(result), row["result"], "{row}");
        let identity = |word| {
            if word == 0 {
                "null"
            } else if word == handle {
                "handle"
            } else if word == neighbor {
                "neighbor"
            } else if word == old {
                "old"
            } else if word == created {
                "created"
            } else {
                panic!("unexpected identity {word:#x}")
            }
        };
        let link = |word| {
            let value = host.node(word).unwrap();
            serde_json::json!({"prev":identity(value.prev), "next":identity(value.next),
                "id":value.id,"container":identity(value.container)})
        };
        assert_eq!(link(handle), row["handle"], "{row}");
        assert_eq!(link(neighbor), row["neighbor"], "{row}");
        assert_eq!(
            identity(host.container(old).unwrap().head),
            row["old_head"].as_str().unwrap(),
            "{row}"
        );
        assert_eq!(
            identity(host.container(created).unwrap().head),
            row["created_head"].as_str().unwrap(),
            "{row}"
        );
    }
}
