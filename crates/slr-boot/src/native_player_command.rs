//! Actual47a920's command entry. Boot device/setcontrol, renderinstance and
//! team paths are recovered; unrecovered camera/target helpers fail explicitly.
use crate::native_stack::{self as s, Reg};
use slr_physics::space::Space;

pub trait Host {
    ///419860 receives the live raw resource and access flags through its frame.
    fn access(&mut self, m: &mut Space, raw: u32) -> Result<u32, String>;
    ///425a90 is entered with its actual variadic cdecl frame.
    fn script(
        &mut self,
        m: &mut Space,
        envelope: u32,
        method: u32,
        values: &[u32],
    ) -> Result<(), String>;
    ///477e00 native mapping receives a pending real CALL with two cdecl arguments.
    fn device(&mut self, m: &mut Space, input: u32, device: u32) -> Result<(), String>;
    ///545fc0's untyped live weak-reference binding.
    fn bind(&mut self, m: &mut Space, reference: u32, id: u32) -> Result<(), String>;
}

fn text(m: &Space, pointer: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for i in 0..4096 {
        let byte = m.r8(pointer.wrapping_add(i));
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err("NativePlayerCommandBoundary: unterminated command exceeds bounded reader".into())
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, 9..=13 | 32)
}
///553890 %*s %d: decimal accepts minus, but not plus, with wrapping32-bit math.
fn integer(bytes: &[u8], mut at: usize) -> u32 {
    while bytes.get(at).is_some_and(|b| whitespace(*b)) {
        at += 1;
    }
    let negative = bytes.get(at) == Some(&b'-');
    if negative {
        at += 1;
    }
    let mut result = 0u32;
    while let Some(byte @ b'0'..=b'9') = bytes.get(at) {
        result = result.wrapping_mul(10).wrapping_add((byte - b'0') as u32);
        at += 1;
    }
    if negative {
        result.wrapping_neg()
    } else {
        result
    }
}

/// Original47a920; `value` is the actual command CString for event0x10.
/// Other event kinds except1 return1; event1 collision processing is explicit.
/// exe: FUN_0047a920
pub fn command(
    m: &mut Space,
    k: u32,
    body: u32,
    parameter: u32,
    event: u32,
    value: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    s::function(m, &[body, parameter, event, value], Some(k), |m| {
        s::push_reg(m, Reg::Ebp);
        let frame = s::sp(m);
        s::set_reg(m, Reg::Ebp, frame);
        s::alloc(m, 0x19c);
        for reg in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            s::push_reg(m, reg);
        }
        if event == 1 {
            return Err("NativePlayerCommandBoundary: collision event1 is not ported".into());
        }
        if event != 0x10 {
            return Ok(1);
        }
        let bytes = text(m, value)?;
        let begin = bytes
            .iter()
            .position(|b| !whitespace(*b))
            .unwrap_or(bytes.len());
        let end = bytes[begin..]
            .iter()
            .position(|b| whitespace(*b))
            .map(|n| begin + n)
            .unwrap_or(bytes.len());
        let token = &bytes[begin..end];
        if token.len() > 75 {
            return Err(
                "NativePlayerCommandBoundary: original command token overflows76-byte local".into(),
            );
        }
        for (i, byte) in token.iter().copied().chain([0]).enumerate() {
            m.w8(frame - 0x5c + i as u32, byte);
        }
        s::set_reg(m, Reg::Esi, value);
        let number = integer(&bytes, end);
        //5d7190 default path folds ASCII A..Z; locale path is separate.
        if m.r32(0x79d6b0) != 0 {
            return Err(
                "NativePlayerCommandBoundary: localized string comparison is not ported".into(),
            );
        }
        let lower = token.to_ascii_lowercase();
        let token = lower.as_slice();
        match token {
            b"device" | b"setcontrol" => {
                if token == b"device" {
                    m.w32(frame + 0x14, number);
                }
                let raw = m.r32(body + 0xc);
                if raw == 0 {
                    return Ok(1);
                }
                if token == b"device" {
                    s::set_reg(m, Reg::Esi, body);
                }
                let pc = if token == b"device" {
                    0x47a9a0
                } else {
                    0x47aa27
                };
                let envelope =
                    s::external(m, pc, &[0x80000000, 0x3f800000, 0, 0], Some(raw), |m| {
                        host.access(m, raw)
                    })?;
                if envelope == 0 {
                    return Ok(1);
                }
                let script = m.r32(envelope + 0x50);
                if script != 0 {
                    if token == b"device" {
                        s::set_reg(m, Reg::Ecx, number);
                        let values = [0, 2, number, 0x4f];
                        s::external(
                            m,
                            0x47a9c4,
                            &[envelope, 0x612b2c, 0, 2, number, 0x4f],
                            None,
                            |m| host.script(m, envelope, 0x612b2c, &values),
                        )?;
                    } else {
                        s::set_reg(m, Reg::Ecx, script);
                        let values = [0, 0x4f];
                        s::external(m, 0x47aa49, &[envelope, 0x612b4c, 0, 0x4f], None, |m| {
                            host.script(m, envelope, 0x612b4c, &values)
                        })?;
                    }
                } else if token == b"device" {
                    s::set_reg(m, Reg::Ecx, 0);
                    s::set_reg(m, Reg::Esi, body + 0x1c);
                    s::call(m, 0x47a9e2, &[body + 0x1c, number], None, |m| {
                        host.device(m, body + 0x1c, number)
                    })?;
                }
            }
            b"renderinstance" => {
                m.w32(frame + 0x14, number);
                s::external(m, 0x47ac01, &[number], Some(body + 0x2194), |m| {
                    host.bind(m, body + 0x2194, number)
                })?;
            }
            b"team" => {
                m.w32(frame + 0x14, number);
                m.w32(body + 0x242c, number);
            }
            b"viewport" | b"osd" | b"camera" | b"controllable" | b"leave" | b"control"
            | b"lost" => {
                return Err(format!(
                    "NativePlayerCommandBoundary: {} helper path is not ported",
                    String::from_utf8_lossy(token)
                ));
            }
            _ => {}
        }
        Ok(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    const WORDS: usize = 0x28e0 / 4;
    const TEXTS: [&str; 19] = [
        "device 0",
        "device -7",
        "device 2147483648",
        "device 4294967297",
        "device +3",
        "device junk",
        "  device\t9",
        "setcontrol",
        "setcontrol 55",
        "renderinstance -57343",
        "renderinstance 0",
        "renderinstance junk",
        "team -19",
        "team 2147483648",
        "team +1",
        "team 7tail",
        "uncontrol 3",
        "TEAM 2",
        "unrecognized 123",
    ];
    const SCENARIOS: [&str; 5] = [
        "null-raw",
        "no-envelope",
        "native",
        "script",
        "script-mutation",
    ];
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().expect("unsigned word")).expect("32-bit word")
    }
    fn heap(m: &Space) -> Value {
        json!((0..WORDS)
            .map(|i| m.r32(0x20000000 + i as u32 * 4))
            .collect::<Vec<_>>())
    }
    fn valid(row: &Value) -> bool {
        let complete = |v: &Value| {
            v.as_array().is_some_and(|a| {
                a.len() == WORDS
                    && a.iter()
                        .all(|v| v.as_u64().is_some_and(|n| n <= u32::MAX as u64))
            })
        };
        complete(&row["before"])
            && complete(&row["after"])
            && row["events"].as_array().is_some_and(|e| {
                e.iter().all(|e| {
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
        fn access(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
            self.record(m, "access", 4);
            Ok(if self.row["scenario"] == "no-envelope" {
                0
            } else {
                w(&self.row["layout"]["envelope"])
            })
        }
        fn script(&mut self, m: &mut Space, _: u32, _: u32, values: &[u32]) -> Result<(), String> {
            self.record(m, "script", 2 + values.len());
            if self.row["scenario"] == "script-mutation" {
                let b = w(&self.row["layout"]["body"]);
                m.w32(b + 0x2414, m.r32(b + 0x2414) ^ 0x99887766);
            }
            Ok(())
        }
        fn device(&mut self, m: &mut Space, input: u32, device: u32) -> Result<(), String> {
            s::function(m, &[input, device], None, |m| {
                self.record(m, "device", 2);
                m.w32(input + 4, device);
                Ok(())
            })
        }
        fn bind(&mut self, m: &mut Space, reference: u32, id: u32) -> Result<(), String> {
            self.record(m, "bind", 1);
            let raw = match id {
                0xffff2001 => w(&self.row["layout"]["raw"]),
                0xffff2002 => w(&self.row["layout"]["other"]),
                _ => 0,
            };
            crate::native_part::bind_reference(m, reference, raw);
            Ok(())
        }
    }
    #[test]
    fn original_boot_commands_match_complete_heap_and_actual_callback_abi() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-player-command-original.json"))
                .expect("run native_player_command_probe.py"),
        )
        .unwrap();
        assert_eq!(fixture["schema"], 1);
        for (field, path) in [
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
            (
                "producer_sha256",
                "re/oracle/native_player_command_probe.py",
            ),
            ("oracle_sha256", "re/oracle/oracle.py"),
            (
                "binding_collaborator_sha256",
                "re/oracle/native_player_probe.py",
            ),
        ] {
            assert_eq!(
                fixture[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = fixture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 200);
        let actual = rows
            .iter()
            .map(|r| {
                (
                    w(&r["fill"]),
                    r["scenario"].as_str().unwrap(),
                    r["command"].as_str().unwrap(),
                    w(&r["event"]),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected = std::collections::BTreeSet::new();
        for fill in [0, 0xa5] {
            for scenario in SCENARIOS {
                for command in TEXTS {
                    expected.insert((fill, scenario, command, 0x10));
                }
            }
            for event in [0, 2, 8, 0x1c, u32::MAX] {
                expected.insert((fill, "native", "unused", event));
            }
        }
        assert_eq!(actual, expected);
        //Unrecovered branches must remain visible to the integrating caller.
        for name in [
            "viewport",
            "osd",
            "camera",
            "controllable",
            "leave",
            "control",
            "lost",
        ] {
            let mut m = Space::new();
            let body = m.alloc(0x2510);
            let k = m.alloc(0x50);
            let pointer = m.alloc(128);
            for (i, byte) in name.bytes().chain([0]).enumerate() {
                m.w8(pointer + i as u32, byte);
            }
            let mut host = Controlled {
                row: &rows[0],
                events: vec![],
            };
            assert!(command(&mut m, k, body, 0, 0x10, pointer, &mut host)
                .unwrap_err()
                .contains("helper path"));
        }
        let mut m = Space::new();
        let mut host = Controlled {
            row: &rows[0],
            events: vec![],
        };
        assert!(command(&mut m, 0, 0, 0, 1, 0, &mut host)
            .unwrap_err()
            .contains("collision event1"));
        let mut broken = rows[0].clone();
        broken["after"].as_array_mut().unwrap().pop();
        assert!(!valid(&broken));
        let mut broken = rows[0].clone();
        broken["before"][0] = json!(0x100000000u64);
        assert!(!valid(&broken));
        for (i, row) in rows.iter().enumerate() {
            assert!(valid(row), "case{i} complete machine-word observations");
            let mut m = Space::new();
            for (n, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + n as u32 * 4, w(value));
            }
            let pointer = w(&row["layout"]["command"]);
            for (n, byte) in row["command"]
                .as_str()
                .unwrap()
                .bytes()
                .chain([0])
                .enumerate()
            {
                m.w8(pointer + n as u32, byte);
            }
            let mut host = Controlled {
                row,
                events: vec![],
            };
            let result = command(
                &mut m,
                w(&row["layout"]["k"]),
                w(&row["layout"]["body"]),
                0x13572468,
                w(&row["event"]),
                pointer,
                &mut host,
            )
            .unwrap();
            assert_eq!(result, w(&row["result"]), "case{i} return");
            assert_eq!(
                host.events.len(),
                row["events"].as_array().unwrap().len(),
                "case{i} callback count"
            );
            for (call, (a, b)) in host
                .events
                .iter()
                .zip(row["events"].as_array().unwrap())
                .enumerate()
            {
                for field in ["name", "this", "args", "esp", "return_pc", "heap"] {
                    assert_eq!(a[field], b[field], "case{i} call{call} {field}");
                }
            }
            assert_eq!(heap(&m), row["after"], "case{i} full caller heap");
            assert_eq!(row["esp_delta"], 16);
            assert_eq!(s::sp(&m), 0x7feffef0);
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
