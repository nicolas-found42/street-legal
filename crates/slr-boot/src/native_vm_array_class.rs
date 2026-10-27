//! Retained4109e0 array metadata construction from the actual descriptor CALL.
//! The prefix stops before constructing the synthetic length-field record.
use crate::native_stack::{self as stack, Frame, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
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
fn length(m: &Space, p: u32) -> Result<u32, String> {
    (0..0x10000)
        .find(|i| m.r8(p + i) == 0)
        .ok_or_else(|| "NativeVmArrayClass: unterminated descriptor".into())
}
fn allocate(m: &mut Space, pc: u32, n: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| a.allocate(m, n))?;
    s(m, Reg::Eax, p);
    stack::add_sp(m, 4);
    if p == 0 {
        return Err("NativeVmArrayClass: allocation failure".into());
    }
    Ok(p)
}
fn pool(m: &mut Space, p: u32) {
    stack::function(m, &[], Some(p), |m| {
        s(m, Reg::Eax, p);
        s(m, Reg::Ecx, 0);
        for off in [8, 16, 12] {
            write(m, p + off, 0);
        }
        write(m, p + 4, 0x5e736c);
        write(m, p, 0x5e735c);
    })
}
pub struct Prefix {
    pub frame: Frame,
    pub module: u32,
    pub pc: u32,
    pub jvm: u32,
    pub base: u32,
    pub name: u32,
}
enum Lookup {
    Cached(u32),
    Fresh(Prefix),
}
fn lookup(m: &mut Space, a: &mut impl Allocator) -> Result<Lookup, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmArrayClass: actual pending array-class CALL required".into());
    }
    let jvm = r(m, Reg::Ecx);
    let descriptor = stack::argument(m, 0);
    if jvm == 0 || m.r8(descriptor) != b'[' {
        return Err("NativeVmArrayClass: JVM and array descriptor required".into());
    }
    let len = length(m, descriptor)?;
    if len > 200 {
        return Err("NativeVmArrayClass: descriptor outside bounded source buffers".into());
    }
    let frame = stack::enter(m, &[descriptor], Some(jvm));
    stack::alloc(m, 0x428);
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Ebp);
    s(m, Reg::Ebp, descriptor);
    stack::push_reg(m, Reg::Esi);
    s(m, Reg::Esi, jvm);
    stack::push_reg(m, Reg::Edi);
    let base = stack::sp(m);
    write(m, base + 0x10, jvm);
    s(m, Reg::Ebx, 0);
    s(m, Reg::Esi, 0);
    s(m, Reg::Eax, descriptor + 1);
    let mut dimensions = 0;
    loop {
        low(m, Reg::Ecx, m.r8(descriptor + dimensions + 1));
        dimensions += 1;
        s(m, Reg::Esi, dimensions);
        s(m, Reg::Eax, descriptor + dimensions + 1);
        if m.r8(descriptor + dimensions) != b'[' {
            break;
        }
    }
    low(m, Reg::Edx, m.r8(0x62f120));
    s(m, Reg::Eax, 0);
    let buffer = base + 0x38;
    m.w8(buffer, r(m, Reg::Edx) as u8);
    s(m, Reg::Ecx, 0x7f);
    s(m, Reg::Edi, buffer + 1);
    for i in 0..127 {
        write(m, buffer + 1 + i * 4, 0);
    }
    s(m, Reg::Ecx, 0);
    s(m, Reg::Edi, buffer + 509);
    m.w8(buffer + 509, 0);
    m.w8(buffer + 510, 0);
    s(m, Reg::Edi, buffer + 511);
    m.w8(buffer + 511, 0);
    s(m, Reg::Edi, buffer + 512);
    write(m, base + 0x14, 1);
    let mut name = descriptor;
    if m.r8(descriptor + dimensions) == b'L' {
        s(m, Reg::Edi, dimensions);
        for i in (0..dimensions).rev() {
            stack::push(m, 256);
            s(m, Reg::Eax, buffer);
            stack::push(m, 0x60adc4);
            stack::push(m, buffer);
            stack::call(m, 0x410a53, &[], None, |m| {
                crate::native_vm_callback::append(m, buffer, 0x60adc4, 256)
            })?;
            stack::add_sp(m, 12);
            s(m, Reg::Edi, i);
        }
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        s(m, Reg::Edi, descriptor + len + 1);
        s(m, Reg::Ecx, len - dimensions);
        let limit = len - dimensions;
        stack::push(m, limit);
        s(m, Reg::Ecx, descriptor + dimensions + 1);
        stack::push(m, descriptor + dimensions + 1);
        s(m, Reg::Edx, buffer);
        stack::push(m, buffer);
        stack::call(m, 0x410a77, &[], None, |m| {
            crate::native_vm_callback::append(m, buffer, descriptor + dimensions + 1, limit)
        })?;
        s(m, Reg::Eax, buffer);
        stack::add_sp(m, 12);
        write(m, base + 0x43c, buffer);
        write(m, base + 0x14, 0);
        s(m, Reg::Ebp, buffer);
        name = buffer;
    }
    let name_len = length(m, name)?;
    s(m, Reg::Ecx, u32::MAX);
    s(m, Reg::Eax, 0);
    s(m, Reg::Edi, name + name_len + 1);
    s(m, Reg::Ecx, !(name_len + 1));
    s(m, Reg::Edx, 1u32.wrapping_sub(dimensions));
    s(m, Reg::Ecx, name_len + 1 - dimensions);
    let limit = name_len + 1 - dimensions;
    stack::push(m, limit);
    s(m, Reg::Esi, name + dimensions);
    let tail = base + 0x238;
    s(m, Reg::Eax, tail);
    stack::push(m, name + dimensions);
    stack::push(m, tail);
    stack::call(m, 0x410ab1, &[], None, |m| {
        crate::native_vm_metadata::name_copy(m, tail, name + dimensions, limit)
    });
    s(m, Reg::Edi, jvm);
    stack::add_sp(m, 12);
    stack::push(m, name);
    s(m, Reg::Ecx, jvm);
    let cached = stack::call(m, 0x410ac0, &[], Some(jvm), |m| {
        crate::native_vm_lookup::find_class(m, jvm, name)
    })?;
    stack::add_sp(m, 4);
    if cached != 0 {
        stack::leave(m, frame);
        return Ok(Lookup::Cached(cached));
    }
    stack::push(m, 100);
    let module = allocate(m, 0x410acf, 100, a)?;
    s(m, Reg::Esi, module);
    s(m, Reg::Ecx, module + 16);
    write(m, module, 0x5f08a4);
    write(m, module + 8, 0);
    write(m, module + 4, 0);
    stack::call(m, 0x410af0, &[], Some(module + 16), |m| {
        pool(m, module + 16)
    });
    stack::push(m, 16);
    write(m, module, 0x5f08a0);
    write(m, module + 12, jvm);
    for (index, (pc, ctor_pc, offset, method)) in [
        (0x410b00, 0x410b0e, 0x50, true),
        (0x410b1c, 0x410b2a, 0x4c, false),
        (0x410b38, 0x410b46, 0x58, true),
        (0x410b54, 0x410b62, 0x54, false),
    ]
    .into_iter()
    .enumerate()
    {
        let p = allocate(m, pc, 16, a)?;
        s(m, Reg::Ecx, p);
        stack::call(m, ctor_pc, &[], Some(p), |m| {
            stack::function(m, &[], Some(p), |m| {
                s(m, Reg::Eax, p);
                s(m, Reg::Ecx, 0);
                for off in [4, 12, 8] {
                    write(m, p + off, 0);
                }
                write(m, p, if method { 0x5f08a8 } else { 0x5f08b8 });
            })
        });
        stack::push(m, if index < 3 { 16 } else { 12 });
        write(m, module + offset, p);
    }
    let vars = allocate(m, 0x410b70, 12, a)?;
    for off in [0, 8, 4] {
        write(m, vars + off, 0);
    }
    stack::push(m, 12);
    write(m, module + 0x5c, vars);
    let tree = allocate(m, 0x410b8d, 12, a)?;
    s(m, Reg::Ecx, tree);
    stack::call(m, 0x410b9b, &[], Some(tree), |m| {
        stack::function(m, &[], Some(tree), |m| {
            s(m, Reg::Eax, tree);
            s(m, Reg::Ecx, 0);
            for off in [0, 8, 4] {
                write(m, tree + off, 0);
            }
        })
    });
    stack::push(m, 20);
    write(m, module + 0x60, tree);
    let constants = allocate(m, 0x410ba9, 20, a)?;
    s(m, Reg::Ecx, constants);
    stack::call(m, 0x410bb7, &[], Some(constants), |m| pool(m, constants));
    write(m, module + 0x24, constants);
    for off in [0x28, 0x34, 0x48, 0x30] {
        write(m, module + off, 0);
    }
    Ok(Lookup::Fresh(Prefix {
        frame,
        module,
        pc: 0x410bdd,
        jvm,
        base,
        name,
    }))
}
impl Prefix {
    pub fn finish(
        self,
        m: &mut Space,
        a: &mut impl crate::native_vm_context::Environment,
    ) -> Result<u32, String> {
        if self.pc != 0x410bdd || stack::sp(m) != self.base || r(m, Reg::Esi) != self.module {
            return Err("NativeVmArrayClass: retained metadata prefix required".into());
        }
        let module = self.module;
        let base = self.base;
        let pool = m.r32(module + 0x24);
        stack::push(m, 20);
        for off in [0x44, 0x48, 0x34, 0x28] {
            write(m, module + off, 0);
        }
        let field = allocate(m, 0x410bf0, 20, a)?;
        s(m, Reg::Ebx, field);
        s(m, Reg::Edx, pool);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        let n = length(m, 0x60adc8)?;
        s(m, Reg::Edi, 0x60adc8 + n + 1);
        s(m, Reg::Ecx, n);
        stack::push(m, n);
        stack::push(m, 0x60adc8);
        s(m, Reg::Ecx, pool);
        let ty = stack::call(m, 0x410c18, &[], Some(pool), |m| {
            crate::native_vm_lookup::literal(m, pool, 0x60adc8, n, a)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Edx, pool);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Ebp, ty);
        s(m, Reg::Eax, 0);
        let n = length(m, 0x60adcc)?;
        s(m, Reg::Edi, 0x60adcc + n + 1);
        s(m, Reg::Ecx, n);
        stack::push(m, n);
        stack::push(m, 0x60adcc);
        s(m, Reg::Ecx, pool);
        let name_index = stack::call(m, 0x410c39, &[], Some(pool), |m| {
            crate::native_vm_lookup::literal(m, pool, 0x60adcc, n, a)
        })?;
        stack::add_sp(m, 8);
        write(m, field + 12, ty);
        s(m, Reg::Ebp, m.r32(base + 0x43c));
        write(m, field, 0x5f0890);
        write(m, field + 4, 0x21);
        write(m, field + 8, name_index);
        write(m, field + 16, u32::MAX);
        let members = m.r32(module + 0x4c);
        s(m, Reg::Edi, members);
        s(m, Reg::Ecx, m.r32(members + 12));
        let vector = members + 4;
        s(m, Reg::Edi, vector);
        let wanted = r(m, Reg::Ecx) + 1;
        s(m, Reg::Ecx, wanted);
        stack::push(m, wanted);
        s(m, Reg::Ecx, vector);
        stack::call(m, 0x410c70, &[], Some(vector), |m| {
            crate::native_vm_values::reserve(m, vector, wanted, a)
        })?;
        stack::add_sp(m, 4);
        s(m, Reg::Edx, m.r32(vector + 8));
        s(m, Reg::Eax, m.r32(vector + 4));
        write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, field);
        s(m, Reg::Ecx, m.r32(vector + 8));
        s(m, Reg::Eax, m.r32(base + 0x14));
        s(m, Reg::Ecx, r(m, Reg::Ecx) + 1);
        write(m, vector + 8, r(m, Reg::Ecx));
        if r(m, Reg::Eax) != 0 {
            write(m, module + 0x38, 0);
        } else {
            let tail = base + 0x238;
            s(m, Reg::Ecx, tail);
            stack::push(m, tail);
            s(m, Reg::Ecx, self.jvm);
            let element = stack::call(m, 0x410ca2, &[], Some(self.jvm), |m| {
                crate::native_vm_lookup::module_from_name_with(m, self.jvm, tail, a)
            })?;
            stack::add_sp(m, 4);
            s(m, Reg::Edx, m.r32(element + 0x38));
            write(m, module + 0x38, r(m, Reg::Edx));
        }
        s(m, Reg::Edx, pool);
        s(m, Reg::Ecx, u32::MAX);
        s(m, Reg::Eax, 0);
        let name = self.name;
        let n = length(m, name)?;
        s(m, Reg::Edi, name + n + 1);
        s(m, Reg::Ecx, n);
        stack::push(m, n);
        stack::push(m, name);
        s(m, Reg::Ecx, pool);
        let ni = stack::call(m, 0x410cc0, &[], Some(pool), |m| {
            crate::native_vm_lookup::literal(m, pool, name, n, a)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Ecx, pool);
        stack::push(m, ni);
        let ci = stack::call(m, 0x410cc9, &[], Some(pool), |m| {
            crate::native_vm_lookup::class_constant(m, pool, ni, a)
        })?;
        stack::add_sp(m, 4);
        s(m, Reg::Edx, pool);
        s(m, Reg::Ecx, u32::MAX);
        write(m, module + 0x3c, ci);
        s(m, Reg::Eax, 0);
        let n = length(m, 0x60add4)?;
        s(m, Reg::Edi, 0x60add4 + n + 1);
        s(m, Reg::Ecx, n);
        stack::push(m, n);
        stack::push(m, 0x60add4);
        s(m, Reg::Ecx, pool);
        let ni = stack::call(m, 0x410ceb, &[], Some(pool), |m| {
            crate::native_vm_lookup::literal(m, pool, 0x60add4, n, a)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Ecx, pool);
        stack::push(m, ni);
        let base_index = stack::call(m, 0x410cf4, &[], Some(pool), |m| {
            crate::native_vm_lookup::class_constant(m, pool, ni, a)
        })?;
        stack::add_sp(m, 4);
        stack::push(m, 0x1d8);
        write(m, module + 0x40, base_index);
        let class = allocate(m, 0x410d01, 0x1d8, a)?;
        s(m, Reg::Ecx, self.jvm);
        stack::push(m, module);
        stack::push(m, self.jvm);
        s(m, Reg::Ecx, class);
        let result = stack::call(m, 0x410d19, &[], Some(class), |m| {
            crate::native_vm_class_context::construct(m, class, self.jvm, module, a)
        })?;
        stack::add_sp(m, 8);
        stack::leave(m, self.frame);
        Ok(result)
    }
}
pub fn begin(m: &mut Space, a: &mut impl Allocator) -> Result<Prefix, String> {
    match lookup(m, a)? {
        Lookup::Fresh(prefix) => Ok(prefix),
        Lookup::Cached(_) => {
            Err("NativeVmArrayClass: cached class already returned; fresh prefix required".into())
        }
    }
}
pub fn get(
    m: &mut Space,
    a: &mut impl crate::native_vm_context::Environment,
) -> Result<u32, String> {
    match lookup(m, a)? {
        Lookup::Fresh(prefix) => prefix.finish(m, a),
        Lookup::Cached(class) => Ok(class),
    }
}
