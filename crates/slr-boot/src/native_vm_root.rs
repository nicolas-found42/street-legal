//! Source427980 startup caller through the JVM/bootstrap/registry handoff.
//! Retains its actual stack and resource bindings; it never seeds caller pages.
use crate::{native_stack as stack, native_vm_fpu, native_vm_live};
use slr_physics::space::Space;
use stack::Reg;
#[path = "native_vm_root_world.rs"]
pub mod world;

pub trait Environment:
    crate::native_vm_jvm::Environment
    + crate::native_vm_namespace::FileEnvironment
    + crate::native_vm_metadata::registration::resources::Environment
{
}
impl<E> Environment for E where
    E: crate::native_vm_jvm::Environment
        + crate::native_vm_namespace::FileEnvironment
        + crate::native_vm_metadata::registration::resources::Environment
{
}
/// Remaining domain callers must consume their actual pending CALL. These
/// boundaries carry the live runtime through Config construction and settings;
/// supplying a result does not prove the collaborator's implementation.
pub trait ContinuationEnvironment: Environment {
    fn initialize_world(&mut self, m: &mut Space) -> Result<(), String>;
    fn refresh_game_root(&mut self, m: &mut Space) -> Result<(), String>;
    fn create_config(
        &mut self,
        m: &mut Space,
        runtime: &mut native_vm_live::Runtime,
    ) -> Result<(), String>;
    fn apply_settings(
        &mut self,
        m: &mut Space,
        runtime: &mut native_vm_live::Runtime,
    ) -> Result<(), String>;
}
#[must_use]
pub struct Startup {
    caller: stack::Frame,
    world: u32,
    runtime: native_vm_live::Runtime,
    pc: u32,
    base: u32,
    returned: bool,
}
impl Startup {
    /// Actual source427980 entry. The embedding must supply the world owner
    /// through its pending CALL; this is not an independent driver fallback.
    /// exe: FUN_00427980
    pub fn begin(
        m: &mut Space,
        fpu: native_vm_fpu::State,
        environment: &mut impl Environment,
    ) -> Result<Self, String> {
        if !stack::active(m) || !stack::has_pending_call(m) {
            return Err("NativeVmRoot: actual startup CALL required".into());
        }
        let world = stack::reg(m, Reg::Ecx);
        if world == 0 {
            return Err("NativeVmRoot: startup world owner absent".into());
        }
        let caller = stack::enter(m, &[], Some(world));
        stack::alloc(m, 0x10);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Edi, 0);
        stack::push(m, 0);
        stack::push(m, 8);
        stack::set_reg(m, Reg::Esi, world);
        stack::push(m, 1);
        write(m, world + 0x110, 0x3f800000);
        resource(m, environment, 0x60caec, 1, 0x62f260, 0x4279a9, 0x4279b4)?;
        stack::set_reg(m, Reg::Ecx, m.r32(0x618d48));
        stack::push(m, 0);
        stack::push(m, 1);
        stack::push(m, 4);
        resource(m, environment, 0x60caf8, 4, 0x636460, 0x4279c9, 0x4279d4)?;
        stack::push(m, 0x80);
        let jvm = stack::external(m, 0x4279de, &[], None, |m| {
            crate::native_vm_jvm::Environment::allocate(environment, m, 0x80)
        })?;
        stack::set_reg(m, Reg::Eax, jvm);
        stack::add_sp(m, 4);
        if jvm == 0 {
            return Err("NativeVmRoot: null JVM allocation is outside admitted startup".into());
        }
        stack::set_reg(m, Reg::Ecx, jvm);
        stack::begin_call(m, 0x4279ec, &[], None);
        let runtime = native_vm_live::Runtime::construct(m, jvm, fpu, environment)?;
        Ok(Self {
            caller,
            world,
            runtime,
            pc: 0x4279ec,
            base: stack::sp(m),
            returned: false,
        })
    }
    pub fn bootstrap(
        &mut self,
        m: &mut Space,
        environment: &mut impl Environment,
    ) -> Result<(), String> {
        if self.pc != 0x4279ec
            || !stack::active(m)
            || stack::has_pending_call(m)
            || stack::sp(m) != self.base
            || stack::reg(m, Reg::Esi) != self.world
            || stack::reg(m, Reg::Eax) != self.runtime.jvm()
        {
            return Err("NativeVmRoot: startup bootstrap phase differs".into());
        }
        let jvm = self.runtime.jvm();
        stack::set_reg(m, Reg::Ecx, jvm);
        write(m, self.world + 0xe4, jvm);
        stack::begin_call(m, 0x4279fd, &[], None);
        self.runtime.bootstrap(m, environment)?;
        self.pc = 0x4279fd;
        Ok(())
    }
    pub fn register(
        &mut self,
        m: &mut Space,
        environment: &mut impl Environment,
    ) -> Result<(), String> {
        if self.pc != 0x4279fd
            || m.r32(self.world + 0xe4) != self.runtime.jvm()
            || !stack::active(m)
            || stack::has_pending_call(m)
            || stack::sp(m) != self.base
            || stack::reg(m, Reg::Esi) != self.world
        {
            return Err("NativeVmRoot: startup JVM owner/registry phase differs".into());
        }
        stack::begin_call(m, 0x427a02, &[], None);
        self.runtime
            .prepare_registry(m, environment)?
            .finish(m, environment)?;
        self.pc = 0x427a02;
        Ok(())
    }
    pub fn register_remaining(
        &mut self,
        m: &mut Space,
        environment: &mut impl Environment,
    ) -> Result<(), String> {
        if self.pc != 0x427a02
            || !stack::active(m)
            || stack::has_pending_call(m)
            || stack::sp(m) != self.base
            || m.r32(0x63641c) != self.runtime.jvm()
            || stack::reg(m, Reg::Esi) != self.world
        {
            return Err("NativeVmRoot: secondary catalog caller/owner differs".into());
        }
        for (target, return_pc) in [(0x46b530, 0x427a07), (0x4427e0, 0x427a0c)] {
            stack::begin_call(m, return_pc, &[], None);
            crate::native_vm_metadata::registration::secondary_catalog(m, target, environment)?;
            self.pc = return_pc;
        }
        Ok(())
    }
    pub fn runtime(&mut self) -> &mut native_vm_live::Runtime {
        &mut self.runtime
    }
    pub fn pc(&self) -> u32 {
        self.pc
    }
    pub fn caller(&self) -> stack::Frame {
        self.caller
    }
    /// Execute the remaining427980 instructions through their actual RET.
    /// World initialization, resource refresh, Config construction and settings
    /// keep their real CALL frames and remain explicit domain collaborators.
    pub fn finish(
        &mut self,
        m: &mut Space,
        e: &mut impl ContinuationEnvironment,
    ) -> Result<(), String> {
        if self.pc != 0x427a0c
            || !stack::active(m)
            || stack::has_pending_call(m)
            || stack::sp(m) != self.base
            || stack::reg(m, Reg::Esi) != self.world
            || stack::reg(m, Reg::Edi) != 0
            || m.r32(self.world + 0xe4) != self.runtime.jvm()
        {
            return Err("NativeVmRoot: retained resource/world suffix required".into());
        }
        for offset in [0xc4, 0xc8, 0xcc, 0xd0, 0xd4, 0xd8] {
            write(m, self.world + offset, 0);
        }
        stack::set_reg(m, Reg::Ecx, m.r32(0x617650));
        stack::begin_call(m, 0x427a3b, &[], None);
        e.initialize_world(m)?;
        returned(m, self.base)?;
        for (name, local, kind, reference, lookup_return, bind_return) in [
            (0x60cb04, 3, 14, 0x636328, 0x427a50, 0x427a5b),
            (0x60cb10, 12, 12, 0x62f250, 0x427a70, 0x427a7b),
            (0x60cb1c, 11, 13, 0x636318, 0x427a90, 0x427a9b),
            (0x60cb28, 12, 12, 0x636450, 0x427ab0, 0x427abb),
        ] {
            stack::set_reg(m, Reg::Ecx, m.r32(0x618d48));
            stack::push(m, 0);
            stack::push(m, kind);
            stack::push(m, local);
            resource(m, e, name, local, reference, lookup_return, bind_return)?;
        }
        let game_root = m.r32(0x63646c);
        stack::set_reg(m, Reg::Ecx, game_root);
        if game_root != 0 {
            let flags = m.r32(game_root + 0x54) | 0x10;
            stack::set_reg(m, Reg::Edx, flags);
            stack::push(m, 0x636460);
            write(m, game_root + 0x54, flags);
            stack::begin_call(m, 0x427ad8, &[], None);
            e.refresh_game_root(m)?;
            returned(m, self.base)?;
        }
        stack::set_reg(m, Reg::Ecx, m.r32(0x618d48));
        stack::push(m, 0);
        stack::push(m, 8);
        stack::push(m, 0x47);
        stack::push(m, 0x60cb34);
        for offset in [0x1c, 0x20, 0x24, 0x28] {
            write(m, stack::sp(m) + offset, 0);
        }
        let manager = m.r32(0x618d48);
        let id = stack::external(m, 0x427afd, &[], None, |m| {
            e.package_id(m, manager, 0x60cb34, 0x47)
        })?;
        stack::add_sp(m, 8);
        stack::set_reg(m, Reg::Eax, id);
        stack::push(m, id);
        stack::set_reg(m, Reg::Ecx, stack::sp(m) + 0x18);
        stack::call(m, 0x427b07, &[], None, |m| {
            crate::native_vm_metadata::registration::resources::bind(m, e)
        })?;
        stack::add_sp(m, 12);
        stack::push(m, 0x60cb40);
        stack::push(m, 0x63c55c);
        let request = stack::sp(m) + 0x14;
        stack::set_reg(m, Reg::Eax, request);
        stack::push(m, request);
        stack::push(m, 0x636460);
        stack::begin_call(m, 0x427b20, &[], None);
        e.create_config(m, &mut self.runtime)?;
        returned(m, self.base - 16)?;
        stack::set_reg(m, Reg::Edx, m.r32(self.world + 0x104));
        stack::set_reg(m, Reg::Ecx, self.world + 0xf8);
        stack::add_sp(m, 16);
        bind_config(m);
        stack::set_reg(m, Reg::Ecx, self.world);
        stack::begin_call(m, 0x427bad, &[], None);
        e.apply_settings(m, &mut self.runtime)?;
        returned(m, self.base)?;
        stack::set_reg(m, Reg::Ecx, stack::load(m, 0x18));
        if stack::reg(m, Reg::Ecx) != 0 {
            stack::set_reg(m, Reg::Eax, stack::load(m, 0xc));
            if stack::reg(m, Reg::Eax) != 0 {
                stack::set_reg(m, Reg::Ecx, stack::load(m, 0x10));
                write(m, stack::reg(m, Reg::Eax) + 4, stack::reg(m, Reg::Ecx));
            } else {
                stack::set_reg(m, Reg::Edx, stack::load(m, 0x10));
                write(m, stack::reg(m, Reg::Ecx) + 0x48, stack::reg(m, Reg::Edx));
            }
            stack::set_reg(m, Reg::Eax, stack::load(m, 0x10));
            if stack::reg(m, Reg::Eax) != 0 {
                stack::set_reg(m, Reg::Ecx, stack::load(m, 0xc));
                write(m, stack::reg(m, Reg::Eax), stack::reg(m, Reg::Ecx));
            }
        }
        for register in [Reg::Edi, Reg::Esi] {
            pop(m, register);
        }
        stack::set_reg(m, Reg::Eax, 1);
        pop(m, Reg::Ebx);
        stack::add_sp(m, 16);
        let return_pc = stack::load(m, 0);
        stack::leave(m, self.caller);
        self.pc = return_pc;
        self.returned = true;
        Ok(())
    }
    pub fn into_runtime(self) -> Result<native_vm_live::Runtime, String> {
        if !self.returned {
            return Err("NativeVmRoot: startup has not returned".into());
        }
        Ok(self.runtime)
    }
}
fn returned(m: &Space, expected_sp: u32) -> Result<(), String> {
    if stack::has_pending_call(m) || stack::sp(m) != expected_sp {
        return Err("NativeVmRoot: domain collaborator did not return through its CALL".into());
    }
    Ok(())
}
fn pop(m: &mut Space, register: Reg) {
    stack::set_reg(m, register, stack::load(m, 0));
    stack::add_sp(m, 4);
}
/// Inline427b20 resource-reference assignment retains its exact volatile GPR
/// history; it is not the546070 binding routine (its input is a raw node).
fn bind_config(m: &mut Space) {
    let node = stack::reg(m, Reg::Ecx);
    let new = stack::reg(m, Reg::Eax);
    if stack::reg(m, Reg::Edx) == new {
        return;
    }
    if stack::reg(m, Reg::Edx) != 0 {
        stack::set_reg(m, Reg::Ebx, m.r32(node));
        if stack::reg(m, Reg::Ebx) != 0 {
            stack::set_reg(m, Reg::Edx, m.r32(node + 4));
            write(m, stack::reg(m, Reg::Ebx) + 4, stack::reg(m, Reg::Edx));
        } else {
            stack::set_reg(m, Reg::Ebx, m.r32(node + 4));
            write(m, stack::reg(m, Reg::Edx) + 0x48, stack::reg(m, Reg::Ebx));
        }
        stack::set_reg(m, Reg::Edx, m.r32(node + 4));
        if stack::reg(m, Reg::Edx) != 0 {
            stack::set_reg(m, Reg::Ebx, m.r32(node));
            write(m, stack::reg(m, Reg::Edx), stack::reg(m, Reg::Ebx));
        }
        for offset in [12, 8, 0, 4] {
            write(m, node + offset, 0);
        }
    }
    if new == 0 {
        for offset in [12, 8, 0, 4] {
            write(m, node + offset, 0);
        }
    } else {
        stack::set_reg(m, Reg::Ebx, m.r32(new + 0x48));
        stack::set_reg(m, Reg::Edx, new.wrapping_add(0x44));
        if stack::reg(m, Reg::Ebx) != 0 {
            write(m, stack::reg(m, Reg::Ebx), node);
        }
        write(m, node, 0);
        stack::set_reg(m, Reg::Ebx, m.r32(stack::reg(m, Reg::Edx).wrapping_add(4)));
        write(m, node + 4, stack::reg(m, Reg::Ebx));
        write(m, stack::reg(m, Reg::Edx).wrapping_add(4), node);
        let owner = if stack::reg(m, Reg::Edx) == 0 { 0 } else { new };
        stack::set_reg(m, Reg::Edx, owner);
        write(m, node + 12, owner);
        stack::set_reg(m, Reg::Eax, m.r32(new.wrapping_add(0x50)));
        write(m, node + 8, stack::reg(m, Reg::Eax));
    }
}
fn resource(
    m: &mut Space,
    e: &mut impl Environment,
    name: u32,
    local: u32,
    reference: u32,
    lookup_return: u32,
    bind_return: u32,
) -> Result<(), String> {
    let manager = m.r32(0x618d48);
    stack::set_reg(m, Reg::Ecx, manager);
    stack::push(m, name);
    let id = stack::external(m, lookup_return, &[], None, |m| {
        e.package_id(m, manager, name, local)
    })?;
    stack::add_sp(m, 8);
    stack::set_reg(m, Reg::Eax, id);
    stack::push(m, id);
    stack::set_reg(m, Reg::Ecx, reference);
    stack::call(m, bind_return, &[], None, |m| {
        crate::native_vm_metadata::registration::resources::bind(m, e)
    })?;
    stack::add_sp(m, 12);
    Ok(())
}
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
}
