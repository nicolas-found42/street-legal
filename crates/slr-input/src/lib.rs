//! Headless input capture (M1-12), sharing M1-09's device-less native behavior.
//!
//! No second input host is created: the facade uses `slr-io`'s thread-local
//! state and records the same native result into the ordered headless stream.
//! Device acquisition and live DirectInput semantics remain M6-04 work; this
//! backend emits native calls, not synthetic DirectInput reports.
pub use slr_io::input::{all_stubs, reset_input, with_input, InputHost};
use slr_vm::host::{CallInfo, VmHost};

/// Install all twenty Input, MouseCursor and Controller logging natives.
pub fn install_headless(vm: &mut VmHost) {
    slr_io::input::register(vm);
    for stub in all_stubs() {
        vm.native_impls.insert(
            stub.binding.impl_addr,
            headless_bridge as slr_vm::host::NativeFn,
        );
    }
}

fn headless_bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // Resolve before calling: a missing registration must not duplicate an
    // earlier record when the underlying bridge declines a call.
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    slr_io::input::stub_by_addr(address)?;
    let result = slr_io::input::bridge(vm, info);
    with_input(|host| {
        slr_calllog::headless::record(host.records().last().expect("input call recorded").clone())
    });
    result
}
