//! Source ResourceRef native bodies, with actual marshalling and field owners.
//! Runtime admission still requires a source-produced, validated native Entry.
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
fn low(m: &mut Space, reg: Reg, value: u8) {
    s(m, reg, (r(m, reg) & !255) | u32::from(value));
}
///45d910's zero/one-integer paths write the actual receiver and argument words.
fn arguments(
    m: &mut Space,
    info: u32,
    destination: u32,
    integer: Option<u32>,
) -> Result<(), String> {
    let args = if let Some(integer) = integer {
        vec![info, destination, integer]
    } else {
        vec![info, destination]
    };
    stack::function_cdecl(m, &args, |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, info);
        let vector = m.r32(info + 8);
        s(m, Reg::Eax, vector);
        let last = m.r32(vector + 8).wrapping_sub(1);
        s(m, Reg::Ecx, last);
        s(m, Reg::Edx, m.r32(vector));
        s(m, Reg::Ebx, 0);
        stack::push_reg(m, Reg::Edi);
        stack::store(m, 0x10, last);
        if (r(m, Reg::Edx) as i32) <= (last as i32) {
            return Err("NativeVmResource: missing receiver argument owner".into());
        }
        s(m, Reg::Eax, m.r32(vector + 4));
        let cell = m.r32(r(m, Reg::Eax) + last * 4);
        s(m, Reg::Eax, cell);
        s(m, Reg::Ecx, destination);
        if destination != 0 {
            s(m, Reg::Edx, m.r32(cell + 8));
            m.w32(destination, r(m, Reg::Edx));
        }
        let method = m.r32(info + 0xc);
        s(m, Reg::Eax, method);
        let signature = m.r32(method + 0x14);
        s(m, Reg::Ecx, signature);
        let text = stack::call(m, 0x45d94c, &[], Some(signature), |m| {
            stack::function(m, &[], Some(signature), |m| {
                stack::alloc(m, 0x400);
                stack::push_reg(m, Reg::Ebx);
                s(m, Reg::Ebx, signature);
                let text = m.r32(signature + 4);
                s(m, Reg::Eax, text);
                if text == 0 {
                    return Err("NativeVmResource: absent signature text".into());
                }
                Ok::<u32, String>(text)
            })
        })?;
        s(m, Reg::Edi, text);
        low(m, Reg::Ecx, m.r8(text + 1));
        s(m, Reg::Edi, text + 1);
        if m.r8(text + 1) != b')' {
            let integer = integer.ok_or("NativeVmResource: missing native argument destination")?;
            if m.r8(text + 1) != b'I' || m.r8(text + 2) != b')' || last == 0 {
                return Err("NativeVmResource: unproved marshalling parameter grammar".into());
            }
            stack::push_reg(m, Reg::Ebp);
            s(m, Reg::Ebp, stack::sp(m) + 0x18);
            s(m, Reg::Edx, stack::load(m, 0x14));
            s(m, Reg::Eax, r(m, Reg::Edx));
            s(m, Reg::Edx, r(m, Reg::Edx) - 1);
            stack::store(m, 0x14, r(m, Reg::Edx));
            s(m, Reg::Eax, vector);
            let index = r(m, Reg::Edx);
            if index >= m.r32(vector) {
                return Err("NativeVmResource: native argument outside vector owner".into());
            }
            s(m, Reg::Eax, m.r32(vector + 4));
            let cell = m.r32(r(m, Reg::Eax) + index * 4);
            s(m, Reg::Eax, cell);
            s(m, Reg::Ecx, 3);
            s(m, Reg::Edx, 0);
            s(m, Reg::Ecx, integer);
            s(m, Reg::Edx, m.r32(cell + 8));
            s(m, Reg::Ebp, r(m, Reg::Ebp) + 4);
            m.w32(integer, r(m, Reg::Edx));
            low(m, Reg::Ecx, m.r8(text + 2));
            s(m, Reg::Edi, text + 2);
        }
        Ok(())
    })
}
fn field(m: &mut Space, object: u32, index: u32, value: Option<u32>) -> Result<u32, String> {
    let args = if let Some(value) = value {
        vec![object, index, value]
    } else {
        vec![object, index]
    };
    stack::function_cdecl(m, &args, |m| {
        stack::alloc(m, 0x20);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, index);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, object);
        let owner = m.r32(object + 0x10);
        s(m, Reg::Ecx, owner);
        let cell = stack::call(
            m,
            if value.is_some() { 0x42a9f6 } else { 0x42ab66 },
            &[index],
            Some(owner),
            |m| crate::native_vm_lookup::variable_at(m, owner, index),
        )?;
        if cell == 0 {
            return Err("NativeVmResource: missing native pointer field".into());
        }
        s(m, Reg::Ecx, m.r32(cell + 4));
        if m.r8(r(m, Reg::Ecx)) != b'I' {
            return Err("NativeVmResource: pointer field is not original integer storage".into());
        }
        if let Some(value) = value {
            s(m, Reg::Edx, value);
            m.w32(cell + 8, value);
        } else {
            s(m, Reg::Eax, m.r32(cell + 8));
        }
        Ok(r(m, Reg::Eax))
    })
}
///47cea0 ResourceRef.newNative, including45d910 and42ab50/42a9e0.
pub fn new_native(
    m: &mut Space,
    info: u32,
    env: &mut impl crate::native_vm_pool::Allocator,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[info], |m| {
        s(m, Reg::Ecx, info);
        let destination = stack::sp(m) + 4;
        s(m, Reg::Eax, destination);
        stack::push(m, destination);
        stack::push(m, info);
        stack::call(m, 0x47ceaf, &[], None, |m| {
            arguments(m, info, destination, None)
        })?;
        let index = m.r32(0x62e008);
        s(m, Reg::Edx, index);
        let object = m.r32(stack::sp(m) + 0xc);
        s(m, Reg::Eax, object);
        stack::push(m, index);
        stack::push(m, object);
        let pointer = stack::call(m, 0x47cec0, &[], None, |m| field(m, object, index, None))?;
        stack::add_sp(m, 0x10);
        if pointer != 0 {
            return Ok(pointer);
        }
        stack::push(m, 0x10);
        let node = stack::external(m, 0x47cece, &[], None, |m| env.allocate(m, 0x10))?;
        s(m, Reg::Eax, node);
        s(m, Reg::Ecx, 0);
        stack::add_sp(m, 4);
        if node == 0 {
            return Err("NativeVmResource: native node allocation failed".into());
        }
        for off in [0, 4, 8, 12] {
            m.w32(node + off, 0);
        }
        let index = m.r32(0x62e008);
        s(m, Reg::Ecx, index);
        let object = m.r32(stack::sp(m) + 4);
        s(m, Reg::Edx, object);
        stack::push(m, node);
        stack::push(m, index);
        stack::push(m, object);
        let result = stack::call(m, 0x47cef8, &[], None, |m| {
            field(m, object, index, Some(node))
        })?;
        stack::add_sp(m, 12);
        Ok(result)
    })
}

///47cf80 ResourceRef.set(I), including first binding through536820.
pub fn set_id(
    m: &mut Space,
    info: u32,
    env: &mut impl crate::native_vm_thread_run::Environment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[info], |m| {
        stack::push_reg(m, Reg::Ecx);
        s(m, Reg::Edx, info);
        let integer = stack::sp(m);
        s(m, Reg::Eax, integer);
        stack::push(m, integer);
        let destination = stack::sp(m) + 0xc;
        s(m, Reg::Ecx, destination);
        stack::push(m, destination);
        stack::push(m, info);
        stack::call(m, 0x47cf95, &[], None, |m| {
            arguments(m, info, destination, Some(integer))
        })?;
        let index = m.r32(0x62e008);
        s(m, Reg::Eax, index);
        let object = m.r32(stack::sp(m) + 0x14);
        s(m, Reg::Ecx, object);
        stack::push(m, index);
        stack::push(m, object);
        let node = stack::call(m, 0x47cfa5, &[], None, |m| field(m, object, index, None))?;
        stack::add_sp(m, 0x14);
        if node != 0 {
            let id = m.r32(stack::sp(m));
            s(m, Reg::Edx, id);
            s(m, Reg::Ecx, node);
            stack::call(m, 0x47cfb8, &[id], Some(node), |m| {
                stack::function(m, &[id], Some(node), |m| {
                    stack::push_reg(m, Reg::Esi);
                    s(m, Reg::Esi, node);
                    s(m, Reg::Eax, m.r32(node + 8));
                    stack::push_reg(m, Reg::Edi);
                    s(m, Reg::Edi, id);
                    if m.r32(node + 8) != id {
                        let old = m.r32(node + 0xc);
                        s(m, Reg::Ecx, old);
                        if old != 0 {
                            return Err("NativeVmResource: bound resource replacement continuation unavailable".into());
                        }
                        if id != 0 {
                            let manager = m.r32(0x618d48);
                            s(m, Reg::Ecx, manager);
                            let resource =
                                stack::external(m, 0x546028, &[id, 0, 0], Some(manager), |m| {
                                    env.resolve_resource(m, manager, id, 0, 0)
                                })?;
                            s(m, Reg::Eax, resource);
                            if resource != 0 {
                                s(m, Reg::Eax, resource + 0x44);
                                m.w32(node + 8, id);
                                let head = m.r32(resource + 0x48);
                                s(m, Reg::Ecx, head);
                                if head != 0 {
                                    m.w32(head, node);
                                }
                                m.w32(node, 0);
                                s(m, Reg::Edx, m.r32(resource + 0x48));
                                m.w32(node + 4, r(m, Reg::Edx));
                                m.w32(resource + 0x48, node);
                                s(m, Reg::Eax, resource);
                                m.w32(node + 0xc, resource);
                            }
                        }
                    }
                    Ok::<(), String>(())
                })
            })?;
        }
        s(m, Reg::Ecx, stack::load(m, 0));
        stack::add_sp(m, 4);
        Ok(r(m, Reg::Eax))
    })
}
