//! Headless call-log seam: no graphics device or game data is required.
use slr_calllog::{headless, LogReader, LogRecord, LogWriter, Value as LogValue};
use slr_render_natives as render;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value;

#[test]
fn headless_camera_create_captures_every_argument_without_a_device() {
    headless::reset();
    render::reset_render();
    let mut vm = VmHost::new();
    render::install_headless(&mut vm);
    // Registered Camera.create takes ten parameters, plus the receiver.
    let params = [
        Value::reference(0x1234),
        Value::reference(0x5678),
        Value::int(9),
        Value::float(1.0),
        Value::float(-0.0),
        Value::float(3.0),
        Value::float(4.0),
        Value::float(5.0),
        Value::int(10),
        Value::int(11),
    ];
    let mut args: Vec<_> = params.into_iter().rev().collect();
    args.push(Value::reference(0xabcd));
    let addr = vm
        .natives
        .resolve(
            "java.render.Camera",
            "create",
            "(Ljava.util.resource.ResourceRef;Ljava.render.Viewport;IFFFFFII)V",
        )
        .expect("camera is installed");
    let native = vm.native_impls[&addr];
    assert_eq!(
        native(
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: "java.render.Camera",
                name: "create",
                sig: "(Ljava.util.resource.ResourceRef;Ljava.render.Viewport;IFFFFFII)V"
            }
        ),
        None
    );
    let want = LogRecord::NativeCall {
        class: "java.render.Camera".into(),
        name: "create".into(),
        args: vec![
            LogValue::Int(0x1234),
            LogValue::Int(0x5678),
            LogValue::Int(9),
            LogValue::FloatBits(0x3f800000),
            LogValue::FloatBits(0x80000000),
            LogValue::FloatBits(0x40400000),
            LogValue::FloatBits(0x40800000),
            LogValue::FloatBits(0x40a00000),
            LogValue::Int(10),
            LogValue::Int(11),
        ],
        result: None,
    };
    assert_eq!(headless::take_records(), vec![want.clone()]);
    let mut writer = LogWriter::create(Vec::new()).unwrap();
    writer.record(&want).unwrap();
    let bytes = writer.into_inner();
    let mut reader = LogReader::open(bytes.as_slice()).unwrap();
    assert_eq!(reader.next_record().unwrap().unwrap(), &want);
    assert!(reader.next_record().unwrap().is_none());
}

#[test]
fn capture_only_results_are_explicit_headless_inputs() {
    headless::reset();
    render::reset_render();
    let mut vm = VmHost::new();
    render::install_headless(&mut vm);
    let class = "java.render.Text";
    let name = "getWidthPixels";
    let sig = "(Ljava.lang.String;Ljava.util.resource.ResourceRef;)F";
    let address = vm.natives.resolve(class, name, sig).unwrap();
    render::with_render(|host| host.set_headless_result(address, 0x80000000));
    let args = [
        Value::reference(7),
        Value::reference(8),
        Value::reference(9),
    ];
    assert_eq!(
        vm.native_impls[&address](
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class,
                name,
                sig
            }
        ),
        Some(0x80000000)
    );
    assert_eq!(
        headless::take_records(),
        vec![LogRecord::NativeCall {
            class: class.into(),
            name: name.into(),
            args: vec![LogValue::Int(8), LogValue::Int(7)],
            result: Some(LogValue::FloatBits(0x80000000))
        }]
    );
}
