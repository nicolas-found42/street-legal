//! GroundRef water mutations share the configured-body physics address space.
use slr_physics::cfg::PHYSICS_WORLD;
use slr_resource::environment::{self, WaterCommand};
use slr_vm::host::VmHost;

fn water(_: &mut VmHost, command: WaterCommand) {
    crate::native_body::with_world_memory(|memory| {
        environment::apply_water(memory, PHYSICS_WORLD, command);
    })
    .expect("GroundRef native requires an available shared physics world");
}

pub fn install(vm: &mut VmHost) {
    environment::install(vm, water);
}
