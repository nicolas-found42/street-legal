//! Original47cc10 Input.lastKey and556e00 DirectInput polling.
//! Device and Win32 calls are explicit environmental leaves, never native
//! return tables. Caller ABI, keyboard construction and physical input binding
//! are separate dependencies; the VM native-entry guard remains in place.
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaf {
    Acquire,
    GetDeviceData,
    MapVirtualKey,
    GetKeyboardState,
    ToAscii,
}
pub trait Environment {
    fn call(
        &mut self,
        m: &mut Space,
        leaf: Leaf,
        implementation: u32,
        args: &[u32],
    ) -> Result<u32, String>;
}
#[cfg(test)]
thread_local! { static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())}; }
fn word(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    if p < 0x7fe00000 {
        STORES.with(|s| s.borrow_mut().push((p, 4, v)));
    }
    m.w32(p, v);
}
// The oracle hook retains the full source register even for byte stores.
fn byte(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    if p < 0x7fe00000 {
        STORES.with(|s| s.borrow_mut().push((p, 1, v)));
    }
    m.w8(p, v as u8);
}
fn set(m: &mut Space, r: Reg, v: u32) {
    stack::set_reg(m, r, v);
}
fn leaf(
    m: &mut Space,
    kind: Leaf,
    target: u32,
    pc: u32,
    args: &[u32],
    env: &mut impl Environment,
) -> Result<u32, String> {
    if target == 0 {
        return Err("NativeKeyboard: unresolved environmental leaf".into());
    }
    let value = stack::external(m, pc, args, None, |m| env.call(m, kind, target, args))?;
    set(m, Reg::Eax, value);
    Ok(value)
}
fn poll(m: &mut Space, env: &mut impl Environment) -> Result<u32, String> {
    stack::function_cdecl(m, &[], |m| {
        let device = m.r32(0x76f9b0);
        let table = m.r32(device);
        if device == 0 || table == 0 {
            return Err("NativeKeyboard: missing original keyboard/vtable".into());
        }
        set(m, Reg::Eax, device);
        set(m, Reg::Ecx, table);
        stack::alloc(m, 0x11c);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        let b = stack::sp(m);
        let result = (|| {
            let acquire = leaf(
                m,
                Leaf::Acquire,
                m.r32(table + 0x1c),
                0x556e13,
                &[device],
                env,
            )?;
            if (acquire as i32) < 0 {
                return Ok(0);
            }
            let device = m.r32(0x76f9b0);
            set(m, Reg::Eax, device);
            set(m, Reg::Ecx, b + 0x10);
            set(m, Reg::Esi, 1);
            word(m, b + 0xc, 1);
            let table = m.r32(device);
            set(m, Reg::Edx, table);
            let status = leaf(
                m,
                Leaf::GetDeviceData,
                m.r32(table + 0x28),
                0x556e3d,
                &[device, 20, b + 0x10, b + 0xc, 0],
                env,
            )?;
            if (status as i32) < 0 {
                return Ok(0);
            }
            let scan = m.r32(b + 0x10);
            set(m, Reg::Eax, scan);
            set(m, Reg::Edi, scan);
            if m.r32(b + 0xc) != stack::reg(m, Reg::Esi)
                || scan >= 256
                || m.r8(b + 0x14) & 0x80 == 0
            {
                return Ok(0);
            }
            let layout = m.r32(0x777430);
            set(m, Reg::Edx, layout);
            word(m, b + 8, 0);
            let vk = leaf(
                m,
                Leaf::MapVirtualKey,
                m.r32(0x5e72b0),
                0x556e72,
                &[scan, 1, layout],
                env,
            )?;
            set(m, Reg::Esi, vk);
            set(m, Reg::Eax, b + 0x24);
            let state = leaf(
                m,
                Leaf::GetKeyboardState,
                m.r32(0x5e72b4),
                0x556e7f,
                &[b + 0x24],
                env,
            )?;
            if state != 0 {
                let layout = m.r32(0x777430);
                set(m, Reg::Ecx, layout);
                set(m, Reg::Edx, b + 8);
                set(m, Reg::Eax, b + 0x24);
                leaf(
                    m,
                    Leaf::ToAscii,
                    m.r32(0x5e72b8),
                    0x556e9e,
                    &[vk, scan, b + 0x24, b + 8, 0, layout],
                    env,
                )?;
            }
            Ok((m.r32(b + 8) & 0xffff) << 16 | scan)
        })();
        if let Ok(value) = result.as_ref() {
            set(m, Reg::Eax, *value);
        }
        set(m, Reg::Edi, m.r32(stack::sp(m)));
        stack::add_sp(m, 4);
        set(m, Reg::Esi, m.r32(stack::sp(m)));
        stack::add_sp(m, 4);
        stack::add_sp(m, 0x11c);
        result
    })
}
/// exe: LAB_0047cc10
#[allow(dead_code)]
pub fn last_key(m: &mut Space, info: u32, env: &mut impl Environment) -> Result<u32, String> {
    stack::function_cdecl(m, &[info], |m| {
        let value = stack::call(m, 0x47cc15, &[], None, |m| poll(m, env))?;
        if value & 0xff0000 != 0 {
            let ring = m.r32(0x612c68);
            if !(0x640924..0x640934).contains(&ring) {
                return Err("NativeKeyboard: unresolved live text ring pointer".into());
            }
            set(m, Reg::Edx, ring);
            set(m, Reg::Ecx, (value as i32 >> 16) as u32);
            byte(m, ring, stack::reg(m, Reg::Ecx));
            let next = m.r32(0x612c68) + 1;
            set(m, Reg::Ecx, next);
            word(m, 0x612c68, next);
            if next == 0x640934 {
                set(m, Reg::Ecx, next - 16);
                word(m, 0x612c68, next - 16);
            }
        }
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    const GLOBALS: [u32; 6] = [0x76f9b0, 0x777430, 0x612c68, 0x5e72b0, 0x5e72b4, 0x5e72b8];
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().expect("machine word")).unwrap()
    }
    fn regs() -> [(Reg, &'static str); 7] {
        [
            (Reg::Eax, "eax"),
            (Reg::Ebx, "ebx"),
            (Reg::Ecx, "ecx"),
            (Reg::Edx, "edx"),
            (Reg::Esi, "esi"),
            (Reg::Edi, "edi"),
            (Reg::Ebp, "ebp"),
        ]
    }
    fn bytes(v: &Value, n: usize) -> Vec<u8> {
        let a = v.as_array().unwrap();
        assert_eq!(a.len(), n);
        a.iter().map(|v| u8::try_from(w(v)).unwrap()).collect()
    }
    fn compare(m: &Space, v: &Value) {
        for (name, p, n) in [
            ("heap", 0x20000000, 0x100),
            ("stack", 0x7feffb00, 0x500),
            ("ring", 0x640924, 16),
        ] {
            let expected = bytes(&v[name], n);
            assert_eq!(
                (0..n).map(|i| m.r8(p + i as u32)).collect::<Vec<_>>(),
                expected,
                "{name}"
            );
        }
        assert_eq!(m.heap_ptr(), w(&v["heap_ptr"]));
        assert_eq!(v["globals"].as_object().unwrap().len(), GLOBALS.len());
        for p in GLOBALS {
            assert_eq!(m.r32(p), w(&v["globals"][p.to_string()]), "global{p:x}");
        }
        assert_eq!(v["registers"].as_object().unwrap().len(), 8);
        for (r, n) in regs() {
            assert_eq!(stack::reg(m, r), w(&v["registers"][n]), "{n}");
        }
        assert_eq!(stack::sp(m), w(&v["registers"]["esp"]));
    }
    fn label(l: Leaf) -> &'static str {
        match l {
            Leaf::Acquire => "acquire",
            Leaf::GetDeviceData => "data",
            Leaf::MapVirtualKey => "map",
            Leaf::GetKeyboardState => "state",
            Leaf::ToAscii => "ascii",
        }
    }
    struct Env<'a> {
        row: &'a Value,
        index: usize,
    }
    impl Environment for Env<'_> {
        fn call(
            &mut self,
            m: &mut Space,
            l: Leaf,
            target: u32,
            args: &[u32],
        ) -> Result<u32, String> {
            let event = &self.row["events"][self.index];
            self.index += 1;
            assert_eq!(event["kind"], label(l));
            assert_eq!(json!(args), event["args"]);
            compare(m, &event["state"]);
            let c = &self.row["case"];
            Ok(match l {
                Leaf::Acquire => {
                    assert_eq!(target, 0x41b860);
                    w(&c["acquire"])
                }
                Leaf::GetDeviceData => {
                    assert_eq!(target, 0x41b870);
                    if c["data_count_write"] == true {
                        m.w32(args[3], w(&c["count"]));
                    }
                    if c["data_writes"] == true {
                        for (i, value) in [
                            w(&c["scan"]),
                            w(&c["data"]),
                            0x12345678,
                            0x23456789,
                            0x3456789a,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            m.w32(args[2] + i as u32 * 4, value);
                        }
                    }
                    w(&c["data_result"])
                }
                Leaf::MapVirtualKey => {
                    assert_eq!(target, m.r32(0x5e72b0));
                    w(&c["vk"])
                }
                Leaf::GetKeyboardState => {
                    assert_eq!(target, m.r32(0x5e72b4));
                    if c["state_writes"] == true {
                        for i in 0..256 {
                            m.w8(args[0] + i, (i * 17 + 3) as u8);
                        }
                    }
                    w(&c["state_result"])
                }
                Leaf::ToAscii => {
                    assert_eq!(target, m.r32(0x5e72b8));
                    let v = w(&c["ascii"]);
                    m.w8(args[3], v as u8);
                    m.w8(args[3] + 1, (v >> 8) as u8);
                    w(&c["ascii_result"])
                }
            })
        }
    }
    #[test]
    fn original_wrapper_poll_all_branches_match_entire_memory_scratch_registers_and_leaf_sequence()
    {
        let required_fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-keyboard-original-attempt3.json");
        if crate::oracle_fixture::file(&required_fixture).is_none() {
            return;
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-keyboard-original-attempt3.json"))
                .expect("run native_keyboard_probe.py"),
        )
        .unwrap();
        let digest =
            |p: &std::path::Path| format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()));
        assert_eq!(fixture["schema"], 1);
        assert_eq!(fixture["entry"], 0x47cc10);
        assert_eq!(fixture["cw"], 127);
        assert_eq!(fixture["unicorn"], "2.1.4");
        assert_eq!(
            fixture["exe_sha256"],
            digest(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
        );
        let producers = fixture["producer_sources"].as_object().unwrap();
        assert_eq!(producers.len(), 2);
        for n in ["native_keyboard_probe.py", "oracle.py"] {
            assert_eq!(producers[n], digest(&root.join("re/oracle").join(n)));
        }
        let cases = [
            json!({"label":"acquire-failed","acquire":0x8007001eu32}),
            json!({"label":"data-failed","data_result":0x8007001eu32}),
            json!({"label":"empty","count":0}),
            json!({"label":"two-events","count":2}),
            json!({"label":"scan-too-high","scan":256}),
            json!({"label":"key-up","data":0}),
            json!({"label":"state-failed","state_result":0}),
            json!({"label":"ascii-zero","ascii":0}),
            json!({"label":"ascii-one","ascii":65}),
            json!({"label":"ascii-two","ascii":0x4241}),
            json!({"label":"dead-key","ascii":0xb4,"ascii_result":0xffffffffu32}),
            json!({"label":"scan-zero","scan":0,"ascii":81}),
            json!({"label":"scan-last","scan":255,"ascii":127}),
            json!({"label":"ascii-signed","ascii":0xff80}),
            json!({"label":"ascii-high-only","ascii":0x100}),
            json!({"label":"buffer-overflow","data_result":1}),
            json!({"label":"quiescent-count-only","count":0,"data_writes":false}),
            json!({"label":"data-failed-untouched","data_result":0x8007001eu32,"data_writes":false,"data_count_write":false}),
            json!({"label":"state-failed-untouched","state_result":0,"state_writes":false}),
        ];
        let rows = fixture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 76);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(
                row["input"],
                json!({"case":i/4,"fill":([0,0xa5][i/2%2]),"ring_offset":([0,15][i%2])})
            );
            let mut expected = json!({"acquire":0,"data_result":0,"count":1,"scan":30,"data":128,"state_result":1,"ascii":65,"ascii_result":1,"vk":0x1234,"data_writes":true,"data_count_write":true,"state_writes":true});
            for (k, v) in cases[i / 4].as_object().unwrap() {
                expected[k] = v.clone();
            }
            assert_eq!(row["case"], expected);
            let mut m = Space::new();
            let before = &row["before"];
            for (name, p, n) in [
                ("heap", 0x20000000, 0x100),
                ("stack", 0x7feffb00, 0x500),
                ("ring", 0x640924, 16),
            ] {
                for (j, b) in bytes(&before[name], n).into_iter().enumerate() {
                    m.w8(p + j as u32, b);
                }
            }
            m.set_heap_ptr(w(&before["heap_ptr"]));
            for p in GLOBALS {
                m.w32(p, w(&before["globals"][p.to_string()]));
            }
            for (r, n) in regs() {
                set(&mut m, r, w(&row["register_inputs"][n]));
            }
            let esp = w(&row["entry_esp"]);
            stack::set_sp(&mut m, esp);
            m.w32(esp, 0x7fff0000);
            m.w32(esp + 4, w(&row["info"]));
            m.w32(0x7ffd_f004, 1);
            STORES.with(|s| s.borrow_mut().clear());
            let mut env = Env { row, index: 0 };
            let result = last_key(&mut m, w(&row["info"]), &mut env).unwrap();
            assert_eq!(result, w(&row["result"]), "case{i}");
            assert_eq!(env.index, row["events"].as_array().unwrap().len());
            compare(&m, &row["after"]);
            let stores = STORES.with(|s| s.borrow().clone());
            assert_eq!(
                json!(stores),
                row["stores"],
                "ordered ring/global stores case{i}"
            );
        }
    }
}
