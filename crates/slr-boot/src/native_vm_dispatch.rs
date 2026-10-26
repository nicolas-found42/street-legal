//! Source-derived raw41fbc0 dispatch continuations.
//!
//! The admitted composed path has a cached zero-parameter signature: entry,
//! native overload selection, receiver/CallInfo construction, native execution
//! and result ownership/return. Callback creation, Thread.run, uncached metadata
//! and argument conversion remain prerequisites. Production's ABI guard stays.
use crate::native_stack::{self as stack, Frame, Reg};
use slr_physics::space::Space;

#[allow(dead_code)]
pub struct PendingDispatch {
    pub frame: Frame,
    pub thread: u32,
    pub method: u32,
    pub signature: u32,
    pub pc: u32,
}

/// Execute the shared prologue from the actual pending CALL. No input stack
/// image or register template is accepted. Invalid/uncached inputs fail before
/// the caller is consumed or any retained memory is changed.
#[allow(dead_code)]
pub fn begin_cached(m: &mut Space) -> Result<PendingDispatch, String> {
    begin_inner(m, None)
}
#[allow(dead_code)]
pub fn begin_materialize(
    m: &mut Space,
    a: &mut dyn crate::native_vm_pool::Allocator,
) -> Result<PendingDispatch, String> {
    begin_inner(m, Some(a))
}
fn begin_inner(
    m: &mut Space,
    mut allocator: Option<&mut dyn crate::native_vm_pool::Allocator>,
) -> Result<PendingDispatch, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmDispatch: actual pending caller is required".into());
    }
    let thread = stack::reg(m, Reg::Ecx);
    let args = [
        stack::argument(m, 0),
        stack::argument(m, 1),
        stack::argument(m, 2),
    ];
    let class = m.r32(args[0]);
    let method = m.r32(args[0].wrapping_add(8));
    let pool = m.r32(class.wrapping_add(0x14));
    let index = m.r32(method.wrapping_add(8));
    let signature = m.r32(method.wrapping_add(0x18));
    let constant = if pool != 0 && (index as i32) >= 0 && index < m.r32(pool + 8) {
        m.r32(m.r32(pool + 12).wrapping_add(index * 4))
    } else {
        0
    };
    if thread == 0
        || m.r32(thread + 0x10) == 0
        || args[0] == 0
        || class == 0
        || method == 0
        || pool == 0
        || constant == 0
        || m.r32(constant + 4) != 0
        || m.r32(constant + 8) == 0
        || (signature == 0 && allocator.is_none())
        || (m.r8(method + 4) & 8 == 0 && args[2] == 0)
    {
        return Err("NativeVmDispatch: missing cached method/string/receiver context".into());
    }
    let frame = stack::enter(m, &args, Some(thread));
    stack::alloc(m, 0x3c);
    stack::set_reg(m, Reg::Eax, args[0]);
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Ebp);
    stack::set_reg(m, Reg::Ebp, class);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, method);
    stack::set_reg(m, Reg::Eax, index);
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Edi, thread);
    stack::set_reg(m, Reg::Ecx, pool);
    stack::push(m, index);
    stack::store(m, 0x14, thread);
    stack::store(m, 0x1c, method);
    //406140's ordinary in-range string constant branch. The source updates
    //ECX twice as well as EAX, which matters to the following caller.
    stack::call(m, 0x41fbe6, &[], Some(pool), |m| {
        stack::function(m, &[index], Some(pool), |m| {
            stack::set_reg(m, Reg::Eax, index);
            stack::set_reg(m, Reg::Ecx, m.r32(pool + 12));
            stack::set_reg(m, Reg::Eax, constant);
            stack::set_reg(m, Reg::Ecx, m.r32(constant + 4));
            stack::set_reg(m, Reg::Eax, m.r32(constant + 8));
        })
    });
    stack::add_sp(m, 4);
    let name = stack::reg(m, Reg::Eax);
    stack::set_reg(m, Reg::Ebx, name);
    stack::store(m, 0x20, name);
    //407ac0 cached branch. Preserve its PUSH ESI and the caller's two actual
    //arguments, rather than directly assigning the semantic signature.
    let jvm = m.r32(thread + 0x10);
    stack::set_reg(m, Reg::Edx, pool);
    stack::set_reg(m, Reg::Eax, jvm);
    let signature = stack::call(m, 0x41fc27, &[jvm, pool], Some(method), |m| {
        if let Some(a) = allocator.as_mut() {
            crate::native_vm_signature::method(m, method, jvm, pool, *a)
        } else {
            stack::function(m, &[jvm, pool], Some(method), |m| {
                stack::push_reg(m, Reg::Esi);
                stack::set_reg(m, Reg::Esi, method);
                stack::set_reg(m, Reg::Eax, signature);
                Ok(signature)
            })
        }
    })?;
    stack::set_reg(m, Reg::Edx, signature);
    stack::store(m, 0x50, signature);
    Ok(PendingDispatch {
        frame,
        thread,
        method,
        signature,
        pc: 0x41fc31,
    })
}

#[cfg(test)]
#[path = "native_vm_dispatch_tests.rs"]
mod tests;

fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
fn low(m: &mut Space, reg: Reg, value: u8) {
    s(m, reg, (r(m, reg) & !255) | u32::from(value));
}
fn signed(byte: u8) -> u32 {
    i32::from(byte as i8) as u32
}

// Ordinary source406f60/406140 lookups. The descriptor/record addresses come
// directly from the declaring module's original pool, including its indices.
fn class_name(m: &mut Space, class: u32, class_pc: u32, string_pc: u32) -> Result<u32, String> {
    let module = m.r32(class + 4);
    let pool = m.r32(module + 0x24);
    let index = m.r32(module + 0x3c);
    if module == 0 || pool == 0 || (index as i32) < 0 || index >= m.r32(pool + 8) {
        return Err("NativeVmDispatch: malformed declaring module".into());
    }
    let constant = m.r32(m.r32(pool + 12) + index * 4);
    if constant == 0 || m.r32(constant + 4) != 4 {
        return Err("NativeVmDispatch: declaring class constant is unavailable".into());
    }
    let name_index = m.r32(constant + 8);
    if (name_index as i32) < 0 || name_index >= m.r32(pool + 8) {
        return Err("NativeVmDispatch: malformed declaring class name index".into());
    }
    let name_constant = m.r32(m.r32(pool + 12) + name_index * 4);
    if name_constant == 0 || m.r32(name_constant + 4) != 0 || m.r32(name_constant + 8) == 0 {
        return Err("NativeVmDispatch: declaring class name is unavailable".into());
    }
    s(m, Reg::Esi, module);
    s(m, Reg::Ecx, index);
    stack::call(m, class_pc, &[index], Some(pool), |m| {
        stack::function(m, &[index], Some(pool), |m| {
            s(m, Reg::Eax, index);
            s(m, Reg::Ecx, m.r32(pool + 12));
            s(m, Reg::Eax, constant);
            s(m, Reg::Eax, name_index);
        })
    });
    stack::call(m, string_pc, &[name_index], Some(pool), |m| {
        stack::function(m, &[name_index], Some(pool), |m| {
            s(m, Reg::Eax, name_index);
            s(m, Reg::Ecx, m.r32(pool + 12));
            s(m, Reg::Eax, name_constant);
            s(m, Reg::Ecx, 0);
            s(m, Reg::Eax, m.r32(name_constant + 8));
        })
    });
    Ok(r(m, Reg::Eax))
}
fn registry_first(m: &mut Space, registry: u32, hash: u32, hash_ptr: u32) -> u32 {
    stack::call(m, 0x41fc9b, &[hash_ptr], Some(registry), |m| {
        stack::function(m, &[hash_ptr], Some(registry), |m| {
            s(m, Reg::Eax, hash_ptr);
            s(m, Reg::Edx, hash);
            let mut node = registry + (hash & 127) * 12;
            s(m, Reg::Eax, node);
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, m.r32(node + 4));
            if r(m, Reg::Esi) != 0 {
                for _ in 0..1024 {
                    if node == 0 {
                        break;
                    }
                    if m.r32(node) == hash {
                        m.w32(registry + 0x628, node);
                        s(m, Reg::Eax, m.r32(node + 4));
                        return r(m, Reg::Eax);
                    }
                    node = m.r32(node + 8);
                    s(m, Reg::Eax, node);
                }
            }
            s(m, Reg::Eax, 0);
            0
        })
    })
}
fn registry_next(m: &mut Space, registry: u32) -> u32 {
    stack::call(m, 0x41fd51, &[], Some(registry), |m| {
        stack::function(m, &[], Some(registry), |m| {
            let current = m.r32(registry + 0x628);
            s(m, Reg::Edx, current);
            let mut node = m.r32(current + 8);
            s(m, Reg::Eax, node);
            if node != 0 {
                let hash = m.r32(current);
                s(m, Reg::Edx, hash);
                for _ in 0..1024 {
                    if node == 0 {
                        break;
                    }
                    if m.r32(node) == hash {
                        m.w32(registry + 0x628, node);
                        s(m, Reg::Eax, m.r32(node + 4));
                        return r(m, Reg::Eax);
                    }
                    node = m.r32(node + 8);
                    s(m, Reg::Eax, node);
                }
            }
            s(m, Reg::Eax, 0);
            0
        })
    })
}
// The two original inline strcmp loops use different source registers. Retain
// their byte-register writes, pointer increments, and unsigned comparison.
fn compare_class(m: &mut Space, mut left: u32, mut right: u32) -> Result<bool, String> {
    s(m, Reg::Ecx, left);
    s(m, Reg::Esi, right);
    for _ in 0..1024 {
        for offset in 0..2 {
            let a = m.r8(left + offset);
            let b = m.r8(right + offset);
            low(m, Reg::Edx, a);
            low(m, Reg::Eax, a);
            if a != b {
                s(m, Reg::Eax, if a < b { u32::MAX } else { 1 });
                return Ok(false);
            }
            if offset == 0 && a == 0 {
                s(m, Reg::Eax, 0);
                return Ok(true);
            }
            if offset == 1 {
                left += 2;
                right += 2;
                s(m, Reg::Ecx, left);
                s(m, Reg::Esi, right);
                if a == 0 {
                    s(m, Reg::Eax, 0);
                    return Ok(true);
                }
            }
        }
    }
    Err("NativeVmDispatch: unterminated native class name".into())
}
fn compare_method(m: &mut Space, mut left: u32, mut right: u32) -> Result<bool, String> {
    s(m, Reg::Eax, left);
    s(m, Reg::Esi, right);
    for _ in 0..1024 {
        for offset in 0..2 {
            let a = m.r8(left + offset);
            let b = m.r8(right + offset);
            low(m, Reg::Edx, a);
            low(m, Reg::Ecx, a);
            if a != b {
                s(m, Reg::Eax, if a < b { u32::MAX } else { 1 });
                return Ok(false);
            }
            if offset == 0 && a == 0 {
                s(m, Reg::Eax, 0);
                return Ok(true);
            }
            if offset == 1 {
                left += 2;
                right += 2;
                s(m, Reg::Eax, left);
                s(m, Reg::Esi, right);
                if a == 0 {
                    s(m, Reg::Eax, 0);
                    return Ok(true);
                }
            }
        }
    }
    Err("NativeVmDispatch: unterminated native method name".into())
}
/// Continue source41fc31's native branch through registry overload selection.
/// Signature equality is canonical-pointer equality, exactly41ded0; equal
/// descriptor bytes at different addresses do not establish identity.
#[allow(dead_code)]
pub fn select_native(
    m: &mut Space,
    mut pending: PendingDispatch,
) -> Result<PendingDispatch, String> {
    if !stack::active(m)
        || stack::has_pending_call(m)
        || pending.pc != 0x41fc31
        || m.r8(pending.method + 4) & 0x40 == 0
    {
        return Err("NativeVmDispatch: missing retained native selection caller".into());
    }
    let class = r(m, Reg::Ebp);
    let method_name = r(m, Reg::Ebx);
    let name = class_name(m, class, 0x41fc46, 0x41fc4f)?;
    low(m, Reg::Edx, m.r8(method_name));
    s(m, Reg::Ecx, 0);
    s(m, Reg::Edi, name);
    s(m, Reg::Esi, method_name);
    let mut p = method_name;
    for _ in 0..1024 {
        let byte = m.r8(p);
        if byte == 0 {
            break;
        }
        s(m, Reg::Edx, signed(byte));
        s(m, Reg::Ecx, r(m, Reg::Ecx).wrapping_add(r(m, Reg::Edx)));
        p += 1;
        low(m, Reg::Edx, m.r8(p));
        s(m, Reg::Esi, p);
        if p == method_name + 1024 {
            return Err("NativeVmDispatch: unterminated method hash input".into());
        }
    }
    let sum = r(m, Reg::Ecx);
    s(m, Reg::Eax, sum.wrapping_mul(9));
    s(m, Reg::Edx, sum.wrapping_mul(73));
    low(m, Reg::Eax, m.r8(name));
    s(m, Reg::Ecx, name);
    p = name;
    for _ in 0..1024 {
        let byte = m.r8(p);
        if byte == 0 {
            break;
        }
        s(m, Reg::Eax, signed(byte));
        s(m, Reg::Edx, r(m, Reg::Edx).wrapping_add(r(m, Reg::Eax)));
        p += 1;
        low(m, Reg::Eax, m.r8(p));
        s(m, Reg::Ecx, p);
        if p == name + 1024 {
            return Err("NativeVmDispatch: unterminated class hash input".into());
        }
    }
    let hash = r(m, Reg::Edx);
    let b = stack::sp(m);
    let registry = m.r32(m.r32(pending.thread + 0x10) + 12);
    if registry == 0 {
        return Err("NativeVmDispatch: missing original native registry".into());
    }
    s(m, Reg::Ecx, pending.thread);
    stack::store(m, 0x1c, hash);
    s(m, Reg::Edx, m.r32(pending.thread + 0x10));
    s(m, Reg::Ecx, registry);
    s(m, Reg::Eax, b + 0x1c);
    let mut slot = registry_first(m, registry, hash, b + 0x1c);
    let mut first = true;
    for _ in 0..1024 {
        if slot == 0 {
            return Err("NativeVmDispatch: native implementation not found".into());
        }
        let record = m.r32(slot);
        s(m, Reg::Edi, record);
        stack::store(m, 0x14, record);
        if record == 0 {
            return Err("NativeVmDispatch: native registry slot is empty".into());
        }
        let (a, z) = if first {
            (0x41fcc0, 0x41fcc9)
        } else {
            (0x41fd76, 0x41fd7f)
        };
        let name = class_name(m, class, a, z)?;
        let class_matches = compare_class(m, m.r32(record + 12), name)?;
        let method_matches = class_matches && compare_method(m, m.r32(record + 16), method_name)?;
        if method_matches {
            let signature = m.r32(record + 0x14);
            s(m, Reg::Eax, pending.signature);
            s(m, Reg::Ecx, signature);
            let pc = if first { 0x41fd3a } else { 0x41fdf4 };
            let matches = stack::call(m, pc, &[pending.signature], Some(signature), |m| {
                stack::function(m, &[pending.signature], Some(signature), |m| {
                    s(m, Reg::Eax, m.r32(signature + 4));
                    s(m, Reg::Ecx, pending.signature);
                    let equal = m.r32(signature + 4) == m.r32(pending.signature + 4);
                    s(m, Reg::Eax, u32::from(equal));
                    equal
                })
            });
            if matches {
                pending.pc = 0x41fe04;
                return Ok(pending);
            }
        }
        s(m, Reg::Ecx, pending.thread);
        s(m, Reg::Edx, m.r32(pending.thread + 0x10));
        slot = registry_next(m, registry);
        first = false;
    }
    Err("NativeVmDispatch: cyclic/oversized native registry chain".into())
}

///41fe04..420025 builds scalar argument and receiver cells in source order.
/// Unproved conversions and reference unboxing remain explicit boundaries.
#[allow(dead_code)]
pub fn prepare_arguments(
    m: &mut Space,
    mut pending: PendingDispatch,
    a: &mut impl crate::native_vm_pool::Allocator,
) -> Result<PendingDispatch, String> {
    let b = stack::sp(m);
    let record = m.r32(b + 0x14);
    let class = m.r32(b + 0x54);
    if pending.pc != 0x41fe04
        || !stack::active(m)
        || stack::has_pending_call(m)
        || class == 0
        || m.r32(class + 8) == 0
        || m.r32(m.r32(class + 8)) == 0
        || record == 0
        || m.r32(record + 8) == 0
        || m.r32(0x62f130) == 0
    {
        return Err(
            "NativeVmDispatch: cached zero-argument continuation prerequisites are missing".into(),
        );
    }
    s(m, Reg::Ecx, record);
    s(m, Reg::Edi, m.r32(record + 8));
    s(m, Reg::Ecx, pending.signature);
    s(m, Reg::Ebp, 0xfffffffd);
    stack::store(m, 0x1c, r(m, Reg::Edi));
    stack::store(m, 0x20, r(m, Reg::Ebp));
    let count = stack::call(m, 0x41fe21, &[], Some(pending.signature), |m| {
        crate::native_vm_signature::count(m, pending.signature, a)
    })?;
    s(m, Reg::Ebx, count);
    s(m, Reg::Eax, 0);
    s(m, Reg::Edx, count);
    s(m, Reg::Ebx, count.wrapping_sub(1));
    for offset in [0x24, 0x2c, 0x28, 0x30, 0x38, 0x34] {
        stack::store(m, offset, 0);
    }
    for index in (0..count).rev() {
        let thread = stack::load(m, 0x10);
        s(m, Reg::Eax, thread);
        let frame = m.r32(thread + 0x20);
        s(m, Reg::Ecx, frame);
        let operands = m.r32(frame + 0x28);
        s(m, Reg::Ecx, operands);
        let mut cell = stack::call(m, 0x41fe57, &[], Some(operands), |m| {
            crate::native_vm_thread_run::pop_operand(m, operands)
        })?;
        a.observe_context(
            m,
            crate::native_vm_context::Observation::OperandPop {
                thread,
                frame,
                index: m.r32(operands + 8) as usize,
                cell,
            },
        )?;
        s(m, Reg::Esi, cell);
        let vt = m.r32(cell);
        s(m, Reg::Edx, vt);
        s(m, Reg::Ecx, cell);
        let kind = match vt {
            0x5e734c => 1,
            0x5e7344 => 2,
            _ => return Err("NativeVmDispatch: native argument virtual type unavailable".into()),
        };
        stack::call(m, 0x41fe5f, &[], Some(cell), |m| {
            stack::function(m, &[], Some(cell), |m| s(m, Reg::Eax, kind))
        });
        if kind == 2 {
            let pool = m.r32(0x62f130);
            s(m, Reg::Ecx, pool);
            let copy = stack::call(m, 0x41fe6f, &[], Some(pool), |m| {
                crate::native_vm_values::allocate(m, pool, a)
            })?;
            if copy == 0 {
                return Err("NativeVmDispatch: native argument copy allocation failed".into());
            }
            cell = stack::call(m, 0x41fe7b, &[cell], Some(copy), |m| {
                crate::native_vm_thread_run::copy_value(m, copy, cell)
            });
            a.observe_value(
                m,
                crate::native_vm_values::Observation::Constructed { address: cell },
            )?;
            s(m, Reg::Esi, cell);
        }
        let signature = stack::load(m, 0x50);
        s(m, Reg::Ecx, signature);
        s(m, Reg::Ebp, cell);
        let ty = stack::call(m, 0x41fe8d, &[index], Some(signature), |m| {
            stack::function(m, &[index], Some(signature), |m| {
                stack::push_reg(m, Reg::Esi);
                s(m, Reg::Esi, signature);
                let types = m.r32(signature + 8);
                s(m, Reg::Eax, types);
                s(m, Reg::Ecx, index);
                if types == 0 || index >= m.r32(types) {
                    return Err("NativeVmDispatch: native parameter type owner missing".into());
                }
                s(m, Reg::Eax, m.r32(types + 4));
                let ty = m.r32(r(m, Reg::Eax) + index * 4);
                s(m, Reg::Eax, ty);
                Ok::<u32, String>(ty)
            })
        })?;
        s(m, Reg::Ecx, m.r32(cell + 4));
        s(m, Reg::Edi, ty);
        s(m, Reg::Eax, m.r32(ty));
        if r(m, Reg::Eax) != r(m, Reg::Ecx) {
            return Err(
                "NativeVmDispatch: native parameter conversion continuation unavailable".into(),
            );
        }
        let descriptor = m.r32(cell + 4);
        s(m, Reg::Eax, descriptor);
        if !matches!(m.r8(descriptor), b'I' | b'F') {
            return Err(
                "NativeVmDispatch: native reference unboxing continuation unavailable".into(),
            );
        }
        let used = stack::load(m, 0x2c);
        let capacity = stack::load(m, 0x24);
        s(m, Reg::Ecx, capacity);
        s(m, Reg::Eax, used + 1);
        if used + 1 > capacity {
            let next = capacity + used + 1;
            s(m, Reg::Eax, next);
            stack::call(m, 0x41ffa9, &[next], Some(b + 0x24), |m| {
                crate::native_vm_values::resize(m, b + 0x24, next, a)
            })?;
        }
        s(m, Reg::Eax, stack::load(m, 0x2c));
        s(m, Reg::Edx, stack::load(m, 0x28));
        m.w32(r(m, Reg::Edx) + r(m, Reg::Eax) * 4, cell);
        s(m, Reg::Esi, stack::load(m, 0x2c) + 1);
        s(m, Reg::Ecx, index);
        s(m, Reg::Ebx, index.wrapping_sub(1));
        stack::store(m, 0x2c, r(m, Reg::Esi));
    }
    if count != 0 {
        s(m, Reg::Edi, stack::load(m, 0x1c));
        s(m, Reg::Ebp, stack::load(m, 0x20));
    }
    let pool = m.r32(0x62f130);
    s(m, Reg::Ecx, pool);
    let cell = stack::call(m, 0x41ffdb, &[], Some(pool), |m| {
        crate::native_vm_values::allocate(m, pool, a)
    })?;
    s(m, Reg::Edx, class);
    stack::call(m, 0x41ffeb, &[class], Some(cell), |m| {
        stack::function(m, &[class], Some(cell), |m| {
            s(m, Reg::Eax, cell);
            s(m, Reg::Ecx, class);
            m.w32(cell, 0x5e734c);
            let descriptor = m.r32(class + 8);
            s(m, Reg::Edx, descriptor);
            s(m, Reg::Ecx, m.r32(descriptor));
            m.w32(cell + 4, r(m, Reg::Ecx));
            m.w32(cell + 8, 0);
            m.w32(0x62f138, m.r32(0x62f138).wrapping_add(1));
        })
    });
    s(m, Reg::Esi, cell);
    s(m, Reg::Eax, pending.method);
    if m.r8(pending.method + 4) & 8 == 0 {
        s(m, Reg::Ecx, m.r32(b + 0x58));
        m.w32(cell + 8, r(m, Reg::Ecx));
    }
    a.observe_value(
        m,
        crate::native_vm_values::Observation::Constructed { address: cell },
    )?;
    let used = stack::load(m, 0x2c);
    let capacity = stack::load(m, 0x24);
    s(m, Reg::Edx, used);
    s(m, Reg::Ecx, capacity);
    s(m, Reg::Eax, used + 1);
    if used + 1 > capacity {
        let next = used + 1 + capacity;
        s(m, Reg::Eax, next);
        stack::call(m, 0x42001d, &[next], Some(b + 0x24), |m| {
            crate::native_vm_values::resize(m, b + 0x24, next, a)
        })?;
    }
    s(m, Reg::Ecx, m.r32(b + 0x2c));
    s(m, Reg::Eax, m.r32(b + 0x28));
    pending.pc = 0x420025;
    Ok(pending)
}

#[allow(dead_code)]
pub trait Environment: crate::native_vm_context::Environment {
    /// Declared allocator leaf, including any profile-specific GPR effects.
    fn free(&mut self, m: &mut Space, pointer: u32) -> Result<(), String>;
}
fn cached_result(m: &mut Space, signature: u32, pc: u32) -> u32 {
    stack::call(m, pc, &[], Some(signature), |m| {
        stack::function(m, &[], Some(signature), |m| {
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, signature);
            let types = m.r32(signature + 8);
            s(m, Reg::Eax, types);
            s(m, Reg::Ecx, types);
            let index = m.r32(types + 8).wrapping_sub(1);
            let capacity = m.r32(types);
            s(m, Reg::Eax, index);
            s(m, Reg::Edx, capacity);
            if capacity as i32 > index as i32 {
                let data = m.r32(types + 4);
                s(m, Reg::Ecx, data);
                s(m, Reg::Eax, m.r32(data.wrapping_add(index.wrapping_mul(4))));
            } else {
                s(m, Reg::Eax, 0)
            }
            r(m, Reg::Eax)
        })
    })
}
/// Resume41fbc0 after the native CALL, preserving result/free-list ownership.
/// `result` is the executing native's actual return bits, not a replay answer.
/// The retained prefix token and cached signature remain mandatory.
#[allow(dead_code)]
pub fn finish_cached(
    m: &mut Space,
    pending: PendingDispatch,
    kind: crate::native_vm_abi::ReturnKind,
    result: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    use crate::native_vm_abi::ReturnKind;
    if pending.pc != 0x420025 || !stack::active(m) || stack::has_pending_call(m) {
        return Err("NativeVmDispatch: missing retained post-native caller".into());
    }
    let b = stack::sp(m);
    let payload = match kind {
        ReturnKind::Absent | ReturnKind::Void => 0,
        _ => result,
    };
    if kind == ReturnKind::Float {
        m.w32(b + 0x58, result)
    }
    s(m, Reg::Ebp, payload);
    s(m, Reg::Eax, m.r32(b + 0x2c));
    s(m, Reg::Edi, 0);
    s(m, Reg::Esi, 0);
    for (data, count, pc) in [(0x28, 0x2c, 0x4200dc), (0x34, 0x38, 0x42010c)] {
        let n = m.r32(b + count);
        if n > 1024 {
            return Err("NativeVmDispatch: malformed temporary native vector".into());
        }
        for i in 0..n {
            let vector = m.r32(b + data);
            s(m, Reg::Ecx, vector);
            let cell = m.r32(vector + i * 4);
            s(m, Reg::Ecx, cell);
            if cell != 0 {
                s(m, Reg::Edx, m.r32(cell));
                stack::call(m, pc, &[1], Some(cell), |m| {
                    crate::native_vm_values::release(m, cell, environment)
                })?;
            }
            s(m, Reg::Eax, vector);
            m.w32(vector + i * 4, 0);
            s(m, Reg::Eax, n);
            s(m, Reg::Esi, i + 1);
        }
        if count == 0x2c {
            s(m, Reg::Eax, m.r32(b + 0x38));
            s(m, Reg::Esi, 0)
        }
        m.w32(b + count, 0);
    }
    let signature = m.r32(b + 0x50);
    let result_type = cached_result(m, signature, 0x420129);
    s(m, Reg::Esi, result_type);
    if result_type != 0 {
        let pool = m.r32(0x62f130);
        s(m, Reg::Ecx, pool);
        let cell = stack::call(m, 0x42013a, &[], Some(pool), |m| {
            crate::native_vm_values::allocate(m, pool, environment)
        })?;
        m.w32(cell, 0x5e734c);
        s(m, Reg::Ecx, m.r32(result_type));
        m.w32(cell + 4, r(m, Reg::Ecx));
        m.w32(cell + 8, 0);
        m.w32(0x62f138, m.r32(0x62f138).wrapping_add(1));
        s(m, Reg::Edi, cell);
        s(m, Reg::Edx, m.r32(cell + 4));
        m.w32(cell + 8, payload);
        environment.observe_value(
            m,
            crate::native_vm_values::Observation::Constructed { address: cell },
        )?;
        let frame = m.r32(pending.thread + 0x20);
        s(m, Reg::Eax, frame);
        let vector = m.r32(frame + 0x28);
        s(m, Reg::Esi, vector);
        let count = m.r32(vector + 8);
        let capacity = m.r32(vector);
        s(m, Reg::Eax, count + 1);
        s(m, Reg::Ecx, capacity);
        if count + 1 > capacity {
            s(m, Reg::Ecx, capacity + count + 1);
            stack::call(m, 0x42017f, &[capacity + count + 1], Some(vector), |m| {
                crate::native_vm_values::resize(m, vector, capacity + count + 1, environment)
            })?;
        }
        s(m, Reg::Ecx, count);
        s(m, Reg::Edx, m.r32(vector + 4));
        m.w32(r(m, Reg::Edx) + count * 4, cell);
        m.w32(vector + 8, count + 1);
        environment.observe_context(
            m,
            crate::native_vm_context::Observation::OperandPush {
                thread: pending.thread,
                frame,
                index: count as usize,
                cell,
            },
        )?;
        s(m, Reg::Edi, 0);
    }
    if m.r32(pending.thread + 0x24) == 0 {
        let flags = m.r32(pending.thread + 0x2c);
        s(m, Reg::Eax, flags);
        s(m, Reg::Ecx, u32::from(flags == 0));
        if flags == 0 {
            s(m, Reg::Eax, flags | 0x40);
            m.w32(pending.thread + 0x2c, flags | 0x40)
        }
    }
    let conversions = m.r32(b + 0x34);
    s(m, Reg::Edx, conversions);
    stack::external(m, 0x4201b1, &[conversions], None, |m| {
        environment.free(m, conversions)
    })?;
    s(m, Reg::Eax, m.r32(b + 0x38));
    m.w32(b + 0x34, 0);
    if (r(m, Reg::Eax) as i32) > 0 {
        m.w32(b + 0x38, 0)
    }
    let args = m.r32(b + 0x28);
    s(m, Reg::Eax, args);
    stack::push(m, args);
    m.w32(b + 0x30, 0);
    stack::external(m, 0x4201d2, &[], None, |m| environment.free(m, args))?;
    stack::add_sp(m, 4);
    s(m, Reg::Eax, 1);
    stack::leave(m, pending.frame);
    Ok(1)
}

///420211..4202fe: resolve the original script body, create/link its raw frame,
///and attach the instance receiver local. Stops before signature parsing and
///parameter transfer. No Thread.run entry or completed method call is implied.
#[allow(dead_code)]
pub fn prepare_script_frame(
    m: &mut Space,
    mut pending: PendingDispatch,
    a: &mut impl crate::native_vm_context::Environment,
) -> Result<PendingDispatch, String> {
    use crate::native_vm_metadata::FrameInit;
    if pending.pc != 0x41fc31
        || !stack::active(m)
        || stack::has_pending_call(m)
        || m.r8(pending.method + 4) & 0x40 != 0
    {
        return Err("NativeVmDispatch: missing retained script caller".into());
    }
    let b = stack::sp(m);
    let class = r(m, Reg::Ebp);
    let body_index = m.r32(pending.method + 0x10);
    let bodies = m.r32(class + 0x18);
    let caller = m.r32(pending.thread + 0x20);
    let frame_pool = m.r32(0x62ddf0);
    let locals = m.r32(pending.method + 0x14);
    if bodies == 0 || caller == 0 || frame_pool == 0 {
        return Err(format!("NativeVmDispatch: script body/frame/local context unavailable: body={body_index:#x}, count={}, caller={caller:#x}, pool={frame_pool:#x}, locals={locals}", m.r32(bodies)));
    }
    a.observe_context(
        m,
        crate::native_vm_context::Observation::MethodSelect {
            thread: pending.thread,
            class,
            method: pending.method,
            receiver: if m.r8(pending.method + 4) & 8 == 0 {
                m.r32(b + 0x58)
            } else {
                0
            },
        },
    )?;
    s(m, Reg::Eax, body_index);
    s(m, Reg::Ecx, bodies);
    if (m.r32(bodies) as i32) <= (body_index as i32) {
        pending.pc = 0x420583;
        return Ok(pending);
    }
    s(m, Reg::Ecx, m.r32(bodies + 4));
    let body = m.r32(r(m, Reg::Ecx) + body_index * 4);
    s(m, Reg::Esi, body);
    let receiver = m.r32(b + 0x58);
    if body == 0 {
        pending.pc = 0x420583;
        return Ok(pending);
    }
    let instance = m.r8(pending.method + 4) & 8 == 0;
    if instance && (receiver == 0 || m.r32(receiver + 12) == 0 || m.r32(0x62f134) == 0) {
        return Err("NativeVmDispatch: raw receiver/variable pool is unavailable".into());
    }
    s(m, Reg::Eax, body_index);
    s(m, Reg::Ecx, bodies);
    s(m, Reg::Ecx, m.r32(bodies + 4));
    s(m, Reg::Esi, body);
    s(m, Reg::Edi, pending.thread);
    s(m, Reg::Edx, caller);
    s(m, Reg::Eax, m.r32(pending.thread + 0x24));
    m.w32(caller + 0x2c, r(m, Reg::Eax));
    a.observe_context(
        m,
        crate::native_vm_context::Observation::SavedPc {
            thread: pending.thread,
            frame: caller,
            pc: m.r32(caller + 0x2c),
        },
    )?;
    s(m, Reg::Ebx, caller);
    stack::store(m, 0x14, caller);
    let frame = stack::call(m, 0x42024c, &[], Some(frame_pool), |m| {
        crate::native_vm_values::allocate_frame(m, frame_pool, a)
    })?;
    s(m, Reg::Ecx, pending.method);
    s(m, Reg::Edx, locals);
    s(m, Reg::Ecx, receiver);
    let input = FrameInit {
        thread: pending.thread,
        context: class,
        pc: body,
        receiver,
        locals,
    };
    stack::call(
        m,
        0x420267,
        &[pending.thread, class, body, receiver, locals],
        Some(frame),
        |m| crate::native_vm_metadata::construct_frame(m, frame, input, a),
    )?;
    s(m, Reg::Ebp, frame);
    let list = m.r32(pending.thread + 12);
    s(m, Reg::Eax, list);
    stack::store(m, 0x54, frame);
    let first = m.r32(list + 8);
    s(m, Reg::Edx, first);
    m.w32(frame + 4, first);
    s(m, Reg::Ecx, first);
    s(m, Reg::Edx, m.r32(first + 8));
    m.w32(frame + 8, r(m, Reg::Edx));
    s(m, Reg::Ecx, first);
    m.w32(first + 8, frame);
    m.w32(list + 8, frame);
    s(m, Reg::Edx, m.r32(b + 0x20));
    s(m, Reg::Eax, pending.method);
    m.w32(frame + 0x38, r(m, Reg::Edx));
    a.observe_context(
        m,
        crate::native_vm_context::Observation::MethodFrame {
            address: frame,
            caller,
            method: pending.method,
            signature: m.r32(frame + 0x38),
            enter: false,
        },
    )?;
    if instance {
        m.w32(pending.thread + 0x20, frame);
        a.observe_context(
            m,
            crate::native_vm_context::Observation::CurrentFrame {
                thread: pending.thread,
                frame,
            },
        )?;
        let pool = m.r32(0x62f134);
        let variable = stack::call(m, 0x4202af, &[], Some(pool), |m| {
            crate::native_vm_values::allocate_variable(m, pool, a)
        })?;
        s(m, Reg::Ecx, receiver);
        let receiver_class = m.r32(receiver + 12);
        s(m, Reg::Edx, receiver_class);
        stack::call(
            m,
            0x4202c7,
            &[0x60c37c, receiver_class],
            Some(variable),
            |m| {
                stack::function(m, &[0x60c37c, receiver_class], Some(variable), |m| {
                    s(m, Reg::Eax, variable);
                    s(m, Reg::Ecx, receiver_class);
                    let descriptor = m.r32(receiver_class + 8);
                    s(m, Reg::Ecx, descriptor);
                    m.w32(variable, 0x5e734c);
                    s(m, Reg::Edx, m.r32(descriptor));
                    m.w32(variable + 4, r(m, Reg::Edx));
                    m.w32(variable + 8, 0);
                    m.w32(0x62f138, m.r32(0x62f138).wrapping_add(1));
                    s(m, Reg::Ecx, 0x60c37c);
                    m.w32(variable, 0x5e7344);
                    m.w32(variable + 12, 0x60c37c);
                })
            },
        );
        a.observe_context(
            m,
            crate::native_vm_context::Observation::VariableConstructed { address: variable },
        )?;
        s(m, Reg::Edi, variable);
        s(m, Reg::Eax, pending.thread);
        s(m, Reg::Ecx, frame);
        let vector = m.r32(frame + 0x18);
        s(m, Reg::Esi, vector);
        let count = m.r32(vector + 8);
        let capacity = m.r32(vector);
        s(m, Reg::Eax, count + 1);
        s(m, Reg::Ecx, capacity);
        if count + 1 > capacity {
            s(m, Reg::Ecx, capacity + count + 1);
            stack::call(m, 0x4202eb, &[capacity + count + 1], Some(vector), |m| {
                crate::native_vm_values::resize(m, vector, capacity + count + 1, a)
            })?;
        }
        s(m, Reg::Edx, count);
        s(m, Reg::Eax, m.r32(vector + 4));
        s(m, Reg::Ecx, receiver);
        m.w32(r(m, Reg::Eax) + count * 4, variable);
        m.w32(vector + 8, count + 1);
        a.observe_context(
            m,
            crate::native_vm_context::Observation::Local {
                thread: pending.thread,
                frame,
                append: true,
                index: count as usize,
                variable,
            },
        )?;
        let before = crate::native_vm_context::ValueSnapshot::read(m, variable)?;
        m.w32(variable + 8, receiver);
        a.observe_context(
            m,
            crate::native_vm_context::Observation::VariableStore {
                address: variable,
                before,
                after: crate::native_vm_context::ValueSnapshot::read(m, variable)?,
            },
        )?;
    }
    pending.pc = 0x4202fe;
    Ok(pending)
}

///4202fe's zero-parameter script return after the newly created frame owns
///its receiver local. Thread.run itself has not entered yet.
#[allow(dead_code)]
pub fn finish_script_zero_arguments(
    m: &mut Space,
    pending: PendingDispatch,
    a: &mut impl crate::native_vm_context::Environment,
) -> Result<u32, String> {
    finish_script_inner(m, pending, a, None)
}
pub fn finish_script_arguments(
    m: &mut Space,
    pending: PendingDispatch,
    a: &mut impl crate::native_vm_context::Environment,
    fpu: &mut crate::native_vm_fpu::State,
) -> Result<u32, String> {
    finish_script_inner(m, pending, a, Some(fpu))
}
fn finish_script_inner(
    m: &mut Space,
    pending: PendingDispatch,
    a: &mut impl crate::native_vm_context::Environment,
    mut fpu: Option<&mut crate::native_vm_fpu::State>,
) -> Result<u32, String> {
    if pending.pc == 0x420583 {
        let signature = r(m, Reg::Edx);
        s(m, Reg::Ecx, signature);
        let count = stack::call(m, 0x42058a, &[], Some(signature), |m| {
            crate::native_vm_signature::count(m, signature, a)
        })?;
        s(m, Reg::Ecx, count);
        s(m, Reg::Eax, count.wrapping_sub(1));
        if count != 0 {
            return Err(
                "NativeVmDispatch: empty body argument disposal continuation unavailable".into(),
            );
        }
        let signature = stack::load(m, 0x50);
        s(m, Reg::Ecx, signature);
        let result = stack::call(m, 0x42066a, &[], Some(signature), |m| {
            stack::function(m, &[], Some(signature), |m| {
                stack::push_reg(m, Reg::Esi);
                s(m, Reg::Esi, signature);
                let types = m.r32(signature + 8);
                s(m, Reg::Eax, types);
                if types == 0 {
                    return Err::<u32, String>(
                        "NativeVmDispatch: no-body return types missing".into(),
                    );
                }
                s(m, Reg::Ecx, types);
                let count = m.r32(types + 8);
                s(m, Reg::Eax, count);
                s(m, Reg::Edx, m.r32(types));
                s(m, Reg::Eax, count.wrapping_sub(1));
                if (r(m, Reg::Edx) as i32) <= (r(m, Reg::Eax) as i32) {
                    s(m, Reg::Eax, 0);
                    return Ok(0);
                }
                s(m, Reg::Ecx, m.r32(types + 4));
                s(m, Reg::Eax, m.r32(r(m, Reg::Ecx) + (count - 1) * 4));
                Ok(r(m, Reg::Eax))
            })
        })?;
        if result != 0 {
            return Err(
                "NativeVmDispatch: empty-body default result continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, 1);
        stack::leave(m, pending.frame);
        return Ok(1);
    }
    if pending.pc != 0x4202fe || !stack::active(m) || stack::has_pending_call(m) {
        return Err("NativeVmDispatch: missing retained script frame continuation".into());
    }
    let b = stack::sp(m);
    let frame = r(m, Reg::Ebp);
    let signature = m.r32(b + 0x50);
    s(m, Reg::Ecx, signature);
    stack::store(m, 0x1c, 0);
    let n = stack::call(m, 0x42030f, &[], Some(signature), |m| {
        crate::native_vm_signature::count(m, signature, a)
    })?;
    s(m, Reg::Edx, n);
    s(m, Reg::Eax, n.wrapping_sub(1));
    stack::store(m, 0x58, r(m, Reg::Eax));
    if n != 0 {
        let caller = r(m, Reg::Ebx);
        for index in (0..n).rev() {
            let fpu = fpu
                .as_deref_mut()
                .ok_or("NativeVmDispatch: script scalar parameter context unavailable")?;
            s(m, Reg::Eax, pending.thread);
            m.w32(pending.thread + 0x20, caller);
            a.observe_context(
                m,
                crate::native_vm_context::Observation::CurrentFrame {
                    thread: pending.thread,
                    frame: caller,
                },
            )?;
            let vector = m.r32(caller + 0x28);
            let mut source = stack::call(m, 0x420333, &[], Some(vector), |m| {
                crate::native_vm_thread_run::pop_operand(m, vector)
            })?;
            a.observe_context(
                m,
                crate::native_vm_context::Observation::OperandPop {
                    thread: pending.thread,
                    frame: caller,
                    index: m.r32(vector + 8) as usize,
                    cell: source,
                },
            )?;
            s(m, Reg::Ebp, source);
            s(m, Reg::Edx, m.r32(source));
            stack::store(m, 0x18, source);
            let kind = match m.r32(source) {
                0x5e734c => 1,
                0x5e7344 => 2,
                _ => return Err("NativeVmDispatch: argument virtual type unavailable".into()),
            };
            stack::call(m, 0x420340, &[], Some(source), |m| {
                stack::function(m, &[], Some(source), |m| s(m, Reg::Eax, kind))
            });
            if kind == 2 {
                let pool = m.r32(0x62f130);
                s(m, Reg::Esi, pool);
                let free = m.r32(pool + 0x10);
                s(m, Reg::Ecx, free);
                let cell = if free != 0 {
                    s(m, Reg::Eax, m.r32(free));
                    m.w32(pool + 0x10, r(m, Reg::Eax));
                    free
                } else {
                    let capacity = m.r32(pool + 0xc);
                    s(m, Reg::Eax, capacity);
                    if m.r32(pool + 8) == capacity {
                        let blocks = m.r32(pool + 4);
                        s(m, Reg::Edi, blocks);
                        let old = m.r32(pool);
                        s(m, Reg::Edx, old);
                        s(m, Reg::Eax, capacity + 512);
                        m.w32(pool + 0xc, r(m, Reg::Eax));
                        s(m, Reg::Eax, blocks + 1);
                        s(m, Reg::Ecx, (blocks + 1) * 4);
                        stack::push(m, r(m, Reg::Ecx));
                        stack::push(m, old);
                        m.w32(pool + 4, blocks + 1);
                        let table = stack::external(m, 0x420386, &[], None, |m| {
                            a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
                        })?;
                        s(m, Reg::Eax, table);
                        stack::push(m, 0x1800);
                        m.w32(pool, table);
                        let block =
                            stack::external(m, 0x420392, &[], None, |m| a.allocate(m, 0x1800))?;
                        s(m, Reg::Eax, block);
                        s(m, Reg::Ecx, m.r32(pool));
                        stack::add_sp(m, 12);
                        if table == 0 || block == 0 {
                            return Err(
                                "NativeVmDispatch: argument copy pool allocation failed".into()
                            );
                        }
                        m.w32(table + blocks * 4, block);
                    }
                    let count = m.r32(pool + 8);
                    s(m, Reg::Ecx, count);
                    s(m, Reg::Edx, count + 1);
                    m.w32(pool + 8, count + 1);
                    s(m, Reg::Esi, m.r32(pool));
                    s(m, Reg::Eax, (count & 511) * 3);
                    s(m, Reg::Edx, count / 512);
                    let block = m.r32(r(m, Reg::Esi) + (count / 512) * 4);
                    s(m, Reg::Ecx, block + (count & 511) * 12);
                    r(m, Reg::Ecx)
                };
                if cell == 0 {
                    return Err("NativeVmDispatch: null argument copy allocation".into());
                }
                a.observe_value(
                    m,
                    crate::native_vm_values::Observation::Allocated {
                        address: cell,
                        pool,
                        freelist: free != 0,
                    },
                )?;
                source = stack::call(m, 0x4203d6, &[source], Some(cell), |m| {
                    crate::native_vm_thread_run::copy_value(m, cell, source)
                });
                a.observe_value(
                    m,
                    crate::native_vm_values::Observation::Constructed { address: source },
                )?;
                stack::store(m, 0x18, source);
            }
            s(m, Reg::Eax, index);
            s(m, Reg::Edx, frame);
            s(m, Reg::Edi, pending.thread);
            s(m, Reg::Ecx, signature);
            m.w32(pending.thread + 0x20, frame);
            a.observe_context(
                m,
                crate::native_vm_context::Observation::CurrentFrame {
                    thread: pending.thread,
                    frame,
                },
            )?;
            s(m, Reg::Ebp, 0);
            let ty = stack::call(m, 0x4203f9, &[index], Some(signature), |m| {
                stack::function(m, &[index], Some(signature), |m| {
                    stack::push_reg(m, Reg::Esi);
                    s(m, Reg::Esi, signature);
                    let types = m.r32(signature + 8);
                    s(m, Reg::Eax, types);
                    s(m, Reg::Ecx, index);
                    if types == 0 || index >= m.r32(types) {
                        return Err("NativeVmDispatch: cached parameter type unavailable".into());
                    }
                    s(m, Reg::Eax, m.r32(types + 4));
                    let ty = m.r32(r(m, Reg::Eax) + index * 4);
                    s(m, Reg::Eax, ty);
                    Ok::<u32, String>(ty)
                })
            })?;
            s(m, Reg::Esi, m.r32(b + 0x1c));
            s(m, Reg::Ebx, ty);
            if r(m, Reg::Esi) != 0 {
                return Err("NativeVmDispatch: named parameter continuation unavailable".into());
            }
            let pool = m.r32(0x62f134);
            s(m, Reg::Esi, pool);
            let free = m.r32(pool + 0x10);
            s(m, Reg::Eax, free);
            let variable = if free != 0 {
                s(m, Reg::Ecx, m.r32(free));
                m.w32(pool + 0x10, r(m, Reg::Ecx));
                free
            } else {
                let capacity = m.r32(pool + 0xc);
                s(m, Reg::Eax, capacity);
                if m.r32(pool + 8) == capacity {
                    let blocks = m.r32(pool + 4);
                    s(m, Reg::Edi, blocks);
                    s(m, Reg::Eax, capacity + 0x14820);
                    m.w32(pool + 0xc, r(m, Reg::Eax));
                    s(m, Reg::Eax, blocks + 1);
                    s(m, Reg::Edx, (blocks + 1) * 4);
                    m.w32(pool + 4, blocks + 1);
                    let old = m.r32(pool);
                    s(m, Reg::Eax, old);
                    stack::push(m, (blocks + 1) * 4);
                    stack::push(m, old);
                    let table = stack::external(m, 0x42046c, &[], None, |m| {
                        a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
                    })?;
                    s(m, Reg::Eax, table);
                    stack::push(m, 0x148200);
                    m.w32(pool, table);
                    let block =
                        stack::external(m, 0x420478, &[], None, |m| a.allocate(m, 0x148200))?;
                    s(m, Reg::Eax, block);
                    s(m, Reg::Ecx, m.r32(pool));
                    stack::add_sp(m, 0xc);
                    if table == 0 || block == 0 {
                        return Err("NativeVmDispatch: parameter pool allocation failed".into());
                    }
                    m.w32(table + blocks * 4, block);
                }
                let index = m.r32(pool + 8);
                s(m, Reg::Edi, index);
                s(m, Reg::Edx, index + 1);
                m.w32(pool + 8, index + 1);
                let quotient = index / 0x14820;
                let remainder = index % 0x14820;
                s(m, Reg::Ecx, quotient);
                s(m, Reg::Eax, quotient);
                s(m, Reg::Edx, remainder);
                s(m, Reg::Edi, 0x14820);
                s(m, Reg::Esi, m.r32(pool));
                let block = m.r32(r(m, Reg::Esi) + quotient * 4);
                s(m, Reg::Eax, block);
                s(m, Reg::Edi, stack::load(m, 0x10));
                s(m, Reg::Edx, remainder * 16);
                s(m, Reg::Eax, block + remainder * 16);
                r(m, Reg::Eax)
            };
            a.observe_context(
                m,
                crate::native_vm_context::Observation::VariableAllocated {
                    address: variable,
                    pool,
                    freelist: free != 0,
                },
            )?;
            m.w32(variable, 0x5e734c);
            s(m, Reg::Ecx, m.r32(ty));
            m.w32(variable + 4, r(m, Reg::Ecx));
            m.w32(variable + 8, 0);
            m.w32(0x62f138, m.r32(0x62f138).wrapping_add(1));
            m.w32(variable + 0xc, 0);
            m.w32(variable, 0x5e7344);
            a.observe_context(
                m,
                crate::native_vm_context::Observation::VariableConstructed { address: variable },
            )?;
            s(m, Reg::Ebp, variable);
            s(m, Reg::Edx, frame);
            let locals = m.r32(frame + 0x18);
            s(m, Reg::Esi, locals);
            let count = m.r32(locals + 8);
            s(m, Reg::Eax, count + 1);
            s(m, Reg::Ecx, m.r32(locals));
            if count + 1 > m.r32(locals) {
                return Err(
                    "NativeVmDispatch: parameter local vector growth continuation unavailable"
                        .into(),
                );
            }
            s(m, Reg::Eax, count);
            s(m, Reg::Ecx, m.r32(locals + 4));
            m.w32(r(m, Reg::Ecx) + count * 4, variable);
            m.w32(locals + 8, count + 1);
            a.observe_context(
                m,
                crate::native_vm_context::Observation::Local {
                    thread: pending.thread,
                    frame,
                    append: true,
                    index: count as usize,
                    variable,
                },
            )?;
            s(m, Reg::Esi, source);
            let converter = m.r32(pending.thread + 0x14);
            let source_before = crate::native_vm_context::ValueSnapshot::read(m, source)?;
            let destination_before = crate::native_vm_context::ValueSnapshot::read(m, variable)?;
            stack::call(m, 0x420546, &[variable, source], Some(converter), |m| {
                crate::native_vm_alu::assign_integer(m, converter, variable, source, fpu)
            })?;
            a.observe_context(
                m,
                crate::native_vm_context::Observation::ParameterAssignment {
                    thread: pending.thread,
                    frame,
                    source,
                    destination: variable,
                    source_before,
                    source_after: crate::native_vm_context::ValueSnapshot::read(m, source)?,
                    destination_before,
                    destination_after: crate::native_vm_context::ValueSnapshot::read(m, variable)?,
                },
            )?;
            s(m, Reg::Edx, m.r32(source));
            stack::call(m, 0x420553, &[1], Some(source), |m| {
                crate::native_vm_values::release(m, source, a)
            })?;
            s(m, Reg::Eax, index);
            s(m, Reg::Ecx, index);
            s(m, Reg::Eax, index.wrapping_sub(1));
            stack::store(m, 0x58, r(m, Reg::Eax));
        }
        s(m, Reg::Ebp, frame);
    }
    s(m, Reg::Eax, pending.thread);
    m.w32(pending.thread + 0x20, frame);
    a.observe_context(
        m,
        crate::native_vm_context::Observation::CurrentFrame {
            thread: pending.thread,
            frame,
        },
    )?;
    s(m, Reg::Edx, m.r32(frame + 0x2c));
    m.w32(pending.thread + 0x24, r(m, Reg::Edx));
    a.observe_context(
        m,
        crate::native_vm_context::Observation::MethodFrame {
            address: frame,
            caller: r(m, Reg::Ebx),
            method: pending.method,
            signature: m.r32(frame + 0x38),
            enter: true,
        },
    )?;
    s(m, Reg::Eax, 0);
    stack::leave(m, pending.frame);
    Ok(0)
}
