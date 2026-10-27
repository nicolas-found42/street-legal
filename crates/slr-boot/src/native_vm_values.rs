//! Original423c00 value-pool allocation with its live CALL scratch and GPRs.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
/// Facts issued by the original global value-pool and constructor bodies.
#[derive(Clone, Copy, Debug)]
pub enum Observation {
    Allocated {
        address: u32,
        pool: u32,
        freelist: bool,
    },
    Constructed {
        address: u32,
    },
    Releasing {
        address: u32,
    },
    Released {
        address: u32,
    },
}
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
#[allow(dead_code)]
pub fn allocate(m: &mut Space, pool: u32, a: &mut impl Allocator) -> Result<u32, String> {
    if pool == 0 || m.r32(pool + 8) > m.r32(pool + 12) || m.r32(pool + 12) > i32::MAX as u32 - 512 {
        return Err("NativeVmValues: missing/malformed original value pool".into());
    }
    stack::function(m, &[], Some(pool), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, pool);
        let free = m.r32(pool + 16);
        s(m, Reg::Eax, free);
        if free != 0 {
            s(m, Reg::Ecx, m.r32(free));
            write(m, pool + 16, r(m, Reg::Ecx));
            a.observe_value(
                m,
                Observation::Allocated {
                    address: free,
                    pool,
                    freelist: true,
                },
            )?;
            return Ok(free);
        }
        let capacity = m.r32(pool + 12);
        s(m, Reg::Eax, capacity);
        if m.r32(pool + 8) == capacity {
            s(m, Reg::Eax, capacity + 512);
            stack::push_reg(m, Reg::Edi);
            let blocks = m.r32(pool + 4);
            s(m, Reg::Edi, blocks);
            write(m, pool + 12, capacity + 512);
            s(m, Reg::Eax, blocks + 1);
            s(m, Reg::Edx, (blocks + 1) * 4);
            write(m, pool + 4, blocks + 1);
            let old = m.r32(pool);
            s(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x423c3b, &[], None, |m| {
                a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
            })?;
            if table == 0 {
                return Err("NativeVmValues: pool table allocation failed".into());
            }
            s(m, Reg::Eax, table);
            stack::push(m, 0x1800);
            write(m, pool, table);
            let block = stack::external(m, 0x423c47, &[], None, |m| a.allocate(m, 0x1800))?;
            if block == 0 {
                return Err("NativeVmValues: pool block allocation failed".into());
            }
            s(m, Reg::Eax, block);
            s(m, Reg::Ecx, table);
            stack::add_sp(m, 12);
            write(m, table + blocks * 4, block);
            stack::add_sp(m, 4);
        }
        let index = m.r32(pool + 8);
        s(m, Reg::Ecx, index);
        s(m, Reg::Edx, index + 1);
        write(m, pool + 8, index + 1);
        let table = m.r32(pool);
        s(m, Reg::Esi, table);
        s(m, Reg::Eax, index);
        s(m, Reg::Edx, index / 512);
        s(m, Reg::Eax, index % 512);
        let block = m.r32(table + (index / 512) * 4);
        s(m, Reg::Ecx, block);
        s(m, Reg::Eax, (index % 512) * 3);
        let cell = block + (index % 512) * 12;
        s(m, Reg::Eax, cell);
        a.observe_value(
            m,
            Observation::Allocated {
                address: cell,
                pool,
                freelist: false,
            },
        )?;
        Ok(cell)
    })
}
///423e60 builds a class-typed reference value in the actual issued cell.
pub fn construct_reference(m: &mut Space, cell: u32, class: u32) -> Result<u32, String> {
    if cell == 0 || class == 0 || m.r32(class + 8) == 0 {
        return Err("NativeVmValues: reference requires issued cell and class descriptor".into());
    }
    stack::function(m, &[class], Some(cell), |m| {
        s(m, Reg::Eax, cell);
        s(m, Reg::Ecx, class);
        write(m, cell, 0x5e734c);
        s(m, Reg::Edx, m.r32(class + 8));
        s(m, Reg::Ecx, m.r32(r(m, Reg::Edx)));
        write(m, cell + 4, r(m, Reg::Ecx));
        write(m, cell + 8, 0);
        write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
        Ok(cell)
    })
}
///405180 nonzero vector resize; source-derived callers supply actual vectors.
#[allow(dead_code)]
pub fn resize(
    m: &mut Space,
    vector: u32,
    capacity: u32,
    a: &mut impl Allocator,
) -> Result<(), String> {
    if vector == 0 || capacity == 0 || capacity > i32::MAX as u32 / 4 {
        return Err("NativeVmValues: unsupported vector resize".into());
    }
    stack::function(m, &[capacity], Some(vector), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, capacity);
        s(m, Reg::Esi, vector);
        let old = m.r32(vector + 4);
        let old_bytes = m.r32(vector) * 4;
        s(m, Reg::Edx, old);
        s(m, Reg::Ecx, capacity * 4);
        let data = stack::external(m, 0x4051ae, &[old, capacity * 4], None, |m| {
            a.reallocate(m, old, old_bytes, capacity * 4)
        })?;
        if data == 0 {
            return Err("NativeVmValues: vector reallocation failed".into());
        }
        s(m, Reg::Eax, data);
        write(m, vector + 4, data);
        if capacity < m.r32(vector + 8) {
            write(m, vector + 8, capacity)
        }
        write(m, vector, capacity);
        Ok(())
    })
}

///405040 deleting variable destructor, including the shared value destructor
/// and the distinct variable-pool free list.
pub fn release_variable(m: &mut Space, cell: u32, a: &mut impl Allocator) -> Result<(), String> {
    if cell == 0 || m.r32(cell) != 0x5e7344 || m.r32(0x62f134) == 0 {
        return Err("NativeVmValues: unsupported variable destructor context".into());
    }
    a.observe_context(
        m,
        crate::native_vm_context::Observation::VariableReleasing { address: cell },
    )?;
    stack::function(m, &[1], Some(cell), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, cell);
        write(m, cell, 0x5e7344);
        stack::call(m, 0x40504e, &[], Some(cell), |m| {
            stack::function(m, &[], Some(cell), |m| {
                write(m, cell, 0x5e734c);
                write(m, 0x62f138, m.r32(0x62f138).wrapping_sub(1));
            })
        });
        let pool = m.r32(0x62f134);
        s(m, Reg::Eax, pool);
        s(m, Reg::Ecx, m.r32(pool + 16));
        write(m, cell, r(m, Reg::Ecx));
        write(m, pool + 16, cell);
        s(m, Reg::Eax, cell);
        a.observe_context(
            m,
            crate::native_vm_context::Observation::VariableReleased { address: cell },
        )?;
        Ok(())
    })
}

///405000 deleting value destructor, including423e90 and the pool free list.
#[allow(dead_code)]
pub fn release(m: &mut Space, cell: u32, e: &mut impl Allocator) -> Result<(), String> {
    if cell == 0 || m.r32(cell) != 0x5e734c || m.r32(0x62f130) == 0 {
        return Err("NativeVmValues: unsupported value destructor context".into());
    }
    e.observe_value(m, Observation::Releasing { address: cell })?;
    stack::function(m, &[1], Some(cell), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, cell);
        stack::call(m, 0x405008, &[], Some(cell), |m| {
            stack::function(m, &[], Some(cell), |m| {
                write(m, cell, 0x5e734c);
                write(m, 0x62f138, m.r32(0x62f138).wrapping_sub(1));
            })
        });
        let pool = m.r32(0x62f130);
        s(m, Reg::Eax, pool);
        s(m, Reg::Ecx, m.r32(pool + 16));
        write(m, cell, r(m, Reg::Ecx));
        write(m, pool + 16, cell);
        s(m, Reg::Eax, cell);
        e.observe_value(m, Observation::Released { address: cell })?;
        Ok(())
    })
}

///408d90 raw256×64 frame pool, including allocator growth and retained CALLs.
#[allow(dead_code)]
pub fn allocate_frame(m: &mut Space, pool: u32, a: &mut impl Allocator) -> Result<u32, String> {
    if pool == 0 || m.r32(pool + 8) > m.r32(pool + 12) || m.r32(pool + 12) > i32::MAX as u32 - 256 {
        return Err("NativeVmValues: malformed original frame pool".into());
    }
    stack::function(m, &[], Some(pool), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, pool);
        let free = m.r32(pool + 16);
        s(m, Reg::Eax, free);
        if free != 0 {
            s(m, Reg::Ecx, m.r32(free));
            write(m, pool + 16, r(m, Reg::Ecx));
            return Ok(free);
        }
        let cap = m.r32(pool + 12);
        s(m, Reg::Eax, cap);
        if m.r32(pool + 8) == cap {
            s(m, Reg::Eax, cap + 256);
            stack::push_reg(m, Reg::Edi);
            let blocks = m.r32(pool + 4);
            s(m, Reg::Edi, blocks);
            write(m, pool + 12, cap + 256);
            s(m, Reg::Eax, blocks + 1);
            s(m, Reg::Edx, (blocks + 1) * 4);
            write(m, pool + 4, blocks + 1);
            let old = m.r32(pool);
            s(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x408dcb, &[], None, |m| {
                a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
            })?;
            s(m, Reg::Eax, table);
            if table == 0 {
                return Err("NativeVmValues: frame pool table allocation failed".into());
            }
            stack::push(m, 0x4000);
            write(m, pool, table);
            let block = stack::external(m, 0x408dd7, &[], None, |m| a.allocate(m, 0x4000))?;
            s(m, Reg::Eax, block);
            if block == 0 {
                return Err("NativeVmValues: frame pool block allocation failed".into());
            }
            s(m, Reg::Ecx, table);
            stack::add_sp(m, 12);
            write(m, table + blocks * 4, block);
            stack::add_sp(m, 4);
        }
        let index = m.r32(pool + 8);
        s(m, Reg::Ecx, index);
        s(m, Reg::Edx, index + 1);
        write(m, pool + 8, index + 1);
        let table = m.r32(pool);
        s(m, Reg::Eax, index / 256);
        s(m, Reg::Edx, table);
        s(m, Reg::Ecx, index % 256);
        let block = m.r32(table + (index / 256) * 4);
        s(m, Reg::Eax, block);
        s(m, Reg::Ecx, (index % 256) * 64);
        let frame = block + (index % 256) * 64;
        s(m, Reg::Eax, frame);
        Ok(frame)
    })
}
///4050b0 raw84000×16 variable pool. Its signed division/GPR effects differ
///from the512×12 value pool even when both allocate from an existing block.
#[allow(dead_code)]
pub fn allocate_variable(m: &mut Space, pool: u32, a: &mut impl Allocator) -> Result<u32, String> {
    if pool == 0
        || m.r32(pool + 8) > m.r32(pool + 12)
        || m.r32(pool + 12) > i32::MAX as u32 - 0x14820
    {
        return Err("NativeVmValues: malformed original variable pool".into());
    }
    stack::function(m, &[], Some(pool), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, pool);
        let free = m.r32(pool + 16);
        s(m, Reg::Eax, free);
        if free != 0 {
            s(m, Reg::Ecx, m.r32(free));
            write(m, pool + 16, r(m, Reg::Ecx));
            a.observe_context(
                m,
                crate::native_vm_context::Observation::VariableAllocated {
                    address: free,
                    pool,
                    freelist: true,
                },
            )?;
            return Ok(free);
        }
        let cap = m.r32(pool + 12);
        s(m, Reg::Eax, cap);
        stack::push_reg(m, Reg::Edi);
        if m.r32(pool + 8) == cap {
            let blocks = m.r32(pool + 4);
            s(m, Reg::Edi, blocks);
            s(m, Reg::Eax, cap + 0x14820);
            write(m, pool + 12, cap + 0x14820);
            s(m, Reg::Eax, blocks + 1);
            s(m, Reg::Edx, (blocks + 1) * 4);
            write(m, pool + 4, blocks + 1);
            let old = m.r32(pool);
            s(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x4050eb, &[], None, |m| {
                a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
            })?;
            s(m, Reg::Eax, table);
            if table == 0 {
                return Err("NativeVmValues: variable pool table allocation failed".into());
            }
            stack::push(m, 0x148200);
            write(m, pool, table);
            let block = stack::external(m, 0x4050f7, &[], None, |m| a.allocate(m, 0x148200))?;
            s(m, Reg::Eax, block);
            if block == 0 {
                return Err("NativeVmValues: variable pool block allocation failed".into());
            }
            s(m, Reg::Ecx, table);
            stack::add_sp(m, 12);
            write(m, table + blocks * 4, block);
        }
        let index = m.r32(pool + 8);
        s(m, Reg::Edi, index);
        s(m, Reg::Edx, index + 1);
        write(m, pool + 8, index + 1);
        let quotient = index / 0x14820;
        let remainder = index % 0x14820;
        s(m, Reg::Ecx, quotient);
        s(m, Reg::Eax, quotient);
        s(m, Reg::Edx, remainder);
        s(m, Reg::Edi, 0x14820);
        let table = m.r32(pool);
        s(m, Reg::Esi, table);
        let block = m.r32(table + quotient * 4);
        s(m, Reg::Eax, block);
        s(m, Reg::Edx, remainder * 16);
        let variable = block + remainder * 16;
        s(m, Reg::Eax, variable);
        a.observe_context(
            m,
            crate::native_vm_context::Observation::VariableAllocated {
                address: variable,
                pool,
                freelist: false,
            },
        )?;
        Ok(variable)
    })
}

// Original41a570 operand/type-vector reservation.
pub fn reserve(
    m: &mut Space,
    vector: u32,
    required: u32,
    a: &mut impl Allocator,
) -> Result<(), String> {
    let capacity = m.r32(vector);
    if vector == 0 || required > i32::MAX as u32 / 8 || capacity > i32::MAX as u32 / 8 {
        return Err("NativeVmRun: invalid operand vector geometry".into());
    }
    stack::function(m, &[required], Some(vector), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, vector);
        s(m, Reg::Eax, capacity);
        s(m, Reg::Ecx, required);
        if (required as i32) > (capacity as i32) {
            stack::push_reg(m, Reg::Edi);
            let next = capacity + required;
            s(m, Reg::Edi, next);
            let old = m.r32(vector + 4);
            s(m, Reg::Edx, old);
            s(m, Reg::Ecx, next * 4);
            stack::push(m, next * 4);
            stack::push(m, old);
            let data = stack::external(m, 0x41a5a7, &[], None, |m| {
                a.reallocate(m, old, capacity * 4, next * 4)
            })?;
            s(m, Reg::Eax, data);
            stack::add_sp(m, 8);
            if data == 0 {
                return Err("NativeVmRun: operand growth allocation failed".into());
            }
            write(m, vector + 4, data);
            if next < m.r32(vector + 8) {
                return Err("NativeVmRun: source truncation continuation unavailable".into());
            }
            write(m, vector, next);
            stack::add_sp(m, 4);
        }
        stack::add_sp(m, 4);
        Ok(())
    })
}

///407d10/407ba0 deleting frame destructor. Values on the operand list are
/// released in reverse order, then the owned locals in declaration order.
pub fn release_frame(
    m: &mut Space,
    frame: u32,
    a: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    a.observe_context(
        m,
        crate::native_vm_context::Observation::FrameRetiring { address: frame },
    )?;
    stack::function(m, &[1], Some(frame), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, frame);
        stack::call(m, 0x407d18, &[], Some(frame), |m| {
            stack::function(m, &[], Some(frame), |m| -> Result<(), String> {
                for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi] {
                    stack::push_reg(m, reg);
                }
                s(m, Reg::Esi, frame);
                let operands = m.r32(frame + 0x28);
                s(m, Reg::Eax, operands);
                write(m, frame, 0x5e7410);
                let count = m.r32(operands + 8);
                s(m, Reg::Ebx, count.wrapping_sub(1));
                s(m, Reg::Ecx, count);
                s(m, Reg::Ebp, 0);
                stack::push_reg(m, Reg::Edi);
                for index in (0..count).rev() {
                    s(m, Reg::Eax, m.r32(frame + 0x28));
                    if index >= m.r32(operands) {
                        return Err("NativeVmValues: frame operand outside owner".into());
                    }
                    s(m, Reg::Edx, m.r32(operands + 4));
                    let cell = m.r32(r(m, Reg::Edx) + index * 4);
                    s(m, Reg::Edi, cell);
                    let vt = m.r32(cell);
                    s(m, Reg::Eax, vt);
                    s(m, Reg::Ecx, cell);
                    let kind = match vt {
                        0x5e734c => 1,
                        0x5e7344 => 2,
                        _ => return Err("NativeVmValues: frame operand type unavailable".into()),
                    };
                    stack::call(m, 0x407bd2, &[], Some(cell), |m| {
                        stack::function(m, &[], Some(cell), |m| s(m, Reg::Eax, kind))
                    });
                    a.observe_context(
                        m,
                        crate::native_vm_context::Observation::OperandDrop {
                            thread: m.r32(frame + 0x3c),
                            frame,
                            index: index as usize,
                            cell,
                        },
                    )?;
                    if kind == 1 {
                        s(m, Reg::Edx, vt);
                        s(m, Reg::Ecx, cell);
                        stack::call(m, 0x407be3, &[1], Some(cell), |m| release(m, cell, a))?;
                    }
                    s(m, Reg::Eax, index);
                    s(m, Reg::Ebx, index.wrapping_sub(1));
                }
                let locals = m.r32(frame + 0x18);
                s(m, Reg::Edi, locals);
                s(m, Reg::Eax, m.r32(locals + 8));
                s(m, Reg::Ebx, 0);
                let count = m.r32(locals + 8);
                for index in 0..count {
                    s(m, Reg::Ecx, m.r32(locals + 4));
                    let cell = m.r32(r(m, Reg::Ecx) + index * 4);
                    s(m, Reg::Ecx, cell);
                    if cell != 0 {
                        a.observe_context(
                            m,
                            crate::native_vm_context::Observation::LocalRetiring {
                                thread: m.r32(frame + 0x3c),
                                frame,
                                index: index as usize,
                                variable: cell,
                            },
                        )?;
                        s(m, Reg::Edx, m.r32(cell));
                        stack::call(m, 0x407c07, &[1], Some(cell), |m| {
                            release_variable(m, cell, a)
                        })?;
                    }
                    s(m, Reg::Eax, m.r32(locals + 4));
                    write(m, r(m, Reg::Eax) + index * 4, 0);
                    s(m, Reg::Eax, m.r32(locals + 8));
                    s(m, Reg::Ebx, index + 1);
                }
                write(m, locals + 8, 0);
                let next = m.r32(frame + 8);
                s(m, Reg::Eax, next);
                if next != 0 {
                    let previous = m.r32(frame + 4);
                    s(m, Reg::Ecx, previous);
                    if previous != 0 {
                        write(m, previous + 8, next);
                        s(m, Reg::Ecx, next);
                        s(m, Reg::Edx, previous);
                        write(m, next + 4, previous);
                    }
                }
                let thread = m.r32(frame + 0x3c);
                s(m, Reg::Ecx, thread);
                write(m, frame + 4, 0);
                write(m, frame + 8, 0);
                s(m, Reg::Eax, m.r32(thread + 0xc));
                let head = m.r32(r(m, Reg::Eax) + 8);
                s(m, Reg::Edx, head);
                let current = if m.r32(head + 4) != 0 { head } else { 0 };
                s(m, Reg::Eax, current);
                write(m, thread + 0x20, current);
                let data = m.r32(frame + 0x20);
                s(m, Reg::Eax, data);
                stack::external(m, 0x407c56, &[data], None, |m| a.free(m, data))?;
                s(m, Reg::Eax, m.r32(frame + 0x24));
                write(m, frame + 0x20, 0);
                if (r(m, Reg::Eax) as i32) > 0 {
                    write(m, frame + 0x24, 0);
                }
                write(m, frame + 0x1c, 0);
                let data = m.r32(frame + 0x10);
                s(m, Reg::Ecx, data);
                stack::external(m, 0x407c72, &[data], None, |m| a.free(m, data))?;
                s(m, Reg::Eax, m.r32(frame + 0x14));
                write(m, frame + 0x10, 0);
                if (r(m, Reg::Eax) as i32) > 0 {
                    write(m, frame + 0x14, 0);
                }
                write(m, frame + 0xc, 0);
                s(m, Reg::Ecx, m.r32(frame + 8));
                write(m, frame, 0x5e7414);
                write(m, frame + 4, 0);
                write(m, frame + 8, 0);
                Ok(())
            })
        })?;
        let pool = m.r32(0x62ddf0);
        s(m, Reg::Eax, pool);
        s(m, Reg::Ecx, m.r32(pool + 0x10));
        write(m, frame, r(m, Reg::Ecx));
        write(m, pool + 0x10, frame);
        s(m, Reg::Eax, frame);
        a.observe_owner(
            m,
            crate::native_vm_context::Observation::FrameReleased { address: frame },
        )?;
        Ok(())
    })
}

///408d60 zero-value constructor with a source-owned canonical descriptor.
pub fn construct_zero(
    m: &mut Space,
    cell: u32,
    jvm: u32,
    descriptor: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    stack::function(m, &[jvm, descriptor], Some(cell), |m| {
        s(m, Reg::Eax, descriptor);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, cell);
        s(m, Reg::Ecx, jvm);
        stack::push(m, descriptor);
        stack::push(m, jvm);
        s(m, Reg::Ecx, cell + 4);
        write(m, cell, 0x5e734c);
        stack::call(m, 0x408d7b, &[], Some(cell + 4), |m| {
            crate::native_vm_signature::descriptor(m, cell + 4, jvm, descriptor, a)
        })?;
        stack::add_sp(m, 8);
        write(m, cell + 8, 0);
        write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
        s(m, Reg::Eax, cell);
        Ok(cell)
    })
}
