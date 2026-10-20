//! Embedding failures stop before native execution and preserve its exact name.
use slr_vm::{
    host::{CallInfo, VmHost},
    interp::Thread,
    vmclass::{Class, Method},
};
fn unavailable(_: &CallInfo<'_>) -> Result<(), String> {
    Err("engine collaborator unavailable".into())
}
fn must_not_run(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
    panic!("unavailable native executed")
}
#[test]
fn unavailable_native_backend_does_not_execute_or_fabricate_result() {
    let mut vm = VmHost::new();
    let class = vm.loader.insert_test_class(Class {
        kind: 0,
        name: "test.Backend".into(),
        base: None,
        interfaces: vec![],
        fields: vec![],
        bodies: vec![],
        methods: vec![Method {
            class: "test.Backend".into(),
            name: "query".into(),
            sig: "()I".into(),
            flags: 0x49,
            static_vector: true,
            is_native: true,
            body: 0,
            is_abstract: false,
            extra: 0,
        }],
    });
    vm.natives
        .register("test.Backend", "query", "()I", 0x12345678);
    vm.native_impls.insert(0x12345678, must_not_run);
    vm.native_boundary = Some(unavailable);
    let mut thread = Thread::new();
    assert!(vm
        .enter_method(&mut thread, class, 0, "query", &[])
        .is_err());
    assert_eq!(
        vm.native_failure.as_deref(),
        Some("test.Backend.query()I: engine collaborator unavailable")
    );
    assert!(thread.frames[0].stack.is_empty());
}
