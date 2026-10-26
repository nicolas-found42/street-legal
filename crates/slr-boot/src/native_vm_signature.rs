//! Source407ac0/41dc30 construction with existing canonical signature names.
//! Missing canonical names remain an explicit boundary. This module never
//! substitutes semantic identities for original holder/table/record addresses.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! { static OBSERVER: std::cell::Cell<Option<StoreObserver>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn set_test_store_observer(observer: Option<StoreObserver>) {
    OBSERVER.with(|slot| slot.set(observer));
}
fn observe(address: u32, size: u32, value: u32) {
    #[cfg(test)]
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(address, size, value);
        }
    });
    #[cfg(not(test))]
    let _ = (address, size, value);
}
fn write(m: &mut Space, address: u32, value: u32) {
    observe(address, 4, value);
    m.w32(address, value);
}
fn store_byte(m: &mut Space, address: u32, value: u8) {
    observe(address, 1, u32::from(value));
    m.w8(address, value);
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, v: u32) {
    stack::set_reg(m, reg, v)
}
fn low(m: &mut Space, reg: Reg, v: u8) {
    s(m, reg, (r(m, reg) & !255) | u32::from(v))
}
fn signed(v: u8) -> u32 {
    i32::from(v as i8) as u32
}
fn hash(m: &mut Space, p: u32, n: u32) -> u32 {
    if n == 0 {
        s(m, Reg::Eax, 0);
        0
    } else if n <= 3 {
        let x = signed(m.r8(p)).wrapping_add(n);
        s(m, Reg::Eax, x);
        x
    } else {
        s(m, Reg::Edx, signed(m.r8(p + 3)));
        s(m, Reg::Eax, signed(m.r8(p + n - 4)));
        let x = r(m, Reg::Eax).wrapping_add(r(m, Reg::Edx));
        s(m, Reg::Eax, x);
        s(m, Reg::Edx, x.wrapping_mul(9));
        s(m, Reg::Eax, x.wrapping_mul(73));
        s(m, Reg::Edx, signed(m.r8(p + n - 2)));
        let value = r(m, Reg::Eax).wrapping_add(r(m, Reg::Edx)).wrapping_add(n);
        s(m, Reg::Eax, value);
        value
    }
}
fn lookup(m: &mut Space, table: u32, key: u32) -> Result<u32, String> {
    stack::function(m, &[key], Some(table), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, key);
        s(m, Reg::Edx, 0);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Eax, key);
        s(m, Reg::Edi, 0xa97);
        let mut node = table + (key % 0xa97) * 12;
        s(m, Reg::Eax, (key % 0xa97) * 3);
        s(m, Reg::Edx, m.r32(node + 4));
        s(m, Reg::Eax, node);
        if r(m, Reg::Edx) != 0 {
            for _ in 0..1024 {
                if node == 0 {
                    break;
                }
                if m.r32(node) == key {
                    write(m, table + 0x7f3c, node);
                    let slot = m.r32(node + 4);
                    s(m, Reg::Eax, slot);
                    if slot != 0 {
                        let record = m.r32(slot);
                        s(m, Reg::Eax, record);
                        return Ok(record);
                    }
                    break;
                }
                node = m.r32(node + 8);
                s(m, Reg::Eax, node);
            }
        }
        s(m, Reg::Eax, 0);
        Ok(0)
    })
}
fn next(m: &mut Space, table: u32) -> u32 {
    stack::function(m, &[], Some(table), |m| {
        let previous = m.r32(table + 0x7f3c);
        s(m, Reg::Edx, previous);
        let mut node = m.r32(previous + 8);
        s(m, Reg::Eax, node);
        if node != 0 {
            let key = m.r32(previous);
            s(m, Reg::Edx, key);
            for _ in 0..1024 {
                if node == 0 {
                    break;
                }
                if m.r32(node) == key {
                    write(m, table + 0x7f3c, node);
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
}
fn equal(m: &mut Space, mut left: u32, mut right: u32) -> Result<bool, String> {
    s(m, Reg::Ecx, left);
    s(m, Reg::Esi, right);
    for _ in 0..1024 {
        for off in 0..2 {
            let a = m.r8(left + off);
            let b = m.r8(right + off);
            low(m, Reg::Eax, a);
            low(m, Reg::Edx, a);
            if a != b {
                s(m, Reg::Ecx, if a < b { u32::MAX } else { 1 });
                return Ok(false);
            }
            if off == 0 && a == 0 {
                s(m, Reg::Ecx, 0);
                return Ok(true);
            }
            if off == 1 {
                left += 2;
                right += 2;
                s(m, Reg::Ecx, left);
                s(m, Reg::Esi, right);
                if a == 0 {
                    s(m, Reg::Ecx, 0);
                    return Ok(true);
                }
            }
        }
    }
    Err("NativeVmSignature: unterminated canonical name".into())
}
fn intern_existing(
    m: &mut Space,
    holder: u32,
    source: u32,
    len: u32,
    mut allocator: Option<&mut dyn Allocator>,
) -> Result<u32, String> {
    let table = m.r32(holder + 4);
    stack::function(m, &[source, len], Some(holder), |m| {
        stack::push_reg(m, Reg::Ecx);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, source);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::store(m, 0x10, holder);
        s(m, Reg::Ebp, len);
        let key = hash(m, source, len);
        let mut record = stack::call(m, 0x41e660, &[key], Some(table), |m| lookup(m, table, key))?;
        s(m, Reg::Edi, record);
        for _ in 0..1024 {
            if record == 0 {
                let a = allocator
                    .take()
                    .ok_or("NativeVmSignature: canonical name allocation remains unresolved")?;
                return crate::native_vm_bootstrap::allocate_name(m, holder, source, len, key, a);
            }
            if m.r32(record + 8) == len && equal(m, m.r32(record + 4), source)? {
                write(m, record + 12, m.r32(record + 12).wrapping_add(1));
                let text = m.r32(record + 4);
                s(m, Reg::Eax, text);
                s(m, Reg::Ecx, holder);
                return Ok(text);
            }
            s(m, Reg::Ecx, holder);
            let slot = stack::call(m, 0x41e6ad, &[], Some(table), |m| next(m, table));
            record = if slot == 0 { 0 } else { m.r32(slot) };
            if slot != 0 {
                s(m, Reg::Edi, record);
            }
        }
        Err("NativeVmSignature: cyclic canonical name chain".into())
    })
}
pub(crate) fn canonical(m: &mut Space, holder: u32, source: u32) -> Result<u32, String> {
    canonical_impl(m, holder, source, None)
}
pub(crate) fn canonical_with_allocator(
    m: &mut Space,
    holder: u32,
    source: u32,
    allocator: &mut dyn Allocator,
) -> Result<u32, String> {
    canonical_impl(m, holder, source, Some(allocator))
}
fn canonical_impl(
    m: &mut Space,
    holder: u32,
    source: u32,
    allocator: Option<&mut dyn Allocator>,
) -> Result<u32, String> {
    let len = (0..1024)
        .find(|i| m.r8(source + i) == 0)
        .ok_or("NativeVmSignature: unterminated signature")?;
    if holder == 0 || m.r32(holder + 4) == 0 {
        return Err("NativeVmSignature: missing original canonical holder".into());
    }
    stack::function(m, &[source], Some(holder), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, source);
        s(m, Reg::Edx, holder);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, source + len + 1);
        s(m, Reg::Ecx, len);
        stack::call(m, 0x41e81d, &[source, len], Some(holder), |m| {
            intern_existing(m, holder, source, len, allocator)
        })
    })
}
///423e10 canonical descriptor-cell constructor with actual source CALLs.
pub fn descriptor(
    m: &mut Space,
    cell: u32,
    jvm: u32,
    source: u32,
    a: &mut impl Allocator,
) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) || cell == 0 || jvm == 0 {
        return Err("NativeVmSignature: actual descriptor constructor CALL required".into());
    }
    stack::function(m, &[jvm, source], Some(cell), |m| {
        s(m, Reg::Eax, source);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, cell);
        s(m, Reg::Ecx, jvm);
        let holder = m.r32(jvm + 0x68);
        s(m, Reg::Ecx, holder);
        let pointer = stack::call(m, 0x423e24, &[source], Some(holder), |m| {
            canonical_impl(m, holder, source, Some(a))
        })?;
        write(m, cell, pointer);
        Ok(())
    })
}
///407ac0's real lazy method signature allocation and41dc30 constructor. A
///signature already present in the canonical table is required for this seam.
#[allow(dead_code)]
pub fn method(
    m: &mut Space,
    method: u32,
    jvm: u32,
    pool: u32,
    a: &mut dyn Allocator,
) -> Result<u32, String> {
    stack::function(m, &[jvm, pool], Some(method), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, method);
        let cached = m.r32(method + 24);
        s(m, Reg::Eax, cached);
        if cached != 0 {
            return Ok(cached);
        }
        stack::push_reg(m, Reg::Edi);
        let signature = stack::external(m, 0x407ad2, &[12], None, |m| a.allocate(m, 12))?;
        s(m, Reg::Eax, signature);
        s(m, Reg::Edi, signature);
        if signature == 0 {
            write(m, method + 24, 0);
            return Err("NativeVmSignature: method signature allocation failed".into());
        }
        let index = m.r32(method + 12);
        s(m, Reg::Eax, index);
        if (index as i32) < 0 || index >= m.r32(pool + 8) {
            return Err("NativeVmSignature: invalid signature constant index".into());
        }
        let constant = m.r32(m.r32(pool + 12) + index * 4);
        if constant == 0 || m.r32(constant + 4) != 0 {
            return Err("NativeVmSignature: signature constant is not a string".into());
        }
        let source = stack::call(m, 0x407ae8, &[index], Some(pool), |m| {
            stack::function(m, &[index], Some(pool), |m| {
                s(m, Reg::Eax, index);
                s(m, Reg::Ecx, m.r32(pool + 12));
                s(m, Reg::Eax, constant);
                s(m, Reg::Ecx, 0);
                s(m, Reg::Eax, m.r32(constant + 8));
                r(m, Reg::Eax)
            })
        });
        s(m, Reg::Ecx, jvm);
        let result = stack::call(m, 0x407af5, &[jvm, source], Some(signature), |m| {
            construct(m, signature, jvm, source, a)
        })?;
        write(m, method + 24, result);
        Ok(result)
    })
}

//41dfd0's ()V path. General parameter/reference parsing remains guarded.
fn parse_void(m: &mut Space, signature: u32, a: &mut impl Allocator) -> Result<(), String> {
    let source = m.r32(signature + 4);
    let holder = m.r32(0x62e010);
    if (0..4).map(|i| m.r8(source + i)).collect::<Vec<_>>() != b"()V\0"
        || holder == 0
        || m.r32(holder) == 0
    {
        return Err("NativeVmSignature: unproved uncached parameter/return parser path".into());
    }
    stack::function(m, &[], Some(signature), |m| {
        stack::alloc(m, 0x108);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::push(m, 12);
        s(m, Reg::Ebx, signature);
        let types = stack::external(m, 0x41dfe3, &[], None, |m| a.allocate(m, 12))?;
        s(m, Reg::Eax, types);
        s(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        if types == 0 {
            return Err("NativeVmSignature: type vector allocation failed".into());
        }
        write(m, types, 0);
        write(m, types + 8, 0);
        write(m, types + 4, 0);
        s(m, Reg::Esi, source + 1);
        write(m, signature + 8, types);
        low(m, Reg::Eax, b')');
        s(m, Reg::Ecx, 0);
        s(m, Reg::Edx, 0);
        //')' classifier table index0 and its byte-load into DL.
        s(m, Reg::Ecx, 0);
        low(m, Reg::Edx, 0);
        low(m, Reg::Eax, b'V');
        s(m, Reg::Esi, source + 2);
        s(m, Reg::Ecx, 0);
        stack::store(m, 0x14, 0);
        //Loop classifier for V: index0x2d -> jump-table case4.
        low(m, Reg::Eax, b'V');
        s(m, Reg::Ecx, 0x2d);
        s(m, Reg::Edx, 4);
        let cell = stack::external(m, 0x41e184, &[4], None, |m| a.allocate(m, 4))?;
        s(m, Reg::Eax, cell);
        if cell == 0 {
            return Err("NativeVmSignature: void type allocation failed".into());
        }
        s(m, Reg::Edx, holder);
        s(m, Reg::Ecx, m.r32(holder));
        write(m, cell, r(m, Reg::Ecx));
        s(m, Reg::Ebp, cell);
        s(m, Reg::Edi, types);
        s(m, Reg::Eax, 1);
        s(m, Reg::Ecx, 1);
        stack::call(m, 0x41e1b2, &[1], Some(types), |m| {
            crate::native_vm_values::resize(m, types, 1, a)
        })?;
        s(m, Reg::Edx, m.r32(types + 8));
        s(m, Reg::Eax, m.r32(types + 4));
        write(m, r(m, Reg::Eax), cell);
        write(m, types + 8, 1);
        s(m, Reg::Esi, source + 3);
        low(m, Reg::Eax, 0);
        s(m, Reg::Ebp, 0);
        Ok(())
    })
}
///41e2d0 parameter count, materializing the source ()V types when absent.
#[allow(dead_code)]
pub fn count(m: &mut Space, signature: u32, a: &mut impl Allocator) -> Result<u32, String> {
    stack::function(m, &[], Some(signature), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, signature);
        s(m, Reg::Eax, m.r32(signature + 8));
        if r(m, Reg::Eax) == 0 {
            stack::call(m, 0x41e2df, &[], Some(signature), |m| {
                parse_empty(m, signature, a)
            })?;
        }
        let types = m.r32(signature + 8);
        s(m, Reg::Eax, types);
        s(m, Reg::Eax, m.r32(types + 8).wrapping_sub(1));
        Ok(r(m, Reg::Eax))
    })
}

pub fn construct(
    m: &mut Space,
    signature: u32,
    jvm: u32,
    source: u32,
    a: &mut dyn Allocator,
) -> Result<u32, String> {
    stack::function(m, &[jvm, source], Some(signature), |m| {
        s(m, Reg::Eax, jvm);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, signature);
        s(m, Reg::Ecx, source);
        write(m, signature + 8, 0);
        write(m, signature, jvm);
        let holder = m.r32(jvm + 0x68);
        let text = stack::call(m, 0x41dc4d, &[source], Some(holder), |m| {
            canonical_with_allocator(m, holder, source, a)
        })?;
        write(m, signature + 4, text);
        s(m, Reg::Eax, signature);
        Ok(signature)
    })
}

fn parse_primitives(m: &mut Space, signature: u32, a: &mut impl Allocator) -> Result<(), String> {
    let source = m.r32(signature + 4);
    stack::function(m, &[], Some(signature), |m| {
        stack::alloc(m, 0x108);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        stack::push(m, 12);
        s(m, Reg::Ebx, signature);
        let types = stack::external(m, 0x41dfe3, &[], None, |m| a.allocate(m, 12))?;
        s(m, Reg::Eax, types);
        s(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        if types == 0 {
            return Err("NativeVmSignature: primitive types allocation failed".into());
        }
        for off in [0, 8, 4] {
            write(m, types + off, 0);
        }
        s(m, Reg::Esi, source + 1);
        write(m, signature + 8, types);
        let mut cursor = source + 1;
        let mut separator = false;
        loop {
            let tag = m.r8(cursor);
            low(m, Reg::Eax, tag);
            if tag == 0 {
                break;
            }
            s(m, Reg::Ecx, u32::from(tag).wrapping_sub(0x29));
            s(
                m,
                Reg::Edx,
                match tag {
                    b')' => 0,
                    b'F' => 1,
                    b'I' => 2,
                    b'L' => 3,
                    b'V' => 4,
                    _ => {
                        return Err(
                            "NativeVmSignature: nonprimitive parser continuation unavailable"
                                .into(),
                        )
                    }
                },
            );
            if tag == b')' {
                cursor += 1;
                low(m, Reg::Eax, m.r8(cursor));
                s(m, Reg::Esi, cursor);
                s(m, Reg::Ecx, u32::from(m.r8(cursor) == 0));
                stack::store(m, 0x14, r(m, Reg::Ecx));
                separator = true;
                continue;
            }
            if tag == b'L' {
                let buffer = stack::sp(m) + 0x18;
                low(m, Reg::Ecx, tag);
                cursor += 1;
                s(m, Reg::Esi, cursor);
                store_byte(m, buffer, tag);
                let mut destination = buffer + 1;
                s(m, Reg::Eax, destination);
                loop {
                    if destination - buffer >= 255 {
                        return Err(
                            "NativeVmSignature: reference descriptor exceeds source buffer".into(),
                        );
                    }
                    let byte = m.r8(cursor);
                    if byte == 0 {
                        return Err("NativeVmSignature: unterminated reference descriptor".into());
                    }
                    low(m, Reg::Ecx, byte);
                    store_byte(m, destination, byte);
                    cursor += 1;
                    destination += 1;
                    s(m, Reg::Eax, destination);
                    s(m, Reg::Esi, cursor);
                    if byte == b';' {
                        break;
                    }
                }
                stack::push(m, 4);
                store_byte(m, destination, 0);
                let cell = stack::external(m, 0x41e137, &[], None, |m| a.allocate(m, 4))?;
                s(m, Reg::Eax, cell);
                s(m, Reg::Edi, cell);
                stack::add_sp(m, 4);
                if cell == 0 {
                    return Err("NativeVmSignature: reference type allocation failed".into());
                }
                let jvm = m.r32(signature);
                s(m, Reg::Edx, jvm);
                s(m, Reg::Ecx, buffer);
                stack::call(m, 0x41e14f, &[jvm, buffer], Some(cell), |m| {
                    descriptor(m, cell, jvm, buffer, a)
                })?;
                s(m, Reg::Ebp, cell);
                s(m, Reg::Edi, types);
                let count = m.r32(types + 8);
                let capacity = m.r32(types);
                s(m, Reg::Eax, count + 1);
                s(m, Reg::Ecx, capacity);
                if count + 1 > capacity {
                    let next = capacity + count + 1;
                    s(m, Reg::Ecx, next);
                    stack::call(m, 0x41e16c, &[next], Some(types), |m| {
                        crate::native_vm_values::resize(m, types, next, a)
                    })?;
                }
                s(m, Reg::Eax, m.r32(types + 8));
                s(m, Reg::Ecx, m.r32(types + 4));
                write(m, r(m, Reg::Ecx) + count * 4, cell);
                write(m, types + 8, count + 1);
                continue;
            }
            let (global, alloc_pc) = match tag {
                b'I' => (0x62e00c, 0x41e02e),
                b'F' => (0x62deec, 0x41e06d),
                b'V' => (0x62e010, 0x41e184),
                _ => unreachable!(),
            };
            let cell = stack::external(m, alloc_pc, &[4], None, |m| a.allocate(m, 4))?;
            s(m, Reg::Eax, cell);
            if cell == 0 {
                return Err("NativeVmSignature: primitive type allocation failed".into());
            }
            if tag == b'V' {
                s(m, Reg::Edx, m.r32(global));
                s(m, Reg::Ecx, m.r32(r(m, Reg::Edx)));
                write(m, cell, r(m, Reg::Ecx));
            } else {
                s(m, Reg::Ecx, m.r32(global));
                s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
                write(m, cell, r(m, Reg::Edx));
            }
            s(m, Reg::Ebp, cell);
            s(m, Reg::Edi, types);
            let count = m.r32(types + 8);
            s(m, Reg::Eax, count + 1);
            if tag == b'I' {
                stack::call(m, 0x41e054, &[count + 1], Some(types), |m| {
                    crate::native_vm_values::reserve(m, types, count + 1, a)
                })?;
                s(m, Reg::Ecx, m.r32(types + 8));
                s(m, Reg::Edx, m.r32(types + 4));
                write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, cell);
            } else {
                let capacity = m.r32(types);
                s(m, Reg::Ecx, capacity);
                if count + 1 > capacity {
                    let new_capacity = capacity + count + 1;
                    s(m, Reg::Ecx, new_capacity);
                    stack::call(
                        m,
                        if tag == b'F' { 0x41e09b } else { 0x41e1b2 },
                        &[new_capacity],
                        Some(types),
                        |m| crate::native_vm_values::resize(m, types, new_capacity, a),
                    )?;
                }
                if tag == b'F' {
                    s(m, Reg::Eax, m.r32(types + 8));
                    s(m, Reg::Ecx, m.r32(types + 4));
                    write(m, r(m, Reg::Ecx) + r(m, Reg::Eax) * 4, cell);
                } else {
                    s(m, Reg::Edx, m.r32(types + 8));
                    s(m, Reg::Eax, m.r32(types + 4));
                    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, cell);
                }
            }
            write(m, types + 8, count + 1);
            cursor += 1;
            s(m, Reg::Esi, cursor);
        }
        s(m, Reg::Ebp, 0);
        if !separator {
            return Err("NativeVmSignature: missing parameter separator".into());
        }
        if stack::load(m, 0x14) != 0 {
            s(m, Reg::Esi, types);
            let count = m.r32(types + 8);
            let capacity = m.r32(types);
            s(m, Reg::Eax, count + 1);
            s(m, Reg::Ecx, capacity);
            if count + 1 > capacity {
                let next = capacity + count + 1;
                s(m, Reg::Edi, next);
                if next == 0 {
                    return Err("NativeVmSignature: absent-return size overflow".into());
                }
                let old = m.r32(types + 4);
                s(m, Reg::Ecx, old);
                s(m, Reg::Eax, next * 4);
                let data = stack::external(m, 0x41e259, &[old, next * 4], None, |m| {
                    a.reallocate(m, old, capacity * 4, next * 4)
                })?;
                s(m, Reg::Eax, data);
                write(m, types + 4, data);
                if next < count {
                    write(m, types + 8, next);
                }
                write(m, types, next);
            }
            s(m, Reg::Edx, m.r32(types + 8));
            s(m, Reg::Eax, m.r32(types + 4));
            write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, 0);
            write(m, types + 8, m.r32(types + 8) + 1);
        }
        Ok(())
    })
}
fn parse_absent(m: &mut Space, signature: u32, a: &mut impl Allocator) -> Result<(), String> {
    let source = m.r32(signature + 4);
    stack::function(m, &[], Some(signature), |m| {
        stack::alloc(m, 0x108);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        stack::push(m, 12);
        s(m, Reg::Ebx, signature);
        let types = stack::external(m, 0x41dfe3, &[], None, |m| a.allocate(m, 12))?;
        s(m, Reg::Eax, types);
        s(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        if types == 0 {
            return Err("NativeVmSignature: absent-return types allocation failed".into());
        }
        for off in [0, 8, 4] {
            write(m, types + off, 0);
        }
        s(m, Reg::Esi, source + 1);
        write(m, signature + 8, types);
        low(m, Reg::Eax, b')');
        s(m, Reg::Ecx, 0);
        s(m, Reg::Edx, 0);
        low(m, Reg::Eax, 0);
        s(m, Reg::Esi, source + 2);
        s(m, Reg::Ecx, 1);
        stack::store(m, 0x14, 1);
        low(m, Reg::Eax, 0);
        s(m, Reg::Ebp, 0);
        s(m, Reg::Esi, types);
        s(m, Reg::Eax, 1);
        s(m, Reg::Ecx, 0);
        s(m, Reg::Edi, 1);
        s(m, Reg::Ecx, 0);
        s(m, Reg::Eax, 4);
        let data = stack::external(m, 0x41e259, &[0, 4], None, |m| a.reallocate(m, 0, 0, 4))?;
        s(m, Reg::Eax, data);
        write(m, types + 4, data);
        write(m, types, 1);
        s(m, Reg::Edx, 0);
        s(m, Reg::Eax, data);
        write(m, data, 0);
        write(m, types + 8, 1);
        Ok(())
    })
}
/// Original41dfd0 empty-parameter primitive signatures. Other grammar arms
/// stay explicit until their instruction histories have comparisons.
pub fn parse_empty(m: &mut Space, signature: u32, a: &mut impl Allocator) -> Result<(), String> {
    let source = m.r32(signature.wrapping_add(4));
    if (0..3).map(|i| m.r8(source + i)).collect::<Vec<_>>() == b"()\0" {
        return parse_absent(m, signature, a);
    }
    if (0..4).map(|i| m.r8(source + i)).collect::<Vec<_>>() == b"()V\0" {
        return parse_void(m, signature, a);
    }
    if m.r8(source) == b'(' && (m.r8(source + 1) != b')' || m.r8(source + 2) == b'L') {
        return parse_primitives(m, signature, a);
    }
    if (0..4).map(|i| m.r8(source + i)).collect::<Vec<_>>() != b"()I\0" || m.r32(0x62e00c) == 0 {
        return Err("NativeVmSignature: unproved signature grammar continuation".into());
    }
    stack::function(m, &[], Some(signature), |m| {
        stack::alloc(m, 0x108);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        stack::push(m, 12);
        s(m, Reg::Ebx, signature);
        let types = stack::external(m, 0x41dfe3, &[], None, |m| a.allocate(m, 12))?;
        s(m, Reg::Eax, types);
        s(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        if types == 0 {
            return Err("NativeVmSignature: type-vector allocation failed".into());
        }
        for offset in [0, 8, 4] {
            write(m, types + offset, 0);
        }
        s(m, Reg::Esi, source + 1);
        write(m, signature + 8, types);
        low(m, Reg::Eax, b')');
        s(m, Reg::Ecx, 0);
        s(m, Reg::Edx, 0);
        low(m, Reg::Eax, b'I');
        s(m, Reg::Esi, source + 2);
        s(m, Reg::Ecx, 0);
        stack::store(m, 0x14, 0);
        low(m, Reg::Eax, b'I');
        s(m, Reg::Ecx, 0x20);
        s(m, Reg::Edx, 2);
        let cell = stack::external(m, 0x41e02e, &[4], None, |m| a.allocate(m, 4))?;
        s(m, Reg::Eax, cell);
        if cell == 0 {
            return Err("NativeVmSignature: integer type allocation failed".into());
        }
        s(m, Reg::Ecx, m.r32(0x62e00c));
        s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
        write(m, cell, r(m, Reg::Edx));
        s(m, Reg::Ebp, cell);
        s(m, Reg::Edi, types);
        s(m, Reg::Eax, m.r32(types + 8) + 1);
        stack::call(m, 0x41e054, &[1], Some(types), |m| {
            crate::native_vm_values::reserve(m, types, 1, a)
        })?;
        s(m, Reg::Ecx, m.r32(types + 8));
        s(m, Reg::Edx, m.r32(types + 4));
        write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, r(m, Reg::Ebp));
        write(m, types + 8, r(m, Reg::Ecx) + 1);
        s(m, Reg::Esi, source + 3);
        low(m, Reg::Eax, 0);
        s(m, Reg::Ebp, 0);
        Ok(())
    })
}
///41ddc0 ordinary matching empty parameter lists. Return descriptors do not
/// participate in this comparison; nonempty conversion-distance arms are open.
pub fn compare_empty(
    m: &mut Space,
    left: u32,
    right: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    for signature in [left, right] {
        let source = m.r32(signature + 4);
        if m.r8(source) != b'(' {
            return Err(
                "NativeVmSignature: parameter conversion comparison continuation unavailable"
                    .into(),
            );
        }
    }
    stack::function(m, &[right], Some(left), |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, right);
        s(m, Reg::Eax, m.r32(right + 8));
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, left);
        if r(m, Reg::Eax) == 0 {
            stack::call(m, 0x41ddd8, &[], Some(right), |m| parse_empty(m, right, a))?;
        }
        s(m, Reg::Eax, m.r32(right + 8));
        s(m, Reg::Esi, m.r32(r(m, Reg::Eax) + 8));
        s(m, Reg::Eax, m.r32(left + 8));
        s(m, Reg::Esi, r(m, Reg::Esi).wrapping_sub(1));
        if r(m, Reg::Eax) == 0 {
            stack::call(m, 0x41dded, &[], Some(left), |m| parse_empty(m, left, a))?;
        }
        s(m, Reg::Ecx, m.r32(left + 8));
        s(m, Reg::Edx, m.r32(r(m, Reg::Ecx) + 8).wrapping_sub(1));
        if r(m, Reg::Esi) != r(m, Reg::Edx) {
            s(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        let count = r(m, Reg::Esi);
        s(m, Reg::Eax, count);
        s(m, Reg::Esi, count.wrapping_sub(1));
        stack::store(m, 0x14, 0);
        for index in (0..count).rev() {
            s(m, Reg::Eax, m.r32(left + 8));
            let types = r(m, Reg::Eax);
            s(m, Reg::Ecx, m.r32(types + 4));
            let destination = m.r32(r(m, Reg::Ecx) + index * 4);
            s(m, Reg::Ebx, destination);
            let types = m.r32(right + 8);
            s(m, Reg::Eax, types);
            s(m, Reg::Edx, m.r32(types + 4));
            let source = m.r32(r(m, Reg::Edx) + index * 4);
            s(m, Reg::Eax, source);
            let jvm = m.r32(left);
            s(m, Reg::Ecx, jvm);
            let cost = stack::call(m, 0x41de4e, &[source, destination], Some(jvm), |m| {
                stack::function(m, &[source, destination], Some(jvm), |m| {
                    //417c30's canonical-identical descriptor arm.
                    stack::push_reg(m, Reg::Ebx);
                    s(m, Reg::Ebx, destination);
                    s(m, Reg::Eax, m.r32(destination));
                    stack::push_reg(m, Reg::Ebp);
                    s(m, Reg::Ebp, source);
                    stack::push_reg(m, Reg::Esi);
                    s(m, Reg::Esi, jvm);
                    s(m, Reg::Ecx, m.r32(source));
                    stack::push_reg(m, Reg::Edi);
                    if m.r32(source) != m.r32(destination) {
                        let target_tag = m.r8(m.r32(destination));
                        if !matches!(target_tag, b'L' | b'[')
                            || !matches!(m.r8(m.r32(source)), b'I' | b'F')
                        {
                            return Err(
                                "NativeVmSignature: conversion distance continuation unavailable"
                                    .into(),
                            );
                        }
                        s(m, Reg::Edx, u32::from(target_tag));
                        s(m, Reg::Edi, u32::from(target_tag) - 0x46);
                        s(m, Reg::Edx, 2);
                        let source_tag = m.r8(m.r32(source));
                        s(m, Reg::Edx, u32::from(source_tag));
                        s(m, Reg::Edi, u32::from(source_tag) - 0x46);
                        s(m, Reg::Edx, 0);
                        let string = stack::call(m, 0x417cf5, &[0x60ba5c], Some(jvm), |m| {
                            crate::native_vm_lookup::module_from_name(m, jvm, 0x60ba5c)
                        })?;
                        let descriptor = m.r32(destination);
                        s(m, Reg::Ecx, descriptor);
                        stack::push(m, string);
                        let target = stack::call(m, 0x417d00, &[descriptor], Some(jvm), |m| {
                            crate::native_vm_lookup::module_from_name(m, jvm, descriptor)
                        })?;
                        s(m, Reg::Ecx, target);
                        let matches = stack::call(m, 0x417d07, &[], Some(target), |m| {
                            stack::function(m, &[string], Some(target), |m| {
                                s(m, Reg::Eax, string);
                                let distance =
                                    stack::call(m, 0x40523a, &[string], Some(target), |m| {
                                        crate::native_vm_lookup::module_distance(m, target, string)
                                    })?;
                                s(m, Reg::Ecx, u32::from((distance as i32) >= 0));
                                s(m, Reg::Eax, r(m, Reg::Ecx));
                                Ok::<u32, String>(r(m, Reg::Eax))
                            })
                        })?;
                        stack::add_sp(m, 4);
                        let result = if matches != 0 { 100 } else { u32::MAX };
                        s(m, Reg::Eax, result);
                        return Ok(result);
                    }
                    s(m, Reg::Eax, 0);
                    Ok::<u32, String>(0)
                })
            })?;
            if (cost as i32) < 0 {
                s(m, Reg::Eax, u32::MAX);
                return Ok(u32::MAX);
            }
            s(m, Reg::Ebx, stack::load(m, 0x14).wrapping_add(cost));
            s(m, Reg::Eax, index);
            s(m, Reg::Esi, index.wrapping_sub(1));
            stack::store(m, 0x14, r(m, Reg::Ebx));
        }
        s(m, Reg::Eax, 0);
        s(m, Reg::Esi, u32::MAX);
        stack::store(m, 0x14, 0);
        s(m, Reg::Eax, stack::load(m, 0x14));
        Ok(0)
    })
}
