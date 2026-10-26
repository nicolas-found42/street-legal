//! Source40fe70 bootstrap prefix. Intermediate contexts retain the entered
//! original caller; no unwind, completed bootstrap, or class load is implied.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<StoreObserver>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]
pub fn set_test_store_observer(observer: Option<fn(u32, u32, u32)>) {
    OBSERVER.with(|o| o.set(observer));
}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    if p < 0x7fe00000 {
        OBSERVER.with(|o| {
            if let Some(f) = o.get() {
                f(p, 4, v)
            }
        })
    }
    m.w32(p, v);
}
fn byte(m: &mut Space, p: u32, v: u8) {
    #[cfg(test)]
    if p < 0x7fe00000 {
        OBSERVER.with(|o| {
            if let Some(f) = o.get() {
                f(p, 1, v as u32)
            }
        })
    }
    m.w8(p, v);
}
// Unicorn reports the full source register for MOV byte [EDX],CL. Preserve
// that stronger raw-store observation as well as the actual low byte.
fn byte_from_cl(m: &mut Space, p: u32) {
    let source = get(m, Reg::Ecx);
    #[cfg(test)]
    OBSERVER.with(|o| {
        if let Some(f) = o.get() {
            f(p, 1, source)
        }
    });
    m.w8(p, source as u8);
}
fn set(m: &mut Space, r: Reg, v: u32) {
    stack::set_reg(m, r, v)
}
fn get(m: &Space, r: Reg) -> u32 {
    stack::reg(m, r)
}
fn pop(m: &mut Space, r: Reg) {
    set(m, r, stack::load(m, 0));
    stack::add_sp(m, 4)
}
fn allocate(
    m: &mut Space,
    pc: u32,
    n: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.allocate(m, n))?;
    if p == 0 {
        return Err("NativeVmBootstrap: allocation failure outside admitted source path".into());
    }
    set(m, Reg::Eax, p);
    Ok(p)
}
fn realloc(
    m: &mut Space,
    pc: u32,
    old: u32,
    old_n: u32,
    n: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.reallocate(m, old, old_n, n))?;
    if p == 0 {
        return Err("NativeVmBootstrap: reallocation failure".into());
    }
    set(m, Reg::Eax, p);
    Ok(p)
}
//551120→5548c0 exact byte loop and retained CALL scratch.
fn copy(m: &mut Space, dst: u32, src: u32, n: u32) {
    stack::function_cdecl(m, &[dst, src, n], |m| {
        set(m, Reg::Eax, n);
        set(m, Reg::Ecx, src);
        set(m, Reg::Edx, dst);
        stack::call(m, 0x551134, &[dst, src, n], None, |m| {
            stack::function_cdecl(m, &[dst, src, n], |m| {
                set(m, Reg::Eax, dst);
                stack::push_reg(m, Reg::Ebx);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                set(m, Reg::Esi, src);
                let mut ch = m.r8(src);
                set(m, Reg::Ecx, (get(m, Reg::Ecx) & !255) | ch as u32);
                stack::push_reg(m, Reg::Edi);
                set(m, Reg::Edi, 1);
                set(m, Reg::Edx, dst);
                let mut offset = 0;
                if ch != 0 {
                    set(m, Reg::Ebx, n);
                    loop {
                        let edi = get(m, Reg::Edi);
                        set(m, Reg::Ebp, edi);
                        set(m, Reg::Edi, edi + 1);
                        if edi as i32 >= n as i32 {
                            break;
                        }
                        byte_from_cl(m, dst + offset);
                        offset += 1;
                        ch = m.r8(src + offset);
                        set(m, Reg::Ecx, (get(m, Reg::Ecx) & !255) | ch as u32);
                        set(m, Reg::Edx, dst + offset);
                        set(m, Reg::Esi, src + offset);
                        if ch == 0 {
                            break;
                        }
                    }
                }
                stack::add_sp(m, 12);
                byte(m, dst + offset, 0);
                stack::add_sp(m, 4);
            })
        });
    })
}
fn lookup(m: &mut Space, table: u32, hash: u32) -> Result<u32, String> {
    stack::function(m, &[hash], Some(table), |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, hash);
        set(m, Reg::Edx, 0);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Eax, hash);
        set(m, Reg::Edi, 0xa97);
        let bucket = table + (hash % 0xa97) * 12;
        set(m, Reg::Eax, (hash % 0xa97) * 3);
        let slot = m.r32(bucket + 4);
        set(m, Reg::Edx, slot);
        set(m, Reg::Eax, bucket);
        if slot != 0 {
            if m.r32(bucket) != hash {
                return Err("NativeVmBootstrap: source name collision chain unresolved".into());
            }
            write(m, table + 0x7f3c, bucket);
            set(m, Reg::Eax, slot);
            let p = m.r32(slot);
            set(m, Reg::Eax, p);
            stack::add_sp(m, 8);
            return Ok(p);
        }
        stack::add_sp(m, 4);
        set(m, Reg::Eax, 0);
        stack::add_sp(m, 4);
        Ok(0)
    })
}
//41ef88..41f0f7 inserts an occupied canonical-name bucket.
fn insert_collision(
    m: &mut Space,
    table: u32,
    bucket: u32,
    key: u32,
    value: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<(), String> {
    let free = m.r32(table + 0x7f24);
    set(m, Reg::Edi, free);
    let node = if free != 0 {
        set(m, Reg::Ecx, m.r32(free));
        write(m, table + 0x7f24, get(m, Reg::Ecx));
        free
    } else {
        let capacity = m.r32(table + 0x7f20);
        set(m, Reg::Eax, capacity);
        if m.r32(table + 0x7f1c) == capacity {
            let blocks = m.r32(table + 0x7f18);
            set(m, Reg::Edi, blocks);
            set(m, Reg::Eax, capacity + 1024);
            write(m, table + 0x7f20, capacity + 1024);
            set(m, Reg::Eax, blocks + 1);
            set(m, Reg::Edx, (blocks + 1) * 4);
            write(m, table + 0x7f18, blocks + 1);
            let old = m.r32(table + 0x7f14);
            set(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let index = realloc(m, 0x41efdb, old, blocks * 4, (blocks + 1) * 4, e)?;
            stack::push(m, 0x3000);
            write(m, table + 0x7f14, index);
            let block = allocate(m, 0x41efeb, 0x3000, e)?;
            set(m, Reg::Ecx, m.r32(table + 0x7f14));
            stack::add_sp(m, 12);
            write(m, index + blocks * 4, block);
        }
        let count = m.r32(table + 0x7f1c);
        let index = m.r32(table + 0x7f14);
        set(m, Reg::Ecx, count);
        set(m, Reg::Edi, index);
        set(m, Reg::Edx, count + 1);
        write(m, table + 0x7f1c, count + 1);
        set(m, Reg::Edx, count / 1024);
        set(m, Reg::Eax, (count & 1023) * 3);
        let block = m.r32(index + (count / 1024) * 4);
        set(m, Reg::Ecx, block);
        set(m, Reg::Edi, block + (count & 1023) * 12);
        get(m, Reg::Edi)
    };
    let hash = m.r32(key);
    set(m, Reg::Edx, hash);
    write(m, node, hash);
    let free = m.r32(table + 0x7f38);
    set(m, Reg::Eax, free);
    let slot = if free != 0 {
        set(m, Reg::Ecx, m.r32(free));
        write(m, table + 0x7f38, get(m, Reg::Ecx));
        free
    } else {
        let capacity = m.r32(table + 0x7f34);
        set(m, Reg::Eax, capacity);
        if m.r32(table + 0x7f30) == capacity {
            let blocks = m.r32(table + 0x7f2c);
            set(m, Reg::Ebx, blocks);
            set(m, Reg::Eax, capacity + 1024);
            write(m, table + 0x7f34, capacity + 1024);
            set(m, Reg::Eax, blocks + 1);
            set(m, Reg::Edx, (blocks + 1) * 4);
            write(m, table + 0x7f2c, blocks + 1);
            let old = m.r32(table + 0x7f28);
            set(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let index = realloc(m, 0x41f08a, old, blocks * 4, (blocks + 1) * 4, e)?;
            stack::push(m, 0x1000);
            write(m, table + 0x7f28, index);
            let block = allocate(m, 0x41f09a, 0x1000, e)?;
            set(m, Reg::Ecx, m.r32(table + 0x7f28));
            stack::add_sp(m, 12);
            write(m, index + blocks * 4, block);
        }
        let count = m.r32(table + 0x7f30);
        set(m, Reg::Ecx, count & 1023);
        set(m, Reg::Edx, count + 1);
        write(m, table + 0x7f30, count + 1);
        let index = m.r32(table + 0x7f28);
        set(m, Reg::Edx, index);
        let slot = m.r32(index + (count / 1024) * 4) + (count & 1023) * 4;
        set(m, Reg::Eax, slot);
        slot
    };
    set(m, Reg::Ecx, value);
    set(m, Reg::Edx, m.r32(value));
    write(m, slot, get(m, Reg::Edx));
    write(m, node + 4, slot);
    set(m, Reg::Eax, m.r32(bucket + 8));
    write(m, node + 8, get(m, Reg::Eax));
    write(m, bucket + 8, node);
    Ok(())
}
//41ef60 empty-bucket insertion used by the four source builtin names. Other
//collision/pool states are explicit unresolved inputs, not guessed headers.
fn insert(
    m: &mut Space,
    table: u32,
    key: u32,
    value: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<(), String> {
    stack::function(m, &[key, value], Some(table), |m| {
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Ebx, key);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, table);
        let hash = m.r32(key);
        set(m, Reg::Ecx, hash);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edx, 0);
        set(m, Reg::Eax, hash);
        set(m, Reg::Edi, 0xa97);
        let bucket = table + (hash % 0xa97) * 12;
        set(m, Reg::Edx, hash % 0xa97);
        set(m, Reg::Eax, (hash % 0xa97) * 3);
        set(m, Reg::Ebp, bucket);
        set(m, Reg::Eax, m.r32(bucket + 4));
        if m.r32(bucket + 4) != 0 {
            return insert_collision(m, table, bucket, key, value, e);
        }
        if m.r32(table + 0x7f38) != 0 {
            return Err(
                "NativeVmBootstrap: occupied bucket/freelist outside fresh builtin insertion"
                    .into(),
            );
        }
        write(m, bucket, hash);
        set(m, Reg::Eax, 0);
        let count = m.r32(table + 0x7f30);
        let cap = m.r32(table + 0x7f34);
        set(m, Reg::Eax, cap);
        if count == cap {
            let blocks = m.r32(table + 0x7f2c);
            set(m, Reg::Edi, blocks);
            set(m, Reg::Eax, cap + 1024);
            write(m, table + 0x7f34, cap + 1024);
            set(m, Reg::Eax, blocks + 1);
            set(m, Reg::Edx, (blocks + 1) * 4);
            write(m, table + 0x7f2c, blocks + 1);
            let old = m.r32(table + 0x7f28);
            set(m, Reg::Eax, old);
            stack::push(m, (blocks + 1) * 4);
            stack::push(m, old);
            let index = realloc(m, 0x41f150, old, blocks * 4, (blocks + 1) * 4, e)?;
            stack::push(m, 0x1000);
            write(m, table + 0x7f28, index);
            let block = allocate(m, 0x41f160, 0x1000, e)?;
            set(m, Reg::Ecx, index);
            stack::add_sp(m, 12);
            write(m, index + blocks * 4, block);
        }
        set(m, Reg::Ecx, count);
        set(m, Reg::Edx, count + 1);
        write(m, table + 0x7f30, count + 1);
        set(m, Reg::Eax, count);
        set(m, Reg::Edx, 0);
        let index = m.r32(table + 0x7f28);
        set(m, Reg::Edx, index);
        set(m, Reg::Eax, count / 1024);
        set(m, Reg::Ecx, count % 1024);
        let slot = m.r32(index + (count / 1024) * 4) + (count % 1024) * 4;
        set(m, Reg::Eax, slot);
        set(m, Reg::Ecx, value);
        let record = m.r32(value);
        set(m, Reg::Edx, record);
        pop(m, Reg::Edi);
        write(m, slot, record);
        write(m, bucket + 4, slot);
        pop(m, Reg::Esi);
        write(m, bucket + 8, 0);
        pop(m, Reg::Ebp);
        pop(m, Reg::Ebx);
        Ok(())
    })
}
///41e610's fresh-name suffix. The caller retains the actual interner frame.
/// Occupied hash buckets remain a guarded insertion dependency.
pub(crate) fn allocate_name(
    m: &mut Space,
    holder: u32,
    src: u32,
    len: u32,
    hash: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<u32, String> {
    let table = m.r32(holder + 4);
    stack::push(m, 20);
    let record = allocate(m, 0x41e707, 20, e)?;
    set(m, Reg::Esi, 0);
    stack::add_sp(m, 4);
    for off in [4, 8, 12, 16] {
        write(m, record + off, 0)
    }
    write(m, record, 0x5f0980);
    set(m, Reg::Esi, record);
    set(m, Reg::Eax, hash);
    stack::store(m, 24, hash);
    set(m, Reg::Ecx, holder);
    set(m, Reg::Ecx, table);
    let value = stack::sp(m) + 28;
    let key = stack::sp(m) + 24;
    set(m, Reg::Edx, value);
    set(m, Reg::Eax, key);
    m.w32(value, record);
    stack::call(m, 0x41e77a, &[key, value], Some(table), |m| {
        insert(m, table, key, value, e)
    })?;
    set(m, Reg::Edi, len + 1);
    stack::push(m, len + 1);
    let buf = allocate(m, 0x41e783, len + 1, e)?;
    write(m, record + 4, buf);
    stack::call(m, 0x41e78e, &[buf, src, len + 1], None, |m| {
        copy(m, buf, src, len + 1)
    });
    set(m, Reg::Edx, buf);
    stack::add_sp(m, 4);
    byte(m, buf + len, 0);
    set(m, Reg::Eax, buf);
    pop(m, Reg::Edi);
    write(m, record + 8, len);
    write(m, record + 12, 1);
    write(m, record + 16, 0);
    pop(m, Reg::Esi);
    pop(m, Reg::Ebp);
    pop(m, Reg::Ebx);
    pop(m, Reg::Ecx);
    Ok(buf)
}
fn intern_raw(
    m: &mut Space,
    holder: u32,
    src: u32,
    len: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<u32, String> {
    stack::function(m, &[src, len], Some(holder), |m| {
        if src == 0 || len == 0 || len > 3 {
            return Err(
                "NativeVmBootstrap: non-builtin canonical name source path unresolved".into(),
            );
        }
        stack::push_reg(m, Reg::Ecx);
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Ebx, src);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::store(m, 16, holder);
        set(m, Reg::Ebp, len);
        let hash = (m.r8(src) as i8 as i32).wrapping_add(len as i32) as u32;
        set(m, Reg::Eax, hash);
        let table = m.r32(holder + 4);
        set(m, Reg::Ecx, table);
        let found = stack::call(m, 0x41e660, &[hash], Some(table), |m| {
            lookup(m, table, hash)
        })?;
        set(m, Reg::Edi, found);
        if found != 0 {
            if m.r32(found + 8) != len {
                return Err("NativeVmBootstrap: builtin hash collision unresolved".into());
            }
            let buf = m.r32(found + 4);
            set(m, Reg::Ecx, buf);
            set(m, Reg::Esi, src);
            let mut offset = 0;
            loop {
                let a = m.r8(buf + offset);
                let b = m.r8(src + offset);
                set(m, Reg::Eax, (get(m, Reg::Eax) & !255) | a as u32);
                set(m, Reg::Edx, (get(m, Reg::Edx) & !255) | a as u32);
                if a != b {
                    return Err("NativeVmBootstrap: unequal builtin canonical name".into());
                }
                if a == 0 {
                    break;
                }
                offset += 1;
                if offset % 2 == 0 {
                    set(m, Reg::Ecx, buf + offset);
                    set(m, Reg::Esi, src + offset);
                }
            }
            set(m, Reg::Ecx, 0);
            write(m, found + 12, m.r32(found + 12) + 1);
            set(m, Reg::Eax, buf);
            stack::add_sp(m, 16);
            pop(m, Reg::Ecx);
            return Ok(buf);
        }
        allocate_name(m, holder, src, len, hash, e)
    })
}
fn intern(
    m: &mut Space,
    holder: u32,
    src: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<u32, String> {
    stack::function(m, &[src], Some(holder), |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, src);
        set(m, Reg::Edx, holder);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Ecx, u32::MAX);
        set(m, Reg::Eax, 0);
        set(m, Reg::Edi, src);
        let mut len = 0;
        while m.r8(src + len) != 0 {
            len += 1;
            if len > 3 {
                return Err("NativeVmBootstrap: builtin descriptor length mismatch".into());
            }
        }
        set(m, Reg::Edi, src + len + 1);
        set(m, Reg::Ecx, len);
        let result = stack::call(m, 0x41e81d, &[src, len], Some(holder), |m| {
            intern_raw(m, holder, src, len, e)
        })?;
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        Ok(result)
    })
}
fn descriptor(
    m: &mut Space,
    cell: u32,
    jvm: u32,
    src: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<(), String> {
    stack::function(m, &[jvm, src], Some(cell), |m| {
        set(m, Reg::Eax, src);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, cell);
        set(m, Reg::Ecx, jvm);
        let holder = m.r32(jvm + 0x68);
        set(m, Reg::Ecx, holder);
        let value = stack::call(m, 0x423e24, &[src], Some(holder), |m| {
            intern(m, holder, src, e)
        })?;
        write(m, cell, value);
        pop(m, Reg::Esi);
        Ok(())
    })
}
/// Live40fe70 source frame stopped before package creation at40ff4e.
/// Caller ownership transfers explicitly to the later namespace continuation.
#[must_use]
pub struct DescriptorPrefix {
    pub caller: stack::Frame,
    pub jvm: u32,
    pub pc: u32,
}
#[must_use]
pub struct NamespacePrefix {
    pub caller: stack::Frame,
    pub jvm: u32,
    pub root: u32,
    pub pc: u32,
}
pub fn prepare_namespace_root(
    m: &mut Space,
    prefix: DescriptorPrefix,
    environment: &mut impl crate::native_vm_namespace::Environment,
) -> Result<NamespacePrefix, String> {
    if prefix.pc != 0x40ff4e || !stack::active(m) || get(m, Reg::Esi) != prefix.jvm {
        return Err("NativeVmBootstrap: missing retained descriptor caller".into());
    }
    stack::push(m, 16);
    write(m, 0x62dee8, 0);
    write(m, 0x62dffc, 1);
    write(m, 0x62e008, 0);
    let root = allocate(m, 0x40ff6b, 16, environment)?;
    stack::add_sp(m, 4);
    stack::call(m, 0x40ff80, &[0, 0, 0x60ab64], Some(root), |m| {
        crate::native_vm_namespace::construct_root(m, root, environment)
    })?;
    Ok(NamespacePrefix {
        caller: prefix.caller,
        jvm: prefix.jvm,
        root,
        pc: 0x40ff84,
    })
}
/// Live source40fe70 frame at the actual pre-CALL41001d class-load boundary.
/// Loading/returning and the bootstrap suffix remain unresolved.
#[must_use]
pub struct PendingClassLoad {
    pub caller: stack::Frame,
    pub jvm: u32,
    pub name: u32,
    pub pc: u32,
}
pub fn prepare_class_load(
    m: &mut Space,
    prefix: NamespacePrefix,
    environment: &mut impl crate::native_vm_namespace::Environment,
) -> Result<PendingClassLoad, String> {
    if prefix.pc != 0x40ff84 || !stack::active(m) || get(m, Reg::Esi) != prefix.jvm {
        return Err("NativeVmBootstrap: missing retained namespace caller".into());
    }
    for (index, (return_pc, path, name)) in [
        (0x40ff98, 0x60ab78, 0x60ab6c),
        (0x40ffaa, 0x60ab94, 0x60ab8c),
        (0x40ffbc, 0x60abb4, 0x60aba8),
        (0x40ffce, 0x60abd4, 0x60abc8),
        (0x40ffe0, 0x60abf8, 0x60abec),
        (0x40fff2, 0x60ac1c, 0x60ac10),
        (0x410004, 0x60ac3c, 0x60ac2c),
        (0x410016, 0x60ac68, 0x60ac4c),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 0 {
            write(m, prefix.jvm + 0x7c, prefix.root);
        }
        stack::call(m, return_pc, &[path, name], Some(prefix.root), |m| {
            crate::native_vm_namespace::map(m, prefix.root, path, name, environment)
        })?;
    }
    stack::push(m, 0x60ac84);
    set(m, Reg::Ecx, prefix.jvm);
    Ok(PendingClassLoad {
        caller: prefix.caller,
        jvm: prefix.jvm,
        name: 0x60ac84,
        pc: 0x41001d,
    })
}
pub fn prepare_second_class(
    m: &mut Space,
    pending: &mut crate::native_vm_namespace::PendingClassResolution,
) -> Result<PendingClassLoad, String> {
    if pending.pc != 0x410022
        || !stack::active(m)
        || stack::has_pending_call(m)
        || get(m, Reg::Esi) != pending.jvm
    {
        return Err("NativeVmBootstrap: returned first class caller required".into());
    }
    stack::push(m, 0x60ac98);
    set(m, Reg::Ecx, pending.jvm);
    pending.pc = 0x410029;
    Ok(PendingClassLoad {
        caller: pending
            .bootstrap
            .ok_or("NativeVmBootstrap: missing bootstrap owner")?,
        jvm: pending.jvm,
        name: 0x60ac98,
        pc: 0x410029,
    })
}
pub fn finish_bootstrap(
    m: &mut Space,
    pending: &mut crate::native_vm_namespace::PendingClassResolution,
) -> Result<(), String> {
    if pending.pc != 0x41002e
        || !stack::active(m)
        || stack::has_pending_call(m)
        || get(m, Reg::Esi) != pending.jvm
    {
        return Err("NativeVmBootstrap: returned second class caller required".into());
    }
    pop(m, Reg::Edi);
    pop(m, Reg::Esi);
    pop(m, Reg::Ebx);
    let return_pc = stack::load(m, 0);
    stack::leave(
        m,
        pending
            .bootstrap
            .ok_or("NativeVmBootstrap: missing bootstrap owner")?,
    );
    pending.pc = return_pc;
    Ok(())
}
pub fn prepare_descriptors(
    m: &mut Space,
    jvm: u32,
    e: &mut (impl Allocator + ?Sized),
) -> Result<DescriptorPrefix, String> {
    if jvm == 0 || m.r32(jvm + 0x68) == 0 || m.r32(m.r32(jvm + 0x68) + 4) == 0 {
        return Err("NativeVmBootstrap: missing actual JVM canonical-name holder".into());
    }
    for (p, text) in [
        (0x60ab50, b"I\0".as_slice()),
        (0x60ab54, b"I\0".as_slice()),
        (0x60ab58, b"F\0".as_slice()),
        (0x60ab5c, b"L;\0".as_slice()),
        (0x60ab60, b"V\0".as_slice()),
    ] {
        if text
            .iter()
            .enumerate()
            .any(|(i, b)| m.r8(p + i as u32) != *b)
        {
            return Err("NativeVmBootstrap: missing source builtin descriptor bytes".into());
        }
    }
    let caller = stack::enter(m, &[], Some(jvm));
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Esi);
    stack::push_reg(m, Reg::Edi);
    stack::push(m, 4);
    set(m, Reg::Esi, jvm);
    for (index, (pc, ret, src, global)) in [
        (0x40fe7c, 0x40fe94, 0x60ab50, 0x62def0),
        (0x40fea9, 0x40febf, 0x60ab54, 0x62e00c),
        (0x40fed4, 0x40feea, 0x60ab58, 0x62deec),
        (0x40feff, 0x40ff15, 0x60ab5c, 0x62e000),
        (0x40ff2a, 0x40ff40, 0x60ab60, 0x62e010),
    ]
    .into_iter()
    .enumerate()
    {
        if index != 0 {
            stack::push(m, 4)
        }
        let cell = allocate(m, pc, 4, e)?;
        set(m, Reg::Edi, cell);
        if index == 0 {
            set(m, Reg::Ebx, 0)
        }
        stack::add_sp(m, 4);
        stack::call(m, ret, &[jvm, src], Some(cell), |m| {
            descriptor(m, cell, jvm, src, e)
        })?;
        write(m, global, cell);
    }
    Ok(DescriptorPrefix {
        caller,
        jvm,
        pc: 0x40ff4e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value as Json};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
    fn observe(p: u32, n: u32, v: u32) {
        if p < 0x7fe00000 {
            STORES.with(|s| s.borrow_mut().push((p, n, v)));
        }
    }
    fn word(v: &Json) -> u32 {
        u32::try_from(v.as_u64().expect("u32")).unwrap()
    }
    fn digest(p: &std::path::Path) -> String {
        format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
    }
    fn decode(blobs: &[&Json]) -> Vec<Vec<u8>> {
        let script="import base64,hashlib,json,struct,sys,zlib\nfor b in json.load(sys.stdin):\n raw=base64.b64decode(b['zlib_base64'],validate=True); z=zlib.decompressobj(); d=z.decompress(raw,2097153); assert len(d)<=2097152 and not z.unconsumed_tail; d+=z.flush(); assert z.eof and not z.unused_data and len(d)==b['bytes'] and hashlib.sha256(d).hexdigest()==b['sha256']; sys.stdout.buffer.write(struct.pack('<I',len(d))+d)";
        let mut child = std::process::Command::new("python3")
            .args(["-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(blobs).unwrap())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut cursor = 0;
        let result = blobs
            .iter()
            .map(|b| {
                assert!(out.stdout.len() >= cursor + 4);
                let n =
                    u32::from_le_bytes(out.stdout[cursor..cursor + 4].try_into().unwrap()) as usize;
                cursor += 4;
                assert_eq!(n, word(&b["bytes"]) as usize);
                assert!(out.stdout.len() >= cursor + n);
                let data = out.stdout[cursor..cursor + n].to_vec();
                cursor += n;
                assert_eq!(json!(format!("{:x}", Sha256::digest(&data))), b["sha256"]);
                data
            })
            .collect();
        assert_eq!(cursor, out.stdout.len());
        result
    }
    struct State {
        heap: Vec<u8>,
        stack: Vec<u8>,
        image: Vec<(u32, Vec<u8>)>,
        heap_ptr: u32,
        registers: [u32; 8],
    }
    fn states(values: &[&Json]) -> Vec<State> {
        let mut blobs = vec![];
        for v in values {
            blobs.push(&v["heap"]);
            blobs.push(&v["stack"]);
            for item in v["image"].as_array().unwrap() {
                blobs.push(&item["data"]);
            }
        }
        let mut bytes = decode(&blobs).into_iter();
        values
            .iter()
            .map(|v| {
                assert_eq!(v["fpu_cw"], 127);
                assert_eq!(v["fpu_depth"], 0);
                let keys = v["registers"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                assert_eq!(
                    keys,
                    ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"]
                        .into_iter()
                        .collect()
                );
                let heap = bytes.next().unwrap();
                let stack = bytes.next().unwrap();
                assert!(matches!(heap.len(), 0x80000 | 0x100000 | 0x200000));
                assert_eq!(stack.len(), 0x3000);
                let images = v["image"].as_array().unwrap();
                assert!(matches!(images.len(), 3..=7));
                let mut ranges = if images.len() >= 4 {
                    vec![
                        (0x62d000, 0x3000),
                        (0x766000, 0x3000),
                        (0x777000, 0x8000),
                        (0x61b000, 0x1000),
                    ]
                } else {
                    vec![(0x62d000, 0x3000), (0x766000, 0x3000), (0x777000, 0x1000)]
                };
                if images.len() >= 5 {
                    ranges.push((0x636000, 0x1000));
                }
                if images.len() >= 6 {
                    ranges.push((0x618000, 0x1000));
                }
                if images.len() == 7 {
                    ranges.push((0x640000, 0x1000));
                }
                let image = images
                    .iter()
                    .zip(ranges)
                    .map(|(b, (p, n))| {
                        assert_eq!(word(&b["base"]), p);
                        let data = bytes.next().unwrap();
                        assert_eq!(data.len(), n);
                        (p, data)
                    })
                    .collect();
                State {
                    heap,
                    stack,
                    image,
                    heap_ptr: word(&v["heap_ptr"]),
                    registers: ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"]
                        .map(|r| word(&v["registers"][r])),
                }
            })
            .collect()
    }
    fn compare(m: &Space, v: &State, label: &str) {
        let mut regions = vec![(0x20000000, &v.heap), (0x7fefd000, &v.stack)];
        regions.extend(v.image.iter().map(|(p, b)| (*p, b)));
        for (base, bytes) in regions {
            let difference = m.first_difference(base, bytes);
            assert!(
                difference.is_none(),
                "{label} memory {base:x}: {difference:x?}"
            );
        }
        assert_eq!(m.heap_ptr(), v.heap_ptr, "{label} heap cursor");
        for (index, r) in [
            Reg::Eax,
            Reg::Ebx,
            Reg::Ecx,
            Reg::Edx,
            Reg::Esi,
            Reg::Edi,
            Reg::Ebp,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(get(m, r), v.registers[index], "{label} register{index}")
        }
        assert_eq!(stack::sp(m), v.registers[7], "{label} esp");
    }
    struct Env<'a> {
        events: &'a [Json],
        states: &'a [State],
        index: usize,
    }
    impl Allocator for Env<'_> {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            let v = &self.events[self.index];
            assert_eq!(v["kind"], "allocate");
            assert_eq!(v["args"], json!([n]));
            compare(
                m,
                &self.states[self.index],
                &format!("allocate{}", self.index),
            );
            self.index += 1;
            Ok(m.alloc(n))
        }
        fn reallocate(
            &mut self,
            m: &mut Space,
            old: u32,
            old_n: u32,
            n: u32,
        ) -> Result<u32, String> {
            let v = &self.events[self.index];
            assert_eq!(v["kind"], "reallocate");
            assert_eq!(v["args"], json!([old, n]));
            compare(
                m,
                &self.states[self.index],
                &format!("reallocate{}", self.index),
            );
            self.index += 1;
            let p = m.alloc(n);
            for i in 0..old_n.min(n) {
                m.w8(p + i, m.r8(old + i));
            }
            Ok(p)
        }
    }
    impl crate::native_vm_namespace::Environment for Env<'_> {
        fn free(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
            panic!("unexpected free before root endpoint")
        }
        fn find_first(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
            panic!("unexpected filesystem before root endpoint")
        }
        fn find_next(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
            panic!("unexpected filesystem before root endpoint")
        }
        fn find_close(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
            panic!("unexpected filesystem before root endpoint")
        }
    }
    #[test]
    fn original_builtin_descriptors_retain_caller_and_match_every_leaf_memory_register_and_store() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) = crate::oracle_fixture::file(
            &root.join("re/out/boot/native-vm-bootstrap-original.json"),
        ) else {
            return;
        };
        let v: Json = serde_json::from_slice(
            &std::fs::read(fixture_path).expect("run native_vm_bootstrap_probe.py"),
        )
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["cw"], 127);
        assert_eq!(v["unicorn"], "2.1.4");
        assert_eq!(
            v["exe_sha256"],
            digest(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
        );
        let sources = v["producer_sources"].as_object().unwrap();
        assert_eq!(sources.len(), 2);
        for name in ["native_vm_bootstrap_probe.py", "oracle.py"] {
            assert_eq!(sources[name], digest(&root.join("re/oracle").join(name)));
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(
                row["input"],
                json!({"fill":([0,0xa5][index/4]),"seed":([606,20261023][index/2%2]),"gap":([0,48][index%2])})
            );
            assert_eq!(row["endpoint"], 0x41001d);
            let phases = row["phases"].as_array().unwrap();
            assert_eq!(phases.len(), 3);
            assert_eq!(
                phases.iter().map(|p| word(&p["pc"])).collect::<Vec<_>>(),
                vec![0x40ff4e, 0x40ff84, 0x41001d]
            );
            let count = word(&phases[1]["event_count"]) as usize;
            assert_eq!(count, 18);
            let events = &row["events"].as_array().unwrap()[..count];
            assert!(events[..15].iter().all(|e| e["phase"] == "descriptors"));
            assert!(events[15..].iter().all(|e| e["phase"] == "namespace"));
            let mut values = vec![&row["before"]];
            values.extend(events.iter().map(|e| &e["state"]));
            values.push(&phases[0]["state"]);
            values.push(&phases[1]["state"]);
            let snapshots = states(&values);
            let mut m = Space::new();
            let input = row["input_pages"].as_array().unwrap();
            assert!(!input.is_empty());
            let blobs = input.iter().map(|p| &p["data"]).collect::<Vec<_>>();
            let pages = decode(&blobs);
            let mut seen = std::collections::HashSet::new();
            for (p, b) in input.iter().zip(pages) {
                let base = word(&p["base"]);
                assert!(
                    (0x400000..0x7c0000).contains(&base)
                        && base.is_multiple_of(4096)
                        && seen.insert(base)
                );
                assert_eq!(b.len(), 4096);
                for (i, b) in b.into_iter().enumerate() {
                    m.w8(base + i as u32, b)
                }
            }
            let before = &snapshots[0];
            for (base, bytes) in [(0x20000000, &before.heap), (0x7fefd000, &before.stack)]
                .into_iter()
                .chain(before.image.iter().map(|(p, b)| (*p, b)))
            {
                for (i, b) in bytes.iter().enumerate() {
                    m.w8(base + i as u32, *b)
                }
            }
            m.set_heap_ptr(before.heap_ptr);
            let inputs = row["register_inputs"].as_object().unwrap();
            assert_eq!(inputs.len(), 7);
            for (r, n) in [
                (Reg::Eax, "eax"),
                (Reg::Ebx, "ebx"),
                (Reg::Ecx, "ecx"),
                (Reg::Edx, "edx"),
                (Reg::Esi, "esi"),
                (Reg::Edi, "edi"),
                (Reg::Ebp, "ebp"),
            ] {
                set(&mut m, r, word(&inputs[n]))
            }
            stack::set_sp(&mut m, 0x7fef_feec);
            m.w32(0x7fef_feec, 0x7fff0000);
            m.w32(0x7ffd_f004, 1);
            STORES.with(|s| s.borrow_mut().clear());
            set_test_store_observer(Some(observe));
            crate::native_vm_test_store::set(Some(observe));
            crate::native_vm_namespace::set_test_store_observer(Some(observe));
            crate::native_vm_class_context::set_test_store_observer(Some(observe));
            crate::native_vm_signature::set_test_store_observer(Some(observe));
            crate::native_vm_jvm::set_test_store_observer(Some(observe));
            crate::native_vm_lookup::set_test_store_observer(Some(observe));
            crate::native_vm_metadata::registration::set_test_store_observer(Some(observe));
            let mut env = Env {
                events,
                states: &snapshots[1..1 + count],
                index: 0,
            };
            let prefix = prepare_descriptors(&mut m, word(&row["this"]), &mut env).unwrap();
            assert_eq!(prefix.jvm, word(&row["this"]));
            assert_eq!(prefix.pc, 0x40ff4e);
            assert!(stack::active(&m), "live source frame must remain entered");
            assert_eq!(env.index, 15);
            compare(
                &m,
                &snapshots[snapshots.len() - 2],
                &format!("descriptor endpoint row{index}"),
            );
            let prefix = prepare_namespace_root(&mut m, prefix, &mut env).unwrap();
            set_test_store_observer(None);
            crate::native_vm_test_store::set(None);
            crate::native_vm_namespace::set_test_store_observer(None);
            crate::native_vm_class_context::set_test_store_observer(None);
            crate::native_vm_signature::set_test_store_observer(None);
            crate::native_vm_jvm::set_test_store_observer(None);
            crate::native_vm_lookup::set_test_store_observer(None);
            crate::native_vm_metadata::registration::set_test_store_observer(None);
            assert_eq!(prefix.pc, 0x40ff84);
            assert_eq!(prefix.jvm, word(&row["this"]));
            assert!(stack::active(&m));
            assert_eq!(env.index, count);
            compare(
                &m,
                snapshots.last().unwrap(),
                &format!("namespace root endpoint row{index}"),
            );
            let store_count = word(&phases[1]["store_count"]) as usize;
            assert!(store_count > 0);
            let expected = row["stores"].as_array().unwrap()[..store_count]
                .iter()
                .map(|s| (word(&s[0]), word(&s[1]), word(&s[2])))
                .collect::<Vec<_>>();
            let actual = STORES.with(|s| s.borrow().clone());
            assert_eq!(
                actual.len(),
                expected.len(),
                "ordered store count row{index}"
            );
            let difference = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a != b);
            assert!(
                difference.is_none(),
                "ordered stores row{index}: {difference:x?}"
            );
        }
    }
    struct FullEnv<'a> {
        base: Env<'a>,
        enumerations: &'a [Json],
        next_enumeration: usize,
        platform: slr_boot::native_vm_platform::Files,
        logging: u32,
    }
    fn cstring(m: &Space, p: u32) -> String {
        let bytes = (0..256)
            .map(|i| m.r8(p + i))
            .take_while(|b| *b != 0)
            .collect::<Vec<_>>();
        assert!(bytes.len() < 256);
        String::from_utf8(bytes).unwrap()
    }
    impl FullEnv<'_> {
        fn check(&mut self, m: &Space, kind: &str, args: Json) {
            let entry = &self.base.events[self.base.index];
            assert_eq!(entry["kind"], kind, "leaf{}", self.base.index);
            assert_eq!(entry["args"], args, "leaf{}", self.base.index);
            compare(
                m,
                &self.base.states[self.base.index],
                &format!("{kind}{}", self.base.index),
            );
            self.base.index += 1;
        }
    }
    impl Allocator for FullEnv<'_> {
        fn deallocate(&mut self, m: &mut Space, pointer: u32) -> Result<(), String> {
            self.check(m, "free", json!([pointer]));
            Ok(())
        }
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            self.base.allocate(m, n)
        }
        fn reallocate(&mut self, m: &mut Space, p: u32, old_n: u32, n: u32) -> Result<u32, String> {
            self.base.reallocate(m, p, old_n, n)
        }
    }
    impl crate::native_vm_context::Environment for FullEnv<'_> {}
    impl crate::native_vm_thread_run::Environment for FullEnv<'_> {
        fn query_frequency(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
            self.check(m, "frequency", json!([destination]));
            m.w32(destination, 1_000_000);
            m.w32(destination + 4, 0);
            Ok(1)
        }
        fn query_counter(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
            self.check(m, "counter", json!([destination]));
            m.w32(destination, 1_000_000);
            m.w32(destination + 4, 0);
            Ok(1)
        }
    }
    impl crate::native_vm_dispatch::Environment for FullEnv<'_> {
        fn free(&mut self, m: &mut Space, pointer: u32) -> Result<(), String> {
            self.check(m, "free", json!([pointer]));
            Ok(())
        }
    }
    impl crate::native_vm_jvm::Environment for FullEnv<'_> {
        fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
            self.base.allocate(m, bytes)
        }
        fn logger(&mut self, m: &mut Space, logger: u32, message: u32) -> Result<u32, String> {
            assert_eq!(stack::reg(m, Reg::Ecx), logger);
            self.check(m, "logger", json!([message]));
            self.logging += 1;
            Ok(0xaabb0000 + self.logging)
        }
    }
    impl crate::native_vm_namespace::Environment for FullEnv<'_> {
        fn free(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
            self.check(m, "free", json!([p]));
            Ok(0)
        }
        fn find_first(
            &mut self,
            m: &mut Space,
            pattern_address: u32,
            dest: u32,
        ) -> Result<u32, String> {
            self.check(m, "FindFirstFileA", json!([pattern_address, dest]));
            let pattern = cstring(m, pattern_address).replace('\\', "/");
            let entry = &self.enumerations[self.next_enumeration];
            assert_eq!(entry["pattern"], pattern);
            assert_eq!(
                json!(self.platform.entries(&pattern)?),
                entry["entries"],
                "live stock directory source differs"
            );
            self.next_enumeration += 1;
            self.platform.find_first(m, pattern_address, dest)
        }
        fn find_next(&mut self, m: &mut Space, handle: u32, dest: u32) -> Result<u32, String> {
            self.check(m, "FindNextFileA", json!([handle, dest]));
            self.platform.find_next(m, handle, dest)
        }
        fn find_close(&mut self, m: &mut Space, handle: u32) -> Result<u32, String> {
            self.check(m, "FindClose", json!([handle]));
            self.platform.find_close(handle)
        }
    }
    impl crate::native_vm_namespace::FileEnvironment for FullEnv<'_> {
        fn file_leaf(
            &mut self,
            m: &mut Space,
            leaf: crate::native_vm_namespace::FileLeaf,
            _target: u32,
            args: &[u32],
        ) -> Result<u32, String> {
            use crate::native_vm_namespace::FileLeaf;
            let name = match leaf {
                FileLeaf::Create => "CreateFileA",
                FileLeaf::Close => "CloseHandle",
                FileLeaf::Size => "GetFileSize",
                FileLeaf::Read => "ReadFile",
                FileLeaf::Wait => "WaitForSingleObject",
                FileLeaf::Release => "ReleaseSemaphore",
            };
            self.check(m, name, json!(args));
            self.platform.file_leaf(m, leaf, args)
        }
    }
    #[test]
    fn complete_prefix_reaches_actual_first_class_load_with_retained_caller_and_exact_all_leaf_states(
    ) {
        bootstrap_prefix(0);
    }
    #[test]
    fn class_resolution_retains_bootstrap_caller_through_cache_preparation() {
        bootstrap_prefix(1);
    }
    #[test]
    fn cache_header_validation_retains_live_class_load_caller() {
        bootstrap_prefix(2);
    }
    #[test]
    fn full_class_file_input_retains_source_loader_call() {
        bootstrap_prefix(3);
    }
    #[test]
    fn source_module_construction_retains_deserializer_call() {
        bootstrap_prefix(4);
    }
    #[test]
    fn source_constants_deserialize_under_retained_loader_call() {
        bootstrap_prefix(5);
    }
    #[test]
    fn source_fields_and_methods_deserialize_under_retained_loader_call() {
        bootstrap_prefix(6);
    }
    #[test]
    fn complete_source_module_reaches_original_loader_epilogue() {
        bootstrap_prefix(7);
    }
    #[test]
    fn source_class_load_returns_into_retained_bootstrap_caller() {
        bootstrap_prefix(8);
    }
    #[test]
    fn complete_source_bootstrap_loads_both_classes_and_returns() {
        bootstrap_prefix(9);
    }
    #[test]
    fn source_runtime_constructs_and_boots_without_prebuilt_vm_metadata() {
        bootstrap_prefix(10);
    }
    #[test]
    fn source_runtime_loads_callback_classes_from_live_calls() {
        bootstrap_prefix(11);
    }
    #[test]
    fn source_runtime_registers_natives_with_original_caller_context() {
        bootstrap_prefix(12);
    }
    #[test]
    fn source_runtime_executes_original_native_catalog_caller() {
        bootstrap_prefix(13);
    }
    #[test]
    fn source_runtime_initializes_class_and_autoloads_its_dependency() {
        bootstrap_prefix(14);
    }
    #[test]
    fn source_runtime_constructs_object_with_issued_field_and_frame_owners() {
        bootstrap_prefix(15);
    }
    #[test]
    fn source_runtime_loads_all_modules_before_returning_first_class() {
        bootstrap_prefix(16);
    }
    #[test]
    fn source_runtime_executes_cold_scripted_static_initializers() {
        bootstrap_prefix(17);
    }
    #[test]
    fn source_runtime_constructs_input_after_scripted_static_initializers() {
        bootstrap_prefix(18);
    }
    static SOURCE_PROFILE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn bootstrap_prefix(mode: u8) {
        let resolve = mode != 0;
        let cache = mode >= 2;
        let file_input = mode >= 3;
        let module_input = mode >= 4;
        let constants_input = mode >= 5;
        let members_input = mode >= 6;
        let tree_input = mode >= 7;
        let returned_input = mode >= 8;
        let completed_input = mode >= 9;
        let cold_input = mode >= 10;
        let lookup_input = matches!(mode, 11 | 16);
        let multiple_input = mode == 16;
        let register_input = mode == 12;
        let catalog_input = mode == 13;
        let autoload_input = matches!(mode, 14 | 15 | 17 | 18);
        let scripted_input = matches!(mode, 17 | 18);
        let object_input = matches!(mode, 15 | 18);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let producer = if mode == 18 {
            "native_vm_runtime_input_object_probe.py"
        } else if scripted_input {
            "native_vm_runtime_input_probe.py"
        } else if multiple_input {
            "native_vm_runtime_controller_probe.py"
        } else if object_input {
            "native_vm_runtime_object_probe.py"
        } else if autoload_input {
            "native_vm_runtime_autoload_probe.py"
        } else if catalog_input {
            "native_vm_runtime_catalog_probe.py"
        } else if register_input {
            "native_vm_runtime_register_probe.py"
        } else if lookup_input {
            "native_vm_runtime_classes_probe.py"
        } else if cold_input {
            "native_vm_runtime_boot_probe.py"
        } else if completed_input {
            "native_vm_bootstrap_complete_probe.py"
        } else if returned_input {
            "native_vm_class_return_probe.py"
        } else if tree_input {
            "native_vm_class_tree_probe.py"
        } else if members_input {
            "native_vm_class_members_probe.py"
        } else if constants_input {
            "native_vm_class_constants_probe.py"
        } else if module_input {
            "native_vm_class_module_probe.py"
        } else if file_input {
            "native_vm_class_file_probe.py"
        } else if cache {
            "native_vm_class_cache_probe.py"
        } else if resolve {
            "native_vm_class_load_probe.py"
        } else {
            "native_vm_bootstrap_probe.py"
        };
        let fixture = if mode == 18 {
            "native-vm-runtime-input-object-original.json"
        } else if scripted_input {
            "native-vm-runtime-input-original.json"
        } else if multiple_input {
            "native-vm-runtime-controller-original.json"
        } else if object_input {
            "native-vm-runtime-object-original.json"
        } else if autoload_input {
            "native-vm-runtime-autoload-original.json"
        } else if catalog_input {
            "native-vm-runtime-catalog-original.json"
        } else if register_input {
            "native-vm-runtime-register-original.json"
        } else if lookup_input {
            "native-vm-runtime-classes-original.json"
        } else if cold_input {
            "native-vm-runtime-boot-original.json"
        } else if completed_input {
            "native-vm-bootstrap-complete-original.json"
        } else if returned_input {
            "native-vm-class-return-original.json"
        } else if tree_input {
            "native-vm-class-tree-original.json"
        } else if members_input {
            "native-vm-class-members-original.json"
        } else if constants_input {
            "native-vm-class-constants-original.json"
        } else if module_input {
            "native-vm-class-module-original.json"
        } else if file_input {
            "native-vm-class-file-original.json"
        } else if cache {
            "native-vm-class-cache-original.json"
        } else if resolve {
            "native-vm-class-load-original.json"
        } else {
            "native-vm-bootstrap-original.json"
        };
        let Some(data) = crate::oracle_fixture::file(&root.join("re/out/boot").join(fixture))
        else {
            return;
        };
        // Composed fixture states are large. Serialize these profiles so
        // parallel test execution cannot multiply their decoded heap storage.
        let _profile = SOURCE_PROFILE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let v: Json = serde_json::from_slice(&std::fs::read(data).unwrap()).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["cw"], 127);
        assert_eq!(v["unicorn"], "2.1.4");
        assert_eq!(
            v["exe_sha256"],
            digest(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
        );
        let sources = v["producer_sources"].as_object().unwrap();
        assert_eq!(
            sources.len(),
            if lookup_input || catalog_input {
                5
            } else if cold_input {
                4
            } else if completed_input {
                10
            } else if returned_input {
                9
            } else if tree_input {
                8
            } else if members_input {
                7
            } else if constants_input {
                6
            } else if module_input {
                5
            } else if file_input {
                4
            } else if cache {
                3
            } else {
                2
            }
        );
        let producers = if catalog_input {
            vec![
                producer,
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
                "native_vm_registry_catalog.py",
            ]
        } else if lookup_input {
            vec![
                producer,
                "native_vm_runtime_boot_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if cold_input {
            vec![
                producer,
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if completed_input {
            vec![
                producer,
                "native_vm_class_return_probe.py",
                "native_vm_class_tree_probe.py",
                "native_vm_class_members_probe.py",
                "native_vm_class_constants_probe.py",
                "native_vm_class_module_probe.py",
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if returned_input {
            vec![
                producer,
                "native_vm_class_tree_probe.py",
                "native_vm_class_members_probe.py",
                "native_vm_class_constants_probe.py",
                "native_vm_class_module_probe.py",
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if tree_input {
            vec![
                producer,
                "native_vm_class_members_probe.py",
                "native_vm_class_constants_probe.py",
                "native_vm_class_module_probe.py",
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if members_input {
            vec![
                producer,
                "native_vm_class_constants_probe.py",
                "native_vm_class_module_probe.py",
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if constants_input {
            vec![
                producer,
                "native_vm_class_module_probe.py",
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if module_input {
            vec![
                producer,
                "native_vm_class_file_probe.py",
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if file_input {
            vec![
                producer,
                "native_vm_class_cache_probe.py",
                "native_vm_class_load_probe.py",
                "oracle.py",
            ]
        } else if cache {
            vec![producer, "native_vm_class_load_probe.py", "oracle.py"]
        } else {
            vec![producer, "oracle.py"]
        };
        for name in producers {
            assert_eq!(sources[name], digest(&root.join("re/oracle").join(name)));
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(
                row["input"],
                json!({"fill":([0,0xa5][index/4]),"seed":([606,20261023][index/2%2]),"gap":([0,48][index%2])})
            );
            assert_eq!(
                row["endpoint"],
                if catalog_input {
                    0x489e20
                } else if completed_input {
                    0x7fff0000
                } else if returned_input {
                    0x410022
                } else if tree_input {
                    0x411d03
                } else if members_input {
                    0x411bdc
                } else if constants_input {
                    0x411f03
                } else if module_input {
                    0x407660
                } else if file_input {
                    0x411b20
                } else if cache {
                    0x410451
                } else if resolve {
                    0x41044c
                } else {
                    0x41001d
                }
            );
            let events = row["events"].as_array().unwrap();
            if !resolve {
                assert_eq!(events.len(), 770);
            }
            let mut values = vec![&row["before"]];
            values.extend(events.iter().map(|e| &e["state"]));
            values.push(&row["after"]);
            let snapshots = states(&values);
            assert_eq!(
                snapshots[0].heap.len(),
                if object_input || scripted_input {
                    0x200000
                } else if lookup_input {
                    0x100000
                } else {
                    0x80000
                }
            );
            let mut m = Space::new();
            let input = row["input_pages"].as_array().unwrap();
            assert!(!input.is_empty());
            let blobs = input.iter().map(|p| &p["data"]).collect::<Vec<_>>();
            let pages = decode(&blobs);
            let mut seen = std::collections::HashSet::new();
            for (p, b) in input.iter().zip(pages) {
                let base = word(&p["base"]);
                assert!(
                    (0x400000..0x7c0000).contains(&base)
                        && base.is_multiple_of(4096)
                        && seen.insert(base)
                );
                assert_eq!(b.len(), 4096);
                for (i, b) in b.into_iter().enumerate() {
                    m.w8(base + i as u32, b)
                }
            }
            let before = &snapshots[0];
            for (base, bytes) in [(0x20000000, &before.heap), (0x7fefd000, &before.stack)]
                .into_iter()
                .chain(before.image.iter().map(|(p, b)| (*p, b)))
            {
                for (i, b) in bytes.iter().enumerate() {
                    m.w8(base + i as u32, *b)
                }
            }
            m.set_heap_ptr(before.heap_ptr);
            let inputs = row["register_inputs"].as_object().unwrap();
            assert_eq!(inputs.len(), 7);
            for (r, n) in [
                (Reg::Eax, "eax"),
                (Reg::Ebx, "ebx"),
                (Reg::Ecx, "ecx"),
                (Reg::Edx, "edx"),
                (Reg::Esi, "esi"),
                (Reg::Edi, "edi"),
                (Reg::Ebp, "ebp"),
            ] {
                set(&mut m, r, word(&inputs[n]))
            }
            stack::set_sp(&mut m, 0x7feffeec);
            m.w32(0x7feffeec, 0x7fff0000);
            m.w32(0x7ffdf004, 1);
            STORES.with(|s| s.borrow_mut().clear());
            set_test_store_observer(Some(observe));
            crate::native_vm_test_store::set(Some(observe));
            crate::native_vm_namespace::set_test_store_observer(Some(observe));
            crate::native_vm_class_context::set_test_store_observer(Some(observe));
            crate::native_vm_signature::set_test_store_observer(Some(observe));
            crate::native_vm_jvm::set_test_store_observer(Some(observe));
            crate::native_vm_lookup::set_test_store_observer(Some(observe));
            crate::native_vm_metadata::registration::set_test_store_observer(Some(observe));
            let mut env = FullEnv {
                base: Env {
                    events,
                    states: &snapshots[1..1 + events.len()],
                    index: 0,
                },
                enumerations: row["stock_enumerations"].as_array().unwrap(),
                next_enumeration: 0,
                platform: slr_boot::native_vm_platform::Files::new(root.join("stock"), 0x3000)
                    .unwrap(),
                logging: 0,
            };
            if cold_input {
                let parent = stack::enter_cdecl(&mut m, &[]);
                stack::begin_call(&mut m, 0x30000028, &[], Some(word(&row["this"])));
                let before = &row["before"]["fpu"];
                let registers = std::array::from_fn(|i| {
                    let mut data = [0; 10];
                    data[..8].copy_from_slice(
                        &before["registers"][i][0].as_u64().unwrap().to_le_bytes(),
                    );
                    data[8..]
                        .copy_from_slice(&(word(&before["registers"][i][1]) as u16).to_le_bytes());
                    slr_x87::X87::from_bytes(data)
                });
                let fpu = crate::native_vm_fpu::State::from_saved(
                    word(&before["control"]) as u16,
                    word(&before["status"]) as u16,
                    word(&before["tags"]) as u16,
                    registers,
                );
                let mut runtime = crate::native_vm_live::Runtime::construct(
                    &mut m,
                    word(&row["this"]),
                    fpu,
                    &mut env,
                )
                .unwrap();
                env.check(&m, "constructor-return", json!([]));
                stack::set_reg(&mut m, Reg::Ecx, word(&row["this"]));
                stack::begin_call(&mut m, 0x30000032, &[], Some(word(&row["this"])));
                runtime.bootstrap(&mut m, &mut env).unwrap();
                env.check(&m, "bootstrap-return", json!([]));
                assert_eq!(runtime.jvm(), word(&row["this"]));
                assert_eq!(runtime.files().len(), 2);
                if lookup_input {
                    let requests = row["requests"].as_array().unwrap();
                    assert_eq!(requests.len(), if multiple_input { 2 } else { 5 });
                    let mut results = Vec::new();
                    for (index, request) in requests.iter().enumerate() {
                        let pointer = word(&request["pointer"]);
                        for (i, byte) in request["name"]
                            .as_str()
                            .unwrap()
                            .bytes()
                            .chain([0])
                            .enumerate()
                        {
                            m.w8(pointer + i as u32, byte);
                        }
                        stack::begin_call(
                            &mut m,
                            word(&request["return_pc"]),
                            &[pointer],
                            Some(runtime.jvm()),
                        );
                        let class = runtime.class_lookup(&mut m, &mut env).unwrap();
                        assert_eq!(class, stack::reg(&m, Reg::Eax));
                        results.push(class);
                        env.check(&m, "class-return", json!([index]));
                    }
                    if multiple_input {
                        assert_eq!(results[0], results[1]);
                    } else {
                        assert_eq!(results[1], results[4]);
                    }
                    assert_eq!(
                        runtime.files().len(),
                        if multiple_input { 3 } else { 6 },
                        "cache hit must not issue another file"
                    );
                }
                if autoload_input {
                    let request = &row["requests"][0];
                    let pointer = word(&request["pointer"]);
                    for (i, byte) in request["name"]
                        .as_str()
                        .unwrap()
                        .bytes()
                        .chain([0])
                        .enumerate()
                    {
                        m.w8(pointer + i as u32, byte);
                    }
                    stack::begin_call(
                        &mut m,
                        word(&request["return_pc"]),
                        &[pointer],
                        Some(runtime.jvm()),
                    );
                    let class = runtime.class_lookup(&mut m, &mut env).unwrap();
                    env.check(&m, "class-return", json!([0]));
                    if object_input {
                        stack::push_reg(&mut m, Reg::Ebx);
                        stack::set_reg(&mut m, Reg::Ebx, class);
                    }
                    stack::set_reg(&mut m, Reg::Ecx, class);
                    stack::begin_call(
                        &mut m,
                        if object_input { 0x3000004b } else { 0x30000048 },
                        &[],
                        None,
                    );
                    if scripted_input {
                        runtime.initialize_class(&mut m, &mut env).unwrap();
                    } else {
                        runtime.initialize_empty_class(&mut m, &mut env).unwrap();
                    }
                    if object_input {
                        stack::set_reg(&mut m, Reg::Ecx, class);
                        stack::begin_call(&mut m, 0x30000052, &[], None);
                        let object = runtime.create_instance(&mut m, &mut env).unwrap();
                        assert_eq!(m.r32(object + 12), class);
                        let saved = stack::load(&m, 0);
                        stack::set_reg(&mut m, Reg::Ebx, saved);
                        stack::add_sp(&mut m, 4);
                    }
                    assert_ne!(m.r32(class + 0x1d0), 0);
                    assert_eq!(runtime.files().len(), if scripted_input { 5 } else { 4 });
                    if scripted_input {
                        let values = runtime.value_history();
                        assert!(values.len() > 100);
                        assert!(values.iter().all(|value| value.released));
                        assert!(
                            values.iter().all(|value| value.initial.is_some()),
                            "every successful source allocation must complete its constructor"
                        );
                        assert!(values
                            .iter()
                            .any(|value| value.freelist && value.generation > 100));
                        let frames = runtime.script_frame_history();
                        assert!(frames.len() > 100);
                        assert!(frames.iter().all(|frame| frame.released
                            && frame.receiver == 0
                            && frame.locals == 0));
                        assert_eq!(
                            frames
                                .iter()
                                .map(|frame| frame.generation)
                                .collect::<std::collections::HashSet<_>>()
                                .len(),
                            frames.len()
                        );
                        let own = frames
                            .iter()
                            .filter(|frame| frame.context == class)
                            .collect::<Vec<_>>();
                        assert!(own.len() > 100);
                        assert!(own.iter().all(|frame| frame.address == own[0].address));
                    }
                }
                if catalog_input {
                    stack::set_reg(&mut m, Reg::Eax, runtime.jvm());
                    write(&mut m, 0x63641c, runtime.jvm());
                    stack::begin_call(&mut m, 0x30000041, &[], None);
                    let registry = runtime.prepare_registry(&mut m, &mut env).unwrap();
                    assert_eq!(registry.pc, 0x489e20);
                    let _retained_caller = registry.caller;
                    assert!(stack::active(&m));
                    assert!(!stack::has_pending_call(&m));
                }
                if register_input {
                    let registrations = row["registrations"].as_array().unwrap();
                    assert_eq!(registrations.len(), 76);
                    for registration in registrations {
                        let args = registration["arguments"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(word)
                            .collect::<Vec<_>>();
                        for (source, bytes) in
                            args.iter().zip(registration["bytes"].as_array().unwrap())
                        {
                            for (i, byte) in bytes
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(word)
                                .chain([0])
                                .enumerate()
                            {
                                m.w8(source + i as u32, byte as u8);
                            }
                        }
                        stack::begin_call(
                            &mut m,
                            word(&registration["return_pc"]),
                            &args,
                            Some(runtime.jvm()),
                        );
                        assert_eq!(runtime.register(&mut m, &mut env).unwrap(), 1);
                    }
                }
                for file in runtime.files() {
                    assert!(!file.bytes().is_empty());
                    let raw = file.module();
                    assert_eq!(m.r32(raw.class + 4), raw.module);
                }
                // Bind only source-issued records, after the recorder has
                // independently drained its copy of the chronology.
                let mut vm = slr_vm::host::VmHost::new();
                vm.start_machine_recording().unwrap();
                let trace = vm.machine_trace();
                let reader = trace.reader().unwrap();
                let mut identities = Vec::new();
                for file in runtime.files() {
                    let classes = vm.loader.load_file(file.bytes()).unwrap();
                    assert_eq!(classes.len(), file.modules().len());
                    identities.extend(
                        classes
                            .iter()
                            .zip(file.modules())
                            .map(|(class, raw)| (class.0, *raw)),
                    );
                }
                let semantic_object = if autoload_input {
                    // Drive actual initialization/instance chronology. Only the
                    // class-only profile leaves object allocation unacknowledged.
                    Some(
                        vm.alloc_object(if scripted_input {
                            "java.io.Input"
                        } else {
                            "java.lang.Native"
                        })
                        .unwrap(),
                    )
                } else {
                    None
                };
                let entries = trace.pending(usize::MAX);
                trace.acknowledge(entries.last().unwrap().sequence).unwrap();
                assert!(trace.pending(usize::MAX).is_empty());
                assert_eq!(reader.pending(usize::MAX).len(), entries.len());
                let first_value = entries.iter().find_map(|entry| match &entry.event {
                    slr_vm::machine::Event::Pool(slr_vm::values::PoolEvent::Allocate {
                        cell,
                        ..
                    }) => Some(slr_vm::values::CellId {
                        pool: cell.pool,
                        slot: cell.slot,
                        generation: cell.generation,
                    }),
                    _ => None,
                });
                let first_script = entries.iter().find_map(|entry| match &entry.event {
                    slr_vm::machine::Event::Frame {
                        phase: slr_vm::machine::FramePhase::Enter,
                        frame,
                        ..
                    } if frame.context != 0 => Some((frame.identity, frame.context)),
                    _ => None,
                });
                for entry in entries {
                    if autoload_input
                        && !object_input
                        && matches!(
                            entry.event,
                            slr_vm::machine::Event::ObjectAllocate {
                                array_descriptor: None,
                                ..
                            }
                        )
                    {
                        m.begin_journal();
                        assert!(runtime.consume(&mut m, &reader, 1).is_err());
                        assert!(m.end_journal().is_empty());
                        assert_eq!(reader.pending(1)[0], entry);
                        break;
                    }
                    let mut wrong = entry.clone();
                    wrong.sequence += 1;
                    m.begin_journal();
                    assert!(runtime.apply(&mut m, &wrong).is_err());
                    assert!(m.end_journal().is_empty());
                    if let slr_vm::machine::Event::FileRead { bytes, .. } = &mut wrong.event {
                        wrong.sequence = entry.sequence;
                        bytes[0] ^= 1;
                        m.begin_journal();
                        assert!(runtime.apply(&mut m, &wrong).is_err());
                        assert!(m.end_journal().is_empty());
                    }
                    if autoload_input {
                        let mut wrong = entry.clone();
                        let altered = match &mut wrong.event {
                            slr_vm::machine::Event::ClassDependencies {
                                identity, base, ..
                            } => {
                                *base = Some(*identity);
                                true
                            }
                            slr_vm::machine::Event::ClassInstall {
                                identity,
                                interfaces,
                                ..
                            } => {
                                interfaces.push(*identity);
                                true
                            }
                            slr_vm::machine::Event::ObjectAllocate { class_context, .. } => {
                                *class_context ^= 1;
                                true
                            }
                            slr_vm::machine::Event::HeapConstruct { value, .. } => {
                                value.payload ^= 1;
                                true
                            }
                            slr_vm::machine::Event::Pool(slr_vm::values::PoolEvent::Allocate {
                                cell,
                                ..
                            }) => {
                                cell.generation ^= 1;
                                true
                            }
                            slr_vm::machine::Event::CellStore { after, .. }
                            | slr_vm::machine::Event::HeapStore { after, .. } => {
                                after.payload ^= 1;
                                true
                            }
                            slr_vm::machine::Event::CurrentFrame { frame, .. } => {
                                *frame = 0;
                                true
                            }
                            slr_vm::machine::Event::Pool(slr_vm::values::PoolEvent::Release {
                                cell,
                                ..
                            }) => {
                                cell.generation ^= 1;
                                true
                            }
                            slr_vm::machine::Event::Operand { slot, .. } => {
                                *slot = slr_vm::machine::Operand::Null;
                                true
                            }
                            slr_vm::machine::Event::Run { budget_bits, .. } => {
                                *budget_bits ^= 1;
                                true
                            }
                            slr_vm::machine::Event::Opcode { immediate, .. } => {
                                *immediate ^= 1;
                                true
                            }
                            slr_vm::machine::Event::ThreadOrigin { name, .. } => {
                                name.push('x');
                                true
                            }
                            slr_vm::machine::Event::ThreadRetire { flags, .. } => {
                                *flags ^= 0x10;
                                true
                            }
                            slr_vm::machine::Event::Frame { frame, .. } => {
                                frame.context = 1;
                                true
                            }
                            _ => false,
                        };
                        if altered {
                            m.begin_journal();
                            assert!(
                                runtime.apply(&mut m, &wrong).is_err(),
                                "accepted altered source event: {:?}",
                                wrong.event
                            );
                            assert!(m.end_journal().is_empty());
                        }
                    }
                    if scripted_input {
                        if let slr_vm::machine::Event::ObjectAllocate {
                            array_descriptor: Some(_),
                            ..
                        } = &entry.event
                        {
                            for change in 0..2 {
                                let mut wrong = entry.clone();
                                let slr_vm::machine::Event::ObjectAllocate {
                                    array_descriptor,
                                    array_length,
                                    ..
                                } = &mut wrong.event
                                else {
                                    unreachable!()
                                };
                                if change == 0 {
                                    array_descriptor.as_mut().unwrap().push('x');
                                } else {
                                    *array_length = array_length.map(|length| length + 1);
                                }
                                m.begin_journal();
                                assert!(runtime.apply(&mut m, &wrong).is_err());
                                assert!(m.end_journal().is_empty());
                            }
                        }
                        if let slr_vm::machine::Event::Frame { frame, .. } = &entry.event {
                            if frame.context != 0 {
                                for change in 0..3 {
                                    let mut wrong = entry.clone();
                                    let slr_vm::machine::Event::Frame { frame, caller, .. } =
                                        &mut wrong.event
                                    else {
                                        unreachable!()
                                    };
                                    match change {
                                        0 => frame.saved_pc.as_mut().unwrap().idx += 1,
                                        1 => frame.signature ^= 1,
                                        _ => *caller = None,
                                    }
                                    m.begin_journal();
                                    assert!(runtime.apply(&mut m, &wrong).is_err());
                                    assert!(m.end_journal().is_empty());
                                }
                            }
                        }
                    }
                    assert_eq!(reader.pending(1)[0].sequence, entry.sequence);
                    m.begin_journal();
                    assert_eq!(runtime.consume(&mut m, &reader, 1).unwrap(), 1);
                    assert!(m.end_journal().is_empty());
                }
                if scripted_input {
                    if object_input {
                        assert!(
                            reader.pending(usize::MAX).is_empty(),
                            "the source-issued Input object must consume the complete stream"
                        );
                    } else {
                        assert!(matches!(reader.pending(1)[0].event,
                            slr_vm::machine::Event::ObjectAllocate { .. }),
                            "all scripted initializer phases must bind before the unissued ordinary object allocation");
                    }
                    assert!(runtime
                        .script_frame_history()
                        .iter()
                        .all(|frame| frame.released));
                    assert!(runtime.owner_history().iter().all(|thread| thread.retired));
                    assert!(
                        runtime.value(&m, first_value.unwrap()).is_err(),
                        "historical/released source cells must never grant live values"
                    );
                    let (frame, context) = first_script.unwrap();
                    assert!(
                        runtime.frame(&m, frame).is_err(),
                        "historical/released source frames must never grant live context"
                    );
                    let variable = runtime.static_field(&m, context, 0).unwrap();
                    assert_eq!(m.r32(variable), 0x5e7344);
                    let descriptor = m.r32(variable + 4);
                    m.w32(variable + 4, descriptor ^ 1);
                    assert!(runtime.static_field(&m, context, 0).is_err());
                    m.w32(variable + 4, descriptor);
                } else if autoload_input {
                    let history = runtime.owner_history();
                    assert_eq!(history.len(), if object_input { 3 } else { 2 });
                    assert!(history.iter().all(|owner| owner.retired));
                    assert_eq!(
                        history[0].raw_frame, history[1].raw_frame,
                        "source frame free list must actually reuse its address"
                    );
                    assert_ne!(history[0].frame_generation, history[1].frame_generation);
                    assert_ne!(history[0].thread_generation, history[1].thread_generation);
                    assert_ne!(history[0].frame, history[1].frame);
                    if object_input {
                        assert!(reader.pending(usize::MAX).is_empty());
                    } else {
                        assert!(matches!(
                            reader.pending(1)[0].event,
                            slr_vm::machine::Event::ObjectAllocate { .. }
                        ));
                    }
                } else {
                    assert!(reader.pending(usize::MAX).is_empty());
                }
                if object_input {
                    let word = semantic_object.unwrap();
                    let raw_object = runtime.object(&m, word).unwrap();
                    assert_eq!(raw_object, stack::reg(&m, Reg::Eax));
                    assert!(
                        runtime.object(&m, raw_object).is_err(),
                        "raw addresses must not be semantic object identities"
                    );
                    let vector = m.r32(m.r32(raw_object + 16) + 12);
                    let fields = if scripted_input { 0 } else { 1 };
                    assert_eq!(m.r32(vector + 8), fields);
                    assert!(runtime.object_field(&m, word, fields as usize).is_err());
                    for index in 0..fields {
                        let variable = runtime.object_field(&m, word, index as usize).unwrap();
                        assert_eq!(variable, m.r32(m.r32(vector + 4) + index * 4));
                        assert_eq!(m.r32(variable + 8), 0);
                        let descriptor = m.r32(variable + 4);
                        m.w32(variable + 4, descriptor + 1);
                        assert!(runtime.object_field(&m, word, index as usize).is_err());
                        m.w32(variable + 4, descriptor);
                    }
                }
                for (identity, raw) in identities {
                    assert_eq!(runtime.metadata().class(identity.get()).unwrap(), raw);
                    let class = vm.loader.find_by_id(identity).unwrap();
                    let tree = m.r32(raw.module + 0x60);
                    for (offset, is_static) in [(0x54, true), (0x4c, false)] {
                        let container = m.r32(raw.module + offset);
                        for index in 0..m.r32(container + 12) {
                            let record = m.r32(m.r32(container + 8) + index * 4);
                            assert_eq!(
                                runtime
                                    .metadata()
                                    .field(&m, identity.get(), is_static, index as usize)
                                    .unwrap(),
                                record
                            );
                            let flags = m.r32(record + 4);
                            m.w32(record + 4, flags ^ 1);
                            assert!(runtime
                                .metadata()
                                .field(&m, identity.get(), is_static, index as usize)
                                .is_err());
                            m.w32(record + 4, flags);
                        }
                    }
                    for (index, nodes) in class.bodies.iter().enumerate() {
                        let body = vm.loader.body_id_of(identity, index).unwrap();
                        let address = m.r32(m.r32(tree + 4) + index as u32 * 4);
                        for idx in 0..nodes.len() {
                            assert_eq!(
                                runtime
                                    .metadata()
                                    .pc(&m, slr_vm::interp::Pc { body, idx })
                                    .unwrap(),
                                address + idx as u32 * 8
                            );
                        }
                    }
                    let mut selected = 0;
                    for offset in [0x58, 0x50] {
                        let container = m.r32(raw.module + offset);
                        for index in 0..m.r32(container + 12) {
                            assert_eq!(
                                runtime
                                    .metadata()
                                    .method(&m, identity.get(), selected)
                                    .unwrap(),
                                m.r32(m.r32(container + 8) + index * 4)
                            );
                            selected += 1;
                        }
                    }
                    assert_eq!(selected, class.methods.len());
                }
                let after = &row["after"]["fpu"];
                assert_eq!(runtime.fpu().control(), word(&after["control"]) as u16);
                assert_eq!(runtime.fpu().status(), word(&after["status"]) as u16);
                assert_eq!(runtime.fpu().tags(), word(&after["tags"]) as u16);
                for (i, register) in runtime.fpu().registers().iter().enumerate() {
                    let data = register.to_bytes();
                    assert_eq!(
                        u64::from_le_bytes(data[..8].try_into().unwrap()),
                        after["registers"][i][0].as_u64().unwrap()
                    );
                    assert_eq!(
                        u16::from_le_bytes(data[8..].try_into().unwrap()),
                        word(&after["registers"][i][1]) as u16
                    );
                }
                if !catalog_input {
                    stack::leave(&mut m, parent);
                    assert!(!stack::active(&m));
                }
            } else {
                let prefix = prepare_descriptors(&mut m, word(&row["this"]), &mut env).unwrap();
                let prefix = prepare_namespace_root(&mut m, prefix, &mut env).unwrap();
                let pending = prepare_class_load(&mut m, prefix, &mut env).unwrap();
                assert_eq!(pending.pc, 0x41001d);
                assert_eq!(pending.name, 0x60ac84);
                assert_eq!(pending.jvm, word(&row["this"]));
                assert!(stack::active(&m));
                assert_eq!(m.r32(stack::sp(&m)), pending.name);
                if resolve {
                    let mut resolved: crate::native_vm_namespace::PendingClassResolution =
                        crate::native_vm_namespace::begin_class_resolution(
                            &mut m, pending, &mut env,
                        )
                        .unwrap();
                    assert_eq!(resolved.pc, 0x41044c);
                    assert_eq!(resolved.jvm, word(&row["this"]));
                    assert_eq!(stack::load(&m, 0), resolved.path);
                    assert_eq!(stack::load(&m, 4), resolved.name);
                    if cache {
                        crate::native_vm_namespace::prepare_class_cache(
                            &mut m,
                            &mut resolved,
                            &mut env,
                        )
                        .unwrap();
                    }
                    if file_input {
                        let input = crate::native_vm_namespace::read_class_input(
                            &mut m,
                            &mut resolved,
                            &mut env,
                        )
                        .unwrap();
                        assert_eq!(resolved.pc, 0x411b20);
                        assert!(stack::has_pending_call(&m));
                        assert_eq!(stack::argument(&m, 0), input.source);
                        assert_eq!(stack::argument(&m, 1), input.bytes);
                        if module_input {
                            let mut module = crate::native_vm_namespace::begin_module_load(
                                &mut m,
                                resolved.jvm,
                                &input,
                                &mut env,
                            )
                            .unwrap();
                            assert_eq!(module.pc, 0x407660);
                            assert_eq!(stack::reg(&m, Reg::Esi), module.module);
                            assert_eq!(stack::reg(&m, Reg::Ecx), module.pool);
                            assert!(stack::has_pending_call(&m));
                            assert_eq!(m.r32(stack::sp(&m)), 0x411f03);
                            assert_eq!(stack::argument(&m, 0), module.source);
                            if constants_input {
                                crate::native_vm_namespace::deserialize_constants(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                assert_eq!(module.pc, 0x411f03);
                                assert!(!stack::has_pending_call(&m));
                            }
                            if members_input {
                                crate::native_vm_namespace::deserialize_members(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                assert_eq!(module.pc, 0x411bdc);
                            }
                            if tree_input {
                                let class = crate::native_vm_namespace::finish_module_tree(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                assert_eq!(module.pc, 0x411d03);
                                assert_eq!(stack::load(&m, 0x10), class);
                            }
                            if returned_input {
                                let class = crate::native_vm_namespace::finish_class_load(
                                    &mut m,
                                    &mut resolved,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                assert_eq!(resolved.pc, 0x410022);
                                assert_eq!(stack::reg(&m, Reg::Eax), class);
                            }
                            if completed_input {
                                let next = prepare_second_class(&mut m, &mut resolved).unwrap();
                                let mut second =
                                    crate::native_vm_namespace::begin_class_resolution(
                                        &mut m, next, &mut env,
                                    )
                                    .unwrap();
                                crate::native_vm_namespace::prepare_class_cache(
                                    &mut m,
                                    &mut second,
                                    &mut env,
                                )
                                .unwrap();
                                let input = crate::native_vm_namespace::read_class_input(
                                    &mut m,
                                    &mut second,
                                    &mut env,
                                )
                                .unwrap();
                                let mut module = crate::native_vm_namespace::begin_module_load(
                                    &mut m, second.jvm, &input, &mut env,
                                )
                                .unwrap();
                                crate::native_vm_namespace::deserialize_constants(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                crate::native_vm_namespace::deserialize_members(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                crate::native_vm_namespace::finish_module_tree(
                                    &mut m,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                crate::native_vm_namespace::finish_class_load(
                                    &mut m,
                                    &mut second,
                                    &mut module,
                                    &mut env,
                                )
                                .unwrap();
                                assert_eq!(second.pc, 0x41002e);
                                finish_bootstrap(&mut m, &mut second).unwrap();
                                assert!(!stack::active(&m));
                                assert_eq!(second.pc, 0x7fff0000);
                            }
                            let _caller = module.caller;
                        }
                    }
                    let _retained = (
                        resolved.bootstrap,
                        resolved.lookup,
                        resolved.namespace,
                        resolved.loader,
                    );
                }
            }
            set_test_store_observer(None);
            crate::native_vm_test_store::set(None);
            crate::native_vm_namespace::set_test_store_observer(None);
            crate::native_vm_class_context::set_test_store_observer(None);
            crate::native_vm_signature::set_test_store_observer(None);
            crate::native_vm_jvm::set_test_store_observer(None);
            crate::native_vm_lookup::set_test_store_observer(None);
            crate::native_vm_metadata::registration::set_test_store_observer(None);
            assert_eq!(env.base.index, events.len());
            assert_eq!(env.next_enumeration, env.enumerations.len());
            assert!(env.platform.searches_closed());
            compare(
                &m,
                snapshots.last().unwrap(),
                &format!("actual class-load boundary row{index}"),
            );
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (word(&s[0]), word(&s[1]), word(&s[2])))
                .collect::<Vec<_>>();
            if !resolve {
                assert_eq!(expected.len(), 34507);
            }
            let actual = STORES.with(|s| s.borrow().clone());
            assert_eq!(
                actual.len(),
                expected.len(),
                "storecount row{index}; first difference {:?}",
                actual
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
            );
            let difference = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a != b);
            assert!(
                difference.is_none(),
                "ordered stores row{index}: {difference:x?}"
            );
        }
    }
    include!("native_vm_root_tests.rs");
}
