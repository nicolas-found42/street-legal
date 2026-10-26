//! Raw54cf50 sorted enumeration and559340/5593f0/559480 Win32 adapters.
//! Slot tables, local buffers, source qsort and every CALL remain native data.
use super::*;
fn reg(m: &Space, r: Reg) -> u32 {
    stack::reg(m, r)
}
fn scan_name(m: &mut Space, p: u32) -> Result<u32, String> {
    set(m, Reg::Eax, 0);
    scan(m, p)
}
fn move_name(m: &mut Space, destination: u32, source: u32, n: u32) {
    set(m, Reg::Esi, source);
    set(m, Reg::Edi, destination);
    set(m, Reg::Ecx, n >> 2);
    let words = n / 4;
    for i in 0..words {
        write(m, destination + i * 4, m.r32(source + i * 4));
        set(m, Reg::Esi, source + (i + 1) * 4);
        set(m, Reg::Edi, destination + (i + 1) * 4);
        set(m, Reg::Ecx, words - i - 1);
    }
}
fn move_tail(m: &mut Space, n: u32) {
    let source = reg(m, Reg::Esi);
    let destination = reg(m, Reg::Edi);
    set(m, Reg::Ecx, n & 3);
    for i in 0..(n & 3) {
        byte(m, destination + i, m.r8(source + i));
        set(m, Reg::Esi, source + i + 1);
        set(m, Reg::Edi, destination + i + 1);
        set(m, Reg::Ecx, (n & 3) - i - 1);
    }
}
fn first(
    m: &mut Space,
    pattern: u32,
    destination: u32,
    directory: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Ebx, 0);
        set(m, Reg::Eax, 0x777458);
        let mut slot = 0;
        while slot < 64 && m.r32(0x777458 + slot * 4) != 0 {
            slot += 1;
            set(m, Reg::Eax, 0x777458 + slot * 4);
            set(m, Reg::Ebx, slot);
        }
        if slot == 64 {
            set(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        set(m, Reg::Eax, pattern);
        set(m, Reg::Esi, slot * 320);
        let buffer = 0x777758 + slot * 320;
        set(m, Reg::Ebp, buffer);
        let handle = stack::external(m, 0x559385, &[pattern, buffer], None, |m| {
            e.find_first(m, pattern, buffer)
        })?;
        set(m, Reg::Eax, handle);
        write(m, 0x777458 + slot * 4, handle);
        if handle == u32::MAX {
            write(m, 0x777458 + slot * 4, 0);
            set(m, Reg::Eax, u32::MAX - 1);
            return Ok(u32::MAX - 1);
        }
        stack::push_reg(m, Reg::Edi);
        let name = buffer + 44;
        let n = scan_name(m, name)?;
        if n > 260 {
            return Err("NativeVmFind: Win32 name exceeds supplied find-data span".into());
        }
        set(m, Reg::Eax, directory);
        set(m, Reg::Edi, name);
        set(m, Reg::Edx, n);
        move_name(m, destination, name, n);
        set(m, Reg::Ecx, n);
        move_tail(m, n);
        stack::add_sp(m, 4);
        if directory != 0 {
            let flag = (m.r32(buffer) >> 4) & 1;
            set(m, Reg::Ecx, flag);
            write(m, directory, flag);
        }
        set(m, Reg::Eax, slot);
        Ok(slot)
    })
}
fn next(
    m: &mut Space,
    slot: u32,
    destination: u32,
    directory: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, slot);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        if slot >= 64 {
            set(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        let handle = m.r32(0x777458 + slot * 4);
        set(m, Reg::Ecx, handle);
        if handle == 0 {
            set(m, Reg::Eax, u32::MAX - 1);
            return Ok(u32::MAX - 1);
        }
        set(m, Reg::Esi, slot * 320);
        let buffer = 0x777758 + slot * 320;
        set(m, Reg::Ebx, buffer);
        let result = stack::external(m, 0x559426, &[handle, buffer], None, |m| {
            e.find_next(m, handle, buffer)
        })?;
        set(m, Reg::Eax, result);
        if result == 0 {
            set(m, Reg::Eax, u32::MAX - 2);
            return Ok(u32::MAX - 2);
        }
        stack::push_reg(m, Reg::Edi);
        let name = buffer + 44;
        let n = scan_name(m, name)?;
        if n > 260 {
            return Err("NativeVmFind: Win32 name exceeds supplied find-data span".into());
        }
        set(m, Reg::Edi, name);
        set(m, Reg::Eax, n);
        move_name(m, destination, name, n);
        set(m, Reg::Ecx, n);
        set(m, Reg::Eax, directory);
        move_tail(m, n);
        stack::add_sp(m, 4);
        if directory != 0 {
            let flag = (m.r32(buffer) >> 4) & 1;
            set(m, Reg::Ecx, flag);
            write(m, directory, flag);
        }
        set(m, Reg::Eax, 0);
        Ok(0)
    })
}
fn close(m: &mut Space, slot: u32, e: &mut impl Environment) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, slot);
        if slot >= 64 {
            set(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        let handle = m.r32(0x777458 + slot * 4);
        set(m, Reg::Eax, handle);
        if handle == 0 {
            set(m, Reg::Eax, u32::MAX - 1);
            return Ok(u32::MAX - 1);
        }
        let result = stack::external(m, 0x5594a7, &[handle], None, |m| e.find_close(m, handle))?;
        set(m, Reg::Eax, result);
        if result == 0 {
            set(m, Reg::Eax, u32::MAX - 2);
            return Ok(u32::MAX - 2);
        }
        write(m, 0x777458 + slot * 4, 0);
        set(m, Reg::Eax, 0);
        Ok(0)
    })
}
/// Source archive-disabled profile. The alternate archive implementation has
/// its own source state and remains an explicit dependency when enabled.
/// exe: 0x54cf50
pub(super) fn open(
    m: &mut Space,
    pattern: u32,
    destination: u32,
    directory: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if m.r32(0x7686e0) != 0 {
        return Err("NativeVmFind: archive-enabled source find state unresolved".into());
    }
    if pattern == 0 || !(0..256).any(|i| m.r8(pattern + i) == 0) {
        return Err("NativeVmFind: absent or oversized source pattern".into());
    }
    stack::function_cdecl(m, &[pattern, destination, directory], |m| {
        set(m, Reg::Eax, pattern);
        stack::alloc(m, 0x210);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        let frame = stack::sp(m);
        let local_pattern = frame + 0x18;
        let local_name = frame + 0x118;
        let flag = frame + 0x14;
        stack::push(m, pattern);
        set(m, Reg::Ecx, local_pattern);
        stack::push(m, local_pattern);
        set(m, Reg::Esi, local_pattern);
        stack::call(m, 0x54cf6b, &[], None, |m| {
            strcpy(m, local_pattern, pattern)
        });
        low(m, Reg::Eax, m.r8(local_pattern));
        stack::add_sp(m, 8);
        let mut cursor = local_pattern;
        while m.r8(cursor) != 0 {
            if m.r8(cursor) == b'/' {
                m.w8(cursor, b'\\');
            }
            cursor += 1;
            low(m, Reg::Eax, m.r8(cursor));
            set(m, Reg::Esi, cursor);
        }
        set(m, Reg::Eax, 0);
        set(m, Reg::Edx, flag);
        stack::push(m, flag);
        set(m, Reg::Eax, local_name);
        stack::push(m, local_name);
        set(m, Reg::Ecx, local_pattern);
        stack::push(m, local_pattern);
        let windows = stack::call(m, 0x54cfc8, &[], None, |m| {
            first(m, local_pattern, local_name, flag, e)
        })?;
        stack::add_sp(m, 12);
        m.w32(frame + 0x10, windows);
        if (windows as i32) < 0 {
            return Ok(windows);
        }
        m.w8(frame + 0xb, 0);
        set(m, Reg::Esi, 0);
        m.w32(frame + 0xc, 0);
        set(m, Reg::Eax, 0x7663d8);
        let mut slot = 0;
        while slot < 32 && m.r32(0x7663d8 + slot * 4) != 0 {
            slot += 1;
            set(m, Reg::Eax, 0x7663d8 + slot * 4);
            set(m, Reg::Esi, slot);
        }
        m.w32(frame + 0xc, slot);
        if slot == 32 {
            set(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        stack::push(m, 13000);
        set(m, Reg::Ebx, 50);
        let allocation = stack::external(m, 0x54d022, &[], None, |m| e.allocate(m, 13000))?;
        set(m, Reg::Eax, allocation);
        stack::add_sp(m, 4);
        write(m, 0x7663d8 + slot * 4, allocation);
        if allocation == 0 {
            set(m, Reg::Eax, u32::MAX);
            return Ok(u32::MAX);
        }
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, 0);
        set(m, Reg::Esi, 0);
        set(m, Reg::Ebp, 13000);
        let mut count = 0;
        let mut capacity = 50;
        let mut allocation_bytes = 13000;
        loop {
            if count == capacity {
                set(m, Reg::Edx, slot);
                let old = m.r32(0x7663d8 + slot * 4);
                set(m, Reg::Eax, old);
                let previous = allocation_bytes;
                allocation_bytes += 13000;
                set(m, Reg::Ebp, allocation_bytes);
                stack::push(m, allocation_bytes);
                stack::push(m, old);
                capacity += 50;
                set(m, Reg::Ebx, capacity);
                let pointer = stack::external(m, 0x54d066, &[], None, |m| {
                    e.reallocate(m, old, previous, allocation_bytes)
                })?;
                set(m, Reg::Eax, pointer);
                set(m, Reg::Ecx, slot);
                stack::add_sp(m, 8);
                write(m, 0x7663d8 + slot * 4, pointer);
                if pointer == 0 {
                    return Err("NativeVmFind: source find-record realloc failure".into());
                }
            }
            set(m, Reg::Eax, slot);
            let records = m.r32(0x7663d8 + slot * 4);
            set(m, Reg::Ecx, records);
            set(m, Reg::Edx, local_name);
            stack::push(m, local_name);
            let output = records + count * 260;
            set(m, Reg::Ecx, output);
            stack::push(m, output);
            stack::call(m, 0x54d08f, &[], None, |m| strcpy(m, output, local_name));
            set(m, Reg::Edx, slot);
            let records = m.r32(0x7663d8 + slot * 4);
            set(m, Reg::Eax, records);
            let directory = m.r32(flag);
            set(m, Reg::Ecx, directory);
            stack::add_sp(m, 8);
            write(m, records + count * 260 + 256, directory);
            low(m, Reg::Eax, 0);
            set(m, Reg::Ecx, windows);
            count += 1;
            set(m, Reg::Edi, count);
            set(m, Reg::Esi, count * 260);
            set(m, Reg::Edx, flag);
            stack::push(m, flag);
            set(m, Reg::Eax, local_name);
            stack::push(m, local_name);
            stack::push(m, windows);
            let result = stack::call(m, 0x54d0d5, &[], None, |m| {
                next(m, windows, local_name, flag, e)
            })?;
            stack::add_sp(m, 12);
            if result != 0 {
                break;
            }
        }
        low(m, Reg::Eax, 0);
        set(m, Reg::Eax, windows);
        stack::push(m, windows);
        stack::call(m, 0x54d0fe, &[], None, |m| close(m, windows, e))?;
        set(m, Reg::Esi, slot);
        let records = m.r32(0x7663d8 + slot * 4);
        set(m, Reg::Ecx, records);
        stack::add_sp(m, 4);
        stack::push(m, 0x54d1a0);
        stack::push(m, 260);
        stack::push(m, count);
        stack::push(m, records);
        stack::call(m, 0x54d11d, &[], None, |m| {
            sorting::sort(m, records, count, 260, 0x54d1a0)
        })?;
        let records = m.r32(0x7663d8 + slot * 4);
        set(m, Reg::Eax, records);
        stack::add_sp(m, 16);
        write(m, 0x768660 + slot * 4, count);
        write(m, 0x7685e0 + slot * 4, 0);
        if records != 0 && count > 0 {
            set(m, Reg::Edx, destination);
            stack::push(m, records);
            stack::push(m, destination);
            stack::call(m, 0x54d14f, &[], None, |m| strcpy(m, destination, records));
            let index = m.r32(0x7685e0 + slot * 4);
            set(m, Reg::Eax, index);
            set(m, Reg::Edx, records);
            set(m, Reg::Ecx, index * 65);
            let value = m.r32(records + index * 260 + 256);
            set(m, Reg::Eax, value);
            set(m, Reg::Ecx, directory);
            write(m, directory, value);
            set(m, Reg::Eax, index);
            stack::add_sp(m, 8);
            set(m, Reg::Eax, index + 1);
            write(m, 0x7685e0 + slot * 4, index + 1);
        }
        stack::add_sp(m, 4);
        set(m, Reg::Eax, slot);
        stack::add_sp(m, 4);
        Ok(slot)
    })
}

#[cfg(test)]
#[path = "native_vm_find_tests.rs"]
mod tests;
