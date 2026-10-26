//! Original53a2a0 caller, with explicit resource-manager and virtual callees.
//!
//! The caller owns its temporary Ref and the constructor's pending CALL. This
//! module must not be replaced by an independent constructor-driver entry.
use crate::native_stack::{self as s, Reg};
use slr_physics::space::Space;

/// Each collaborator receives its actual pending CALL. Implementations either
/// enter a ported function or explicitly declare an external controlled body.
pub trait Host {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        arguments: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String>;
}

pub(crate) fn invoke(
    m: &mut Space,
    host: &mut dyn Host,
    address: u32,
    pc: u32,
    arguments: &[u32],
    this: Option<u32>,
) -> Result<u32, String> {
    let result = s::call(m, pc, arguments, this, |m| {
        host.call(m, address, arguments, this)
    })?;
    s::set_reg(m, Reg::Eax, result);
    Ok(result)
}

pub(crate) fn prepared(
    m: &mut Space,
    host: &mut dyn Host,
    address: u32,
    pc: u32,
    arguments: &[u32],
) -> Result<u32, String> {
    let result = s::call(m, pc, &[], None, |m| host.call(m, address, arguments, None))?;
    s::set_reg(m, Reg::Eax, result);
    Ok(result)
}

pub(crate) fn copy_name(m: &mut Space, destination: u32, source: u32, limit: u32) {
    let args = [destination, source, limit];
    s::function_cdecl(m, &args, |m| {
        s::set_reg(m, Reg::Eax, limit);
        s::set_reg(m, Reg::Ecx, source);
        s::set_reg(m, Reg::Edx, destination);
        s::call(m, 0x551134, &args, None, |m| {
            s::function_cdecl(m, &args, |m| {
                s::set_reg(m, Reg::Eax, destination);
                for r in [Reg::Ebx, Reg::Ebp, Reg::Esi] {
                    s::push_reg(m, r);
                }
                s::set_reg(m, Reg::Esi, source);
                let mut byte = m.r8(source);
                s::set_reg(m, Reg::Ecx, (s::reg(m, Reg::Ecx) & !0xff) | byte as u32);
                s::push_reg(m, Reg::Edi);
                s::set_reg(m, Reg::Edi, 1);
                s::set_reg(m, Reg::Edx, destination);
                if byte != 0 {
                    s::set_reg(m, Reg::Ebx, limit);
                    loop {
                        let count = s::reg(m, Reg::Edi);
                        s::set_reg(m, Reg::Ebp, count);
                        s::set_reg(m, Reg::Edi, count.wrapping_add(1));
                        if count as i32 >= limit as i32 {
                            break;
                        }
                        m.w8(s::reg(m, Reg::Edx), byte);
                        let next = s::reg(m, Reg::Esi).wrapping_add(1);
                        byte = m.r8(next);
                        s::set_reg(m, Reg::Ecx, (s::reg(m, Reg::Ecx) & !0xff) | byte as u32);
                        s::set_reg(m, Reg::Edx, s::reg(m, Reg::Edx).wrapping_add(1));
                        s::set_reg(m, Reg::Esi, next);
                        if byte == 0 {
                            break;
                        }
                    }
                }
                m.w8(s::reg(m, Reg::Edx), 0);
            });
        });
    });
}

//429060 is used here rather than a semantic bind so the caller retains the
//real weak-reference callee's saved registers, return PC and volatile results.
pub(crate) fn bind(m: &mut Space, link: u32, raw: u32) {
    s::function(m, &[raw], Some(link), |m| {
        let old = m.r32(link + 12);
        s::set_reg(m, Reg::Eax, old);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        s::set_reg(m, Reg::Edi, raw);
        if old == raw {
            return;
        }
        s::set_reg(m, Reg::Esi, 0);
        if old != 0 {
            let previous = m.r32(link);
            s::set_reg(m, Reg::Edx, previous);
            if previous != 0 {
                let next = m.r32(link + 4);
                s::set_reg(m, Reg::Eax, next);
                m.w32(previous + 4, next);
            } else {
                let next = m.r32(link + 4);
                s::set_reg(m, Reg::Edx, next);
                m.w32(old + 0x48, next);
            }
            let next = m.r32(link + 4);
            s::set_reg(m, Reg::Eax, next);
            if next != 0 {
                s::set_reg(m, Reg::Edx, m.r32(link));
                m.w32(next, m.r32(link));
            }
            for offset in [12, 8, 0, 4] {
                m.w32(link + offset, 0);
            }
        }
        if raw != 0 {
            let following = m.r32(raw + 0x48);
            s::set_reg(m, Reg::Edx, following);
            s::set_reg(m, Reg::Eax, raw + 0x44);
            if following != 0 {
                m.w32(following, link);
            }
            m.w32(link, 0);
            s::set_reg(m, Reg::Edx, following);
            m.w32(link + 4, following);
            m.w32(raw + 0x48, link);
            s::set_reg(m, Reg::Eax, raw);
            m.w32(link + 12, raw);
            s::set_reg(m, Reg::Eax, m.r32(raw + 0x50));
            m.w32(link + 8, m.r32(raw + 0x50));
        } else {
            for offset in [12, 8, 0, 4] {
                m.w32(link + offset, 0);
            }
        }
    });
}

pub(crate) fn virtual_call(
    m: &mut Space,
    host: &mut dyn Host,
    object: u32,
    offset: u32,
    pc: u32,
    args: &[u32],
) -> Result<u32, String> {
    let vtable = m.r32(object);
    invoke(m, host, m.r32(vtable + offset), pc, args, Some(object))
}

/// Execute the actual caller from its entry. `arguments` are the original
/// owner Ref, template Ref, script, native parameter string and name pointer.
/// No captured frame/register values or constructor answers are supplied.
pub fn create(m: &mut Space, arguments: [u32; 5], host: &mut dyn Host) -> Result<u32, String> {
    s::function_cdecl(m, &arguments, |m| {
        s::push_reg(m, Reg::Ebp);
        let bp = s::sp(m);
        s::set_reg(m, Reg::Ebp, bp);
        s::alloc(m, 0x28);
        for r in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            s::push_reg(m, r);
        }
        let [owner, request, script, parameters, name] = arguments;
        s::set_reg(m, Reg::Edi, owner);
        s::set_reg(m, Reg::Eax, m.r32(owner + 12));
        s::set_reg(m, Reg::Ebx, 0);
        m.w32(bp - 0x14, 0);
        s::set_reg(m, Reg::Esi, request);
        let requested = m.r32(request + 8);
        s::set_reg(m, Reg::Ecx, requested);
        if requested != 0 {
            let template = m.r32(request + 12);
            s::set_reg(m, Reg::Eax, template);
            if template == 0 || m.r32(template + 0x4c) != 8 {
                let manager = m.r32(0x618d48);
                let text = invoke(m, host, 0x5383f0, 0x53a2e2, &[requested], Some(manager))?;
                s::set_reg(m, Reg::Ecx, m.r32(request + 8));
                s::set_reg(m, Reg::Eax, text + 12);
                invoke(
                    m,
                    host,
                    0x551330,
                    0x53a2f4,
                    &[0x6198ac, text + 12, requested],
                    None,
                )?;
            }
        }
        if m.r32(request + 8) == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        let id = invoke(m, host, 0x536ed0, 0x53a307, &[], Some(m.r32(0x618d48)))?;
        s::set_reg(m, Reg::Esi, id);
        if id == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        // PUSH20 precedes the descriptor stores in the original.
        s::push(m, 0x20);
        for (offset, value) in [(0x28, 1), (0x24, id), (0x20, 0x21), (0x18, 0), (0x1c, 0)] {
            m.w32(bp - offset, value);
        }
        let buffer = prepared(m, host, 0x54f560, 0x53a344, &[0x20])?;
        s::set_reg(m, Reg::Ecx, name);
        s::add_sp(m, 4);
        m.w32(bp - 0x14, buffer);
        let source = if name == 0 { 0x6198d0 } else { name };
        s::call(m, 0x53a361, &[buffer, source, 0x1f], None, |m| {
            copy_name(m, buffer, source, 0x1f)
        });
        let parent = m.r32(owner + 12);
        s::set_reg(m, Reg::Eax, parent);
        let root_id = m.r32(owner + 8);
        s::set_reg(m, Reg::Ecx, root_id);
        let raw = if parent != 0 {
            s::set_reg(m, Reg::Edx, bp - 0x28);
            invoke(
                m,
                host,
                0x536900,
                0x53a382,
                &[id, root_id, bp - 0x28, parent, 0, 0],
                Some(m.r32(0x618d48)),
            )?
        } else {
            s::set_reg(m, Reg::Eax, root_id);
            invoke(
                m,
                host,
                0x536890,
                0x53a39a,
                &[id, root_id, bp - 0x28, 0, 0],
                Some(m.r32(0x618d48)),
            )?
        };
        s::set_reg(m, Reg::Esi, raw);
        let inherited = m.r32(raw + 0x14);
        s::set_reg(m, Reg::Edi, inherited);
        if inherited != 0 {
            s::set_reg(m, Reg::Edx, m.r32(raw));
            virtual_call(m, host, raw, 0x18, 0x53a3b1, &[m.r32(inherited + 0x70)])?;
            s::set_reg(m, Reg::Eax, m.r32(raw));
            virtual_call(m, host, raw, 0x14, 0x53a3bf, &[m.r32(inherited + 0x6c)])?;
            m.w32(raw + 0x68, m.r32(inherited + 0x68));
            if m.r32(inherited + 0x64) & 1 == 0 {
                if m.r32(inherited + 0x4c) != 1 {
                    s::set_reg(m, Reg::Edx, m.r32(inherited));
                    virtual_call(m, host, inherited, 0x14, 0x53a3dd, &[1f32.to_bits()])?;
                }
                let result = invoke(m, host, 0x5447d0, 0x53a3e7, &[0, 0, 0], Some(inherited))?;
                if result & 0x80000000 == 0 {
                    s::set_reg(m, Reg::Eax, m.r32(inherited));
                    virtual_call(m, host, inherited, 0xc, 0x53a3fa, &[1f32.to_bits()])?;
                }
            }
        }
        m.w32(0x765f60, m.r32(0x765f60).wrapping_add(1));
        s::set_reg(m, Reg::Eax, m.r32(raw + 0x54) | 0x200);
        m.w32(raw + 0x54, m.r32(raw + 0x54) | 0x200);
        invoke(m, host, 0x544b90, 0x53a414, &[1], Some(raw))?;
        m.w32(raw + 0x68, 1f32.to_bits());
        s::set_reg(m, Reg::Edx, m.r32(raw));
        let envelope = virtual_call(m, host, raw, 0xc, 0x53a427, &[1f32.to_bits()])?;
        s::set_reg(m, Reg::Ebx, envelope);
        s::set_reg(m, Reg::Eax, request);
        s::set_reg(m, Reg::Ecx, m.r32(request + 12));
        s::set_reg(m, Reg::Edi, envelope + 0x38);
        let template = m.r32(request + 12);
        s::call(m, 0x53a43a, &[template], Some(envelope + 0x38), |m| {
            bind(m, envelope + 0x38, template)
        });
        s::set_reg(m, Reg::Edx, request);
        s::set_reg(m, Reg::Eax, m.r32(request + 8));
        let mut free_pc = 0x53a455;
        if m.r32(request + 8) != 0 {
            free_pc = 0x53a4b4;
            let template = m.r32(envelope + 0x44);
            s::set_reg(m, Reg::Edi, template);
            if template != 0 {
                if m.r32(template + 0x4c) != 1 {
                    s::set_reg(m, Reg::Eax, m.r32(template));
                    virtual_call(m, host, template, 0x14, 0x53a47a, &[1f32.to_bits()])?;
                }
                let result = invoke(
                    m,
                    host,
                    0x5447d0,
                    0x53a48a,
                    &[0x80000000, 0, 0],
                    Some(template),
                )?;
                if result & 0x80000000 == 0 {
                    s::set_reg(m, Reg::Edx, m.r32(template));
                    let data = virtual_call(m, host, template, 0xc, 0x53a49d, &[1f32.to_bits()])?;
                    s::set_reg(m, Reg::Edi, data);
                    if data != 0 {
                        finish(m, host, bp, raw, envelope, data, [script, parameters])?;
                        free_pc = 0x53a5c6;
                    }
                }
            }
        }
        let buffer = m.r32(bp - 0x14);
        s::set_reg(m, Reg::Eax, buffer);
        if buffer != 0 {
            invoke(m, host, 0x54f5b0, free_pc, &[buffer], None)?;
        }
        s::set_reg(m, Reg::Eax, raw);
        Ok(raw)
    })
}

fn finish(
    m: &mut Space,
    host: &mut dyn Host,
    bp: u32,
    raw: u32,
    envelope: u32,
    data: u32,
    script_parameters: [u32; 2],
) -> Result<(), String> {
    let [script, parameters] = script_parameters;
    s::set_reg(m, Reg::Eax, 0);
    let reference = bp - 0x10;
    for i in 0..4 {
        m.w32(reference + i * 4, 0);
    }
    if raw != 0 {
        s::set_reg(m, Reg::Eax, reference);
        s::call(m, 0x53a4de, &[reference], Some(raw + 0x44), |m| {
            s::function(m, &[reference], Some(raw + 0x44), |m| {
                let following = m.r32(raw + 0x48);
                s::set_reg(m, Reg::Edx, following);
                s::set_reg(m, Reg::Eax, reference);
                if following != 0 {
                    m.w32(following, reference);
                }
                m.w32(reference, 0);
                m.w32(reference + 4, following);
                m.w32(raw + 0x48, reference);
                s::set_reg(m, Reg::Ecx, raw);
                m.w32(reference + 12, raw);
            });
        });
        s::set_reg(m, Reg::Ecx, m.r32(raw + 0x50));
        m.w32(reference + 8, m.r32(raw + 0x50));
        s::set_reg(m, Reg::Eax, 0);
    }
    s::set_reg(m, Reg::Edx, m.r32(data + 0x18));
    m.w32(raw + 0xbc, m.r32(data + 0x18));
    s::set_reg(m, Reg::Ecx, m.r32(data + 0x1c));
    m.w32(raw + 0xb8, m.r32(data + 0x1c));
    let class = m.r32(data + 0xc);
    s::set_reg(m, Reg::Ecx, class);
    if class != 0 {
        s::set_reg(m, Reg::Eax, parameters);
        s::set_reg(m, Reg::Edx, m.r32(class));
        s::set_reg(m, Reg::Eax, reference);
        let body = virtual_call(m, host, class, 0xc, 0x53a50c, &[reference, parameters])?;
        m.w32(envelope + 0x4c, body);
    }
    s::set_reg(m, Reg::Ecx, script);
    s::set_reg(m, Reg::Edx, reference);
    m.w32(envelope + 0x50, script);
    invoke(m, host, 0x427210, 0x53a523, &[reference], Some(0x636338))?;
    let class = m.r32(data + 0xc);
    s::set_reg(m, Reg::Ecx, class);
    s::set_reg(m, Reg::Edi, 0);
    if class != 0 {
        s::set_reg(m, Reg::Edx, m.r32(envelope + 0x4c));
        s::set_reg(m, Reg::Eax, m.r32(class));
        virtual_call(m, host, class, 0x40, 0x53a535, &[m.r32(envelope + 0x4c)])?;
    }
    invoke(m, host, 0x544b90, 0x53a53e, &[1], Some(raw))?;
    m.w32(raw + 0x68, 1f32.to_bits());
    // cdecl4988a0's first argument remains below498b10 until53a551.
    s::push(m, raw);
    prepared(m, host, 0x4988a0, 0x53a54b, &[raw])?;
    let mut moved = invoke(m, host, 0x498b10, 0x53a551, &[raw], None)?;
    s::add_sp(m, 4);
    while moved != 0 {
        moved = invoke(m, host, 0x498b10, 0x53a55e, &[raw], None)?;
    }
    let collision = invoke(m, host, 0x48ae40, 0x53a570, &[], Some(raw + 0xc0))?;
    if collision != 0 {
        s::set_reg(m, Reg::Ecx, reference);
        invoke(
            m,
            host,
            0x4b3ee0,
            0x53a580,
            &[reference, 0],
            Some(collision),
        )?;
    }
    let retained = m.r32(reference + 12);
    s::set_reg(m, Reg::Ecx, retained);
    if retained != 0 {
        let previous = m.r32(reference);
        s::set_reg(m, Reg::Eax, previous);
        if previous != 0 {
            s::set_reg(m, Reg::Edx, m.r32(reference + 4));
            m.w32(previous + 4, m.r32(reference + 4));
        } else {
            s::set_reg(m, Reg::Eax, m.r32(reference + 4));
            m.w32(retained + 0x48, m.r32(reference + 4));
        }
        let next = m.r32(reference + 4);
        s::set_reg(m, Reg::Eax, next);
        if next != 0 {
            s::set_reg(m, Reg::Ecx, m.r32(reference));
            m.w32(next, m.r32(reference));
        }
        for offset in [12, 8, 0, 4] {
            m.w32(reference + offset, 0);
        }
    } else {
        m.w32(reference + 8, 0);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    const REGS: [(&str, Reg); 7] = [
        ("eax", Reg::Eax),
        ("ebx", Reg::Ebx),
        ("ecx", Reg::Ecx),
        ("edx", Reg::Edx),
        ("esi", Reg::Esi),
        ("edi", Reg::Edi),
        ("ebp", Reg::Ebp),
    ];
    fn word(v: &Value) -> u32 {
        v.as_u64().unwrap().try_into().unwrap()
    }
    fn hex(v: &Value) -> Vec<u8> {
        let v = v.as_str().unwrap();
        assert_eq!(v.len() % 2, 0);
        (0..v.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&v[i..i + 2], 16).unwrap())
            .collect()
    }
    fn words(m: &mut Space, base: u32, values: &Value) {
        for (i, v) in values.as_array().unwrap().iter().enumerate() {
            m.w32(base + i as u32 * 4, word(v));
        }
    }
    pub(crate) fn check(m: &Space, expected: &Value, layout: &Value, label: &str) {
        for (field, base) in [
            ("heap", word(&layout["heap_address"])),
            ("stack", word(&layout["stack_address"])),
        ] {
            for (i, v) in expected[field].as_array().unwrap().iter().enumerate() {
                let address = base + i as u32 * 4;
                assert_eq!(m.r32(address), word(v), "{label} {field} {address:#x}");
            }
        }
        for (address, v) in expected["globals"].as_object().unwrap() {
            let address = address.parse::<u32>().unwrap();
            assert_eq!(m.r32(address), word(v), "{label} global{address:#x}");
        }
        for (name, r) in REGS {
            assert_eq!(
                s::reg(m, r),
                word(&expected["registers"][name]),
                "{label} {name}"
            );
        }
        assert_eq!(s::sp(m), word(&expected["registers"]["esp"]), "{label} ESP");
    }
    pub(crate) struct Controlled<'a> {
        pub(crate) row: &'a Value,
        pub(crate) next: usize,
    }
    impl Controlled<'_> {
        pub(crate) fn record(&mut self, m: &Space, address: u32, args: &[u32]) {
            let event = &self.row["events"][self.next];
            assert_eq!(
                word(&event["address"]),
                address,
                "callback{} {:?}",
                self.next,
                self.row["case"]
            );
            let expected: Vec<_> = event["args"].as_array().unwrap().iter().map(word).collect();
            assert_eq!(args, expected, "callback{} arguments", self.next);
            assert_eq!(
                m.r32(s::sp(m)),
                word(&event["return_pc"]),
                "callback{} returnPC",
                self.next
            );
            check(
                m,
                event,
                &self.row["layout"],
                &format!("callback{} {address:#x}", self.next),
            );
            self.next += 1;
        }
        fn value(&self, name: &str) -> u32 {
            word(&self.row["layout"][name])
        }
        fn text(m: &Space, p: u32) -> String {
            let mut bytes = vec![];
            for i in 0..4096 {
                let b = m.r8(p + i);
                if b == 0 {
                    return String::from_utf8(bytes).unwrap();
                }
                bytes.push(b);
            }
            panic!("unterminated fixture CString")
        }
        fn bound(&self, m: &mut Space, link: u32, id: u32) {
            let node = self.value("node");
            let raw = std::iter::once(node)
                .chain(
                    self.row["layout"]["resources"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(word),
                )
                .find(|r| m.r32(*r + 0x50) == id)
                .unwrap_or(0);
            crate::native_part::bind_reference(m, link, raw);
        }
    }
    impl Host for Controlled<'_> {
        fn call(
            &mut self,
            m: &mut Space,
            address: u32,
            args: &[u32],
            this: Option<u32>,
        ) -> Result<u32, String> {
            if address == 0x4797d0 {
                let parameters = Self::text(m, args[1]);
                return crate::native_player::construct(
                    m,
                    this.unwrap(),
                    args[0],
                    &parameters,
                    self,
                );
            }
            if address == 0x45fdf0 {
                return crate::native_cursor::construct(m, this.unwrap(), args[0], self);
            }
            s::function(m, args, this, |m| {
                self.record(m, address, args);
                let mode = self.row["case"]["mode"].as_str().unwrap();
                Ok(match address {
                    0x536ed0 => {
                        if mode == "allocation-zero" {
                            0
                        } else {
                            0xffff007d
                        }
                    }
                    0x54f560 => self.value("name_allocation"),
                    0x54f5b0 | 0x544b90 | 0x300f0060 | 0x4988a0 | 0x4b3ee0 => 0,
                    0x498b10 => u32::from(
                        mode == "relocate-once" && matches!(m.r32(s::sp(m)), 0x53a551 | 0x53aa45),
                    ),
                    0x48ae40 => {
                        if mode == "collision-present" {
                            word(&self.row["layout"]["resources"][3])
                        } else {
                            0
                        }
                    }
                    0x536890 | 0x536900 => self.value("node"),
                    0x5447d0 => {
                        if this == Some(self.value("template")) && mode == "template-pending" {
                            0x80000001
                        } else {
                            0
                        }
                    }
                    0x300f0000 => self.value("node") + 0xd8,
                    0x419860 => {
                        if mode == "template-pending" || mode == "template-no-payload" {
                            0
                        } else {
                            self.value("template_payload")
                        }
                    }
                    0x300f0010 => {
                        if mode == "template-no-payload" {
                            0
                        } else {
                            self.value("template_payload")
                        }
                    }
                    0x300f0020 => self.value("parent") + 0xd8,
                    0x300f0030 => {
                        m.w32(this.unwrap() + 0x6c, args[0]);
                        s::reg(m, Reg::Eax)
                    }
                    0x300f0040 => {
                        m.w32(this.unwrap() + 0x70, args[0]);
                        s::reg(m, Reg::Eax)
                    }
                    0x300f0050 => {
                        if mode == "constructor-null" {
                            0
                        } else {
                            self.value("body")
                        }
                    }
                    0x427210 => {
                        if mode == "registration-replaces-body" {
                            m.w32(self.value("node") + 0x124, self.value("body") + 4);
                        }
                        if mode == "registration-clears-class" {
                            m.w32(self.value("template_payload") + 12, 0);
                        }
                        if mode == "registration-rebinds-reference" {
                            crate::native_part::bind_reference(
                                m,
                                args[0],
                                word(&self.row["layout"]["resources"][0]),
                            );
                        }
                        if mode == "registration-retires-reference" {
                            crate::native_part::bind_reference(m, args[0], 0);
                        }
                        0
                    }
                    _ => {
                        return Err(format!(
                            "undeclared source-caller fixture collaborator{address:#x}"
                        ))
                    }
                })
            })
        }
    }
    impl crate::native_player::Host for Controlled<'_> {
        fn allocate(&mut self, m: &mut Space, _bytes: u32) -> Result<u32, String> {
            self.record(m, 0x54f560, &[0x2510]);
            Ok(self.value("body"))
        }
        fn name(&mut self, _m: &mut Space, _id: u32) -> Result<u32, String> {
            unreachable!("template setup is before measured caller")
        }
        fn lookup(&mut self, m: &mut Space, name: u32, entry: u32) -> Result<u32, String> {
            self.record(m, 0x538440, &[name, entry]);
            Ok(match name {
                0x612624 => 0xffff2001,
                0x612630 => 0xffff2002,
                _ => 0xffff2003,
            })
        }
        fn bind(
            &mut self,
            m: &mut Space,
            reference: u32,
            id: u32,
            kind: u32,
            flags: u32,
        ) -> Result<(), String> {
            if m.r32(s::sp(m)) == 0x479a52 {
                self.record(m, 0x545fc0, &[id]);
            } else {
                self.record(m, 0x546070, &[id, kind, flags]);
            }
            self.bound(m, reference, id);
            s::set_reg(m, Reg::Eax, 0);
            Ok(())
        }
        fn register(
            &mut self,
            m: &mut Space,
            c: u32,
            n: u32,
            signature: u32,
            address: u32,
        ) -> Result<(), String> {
            self.record(m, 0x416b00, &[c, n, signature, address]);
            s::set_reg(m, Reg::Eax, 0);
            Ok(())
        }
        fn integer(&mut self, m: &mut Space, key: u32) -> Result<u32, String> {
            self.record(m, 0x426170, &[key]);
            Ok(0)
        }
        fn float(&mut self, m: &mut Space, key: u32) -> Result<u32, String> {
            self.record(m, 0x4261f0, &[key]);
            Ok(0.5f32.to_bits())
        }
        fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String> {
            self.record(m, 0x48d3d0, &[reference, mask, 0]);
            let raw = m.r32(reference + 12);
            m.w32(raw + 0x148, m.r32(raw + 0x148) | mask);
            s::set_reg(m, Reg::Eax, 0);
            Ok(())
        }
    }
    impl crate::native_cursor::Host for Controlled<'_> {
        fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
            self.record(m, 0x54f560, &[bytes]);
            Ok(self.value("body"))
        }
        fn sensitivity(&mut self, m: &mut Space) -> Result<u32, String> {
            self.record(m, 0x4261f0, &[0x610a84]);
            Ok(0.5f32.to_bits())
        }
        fn system_cursor(&mut self, m: &mut Space) -> Result<Option<u32>, String> {
            self.record(m, 0x426100, &[0x610a9c, 0]);
            s::set_reg(m, Reg::Eax, 1);
            Ok(Some(0))
        }
        fn system_cursor_integer(&mut self, m: &mut Space, _value: u32) -> Result<u32, String> {
            self.record(m, 0x426170, &[0x610aa8]);
            s::set_reg(m, Reg::Eax, 0);
            Ok(0)
        }
        fn lookup(&mut self, m: &mut Space, _name: &str, entry: u32) -> Result<u32, String> {
            let name = if m.r32(s::sp(m)) == 0x45ff0b {
                0x610a64
            } else {
                0x610a74
            };
            self.record(m, 0x538440, &[name, entry]);
            s::set_reg(m, Reg::Eax, 0xffff2003);
            Ok(0xffff2003)
        }
        fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String> {
            crate::native_player::Host::subscribe(self, m, reference, mask)
        }
        fn resolve(&mut self, m: &mut Space, id: u32) -> Result<u32, String> {
            self.record(m, 0x545fc0, &[id]);
            s::set_reg(m, Reg::Eax, 0);
            Ok(self.row["layout"]["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(word)
                .find(|r| m.r32(*r + 0x50) == id)
                .unwrap_or(0))
        }
        fn resolve_kind(&mut self, m: &mut Space, id: u32, kind: u32) -> Result<u32, String> {
            self.record(m, 0x536820, &[id, kind, 0]);
            Ok(word(&self.row["layout"]["resources"][2]))
        }
    }
    #[test]
    fn original_full_caller_and_composed_constructors() {
        compare_fixture(false);
    }
    #[test]
    fn original_new_native_only_caller_and_composed_constructors() {
        compare_fixture(true);
    }
    fn compare_fixture(new: bool) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(bytes) = std::fs::read(root.join(if new {
            "re/out/boot/native-instance-new-original.json"
        } else {
            "re/out/boot/native-instance-original.json"
        })) else {
            assert!(
                std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
                "required native_instance_probe.py fixture absent"
            );
            eprintln!("SLR_ORACLE_SKIPPED: native_instance_probe.py");
            return;
        };
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        for (field, path) in [
            (
                "producer_sha256",
                if new {
                    "re/oracle/native_instance_new_probe.py"
                } else {
                    "re/oracle/native_instance_probe.py"
                },
            ),
            ("oracle_sha256", "re/oracle/oracle.py"),
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        assert_eq!(data["schema"], 1);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), if new { 70 } else { 74 });
        assert_eq!(
            data["inventory"].as_array().unwrap().len(),
            if new { 70 } else { 74 }
        );
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["case"], data["inventory"][index]);
            let mut m = Space::new();
            for id in row["input_page_ids"].as_array().unwrap() {
                let page = &data["input_pages"][id.as_str().unwrap()];
                let bytes = hex(&page["bytes"]);
                assert_eq!(bytes.len(), 4096);
                assert_eq!(page["sha256"], format!("{:x}", Sha256::digest(&bytes)));
                for (i, b) in bytes.into_iter().enumerate() {
                    m.w8(word(&page["address"]) + i as u32, b);
                }
            }
            for (field, base) in [
                ("heap", word(&row["layout"]["heap_address"])),
                ("stack", word(&row["layout"]["stack_address"])),
            ] {
                words(&mut m, base, &row["before"][field]);
            }
            for (address, v) in row["before"]["globals"].as_object().unwrap() {
                m.w32(address.parse().unwrap(), word(v));
            }
            let args: Vec<u32> = row["args"].as_array().unwrap().iter().map(word).collect();
            let entry = &row["contexts"][0]["registers"];
            let mut host = Controlled { row, next: 0 };
            s::function(&mut m, &[], None, |m| {
                for (name, r) in REGS {
                    s::set_reg(m, r, word(&entry[name]));
                }
                s::set_sp(m, word(&entry["esp"]) + (args.len() as u32 + 1) * 4);
                s::call(m, 0x7fff0000, &args, None, |m| {
                    let result = if new {
                        crate::native_instance_new::create(
                            m,
                            args.clone().try_into().unwrap(),
                            &mut host,
                        )
                    } else {
                        create(m, args.clone().try_into().unwrap(), &mut host)
                    }
                    .unwrap_or_else(|e| panic!("case{index} {:?}: {e}", row["case"]));
                    assert_eq!(result, word(&row["result"]), "case{index} result");
                    check(
                        m,
                        &row["after"],
                        &row["layout"],
                        &format!("case{index} endpoint"),
                    );
                });
            });
            assert_eq!(
                host.next,
                row["events"].as_array().unwrap().len(),
                "case{index} full callback inventory"
            );
        }
    }
}
