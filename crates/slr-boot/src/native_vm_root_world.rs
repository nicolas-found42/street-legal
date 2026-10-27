//! Source4a5580 world initialization caller. Primitive-body and root-entity
//! construction remain explicit source collaborators with actual pending CALLs.
use super::{pop, returned, write};
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;

pub trait Environment {
    fn body(&mut self, m: &mut Space) -> Result<(), String>;
    fn entity(&mut self, m: &mut Space) -> Result<(), String>;
}

/// Consume the actual4a5580 CALL. Neither incoming world storage nor its child
/// bodies are reconstructed from semantic identities or saved machine pages.
/// exe: FUN_004a5580
pub fn initialize(m: &mut Space, e: &mut impl Environment) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRootWorld: actual world initialization CALL required".into());
    }
    let world = stack::reg(m, Reg::Ecx);
    if world == 0 || m.r32(0x617650) != world {
        return Err("NativeVmRootWorld: source world owner differs".into());
    }
    let caller = stack::enter(m, &[], Some(world));
    initialize_body(m, world, e)?;
    stack::leave(m, caller);
    Ok(())
}

/// Shared entered body for the established physics API and strict root caller.
/// Direct physics tests retain their existing documented driver; raw startup
/// enters exclusively through the pending-CALL validation above.
pub(crate) fn initialize_body(
    m: &mut Space,
    world: u32,
    e: &mut impl Environment,
) -> Result<(), String> {
    stack::alloc(m, 0x3c);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, world);
    stack::set_reg(m, Reg::Eax, 0x3ca3d70a);
    write(m, world + 12, 0x3ca3d70a);
    write(m, world + 8, 0x3ca3d70a);
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Edi, 0);
    for offset in [
        0, 4, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x20,
    ] {
        write(m, world + offset, 0);
    }
    stack::set_reg(m, Reg::Eax, 1);
    for (offset, value) in [
        (0x78, 1),
        (0x7c, 1),
        (0x104, 0),
        (0x108, 0xc11cf5c3),
        (0x10c, 0),
        (0x1b4, 0x3fa57a78),
        (0x80, 0),
        (0x1c4, 0),
        (0x1c8, 0xbf800000),
        (0x1cc, 0),
        (0x1b8, 0),
        (0x1bc, 0xc0e00000),
        (0x1c0, 0),
        (0x1d8, 0),
        (0x1dc, 0),
        (0x1e0, 0),
    ] {
        write(m, world + offset, value);
    }
    let base = stack::sp(m);
    let rotation = base + 0x14;
    stack::set_reg(m, Reg::Ecx, rotation);
    for (offset, value) in [
        (0x1e4, 0x43960000),
        (0x1e8, 0x44098000),
        (0x1d4, 0),
        (0x1d0, 0),
    ] {
        write(m, world + offset, value);
    }
    for offset in [8, 12, 16] {
        write(m, base + offset, 0);
    }
    stack::begin_call(m, 0x4a565b, &[], None);
    identity(m);
    for (index, offset, body_return, entity_return) in
        [(0, 0x84, 0x4a5672, 0x4a567a), (1, 0x88, 0x4a56a4, 0x4a56ac)]
    {
        stack::set_reg(m, if index == 0 { Reg::Eax } else { Reg::Edx }, rotation);
        stack::push(m, rotation);
        stack::set_reg(m, if index == 0 { Reg::Ecx } else { Reg::Eax }, base + 8);
        stack::push(m, base + 8);
        for _ in 0..6 {
            stack::push(m, 0);
        }
        stack::set_reg(m, Reg::Ecx, world);
        stack::begin_call(m, body_return, &[], None);
        e.body(m)?;
        returned(m, base - 8)?;
        stack::push(m, stack::reg(m, Reg::Eax));
        stack::set_reg(m, Reg::Ecx, world);
        stack::begin_call(m, entity_return, &[], None);
        e.entity(m)?;
        returned(m, base)?;
        let entity = stack::reg(m, Reg::Eax);
        write(m, world + offset, entity);
        let body = m.r32(entity + 0x5c);
        stack::set_reg(m, Reg::Eax, body);
        write(m, body + 0x9c, m.r32(body + 0x9c) | 0x100);
    }
    stack::set_reg(m, Reg::Ecx, world + 0x200);
    stack::set_reg(m, Reg::Edx, world + 0x1f0);
    stack::set_reg(m, Reg::Esi, world + 0x1f8);
    pop(m, Reg::Edi);
    write(m, 0x62dcac, world + 0x1f8);
    write(m, 0x62dcb0, world + 0x200);
    write(m, 0x62dca8, world + 0x1f0);
    pop(m, Reg::Esi);
    stack::add_sp(m, 0x3c);
    Ok(())
}

/// Reached54e1d0 writes nine identity words, preserving the three padding gaps.
fn identity(m: &mut Space) {
    let matrix = stack::reg(m, Reg::Ecx);
    let caller = stack::enter(m, &[], Some(matrix));
    stack::set_reg(m, Reg::Eax, 0);
    stack::set_reg(m, Reg::Edx, 0x3f800000);
    for (offset, value) in [
        (0, 0x3f800000),
        (4, 0),
        (8, 0),
        (16, 0),
        (20, 0x3f800000),
        (24, 0),
        (32, 0),
        (36, 0),
        (40, 0x3f800000),
    ] {
        write(m, matrix + offset, value);
    }
    stack::leave(m, caller);
}
