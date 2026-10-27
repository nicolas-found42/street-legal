//! Source410890 miss and41d290 namespace resolution. Caller frames remain
//! entered at41044c; cache compilation and file loading must resume them.
use super::*;
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn pop(m: &mut Space, reg: Reg) {
    set(m, reg, stack::load(m, 0));
    stack::add_sp(m, 4);
}
pub(super) fn last_character(m: &mut Space, source: u32, ch: u32) -> u32 {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, ch);
        set(m, Reg::Ecx, source);
        stack::push(m, ch);
        stack::push(m, source);
        let result = stack::call(m, 0x55118f, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Ecx, source);
                low(m, Reg::Edx, m.r8(source));
                stack::push_reg(m, Reg::Esi);
                set(m, Reg::Esi, 0);
                let mut p = source;
                if m.r8(p) != 0 {
                    low(m, Reg::Eax, ch as u8);
                    loop {
                        if m.r8(p) == ch as u8 {
                            set(m, Reg::Esi, p);
                        }
                        low(m, Reg::Edx, m.r8(p + 1));
                        p += 1;
                        set(m, Reg::Ecx, p);
                        if m.r8(p) == 0 {
                            break;
                        }
                    }
                }
                set(m, Reg::Eax, r(m, Reg::Esi));
                pop(m, Reg::Esi);
                r(m, Reg::Eax)
            })
        });
        stack::add_sp(m, 8);
        result
    })
}
fn repeated_equal(m: &mut Space, a: u32, b: u32, n: u32) -> bool {
    set(m, Reg::Esi, a);
    set(m, Reg::Edi, b);
    set(m, Reg::Ecx, n);
    for i in 0..n {
        let equal = m.r8(a + i) == m.r8(b + i);
        set(m, Reg::Esi, a + i + 1);
        set(m, Reg::Edi, b + i + 1);
        set(m, Reg::Ecx, n - i - 1);
        if !equal {
            return false;
        }
    }
    true
}
fn owned_name(
    m: &mut Space,
    item: u32,
    source: u32,
    n: u32,
    e: &mut impl Environment,
) -> Result<(), String> {
    stack::function(m, &[source, n], Some(item), |m| {
        set(m, Reg::Eax, n);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, n + 1);
        stack::push(m, n + 1);
        set(m, Reg::Esi, item);
        let p = stack::external(m, 0x41ab31, &[], None, |m| e.allocate(m, n + 1))?;
        set(m, Reg::Eax, p);
        set(m, Reg::Ecx, source);
        stack::push(m, n + 1);
        stack::push(m, source);
        stack::push(m, p);
        write(m, item, p);
        stack::call(m, 0x41ab3f, &[], None, |m| {
            bounded_copy(m, p, source, n + 1)
        });
        stack::add_sp(m, 16);
        pop(m, Reg::Edi);
        set(m, Reg::Eax, item);
        pop(m, Reg::Esi);
        Ok(())
    })
}
fn duplicate_name(m: &mut Space, names: u32, path: u32, name: u32) -> Result<bool, String> {
    for index in 0..m.r32(names + 8) {
        set(m, Reg::Eax, 0);
        let n = scan(m, name)?;
        set(m, Reg::Eax, m.r32(names));
        set(m, Reg::Ecx, n - 1);
        set(m, Reg::Edx, n - 7);
        if index >= m.r32(names) {
            return Err("NativeVmNamespace: malformed duplicate inventory".into());
        }
        set(m, Reg::Eax, m.r32(names + 4));
        set(m, Reg::Eax, m.r32(r(m, Reg::Eax) + index * 4));
        let existing = m.r32(r(m, Reg::Eax));
        set(m, Reg::Esi, existing);
        set(m, Reg::Eax, 0);
        let other = scan(m, existing)?;
        set(m, Reg::Ecx, other - 1);
        if other - 1 == n - 7 {
            stack::push(m, n - 7);
            set(m, Reg::Ecx, name);
            stack::push(m, name);
            stack::push(m, existing);
            let result = stack::call(m, 0x41d8e1, &[], None, |m| {
                mapping::compare_prefix(m, existing, name, n - 7)
            });
            stack::add_sp(m, 12);
            if result == 0 {
                return Ok(true);
            }
            set(m, Reg::Ebp, path);
        }
        set(m, Reg::Esi, names);
        set(m, Reg::Eax, m.r32(names + 8));
        set(m, Reg::Ebx, index + 1);
    }
    Ok(false)
}
fn inventory(m: &mut Space, path: u32, e: &mut impl Environment) -> Result<(), String> {
    stack::function(m, &[], Some(path), |m| {
        stack::alloc(m, 0x20c);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, path);
        stack::push(m, 12);
        stack::store(m, 0xc, path);
        let names = stack::external(m, 0x41d6a4, &[], None, |m| e.allocate(m, 12))?;
        set(m, Reg::Eax, names);
        set(m, Reg::Ecx, 0);
        stack::add_sp(m, 4);
        if names == 0 {
            return Err("NativeVmNamespace: name inventory allocation failed".into());
        }
        for off in [0, 8, 4] {
            write(m, names + off, 0);
        }
        write(m, path + 4, names);
        set(m, Reg::Eax, m.r32(path));
        if m.r32(path) == 0 {
            return Ok(());
        }
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Edi);
        let b = stack::sp(m);
        let name = b + 0x1c;
        let pattern = b + 0x11c;
        let directory = b + 0x10;
        stack::push(m, 0x100);
        stack::push(m, m.r32(path));
        stack::push(m, pattern);
        stack::call(m, 0x41d6dc, &[], None, |m| {
            bounded_copy(m, pattern, m.r32(path), 0x100)
        });
        stack::push(m, 0x100);
        stack::push(m, 0x60c028);
        stack::push(m, pattern);
        stack::call(m, 0x41d6f3, &[], None, |m| {
            mapping::append(m, pattern, 0x60c028, 0x100)
        });
        stack::push(m, directory);
        stack::push(m, name);
        stack::push(m, pattern);
        let handle = stack::call(m, 0x41d70a, &[], None, |m| {
            raw_find_open(m, pattern, name, directory, e)
        })?;
        stack::add_sp(m, 0x24);
        set(m, Reg::Ebp, handle);
        loop {
            set(m, Reg::Edx, 0);
            let dot = repeated_equal(m, name, 0x60c034, 2);
            let skip = if dot {
                true
            } else {
                set(m, Reg::Eax, 0);
                repeated_equal(m, name, 0x60c038, 3)
            };
            if !skip {
                set(m, Reg::Eax, m.r32(directory));
                if m.r32(directory) == 0 {
                    set(m, Reg::Ecx, name);
                    stack::push(m, 0x2e);
                    stack::push(m, name);
                    let extension =
                        stack::call(m, 0x41d74f, &[], None, |m| last_character(m, name, 0x2e));
                    set(m, Reg::Ebx, extension);
                    stack::add_sp(m, 8);
                    if extension != 0 {
                        set(m, Reg::Edx, 0);
                        if repeated_equal(m, extension, 0x60c03c, 6) {
                            return Err("NativeVmNamespace: Java source inventory requires compilation owner".into());
                        }
                    }
                }
            }
            set(m, Reg::Eax, directory);
            stack::push(m, directory);
            set(m, Reg::Ecx, name);
            stack::push(m, name);
            stack::push(m, handle);
            let next = stack::call(m, 0x41d7c8, &[], None, |m| {
                raw_find_next(m, handle, name, directory)
            })?;
            stack::add_sp(m, 12);
            if (next as i32) < 0 {
                break;
            }
        }
        stack::push(m, handle);
        stack::call(m, 0x41d7d9, &[], None, |m| raw_find_close(m, handle, e))?;
        set(m, Reg::Edx, path);
        set(m, Reg::Eax, m.r32(path));
        stack::push(m, 0x100);
        stack::push(m, m.r32(path));
        stack::push(m, pattern);
        stack::call(m, 0x41d7f2, &[], None, |m| {
            bounded_copy(m, pattern, m.r32(path), 0x100)
        });
        stack::push(m, 0x100);
        stack::push(m, 0x60c044);
        stack::push(m, pattern);
        stack::call(m, 0x41d809, &[], None, |m| {
            mapping::append(m, pattern, 0x60c044, 0x100)
        });
        stack::push(m, directory);
        stack::push(m, name);
        stack::push(m, pattern);
        let handle = stack::call(m, 0x41d820, &[], None, |m| {
            raw_find_open(m, pattern, name, directory, e)
        })?;
        stack::add_sp(m, 0x28);
        m.w32(b + 0x18, handle);
        loop {
            set(m, Reg::Eax, 0);
            let dot = repeated_equal(m, name, 0x60c050, 2);
            let skip = if dot {
                true
            } else {
                set(m, Reg::Edx, 0);
                repeated_equal(m, name, 0x60c054, 3)
            };
            if !skip {
                set(m, Reg::Eax, m.r32(directory));
                if m.r32(directory) == 0 {
                    set(m, Reg::Eax, name);
                    stack::push(m, 0x2e);
                    stack::push(m, name);
                    let extension =
                        stack::call(m, 0x41d86f, &[], None, |m| last_character(m, name, 0x2e));
                    stack::add_sp(m, 8);
                    if extension != 0 {
                        set(m, Reg::Edx, 0);
                        if repeated_equal(m, extension, 0x60c058, 7) {
                            set(m, Reg::Ebp, path);
                            set(m, Reg::Esi, names);
                            set(m, Reg::Eax, m.r32(names + 8));
                            set(m, Reg::Ebx, 0);
                            if !duplicate_name(m, names, path, name)? {
                                stack::push(m, 4);
                                let item =
                                    stack::external(m, 0x41d902, &[], None, |m| e.allocate(m, 4))?;
                                set(m, Reg::Edx, item);
                                stack::add_sp(m, 4);
                                if item == 0 {
                                    return Err(
                                        "NativeVmNamespace: class name allocation failed".into()
                                    );
                                }
                                set(m, Reg::Eax, 0);
                                let n = scan(m, name)?;
                                set(m, Reg::Ecx, n - 7);
                                set(m, Reg::Eax, name);
                                stack::call(m, 0x41d928, &[name, n - 7], Some(item), |m| {
                                    owned_name(m, item, name, n - 7, e)
                                })?;
                                set(m, Reg::Ebx, item);
                                set(m, Reg::Esi, names);
                                let count = m.r32(names + 8);
                                let cap = m.r32(names);
                                set(m, Reg::Eax, count + 1);
                                set(m, Reg::Ecx, cap);
                                if count + 1 > cap {
                                    let new_cap = cap + count + 1;
                                    set(m, Reg::Edi, new_cap);
                                    if new_cap == 0 {
                                        return Err(
                                            "NativeVmNamespace: inventory size overflow".into()
                                        );
                                    }
                                    let old = m.r32(names + 4);
                                    set(m, Reg::Eax, old);
                                    set(m, Reg::Edx, new_cap * 4);
                                    stack::push(m, new_cap * 4);
                                    stack::push(m, old);
                                    let data = stack::external(m, 0x41d964, &[], None, |m| {
                                        e.reallocate(m, old, cap * 4, new_cap * 4)
                                    })?;
                                    set(m, Reg::Eax, data);
                                    stack::add_sp(m, 8);
                                    write(m, names + 4, data);
                                    if new_cap < count {
                                        write(m, names + 8, new_cap);
                                    }
                                    write(m, names, new_cap);
                                }
                                set(m, Reg::Ecx, m.r32(names + 8));
                                set(m, Reg::Edx, m.r32(names + 4));
                                write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, item);
                                write(m, names + 8, r(m, Reg::Ecx) + 1);
                            }
                        }
                    }
                }
            }
            set(m, Reg::Esi, handle);
            set(m, Reg::Eax, directory);
            stack::push(m, directory);
            set(m, Reg::Ecx, name);
            stack::push(m, name);
            stack::push(m, handle);
            let next = stack::call(m, 0x41d994, &[], None, |m| {
                raw_find_next(m, handle, name, directory)
            })?;
            stack::add_sp(m, 12);
            if (next as i32) < 0 {
                break;
            }
        }
        stack::push(m, handle);
        stack::call(m, 0x41d9a5, &[], None, |m| raw_find_close(m, handle, e))?;
        stack::add_sp(m, 4);
        pop(m, Reg::Edi);
        pop(m, Reg::Ebp);
        pop(m, Reg::Ebx);
        pop(m, Reg::Esi);
        stack::add_sp(m, 0x20c);
        Ok(())
    })
}
fn resolve(
    m: &mut Space,
    namespace: u32,
    name: u32,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if namespace == 0 {
        return Err("NativeVmNamespace: absent physical namespace owner".into());
    }
    stack::function(m, &[name], Some(namespace), |m| {
        stack::alloc(m, 12);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, name);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, namespace);
        stack::push(m, 0x2e);
        stack::push(m, name);
        stack::store(m, 0x18, namespace);
        let last = stack::call(m, 0x41d2a9, &[], None, |m| last_character(m, name, 0x2e));
        set(m, Reg::Ebp, last);
        stack::add_sp(m, 8);
        if last == 0 {
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        stack::push(m, 0x2e);
        stack::push(m, name);
        let first = stack::call(m, 0x41d2be, &[], None, |m| {
            mapping::character(m, name, 0x2e)
        });
        set(m, Reg::Ebx, first);
        stack::add_sp(m, 8);
        if first == 0 {
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        let prefix = m.r32(namespace);
        set(m, Reg::Edx, prefix);
        set(m, Reg::Eax, 0);
        let n = scan(m, prefix)?;
        set(m, Reg::Ecx, n - 1);
        set(m, Reg::Edi, n - 1);
        stack::push(m, n - 1);
        stack::push(m, prefix);
        stack::push(m, name);
        let comparison = stack::call(m, 0x41d2e3, &[], None, |m| {
            mapping::compare_prefix(m, name, prefix, n - 1)
        });
        stack::add_sp(m, 12);
        if comparison != 0 {
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        set(m, Reg::Eax, first - name);
        if n - 1 != first - name {
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        set(m, Reg::Ebx, first + 1);
        set(m, Reg::Ebp, last + 1);
        stack::store(m, 0x18, last + 1);
        if last != first {
            set(m, Reg::Edi, namespace);
            let children = m.r32(namespace + 4);
            set(m, Reg::Eax, children);
            let count = m.r32(children + 8);
            set(m, Reg::Ecx, count);
            set(m, Reg::Esi, 0);
            for index in 0..count {
                if index >= m.r32(children) {
                    return Err("NativeVmNamespace: malformed child inventory".into());
                }
                set(m, Reg::Eax, m.r32(children + 4));
                let child = m.r32(r(m, Reg::Eax) + index * 4);
                set(m, Reg::Ecx, child);
                let found = stack::call(m, 0x41d3f2, &[first + 1], Some(child), |m| {
                    resolve(m, child, first + 1, e)
                })?;
                if found != 0 {
                    return Ok(found);
                }
                set(m, Reg::Eax, children);
                set(m, Reg::Ecx, m.r32(children + 8));
                set(m, Reg::Esi, index + 1);
            }
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        set(m, Reg::Ecx, namespace);
        let paths = m.r32(namespace + 8);
        set(m, Reg::Eax, paths);
        if paths == 0 {
            set(m, Reg::Eax, 0);
            return Ok(0);
        }
        set(m, Reg::Ecx, m.r32(paths + 8));
        set(m, Reg::Ebx, 0);
        stack::store(m, 0x20, 0);
        for index in 0..m.r32(paths + 8) {
            if index >= m.r32(paths) {
                return Err("NativeVmNamespace: malformed path inventory".into());
            }
            set(m, Reg::Edx, m.r32(paths + 4));
            let path = m.r32(r(m, Reg::Edx) + index * 4);
            set(m, Reg::Esi, path);
            set(m, Reg::Eax, m.r32(path + 4));
            stack::store(m, 0x14, path);
            if m.r32(path + 4) == 0 {
                stack::call(m, 0x41d348, &[], Some(path), |m| inventory(m, path, e))?;
            }
            let names = m.r32(path + 4);
            set(m, Reg::Ebp, names);
            let count = m.r32(names + 8);
            set(m, Reg::Edx, count);
            set(m, Reg::Edi, 0);
            for item in 0..count {
                if item >= m.r32(names) {
                    return Err("NativeVmNamespace: malformed class-name inventory".into());
                }
                set(m, Reg::Eax, m.r32(names + 4));
                set(m, Reg::Eax, m.r32(r(m, Reg::Eax) + item * 4));
                set(m, Reg::Esi, last + 1);
                set(m, Reg::Eax, m.r32(r(m, Reg::Eax)));
                let mut a = r(m, Reg::Eax);
                let mut b = last + 1;
                let matched = loop {
                    let mut done = None;
                    for off in 0..2 {
                        let av = m.r8(a + off);
                        let bv = m.r8(b + off);
                        low(m, Reg::Ebx, av);
                        low(m, Reg::Ecx, av);
                        if av != bv {
                            set(m, Reg::Eax, if av < bv { u32::MAX } else { 1 });
                            done = Some(false);
                            break;
                        }
                        if off == 0 && av == 0 {
                            set(m, Reg::Eax, 0);
                            done = Some(true);
                            break;
                        }
                        if off == 1 {
                            a += 2;
                            b += 2;
                            set(m, Reg::Eax, a);
                            set(m, Reg::Esi, b);
                            if av == 0 {
                                set(m, Reg::Eax, 0);
                                done = Some(true);
                            }
                        }
                    }
                    if let Some(value) = done {
                        break value;
                    }
                };
                if matched {
                    set(m, Reg::Ecx, path);
                    set(m, Reg::Eax, m.r32(path));
                    if r(m, Reg::Eax) != 0 {
                        return Ok(r(m, Reg::Eax));
                    }
                    break;
                }
                set(m, Reg::Edi, item + 1);
            }
            set(m, Reg::Ebx, index);
            set(m, Reg::Edx, namespace);
            set(m, Reg::Eax, paths);
            set(m, Reg::Ecx, m.r32(paths + 8));
            set(m, Reg::Ebx, index + 1);
            stack::store(m, 0x20, index + 1);
        }
        set(m, Reg::Eax, 0);
        Ok(0)
    })
}
#[must_use]
pub struct PendingClassResolution {
    pub bootstrap: Option<stack::Frame>,
    pub lookup: stack::Frame,
    pub namespace: stack::Frame,
    pub loader: stack::Frame,
    pub jvm: u32,
    pub path: u32,
    pub name: u32,
    pub pc: u32,
    pub return_pc: u32,
}
pub fn begin_class_resolution(
    m: &mut Space,
    pending: crate::native_vm_bootstrap::PendingClassLoad,
    e: &mut impl Environment,
) -> Result<PendingClassResolution, String> {
    let jvm = pending.jvm;
    let name = pending.name;
    let return_pc = match (pending.pc, pending.name) {
        (0x41001d, 0x60ac84) => 0x410022,
        (0x410029, 0x60ac98) => 0x41002e,
        _ => return Err("NativeVmNamespace: unsupported bootstrap class-load instruction".into()),
    };
    if !stack::active(m)
        || stack::has_pending_call(m)
        || r(m, Reg::Ecx) != jvm
        || r(m, Reg::Esi) != jvm
        || stack::load(m, 0) != name
    {
        return Err(
            "NativeVmNamespace: actual retained bootstrap class-load caller required".into(),
        );
    }
    stack::begin_call(m, return_pc, &[], Some(jvm));
    match begin_class_lookup(m, jvm, name, e)? {
        ClassLookup::Loading(mut pending_load) => {
            pending_load.bootstrap = Some(pending.caller);
            Ok(pending_load)
        }
        ClassLookup::Cached(_) => Err("NativeVmNamespace: bootstrap class already exists".into()),
    }
}
#[must_use]
pub enum ClassLookup {
    Cached(u32),
    Loading(PendingClassResolution),
}
/// Enter the actual pending410890 CALL. No parent frame is invented for
/// callers outside bootstrap; its owner retains the return continuation.
pub fn begin_class_lookup(
    m: &mut Space,
    jvm: u32,
    name: u32,
    e: &mut impl Environment,
) -> Result<ClassLookup, String> {
    if !stack::active(m)
        || !stack::has_pending_call(m)
        || r(m, Reg::Ecx) != jvm
        || stack::argument(m, 0) != name
    {
        return Err("NativeVmNamespace: actual pending class lookup CALL required".into());
    }
    if matches!(m.r8(name), b'[' | b'L') {
        return Err("NativeVmNamespace: descriptor/array class continuation unavailable".into());
    }
    let return_pc = stack::load(m, 0);
    let lookup = stack::enter(m, &[name], Some(jvm));
    stack::alloc(m, 0x220);
    stack::push_reg(m, Reg::Ebx);
    set(m, Reg::Ebx, name);
    low(m, Reg::Eax, m.r8(name));
    stack::push_reg(m, Reg::Esi);
    set(m, Reg::Esi, jvm);
    let existing = stack::call(m, 0x4108e8, &[name], Some(jvm), |m| {
        crate::native_vm_lookup::find_class(m, jvm, name)
    })?;
    if existing != 0 {
        pop(m, Reg::Esi);
        pop(m, Reg::Ebx);
        stack::add_sp(m, 0x220);
        stack::leave(m, lookup);
        return Ok(ClassLookup::Cached(existing));
    }
    stack::begin_call(m, 0x4108f8, &[name], Some(jvm));
    let namespace = stack::enter(m, &[name], Some(jvm));
    stack::alloc(m, 0x120);
    for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
        stack::push_reg(m, reg);
    }
    set(m, Reg::Edi, name);
    set(m, Reg::Esi, jvm);
    set(m, Reg::Ecx, m.r32(jvm + 0x7c));
    set(m, Reg::Ebp, 0);
    let path = stack::call(m, 0x41064e, &[name], Some(m.r32(jvm + 0x7c)), |m| {
        resolve(m, m.r32(jvm + 0x7c), name, e)
    })?;
    set(m, Reg::Ebx, path);
    stack::push(m, 0x100);
    if path == 0 {
        return Err("NativeVmNamespace: first class has no source path".into());
    }
    stack::push(m, 0x2e);
    stack::push(m, name);
    let last = stack::call(m, 0x410661, &[], None, |m| last_character(m, name, 0x2e));
    stack::add_sp(m, 8);
    set(m, Reg::Eax, last + 1);
    stack::push(m, last + 1);
    let buffer = stack::sp(m) + 0x38;
    set(m, Reg::Eax, buffer);
    stack::push(m, buffer);
    stack::call(m, 0x410670, &[], None, |m| {
        bounded_copy(m, buffer, last + 1, 0x100)
    });
    stack::add_sp(m, 12);
    set(m, Reg::Ecx, buffer);
    stack::begin_call(m, 0x410680, &[path, buffer], Some(jvm));
    let loader = stack::enter(m, &[path, buffer], Some(jvm));
    stack::alloc(m, 0x204);
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Ebp);
    set(m, Reg::Ebp, path);
    stack::push_reg(m, Reg::Esi);
    set(m, Reg::Esi, buffer);
    stack::push_reg(m, Reg::Edi);
    stack::push(m, buffer);
    set(m, Reg::Edi, jvm);
    stack::push(m, path);
    Ok(ClassLookup::Loading(PendingClassResolution {
        bootstrap: None,
        lookup,
        namespace,
        loader,
        jvm,
        path,
        name: buffer,
        pc: 0x41044c,
        return_pc,
    }))
}
