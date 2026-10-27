//! Source41f780/41f4d0 terminal marking and destruction of an ordinary thread
//! with empty script frames. Waiting/native-owned thread cleanup stays guarded.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_dispatch::Environment;
use slr_physics::space::Space;
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, v: u32) {
    stack::set_reg(m, reg, v)
}
pub fn mark_terminal(m: &mut Space, thread: u32) -> Result<(), String> {
    if m.r32(thread + 0x2c) & 0x10 != 0 {
        return Err("NativeVmThread: running-owner terminal transition unavailable".into());
    }
    stack::function(m, &[], Some(thread), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, thread);
        write(m, thread + 0x2c, m.r32(thread + 0x2c) | 0x80);
        Ok(())
    })
}
fn free(m: &mut Space, pc: u32, p: u32, e: &mut impl Environment) -> Result<(), String> {
    stack::push(m, p);
    stack::external(m, pc, &[], None, |m| e.free(m, p))?;
    stack::add_sp(m, 4);
    Ok(())
}
fn frame_list(m: &mut Space, list: u32, e: &mut impl Environment) -> Result<(), String> {
    stack::function(m, &[1], Some(list), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, list);
        let mut node = m.r32(list + 8);
        s(m, Reg::Ecx, node);
        stack::push_reg(m, Reg::Edi);
        write(m, list, 0x5f0990);
        s(m, Reg::Eax, m.r32(node + 4));
        s(m, Reg::Edi, 0);
        while r(m, Reg::Eax) != 0 {
            let next = m.r32(node + 8);
            s(m, Reg::Edx, next);
            if next != 0 {
                let previous = m.r32(node + 4);
                s(m, Reg::Eax, previous);
                if previous != 0 {
                    write(m, previous + 8, next);
                    s(m, Reg::Eax, next);
                    s(m, Reg::Edx, previous);
                    write(m, next + 4, previous);
                }
            }
            write(m, node + 4, 0);
            write(m, node + 8, 0);
            if node != 0 {
                if m.r32(node) != 0x5e7410 {
                    return Err("NativeVmThread: frame list owns an unsupported node".into());
                }
                s(m, Reg::Eax, m.r32(node));
                stack::push(m, 1);
                stack::call(m, 0x423cd0, &[], Some(node), |m| {
                    crate::native_vm_values::release_frame(m, node, e)
                })?;
                stack::add_sp(m, 4);
            }
            node = m.r32(list + 8);
            s(m, Reg::Ecx, node);
            s(m, Reg::Eax, m.r32(node + 4));
        }
        let next = m.r32(list + 0x18);
        s(m, Reg::Eax, next);
        s(m, Reg::Edx, 0x5e7414);
        write(m, list + 0x10, 0x5e7414);
        if next != 0 {
            let prev = m.r32(list + 0x14);
            s(m, Reg::Ecx, prev);
            if prev != 0 {
                write(m, prev + 8, next);
                s(m, Reg::Ecx, next);
                s(m, Reg::Eax, prev);
                write(m, next + 4, prev);
            }
        }
        write(m, list + 0x14, 0);
        write(m, list + 0x18, 0);
        let next = m.r32(list + 0xc);
        s(m, Reg::Eax, next);
        write(m, list + 4, r(m, Reg::Edx));
        if next != 0 {
            let prev = m.r32(list + 8);
            s(m, Reg::Ecx, prev);
            if prev != 0 {
                write(m, prev + 8, next);
                s(m, Reg::Ecx, next);
                s(m, Reg::Edx, prev);
                write(m, next + 4, prev);
            }
        }
        write(m, list + 8, 0);
        write(m, list + 0xc, 0);
        free(m, 0x423d32, list, e)?;
        stack::add_sp(m, 4);
        s(m, Reg::Eax, list);
        stack::add_sp(m, 4);
        Ok(())
    })
}
pub fn destroy(m: &mut Space, thread: u32, e: &mut impl Environment) -> Result<(), String> {
    let flags = m.r32(thread + 0x2c);
    if flags & 0x82 == 0 || flags & 0x10 != 0 || m.r32(thread + 0x18) != 0 {
        return Err("NativeVmThread: terminal ordinary owner required for destruction".into());
    }
    stack::function(m, &[], Some(thread), |m| {
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::ThreadRetiring {
                address: thread,
                flags,
            },
        )?;
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, thread);
        stack::push_reg(m, Reg::Edi);
        write(m, thread, 0x5f098c);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, 0);
        let list = m.r32(thread + 0xc);
        s(m, Reg::Ecx, list);
        if list != 0 {
            if m.r32(list) != 0x5f0988 {
                return Err("NativeVmThread: original frame-list owner required".into());
            }
            s(m, Reg::Eax, m.r32(list));
            stack::push(m, 1);
            stack::call(m, 0x41f532, &[], Some(list), |m| frame_list(m, list, e))?;
            stack::add_sp(m, 4);
        }
        let name = m.r32(thread + 0x1c);
        s(m, Reg::Ecx, name);
        free(m, 0x41f53b, name, e)?;
        let next = m.r32(thread + 8);
        s(m, Reg::Ecx, next);
        write(m, thread, 0x5f087c);
        if next != 0 {
            let prev = m.r32(thread + 4);
            s(m, Reg::Eax, prev);
            if prev != 0 {
                write(m, prev + 8, next);
                s(m, Reg::Edx, next);
                s(m, Reg::Eax, prev);
                write(m, next + 4, prev);
            }
        }
        write(m, thread + 4, 0);
        write(m, thread + 8, 0);
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::ThreadDestroyed { address: thread },
        )?;
        Ok(())
    })
}

///423ba0 pushes a caller-owned operand, including the source inline capacity
/// growth. This does not copy or transfer the pointed-to variable's ownership.
pub fn push_argument(
    m: &mut Space,
    thread: u32,
    cell: u32,
    e: &mut impl Environment,
) -> Result<(), String> {
    stack::function(m, &[cell], Some(thread), |m| {
        let frame = m.r32(thread + 0x20);
        s(m, Reg::Eax, frame);
        stack::push_reg(m, Reg::Esi);
        let vector = m.r32(frame + 0x28);
        s(m, Reg::Esi, vector);
        let count = m.r32(vector + 8);
        let cap = m.r32(vector);
        s(m, Reg::Eax, count + 1);
        s(m, Reg::Ecx, cap);
        if count + 1 > cap {
            stack::push_reg(m, Reg::Edi);
            let n = cap + count + 1;
            s(m, Reg::Edi, n);
            if n == 0 {
                return Err("NativeVmThread: operand capacity overflow".into());
            }
            let old = m.r32(vector + 4);
            s(m, Reg::Eax, old);
            s(m, Reg::Edx, n * 4);
            stack::push(m, n * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x423bdb, &[], None, |m| {
                e.reallocate(m, old, cap * 4, n * 4)
            })?;
            s(m, Reg::Eax, table);
            stack::add_sp(m, 8);
            if table == 0 {
                return Err("NativeVmThread: operand allocation failure".into());
            }
            write(m, vector + 4, table);
            if n < m.r32(vector + 8) {
                write(m, vector + 8, n);
            }
            write(m, vector, n);
            s(m, Reg::Edi, stack::load(m, 0));
            stack::add_sp(m, 4);
        }
        s(m, Reg::Ecx, m.r32(vector + 8));
        s(m, Reg::Edx, m.r32(vector + 4));
        s(m, Reg::Eax, cell);
        write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, cell);
        write(m, vector + 8, m.r32(vector + 8) + 1);
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::OperandPush {
                thread,
                frame,
                index: count as usize,
                cell,
            },
        )?;
        Ok(())
    })
}
///41f8e0 drops the expected caller-owned variable from the operand vector.
pub fn pop_variable(m: &mut Space, thread: u32) -> Result<u32, String> {
    stack::function(m, &[], Some(thread), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, thread);
        let frame = m.r32(thread + 0x20);
        s(m, Reg::Eax, frame);
        let vector = m.r32(frame + 0x28);
        s(m, Reg::Eax, vector);
        let count = m.r32(vector + 8);
        s(m, Reg::Ecx, count);
        if count == 0 {
            return Err("NativeVmThread: variable stack underflow".into());
        }
        let table = m.r32(vector + 4);
        s(m, Reg::Edx, table);
        s(m, Reg::Ecx, count - 1);
        write(m, vector + 8, count - 1);
        let p = table + (count - 1) * 4;
        let cell = m.r32(p);
        s(m, Reg::Esi, cell);
        s(m, Reg::Eax, p);
        write(m, p, 0);
        let vt = m.r32(cell);
        s(m, Reg::Eax, vt);
        s(m, Reg::Ecx, cell);
        if vt != 0x5e7344 {
            return Err("NativeVmThread: expected variable operand".into());
        }
        stack::call(m, 0x41f94d, &[], Some(cell), |m| {
            stack::function(m, &[], Some(cell), |m| s(m, Reg::Eax, 2))
        });
        stack::add_sp(m, 4);
        s(m, Reg::Eax, cell);
        stack::add_sp(m, 4);
        Ok(cell)
    })
}
