//! Per-resource frame dispatch. Collaborators retain original access order;
//! registered native bodies are never silently replaced with script calls.
use slr_physics::space::Space;

pub trait Host {
    fn minimum(&mut self, m: &mut Space, resource: u32) -> Result<(), String>;
    fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String>;
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    fn native_drive(&mut self, m: &mut Space, class: u32, body: u32, dt: u32)
        -> Result<(), String>;
    fn script_drive(&mut self, m: &mut Space, envelope: u32) -> Result<(), String>;
}

/// exe: FUN_00426e10
pub fn drive(m: &mut Space, reference: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    stack::function(m, &[dt, reference], None, |m| {
        stack::alloc(m, 0x10);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        let local = stack::sp(m) + 12;
        for i in 0..4 {
            m.w32(local + i * 4, 0);
        }
        crate::native_part::bind_reference(m, local, m.r32(reference + 12));
        let resource = m.r32(local + 12);
        let flags = if resource == 0 {
            0
        } else {
            m.r32(resource + 0x54)
        };
        stack::store(m, 0x24, flags);
        if resource != 0 {
            m.w32(resource + 0x54, flags & !0x100);
        }
        let result = dispatch(m, reference, dt, host);
        // The retained local reference follows resource destruction during a
        // callback. Restore through that live link, not a stale resource word.
        let retained = m.r32(local + 12);
        if retained != 0 {
            m.w32(retained + 0x54, m.r32(retained + 0x54) | flags);
        }
        crate::native_part::bind_reference(m, local, 0);
        result
    })
}

fn dispatch(m: &mut Space, reference: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    let resource = m.r32(reference + 12);
    if resource == 0 {
        return Ok(());
    }
    if m.r32(resource + 0x4c) != 1 {
        host.minimum(m, resource)?;
    }
    if host.prepare(m, resource, 0xa0000000)? & 0x80000000 != 0 {
        return Ok(());
    }
    let envelope = host.payload(m, resource)?;
    if envelope == 0 {
        return Ok(());
    }
    let body = m.r32(envelope + 0x4c);
    if body == 0 {
        if m.r32(envelope + 0x50) != 0 {
            host.script_drive(m, envelope)?;
        }
        return Ok(());
    }
    let template = m.r32(envelope + 0x44);
    if template == 0 {
        return Ok(());
    }
    if m.r32(template + 0x4c) != 1 {
        host.minimum(m, template)?;
    }
    if host.prepare(m, template, 0x80000000)? & 0x80000000 != 0 {
        return Ok(());
    }
    let template_data = host.payload(m, template)?;
    if template_data == 0 {
        return Ok(());
    }
    let class = m.r32(template_data + 12);
    if class != 0 {
        // Template access can execute callbacks that replace the live body.
        //426eee reloads the envelope after those calls rather than using the
        // value that selected the native branch at426e9d.
        host.native_drive(m, class, m.r32(envelope + 0x4c), dt)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Control,
    Animate,
}

/// Control/animation virtuals are distinct from drive. A nonzero native body
/// always goes through its native class; script callbacks are only the original
/// zero-body/nonzero-script branch. Calls arrive with the original x86 frame.
/// `native_callback` must consume the pending CALL with its native function's
/// `native_stack::function`; script_callback_entry consumes the wrapper CALL.
pub trait CallbackHost: Host {
    fn native_callback(
        &mut self,
        m: &mut Space,
        class: u32,
        body: u32,
        dt: u32,
        kind: Callback,
    ) -> Result<(), String>;
    /// Consume the actual5447d0 CALL; source backends can retain its FP state.
    fn prepare_callback(
        &mut self,
        m: &mut Space,
        resource: u32,
        flags: u32,
    ) -> Result<u32, String> {
        crate::native_stack::function(m, &[flags, 0, 0], Some(resource), |m| {
            self.prepare(m, resource, flags)
        })
    }
    /// The425a90 wrapper CALL is still pending at this seam. A source VM owns
    /// its wrapper/Thread.run frames; the existing bridge enters its external body.
    fn script_callback_entry(
        &mut self,
        m: &mut Space,
        envelope: u32,
        dt: u32,
        kind: Callback,
    ) -> Result<(), String> {
        let count = if kind == Callback::Control { 7 } else { 4 };
        let args = (0..count)
            .map(|i| crate::native_stack::argument(m, i))
            .collect::<Vec<_>>();
        crate::native_stack::function_cdecl(m, &args, |m| {
            self.script_callback(m, envelope, dt, kind)
        })
    }
    fn script_callback(
        &mut self,
        m: &mut Space,
        envelope: u32,
        dt: u32,
        kind: Callback,
    ) -> Result<(), String>;
}

fn reference_call(m: &mut Space, pc: u32, link: u32, target: u32) {
    use crate::native_stack::{self as s, Reg};
    s::call(m, pc, &[target], Some(link), |m| {
        s::function(m, &[target], Some(link), |m| {
            s::push_reg(m, Reg::Esi);
            s::push_reg(m, Reg::Edi);
            crate::native_part::bind_reference(m, link, target);
        });
    });
}

fn flag_call(m: &mut Space, pc: u32, link: u32, bits: u32, clear: bool) -> u32 {
    use crate::native_stack::{self as s, Reg};
    s::call(m, pc, &[bits], Some(link), |m| {
        s::function(m, &[bits], Some(link), |m| {
            let raw = m.r32(link + 12);
            s::set_reg(m, Reg::Ecx, raw);
            if raw == 0 {
                s::set_reg(m, Reg::Eax, 0);
                0
            } else {
                let flags = m.r32(raw + 0x54);
                let value = if clear { flags & !bits } else { flags | bits };
                m.w32(raw + 0x54, value);
                s::set_reg(m, Reg::Eax, value);
                s::set_reg(m, Reg::Edx, value);
                value
            }
        })
    })
}

/// Original419860's wrapper, including the actual nested CALL sites. The
/// minimum/prepare/payload bodies stay declared live collaborators.
fn access_frame(
    m: &mut Space,
    raw: u32,
    flags: u32,
    pc: u32,
    host: &mut dyn CallbackHost,
) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    let args = [flags, 0x3f800000, 0, 0];
    s::call(m, pc, &args, Some(raw), |m| {
        s::function(m, &args, Some(raw), |m| {
            s::push_reg(m, Reg::Esi);
            s::set_reg(m, Reg::Esi, raw);
            if m.r32(raw + 0x4c) != 1 {
                s::set_reg(m, Reg::Eax, m.r32(raw));
                s::external(m, 0x419875, &[0x3f800000], Some(raw), |m| {
                    host.minimum(m, raw)
                })?;
            }
            s::set_reg(m, Reg::Eax, 0);
            s::set_reg(m, Reg::Edx, 0);
            let pending = s::external(m, 0x41988b, &[flags, 0, 0], Some(raw), |m| {
                host.prepare(m, raw, flags)
            })?;
            if pending & 0x80000000 != 0 {
                s::set_reg(m, Reg::Eax, 0);
                Ok(0)
            } else {
                s::set_reg(m, Reg::Eax, pending);
                s::set_reg(m, Reg::Edx, m.r32(raw));
                let payload = s::external(m, 0x41989e, &[0x3f800000], Some(raw), |m| {
                    host.payload(m, raw)
                })?;
                s::set_reg(m, Reg::Eax, payload);
                Ok(payload)
            }
        })
    })
}

fn retire_callback(m: &mut Space, node: u32) {
    crate::native_part::bind_reference(m, node + 12, 0);
    m.w32(node, 0x5f09ac);
    let previous = m.r32(node + 8);
    let next = m.r32(node + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}

fn callback_payload(
    m: &mut Space,
    envelope: u32,
    dt: u32,
    kind: Callback,
    host: &mut dyn CallbackHost,
) -> Result<(), String> {
    use crate::native_stack::{self as s, Reg};
    if envelope == 0 {
        return Ok(());
    }
    if m.r32(envelope + 0x4c) != 0 {
        let template = m.r32(envelope + 0x44);
        if template == 0 {
            return Ok(());
        }
        let pc = if kind == Callback::Control {
            0x426d69
        } else {
            0x4270df
        };
        let data = access_frame(m, template, 0x80000000, pc, host)?;
        if data != 0 {
            let class = m.r32(data + 12);
            if class != 0 {
                // Access may replace the body that selected this branch.
                let body = m.r32(envelope + 0x4c);
                s::set_reg(m, Reg::Eax, class);
                s::set_reg(m, Reg::Edx, m.r32(class));
                let pc = if kind == Callback::Control {
                    0x426d83
                } else {
                    0x4270f9
                };
                s::call(m, pc, &[body, dt], Some(class), |m| {
                    host.native_callback(m, class, body, dt, kind)
                })?;
            }
        }
    } else if m.r32(envelope + 0x50) != 0 {
        if kind == Callback::Animate {
            s::set_reg(m, Reg::Eax, m.r32(envelope + 0x50));
        }
        let (pc, args) = if kind == Callback::Control {
            let wide = (f32::from_bits(dt) as f64).to_bits();
            (
                0x426da3,
                vec![
                    envelope,
                    0x60cf38,
                    0,
                    3,
                    wide as u32,
                    (wide >> 32) as u32,
                    0x4f,
                ],
            )
        } else {
            (0x427111, vec![envelope, 0x60cae4, 0, 0x4f])
        };
        s::call(m, pc, &args, None, |m| {
            host.script_callback_entry(m, envelope, dt, kind)
        })?;
    }
    Ok(())
}

/// Original426c90, the complete kind8 list pass. It deliberately has no
/// resource-type eligibility test: non-type1 registrations use419860 too.
/// Returned nodes are the original allocator's deferred free observations.
/// exe: FUN_00426c90
pub fn control(
    m: &mut Space,
    context: u32,
    dt: u32,
    host: &mut dyn CallbackHost,
) -> Result<Vec<u32>, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[dt], Some(context), |m| {
        s::push_reg(m, Reg::Ebp);
        s::set_reg(m, Reg::Ebp, s::sp(m));
        s::set_sp(m, s::sp(m) & !7);
        s::alloc(m, 0x1c);
        s::push_reg(m, Reg::Ebx);
        s::push_reg(m, Reg::Esi);
        s::set_reg(m, Reg::Esi, context);
        s::set_reg(m, Reg::Ebx, 0);
        s::push_reg(m, Reg::Edi);
        s::store(m, 0x14, context);
        let local = s::sp(m) + 0x18;
        let mut retired = vec![];
        let mut node = crate::native_install::first(m, context);
        while node != 0 {
            let next = crate::native_install::next(m, node);
            s::store(m, 0x10, next);
            if m.r32(node + 20) == 0 {
                retire_callback(m, node);
                retired.push(node);
                m.w32(context + 0xc4, m.r32(context + 0xc4).wrapping_sub(1));
            } else {
                s::set_reg(m, Reg::Esi, node + 12);
                s::call(m, 0x426d07, &[], Some(local), |m| {
                    s::function(m, &[], Some(local), |m| {
                        for i in 0..4 {
                            m.w32(local + i * 4, 0);
                        }
                    });
                });
                reference_call(m, 0x426d14, local, m.r32(node + 24));
                let flags = flag_call(m, 0x426d1e, local, 0, false);
                s::set_reg(m, Reg::Edi, flags);
                flag_call(m, 0x426d2e, local, 0x100, true);
                let resource = m.r32(node + 24);
                let outcome = if resource == 0 {
                    Ok(())
                } else {
                    access_frame(m, resource, 0xa0000000, 0x426d46, host).and_then(|envelope| {
                        s::set_reg(m, Reg::Esi, envelope);
                        callback_payload(m, envelope, dt, Callback::Control, host)
                    })
                };
                flag_call(m, 0x426db0, local, flags, false);
                crate::native_part::bind_reference(m, local, 0);
                outcome?;
                s::set_reg(m, Reg::Esi, context);
            }
            node = next;
        }
        Ok(retired)
    })
}

/// Original426fd0. Unlike control, animation accepts only type1 resources.
/// Its retained weak reference lives across iterations and is released once,
/// after the pass; this changes callback-visible intrusive-link order.
/// exe: FUN_00426fd0
pub fn animate(
    m: &mut Space,
    context: u32,
    dt: u32,
    host: &mut dyn CallbackHost,
) -> Result<Vec<u32>, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[dt], Some(context), |m| {
        s::push_reg(m, Reg::Ebp);
        s::set_reg(m, Reg::Ebp, s::sp(m));
        let bp = s::sp(m);
        s::alloc(m, 0x14);
        s::push_reg(m, Reg::Edi);
        s::set_reg(m, Reg::Edi, context);
        let local = bp - 0x14;
        for i in 0..4 {
            m.w32(local + i * 4, 0);
        }
        m.w32(bp - 4, context);
        s::push_reg(m, Reg::Ebx);
        s::push_reg(m, Reg::Esi);
        let mut retired = vec![];
        let mut node = crate::native_install::first(m, context + 0x38);
        let mut outcome = Ok(());
        while node != 0 {
            let next = crate::native_install::next(m, node);
            s::set_reg(m, Reg::Ebx, next);
            if m.r32(node + 20) == 0 {
                retire_callback(m, node);
                retired.push(node);
                m.w32(context + 0xcc, m.r32(context + 0xcc).wrapping_sub(1));
            } else {
                let resource = m.r32(node + 24);
                s::set_reg(m, Reg::Esi, node + 12);
                if resource != 0 && m.r32(resource + 0x4c) == 1 {
                    reference_call(m, 0x42705f, local, resource);
                    let flags = flag_call(m, 0x427069, local, 0, false);
                    s::set_reg(m, Reg::Edi, flags);
                    flag_call(m, 0x427078, local, 0x100, true);
                    let resource = m.r32(node + 24);
                    s::set_reg(m, Reg::Esi, resource);
                    outcome = (|| {
                        if resource == 0 {
                            return Ok(());
                        }
                        if m.r32(resource + 0x4c) != 1 {
                            s::set_reg(m, Reg::Edx, m.r32(resource));
                            s::external(m, 0x427095, &[0x3f800000], Some(resource), |m| {
                                host.minimum(m, resource)
                            })?;
                        }
                        let pending =
                            s::call(m, 0x4270a5, &[0xa0000000, 0, 0], Some(resource), |m| {
                                host.prepare_callback(m, resource, 0xa0000000)
                            })?;
                        s::set_reg(m, Reg::Eax, pending);
                        if pending & 0x80000000 != 0 {
                            return Ok(());
                        }
                        s::set_reg(m, Reg::Eax, m.r32(resource));
                        let envelope =
                            s::external(m, 0x4270b8, &[0x3f800000], Some(resource), |m| {
                                host.payload(m, resource)
                            })?;
                        s::set_reg(m, Reg::Eax, envelope);
                        s::set_reg(m, Reg::Esi, envelope);
                        callback_payload(m, envelope, dt, Callback::Animate, host)
                    })();
                    flag_call(m, 0x427120, local, flags, false);
                    s::set_reg(m, Reg::Edi, context);
                    if outcome.is_err() {
                        break;
                    }
                }
            }
            node = next;
        }
        //427123..427159 unlink the stack reference in place. The native
        // destructor retains its local words; bind_reference would erase them.
        s::set_reg(m, Reg::Ecx, 0);
        s::set_reg(m, Reg::Edx, 0);
        let retained = m.r32(bp - 8);
        s::set_reg(m, Reg::Ecx, retained);
        if retained != 0 {
            let previous = m.r32(local);
            s::set_reg(m, Reg::Eax, previous);
            if previous != 0 {
                s::set_reg(m, Reg::Ecx, m.r32(local + 4));
                m.w32(previous + 4, s::reg(m, Reg::Ecx));
            } else {
                s::set_reg(m, Reg::Eax, m.r32(local + 4));
                m.w32(retained + 0x48, s::reg(m, Reg::Eax));
            }
            let next = m.r32(local + 4);
            s::set_reg(m, Reg::Eax, next);
            if next != 0 {
                s::set_reg(m, Reg::Ecx, m.r32(local));
                m.w32(next, s::reg(m, Reg::Ecx));
            }
        }
        outcome.map(|()| retired)
    })
}

#[cfg(test)]
mod callback_tests {
    use super::*;
    use crate::native_stack::{self as stack, Reg};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    const MODES: &[&str] = &[
        "empty-list",
        "retired",
        "unbound",
        "pending",
        "no-payload",
        "empty",
        "script",
        "native-no-template",
        "template-pending",
        "template-no-payload",
        "template-no-class",
        "native",
        "template-mutates-body",
        "native-flags",
        "native-destroy-current",
        "native-retire-next",
        "native-rebind-local",
        "native-retire-self",
    ];

    fn word(value: &Value) -> u32 {
        value.as_u64().unwrap() as u32
    }

    fn validate(row: &Value) -> Result<(), &'static str> {
        let words = if row["mode"] == "empty-list" {
            0x7b0 / 4
        } else {
            0x80c / 4
        };
        for name in ["before", "after"] {
            if row[name].as_array().map(Vec::len) != Some(words) {
                return Err("incomplete caller heap");
            }
        }
        for event in row["events"].as_array().ok_or("missing events")? {
            if event["heap"].as_array().map(Vec::len) != Some(words)
                || event["esp"].as_u64().is_none()
                || event["return_pc"].as_u64().is_none()
            {
                return Err("incomplete callback heap or CALL identity");
            }
        }
        if row["fpu_cw"] != 0x7f || row["fpu_depth"] != 0 || row["esp_delta"] != 4 {
            return Err("unexpected original ABI/FPU state");
        }
        Ok(())
    }

    struct Collaborators<'a> {
        row: &'a Value,
        events: Vec<Value>,
        mutated: bool,
    }
    impl Collaborators<'_> {
        fn at(&self, name: &str) -> u32 {
            word(&self.row[name])
        }
        fn resource(&self, index: usize) -> u32 {
            word(&self.row["resources"][index])
        }
        fn record(&mut self, m: &Space, name: &str, args: &[u32]) {
            self.events.push(
                json!({"name":name,"this":stack::reg(m,Reg::Ecx),"args":args,
                "esp":stack::sp(m),"return_pc":m.r32(stack::sp(m)),
                "registers": {"eax":stack::reg(m,Reg::Eax),"ebx":stack::reg(m,Reg::Ebx),
                    "ecx":stack::reg(m,Reg::Ecx),"edx":stack::reg(m,Reg::Edx),
                    "esi":stack::reg(m,Reg::Esi),"edi":stack::reg(m,Reg::Edi),"ebp":stack::reg(m,Reg::Ebp)},
                "heap":(0..self.row["before"].as_array().unwrap().len())
                    .map(|i|m.r32(0x20000000+i as u32*4)).collect::<Vec<_>>()}),
            );
        }
    }
    impl Host for Collaborators<'_> {
        fn minimum(&mut self, m: &mut Space, _: u32) -> Result<(), String> {
            self.record(m, "minimum", &[0x3f800000]);
            Ok(())
        }
        fn prepare(&mut self, m: &mut Space, raw: u32, flags: u32) -> Result<u32, String> {
            self.record(m, "prepare", &[flags, 0, 0]);
            if self.row["mode"] == "template-mutates-body" && raw == self.at("template") {
                m.w32(self.at("envelope") + 0x4c, 0x87654321);
            }
            Ok(
                if self.row["mode"] == "pending"
                    || (self.row["mode"] == "template-pending" && raw == self.at("template"))
                {
                    0x80000001
                } else {
                    0
                },
            )
        }
        fn payload(&mut self, m: &mut Space, raw: u32) -> Result<u32, String> {
            self.record(m, "payload", &[0x3f800000]);
            Ok(if (0..3).any(|i| raw == self.resource(i)) {
                if self.row["mode"] == "no-payload" {
                    0
                } else {
                    self.at("envelope")
                }
            } else if self.row["mode"] == "template-no-payload" {
                0
            } else {
                self.at("template_data")
            })
        }
        fn native_drive(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<(), String> {
            panic!("drive is not a control/animation substitute")
        }
        fn script_drive(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            panic!("drive is not a control/animation substitute")
        }
    }
    impl CallbackHost for Collaborators<'_> {
        fn native_callback(
            &mut self,
            m: &mut Space,
            class: u32,
            body: u32,
            dt: u32,
            _: Callback,
        ) -> Result<(), String> {
            stack::function(m, &[body, dt], Some(class), |m| {
                self.record(m, "native", &[body, dt]);
                if self.row["mode"] == "native-flags" {
                    for i in 0..3 {
                        m.w32(self.resource(i) + 0x54, 0x40);
                    }
                }
                if self.mutated {
                    return Ok(());
                }
                self.mutated = true;
                let mode = self.row["mode"].as_str().unwrap();
                match mode {
                    "native-destroy-current" => {
                        let raw = self.resource(0);
                        let mut link = m.r32(raw + 0x48);
                        while link != 0 {
                            let next = m.r32(link + 4);
                            for i in 0..4 {
                                m.w32(link + i * 4, 0);
                            }
                            link = next;
                        }
                        m.w32(raw + 0x48, 0);
                    }
                    "native-retire-next" => {
                        crate::native_part::bind_reference(m, word(&self.row["nodes"][1]) + 12, 0)
                    }
                    "native-retire-self" => {
                        crate::native_part::bind_reference(m, word(&self.row["nodes"][0]) + 12, 0)
                    }
                    "native-rebind-local" => crate::native_part::bind_reference(
                        m,
                        m.r32(self.resource(0) + 0x48),
                        self.resource(2),
                    ),
                    _ => {}
                }
                Ok(())
            })
        }
        fn script_callback(
            &mut self,
            m: &mut Space,
            envelope: u32,
            dt: u32,
            kind: Callback,
        ) -> Result<(), String> {
            let args = if kind == Callback::Control {
                let wide = (f32::from_bits(dt) as f64).to_bits();
                vec![
                    envelope,
                    0x60cf38,
                    0,
                    3,
                    wide as u32,
                    (wide >> 32) as u32,
                    0x4f,
                ]
            } else {
                vec![envelope, 0x60cae4, 0, 0x4f]
            };
            self.record(m, "script", &args);
            Ok(())
        }
    }

    #[test]
    fn original_control_animation_passes_match_complete_heaps_events_and_abi() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-control-animation-original.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-control-animation-original.json"))
                .expect("run native_control_animation_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["entries"], json!([0x426c90, 0x426fd0]));
        assert_eq!(data["fpu_cw"], 0x7f);
        for (field, path) in [
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
            (
                "producer_sha256",
                "re/oracle/native_control_animation_probe.py",
            ),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 432);
        let actual = rows
            .iter()
            .map(|r| {
                (
                    word(&r["kind"]),
                    word(&r["flags"]),
                    word(&r["resource_kind"]),
                    word(&r["template_kind"]),
                    r["mode"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected = std::collections::BTreeSet::new();
        for kind in [8, 0x1c] {
            for flags in [0, 0x100, 0x08000110] {
                for resource in [1, 2] {
                    for template in [1, 2] {
                        for mode in MODES {
                            expected.insert((kind, flags, resource, template, (*mode).to_owned()));
                        }
                    }
                }
            }
        }
        assert_eq!(
            actual, expected,
            "complete original branch/mutation inventory"
        );
        for (index, row) in rows.iter().enumerate() {
            validate(row).unwrap_or_else(|e| panic!("case{index}: {e}"));
            let mut m = Space::new();
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word(value));
            }
            let mut host = Collaborators {
                row,
                events: vec![],
                mutated: false,
            };
            let retired = if row["kind"] == 8 {
                control(&mut m, word(&row["context"]), word(&row["dt"]), &mut host)
            } else {
                animate(&mut m, word(&row["context"]), word(&row["dt"]), &mut host)
            }
            .unwrap_or_else(|e| panic!("case{index}: {e}"));
            assert_eq!(json!(retired), row["frees"], "case{index} deferred frees");
            let expected = row["events"].as_array().unwrap();
            assert_eq!(host.events.len(), expected.len(), "case{index} call count");
            for (call, (actual, expected)) in host.events.iter().zip(expected).enumerate() {
                for field in [
                    "name",
                    "this",
                    "args",
                    "esp",
                    "return_pc",
                    "registers",
                    "heap",
                ] {
                    assert_eq!(
                        actual[field], expected[field],
                        "case{index} {0} call{call} {field}",
                        row["mode"]
                    );
                }
            }
            assert_eq!(
                json!((0..row["after"].as_array().unwrap().len())
                    .map(|i| m.r32(0x20000000 + i as u32 * 4))
                    .collect::<Vec<_>>()),
                row["after"],
                "case{index} complete caller heap"
            );
            for (name, reg) in [
                ("ebx", Reg::Ebx),
                ("esi", Reg::Esi),
                ("edi", Reg::Edi),
                ("ebp", Reg::Ebp),
            ] {
                assert_eq!(
                    stack::reg(&m, reg),
                    word(&row["nonvolatile"][name]),
                    "case{index} callee-saved {name}"
                );
            }
        }
        let row = rows
            .iter()
            .find(|r| !r["events"].as_array().unwrap().is_empty())
            .unwrap();
        for field in ["before", "after"] {
            let mut corrupt = row.clone();
            corrupt[field] = json!([]);
            assert!(validate(&corrupt).is_err());
        }
        let mut corrupt = row.clone();
        corrupt["events"][0]["heap"] = json!([]);
        assert!(validate(&corrupt).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    fn validate_row(row: &Value) -> Result<(), &'static str> {
        // Three0x100 resources, two0x40 payloads, one0x10 reference.
        const WORDS: usize = 0x390 / 4;
        if row["before"].as_array().map(Vec::len) != Some(WORDS)
            || row["after"].as_array().map(Vec::len) != Some(WORDS)
        {
            return Err("incomplete caller heap");
        }
        let events = row["events"]
            .as_array()
            .ok_or("missing callback observations")?;
        if events
            .iter()
            .any(|event| event["heap"].as_array().map(Vec::len) != Some(WORDS))
        {
            return Err("incomplete callback heap");
        }
        if row["fpu_cw"] != 0x7f || row["esp_delta"] != 8 {
            return Err("unexpected original ABI or control word");
        }
        Ok(())
    }

    #[test]
    fn truncated_drive_observations_cannot_establish_parity() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-frame-original.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-frame-original.json")).unwrap(),
        )
        .unwrap();
        let valid = data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| !row["events"].as_array().unwrap().is_empty())
            .unwrap();
        validate_row(valid).unwrap();
        for field in ["before", "after"] {
            let mut truncated = valid.clone();
            truncated[field] = json!([]);
            assert!(validate_row(&truncated).is_err());
        }
        let mut truncated = valid.clone();
        truncated["events"][0]["heap"] = json!([]);
        assert!(validate_row(&truncated).is_err());
        for field in ["fpu_cw", "esp_delta"] {
            let mut wrong = valid.clone();
            wrong[field] = json!(0);
            assert!(validate_row(&wrong).is_err());
        }
    }

    struct Collaborators<'a> {
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Collaborators<'_> {
        fn record(&mut self, m: &Space, name: &str, this: u32, args: &[u32]) {
            self.events.push(json!({"name":name,"this":this,"args":args,
                "heap":(0..self.row["before"].as_array().unwrap().len())
                    .map(|i| m.r32(0x20000000+i as u32*4)).collect::<Vec<_>>()}));
        }
        fn value(&self, field: &str) -> u32 {
            self.row[field].as_u64().unwrap() as u32
        }
    }
    impl Host for Collaborators<'_> {
        fn minimum(&mut self, m: &mut Space, resource: u32) -> Result<(), String> {
            self.record(m, "minimum", resource, &[0x3f800000]);
            Ok(())
        }
        fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
            self.record(m, "prepare", resource, &[flags, 0, 0]);
            if self.row["mode"] == "template-mutates-body" && resource == self.value("template") {
                m.w32(self.value("envelope") + 0x4c, 0x87654321);
            }
            Ok(
                if self.row["mode"] == "pending"
                    || (self.row["mode"] == "template-pending"
                        && resource == self.value("template"))
                {
                    0x80000001
                } else {
                    0
                },
            )
        }
        fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
            self.record(m, "payload", resource, &[0x3f800000]);
            Ok(if resource == self.value("resource") {
                if self.row["mode"] == "no-payload" {
                    0
                } else {
                    self.value("envelope")
                }
            } else if self.row["mode"] == "template-no-payload" {
                0
            } else {
                self.value("template_data")
            })
        }
        fn native_drive(
            &mut self,
            m: &mut Space,
            class: u32,
            body: u32,
            dt: u32,
        ) -> Result<(), String> {
            self.record(m, "native", class, &[body, dt]);
            if self.row["mode"] == "native-mutates" {
                m.w32(self.value("resource") + 0x54, 0x40);
            }
            Ok(())
        }
        fn script_drive(&mut self, m: &mut Space, envelope: u32) -> Result<(), String> {
            self.record(
                m,
                "script",
                self.value("resource"),
                &[envelope, 0x60cadc, 0, 0x4f],
            );
            Ok(())
        }
    }
    #[test]
    fn original_drive_dispatch_matches_complete_heaps_and_callback_states() {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-frame-original.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-frame-original.json"))
                .expect("run native_frame_probe.py"),
        )
        .unwrap();
        assert_eq!(data["entry"], 0x426e10);
        assert_eq!(data["fpu_cw"], 0x7f);
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_frame_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 72);
        let actual_inventory = rows
            .iter()
            .map(|row| {
                (
                    row["kind"].as_u64().unwrap(),
                    row["flags"].as_u64().unwrap(),
                    row["mode"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected_inventory = std::collections::BTreeSet::new();
        for kind in [1, 2] {
            for flags in [0, 0x100, 0x08000110] {
                for mode in [
                    "null",
                    "pending",
                    "no-payload",
                    "empty",
                    "script",
                    "native-no-template",
                    "template-pending",
                    "template-no-payload",
                    "template-no-class",
                    "native",
                    "native-mutates",
                    "template-mutates-body",
                ] {
                    expected_inventory.insert((kind, flags, mode.to_owned()));
                }
            }
        }
        assert_eq!(
            actual_inventory, expected_inventory,
            "original scenario inventory"
        );
        for (index, row) in rows.iter().enumerate() {
            validate_row(row).unwrap_or_else(|error| panic!("case{index}: {error}"));
            let mut m = Space::new();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut host = Collaborators {
                row,
                events: vec![],
            };
            drive(
                &mut m,
                row["reference"].as_u64().unwrap() as u32,
                0x3c888889,
                &mut host,
            )
            .unwrap_or_else(|error| panic!("case{index}: {error}"));
            let expected_events = row["events"].as_array().unwrap();
            assert_eq!(
                host.events.len(),
                expected_events.len(),
                "case{index} callback count"
            );
            for (call, (actual, expected)) in host.events.iter().zip(expected_events).enumerate() {
                for field in ["name", "this", "args"] {
                    assert_eq!(
                        actual[field], expected[field],
                        "case{index} call{call} {field}"
                    );
                }
                for (word, (actual, expected)) in actual["heap"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(expected["heap"].as_array().unwrap())
                    .enumerate()
                {
                    assert_eq!(
                        actual, expected,
                        "case{index} call{call} full heap word{word}"
                    );
                }
            }
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "case{index} complete heap word{i}"
                );
            }
        }
    }
}
