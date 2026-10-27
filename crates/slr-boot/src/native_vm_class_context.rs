//! Source403a90 class constructor, including descriptor scratch and class-cache
//! pool allocation from the actual caller. No semantic IDs or pointer templates.
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
fn allocate(m: &mut Space, pc: u32, n: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| a.allocate(m, n))?;
    s(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmClassContext: allocation failure".into());
    }
    Ok(p)
}
fn copy(m: &mut Space, pc: u32, d: u32, src: u32) {
    stack::call(m, pc, &[], None, |m| {
        crate::native_vm_metadata::name_copy(m, d, src, 256)
    });
}
fn append(m: &mut Space, pc: u32, d: u32, src: u32) -> Result<(), String> {
    stack::call(m, pc, &[], None, |m| {
        crate::native_vm_callback::append(m, d, src, 256)
    })
}
/// Source cache's class-pointer pool, with distinct CALL sites for occupied
/// and empty buckets. The source cache node and pointer pools stay distinct.
fn slot(
    m: &mut Space,
    cache: u32,
    bucket: u32,
    occupied: bool,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let h = cache + 0x1814;
    let free = m.r32(h + 16);
    s(m, Reg::Eax, free);
    if free != 0 {
        s(m, Reg::Edx, m.r32(free));
        write(m, h + 16, r(m, Reg::Edx));
        return Ok(free);
    }
    let handed = m.r32(h + 8);
    let cap = m.r32(h + 12);
    s(m, Reg::Eax, cap);
    if handed == cap {
        let blocks = m.r32(h + 4);
        let old = m.r32(h);
        s(m, Reg::Ebp, blocks);
        s(m, Reg::Ecx, old);
        s(m, Reg::Eax, cap + 64);
        write(m, h + 12, cap + 64);
        s(m, Reg::Eax, blocks + 1);
        write(m, h + 4, blocks + 1);
        s(m, Reg::Eax, (blocks + 1) * 4);
        stack::push(m, (blocks + 1) * 4);
        stack::push(m, old);
        let p = stack::external(
            m,
            if occupied { 0x403d48 } else { 0x403e07 },
            &[],
            None,
            |m| a.reallocate(m, old, blocks * 4, (blocks + 1) * 4),
        )?;
        s(m, Reg::Eax, p);
        if p == 0 {
            return Err("NativeVmClassContext: cache pointer table allocation failure".into());
        }
        stack::push(m, 256);
        write(m, h, p);
        let block = allocate(m, if occupied { 0x403d58 } else { 0x403e17 }, 256, a)?;
        s(m, Reg::Edx, p);
        s(m, Reg::Ecx, bucket);
        stack::add_sp(m, 12);
        write(m, p + blocks * 4, block);
    }
    s(m, Reg::Ebp, handed);
    s(m, Reg::Eax, handed + 1);
    write(m, h + 8, handed + 1);
    s(m, Reg::Eax, handed / 64);
    s(m, Reg::Edx, m.r32(h));
    s(m, Reg::Ebp, handed % 64);
    let block = m.r32(m.r32(h) + (handed / 64) * 4);
    s(m, Reg::Eax, block);
    let slot = block + (handed % 64) * 4;
    s(m, Reg::Eax, slot);
    Ok(slot)
}
pub fn construct(
    m: &mut Space,
    class: u32,
    jvm: u32,
    module: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    if !stack::active(m)
        || !stack::has_pending_call(m)
        || r(m, Reg::Ecx) != class
        || class == 0
        || jvm == 0
        || module == 0
    {
        return Err("NativeVmClassContext: actual class constructor CALL required".into());
    }
    stack::function(m, &[jvm, module], Some(class), |m| {
        stack::alloc(m, 0x204);
        for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        let base = stack::sp(m);
        s(m, Reg::Edi, class);
        for (offset, pc) in [(0x19c, 0x403aa7), (0x1b0, 0x403ab2)] {
            let p = class + offset;
            s(m, Reg::Ecx, p);
            stack::call(m, pc, &[], Some(p), |m| {
                stack::function(m, &[], Some(p), |m| {
                    s(m, Reg::Eax, p);
                    s(m, Reg::Ecx, 0);
                    for o in [0, 4, 8, 12, 16] {
                        write(m, p + o, 0);
                    }
                })
            });
        }
        s(m, Reg::Eax, class + 0x20);
        s(m, Reg::Ecx, 32);
        for i in 0..32 {
            write(m, class + 0x20 + i * 12, 0);
            s(m, Reg::Eax, class + 0x20 + (i + 1) * 12);
            s(m, Reg::Ecx, 31 - i);
        }
        s(m, Reg::Esi, module);
        s(m, Reg::Ebx, jvm);
        write(m, class, 0x5e7340);
        write(m, class + 4, module);
        let pool = m.r32(module + 0x24);
        s(m, Reg::Eax, pool);
        write(m, class + 0x14, pool);
        s(m, Reg::Ecx, m.r32(module + 0x60));
        write(m, class + 0x18, r(m, Reg::Ecx));
        s(m, Reg::Edx, m.r32(module + 0x38));
        write(m, class + 0x10, r(m, Reg::Edx));
        write(m, class + 12, jvm);
        write(m, class + 0x1d4, 0);
        let ci = m.r32(module + 0x3c);
        s(m, Reg::Eax, ci);
        s(m, Reg::Ecx, pool);
        let ni = stack::call(m, 0x403b08, &[ci], Some(pool), |m| {
            crate::native_vm_lookup::constant(m, pool, ci, 4, 8)
        })?;
        s(m, Reg::Ecx, pool);
        let name = stack::call(m, 0x403b11, &[ni], Some(pool), |m| {
            crate::native_vm_lookup::constant(m, pool, ni, 0, 8)
        })?;
        s(m, Reg::Esi, name);
        low(m, Reg::Eax, m.r8(name));
        let scratch = base + 0x14;
        if m.r8(name) == b'[' {
            s(m, Reg::Ebp, scratch);
            let mut source = name;
            let mut destination = scratch;
            while m.r8(source) == b'[' {
                store_byte(m, destination, m.r8(source));
                low(m, Reg::Eax, m.r8(source + 1));
                source += 1;
                destination += 1;
                s(m, Reg::Ebp, destination);
                s(m, Reg::Esi, source);
            }
            if matches!(m.r8(source), b'I' | b'F') {
                stack::push(m, 256);
                stack::push(m, source);
                stack::push(m, destination);
                copy(m, 0x403b66, destination, source);
                stack::add_sp(m, 12);
            } else {
                stack::push(m, 256);
                stack::push(m, 0x60750c);
                stack::push(m, destination);
                copy(m, 0x403b41, destination, 0x60750c);
                stack::push(m, 256);
                stack::push(m, source);
                stack::push(m, destination);
                append(m, 0x403b4d, destination, source)?;
                stack::push(m, 256);
                stack::push(m, 0x607510);
                stack::push(m, destination);
                append(m, 0x403ba3, destination, 0x607510)?;
                stack::add_sp(m, 36);
            }
        } else {
            stack::push(m, 256);
            s(m, Reg::Ecx, scratch);
            stack::push(m, 0x607514);
            stack::push(m, scratch);
            copy(m, 0x403b7f, scratch, 0x607514);
            stack::push(m, 256);
            s(m, Reg::Edx, scratch);
            stack::push(m, name);
            stack::push(m, scratch);
            append(m, 0x403b8f, scratch, name)?;
            stack::push(m, 256);
            stack::push(m, 0x607518);
            s(m, Reg::Eax, scratch);
            stack::push(m, scratch);
            append(m, 0x403ba3, scratch, 0x607518)?;
            stack::add_sp(m, 36);
        }
        stack::push(m, 4);
        let cell = allocate(m, 0x403bad, 4, a)?;
        s(m, Reg::Esi, cell);
        s(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        s(m, Reg::Ecx, scratch);
        stack::push(m, scratch);
        stack::push(m, jvm);
        s(m, Reg::Ecx, cell);
        stack::call(m, 0x403bc5, &[], Some(cell), |m| {
            crate::native_vm_signature::descriptor(m, cell, jvm, scratch, a)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Eax, cell);
        s(m, Reg::Esi, module);
        write(m, class + 8, cell);
        write(m, class + 0x1d0, 0);
        write(m, class + 0x1cc, 0);
        s(m, Reg::Edx, ci);
        s(m, Reg::Ecx, pool);
        let ni = stack::call(m, 0x403be9, &[ci], Some(pool), |m| {
            crate::native_vm_lookup::constant(m, pool, ci, 4, 8)
        })?;
        s(m, Reg::Ecx, pool);
        let name = stack::call(m, 0x403bf2, &[ni], Some(pool), |m| {
            crate::native_vm_lookup::constant(m, pool, ni, 0, 8)
        })?;
        low(m, Reg::Ecx, m.r8(name));
        s(m, Reg::Edx, 0);
        s(m, Reg::Esi, 0);
        let mut i = 0;
        let mut sum = 0u32;
        while m.r8(name + i) != 0 {
            s(m, Reg::Ecx, i32::from(m.r8(name + i) as i8) as u32);
            sum = sum.wrapping_add(r(m, Reg::Ecx));
            s(m, Reg::Edx, sum);
            low(m, Reg::Ecx, m.r8(name + i + 1));
            i += 1;
            s(m, Reg::Eax, name + i);
            s(m, Reg::Esi, i);
        }
        s(m, Reg::Eax, sum.wrapping_mul(9));
        let hash = sum.wrapping_mul(37).wrapping_add(i * 7);
        s(m, Reg::Ebp, sum.wrapping_mul(37));
        s(m, Reg::Ecx, i * 7);
        let cache = m.r32(jvm + 8);
        s(m, Reg::Esi, cache);
        s(m, Reg::Ebp, hash);
        s(m, Reg::Eax, hash & 511);
        s(m, Reg::Edx, (hash & 511) * 3);
        let bucket = cache + (hash & 511) * 12;
        s(m, Reg::Eax, m.r32(bucket + 4));
        s(m, Reg::Ecx, bucket);
        write(m, base + 0x10, bucket);
        if m.r32(bucket + 4) == 0 {
            write(m, bucket, hash);
            let pointer = slot(m, cache, bucket, false, a)?;
            write(m, pointer, class);
            write(m, bucket + 4, pointer);
            s(m, Reg::Eax, class);
            write(m, bucket + 8, 0);
        } else {
            let h = cache + 0x1800;
            let mut node = m.r32(h + 16);
            s(m, Reg::Eax, node);
            if node != 0 {
                s(m, Reg::Edx, m.r32(node));
                write(m, h + 16, r(m, Reg::Edx));
                s(m, Reg::Ebx, node);
            } else {
                let count = m.r32(h + 8);
                let cap = m.r32(h + 12);
                s(m, Reg::Eax, cap);
                if count == cap {
                    let blocks = m.r32(h + 4);
                    let old = m.r32(h);
                    s(m, Reg::Ebx, blocks);
                    s(m, Reg::Ecx, old);
                    s(m, Reg::Eax, cap + 64);
                    write(m, h + 12, cap + 64);
                    s(m, Reg::Eax, blocks + 1);
                    write(m, h + 4, blocks + 1);
                    s(m, Reg::Eax, (blocks + 1) * 4);
                    stack::push(m, (blocks + 1) * 4);
                    stack::push(m, old);
                    let table = stack::external(m, 0x403c8a, &[], None, |m| {
                        a.reallocate(m, old, blocks * 4, (blocks + 1) * 4)
                    })?;
                    s(m, Reg::Eax, table);
                    stack::push(m, 768);
                    write(m, h, table);
                    let block = allocate(m, 0x403c9a, 768, a)?;
                    s(m, Reg::Edx, table);
                    stack::add_sp(m, 12);
                    write(m, table + blocks * 4, block);
                }
                s(m, Reg::Ecx, count);
                s(m, Reg::Ebx, m.r32(h));
                s(m, Reg::Eax, count + 1);
                write(m, h + 8, count + 1);
                s(m, Reg::Eax, count % 64);
                s(m, Reg::Edx, count / 64);
                s(m, Reg::Ecx, m.r32(m.r32(h) + (count / 64) * 4));
                s(m, Reg::Eax, (count % 64) * 3);
                node = r(m, Reg::Ecx) + (count % 64) * 12;
                s(m, Reg::Ebx, node);
                s(m, Reg::Ecx, bucket);
            }
            write(m, node, hash);
            let pointer = slot(m, cache, bucket, true, a)?;
            write(m, pointer, class);
            write(m, node + 4, pointer);
            s(m, Reg::Edx, m.r32(bucket + 8));
            s(m, Reg::Eax, class);
            write(m, node + 8, r(m, Reg::Edx));
            write(m, bucket + 8, node);
        }
        Ok(class)
    })
}
