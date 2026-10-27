//! Retained original403e80 dependency prefix. Recursive class initialization
//! and static-field construction resume from the resulting source frame.
use crate::native_stack::{self as stack, Frame, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
fn write(m: &mut Space, address: u32, value: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(address, 4, value);
    m.w32(address, value);
}
pub struct Initialization {
    pub frame: Frame,
    pub class: u32,
    pub pc: u32,
    base: u32,
    return_pc: u32,
}
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn s(m: &mut Space, reg: Reg, v: u32) {
    stack::set_reg(m, reg, v)
}
///405180: source vector capacity change, including the actual free(NULL)
/// leaf for an empty dependency list.
fn resize(m: &mut Space, vector: u32, n: u32, a: &mut impl Allocator) -> Result<(), String> {
    stack::function(m, &[n], Some(vector), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, n);
        s(m, Reg::Esi, vector);
        let old = m.r32(vector + 4);
        if n == 0 {
            s(m, Reg::Eax, old);
            stack::push(m, old);
            stack::external(m, 0x405195, &[], None, |m| a.deallocate(m, old))?;
            stack::add_sp(m, 4);
            write(m, vector + 4, 0);
        } else {
            s(m, Reg::Edx, old);
            s(m, Reg::Ecx, n * 4);
            stack::push(m, n * 4);
            stack::push(m, old);
            let p = stack::external(m, 0x4051ae, &[], None, |m| {
                a.reallocate(m, old, m.r32(vector) * 4, n * 4)
            })?;
            s(m, Reg::Eax, p);
            stack::add_sp(m, 8);
            if p == 0 {
                return Err("NativeVmClassInit: vector allocation failure".into());
            }
            write(m, vector + 4, p);
        }
        if n < m.r32(vector + 8) {
            write(m, vector + 8, n);
        }
        write(m, vector, n);
        Ok(())
    })
}
pub fn begin(
    m: &mut Space,
    a: &mut impl crate::native_vm_lookup::Environment,
) -> Result<Initialization, String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmClassInit: actual pending source CALL required".into());
    }
    let class = r(m, Reg::Ecx);
    if class == 0 || m.r32(class + 4) == 0 || m.r32(class + 0xc) == 0 || m.r32(class + 0x1d0) != 0 {
        return Err("NativeVmClassInit: fresh raw class required".into());
    }
    let return_pc = stack::load(m, 0);
    let frame = stack::enter(m, &[], Some(class));
    stack::alloc(m, 0x4c);
    stack::push_reg(m, Reg::Ebp);
    stack::push_reg(m, Reg::Edi);
    s(m, Reg::Edi, class);
    s(m, Reg::Eax, m.r32(class + 0x1d0));
    s(m, Reg::Ebp, 0);
    let module = m.r32(class + 4);
    s(m, Reg::Eax, module);
    let base_index = m.r32(module + 0x40);
    s(m, Reg::Eax, base_index);
    if (base_index as i32) < 0 {
        write(m, class + 0x1c8, 0);
    } else {
        let pool = m.r32(class + 0x14);
        s(m, Reg::Ecx, pool);
        stack::push(m, base_index);
        stack::push(m, pool);
        let jvm = m.r32(class + 0xc);
        s(m, Reg::Ecx, jvm);
        let base = stack::call(m, 0x403eb6, &[], Some(jvm), |m| {
            crate::native_vm_lookup::class_from_pool_with(m, jvm, pool, base_index, a)
        })?;
        stack::add_sp(m, 8);
        write(m, class + 0x1c8, base);
    }
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Esi);
    stack::push(m, 12);
    let vector = stack::external(m, 0x403ec5, &[], None, |m| a.allocate(m, 12))?;
    s(m, Reg::Eax, vector);
    s(m, Reg::Esi, vector);
    stack::add_sp(m, 4);
    if vector == 0 {
        return Err("NativeVmClassInit: dependency owner allocation failure".into());
    }
    s(m, Reg::Edx, module);
    let count = m.r32(module + 0x44);
    s(m, Reg::Eax, count);
    stack::push(m, count);
    s(m, Reg::Ecx, vector);
    for off in [0, 8, 4] {
        write(m, vector + off, 0);
    }
    stack::call(m, 0x403ee4, &[], Some(vector), |m| {
        resize(m, vector, count, a)
    })?;
    stack::add_sp(m, 4);
    s(m, Reg::Eax, module);
    write(m, class + 0x1cc, vector);
    write(m, stack::sp(m) + 0x14, 0);
    let mut index = 0;
    while index < count {
        let indices = m.r32(module + 0x48);
        s(m, Reg::Eax, indices);
        let ci = m.r32(indices + index * 4);
        s(m, Reg::Ecx, ci);
        let pool = m.r32(class + 0x14);
        s(m, Reg::Edx, pool);
        stack::push(m, ci);
        let jvm = m.r32(class + 0xc);
        s(m, Reg::Ecx, jvm);
        stack::push(m, pool);
        let dep = stack::call(m, 0x403f11, &[], Some(jvm), |m| {
            crate::native_vm_lookup::class_from_pool_with(m, jvm, pool, ci, a)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Ebx, dep);
        if m.r32(dep + 0x10) != 1 {
            return Err(
                "NativeVmClassInit: invalid-interface diagnostic continuation unavailable".into(),
            );
        }
        s(m, Reg::Esi, vector);
        let used = m.r32(vector + 8);
        let capacity = m.r32(vector);
        s(m, Reg::Eax, used + 1);
        s(m, Reg::Ecx, capacity);
        if used + 1 > capacity {
            let next = capacity + used + 1;
            s(m, Reg::Ecx, next);
            stack::push(m, next);
            s(m, Reg::Ecx, vector);
            stack::call(m, 0x403fb8, &[], Some(vector), |m| {
                resize(m, vector, next, a)
            })?;
            stack::add_sp(m, 4);
        }
        s(m, Reg::Edx, m.r32(vector + 8));
        s(m, Reg::Eax, m.r32(vector + 4));
        write(m, r(m, Reg::Eax) + r(m, Reg::Edx) * 4, dep);
        write(m, vector + 8, m.r32(vector + 8) + 1);
        s(m, Reg::Eax, module);
        s(m, Reg::Ecx, count);
        index += 1;
        s(m, Reg::Ebp, index);
        write(m, stack::sp(m) + 0x14, index);
    }
    if count > 0 {
        s(m, Reg::Ebp, 0);
    }
    a.observe_owner(
        m,
        crate::native_vm_context::Observation::Dependencies {
            class,
            base: m.r32(class + 0x1c8),
            interfaces: (0..m.r32(vector + 8))
                .map(|i| m.r32(m.r32(vector + 4) + i * 4))
                .collect(),
        },
    )?;
    Ok(Initialization {
        frame,
        class,
        pc: 0x403fd9,
        base: stack::sp(m),
        return_pc,
    })
}
impl Initialization {
    pub fn finish_empty_owner(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_dispatch::Environment,
    ) -> Result<(), String> {
        if self.pc != 0x404377 || stack::sp(m) != self.base || r(m, Reg::Edi) != self.class {
            return Err("NativeVmClassInit: static owner continuation context mismatch".into());
        }
        let thread = self.base + 0x24;
        s(m, Reg::Ecx, thread);
        stack::call(m, 0x404380, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::mark_terminal(m, thread)
        })?;
        s(m, Reg::Ecx, thread);
        stack::call(m, 0x404389, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::destroy(m, thread, e)
        })?;
        stack::leave(m, self.frame);
        self.pc = self.return_pc;
        Ok(())
    }
    /// Construct the declared static variables whose TREE initializer is absent.
    /// A present initializer stops at its actual source boundary after installing
    /// the variable; it cannot be replaced by a semantic final-value write.
    pub fn construct_plain_fields(
        &mut self,
        m: &mut Space,
        a: &mut impl crate::native_vm_context::Environment,
    ) -> Result<(), String> {
        if self.pc != 0x404092 || stack::sp(m) != self.base || r(m, Reg::Edi) != self.class {
            return Err("NativeVmClassInit: static field continuation context mismatch".into());
        }
        let class = self.class;
        let module = m.r32(class + 4);
        let pool = m.r32(class + 0x14);
        let fields = m.r32(module + 0x54);
        let count = m.r32(fields + 0xc);
        let jvm = m.r32(class + 0xc);
        let mut index = m.r32(self.base + 0x14);
        while index < count {
            if index > 0 {
                s(m, Reg::Ebx, m.r32(self.base + 0x1c));
                s(m, Reg::Eax, m.r32(self.base + 0x18));
                s(m, Reg::Ecx, index);
                s(m, Reg::Ebp, 0);
            }
            if index >= m.r32(fields + 4) {
                return Err("NativeVmClassInit: missing serialized static field".into());
            }
            s(m, Reg::Eax, m.r32(fields + 8));
            let field = m.r32(r(m, Reg::Eax) + index * 4);
            s(m, Reg::Esi, field);
            write(m, self.base + 0x10, field);
            let type_index = m.r32(field + 12);
            s(m, Reg::Ecx, type_index);
            stack::push(m, type_index);
            s(m, Reg::Ecx, pool);
            let descriptor = stack::call(m, 0x4040b4, &[], Some(pool), |m| {
                crate::native_vm_lookup::constant(m, pool, type_index, 0, 8)
            })?;
            stack::add_sp(m, 4);
            let name_index = m.r32(field + 8);
            s(m, Reg::Edx, name_index);
            stack::push(m, name_index);
            s(m, Reg::Ecx, pool);
            s(m, Reg::Ebp, descriptor);
            let name = stack::call(m, 0x4040c1, &[], Some(pool), |m| {
                crate::native_vm_lookup::constant(m, pool, name_index, 0, 8)
            })?;
            stack::add_sp(m, 4);
            let header = m.r32(0x62f134);
            s(m, Reg::Ecx, header);
            s(m, Reg::Ebx, name);
            let variable = stack::call(m, 0x4040ce, &[], Some(header), |m| {
                crate::native_vm_values::allocate_variable(m, header, a)
            })?;
            s(m, Reg::Esi, variable);
            if variable == 0 {
                return Err("NativeVmClassInit: variable allocation failed".into());
            }
            s(m, Reg::Eax, jvm);
            stack::push(m, descriptor);
            stack::push(m, jvm);
            s(m, Reg::Ecx, variable + 4);
            write(m, variable, 0x5e734c);
            stack::call(m, 0x4040e7, &[], Some(variable + 4), |m| {
                crate::native_vm_signature::descriptor(m, variable + 4, jvm, descriptor, a)
            })?;
            stack::add_sp(m, 8);
            write(m, variable + 8, 0);
            write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
            write(m, variable, 0x5e7344);
            write(m, variable + 12, name);
            let owner = m.r32(class + 0x1d0);
            s(m, Reg::Ecx, owner);
            stack::push(m, variable);
            let inserted = stack::call(m, 0x40410d, &[], Some(owner), |m| {
                crate::native_vm_metadata::variables::append(m, owner, variable, a)
            })?;
            stack::add_sp(m, 4);
            if inserted & 255 == 0 {
                return Err(
                    "NativeVmClassInit: duplicate-field diagnostic continuation required".into(),
                );
            }
            a.observe_owner(
                m,
                crate::native_vm_context::Observation::StaticField {
                    class,
                    index: index as usize,
                    variable,
                    record: field,
                },
            )?;
            s(m, Reg::Ecx, field);
            let initializer = m.r32(field + 16);
            s(m, Reg::Eax, initializer);
            if (initializer as i32) >= 0 {
                self.pc = 0x4041ab;
                return Ok(());
            }
            s(m, Reg::Eax, index);
            s(m, Reg::Ecx, fields);
            s(m, Reg::Edx, count);
            index += 1;
            s(m, Reg::Eax, index);
            write(m, self.base + 0x14, index);
        }
        self.pc = 0x404377;
        Ok(())
    }
    /// Resume after the dependencies' own initialization has completed. Every
    /// dependency must already own its actual installed raw static storage.
    pub fn install_empty_owner(
        &mut self,
        m: &mut Space,
        a: &mut impl crate::native_vm_context::Environment,
    ) -> Result<(), String> {
        self.install_with(m, a, |m, class, _| {
            crate::native_vm_lookup::initialized_class(m, class)
        })
    }
    fn install_with<
        A: crate::native_vm_context::Environment,
        F: FnMut(&mut Space, u32, &mut A) -> Result<(), String>,
    >(
        &mut self,
        m: &mut Space,
        a: &mut A,
        mut initialize: F,
    ) -> Result<(), String> {
        if self.pc != 0x403fd9 || r(m, Reg::Edi) != self.class {
            return Err("NativeVmClassInit: dependency continuation context mismatch".into());
        }
        let class = self.class;
        let module = m.r32(class + 4);
        let base = m.r32(class + 0x1c8);
        s(m, Reg::Ecx, base);
        if base != 0 {
            stack::call(m, 0x403fe8, &[], Some(base), |m| initialize(m, base, a))?;
        }
        let vector = m.r32(class + 0x1cc);
        s(m, Reg::Eax, vector);
        let count = m.r32(vector + 8);
        s(m, Reg::Ecx, count);
        s(m, Reg::Esi, 0);
        for index in 0..count {
            if index >= m.r32(vector) {
                return Err("NativeVmClassInit: dependency index outside raw vector".into());
            }
            let table = m.r32(vector + 4);
            s(m, Reg::Ecx, table);
            let dep = m.r32(table + index * 4);
            s(m, Reg::Ecx, dep);
            stack::call(m, 0x40400a, &[], Some(dep), |m| initialize(m, dep, a))?;
            s(m, Reg::Eax, vector);
            s(m, Reg::Ecx, count);
            s(m, Reg::Esi, index + 1);
        }
        stack::push(m, 20);
        let owner = stack::external(m, 0x40401f, &[], None, |m| a.allocate(m, 20))?;
        s(m, Reg::Eax, owner);
        stack::add_sp(m, 4);
        if owner == 0 {
            return Err("NativeVmClassInit: static owner allocation failure".into());
        }
        let jvm = m.r32(class + 0xc);
        s(m, Reg::Edx, jvm);
        stack::push(m, jvm);
        s(m, Reg::Ecx, owner);
        stack::call(m, 0x404031, &[], Some(owner), |m| {
            crate::native_vm_metadata::variables::construct(m, owner, jvm, a)
        })?;
        stack::add_sp(m, 4);
        stack::push(m, 0x60754c);
        stack::push(m, 1);
        write(m, class + 0x1d0, owner);
        a.observe_owner(
            m,
            crate::native_vm_context::Observation::StaticOwner { class, owner },
        )?;
        s(m, Reg::Eax, jvm);
        stack::push(m, 10);
        stack::push(m, jvm);
        let thread = stack::sp(m) + 0x34;
        s(m, Reg::Ecx, thread);
        let input = crate::native_vm_metadata::ThreadInit {
            jvm,
            priority: 10,
            flags: 1,
            name: 0x60754c,
        };
        stack::call(m, 0x404051, &[], Some(thread), |m| {
            crate::native_vm_metadata::construct_thread(m, thread, input, a)
        })?;
        stack::add_sp(m, 16);
        s(m, Reg::Edx, module);
        let fields = m.r32(module + 0x54);
        s(m, Reg::Eax, fields);
        let pool = m.r32(class + 0x14);
        s(m, Reg::Ebx, pool);
        let tree = m.r32(class + 0x18);
        s(m, Reg::Ecx, tree);
        let sp = stack::sp(m);
        write(m, sp + 0x1c, pool);
        write(m, sp + 0x20, tree);
        write(m, sp + 0x18, fields);
        if fields != 0 {
            s(m, Reg::Edx, m.r32(fields + 0xc));
            s(m, Reg::Ecx, 0);
            write(m, sp + 0x14, 0);
            if m.r32(fields + 0xc) > 0 {
                self.pc = 0x404092;
                return Ok(());
            }
        }
        self.pc = 0x404377;
        Ok(())
    }
}
/// Complete ordinary class initialization with no declared static fields.
/// Dependencies recurse through the same source function, retaining each
/// parent's caller and initializer-thread lifecycle.
pub fn initialize_empty<
    E: crate::native_vm_dispatch::Environment + crate::native_vm_lookup::Environment,
>(
    m: &mut Space,
    class: u32,
    e: &mut E,
) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) || r(m, Reg::Ecx) != class {
        return Err("NativeVmClassInit: actual matching source CALL required".into());
    }
    if m.r32(class + 0x1d0) != 0 {
        return crate::native_vm_lookup::initialized_class(m, class);
    }
    let mut init = begin(m, e)?;
    init.install_with(m, e, initialize_empty::<E>)?;
    if init.pc == 0x404092 {
        init.construct_plain_fields(m, e)?;
    }
    init.finish_empty_owner(m, e)
}
struct FieldRun {
    run: crate::native_vm_thread_run::Run,
    frame: u32,
    variable: u32,
    thread: u32,
}
impl Initialization {
    fn start_field<
        E: crate::native_vm_dispatch::Environment + crate::native_vm_thread_run::Environment,
    >(
        &mut self,
        m: &mut Space,
        fpu: &mut crate::native_vm_fpu::State,
        e: &mut E,
    ) -> Result<FieldRun, String> {
        if self.pc != 0x4041ab || stack::sp(m) != self.base {
            return Err("NativeVmClassInit: initializer CALL context mismatch".into());
        }
        let class = self.class;
        let variable = r(m, Reg::Esi);
        let initializer = r(m, Reg::Eax);
        let descriptor = m.r32(class + 8);
        s(m, Reg::Edx, descriptor);
        s(m, Reg::Ebx, m.r32(variable + 4));
        if m.r32(descriptor) == r(m, Reg::Ebx) {
            return Err(
                "NativeVmClassInit: self-containing field diagnostic continuation required".into(),
            );
        }
        let tree = m.r32(self.base + 0x20);
        s(m, Reg::Ecx, tree);
        if initializer >= m.r32(tree) {
            return Err("NativeVmClassInit: missing initializer TREE".into());
        }
        s(m, Reg::Ecx, m.r32(tree + 4));
        let pc = m.r32(r(m, Reg::Ecx) + initializer * 4);
        s(m, Reg::Edx, pc);
        write(m, self.base + 0x10, pc);
        let pool = m.r32(0x62ddf0);
        s(m, Reg::Ebp, pool);
        let mut frame = m.r32(pool + 16);
        s(m, Reg::Ecx, frame);
        if frame != 0 {
            s(m, Reg::Eax, m.r32(frame));
            write(m, pool + 16, r(m, Reg::Eax));
        } else {
            let capacity = m.r32(pool + 12);
            let handed = m.r32(pool + 8);
            s(m, Reg::Eax, capacity);
            if handed == capacity {
                let blocks = m.r32(pool + 4);
                let table = m.r32(pool);
                s(m, Reg::Ebx, blocks);
                s(m, Reg::Edx, table);
                s(m, Reg::Eax, capacity + 256);
                write(m, pool + 12, capacity + 256);
                s(m, Reg::Eax, blocks + 1);
                s(m, Reg::Ecx, (blocks + 1) * 4);
                stack::push(m, (blocks + 1) * 4);
                stack::push(m, table);
                write(m, pool + 4, blocks + 1);
                let table = stack::external(m, 0x404269, &[], None, |m| {
                    crate::native_vm_pool::Allocator::reallocate(
                        e,
                        m,
                        table,
                        blocks * 4,
                        (blocks + 1) * 4,
                    )
                })?;
                s(m, Reg::Eax, table);
                stack::push(m, 0x4000);
                write(m, pool, table);
                let block = stack::external(m, 0x404276, &[], None, |m| {
                    crate::native_vm_pool::Allocator::allocate(e, m, 0x4000)
                })?;
                s(m, Reg::Eax, block);
                s(m, Reg::Ecx, table);
                stack::add_sp(m, 12);
                write(m, table + blocks * 4, block);
            }
            s(m, Reg::Ecx, handed);
            s(m, Reg::Edx, handed + 1);
            write(m, pool + 8, handed + 1);
            s(m, Reg::Eax, handed / 256);
            s(m, Reg::Edx, m.r32(pool));
            s(m, Reg::Ebx, (handed % 256) * 64);
            let block = m.r32(m.r32(pool) + (handed / 256) * 4);
            s(m, Reg::Ecx, block);
            frame = block + (handed % 256) * 64;
            s(m, Reg::Ecx, frame);
        }
        if frame == 0 {
            return Err("NativeVmClassInit: initializer frame allocation failure".into());
        }
        s(m, Reg::Eax, pc);
        stack::push(m, 0);
        stack::push(m, 0);
        stack::push(m, pc);
        stack::push(m, class);
        let thread = self.base + 0x24;
        s(m, Reg::Edx, thread);
        stack::push(m, thread);
        let input = crate::native_vm_metadata::FrameInit {
            thread,
            context: class,
            pc,
            receiver: 0,
            locals: 0,
        };
        stack::call(m, 0x4042cb, &[], Some(frame), |m| {
            crate::native_vm_metadata::construct_frame(m, frame, input, e)
        })?;
        stack::add_sp(m, 20);
        s(m, Reg::Ebp, frame);
        write(m, frame + 0x38, 0x6075c0);
        let owner = m.r32(self.base + 0x30);
        s(m, Reg::Eax, owner);
        let tail = m.r32(owner + 8);
        s(m, Reg::Ecx, tail);
        s(m, Reg::Eax, owner + 8);
        write(m, frame + 4, tail);
        s(m, Reg::Edx, tail);
        let next = m.r32(tail + 8);
        s(m, Reg::Ecx, next);
        write(m, frame + 8, next);
        s(m, Reg::Edx, tail);
        write(m, tail + 8, frame);
        write(m, owner + 8, frame);
        s(m, Reg::Ecx, thread);
        write(m, self.base + 0x44, frame);
        stack::call(m, 0x404305, &[], Some(thread), |m| {
            stack::function(m, &[], Some(thread), |m| {
                let current = m.r32(thread + 0x20);
                s(m, Reg::Eax, current);
                s(m, Reg::Edx, m.r32(current + 0x2c));
                write(m, thread + 0x24, r(m, Reg::Edx));
            })
        });
        let index = m.r32(self.base + 0x14) as usize;
        let fields = m.r32(self.base + 0x18);
        let record = m.r32(m.r32(fields + 8) + index as u32 * 4);
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::StaticFieldFrame {
                address: frame,
                class,
                index,
                variable,
                record,
            },
        )?;
        stack::push(m, variable);
        s(m, Reg::Ecx, thread);
        write(m, self.base + 0x50, 1);
        stack::call(m, 0x404317, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::push_argument(m, thread, variable, e)
        })?;
        stack::add_sp(m, 4);
        stack::push(m, 0);
        s(m, Reg::Ecx, thread);
        stack::begin_call(m, 0x404322, &[], Some(thread));
        let run = crate::native_vm_thread_run::begin(m, fpu, e)?;
        Ok(FieldRun {
            run,
            frame,
            variable,
            thread,
        })
    }
    fn finish_field(
        &mut self,
        m: &mut Space,
        pending: FieldRun,
        fpu: &mut crate::native_vm_fpu::State,
        e: &mut impl crate::native_vm_dispatch::Environment,
    ) -> Result<(), String> {
        if !pending.run.has_returned() || pending.run.pc != 0x404322 || stack::sp(m) != self.base {
            return Err("NativeVmClassInit: initializer has not returned".into());
        }
        fpu.pop()?;
        let thread = pending.thread;
        let variable = pending.variable;
        let frame = pending.frame;
        let operands = m.r32(frame + 0x28);
        s(m, Reg::Ecx, thread);
        let value = stack::call(m, 0x40432d, &[], Some(thread), |m| {
            crate::native_vm_thread_run::pop_deref_with(m, thread, e)
        })?;
        s(m, Reg::Ecx, thread);
        s(m, Reg::Ebx, value);
        let destination_index = m
            .r32(operands + 8)
            .checked_sub(1)
            .ok_or("NativeVmClassInit: initializer destination stack absent")?
            as usize;
        let popped = stack::call(m, 0x404338, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::pop_variable(m, thread)
        })?;
        if popped != variable {
            return Err("NativeVmClassInit: initializer popped another destination".into());
        }
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::OperandPop {
                thread,
                frame,
                index: destination_index,
                cell: popped,
            },
        )?;
        let source_before = crate::native_vm_context::ValueSnapshot::read(m, value)?;
        let destination_before = crate::native_vm_context::ValueSnapshot::read(m, variable)?;
        let converter = m.r32(self.base + 0x38);
        s(m, Reg::Ecx, converter);
        stack::push(m, 0x23);
        stack::push(m, value);
        stack::push(m, variable);
        stack::call(m, 0x404345, &[], Some(converter), |m| {
            crate::native_vm_alu::assignment(m, converter, variable, value, fpu)
        })?;
        stack::add_sp(m, 12);
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::InitializerAssignment {
                thread,
                frame,
                source: value,
                destination: variable,
                source_before,
                destination_before,
            },
        )?;
        if value != 0 {
            s(m, Reg::Eax, m.r32(value));
            stack::push(m, 1);
            s(m, Reg::Ecx, value);
            stack::call(m, 0x404352, &[], Some(value), |m| {
                crate::native_vm_values::release(m, value, e)
            })?;
            stack::add_sp(m, 4);
        }
        s(m, Reg::Edx, m.r32(frame));
        stack::push(m, 1);
        s(m, Reg::Ecx, frame);
        stack::call(m, 0x40435f, &[], Some(frame), |m| {
            crate::native_vm_values::release_frame(m, frame, e)
        })?;
        stack::add_sp(m, 4);
        let index = m.r32(self.base + 0x14);
        let fields = m.r32(self.base + 0x18);
        s(m, Reg::Eax, index);
        s(m, Reg::Ecx, fields);
        s(m, Reg::Edx, m.r32(fields + 0xc));
        s(m, Reg::Eax, index + 1);
        write(m, self.base + 0x14, index + 1);
        self.pc = if index + 1 < m.r32(fields + 0xc) {
            0x404092
        } else {
            0x404377
        };
        Ok(())
    }
}
/// Source initializer scripts execute in their actual raw Thread.run frames.
/// Unsupported nested native entries remain guarded by Run's capability API.
pub fn initialize_with_scripts<
    E: crate::native_vm_dispatch::Environment + crate::native_vm_thread_run::Environment,
>(
    m: &mut Space,
    class: u32,
    fpu: &mut crate::native_vm_fpu::State,
    e: &mut E,
) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) || r(m, Reg::Ecx) != class {
        return Err("NativeVmClassInit: actual matching source CALL required".into());
    }
    if m.r32(class + 0x1d0) != 0 {
        return crate::native_vm_lookup::initialized_class(m, class);
    }
    let mut init = begin(m, e)?;
    init.install_with(m, e, |m, class, e| {
        initialize_with_scripts(m, class, fpu, e)
    })?;
    loop {
        if init.pc == 0x404092 {
            init.construct_plain_fields(m, e)?;
        }
        if init.pc != 0x4041ab {
            break;
        }
        let mut pending = init.start_field(m, fpu, e)?;
        crate::native_vm_thread_run::drive(m, &mut pending.run, fpu, e)?;
        init.finish_field(m, pending, fpu, e)?;
    }
    init.finish_empty_owner(m, e)
}
#[cfg(test)]
#[path = "native_vm_class_init_tests.rs"]
mod tests;
