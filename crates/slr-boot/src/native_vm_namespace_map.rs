//! Source package-tree mapping and directory traversal. Physical nodes, path
//! records and child arrays come from the actual allocator/caller chronology.
use super::*;
fn reg(m: &Space, r: Reg) -> u32 {
    stack::reg(m, r)
}
fn pop(m: &mut Space, r: Reg) {
    set(m, r, stack::load(m, 0));
    stack::add_sp(m, 4)
}
fn allocate(m: &mut Space, pc: u32, n: u32, e: &mut impl Environment) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.allocate(m, n))?;
    set(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmNamespaceMap: allocation failure outside ordinary success".into());
    }
    Ok(p)
}
fn reallocate(
    m: &mut Space,
    pc: u32,
    old: u32,
    old_n: u32,
    n: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.reallocate(m, old, old_n, n))?;
    set(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmNamespaceMap: reallocation failure".into());
    }
    Ok(p)
}
fn pairs(m: &mut Space, a: Reg, b: Reg, value: Reg, copy: Reg, result: Reg) -> u32 {
    loop {
        for offset in 0..2 {
            let av = m.r8(reg(m, a) + offset);
            let bv = m.r8(reg(m, b) + offset);
            low(m, value, av);
            low(m, copy, av);
            if av != bv {
                let out = if av < bv { u32::MAX } else { 1 };
                set(m, result, out);
                return out;
            }
            if av == 0 {
                set(m, result, 0);
                return 0;
            }
        }
        set(m, a, reg(m, a) + 2);
        set(m, b, reg(m, b) + 2);
    }
}
//551160 ->554950, including volatile high bytes left by byte-register stores.
pub(super) fn character(m: &mut Space, source: u32, ch: u32) -> u32 {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, ch);
        set(m, Reg::Ecx, source);
        stack::push(m, ch);
        stack::push(m, source);
        let result = stack::call(m, 0x55116f, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, source);
                let mut b = m.r8(source);
                low(m, Reg::Ecx, b);
                low(m, Reg::Edx, ch as u8);
                let mut p = source;
                while b != 0 {
                    if b == ch as u8 {
                        return p;
                    }
                    p += 1;
                    b = m.r8(p);
                    low(m, Reg::Ecx, b);
                    set(m, Reg::Eax, p);
                }
                set(m, Reg::Ecx, if ch as u8 != 0 { 0 } else { u32::MAX });
                set(m, Reg::Eax, p & reg(m, Reg::Ecx));
                reg(m, Reg::Eax)
            })
        });
        stack::add_sp(m, 8);
        result
    })
}
pub(super) fn compare_prefix(m: &mut Space, a: u32, b: u32, n: u32) -> u32 {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Ebp);
        set(m, Reg::Ebp, stack::sp(m));
        stack::push_reg(m, Reg::Edi);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Ecx, n);
        if n != 0 {
            set(m, Reg::Ebx, n);
            set(m, Reg::Edi, a);
            set(m, Reg::Esi, a);
            set(m, Reg::Eax, 0);
            let mut count = 0;
            while count < n {
                let byte = m.r8(a + count);
                count += 1;
                if byte == 0 {
                    break;
                }
            }
            set(m, Reg::Ecx, count);
            set(m, Reg::Edi, a);
            set(m, Reg::Esi, b);
            let mut offset = 0;
            let mut av = 0;
            let mut bv = 0;
            while offset < count {
                av = m.r8(a + offset);
                bv = m.r8(b + offset);
                offset += 1;
                set(m, Reg::Edi, a + offset);
                set(m, Reg::Esi, b + offset);
                set(m, Reg::Ecx, count - offset);
                if av != bv {
                    break;
                }
            }
            low(m, Reg::Eax, bv);
            set(
                m,
                Reg::Ecx,
                if av < bv {
                    u32::MAX
                } else if av > bv {
                    1
                } else {
                    0
                },
            );
        }
        let result = reg(m, Reg::Ecx);
        set(m, Reg::Eax, result);
        pop(m, Reg::Ebx);
        pop(m, Reg::Esi);
        pop(m, Reg::Edi);
        stack::set_sp(m, reg(m, Reg::Ebp));
        pop(m, Reg::Ebp);
        result
    })
}
pub(super) fn append(m: &mut Space, dst: u32, src: u32, cap: u32) {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, cap);
        set(m, Reg::Ecx, src);
        set(m, Reg::Edx, dst);
        stack::push(m, cap);
        stack::push(m, src);
        stack::push(m, dst);
        stack::call(m, 0x551154, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, dst);
                stack::push_reg(m, Reg::Ebx);
                set(m, Reg::Ebx, cap);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                stack::push_reg(m, Reg::Edi);
                set(m, Reg::Edi, 1);
                set(m, Reg::Ecx, dst);
                let mut p = dst;
                if m.r8(dst) != 0 {
                    loop {
                        let index = reg(m, Reg::Edi);
                        set(m, Reg::Edx, index);
                        set(m, Reg::Edi, index + 1);
                        if index as i32 >= cap as i32 {
                            break;
                        }
                        p += 1;
                        low(m, Reg::Edx, m.r8(p));
                        set(m, Reg::Ecx, p);
                        if m.r8(p) == 0 {
                            break;
                        }
                    }
                }
                set(m, Reg::Esi, src);
                let mut s = src;
                let mut ch = m.r8(s);
                low(m, Reg::Edx, ch);
                while ch != 0 {
                    let index = reg(m, Reg::Edi);
                    set(m, Reg::Ebp, index);
                    set(m, Reg::Edi, index + 1);
                    if index as i32 >= cap as i32 {
                        break;
                    }
                    observe(p, 1, reg(m, Reg::Edx));
                    m.w8(p, ch);
                    s += 1;
                    ch = m.r8(s);
                    low(m, Reg::Edx, ch);
                    p += 1;
                    set(m, Reg::Ecx, p);
                    set(m, Reg::Esi, s);
                }
                pop(m, Reg::Edi);
                pop(m, Reg::Esi);
                pop(m, Reg::Ebp);
                byte(m, p, 0);
                pop(m, Reg::Ebx);
            })
        });
        stack::add_sp(m, 12);
    })
}
pub(super) fn grow(
    m: &mut Space,
    header: u32,
    cap: u32,
    e: &mut impl Environment,
) -> Result<(), String> {
    stack::function(m, &[cap], Some(header), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, cap);
        set(m, Reg::Esi, header);
        if cap == 0 {
            let old = m.r32(header + 4);
            set(m, Reg::Eax, old);
            stack::push(m, old);
            let result = stack::external(m, 0x405195, &[], None, |m| e.free(m, old))?;
            set(m, Reg::Eax, result);
            stack::add_sp(m, 4);
            write(m, header + 4, 0)
        } else {
            let old = m.r32(header + 4);
            set(m, Reg::Edx, old);
            set(m, Reg::Ecx, cap * 4);
            stack::push(m, cap * 4);
            stack::push(m, old);
            let p = reallocate(m, 0x4051ae, old, m.r32(header) * 4, cap * 4, e)?;
            stack::add_sp(m, 8);
            write(m, header + 4, p)
        }
        if (cap as i32) < (m.r32(header + 8) as i32) {
            write(m, header + 8, cap)
        }
        write(m, header, cap);
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        Ok(())
    })
}
/// Actual duplicate-path walk41d410. Names are compared through live buffers;
/// no semantic path table or payload equality supplies raw owner addresses.
fn path_exists(m: &mut Space, this: u32, skip: u32, path: u32) -> Result<u32, String> {
    stack::function(m, &[skip, path], Some(this), |m| {
        stack::push_reg(m, Reg::Ecx);
        set(m, Reg::Eax, skip);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        set(m, Reg::Ebx, this);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::store(m, 16, this);
        let mut found = false;
        if this != skip {
            let paths = m.r32(this + 8);
            set(m, Reg::Ebp, paths);
            if paths != 0 {
                let count = m.r32(paths + 8);
                set(m, Reg::Edx, count);
                set(m, Reg::Edi, 0);
                for index in 0..count {
                    let cell = if index < m.r32(paths) {
                        let data = m.r32(paths + 4);
                        set(m, Reg::Eax, data);
                        m.r32(data + index * 4)
                    } else {
                        0
                    };
                    if cell == 0 {
                        return Err("NativeVmNamespaceMap: null existing path cell".into());
                    }
                    set(m, Reg::Eax, cell);
                    set(m, Reg::Esi, path);
                    set(m, Reg::Eax, m.r32(cell));
                    if pairs(m, Reg::Eax, Reg::Esi, Reg::Ebx, Reg::Ecx, Reg::Eax) == 0 {
                        found = true;
                        break;
                    }
                    set(m, Reg::Edi, index + 1);
                }
                if !found {
                    set(m, Reg::Ebx, this)
                }
            }
            if !found {
                let children = m.r32(this + 4);
                set(m, Reg::Eax, children);
                if children != 0 {
                    let count = m.r32(children + 8);
                    set(m, Reg::Ecx, count);
                    set(m, Reg::Esi, 0);
                    for index in 0..count {
                        let child = if index < m.r32(children) {
                            set(m, Reg::Ecx, m.r32(children + 4));
                            m.r32(m.r32(children + 4) + index * 4)
                        } else {
                            0
                        };
                        if child == 0 {
                            return Err("NativeVmNamespaceMap: null existing child".into());
                        }
                        set(m, Reg::Ecx, child);
                        set(m, Reg::Edx, path);
                        set(m, Reg::Eax, skip);
                        let result = stack::call(m, 0x41d4af, &[skip, path], Some(child), |m| {
                            path_exists(m, child, skip, path)
                        })?;
                        if result != 0 {
                            found = true;
                            break;
                        }
                        set(m, Reg::Eax, m.r32(this + 4));
                        set(m, Reg::Ecx, m.r32(m.r32(this + 4) + 8));
                        set(m, Reg::Esi, index + 1);
                    }
                }
            }
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        pop(m, Reg::Ebp);
        set(m, Reg::Eax, u32::from(found));
        pop(m, Reg::Ebx);
        pop(m, Reg::Ecx);
        Ok(u32::from(found))
    })
}

/// Source41cfe0 namespace mapping, preserving each recursive CALL frame.
pub(super) fn map(
    m: &mut Space,
    this: u32,
    path: u32,
    name: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if this == 0 || name == 0 {
        return Err("NativeVmNamespaceMap: absent actual package/name".into());
    }
    stack::function(m, &[path, name], Some(this), |m| {
        stack::alloc(m, 0x100);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        set(m, Reg::Ebp, name);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::push(m, 46);
        stack::push(m, name);
        set(m, Reg::Ebx, this);
        let dot = stack::call(m, 0x41cffb, &[], None, |m| character(m, name, 46));
        set(m, Reg::Esi, dot);
        stack::add_sp(m, 8);
        let result = if dot == 0 {
            set(m, Reg::Esi, m.r32(this));
            set(m, Reg::Eax, name);
            if pairs(m, Reg::Eax, Reg::Esi, Reg::Edx, Reg::Ecx, Reg::Eax) == 0 {
                set(m, Reg::Eax, path);
                stack::call(m, 0x41d272, &[path], Some(this), |m| {
                    attach_path(m, this, path, e)
                })?
            } else {
                set(m, Reg::Eax, 0);
                0
            }
        } else {
            let current_name = m.r32(this);
            set(m, Reg::Edx, current_name);
            set(m, Reg::Ecx, u32::MAX);
            set(m, Reg::Eax, 0);
            set(m, Reg::Edi, current_name);
            let n = scan(m, current_name)? - 1;
            set(m, Reg::Ecx, n);
            set(m, Reg::Edi, n);
            stack::push(m, n);
            stack::push(m, current_name);
            stack::push(m, name);
            let comparison = stack::call(m, 0x41d020, &[], None, |m| {
                compare_prefix(m, name, current_name, n)
            });
            stack::add_sp(m, 12);
            if comparison != 0 {
                set(m, Reg::Eax, 0);
                0
            } else {
                set(m, Reg::Eax, dot - name);
                if n != dot - name {
                    set(m, Reg::Eax, 0);
                    0
                } else {
                    let rest = dot + 1;
                    set(m, Reg::Esi, rest);
                    stack::push(m, 46);
                    stack::push(m, rest);
                    let next_dot = stack::call(m, 0x41d040, &[], None, |m| character(m, rest, 46));
                    let children = m.r32(this + 4);
                    set(m, Reg::Ecx, children);
                    let count = m.r32(children + 8);
                    set(m, Reg::Edi, count);
                    stack::add_sp(m, 8);
                    set(m, Reg::Edi, count.wrapping_sub(1));
                    set(m, Reg::Ebp, next_dot);
                    let mut found = 0;
                    for index in (0..count).rev() {
                        set(m, Reg::Eax, children);
                        let child = if index < m.r32(children) {
                            let data = m.r32(children + 4);
                            set(m, Reg::Edx, data);
                            m.r32(data + index * 4)
                        } else {
                            0
                        };
                        set(m, Reg::Ecx, child);
                        set(m, Reg::Eax, path);
                        found = stack::call(m, 0x41d06d, &[path, rest], Some(child), |m| {
                            map(m, child, path, rest, e)
                        })?;
                        if found != 0 {
                            break;
                        }
                        set(m, Reg::Edi, index.wrapping_sub(1));
                    }
                    if found != 0 {
                        found
                    } else {
                        let multiline = next_dot != 0;
                        let source = if multiline {
                            let cap = next_dot - rest + 1;
                            set(m, Reg::Ebp, cap);
                            stack::push(m, cap);
                            let local = stack::sp(m) + 20;
                            set(m, Reg::Ecx, local);
                            stack::push(m, rest);
                            stack::push(m, local);
                            stack::call(m, 0x41d08f, &[], None, |m| {
                                bounded_copy(m, local, rest, cap)
                            });
                            local
                        } else {
                            rest
                        };
                        stack::push(m, 16);
                        let child =
                            allocate(m, if multiline { 0x41d096 } else { 0x41d153 }, 16, e)?;
                        set(m, Reg::Ebp, child);
                        stack::add_sp(m, if multiline { 16 } else { 4 });
                        let owner = m.r32(this + 12);
                        set(m, Reg::Eax, if owner == 0 { child } else { owner });
                        write(m, child + 12, reg(m, Reg::Eax));
                        set(m, Reg::Ecx, u32::MAX);
                        set(m, Reg::Eax, 0);
                        set(m, Reg::Edi, source);
                        let len = scan(m, source)?;
                        stack::push(m, len);
                        let buffer =
                            allocate(m, if multiline { 0x41d0be } else { 0x41d179 }, len, e)?;
                        set(m, Reg::Edx, buffer);
                        set(m, Reg::Ecx, u32::MAX);
                        set(m, Reg::Eax, 0);
                        write(m, child, buffer);
                        set(m, Reg::Edi, source);
                        let len = scan(m, source)?;
                        stack::push(m, len);
                        if multiline {
                            set(m, Reg::Eax, source)
                        }
                        stack::push(m, source);
                        stack::push(m, buffer);
                        stack::call(
                            m,
                            if multiline { 0x41d0dc } else { 0x41d191 },
                            &[],
                            None,
                            |m| bounded_copy(m, buffer, source, len),
                        );
                        stack::push(m, 12);
                        let list = allocate(m, if multiline { 0x41d0e3 } else { 0x41d198 }, 12, e)?;
                        set(m, Reg::Ecx, 0);
                        stack::add_sp(m, 20);
                        for off in [0, 8, 4] {
                            write(m, list + off, 0)
                        }
                        write(m, child + 4, list);
                        if multiline {
                            write(m, child + 8, 0)
                        } else {
                            set(m, Reg::Eax, path);
                            write(m, child + 8, 0);
                            if path != 0 {
                                stack::call(m, 0x41d1c6, &[path], Some(child), |m| {
                                    attach_path(m, child, path, e)
                                })?;
                            }
                        }
                        let parent_children = m.r32(this + 4);
                        if multiline {
                            set(m, Reg::Edi, parent_children);
                            let old_count = m.r32(parent_children + 8);
                            let capacity = m.r32(parent_children);
                            set(m, Reg::Eax, old_count + 1);
                            set(m, Reg::Ecx, capacity);
                            if old_count + 1 > capacity {
                                set(m, Reg::Ecx, capacity + old_count + 1);
                                stack::call(
                                    m,
                                    0x41d11f,
                                    &[capacity + old_count + 1],
                                    Some(parent_children),
                                    |m| grow(m, parent_children, capacity + old_count + 1, e),
                                )?;
                            }
                            let old_count = m.r32(parent_children + 8);
                            let data = m.r32(parent_children + 4);
                            set(m, Reg::Ecx, old_count);
                            set(m, Reg::Edx, data);
                            set(m, Reg::Eax, path);
                            write(m, data + old_count * 4, child);
                            set(m, Reg::Ebx, old_count + 1);
                            set(m, Reg::Ecx, child);
                            write(m, parent_children + 8, old_count + 1);
                            stack::call(m, 0x41d13f, &[path, rest], Some(child), |m| {
                                map(m, child, path, rest, e)
                            })?
                        } else {
                            set(m, Reg::Esi, parent_children);
                            let old_count = m.r32(parent_children + 8);
                            let capacity = m.r32(parent_children);
                            set(m, Reg::Eax, old_count + 1);
                            set(m, Reg::Ecx, capacity);
                            if old_count + 1 > capacity {
                                let cap = capacity + old_count + 1;
                                set(m, Reg::Edi, cap);
                                let old = m.r32(parent_children + 4);
                                set(m, Reg::Eax, old);
                                set(m, Reg::Edx, cap * 4);
                                stack::push(m, cap * 4);
                                stack::push(m, old);
                                let data = reallocate(m, 0x41d200, old, capacity * 4, cap * 4, e)?;
                                stack::add_sp(m, 8);
                                write(m, parent_children + 4, data);
                                if cap < m.r32(parent_children + 8) {
                                    write(m, parent_children + 8, cap)
                                }
                                write(m, parent_children, cap);
                            }
                            let old_count = m.r32(parent_children + 8);
                            let data = m.r32(parent_children + 4);
                            set(m, Reg::Ecx, old_count);
                            set(m, Reg::Edx, data);
                            write(m, data + old_count * 4, child);
                            set(m, Reg::Eax, old_count + 1);
                            write(m, parent_children + 8, old_count + 1);
                            set(m, Reg::Eax, 1);
                            1
                        }
                    }
                }
            }
        };
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        pop(m, Reg::Ebp);
        pop(m, Reg::Ebx);
        stack::add_sp(m, 0x100);
        Ok(result)
    })
}
/// Actual41cdc0 path ownership, duplicate walk and recursively found children.
pub(super) fn attach_path(
    m: &mut Space,
    this: u32,
    path: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if this == 0 || path == 0 {
        return Err("NativeVmNamespaceMap: missing actual path attachment".into());
    }
    stack::function(m, &[path], Some(this), |m| {
        stack::push_reg(m, Reg::Ecx);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        set(m, Reg::Ebp, path);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Ebx, this);
        let root = m.r32(this + 12);
        set(m, Reg::Ecx, root);
        stack::store(m, 16, this);
        let duplicate = stack::call(m, 0x41cdd9, &[this, path], Some(root), |m| {
            path_exists(m, root, this, path)
        })?;
        if duplicate != 0 {
            pop(m, Reg::Edi);
            pop(m, Reg::Esi);
            pop(m, Reg::Ebp);
            set(m, Reg::Eax, u32::MAX);
            pop(m, Reg::Ebx);
            pop(m, Reg::Ecx);
            return Ok(u32::MAX);
        }
        let mut paths = m.r32(this + 8);
        set(m, Reg::Edx, paths);
        set(m, Reg::Esi, 0);
        if paths == 0 {
            stack::push(m, 12);
            paths = allocate(m, 0x41cdf8, 12, e)?;
            stack::add_sp(m, 4);
            for off in [0, 8, 4] {
                write(m, paths + off, 0)
            }
            write(m, this + 8, paths)
        } else {
            let count = m.r32(paths + 8);
            set(m, Reg::Edi, count.wrapping_sub(1));
            let mut found = false;
            for index in (0..count).rev() {
                let record = if index < m.r32(paths) {
                    let data = m.r32(paths + 4);
                    set(m, Reg::Eax, data);
                    m.r32(data + index * 4)
                } else {
                    0
                };
                if record == 0 {
                    return Err("NativeVmNamespaceMap: missing owned path record".into());
                }
                set(m, Reg::Eax, m.r32(record));
                set(m, Reg::Esi, path);
                if pairs(m, Reg::Eax, Reg::Esi, Reg::Ebx, Reg::Ecx, Reg::Eax) == 0 {
                    found = true;
                    break;
                }
                set(m, Reg::Edi, index.wrapping_sub(1));
            }
            if found {
                pop(m, Reg::Edi);
                pop(m, Reg::Esi);
                pop(m, Reg::Ebp);
                set(m, Reg::Eax, 1);
                pop(m, Reg::Ebx);
                pop(m, Reg::Ecx);
                return Ok(1);
            }
            set(m, Reg::Ebx, this);
        }
        stack::push(m, 8);
        let record = allocate(m, 0x41ce6a, 8, e)?;
        set(m, Reg::Esi, record);
        stack::add_sp(m, 4);
        set(m, Reg::Ecx, u32::MAX);
        set(m, Reg::Eax, 0);
        set(m, Reg::Edi, path);
        let len = scan(m, path)?;
        stack::push(m, len);
        let buffer = allocate(m, 0x41ce88, len, e)?;
        set(m, Reg::Edx, buffer);
        set(m, Reg::Ecx, u32::MAX);
        set(m, Reg::Eax, 0);
        write(m, record, buffer);
        set(m, Reg::Edi, path);
        let len = scan(m, path)?;
        stack::push(m, len);
        stack::push(m, path);
        stack::push(m, buffer);
        stack::call(m, 0x41ce9f, &[], None, |m| {
            bounded_copy(m, buffer, path, len)
        });
        stack::add_sp(m, 16);
        write(m, record + 4, 0);
        set(m, Reg::Edi, record);
        let paths = m.r32(this + 8);
        set(m, Reg::Esi, paths);
        let count = m.r32(paths + 8);
        let capacity = m.r32(paths);
        set(m, Reg::Eax, count + 1);
        set(m, Reg::Ecx, capacity);
        if count + 1 > capacity {
            set(m, Reg::Ecx, capacity + count + 1);
            stack::call(m, 0x41cf09, &[capacity + count + 1], Some(paths), |m| {
                grow(m, paths, capacity + count + 1, e)
            })?;
        }
        let count = m.r32(paths + 8);
        let data = m.r32(paths + 4);
        set(m, Reg::Ecx, count);
        set(m, Reg::Edx, data);
        write(m, data + count * 4, record);
        write(m, paths + 8, count + 1);
        let root = m.r32(this + 12);
        set(m, Reg::Eax, root);
        let found = stack::call(m, 0x41cf20, &[root], Some(record), |m| {
            directory_children(m, record, root, e)
        })?;
        set(m, Reg::Ebx, found);
        let count = m.r32(found + 8);
        set(m, Reg::Ebp, count.wrapping_sub(1));
        for index in (0..count).rev() {
            let child = if index < m.r32(found) {
                set(m, Reg::Ecx, m.r32(found + 4));
                let p = m.r32(m.r32(found + 4) + index * 4);
                set(m, Reg::Edx, p);
                p
            } else {
                0
            };
            stack::store(m, 24, child);
            set(m, Reg::Eax, this);
            let children = m.r32(this + 4);
            set(m, Reg::Esi, children);
            let n = m.r32(children + 8);
            let cap = m.r32(children);
            set(m, Reg::Eax, n + 1);
            set(m, Reg::Ecx, cap);
            if n + 1 > cap {
                let capacity = cap + n + 1;
                set(m, Reg::Edi, capacity);
                let old = m.r32(children + 4);
                set(m, Reg::Eax, old);
                set(m, Reg::Edx, capacity * 4);
                stack::push(m, capacity * 4);
                stack::push(m, old);
                let data = reallocate(m, 0x41cf7a, old, cap * 4, capacity * 4, e)?;
                stack::add_sp(m, 8);
                write(m, children + 4, data);
                if capacity < m.r32(children + 8) {
                    write(m, children + 8, capacity)
                }
                write(m, children, capacity);
            }
            let n = m.r32(children + 8);
            let data = m.r32(children + 4);
            set(m, Reg::Ecx, n);
            set(m, Reg::Edx, data);
            set(m, Reg::Eax, child);
            write(m, data + n * 4, child);
            set(m, Reg::Ecx, n + 1);
            set(m, Reg::Ebp, index.wrapping_sub(1));
            write(m, children + 8, n + 1);
        }
        set(m, Reg::Esi, 0);
        if found != 0 {
            let data = m.r32(found + 4);
            set(m, Reg::Ecx, data);
            stack::push(m, data);
            let result = stack::external(m, 0x41cfb0, &[], None, |m| e.free(m, data))?;
            set(m, Reg::Eax, result);
            set(m, Reg::Eax, m.r32(found + 8));
            stack::add_sp(m, 4);
            write(m, found + 4, 0);
            if m.r32(found + 8) > 0 {
                write(m, found + 8, 0)
            }
            stack::push(m, found);
            write(m, found, 0);
            let result = stack::external(m, 0x41cfc8, &[], None, |m| e.free(m, found))?;
            set(m, Reg::Eax, result);
            stack::add_sp(m, 4);
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        pop(m, Reg::Ebp);
        set(m, Reg::Eax, 1);
        pop(m, Reg::Ebx);
        pop(m, Reg::Ecx);
        Ok(1)
    })
}

fn memory_equal(m: &mut Space, left: u32, right: u32, n: u32) {
    let mut i = 0;
    while i < n {
        let same = m.r8(left + i) == m.r8(right + i);
        i += 1;
        if !same {
            break;
        }
    }
    set(m, Reg::Esi, left + i);
    set(m, Reg::Edi, right + i);
    set(m, Reg::Ecx, n - i);
}
/// Source41d9c0 discovers namespace children through the raw find slot layer.
fn directory_children(
    m: &mut Space,
    path_record: u32,
    root: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    stack::function(m, &[root], Some(path_record), |m| {
        stack::alloc(m, 0x30c);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Esi, path_record);
        let local = stack::sp(m);
        let flag = local + 16;
        let handle_cell = local + 20;
        let name = local + 28;
        let child_path = local + 0x11c;
        let pattern = local + 0x21c;
        stack::push(m, 12);
        write(m, local + 24, path_record);
        let result = allocate(m, 0x41d9d7, 12, e)?;
        set(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        for off in [0, 8, 4] {
            write(m, result + off, 0)
        }
        set(m, Reg::Ebx, result);
        let source = m.r32(path_record);
        set(m, Reg::Eax, source);
        stack::push(m, 256);
        stack::push(m, source);
        set(m, Reg::Ecx, pattern);
        stack::push(m, pattern);
        stack::call(m, 0x41da03, &[], None, |m| {
            bounded_copy(m, pattern, source, 256)
        });
        stack::push(m, 256);
        set(m, Reg::Edx, pattern);
        stack::push(m, 0x60c060);
        stack::push(m, pattern);
        stack::call(m, 0x41da1a, &[], None, |m| {
            append(m, pattern, 0x60c060, 256)
        });
        set(m, Reg::Eax, flag);
        stack::push(m, flag);
        set(m, Reg::Ecx, name);
        stack::push(m, name);
        set(m, Reg::Edx, pattern);
        stack::push(m, pattern);
        let handle = stack::call(m, 0x41da31, &[], None, |m| {
            raw_find_open(m, pattern, name, flag, e)
        })?;
        stack::add_sp(m, 36);
        write(m, handle_cell, handle);
        loop {
            set(m, Reg::Edi, 0x60c064);
            set(m, Reg::Esi, name);
            set(m, Reg::Ecx, 2);
            set(m, Reg::Eax, 0);
            memory_equal(m, name, 0x60c064, 2);
            let dot = m.r8(name) == b'.' && m.r8(name + 1) == 0;
            let mut dotdot = false;
            if !dot {
                set(m, Reg::Edi, 0x60c068);
                set(m, Reg::Esi, name);
                set(m, Reg::Ecx, 3);
                set(m, Reg::Edx, 0);
                memory_equal(m, name, 0x60c068, 3);
                dotdot = m.r8(name) == b'.' && m.r8(name + 1) == b'.' && m.r8(name + 2) == 0;
            }
            if !dot && !dotdot && m.r32(flag) != 0 {
                set(m, Reg::Eax, path_record);
                set(m, Reg::Ecx, source);
                stack::push(m, 256);
                stack::push(m, source);
                set(m, Reg::Edx, child_path);
                stack::push(m, child_path);
                stack::call(m, 0x41da8b, &[], None, |m| {
                    bounded_copy(m, child_path, source, 256)
                });
                stack::push(m, 256);
                set(m, Reg::Eax, child_path);
                stack::push(m, 0x60c06c);
                stack::push(m, child_path);
                stack::call(m, 0x41daa2, &[], None, |m| {
                    append(m, child_path, 0x60c06c, 256)
                });
                stack::push(m, 256);
                set(m, Reg::Ecx, name);
                stack::push(m, name);
                set(m, Reg::Edx, child_path);
                stack::push(m, child_path);
                stack::call(m, 0x41dab9, &[], None, |m| append(m, child_path, name, 256));
                stack::push(m, 16);
                let child = allocate(m, 0x41dac0, 16, e)?;
                set(m, Reg::Esi, child);
                stack::add_sp(m, 40);
                set(m, Reg::Eax, if root == 0 { child } else { root });
                write(m, child + 12, reg(m, Reg::Eax));
                set(m, Reg::Ecx, u32::MAX);
                set(m, Reg::Eax, 0);
                set(m, Reg::Edi, name);
                let len = scan(m, name)?;
                stack::push(m, len);
                let buffer = allocate(m, 0x41daec, len, e)?;
                set(m, Reg::Edx, buffer);
                set(m, Reg::Ecx, u32::MAX);
                set(m, Reg::Eax, 0);
                write(m, child, buffer);
                set(m, Reg::Edi, name);
                let len = scan(m, name)?;
                stack::push(m, len);
                set(m, Reg::Eax, name);
                stack::push(m, name);
                stack::push(m, buffer);
                stack::call(m, 0x41db09, &[], None, |m| {
                    bounded_copy(m, buffer, name, len)
                });
                stack::push(m, 12);
                let children = allocate(m, 0x41db10, 12, e)?;
                stack::add_sp(m, 20);
                for off in [0, 8, 4] {
                    write(m, children + off, 0)
                }
                set(m, Reg::Ecx, child_path);
                write(m, child + 4, children);
                write(m, child + 8, 0);
                set(m, Reg::Edx, child_path);
                stack::call(m, 0x41db43, &[child_path], Some(child), |m| {
                    attach_path(m, child, child_path, e)
                })?;
                let count = m.r32(result + 8);
                let capacity = m.r32(result);
                set(m, Reg::Eax, count + 1);
                set(m, Reg::Ecx, capacity);
                if count + 1 > capacity {
                    let cap = capacity + count + 1;
                    set(m, Reg::Edi, cap);
                    let old = m.r32(result + 4);
                    set(m, Reg::Edx, old);
                    set(m, Reg::Ecx, cap * 4);
                    stack::push(m, cap * 4);
                    stack::push(m, old);
                    let data = reallocate(m, 0x41db7a, old, capacity * 4, cap * 4, e)?;
                    stack::add_sp(m, 8);
                    write(m, result + 4, data);
                    if cap < m.r32(result + 8) {
                        write(m, result + 8, cap)
                    }
                    write(m, result, cap);
                }
                let count = m.r32(result + 8);
                let data = m.r32(result + 4);
                set(m, Reg::Eax, count);
                set(m, Reg::Ecx, data);
                write(m, data + count * 4, child);
                write(m, result + 8, count + 1);
            }
            set(m, Reg::Ecx, handle);
            set(m, Reg::Edx, flag);
            set(m, Reg::Eax, name);
            stack::push(m, flag);
            stack::push(m, name);
            stack::push(m, handle);
            let status = stack::call(m, 0x41dbaa, &[], None, |m| {
                raw_find_next(m, handle, name, flag)
            })?;
            stack::add_sp(m, 12);
            if (status as i32) < 0 {
                set(m, Reg::Edx, handle);
                stack::push(m, handle);
                stack::call(m, 0x41dbbf, &[], None, |m| raw_find_close(m, handle, e))?;
                stack::add_sp(m, 4);
                break;
            }
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        pop(m, Reg::Ebp);
        set(m, Reg::Eax, result);
        pop(m, Reg::Ebx);
        stack::add_sp(m, 0x30c);
        Ok(result)
    })
}
