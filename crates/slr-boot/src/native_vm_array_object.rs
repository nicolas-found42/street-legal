//! Source404e60 ->407da0 array instance allocation with original variable
//! owners, hidden storage and element cells. Instance initializers stay guarded.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_thread_run::Environment;
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
fn allocate(m: &mut Space, pc: u32, n: u32, e: &mut impl Environment) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| e.allocate(m, n))?;
    s(m, Reg::Eax, p);
    if p == 0 {
        return Err("NativeVmArrayObject: allocation failure".into());
    }
    Ok(p)
}
fn inline_variable(m: &mut Space, length: u32, element: bool) -> Result<u32, String> {
    let h = m.r32(0x62f134);
    s(m, Reg::Esi, h);
    let free = m.r32(h + 16);
    s(m, Reg::Eax, free);
    if element {
        s(m, Reg::Ebx, 0);
    }
    if free != 0 {
        let next = m.r32(free);
        s(m, if element { Reg::Ecx } else { Reg::Edx }, next);
        write(m, h + 16, next);
        return Ok(free);
    }
    let cap = m.r32(h + 12);
    s(m, Reg::Eax, cap);
    let handed = m.r32(h + 8);
    if handed == cap {
        return Err(
            "NativeVmArrayObject: inline variable pool growth continuation required".into(),
        );
    }
    s(m, Reg::Edi, handed);
    s(m, if element { Reg::Edx } else { Reg::Eax }, handed + 1);
    write(m, h + 8, handed + 1);
    let q = handed / 84000;
    let rem = handed % 84000;
    s(m, Reg::Ecx, q);
    s(m, Reg::Eax, q);
    s(m, Reg::Edx, rem);
    s(m, Reg::Edi, 84000);
    let table = m.r32(h);
    s(m, Reg::Esi, table);
    let block = m.r32(table + q * 4);
    s(m, Reg::Eax, block);
    if !element {
        s(m, Reg::Edi, length);
    }
    s(m, Reg::Edx, rem * 16);
    let cell = block + rem * 16;
    s(m, Reg::Eax, cell);
    if element {
        s(m, Reg::Ebx, 0);
    }
    Ok(cell)
}
fn initialize_field(
    m: &mut Space,
    base: u32,
    initializer: u32,
    fpu: &mut crate::native_vm_fpu::State,
    e: &mut impl Environment,
) -> Result<(), String> {
    let tree = m.r32(base + 0x2c);
    s(m, Reg::Ecx, tree);
    if initializer >= m.r32(tree) {
        return Err("NativeVmArrayObject: missing instance initializer TREE".into());
    }
    s(m, Reg::Ecx, m.r32(tree + 4));
    let pc = m.r32(r(m, Reg::Ecx) + initializer * 4);
    s(m, Reg::Esi, pc);
    let h = m.r32(0x62ddf0);
    s(m, Reg::Ecx, h);
    let frame = stack::call(m, 0x407fdb, &[], Some(h), |m| {
        crate::native_vm_values::allocate_frame(m, h, e)
    })?;
    if frame == 0 {
        return Err("NativeVmArrayObject: initializer frame allocation failure".into());
    }
    let object = m.r32(base + 0x14);
    let context = m.r32(base + 0x10);
    let thread = base + 0x50;
    s(m, Reg::Edx, object);
    s(m, Reg::Ecx, context);
    stack::push(m, 0);
    stack::push(m, object);
    stack::push(m, pc);
    stack::push(m, context);
    s(m, Reg::Edx, thread);
    stack::push(m, thread);
    s(m, Reg::Ecx, frame);
    let input = crate::native_vm_metadata::FrameInit {
        thread,
        context,
        pc,
        receiver: object,
        locals: 0,
    };
    stack::call(m, 0x407ff8, &[], Some(frame), |m| {
        crate::native_vm_metadata::construct_frame(m, frame, input, e)
    })?;
    stack::add_sp(m, 20);
    s(m, Reg::Esi, frame);
    write(m, frame + 0x38, 0x607984);
    let list = m.r32(base + 0x5c);
    s(m, Reg::Ecx, list);
    stack::push(m, frame);
    stack::call(m, 0x40800f, &[], Some(list), |m| {
        stack::function(m, &[frame], Some(list), |m| {
            s(m, Reg::Eax, frame);
            let tail = m.r32(list + 8);
            s(m, Reg::Edx, tail);
            write(m, frame + 4, tail);
            s(m, Reg::Edx, m.r32(list + 8));
            s(m, Reg::Edx, m.r32(r(m, Reg::Edx) + 8));
            write(m, frame + 8, r(m, Reg::Edx));
            s(m, Reg::Edx, m.r32(list + 8));
            write(m, r(m, Reg::Edx) + 8, frame);
            write(m, list + 8, frame);
        })
    });
    stack::add_sp(m, 4);
    s(m, Reg::Ecx, thread);
    write(m, base + 0x70, frame);
    stack::call(m, 0x40801c, &[], Some(thread), |m| {
        stack::function(m, &[], Some(thread), |m| {
            s(m, Reg::Eax, m.r32(thread + 0x20));
            s(m, Reg::Edx, m.r32(r(m, Reg::Eax) + 0x2c));
            write(m, thread + 0x24, r(m, Reg::Edx));
        })
    });
    let variable = r(m, Reg::Ebp);
    stack::push(m, variable);
    s(m, Reg::Ecx, thread);
    write(m, base + 0x7c, 1);
    stack::call(m, 0x408031, &[], Some(thread), |m| {
        crate::native_vm_thread_lifecycle::push_argument(m, thread, variable, e)
    })?;
    stack::add_sp(m, 4);
    stack::push(m, 0);
    s(m, Reg::Ecx, thread);
    stack::begin_call(m, 0x40803c, &[], Some(thread));
    let mut run = crate::native_vm_thread_run::begin(m, fpu, e)?;
    crate::native_vm_thread_run::drive(m, &mut run, fpu, e)?;
    if run.pc != 0x40803c || stack::sp(m) != base {
        return Err("NativeVmArrayObject: initializer did not return to its source caller".into());
    }
    fpu.pop()?;
    s(m, Reg::Ecx, thread);
    let value = stack::call(m, 0x408047, &[], Some(thread), |m| {
        crate::native_vm_thread_run::pop_deref(m, thread)
    })?;
    s(m, Reg::Ecx, thread);
    s(m, Reg::Esi, value);
    stack::call(m, 0x408052, &[], Some(thread), |m| {
        crate::native_vm_thread_lifecycle::pop_variable(m, thread)
    })?;
    let alu = m.r32(base + 0x64);
    s(m, Reg::Ecx, alu);
    stack::push(m, 0x23);
    stack::push(m, value);
    stack::push(m, variable);
    stack::call(m, 0x40805f, &[], Some(alu), |m| {
        crate::native_vm_alu::assignment(m, alu, variable, value, fpu)
    })?;
    stack::add_sp(m, 12);
    if value != 0 {
        s(m, Reg::Eax, m.r32(value));
        stack::push(m, 1);
        s(m, Reg::Ecx, value);
        stack::call(m, 0x40806c, &[], Some(value), |m| {
            crate::native_vm_values::release(m, value, e)
        })?;
        stack::add_sp(m, 4);
    }
    Ok(())
}
fn construct(
    m: &mut Space,
    object: u32,
    class: u32,
    length: u32,
    fpu: &mut crate::native_vm_fpu::State,
    e: &mut impl Environment,
) -> Result<(), String> {
    stack::function(m, &[class, length], Some(object), |m| {
        stack::alloc(m, 0x78);
        stack::push_reg(m, Reg::Ebx);
        s(m, Reg::Ebx, class);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        s(m, Reg::Ebp, object);
        stack::push_reg(m, Reg::Edi);
        let base = stack::sp(m);
        s(m, Reg::Ecx, class);
        write(m, base + 0x14, object);
        stack::call(m, 0x407dbb, &[], Some(class), |m| {
            crate::native_vm_lookup::initialized_class(m, class)
        })?;
        stack::push(m, 0x607948);
        stack::push(m, 1);
        write(m, object + 12, class);
        write(m, object + 0x14, 0);
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::ObjectBegin {
                address: object,
                class,
                length,
            },
        )?;
        let jvm = m.r32(class + 12);
        s(m, Reg::Eax, jvm);
        stack::push(m, 10);
        stack::push(m, jvm);
        let thread = base + 0x50;
        s(m, Reg::Ecx, thread);
        let input = crate::native_vm_metadata::ThreadInit {
            jvm,
            priority: 10,
            flags: 1,
            name: 0x607948,
        };
        stack::call(m, 0x407ddb, &[], Some(thread), |m| {
            crate::native_vm_metadata::construct_thread(m, thread, input, e)
        })?;
        stack::add_sp(m, 16);
        stack::push(m, 20);
        let owner = allocate(m, 0x407de2, 20, e)?;
        stack::add_sp(m, 4);
        s(m, Reg::Ecx, jvm);
        stack::push(m, jvm);
        s(m, Reg::Ecx, owner);
        stack::call(m, 0x407df4, &[], Some(owner), |m| {
            crate::native_vm_metadata::variables::construct(m, owner, jvm, e)
        })?;
        stack::add_sp(m, 4);
        write(m, object + 16, owner);
        s(m, Reg::Eax, jvm);
        write(m, jvm + 0x34, m.r32(jvm + 0x34).wrapping_add(1));
        s(m, Reg::Edx, jvm);
        let gc = m.r32(jvm + 0x20);
        s(m, Reg::Esi, gc);
        let marked = u32::from(gc != 0);
        s(m, Reg::Eax, marked);
        write(m, object + 0x18, marked);
        s(m, Reg::Ecx, jvm);
        let objects = m.r32(jvm + 0x10);
        s(m, Reg::Eax, objects);
        let tail = m.r32(objects + 0x18);
        s(m, Reg::Edx, tail);
        s(m, Reg::Ecx, m.r32(tail + 4));
        write(m, object + 4, r(m, Reg::Ecx));
        s(m, Reg::Edx, tail);
        write(m, object + 8, tail);
        s(m, Reg::Ecx, tail);
        write(m, tail + 4, object);
        write(m, objects + 0x18, object);
        let mut depth = 0;
        let mut ancestor = m.r32(class + 0x1c8);
        s(m, Reg::Eax, ancestor);
        s(m, Reg::Edx, 0);
        write(m, base + 0x1c, 0);
        while ancestor != 0 {
            ancestor = m.r32(ancestor + 0x1c8);
            s(m, Reg::Eax, ancestor);
            depth += 1;
            s(m, Reg::Edx, depth);
        }
        if depth != 0 {
            write(m, base + 0x1c, depth);
        }
        s(m, Reg::Ecx, depth);
        write(m, base + 0x24, depth);
        for remaining in (0..=depth).rev() {
            s(m, Reg::Eax, class);
            write(m, base + 0x10, class);
            let mut current = class;
            let mut walk = remaining;
            while walk > 0 {
                walk -= 1;
                s(m, Reg::Ecx, walk);
                current = m.r32(current + 0x1c8);
                s(m, Reg::Eax, current);
                write(m, base + 0x10, current);
            }
            let pool = m.r32(current + 0x14);
            s(m, Reg::Esi, pool);
            let tree = m.r32(current + 0x18);
            s(m, Reg::Ecx, tree);
            let module = m.r32(current + 4);
            s(m, Reg::Eax, module);
            let fields = m.r32(module + 0x4c);
            s(m, Reg::Eax, fields);
            write(m, base + 0x28, pool);
            write(m, base + 0x2c, tree);
            write(m, base + 0x20, fields);
            if fields != 0 {
                let count = m.r32(fields + 12);
                s(m, Reg::Edi, count);
                s(m, Reg::Ecx, 0);
                write(m, base + 0x18, 0);
                for index in 0..count {
                    if index > 0 {
                        s(m, Reg::Ecx, index);
                        s(m, Reg::Eax, fields);
                        s(m, Reg::Esi, pool);
                    }
                    if index >= m.r32(fields + 4) {
                        return Err("NativeVmArrayObject: absent instance field".into());
                    }
                    let table = m.r32(fields + 8);
                    s(m, Reg::Edx, table);
                    let field = m.r32(table + index * 4);
                    s(m, Reg::Ebx, field);
                    let ti = m.r32(field + 12);
                    s(m, Reg::Eax, ti);
                    stack::push(m, ti);
                    s(m, Reg::Ecx, pool);
                    let ty = stack::call(m, 0x407ec8, &[], Some(pool), |m| {
                        crate::native_vm_lookup::constant(m, pool, ti, 0, 8)
                    })?;
                    stack::add_sp(m, 4);
                    let ni = m.r32(field + 8);
                    s(m, Reg::Ecx, ni);
                    stack::push(m, ni);
                    s(m, Reg::Ecx, pool);
                    s(m, Reg::Ebp, ty);
                    let name = stack::call(m, 0x407ed5, &[], Some(pool), |m| {
                        crate::native_vm_lookup::constant(m, pool, ni, 0, 8)
                    })?;
                    stack::add_sp(m, 4);
                    let h = m.r32(0x62f134);
                    s(m, Reg::Ecx, h);
                    s(m, Reg::Edi, name);
                    let cell = stack::call(m, 0x407ee2, &[], Some(h), |m| {
                        crate::native_vm_values::allocate_variable(m, h, e)
                    })?;
                    s(m, Reg::Esi, cell);
                    s(m, Reg::Edx, class);
                    s(m, Reg::Eax, jvm);
                    stack::push(m, ty);
                    stack::push(m, jvm);
                    s(m, Reg::Ecx, cell);
                    stack::call(m, 0x407efb, &[], Some(cell), |m| {
                        crate::native_vm_values::construct_zero(m, cell, jvm, ty, e)
                    })?;
                    stack::add_sp(m, 8);
                    write(m, cell, 0x5e7344);
                    write(m, cell + 12, name);
                    s(m, Reg::Ebp, cell);
                    s(m, Reg::Ecx, object);
                    s(m, Reg::Ecx, owner);
                    stack::push(m, cell);
                    let inserted = stack::call(m, 0x407f17, &[], Some(owner), |m| {
                        crate::native_vm_metadata::variables::append(m, owner, cell, e)
                    })?;
                    stack::add_sp(m, 4);
                    if inserted & 255 == 0 {
                        return Err(
                            "NativeVmArrayObject: duplicate-field diagnostic continuation required"
                                .into(),
                        );
                    }
                    let variable_vector = m.r32(owner + 12);
                    let issued_index = (m.r32(variable_vector + 8) as usize)
                        .checked_sub(1)
                        .ok_or("NativeVmArrayObject: source append issued no field")?;
                    e.observe_owner(
                        m,
                        crate::native_vm_context::Observation::InstanceField {
                            object,
                            index: issued_index,
                            variable: cell,
                            declaring: current,
                            declared_index: index as usize,
                            record: field,
                        },
                    )?;
                    let initializer = m.r32(field + 16);
                    s(m, Reg::Eax, initializer);
                    if (initializer as i32) >= 0 {
                        initialize_field(m, base, initializer, fpu, e)?;
                    }
                    s(m, Reg::Eax, index);
                    s(m, Reg::Ecx, fields);
                    s(m, Reg::Edx, count);
                    s(m, Reg::Eax, index + 1);
                    write(m, base + 0x18, index + 1);
                }
                if count > 0 {
                    s(m, Reg::Ebx, class);
                    s(m, Reg::Edx, remaining);
                    s(m, Reg::Ebp, object);
                }
            }
            s(m, Reg::Ecx, remaining);
            s(m, Reg::Eax, remaining);
            s(m, Reg::Edx, remaining.wrapping_sub(1));
            s(m, Reg::Ecx, remaining.wrapping_sub(1));
            write(m, base + 0x1c, remaining.wrapping_sub(1));
            write(m, base + 0x24, remaining.wrapping_sub(1));
        }
        s(m, Reg::Ecx, m.r32(class + 8));
        let descriptor = m.r32(r(m, Reg::Ecx));
        s(m, Reg::Edx, descriptor);
        if m.r8(descriptor) == b'[' {
            s(m, Reg::Edi, length);
            if length == 0 || (length as i32) < 0 {
                return Err(
                    "NativeVmArrayObject: nonpositive-length diagnostic continuation required"
                        .into(),
                );
            }
            let selector = m.r32(0x62dee8);
            s(m, Reg::Eax, selector);
            s(m, Reg::Ecx, owner);
            stack::push(m, selector);
            let length_cell = stack::call(m, 0x408213, &[], Some(owner), |m| {
                crate::native_vm_lookup::variable_at(m, owner, selector)
            })?;
            stack::add_sp(m, 4);
            stack::push(m, 12);
            write(m, length_cell + 8, length);
            let storage = allocate(m, 0x40821d, 12, e)?;
            s(m, Reg::Esi, storage);
            s(m, Reg::Ebp, 0);
            stack::add_sp(m, 4);
            for off in [0, 8, 4] {
                write(m, storage + off, 0);
            }
            s(m, Reg::Ecx, length * 4);
            stack::push(m, length * 4);
            stack::push(m, 0);
            let table = stack::external(m, 0x408250, &[], None, |m| {
                e.reallocate(m, 0, 0, length * 4)
            })?;
            s(m, Reg::Eax, table);
            stack::add_sp(m, 8);
            if table == 0 {
                return Err("NativeVmArrayObject: element storage allocation failure".into());
            }
            write(m, storage + 4, table);
            write(m, storage, length);
            s(m, Reg::Ebp, storage);
            let hidden = inline_variable(m, length, false)?;
            let int = m.r32(0x62e00c);
            s(m, Reg::Ecx, int);
            write(m, hidden, 0x5e734c);
            s(m, Reg::Ecx, m.r32(int));
            write(m, hidden + 4, r(m, Reg::Ecx));
            write(m, hidden + 8, 0);
            write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
            write(m, hidden, 0x5e7344);
            write(m, hidden + 12, 0x607a28);
            s(m, Reg::Edx, object);
            write(m, hidden + 8, storage);
            s(m, Reg::Ecx, owner);
            stack::push(m, hidden);
            stack::call(m, 0x408329, &[], Some(owner), |m| {
                crate::native_vm_metadata::variables::append(m, owner, hidden, e)
            })?;
            stack::add_sp(m, 4);
            s(m, Reg::Eax, m.r32(class + 8));
            s(m, Reg::Ecx, descriptor);
            s(m, Reg::Edx, jvm);
            s(m, Reg::Ecx, descriptor + 1);
            stack::push(m, descriptor + 1);
            stack::push(m, jvm);
            let element_type = base + 0x2c;
            s(m, Reg::Ecx, element_type);
            stack::call(m, 0x40833d, &[], Some(element_type), |m| {
                crate::native_vm_signature::descriptor(m, element_type, jvm, descriptor + 1, e)
            })?;
            stack::add_sp(m, 8);
            s(m, Reg::Eax, length);
            s(m, Reg::Edi, length - 1);
            s(m, Reg::Edi, length);
            write(m, base + 0x24, length);
            for remaining in (1..=length).rev() {
                let cell = inline_variable(m, length, true)?;
                write(m, cell, 0x5e734c);
                s(m, Reg::Ecx, m.r32(element_type));
                write(m, cell + 4, r(m, Reg::Ecx));
                write(m, cell + 8, 0);
                write(m, 0x62f138, m.r32(0x62f138).wrapping_add(1));
                write(m, cell, 0x5e7344);
                write(m, cell + 12, 0);
                s(m, Reg::Edi, cell);
                let used = m.r32(storage + 8);
                s(m, Reg::Eax, used + 1);
                s(m, Reg::Ecx, m.r32(storage));
                if used + 1 > m.r32(storage) {
                    return Err("NativeVmArrayObject: unexpected element capacity growth".into());
                }
                s(m, Reg::Edx, used);
                s(m, Reg::Eax, table);
                write(m, table + used * 4, cell);
                s(m, Reg::Ecx, used);
                s(m, Reg::Eax, remaining);
                s(m, Reg::Ecx, used + 1);
                s(m, Reg::Eax, remaining - 1);
                write(m, storage + 8, used + 1);
                write(m, base + 0x24, remaining - 1);
            }
        }
        s(m, Reg::Ecx, thread);
        stack::call(m, 0x408468, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::mark_terminal(m, thread)
        })?;
        s(m, Reg::Ecx, thread);
        stack::call(m, 0x408471, &[], Some(thread), |m| {
            crate::native_vm_thread_lifecycle::destroy(m, thread, e)
        })?;
        e.observe_owner(
            m,
            crate::native_vm_context::Observation::ObjectCommitted { address: object },
        )?;
        Ok(())
    })
}
pub fn create(
    m: &mut Space,
    class: u32,
    length: u32,
    fpu: &mut crate::native_vm_fpu::State,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if !stack::active(m) || !stack::has_pending_call(m) || r(m, Reg::Ecx) != class {
        return Err("NativeVmArrayObject: actual array allocation CALL required".into());
    }
    stack::function(m, &[length], Some(class), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, class);
        stack::call(m, 0x404e69, &[], Some(class), |m| {
            crate::native_vm_class_init::initialize_with_scripts(m, class, fpu, e)
        })?;
        stack::push(m, 28);
        let object = allocate(m, 0x404e70, 28, e)?;
        s(m, Reg::Esi, object);
        s(m, Reg::Eax, 0);
        stack::add_sp(m, 4);
        write(m, object + 8, 0);
        write(m, object + 4, 0);
        s(m, Reg::Eax, length);
        stack::push(m, length);
        stack::push(m, class);
        s(m, Reg::Ecx, object);
        write(m, object, 0x5e7354);
        stack::call(m, 0x404e94, &[], Some(object), |m| {
            construct(m, object, class, length, fpu, e)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Eax, object);
        Ok(object)
    })
}

///404e20 ordinary new-instance entry. Its zero length is an actual PUSH EAX,
/// and its CALL sites differ from404e60; both paths retain the shared407da0
/// constructor and execute the class/instance initializers in source order.
pub fn create_instance(
    m: &mut Space,
    class: u32,
    fpu: &mut crate::native_vm_fpu::State,
    e: &mut impl Environment,
) -> Result<u32, String> {
    if !stack::active(m) || !stack::has_pending_call(m) || r(m, Reg::Ecx) != class {
        return Err("NativeVmArrayObject: actual new-instance CALL required".into());
    }
    stack::function(m, &[], Some(class), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Edi, class);
        stack::call(m, 0x404e29, &[], Some(class), |m| {
            crate::native_vm_class_init::initialize_with_scripts(m, class, fpu, e)
        })?;
        stack::push(m, 28);
        let object = allocate(m, 0x404e30, 28, e)?;
        s(m, Reg::Esi, object);
        s(m, Reg::Eax, 0);
        stack::add_sp(m, 4);
        stack::push_reg(m, Reg::Eax);
        stack::push_reg(m, Reg::Edi);
        s(m, Reg::Ecx, object);
        write(m, object + 8, 0);
        write(m, object + 4, 0);
        write(m, object, 0x5e7354);
        stack::call(m, 0x404e50, &[], Some(object), |m| {
            construct(m, object, class, 0, fpu, e)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Eax, object);
        Ok(object)
    })
}
