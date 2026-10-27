//! Original425a90/4255e0 callback caller reconstruction. Every entry must
//! arrive through its actual pending source CALL; no captured state is loaded.
use crate::{
    native_stack::{self as stack, Frame, Reg},
    native_vm_fpu,
};
use slr_physics::space::Space;
pub struct Callback {
    pub wrapper: Frame,
    pub setup: Frame,
    pub thread: u32,
    base: u32,
    finished: bool,
}
impl Callback {
    pub fn finish_setup(
        &mut self,
        m: &mut Space,
        run: &crate::native_vm_thread_run::Run,
        fpu: &mut native_vm_fpu::State,
    ) -> Result<(), String> {
        if self.finished
            || !run.has_returned()
            || run.thread != self.thread
            || run.pc != 0x425719
            || stack::sp(m) != self.base
        {
            return Err("NativeVmCallback: returned interpreter context is not current".into());
        }
        fpu.pop()?;
        s(m, Reg::Ebp, 0);
        if r(m, Reg::Ebx) & 1 != 0 || r(m, Reg::Edi) != 0 {
            return Err(
                "NativeVmCallback: protected/result disposal continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, 0);
        stack::leave(m, self.setup);
        self.finished = true;
        Ok(())
    }
}
/// A retained source callback and interpreter continuation. Construction
/// consumes the actual pending wrapper CALL and its inherited FP state. A
/// native pause retains its capability instead of unwinding into a bridge.
#[must_use]
pub struct Context {
    callback: Callback,
    run: crate::native_vm_thread_run::Run,
    setup_return: Option<u32>,
    wrapper_finished: bool,
}
impl Context {
    pub fn begin(
        m: &mut Space,
        fpu: &mut native_vm_fpu::State,
        e: &mut impl crate::native_vm_thread_run::Environment,
    ) -> Result<Self, String> {
        let callback = begin(m, fpu, e)?;
        let run = crate::native_vm_thread_run::begin(m, fpu, e)?;
        Ok(Self {
            callback,
            run,
            setup_return: None,
            wrapper_finished: false,
        })
    }
    pub fn thread(&self) -> u32 {
        self.run.thread
    }
    /// Execute source operations until the actual native CALL or script return.
    /// Errors retain the entered continuation for diagnosis; they do not unwind.
    pub fn advance(
        &mut self,
        m: &mut Space,
        fpu: &mut native_vm_fpu::State,
        e: &mut impl crate::native_vm_thread_run::Environment,
    ) -> Result<bool, String> {
        if self.setup_return.is_some() {
            return Err("NativeVmCallback: setup already returned".into());
        }
        while !self.run.has_returned() && !self.run.has_pending_native() {
            crate::native_vm_thread_run::step(m, &mut self.run, fpu, e)?;
        }
        if self.run.has_pending_native() {
            self.run.native_entry(m)?;
        }
        Ok(self.run.has_returned())
    }
    pub fn native_entry(&self, m: &Space) -> Result<&crate::native_vm_abi::Entry, String> {
        self.run.native_entry(m)
    }
    pub fn resume_native(
        &mut self,
        m: &mut Space,
        fpu: &mut native_vm_fpu::State,
        e: &mut impl crate::native_vm_thread_run::Environment,
    ) -> Result<(), String> {
        self.run
            .resume_native(m, fpu, e, |m, fpu, e, target, info, kind| {
                e.native(m, fpu, target, info, kind)
            })
    }
    pub fn finish_setup(
        &mut self,
        m: &mut Space,
        fpu: &mut native_vm_fpu::State,
    ) -> Result<(), String> {
        self.callback.finish_setup(m, &self.run, fpu)?;
        self.setup_return = Some(stack::sp(m));
        Ok(())
    }
    ///425a90's original ADD ESP,20 / RET, after its setup has returned.
    pub fn finish_wrapper(&mut self, m: &mut Space) -> Result<(), String> {
        if self.wrapper_finished
            || !stack::active(m)
            || stack::has_pending_call(m)
            || self.setup_return != Some(stack::sp(m))
        {
            return Err("NativeVmCallback: returned wrapper continuation is not current".into());
        }
        stack::add_sp(m, 20);
        stack::leave(m, self.callback.wrapper);
        self.wrapper_finished = true;
        Ok(())
    }
}
pub fn begin(
    m: &mut Space,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<Callback, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmCallback: actual pending wrapper CALL required".into());
    }
    let args = std::array::from_fn::<_, 4, _>(|i| stack::argument(m, i));
    let wrapper = stack::enter_cdecl(m, &args);
    let va = stack::sp(m) + 0x14;
    s(m, Reg::Ecx, args[3]);
    s(m, Reg::Edx, args[2]);
    s(m, Reg::Eax, va);
    // The wrapper's interleaved PUSH/MOV sequence ends with these actual
    // argument words and caller-clobbered registers.
    s(m, Reg::Eax, args[1]);
    s(m, Reg::Ecx, args[0]);
    let setup_args = [args[0], args[1], args[2], args[3], va];
    stack::begin_call(m, 0x425aae, &setup_args, None);
    let setup = stack::enter_cdecl(m, &setup_args);
    stack::alloc(m, 0x40);
    stack::push_reg(m, Reg::Ebp);
    let envelope = args[0];
    s(m, Reg::Ebp, envelope);
    s(m, Reg::Eax, m.r32(envelope + 0x50));
    stack::push_reg(m, Reg::Esi);
    if r(m, Reg::Eax) == 0 {
        return Err("NativeVmCallback: absent script return continuation unavailable".into());
    }
    let resource = m.r32(envelope + 0x44);
    s(m, Reg::Esi, resource);
    s(m, Reg::Ebp, envelope + 0x38);
    if resource == 0 {
        return Err("NativeVmCallback: absent secondary return continuation unavailable".into());
    }
    if m.r32(resource + 0x4c) != 1 {
        s(m, Reg::Eax, m.r32(resource));
        stack::call(m, 0x425614, &[0x3f800000], Some(resource), |m| {
            stack::function(m, &[0x3f800000], Some(resource), |m| {
                s(m, Reg::Eax, m.r32(resource + 0x7c));
                if r(m, Reg::Eax) >= m.r32(0x6200a4) {
                    return Err(
                        "NativeVmCallback: resource minimum refresh continuation unavailable"
                            .into(),
                    );
                }
                s(m, Reg::Edx, 0x3f800000);
                m.w32(resource + 0x6c, 0x3f800000);
                Ok::<(), String>(())
            })
        })?;
    }
    stack::call(m, 0x425624, &[0x80000000, 0, 0], Some(resource), |m| {
        prepare_loaded(m, resource, 0x80000000, fpu)
    })?;
    s(m, Reg::Edx, m.r32(resource));
    let payload = stack::call(m, 0x425637, &[0x3f800000], Some(resource), |m| {
        stack::function(m, &[0x3f800000], Some(resource), |m| {
            if m.r32(m.r32(resource) + 0xc) != 0x546ce0 {
                return Err(format!(
                    "NativeVmCallback: resource payload virtual {:#x} unavailable",
                    m.r32(resource)
                ));
            }
            s(m, Reg::Eax, resource + 0x80);
            Ok::<u32, String>(resource + 0x80)
        })
    })?;
    if payload == 0 {
        return Err("NativeVmCallback: absent payload return continuation unavailable".into());
    }
    s(m, Reg::Eax, args[2]);
    stack::push_reg(m, Reg::Ebx);
    s(m, Reg::Ebx, 0);
    stack::push_reg(m, Reg::Edi);
    if args[2] != 0 {
        return Err("NativeVmCallback: protected callback flags continuation unavailable".into());
    }
    let b = stack::sp(m);
    s(m, Reg::Eax, m.r32(0x60c968));
    s(m, Reg::Ecx, m.r32(0x60c96c));
    s(m, Reg::Edx, m.r32(0x60c970));
    m.w32(b + 0x10, r(m, Reg::Eax));
    s(
        m,
        Reg::Eax,
        (r(m, Reg::Eax) & !255) | u32::from(m.r8(0x60c974)),
    );
    m.w32(b + 0x14, r(m, Reg::Ecx));
    m.w8(b + 0x1c, r(m, Reg::Eax) as u8);
    m.w32(b + 0x18, r(m, Reg::Edx));
    s(m, Reg::Eax, 0);
    s(m, Reg::Ecx, 12);
    s(m, Reg::Edi, b + 0x1d);
    for i in 0..12 {
        m.w32(b + 0x1d + i * 4, 0);
    }
    s(m, Reg::Edi, b + 0x4d);
    s(m, Reg::Ecx, 0);
    m.w8(b + 0x4d, 0);
    m.w8(b + 0x4e, 0);
    s(m, Reg::Edi, b + 0x4f);
    stack::push(m, 0x40);
    m.w8(b + 0x4f, 0);
    s(m, Reg::Edi, b + 0x50);
    let text = stack::call(m, 0x425693, &[], Some(envelope + 0x38), |m| {
        stack::function(m, &[], Some(envelope + 0x38), |m| {
            s(m, Reg::Eax, m.r32(envelope + 0x44));
            let p = r(m, Reg::Eax);
            if p == 0 {
                return Err("NativeVmCallback: thread name resource absent".into());
            }
            s(m, Reg::Eax, m.r32(p + 0x60));
            Ok::<u32, String>(r(m, Reg::Eax))
        })
    })?;
    stack::push(m, text);
    s(m, Reg::Ecx, b + 0x10);
    stack::push(m, b + 0x10);
    stack::call(m, 0x42569e, &[], None, |m| append(m, b + 0x10, text, 0x40))?;
    stack::push(m, 0x40);
    s(m, Reg::Edx, b + 0x10);
    stack::push(m, 0x60c978);
    stack::push(m, b + 0x10);
    stack::call(m, 0x4256af, &[], None, |m| {
        append(m, b + 0x10, 0x60c978, 0x40)
    })?;
    s(m, Reg::Edi, args[1]);
    stack::push(m, 0x40);
    s(m, Reg::Eax, b + 0x10);
    stack::push(m, args[1]);
    stack::push(m, b + 0x10);
    stack::call(m, 0x4256c0, &[], None, |m| {
        append(m, b + 0x10, args[1], 0x40)
    })?;
    stack::push(m, 0x38);
    let thread = stack::external(m, 0x4256c7, &[], None, |m| env.allocate(m, 0x38))?;
    s(m, Reg::Eax, thread);
    stack::add_sp(m, 0x28);
    if thread == 0 {
        return Err("NativeVmCallback: thread allocation failed".into());
    }
    s(m, Reg::Edx, m.r32(0x63641c));
    s(m, Reg::Ecx, b + 0x10);
    let jvm = r(m, Reg::Edx);
    let thread = stack::call(m, 0x4256e4, &[jvm, 10, 0, b + 0x10], Some(thread), |m| {
        crate::native_vm_metadata::construct_thread(
            m,
            thread,
            crate::native_vm_metadata::ThreadInit {
                jvm,
                priority: 10,
                flags: 0,
                name: b + 0x10,
            },
            env,
        )
    })?;
    s(m, Reg::Esi, thread);
    s(m, Reg::Eax, va);
    s(m, Reg::Ecx, args[3]);
    stack::push(m, va);
    stack::push(m, args[3]);
    stack::push(m, thread);
    stack::call(m, 0x4256fa, &[], None, |m| {
        arguments_empty(m, thread, args[3], va, env)
    })?;
    s(m, Reg::Edx, envelope);
    s(m, Reg::Ecx, m.r32(envelope + 0x50));
    stack::add_sp(m, 0xc);
    let receiver = m.r32(envelope + 0x50);
    let result = stack::call(m, 0x42570b, &[thread, args[1]], Some(receiver), |m| {
        stack::function(m, &[thread, args[1]], Some(receiver), |m| {
            s(m, Reg::Eax, args[1]);
            s(m, Reg::Ecx, m.r32(receiver + 0xc));
            let class = r(m, Reg::Ecx);
            stack::call(
                m,
                0x408a43,
                &[class, receiver, args[1]],
                Some(thread),
                |m| call_by_name(m, thread, class, receiver, args[1], fpu, env),
            )
        })
    })?;
    s(m, Reg::Edi, result);
    if result != 0 {
        return Err("NativeVmCallback: immediate method return continuation unavailable".into());
    }
    stack::begin_call(m, 0x425719, &[result], Some(thread));
    Ok(Callback {
        wrapper,
        setup,
        thread,
        base: b,
        finished: false,
    })
}
pub(crate) fn call_by_name(
    m: &mut Space,
    thread: u32,
    class: u32,
    receiver: u32,
    name: u32,
    fpu: &mut native_vm_fpu::State,
    env: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<u32, String> {
    stack::function(m, &[class, receiver, name], Some(thread), |m| {
        stack::alloc(m, 0xc);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, class);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, thread);
        if class == 0 {
            return Err("NativeVmCallback: null method class continuation unavailable".into());
        }
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        let b = stack::sp(m);
        let signature = b + 0x10;
        s(m, Reg::Ecx, signature);
        stack::call(m, 0x4207fc, &[thread], Some(signature), |m| {
            call_signature(m, signature, thread, env)
        })?;
        s(m, Reg::Ecx, signature);
        let descriptor = stack::call(m, 0x420805, &[], Some(signature), |m| {
            stack::function(m, &[], Some(signature), |m| {
                stack::alloc(m, 0x400);
                stack::push_reg(m, Reg::Ebx);
                s(m, Reg::Ebx, signature);
                let d = m.r32(signature + 4);
                s(m, Reg::Eax, d);
                if d == 0 {
                    return Err(
                        "NativeVmCallback: lazy call signature continuation unavailable".into(),
                    );
                }
                Ok::<u32, String>(d)
            })
        })?;
        s(m, Reg::Ebp, name);
        let record = stack::call(m, 0x420812, &[name, descriptor], Some(class), |m| {
            crate::native_vm_lookup::method_by_name(m, class, name, descriptor, env)
        })?;
        s(m, Reg::Esi, record);
        if record == 0 {
            return Err("NativeVmCallback: missing method continuation unavailable".into());
        }
        s(m, Reg::Edx, receiver);
        let result = stack::call(m, 0x420826, &[record, class, receiver], Some(thread), |m| {
            let pending = crate::native_vm_dispatch::begin_materialize(m, env)?;
            let pending = crate::native_vm_dispatch::prepare_script_frame(m, pending, env)?;
            crate::native_vm_dispatch::finish_script_arguments(m, pending, env, fpu)
        })?;
        s(m, Reg::Ebp, result);
        s(m, Reg::Eax, m.r32(record + 0xc));
        if r(m, Reg::Eax) != 0 {
            return Err(
                "NativeVmCallback: uncached call record disposal continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, m.r32(signature + 8));
        if r(m, Reg::Eax) != 0 {
            return Err("NativeVmCallback: call signature cleanup continuation unavailable".into());
        }
        s(m, Reg::Eax, result);
        Ok(result)
    })
}
fn call_signature(
    m: &mut Space,
    signature: u32,
    thread: u32,
    env: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<u32, String> {
    stack::function(m, &[thread], Some(signature), |m| {
        stack::alloc(m, 0x404);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Edi);
        let b = stack::sp(m);
        let buffer = b + 0x10;
        s(m, Reg::Eax, buffer);
        s(m, Reg::Ebp, signature);
        m.w32(signature + 8, 0);
        stack::call(m, 0x41dcd6, &[buffer, 0x60c070, 256], None, |m| {
            crate::native_vm_metadata::name_copy(m, buffer, 0x60c070, 256)
        });
        s(m, Reg::Ebx, thread);
        let jvm = m.r32(thread + 0x10);
        s(m, Reg::Ecx, jvm);
        m.w32(signature, jvm);
        let count = stack::call(m, 0x41dced, &[], Some(thread), |m| {
            crate::native_vm_thread_run::pop_deref(m, thread)
        })?;
        let n = m.r32(count + 8);
        s(m, Reg::Edi, n);
        if count != 0 {
            s(m, Reg::Edx, m.r32(count));
            stack::call(m, 0x41dcfd, &[1], Some(count), |m| {
                crate::native_vm_values::release(m, count, env)
            })?;
        }
        if n != 0 {
            s(m, Reg::Eax, m.r32(thread + 0x20));
            let vector = m.r32(r(m, Reg::Eax) + 0x28);
            s(m, Reg::Ecx, vector);
            let count = m.r32(vector + 8);
            s(m, Reg::Eax, count);
            stack::push_reg(m, Reg::Esi);
            s(m, Reg::Esi, count.wrapping_sub(n));
            stack::store(m, 0x10, count);
            if n > count {
                return Err(
                    "NativeVmCallback: argument count exceeds original operand owner".into(),
                );
            }
            for index in count - n..count {
                let frame = m.r32(thread + 0x20);
                s(m, Reg::Edx, frame);
                let vector = m.r32(frame + 0x28);
                s(m, Reg::Edx, vector);
                if index >= m.r32(vector) {
                    return Err(
                        "NativeVmCallback: signature operand outside original vector".into(),
                    );
                }
                s(m, Reg::Eax, m.r32(vector + 4));
                let cell = m.r32(r(m, Reg::Eax) + index * 4);
                s(m, Reg::Eax, cell);
                let descriptor = m.r32(cell + 4);
                s(m, Reg::Ecx, descriptor);
                let tag = m.r8(descriptor);
                s(m, Reg::Ecx, (r(m, Reg::Ecx) & !255) | u32::from(tag));
                let text = if matches!(tag, b'L' | b'[') && m.r32(cell + 8) != 0 {
                    let object = m.r32(cell + 8);
                    s(m, Reg::Eax, object);
                    let class = m.r32(object + 0xc);
                    s(m, Reg::Ecx, class);
                    s(m, Reg::Edx, m.r32(class + 8));
                    let text = m.r32(r(m, Reg::Edx));
                    s(m, Reg::Eax, text);
                    text
                } else {
                    s(m, Reg::Edx, m.r32(vector + 4));
                    s(m, Reg::Eax, m.r32(r(m, Reg::Edx) + index * 4));
                    s(m, Reg::Eax, descriptor);
                    descriptor
                };
                s(m, Reg::Ecx, buffer);
                stack::call(m, 0x41dd71, &[buffer, text, 256], None, |m| {
                    append(m, buffer, text, 256)
                })?;
                s(m, Reg::Eax, stack::load(m, 0x10));
                s(m, Reg::Esi, index + 1);
            }
            s(m, Reg::Esi, stack::load(m, 0));
            stack::add_sp(m, 4);
        }
        s(m, Reg::Edx, buffer);
        stack::call(m, 0x41dd92, &[buffer, 0x60c074, 256], None, |m| {
            append(m, buffer, 0x60c074, 256)
        })?;
        s(m, Reg::Ecx, jvm);
        let holder = m.r32(jvm + 0x68);
        s(m, Reg::Ecx, holder);
        s(m, Reg::Eax, buffer);
        let name = stack::call(m, 0x41dda5, &[buffer], Some(holder), |m| {
            crate::native_vm_signature::canonical_with_allocator(m, holder, buffer, env)
        })?;
        m.w32(signature + 4, name);
        s(m, Reg::Eax, signature);
        Ok(signature)
    })
}
fn arguments_empty(
    m: &mut Space,
    thread: u32,
    kind: u32,
    va: u32,
    env: &mut impl crate::native_vm_dispatch::Environment,
) -> Result<(), String> {
    if kind != 0x4f {
        return Err("NativeVmCallback: nonempty argument transfer continuation unavailable".into());
    }
    stack::function_cdecl(m, &[thread, kind, va], |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, kind);
        let temporary = stack::external(m, 0x42524d, &[12], None, |m| env.allocate(m, 12))?;
        s(m, Reg::Eax, temporary);
        s(m, Reg::Esi, 0);
        if temporary == 0 {
            return Err("NativeVmCallback: argument vector allocation failed".into());
        }
        for off in [0, 8, 4] {
            m.w32(temporary + off, 0);
        }
        s(m, Reg::Esi, temporary);
        stack::call(m, 0x425466, &[temporary], Some(thread), |m| {
            stack::function(m, &[temporary], Some(thread), |m| {
                stack::push_reg(m, Reg::Ecx);
                stack::push_reg(m, Reg::Ebx);
                s(m, Reg::Ebx, temporary);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                s(m, Reg::Ebp, 0);
                stack::push_reg(m, Reg::Edi);
                stack::store(m, 0x10, thread);
                s(m, Reg::Eax, m.r32(temporary + 8));
                let pool = m.r32(0x62f130);
                s(m, Reg::Esi, pool);
                let cell = m.r32(pool + 0x10);
                s(m, Reg::Eax, cell);
                if cell == 0 {
                    return Err("NativeVmCallback: inline argument-count pool growth continuation unavailable".into());
                }
                s(m, Reg::Ecx, m.r32(cell));
                m.w32(pool + 0x10, r(m, Reg::Ecx));
                s(m, Reg::Ebx, 0);
                s(m, Reg::Ecx, m.r32(0x62e00c));
                m.w32(cell, 0x5e734c);
                s(m, Reg::Edx, m.r32(r(m, Reg::Ecx)));
                m.w32(cell + 8, 0);
                m.w32(cell + 4, r(m, Reg::Edx));
                m.w32(0x62f138, m.r32(0x62f138).wrapping_add(1));
                s(m, Reg::Ebx, cell);
                s(m, Reg::Eax, stack::load(m, 0x10));
                m.w32(cell + 8, 0);
                let frame = m.r32(thread + 0x20);
                s(m, Reg::Ecx, frame);
                let vector = m.r32(frame + 0x28);
                s(m, Reg::Esi, vector);
                let count = m.r32(vector + 8);
                let capacity = m.r32(vector);
                s(m, Reg::Eax, count + 1);
                s(m, Reg::Ecx, capacity);
                if count + 1 > capacity {
                    let new_capacity = capacity + count + 1;
                    s(m, Reg::Edi, new_capacity);
                    if new_capacity == 0 {
                        return Err("NativeVmCallback: argument vector wraparound".into());
                    }
                    let old = m.r32(vector + 4);
                    s(m, Reg::Ecx, old);
                    s(m, Reg::Eax, new_capacity * 4);
                    let data = stack::external(m, 0x41fb81, &[old, new_capacity * 4], None, |m| {
                        env.reallocate(m, old, capacity * 4, new_capacity * 4)
                    })?;
                    s(m, Reg::Eax, data);
                    m.w32(vector + 4, data);
                    if new_capacity < m.r32(vector + 8) {
                        m.w32(vector + 8, new_capacity);
                    }
                    m.w32(vector, new_capacity);
                }
                s(m, Reg::Edx, m.r32(vector + 8));
                s(m, Reg::Eax, m.r32(vector + 4));
                m.w32(r(m, Reg::Eax) + r(m, Reg::Edx) * 4, cell);
                s(m, Reg::Eax, m.r32(vector + 8) + 1);
                m.w32(vector + 8, r(m, Reg::Eax));
                s(m, Reg::Eax, 0);
                Ok::<(), String>(())
            })
        })?;
        stack::call(m, 0x425473, &[0], Some(temporary), |m| {
            crate::native_vm_lookup::resize_zero(m, temporary, env)
        })?;
        stack::external(m, 0x425479, &[temporary], None, |m| env.free(m, temporary))?;
        s(m, Reg::Eax, 1);
        Ok(())
    })
}
pub(crate) fn prepare_loaded(
    m: &mut Space,
    resource: u32,
    flags: u32,
    fpu: &mut native_vm_fpu::State,
) -> Result<(), String> {
    if flags & 0x40000000 != 0 {
        return Err("NativeVmCallback: forced prepare continuation unavailable".into());
    }
    stack::function(m, &[flags, 0, 0], Some(resource), |m| {
        stack::alloc(m, 8);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, 0);
        stack::push_reg(m, Reg::Ebp);
        s(m, Reg::Ebp, 0);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, flags);
        s(m, Reg::Eax, 0);
        s(m, Reg::Esi, resource);
        stack::store(m, 0x14, 0);
        s(m, Reg::Ecx, m.r32(resource + 0x64));
        if r(m, Reg::Ecx) & 1 == 0 {
            return Err("NativeVmCallback: resource request continuation unavailable".into());
        }
        fpu.fld_f32(m.r32(resource + 0x6c))?;
        fpu.fcomp_f32(m.r32(0x5f08f0))?;
        s(
            m,
            Reg::Eax,
            (r(m, Reg::Eax) & 0xffff0000) | u32::from(fpu.status()),
        );
        if fpu.status() & 0x100 != 0 {
            return Err("NativeVmCallback: subunit quality continuation unavailable".into());
        }
        fpu.fld_f32(m.r32(0x5f08f0))?;
        fpu.fld_f32(m.r32(resource + 0x68))?;
        fpu.fcomp_f32(m.r32(0x5f08f0))?;
        s(
            m,
            Reg::Eax,
            (r(m, Reg::Eax) & 0xffff0000) | u32::from(fpu.status()),
        );
        fpu.pop()?;
        if fpu.status() & 0x100 != 0 {
            return Err(
                "NativeVmCallback: pending resource quality continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, m.r32(resource + 0x54));
        if r(m, Reg::Eax) & 0x100 != 0 {
            return Err(
                "NativeVmCallback: spatial resource request continuation unavailable".into(),
            );
        }
        let kind = m.r32(resource + 0x4c);
        s(m, Reg::Edx, kind);
        s(m, Reg::Eax, m.r32(0x619dc0 + kind * 4));
        if r(m, Reg::Eax) != 0 {
            return Err(
                "NativeVmCallback: quality-sensitive resource continuation unavailable".into(),
            );
        }
        s(m, Reg::Eax, stack::load(m, 0x14));
        s(m, Reg::Ecx, m.r32(resource + 0x74));
        if r(m, Reg::Ecx) != m.r32(0x6200a4) {
            return Err("NativeVmCallback: resource touch continuation unavailable".into());
        }
        s(m, Reg::Eax, 0);
        Ok(())
    })
}
pub(crate) fn append(
    m: &mut Space,
    destination: u32,
    source: u32,
    limit: u32,
) -> Result<(), String> {
    stack::function_cdecl(m, &[destination, source, limit], |m| {
        s(m, Reg::Eax, limit);
        s(m, Reg::Ecx, source);
        s(m, Reg::Edx, destination);
        stack::call(m, 0x551154, &[destination, source, limit], None, |m| {
            stack::function_cdecl(m, &[destination, source, limit], |m| {
                s(m, Reg::Eax, destination);
                stack::push_reg(m, Reg::Ebx);
                s(m, Reg::Ebx, limit);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                stack::push_reg(m, Reg::Edi);
                s(m, Reg::Edi, 1);
                s(m, Reg::Ecx, destination);
                if m.r8(destination) != 0 {
                    loop {
                        s(m, Reg::Edx, r(m, Reg::Edi));
                        s(m, Reg::Edi, r(m, Reg::Edi) + 1);
                        if r(m, Reg::Edx) >= limit {
                            break;
                        }
                        let p = r(m, Reg::Ecx);
                        s(
                            m,
                            Reg::Edx,
                            (r(m, Reg::Edx) & !255) | u32::from(m.r8(p + 1)),
                        );
                        s(m, Reg::Ecx, p + 1);
                        if r(m, Reg::Edx) & 255 == 0 {
                            break;
                        }
                    }
                }
                s(m, Reg::Esi, source);
                s(
                    m,
                    Reg::Edx,
                    (r(m, Reg::Edx) & !255) | u32::from(m.r8(source)),
                );
                while r(m, Reg::Edx) & 255 != 0 {
                    s(m, Reg::Ebp, r(m, Reg::Edi));
                    s(m, Reg::Edi, r(m, Reg::Edi) + 1);
                    if r(m, Reg::Ebp) >= limit {
                        break;
                    }
                    m.w8(r(m, Reg::Ecx), r(m, Reg::Edx) as u8);
                    let p = r(m, Reg::Esi);
                    s(
                        m,
                        Reg::Edx,
                        (r(m, Reg::Edx) & !255) | u32::from(m.r8(p + 1)),
                    );
                    s(m, Reg::Ecx, r(m, Reg::Ecx) + 1);
                    s(m, Reg::Esi, p + 1);
                }
                m.w8(r(m, Reg::Ecx), 0);
                Ok(())
            })
        })
    })
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, value: u32) {
    stack::set_reg(m, reg, value);
}
