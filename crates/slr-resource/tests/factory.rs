mod support;
use serde_json::{json, Value};
use slr_resource::{
    factory::{Event, Factory, Root, RuntimeIds},
    node as n,
};
use slr_vm::{host::VmHost, vmclass::ClassId};

fn rows(name: &str) -> Option<Vec<Value>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../re/out/boot/{name}.jsonl"));
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: {name}");
        return None;
    };
    Some(
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
    )
}
fn class() -> ClassId {
    ClassId::from_raw(17).unwrap()
}
fn root(parent: bool) -> Root {
    Root {
        resource_id: 77,
        container: parent.then(|| n::with_res(|h| h.add_container(n::Container::new(1, 77)))),
    }
}
#[test]
fn template_original_rows() {
    let Some(rows) = rows("gametype-factory") else {
        return;
    };
    assert_eq!(rows.len(), 3);
    for row in rows {
        n::reset_res();
        let mut f = Factory::default();
        let root = root(row["parent"].as_bool().unwrap());
        let id = row["allocated"].as_u64().unwrap() as u32;
        let t = f
            .create_template(root, class(), "java.util.resource.Dummy", &mut || id)
            .unwrap();
        let events:Vec<_>=f.events.iter().map(|e|match e {
   Event::Allocate(id)=>json!({"call":"allocate-id","id":id}),
   Event::CopyName(name)=>json!({"call":"copy-name","limit":31,"name":name}),
   Event::Construct {descriptor,..}=>json!({"call":if root.container.is_some(){"create-child"}else{"create-root"},"descriptor":descriptor,"id":id,"parent_id":77,"parent_node_matches":root.container.map(|p|n::with_res(|h|h.container(t.as_ref().unwrap().container).unwrap().parent==p)),"trailing":[0,0]}),
   Event::Retain=>json!({"call":"retain","count":1}), Event::Data=>json!({"call":"data","quality_bits":1f32.to_bits()}),_=>panic!("unexpected event")}).collect();
        assert_eq!(json!(events), row["events"]);
        assert_eq!(
            t.as_ref().map_or(0, |t| t.quality),
            row["node_quality_bits"].as_u64().unwrap() as u32
        );
        assert_eq!(
            t.as_ref().is_some_and(|t| t.class == class()),
            row["data_class_matches_argument2"]
        );
        if let Some(t) = t {
            n::with_res(|h| assert_eq!(h.container(t.container).unwrap().flags, 2));
        }
    }
}
fn native_rows(name: &str, initial_flags: u32) {
    let Some(rows) = rows(name) else { return };
    assert_eq!(rows.len(), if name == "native-factory" { 3 } else { 1 });
    for row in rows {
        n::reset_res();
        let mut f = Factory {
            initial_instance_flags: initial_flags,
            ..Factory::default()
        };
        let root = root(false);
        let template = f
            .create_template(root, class(), "java.util.resource.Dummy", &mut || {
                0xffff007e
            })
            .unwrap()
            .unwrap();
        let original_words = f.templates.get_mut(&template.id).unwrap();
        original_words.words = [0x1234, 0x5678];
        f.events.clear();
        let request = row["request_id"].as_u64().unwrap() as u32;
        let id = row["allocated"].as_u64().unwrap() as u32;
        let instance = f
            .create_instance(root, request, 0x3000, "Dummy", &mut || id)
            .unwrap();
        let events: Vec<_> = f
            .events
            .iter()
            .map(|e| match e {
                Event::Call("instance-data") => "0x300f0000",
                Event::Call("template-materialise") => "0x300f0020",
                Event::Call("gametype-data") => "0x300f0010",
                Event::Call(c) => c,
                _ => panic!("unexpected event"),
            })
            .collect();
        assert_eq!(json!(events), row["events"]);
        assert_eq!(
            instance.as_ref().map_or(0, |i| i.flags),
            row["flags"].as_u64().unwrap() as u32
        );
        assert_eq!(
            instance.as_ref().map_or(0, |i| i.quality),
            row["quality_bits"].as_u64().unwrap() as u32
        );
        assert_eq!(
            instance.as_ref().is_some_and(|i| i.script == 0x3000),
            row["script_backref_matches_argument3"]
        );
        if let Some(i) = instance {
            assert_eq!(i.words, [0x1234, 0x5678]);
            n::with_res(|h| {
                assert_eq!(h.container(i.container).unwrap().head, 0);
                assert_eq!(
                    h.container(i.container)
                        .unwrap()
                        .instance
                        .as_ref()
                        .unwrap()
                        .script,
                    0x3000
                );
                assert_eq!(
                    h.container(template.container).unwrap().head,
                    i.template_link
                );
                assert_eq!(h.node(i.template_link).unwrap().id, template.id);
            });
            assert_eq!(row["temporary_instance_link_released"], true);
            assert_eq!(row["template_link_retained"], true);
        }
        assert_eq!(row["script_entries"], json!([]));
    }
}
#[test]
fn native_factory_controlled_original_rows() {
    native_rows("native-factory", 0);
}
#[test]
fn native_factory_fresh_original_row() {
    native_rows("native-factory-fresh", 0x21);
}
#[test]
fn runtime_id_original_rows() {
    let Some(rows) = rows("resource-ids") else {
        return;
    };
    assert_eq!(rows.len(), 2);
    for row in rows {
        let mut ids = RuntimeIds::new(row["seed"].as_u64().unwrap() as u32);
        let actual: Vec<_> = (0..130).map(|_| ids.allocate()).collect();
        assert_eq!(json!(actual), row["ids"]);
        assert_eq!(ids.next(), row["next"].as_u64().unwrap() as u32);
        assert_eq!(ids.remaining(), row["remaining"].as_u64().unwrap() as usize);
        ids.recycle(0xffff1234);
        assert_eq!(ids.allocate(), row["recycled"].as_u64().unwrap() as u32);
    }
}
#[test]
fn default_cache_and_script_identity() {
    n::reset_res();
    let mut f = Factory::default();
    let root = root(true);
    let mut ids = RuntimeIds::new(1);
    ids.reserve(0xffff0080);
    ids.reserve(0xffff007f);
    let t = f
        .default_for_class(root, class(), "Dummy", &mut || ids.allocate())
        .unwrap();
    assert_eq!(t.id, 0xffff007e);
    let before = ids.remaining();
    let again = f
        .default_for_class(root, class(), "Dummy", &mut || panic!("cache allocation"))
        .unwrap();
    assert_eq!(again.id, t.id);
    assert_eq!(ids.remaining(), before);
    let mut vm = VmHost::new();
    support::resource_classes(&mut vm);
    let script = vm.alloc_object(n::GAME_REF_CLASS).unwrap();
    let linked = n::link_created_script(&mut vm, script, 123).unwrap();
    n::finish_created_script(linked).unwrap();
    assert!(f
        .create_instance(root, t.id, script, "Dummy", &mut || panic!(
            "parent inheritance allocated"
        ))
        .is_err());
    let instance = f
        .create_instance(
            Root {
                resource_id: 77,
                container: None,
            },
            t.id,
            script,
            "Dummy",
            &mut || ids.allocate(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(instance.flags, 0x221);
    let old_ptr = vm.read_field(script, "ptr").unwrap().payload;
    let link = n::bind_existing_script(&mut vm, script, instance.container).unwrap();
    assert_eq!(link.node, old_ptr);
    assert_eq!(link.resource_id, instance.id);
    let count = n::with_res(|h| h.node_count());
    assert_eq!(
        n::bind_existing_script(&mut vm, script, instance.container).unwrap(),
        link
    );
    assert_eq!(n::with_res(|h| h.node_count()), count);
    assert!(f
        .create_instance(root, 0x1b, script, "Dummy", &mut || panic!(
            "unknown template allocated"
        ))
        .is_err());
}
