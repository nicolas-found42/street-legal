//! Source4165a0 cache preparation with explicit read-only Win32 collaborators.
use super::*;
pub use slr_boot::native_vm_platform::FileLeaf;
pub trait FileEnvironment: Environment {
    fn file_leaf(
        &mut self,
        m: &mut Space,
        leaf: FileLeaf,
        target: u32,
        args: &[u32],
    ) -> Result<u32, String>;
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn pop(m: &mut Space, reg: Reg) {
    set(m, reg, stack::load(m, 0));
    stack::add_sp(m, 4);
}
fn leaf(
    m: &mut Space,
    pc: u32,
    slot: u32,
    kind: FileLeaf,
    args: &[u32],
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    let target = m.r32(slot);
    let result = stack::external(m, pc, args, None, |m| e.file_leaf(m, kind, target, args))?;
    set(m, Reg::Eax, result);
    Ok(result)
}
fn exists(m: &mut Space, source: u32, e: &mut impl FileEnvironment) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, source);
        stack::alloc(m, 0x100);
        stack::push_reg(m, Reg::Esi);
        let buffer = stack::sp(m) + 4;
        stack::push(m, source);
        set(m, Reg::Ecx, buffer);
        stack::push(m, buffer);
        set(m, Reg::Esi, buffer);
        stack::call(m, 0x54c18a, &[], None, |m| strcpy(m, buffer, source));
        low(m, Reg::Eax, m.r8(buffer));
        stack::add_sp(m, 8);
        let mut p = buffer;
        while m.r8(p) != 0 {
            if m.r8(p) == b'/' {
                byte(m, p, b'\\');
            }
            low(m, Reg::Eax, m.r8(p + 1));
            p += 1;
            set(m, Reg::Esi, p);
        }
        set(m, Reg::Eax, m.r32(0x7686e0));
        pop(m, Reg::Esi);
        if m.r32(0x7686e0) != 0 {
            return Err("NativeVmClassCache: archive-backed existence owner unavailable".into());
        }
        set(m, Reg::Eax, buffer);
        stack::push(m, buffer);
        let result = stack::call(m, 0x54c1d6, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, buffer);
                let handle = leaf(
                    m,
                    0x5586cd,
                    0x5e7194,
                    FileLeaf::Create,
                    &[buffer, 0x80000000, 1, 0, 3, 0x80, 0],
                    e,
                )?;
                if handle == u32::MAX {
                    set(m, Reg::Eax, 0);
                    return Ok::<u32, String>(0);
                }
                leaf(m, 0x5586d9, 0x5e7118, FileLeaf::Close, &[handle], e)?;
                set(m, Reg::Eax, 1);
                Ok(1)
            })
        })?;
        stack::add_sp(m, 4 + 0x100);
        Ok(result)
    })
}
fn file_mutex(m: &mut Space, enter: bool) -> Result<(), String> {
    let args = if enter { vec![u32::MAX] } else { vec![] };
    stack::function(m, &args, Some(0x766458), |m| {
        set(m, Reg::Eax, m.r32(0x766458));
        if m.r32(0x766458) != 0 {
            return Err("NativeVmClassCache: file-cache critical-section owner unavailable".into());
        }
        Ok(())
    })
}
fn lock(m: &mut Space, index: u32, e: &mut impl FileEnvironment) -> Result<(), String> {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, index);
        let handle = m.r32(0x77c758 + index * 8);
        set(m, Reg::Eax, handle);
        if handle == 0 {
            return Err("NativeVmClassCache: actual file semaphore absent".into());
        }
        set(m, Reg::Ecx, u32::MAX);
        if leaf(
            m,
            0x559859,
            0x5e70a8,
            FileLeaf::Wait,
            &[handle, u32::MAX],
            e,
        )? != 0
        {
            return Err("NativeVmClassCache: semaphore wait did not acquire".into());
        }
        let depth = 0x77c75c + index * 8;
        set(m, Reg::Ecx, m.r32(depth));
        set(m, Reg::Eax, depth);
        set(m, Reg::Ecx, r(m, Reg::Ecx) + 1);
        write(m, depth, r(m, Reg::Ecx));
        set(m, Reg::Eax, 0);
        pop(m, Reg::Esi);
        Ok(())
    })
}
fn unlock(m: &mut Space, index: u32, e: &mut impl FileEnvironment) -> Result<(), String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, index);
        set(m, Reg::Ecx, m.r32(0x77c758 + index * 8));
        if r(m, Reg::Ecx) == 0 {
            return Err("NativeVmClassCache: release semaphore absent".into());
        }
        let depth = 0x77c75c + index * 8;
        set(m, Reg::Ecx, m.r32(depth).wrapping_sub(1));
        stack::push(m, 0);
        write(m, depth, r(m, Reg::Ecx));
        set(m, Reg::Eax, m.r32(0x77c758 + index * 8));
        stack::push(m, 1);
        stack::push(m, r(m, Reg::Eax));
        let args = [r(m, Reg::Eax), 1, 0];
        let result = stack::external(m, 0x5598b4, &[], None, |m| {
            e.file_leaf(m, FileLeaf::Release, m.r32(0x5e7110), &args)
        })?;
        stack::add_sp(m, 12);
        set(m, Reg::Eax, if result == 0 { 0xffff_fffe } else { 0 });
        Ok(())
    })
}
fn open_os(m: &mut Space, source: u32, e: &mut impl FileEnvironment) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        let mutex = m.r32(0x61b630);
        set(m, Reg::Eax, mutex);
        stack::push(m, u32::MAX);
        stack::push(m, mutex);
        stack::call(m, 0x558d8d, &[], None, |m| lock(m, mutex, e))?;
        stack::add_sp(m, 8);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, 0);
        set(m, Reg::Eax, 0x777558);
        let mut slot = 0;
        while slot < 64 && m.r32(0x777558 + slot * 8) != 0 {
            slot += 1;
            set(m, Reg::Eax, 0x777558 + slot * 8);
            set(m, Reg::Esi, slot);
        }
        if slot == 64 {
            return Err("NativeVmClassCache: physical file table full".into());
        }
        set(m, Reg::Eax, source);
        let handle = leaf(
            m,
            0x558df9,
            0x5e7194,
            FileLeaf::Create,
            &[source, 0x80000000, 1, 0, 3, 0x80, 0],
            e,
        )?;
        if handle == u32::MAX {
            return Err("NativeVmClassCache: cached file failed to open".into());
        }
        stack::push(m, 0);
        stack::push(m, handle);
        write(m, 0x777558 + slot * 8, handle);
        let args = [handle, 0];
        let size = stack::external(m, 0x558e22, &[], None, |m| {
            e.file_leaf(m, FileLeaf::Size, m.r32(0x5e7198), &args)
        })?;
        stack::add_sp(m, 8);
        set(m, Reg::Eax, size);
        set(m, Reg::Edx, mutex);
        stack::push(m, mutex);
        write(m, 0x77755c + slot * 8, size);
        stack::call(m, 0x558e35, &[], None, |m| unlock(m, mutex, e))?;
        stack::add_sp(m, 4);
        set(m, Reg::Eax, slot);
        pop(m, Reg::Esi);
        Ok(slot)
    })
}
fn open_cached(m: &mut Space, source: u32, e: &mut impl FileEnvironment) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, source);
        stack::alloc(m, 0x100);
        stack::push_reg(m, Reg::Esi);
        let buffer = stack::sp(m) + 4;
        stack::push(m, source);
        set(m, Reg::Ecx, buffer);
        stack::push(m, buffer);
        set(m, Reg::Esi, buffer);
        stack::call(m, 0x54c89a, &[], None, |m| strcpy(m, buffer, source));
        low(m, Reg::Eax, m.r8(buffer));
        stack::add_sp(m, 8);
        let mut p = buffer;
        while m.r8(p) != 0 {
            if m.r8(p) == b'/' {
                byte(m, p, b'\\');
            }
            low(m, Reg::Eax, m.r8(p + 1));
            p += 1;
            set(m, Reg::Esi, p);
        }
        set(m, Reg::Edx, buffer);
        stack::push(m, 0);
        stack::push(m, buffer);
        let slot = stack::call(m, 0x54c8c1, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                stack::alloc(m, 8);
                for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
                    stack::push_reg(m, reg);
                }
                set(m, Reg::Ebx, u32::MAX);
                stack::call(m, 0x54c8e5, &[u32::MAX], Some(0x766458), |m| {
                    file_mutex(m, true)
                })?;
                stack::store(m, 0x14, u32::MAX);
                set(m, Reg::Ebp, 1);
                stack::store(m, 0x10, 0xffff_fef4);
                set(m, Reg::Edi, 0x10c);
                let mut candidate = None;
                let mut fallback = None;
                for index in 1..32u32 {
                    let offset = index * 0x10c;
                    let record = 0x766460 + offset;
                    set(m, Reg::Eax, m.r32(record));
                    set(m, Reg::Esi, record + 4);
                    if (m.r32(record) as i32) >= 0 {
                        low(m, Reg::Eax, m.r8(record + 0x108));
                        if m.r8(record + 0x108) == 0 {
                            set(m, Reg::Eax, buffer);
                            stack::push(m, 0);
                            stack::push(m, buffer);
                            stack::push(m, record + 4);
                            let same = stack::call(m, 0x54c922, &[], None, |m| {
                                sorting::string_compare(m, record + 4, buffer, false)
                            });
                            stack::add_sp(m, 12);
                            if same == 0 {
                                return Err("NativeVmClassCache: same-file cache reuse continuation unavailable".into());
                            }
                            if fallback.is_none() {
                                fallback = Some(index);
                                set(m, Reg::Ebx, index);
                                stack::store(m, 0x10, offset);
                            } else {
                                let previous = stack::load(m, 0x10);
                                set(m, Reg::Edx, previous);
                                set(m, Reg::Ecx, m.r32(record + 0x104));
                                if m.r32(record + 0x104) < m.r32(0x766464 + previous + 0x100) {
                                    fallback = Some(index);
                                    set(m, Reg::Ebx, index);
                                    stack::store(m, 0x10, offset);
                                }
                            }
                        }
                    } else {
                        set(m, Reg::Eax, stack::load(m, 0x14));
                        if candidate.is_none() {
                            candidate = Some(index);
                            stack::store(m, 0x14, index);
                        }
                    }
                    set(m, Reg::Edi, offset + 0x10c);
                    set(m, Reg::Ebp, index + 1);
                }
                let index = candidate.ok_or("NativeVmClassCache: no cache slot")?;
                set(m, Reg::Edi, index);
                low(m, Reg::Ebx, 0);
                set(m, Reg::Eax, index * 33);
                set(m, Reg::Ecx, index * 67);
                let record = 0x766460 + index * 0x10c;
                set(m, Reg::Esi, record);
                set(m, Reg::Eax, buffer);
                stack::push(m, buffer);
                let os = stack::call(m, 0x54ca83, &[], None, |m| open_os(m, buffer, e))?;
                stack::add_sp(m, 4);
                write(m, record, os);
                set(m, Reg::Ecx, buffer);
                stack::push(m, buffer);
                set(m, Reg::Edx, record + 4);
                stack::push(m, record + 4);
                stack::call(m, 0x54ca9a, &[], None, |m| strcpy(m, record + 4, buffer));
                set(m, Reg::Eax, m.r32(0x76645c));
                write(m, record + 0x104, r(m, Reg::Eax));
                set(m, Reg::Eax, m.r32(0x76645c));
                stack::add_sp(m, 8);
                set(m, Reg::Eax, r(m, Reg::Eax) + 1);
                write(m, 0x76645c, r(m, Reg::Eax));
                set(m, Reg::Ecx, 0x766458);
                byte(m, record + 0x108, 1);
                observe(record + 0x109, 1, r(m, Reg::Ebx));
                m.w8(record + 0x109, r(m, Reg::Ebx) as u8);
                stack::call(m, 0x54caca, &[], Some(0x766458), |m| file_mutex(m, false))?;
                set(m, Reg::Eax, index);
                for reg in [Reg::Edi, Reg::Esi, Reg::Ebp, Reg::Ebx] {
                    pop(m, reg);
                }
                stack::add_sp(m, 8);
                Ok::<u32, String>(index)
            })
        })?;
        stack::add_sp(m, 8);
        pop(m, Reg::Esi);
        stack::add_sp(m, 0x100);
        Ok(slot)
    })
}
fn read_os(
    m: &mut Space,
    os: u32,
    destination: u32,
    bytes: u32,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, os);
        let handle = m.r32(0x777558 + os * 8);
        set(m, Reg::Eax, handle);
        stack::alloc(m, 0x108);
        if handle == 0 {
            return Err("NativeVmClassCache: cached read has no OS file owner".into());
        }
        let count = stack::sp(m) + 4;
        set(m, Reg::Edx, bytes);
        stack::push(m, 0);
        set(m, Reg::Ecx, count);
        stack::push(m, count);
        set(m, Reg::Ecx, destination);
        stack::push(m, bytes);
        stack::push(m, destination);
        stack::push(m, handle);
        let args = [handle, destination, bytes, count, 0];
        let result = stack::external(m, 0x559203, &[], None, |m| {
            e.file_leaf(m, FileLeaf::Read, m.r32(0x5e719c), &args)
        })?;
        stack::add_sp(m, 20);
        set(m, Reg::Eax, result);
        if result == 0 {
            return Err("NativeVmClassCache: cached read failed".into());
        }
        set(m, Reg::Eax, m.r32(count));
        stack::add_sp(m, 0x108);
        Ok::<u32, String>(r(m, Reg::Eax))
    })
}
fn read_cached(
    m: &mut Space,
    file: u32,
    destination: u32,
    bytes: u32,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        if !(1..32).contains(&file) {
            return Err("NativeVmClassCache: invalid cached read owner".into());
        }
        set(m, Reg::Eax, file);
        set(m, Reg::Ecx, bytes);
        set(m, Reg::Edx, destination);
        stack::push(m, bytes);
        set(m, Reg::Ecx, file * 33);
        stack::push(m, destination);
        set(m, Reg::Edx, file * 67);
        let os = m.r32(0x766460 + file * 0x10c);
        set(m, Reg::Eax, os);
        stack::push(m, os);
        let count = stack::call(m, 0x54ceee, &[], None, |m| {
            read_os(m, os, destination, bytes, e)
        })?;
        stack::add_sp(m, 12);
        Ok(count)
    })
}
fn close_cached(m: &mut Space, file: u32) -> Result<(), String> {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Esi);
        stack::call(m, 0x54ccad, &[u32::MAX], Some(0x766458), |m| {
            file_mutex(m, true)
        })?;
        set(m, Reg::Eax, file);
        if (1..32).contains(&file) {
            set(m, Reg::Ecx, file * 33);
            set(m, Reg::Esi, file * 0x10c);
            let record = 0x766460 + file * 0x10c;
            low(m, Reg::Eax, m.r8(record + 0x109));
            if m.r8(record + 0x109) != 0 {
                return Err("NativeVmClassCache: write-mode cache close owner unavailable".into());
            }
            byte(m, record + 0x108, 0);
        }
        stack::call(m, 0x54cd07, &[], Some(0x766458), |m| file_mutex(m, false))?;
        pop(m, Reg::Esi);
        Ok(())
    })
}
pub(super) fn read_word(m: &mut Space, source: u32) -> u32 {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, source);
        stack::push(m, source);
        let word = stack::call(m, 0x54f68a, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, source);
                set(m, Reg::Ecx, 0);
                set(m, Reg::Edx, 0);
                set(m, Reg::Ecx, m.r32(source));
                set(m, Reg::Edx, u32::from(m.r8(source)));
                set(m, Reg::Eax, r(m, Reg::Ecx));
                r(m, Reg::Eax)
            })
        });
        stack::add_sp(m, 4);
        word
    })
}
pub fn prepare_class_cache(
    m: &mut Space,
    pending: &mut PendingClassResolution,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    if pending.pc != 0x41044c
        || !stack::active(m)
        || stack::has_pending_call(m)
        || stack::load(m, 0) != pending.path
        || stack::load(m, 4) != pending.name
        || r(m, Reg::Ecx) != pending.jvm
    {
        return Err("NativeVmClassCache: actual retained source loader CALL required".into());
    }
    stack::begin_call(m, 0x410451, &[], Some(pending.jvm));
    let cache = stack::enter(m, &[pending.path, pending.name], Some(pending.jvm));
    stack::alloc(m, 0x90c);
    for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
        stack::push_reg(m, reg);
    }
    let b = stack::sp(m);
    let java = b + 0x1c;
    let class = b + 0x41c;
    set(m, Reg::Edi, pending.path);
    stack::push(m, 0x100);
    set(m, Reg::Eax, java);
    stack::push(m, pending.path);
    stack::push(m, java);
    set(m, Reg::Ebp, pending.jvm);
    m.w8(b + 0x13, 0);
    low(m, Reg::Ebx, 0);
    stack::call(m, 0x4165ca, &[], None, |m| {
        bounded_copy(m, java, pending.path, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Ecx, java);
    stack::push(m, 0x60b720);
    stack::push(m, java);
    stack::call(m, 0x4165de, &[], None, |m| {
        mapping::append(m, java, 0x60b720, 0x100)
    });
    set(m, Reg::Esi, pending.name);
    stack::push(m, 0x100);
    set(m, Reg::Edx, java);
    stack::push(m, pending.name);
    stack::push(m, java);
    stack::call(m, 0x4165f5, &[], None, |m| {
        mapping::append(m, java, pending.name, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Eax, java);
    stack::push(m, 0x60b728);
    stack::push(m, java);
    stack::call(m, 0x416609, &[], None, |m| {
        mapping::append(m, java, 0x60b728, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Ecx, class);
    stack::push(m, pending.path);
    stack::push(m, class);
    stack::call(m, 0x41661c, &[], None, |m| {
        bounded_copy(m, class, pending.path, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Edx, class);
    stack::push(m, 0x60b730);
    stack::push(m, class);
    stack::call(m, 0x416633, &[], None, |m| {
        mapping::append(m, class, 0x60b730, 0x100)
    });
    stack::add_sp(m, 0x48);
    stack::push(m, 0x100);
    set(m, Reg::Eax, class);
    stack::push(m, pending.name);
    stack::push(m, class);
    stack::call(m, 0x416649, &[], None, |m| {
        mapping::append(m, class, pending.name, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Ecx, class);
    stack::push(m, 0x60b734);
    stack::push(m, class);
    stack::call(m, 0x416660, &[], None, |m| {
        mapping::append(m, class, 0x60b734, 0x100)
    });
    set(m, Reg::Edx, java);
    stack::push(m, java);
    let source = stack::call(m, 0x41666a, &[], None, |m| exists(m, java, e))?;
    set(m, Reg::Esi, source);
    set(m, Reg::Eax, class);
    stack::push(m, class);
    m.w32(b + 0x14, source);
    let cached = stack::call(m, 0x41667d, &[], None, |m| exists(m, class, e))?;
    stack::add_sp(m, 0x20);
    if source != 0 || cached == 0 {
        return Err("NativeVmClassCache: source compilation path unavailable".into());
    }
    stack::push(m, 12);
    let header = stack::external(m, 0x4166b3, &[], None, |m| e.allocate(m, 12))?;
    set(m, Reg::Eax, header);
    set(m, Reg::Esi, header);
    set(m, Reg::Eax, class);
    stack::push(m, class);
    let file = stack::call(m, 0x4166c2, &[], None, |m| open_cached(m, class, e))?;
    stack::push(m, 12);
    set(m, Reg::Edi, file);
    stack::push(m, header);
    stack::push(m, file);
    stack::call(m, 0x4166cd, &[], None, |m| {
        read_cached(m, file, header, 12, e)
    })?;
    stack::push(m, file);
    stack::call(m, 0x4166d3, &[], None, |m| close_cached(m, file))?;
    stack::push(m, header);
    let magic = stack::call(m, 0x4166d9, &[], None, |m| read_word(m, header));
    set(m, Reg::Ecx, header + 4);
    stack::push(m, header + 4);
    set(m, Reg::Edi, magic);
    let length = stack::call(m, 0x4166e4, &[], None, |m| read_word(m, header + 4));
    set(m, Reg::Edx, header + 8);
    stack::push(m, header + 8);
    m.w32(b + 0x18, length);
    let version = stack::call(m, 0x4166f1, &[], None, |m| read_word(m, header + 8));
    stack::add_sp(m, 0x24);
    if magic != 0x41465554 || length != 4 || version != 0x11ae1 {
        return Err("NativeVmClassCache: cached compiler header requires rebuild".into());
    }
    stack::push(m, header);
    let freed = stack::external(m, 0x416712, &[], None, |m| e.free(m, header))?;
    set(m, Reg::Eax, freed);
    set(m, Reg::Esi, source);
    stack::add_sp(m, 4);
    set(m, Reg::Edi, 0);
    if r(m, Reg::Ebx) as u8 != 0 {
        return Err("NativeVmClassCache: compilation remains pending".into());
    }
    pop(m, Reg::Edi);
    pop(m, Reg::Esi);
    pop(m, Reg::Ebp);
    low(m, Reg::Eax, 1);
    pop(m, Reg::Ebx);
    stack::add_sp(m, 0x90c);
    stack::leave(m, cache);
    pending.pc = 0x410451;
    Ok(())
}

#[must_use]
pub struct FileInput {
    pub source: u32,
    pub bytes: u32,
}
fn close_os(m: &mut Space, os: u32, e: &mut impl FileEnvironment) -> Result<(), String> {
    stack::function_cdecl(m, &[], |m| {
        let mutex = m.r32(0x61b630);
        set(m, Reg::Eax, mutex);
        stack::push(m, u32::MAX);
        stack::push(m, mutex);
        stack::call(m, 0x55909d, &[], None, |m| lock(m, mutex, e))?;
        stack::add_sp(m, 8);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, os);
        let handle = m.r32(0x777558 + os * 8);
        set(m, Reg::Eax, handle);
        if handle != 0 {
            leaf(m, 0x5590d0, 0x5e7118, FileLeaf::Close, &[handle], e)?;
        }
        set(m, Reg::Edx, mutex);
        write(m, 0x777558 + os * 8, 0);
        stack::push(m, mutex);
        write(m, 0x77755c + os * 8, 0);
        stack::call(m, 0x5590f2, &[], None, |m| unlock(m, mutex, e))?;
        stack::add_sp(m, 4);
        set(m, Reg::Eax, 0);
        pop(m, Reg::Esi);
        Ok(())
    })
}
fn whole_file(
    m: &mut Space,
    source: u32,
    output: u32,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, source);
        stack::alloc(m, 0x100);
        for reg in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            stack::push_reg(m, reg);
        }
        let buffer = stack::sp(m) + 12;
        stack::push(m, source);
        set(m, Reg::Ecx, buffer);
        stack::push(m, buffer);
        set(m, Reg::Esi, buffer);
        stack::call(m, 0x54c31c, &[], None, |m| strcpy(m, buffer, source));
        low(m, Reg::Eax, m.r8(buffer));
        stack::add_sp(m, 8);
        let mut p = buffer;
        while m.r8(p) != 0 {
            if m.r8(p) == b'/' {
                byte(m, p, b'\\');
            }
            low(m, Reg::Eax, m.r8(p + 1));
            p += 1;
            set(m, Reg::Esi, p);
        }
        set(m, Reg::Edx, buffer);
        stack::push(m, buffer);
        let os = stack::call(m, 0x54c341, &[], None, |m| open_os(m, buffer, e))?;
        set(m, Reg::Esi, os);
        stack::add_sp(m, 4);
        stack::push(m, os);
        let bytes = stack::call(m, 0x54c35d, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, os);
                set(m, Reg::Ecx, m.r32(0x777558 + os * 8));
                if r(m, Reg::Ecx) == 0 {
                    return Err("NativeVmClassCache: whole-file size owner unavailable".into());
                }
                set(m, Reg::Eax, m.r32(0x77755c + os * 8));
                Ok::<u32, String>(r(m, Reg::Eax))
            })
        })?;
        set(m, Reg::Ebx, output);
        set(m, Reg::Edi, bytes);
        set(m, Reg::Eax, m.r32(output));
        stack::add_sp(m, 4);
        if m.r32(output) != 0 {
            return Err("NativeVmClassCache: existing whole-file output owner unavailable".into());
        }
        stack::push(m, bytes);
        let data = stack::external(m, 0x54c375, &[], None, |m| e.allocate(m, bytes))?;
        set(m, Reg::Eax, data);
        stack::add_sp(m, 4);
        write(m, output, data);
        if data == 0 {
            return Err("NativeVmClassCache: whole-file allocation failed".into());
        }
        set(m, Reg::Eax, data);
        stack::push(m, bytes);
        stack::push(m, data);
        stack::push(m, os);
        let read = stack::call(m, 0x54c3a0, &[], None, |m| read_os(m, os, data, bytes, e))?;
        stack::push(m, os);
        set(m, Reg::Edi, read);
        stack::call(m, 0x54c3a8, &[], None, |m| close_os(m, os, e))?;
        stack::add_sp(m, 0x10);
        set(m, Reg::Eax, read);
        for reg in [Reg::Edi, Reg::Esi, Reg::Ebx] {
            pop(m, reg);
        }
        stack::add_sp(m, 0x100);
        Ok(read)
    })
}
pub fn read_class_input(
    m: &mut Space,
    pending: &mut PendingClassResolution,
    e: &mut impl FileEnvironment,
) -> Result<FileInput, String> {
    if pending.pc != 0x410451
        || stack::has_pending_call(m)
        || r(m, Reg::Ebp) != pending.path
        || r(m, Reg::Esi) != pending.name
        || r(m, Reg::Edi) != pending.jvm
    {
        return Err("NativeVmClassCache: actual post-cache loader continuation required".into());
    }
    let b = stack::sp(m);
    let path = b + 0x14;
    let output = b + 0x10;
    stack::push(m, 0x100);
    set(m, Reg::Eax, path);
    stack::push(m, pending.path);
    stack::push(m, path);
    stack::call(m, 0x410461, &[], None, |m| {
        bounded_copy(m, path, pending.path, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Ecx, path);
    stack::push(m, 0x60acc4);
    stack::push(m, path);
    stack::call(m, 0x410475, &[], None, |m| {
        mapping::append(m, path, 0x60acc4, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Edx, path);
    stack::push(m, pending.name);
    stack::push(m, path);
    stack::call(m, 0x410485, &[], None, |m| {
        mapping::append(m, path, pending.name, 0x100)
    });
    stack::push(m, 0x100);
    set(m, Reg::Eax, path);
    stack::push(m, 0x60acc8);
    stack::push(m, path);
    stack::call(m, 0x410499, &[], None, |m| {
        mapping::append(m, path, 0x60acc8, 0x100)
    });
    set(m, Reg::Ecx, output);
    stack::push(m, output);
    set(m, Reg::Edx, path);
    stack::push(m, path);
    m.w32(output, 0);
    let bytes = stack::call(m, 0x4104b0, &[], None, |m| whole_file(m, path, output, e))?;
    stack::add_sp(m, 0x38);
    if bytes == 0 || (bytes as i32) < 0 {
        return Err("NativeVmClassCache: whole class-file input failed".into());
    }
    stack::push(m, bytes);
    let source = m.r32(output);
    set(m, Reg::Eax, source);
    stack::push(m, source);
    set(m, Reg::Ecx, pending.jvm);
    stack::begin_call(m, 0x4104c8, &[], Some(pending.jvm));
    pending.pc = 0x411b20;
    Ok(FileInput { source, bytes })
}
