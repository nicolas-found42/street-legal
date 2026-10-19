//! Call-log seam shared by the headless render/sound/input backends.
use slr_calllog::{headless, LogReader, LogRecord, LogWriter, Value as LogValue};
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value,
};

fn call(vm: &mut VmHost, class: &str, name: &str, sig: &str, params: &[Value]) -> Option<u32> {
    let mut args: Vec<_> = params.iter().rev().copied().collect();
    args.push(Value::reference(0x1000));
    let addr = vm
        .natives
        .resolve(class, name, sig)
        .expect("installed binding");
    vm.native_impls[&addr](
        vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class,
            name,
            sig,
        },
    )
}

#[test]
fn mixed_backend_calls_keep_execution_order_and_bit_patterns() {
    headless::reset();
    slr_render_natives::reset_render();
    slr_sound::reset_sound();
    slr_input::reset_input();
    let mut vm = VmHost::new();
    slr_render_natives::install_headless(&mut vm);
    slr_sound::install_headless(&mut vm);
    slr_input::install_headless(&mut vm);
    assert_eq!(
        call(
            &mut vm,
            "java.render.Text",
            "setDefaultColor",
            "(I)V",
            &[Value::int(0x11223344)]
        ),
        None
    );
    assert_eq!(
        call(
            &mut vm,
            "java.sound.Sound",
            "setVolume",
            "(IF)V",
            &[Value::int(2), Value::float(-0.0)]
        ),
        None
    );
    assert_eq!(
        call(
            &mut vm,
            "java.io.Input",
            "activeAxis",
            "(I)I",
            &[Value::int(7)]
        ),
        Some(u32::MAX)
    );
    assert_eq!(
        call(&mut vm, "java.render.Text", "getDefaultColor", "()I", &[]),
        Some(0x11223344)
    );
    let want = vec![
        LogRecord::NativeCall {
            class: "java.render.Text".into(),
            name: "setDefaultColor".into(),
            args: vec![LogValue::Int(0x11223344)],
            result: None,
        },
        LogRecord::NativeCall {
            class: "java.sound.Sound".into(),
            name: "setVolume".into(),
            args: vec![LogValue::Int(2), LogValue::FloatBits(0x80000000)],
            result: None,
        },
        LogRecord::NativeCall {
            class: "java.io.Input".into(),
            name: "activeAxis".into(),
            args: vec![LogValue::Int(7)],
            result: Some(LogValue::Int(-1)),
        },
        LogRecord::NativeCall {
            class: "java.render.Text".into(),
            name: "getDefaultColor".into(),
            args: vec![],
            result: Some(LogValue::Int(0x11223344)),
        },
    ];
    let got = headless::take_records();
    assert_eq!(got, want);
    assert!(headless::take_records().is_empty());
    let mut writer = LogWriter::create(Vec::new()).unwrap();
    for record in &got {
        writer.record(record).unwrap();
    }
    let bytes = writer.into_inner();
    let mut reader = LogReader::open(bytes.as_slice()).unwrap();
    for record in &want {
        assert_eq!(reader.next_record().unwrap(), Some(record));
    }
    assert_eq!(reader.next_record().unwrap(), None);
}

#[test]
fn every_render_sound_input_registration_emits_a_schema_valid_call() {
    headless::reset();
    slr_render_natives::reset_render();
    slr_sound::reset_sound();
    slr_input::reset_input();
    let mut vm = VmHost::new();
    slr_render_natives::install_headless(&mut vm);
    slr_sound::install_headless(&mut vm);
    slr_input::install_headless(&mut vm);
    let mut rows = Vec::new();
    for row in slr_render_natives::RENDER_NATIVE_BINDINGS
        .iter()
        .chain(slr_render_natives::HEADLESS_RENDER_BINDINGS)
    {
        rows.push((row.class, row.name, row.sig, row.impl_addr));
    }
    assert_eq!(rows.len(), 34, "the native table has 34 java.render rows");
    for row in slr_sound::SOUND_NATIVE_BINDINGS {
        rows.push((row.class, row.name, row.sig, row.impl_addr));
    }
    assert_eq!(rows.len(), 44, "ten java.sound rows");
    for stub in slr_input::all_stubs() {
        let row = stub.binding;
        rows.push((row.class, row.name, row.sig, row.impl_addr));
    }
    assert_eq!(
        rows.len(),
        64,
        "thirteen Input/MouseCursor plus seven Controller rows"
    );
    let unique: std::collections::HashSet<_> = rows.iter().copied().collect();
    assert_eq!(unique.len(), 64, "no duplicated registration");
    // Check the complete registered triples against the independently extracted
    // table when available. Controller's seven gap entries are not in that TSV.
    let table =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/native_api.tsv");
    if let Ok(text) = std::fs::read_to_string(table) {
        let table_rows: std::collections::HashSet<_> = text
            .lines()
            .filter_map(|line| {
                let columns: Vec<_> = line.split('\t').collect();
                if columns.len() != 4
                    || !(columns[0].starts_with("java.render.")
                        || columns[0] == "java.sound.Sound"
                        || columns[0] == "java.io.Input"
                        || columns[0] == "java.io.MouseCursor")
                {
                    return None;
                }
                let address = u32::from_str_radix(&columns[3][columns[3].len() - 8..], 16).unwrap();
                Some((columns[0], columns[1], columns[2], address))
            })
            .collect();
        let captured: std::collections::HashSet<_> = rows
            .iter()
            .copied()
            .filter(|r| r.0 != "java.io.Controller")
            .collect();
        assert_eq!(
            captured, table_rows,
            "headless registrations match the original native table"
        );
    }
    for (class, name, sig, address) in rows {
        assert_eq!(vm.natives.resolve(class, name, sig), Some(address));
        // Synthetic test inputs exercise wire encoding and ABI argument order;
        // their values are not differential claims about graphics/device state.
        let types = slr_render_natives::param_types(sig);
        let words: Vec<_> = (0..types.len())
            .map(|index| 0x3f000001 + index as u32)
            .collect();
        let params: Vec<_> = types
            .iter()
            .zip(&words)
            .map(|(ty, word)| match ty {
                b'F' => Value::float(f32::from_bits(*word)),
                b'L' | b'[' => Value::reference(*word),
                _ => Value::int(*word as i32),
            })
            .collect();
        let answer = call(&mut vm, class, name, sig, &params);
        let records = headless::take_records();
        assert_eq!(records.len(), 1, "{class}.{name}: one call");
        let LogRecord::NativeCall {
            class: got_class,
            name: got_name,
            args,
            result,
        } = &records[0]
        else {
            panic!("native record")
        };
        assert_eq!(got_class, class);
        assert_eq!(got_name, name);
        let expected_args: Vec<_> = types
            .iter()
            .zip(words)
            .map(|(ty, word)| {
                if *ty == b'F' {
                    LogValue::FloatBits(word)
                } else {
                    LogValue::Int(word as i32)
                }
            })
            .collect();
        assert_eq!(
            args, &expected_args,
            "{class}.{name}: typed declaration order"
        );
        let return_type = slr_render_natives::return_type(sig);
        assert_eq!(
            answer.is_some(),
            return_type != b'V',
            "{class}.{name}: declared result"
        );
        assert!(
            match (return_type, result) {
                (b'V', None) | (b'F', Some(LogValue::FloatBits(_))) => true,
                (ty, Some(LogValue::Int(_))) if ty != b'V' && ty != b'F' => true,
                _ => false,
            },
            "{class}.{name}: result type"
        );
        let mut writer = LogWriter::create(Vec::new()).unwrap();
        writer.record(&records[0]).unwrap();
        let bytes = writer.into_inner();
        let mut reader = LogReader::open(bytes.as_slice()).unwrap();
        assert_eq!(reader.next_record().unwrap(), Some(&records[0]));
        assert_eq!(reader.next_record().unwrap(), None);
    }
}
