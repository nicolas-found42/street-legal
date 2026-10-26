//! Source cached class/constant lookup used by interpreter method selection.
//! Addresses come from actual module pools and the JVM class registry. No
//! semantic-ID pointer arithmetic, captured pointer answers or name allowlist.
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! { static OBSERVER: std::cell::Cell<Option<StoreObserver>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn set_test_store_observer(observer: Option<StoreObserver>) {
    OBSERVER.with(|slot| slot.set(observer));
}
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(address, 4, value);
        }
    });
    m.w32(address, value);
}
fn byte(m: &mut Space, address: u32, value: u8) {
    #[cfg(test)]
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(address, 1, u32::from(value));
        }
    });
    m.w8(address, value);
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
fn text(m: &Space, source: u32) -> Result<Vec<u8>, String> {
    if source == 0 {
        return Err("NativeVmLookup: missing source string".into());
    }
    let mut out = Vec::new();
    for i in 0..0x100000 {
        let b = m.r8(source + i);
        if b == 0 {
            return Ok(out);
        }
        out.push(b);
    }
    Err("NativeVmLookup: unterminated source string".into())
}
///406140 /406f60 /406bb0 /406c20 ordinary in-range, correctly typed branches.
/// `field` is the source record offset (8 or16), not a semantic cache lookup.
pub fn constant(
    m: &mut Space,
    pool: u32,
    index: u32,
    kind: u32,
    field: u32,
) -> Result<u32, String> {
    if pool == 0 || (index as i32) < 0 || index >= m.r32(pool + 8) {
        return Err("NativeVmLookup: constant source index unavailable".into());
    }
    let entry = m.r32(m.r32(pool + 12) + index * 4);
    if entry == 0 || m.r32(entry + 4) != kind || !matches!((kind, field), (0 | 4 | 5, 8) | (5, 16))
    {
        return Err("NativeVmLookup: source constant kind unavailable".into());
    }
    Ok(stack::function(m, &[index], Some(pool), |m| {
        s(m, Reg::Eax, index);
        s(m, Reg::Ecx, m.r32(pool + 12));
        s(m, Reg::Eax, entry);
        if kind == 0 {
            s(m, Reg::Ecx, 0);
        }
        let value = m.r32(entry + field);
        s(m, Reg::Eax, value);
        value
    }))
}
fn strip_descriptor(
    m: &mut Space,
    source: u32,
    buffer: u32,
    return_pc: u32,
) -> Result<u32, String> {
    let bytes = text(m, source)?;
    if bytes.first() != Some(&b'L') || bytes.last() != Some(&b';') || bytes.len() > 512 {
        return Err("NativeVmLookup: descriptor source outside ordinary class path".into());
    }
    s(m, Reg::Ecx, u32::MAX);
    s(m, Reg::Eax, 0);
    s(m, Reg::Edi, source + bytes.len() as u32 + 1);
    let maximum = bytes.len() as u32 - 1;
    s(m, Reg::Ecx, maximum);
    stack::push(m, maximum);
    if return_pc == 0x4108d8 {
        s(m, Reg::Ebx, source + 1);
    } else {
        s(m, Reg::Ebp, source + 1);
    }
    s(m, Reg::Eax, buffer);
    stack::push(m, source + 1);
    stack::push(m, buffer);
    stack::call(m, return_pc, &[], None, |m| {
        crate::native_vm_metadata::name_copy(m, buffer, source + 1, maximum)
    });
    stack::add_sp(m, 12);
    Ok(buffer)
}
///411610 cached class lookup, including real signed-byte hash and two-byte
/// strcmp register effects. Missing classes return zero for410890's loader.
pub fn find_class(m: &mut Space, jvm: u32, name: u32) -> Result<u32, String> {
    text(m, name)?;
    let cache = m.r32(jvm.wrapping_add(8));
    if jvm == 0 || cache == 0 {
        return Err("NativeVmLookup: class cache unavailable".into());
    }
    stack::function(m, &[name], Some(jvm), |m| {
        stack::alloc(m, 0x200);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, name);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Ebx, jvm);
        let name = if m.r8(name) == b'L' {
            let buffer = stack::sp(m) + 0x10;
            let p = strip_descriptor(m, name, buffer, 0x411644)?;
            s(m, Reg::Ebp, p);
            p
        } else {
            name
        };
        low(m, Reg::Ecx, m.r8(name));
        s(m, Reg::Edx, 0);
        s(m, Reg::Eax, 0);
        let bytes = text(m, name)?;
        for (i, b) in bytes.iter().enumerate() {
            s(m, Reg::Ecx, i32::from(*b as i8) as u32);
            s(m, Reg::Edx, r(m, Reg::Edx).wrapping_add(r(m, Reg::Ecx)));
            low(m, Reg::Ecx, m.r8(name + i as u32 + 1));
            s(m, Reg::Eax, i as u32 + 1);
        }
        s(m, Reg::Ecx, r(m, Reg::Eax).wrapping_mul(7));
        let sum = r(m, Reg::Edx);
        s(m, Reg::Eax, sum.wrapping_mul(9));
        s(
            m,
            Reg::Edx,
            sum.wrapping_add(r(m, Reg::Eax).wrapping_mul(4)),
        );
        s(m, Reg::Ecx, r(m, Reg::Ecx).wrapping_add(r(m, Reg::Edx)));
        let hash = r(m, Reg::Ecx);
        s(m, Reg::Edx, cache);
        s(m, Reg::Eax, (hash & 511) * 3);
        let bucket = cache + (hash & 511) * 12;
        s(m, Reg::Esi, m.r32(bucket + 4));
        s(m, Reg::Eax, bucket);
        let mut node = bucket;
        if r(m, Reg::Esi) == 0 {
            s(m, Reg::Edi, 0);
            s(m, Reg::Eax, 0);
            return Ok(0);
        }
        let mut seen = std::collections::HashSet::new();
        loop {
            if node == 0 {
                s(m, Reg::Edi, 0);
                s(m, Reg::Eax, 0);
                return Ok(0);
            }
            if !seen.insert(node) {
                return Err("NativeVmLookup: cyclic class-cache chain".into());
            }
            if m.r32(node) == hash {
                break;
            }
            node = m.r32(node + 8);
            s(m, Reg::Eax, node);
        }
        write(m, cache + 0x1828, node);
        let slot = m.r32(node + 4);
        s(m, Reg::Eax, slot);
        if slot == 0 {
            s(m, Reg::Edi, 0);
            s(m, Reg::Eax, 0);
            return Ok(0);
        }
        let class = m.r32(slot);
        s(m, Reg::Edi, class);
        if class == 0 {
            s(m, Reg::Eax, 0);
            return Ok(0);
        }
        let module = m.r32(class + 4);
        let pool = m.r32(module + 0x24);
        let index = m.r32(module + 0x3c);
        s(m, Reg::Esi, module);
        s(m, Reg::Ecx, index);
        let name_index = stack::call(m, 0x4116cf, &[index], Some(pool), |m| {
            constant(m, pool, index, 4, 8)
        })?;
        s(m, Reg::Ecx, pool);
        let found = stack::call(m, 0x4116d8, &[name_index], Some(pool), |m| {
            constant(m, pool, name_index, 0, 8)
        })?;
        s(m, Reg::Esi, name);
        let mut offset = 0;
        loop {
            let a = m.r8(found + offset);
            let b = m.r8(name + offset);
            low(m, Reg::Edx, a);
            low(m, Reg::Ecx, a);
            if a != b {
                return Err(
                    "NativeVmLookup: same-hash class-name collision continuation unavailable"
                        .into(),
                );
            }
            if a == 0 {
                break;
            }
            let a = m.r8(found + offset + 1);
            let b = m.r8(name + offset + 1);
            low(m, Reg::Edx, a);
            low(m, Reg::Ecx, a);
            if a != b {
                return Err(
                    "NativeVmLookup: same-hash class-name collision continuation unavailable"
                        .into(),
                );
            }
            offset += 2;
            s(m, Reg::Eax, found + offset);
            s(m, Reg::Esi, name + offset);
            if a == 0 {
                break;
            }
        }
        s(m, Reg::Eax, 0);
        s(m, Reg::Eax, class);
        Ok(class)
    })
}
///410890 ordinary non-array cached class branch. Loading an absent class and
/// array metadata construction are explicit source dependencies.
pub fn get_class(m: &mut Space, jvm: u32, name: u32) -> Result<u32, String> {
    let bytes = text(m, name)?;
    let entry_return = stack::load(m, 0);
    if bytes.first() == Some(&b'[') {
        return Err("NativeVmLookup: array class construction continuation unavailable".into());
    }
    stack::function(m, &[name], Some(jvm), |m| {
        stack::alloc(m, 0x220);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, name);
        low(m, Reg::Eax, m.r8(name));
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, jvm);
        let name = if bytes.first() == Some(&b'L') {
            stack::push_reg(m, Reg::Edi);
            let buffer = stack::sp(m) + 0x2c;
            let p = strip_descriptor(m, name, buffer, 0x4108d8)?;
            s(m, Reg::Ebx, p);
            s(m, Reg::Edi, stack::load(m, 0));
            stack::add_sp(m, 4);
            p
        } else {
            name
        };
        let class = stack::call(m, 0x4108e8, &[name], Some(jvm), |m| {
            find_class(m, jvm, name)
        })?;
        if class == 0 {
            return Err(format!(
                "NativeVmLookup: class-load continuation required for {} from {entry_return:#x}",
                String::from_utf8_lossy(&text(m, name)?)
            ));
        }
        Ok(class)
    })
}
pub use crate::native_vm_context::Environment;
///4117a0's class constant→string→cached Class path.
pub fn class_from_pool_with(
    m: &mut Space,
    jvm: u32,
    pool: u32,
    index: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    class_from_pool_resolved(m, jvm, pool, index, |m, jvm, name| {
        e.class_lookup(m, jvm, name)
    })
}
fn class_from_pool_resolved(
    m: &mut Space,
    jvm: u32,
    pool: u32,
    index: u32,
    mut lookup: impl FnMut(&mut Space, u32, u32) -> Result<u32, String>,
) -> Result<u32, String> {
    stack::function(m, &[pool, index], Some(jvm), |m| {
        s(m, Reg::Eax, index);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, pool);
        s(m, Reg::Esi, jvm);
        let index = stack::call(m, 0x4117b4, &[index], Some(pool), |m| {
            constant(m, pool, index, 4, 8)
        })?;
        let name = stack::call(m, 0x4117bc, &[index], Some(pool), |m| {
            constant(m, pool, index, 0, 8)
        })?;
        stack::call(m, 0x4117c4, &[name], Some(jvm), |m| lookup(m, jvm, name))
    })
}

///407aa0's capacity-indexed raw vector access.
pub fn vector_item(m: &mut Space, vector: u32, index: u32) -> Result<u32, String> {
    if vector == 0 || (index as i32) < 0 || index >= m.r32(vector) {
        return Err("NativeVmLookup: source vector item unavailable".into());
    }
    Ok(stack::function(m, &[index], Some(vector), |m| {
        s(m, Reg::Eax, index);
        s(m, Reg::Ecx, m.r32(vector + 4));
        s(m, Reg::Eax, m.r32(r(m, Reg::Ecx) + index * 4));
        r(m, Reg::Eax)
    }))
}
///406170's reverse constant-string search, including unsuccessful comparisons.
pub fn find_string(m: &mut Space, pool: u32, source: u32) -> Result<u32, String> {
    let name = text(m, source)?;
    if pool == 0 || m.r32(pool + 16) > m.r32(pool + 8) {
        return Err("NativeVmLookup: source constant pool unavailable".into());
    }
    stack::function(m, &[source], Some(pool), |m| {
        for reg in [Reg::Ebx, Reg::Ebp] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Ebp, pool);
        for reg in [Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Edi, source);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, m.r32(pool + 16));
        s(m, Reg::Ecx, name.len() as u32);
        s(m, Reg::Edi, r(m, Reg::Edi).wrapping_sub(1));
        loop {
            let index = r(m, Reg::Edi);
            if (index as i32) < 0 {
                break;
            }
            s(m, Reg::Eax, m.r32(pool + 12));
            let entry = m.r32(r(m, Reg::Eax) + index * 4);
            s(m, Reg::Edx, entry);
            s(m, Reg::Eax, m.r32(entry + 4));
            if r(m, Reg::Eax) == 0 && m.r32(entry + 12) == r(m, Reg::Ecx) {
                s(m, Reg::Esi, source);
                let found = m.r32(entry + 8);
                s(m, Reg::Edx, found);
                let mut offset = 0;
                loop {
                    let left = m.r8(found + offset);
                    let right = m.r8(source + offset);
                    low(m, Reg::Eax, left);
                    low(m, Reg::Ebx, left);
                    if left != right {
                        s(m, Reg::Edx, if left < right { u32::MAX } else { 1 });
                        break;
                    }
                    if left == 0 {
                        s(m, Reg::Edx, 0);
                        break;
                    }
                    let left = m.r8(found + offset + 1);
                    let right = m.r8(source + offset + 1);
                    low(m, Reg::Eax, left);
                    low(m, Reg::Ebx, left);
                    if left != right {
                        s(m, Reg::Edx, if left < right { u32::MAX } else { 1 });
                        break;
                    }
                    offset += 2;
                    s(m, Reg::Edx, found + offset);
                    s(m, Reg::Esi, source + offset);
                    if left == 0 {
                        s(m, Reg::Edx, 0);
                        break;
                    }
                }
                if r(m, Reg::Edx) == 0 {
                    break;
                }
            }
            s(m, Reg::Edi, index.wrapping_sub(1));
        }
        s(m, Reg::Eax, r(m, Reg::Edi));
        Ok(r(m, Reg::Eax))
    })
}
///412670 ordinary cached-class→module branch with an empty explicit module list.
pub fn module_from_name(m: &mut Space, jvm: u32, name: u32) -> Result<u32, String> {
    module_from_name_resolved(m, jvm, name, get_class)
}
pub fn module_from_name_with(
    m: &mut Space,
    jvm: u32,
    name: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    module_from_name_resolved(m, jvm, name, |m, jvm, name| e.class_lookup(m, jvm, name))
}
fn module_from_name_resolved(
    m: &mut Space,
    jvm: u32,
    name: u32,
    mut lookup: impl FnMut(&mut Space, u32, u32) -> Result<u32, String>,
) -> Result<u32, String> {
    let name_bytes = text(m, name)?;
    let list = m.r32(jvm.wrapping_add(0x54));
    if list == 0 || m.r32(list + 4) != 0 {
        return Err(
            "NativeVmLookup: explicit module-list/descriptor traversal continuation unavailable"
                .into(),
        );
    }
    stack::function(m, &[name], Some(jvm), |m| {
        stack::alloc(m, 0x224);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, name);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, jvm);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, name + name_bytes.len() as u32 + 1);
        s(m, Reg::Ecx, name_bytes.len() as u32 + 1);
        low(m, Reg::Eax, *name_bytes.last().unwrap_or(&0));
        s(m, Reg::Ecx, name_bytes.len() as u32);
        stack::store(m, 0x10, jvm);
        let name = if name_bytes.first() == Some(&b'L') && name_bytes.last() == Some(&b';') {
            let buffer = stack::sp(m) + 0x34;
            strip_descriptor(m, name, buffer, 0x4126bc)?;
            s(m, Reg::Ebp, buffer);
            buffer
        } else {
            name
        };
        s(m, Reg::Eax, list);
        s(m, Reg::Ecx, 0);
        let class = stack::call(m, 0x41276d, &[name], Some(jvm), |m| lookup(m, jvm, name))?;
        s(m, Reg::Eax, if class == 0 { 0 } else { m.r32(class + 4) });
        Ok(r(m, Reg::Eax))
    })
}
///412870's pool-qualified module lookup. Its module list is a distinct source
/// owner from the hash cache; this seam never treats equal names as ownership.
pub fn module_from_pool_with(
    m: &mut Space,
    jvm: u32,
    pool: u32,
    index: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    module_from_pool_resolved(m, jvm, pool, index, |m, jvm, name| {
        e.class_lookup(m, jvm, name)
    })
}
fn module_from_pool_resolved(
    m: &mut Space,
    jvm: u32,
    pool: u32,
    index: u32,
    mut lookup: impl FnMut(&mut Space, u32, u32) -> Result<u32, String>,
) -> Result<u32, String> {
    let list = m.r32(jvm.wrapping_add(0x54));
    if list == 0 || m.r32(list + 4) != 0 {
        return Err(
            "NativeVmLookup: explicit module-list traversal continuation unavailable".into(),
        );
    }
    stack::function(m, &[pool, index], Some(jvm), |m| {
        stack::alloc(m, 0x24);
        s(m, Reg::Eax, index);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Edi, pool);
        s(m, Reg::Esi, jvm);
        stack::store(m, 0x10, jvm);
        let name_index = stack::call(m, 0x41288d, &[index], Some(pool), |m| {
            constant(m, pool, index, 4, 8)
        })?;
        let name = stack::call(m, 0x412895, &[name_index], Some(pool), |m| {
            constant(m, pool, name_index, 0, 8)
        })?;
        s(m, Reg::Ebx, name);
        s(m, Reg::Eax, list);
        s(m, Reg::Ecx, 0);
        s(m, Reg::Eax, index);
        s(m, Reg::Edi, pool);
        let name_index = stack::call(m, 0x412949, &[index], Some(pool), |m| {
            constant(m, pool, index, 4, 8)
        })?;
        let name = stack::call(m, 0x412951, &[name_index], Some(pool), |m| {
            constant(m, pool, name_index, 0, 8)
        })?;
        let class = stack::call(m, 0x412959, &[name], Some(jvm), |m| lookup(m, jvm, name))?;
        s(m, Reg::Eax, if class == 0 { 0 } else { m.r32(class + 4) });
        Ok(r(m, Reg::Eax))
    })
}

fn heap_free(
    m: &mut Space,
    pc: u32,
    address: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    stack::external(m, pc, &[address], None, |m| e.free(m, address))
}
fn clear_types(
    m: &mut Space,
    vector: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    stack::function(m, &[], Some(vector), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, vector);
        s(m, Reg::Eax, m.r32(vector + 8));
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, 0);
        while r(m, Reg::Edi) < m.r32(vector + 8) {
            let i = r(m, Reg::Edi);
            s(m, Reg::Eax, m.r32(vector + 4));
            s(m, Reg::Ecx, m.r32(r(m, Reg::Eax) + i * 4));
            heap_free(m, 0x405cf9, r(m, Reg::Ecx), e)?;
            s(m, Reg::Edx, m.r32(vector + 4));
            write(m, r(m, Reg::Edx) + i * 4, 0);
            s(m, Reg::Eax, m.r32(vector + 8));
            s(m, Reg::Edi, i + 1);
        }
        write(m, vector + 8, 0);
        Ok(())
    })
}
pub(crate) fn resize_zero(
    m: &mut Space,
    vector: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    stack::function(m, &[0], Some(vector), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, 0);
        s(m, Reg::Esi, vector);
        s(m, Reg::Eax, m.r32(vector + 4));
        heap_free(m, 0x405195, r(m, Reg::Eax), e)?;
        write(m, vector + 4, 0);
        if m.r32(vector + 8) > 0 {
            write(m, vector + 8, 0);
        }
        write(m, vector, 0);
        Ok(())
    })
}
fn parent_module_with(
    m: &mut Space,
    module: u32,
    class_pc: u32,
    string_pc: u32,
    module_pc: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    let index = m.r32(module + 0x40);
    let pool = m.r32(module + 0x24);
    let jvm = m.r32(module + 0xc);
    s(m, Reg::Eax, index);
    if (index as i32) < 0 {
        return Ok(0);
    }
    let index = stack::call(m, class_pc, &[index], Some(pool), |m| {
        constant(m, pool, index, 4, 8)
    })?;
    let name = stack::call(m, string_pc, &[index], Some(pool), |m| {
        constant(m, pool, index, 0, 8)
    })?;
    stack::call(m, module_pc, &[name], Some(jvm), |m| {
        module_from_name_with(m, jvm, name, e)
    })
}
///405920's exact empty-parameter method match and inherited ordinal. Method
/// vectors remain the actual static/instance vectors of their defining module.
/// Nonzero overload distances and ambiguous signatures remain explicit paths.
pub fn method_index(
    m: &mut Space,
    module: u32,
    name: u32,
    descriptor: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<u32, String> {
    let input = text(m, descriptor)?;
    if !matches!(
        input.as_slice(),
        b"()I" | b"()V" | b"()" | b"(I)" | b"(I)V" | b"(II)" | b"(F)I"
    ) && !(input.starts_with(b"()L") && input.ends_with(b";"))
    {
        return Err(format!(
            "NativeVmLookup: method signature distance continuation unavailable for {:?}",
            String::from_utf8_lossy(&input)
        ));
    }
    stack::function(m, &[name, descriptor], Some(module), |m| {
        stack::alloc(m, 0x5c);
        s(m, Reg::Eax, descriptor);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Ebp, module);
        s(m, Reg::Ecx, m.r32(module + 0xc));
        stack::push_reg(m, Reg::Edi);
        let b = stack::sp(m);
        let input_signature = b + 0x40;
        let candidate_signature = b + 0x34;
        let jvm = m.r32(module + 0xc);
        s(m, Reg::Ecx, input_signature);
        write(m, b + 0x20, module);
        stack::call(
            m,
            0x40593f,
            &[jvm, descriptor],
            Some(input_signature),
            |m| crate::native_vm_signature::construct(m, input_signature, jvm, descriptor, e),
        )?;
        s(m, Reg::Ebx, 0);
        s(m, Reg::Edi, u32::MAX);
        s(m, Reg::Esi, module);
        write(m, b + 0x18, 0);
        write(m, b + 0x10, 1_000_000);
        write(m, b + 0x24, u32::MAX);
        let mut current = module;
        while current != 0 && r(m, Reg::Edi) != 0 {
            s(m, Reg::Edx, name);
            let pool = m.r32(current + 0x24);
            let name_index = stack::call(m, 0x405973, &[name], Some(pool), |m| {
                find_string(m, pool, name)
            })?;
            write(m, b + 0x2c, name_index);
            if (name_index as i32) >= 0 {
                s(m, Reg::Eax, m.r32(current + 0x58));
                write(m, b + 0x14, r(m, Reg::Eax));
                write(m, b + 0x74, 1);
                loop {
                    let vector = m.r32(b + 0x14);
                    s(m, Reg::Eax, vector);
                    s(m, Reg::Ebp, m.r32(vector + 0xc));
                    s(m, Reg::Ecx, r(m, Reg::Ebp));
                    s(m, Reg::Ebp, r(m, Reg::Ebp).wrapping_sub(1));
                    while (r(m, Reg::Ebp) as i32) >= 0 {
                        let index = r(m, Reg::Ebp);
                        s(m, Reg::Eax, vector);
                        if index >= m.r32(vector + 4) {
                            return Err("NativeVmLookup: malformed method vector".into());
                        }
                        s(m, Reg::Edx, m.r32(vector + 8));
                        let method = m.r32(r(m, Reg::Edx) + index * 4);
                        s(m, Reg::Eax, method);
                        s(m, Reg::Ecx, name_index);
                        if m.r32(method + 8) == name_index {
                            let signature_index = m.r32(method + 0xc);
                            s(m, Reg::Edx, signature_index);
                            let signature =
                                stack::call(m, 0x4059ce, &[signature_index], Some(pool), |m| {
                                    constant(m, pool, signature_index, 0, 8)
                                })?;
                            s(m, Reg::Eax, module);
                            s(m, Reg::Ecx, jvm);
                            stack::call(
                                m,
                                0x4059e0,
                                &[jvm, signature],
                                Some(candidate_signature),
                                |m| {
                                    crate::native_vm_signature::construct(
                                        m,
                                        candidate_signature,
                                        jvm,
                                        signature,
                                        e,
                                    )
                                },
                            )?;
                            s(m, Reg::Edx, input_signature);
                            let cost = stack::call(
                                m,
                                0x4059ee,
                                &[input_signature],
                                Some(candidate_signature),
                                |m| {
                                    crate::native_vm_signature::compare_empty(
                                        m,
                                        candidate_signature,
                                        input_signature,
                                        e,
                                    )
                                },
                            )?;
                            s(m, Reg::Edi, cost);
                            if (cost as i32) < 0 {
                                let types = m.r32(candidate_signature + 8);
                                s(m, Reg::Ecx, types);
                                if types != 0 {
                                    stack::call(m, 0x405a2b, &[], Some(types), |m| {
                                        clear_types(m, types, e)
                                    })?;
                                    s(m, Reg::Ebx, m.r32(candidate_signature + 8));
                                    let vector = r(m, Reg::Ebx);
                                    stack::call(m, 0x405a3c, &[0], Some(vector), |m| {
                                        resize_zero(m, vector, e)
                                    })?;
                                    heap_free(m, 0x405a42, vector, e)?;
                                }
                                s(m, Reg::Ecx, index);
                                s(m, Reg::Ebp, index.wrapping_sub(1));
                                continue;
                            }
                            if cost != 0 {
                                return Err("NativeVmLookup: nonzero overload distance continuation unavailable".into());
                            }
                            let types = m.r32(candidate_signature + 8);
                            s(m, Reg::Ecx, types);
                            s(m, Reg::Edx, m.r32(b + 0x74));
                            write(m, b + 0x10, 0);
                            write(m, b + 0x18, current);
                            write(m, b + 0x28, index);
                            write(m, b + 0x1c, r(m, Reg::Edx));
                            if types != 0 {
                                stack::call(m, 0x405a77, &[], Some(types), |m| {
                                    clear_types(m, types, e)
                                })?;
                                s(m, Reg::Ebx, m.r32(candidate_signature + 8));
                                let vector = r(m, Reg::Ebx);
                                stack::call(m, 0x405a88, &[0], Some(vector), |m| {
                                    resize_zero(m, vector, e)
                                })?;
                                heap_free(m, 0x405a8e, vector, e)?;
                            }
                            break;
                        }
                        s(m, Reg::Ecx, index);
                        s(m, Reg::Ebp, index.wrapping_sub(1));
                    }
                    s(m, Reg::Eax, m.r32(current + 0x50));
                    write(m, b + 0x14, r(m, Reg::Eax));
                    let mode = m.r32(b + 0x74).wrapping_sub(1);
                    write(m, b + 0x74, mode);
                    if (mode as i32) < 0 {
                        break;
                    }
                }
                s(m, Reg::Ebx, m.r32(b + 0x18));
                s(m, Reg::Ebp, m.r32(b + 0x20));
            }
            let parent = parent_module_with(m, current, 0x405aba, 0x405ac3, 0x405acc, e)?;
            if parent == 0 {
                break;
            }
            current = parent;
            s(m, Reg::Esi, parent);
        }
        let selected = r(m, Reg::Ebx);
        if selected == 0 {
            return Err(
                "NativeVmLookup: original method-not-found cleanup continuation unavailable".into(),
            );
        }
        s(m, Reg::Ecx, m.r32(b + 0x10));
        if m.r32(b + 0x24) == r(m, Reg::Ecx) {
            return Err(
                "NativeVmLookup: ambiguous method diagnostic continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, m.r32(selected + 0x40));
        s(m, Reg::Ebp, m.r32(b + 0x28));
        let parent = parent_module_with(m, selected, 0x405bb4, 0x405bbd, 0x405bc6, e)?;
        if parent != 0 {
            s(m, Reg::Edi, 0);
            s(m, Reg::Esi, parent);
            let mut current = parent;
            loop {
                let mode = m.r32(b + 0x1c);
                s(m, Reg::Eax, mode);
                let vector = m.r32(current + if mode == 0 { 0x50 } else { 0x58 });
                s(m, Reg::Eax, vector);
                s(m, Reg::Edx, m.r32(vector + 0xc));
                s(m, Reg::Eax, m.r32(current + 0x40));
                s(m, Reg::Edi, r(m, Reg::Edi).wrapping_add(r(m, Reg::Edx)));
                if (r(m, Reg::Eax) as i32) < 0 {
                    break;
                }
                current = parent_module_with(m, current, 0x405bf3, 0x405bfc, 0x405c05, e)?;
                s(m, Reg::Esi, current);
                if current == 0 {
                    break;
                }
            }
            s(m, Reg::Ebp, r(m, Reg::Ebp).wrapping_add(r(m, Reg::Edi)));
        }
        s(m, Reg::Eax, m.r32(b + 0x1c));
        if r(m, Reg::Eax) != 0 {
            s(m, Reg::Ebp, r(m, Reg::Ebp) | 0x40000000);
        }
        let types = m.r32(input_signature + 8);
        s(m, Reg::Eax, types);
        if types != 0 {
            s(m, Reg::Ecx, m.r32(types + 8));
            s(m, Reg::Edi, types + 8);
            s(m, Reg::Esi, 0);
            if (r(m, Reg::Ecx) as i32) > 0 {
                s(m, Reg::Ebx, types + 4);
                loop {
                    let index = r(m, Reg::Esi);
                    s(m, Reg::Edx, m.r32(r(m, Reg::Ebx)));
                    s(m, Reg::Eax, m.r32(r(m, Reg::Edx) + index * 4));
                    heap_free(m, 0x405c3d, r(m, Reg::Eax), e)?;
                    s(m, Reg::Ecx, m.r32(r(m, Reg::Ebx)));
                    write(m, r(m, Reg::Ecx) + index * 4, 0);
                    s(m, Reg::Eax, m.r32(r(m, Reg::Edi)));
                    s(m, Reg::Esi, index + 1);
                    if r(m, Reg::Esi) >= r(m, Reg::Eax) {
                        break;
                    }
                }
            }
            write(m, r(m, Reg::Edi), 0);
            s(m, Reg::Esi, m.r32(input_signature + 8));
            let types = r(m, Reg::Esi);
            stack::call(m, 0x405c67, &[0], Some(types), |m| resize_zero(m, types, e))?;
            heap_free(m, 0x405c6d, types, e)?;
        }
        s(m, Reg::Eax, r(m, Reg::Ebp));
        Ok(r(m, Reg::Eax))
    })
}

///406d00 method-reference cache fill through real name/type constants.
pub fn resolve_method_reference(
    m: &mut Space,
    pool: u32,
    jvm: u32,
    index: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    if pool == 0 || (index as i32) < 0 || index >= m.r32(pool + 8) {
        return Err("NativeVmLookup: method-reference index unavailable".into());
    }
    let field = m.r32(m.r32(pool + 12) + index * 4);
    if field == 0 || m.r32(field + 4) != 5 {
        return Err("NativeVmLookup: method-reference constant unavailable".into());
    }
    stack::function(m, &[jvm, index], Some(pool), |m| {
        s(m, Reg::Eax, index);
        for reg in [Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Esi, pool);
        s(m, Reg::Ecx, m.r32(pool + 12));
        s(m, Reg::Eax, field);
        s(m, Reg::Ebp, field);
        let nat_index = m.r32(field + 0xc);
        s(m, Reg::Eax, nat_index);
        let nat = stack::call(m, 0x406d3e, &[nat_index], Some(pool + 8), |m| {
            vector_item(m, pool + 8, nat_index)
        })?;
        if m.r32(nat + 4) != 7 {
            return Err("NativeVmLookup: name/type constant unavailable".into());
        }
        let name_index = m.r32(nat + 8);
        s(m, Reg::Eax, name_index);
        let name_record = stack::call(m, 0x406d58, &[name_index], Some(pool + 8), |m| {
            vector_item(m, pool + 8, name_index)
        })?;
        s(m, Reg::Ecx, m.r32(name_record + 4));
        if r(m, Reg::Ecx) != 0 {
            return Err("NativeVmLookup: method name is not a raw string".into());
        }
        let name = m.r32(name_record + 8);
        s(m, Reg::Edi, name);
        s(m, Reg::Eax, nat_index);
        let nat = stack::call(m, 0x406db9, &[nat_index], Some(pool + 8), |m| {
            vector_item(m, pool + 8, nat_index)
        })?;
        let type_index = m.r32(nat + 0xc);
        s(m, Reg::Eax, type_index);
        let type_record = stack::call(m, 0x406dd3, &[type_index], Some(pool + 8), |m| {
            vector_item(m, pool + 8, type_index)
        })?;
        s(m, Reg::Ecx, m.r32(type_record + 4));
        if r(m, Reg::Ecx) != 0 {
            return Err("NativeVmLookup: method descriptor is not a raw string".into());
        }
        let descriptor = m.r32(type_record + 8);
        s(m, Reg::Eax, descriptor);
        if m.r8(descriptor) != b'(' {
            let class_index = m.r32(field + 8);
            s(m, Reg::Eax, class_index);
            let result = stack::call(m, 0x406e51, &[pool, class_index, name], Some(jvm), |m| {
                stack::function(m, &[pool, class_index, name], Some(jvm), |m| {
                    s(m, Reg::Eax, class_index);
                    s(m, Reg::Edx, pool);
                    let module = stack::call(m, 0x412a7f, &[pool, class_index], Some(jvm), |m| {
                        module_from_pool_with(m, jvm, pool, class_index, e)
                    })?;
                    s(m, Reg::Ecx, name);
                    stack::call(m, 0x412a8b, &[name], Some(module), |m| {
                        field_index(m, module, name, e)
                    })
                })
            })?;
            write(m, field + 0x10, result);
            return Ok(());
        }
        let class_index = m.r32(field + 8);
        s(m, Reg::Edx, class_index);
        s(m, Reg::Ecx, jvm);
        let result = stack::call(
            m,
            0x406e39,
            &[pool, class_index, name, descriptor],
            Some(jvm),
            |m| {
                stack::function(m, &[pool, class_index, name, descriptor], Some(jvm), |m| {
                    s(m, Reg::Eax, class_index);
                    s(m, Reg::Edx, pool);
                    let module = stack::call(m, 0x412abf, &[pool, class_index], Some(jvm), |m| {
                        module_from_pool_with(m, jvm, pool, class_index, e)
                    })?;
                    s(m, Reg::Ecx, descriptor);
                    s(m, Reg::Edx, name);
                    stack::call(m, 0x412ad0, &[name, descriptor], Some(module), |m| {
                        method_index(m, module, name, descriptor, e)
                    })
                })
            },
        )?;
        write(m, field + 0x10, result);
        Ok(())
    })
}
pub(crate) fn initialized_class(m: &mut Space, class: u32) -> Result<(), String> {
    if class == 0 || m.r32(class + 0x1d0) == 0 {
        return Err("NativeVmLookup: class initialization continuation unavailable".into());
    }
    stack::function(m, &[], Some(class), |m| {
        stack::alloc(m, 0x4c);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, class);
        s(m, Reg::Eax, m.r32(class + 0x1d0));
        s(m, Reg::Ebp, 0);
        Ok(())
    })
}
///404d40 maps the inherited method ordinal to its real defining class and
/// method pointer, then creates the source16-byte temporary call record.
pub fn method_record(
    m: &mut Space,
    class: u32,
    index: u32,
    static_vector: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<u32, String> {
    stack::function(m, &[index, static_vector], Some(class), |m| {
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Esi, class);
        stack::call(m, 0x404d4b, &[], Some(class), |m| {
            initialized_class(m, class)
        })?;
        s(m, Reg::Edi, class);
        s(m, Reg::Ebx, static_vector);
        s(m, Reg::Ebp, index);
        let mut current = class;
        loop {
            let parent = m.r32(current + 0x1c8);
            s(m, Reg::Esi, parent);
            let mut count = 0;
            if parent != 0 {
                stack::call(m, 0x404d6a, &[], Some(parent), |m| {
                    initialized_class(m, parent)
                })?;
                s(m, Reg::Edx, 0);
                s(m, Reg::Eax, parent);
                let mut ancestor = parent;
                while ancestor != 0 {
                    let module = m.r32(ancestor + 4);
                    s(m, Reg::Ecx, module);
                    let vector = m.r32(module + if static_vector == 0 { 0x50 } else { 0x58 });
                    s(m, Reg::Ecx, vector);
                    if vector != 0 {
                        count += m.r32(vector + 0xc);
                        s(m, Reg::Edx, count);
                    }
                    ancestor = m.r32(ancestor + 0x1c8);
                    s(m, Reg::Eax, ancestor);
                }
            }
            s(m, Reg::Eax, count);
            if count <= r(m, Reg::Ebp) {
                break;
            }
            current = parent;
            s(m, Reg::Edi, current);
        }
        s(m, Reg::Ebp, r(m, Reg::Ebp) - r(m, Reg::Eax));
        let local = r(m, Reg::Ebp);
        let module = m.r32(current + 4);
        s(
            m,
            if static_vector == 0 {
                Reg::Eax
            } else {
                Reg::Edx
            },
            module,
        );
        let vector = m.r32(module + if static_vector == 0 { 0x50 } else { 0x58 });
        s(m, Reg::Eax, vector);
        if vector == 0 || local >= m.r32(vector + 4) {
            return Err("NativeVmLookup: method ordinal out of range".into());
        }
        s(m, Reg::Ecx, m.r32(vector + 8));
        let method = m.r32(r(m, Reg::Ecx) + local * 4);
        s(m, Reg::Esi, method);
        let record = stack::external(m, 0x404ddd, &[16], None, |m| e.allocate(m, 16))?;
        s(m, Reg::Eax, record);
        if record == 0 {
            return Err("NativeVmLookup: method record allocation failed".into());
        }
        write(m, record, current);
        write(m, record + 8, method);
        write(m, record + 4, 0);
        write(m, record + 12, 1);
        Ok(record)
    })
}

///403820 variable-owner slot lookup. Lvalue identity remains the actual vector
/// entry; it is never found by comparing payloads.
pub fn variable_at(m: &mut Space, owner: u32, index: u32) -> Result<u32, String> {
    let vector = m.r32(owner.wrapping_add(0xc));
    if owner == 0 || vector == 0 || (index as i32) < 0 || index >= m.r32(vector) {
        return Err(format!("NativeVmLookup: variable-owner slot unavailable: owner={owner:#x}, vector={vector:#x}, index={index}, capacity={}", m.r32(vector)));
    }
    Ok(stack::function(m, &[index], Some(owner), |m| {
        s(m, Reg::Edx, index);
        s(m, Reg::Eax, vector);
        s(m, Reg::Eax, m.r32(vector + 4));
        s(m, Reg::Eax, m.r32(r(m, Reg::Eax) + index * 4));
        r(m, Reg::Eax)
    }))
}
///404800 static variable owner, after its source class-initialization check.
pub fn static_field(m: &mut Space, class: u32, index: u32) -> Result<u32, String> {
    stack::function(m, &[index], Some(class), |m| {
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, class);
        stack::call(m, 0x404808, &[], Some(class), |m| {
            initialized_class(m, class)
        })?;
        s(m, Reg::Eax, index);
        let variables = m.r32(class + 0x1d0);
        s(m, Reg::Ecx, variables);
        stack::call(m, 0x404818, &[index], Some(variables), |m| {
            variable_at(m, variables, index)
        })
    })
}

///408830's existing, populated array path. Reads the original length/storage
/// fields and returns their lvalue cell; lazy allocation remains a separate path.
pub fn array_item(m: &mut Space, object: u32, index: u32) -> Result<u32, String> {
    stack::function(m, &[index], Some(object), |m| {
        s(m, Reg::Eax, m.r32(0x62dee8));
        stack::alloc(m, 0x60);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        s(m, Reg::Esi, object);
        let owner = m.r32(object + 0x10);
        s(m, Reg::Ecx, owner);
        let length_index = r(m, Reg::Eax);
        let length = stack::call(m, 0x408847, &[length_index], Some(owner), |m| {
            variable_at(m, owner, length_index)
        })?;
        s(m, Reg::Ebp, index);
        s(m, Reg::Edi, length);
        s(m, Reg::Ebx, m.r32(length + 8));
        if (index as i32) < 0 || (index as i32) >= r(m, Reg::Ebx) as i32 {
            return Err("NativeVmLookup: array bounds exception continuation unavailable".into());
        }
        let storage_index = m.r32(0x62dffc);
        s(m, Reg::Ecx, storage_index);
        s(m, Reg::Ecx, owner);
        let storage = stack::call(m, 0x408863, &[storage_index], Some(owner), |m| {
            variable_at(m, owner, storage_index)
        })?;
        s(m, Reg::Esi, storage);
        let vector = m.r32(storage + 8);
        s(m, Reg::Eax, vector);
        if vector == 0 || index >= m.r32(vector) {
            return Err("NativeVmLookup: array storage allocation continuation unavailable".into());
        }
        s(m, Reg::Edx, m.r32(vector + 4));
        let result = m.r32(r(m, Reg::Edx) + index * 4);
        s(m, Reg::Eax, result);
        Ok(result)
    })
}

///405690 reverse field-name search through the static and instance vectors,
/// followed by the original inherited-instance ordinal accumulation.
pub fn field_index(
    m: &mut Space,
    module: u32,
    name: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    stack::function(m, &[name], Some(module), |m| {
        stack::push_reg(m, Reg::Ecx);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        let b = stack::sp(m);
        s(m, Reg::Edi, module);
        if module == 0 {
            return Err("NativeVmLookup: missing field module".into());
        }
        let mut current = module;
        for _ in 0..1024 {
            s(m, Reg::Ebx, m.r32(current + 0x54));
            write(m, b + 0x10, 1);
            loop {
                let vector = r(m, Reg::Ebx);
                let mut index = m.r32(vector + 0xc);
                s(m, Reg::Ebp, index);
                s(m, Reg::Eax, index);
                index = index.wrapping_sub(1);
                s(m, Reg::Ebp, index);
                while (index as i32) >= 0 {
                    if index >= m.r32(vector + 4) {
                        return Err("NativeVmLookup: malformed field vector".into());
                    }
                    s(m, Reg::Ecx, m.r32(vector + 8));
                    let field = m.r32(r(m, Reg::Ecx) + index * 4);
                    s(m, Reg::Esi, field);
                    let pool = m.r32(current + 0x24);
                    let name_index = m.r32(field + 8);
                    s(m, Reg::Edx, name_index);
                    stack::call(m, 0x4056cf, &[name_index], Some(pool), |m| {
                        constant(m, pool, name_index, 0, 8)
                    })?;
                    s(m, Reg::Eax, name_index);
                    let candidate = stack::call(m, 0x4056db, &[name_index], Some(pool), |m| {
                        constant(m, pool, name_index, 0, 8)
                    })?;
                    s(m, Reg::Esi, name);
                    let mut a = candidate;
                    let mut other = name;
                    let matched = loop {
                        let mut done = None;
                        for offset in 0..2 {
                            let lhs = m.r8(a + offset);
                            let rhs = m.r8(other + offset);
                            s(m, Reg::Edx, (r(m, Reg::Edx) & !255) | u32::from(lhs));
                            s(m, Reg::Ecx, (r(m, Reg::Ecx) & !255) | u32::from(lhs));
                            if lhs != rhs {
                                s(m, Reg::Eax, if lhs < rhs { u32::MAX } else { 1 });
                                done = Some(false);
                                break;
                            }
                            if offset == 0 && lhs == 0 {
                                s(m, Reg::Eax, 0);
                                done = Some(true);
                                break;
                            }
                            if offset == 1 {
                                a += 2;
                                other += 2;
                                s(m, Reg::Eax, a);
                                s(m, Reg::Esi, other);
                                if lhs == 0 {
                                    s(m, Reg::Eax, 0);
                                    done = Some(true);
                                }
                            }
                        }
                        if let Some(result) = done {
                            break result;
                        }
                    };
                    if matched {
                        let mode = m.r32(b + 0x10);
                        s(m, Reg::Eax, mode);
                        s(m, Reg::Esi, index);
                        write(m, 0x62f11c, current);
                        if mode != 0 {
                            let result = index | 0x40000000;
                            s(m, Reg::Eax, result);
                            return Ok(result);
                        }
                        let parent = m.r32(current + 0x40);
                        s(m, Reg::Eax, parent);
                        let mut inherited = if (parent as i32) >= 0 {
                            parent_module_with(m, current, 0x405776, 0x40577f, 0x405788, e)?
                        } else {
                            0
                        };
                        if inherited != 0 {
                            s(m, Reg::Edi, 0);
                            s(m, Reg::Esi, inherited);
                            loop {
                                s(m, Reg::Eax, m.r32(inherited + 0x4c));
                                s(m, Reg::Edx, m.r32(r(m, Reg::Eax) + 0xc));
                                let parent = m.r32(inherited + 0x40);
                                s(m, Reg::Eax, parent);
                                s(m, Reg::Edi, r(m, Reg::Edi).wrapping_add(r(m, Reg::Edx)));
                                if (parent as i32) < 0 {
                                    break;
                                }
                                inherited = parent_module_with(
                                    m, inherited, 0x4057a8, 0x4057b1, 0x4057ba, e,
                                )?;
                                s(m, Reg::Esi, inherited);
                                if inherited == 0 {
                                    break;
                                }
                            }
                            s(m, Reg::Esi, r(m, Reg::Edi).wrapping_add(index));
                        }
                        s(m, Reg::Eax, r(m, Reg::Esi));
                        return Ok(r(m, Reg::Eax));
                    }
                    s(m, Reg::Eax, index);
                    index = index.wrapping_sub(1);
                    s(m, Reg::Ebp, index);
                }
                s(m, Reg::Eax, m.r32(b + 0x10));
                s(m, Reg::Ebx, m.r32(current + 0x4c));
                let mode = r(m, Reg::Eax).wrapping_sub(1);
                s(m, Reg::Eax, mode);
                write(m, b + 0x10, mode);
                if (mode as i32) < 0 {
                    break;
                }
            }
            let parent = m.r32(current + 0x40);
            if (parent as i32) < 0 {
                return Err("NativeVmLookup: field not found continuation unavailable".into());
            }
            current = parent_module_with(m, current, 0x405735, 0x40573e, 0x405747, e)?;
            s(m, Reg::Edi, current);
        }
        Err("NativeVmLookup: cyclic field inheritance".into())
    })
}

///406010 constant-string reuse or source-allocated insertion.
pub(crate) fn literal(
    m: &mut Space,
    pool: u32,
    source: u32,
    length: u32,
    a: &mut impl crate::native_vm_pool::Allocator,
) -> Result<u32, String> {
    literal_inner(m, pool, source, length, Some(a))
}
fn literal_inner(
    m: &mut Space,
    pool: u32,
    source: u32,
    length: u32,
    mut allocator: Option<&mut dyn crate::native_vm_pool::Allocator>,
) -> Result<u32, String> {
    stack::function(m, &[source, length], Some(pool), |m| {
        s(m, Reg::Eax, source);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, pool);
        if source == 0 {
            return Err("NativeVmLookup: null literal continuation unavailable".into());
        }
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, length);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, m.r32(pool + 0x10).wrapping_sub(1));
        while (r(m, Reg::Edi) as i32) >= 0 {
            let i = r(m, Reg::Edi);
            if i >= m.r32(pool + 8) {
                return Err("NativeVmLookup: malformed literal pool".into());
            }
            s(m, Reg::Eax, m.r32(pool + 0xc));
            let entry = m.r32(r(m, Reg::Eax) + i * 4);
            s(m, Reg::Eax, entry);
            s(m, Reg::Ecx, m.r32(entry + 4));
            if r(m, Reg::Ecx) == 0 && m.r32(entry + 0xc) == length {
                s(m, Reg::Esi, source);
                let found = m.r32(entry + 8);
                s(m, Reg::Eax, found);
                let mut off = 0;
                loop {
                    let mut finished = false;
                    for j in 0..2 {
                        let a = m.r8(found + off + j);
                        let b = m.r8(source + off + j);
                        low(m, Reg::Edx, a);
                        low(m, Reg::Ecx, a);
                        if a != b {
                            s(m, Reg::Eax, if a < b { u32::MAX } else { 1 });
                            finished = true;
                            break;
                        }
                        if j == 0 && a == 0 {
                            s(m, Reg::Eax, 0);
                            finished = true;
                            break;
                        }
                        if j == 1 {
                            off += 2;
                            s(m, Reg::Eax, found + off);
                            s(m, Reg::Esi, source + off);
                            if a == 0 {
                                s(m, Reg::Eax, 0);
                                finished = true;
                            }
                        }
                    }
                    if finished {
                        break;
                    }
                }
                if r(m, Reg::Eax) == 0 {
                    break;
                }
            }
            s(m, Reg::Edi, i.wrapping_sub(1));
        }
        s(m, Reg::Eax, r(m, Reg::Edi));
        if r(m, Reg::Eax) == u32::MAX {
            let a = allocator
                .take()
                .ok_or("NativeVmLookup: literal insertion continuation unavailable")?;
            stack::push(m, 16);
            let record = stack::external(m, 0x406090, &[], None, |m| a.allocate(m, 16))?;
            s(m, Reg::Eax, record);
            s(m, Reg::Esi, record);
            stack::add_sp(m, 4);
            if record == 0 {
                return Err("NativeVmLookup: constant string allocation failure".into());
            }
            s(m, Reg::Edi, length + 1);
            stack::push(m, length + 1);
            write(m, record, 0x5e738c);
            write(m, record + 4, 0);
            write(m, record + 12, length);
            let buffer = stack::external(m, 0x4060b2, &[], None, |m| a.allocate(m, length + 1))?;
            s(m, Reg::Eax, buffer);
            if buffer == 0 {
                return Err("NativeVmLookup: string buffer allocation failure".into());
            }
            s(m, Reg::Ecx, source);
            stack::push(m, length + 1);
            stack::push(m, source);
            stack::push(m, buffer);
            write(m, record + 8, buffer);
            stack::call(m, 0x4060c1, &[], None, |m| {
                crate::native_vm_metadata::name_copy(m, buffer, source, length + 1)
            });
            s(m, Reg::Edx, buffer);
            stack::add_sp(m, 16);
            byte(m, buffer + length, 0);
            let count = m.r32(pool + 16);
            let capacity = m.r32(pool + 8);
            s(m, Reg::Eax, count + 1);
            s(m, Reg::Ecx, capacity);
            if count + 1 > capacity {
                let next = capacity + count + 1;
                s(m, Reg::Edi, next);
                let old = m.r32(pool + 12);
                s(m, Reg::Edx, old);
                s(m, Reg::Ecx, next * 4);
                stack::push(m, next * 4);
                stack::push(m, old);
                let table = stack::external(m, 0x406103, &[], None, |m| {
                    a.reallocate(m, old, capacity * 4, next * 4)
                })?;
                s(m, Reg::Eax, table);
                stack::add_sp(m, 8);
                if table == 0 {
                    return Err("NativeVmLookup: constant string vector allocation failure".into());
                }
                write(m, pool + 12, table);
                if next < m.r32(pool + 16) {
                    write(m, pool + 16, next);
                }
                write(m, pool + 8, next);
            }
            let count = m.r32(pool + 16);
            s(m, Reg::Eax, count);
            s(m, Reg::Ecx, m.r32(pool + 12));
            write(m, r(m, Reg::Ecx) + count * 4, record);
            s(m, Reg::Edx, count + 1);
            s(m, Reg::Eax, count + 1);
            write(m, pool + 16, count + 1);
            s(m, Reg::Eax, count);
        }
        Ok(r(m, Reg::Eax))
    })
}
///406ea0 inserts or reuses a class constant by its issued string index.
pub(crate) fn class_constant(
    m: &mut Space,
    pool: u32,
    index: u32,
    a: &mut impl crate::native_vm_pool::Allocator,
) -> Result<u32, String> {
    stack::function(m, &[index], Some(pool), |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, pool);
        let count = m.r32(pool + 16);
        s(m, Reg::Eax, count.wrapping_sub(1));
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, index);
        s(m, Reg::Ebx, 4);
        let mut i = count.wrapping_sub(1);
        if (i as i32) >= 0 {
            s(m, Reg::Edx, m.r32(pool + 8));
        }
        while (i as i32) >= 0 {
            if i >= m.r32(pool + 8) {
                return Err("NativeVmLookup: malformed class constant vector".into());
            }
            s(m, Reg::Ecx, m.r32(pool + 12));
            let record = m.r32(r(m, Reg::Ecx) + i * 4);
            s(m, Reg::Ecx, record);
            if m.r32(record + 4) == 4 && m.r32(record + 8) == index {
                return Ok(i);
            }
            i = i.wrapping_sub(1);
            s(m, Reg::Eax, i);
        }
        stack::push(m, 16);
        let record = stack::external(m, 0x406ede, &[], None, |m| a.allocate(m, 16))?;
        s(m, Reg::Eax, record);
        stack::add_sp(m, 4);
        if record == 0 {
            return Err("NativeVmLookup: class constant allocation failure".into());
        }
        write(m, record + 4, 4);
        write(m, record, 0x5e73f0);
        write(m, record + 8, index);
        s(m, Reg::Ebx, record);
        let count = m.r32(pool + 16);
        let cap = m.r32(pool + 8);
        s(m, Reg::Eax, count + 1);
        s(m, Reg::Ecx, cap);
        if count + 1 > cap {
            let next = cap + count + 1;
            s(m, Reg::Edi, next);
            let old = m.r32(pool + 12);
            s(m, Reg::Ecx, old);
            s(m, Reg::Eax, next * 4);
            stack::push(m, next * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x406f2b, &[], None, |m| {
                a.reallocate(m, old, cap * 4, next * 4)
            })?;
            s(m, Reg::Eax, table);
            stack::add_sp(m, 8);
            if table == 0 {
                return Err("NativeVmLookup: class constant vector allocation failure".into());
            }
            write(m, pool + 12, table);
            if next < m.r32(pool + 16) {
                write(m, pool + 16, next);
            }
            write(m, pool + 8, next);
        }
        let count = m.r32(pool + 16);
        s(m, Reg::Edx, count);
        s(m, Reg::Eax, m.r32(pool + 12));
        write(m, r(m, Reg::Eax) + count * 4, record);
        s(m, Reg::Edx, count + 1);
        s(m, Reg::Eax, count + 1);
        write(m, pool + 16, count + 1);
        s(m, Reg::Eax, count);
        Ok(count)
    })
}
//40496e/4049a4: source two-byte strcmp loops. Preserve DL/CL and EDI
// even at the terminating NUL; the SBB result is signed by unsigned bytes.
fn compare_cached_text(m: &mut Space) -> Result<bool, String> {
    for _ in 0..0x80000 {
        let left = m.r8(r(m, Reg::Eax));
        let right = m.r8(r(m, Reg::Edi));
        low(m, Reg::Edx, left);
        low(m, Reg::Ecx, left);
        if left != right {
            s(m, Reg::Eax, if left < right { u32::MAX } else { 1 });
            return Ok(false);
        }
        if left == 0 {
            s(m, Reg::Eax, 0);
            return Ok(true);
        }
        let left = m.r8(r(m, Reg::Eax) + 1);
        let right = m.r8(r(m, Reg::Edi) + 1);
        low(m, Reg::Edx, left);
        low(m, Reg::Ecx, left);
        if left != right {
            s(m, Reg::Eax, if left < right { u32::MAX } else { 1 });
            return Ok(false);
        }
        s(m, Reg::Eax, r(m, Reg::Eax) + 2);
        s(m, Reg::Edi, r(m, Reg::Edi) + 2);
        if left == 0 {
            s(m, Reg::Eax, 0);
            return Ok(true);
        }
    }
    Err("NativeVmLookup: unterminated cached method text".into())
}
///404910 cached lookup and cache-miss path. The actual method/signature/name
///constants and cache pools own each entry; no semantic ID becomes an address.
pub fn method_by_name(
    m: &mut Space,
    class: u32,
    name: u32,
    descriptor: u32,
    e: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<u32, String> {
    let name_length = text(m, name)?.len() as u32;
    let descriptor_length = text(m, descriptor)?.len() as u32;
    let key = name_length.wrapping_mul(descriptor_length);
    stack::function(m, &[name, descriptor], Some(class), |m| {
        stack::alloc(m, 0xc);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, descriptor);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, class);
        s(m, Reg::Ecx, u32::MAX);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        let b = stack::sp(m);
        let cache = class + 0x1c;
        s(m, Reg::Edi, name + name_length + 1);
        s(m, Reg::Eax, 0);
        s(m, Reg::Ecx, name_length);
        s(m, Reg::Edx, name_length);
        s(m, Reg::Ecx, descriptor_length);
        s(m, Reg::Edi, descriptor + descriptor_length + 1);
        s(m, Reg::Edx, key);
        s(m, Reg::Eax, b + 0x18);
        s(m, Reg::Esi, cache);
        write(m, b + 0x10, class);
        write(m, b + 0x18, key);
        let hit = stack::call(m, 0x404951, &[b + 0x18], Some(cache), |m| {
            stack::function(m, &[b + 0x18], Some(cache), |m| {
                s(m, Reg::Eax, b + 0x18);
                s(m, Reg::Edx, key);
                let bucket = cache + (key & 31) * 12;
                s(m, Reg::Eax, bucket);
                stack::push_reg(m, Reg::Esi);
                s(m, Reg::Esi, m.r32(bucket + 4));
                if r(m, Reg::Esi) != 0 {
                    let mut node = bucket;
                    for _ in 0..1024 {
                        if node == 0 {
                            break;
                        }
                        if m.r32(node) == key {
                            write(m, cache + 0x1a8, node);
                            let slot = m.r32(node + 4);
                            s(m, Reg::Eax, slot);
                            return Ok(slot);
                        }
                        node = m.r32(node + 8);
                        s(m, Reg::Eax, node);
                        if node == 0 {
                            break;
                        }
                    }
                    if node != 0 {
                        return Err("NativeVmLookup: cyclic method cache chain".into());
                    }
                }
                s(m, Reg::Eax, 0);
                Ok::<u32, String>(0)
            })
        })?;
        if hit != 0 {
            let mut memo = m.r32(hit);
            s(m, Reg::Eax, memo);
            write(m, b + 0x14, memo);
            let mut attempts = 0;
            while memo != 0 {
                attempts += 1;
                if attempts > 1024 {
                    return Err("NativeVmLookup: cyclic method memo chain".into());
                }
                s(m, Reg::Edi, m.r32(memo + 4));
                s(m, Reg::Eax, name);
                if compare_cached_text(m)? {
                    s(m, Reg::Ebp, memo);
                    s(m, Reg::Edi, m.r32(memo + 8));
                    s(m, Reg::Eax, descriptor);
                    if compare_cached_text(m)? {
                        let record = m.r32(memo);
                        s(m, Reg::Eax, record);
                        if record != 0 {
                            return Ok(record);
                        }
                        break;
                    }
                }
                let current = m.r32(cache + 0x1a8);
                s(m, Reg::Ecx, current);
                let mut next = m.r32(current + 8);
                s(m, Reg::Eax, next);
                if next == 0 {
                    break;
                }
                let wanted = m.r32(current);
                s(m, Reg::Ecx, wanted);
                for _ in 0..1024 {
                    if m.r32(next) == wanted {
                        break;
                    }
                    next = m.r32(next + 8);
                    s(m, Reg::Eax, next);
                    if next == 0 {
                        break;
                    }
                }
                if next == 0 {
                    break;
                }
                if m.r32(next) != wanted {
                    return Err("NativeVmLookup: cyclic cached method continuation".into());
                }
                write(m, cache + 0x1a8, next);
                let slot = m.r32(next + 4);
                s(m, Reg::Eax, slot);
                if slot == 0 {
                    break;
                }
                memo = m.r32(slot);
                s(m, Reg::Eax, memo);
                write(m, b + 0x14, memo);
            }
            s(m, Reg::Ebp, m.r32(b + 0x10));
        }
        s(m, Reg::Eax, name);
        let module = m.r32(class + 4);
        s(m, Reg::Ecx, module);
        let selector = stack::call(m, 0x404a26, &[name, descriptor], Some(module), |m| {
            method_index(m, module, name, descriptor, e)
        })?;
        let mode = (selector >> 27) & 8;
        s(m, Reg::Ecx, mode);
        s(m, Reg::Eax, selector & 0xbfffffff);
        let record = stack::call(
            m,
            0x404a3c,
            &[selector & 0xbfffffff, mode],
            Some(class),
            |m| method_record(m, class, selector & 0xbfffffff, mode, e),
        )?;
        write(m, b + 0x18, record);
        if record == 0 {
            return Err("NativeVmLookup: missing selected method".into());
        }
        let memo = stack::external(m, 0x404a4f, &[12], None, |m| e.allocate(m, 12))?;
        s(m, Reg::Eax, memo);
        s(m, Reg::Ebp, memo);
        if memo == 0 {
            return Err("NativeVmLookup: method cache allocation failed".into());
        }
        s(m, Reg::Edx, class);
        s(m, Reg::Eax, module);
        let pool = m.r32(module + 0x24);
        s(m, Reg::Edx, pool);
        s(m, Reg::Ecx, descriptor_length);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, descriptor + descriptor_length + 1);
        let descriptor_index = stack::call(
            m,
            0x404a7b,
            &[descriptor, descriptor_length],
            Some(pool),
            |m| literal(m, pool, descriptor, descriptor_length, e),
        )?;
        s(m, Reg::Ecx, class);
        s(m, Reg::Edx, module);
        s(m, Reg::Ebx, descriptor_index);
        s(m, Reg::Eax, pool);
        s(m, Reg::Edx, name);
        s(m, Reg::Ecx, u32::MAX);
        write(m, b + 0x14, pool);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, name + name_length + 1);
        s(m, Reg::Ecx, name_length);
        let name_index = stack::call(m, 0x404aa6, &[name, name_length], Some(pool), |m| {
            literal(m, pool, name, name_length, e)
        })?;
        stack::push(m, descriptor_index);
        s(m, Reg::Ebx, class);
        s(m, Reg::Ecx, module);
        s(m, Reg::Ecx, pool);
        s(m, Reg::Edi, name_index);
        let descriptor_text = stack::call(m, 0x404ab8, &[], Some(pool), |m| {
            constant(m, pool, descriptor_index, 0, 8)
        })?;
        stack::add_sp(m, 4);
        s(m, Reg::Edx, module);
        s(m, Reg::Ecx, pool);
        write(m, b + 0x14, descriptor_text);
        let name_text = stack::call(m, 0x404ac8, &[name_index], Some(pool), |m| {
            constant(m, pool, name_index, 0, 8)
        })?;
        s(m, Reg::Edx, descriptor_text);
        s(m, Reg::Ecx, record);
        s(m, Reg::Ebx, descriptor);
        write(m, memo, record);
        write(m, memo + 4, name_text);
        write(m, memo + 8, descriptor_text);
        write(m, b + 0x14, memo);
        s(m, Reg::Edi, name + name_length + 1);
        s(m, Reg::Eax, 0);
        s(m, Reg::Ecx, descriptor_length);
        s(m, Reg::Ebp, key);
        s(m, Reg::Edi, descriptor + descriptor_length + 1);
        let bucket = cache + (key & 31) * 12;
        s(m, Reg::Eax, (key & 31) * 3);
        s(m, Reg::Ebx, bucket);
        s(m, Reg::Eax, m.r32(bucket + 4));
        if r(m, Reg::Eax) != 0 {
            return Err(
                "NativeVmLookup: method cache collision insertion continuation unavailable".into(),
            );
        }
        write(m, bucket, key);
        let free = m.r32(cache + 0x1a4);
        s(m, Reg::Eax, free);
        if free != 0 {
            return Err("NativeVmLookup: method cache free-list continuation unavailable".into());
        }
        let handed = m.r32(cache + 0x19c);
        s(m, Reg::Eax, m.r32(cache + 0x1a0));
        if handed == r(m, Reg::Eax) {
            let blocks = m.r32(cache + 0x198);
            s(m, Reg::Edi, blocks);
            s(m, Reg::Eax, r(m, Reg::Eax) + 32);
            write(m, cache + 0x1a0, r(m, Reg::Eax));
            s(m, Reg::Eax, blocks + 1);
            s(m, Reg::Edx, (blocks + 1) * 4);
            write(m, cache + 0x198, blocks + 1);
            let old = m.r32(cache + 0x194);
            s(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let table = stack::external(m, 0x404cca, &[], None, |m| {
                e.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
            })?;
            s(m, Reg::Eax, table);
            stack::push(m, 0x80);
            write(m, cache + 0x194, table);
            let block = stack::external(m, 0x404cda, &[], None, |m| e.allocate(m, 0x80))?;
            s(m, Reg::Eax, block);
            s(m, Reg::Ecx, m.r32(cache + 0x194));
            stack::add_sp(m, 0xc);
            if table == 0 || block == 0 {
                return Err("NativeVmLookup: method cache block allocation failed".into());
            }
            write(m, table + blocks * 4, block);
        }
        s(m, Reg::Ecx, handed);
        s(m, Reg::Edx, handed + 1);
        write(m, cache + 0x19c, handed + 1);
        s(m, Reg::Eax, handed / 32);
        let table = m.r32(cache + 0x194);
        s(m, Reg::Edx, table);
        s(m, Reg::Ecx, handed & 31);
        let slot = m.r32(table + (handed / 32) * 4) + (handed & 31) * 4;
        s(m, Reg::Eax, slot);
        s(m, Reg::Ecx, memo);
        write(m, slot, memo);
        write(m, bucket + 4, slot);
        write(m, bucket + 8, 0);
        s(m, Reg::Eax, record);
        write(m, record + 0xc, 0);
        Ok(record)
    })
}

#[cfg(test)]
#[path = "native_vm_method_cache_tests.rs"]
mod method_cache_tests;

///406660 resolves only a real Member constant. The resource table is an
/// explicit domain collaborator reached through its actual4122b0 CALL frame.
pub fn resource_literal(
    m: &mut Space,
    pool: u32,
    index: u32,
    e: &mut impl crate::native_vm_thread_run::Environment,
) -> Result<u32, String> {
    stack::function(m, &[index], Some(pool), |m| {
        s(m, Reg::Eax, index);
        if (index as i32) < 0 || index >= m.r32(pool + 8) {
            s(m, Reg::Eax, 0);
            return Ok(0);
        }
        let table = m.r32(pool + 12);
        s(m, Reg::Edx, table);
        let member = m.r32(table + index * 4);
        s(m, Reg::Edx, member);
        if member == 0 || m.r32(member + 4) != 3 {
            s(m, Reg::Eax, 0);
            return Ok(0);
        }
        let package_index = m.r32(member + 12);
        s(m, Reg::Eax, package_index);
        let mut package = 0;
        let mut named = false;
        if (package_index as i32) >= 0 {
            if package_index >= m.r32(pool + 8) {
                return Err(
                    "NativeVmLookup: resource package constant is outside the original pool".into(),
                );
            }
            s(m, Reg::Ecx, table);
            let record = m.r32(table + package_index * 4);
            s(m, Reg::Eax, record);
            if record == 0 {
                return Err("NativeVmLookup: missing resource package constant".into());
            }
            s(m, Reg::Ecx, m.r32(record + 4));
            if r(m, Reg::Ecx) == 0 {
                package = m.r32(record + 8);
                named = true;
            }
        }
        let id = m.r32(member + 8);
        s(m, Reg::Edx, id);
        s(m, Reg::Eax, package);
        stack::push(m, id);
        stack::push(m, package);
        let result = stack::external(m, if named { 0x4066a7 } else { 0x4066b9 }, &[], None, |m| {
            e.resource(m, package, id)
        })?;
        s(m, Reg::Eax, result);
        stack::add_sp(m, 8);
        Ok(result)
    })
}

///403840: search actual owned variable names in reverse declaration order.
fn variable_by_name(m: &mut Space, owner: u32, name: u32) -> Result<u32, String> {
    stack::function(m, &[name], Some(owner), |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        let vector = m.r32(owner + 0xc);
        s(m, Reg::Ebp, vector);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        let count = m.r32(vector + 8);
        s(m, Reg::Edi, count.wrapping_sub(1));
        s(m, Reg::Eax, count);
        for index in (0..count).rev() {
            s(m, Reg::Edx, name);
            if index >= m.r32(vector) {
                return Err("NativeVmLookup: named variable outside owner capacity".into());
            }
            s(m, Reg::Ecx, m.r32(vector + 4));
            let cell = m.r32(r(m, Reg::Ecx) + index * 4);
            s(m, Reg::Eax, m.r32(cell + 0xc));
            s(m, Reg::Esi, name);
            let mut equal = false;
            for _ in 0..1024 {
                let a = m.r8(r(m, Reg::Eax));
                let b = m.r8(r(m, Reg::Esi));
                low(m, Reg::Ebx, a);
                low(m, Reg::Ecx, a);
                if a != b {
                    s(m, Reg::Eax, if a < b { u32::MAX } else { 1 });
                    break;
                }
                if a == 0 {
                    s(m, Reg::Eax, 0);
                    equal = true;
                    break;
                }
                let a = m.r8(r(m, Reg::Eax) + 1);
                let b = m.r8(r(m, Reg::Esi) + 1);
                low(m, Reg::Ebx, a);
                low(m, Reg::Ecx, a);
                if a != b {
                    s(m, Reg::Eax, if a < b { u32::MAX } else { 1 });
                    break;
                }
                s(m, Reg::Eax, r(m, Reg::Eax) + 2);
                s(m, Reg::Esi, r(m, Reg::Esi) + 2);
                if a == 0 {
                    s(m, Reg::Eax, 0);
                    equal = true;
                    break;
                }
            }
            if equal {
                s(m, Reg::Ecx, m.r32(vector + 4));
                s(m, Reg::Eax, cell);
                return Ok(cell);
            }
            s(m, Reg::Eax, index);
            s(m, Reg::Edi, index.wrapping_sub(1));
        }
        s(m, Reg::Eax, 0);
        Ok(0)
    })
}
///408800: the original static-first, then instance named-field lookup.
pub(crate) fn object_field_by_name(m: &mut Space, object: u32, name: u32) -> Result<u32, String> {
    stack::function(m, &[name], Some(object), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, name);
        s(m, Reg::Esi, object);
        let class = m.r32(object + 0xc);
        s(m, Reg::Ecx, class);
        let field = stack::call(m, 0x408811, &[name], Some(class), |m| {
            stack::function(m, &[name], Some(class), |m| {
                stack::push_reg(m, Reg::Esi);
                s(m, Reg::Esi, class);
                stack::call(m, 0x404828, &[], Some(class), |m| {
                    initialized_class(m, class)
                })?;
                s(m, Reg::Eax, name);
                let owner = m.r32(class + 0x1d0);
                s(m, Reg::Ecx, owner);
                stack::call(m, 0x404838, &[name], Some(owner), |m| {
                    variable_by_name(m, owner, name)
                })
            })
        })?;
        if field != 0 {
            return Ok(field);
        }
        let owner = m.r32(object + 0x10);
        s(m, Reg::Ecx, owner);
        stack::call(m, 0x40881e, &[name], Some(owner), |m| {
            variable_by_name(m, owner, name)
        })
    })
}

///405250 ordinary class inheritance distance through actual module owners.
pub(crate) fn module_distance(m: &mut Space, module: u32, base: u32) -> Result<u32, String> {
    stack::function(m, &[base], Some(module), |m| {
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, base);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Esi, module);
        s(m, Reg::Eax, m.r32(module + 0x38));
        stack::push_reg(m, Reg::Edi);
        if m.r32(module + 0x38) != m.r32(base + 0x38) {
            s(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        if m.r32(module + 0x38) != 0 {
            return Err("NativeVmLookup: interface distance continuation unavailable".into());
        }
        s(m, Reg::Edi, 0);
        let mut current = module;
        for distance in 0..512 {
            if current == base {
                let result = if distance == 0 { 0 } else { distance + 100 };
                s(m, Reg::Edi, result);
                s(m, Reg::Eax, result);
                return Ok(result);
            }
            let index = m.r32(current + 0x40);
            s(m, Reg::Eax, index);
            s(m, Reg::Edi, distance + 1);
            if (index as i32) < 0 {
                s(m, Reg::Eax, u32::MAX);
                return Ok(u32::MAX);
            }
            let pool = m.r32(current + 0x24);
            s(m, Reg::Ecx, pool);
            let name_index = stack::call(m, 0x405284, &[index], Some(pool), |m| {
                constant(m, pool, index, 4, 8)
            })?;
            s(m, Reg::Ecx, pool);
            let name = stack::call(m, 0x40528d, &[name_index], Some(pool), |m| {
                constant(m, pool, name_index, 0, 8)
            })?;
            let jvm = m.r32(current + 0xc);
            s(m, Reg::Ecx, jvm);
            current = stack::call(m, 0x405296, &[name], Some(jvm), |m| {
                module_from_name(m, jvm, name)
            })?;
            s(m, Reg::Esi, current);
            if current == 0 {
                s(m, Reg::Eax, u32::MAX);
                return Ok(u32::MAX);
            }
        }
        Err("NativeVmLookup: cyclic module inheritance".into())
    })
}
