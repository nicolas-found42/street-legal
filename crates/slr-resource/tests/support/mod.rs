#![allow(dead_code)] // Shared helpers are used by different integration test targets.
use slr_resource::node as n;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value;

/// Exercise the installed public native bridge, including argument ordering.
pub fn invoke(host: &mut n::ResHost, binding: &n::ResBinding, words: &[u32]) -> Option<u32> {
    let mut vm = VmHost::new();
    vector_classes(&mut vm);
    resource_classes(&mut vm);
    n::install(&mut vm);
    n::with_res(|h| *h = host.clone());
    let mut args: Vec<Value> = n::param_types(binding.sig)
        .into_iter()
        .enumerate()
        .map(|(i, ty)| {
            let word = words.get(i + 1).copied().unwrap_or(0);
            match ty {
                b'F' => Value::float(f32::from_bits(word)),
                b'L' | b'[' => Value::reference(word),
                _ => Value::int(word as i32),
            }
        })
        .rev()
        .collect();
    args.push(Value::reference(words[0]));
    let info = CallInfo {
        jvm: 0,
        thread: 0,
        args: &args,
        class: binding.class,
        name: binding.name,
        sig: binding.sig,
    };
    let f = vm.native_impls[&binding.impl_addr];
    let got = f(&mut vm, &info);
    n::with_res(|h| *host = h.clone());
    if matches!(binding.impl_addr, n::GET_POS | n::GET_ORI | n::GET_VEL) {
        if let Some(word) = got.filter(|w| *w != 0) {
            let object = &vm.objects[(word - 0x1000) as usize];
            assert_eq!(
                object.class_name(&vm.loader),
                if binding.impl_addr == n::GET_ORI {
                    "java.lang.Ypr"
                } else {
                    "java.lang.Vector3"
                }
            );
            let fields: Vec<_> = object
                .fields
                .iter()
                .map(|value| value.as_f32().to_bits())
                .collect();
            assert_eq!(fields, host.vector(word).unwrap());
        }
    }
    if matches!(
        binding.impl_addr,
        n::GET_PARENT | n::GET_WT_ROOT | n::GET_FIRST_CHILD | n::GET_NEXT_CHILD
    ) {
        if let Some(word) = got.filter(|word| *word != 0) {
            let object = &vm.objects[(word - 0x1000) as usize];
            let class_id = match &object.class {
                slr_vm::vmclass::ClassRef::Loaded(id) => *id,
                slr_vm::vmclass::ClassRef::Array(_) => panic!("resource object is not an array"),
            };
            let slot = vm.instance_field_slot_id(class_id, "ptr").unwrap();
            assert_eq!(object.fields[slot].payload, host.object_node(word));
            assert!(vm.loader.resolve_method_id(class_id, "id", "()I").is_some());
        }
    }
    got
}

/// Every native records the marshalled declaration-order arguments and answer.
pub fn assert_record(
    host: &n::ResHost,
    binding: &n::ResBinding,
    words: &[u32],
    result: Option<u32>,
) {
    let expected = slr_calllog::model::LogRecord::NativeCall {
        class: binding.class.to_owned(),
        name: binding.name.to_owned(),
        args: n::param_types(binding.sig)
            .into_iter()
            .enumerate()
            .map(|(i, t)| n::log_value(t, words.get(i + 1).copied().unwrap_or(0)))
            .collect(),
        result: result.map(|w| n::log_value(n::return_type(binding.sig), w)),
    };
    assert_eq!(host.records().last(), Some(&expected));
}

/// Loaded field metadata used by the public VM bridge replay.
pub fn vector_classes(vm: &mut VmHost) {
    for (class, names) in [
        ("java.lang.Vector3", ["x", "y", "z"]),
        ("java.lang.Ypr", ["y", "p", "r"]),
    ] {
        vm.loader.insert_test_class(slr_vm::vmclass::Class {
            kind: 0,
            name: class.to_owned(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: names
                .into_iter()
                .map(|name| slr_vm::vmclass::Field {
                    class: class.to_owned(),
                    name: name.to_owned(),
                    ty: "F".to_owned(),
                    is_static: false,
                })
                .collect(),
        });
    }
}

pub fn resource_classes(vm: &mut VmHost) {
    use slr_vm::vmclass::{Class, Field, Method};
    for (class, base, fields) in [
        ("java.lang.Object", Some("V"), vec![]),
        (
            "java.lang.Native",
            Some("java.lang.Object"),
            vec![("ptr", "I", false)],
        ),
        (
            n::RESOURCE_REF_CLASS,
            Some("java.lang.Native"),
            vec![("RESOURCE_TEXTURE", "I", true)],
        ),
        (
            n::GAME_REF_CLASS,
            Some(n::RESOURCE_REF_CLASS),
            vec![("EVENT_COLLISION", "I", true)],
        ),
        (
            "java.util.resource.RenderRef",
            Some(n::RESOURCE_REF_CLASS),
            vec![("RID_MENUBACK", "I", true)],
        ),
    ] {
        let methods = if class == n::RESOURCE_REF_CLASS {
            ["id", "type"]
                .into_iter()
                .map(|name| Method {
                    class: class.to_owned(),
                    name: name.to_owned(),
                    sig: "()I".to_owned(),
                    flags: 0x41,
                    static_vector: false,
                    is_native: true,
                    body: 0,
                    is_abstract: false,
                    extra: 0,
                })
                .collect()
        } else {
            vec![]
        };
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: class.to_owned(),
            base: base.map(str::to_owned),
            interfaces: vec![],
            methods,
            bodies: vec![],
            fields: fields
                .into_iter()
                .map(|(name, ty, is_static)| Field {
                    class: class.to_owned(),
                    name: name.to_owned(),
                    ty: ty.to_owned(),
                    is_static,
                })
                .collect(),
        });
    }
}
