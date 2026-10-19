//! Explicit loaded-class boundary for native-only fixtures. The separate
//! string_identity integration test uses unchanged stock class files.
pub fn vm() -> slr_vm::host::VmHost {
    use slr_vm::vmclass::{Class, Field};
    let mut vm = slr_vm::host::VmHost::new();
    for (name, base) in [
        ("java.lang.Object", None),
        ("java.lang.Native", Some("java.lang.Object")),
        ("java.lang.String", Some("java.lang.Native")),
    ] {
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: name.into(),
            base: base.map(str::to_owned),
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: if name == "java.lang.Native" {
                vec![Field {
                    class: name.into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }]
            } else {
                vec![]
            },
        });
    }
    vm
}
