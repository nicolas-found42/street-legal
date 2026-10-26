//! Source40fe70 bootstrap prefix. Intermediate contexts retain the entered
//! original caller; no unwind, completed bootstrap, or class load is implied.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<fn(u32,u32,u32)>>=const{std::cell::Cell::new(None)};}
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
fn allocate(m: &mut Space, pc: u32, n: u32, e: &mut impl Allocator) -> Result<u32, String> {
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
    e: &mut impl Allocator,
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
//41ef60 empty-bucket insertion used by the four source builtin names. Other
//collision/pool states are explicit unresolved inputs, not guessed headers.
fn insert(
    m: &mut Space,
    table: u32,
    key: u32,
    value: u32,
    e: &mut impl Allocator,
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
        if m.r32(bucket + 4) != 0 || m.r32(table + 0x7f38) != 0 {
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
fn intern_raw(
    m: &mut Space,
    holder: u32,
    src: u32,
    len: u32,
    e: &mut impl Allocator,
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
    })
}
fn intern(m: &mut Space, holder: u32, src: u32, e: &mut impl Allocator) -> Result<u32, String> {
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
    e: &mut impl Allocator,
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
pub fn prepare_descriptors(
    m: &mut Space,
    jvm: u32,
    e: &mut impl Allocator,
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
        let script="import base64,hashlib,json,struct,sys,zlib\nfor b in json.load(sys.stdin):\n raw=base64.b64decode(b['zlib_base64'],validate=True); z=zlib.decompressobj(); d=z.decompress(raw,524289); assert len(d)<=524288 and not z.unconsumed_tail; d+=z.flush(); assert z.eof and not z.unused_data and len(d)==b['bytes'] and hashlib.sha256(d).hexdigest()==b['sha256']; sys.stdout.buffer.write(struct.pack('<I',len(d))+d)";
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
                assert_eq!(heap.len(), 0x80000);
                assert_eq!(stack.len(), 0x3000);
                let images = v["image"].as_array().unwrap();
                assert_eq!(images.len(), 3);
                let image = images
                    .iter()
                    .zip([(0x62d000, 0x3000), (0x766000, 0x3000), (0x777000, 0x1000)])
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
            let differences = bytes
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    (m.r8(base + i as u32) != *b).then_some((
                        base + i as u32,
                        m.r8(base + i as u32),
                        *b,
                    ))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(
                differences.is_empty(),
                "{label} memory {base:x}: {differences:x?}"
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
        let v: Json = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-bootstrap-original.json"))
                .expect("run native_vm_bootstrap_probe.py"),
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
                    base >= 0x400000 && base < 0x7c0000 && base % 4096 == 0 && seen.insert(base)
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
            crate::native_vm_namespace::set_test_store_observer(Some(observe));
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
            crate::native_vm_namespace::set_test_store_observer(None);
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
        next_handle: u32,
        finds: std::collections::HashMap<u32, (Vec<(String, bool, u32)>, usize)>,
        stock: std::path::PathBuf,
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
        fn write_find(m: &mut Space, dest: u32, entry: &(String, bool, u32)) {
            let mut data = [0u8; 320];
            data[..4].copy_from_slice(&(if entry.1 { 16u32 } else { 32u32 }).to_le_bytes());
            data[32..36].copy_from_slice(&entry.2.to_le_bytes());
            assert!(entry.0.len() < 260);
            data[44..44 + entry.0.len()].copy_from_slice(entry.0.as_bytes());
            for (i, b) in data.into_iter().enumerate() {
                m.w8(dest + i as u32, b)
            }
        }
        fn live_entries(&self, pattern: &str) -> Vec<(String, bool, u32)> {
            let normalized = pattern.replace('\\', "/");
            let (directory, mask) = normalized.rsplit_once('/').unwrap();
            assert!(mask == "*" || mask == "*.*");
            let mut path = self.stock.clone();
            for part in directory.split('/').filter(|s| !s.is_empty()) {
                assert!(part != "." && part != "..");
                let children = std::fs::read_dir(&path)
                    .unwrap()
                    .map(|p| p.unwrap().path())
                    .filter(|p| {
                        p.file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .eq_ignore_ascii_case(part)
                    })
                    .collect::<Vec<_>>();
                assert_eq!(children.len(), 1);
                path = children[0].clone();
                assert!(path
                    .canonicalize()
                    .unwrap()
                    .starts_with(self.stock.canonicalize().unwrap()));
            }
            let mut out = vec![(".".into(), true, 0), ("..".into(), true, 0)];
            for item in std::fs::read_dir(path).unwrap() {
                let p = item.unwrap().path();
                let dir = p.is_dir();
                out.push((
                    p.file_name().unwrap().to_str().unwrap().to_owned(),
                    dir,
                    if dir {
                        0
                    } else {
                        u32::try_from(p.metadata().unwrap().len()).unwrap()
                    },
                ));
            }
            out.sort_by_key(|e| e.0.to_ascii_lowercase());
            let names = out
                .iter()
                .map(|e| e.0.to_ascii_lowercase())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(names.len(), out.len());
            out
        }
    }
    impl Allocator for FullEnv<'_> {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            self.base.allocate(m, n)
        }
        fn reallocate(&mut self, m: &mut Space, p: u32, old_n: u32, n: u32) -> Result<u32, String> {
            self.base.reallocate(m, p, old_n, n)
        }
    }
    impl crate::native_vm_namespace::Environment for FullEnv<'_> {
        fn free(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
            self.check(m, "free", json!([p]));
            Ok(0)
        }
        fn find_first(&mut self, m: &mut Space, pattern: u32, dest: u32) -> Result<u32, String> {
            self.check(m, "FindFirstFileA", json!([pattern, dest]));
            let pattern = cstring(m, pattern).replace('\\', "/");
            let entry = &self.enumerations[self.next_enumeration];
            assert_eq!(entry["pattern"], pattern);
            let rows = self.live_entries(&pattern);
            assert_eq!(
                json!(rows),
                entry["entries"],
                "live stock directory source differs"
            );
            self.next_enumeration += 1;
            self.next_handle += 1;
            Self::write_find(m, dest, &rows[0]);
            self.finds.insert(self.next_handle, (rows, 1));
            Ok(self.next_handle)
        }
        fn find_next(&mut self, m: &mut Space, handle: u32, dest: u32) -> Result<u32, String> {
            self.check(m, "FindNextFileA", json!([handle, dest]));
            let (rows, index) = self.finds.get_mut(&handle).unwrap();
            if *index == rows.len() {
                return Ok(0);
            }
            Self::write_find(m, dest, &rows[*index]);
            *index += 1;
            Ok(1)
        }
        fn find_close(&mut self, m: &mut Space, handle: u32) -> Result<u32, String> {
            self.check(m, "FindClose", json!([handle]));
            self.finds.remove(&handle).unwrap();
            Ok(1)
        }
    }
    #[test]
    fn complete_prefix_reaches_actual_first_class_load_with_retained_caller_and_exact_all_leaf_states(
    ) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Json = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-bootstrap-original.json"))
                .expect("run native_vm_bootstrap_probe.py"),
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
            let events = row["events"].as_array().unwrap();
            assert_eq!(events.len(), 770);
            let mut values = vec![&row["before"]];
            values.extend(events.iter().map(|e| &e["state"]));
            values.push(&row["after"]);
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
                    base >= 0x400000 && base < 0x7c0000 && base % 4096 == 0 && seen.insert(base)
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
            crate::native_vm_namespace::set_test_store_observer(Some(observe));
            let mut env = FullEnv {
                base: Env {
                    events,
                    states: &snapshots[1..1 + events.len()],
                    index: 0,
                },
                enumerations: row["stock_enumerations"].as_array().unwrap(),
                next_enumeration: 0,
                next_handle: 0,
                finds: std::collections::HashMap::new(),
                stock: root.join("stock"),
            };
            let prefix = prepare_descriptors(&mut m, word(&row["this"]), &mut env).unwrap();
            let prefix = prepare_namespace_root(&mut m, prefix, &mut env).unwrap();
            let pending = prepare_class_load(&mut m, prefix, &mut env).unwrap();
            set_test_store_observer(None);
            crate::native_vm_namespace::set_test_store_observer(None);
            assert_eq!(pending.pc, 0x41001d);
            assert_eq!(pending.name, 0x60ac84);
            assert_eq!(pending.jvm, word(&row["this"]));
            let _caller = pending.caller;
            assert!(stack::active(&m));
            assert_eq!(m.r32(stack::sp(&m)), pending.name);
            assert_eq!(env.base.index, events.len());
            assert_eq!(env.next_enumeration, env.enumerations.len());
            assert!(env.finds.is_empty());
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
            assert_eq!(expected.len(), 34507);
            let actual = STORES.with(|s| s.borrow().clone());
            assert_eq!(actual.len(), expected.len(), "storecount row{index}");
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
}
