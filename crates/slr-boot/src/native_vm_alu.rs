//! Source ALU wrappers reached by the retained Thread.run continuation.
//! Owners are raw value cells. Unsupported type promotion remains explicit.
use crate::{
    native_stack::{self as stack, Reg},
    native_vm_fpu,
};
use slr_physics::space::Space;
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
fn low(m: &mut Space, reg: Reg, value: u8) {
    s(m, reg, (r(m, reg) & !255) | u32::from(value));
}
fn integer(m: &Space, cell: u32) -> Result<(), String> {
    if cell == 0 || m.r32(cell) != 0x5e734c || m.r8(m.r32(cell + 4)) != b'I' {
        return Err("NativeVmAlu: integer value owner required for this source arm".into());
    }
    Ok(())
}
///417570's I/I promotion arm; the virtual type read and stack locals are real.
fn promote_integers(m: &mut Space, jvm: u32, left: u32, right: u32) -> Result<(), String> {
    let left_tag = m.r8(m.r32(left + 4));
    let right_tag = m.r8(m.r32(right + 4));
    if m.r32(left) != 0x5e734c
        || m.r32(right) != 0x5e734c
        || !matches!((left_tag, right_tag), (b'I', b'I') | (b'L', b'L'))
    {
        return Err("NativeVmAlu: promotion source arm unavailable".into());
    }
    stack::function(m, &[left, right], Some(jvm), |m| {
        stack::alloc(m, 0x20);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, left);
        s(m, Reg::Eax, m.r32(left));
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, jvm);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Ecx, left);
        stack::call(m, 0x417582, &[], Some(left), |m| {
            stack::function(m, &[], Some(left), |m| s(m, Reg::Eax, 1))
        });
        s(m, Reg::Edx, m.r32(left + 4));
        low(m, Reg::Eax, left_tag);
        s(m, Reg::Edi, left + 4);
        if left_tag == b'L' {
            s(m, Reg::Ebx, right);
            s(m, Reg::Eax, m.r32(right + 4));
            return Ok(());
        }
        s(m, Reg::Ecx, right);
        s(m, Reg::Edx, m.r32(right + 4));
        low(m, Reg::Eax, b'I');
        s(m, Reg::Edi, right + 4);
        Ok(())
    })
}
///417a70's numeric/non-String predicate. Both actual descriptor strings are
/// compared against the source String descriptors; the stack byte is retained.
fn numeric_string_check(m: &mut Space, jvm: u32, left: u32, right: u32) -> Result<(), String> {
    integer(m, left)?;
    integer(m, right)?;
    stack::function(m, &[left, right], Some(jvm), |m| {
        stack::alloc(m, 0x24);
        s(m, Reg::Eax, left);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, m.r32(left + 4));
        s(m, Reg::Eax, left + 4);
        s(m, Reg::Ebp, jvm);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edx, 0);
        s(m, Reg::Edi, 0x60b9ac);
        s(m, Reg::Ecx, 0x13);
        if m.r8(0x60b9ac) != b'L' || m.r8(0x60b9c0) != b'L' {
            return Err("NativeVmAlu: source String descriptors unavailable".into());
        }
        s(m, Reg::Esi, r(m, Reg::Esi) + 1);
        s(m, Reg::Edi, 0x60b9ad);
        s(m, Reg::Ecx, 0x12);
        s(m, Reg::Ecx, right);
        s(m, Reg::Esi, m.r32(right + 4));
        s(m, Reg::Edx, right + 4);
        m.w8(stack::sp(m) + 0x13, 0);
        s(m, Reg::Ebx, 0);
        s(m, Reg::Edi, 0x60b9c0);
        s(m, Reg::Ecx, 0x13);
        s(m, Reg::Esi, r(m, Reg::Esi) + 1);
        s(m, Reg::Edi, 0x60b9c1);
        s(m, Reg::Ecx, 0x12);
        low(m, Reg::Ecx, 0);
        low(m, Reg::Ebx, 0);
        Ok(())
    })
}
fn add_integers(m: &mut Space, converter: u32, left: u32, right: u32) -> Result<(), String> {
    stack::function(m, &[left, right], Some(converter), |m| {
        stack::alloc(m, 0x24);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, right);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, left);
        s(m, Reg::Ebx, converter);
        let jvm = m.r32(converter);
        s(m, Reg::Ecx, jvm);
        stack::call(m, 0x41b4ea, &[left, right], Some(jvm), |m| {
            numeric_string_check(m, jvm, left, right)
        })?;
        stack::call(m, 0x41b4f3, &[left, right], Some(jvm), |m| {
            promote_integers(m, jvm, left, right)
        })?;
        s(m, Reg::Esi, m.r32(left + 4));
        low(m, Reg::Eax, b'I');
        s(m, Reg::Edx, m.r32(right + 8));
        write(m, left + 8, m.r32(left + 8).wrapping_add(r(m, Reg::Edx)));
        Ok(())
    })
}
///41cb60 <=,41c5c0 ==,41b760 subtraction,41bfb0 logical OR.
pub fn binary(
    m: &mut Space,
    converter: u32,
    left: u32,
    right: u32,
    kind: u32,
    fpu: &mut native_vm_fpu::State,
) -> Result<(), String> {
    if kind == 7 && m.r8(m.r32(left + 4)) == b'L' && m.r8(m.r32(right + 4)) == b'L' {
        if m.r32(left) != 0x5e734c || m.r32(right) != 0x5e734c {
            return Err("NativeVmAlu: reference value owner required".into());
        }
    } else {
        integer(m, left)?;
        integer(m, right)?;
    }
    match kind {
        7 | 9 => stack::function(m, &[left, right], Some(converter), |m| {
            let jvm = m.r32(converter);
            s(m, Reg::Ecx, jvm);
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, left);
            stack::push_reg(m, Reg::Edi);
            s(m, Reg::Edi, right);
            let pc = if kind == 9 { 0x41cb73 } else { 0x41c5d3 };
            stack::call(m, pc, &[left, right], Some(jvm), |m| {
                promote_integers(m, jvm, left, right)
            })?;
            let result = if kind == 9 {
                s(m, Reg::Eax, m.r32(left + 4));
                low(m, Reg::Eax, b'I');
                s(m, Reg::Ecx, m.r32(left + 8));
                s(m, Reg::Edx, m.r32(right + 8));
                s(
                    m,
                    Reg::Eax,
                    u32::from((r(m, Reg::Ecx) as i32) > (r(m, Reg::Edx) as i32)),
                );
                s(m, Reg::Edx, u32::from(r(m, Reg::Eax) == 0));
                write(m, left + 8, r(m, Reg::Edx));
                s(m, Reg::Eax, m.r32(0x62e00c));
                s(m, Reg::Ecx, m.r32(r(m, Reg::Eax)));
                write(m, left + 4, r(m, Reg::Ecx));
                r(m, Reg::Edx)
            } else {
                s(m, Reg::Eax, m.r32(left + 4));
                s(m, Reg::Eax, u32::from(m.r8(r(m, Reg::Eax))) - 0x46);
                s(m, Reg::Ecx, 0);
                s(m, Reg::Ecx, m.r32(right + 8));
                s(m, Reg::Eax, u32::from(m.r32(left + 8) == r(m, Reg::Ecx)));
                let result = r(m, Reg::Eax);
                write(m, left + 8, result);
                s(m, Reg::Edx, m.r32(0x62e00c));
                s(m, Reg::Eax, m.r32(r(m, Reg::Edx)));
                write(m, left + 4, r(m, Reg::Eax));
                result
            };
            let _ = result;
            Ok(())
        }),
        0xf => stack::function(m, &[left, right], Some(converter), |m| {
            stack::alloc(m, 0x20);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            s(m, Reg::Edi, right);
            s(m, Reg::Esi, converter);
            let jvm = m.r32(converter);
            s(m, Reg::Ecx, jvm);
            stack::call(m, 0x41b773, &[right], Some(jvm), |m| {
                stack::function(m, &[right], Some(jvm), |_| {})
            });
            s(m, Reg::Eax, m.r32(right + 4));
            low(m, Reg::Eax, b'I');
            s(m, Reg::Ecx, m.r32(right + 8).wrapping_neg());
            write(m, right + 8, r(m, Reg::Ecx));
            s(m, Reg::Edx, left);
            stack::call(m, 0x41b851, &[left, right], Some(converter), |m| {
                add_integers(m, converter, left, right)
            })
        }),
        1 | 2 => stack::function(m, &[left, right], Some(converter), |m| {
            stack::alloc(m, 0x20);
            for reg in [Reg::Ebx, Reg::Esi, Reg::Edi] {
                stack::push_reg(m, reg);
            }
            s(m, Reg::Edi, left);
            s(m, Reg::Esi, converter);
            let jvm = m.r32(converter);
            stack::call(
                m,
                if kind == 1 { 0x41bfc4 } else { 0x41beb4 },
                &[left],
                Some(jvm),
                |m| crate::native_vm_thread_run::as_boolean(m, left, fpu),
            )?;
            s(m, Reg::Ebx, right);
            stack::call(
                m,
                if kind == 1 { 0x41bfd0 } else { 0x41bec0 },
                &[right],
                Some(jvm),
                |m| crate::native_vm_thread_run::as_boolean(m, right, fpu),
            )?;
            s(m, Reg::Eax, m.r32(left + 4));
            s(m, Reg::Eax, m.r32(left + 8));
            if (kind == 1 && r(m, Reg::Eax) == 0) || (kind == 2 && r(m, Reg::Eax) != 0) {
                s(m, Reg::Eax, m.r32(right + 8));
            }
            let result = u32::from(r(m, Reg::Eax) != 0);
            s(m, Reg::Eax, result);
            write(m, left + 8, result);
            Ok(())
        }),
        _ => Err("NativeVmAlu: source operator continuation unavailable".into()),
    }
}

///41ab60's identical integer-descriptor assignment, retaining4178f0's
///conversion check and its actual stack/register history.
pub fn assign_integer(
    m: &mut Space,
    converter: u32,
    destination: u32,
    source: u32,
    fpu: &mut native_vm_fpu::State,
) -> Result<(), String> {
    let destination_tag = m.r8(m.r32(destination + 4));
    let source_tag = m.r8(m.r32(source + 4));
    if m.r32(destination) != 0x5e7344
        || m.r32(source) != 0x5e734c
        || !(matches!(
            (destination_tag, source_tag),
            (b'I', b'I') | (b'F', b'I') | (b'F', b'F')
        ) || (matches!(destination_tag, b'L' | b'[')
            && destination_tag == source_tag
            && m.r32(destination + 4) == m.r32(source + 4)))
    {
        return Err("NativeVmAlu: assignment conversion source arm unavailable".into());
    }
    stack::function(m, &[destination, source], Some(converter), |m| {
        stack::alloc(m, 0x20);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, source);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, destination);
        s(m, Reg::Esi, converter);
        let jvm = m.r32(converter);
        s(m, Reg::Ecx, jvm);
        stack::call(m, 0x41ab79, &[destination, source], Some(jvm), |m| {
            stack::function(m, &[destination, source], Some(jvm), |m| {
                s(m, Reg::Eax, destination);
                stack::alloc(m, 0x20);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                stack::push_reg(m, Reg::Edi);
                s(m, Reg::Esi, jvm);
                s(m, Reg::Ebp, source);
                s(m, Reg::Ecx, m.r32(source + 4));
                s(m, Reg::Edi, destination + 4);
                s(m, Reg::Eax, m.r32(destination + 4));
                if m.r32(destination + 4) != m.r32(source + 4) {
                    if destination_tag != b'F' || source_tag != b'I' {
                        return Err(
                            "NativeVmAlu: scalar conversion continuation unavailable".into()
                        );
                    }
                    stack::call(m, 0x417929, &[source, destination + 4], Some(jvm), |m| {
                        stack::function(m, &[source, destination + 4], Some(jvm), |m| {
                            stack::alloc(m, 0x100);
                            stack::push_reg(m, Reg::Ebx);
                            s(m, Reg::Ebx, destination + 4);
                            s(m, Reg::Eax, m.r32(destination + 4));
                            stack::push_reg(m, Reg::Ebp);
                            stack::push_reg(m, Reg::Esi);
                            s(m, Reg::Esi, source);
                            s(m, Reg::Ebp, m.r32(source + 4));
                            stack::push_reg(m, Reg::Edi);
                            s(m, Reg::Edi, jvm);
                            s(m, Reg::Ecx, 0);
                            s(m, Reg::Edx, 0);
                            fpu.fild_i64(i64::from(m.r32(source + 8) as i32))?;
                            s(m, Reg::Eax, 1);
                            write(m, source + 8, fpu.fstp_f32()?);
                            s(m, Reg::Ecx, m.r32(destination + 4));
                            write(m, source + 4, r(m, Reg::Ecx));
                            Ok::<(), String>(())
                        })
                    })?;
                }
                Ok::<(), String>(())
            })
        })?;
        s(m, Reg::Eax, m.r32(destination + 4));
        s(m, Reg::Eax, u32::from(destination_tag) - 0x46);
        s(
            m,
            Reg::Ecx,
            if destination_tag == b'I' {
                1
            } else if matches!(destination_tag, b'L' | b'[') {
                2
            } else {
                0
            },
        );
        if matches!(destination_tag, b'L' | b'[') {
            s(m, Reg::Ecx, m.r32(source + 8));
            write(m, destination + 8, r(m, Reg::Ecx));
            let jvm = m.r32(converter);
            s(m, Reg::Ecx, jvm);
            s(m, Reg::Eax, m.r32(jvm + 0x20));
            if r(m, Reg::Eax) != 0 {
                return Err(
                    "NativeVmAlu: active-GC reference assignment continuation required".into(),
                );
            }
        } else if destination_tag == b'F' {
            s(m, Reg::Eax, m.r32(source + 8));
            write(m, destination + 8, r(m, Reg::Eax));
        } else {
            s(m, Reg::Edx, m.r32(source + 8));
            write(m, destination + 8, r(m, Reg::Edx));
        }
        Ok(())
    })
}
///41acb0's plain-assignment case (operation0x23).
pub fn assignment(
    m: &mut Space,
    converter: u32,
    destination: u32,
    source: u32,
    fpu: &mut native_vm_fpu::State,
) -> Result<(), String> {
    stack::function(m, &[destination, source, 0x23], Some(converter), |m| {
        s(m, Reg::Eax, 0x23);
        stack::alloc(m, 0xc);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Eax, 0);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Esi, converter);
        s(m, Reg::Ecx, source);
        s(m, Reg::Edx, destination);
        stack::call(m, 0x41ace0, &[destination, source], Some(converter), |m| {
            assign_integer(m, converter, destination, source, fpu)
        })
    })
}
