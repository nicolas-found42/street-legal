//! Original411b20 entered loader with its retained file/bootstrap callers.
use super::*;
#[must_use]
pub struct ModuleLoad {
    pub caller: stack::Frame,
    pub module: u32,
    pub pool: u32,
    pub source: u32,
    pub pc: u32,
    next: Option<(u32, u32)>,
    completed: Vec<crate::native_vm_metadata::loader::LoadedModule>,
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn pool(m: &mut Space, this: u32) {
    stack::function(m, &[], Some(this), |m| {
        set(m, Reg::Eax, this);
        set(m, Reg::Ecx, 0);
        for off in [8, 16, 12] {
            write(m, this + off, 0);
        }
        write(m, this + 4, 0x5e736c);
        write(m, this, 0x5e735c);
    });
}
fn vector(m: &mut Space, this: u32) {
    stack::function(m, &[], Some(this), |m| {
        set(m, Reg::Eax, this);
        set(m, Reg::Ecx, 0);
        for off in [0, 8, 4] {
            write(m, this + off, 0);
        }
    });
}
fn members(m: &mut Space, this: u32, method: bool) {
    stack::function(m, &[], Some(this), |m| {
        set(m, Reg::Eax, this);
        set(m, Reg::Ecx, 0);
        for off in [4, 12, 8] {
            write(m, this + off, 0);
        }
        write(m, this, if method { 0x5f08a8 } else { 0x5f08b8 });
    });
}
fn allocate(
    m: &mut Space,
    pc: u32,
    bytes: u32,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.allocate(m, bytes))?;
    set(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmSourceLoader: original module allocation failed".into());
    }
    Ok(p)
}
pub fn begin_module_load(
    m: &mut Space,
    jvm: u32,
    input: &cache::FileInput,
    e: &mut impl FileEnvironment,
) -> Result<ModuleLoad, String> {
    if !stack::active(m)
        || !stack::has_pending_call(m)
        || stack::load(m, 0) != 0x4104c8
        || stack::argument(m, 0) != input.source
        || stack::argument(m, 1) != input.bytes
        || r(m, Reg::Ecx) != jvm
        || input.bytes < 12
    {
        return Err("NativeVmSourceLoader: actual retained file input CALL required".into());
    }
    let caller = stack::enter(m, &[input.source, input.bytes], Some(jvm));
    stack::alloc(m, 0x14);
    stack::push_reg(m, Reg::Ebx);
    stack::store(m, 0xc, jvm);
    set(m, Reg::Ecx, input.bytes);
    set(m, Reg::Ebx, 0);
    set(m, Reg::Eax, 0);
    stack::push_reg(m, Reg::Ebp);
    set(m, Reg::Ebp, input.source);
    stack::store(m, 8, 0);
    stack::push_reg(m, Reg::Esi);
    set(m, Reg::Esi, input.bytes);
    stack::push_reg(m, Reg::Edi);
    let payload = read_module_header(m, input.source, input.bytes)?;
    construct_module(m, caller, jvm, payload, e)
}
fn read_module_header(m: &mut Space, mut source: u32, mut remaining: u32) -> Result<u32, String> {
    let b = stack::sp(m);
    loop {
        stack::push(m, source);
        let tag = stack::call(m, 0x411b4d, &[], None, |m| cache::read_word(m, source));
        source += 4;
        set(m, Reg::Ebp, source);
        stack::push(m, source);
        set(m, Reg::Edi, tag);
        let bytes = stack::call(m, 0x411b58, &[], None, |m| cache::read_word(m, source));
        set(m, Reg::Ecx, remaining);
        source += 4;
        set(m, Reg::Ebp, source);
        remaining -= 8;
        set(m, Reg::Ecx, remaining);
        stack::add_sp(m, 8);
        m.w32(b + 0x20, bytes);
        m.w32(b + 0x1c, source);
        m.w32(b + 0x2c, remaining);
        if bytes > remaining {
            return Err("NativeVmSourceLoader: source chunk exceeds actual input".into());
        }
        if tag == 0x534e4f43 {
            return Ok(source);
        }
        if tag != 0x41465554 || bytes != 4 {
            return Err("NativeVmSourceLoader: unproved pre-CONS source chunk".into());
        }
        stack::push(m, source);
        let version = stack::call(m, 0x411be7, &[], None, |m| cache::read_word(m, source));
        stack::add_sp(m, 4);
        if version != 0x11ae1 {
            return Err("NativeVmSourceLoader: unsupported TUFA source version".into());
        }
        set(m, Reg::Ecx, bytes);
        set(m, Reg::Eax, remaining.wrapping_sub(bytes));
        source += bytes;
        set(m, Reg::Ebp, source);
        remaining -= bytes;
        m.w32(b + 0x2c, remaining);
    }
}
fn construct_module(
    m: &mut Space,
    caller: stack::Frame,
    jvm: u32,
    payload: u32,
    e: &mut impl FileEnvironment,
) -> Result<ModuleLoad, String> {
    stack::push(m, 0x64);
    let module = allocate(m, 0x411df1, 0x64, e)?;
    set(m, Reg::Esi, module);
    stack::add_sp(m, 4);
    set(m, Reg::Ecx, module + 0x10);
    write(m, module, 0x5f08a4);
    write(m, module + 8, 0);
    write(m, module + 4, 0);
    stack::call(m, 0x411e12, &[], Some(module + 0x10), |m| {
        pool(m, module + 0x10)
    });
    set(m, Reg::Eax, jvm);
    stack::push(m, 0x10);
    write(m, module, 0x5f08a0);
    write(m, module + 0xc, jvm);
    let first = allocate(m, 0x411e26, 0x10, e)?;
    set(m, Reg::Edi, first);
    stack::add_sp(m, 4);
    stack::call(m, 0x411e37, &[], Some(first + 4), |m| vector(m, first + 4));
    write(m, first, 0x5f08a8);
    stack::push(m, 0x10);
    write(m, module + 0x50, first);
    for (off, bytes, alloc_pc, ctor_pc, kind, next) in [
        (0x4c, 16, 0x411e4b, 0x411e59, 0, 16),
        (0x58, 16, 0x411e67, 0x411e75, 1, 16),
        (0x54, 16, 0x411e83, 0x411e91, 0, 12),
        (0x5c, 12, 0x411e9f, 0x411ead, 2, 12),
        (0x60, 12, 0x411ebb, 0x411ec9, 2, 20),
    ] {
        let p = allocate(m, alloc_pc, bytes, e)?;
        stack::add_sp(m, 4);
        stack::call(m, ctor_pc, &[], Some(p), |m| {
            if kind == 2 {
                vector(m, p)
            } else {
                members(m, p, kind == 1)
            }
        });
        stack::push(m, next);
        write(m, module + off, p);
    }
    let constants = allocate(m, 0x411ed7, 20, e)?;
    stack::add_sp(m, 4);
    stack::call(m, 0x411ee5, &[], Some(constants), |m| pool(m, constants));
    write(m, module + 0x24, constants);
    for off in [0x28, 0x34, 0x48, 0x30] {
        write(m, module + off, 0);
    }
    set(m, Reg::Ecx, constants);
    write(m, module + 0x34, 0);
    write(m, module + 0x28, 0);
    set(m, Reg::Edx, m.r32(constants));
    stack::push(m, payload);
    stack::begin_call(m, 0x411f03, &[], Some(constants));
    Ok(ModuleLoad {
        caller,
        module,
        pool: constants,
        source: payload,
        pc: 0x407660,
        next: None,
        completed: Vec::new(),
    })
}

impl ModuleLoad {
    pub fn modules(&self) -> &[crate::native_vm_metadata::loader::LoadedModule] {
        &self.completed
    }
    pub fn has_next(&self) -> bool {
        self.pc == 0x411b47
    }
}
pub fn begin_next_module(
    m: &mut Space,
    module: &mut ModuleLoad,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    if !module.has_next()
        || !stack::active(m)
        || stack::has_pending_call(m)
        || r(m, Reg::Esi) != 0
        || stack::load(m, 0x2c) == 0
    {
        return Err("NativeVmSourceLoader: actual retained next-module caller required".into());
    }
    let payload = read_module_header(m, r(m, Reg::Ebp), stack::load(m, 0x2c))?;
    let mut next = construct_module(m, module.caller, stack::load(m, 0x18), payload, e)?;
    next.completed = std::mem::take(&mut module.completed);
    *module = next;
    Ok(())
}

fn pop(m: &mut Space, reg: Reg) {
    set(m, reg, stack::load(m, 0));
    stack::add_sp(m, 4);
}
fn deserialize_record(
    m: &mut Space,
    record: u32,
    source: u32,
    kind: u32,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    stack::function(m, &[source], Some(record), |m| {
        if kind == 0 {
            stack::push_reg(m, Reg::Ebx);
            stack::push_reg(m, Reg::Ebp);
            set(m, Reg::Ebp, source);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            stack::push(m, source);
            set(m, Reg::Esi, record);
            let count = stack::call(m, 0x407100, &[], None, |m| cache::read_word(m, source));
            set(m, Reg::Edi, count);
            set(m, Reg::Ebx, count + 1);
            stack::push(m, count + 1);
            write(m, record + 4, 0);
            write(m, record + 12, count);
            let buffer = allocate(m, 0x407115, count + 1, e)?;
            stack::push(m, count + 1);
            set(m, Reg::Ebp, source + 4);
            stack::push(m, source + 4);
            stack::push(m, buffer);
            write(m, record + 8, buffer);
            stack::call(m, 0x407123, &[], None, |m| {
                bounded_copy(m, buffer, source + 4, count + 1)
            });
            set(m, Reg::Eax, buffer);
            stack::add_sp(m, 0x14);
            byte(m, buffer + count, 0);
            for reg in [Reg::Edi, Reg::Esi, Reg::Ebp, Reg::Ebx] {
                pop(m, reg);
            }
        } else if kind == 4 {
            set(m, Reg::Eax, source);
            stack::push_reg(m, Reg::Esi);
            stack::push(m, source);
            set(m, Reg::Esi, record);
            let index = stack::call(m, 0x4073dd, &[], None, |m| cache::read_word(m, source));
            stack::add_sp(m, 4);
            write(m, record + 4, 4);
            write(m, record + 8, index);
            pop(m, Reg::Esi);
        } else if matches!(kind, 3 | 5 | 7) {
            stack::push_reg(m, Reg::Ebx);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            set(m, Reg::Edi, source);
            set(m, Reg::Eax, source + 4);
            stack::push(m, source + 4);
            set(m, Reg::Esi, record);
            let second = stack::call(
                m,
                if kind == 3 {
                    0x407282
                } else if kind == 5 {
                    0x407552
                } else {
                    0x407482
                },
                &[],
                None,
                |m| cache::read_word(m, source + 4),
            );
            stack::push(m, source);
            set(m, Reg::Ebx, second);
            let first = stack::call(
                m,
                if kind == 3 {
                    0x40728a
                } else if kind == 5 {
                    0x40755a
                } else {
                    0x40748a
                },
                &[],
                None,
                |m| cache::read_word(m, source),
            );
            stack::add_sp(m, 8);
            pop(m, Reg::Edi);
            if kind == 3 {
                write(m, record + 8, second);
                write(m, record + 4, kind);
                write(m, record + 12, first);
            } else {
                write(m, record + 12, second);
                write(m, record + 4, kind);
                write(m, record + 8, first);
            }
            if kind == 5 {
                write(m, record + 16, u32::MAX);
            }
            pop(m, Reg::Esi);
            pop(m, Reg::Ebx);
        } else {
            return Err(
                "NativeVmSourceLoader: constant kind source continuation unavailable".into(),
            );
        }
        Ok(())
    })
}
pub fn deserialize_constants(
    m: &mut Space,
    module: &mut ModuleLoad,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    if module.pc != 0x407660
        || !stack::has_pending_call(m)
        || stack::load(m, 0) != 0x411f03
        || stack::argument(m, 0) != module.source
        || r(m, Reg::Ecx) != module.pool
        || r(m, Reg::Esi) != module.module
    {
        return Err("NativeVmSourceLoader: actual constant deserializer CALL required".into());
    }
    let frame = stack::enter(m, &[module.source], Some(module.pool));
    stack::alloc(m, 0x24);
    stack::push_reg(m, Reg::Esi);
    stack::push_reg(m, Reg::Edi);
    set(m, Reg::Edi, module.source);
    stack::push(m, module.source);
    set(m, Reg::Esi, module.pool);
    let count = stack::call(m, 0x407671, &[], None, |m| {
        cache::read_word(m, module.source)
    });
    set(m, Reg::Edi, module.source + 4);
    stack::add_sp(m, 4);
    set(m, Reg::Ecx, count);
    set(m, Reg::Eax, count.wrapping_sub(1));
    stack::store(m, 0x30, module.source + 4);
    if count == 0 {
        stack::leave(m, frame);
        module.pc = 0x411f03;
        return Ok(());
    }
    stack::push_reg(m, Reg::Ebp);
    set(m, Reg::Eax, count);
    set(m, Reg::Ebp, module.pool + 8);
    set(m, Reg::Esi, module.source + 4);
    let b = stack::sp(m);
    m.w32(b + 0xc, count);
    for index in 0..count {
        let source = m.r32(b + 0x34);
        set(m, Reg::Edx, source);
        stack::push(m, source);
        let kind = stack::call(m, 0x40769d, &[], None, |m| cache::read_word(m, source));
        set(m, Reg::Ecx, source);
        set(m, Reg::Edi, kind);
        set(m, Reg::Ecx, source + 4);
        stack::add_sp(m, 4);
        m.w32(b + 0x34, source + 4);
        let (bytes, pc, vt) = match kind {
            0 => (16, 0x407710, 0x5e738c),
            3 => (16, 0x40772b, 0x5e73ac),
            4 => (16, 0x407746, 0x5e73f0),
            5 => (20, 0x407761, 0x5e73e0),
            7 => (16, 0x40777c, 0x5e73d0),
            _ => {
                return Err(format!(
                    "NativeVmSourceLoader: unproved constant kind {kind}"
                ))
            }
        };
        stack::push(m, bytes);
        let record = allocate(m, pc, bytes, e)?;
        stack::add_sp(m, 4);
        write(m, record, vt);
        set(m, Reg::Esi, record);
        let source = source + 4;
        set(m, Reg::Edi, source);
        set(m, Reg::Edx, vt);
        stack::call(m, 0x407803, &[source], Some(record), |m| {
            deserialize_record(m, record, source, kind, e)
        })?;
        set(m, Reg::Eax, vt);
        let size = stack::call(m, 0x407809, &[], Some(record), |m| {
            stack::function(m, &[], Some(record), |m| {
                let bytes = if kind == 0 {
                    m.r32(record + 12) + 5
                } else if kind == 4 {
                    4
                } else {
                    8
                };
                set(m, Reg::Eax, bytes);
                bytes
            })
        });
        let vector = module.pool + 8;
        let cap = m.r32(vector);
        let len = m.r32(vector + 8);
        set(m, Reg::Ecx, cap);
        set(m, Reg::Edi, source + size);
        set(m, Reg::Eax, len + 1);
        m.w32(b + 0x34, source + size);
        if len + 1 > cap {
            set(m, Reg::Ecx, cap + len + 1);
            stack::call(m, 0x407824, &[cap + len + 1], Some(vector), |m| {
                mapping::grow(m, vector, cap + len + 1, e)
            })?;
        }
        set(m, Reg::Ecx, m.r32(vector + 8));
        set(m, Reg::Edx, m.r32(vector + 4));
        set(m, Reg::Eax, m.r32(b + 0xc));
        write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, record);
        set(m, Reg::Ecx, m.r32(vector + 8) + 1);
        set(m, Reg::Eax, count - index - 1);
        write(m, vector + 8, r(m, Reg::Ecx));
        m.w32(b + 0xc, r(m, Reg::Eax));
    }
    pop(m, Reg::Ebp);
    pop(m, Reg::Edi);
    pop(m, Reg::Esi);
    stack::add_sp(m, 0x24);
    stack::leave(m, frame);
    module.pc = 0x411f03;
    Ok(())
}

fn read_member(m: &mut Space, record: u32, source: u32, method: bool) {
    stack::function(m, &[source], Some(record), |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, source);
        stack::push_reg(m, Reg::Edi);
        stack::push(m, source);
        set(m, Reg::Edi, record);
        let count = if method { 5 } else { 4 };
        let base = if method { 0x4195b0 } else { 0x419250 };
        for i in 0..count {
            let value = stack::call(m, base + 0xe + i * 12, &[], None, |m| {
                cache::read_word(m, source + i * 4)
            });
            if i + 1 < count {
                set(m, Reg::Esi, source + (i + 1) * 4);
                stack::push(m, source + (i + 1) * 4);
            } else {
                stack::add_sp(m, count * 4);
            }
            write(m, record + 4 + i * 4, value);
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
    });
}
fn member_size(m: &mut Space, record: u32, method: bool) -> u32 {
    stack::function(m, &[], Some(record), |m| {
        let size = if method { 20 } else { 16 };
        set(m, Reg::Eax, size);
        size
    })
}
fn read_members(
    m: &mut Space,
    list: u32,
    source: u32,
    method: bool,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    if m.r32(list + 8) != 0 || m.r32(list + 12) != 0 {
        return Err("NativeVmSourceLoader: nonempty member destruction owner unavailable".into());
    }
    stack::function(m, &[source], Some(list), |m| {
        let base = if method { 0x419b60 } else { 0x419d00 };
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, list);
        set(m, Reg::Eax, m.r32(list + 8));
        stack::push_reg(m, Reg::Edi);
        stack::push(m, m.r32(list + 8));
        let freed = stack::external(m, base + 0xf, &[], None, |m| e.free(m, m.r32(list + 8)))?;
        set(m, Reg::Eax, freed);
        set(m, Reg::Eax, m.r32(list + 12));
        set(m, Reg::Ebp, 0);
        stack::add_sp(m, 4);
        write(m, list + 8, 0);
        set(m, Reg::Ebx, source);
        stack::push(m, source);
        write(m, list + 4, 0);
        let count = stack::call(m, base + 0x2e, &[], None, |m| cache::read_word(m, source));
        set(m, Reg::Edi, count);
        stack::add_sp(m, 4);
        if count == 0 {
            set(m, Reg::Ecx, m.r32(list + 8));
            stack::push(m, m.r32(list + 8));
            let freed = stack::external(m, base + 0x40, &[], None, |m| e.free(m, m.r32(list + 8)))?;
            set(m, Reg::Eax, freed);
            stack::add_sp(m, 4);
            write(m, list + 8, 0);
        } else {
            set(m, Reg::Eax, m.r32(list + 8));
            set(m, Reg::Edx, count * 4);
            stack::push(m, count * 4);
            stack::push(m, m.r32(list + 8));
            let table = stack::external(m, base + 0x59, &[], None, |m| {
                e.reallocate(m, m.r32(list + 8), 0, count * 4)
            })?;
            set(m, Reg::Eax, table);
            stack::add_sp(m, 8);
            write(m, list + 8, table);
        }
        if count < m.r32(list + 12) {
            write(m, list + 12, count);
        }
        set(m, Reg::Eax, count);
        write(m, list + 4, count);
        set(m, Reg::Ebx, source + 4);
        set(m, Reg::Edi, 0);
        write(m, list + 12, count);
        for index in 0..count {
            let size = if method { 32 } else { 20 };
            stack::push(m, size);
            let record = allocate(m, base + 0x7f, size, e)?;
            stack::add_sp(m, 4);
            if method {
                write(m, record + 24, 0);
                write(m, record + 28, 0);
            }
            let vt = if method { 0x5f08d0 } else { 0x5f0890 };
            write(m, record, vt);
            let table = m.r32(list + 8);
            set(m, Reg::Ecx, table);
            write(m, table + index * 4, record);
            set(m, Reg::Edx, table);
            set(m, Reg::Ecx, record);
            set(m, Reg::Eax, vt);
            let input = r(m, Reg::Ebx);
            stack::call(
                m,
                base + if method { 0xa8 } else { 0xa2 },
                &[input],
                Some(record),
                |m| {
                    read_member(m, record, input, method);
                    Ok::<(), String>(())
                },
            )?;
            set(m, Reg::Ecx, table);
            set(m, Reg::Ecx, record);
            set(m, Reg::Edx, vt);
            let bytes = stack::call(
                m,
                base + if method { 0xb2 } else { 0xac },
                &[],
                Some(record),
                |m| member_size(m, record, method),
            );
            set(m, Reg::Ebx, r(m, Reg::Ebx) + bytes);
            set(m, Reg::Eax, m.r32(list + 12));
            set(m, Reg::Edi, index + 1);
        }
        for reg in [Reg::Edi, Reg::Esi, Reg::Ebp, Reg::Ebx] {
            pop(m, reg);
        }
        Ok(())
    })
}
fn members_size(m: &mut Space, list: u32, method: bool) -> u32 {
    stack::function(m, &[], Some(list), |m| {
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, list);
        set(m, Reg::Eax, m.r32(list + 12));
        set(m, Reg::Ebx, 0);
        set(m, Reg::Esi, 0);
        let mut bytes = 0;
        for i in 0..m.r32(list + 12) {
            let table = m.r32(list + 8);
            set(m, Reg::Eax, table);
            let record = m.r32(table + i * 4);
            set(m, Reg::Ecx, record);
            set(m, Reg::Edx, m.r32(record));
            bytes += stack::call(m, 0x419c4a, &[], Some(record), |m| {
                member_size(m, record, method)
            });
            set(m, Reg::Ebx, bytes);
            set(m, Reg::Eax, m.r32(list + 12));
            set(m, Reg::Esi, i + 1);
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
        set(m, Reg::Eax, bytes + 4);
        pop(m, Reg::Ebx);
        bytes + 4
    })
}
fn next_chunk(m: &mut Space) -> Result<(u32, u32), String> {
    let b = stack::sp(m);
    let bytes = m.r32(b + 0x20);
    let remaining = m.r32(b + 0x2c);
    if bytes >= remaining || remaining - bytes < 8 {
        return Err("NativeVmSourceLoader: missing next source chunk".into());
    }
    set(m, Reg::Ecx, bytes);
    set(m, Reg::Eax, remaining - bytes);
    set(m, Reg::Ebp, r(m, Reg::Ebp) + bytes);
    m.w32(b + 0x2c, remaining - bytes);
    let source = r(m, Reg::Ebp);
    stack::push(m, source);
    let tag = stack::call(m, 0x411b4d, &[], None, |m| cache::read_word(m, source));
    set(m, Reg::Ebp, source + 4);
    stack::push(m, source + 4);
    set(m, Reg::Edi, tag);
    let count = stack::call(m, 0x411b58, &[], None, |m| cache::read_word(m, source + 4));
    set(m, Reg::Ecx, remaining - bytes);
    set(m, Reg::Ebp, source + 8);
    set(m, Reg::Ecx, remaining - bytes - 8);
    stack::add_sp(m, 8);
    m.w32(b + 0x20, count);
    m.w32(b + 0x1c, source + 8);
    m.w32(b + 0x2c, remaining - bytes - 8);
    if count > remaining - bytes - 8 {
        return Err("NativeVmSourceLoader: truncated next chunk".into());
    }
    Ok((tag, source + 8))
}
pub fn deserialize_members(
    m: &mut Space,
    module: &mut ModuleLoad,
    e: &mut impl FileEnvironment,
) -> Result<(), String> {
    if module.pc != 0x411f03 || stack::has_pending_call(m) || r(m, Reg::Esi) != module.module {
        return Err("NativeVmSourceLoader: post-CONS caller required".into());
    }
    let mut fields = false;
    loop {
        let (tag, source) = next_chunk(m)?;
        let method = match tag {
            0x444c4946 if !fields => {
                fields = true;
                false
            }
            0x4448544d => true,
            0x53534c43 => {
                // The source dispatch loop permits omitted empty member
                // sections. Retain the header already consumed at411b77.
                module.next = Some((tag, source));
                module.pc = 0x411b77;
                return Ok(());
            }
            _ => return Err("NativeVmSourceLoader: unexpected member chunk order".into()),
        };
        let first = m.r32(module.module + if method { 0x58 } else { 0x54 });
        let second = m.r32(module.module + if method { 0x50 } else { 0x4c });
        set(m, Reg::Ecx, first);
        if method {
            set(m, Reg::Edx, m.r32(first));
        } else {
            set(m, Reg::Eax, m.r32(first));
        }
        stack::call(
            m,
            if method { 0x411bc7 } else { 0x411ba4 },
            &[source],
            Some(first),
            |m| read_members(m, first, source, method, e),
        )?;
        set(m, Reg::Ecx, first);
        if method {
            set(m, Reg::Eax, second);
            set(m, Reg::Edx, m.r32(first));
        } else {
            set(m, Reg::Edx, second);
            set(m, Reg::Eax, m.r32(first));
        }
        set(m, Reg::Edi, m.r32(second));
        let bytes = stack::call(
            m,
            if method { 0x411bd3 } else { 0x411bb0 },
            &[],
            Some(first),
            |m| members_size(m, first, method),
        );
        set(m, Reg::Ecx, second);
        set(m, Reg::Eax, source + bytes);
        stack::call(
            m,
            if method { 0x411bdc } else { 0x411bb9 },
            &[source + bytes],
            Some(second),
            |m| read_members(m, second, source + bytes, method, e),
        )?;
        if method {
            module.pc = 0x411bdc;
            return Ok(());
        }
    }
}

// Like MOV-byte observations, Unicorn exposes the complete source register
// for MOV-word stores. Preserve that operand and write its actual low word.
fn half(m: &mut Space, address: u32, value: u32) {
    observe(address, 2, value);
    m.w8(address, value as u8);
    m.w8(address + 1, (value >> 8) as u8);
}
fn read_half(m: &mut Space, source: u32) -> u32 {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, source);
        stack::push(m, source);
        let value = stack::call(m, 0x54f69a, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, source);
                set(m, Reg::Ecx, 0);
                let value = u32::from(m.r8(source)) | (u32::from(m.r8(source + 1)) << 8);
                set(m, Reg::Ecx, value);
                set(m, Reg::Eax, (source & !0xffff) | value);
                r(m, Reg::Eax)
            })
        });
        stack::add_sp(m, 4);
        value
    })
}
fn read_node(m: &mut Space, node: u32, source: u32) {
    stack::function(m, &[source], Some(node), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, source);
        set(m, Reg::Eax, u32::from(m.r8(source)));
        set(m, Reg::Esi, node);
        set(m, Reg::Edi, source + 1);
        let index = r(m, Reg::Eax);
        let opcode = u16::from_le_bytes([m.r8(0x5f0914 + index * 2), m.r8(0x5f0915 + index * 2)]);
        stack::push(m, source + 1);
        set(m, Reg::Ecx, (node & !0xffff) | u32::from(opcode));
        half(m, node, r(m, Reg::Ecx));
        let operand = stack::call(m, 0x41a60e, &[], None, |m| read_half(m, source + 1));
        half(m, node + 2, operand);
        set(m, Reg::Eax, u32::from(opcode));
        stack::add_sp(m, 4);
        set(m, Reg::Edi, source + 3);
        set(m, Reg::Eax, u32::from(opcode) & 0xf000);
        if matches!(opcode & 0xf000, 0x1000 | 0x2000 | 0x4000) {
            stack::push(m, source + 3);
            let value = stack::call(m, 0x41a63d, &[], None, |m| cache::read_word(m, source + 3));
            stack::add_sp(m, 4);
            write(m, node + 4, value);
        }
        pop(m, Reg::Edi);
        pop(m, Reg::Esi);
    });
}
fn node_size(m: &mut Space, node: u32) -> u32 {
    stack::function(m, &[], Some(node), |m| {
        let op = u16::from_le_bytes([m.r8(node), m.r8(node + 1)]);
        set(m, Reg::Ecx, (node & !0xffff) | u32::from(op));
        set(m, Reg::Eax, 3);
        set(m, Reg::Ecx, u32::from(op) & 0xf000);
        let bytes = if matches!(op & 0xf000, 0x1000 | 0x2000 | 0x4000) {
            7
        } else {
            3
        };
        set(m, Reg::Eax, bytes);
        bytes
    })
}
pub fn finish_module_tree(
    m: &mut Space,
    module: &mut ModuleLoad,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    if !matches!(module.pc, 0x411bdc | 0x411b77)
        || stack::has_pending_call(m)
        || r(m, Reg::Esi) != module.module
    {
        return Err("NativeVmSourceLoader: actual post-member caller required".into());
    }
    let b = stack::sp(m);
    let (tag, source) = if let Some(header) = module.next.take() {
        if module.pc != 0x411b77 {
            return Err("NativeVmSourceLoader: prefetched header caller changed".into());
        }
        header
    } else {
        next_chunk(m)?
    };
    if tag != 0x53534c43 {
        return Err("NativeVmSourceLoader: original CLSS order unavailable".into());
    }
    let mut cursor = source;
    for (index, off) in [0x2c, 0x38, 0x3c, 0x40, 0x44].into_iter().enumerate() {
        stack::push(m, cursor);
        let value = stack::call(m, 0x411d6c + index as u32 * 12, &[], None, |m| {
            cache::read_word(m, cursor)
        });
        if index < 4 {
            cursor += 4;
            set(m, Reg::Edi, cursor);
        } else {
            stack::add_sp(m, 0x14);
            cursor += 4;
            set(m, Reg::Edi, cursor);
        }
        write(m, module.module + off, value);
    }
    let interfaces = m.r32(module.module + 0x44);
    if interfaces == 0 {
        write(m, module.module + 0x48, r(m, Reg::Ebx));
    } else {
        let chunk_bytes = m.r32(b + 0x20);
        if chunk_bytes < 20 || interfaces > (chunk_bytes - 20) / 4 {
            return Err("NativeVmSourceLoader: truncated CLSS interface array".into());
        }
        set(m, Reg::Eax, interfaces * 4);
        stack::push(m, interfaces * 4);
        let table = allocate(m, 0x411db2, interfaces * 4, e)?;
        write(m, module.module + 0x48, table);
        set(m, Reg::Eax, interfaces);
        stack::add_sp(m, 4);
        for index in 0..interfaces {
            stack::push(m, cursor);
            let value = stack::call(m, 0x411dc9, &[], None, |m| cache::read_word(m, cursor));
            set(m, Reg::Edx, table);
            stack::add_sp(m, 4);
            write(m, table + index * 4, value);
            set(m, Reg::Eax, interfaces);
            cursor += 4;
            set(m, Reg::Edi, cursor);
            set(m, Reg::Ebx, index + 1);
        }
        set(m, Reg::Ebx, 0);
    }
    let (tag, source) = next_chunk(m)?;
    if tag != 0x45455254 {
        return Err("NativeVmSourceLoader: original TREE order unavailable".into());
    }
    stack::push(m, source);
    let count = stack::call(m, 0x411bf5, &[], None, |m| cache::read_word(m, source));
    stack::add_sp(m, 4);
    set(m, Reg::Ecx, count);
    set(m, Reg::Eax, count.wrapping_sub(1));
    set(m, Reg::Ebx, source + 4);
    if count != 0 {
        set(m, Reg::Eax, count);
        m.w32(b + 0x14, count);
        for body in 0..count {
            let cursor = r(m, Reg::Ebx);
            stack::push(m, cursor);
            let nodes = stack::call(m, 0x411c11, &[], None, |m| cache::read_word(m, cursor));
            set(m, Reg::Edi, 0);
            set(m, Reg::Ebp, nodes);
            stack::add_sp(m, 4);
            set(m, Reg::Ebx, cursor + 4);
            m.w32(b + 0x28, 0);
            let buffer = if nodes == 0 {
                0
            } else {
                set(m, Reg::Edx, nodes * 8);
                stack::push(m, nodes * 8);
                let p = allocate(m, 0x411c30, nodes * 8, e)?;
                stack::add_sp(m, 4);
                set(m, Reg::Ecx, nodes - 1);
                m.w32(b + 0x28, nodes - 1);
                m.w32(b + 0x28, p);
                p
            };
            if nodes != 0 {
                set(m, Reg::Edi, buffer);
                for index in 0..nodes {
                    let cursor = r(m, Reg::Ebx);
                    let node = buffer + index * 8;
                    stack::call(m, 0x411c54, &[cursor], Some(node), |m| {
                        read_node(m, node, cursor)
                    });
                    let bytes = stack::call(m, 0x411c5b, &[], Some(node), |m| node_size(m, node));
                    set(m, Reg::Ebx, cursor + bytes);
                    set(m, Reg::Edi, node + 8);
                    set(m, Reg::Ebp, nodes - index - 1);
                }
            }
            let vector = m.r32(module.module + 0x60);
            set(m, Reg::Edi, vector);
            let len = m.r32(vector + 8);
            let cap = m.r32(vector);
            set(m, Reg::Eax, len + 1);
            set(m, Reg::Ecx, cap);
            if len + 1 > cap {
                set(m, Reg::Ecx, cap + len + 1);
                stack::call(m, 0x411c7a, &[cap + len + 1], Some(vector), |m| {
                    mapping::grow(m, vector, cap + len + 1, e)
                })?;
            }
            set(m, Reg::Edx, m.r32(vector + 8));
            set(m, Reg::Eax, m.r32(vector + 4));
            set(m, Reg::Ecx, m.r32(b + 0x28));
            write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ecx));
            set(m, Reg::Ecx, m.r32(vector + 8));
            set(m, Reg::Eax, m.r32(b + 0x14));
            set(m, Reg::Ecx, r(m, Reg::Ecx) + 1);
            set(m, Reg::Eax, count - body - 1);
            write(m, vector + 8, r(m, Reg::Ecx));
            m.w32(b + 0x14, r(m, Reg::Eax));
        }
        set(m, Reg::Ebp, m.r32(b + 0x1c));
    }
    let selector = m.r32(module.module + 0x3c);
    let pool = m.r32(module.module + 0x24);
    set(m, Reg::Edx, selector);
    let name_index = stack::call(m, 0x411cad, &[selector], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, selector, 4, 8)
    })?;
    let name = stack::call(m, 0x411cb6, &[name_index], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, name_index, 0, 8)
    })?;
    stack::push(m, 0x1d8);
    write(m, module.module + 0x30, name);
    let class = allocate(m, 0x411cc3, 0x1d8, e)?;
    stack::add_sp(m, 4);
    let jvm = m.r32(b + 0x18);
    set(m, Reg::Ecx, jvm);
    stack::call(m, 0x411cd7, &[jvm, module.module], Some(class), |m| {
        crate::native_vm_class_context::construct(m, class, jvm, module.module, e)
    })?;
    set(m, Reg::Ecx, m.r32(b + 0x10));
    set(m, Reg::Esi, 0);
    if r(m, Reg::Ecx) == 0 {
        m.w32(b + 0x10, class);
    }
    module
        .completed
        .push(crate::native_vm_metadata::loader::LoadedModule {
            module: module.module,
            class,
        });
    set(m, Reg::Ebx, 0);
    let bytes = m.r32(b + 0x20);
    let remaining = m.r32(b + 0x2c);
    set(m, Reg::Ecx, bytes);
    set(m, Reg::Eax, remaining - bytes);
    set(m, Reg::Ebp, r(m, Reg::Ebp) + bytes);
    m.w32(b + 0x2c, remaining - bytes);
    module.pc = if remaining > bytes {
        0x411b47
    } else {
        0x411d03
    };
    Ok(class)
}

pub fn finish_class_load(
    m: &mut Space,
    pending: &mut PendingClassResolution,
    module: &mut ModuleLoad,
    e: &mut impl FileEnvironment,
) -> Result<u32, String> {
    if pending.pc != 0x411b20 || module.pc != 0x411d03 || stack::has_pending_call(m) {
        return Err("NativeVmSourceLoader: completed source module caller required".into());
    }
    let class = stack::load(m, 0x10);
    let first_module = m.r32(class + 4);
    if class == 0
        || module
            .completed
            .first()
            .is_none_or(|first| first.class != class || first.module != first_module)
    {
        return Err("NativeVmSourceLoader: returned class does not belong to its module".into());
    }
    set(m, Reg::Eax, class);
    for reg in [Reg::Edi, Reg::Esi, Reg::Ebp, Reg::Ebx] {
        pop(m, reg);
    }
    stack::add_sp(m, 0x14);
    stack::leave(m, module.caller);
    let b = stack::sp(m);
    let file = m.r32(b + 0x10);
    set(m, Reg::Ecx, file);
    stack::push(m, file);
    set(m, Reg::Ebx, class);
    let freed = stack::external(m, 0x4104d4, &[], None, |m| e.free(m, file))?;
    set(m, Reg::Eax, freed);
    set(m, Reg::Esi, first_module);
    let selector = m.r32(first_module + 0x3c);
    let pool = m.r32(first_module + 0x24);
    set(m, Reg::Edx, selector);
    set(m, Reg::Ecx, pool);
    stack::add_sp(m, 4);
    let index = stack::call(m, 0x4104e6, &[selector], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, selector, 4, 8)
    })?;
    let name = stack::call(m, 0x4104ef, &[index], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, index, 0, 8)
    })?;
    set(m, Reg::Esi, name);
    stack::push(m, 0x2e);
    stack::push(m, name);
    let last = stack::call(m, 0x4104f9, &[], None, |m| {
        resolution::last_character(m, name, 0x2e)
    });
    if last < name {
        return Err("NativeVmSourceLoader: returned class has no namespace".into());
    }
    let capacity = last - name + 1;
    set(m, Reg::Ecx, 1u32.wrapping_sub(name));
    set(m, Reg::Eax, capacity);
    stack::push(m, capacity);
    let namespace = b + 0x114;
    set(m, Reg::Edx, namespace);
    stack::push(m, name);
    stack::push(m, namespace);
    stack::call(m, 0x410511, &[], None, |m| {
        bounded_copy(m, namespace, name, capacity)
    });
    set(m, Reg::Ecx, m.r32(pending.jvm + 0x7c));
    stack::add_sp(m, 0x14);
    set(m, Reg::Eax, namespace);
    let mapped = stack::call(
        m,
        0x410525,
        &[pending.path, namespace],
        Some(m.r32(pending.jvm + 0x7c)),
        |m| mapping::map(m, m.r32(pending.jvm + 0x7c), pending.path, namespace, e),
    )?;
    if (mapped as i32) <= 0 {
        return Err("NativeVmSourceLoader: source namespace publication failed".into());
    }
    pop(m, Reg::Edi);
    pop(m, Reg::Esi);
    pop(m, Reg::Ebp);
    set(m, Reg::Eax, class);
    pop(m, Reg::Ebx);
    stack::add_sp(m, 0x204);
    stack::leave(m, pending.loader);
    for reg in [Reg::Edi, Reg::Esi, Reg::Ebp, Reg::Ebx] {
        pop(m, reg);
    }
    stack::add_sp(m, 0x120);
    stack::leave(m, pending.namespace);
    pop(m, Reg::Esi);
    pop(m, Reg::Ebx);
    stack::add_sp(m, 0x220);
    stack::leave(m, pending.lookup);
    pending.pc = pending.return_pc;
    module.pc = pending.return_pc;
    Ok(class)
}
