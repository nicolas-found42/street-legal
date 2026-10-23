//! Source-derived426470/427210 caller frames. Resource and VM bodies remain
//! explicit collaborators; native class calls consume the actual pending CALL.
#[cfg(test)]
#[path = "native_query_frame_composed_tests.rs"]
mod composed_tests;
use crate::native_stack::{self as s, Reg};
use slr_physics::space::Space;

pub trait Host {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String>;
}
fn invoke(
    m: &mut Space,
    h: &mut dyn Host,
    address: u32,
    pc: u32,
    args: &[u32],
    this: Option<u32>,
) -> Result<u32, String> {
    let v = s::call(m, pc, args, this, |m| h.call(m, address, args, this))?;
    s::set_reg(m, Reg::Eax, v);
    Ok(v)
}
fn virtual_call(
    m: &mut Space,
    h: &mut dyn Host,
    raw: u32,
    offset: u32,
    pc: u32,
    args: &[u32],
) -> Result<u32, String> {
    invoke(m, h, m.r32(m.r32(raw) + offset), pc, args, Some(raw))
}
/// exe: FUN_00426470
pub fn query(
    m: &mut Space,
    reference: u32,
    message: u32,
    argument: u32,
    h: &mut dyn Host,
) -> Result<u32, String> {
    s::function(m, &[reference, message, argument], Some(0x636338), |m| {
        s::set_reg(m, Reg::Eax, reference);
        s::set_reg(m, Reg::Ecx, m.r32(reference + 8));
        if m.r32(reference + 8) == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        s::push_reg(m, Reg::Esi);
        let raw = m.r32(reference + 12);
        s::set_reg(m, Reg::Esi, raw);
        let kind = if raw == 0 { 0 } else { m.r32(raw + 0x4c) };
        s::set_reg(m, Reg::Eax, kind.wrapping_sub(1));
        if kind != 1 {
            s::set_reg(m, Reg::Eax, kind.wrapping_sub(8));
        }
        if kind != 1 && kind != 8 {
            s::set_reg(m, Reg::Eax, message);
            return Ok(message);
        }
        if raw == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        let template = kind == 8;
        if m.r32(raw + 0x4c) != 1 {
            s::set_reg(m, Reg::Eax, m.r32(raw));
            virtual_call(
                m,
                h,
                raw,
                0x14,
                if template { 0x4264b5 } else { 0x426516 },
                &[1f32.to_bits()],
            )?;
        }
        let pending = invoke(
            m,
            h,
            0x5447d0,
            if template { 0x4264c5 } else { 0x426526 },
            &[if template { 0x80000000 } else { 0xa0000000 }, 0, 0],
            Some(raw),
        )?;
        if pending & 0x80000000 != 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        s::set_reg(m, Reg::Edx, m.r32(raw));
        let envelope = virtual_call(
            m,
            h,
            raw,
            0xc,
            if template { 0x4264dc } else { 0x426539 },
            &[1f32.to_bits()],
        )?;
        if template {
            if envelope == 0 {
                s::set_reg(m, Reg::Eax, 0);
                return Ok(0);
            }
            s::set_reg(m, Reg::Edx, argument);
            let payload = m.r32(envelope + 12);
            s::set_reg(m, Reg::Ecx, payload);
            s::set_reg(m, Reg::Eax, m.r32(payload));
            s::push(m, argument);
            s::set_reg(m, Reg::Edx, message);
            // The two PUSHes above/below are already prepared at the source site.
            s::push(m, message);
            s::push(m, 0);
            let target = m.r32(m.r32(payload) + 0x3c);
            let result = s::call(m, 0x4264f8, &[], None, |m| {
                h.call(m, target, &[0, message, argument], Some(payload))
            })?;
            s::add_sp(m, 12);
            s::set_reg(m, Reg::Eax, result);
            return Ok(result);
        }
        s::set_reg(m, Reg::Esi, envelope);
        if envelope == 0 {
            s::set_reg(m, Reg::Eax, 0);
            return Ok(0);
        }
        let body = m.r32(envelope + 0x4c);
        s::set_reg(m, Reg::Eax, body);
        if body != 0 {
            let secondary = m.r32(envelope + 0x44);
            s::set_reg(m, Reg::Ecx, secondary);
            if secondary == 0 {
                s::set_reg(m, Reg::Eax, 0);
                return Ok(0);
            }
            let data = invoke(
                m,
                h,
                0x419860,
                0x426560,
                &[0x80000000, 1f32.to_bits(), 0, 0],
                Some(secondary),
            )?;
            if data == 0 {
                s::set_reg(m, Reg::Eax, 0);
                return Ok(0);
            }
            s::set_reg(m, Reg::Ecx, argument);
            let payload = m.r32(data + 12);
            s::set_reg(m, Reg::Eax, payload);
            s::set_reg(m, Reg::Edx, m.r32(payload));
            s::push(m, argument);
            s::set_reg(m, Reg::Ecx, message);
            s::push(m, message);
            let live = m.r32(envelope + 0x4c);
            s::set_reg(m, Reg::Ecx, live);
            s::push(m, live);
            s::set_reg(m, Reg::Ecx, payload);
            let target = m.r32(m.r32(payload) + 0x3c);
            let result = s::call(m, 0x42657c, &[], None, |m| {
                h.call(m, target, &[live, message, argument], Some(payload))
            })?;
            s::add_sp(m, 12);
            s::set_reg(m, Reg::Eax, result);
            Ok(result)
        } else {
            let script = m.r32(envelope + 0x50);
            s::set_reg(m, Reg::Eax, script);
            if script == 0 {
                s::set_reg(m, Reg::Eax, 0);
                return Ok(0);
            }
            s::set_reg(m, Reg::Edx, message);
            invoke(
                m,
                h,
                0x425a90,
                0x42659d,
                &[envelope, 0x60cad4, 1, 2, message, 0x4f],
                None,
            )
        }
    })
}
fn node_constructor(m: &mut Space, node: u32) {
    s::function(m, &[], Some(node), |m| {
        s::set_reg(m, Reg::Eax, node);
        s::set_reg(m, Reg::Ecx, 0);
        m.w32(node + 8, 0);
        m.w32(node + 4, 0);
        m.w32(node, 0x5f09bc);
    });
}
fn weak_constructor(m: &mut Space, reference: u32) {
    s::function(m, &[], Some(reference), |m| {
        s::set_reg(m, Reg::Eax, 0);
        for at in [0, 4, 8, 12] {
            m.w32(reference + at, 0)
        }
    });
}
/// exe: FUN_00427210
pub fn register(
    m: &mut Space,
    context: u32,
    reference: u32,
    h: &mut dyn Host,
) -> Result<(), String> {
    s::function(m, &[reference], Some(context), |m| {
        s::push_reg(m, Reg::Ecx);
        for r in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
            s::push_reg(m, r)
        }
        s::set_reg(m, Reg::Edi, reference);
        s::set_reg(m, Reg::Esi, context);
        let a = s::call(m, 0x42722a, &[reference, 8, 0], Some(0x636338), |m| {
            query(m, reference, 8, 0, h)
        })?;
        s::set_reg(m, Reg::Eax, a);
        //42722a/42722c/42722e precede MOV EBX,EAX and its CALL.
        for value in [0, 9, reference] {
            s::push(m, value)
        }
        s::set_reg(m, Reg::Ecx, 0x636338);
        s::set_reg(m, Reg::Ebx, a);
        let b = s::call(m, 0x42723b, &[], None, |m| query(m, reference, 9, 0, h))?;
        s::add_sp(m, 12);
        s::set_reg(m, Reg::Eax, b);
        for value in [0, 0x1c, reference] {
            s::push(m, value)
        }
        s::set_reg(m, Reg::Ecx, 0x636338);
        s::store(m, 0x24, b);
        let c = s::call(m, 0x42724e, &[], None, |m| query(m, reference, 0x1c, 0, h))?;
        s::add_sp(m, 12);
        s::set_reg(m, Reg::Eax, c);
        s::store(m, 0x10, c);
        for (index, answer) in [a, b, c].into_iter().enumerate() {
            if index == 1 {
                s::set_reg(m, Reg::Eax, s::load(m, 0x18));
            }
            if index == 2 {
                s::set_reg(m, Reg::Eax, s::load(m, 0x10));
                s::set_reg(m, Reg::Ebp, 0);
            }
            if answer == 0 {
                continue;
            }
            let (malloc_pc, base_pc, weak_pc, bind_pc, offset, count) = match index {
                0 => (0x42725d, 0x42726d, 0x42727d, 0x42728e, 0, 0xc4),
                1 => (0x4272c0, 0x4272d0, 0x4272e0, 0x4272f1, 0x1c, 0xc8),
                _ => (0x42731e, 0x42732e, 0, 0x42734b, 0x38, 0xcc),
            };
            s::push(m, 0x1c);
            let node = s::call(m, malloc_pc, &[], None, |m| {
                h.call(m, 0x54f560, &[0x1c], None)
            })?;
            s::set_reg(m, Reg::Eax, node);
            s::set_reg(m, Reg::Ebx, node);
            s::add_sp(m, 4);
            if node == 0 {
                return Err(
                    "NativeRegistrationBoundary: original null allocation dereference".into(),
                );
            }
            s::call(m, base_pc, &[], Some(node), |m| node_constructor(m, node));
            let link = node + 12;
            if index < 2 {
                s::set_reg(m, Reg::Ebp, link);
                s::set_reg(m, Reg::Ecx, link);
                m.w32(node, 0x5f09b8);
                s::call(m, weak_pc, &[], Some(link), |m| weak_constructor(m, link));
            } else {
                s::set_reg(m, Reg::Ecx, link);
                for at in [0, 4, 8, 12] {
                    m.w32(link + at, 0)
                }
            }
            m.w32(node, 0x5f09b4);
            let raw = m.r32(reference + 12);
            s::set_reg(m, if index == 0 { Reg::Eax } else { Reg::Edx }, raw);
            s::call(m, bind_pc, &[raw], Some(link), |m| {
                crate::native_instance::bind(m, link, raw)
            });
            let tail = m.r32(context + offset + 24);
            s::set_reg(m, Reg::Eax, tail);
            m.w32(tail + 4, node);
            m.w32(node + 8, tail);
            s::set_reg(m, Reg::Ecx, context + offset + 16);
            m.w32(node + 4, context + offset + 16);
            if index == 0 {
                s::set_reg(m, Reg::Eax, m.r32(context + count).wrapping_add(1));
            }
            m.w32(context + offset + 24, node);
            m.w32(context + count, m.r32(context + count).wrapping_add(1));
        }
        //427250 overwrote the initial PUSH ECX slot with the third answer.
        s::set_reg(m, Reg::Ecx, s::load(m, 0x10));
        Ok(())
    })
}

struct Runtime<'a>(&'a mut dyn crate::native_install::Host);
impl Host for Runtime<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String> {
        if let Some(payload) = this.filter(|p| {
            matches!(
                m.r32(*p),
                0x5f0f18 | 0x5f0f60 | 0x5f11e4 | 0x5f12f0 | 0x5f133c | 0x5f0e10
            )
        }) {
            let (expected, class) = match m.r32(payload) {
                0x5f0f18 => (0x45c510, "car"),
                0x5f0f60 => (0x4769e0, "part"),
                0x5f11e4 => (0x4625f0, "cursor"),
                0x5f12f0 => (0x47b220, "player"),
                0x5f133c => (0x47bb70, "trigger"),
                _ => (0x4379d0, "button"),
            };
            if address != expected || args.len() != 3 {
                return Err(format!("NativeQueryBoundary: {class} getInfo target{address:#x}, expected{expected:#x} with three arguments"));
            }
            if class == "player" {
                return crate::native_player::get_info(m, payload, args[0], args[1], args[2]);
            }
            if class == "cursor" {
                return crate::native_cursor::get_info(m, payload, args[0], args[1], args[2]);
            }
            return s::function(m, args, this, |m| {
                crate::native_message::template_query(m, payload, class, args[1], args[0], args[2])
            });
        }
        s::function(m, args, this, |m| match address {
            0x54f560 => Ok(m.malloc(args[0])),
            0x5447d0 => self.0.prepare(m, this.unwrap(), args[0]),
            0x419860 => crate::native_assets::access(
                m,
                this.unwrap(),
                crate::native_resource_load::Access {
                    flags: args[0],
                    quality: args[1],
                    priority: args[2],
                    time: args[3],
                },
            ),
            0x425a90 => self.0.script_query(m, args[0], args[4]),
            _ => {
                let raw = this.ok_or("NativeQueryBoundary: collaborator lacks receiver")?;
                if address == m.r32(m.r32(raw) + 0x14) {
                    self.0.minimum(m, raw, args[0])?;
                    Ok(0)
                } else if address == m.r32(m.r32(raw) + 0xc) {
                    self.0.payload(m, raw)
                } else {
                    Err(format!("NativeQueryBoundary: collaborator{address:#x}"))
                }
            }
        })
    }
}
pub fn register_runtime(
    m: &mut Space,
    vm: &mut slr_vm::host::VmHost,
    context: u32,
    reference: u32,
) -> Result<(), String> {
    register(
        m,
        context,
        reference,
        &mut Runtime(&mut crate::native_install_runtime::Runtime::new(vm)),
    )
}

#[cfg(test)]
mod tests {
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
    fn w(v: &Value) -> u32 {
        v.as_u64().unwrap().try_into().unwrap()
    }
    fn digest(path: &std::path::Path) -> String {
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
    }
    fn bytes(v: &Value) -> Vec<u8> {
        let t = v.as_str().unwrap();
        (0..t.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap())
            .collect()
    }
    fn validate_state(state: &Value) {
        assert_eq!(
            state["heap"].as_array().unwrap().len(),
            1024,
            "complete fixed caller arena"
        );
        assert_eq!(
            bytes(&state["stack"]).len(),
            4096,
            "complete retained stack window"
        );
        for name in ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"] {
            w(&state["regs"][name]);
        }
    }
    fn compare(m: &Space, state: &Value, label: &str) {
        validate_state(state);
        for (i, v) in state["heap"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                m.r32(0x20000000 + i as u32 * 4),
                w(v),
                "{label} heap word{i}"
            )
        }
        for (i, v) in bytes(&state["stack"]).into_iter().enumerate() {
            assert_eq!(
                m.r8(0x7feff000 + i as u32),
                v,
                "{label} stack{:#x}",
                0x7feff000 + i as u32
            )
        }
        for (name, r) in REGS {
            assert_eq!(
                s::reg(m, r),
                w(&state["regs"][name]),
                "{label} register{name}"
            )
        }
        assert_eq!(s::sp(m), w(&state["regs"]["esp"]), "{label} ESP");
    }
    struct Controlled<'a> {
        row: &'a Value,
        events: &'a [Value],
        at: usize,
    }
    impl Host for Controlled<'_> {
        fn call(
            &mut self,
            m: &mut Space,
            address: u32,
            args: &[u32],
            this: Option<u32>,
        ) -> Result<u32, String> {
            let e = &self.events[self.at];
            self.at += 1;
            assert_eq!(address, w(&e["address"]));
            assert_eq!(serde_json::json!(args), e["args"]);
            compare(
                m,
                &e["state"],
                &format!("event{} address{address:#x}", self.at),
            );
            let action = |m: &mut Space| -> Result<u32, String> {
                let data = &self.row["input"];
                let spec = &data["spec"];
                Ok(match address {
                    0x54f560 => m.malloc(args[0]),
                    0x300f0000 => {
                        if spec["data"].as_bool().unwrap() {
                            w(&data["envelope"])
                        } else {
                            0
                        }
                    }
                    0x300f0010 => 0,
                    0x5447d0 => w(&spec["refused"]),
                    0x419860 => {
                        match w(&spec["mutation"]) {
                            4 => m.w32(w(&data["envelope"]) + 0x4c, 0x200000f8),
                            5 => m.w32(w(&data["envelope"]) + 0x4c, 0),
                            _ => {}
                        }
                        if spec["secondary_data"].as_bool().unwrap() {
                            w(&data["secondary_data"])
                        } else {
                            0
                        }
                    }
                    0x300f0020 | 0x425a90 => {
                        let msg = if address == 0x425a90 {
                            args[4]
                        } else {
                            args[1]
                        };
                        if address == 0x425a90 {
                            match w(&spec["mutation"]) {
                                1 => m.w32(w(&data["envelope"]) + 0x4c, 0x200000fc),
                                2 => m.w32(w(&data["envelope"]) + 0x50, 0),
                                3 => m.w32(w(&data["reference"]) + 8, 0),
                                _ => {}
                            }
                        }
                        if spec["registration"].as_bool().unwrap() {
                            let i = [8, 9, 28].iter().position(|v| *v == msg).unwrap();
                            (w(&spec["answers"]) >> i) & 1
                        } else {
                            msg ^ 0x12345678
                        }
                    }
                    _ => panic!("unexpected controlled collaborator{address:#x}"),
                })
            };
            if address == 0x425a90 || address == 0x54f560 {
                s::function_cdecl(m, args, action)
            } else {
                s::function(m, args, this, action)
            }
        }
    }
    #[test]
    fn incomplete_caller_states_are_rejected() {
        let state = serde_json::json!({"heap":vec![0u32;1024],"stack":"00".repeat(4096),"regs":{ "eax":0,"ebx":0,"ecx":0,"edx":0,"esi":0,"edi":0,"ebp":0,"esp":0 }});
        validate_state(&state);
        for field in ["heap", "stack"] {
            let mut broken = state.clone();
            if field == "heap" {
                broken[field].as_array_mut().unwrap().pop();
            } else {
                broken[field] = serde_json::json!("00".repeat(4095));
            }
            assert!(std::panic::catch_unwind(|| validate_state(&broken)).is_err());
        }
    }
    #[test]
    fn runtime_dispatch_rejects_wrong_native_targets_before_writes() {
        let mut m = Space::new();
        let p = m.alloc(0x50);
        let mut detached = crate::native_install::Detached;
        for (table, address) in [
            (0x5f0f18, 0x45c510),
            (0x5f0f60, 0x4769e0),
            (0x5f11e4, 0x4625f0),
            (0x5f12f0, 0x47b220),
            (0x5f133c, 0x47bb70),
            (0x5f0e10, 0x4379d0),
        ] {
            m.w32(p, table);
            for wrong in [0, address + 1] {
                m.begin_journal();
                let before = s::sp(&m);
                let error = Runtime(&mut detached)
                    .call(&mut m, wrong, &[0, 8, 0], Some(p))
                    .unwrap_err();
                assert!(error.contains("getInfo target"));
                assert!(m.end_journal().is_empty());
                assert_eq!(s::sp(&m), before);
            }
            // The actual handler runs through its own pending native CALL.
            let value = s::function(&mut m, &[], None, |m| {
                s::call(m, 0x42657c, &[0, 8, 0], Some(p), |m| {
                    Runtime(&mut detached).call(m, address, &[0, 8, 0], Some(p))
                })
            })
            .unwrap();
            assert_eq!(value, u32::from(table == 0x5f0f18 || table == 0x5f12f0));
        }
    }
    #[test]
    fn query_and_registration_frames_match_complete_original_callers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/native-query-frame-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native_query_frame_probe.py --out re/out/boot/native-query-frame-original.json");
            return;
        };
        let fixture: Value = serde_json::from_str(&text).unwrap();
        for (field, path) in [
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_query_frame_probe.py"),
            ("source_sha256", "re/oracle/instance_query_vectors.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(fixture[field], digest(&root.join(path)))
        }
        assert_eq!(fixture["schema"], 1);
        assert_eq!(fixture["stack_low"], 0x7feff000u32);
        assert_eq!(fixture["fpu_cw"], 0x7f);
        assert_eq!(fixture["stack_bytes"], 4096);
        assert_eq!(fixture["heap_bytes"], 4096);
        let rows = fixture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 680);
        for (position, row) in rows.iter().enumerate() {
            assert_eq!(w(&row["index"]), position as u32 % 340);
            assert_eq!(w(&row["fill"]), if position < 340 { 0 } else { 0xa5 });
        }
        for row in rows {
            validate_state(&row["initial"]);
            let mut m = Space::new();
            for (i, v) in row["initial"]["heap"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                m.w32(0x20000000 + i as u32 * 4, w(v))
            }
            for (i, v) in bytes(&row["initial"]["stack"]).into_iter().enumerate() {
                m.w8(0x7feff000 + i as u32, v)
            }
            let d = &row["input"];
            let vt = 0x20000300;
            // Only the explicit controlled virtual tables are driver inputs.
            m.w32(vt + 12, 0x300f0000);
            m.w32(vt + 20, 0x300f0010);
            m.set_heap_ptr(w(&d["heap_before"]));
            let mut h = Controlled {
                row,
                events: row["events"].as_array().unwrap(),
                at: 0,
            };
            if d["spec"]["registration"].as_bool().unwrap() {
                register(&mut m, w(&d["context"]), w(&d["reference"]), &mut h).unwrap()
            } else {
                let v = query(&mut m, w(&d["reference"]), 8, 37, &mut h).unwrap();
                assert_eq!(v, w(&d["result"]))
            }
            compare(
                &m,
                &row["final"],
                &format!("final case{} fill{}", row["index"], row["fill"]),
            );
            assert_eq!(h.at, h.events.len());
            assert_eq!(m.heap_ptr(), w(&d["heap_after"]));
        }
    }
}
