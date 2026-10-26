//!416b00's retained caller, canonical names, signature and registry pools.
use super::*;
use crate::native_stack::{self as stack, Reg};
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
fn low(m: &mut Space, reg: Reg, value: u8) {
    s(m, reg, (r(m, reg) & !255) | u32::from(value));
}
fn pop(m: &mut Space, reg: Reg) {
    s(m, reg, stack::load(m, 0));
    stack::add_sp(m, 4);
}
fn allocate(m: &mut Space, pc: u32, n: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| a.allocate(m, n))?;
    s(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmRegistration: source allocation failure".into());
    }
    Ok(p)
}
fn reallocate(
    m: &mut Space,
    pc: u32,
    h: u32,
    blocks: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| {
        a.reallocate(m, m.r32(h), blocks * 4, (blocks + 1) * 4)
    })?;
    s(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmRegistration: source table allocation failure".into());
    }
    Ok(p)
}
fn node(m: &mut Space, registry: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let h = registry + 0x600;
    let free = m.r32(h + 16);
    s(m, Reg::Ebx, free);
    if free != 0 {
        s(m, Reg::Edx, m.r32(free));
        write(m, h + 16, r(m, Reg::Edx));
        return Ok(free);
    }
    let cap = m.r32(h + 12);
    s(m, Reg::Eax, cap);
    let handed = m.r32(h + 8);
    if handed == cap {
        let blocks = m.r32(h + 4);
        s(m, Reg::Ebx, blocks);
        s(m, Reg::Ecx, m.r32(h));
        s(m, Reg::Eax, cap + 64);
        write(m, h + 12, cap + 64);
        s(m, Reg::Eax, blocks + 1);
        write(m, h + 4, blocks + 1);
        s(m, Reg::Eax, (blocks + 1) * 4);
        stack::push(m, (blocks + 1) * 4);
        stack::push(m, m.r32(h));
        let table = reallocate(m, 0x416c1d, h, blocks, a)?;
        stack::push(m, 768);
        write(m, h, table);
        let block = allocate(m, 0x416c2d, 768, a)?;
        s(m, Reg::Edx, table);
        stack::add_sp(m, 12);
        write(m, table + blocks * 4, block);
    }
    s(m, Reg::Ecx, handed);
    s(m, Reg::Ebx, m.r32(h));
    s(m, Reg::Eax, handed + 1);
    write(m, h + 8, handed + 1);
    s(m, Reg::Eax, handed % 64);
    s(m, Reg::Edx, handed / 64);
    s(m, Reg::Ecx, m.r32(m.r32(h) + handed / 64 * 4));
    s(m, Reg::Eax, handed % 64 * 3);
    let cell = r(m, Reg::Ecx) + handed % 64 * 12;
    s(m, Reg::Ebx, cell);
    Ok(cell)
}
fn pointer(
    m: &mut Space,
    registry: u32,
    occupied: bool,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let h = registry + 0x614;
    let free = m.r32(h + 16);
    s(m, Reg::Eax, free);
    if free != 0 {
        let reg = if occupied { Reg::Edx } else { Reg::Ecx };
        s(m, reg, m.r32(free));
        write(m, h + 16, r(m, reg));
        return Ok(free);
    }
    let cap = m.r32(h + 12);
    s(m, Reg::Eax, cap);
    let handed = m.r32(h + 8);
    if handed == cap {
        let blocks = m.r32(h + 4);
        s(m, Reg::Edi, blocks);
        if occupied {
            s(m, Reg::Ecx, m.r32(h));
        }
        s(m, Reg::Eax, cap + 64);
        write(m, h + 12, cap + 64);
        s(m, Reg::Eax, blocks + 1);
        if !occupied {
            s(m, Reg::Edx, (blocks + 1) * 4);
        }
        write(m, h + 4, blocks + 1);
        if occupied {
            s(m, Reg::Eax, (blocks + 1) * 4);
        } else {
            s(m, Reg::Eax, m.r32(h));
        }
        stack::push(m, (blocks + 1) * 4);
        stack::push(m, m.r32(h));
        let table = reallocate(m, if occupied { 0x416cbf } else { 0x416d80 }, h, blocks, a)?;
        stack::push(m, 256);
        write(m, h, table);
        let block = allocate(m, if occupied { 0x416ccf } else { 0x416d90 }, 256, a)?;
        s(m, if occupied { Reg::Edx } else { Reg::Ecx }, table);
        stack::add_sp(m, 12);
        write(m, table + blocks * 4, block);
    }
    s(m, Reg::Ecx, handed);
    s(m, if occupied { Reg::Eax } else { Reg::Edx }, handed + 1);
    write(m, h + 8, handed + 1);
    s(m, Reg::Eax, handed / 64);
    s(m, Reg::Edx, m.r32(h));
    s(m, Reg::Ecx, handed % 64);
    let cell = m.r32(m.r32(h) + handed / 64 * 4) + handed % 64 * 4;
    s(m, Reg::Eax, cell);
    Ok(cell)
}
pub fn register(m: &mut Space, a: &mut impl Allocator) -> Result<u32, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRegistration: actual pending416b00 CALL required".into());
    }
    let jvm = r(m, Reg::Ecx);
    let args = std::array::from_fn::<_, 4, _>(|i| stack::argument(m, i));
    if jvm == 0 || m.r32(jvm + 12) == 0 || m.r32(jvm + 0x68) == 0 {
        return Err("NativeVmRegistration: source JVM registry/name owner missing".into());
    }
    for source in &args[..3] {
        bytes(m, *source)?;
    }
    for h in [m.r32(jvm + 12) + 0x600, m.r32(jvm + 12) + 0x614] {
        if m.r32(h + 8) > m.r32(h + 12)
            || m.r32(h + 12) > i32::MAX as u32 - 64
            || m.r32(h + 12) != m.r32(h + 4) * 64
        {
            return Err("NativeVmRegistration: malformed source pool".into());
        }
    }
    let frame = stack::enter(m, &args, Some(jvm));
    for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
        stack::push_reg(m, reg);
    }
    stack::push(m, 24);
    s(m, Reg::Ebp, jvm);
    let record = allocate(m, 0x416b0d, 24, a)?;
    s(m, Reg::Edi, args[1]);
    s(m, Reg::Ebx, args[0]);
    s(m, Reg::Esi, record);
    stack::add_sp(m, 4);
    write(m, record, 0x5f08fc);
    write(m, record + 4, jvm);
    for (pc, source, offset) in [(0x416b30, args[0], 12), (0x416b3c, args[1], 16)] {
        let holder = m.r32(jvm + 0x68);
        s(m, Reg::Ecx, holder);
        let canonical = stack::call(m, pc, &[source], Some(holder), |m| {
            crate::native_vm_signature::canonical_with_allocator(m, holder, source, a)
        })?;
        if offset == 16 {
            stack::push(m, 12);
        }
        write(m, record + offset, canonical);
    }
    let signature = allocate(m, 0x416b46, 12, a)?;
    stack::add_sp(m, 4);
    s(m, Reg::Ecx, args[2]);
    stack::push(m, args[2]);
    stack::push(m, jvm);
    s(m, Reg::Ecx, signature);
    let signature = stack::call(m, 0x416b5a, &[], Some(signature), |m| {
        crate::native_vm_signature::construct(m, signature, jvm, args[2], a)
    })?;
    stack::add_sp(m, 8);
    s(m, Reg::Edx, args[3]);
    write(m, record + 20, signature);
    write(m, record + 8, args[3]);
    stack::store(m, 0x18, record);
    low(m, Reg::Ecx, m.r8(args[1]));
    s(m, Reg::Eax, 0);
    s(m, Reg::Edx, args[1]);
    let mut source = args[1];
    while m.r8(source) != 0 {
        s(m, Reg::Ecx, i32::from(m.r8(source) as i8) as u32);
        s(m, Reg::Eax, r(m, Reg::Eax).wrapping_add(r(m, Reg::Ecx)));
        low(m, Reg::Ecx, m.r8(source + 1));
        source += 1;
        s(m, Reg::Edx, source);
    }
    s(m, Reg::Edx, r(m, Reg::Eax).wrapping_mul(9));
    s(
        m,
        Reg::Edi,
        r(m, Reg::Eax).wrapping_add(r(m, Reg::Edx).wrapping_mul(8)),
    );
    low(m, Reg::Eax, m.r8(args[0]));
    s(m, Reg::Ecx, args[0]);
    source = args[0];
    while m.r8(source) != 0 {
        s(m, Reg::Eax, i32::from(m.r8(source) as i8) as u32);
        s(m, Reg::Edi, r(m, Reg::Edi).wrapping_add(r(m, Reg::Eax)));
        low(m, Reg::Eax, m.r8(source + 1));
        source += 1;
        s(m, Reg::Ecx, source);
    }
    let registry = m.r32(jvm + 12);
    s(m, Reg::Esi, registry);
    s(m, Reg::Eax, r(m, Reg::Edi) & 127);
    s(m, Reg::Ecx, r(m, Reg::Eax) * 3);
    let bucket = registry + r(m, Reg::Ecx) * 4;
    s(m, Reg::Eax, m.r32(bucket + 4));
    s(m, Reg::Ebp, bucket);
    if r(m, Reg::Eax) == 0 {
        write(m, bucket, r(m, Reg::Edi));
        let slot = pointer(m, registry, false, a)?;
        s(m, Reg::Ecx, record);
        pop(m, Reg::Edi);
        write(m, slot, record);
        write(m, bucket + 4, slot);
        pop(m, Reg::Esi);
        write(m, bucket + 8, 0);
    } else {
        let node = node(m, registry, a)?;
        write(m, node, r(m, Reg::Edi));
        let slot = pointer(m, registry, true, a)?;
        s(m, Reg::Ecx, record);
        write(m, slot, record);
        write(m, node + 4, slot);
        s(m, Reg::Edx, m.r32(bucket + 8));
        pop(m, Reg::Edi);
        write(m, node + 8, r(m, Reg::Edx));
        pop(m, Reg::Esi);
        write(m, bucket + 8, node);
    }
    pop(m, Reg::Ebp);
    s(m, Reg::Eax, 1);
    pop(m, Reg::Ebx);
    stack::leave(m, frame);
    Ok(1)
}

#[must_use]
pub struct RegistryBootstrap {
    pub pc: u32,
    pub caller: stack::Frame,
}
#[path = "native_vm_registry_catalog.rs"]
mod catalog;
///487f20's native declarations, preserving the original interleaving of each
/// global ECX load and its four argument PUSHes. The resource suffix retains
/// this actual caller at489e20; registration never invokes an implementation.
pub fn begin_catalog(m: &mut Space, a: &mut impl Allocator) -> Result<RegistryBootstrap, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRegistration: actual pending487f20 CALL required".into());
    }
    let jvm = m.r32(0x63641c);
    if jvm == 0 || m.r32(jvm + 12) == 0 || m.r32(jvm + 0x68) == 0 {
        return Err("NativeVmRegistration: published source JVM is missing".into());
    }
    let caller = stack::enter_cdecl(m, &[]);
    for (pc, args, ecx_after) in catalog::DECLARATIONS {
        for index in 0..4 {
            if index == ecx_after {
                s(m, Reg::Ecx, m.r32(0x63641c));
            }
            stack::push(m, args[3 - index]);
        }
        if ecx_after == 4 {
            s(m, Reg::Ecx, m.r32(0x63641c));
        }
        stack::begin_call(m, pc, &[], None);
        register(m, a)?;
    }
    Ok(RegistryBootstrap {
        pc: 0x489e20,
        caller,
    })
}

#[cfg(test)]
#[path = "native_vm_registry_resources_tests.rs"]
mod resource_tests;
#[path = "native_vm_registry_resources.rs"]
pub mod resources;

#[path = "native_vm_registry_secondary_catalog.rs"]
mod secondary_catalog;
/// Remaining two startup registration callers. Only their original static
/// instruction streams are selected; the implementation addresses are inputs
/// to registration, never evidence that a native may execute.
pub fn secondary_catalog(m: &mut Space, target: u32, a: &mut impl Allocator) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRegistration: actual secondary catalog CALL required".into());
    }
    let entries: &[(u32, [u32; 4], usize)] = match target {
        0x46b530 => &secondary_catalog::VEHICLE,
        0x4427e0 => &secondary_catalog::PART,
        _ => return Err("NativeVmRegistration: unsupported static catalog caller".into()),
    };
    let jvm = m.r32(0x63641c);
    if jvm == 0 || m.r32(jvm + 12) == 0 || m.r32(jvm + 0x68) == 0 {
        return Err("NativeVmRegistration: secondary catalog JVM owner absent".into());
    }
    let caller = stack::enter_cdecl(m, &[]);
    for &(pc, args, ecx_after) in entries {
        for index in 0..4 {
            if index == ecx_after {
                s(m, Reg::Ecx, m.r32(0x63641c));
            }
            stack::push(m, args[3 - index]);
        }
        if ecx_after == 4 {
            s(m, Reg::Ecx, m.r32(0x63641c));
        }
        stack::begin_call(m, pc, &[], None);
        register(m, a)?;
    }
    stack::leave(m, caller);
    Ok(())
}
