use slr_vm::{
    host::VmHost,
    interp::{Flow, Frame, Thread},
    values::Value,
};
/// Replays the original 41fbc0 null-TREE dispatch probe, including the
/// surprising V-return I0 cell. Inputs/expected rows remain ignored.
#[test]
fn empty_dispatch_original_rows() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/boot/empty-dispatch.jsonl");
    let Ok(rows) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: empty_dispatch_original_rows");
        return;
    };
    let mut count = 0;
    for line in rows.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let arguments = row["arguments"].as_bool().unwrap();
        let suffix = row["return_type"].as_str().unwrap_or("");
        let signature = format!("({}){suffix}", if arguments { "IF" } else { "" });
        let mut host = VmHost::new();
        let method = slr_vm::vmclass::Method {
            class: "java.lang.Object".into(),
            name: "empty".into(),
            sig: signature,
            flags: 1,
            static_vector: false,
            is_native: false,
            body: 0,
            is_abstract: false,
            extra: 0,
        };
        let owner = host.loader.insert_test_class(slr_vm::vmclass::Class {
            kind: 0,
            name: "java.lang.Object".into(),
            base: None,
            interfaces: vec![],
            methods: vec![method],
            fields: vec![],
            bodies: vec![vec![]],
        });
        let receiver = host.alloc_object_id(owner).unwrap();
        let mut thread = Thread::new();
        thread.frames.push(Frame::new(None, 0));
        let args = if arguments {
            vec![Value::int(11), Value::float(2.5)]
        } else {
            vec![]
        };
        let before = thread.pc;
        assert_eq!(
            host.enter_method(&mut thread, owner, receiver, "empty", &args)
                .unwrap(),
            Flow::Advance
        );
        assert_eq!(thread.frames.len(), 1);
        assert_eq!(thread.pc, before);
        let mut actual = vec![];
        while !thread.frames[0].stack.is_empty() {
            let value = thread.pop_deref().unwrap();
            actual.push(serde_json::json!({"payload": value.payload, "type": value.tag.name()}));
            thread.release(value);
        }
        actual.reverse();
        assert_eq!(actual, row["stack"].as_array().unwrap().to_owned());
        assert!(row["same_frame"].as_bool().unwrap());
        assert!(row["same_pc"].as_bool().unwrap());
        count += 1;
    }
    assert_eq!(count, 6);
}
