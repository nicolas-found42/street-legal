//! Source420ff0 retained interpreter entry and42108b opcode prefetch.
//! This consumes an actual pending source CALL and live raw metadata. The
//! continuation remains entered; no implicit unwind or direct-driver fallback.
use crate::{
    native_stack::{self as stack, Frame, Reg},
    native_vm_fpu,
};
use slr_physics::space::Space;
use slr_x87::Arithmetic;

fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
}

pub trait Environment:
    crate::native_vm_dispatch::Environment + crate::native_vm_lookup::Environment
{
    fn resource(&mut self, _m: &mut Space, _package: u32, _id: u32) -> Result<u32, String> {
        Err("NativeVmRun: resource-table collaborator unavailable".into())
    }
    fn query_frequency(&mut self, m: &mut Space, destination: u32) -> Result<u32, String>;
    fn query_counter(&mut self, m: &mut Space, destination: u32) -> Result<u32, String>;
    fn resolve_resource(
        &mut self,
        _m: &mut Space,
        _manager: u32,
        _id: u32,
        _kind: u32,
        _flags: u32,
    ) -> Result<u32, String> {
        Err("NativeVmRun: resource-manager collaborator unavailable".into())
    }
    fn native(
        &mut self,
        _m: &mut Space,
        _fpu: &mut native_vm_fpu::State,
        target: u32,
        _info: u32,
        _kind: crate::native_vm_abi::ReturnKind,
    ) -> Result<u32, String> {
        Err(format!(
            "NativeVmRun: source native {target:#x} continuation unavailable"
        ))
    }
    fn observe(&mut self, _m: &Space, _fpu: &native_vm_fpu::State, _pc: u32) {}
}
#[must_use]
pub struct Run {
    pub caller: Frame,
    pub thread: u32,
    pub base: u32,
    pub pc: u32,
    native: Option<NativeCall>,
    returned: bool,
    budget: u32,
    instruction: Option<Instruction>,
}
#[derive(Clone, Copy)]
struct Instruction {
    pc: u32,
    opcode: u16,
    operand: u16,
    immediate: u32,
}

struct NativeCall {
    dispatch: crate::native_vm_dispatch::PendingDispatch,
    entry: crate::native_vm_abi::Entry,
    record: u32,
}
impl Run {
    pub fn has_returned(&self) -> bool {
        self.returned
    }
    pub fn has_pending_native(&self) -> bool {
        self.native.is_some()
    }
    pub fn native_entry(&self, m: &Space) -> Result<&crate::native_vm_abi::Entry, String> {
        let native = self
            .native
            .as_ref()
            .ok_or("NativeVmRun: no pending native entry")?;
        native.entry.validate(m)?;
        Ok(&native.entry)
    }
    /// Execute the produced CALL, resume its dispatcher, and return to the
    /// retained interpreter. The closure must execute the actual source native.
    pub fn resume_native<E: Environment>(
        &mut self,
        m: &mut Space,
        fpu: &mut native_vm_fpu::State,
        env: &mut E,
        native: impl FnOnce(
            &mut Space,
            &mut native_vm_fpu::State,
            &mut E,
            u32,
            u32,
            crate::native_vm_abi::ReturnKind,
        ) -> Result<u32, String>,
    ) -> Result<(), String> {
        self.native_entry(m)?;
        let call = self
            .native
            .take()
            .ok_or("NativeVmRun: no pending native entry")?;
        let kind = call.entry.kind();
        let target = call.entry.target();
        let result = call.entry.execute(m, |m, target, info, kind| {
            native(m, fpu, env, target, info, kind)
        })??;
        let result = if kind == crate::native_vm_abi::ReturnKind::Float {
            fpu.fstp_f32()?
        } else {
            result
        };
        env.observe_context(
            m,
            crate::native_vm_context::Observation::NativeReturn {
                thread: self.thread,
                implementation: target,
                result: match kind {
                    crate::native_vm_abi::ReturnKind::Absent
                    | crate::native_vm_abi::ReturnKind::Void => None,
                    _ => Some(result),
                },
            },
        )?;
        let result = crate::native_vm_dispatch::finish_cached(m, call.dispatch, kind, result, env)?;
        s(m, Reg::Edi, result);
        s(m, Reg::Eax, m.r32(call.record + 0xc));
        if r(m, Reg::Eax) != 0 {
            stack::external(m, 0x421388, &[call.record], None, |m| {
                env.free(m, call.record)
            })?;
        }
        self.pc = 0x421375;
        advance(m, self, result != 0, env)
    }
}
///47bfe0 System.currentTime: return ST(0) through the real clock CALL.
/// The dispatcher performs its source FSTP after the native returns.
pub fn current_time(
    m: &mut Space,
    info: u32,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[info], |m| {
        stack::call(m, 0x47bfe5, &[], None, |m| clock(m, fpu, env))?;
        fpu.arithmetic_f32(Arithmetic::Multiply, m.r32(0x5f10a4))?;
        Ok(0)
    })
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
fn signed_half(m: &Space, p: u32) -> u32 {
    i32::from(i16::from_le_bytes([m.r8(p), m.r8(p + 1)])) as u32
}
fn qword(m: &Space, p: u32) -> i64 {
    (u64::from(m.r32(p)) | (u64::from(m.r32(p + 4)) << 32)) as i64
}
/// Original5516c0: both Win32 BOOLs are ignored; existing destination qwords
/// survive a failed query. All FP operations execute in original order.
pub fn clock(
    m: &mut Space,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    stack::function(m, &[], None, |m| {
        for (pc, destination, frequency) in
            [(0x5516cb, 0x76a280, true), (0x5516d6, 0x76a290, false)]
        {
            let result = stack::call(m, pc, &[destination], None, |m| {
                stack::function(m, &[destination], None, |m| {
                    if frequency {
                        env.query_frequency(m, destination)
                    } else {
                        env.query_counter(m, destination)
                    }
                })
            })?;
            s(m, Reg::Eax, result);
        }
        fpu.fild_i64(qword(m, 0x76a290))?;
        fpu.fild_i64(qword(m, 0x76a288))?;
        fpu.arithmetic_pop(Arithmetic::Subtract)?;
        fpu.fild_i64(qword(m, 0x76a280))?;
        fpu.arithmetic_pop(Arithmetic::Divide)?;
        fpu.arithmetic_f32(Arithmetic::Multiply, m.r32(0x5f0910))
    })
}
fn pool_name(m: &Space, thread: u32) -> Result<(u32, u32, u32, u32, u32), String> {
    let frame = m.r32(thread.wrapping_add(0x20));
    let class = m.r32(frame.wrapping_add(0x34));
    let module = m.r32(class.wrapping_add(4));
    let pool = m.r32(module.wrapping_add(0x24));
    let index = m.r32(module.wrapping_add(0x3c));
    if frame == 0
        || class == 0
        || module == 0
        || pool == 0
        || (index as i32) < 0
        || index >= m.r32(pool + 8)
    {
        return Err("NativeVmRun: missing live frame/class/module/pool context".into());
    }
    let constant = m.r32(m.r32(pool + 12).wrapping_add(index * 4));
    if constant == 0 || m.r32(constant + 4) != 4 {
        return Err("NativeVmRun: class constant unavailable".into());
    }
    let name_index = m.r32(constant + 8);
    if (name_index as i32) < 0 || name_index >= m.r32(pool + 8) {
        return Err("NativeVmRun: class name index out of range".into());
    }
    let name = m.r32(m.r32(pool + 12).wrapping_add(name_index * 4));
    if name == 0 || m.r32(name + 4) != 0 || m.r32(name + 8) == 0 {
        return Err("NativeVmRun: class name string unavailable".into());
    }
    Ok((module, pool, index, constant, name))
}
/// Enter the ordinary nonempty/not-already-running branch through4210c5.
/// The original fatal branches remain explicit unsupported inputs.
pub fn begin(
    m: &mut Space,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<Run, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRun: actual pending Thread.run CALL required".into());
    }
    let thread = r(m, Reg::Ecx);
    let budget = stack::argument(m, 0);
    let jvm = m.r32(thread.wrapping_add(0x10));
    let frame = m.r32(thread.wrapping_add(0x20));
    if thread == 0
        || jvm == 0
        || frame == 0
        || m.r32(thread + 0x24) == 0
        || m.r32(frame + 0x2c) == 0
        || m.r32(thread + 0x2c) & 4 != 0
    {
        return Err("NativeVmRun: missing/empty/already-running source thread".into());
    }
    pool_name(m, thread)?;
    // Reject an unsupported FP path without consuming the actual caller.
    let mut checked = fpu.clone();
    checked.fld_f32(budget)?;
    checked.fcomp_f32(m.r32(0x5e73cc))?;
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::Run {
            thread,
            enter: true,
            budget_bits: budget,
            pc: m.r32(thread + 0x24),
            flags: m.r32(thread + 0x2c),
        },
    )?;
    let caller = stack::enter(m, &[budget], Some(thread));
    stack::alloc(m, 0x44c);
    stack::push_reg(m, Reg::Ebx);
    s(m, Reg::Ebx, thread);
    s(m, Reg::Eax, m.r32(thread + 0x24));
    for reg in [Reg::Ebp, Reg::Esi, Reg::Edi] {
        stack::push_reg(m, reg);
    }
    stack::store(m, 0x40, thread);
    s(m, Reg::Eax, jvm);
    fpu.fld_f32(stack::load(m, 0x460))?;
    s(m, Reg::Edx, m.r32(jvm + 0x78));
    s(m, Reg::Edi, thread + 0x10);
    write(m, jvm + 0x78, thread);
    fpu.fcomp_f32(m.r32(0x5e73cc))?;
    stack::store(m, 0x10, r(m, Reg::Edi));
    stack::store(m, 0x4c, r(m, Reg::Edx));
    m.w8(stack::sp(m) + 0x23, 1);
    s(
        m,
        Reg::Eax,
        (r(m, Reg::Eax) & 0xffff0000) | u32::from(fpu.status()),
    );
    if fpu.status() & 0x4000 == 0 {
        m.w8(stack::sp(m) + 0x23, 0);
    }
    env.observe(m, fpu, 0x421061);
    stack::call(m, 0x421066, &[], None, |m| clock(m, fpu, env))?;
    fpu.arithmetic_f32(Arithmetic::Add, stack::load(m, 0x460))?;
    s(m, Reg::Eax, m.r32(thread + 0x20));
    s(m, Reg::Ecx, m.r32(r(m, Reg::Eax) + 0x2c));
    write(m, thread + 0x24, r(m, Reg::Ecx));
    let deadline = fpu.fstp_f32()?;
    stack::store(m, 0x44, deadline);
    s(m, Reg::Eax, m.r32(thread + 0x2c) | 4);
    stack::store(m, 0x38, 0);
    write(m, thread + 0x2c, r(m, Reg::Eax));
    let mut run = Run {
        caller,
        thread,
        base: stack::sp(m),
        pc: 0x42108b,
        native: None,
        returned: false,
        budget,
        instruction: None,
    };
    prefetch(m, &mut run)?;
    Ok(run)
}
/// The loop's real filename/line/opcode loads. Both raw pool calls retain
/// their actual CALL/push stores rather than assigning a diagnostic label.
pub fn prefetch(m: &mut Space, run: &mut Run) -> Result<u16, String> {
    if !stack::active(m) || stack::sp(m) != run.base || run.pc != 0x42108b {
        return Err("NativeVmRun: missing retained interpreter continuation".into());
    }
    let thread = run.thread;
    let (module, pool, index, constant, name) = pool_name(m, thread)?;
    let pc = m.r32(thread + 0x24);
    let jvm = m.r32(thread + 0x10);
    if pc == 0 || jvm == 0 {
        return Err("NativeVmRun: missing current PC/JVM".into());
    }
    s(m, Reg::Edx, pc);
    s(m, Reg::Eax, signed_half(m, pc + 2));
    s(m, Reg::Ecx, jvm);
    s(m, Reg::Ebp, thread + 0x24);
    write(m, jvm + 0x70, r(m, Reg::Eax));
    s(m, Reg::Edx, m.r32(thread + 0x20));
    s(m, Reg::Eax, m.r32(r(m, Reg::Edx) + 0x34));
    s(m, Reg::Eax, module);
    s(m, Reg::Ecx, pool);
    s(m, Reg::Esi, module + 0x24);
    s(m, Reg::Eax, index);
    stack::call(m, 0x4210b2, &[index], Some(pool), |m| {
        stack::function(m, &[index], Some(pool), |m| {
            s(m, Reg::Eax, index);
            s(m, Reg::Ecx, m.r32(pool + 12));
            s(m, Reg::Eax, constant);
            s(m, Reg::Eax, m.r32(constant + 8));
        })
    });
    let name_index = r(m, Reg::Eax);
    s(m, Reg::Ecx, m.r32(r(m, Reg::Esi)));
    stack::call(m, 0x4210ba, &[name_index], Some(pool), |m| {
        stack::function(m, &[name_index], Some(pool), |m| {
            s(m, Reg::Eax, name_index);
            s(m, Reg::Ecx, m.r32(pool + 12));
            s(m, Reg::Eax, name);
            s(m, Reg::Ecx, 0);
            s(m, Reg::Eax, m.r32(name + 8));
        })
    });
    s(m, Reg::Ecx, m.r32(r(m, Reg::Edi)));
    write(m, r(m, Reg::Ecx) + 0x74, r(m, Reg::Eax));
    s(m, Reg::Ecx, m.r32(r(m, Reg::Ebp)));
    s(m, Reg::Eax, signed_half(m, r(m, Reg::Ecx)));
    run.pc = 0x4210c5;
    Ok(r(m, Reg::Eax) as u16)
}

// Original41a570 ordinary vector growth. Capacity is old+required, not a
// power-of-two guess; old storage and retained allocator CALL bytes survive.
fn grow(
    m: &mut Space,
    vector: u32,
    required: u32,
    env: &mut impl Environment,
) -> Result<(), String> {
    crate::native_vm_values::reserve(m, vector, required, env)
}

fn named_field(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let thread = run.thread;
    let frame = m.r32(thread + 0x20);
    s(m, Reg::Eax, frame);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Ecx, vector);
    let mut cell = stack::call(m, 0x4237d9, &[], Some(vector), |m| pop_operand(m, vector))?;
    s(m, Reg::Esi, cell);
    let vt = m.r32(cell);
    s(m, Reg::Edx, vt);
    s(m, Reg::Ecx, cell);
    stack::store(m, 0x18, cell);
    if !matches!(vt, 0x5e7344 | 0x5e734c) {
        return Err("NativeVmRun: named field operand virtual type unavailable".into());
    }
    stack::call(m, 0x4237e5, &[], Some(cell), |m| {
        stack::function(m, &[], Some(cell), |m| {
            s(m, Reg::Eax, if vt == 0x5e7344 { 2 } else { 1 })
        })
    });
    if vt == 0x5e7344 {
        let pool = m.r32(0x62f130);
        s(m, Reg::Ecx, pool);
        let copy = stack::call(m, 0x4237f5, &[], Some(pool), |m| {
            crate::native_vm_values::allocate(m, pool, env)
        })?;
        cell = stack::call(m, 0x423801, &[cell], Some(copy), |m| {
            copy_value(m, copy, cell)
        });
        s(m, Reg::Esi, cell);
        stack::store(m, 0x18, cell);
    }
    let object = m.r32(cell + 8);
    s(m, Reg::Eax, object);
    s(m, Reg::Esi, cell + 8);
    if object == 0 {
        return Err(
            "NativeVmRun: named field null receiver diagnostic continuation unavailable".into(),
        );
    }
    let node = m.r32(thread + 0x24);
    s(m, Reg::Eax, node);
    s(m, Reg::Ecx, frame);
    let index = m.r32(node + 4);
    s(m, Reg::Eax, index);
    let class = m.r32(frame + 0x34);
    s(m, Reg::Ecx, class);
    let pool = m.r32(class + 0x14);
    s(m, Reg::Ecx, pool);
    let name = stack::call(m, 0x42382a, &[index], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, index, 0, 8)
    })?;
    s(m, Reg::Ecx, object);
    let field = stack::call(m, 0x423832, &[name], Some(object), |m| {
        crate::native_vm_lookup::object_field_by_name(m, object, name)
    })?;
    stack::store(m, 0x58, field);
    if field == 0 {
        return Err(
            "NativeVmRun: named field missing member diagnostic continuation unavailable".into(),
        );
    }
    s(m, Reg::Eax, m.r32(thread + 0x20));
    let vector = m.r32(r(m, Reg::Eax) + 0x28);
    s(m, Reg::Edi, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Eax, count + 1);
    s(m, Reg::Ecx, m.r32(vector));
    s(m, Reg::Esi, vector + 8);
    if count + 1 > m.r32(vector) {
        let capacity = m.r32(vector) + count + 1;
        s(m, Reg::Ecx, capacity);
        stack::call(m, 0x4238cb, &[capacity], Some(vector), |m| {
            crate::native_vm_values::resize(m, vector, capacity, env)
        })?;
    }
    s(m, Reg::Ecx, m.r32(vector + 8));
    s(m, Reg::Edx, m.r32(vector + 4));
    s(m, Reg::Eax, field);
    write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, field);
    s(m, Reg::Ecx, m.r32(vector + 8) + 1);
    s(m, Reg::Eax, stack::load(m, 0x18));
    write(m, vector + 8, r(m, Reg::Ecx));
    if r(m, Reg::Eax) != 0 {
        s(m, Reg::Ecx, r(m, Reg::Eax));
        s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
        stack::call(m, 0x4238ed, &[1], Some(cell), |m| {
            crate::native_vm_values::release(m, cell, env)
        })?;
    }
    Ok(())
}
fn field_ref(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    use crate::native_vm_lookup as lookup;
    let thread = run.thread;
    let frame = m.r32(thread + 0x20);
    let node = m.r32(thread + 0x24);
    let class = m.r32(frame + 0x34);
    let pool = m.r32(class + 0x14);
    let index = m.r32(node + 4);
    if pool == 0 || (index as i32) < 0 || index >= m.r32(pool + 8) {
        return Err("NativeVmRun: field constant index unavailable".into());
    }
    let constant = m.r32(m.r32(pool + 12) + index * 4);
    let mut selector = m.r32(constant.wrapping_add(0x10));
    if constant == 0 || m.r32(constant + 4) != 5 {
        return Err(
            "NativeVmRun: uncached field resolution requires its source continuation".into(),
        );
    }
    let operands = m.r32(frame + 0x28);
    if operands == 0 || m.r32(operands + 8) > m.r32(operands) {
        return Err("NativeVmRun: field operand container unavailable".into());
    }
    s(m, Reg::Eax, frame);
    s(m, Reg::Edi, index);
    s(m, Reg::Eax, class);
    s(m, Reg::Ecx, pool);
    stack::call(m, 0x4234c6, &[index], Some(pool), |m| {
        lookup::constant(m, pool, index, 5, 16)
    })?;
    s(m, Reg::Esi, selector);
    if selector == u32::MAX {
        s(m, Reg::Ecx, frame);
        s(m, Reg::Ebp, stack::load(m, 0x10));
        s(m, Reg::Eax, class);
        let jvm = m.r32(r(m, Reg::Ebp));
        s(m, Reg::Edx, jvm);
        stack::call(m, 0x4234e4, &[jvm, index], Some(pool), |m| {
            lookup::resolve_method_reference(m, pool, jvm, index, env)
        })?;
        s(m, Reg::Eax, frame);
        s(m, Reg::Eax, class);
        selector = stack::call(m, 0x4234f3, &[index], Some(pool), |m| {
            lookup::constant(m, pool, index, 5, 16)
        })?;
        s(m, Reg::Esi, selector);
    }
    s(m, Reg::Ebp, stack::load(m, 0x10));
    s(m, Reg::Ecx, frame);
    if selector & 0x40000000 != 0 {
        s(m, Reg::Eax, class);
        let class_index = stack::call(m, 0x423512, &[index], Some(pool), |m| {
            lookup::constant(m, pool, index, 5, 8)
        })?;
        s(m, Reg::Edx, frame);
        s(m, Reg::Ecx, class);
        let field = selector & 0xbfffffff;
        s(m, Reg::Esi, field);
        stack::push(m, field);
        s(m, Reg::Eax, pool);
        let jvm = m.r32(r(m, Reg::Ebp));
        let owner = stack::call(m, 0x42352c, &[pool, class_index], Some(jvm), |m| {
            lookup::class_from_pool_with(m, jvm, pool, class_index, env)
        })?;
        stack::call(m, 0x423533, &[], Some(owner), |m| {
            lookup::static_field(m, owner, field)
        })?;
        stack::add_sp(m, 4);
    } else {
        let receiver = m.r32(frame + 0x30);
        let variables = m.r32(receiver.wrapping_add(0x10));
        s(m, Reg::Ecx, receiver);
        if receiver == 0 || variables == 0 {
            return Err("NativeVmRun: instance variable owner unavailable".into());
        }
        stack::call(m, 0x423546, &[selector], Some(receiver), |m| {
            stack::function(m, &[selector], Some(receiver), |m| {
                s(m, Reg::Eax, selector);
                s(m, Reg::Ecx, variables);
                stack::call(m, 0x4087ed, &[selector], Some(variables), |m| {
                    lookup::variable_at(m, variables, selector)
                })
            })
        })?;
    }
    stack::store(m, 0x3c, r(m, Reg::Eax));
    s(m, Reg::Eax, frame);
    s(m, Reg::Edi, operands);
    let count = m.r32(operands + 8);
    s(m, Reg::Ecx, count);
    s(m, Reg::Esi, operands + 8);
    s(m, Reg::Ecx, count + 1);
    stack::call(m, 0x423572, &[count + 1], Some(operands), |m| {
        grow(m, operands, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(operands + 4));
    s(m, Reg::Edx, m.r32(r(m, Reg::Esi)));
    s(m, Reg::Ecx, stack::load(m, 0x3c));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ecx));
    write(m, r(m, Reg::Esi), r(m, Reg::Edx) + 1);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPush {
            thread,
            frame,
            index: count as usize,
            cell: stack::load(m, 0x3c),
        },
    )?;
    Ok(())
}

pub(crate) fn pop_operand(m: &mut Space, vector: u32) -> Result<u32, String> {
    if vector == 0 || m.r32(vector + 8) == 0 || m.r32(vector + 8) > m.r32(vector) {
        return Err("NativeVmRun: missing operand owner".into());
    }
    stack::function(m, &[], Some(vector), |m| {
        let count = m.r32(vector + 8);
        s(m, Reg::Eax, count);
        s(m, Reg::Eax, count - 1);
        write(m, vector + 8, count - 1);
        let data = m.r32(vector + 4);
        s(m, Reg::Ecx, data);
        s(m, Reg::Ecx, data + (count - 1) * 4);
        let cell = m.r32(r(m, Reg::Ecx));
        s(m, Reg::Eax, cell);
        write(m, r(m, Reg::Ecx), 0);
        Ok(cell)
    })
}
pub(crate) fn copy_value(m: &mut Space, cell: u32, source: u32) -> u32 {
    stack::function(m, &[source], Some(cell), |m| {
        s(m, Reg::Eax, cell);
        s(m, Reg::Ecx, source);
        write(m, cell, 0x5e734c);
        s(m, Reg::Edx, m.r32(source + 4));
        write(m, cell + 4, r(m, Reg::Edx));
        s(m, Reg::Ecx, m.r32(source + 8));
        write(m, cell + 8, r(m, Reg::Ecx));
        write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
        cell
    })
}
pub(crate) fn as_boolean(
    m: &mut Space,
    cell: u32,
    fpu: &mut native_vm_fpu::State,
) -> Result<(), String> {
    let descriptor = m.r32(cell.wrapping_add(4));
    let tag = m.r8(descriptor);
    if cell == 0 || descriptor == 0 || !matches!(tag, b'I' | b'L' | b'[' | b'F') {
        return Err("NativeVmRun: boolean conversion context unavailable".into());
    }
    stack::function(m, &[cell], None, |m| {
        s(m, Reg::Ecx, cell);
        s(m, Reg::Eax, descriptor);
        s(m, Reg::Eax, (descriptor & !255) | u32::from(tag));
        if matches!(tag, b'L' | b'[') {
            let integer = m.r32(0x62e00c);
            if integer == 0 {
                return Err("NativeVmRun: original integer descriptor unavailable".into());
            }
            s(m, Reg::Edx, integer);
            s(m, Reg::Eax, m.r32(integer));
            write(m, cell + 4, r(m, Reg::Eax));
            s(m, Reg::Eax, m.r32(cell + 8));
            if r(m, Reg::Eax) != 0 {
                write(m, cell + 8, 1);
            }
        } else if tag == b'F' {
            fpu.fld_f32(m.r32(cell + 8))?;
            s(m, Reg::Edx, m.r32(0x62e00c));
            fpu.fcomp_f32(m.r32(0x5e73cc))?;
            s(m, Reg::Eax, m.r32(r(m, Reg::Edx)));
            write(m, cell + 4, r(m, Reg::Eax));
            s(
                m,
                Reg::Eax,
                (r(m, Reg::Eax) & 0xffff0000) | u32::from(fpu.status()),
            );
            let value = u32::from(fpu.status() & 0x4000 == 0);
            s(m, Reg::Eax, value);
            write(m, cell + 8, value);
        }
        Ok(())
    })
}
fn boolean_branch(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
    invert: bool,
) -> Result<(), String> {
    let thread = run.thread;
    let frame = m.r32(thread + 0x20);
    let vector = m.r32(frame + 0x28);
    if vector == 0 || m.r32(vector + 8) == 0 {
        return Err("NativeVmRun: boolean branch stack unavailable".into());
    }
    let source = m.r32(m.r32(vector + 4) + (m.r32(vector + 8) - 1) * 4);
    let vtable = m.r32(source);
    if source == 0 || !matches!(vtable, 0x5e7344 | 0x5e734c) {
        return Err("NativeVmRun: unknown operand virtual type".into());
    }
    let (pop_pc, type_pc, alloc_pc, copy_pc, grow_pc, bool_pc) = if invert {
        (0x4211eb, 0x4211f3, 0x421203, 0x42120f, 0x42122a, 0x421243)
    } else {
        (0x421177, 0x42117f, 0x42118f, 0x42119b, 0x4211b6, 0x4211cf)
    };
    s(m, Reg::Eax, frame);
    s(m, Reg::Ecx, vector);
    let mut cell = stack::call(m, pop_pc, &[], Some(vector), |m| pop_operand(m, vector))?;
    s(m, Reg::Esi, cell);
    s(m, Reg::Edx, vtable);
    s(m, Reg::Ecx, cell);
    let variable = stack::call(m, type_pc, &[], Some(cell), |m| {
        stack::function(m, &[], Some(cell), |m| {
            let kind = if vtable == 0x5e7344 { 2 } else { 1 };
            s(m, Reg::Eax, kind);
            kind == 2
        })
    });
    if variable {
        let pool = m.r32(0x62f130);
        s(m, Reg::Ecx, pool);
        let allocated = stack::call(m, alloc_pc, &[], Some(pool), |m| {
            crate::native_vm_values::allocate(m, pool, env)
        })?;
        if allocated == 0 {
            return Err("NativeVmRun: branch value allocation failure".into());
        }
        s(m, Reg::Ecx, allocated);
        cell = stack::call(m, copy_pc, &[cell], Some(allocated), |m| {
            copy_value(m, allocated, cell)
        });
        s(m, Reg::Esi, cell);
    }
    s(m, Reg::Eax, frame);
    s(m, Reg::Ebp, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Ecx, count);
    s(m, Reg::Edi, vector + 8);
    s(m, Reg::Ecx, count + 1);
    stack::call(m, grow_pc, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(vector + 4));
    s(m, Reg::Edx, m.r32(r(m, Reg::Edi)));
    s(m, Reg::Ecx, stack::load(m, 0x10));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Esi));
    write(m, r(m, Reg::Edi), r(m, Reg::Edx) + 1);
    s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
    s(m, Reg::Ecx, m.r32(r(m, Reg::Edx) + 4));
    let converter = r(m, Reg::Ecx);
    let result = stack::call(m, bool_pc, &[cell], Some(converter), |m| {
        stack::function(m, &[cell], Some(converter), |m| {
            s(m, Reg::Ecx, m.r32(converter));
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, cell);
            stack::call(m, 0x41bd9d, &[cell], None, |m| as_boolean(m, cell, fpu))?;
            s(m, Reg::Eax, m.r32(cell + 8));
            stack::add_sp(m, 4);
            Ok::<u32, String>(r(m, Reg::Eax))
        })
    })?;
    if (result != 0) != invert {
        write(m, thread + 0x24, m.r32(thread + 0x24).wrapping_add(8));
    }
    Ok(())
}

pub(crate) fn pop_deref(m: &mut Space, thread: u32) -> Result<u32, String> {
    pop_deref_observe(m, thread, |_, _| Ok(()))
}
enum DerefObservation {
    Pop { frame: u32, index: usize, cell: u32 },
    Allocated { cell: u32, pool: u32 },
    Constructed { cell: u32 },
}
pub(crate) fn pop_deref_with(
    m: &mut Space,
    thread: u32,
    env: &mut impl crate::native_vm_context::Environment,
) -> Result<u32, String> {
    pop_deref_observe(m, thread, |m, observation| match observation {
        DerefObservation::Pop { frame, index, cell } => env.observe_owner(
            m,
            crate::native_vm_context::Observation::OperandPop {
                thread,
                frame,
                index,
                cell,
            },
        ),
        DerefObservation::Allocated { cell, pool } => env.observe_value(
            m,
            crate::native_vm_values::Observation::Allocated {
                address: cell,
                pool,
                freelist: true,
            },
        ),
        DerefObservation::Constructed { cell } => env.observe_value(
            m,
            crate::native_vm_values::Observation::Constructed { address: cell },
        ),
    })
}
fn pop_deref_observe(
    m: &mut Space,
    thread: u32,
    mut observe: impl FnMut(&Space, DerefObservation) -> Result<(), String>,
) -> Result<u32, String> {
    let frame = m.r32(thread + 0x20);
    let vector = m.r32(frame.wrapping_add(0x28));
    let count = m.r32(vector.wrapping_add(8));
    if frame == 0 || vector == 0 || count == 0 || count > m.r32(vector) {
        return Err("NativeVmRun: dereference operand unavailable".into());
    }
    let source = m.r32(m.r32(vector + 4) + (count - 1) * 4);
    let vtable = m.r32(source);
    if source == 0 || !matches!(vtable, 0x5e7344 | 0x5e734c) {
        return Err("NativeVmRun: dereference virtual type unavailable".into());
    }
    let pool = m.r32(0x62f130);
    if vtable == 0x5e7344 && (pool == 0 || m.r32(pool + 16) == 0) {
        return Err(
            "NativeVmRun: inline dereference pool growth requires its source continuation".into(),
        );
    }
    stack::function(m, &[], Some(thread), |m| {
        s(m, Reg::Eax, frame);
        s(m, Reg::Eax, vector);
        s(m, Reg::Ecx, count);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Edx, m.r32(vector + 4));
        s(m, Reg::Ecx, count - 1);
        write(m, vector + 8, count - 1);
        s(m, Reg::Eax, r(m, Reg::Edx) + (count - 1) * 4);
        s(m, Reg::Ecx, source);
        write(m, r(m, Reg::Eax), 0);
        observe(
            m,
            DerefObservation::Pop {
                frame,
                index: (count - 1) as usize,
                cell: source,
            },
        )?;
        s(m, Reg::Ebx, source);
        s(m, Reg::Eax, vtable);
        s(m, Reg::Ecx, source);
        stack::call(m, 0x41f83b, &[], Some(source), |m| {
            stack::function(m, &[], Some(source), |m| {
                s(m, Reg::Eax, if vtable == 0x5e7344 { 2 } else { 1 })
            })
        });
        let result = if vtable == 0x5e7344 {
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, pool);
            let cell = m.r32(pool + 16);
            s(m, Reg::Ecx, cell);
            s(m, Reg::Edx, m.r32(cell));
            write(m, pool + 16, r(m, Reg::Edx));
            observe(m, DerefObservation::Allocated { cell, pool })?;
            s(m, Reg::Esi, stack::load(m, 0));
            stack::add_sp(m, 4);
            let result = stack::call(m, 0x41f8d1, &[source], Some(cell), |m| {
                copy_value(m, cell, source)
            });
            observe(m, DerefObservation::Constructed { cell })?;
            result
        } else {
            s(m, Reg::Eax, source);
            source
        };
        stack::add_sp(m, 4);
        Ok(result)
    })
}
fn jump(m: &mut Space, thread: u32) {
    let node = m.r32(thread + 0x24);
    s(m, Reg::Ecx, node);
    stack::call(m, 0x42364d, &[], Some(node), |m| {
        stack::function(m, &[], Some(node), |m| s(m, Reg::Eax, m.r32(node + 4)))
    });
    s(m, Reg::Ecx, m.r32(r(m, Reg::Ebp)));
    s(m, Reg::Eax, r(m, Reg::Eax).wrapping_shl(3));
    s(m, Reg::Ecx, r(m, Reg::Ecx).wrapping_add(r(m, Reg::Eax)));
    write(m, r(m, Reg::Ebp), r(m, Reg::Ecx));
}
fn conditional_jump(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<bool, String> {
    let thread = run.thread;
    s(m, Reg::Esi, 13);
    s(m, Reg::Ecx, thread);
    let cell = stack::call(m, 0x423240, &[], Some(thread), |m| pop_deref(m, thread))?;
    s(m, Reg::Ecx, m.r32(r(m, Reg::Edi)));
    s(m, Reg::Esi, cell);
    stack::call(m, 0x42324a, &[cell], Some(r(m, Reg::Ecx)), |m| {
        as_boolean(m, cell, fpu)
    })?;
    s(m, Reg::Edi, m.r32(cell + 8));
    s(m, Reg::Edx, m.r32(cell));
    s(m, Reg::Ecx, cell);
    stack::call(m, 0x42325a, &[1], Some(cell), |m| {
        crate::native_vm_values::release(m, cell, env)
    })?;
    if r(m, Reg::Edi) == 0 {
        jump(m, thread);
        Ok(false)
    } else {
        Ok(true)
    }
}
fn integer_literal(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let pool = m.r32(0x62f130);
    let descriptor = m.r32(0x62e00c);
    if pool == 0 || descriptor == 0 {
        return Err("NativeVmRun: integer literal construction context unavailable".into());
    }
    s(m, Reg::Ecx, pool);
    let cell = stack::call(m, 0x423682, &[], Some(pool), |m| {
        crate::native_vm_values::allocate(m, pool, env)
    })?;
    if cell == 0 {
        return Err("NativeVmRun: literal cell allocation failed".into());
    }
    s(m, Reg::Ecx, descriptor);
    write(m, cell, 0x5e734c);
    s(m, Reg::Ecx, m.r32(descriptor));
    write(m, cell + 4, r(m, Reg::Ecx));
    write(m, cell + 8, 0);
    write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
    s(m, Reg::Ebp, cell);
    let node = m.r32(run.thread + 0x24);
    s(m, Reg::Edx, node);
    s(m, Reg::Eax, m.r32(node + 4));
    write(m, cell + 8, r(m, Reg::Eax));
    env.observe_value(
        m,
        crate::native_vm_values::Observation::Constructed { address: cell },
    )?;
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let vector = m.r32(frame + 0x28);
    let count = m.r32(vector + 8);
    s(m, Reg::Edi, vector);
    s(m, Reg::Edx, count);
    s(m, Reg::Esi, vector + 8);
    s(m, Reg::Edx, count + 1);
    stack::call(m, 0x4236c8, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(r(m, Reg::Esi)));
    s(m, Reg::Ecx, m.r32(vector + 4));
    write(m, r(m, Reg::Ecx) + r(m, Reg::Eax) * 4, r(m, Reg::Ebp));
    write(m, r(m, Reg::Esi), r(m, Reg::Eax) + 1);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPush {
            thread: run.thread,
            frame,
            index: count as usize,
            cell,
        },
    )?;
    Ok(())
}

fn eval_direct_method(
    m: &mut Space,
    thread: u32,
    holder: u32,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<u32, String> {
    use crate::native_vm_lookup as lookup;
    let node = m.r32(holder);
    if !matches!(m.r32(node) as u16, 0x101a | 0x1011) {
        return Err("NativeVmRun: name grammar continuation unavailable".into());
    }
    stack::function(m, &[holder], Some(thread), |m| {
        stack::alloc(m, 12);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, holder);
        s(m, Reg::Eax, node);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, 0);
        s(m, Reg::Ebx, thread);
        stack::store(m, 0x10, 0);
        s(m, Reg::Esi, 1);
        let frame = m.r32(thread + 0x20);
        let declaring = m.r32(frame + 0x34);
        let pool = m.r32(declaring + 0x14);
        let jvm = m.r32(thread + 0x10);
        let count = if m.r32(node) as u16 == 0x101a {
            stack::store(m, 0x14, 1);
            1
        } else {
            let count = m.r32(node + 4);
            s(m, Reg::Ecx, count);
            s(m, Reg::Eax, node + 8);
            stack::store(m, 0x14, count);
            write(m, holder, node + 8);
            count
        };
        if count == 0 || count > 1024 {
            return Err("NativeVmRun: invalid inline name component count".into());
        }
        s(m, Reg::Eax, count);
        s(m, Reg::Edx, count);
        s(m, Reg::Eax, count - 1);
        stack::store(m, 0x14, count - 1);
        let mut first = true;
        let mut target_class;
        loop {
            let mut node = m.r32(holder);
            if first {
                s(m, Reg::Eax, node);
                if m.r32(node) as u16 == 0x1019 {
                    let class_index = m.r32(node + 4);
                    s(m, Reg::Ecx, frame);
                    s(m, Reg::Ecx, declaring);
                    s(m, Reg::Eax, class_index);
                    s(m, Reg::Edx, pool);
                    let class = stack::call(m, 0x420952, &[pool, class_index], Some(jvm), |m| {
                        lookup::class_from_pool_with(m, jvm, pool, class_index, env)
                    })?;
                    s(m, Reg::Edi, class);
                    target_class = class;
                    let remaining = stack::load(m, 0x14);
                    s(m, Reg::Eax, remaining);
                    if remaining == 0 {
                        return Err("NativeVmRun: class qualifier lacks a member".into());
                    }
                    s(m, Reg::Ecx, node);
                    s(m, Reg::Eax, remaining - 1);
                    s(m, Reg::Ecx, node + 8);
                    write(m, holder, node + 8);
                    stack::store(m, 0x14, remaining - 1);
                    node += 8;
                } else {
                    let receiver = m.r32(frame + 0x30);
                    s(m, Reg::Ecx, frame);
                    s(m, Reg::Eax, receiver);
                    target_class = if receiver == 0 {
                        declaring
                    } else {
                        let class = m.r32(receiver + 0xc);
                        s(m, Reg::Edi, class);
                        stack::store(m, 0x10, receiver);
                        class
                    };
                    if receiver == 0 {
                        s(m, Reg::Edi, declaring);
                    }
                }
            } else {
                let receiver = stack::load(m, 0x10);
                s(m, Reg::Edi, receiver);
                if receiver == 0 {
                    return Err("NativeVmRun: null qualified receiver".into());
                }
                let class = m.r32(receiver + 0xc);
                let descriptor = m.r32(class + 8);
                let text = m.r32(descriptor);
                let tag = m.r8(text);
                s(m, Reg::Edx, class);
                s(m, Reg::Eax, descriptor);
                s(m, Reg::Ecx, text);
                s(m, Reg::Eax, (descriptor & !255) | u32::from(tag));
                if !matches!(tag, b'L' | b'[') {
                    return Err("NativeVmRun: qualified member of primitive receiver".into());
                }
                target_class = class;
                s(m, Reg::Edi, class);
            }
            s(m, Reg::Eax, node);
            let opcode = m.r32(node) as u16;
            s(
                m,
                Reg::Ecx,
                u32::from(opcode).wrapping_sub(0x1001).wrapping_sub(0x19),
            );
            if opcode == 0x101a {
                break;
            }
            if opcode != 0x101b {
                return Err(
                    "NativeVmRun: qualified component source continuation unavailable".into(),
                );
            }
            s(m, Reg::Ecx, r(m, Reg::Ecx).wrapping_sub(1));
            let field_index = m.r32(node + 4);
            s(m, Reg::Ebp, field_index);
            s(m, Reg::Eax, frame);
            s(m, Reg::Ecx, declaring);
            s(m, Reg::Esi, pool);
            let class_index = stack::call(m, 0x420a44, &[field_index], Some(pool), |m| {
                lookup::constant(m, pool, field_index, 5, 8)
            })?;
            stack::store(m, 0x18, class_index);
            let referenced = stack::call(m, 0x420a52, &[pool, class_index], Some(jvm), |m| {
                lookup::class_from_pool_with(m, jvm, pool, class_index, env)
            })?;
            if target_class != referenced {
                return Err(
                    "NativeVmRun: virtual qualified-field lookup continuation unavailable".into(),
                );
            }
            let mut selector = stack::call(m, 0x420b00, &[field_index], Some(pool), |m| {
                lookup::constant(m, pool, field_index, 5, 16)
            })?;
            if selector == u32::MAX {
                s(m, Reg::Eax, jvm);
                stack::call(m, 0x420b11, &[jvm, field_index], Some(pool), |m| {
                    lookup::resolve_method_reference(m, pool, jvm, field_index, env)
                })?;
                selector = stack::call(m, 0x420b19, &[field_index], Some(pool), |m| {
                    lookup::constant(m, pool, field_index, 5, 16)
                })?;
            }
            if selector & 0x40000000 == 0 {
                return Err(
                    "NativeVmRun: instance qualified-field lookup continuation unavailable".into(),
                );
            }
            let class_index = stack::load(m, 0x18);
            s(m, Reg::Ecx, class_index);
            let field = selector & 0xbfffffff;
            s(m, Reg::Eax, field);
            stack::push(m, field);
            let owner = stack::call(m, 0x420b34, &[pool, class_index], Some(jvm), |m| {
                lookup::class_from_pool_with(m, jvm, pool, class_index, env)
            })?;
            let cell = stack::call(m, 0x420b3b, &[], Some(owner), |m| {
                stack::function(m, &[field], Some(owner), |m| {
                    stack::push_reg(m, Reg::Esi);
                    s(m, Reg::Esi, owner);
                    stack::call(m, 0x404808, &[], Some(owner), |m| {
                        crate::native_vm_class_init::initialize_with_scripts(m, owner, fpu, env)
                    })?;
                    s(m, Reg::Eax, field);
                    let variables = m.r32(owner + 0x1d0);
                    s(m, Reg::Ecx, variables);
                    stack::call(m, 0x404818, &[field], Some(variables), |m| {
                        lookup::variable_at(m, variables, field)
                    })
                })
            })?;
            stack::add_sp(m, 4);
            s(m, Reg::Edx, m.r32(cell + 8));
            s(m, Reg::Ebp, holder);
            stack::store(m, 0x10, r(m, Reg::Edx));
            s(m, Reg::Eax, m.r32(holder));
            s(m, Reg::Esi, 0);
            s(m, Reg::Eax, r(m, Reg::Eax) + 8);
            write(m, holder, r(m, Reg::Eax));
            let remaining = stack::load(m, 0x14);
            s(m, Reg::Eax, remaining);
            s(m, Reg::Ecx, remaining);
            s(m, Reg::Eax, remaining.wrapping_sub(1));
            stack::store(m, 0x14, r(m, Reg::Eax));
            if remaining == 0 {
                return Err("NativeVmRun: qualified name ended without a method".into());
            }
            first = false;
        }
        s(m, Reg::Edx, frame);
        let operands = m.r32(frame + 0x28);
        s(m, Reg::Ecx, operands);
        let mut argument_count = stack::call(m, 0x420c0d, &[], Some(operands), |m| {
            pop_operand(m, operands)
        })?;
        s(m, Reg::Esi, argument_count);
        let vtable = m.r32(argument_count);
        s(m, Reg::Eax, vtable);
        s(m, Reg::Ecx, argument_count);
        let variable = stack::call(m, 0x420c15, &[], Some(argument_count), |m| {
            stack::function(m, &[], Some(argument_count), |m| {
                let kind = if vtable == 0x5e7344 {
                    2
                } else if vtable == 0x5e734c {
                    1
                } else {
                    0
                };
                s(m, Reg::Eax, kind);
                kind == 2
            })
        });
        if variable {
            let pool = m.r32(0x62f130);
            s(m, Reg::Ecx, pool);
            let cell = stack::call(m, 0x420c25, &[], Some(pool), |m| {
                crate::native_vm_values::allocate(m, pool, env)
            })?;
            argument_count = stack::call(m, 0x420c31, &[argument_count], Some(cell), |m| {
                copy_value(m, cell, argument_count)
            });
            s(m, Reg::Esi, argument_count);
        }
        s(m, Reg::Edx, m.r32(argument_count));
        s(m, Reg::Ecx, argument_count);
        stack::call(m, 0x420c44, &[1], Some(argument_count), |m| {
            crate::native_vm_values::release(m, argument_count, env)
        })?;
        let node = m.r32(holder);
        let field_index = m.r32(node + 4);
        s(m, Reg::Eax, node);
        s(m, Reg::Ecx, frame);
        s(m, Reg::Esi, field_index);
        let declaring = m.r32(frame + 0x34);
        s(m, Reg::Ebp, declaring);
        let pool = m.r32(declaring + 0x14);
        let class_index = stack::call(m, 0x420c59, &[field_index], Some(pool), |m| {
            lookup::constant(m, pool, field_index, 5, 8)
        })?;
        s(m, Reg::Edx, pool);
        let jvm = m.r32(thread + 0x10);
        s(m, Reg::Ecx, jvm);
        let referenced = stack::call(m, 0x420c66, &[pool, class_index], Some(jvm), |m| {
            lookup::class_from_pool_with(m, jvm, pool, class_index, env)
        })?;
        s(m, Reg::Eax, frame);
        s(m, Reg::Eax, declaring);
        s(m, Reg::Ecx, pool);
        if target_class != referenced {
            return Err(
                "NativeVmRun: virtual name/type resolution continuation unavailable".into(),
            );
        }
        let mut selector = stack::call(m, 0x420cbe, &[field_index], Some(pool), |m| {
            lookup::constant(m, pool, field_index, 5, 16)
        })?;
        if selector == u32::MAX {
            s(m, Reg::Ecx, jvm);
            s(m, Reg::Edx, frame);
            s(m, Reg::Eax, declaring);
            stack::call(m, 0x420cd6, &[jvm, field_index], Some(pool), |m| {
                lookup::resolve_method_reference(m, pool, jvm, field_index, env)
            })?;
            s(m, Reg::Ecx, frame);
            s(m, Reg::Eax, declaring);
            selector = stack::call(m, 0x420ce5, &[field_index], Some(pool), |m| {
                lookup::constant(m, pool, field_index, 5, 16)
            })?;
        }
        let static_vector = (selector >> 27) & 8;
        s(m, Reg::Esi, static_vector);
        s(m, Reg::Eax, selector & 0xbfffffff);
        let record = stack::call(
            m,
            0x420cfb,
            &[selector & 0xbfffffff, static_vector],
            Some(target_class),
            |m| lookup::method_record(m, target_class, selector & 0xbfffffff, static_vector, env),
        )?;
        if static_vector == 0 {
            let receiver = stack::load(m, 0x10);
            s(m, Reg::Edx, receiver);
            write(m, record + 4, receiver);
        }
        Ok(record)
    })
}
fn call_context_constructor(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
    base: bool,
) -> Result<bool, String> {
    let thread = run.thread;
    s(m, Reg::Eax, m.r32(thread + 0x20));
    let frame = r(m, Reg::Eax);
    let receiver = m.r32(frame + 0x30);
    s(m, Reg::Ecx, receiver);
    s(m, Reg::Eax, m.r32(frame + 0x34));
    let class = if base {
        m.r32(r(m, Reg::Eax) + 0x1c8)
    } else {
        r(m, Reg::Eax)
    };
    if base {
        s(m, Reg::Edx, class);
    }
    let name = if base { 0x607aec } else { 0x607ae4 };
    let result = stack::call(
        m,
        if base { 0x4212ba } else { 0x4212d7 },
        &[thread, class],
        Some(receiver),
        |m| {
            stack::function(m, &[thread, class], Some(receiver), |m| {
                s(m, Reg::Eax, class);
                if base && class == 0 {
                    s(m, Reg::Eax, 1);
                    return Ok(1);
                }
                stack::call(
                    m,
                    if base { 0x408aa8 } else { 0x408a84 },
                    &[class, receiver, name],
                    Some(thread),
                    |m| {
                        crate::native_vm_callback::call_by_name(
                            m, thread, class, receiver, name, fpu, env,
                        )
                    },
                )
            })
        },
    )?;
    Ok(result != 0)
}
fn call_constructor(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<bool, String> {
    let thread = run.thread;
    s(m, Reg::Eax, m.r32(thread + 0x20));
    let vector = m.r32(r(m, Reg::Eax) + 0x28);
    s(m, Reg::Ecx, vector);
    s(m, Reg::Eax, m.r32(vector + 8).wrapping_sub(1));
    s(m, Reg::Edx, m.r32(vector));
    if (r(m, Reg::Edx) as i32) <= (r(m, Reg::Eax) as i32) {
        return Err("NativeVmRun: constructor count slot outside operand owner".into());
    }
    s(m, Reg::Edx, m.r32(vector + 4));
    s(m, Reg::Eax, m.r32(r(m, Reg::Edx) + r(m, Reg::Eax) * 4));
    s(m, Reg::Edx, m.r32(r(m, Reg::Eax) + 8));
    s(m, Reg::Eax, m.r32(vector + 8).wrapping_sub(r(m, Reg::Edx)));
    s(m, Reg::Edx, m.r32(vector));
    s(m, Reg::Eax, r(m, Reg::Eax).wrapping_sub(2));
    if (r(m, Reg::Edx) as i32) <= (r(m, Reg::Eax) as i32) {
        return Err("NativeVmRun: constructor receiver outside operand owner".into());
    }
    s(m, Reg::Ecx, m.r32(vector + 4));
    s(m, Reg::Eax, m.r32(r(m, Reg::Ecx) + r(m, Reg::Eax) * 4));
    let receiver = m.r32(r(m, Reg::Eax) + 8);
    s(m, Reg::Ecx, receiver);
    let result = stack::call(m, 0x421297, &[thread, 0x60c660], Some(receiver), |m| {
        stack::function(m, &[thread, 0x60c660], Some(receiver), |m| {
            s(m, Reg::Eax, 0x60c660);
            let class = m.r32(receiver + 0xc);
            s(m, Reg::Ecx, class);
            stack::call(
                m,
                0x408a43,
                &[class, receiver, 0x60c660],
                Some(thread),
                |m| {
                    crate::native_vm_callback::call_by_name(
                        m, thread, class, receiver, 0x60c660, fpu, env,
                    )
                },
            )
        })
    })?;
    Ok(result != 0)
}

fn call_method(
    m: &mut Space,
    run: &mut Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<bool, String> {
    let thread = run.thread;
    let node = m.r32(thread + 0x24);
    s(m, Reg::Ecx, node + 8);
    s(m, Reg::Eax, thread + 0x24);
    write(m, thread + 0x24, node + 8);
    let record = stack::call(m, 0x4212f4, &[thread + 0x24], Some(thread), |m| {
        eval_direct_method(m, thread, thread + 0x24, fpu, env)
    })?;
    s(m, Reg::Esi, record);
    if record == 0 {
        return Err("NativeVmRun: name evaluation returned no call record".into());
    }
    let method = m.r32(record + 8);
    s(m, Reg::Ecx, method);
    if m.r32(method + 0xc) == 0xfffffffe {
        return Err("NativeVmRun: name-based dynamic call continuation unavailable".into());
    }
    if m.r8(method + 4) & 8 == 0 {
        s(m, Reg::Eax, m.r32(record + 4));
        if r(m, Reg::Eax) == 0 {
            return Err("NativeVmRun: instance call without receiver".into());
        }
    }
    let receiver = m.r32(record + 4);
    let class = m.r32(record);
    s(m, Reg::Edx, receiver);
    s(m, Reg::Eax, class);
    if m.r8(method + 4) & 0x40 != 0 {
        env.observe_context(
            m,
            crate::native_vm_context::Observation::MethodSelect {
                thread,
                class,
                method,
                receiver: if m.r8(method + 4) & 8 == 0 {
                    receiver
                } else {
                    0
                },
            },
        )?;
        stack::begin_call(m, 0x421375, &[record, class, receiver], Some(thread));
        let pending = crate::native_vm_dispatch::begin_materialize(m, env)?;
        let pending = crate::native_vm_dispatch::select_native(m, pending)?;
        let pending = crate::native_vm_dispatch::prepare_arguments(m, pending, env)?;
        let signature = pending.signature;
        let entry = crate::native_vm_abi::begin_prepared(m)?;
        env.observe_context(
            m,
            crate::native_vm_context::Observation::NativeEnter {
                thread,
                class,
                method,
                signature,
                receiver: if m.r8(method + 4) & 8 == 0 {
                    receiver
                } else {
                    0
                },
                implementation: entry.target(),
                info: entry.info(),
            },
        )?;
        run.pc = entry.target();
        run.native = Some(NativeCall {
            dispatch: pending,
            entry,
            record,
        });
        return Ok(false);
    }
    let result = stack::call(m, 0x421375, &[record, class, receiver], Some(thread), |m| {
        let pending = crate::native_vm_dispatch::begin_materialize(m, env)?;
        let pending = crate::native_vm_dispatch::prepare_script_frame(m, pending, env)?;
        crate::native_vm_dispatch::finish_script_arguments(m, pending, env, fpu)
    })?;
    s(m, Reg::Edi, result);
    s(m, Reg::Eax, m.r32(record + 0xc));
    if r(m, Reg::Eax) != 0 {
        stack::external(m, 0x421388, &[record], None, |m| env.free(m, record))?;
    }
    Ok(result != 0)
}

// Actual source CALL sites for the compiler's repeated pop/type/copy sequence.
// These are instruction identities; values and pointers always come from live
// operand owners and pools, never a captured register/stack image.
fn pop_value_site(
    m: &mut Space,
    thread: u32,
    vector: u32,
    destination: Reg,
    type_register: Reg,
    sites: [u32; 4],
    env: &mut impl Environment,
) -> Result<u32, String> {
    s(m, Reg::Ecx, vector);
    let mut cell = stack::call(m, sites[0], &[], Some(vector), |m| pop_operand(m, vector))?;
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPop {
            thread,
            frame: m.r32(thread + 0x20),
            index: m.r32(vector + 8) as usize,
            cell,
        },
    )?;
    s(m, destination, cell);
    let vtable = m.r32(cell);
    if !matches!(vtable, 0x5e7344 | 0x5e734c) {
        return Err("NativeVmRun: arithmetic operand virtual type unavailable".into());
    }
    s(m, type_register, vtable);
    s(m, Reg::Ecx, cell);
    let variable = stack::call(m, sites[1], &[], Some(cell), |m| {
        stack::function(m, &[], Some(cell), |m| {
            s(m, Reg::Eax, if vtable == 0x5e7344 { 2 } else { 1 });
            vtable == 0x5e7344
        })
    });
    if variable {
        let pool = m.r32(0x62f130);
        s(m, Reg::Ecx, pool);
        let allocated = stack::call(m, sites[2], &[], Some(pool), |m| {
            crate::native_vm_values::allocate(m, pool, env)
        })?;
        if allocated == 0 {
            return Err("NativeVmRun: arithmetic value allocation failed".into());
        }
        cell = stack::call(m, sites[3], &[cell], Some(allocated), |m| {
            copy_value(m, allocated, cell)
        });
        env.observe_value(
            m,
            crate::native_vm_values::Observation::Constructed { address: cell },
        )?;
        s(m, destination, cell);
    }
    Ok(cell)
}
fn binary_operator(
    m: &mut Space,
    run: &Run,
    kind: u32,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let (right_sites, left_sites, operation_pc, release_pc, grow_pc) = match kind {
        9 => (
            [0x422b78, 0x422b80, 0x422b90, 0x422b9c],
            [0x422bad, 0x422bb5, 0x422bc5, 0x422bd1],
            0x422be1,
            0x422bee,
            0x422c03,
        ),
        7 => (
            [0x4228e4, 0x4228ec, 0x4228fc, 0x422908],
            [0x422919, 0x422921, 0x422931, 0x42293d],
            0x42294d,
            0x42295a,
            0x42296f,
        ),
        0xf => (
            [0x422253, 0x42225b, 0x42226b, 0x422277],
            [0x422288, 0x422290, 0x4222a0, 0x4222ac],
            0x4222bc,
            0x4222c9,
            0x4222de,
        ),
        2 => (
            [0x4225ab, 0x4225b3, 0x4225c3, 0x4225cf],
            [0x4225e0, 0x4225e8, 0x4225f8, 0x422604],
            0x422614,
            0x422621,
            0x422636,
        ),
        1 => (
            [0x4226f5, 0x4226fd, 0x42270d, 0x422719],
            [0x42272a, 0x422732, 0x422742, 0x42274e],
            0x42275e,
            0x42276b,
            0x422780,
        ),
        _ => return Err("NativeVmRun: arithmetic source continuation unavailable".into()),
    };
    let frame = m.r32(run.thread + 0x20);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Esi, kind);
    s(m, Reg::Edx, frame);
    let right = pop_value_site(m, run.thread, vector, Reg::Edi, Reg::Eax, right_sites, env)?;
    s(m, Reg::Ecx, frame);
    let left = pop_value_site(m, run.thread, vector, Reg::Esi, Reg::Edx, left_sites, env)?;
    let converter = m.r32(run.thread + 0x14);
    s(m, Reg::Ecx, converter);
    stack::call(m, operation_pc, &[left, right], Some(converter), |m| {
        crate::native_vm_alu::binary(m, converter, left, right, kind, fpu)
    })?;
    s(m, Reg::Eax, m.r32(right));
    s(m, Reg::Ecx, right);
    stack::call(m, release_pc, &[1], Some(right), |m| {
        crate::native_vm_values::release(m, right, env)
    })?;
    s(m, Reg::Ecx, frame);
    s(m, Reg::Ebp, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Edx, count);
    s(m, Reg::Edi, vector + 8);
    s(m, Reg::Edx, count + 1);
    stack::call(m, grow_pc, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(r(m, Reg::Edi)));
    s(m, Reg::Ecx, m.r32(vector + 4));
    write(m, r(m, Reg::Ecx) + r(m, Reg::Eax) * 4, r(m, Reg::Esi));
    write(m, r(m, Reg::Edi), r(m, Reg::Eax) + 1);
    Ok(())
}
fn assign_expression(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Edx, 0);
    s(m, Reg::Eax, frame);
    let source = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Esi,
        Reg::Edx,
        [0x4210f7, 0x4210ff, 0x42110f, 0x42111b],
        env,
    )?;
    s(m, Reg::Eax, frame);
    let destination = stack::call(m, 0x42112c, &[], Some(vector), |m| pop_operand(m, vector))?;
    s(m, Reg::Edi, destination);
    s(m, Reg::Edx, m.r32(destination));
    if m.r32(destination) != 0x5e7344 {
        return Err("NativeVmRun: assignment requires lvalue".into());
    }
    stack::call(m, 0x421134, &[], Some(destination), |m| {
        stack::function(m, &[], Some(destination), |m| s(m, Reg::Eax, 2))
    });
    let converter = m.r32(run.thread + 0x14);
    stack::call(
        m,
        0x421159,
        &[destination, source, 0x23],
        Some(converter),
        |m| crate::native_vm_alu::assignment(m, converter, destination, source, fpu),
    )?;
    if source != 0 {
        s(m, Reg::Edx, m.r32(source));
        stack::call(m, 0x4238ed, &[1], Some(source), |m| {
            crate::native_vm_values::release(m, source, env)
        })?;
    }
    Ok(())
}
fn assignment_statement(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Esi, 0x23);
    s(m, Reg::Edx, frame);
    let destination = stack::call(m, 0x423064, &[], Some(vector), |m| pop_operand(m, vector))?;
    s(m, Reg::Edx, m.r32(destination));
    stack::store(m, 0x34, destination);
    if m.r32(destination) != 0x5e7344 {
        return Err("NativeVmRun: statement assignment requires lvalue".into());
    }
    stack::call(m, 0x42306e, &[], Some(destination), |m| {
        stack::function(m, &[], Some(destination), |m| s(m, Reg::Eax, 2))
    });
    s(m, Reg::Ecx, frame);
    let source = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Esi,
        Reg::Edx,
        [0x42308e, 0x423096, 0x4230a6, 0x4230b2],
        env,
    )?;
    s(m, Reg::Eax, m.r32(run.thread + 0x24));
    s(m, Reg::Eax, m.r32(r(m, Reg::Eax) + 4));
    s(m, Reg::Ecx, stack::load(m, 0x34));
    let converter = m.r32(run.thread + 0x14);
    stack::call(
        m,
        0x4230cd,
        &[destination, source, 0x23],
        Some(converter),
        |m| crate::native_vm_alu::assignment(m, converter, destination, source, fpu),
    )?;
    if source != 0 {
        s(m, Reg::Edx, m.r32(source));
        stack::call(m, 0x4238ed, &[1], Some(source), |m| {
            crate::native_vm_values::release(m, source, env)
        })?;
    }
    Ok(())
}
fn increment_statement(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let kind = m.r32(m.r32(run.thread + 0x24) + 4);
    if kind == 0x23 {
        return assignment_statement(m, run, fpu, env);
    }
    if kind == 0x1b {
        s(m, Reg::Esi, kind);
        s(m, Reg::Edx, 5);
        let frame = m.r32(run.thread + 0x20);
        s(m, Reg::Eax, frame);
        let vector = m.r32(frame + 0x28);
        s(m, Reg::Ecx, vector);
        let cell = pop_value_site(
            m,
            run.thread,
            vector,
            Reg::Esi,
            Reg::Edx,
            [0x422f00, 0x422f08, 0x422f18, 0x422f24],
            env,
        )?;
        if cell != 0 {
            s(m, Reg::Eax, m.r32(cell));
            s(m, Reg::Ecx, cell);
            stack::call(m, 0x422f3b, &[1], Some(cell), |m| {
                crate::native_vm_values::release(m, cell, env)
            })?;
        }
        return Ok(());
    }
    if kind == 0x22 {
        s(m, Reg::Esi, kind);
        s(m, Reg::Edx, 11);
        return Ok(());
    }
    if kind != 0x17 {
        return Err(format!(
            "NativeVmRun: statement operation {kind:#x} continuation unavailable"
        ));
    }
    s(m, Reg::Esi, kind);
    s(m, Reg::Edx, 0);
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let vector = m.r32(frame + 0x28);
    let cell = stack::call(m, 0x4231d8, &[], Some(vector), |m| pop_operand(m, vector))?;
    s(m, Reg::Esi, cell);
    let vtable = m.r32(cell);
    s(m, Reg::Edx, vtable);
    if vtable != 0x5e7344 {
        return Err("NativeVmRun: increment requires lvalue".into());
    }
    stack::call(m, 0x4231e0, &[], Some(cell), |m| {
        stack::function(m, &[], Some(cell), |m| s(m, Reg::Eax, 2))
    });
    let converter = m.r32(run.thread + 0x14);
    stack::call(m, 0x4231fe, &[cell], Some(converter), |m| {
        stack::function(m, &[cell], Some(converter), |m| {
            stack::alloc(m, 0x20);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            s(m, Reg::Edi, cell);
            s(m, Reg::Esi, converter);
            let jvm = m.r32(converter);
            stack::call(m, 0x41b2e3, &[cell], Some(jvm), |m| {
                stack::function(m, &[cell], Some(jvm), |_| {})
            });
            s(m, Reg::Eax, m.r32(cell + 4));
            let tag = m.r8(r(m, Reg::Eax));
            s(m, Reg::Eax, (r(m, Reg::Eax) & !255) | u32::from(tag));
            if tag != b'I' {
                return Err("NativeVmRun: increment type continuation unavailable".into());
            }
            write(m, cell + 8, m.r32(cell + 8).wrapping_add(1));
            Ok::<(), String>(())
        })
    })
}
fn declare_local(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let node = m.r32(run.thread + 0x24);
    let ordinal = m.r32(node + 4);
    s(m, Reg::Edx, 0);
    s(m, Reg::Eax, ordinal);
    stack::store(m, 0x14, ordinal);
    s(m, Reg::Eax, node + 8);
    s(m, Reg::Edi, 0);
    write(m, run.thread + 0x24, node + 8);
    if m.r32(node + 8) as u16 == 0x402e {
        return Err("NativeVmRun: declaration modifier continuation unavailable".into());
    }
    let pool = m.r32(0x62f134);
    let cell = stack::call(m, 0x42178b, &[], Some(pool), |m| {
        crate::native_vm_values::allocate_variable(m, pool, env)
    })?;
    s(m, Reg::Esi, cell);
    if cell == 0 {
        return Err("NativeVmRun: variable allocation failed".into());
    }
    s(m, Reg::Edx, node + 8);
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let descriptor_index = m.r32(node + 12);
    s(m, Reg::Eax, descriptor_index);
    let class = m.r32(frame + 0x34);
    s(m, Reg::Ecx, class);
    let constants = m.r32(class + 0x14);
    let descriptor = stack::call(m, 0x4217a6, &[descriptor_index], Some(constants), |m| {
        crate::native_vm_lookup::constant(m, constants, descriptor_index, 0, 8)
    })?;
    let thread_holder = stack::load(m, 0x10);
    s(m, Reg::Edx, thread_holder);
    let jvm = m.r32(thread_holder);
    s(m, Reg::Ecx, jvm);
    stack::call(m, 0x4217b5, &[jvm, descriptor], Some(cell), |m| {
        stack::function(m, &[jvm, descriptor], Some(cell), |m| {
            s(m, Reg::Eax, descriptor);
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, cell);
            s(m, Reg::Ecx, jvm);
            write(m, cell, 0x5e734c);
            stack::call(m, 0x408d7b, &[jvm, descriptor], Some(cell + 4), |m| {
                stack::function(m, &[jvm, descriptor], Some(cell + 4), |m| {
                    s(m, Reg::Eax, descriptor);
                    stack::push_reg(m, Reg::Esi);
                    s(m, Reg::Esi, cell + 4);
                    s(m, Reg::Ecx, jvm);
                    let holder = m.r32(jvm + 0x68);
                    let name = stack::call(m, 0x423e24, &[descriptor], Some(holder), |m| {
                        crate::native_vm_signature::canonical(m, holder, descriptor)
                    })?;
                    write(m, cell + 4, name);
                    Ok::<(), String>(())
                })
            })?;
            write(m, cell + 8, 0);
            write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
            s(m, Reg::Eax, cell);
            Ok::<(), String>(())
        })
    })?;
    write(m, cell, 0x5e7344);
    write(m, cell + 0xc, r(m, Reg::Edi));
    env.observe_context(
        m,
        crate::native_vm_context::Observation::VariableConstructed { address: cell },
    )?;
    s(m, Reg::Eax, frame);
    let locals = m.r32(frame + 0x18);
    s(m, Reg::Ebp, locals);
    s(m, Reg::Eax, m.r32(locals + 8) + 1);
    s(m, Reg::Ecx, m.r32(locals));
    s(m, Reg::Edi, locals + 8);
    if r(m, Reg::Eax) > r(m, Reg::Ecx) {
        return Err("NativeVmRun: local declaration growth continuation unavailable".into());
    }
    s(m, Reg::Edx, m.r32(locals + 4));
    s(m, Reg::Ecx, m.r32(locals + 8));
    write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, cell);
    write(m, locals + 8, r(m, Reg::Ecx) + 1);
    env.observe_context(
        m,
        crate::native_vm_context::Observation::Local {
            thread: run.thread,
            frame,
            append: true,
            index: (m.r32(locals + 8) - 1) as usize,
            variable: cell,
        },
    )?;
    let next = m.r32(run.thread + 0x24) + 8;
    s(m, Reg::Eax, next);
    if m.r32(next) as u16 != 0x2c {
        return Ok(());
    }
    s(m, Reg::Edx, stack::load(m, 0x14));
    write(m, run.thread + 0x24, next);
    s(m, Reg::Ecx, frame);
    s(m, Reg::Eax, locals);
    if ordinal >= m.r32(locals) {
        return Err("NativeVmRun: initializer local index unavailable".into());
    }
    s(m, Reg::Eax, m.r32(locals + 4));
    s(m, Reg::Ebp, m.r32(r(m, Reg::Eax) + ordinal * 4));
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Edi, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Ecx, count + 1);
    s(m, Reg::Esi, vector + 8);
    stack::call(m, 0x422d83, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(vector + 4));
    s(m, Reg::Edx, m.r32(vector + 8));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ebp));
    write(m, vector + 8, r(m, Reg::Edx) + 1);
    Ok(())
}
fn local_reference(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Edx, frame);
    let locals = m.r32(frame + 0x18);
    s(m, Reg::Eax, locals);
    let ordinal = m.r32(m.r32(run.thread + 0x24) + 4);
    s(m, Reg::Ecx, ordinal);
    if ordinal >= m.r32(locals) {
        return Err("NativeVmRun: local index continuation unavailable".into());
    }
    s(m, Reg::Eax, m.r32(locals + 4));
    s(m, Reg::Ebp, m.r32(r(m, Reg::Eax) + ordinal * 4));
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Edi, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Ecx, count + 1);
    s(m, Reg::Esi, vector + 8);
    stack::call(m, 0x422d83, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(vector + 4));
    s(m, Reg::Edx, m.r32(vector + 8));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ebp));
    write(m, vector + 8, r(m, Reg::Edx) + 1);
    Ok(())
}
fn logical_not(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Esi, 0x15);
    s(m, Reg::Edx, frame);
    let cell = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Esi,
        Reg::Eax,
        [0x4224e7, 0x4224ef, 0x4224ff, 0x42250b],
        env,
    )?;
    let converter = m.r32(run.thread + 0x14);
    stack::call(m, 0x42251a, &[cell], Some(converter), |m| {
        stack::function(m, &[cell], Some(converter), |m| {
            stack::alloc(m, 0x20);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            s(m, Reg::Edi, cell);
            s(m, Reg::Esi, converter);
            s(m, Reg::Ecx, m.r32(converter));
            stack::call(m, 0x41bdc3, &[cell], None, |m| as_boolean(m, cell, fpu))?;
            s(m, Reg::Eax, m.r32(cell + 4));
            if m.r8(r(m, Reg::Eax)) != b'I' {
                return Err("NativeVmRun: boolean conversion did not produce integer".into());
            }
            s(m, Reg::Eax, m.r32(cell + 8));
            s(m, Reg::Ecx, u32::from(r(m, Reg::Eax) == 0));
            write(m, cell + 8, r(m, Reg::Ecx));
            Ok::<(), String>(())
        })
    })?;
    s(m, Reg::Ecx, frame);
    s(m, Reg::Ebp, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Edx, count + 1);
    s(m, Reg::Edi, vector + 8);
    stack::call(m, 0x42252f, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(vector + 8));
    s(m, Reg::Ecx, m.r32(vector + 4));
    write(m, r(m, Reg::Ecx) + r(m, Reg::Eax) * 4, r(m, Reg::Esi));
    write(m, vector + 8, r(m, Reg::Eax) + 1);
    Ok(())
}
fn return_void(m: &mut Space, run: &mut Run, env: &mut impl Environment) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    if frame != 0 {
        s(m, Reg::Eax, m.r32(frame));
        stack::call(m, 0x4216a4, &[1], Some(frame), |m| {
            crate::native_vm_values::release_frame(m, frame, env)
        })?;
    }
    stack::call(m, 0x4216ab, &[], Some(run.thread), |m| {
        stack::function(m, &[], Some(run.thread), |m| {
            let frame = m.r32(run.thread + 0x20);
            s(m, Reg::Eax, frame);
            s(m, Reg::Edx, m.r32(frame + 0x2c));
            write(m, run.thread + 0x24, r(m, Reg::Edx));
        })
    });
    s(m, Reg::Eax, m.r32(run.thread + 0x24));
    if r(m, Reg::Eax) == 0 {
        write(m, run.thread + 0x2c, m.r32(run.thread + 0x2c) | 0x40);
        run.pc = 0x423936;
    }
    Ok(())
}
fn return_value(m: &mut Space, run: &mut Run, env: &mut impl Environment) -> Result<(), String> {
    s(m, Reg::Edx, 0);
    let cell = stack::call(m, 0x421669, &[], Some(run.thread), |m| {
        pop_deref(m, run.thread)
    })?;
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    s(m, Reg::Esi, cell);
    if frame != 0 {
        s(m, Reg::Edx, m.r32(frame));
        stack::call(m, 0x421678, &[1], Some(frame), |m| {
            crate::native_vm_values::release_frame(m, frame, env)
        })?;
    }
    stack::call(m, 0x42167f, &[], Some(run.thread), |m| {
        stack::function(m, &[], Some(run.thread), |m| {
            let frame = m.r32(run.thread + 0x20);
            s(m, Reg::Eax, frame);
            s(m, Reg::Edx, m.r32(frame + 0x2c));
            write(m, run.thread + 0x24, r(m, Reg::Edx));
        })
    });
    stack::call(m, 0x421687, &[cell], Some(run.thread), |m| {
        crate::native_vm_runtime::push_operand(m, run.thread, cell, env)
    })?;
    s(m, Reg::Eax, m.r32(run.thread + 0x24));
    if r(m, Reg::Eax) == 0 {
        write(m, run.thread + 0x2c, m.r32(run.thread + 0x2c) | 0x40);
        run.pc = 0x423936;
    }
    Ok(())
}
fn expression(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    s(m, Reg::Edx, 0);
    s(m, Reg::Ecx, run.thread);
    let cell = stack::call(m, 0x421655, &[], Some(run.thread), |m| {
        pop_deref_with(m, run.thread, env)
    })?;
    stack::call(m, 0x42165d, &[cell], Some(run.thread), |m| {
        crate::native_vm_runtime::push_operand(m, run.thread, cell, env)
    })?;
    let frame = m.r32(run.thread + 0x20);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPush {
            thread: run.thread,
            frame,
            index: (m.r32(m.r32(frame + 0x28) + 8) - 1) as usize,
            cell,
        },
    )
}
fn cleanup_local(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let vector = m.r32(frame + 0x18);
    s(m, Reg::Eax, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Ecx, count);
    if count == 0 {
        return Err("NativeVmRun: empty local cleanup continuation unavailable".into());
    }
    s(m, Reg::Ecx, count - 1);
    write(m, vector + 8, count - 1);
    s(m, Reg::Edx, m.r32(vector + 4));
    let slot = r(m, Reg::Edx) + (count - 1) * 4;
    s(m, Reg::Eax, slot);
    let cell = m.r32(slot);
    s(m, Reg::Ecx, cell);
    write(m, slot, 0);
    env.observe_context(
        m,
        crate::native_vm_context::Observation::Local {
            thread: run.thread,
            frame,
            append: false,
            index: (count - 1) as usize,
            variable: cell,
        },
    )?;
    if cell != 0 {
        s(m, Reg::Eax, m.r32(cell));
        stack::call(m, 0x42185c, &[1], Some(cell), |m| {
            crate::native_vm_values::release_variable(m, cell, env)
        })?;
    }
    Ok(())
}
fn array_access(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Edx, 0);
    s(m, Reg::Ecx, frame);
    let vector = m.r32(frame + 0x28);
    let array = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Edi,
        Reg::Edx,
        [0x4213a3, 0x4213ab, 0x4213bb, 0x4213c7],
        env,
    )?;
    s(m, Reg::Eax, frame);
    let index = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Esi,
        Reg::Edx,
        [0x4213d8, 0x4213e0, 0x4213f0, 0x4213fc],
        env,
    )?;
    s(m, Reg::Eax, m.r32(index + 4));
    if m.r8(r(m, Reg::Eax)) != b'I' {
        return Err("NativeVmRun: array index must be integer".into());
    }
    let ordinal = m.r32(index + 8);
    s(m, Reg::Ebp, ordinal);
    s(m, Reg::Ecx, m.r32(array + 4));
    if (ordinal as i32) < 0 || m.r8(r(m, Reg::Ecx)) != b'[' {
        return Err("NativeVmRun: invalid array access".into());
    }
    s(m, Reg::Eax, m.r32(array + 8));
    if r(m, Reg::Eax) == 0 {
        return Err("NativeVmRun: null array".into());
    }
    let item = stack::call(m, 0x42143c, &[ordinal], Some(array), |m| {
        stack::function(m, &[ordinal], Some(array), |m| {
            s(m, Reg::Eax, ordinal);
            let object = m.r32(array + 8);
            s(m, Reg::Ecx, object);
            stack::call(m, 0x423ead, &[ordinal], Some(object), |m| {
                crate::native_vm_lookup::array_item(m, object, ordinal)
            })
        })
    })?;
    stack::store(m, 0x50, item);
    s(m, Reg::Edx, m.r32(array));
    stack::call(m, 0x42147f, &[1], Some(array), |m| {
        crate::native_vm_values::release(m, array, env)
    })?;
    s(m, Reg::Eax, m.r32(index));
    stack::call(m, 0x42148c, &[1], Some(index), |m| {
        crate::native_vm_values::release(m, index, env)
    })?;
    s(m, Reg::Ecx, frame);
    s(m, Reg::Esi, vector);
    s(m, Reg::Eax, m.r32(vector + 8) + 1);
    s(m, Reg::Ecx, m.r32(vector));
    s(m, Reg::Edi, vector + 8);
    if r(m, Reg::Eax) > r(m, Reg::Ecx) {
        return Err("NativeVmRun: array push growth continuation unavailable".into());
    }
    s(m, Reg::Eax, m.r32(vector + 4));
    s(m, Reg::Edx, m.r32(vector + 8));
    s(m, Reg::Ecx, stack::load(m, 0x50));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ecx));
    write(m, vector + 8, r(m, Reg::Edx) + 1);
    Ok(())
}
fn default_return_value(m: &mut Space, run: &Run) -> Result<(), String> {
    s(m, Reg::Edx, 0);
    let pool = m.r32(0x62f130);
    s(m, Reg::Esi, pool);
    let cell = m.r32(pool + 0x10);
    s(m, Reg::Eax, cell);
    if cell == 0 {
        return Err(
            "NativeVmRun: default return inline pool growth continuation unavailable".into(),
        );
    }
    s(m, Reg::Ecx, m.r32(cell));
    write(m, pool + 0x10, r(m, Reg::Ecx));
    s(m, Reg::Ecx, m.r32(0x62e00c));
    write(m, cell, 0x5e734c);
    s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
    write(m, cell + 4, r(m, Reg::Edx));
    write(m, cell + 8, 0);
    write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let vector = m.r32(frame + 0x28);
    stack::call(m, 0x421636, &[cell], Some(vector), |m| {
        stack::function(m, &[cell], Some(vector), |m| {
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, vector);
            let count = m.r32(vector + 8);
            s(m, Reg::Eax, count + 1);
            s(m, Reg::Ecx, m.r32(vector));
            if count + 1 > m.r32(vector) {
                return Err(
                    "NativeVmRun: default return vector growth continuation unavailable".into(),
                );
            }
            s(m, Reg::Eax, count);
            s(m, Reg::Ecx, m.r32(vector + 4));
            s(m, Reg::Edx, cell);
            write(m, r(m, Reg::Ecx) + count * 4, cell);
            write(m, vector + 8, count + 1);
            Ok(())
        })
    })
}
fn finish_run(
    m: &mut Space,
    run: &mut Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    if run.returned || run.pc != 0x423936 || stack::sp(m) != run.base {
        return Err("NativeVmRun: run exit context is not current".into());
    }
    let flags = m.r32(run.thread + 0x2c) & !4;
    s(m, Reg::Eax, flags);
    write(m, run.thread + 0x2c, flags);
    stack::call(m, 0x423946, &[], Some(run.thread), |m| {
        stack::function(m, &[], Some(run.thread), |m| {
            let frame = m.r32(run.thread + 0x20);
            s(m, Reg::Eax, frame);
            s(m, Reg::Ecx, m.r32(run.thread + 0x24));
            write(m, frame + 0x2c, r(m, Reg::Ecx));
        })
    });
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::SavedPc {
            thread: run.thread,
            frame: m.r32(run.thread + 0x20),
            pc: m.r32(run.thread + 0x24),
        },
    )?;
    s(m, Reg::Edx, stack::load(m, 0x10));
    let jvm = m.r32(r(m, Reg::Edx));
    s(m, Reg::Eax, jvm);
    s(m, Reg::Ecx, stack::load(m, 0x4c));
    write(m, jvm + 0x78, r(m, Reg::Ecx));
    stack::call(m, 0x423958, &[], None, |m| clock(m, fpu, env))?;
    fpu.reverse_subtract_f32(stack::load(m, 0x44))?;
    let return_pc = stack::load(m, 0x45c);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::Run {
            thread: run.thread,
            enter: false,
            budget_bits: run.budget,
            pc: m.r32(run.thread + 0x24),
            flags: m.r32(run.thread + 0x2c),
        },
    )?;
    stack::leave(m, run.caller);
    run.pc = return_pc;
    run.returned = true;
    Ok(())
}
fn instance_test(m: &mut Space, run: &Run, env: &mut impl Environment) -> Result<(), String> {
    let frame = m.r32(run.thread + 0x20);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Esi, 0x20);
    s(m, Reg::Edx, frame);
    let source = pop_value_site(
        m,
        run.thread,
        vector,
        Reg::Esi,
        Reg::Eax,
        [0x422cc2, 0x422cca, 0x422cda, 0x422ce6],
        env,
    )?;
    let pool = m.r32(0x62f130);
    let result = stack::call(m, 0x422cf7, &[], Some(pool), |m| {
        crate::native_vm_values::allocate(m, pool, env)
    })?;
    if result == 0 {
        return Err("NativeVmRun: instance test value allocation failed".into());
    }
    s(m, Reg::Ecx, m.r32(0x62e00c));
    write(m, result, 0x5e734c);
    s(m, Reg::Ecx, m.r32(r(m, Reg::Ecx)));
    write(m, result + 4, r(m, Reg::Ecx));
    write(m, result + 8, 0);
    write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
    s(m, Reg::Ebp, result);
    let node = m.r32(run.thread + 0x24) + 8;
    s(m, Reg::Edx, node);
    write(m, run.thread + 0x24, node);
    s(m, Reg::Ecx, m.r32(source + 8));
    s(m, Reg::Eax, node);
    if r(m, Reg::Ecx) != 0 {
        return Err("NativeVmRun: nonnull instance test continuation unavailable".into());
    }
    write(m, result + 8, 0);
    s(m, Reg::Edx, m.r32(source));
    stack::call(m, 0x422d6e, &[1], Some(source), |m| {
        crate::native_vm_values::release(m, source, env)
    })?;
    s(m, Reg::Eax, frame);
    s(m, Reg::Edi, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Ecx, count + 1);
    s(m, Reg::Esi, vector + 8);
    stack::call(m, 0x422d83, &[count + 1], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    s(m, Reg::Eax, m.r32(vector + 4));
    s(m, Reg::Edx, m.r32(vector + 8));
    write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, r(m, Reg::Ebp));
    write(m, vector + 8, r(m, Reg::Edx) + 1);
    Ok(())
}
fn inline_resource(
    m: &mut Space,
    run: &Run,
    node: u32,
    env: &mut impl Environment,
) -> Result<(), String> {
    let h = m.r32(0x62f130);
    s(m, Reg::Ecx, h);
    let cell = stack::call(m, 0x4219eb, &[], Some(h), |m| {
        crate::native_vm_values::allocate(m, h, env)
    })?;
    s(m, Reg::Esi, 0);
    if cell == 0 {
        return Err("NativeVmRun: resource literal allocation failed".into());
    }
    let descriptor = m.r32(0x62e00c);
    s(m, Reg::Ecx, descriptor);
    write(m, cell, 0x5e734c);
    s(m, Reg::Edx, m.r32(descriptor));
    write(m, cell + 4, r(m, Reg::Edx));
    write(m, cell + 8, 0);
    write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
    s(m, Reg::Esi, cell);
    s(m, Reg::Eax, node);
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let index = m.r32(node + 4);
    s(m, Reg::Eax, index);
    s(m, Reg::Ecx, m.r32(frame + 0x34));
    let pool = m.r32(r(m, Reg::Ecx) + 0x14);
    s(m, Reg::Ecx, pool);
    stack::push(m, index);
    stack::store(m, 0x28, cell);
    let id = stack::call(m, 0x421a26, &[], Some(pool), |m| {
        crate::native_vm_lookup::resource_literal(m, pool, index, env)
    })?;
    stack::add_sp(m, 4);
    write(m, cell + 8, id);
    Ok(())
}
fn inline_literal(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let node = m.r32(run.thread + 0x24).wrapping_add(8);
    s(m, Reg::Eax, node);
    write(m, run.thread + 0x24, node);
    let tag = signed_half(m, node);
    if !matches!(tag, 0xc | 0x100b | 0x100e | 0x200a) {
        return Err(format!(
            "NativeVmRun: inline literal tag {tag:#x} continuation unavailable"
        ));
    }
    if tag == 0x100e {
        inline_resource(m, run, node, env)?;
    } else if tag == 0xc {
        let pool = m.r32(0x62f130);
        s(m, Reg::Ecx, pool);
        let cell = stack::call(m, 0x4219a6, &[], Some(pool), |m| {
            crate::native_vm_values::allocate(m, pool, env)
        })?;
        s(m, Reg::Edx, 0);
        if cell == 0 {
            return Err("NativeVmRun: null literal allocation failed".into());
        }
        s(m, Reg::Ecx, m.r32(0x62e000));
        write(m, cell, 0x5e734c);
        s(m, Reg::Ecx, m.r32(r(m, Reg::Ecx)));
        write(m, cell + 4, r(m, Reg::Ecx));
        write(m, cell + 8, 0);
        write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
        stack::store(m, 0x24, cell);
        write(m, cell + 8, r(m, Reg::Edx));
    } else {
        s(m, Reg::Ecx, m.r32(0x62f130));
        let pool = r(m, Reg::Ecx);
        let cell = stack::call(
            m,
            if tag == 0x200a { 0x421a7b } else { 0x421953 },
            &[],
            Some(pool),
            |m| crate::native_vm_values::allocate(m, pool, env),
        )?;
        if cell == 0 {
            return Err("NativeVmRun: inline literal allocation failed".into());
        }
        let descriptor = m.r32(if tag == 0x200a { 0x62deec } else { 0x62e00c });
        s(m, Reg::Ecx, descriptor);
        write(m, cell, 0x5e734c);
        s(m, Reg::Edx, m.r32(descriptor));
        write(m, cell + 4, r(m, Reg::Edx));
        write(m, cell + 8, 0);
        write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
        s(m, Reg::Ecx, m.r32(run.thread + 0x24));
        if tag == 0x200a {
            fpu.fld_f32(m.r32(r(m, Reg::Ecx) + 4))?;
            stack::store(m, 0x24, cell);
            write(m, cell + 8, fpu.fstp_f32()?);
        } else {
            s(m, Reg::Edx, m.r32(r(m, Reg::Ecx) + 4));
            stack::store(m, 0x24, cell);
            write(m, cell + 8, r(m, Reg::Edx));
        }
    }
    env.observe_value(
        m,
        crate::native_vm_values::Observation::Constructed {
            address: stack::load(m, 0x24),
        },
    )?;
    s(m, Reg::Edx, m.r32(run.thread + 0x20));
    let vector = m.r32(r(m, Reg::Edx) + 0x28);
    s(m, Reg::Edi, vector);
    s(m, Reg::Eax, m.r32(vector + 8) + 1);
    s(m, Reg::Esi, vector + 8);
    let count = r(m, Reg::Eax);
    stack::call(m, 0x421ac3, &[count], Some(vector), |m| {
        grow(m, vector, count, env)
    })?;
    s(m, Reg::Ecx, m.r32(r(m, Reg::Esi)));
    s(m, Reg::Edx, m.r32(vector + 4));
    s(m, Reg::Eax, stack::load(m, 0x24));
    write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, r(m, Reg::Eax));
    write(m, r(m, Reg::Esi), r(m, Reg::Ecx) + 1);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPush {
            thread: run.thread,
            frame: m.r32(run.thread + 0x20),
            index: (count - 1) as usize,
            cell: r(m, Reg::Eax),
        },
    )?;
    Ok(())
}
fn new_instance(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let node = m.r32(run.thread + 0x24) + 8;
    s(m, Reg::Eax, node);
    write(m, run.thread + 0x24, node);
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    s(m, Reg::Eax, m.r32(node + 4));
    let context = m.r32(frame + 0x34);
    s(m, Reg::Ecx, context);
    let pool = m.r32(context + 0x14);
    s(m, Reg::Ecx, pool);
    let index = r(m, Reg::Eax);
    stack::push(m, index);
    let name = stack::call(m, 0x422f63, &[], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, index, 0, 8)
    })?;
    stack::add_sp(m, 4);
    let jvm = m.r32(run.thread + 0x10);
    s(m, Reg::Ecx, jvm);
    stack::push(m, name);
    let class = stack::call(m, 0x422f6b, &[], Some(jvm), |m| {
        env.class_lookup(m, jvm, name)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Esi, class);
    s(m, Reg::Ecx, class);
    let object = stack::call(m, 0x422f74, &[], Some(class), |m| {
        crate::native_vm_array_object::create_instance(m, class, fpu, env)
    })?;
    let h = m.r32(0x62f130);
    s(m, Reg::Ecx, h);
    s(m, Reg::Edi, object);
    let cell = stack::call(m, 0x422f81, &[], Some(h), |m| {
        crate::native_vm_values::allocate(m, h, env)
    })?;
    if cell == 0 {
        return Err("NativeVmRun: new-instance result allocation failed".into());
    }
    stack::push(m, class);
    s(m, Reg::Ecx, cell);
    stack::call(m, 0x422f8d, &[], Some(cell), |m| {
        crate::native_vm_values::construct_reference(m, cell, class)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Ebp, cell);
    write(m, cell + 8, object);
    s(m, Reg::Edx, m.r32(run.thread + 0x20));
    let vector = m.r32(r(m, Reg::Edx) + 0x28);
    s(m, Reg::Edi, vector);
    let count = m.r32(vector + 8);
    s(m, Reg::Eax, count);
    s(m, Reg::Esi, vector + 8);
    s(m, Reg::Eax, count + 1);
    stack::push(m, count + 1);
    s(m, Reg::Ecx, vector);
    stack::call(m, 0x422fab, &[], Some(vector), |m| {
        grow(m, vector, count + 1, env)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Ecx, m.r32(vector + 8));
    s(m, Reg::Edx, m.r32(vector + 4));
    write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, cell);
    write(m, vector + 8, m.r32(vector + 8) + 1);
    Ok(())
}
fn new_array(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    s(m, Reg::Edx, 0x1c); //4218b3: array-allocation classifier
    let original_node = m.r32(run.thread + 0x24);
    let node = original_node + 8;
    s(m, Reg::Ecx, node);
    stack::push(m, 4);
    write(m, run.thread + 0x24, node);
    let descriptor_cell = stack::external(m, 0x421ae3, &[], None, |m| env.allocate(m, 4))?;
    s(m, Reg::Eax, descriptor_cell);
    s(m, Reg::Esi, descriptor_cell);
    stack::add_sp(m, 4);
    if descriptor_cell == 0 {
        return Err("NativeVmRun: array descriptor-cell allocation failure".into());
    }
    s(m, Reg::Eax, node);
    let frame = m.r32(run.thread + 0x20);
    s(m, Reg::Ecx, frame);
    let ti = m.r32(node + 4);
    s(m, Reg::Eax, ti);
    let class = m.r32(frame + 0x34);
    s(m, Reg::Ecx, class);
    let pool = m.r32(class + 0x14);
    s(m, Reg::Ecx, pool);
    stack::push(m, ti);
    let text = stack::call(m, 0x421b01, &[], Some(pool), |m| {
        crate::native_vm_lookup::constant(m, pool, ti, 0, 8)
    })?;
    stack::add_sp(m, 4);
    let jvm = m.r32(run.thread + 0x10);
    s(m, Reg::Ecx, jvm);
    stack::push(m, text);
    stack::push(m, jvm);
    s(m, Reg::Ecx, descriptor_cell);
    stack::call(m, 0x421b0c, &[], Some(descriptor_cell), |m| {
        crate::native_vm_signature::descriptor(m, descriptor_cell, jvm, text, env)
    })?;
    stack::add_sp(m, 8);
    let descriptor = m.r32(descriptor_cell);
    s(m, Reg::Eax, descriptor);
    let first = m.r8(descriptor);
    s(m, Reg::Edx, (r(m, Reg::Edx) & !255) | u32::from(first));
    s(m, Reg::Ebp, 0);
    s(m, Reg::Eax, descriptor + 1);
    stack::store(m, 0x18, descriptor_cell);
    stack::store(m, 0x1c, 0);
    stack::store(m, 0x14, 0);
    if first != b'[' || m.r8(descriptor + 1) == b'[' {
        return Err(
            "NativeVmRun: multidimensional/non-array allocation diagnostic continuation required"
                .into(),
        );
    }
    s(
        m,
        Reg::Ecx,
        (r(m, Reg::Ecx) & !255) | u32::from(m.r8(descriptor + 1)),
    );
    s(m, Reg::Ebp, 1);
    s(m, Reg::Eax, descriptor + 2);
    stack::store(m, 0x14, 1);
    s(m, Reg::Eax, 0);
    stack::store(m, 0x34, 0);
    s(m, Reg::Edi, run.base + 0x5c);
    s(m, Reg::Ebp, 1);
    s(m, Reg::Ecx, frame);
    let operands = m.r32(frame + 0x28);
    s(m, Reg::Ecx, operands);
    let mut size = stack::call(m, 0x421b7c, &[], Some(operands), |m| {
        pop_operand(m, operands)
    })?;
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPop {
            thread: run.thread,
            frame,
            index: m.r32(operands + 8) as usize,
            cell: size,
        },
    )?;
    s(m, Reg::Esi, size);
    let vt = m.r32(size);
    s(m, Reg::Edx, vt);
    s(m, Reg::Ecx, size);
    let kind = stack::call(m, 0x421b84, &[], Some(size), |m| {
        stack::function(m, &[], Some(size), |m| {
            let k = if vt == 0x5e7344 { 2 } else { 1 };
            s(m, Reg::Eax, k);
            k
        })
    });
    if kind == 2 {
        let h = m.r32(0x62f130);
        s(m, Reg::Ecx, h);
        let cell = stack::call(m, 0x421b94, &[], Some(h), |m| {
            crate::native_vm_values::allocate(m, h, env)
        })?;
        if cell == 0 {
            return Err("NativeVmRun: array-size dereference allocation failure".into());
        }
        stack::push(m, size);
        s(m, Reg::Ecx, cell);
        stack::call(m, 0x421ba0, &[], Some(cell), |m| copy_value(m, cell, size));
        stack::add_sp(m, 4);
        env.observe_value(
            m,
            crate::native_vm_values::Observation::Constructed { address: cell },
        )?;
        s(m, Reg::Esi, cell);
        size = cell;
    } else if vt != 0x5e734c {
        return Err("NativeVmRun: invalid array-size owner".into());
    }
    let length = m.r32(size + 8);
    s(m, Reg::Eax, length);
    write(m, run.base + 0x5c, length);
    s(m, Reg::Edx, m.r32(size));
    stack::push(m, 1);
    s(m, Reg::Ecx, size);
    stack::call(m, 0x421bb8, &[], Some(size), |m| {
        crate::native_vm_values::release(m, size, env)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Ebp, 0);
    s(m, Reg::Edi, run.base + 0x58);
    s(m, Reg::Eax, stack::load(m, 0x34));
    s(m, Reg::Ecx, descriptor_cell);
    s(m, Reg::Ebx, descriptor);
    s(m, Reg::Ecx, 1);
    s(m, Reg::Ebp, run.base + 0x5c);
    s(m, Reg::Eax, 1);
    stack::store(m, 0x14, 1);
    s(m, Reg::Edx, stack::load(m, 0x10));
    s(m, Reg::Ecx, jvm);
    stack::push(m, descriptor);
    let array_class = stack::call(m, 0x421be7, &[], Some(jvm), |m| {
        crate::native_vm_array_class::get(m, env)
    })?;
    stack::add_sp(m, 4);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::ArraySupportBegin {
            thread: run.thread,
            class: array_class,
        },
    )?;
    s(m, Reg::Esi, array_class);
    s(m, Reg::Eax, length);
    stack::push(m, length);
    s(m, Reg::Ecx, array_class);
    let object = stack::call(m, 0x421bf4, &[], Some(array_class), |m| {
        crate::native_vm_array_object::create(m, array_class, length, fpu, env)
    })?;
    stack::add_sp(m, 4);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::ArraySupportEnd {
            thread: run.thread,
            class: array_class,
            object,
        },
    )?;
    let h = m.r32(0x62f130);
    s(m, Reg::Ecx, h);
    s(m, Reg::Edi, object);
    let value = stack::call(m, 0x421c01, &[], Some(h), |m| {
        crate::native_vm_values::allocate(m, h, env)
    })?;
    if value == 0 {
        return Err("NativeVmRun: array value allocation failure".into());
    }
    stack::push(m, array_class);
    s(m, Reg::Ecx, value);
    stack::call(m, 0x421c0d, &[], Some(value), |m| {
        crate::native_vm_values::construct_reference(m, value, array_class)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Ecx, stack::load(m, 0x1c));
    write(m, value + 8, object);
    env.observe_value(
        m,
        crate::native_vm_values::Observation::Constructed { address: value },
    )?;
    stack::store(m, 0x1c, value);
    s(m, Reg::Eax, 1);
    s(m, Reg::Ebx, descriptor + 1);
    s(m, Reg::Eax, 0);
    s(m, Reg::Ebp, run.base + 0x58);
    stack::store(m, 0x14, 0);
    s(m, Reg::Ecx, run.thread);
    s(m, Reg::Edx, frame);
    let vector = m.r32(frame + 0x28);
    s(m, Reg::Edi, vector);
    let used = m.r32(vector + 8);
    s(m, Reg::Eax, used + 1);
    s(m, Reg::Esi, vector + 8);
    s(m, Reg::Ecx, vector);
    stack::call(m, 0x421c4a, &[used + 1], Some(vector), |m| {
        grow(m, vector, used + 1, env)
    })?;
    s(m, Reg::Edx, m.r32(vector + 4));
    s(m, Reg::Ecx, m.r32(vector + 8));
    s(m, Reg::Eax, value);
    write(m, r(m, Reg::Edx) + r(m, Reg::Ecx) * 4, value);
    s(m, Reg::Edi, used);
    s(m, Reg::Ecx, descriptor_cell);
    s(m, Reg::Edi, used + 1);
    stack::push(m, descriptor_cell);
    write(m, vector + 8, used + 1);
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::OperandPush {
            thread: run.thread,
            frame,
            index: used as usize,
            cell: value,
        },
    )?;
    stack::external(m, 0x421c65, &[], None, |m| env.free(m, descriptor_cell))?;
    s(m, Reg::Ebx, run.thread);
    stack::add_sp(m, 4);
    Ok(())
}
fn sub_operation(
    m: &mut Space,
    run: &Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    let kind = m.r32(m.r32(run.thread + 0x24) + 4);
    s(m, Reg::Esi, kind); //4218a5: retained arithmetic-selector register
    match kind {
        1 | 2 | 7 | 9 | 0xf => binary_operator(m, run, kind, fpu, env),
        0x20 => instance_test(m, run, env),
        0x1e => new_instance(m, run, fpu, env),
        0x1f => new_array(m, run, fpu, env),
        0x1b => Ok(()),
        0x15 => logical_not(m, run, fpu, env),
        0x1c => inline_literal(m, run, fpu, env),
        _ => Err(format!(
            "NativeVmRun: original arithmetic kind {kind:#x} continuation unavailable"
        )),
    }
}

fn advance(
    m: &mut Space,
    run: &mut Run,
    advance_pc: bool,
    env: &mut impl Environment,
) -> Result<(), String> {
    let thread = run.thread;
    if advance_pc {
        write(m, thread + 0x24, m.r32(thread + 0x24).wrapping_add(8));
    }
    s(
        m,
        Reg::Eax,
        (r(m, Reg::Eax) & !255) | u32::from(m.r8(run.base + 0x23)),
    );
    if m.r8(run.base + 0x23) == 0 {
        return Err("NativeVmRun: timed loop polling requires its source continuation".into());
    }
    if m.r8(thread + 0x2c) & 0x78 != 0 {
        return Err("NativeVmRun: run exit requires its source continuation".into());
    }
    s(m, Reg::Ecx, thread);
    let ready = stack::call(m, 0x423925, &[], Some(thread), |m| {
        stack::function(m, &[], Some(thread), |m| {
            let flags = m.r32(thread + 0x2c);
            s(m, Reg::Eax, flags);
            let result = u32::from(flags & 0x82 == 0);
            s(m, Reg::Eax, result);
            result
        })
    });
    if ready == 0 {
        return Err("NativeVmRun: non-runnable loop exit requires its source continuation".into());
    }
    s(m, Reg::Edi, stack::load(m, 0x10));
    finish_instruction(m, run, env)?;
    run.pc = 0x42108b;
    prefetch(m, run)?;
    Ok(())
}
fn finish_instruction(m: &Space, run: &mut Run, env: &mut impl Environment) -> Result<(), String> {
    let instruction = run
        .instruction
        .ok_or("NativeVmRun: completed opcode has no source entry")?;
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::Opcode {
            thread: run.thread,
            enter: false,
            pc: instruction.pc,
            opcode: instruction.opcode,
            operand: instruction.operand,
            immediate: instruction.immediate,
            next_pc: Some(m.r32(run.thread + 0x24)),
            flags: m.r32(run.thread + 0x2c),
        },
    )?;
    run.instruction = None;
    Ok(())
}
/// Run an initializer with its actual pending native CALL retained and validated.
/// The environment must execute the source native body; no result table is used.
pub fn drive(
    m: &mut Space,
    run: &mut Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    while !run.has_returned() {
        if run.native.is_some() {
            run.resume_native(m, fpu, env, |m, fpu, env, target, info, kind| {
                env.native(m, fpu, target, info, kind)
            })?;
        } else {
            step(m, run, fpu, env)?;
        }
    }
    Ok(())
}
/// Execute one source opcode with the retained interpreter frame still live.
/// Unimplemented arms fail before consuming a different VM operation.
pub fn step(
    m: &mut Space,
    run: &mut Run,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl Environment,
) -> Result<(), String> {
    if run.pc != 0x4210c5 || stack::sp(m) != run.base {
        return Err("NativeVmRun: opcode continuation not current".into());
    }
    if run.instruction.is_some() {
        return Err("NativeVmRun: previous source opcode has not completed".into());
    }
    let pc = m.r32(run.thread + 0x24);
    let word = m.r32(pc);
    let instruction = Instruction {
        pc,
        opcode: word as u16,
        operand: (word >> 16) as u16,
        immediate: m.r32(pc + 4),
    };
    env.observe_owner(
        m,
        crate::native_vm_context::Observation::Opcode {
            thread: run.thread,
            enter: true,
            pc,
            opcode: instruction.opcode,
            operand: instruction.operand,
            immediate: instruction.immediate,
            next_pc: Some(pc),
            flags: m.r32(run.thread + 0x2c),
        },
    )?;
    run.instruction = Some(instruction);
    let opcode = m.r32(m.r32(run.thread + 0x24)) as u16;
    let mut advance_pc = true;
    match opcode {
        0x10 => {
            //4210dd/4210df classifier arm3 ->423932. The initializer frame
            //stays owned by its caller; this opcode only stops this run.
            s(m, Reg::Edx, 3);
            write(m, run.thread + 0x2c, m.r32(run.thread + 0x2c) | 0x40);
            run.pc = 0x423936;
        }
        0x29 => default_return_value(m, run)?,
        0x2b => return_value(m, run, env)?,
        0x2a => return_void(m, run, env)?,
        0x21 => advance_pc = call_constructor(m, run, fpu, env)?,
        0x22 => advance_pc = call_context_constructor(m, run, fpu, env, false)?,
        0x23 => advance_pc = call_context_constructor(m, run, fpu, env, true)?,
        0x28 => expression(m, run, env)?,
        0x2d => assign_expression(m, run, fpu, env)?,
        0x1008 => increment_statement(m, run, fpu, env)?,
        0x1002 => declare_local(m, run, env)?,
        0x1001 => local_reference(m, run, env)?,
        0x1003 => cleanup_local(m, run, env)?,
        0x20 => array_access(m, run, env)?,
        0x24 => advance_pc = call_method(m, run, fpu, env)?,
        0x1007 => sub_operation(m, run, fpu, env)?,
        0x1014 => {
            jump(m, run.thread);
            advance_pc = false;
        }
        0x1015 => advance_pc = conditional_jump(m, run, fpu, env)?,
        0x1027 => integer_literal(m, run, env)?,
        0x101b => field_ref(m, run, env)?,
        0x401c => named_field(m, run, env)?,
        4 | 5 => boolean_branch(m, run, fpu, env, opcode == 5)?,
        _ => {
            return Err(format!(
                "NativeVmRun: original opcode {opcode:#06x} context continuation unavailable"
            ))
        }
    }
    if run.native.is_some() {
        return Ok(());
    }
    if run.pc == 0x423936 {
        finish_instruction(m, run, env)?;
        return finish_run(m, run, fpu, env);
    }
    advance(m, run, advance_pc, env)
}

#[cfg(test)]
#[path = "native_vm_thread_run_tests.rs"]
mod tests;
