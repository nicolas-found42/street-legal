//! Original53a5e0, creating the native body before its new script object.
//! VM allocation, metadata and execution are explicit live collaborators.
use crate::native_instance::{self as old, invoke, prepared, virtual_call, Host};
use crate::native_stack::{self as s, Reg};
use slr_physics::space::Space;

pub fn create(m: &mut Space, args: [u32; 4], host: &mut dyn Host) -> Result<u32, String> {
    s::function_cdecl(m, &args, |m| {
        s::push_reg(m, Reg::Ebp);
        let bp = s::sp(m);
        s::set_reg(m, Reg::Ebp, bp);
        s::alloc(m, 0x3c);
        for r in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            s::push_reg(m, r);
        }
        let [owner, request, parameters, name] = args;
        s::set_reg(m, Reg::Edi, owner);
        s::set_reg(m, Reg::Eax, m.r32(owner + 12));
        m.w32(bp - 0x28, 0);
        s::set_reg(m, Reg::Ebx, request);
        s::set_reg(m, Reg::Ecx, m.r32(request + 8));
        let requested = m.r32(request + 8);
        let template = m.r32(request + 12);
        if requested != 0 {
            s::set_reg(m, Reg::Eax, template);
            if template == 0 || m.r32(template + 0x4c) != 8 {
                let text = invoke(
                    m,
                    host,
                    0x5383f0,
                    0x53a624,
                    &[requested],
                    Some(m.r32(0x618d48)),
                )?;
                s::set_reg(m, Reg::Ecx, requested);
                s::set_reg(m, Reg::Eax, text + 12);
                invoke(
                    m,
                    host,
                    0x551330,
                    0x53a636,
                    &[0x6198dc, text + 12, requested],
                    None,
                )?;
            }
        }
        s::set_reg(m, Reg::Eax, m.r32(owner + 8));
        if m.r32(owner + 8) == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        let id = invoke(m, host, 0x536ed0, 0x53a652, &[], Some(m.r32(0x618d48)))?;
        s::set_reg(m, Reg::Esi, id);
        s::set_reg(m, Reg::Ecx, 0);
        m.w32(bp - 4, id);
        if id == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        s::set_reg(m, Reg::Eax, template);
        for offset in [0x24, 0x20, 0x1c, 0x18] {
            m.w32(bp - offset, 0);
        }
        if template != 0 {
            m.w32(bp + 8, template);
        }
        s::push(m, 0x20);
        for (offset, value) in [(0x3c, 1), (0x38, id), (0x34, 0x21), (0x2c, 0), (0x30, 0)] {
            m.w32(bp - offset, value);
        }
        let buffer = prepared(m, host, 0x54f560, 0x53a6aa, &[0x20])?;
        s::set_reg(m, Reg::Ecx, name);
        s::add_sp(m, 4);
        m.w32(bp - 0x28, buffer);
        s::call(
            m,
            0x53a6c7,
            &[buffer, if name == 0 { 0x619900 } else { name }, 0x1f],
            None,
            |m| old::copy_name(m, buffer, if name == 0 { 0x619900 } else { name }, 0x1f),
        );
        let parent = m.r32(owner + 12);
        let root = m.r32(owner + 8);
        s::set_reg(m, Reg::Eax, parent);
        let raw = if parent != 0 {
            s::set_reg(m, Reg::Ecx, root);
            s::set_reg(m, Reg::Edx, bp - 0x3c);
            invoke(
                m,
                host,
                0x536900,
                0x53a6ea,
                &[id, root, bp - 0x3c, parent, 0, 0],
                Some(m.r32(0x618d48)),
            )?
        } else {
            s::set_reg(m, Reg::Eax, root);
            s::set_reg(m, Reg::Ecx, bp - 0x3c);
            invoke(
                m,
                host,
                0x536890,
                0x53a704,
                &[id, root, bp - 0x3c, 0, 0],
                Some(m.r32(0x618d48)),
            )?
        };
        s::set_reg(m, Reg::Esi, raw);
        let parent = m.r32(raw + 0x14);
        s::set_reg(m, Reg::Edi, parent);
        if parent != 0 {
            s::set_reg(m, Reg::Edx, m.r32(raw));
            virtual_call(m, host, raw, 0x18, 0x53a71b, &[m.r32(parent + 0x70)])?;
            s::set_reg(m, Reg::Eax, m.r32(raw));
            virtual_call(m, host, raw, 0x14, 0x53a729, &[m.r32(parent + 0x6c)])?;
            m.w32(raw + 0x68, m.r32(parent + 0x68));
            if m.r32(parent + 0x64) & 1 == 0 {
                if m.r32(parent + 0x4c) != 1 {
                    s::set_reg(m, Reg::Edx, m.r32(parent));
                    virtual_call(m, host, parent, 0x14, 0x53a747, &[1f32.to_bits()])?;
                }
                if invoke(m, host, 0x5447d0, 0x53a754, &[0, 0, 0], Some(parent))? & 0x80000000 == 0
                {
                    s::set_reg(m, Reg::Eax, m.r32(parent));
                    virtual_call(m, host, parent, 0xc, 0x53a767, &[1f32.to_bits()])?;
                }
            }
        }
        m.w32(0x765f60, m.r32(0x765f60).wrapping_add(1));
        m.w32(raw + 0x54, m.r32(raw + 0x54) | 0x200);
        invoke(m, host, 0x544b90, 0x53a77d, &[1], Some(raw))?;
        m.w32(raw + 0x68, 1f32.to_bits());
        s::set_reg(m, Reg::Edx, m.r32(raw));
        let envelope = virtual_call(m, host, raw, 0xc, 0x53a790, &[1f32.to_bits()])?;
        s::set_reg(m, Reg::Edi, envelope + 0x38);
        m.w32(bp + 8, envelope);
        s::set_reg(m, Reg::Eax, template);
        s::call(m, 0x53a7a1, &[template], Some(envelope + 0x38), |m| {
            old::bind(m, envelope + 0x38, template)
        });
        s::set_reg(m, Reg::Eax, m.r32(request + 8));
        let data = if m.r32(request + 8) != 0 {
            let secondary = m.r32(envelope + 0x44);
            s::set_reg(m, Reg::Ecx, secondary);
            if secondary != 0 {
                let data = invoke(
                    m,
                    host,
                    0x419860,
                    0x53a7d7,
                    &[0x80000000, 1f32.to_bits(), 0, 0],
                    Some(secondary),
                )?;
                s::set_reg(m, Reg::Ebx, data);
                m.w32(bp + 0x14, data);
                data
            } else {
                0
            }
        } else {
            0
        };
        if data == 0 {
            // The secondary local Ref is still empty on these branches.
            s::set_reg(m, Reg::Eax, 0);
            m.w32(bp - 0x1c, 0);
            s::set_reg(m, Reg::Eax, buffer);
            if buffer != 0 {
                invoke(m, host, 0x54f5b0, 0x53a80d, &[buffer], None)?;
            }
            s::set_reg(m, Reg::Eax, raw);
            return Ok(raw);
        }
        s::set_reg(m, Reg::Eax, m.r32(data + 0xc));
        s::set_reg(m, Reg::Edx, 0);
        if m.r32(data + 0xc) == 0 && m.r32(data + 0x10) == 0 {
            return Err("NativeInstanceNewBoundary: original missing-class warning path".into());
        }
        let reference = bp - 0x14;
        for off in [0, 4, 8, 12] {
            m.w32(reference + off, 0);
        }
        if raw != 0 {
            let next = m.r32(raw + 0x48);
            s::set_reg(m, Reg::Ecx, next);
            s::set_reg(m, Reg::Eax, raw + 0x44);
            if next != 0 {
                s::set_reg(m, Reg::Edi, reference);
                m.w32(next, reference);
            }
            m.w32(reference, 0);
            s::set_reg(m, Reg::Ecx, m.r32(raw + 0x48));
            m.w32(reference + 4, m.r32(raw + 0x48));
            s::set_reg(m, Reg::Ecx, reference);
            m.w32(raw + 0x48, reference);
            s::set_reg(m, Reg::Eax, raw);
            m.w32(reference + 12, raw);
            s::set_reg(m, Reg::Eax, m.r32(raw + 0x50));
            m.w32(reference + 8, m.r32(raw + 0x50));
        }
        s::set_reg(m, Reg::Ecx, m.r32(data + 0x18));
        m.w32(raw + 0xbc, m.r32(data + 0x18));
        s::set_reg(m, Reg::Eax, m.r32(data + 0x1c));
        m.w32(raw + 0xb8, m.r32(data + 0x1c));
        let klass = m.r32(data + 0xc);
        s::set_reg(m, Reg::Ecx, klass);
        if klass != 0 {
            s::set_reg(m, Reg::Eax, parameters);
            s::set_reg(m, Reg::Edx, m.r32(klass));
            s::set_reg(m, Reg::Eax, reference);
            let body = virtual_call(m, host, klass, 0xc, 0x53a8c6, &[reference, parameters])?;
            s::set_reg(m, Reg::Ecx, envelope);
            m.w32(envelope + 0x4c, body);
            s::set_reg(m, Reg::Edx, 0);
        }
        if m.r32(data + 0x10) != 0 {
            return Err("NativeInstanceNewBoundary: original nonzero script class needs raw VM metadata/object/thread ownership".into());
        }
        s::set_reg(m, Reg::Eax, reference);
        invoke(m, host, 0x427210, 0x53aa11, &[reference], Some(0x636338))?;
        s::set_reg(m, Reg::Ecx, data);
        let klass = m.r32(data + 0xc);
        s::set_reg(m, Reg::Ecx, klass);
        s::set_reg(m, Reg::Edi, 0);
        if klass != 0 {
            s::set_reg(m, Reg::Eax, envelope);
            let body = m.r32(envelope + 0x4c);
            s::set_reg(m, Reg::Eax, body);
            s::set_reg(m, Reg::Edx, m.r32(klass));
            virtual_call(m, host, klass, 0x40, 0x53aa29, &[body])?;
        }
        invoke(m, host, 0x544b90, 0x53aa32, &[1], Some(raw))?;
        m.w32(raw + 0x68, 1f32.to_bits());
        s::push(m, raw);
        prepared(m, host, 0x4988a0, 0x53aa3f, &[raw])?;
        let mut moved = invoke(m, host, 0x498b10, 0x53aa45, &[raw], None)?;
        s::add_sp(m, 4);
        while moved != 0 {
            moved = invoke(m, host, 0x498b10, 0x53aa52, &[raw], None)?;
        }
        let collision = invoke(m, host, 0x48ae40, 0x53aa64, &[], Some(raw + 0xc0))?;
        if collision != 0 {
            s::set_reg(m, Reg::Ecx, reference);
            invoke(
                m,
                host,
                0x4b3ee0,
                0x53aa74,
                &[reference, 0],
                Some(collision),
            )?;
        }
        release(m, reference);
        release(m, bp - 0x24);
        s::set_reg(m, Reg::Eax, buffer);
        if buffer != 0 {
            invoke(m, host, 0x54f5b0, 0x53aaf3, &[buffer], None)?;
        }
        s::set_reg(m, Reg::Eax, raw);
        Ok(raw)
    })
}
fn release(m: &mut Space, reference: u32) {
    let raw = m.r32(reference + 12);
    s::set_reg(m, Reg::Ecx, raw);
    if raw == 0 {
        m.w32(reference + 8, 0);
        return;
    }
    let previous = m.r32(reference);
    s::set_reg(m, Reg::Eax, previous);
    if previous != 0 {
        s::set_reg(m, Reg::Edx, m.r32(reference + 4));
        m.w32(previous + 4, m.r32(reference + 4));
    } else {
        s::set_reg(m, Reg::Eax, m.r32(reference + 4));
        m.w32(raw + 0x48, m.r32(reference + 4));
    }
    let next = m.r32(reference + 4);
    s::set_reg(m, Reg::Eax, next);
    if next != 0 {
        s::set_reg(m, Reg::Ecx, m.r32(reference));
        m.w32(next, m.r32(reference));
    }
    for off in [12, 8, 0, 4] {
        m.w32(reference + off, 0);
    }
}
