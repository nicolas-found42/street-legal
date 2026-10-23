//! Original477e00 device-profile configuration and54d650/54d7b0 live input
//! storage. Device discovery/samples/type and Config remain actual collaborators;
//! this module neither invents physical devices nor accepts macOS device parity.
use crate::native_stack::{self as s, Reg};
use slr_physics::space::Space;

#[derive(Clone, Copy)]
struct Axis {
    pc: u32,
    values: [u32; 6],
}
#[derive(Clone, Copy)]
struct Mapping {
    pc: u32,
    action: u32,
    device_delta: u32,
    control: u32,
    input_min: Option<u32>,
    input_max: u32,
    output_min: u32,
    output_max: u32,
}
#[path = "native_player_device_tables.rs"]
mod tables;

pub trait Host {
    ///Actual558100 device enumeration name; null means no device at this index.
    fn name(&mut self, m: &mut Space, device: u32) -> Result<u32, String>;
    ///Actual4261f0 Config.controller_deadzone, as the original f32 setting bits.
    fn deadzone(&mut self, m: &mut Space, key: u32) -> Result<u32, String>;
    ///Actual557430 current physical sample, narrowed by original FSTP m32.
    fn sample(&mut self, m: &mut Space, device: u32, control: u32) -> Result<u32, String>;
    ///Actual557990 physical control type; original stores this in logical row+4.
    fn axis_kind(&mut self, m: &mut Space, device: u32, control: u32) -> Result<u32, String>;
}
fn prepared<R>(m: &mut Space, pc: u32, f: impl FnOnce(&mut Space) -> R) -> R {
    s::call(m, pc, &[], None, |m| s::function(m, &[], None, f))
}

/// exe: FUN_0054d7b0
pub fn axis(m: &mut Space, input: u32, args: [u32; 6]) {
    s::function(m, &args, Some(input), |m| {
        let row = input
            .wrapping_add(args[0].wrapping_mul(0x30))
            .wrapping_add(4);
        for (offset, value) in [
            (0xc, args[1]),
            (0x10, args[2]),
            (0x14, args[3]),
            (0x18, args[4]),
            (0x20, args[5]),
        ] {
            m.w32(row + offset, value);
        }
        s::set_reg(m, Reg::Eax, row);
        s::set_reg(m, Reg::Ecx, args[5]);
        s::set_reg(m, Reg::Edx, args[4]);
    });
}

/// Original first-free152-slot insertion. Both physical collaborator calls are
/// real cdecl frames and their four argument words survive until54d6dd.
/// exe: FUN_0054d650
pub fn add(m: &mut Space, input: u32, args: [u32; 7], host: &mut dyn Host) -> Result<u32, String> {
    s::function(m, &args, Some(input), |m| {
        s::push_reg(m, Reg::Ebp);
        s::set_reg(m, Reg::Ebp, input);
        s::set_reg(m, Reg::Ecx, args[1]);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        if (args[1] as i32) < 0 || (args[2] as i32) < 0 {
            return Ok(u32::MAX);
        }
        let Some(index) = (0..0x98).find(|i| m.r32(input + 0xe44 + i * 0x20) == 0) else {
            return Ok(0);
        };
        let slot = input + 0xe44 + index * 0x20;
        s::set_reg(m, Reg::Esi, slot);
        s::set_reg(m, Reg::Edi, index);
        s::push_reg(m, Reg::Ebx);
        s::set_reg(m, Reg::Ebx, args[0]);
        for (offset, value) in [(0, args[0]), (4, args[1]), (8, args[2])] {
            m.w32(slot + offset, value);
        }
        s::push(m, args[2]);
        s::push(m, args[1]);
        let sample = prepared(m, 0x54d6a2, |m| host.sample(m, args[1], args[2]))?;
        m.w32(slot + 0xc, sample);
        for (offset, value) in [
            (0x14, args[4]),
            (0x18, args[5]),
            (0x10, args[3]),
            (0x1c, args[6]),
        ] {
            m.w32(slot + offset, value);
        }
        s::set_reg(m, Reg::Ecx, args[2]);
        s::set_reg(m, Reg::Edx, args[1]);
        s::push(m, args[2]);
        s::push(m, args[1]);
        let kind = prepared(m, 0x54d6d0, |m| host.axis_kind(m, args[1], args[2]))?;
        let row = input.wrapping_add(args[0].wrapping_mul(0x30));
        m.w32(row + 8, kind);
        s::add_sp(m, 0x10);
        m.w32(input, m.r32(input).wrapping_add(1));
        Ok(index + 1)
    })
}

fn ascii_equal(m: &mut Space, pointer: u32, expected: &[u8]) -> Result<bool, String> {
    //5d7190's default loop changesCL only on a raw-byte mismatch. Preserve
    //that volatile value because the following cdecl Config callback sees it.
    for (i, right) in expected.iter().copied().chain([0]).enumerate() {
        let left = m.r8(pointer + i as u32);
        if left != right {
            let fold = |c: u8| if c.is_ascii_uppercase() { c + 0x20 } else { c };
            let low = if left.is_ascii_uppercase() { 0x20 } else { 0 };
            s::set_reg(m, Reg::Ecx, (s::reg(m, Reg::Ecx) & !0xff) | low);
            if fold(left) != fold(right) {
                return Ok(false);
            }
        }
        if right == 0 {
            return Ok(true);
        }
    }
    unreachable!()
}

/// Original477e00. Receives a pending real CALL from47a9dd, or a direct oracle
/// entry. It is cdecl and retains both incoming argument words on return.
/// exe: FUN_00477e00
pub fn configure(
    m: &mut Space,
    input: u32,
    device: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let result = s::function(m, &[input, device], None, |m| {
        for reg in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            s::push_reg(m, reg);
        }
        s::set_reg(m, Reg::Esi, device);
        let name = s::external(m, 0x477e0d, &[device], None, |m| {
            s::external(m, 0x54de2a, &[device], None, |m| host.name(m, device))
        })?;
        s::set_reg(m, Reg::Edi, name);
        if name == 0 {
            return Ok(());
        }
        if m.r32(0x79d6b0) != 0 {
            return Err(
                "NativePlayerDeviceBoundary: localized profile-name comparison is not ported"
                    .into(),
            );
        }
        let names: [&[u8]; 5] = [
            b"SysKeyboard",
            b"4 axis 16 button joystick",
            b"InterAct SV-262 HammerHead Fx",
            b"Microsoft SideWinder game pad",
            b"DUALSHOCK",
        ];
        let mut profile = 5;
        for (index, expected) in names.iter().enumerate() {
            if ascii_equal(m, name, expected)? {
                profile = index;
                break;
            }
        }
        let (axes, mappings, key, pc) = match profile {
            0 => (tables::KEYBOARD_AXES, tables::KEYBOARD_MAPPINGS, 0, 0),
            1 => (
                tables::JOYSTICK_AXES,
                tables::JOYSTICK_MAPPINGS,
                0x61279c,
                0x47849d,
            ),
            2 => (
                tables::HAMMER_AXES,
                tables::HAMMER_MAPPINGS,
                0x6127d0,
                0x478919,
            ),
            3 => (tables::SIDEWINDER_AXES, tables::SIDEWINDER_MAPPINGS, 0, 0),
            4 => (
                tables::DUALSHOCK_AXES,
                tables::DUALSHOCK_MAPPINGS,
                0x612810,
                0x479062,
            ),
            _ => (
                tables::GENERIC_AXES,
                tables::GENERIC_MAPPINGS,
                0x612824,
                0x47950d,
            ),
        };
        let deadzone = if key != 0 {
            s::push(m, key);
            let value = prepared(m, pc, |m| host.deadzone(m, key))?;
            //FSTP [ESP+18] overwrites the incoming device argument, not a new
            //local. ESI retains the real device index for the entire routine.
            m.w32(s::sp(m) + 0x18, value);
            s::add_sp(m, 4);
            value
        } else {
            0
        };
        s::set_reg(m, Reg::Edi, input);
        for row in axes {
            s::call(m, row.pc, &row.values, Some(input), |m| {
                axis(m, input, row.values)
            });
        }
        if key != 0 {
            s::set_reg(m, Reg::Ebx, deadzone);
        }
        for row in mappings {
            let mapped_device = device.wrapping_add(row.device_delta);
            s::set_reg(m, Reg::Esi, mapped_device);
            let args = [
                row.action,
                mapped_device,
                row.control,
                row.input_min.unwrap_or(deadzone),
                row.input_max,
                row.output_min,
                row.output_max,
            ];
            s::call(m, row.pc, &args, Some(input), |m| add(m, input, args, host))?;
        }
        Ok(())
    });
    s::set_sp(m, s::sp(m).wrapping_sub(8));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    const WORDS: usize = 0x2300 / 4;
    const NAMES: [&str; 6] = [
        "SysKeyboard",
        "4 axis 16 button joystick",
        "InterAct SV-262 HammerHead Fx",
        "Microsoft SideWinder game pad",
        "DUALSHOCK",
        "unrecognized",
    ];
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().expect("unsigned machine word")).expect("32-bit machine word")
    }
    fn heap(m: &Space) -> Value {
        json!((0..WORDS)
            .map(|i| m.r32(0x20000000 + i as u32 * 4))
            .collect::<Vec<_>>())
    }
    fn sample(device: u32, control: u32) -> u32 {
        [0x80000000, 0x3eaaaaab, 0xbf400000, 1][device.wrapping_add(control) as usize % 4]
    }
    fn validate(row: &Value) -> bool {
        let complete = |v: &Value| {
            v.as_array().is_some_and(|a| {
                a.len() == WORDS
                    && a.iter()
                        .all(|v| v.as_u64().is_some_and(|n| n <= u32::MAX as u64))
            })
        };
        complete(&row["before"])
            && complete(&row["after"])
            && row["events"].as_array().is_some_and(|events| {
                events.iter().all(|e| {
                    complete(&e["heap"])
                        && ["this", "esp", "return_pc"]
                            .iter()
                            .all(|f| e[f].as_u64().is_some_and(|n| n <= u32::MAX as u64))
                        && e["args"].as_array().is_some()
                })
            })
    }
    struct Controlled<'a> {
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Controlled<'_> {
        fn record(&mut self, m: &Space, name: &str, n: usize) {
            self.events.push(json!({"name":name,"this":s::reg(m,Reg::Ecx),"args":(0..n).map(|i|m.r32(s::sp(m)+4+i as u32*4)).collect::<Vec<_>>(),"esp":s::sp(m),"return_pc":m.r32(s::sp(m)),"heap":heap(m)}));
        }
    }
    impl Host for Controlled<'_> {
        fn name(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
            self.record(m, "name", 1);
            Ok(if self.row["name"].is_null() {
                0
            } else {
                w(&self.row["layout"]["name"])
            })
        }
        fn deadzone(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
            self.record(m, "deadzone", 1);
            Ok(w(&self.row["deadzone"]))
        }
        fn sample(&mut self, m: &mut Space, d: u32, c: u32) -> Result<u32, String> {
            self.record(m, "sample", 2);
            Ok(sample(d, c))
        }
        fn axis_kind(&mut self, m: &mut Space, d: u32, c: u32) -> Result<u32, String> {
            self.record(m, "axis_kind", 2);
            Ok((d ^ c) & 1)
        }
    }
    #[test]
    fn original_device_profiles_match_complete_heap_callbacks_and_cdecl_argument_mutation() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-player-device-original.json"))
                .expect("run native_player_device_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        for (field, path) in [
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_player_device_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
            (
                "recovery_sha256",
                "re/oracle/native_player_device_recover.py",
            ),
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
        assert_eq!(rows.len(), 64);
        let actual = rows
            .iter()
            .map(|r| {
                (
                    w(&r["fill"]),
                    r["name"].as_str(),
                    r["slots"].as_str().unwrap(),
                    w(&r["device"]),
                    w(&r["deadzone"]),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected = std::collections::BTreeSet::new();
        for fill in [0, 0xa5] {
            for name in NAMES {
                for slots in ["empty", "last-two"] {
                    expected.insert((fill, Some(name), slots, 0, 0x3e000000));
                }
            }
            for name in [None, Some("sYSkeYBoARD")] {
                expected.insert((fill, name, "empty", 0, 0x3e000000));
            }
        }
        for name in [NAMES[1], NAMES[2], NAMES[4], NAMES[5]] {
            for dead in [0x80000000, 1, 0x3f666666] {
                expected.insert((0, Some(name), "empty", 0, dead));
            }
        }
        for name in NAMES {
            for device in [u32::MAX, 0x7fffffff] {
                expected.insert((0, Some(name), "empty", device, 0x3e000000));
            }
            for slots in ["full", "sparse"] {
                expected.insert((0, Some(name), slots, 2, 0x3e000000));
            }
        }
        assert_eq!(actual, expected);
        let mut missing = rows[0].clone();
        missing["before"].as_array_mut().unwrap().pop();
        assert!(!validate(&missing));
        let mut missing = rows[0].clone();
        missing["events"][0]
            .as_object_mut()
            .unwrap()
            .remove("return_pc");
        assert!(!validate(&missing));
        for (index, row) in rows.iter().enumerate() {
            assert!(validate(row), "case{index} complete observations");
            let mut m = Space::new();
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, w(value));
            }
            if let Some(name) = row["name"].as_str() {
                let p = w(&row["layout"]["name"]);
                for (i, byte) in name.bytes().chain([0]).enumerate() {
                    m.w8(p + i as u32, byte);
                }
            }
            let input = w(&row["layout"]["input"]);
            let mut host = Controlled {
                row,
                events: vec![],
            };
            configure(&mut m, input, w(&row["device"]), &mut host)
                .unwrap_or_else(|e| panic!("case{index}: {e}"));
            let original = row["events"].as_array().unwrap();
            assert_eq!(
                host.events.len(),
                original.len(),
                "case{index} callback count"
            );
            for (call, (a, b)) in host.events.iter().zip(original).enumerate() {
                for field in ["name", "this", "args", "esp", "return_pc", "heap"] {
                    assert_eq!(a[field], b[field], "case{index} call{call} {field}");
                }
            }
            assert_eq!(heap(&m), row["after"], "case{index} full caller heap");
            assert_eq!(
                json!([m.r32(s::sp(&m)), m.r32(s::sp(&m) + 4)]),
                row["args_after"],
                "case{index} cdecl caller argument bytes"
            );
            assert_eq!(s::sp(&m), 0x7feffef0);
            assert_eq!(row["esp_delta"], 0);
            assert_eq!(row["fpu_cw"], 0x7f);
            assert_eq!(row["fpu_depth"], 0);
            for (name, reg) in [
                ("ebx", Reg::Ebx),
                ("esi", Reg::Esi),
                ("edi", Reg::Edi),
                ("ebp", Reg::Ebp),
            ] {
                assert_eq!(s::reg(&m, reg), w(&row["nonvolatile"][name]));
            }
        }
    }
}
