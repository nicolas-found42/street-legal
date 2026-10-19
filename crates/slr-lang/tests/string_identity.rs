//! String identity through the same public seam used by native producers.
use serde_json::{json, Value as Json};
use slr_lang::seam::{intern_string, string_bytes};
use slr_vm::host::VmHost;

#[test]
fn loaded_stock_string_identity_matches_real_intern_lookup_and_ptr_storage() {
    let test = "loaded_stock_string_identity_matches_real_intern_lookup_and_ptr_storage";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::var_os("SLR_VMEXEC_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/vmexec"));
    let Ok(text) = std::fs::read_to_string(dir.join("vmexec_string_identity.jsonl")) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return;
    };
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "empty or partial original String scenario");
    let mut vm = VmHost::new();
    for name in ["Object", "Native", "String"] {
        let Ok(data) = std::fs::read(root.join(format!("stock/system/Scripts/lang/{name}.class")))
        else {
            eprintln!("SLR_ORACLE_SKIPPED: {test}");
            return;
        };
        vm.loader.load_file(&data).unwrap();
    }
    let observations = rows[0]["result"]["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 9);
    let mut words = vec![];
    for row in observations {
        let data = row["input_hex"].as_str().map(|text| {
            (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>()
        });
        let word = if let Some(bytes) = &data {
            intern_string(&mut vm, bytes)
        } else {
            vm.intern_string(None).unwrap()
        };
        assert_eq!(json!(word == 0), row["null"]);
        assert_eq!(
            json!(words
                .iter()
                .enumerate()
                .filter_map(|(index, old)| (*old == word).then_some(index))
                .collect::<Vec<_>>()),
            row["same_as"]
        );
        assert_eq!(json!(vm.objects.len()), row["allocated_objects"]);
        if word != 0 {
            assert_eq!(
                vm.objects[(word - 0x1000) as usize].class,
                "java.lang.String"
            );
            let bytes = string_bytes(&vm, word).unwrap();
            assert_eq!(
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                row["stored_hex"]
            );
        }
        words.push(word);
    }
}

#[test]
fn script_string_literals_and_native_producers_share_the_actual_heap_object() {
    use slr_vm::interp::{Frame, Pc, Slot, Thread};
    use slr_x87::X87;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::var_os("SLR_VMEXEC_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/vmexec"));
    let test = "script_string_literals_and_native_producers_share_the_actual_heap_object";
    let Ok(text) = std::fs::read_to_string(dir.join("vmexec_string_literal.jsonl")) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return;
    };
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    let mut vm = VmHost::new();
    for name in ["Object", "Native", "String"] {
        let Ok(data) = std::fs::read(root.join(format!("stock/system/Scripts/lang/{name}.class")))
        else {
            eprintln!("SLR_ORACLE_SKIPPED: {test}");
            return;
        };
        vm.loader.load_file(&data).unwrap();
    }
    // Own a literal pool in an embedding class, with real unchanged String
    // metadata and native implementation. The original literal's allocator
    // boundary supplies a different word; its requested bytes/tag are compared.
    vm.loader.insert_test_class(slr_vm::vmclass::Class {
        kind: 0,
        name: "Code".into(),
        base: None,
        interfaces: vec![],
        methods: vec![],
        fields: vec![],
        bodies: vec![],
    });
    let node = |opword, imm| slr_fmt_tufa::Node {
        opword,
        operand: 1,
        imm,
    };
    let body = vm
        .add_body(
            "Code",
            vec![node(0x1007, 0x1c), node(0x4009, 0), node(0x10, 0)],
            vec![slr_fmt_tufa::PoolEntry::Str("hello literal".into())],
        )
        .unwrap();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0);
    frame.class_ctx = vm.class_context("Code").unwrap();
    let mut thread = Thread::new();
    thread.pc = pc;
    thread.frames.push(frame);
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    let Slot::Value(value) = thread.frames[0].stack[0] else {
        panic!("literal lvalue")
    };
    assert_eq!(value.tag.name(), rows[0]["result"]["stack"][0]["type"]);
    assert_eq!(
        std::str::from_utf8(vm.string_bytes(value.payload).unwrap()).unwrap(),
        rows[0]["result"]["calls"][0]["text"]
    );
    assert_eq!(intern_string(&mut vm, b"hello literal"), value.payload);
}

#[test]
fn inherited_string_ptr_script_store_is_observed_by_the_installed_stock_native() {
    use slr_fmt_tufa::{Node, PoolEntry};
    use slr_vm::interp::{Frame, Pc, Thread};
    use slr_vm::values::{TypeTag, Value};
    use slr_x87::X87;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut vm = VmHost::new();
    for name in ["Object", "Native", "String"] {
        let Ok(data) = std::fs::read(root.join(format!("stock/system/Scripts/lang/{name}.class")))
        else {
            eprintln!("SLR_ORACLE_SKIPPED: inherited_string_ptr_script_store_is_observed_by_the_installed_stock_native");
            return;
        };
        vm.loader.load_file(&data).unwrap();
    }
    let first = intern_string(&mut vm, b"alpha");
    let second = intern_string(&mut vm, b"12345678");
    let ptr = vm.instance_field_slot("java.lang.String", "ptr").unwrap();
    assert_eq!(
        vm.objects[(first - 0x1000) as usize].fields[ptr].tag,
        TypeTag::Int
    );
    let buffer = vm.objects[(second - 0x1000) as usize].fields[ptr].payload;
    vm.loader.insert_test_class(slr_vm::vmclass::Class {
        kind: 0,
        name: "Code".into(),
        base: None,
        interfaces: vec![],
        methods: vec![],
        fields: vec![],
        bodies: vec![],
    });
    let n = |opword, imm| Node {
        opword,
        operand: 1,
        imm,
    };
    let pool = vec![
        PoolEntry::Str("ptr".into()),
        PoolEntry::Str("java.lang.String".into()),
        PoolEntry::Class(1),
        PoolEntry::Str("length".into()),
        PoolEntry::Str("()I".into()),
        PoolEntry::Type7 { a: 3, b: 4 },
        PoolEntry::Type5 { a: 2, b: 5 },
    ];
    let body = vm
        .add_body(
            "Code",
            vec![
                n(0x1001, 0),
                n(0x401c, 0),
                n(0x1008, 0x23),
                n(0x1027, 0),
                n(0x24, 0),
                n(0x1011, 2),
                n(0x1001, 0),
                n(0x101a, 6),
                n(0x10, 0),
            ],
            pool,
        )
        .unwrap();
    let mut table = slr_lang::seam::NativeTable::new();
    slr_lang::string::install(&mut table);
    slr_lang::seam::install(&mut vm, &table);
    let mut thread = Thread::new();
    let pc = Some(Pc { body, idx: 0 });
    let mut frame = Frame::new(pc, 0);
    frame.class_ctx = vm.class_context("Code").unwrap();
    frame
        .locals
        .push(thread.new_var(Value::reference_named(first, "Ljava.lang.Object;"), 0));
    thread.frames.push(frame);
    thread.pc = pc;
    thread.push_new(Value::int(buffer as i32)).unwrap();
    thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
    assert_eq!(thread.pop_deref().unwrap(), Value::int(8));
    assert_eq!(vm.string_bytes(first), Some(&b"12345678"[..]));
    assert_eq!(
        vm.objects[(first - 0x1000) as usize].fields[ptr].payload,
        buffer
    );
}

#[test]
fn string_creation_rejects_absent_or_incompatible_ptr_metadata() {
    use slr_vm::vmclass::{Class, Field};
    let mut empty = VmHost::new();
    assert_eq!(empty.intern_string(None).unwrap(), 0);
    assert!(empty.intern_string(Some(b"x")).is_err());
    assert!(empty.objects.is_empty());
    let mut wrong = VmHost::new();
    wrong.loader.insert_test_class(Class {
        kind: 0,
        name: "java.lang.String".into(),
        base: None,
        interfaces: vec![],
        methods: vec![],
        bodies: vec![],
        fields: vec![Field {
            class: "java.lang.String".into(),
            name: "ptr".into(),
            ty: "F".into(),
            is_static: false,
        }],
    });
    assert!(
        wrong.intern_string(Some(b"x")).is_err(),
        "a Float field is not the original Native.ptr cell"
    );
    assert!(wrong.objects.is_empty());
}

#[test]
fn inherited_string_native_reads_a_loaded_subclass_ptr_cell() {
    use slr_vm::host::CallInfo;
    use slr_vm::interp::{HeapSlot, Host};
    use slr_vm::values::Value;
    use slr_vm::vmclass::Class;
    let test = "inherited_string_native_reads_a_loaded_subclass_ptr_cell";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut vm = VmHost::new();
    for name in ["Object", "Native", "String"] {
        let Ok(data) = std::fs::read(root.join(format!("stock/system/Scripts/lang/{name}.class")))
        else {
            eprintln!("SLR_ORACLE_SKIPPED: {test}");
            return;
        };
        vm.loader.load_file(&data).unwrap();
    }
    vm.loader.insert_test_class(Class {
        kind: 0,
        name: "StringSubclass".into(),
        base: Some("java.lang.String".into()),
        interfaces: vec![],
        methods: vec![],
        fields: vec![slr_vm::vmclass::Field {
            class: "StringSubclass".into(),
            name: "extra".into(),
            ty: "I".into(),
            is_static: false,
        }],
        bodies: vec![],
    });
    let original = intern_string(&mut vm, b"subclass chars");
    let subclass = vm.alloc_object("StringSubclass").unwrap();
    let index = vm.instance_field_slot("java.lang.String", "ptr").unwrap();
    let ptr = vm
        .read_cell(HeapSlot::Object {
            word: original,
            slot: index,
        })
        .unwrap();
    vm.write_cell(
        HeapSlot::Object {
            word: subclass,
            slot: index,
        },
        ptr,
    )
    .unwrap();
    let mut table = slr_lang::seam::NativeTable::new();
    slr_lang::string::install(&mut table);
    slr_lang::seam::install(&mut vm, &table);
    let args = [Value::reference_named(subclass, "LStringSubclass;")];
    let info = CallInfo {
        jvm: 0,
        thread: 0,
        args: &args,
        class: "java.lang.String",
        name: "length",
        sig: "()I",
    };
    assert_eq!(slr_lang::seam::bridge(&mut vm, &info), Some(14));
    assert_eq!(vm.string_bytes(subclass), Some(&b"subclass chars"[..]));
}

fn raw_literal_class(bytes: &[u8], bad_name: bool, bad_field_type: bool) -> Vec<u8> {
    let mut cons = vec![];
    for word in [3u32, 0, 4] {
        cons.extend(word.to_le_bytes());
    }
    cons.extend(b"Code\0");
    for word in [0, bytes.len() as u32] {
        cons.extend(word.to_le_bytes());
    }
    cons.extend(bytes);
    cons.push(0);
    for word in [4u32, 0] {
        cons.extend(word.to_le_bytes());
    }
    let clss = [u32::MAX, 0, if bad_name { 1 } else { 2 }, u32::MAX, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let fields = if bad_field_type {
        vec![0u32, 1, 1, 0, 1, u32::MAX]
    } else {
        vec![0, 0]
    }
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    let mut tree = vec![];
    for word in [1u32, 3] {
        tree.extend(word.to_le_bytes());
    }
    for (opcode, imm) in [(7u8, Some(0x1cu32)), (9, Some(1)), (16, None)] {
        tree.push(opcode);
        tree.extend(1u16.to_le_bytes());
        if let Some(imm) = imm {
            tree.extend(imm.to_le_bytes());
        }
    }
    let mut data = b"TUFA".to_vec();
    for word in [4u32, 0x11ae1] {
        data.extend(word.to_le_bytes());
    }
    for (tag, bytes) in [
        (b"CONS", cons),
        (b"FILD", fields),
        (b"MTHD", vec![0u8; 8]),
        (b"CLSS", clss),
        (b"TREE", tree),
    ] {
        data.extend(tag);
        data.extend((bytes.len() as u32).to_le_bytes());
        data.extend(bytes);
    }
    data
}

#[test]
fn parsed_raw_string_literals_preserve_original_bytes_and_nul_termination() {
    use slr_vm::interp::{Frame, Pc, Thread};
    use slr_x87::X87;
    let test = "parsed_raw_string_literals_preserve_original_bytes_and_nul_termination";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = std::env::var_os("SLR_VMEXEC_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/vmexec"));
    let Ok(text) = std::fs::read_to_string(dir.join("vmexec_raw_literals.jsonl")) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        return;
    };
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        3,
        "empty or partial original raw literal vectors"
    );
    for row in rows {
        let mut vm = VmHost::new();
        for name in ["Object", "Native", "String"] {
            let Ok(data) =
                std::fs::read(root.join(format!("stock/system/Scripts/lang/{name}.class")))
            else {
                eprintln!("SLR_ORACLE_SKIPPED: {test}");
                return;
            };
            vm.loader.load_file(&data).unwrap();
        }
        let hex = row["input"]["raw_hex"].as_str().unwrap();
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        vm.loader
            .load_file(&raw_literal_class(&bytes, false, false))
            .unwrap();
        assert!(vm.loader.pool_text("Code", 1).is_err());
        assert_eq!(vm.loader.pool_bytes("Code", 2).unwrap(), None);
        let body = vm.loader.body_id("Code", 0).unwrap();
        let pc = Some(Pc { body, idx: 0 });
        let mut frame = Frame::new(pc, 0);
        frame.class_ctx = vm.class_context("Code").unwrap();
        let mut thread = Thread::new();
        thread.pc = pc;
        thread.frames.push(frame);
        thread.run(&mut vm, 0., &mut || X87::from_f64(0.)).unwrap();
        let value = thread.pop_deref().unwrap();
        assert_eq!(value.tag.name(), row["result"]["stack"][0]["type"]);
        let bytes = vm.string_bytes(value.payload).unwrap();
        let actual = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, row["result"]["calls"][0]["bytes_hex"]);
    }
}

#[test]
fn raw_constant_bytes_cannot_silently_become_class_names_or_field_descriptors() {
    for (bad_name, bad_field_type) in [(true, false), (false, true)] {
        let mut vm = VmHost::new();
        assert!(vm
            .loader
            .load_file(&raw_literal_class(&[0xff], bad_name, bad_field_type))
            .is_err());
        assert!(vm.loader.find("Code").is_none());
        assert!(vm.loader.find("\u{fffd}").is_none());
    }
}
