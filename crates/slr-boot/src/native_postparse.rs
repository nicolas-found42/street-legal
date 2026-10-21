//! Configured car template finalization from4472ef..44751d and46e400.
//! Resource lookup/bind is delegated to the live cfg host.
use slr_physics::{
    cfg::{self, Arg, ExtCall, Host, This},
    space::Space,
};

/// The cfg dispatcher and ownership of its input buffer are caller duties.
pub fn configure_finish(m: &mut Space, k: u32, host: &mut dyn Host) -> Result<(), String> {
    let p = m.r32(k + 0x2e48);
    if p == 0 {
        return Err("NativePostparseBoundary: missing car parameter block".into());
    }
    let count = m.r32(k + 0x1f40) as i32;
    if count > 16 {
        return Err("NativePostparseBoundary: wheel count exceeds sixteen records".into());
    }
    for (count_offset, base, index, gain) in
        [(0x338, 0xf8, 3, 0x3951b717), (0x580, 0x340, 2, 0x39d1b717)]
    {
        if m.r32(p + count_offset) == 0 {
            let resource = host.call(
                m,
                &ExtCall {
                    f: cfg::EXT_LOOKUP,
                    this: This::None,
                    a: vec![Arg::S("sound.rpk".into()), Arg::U(index)],
                },
            );
            m.w32(p + count_offset, 1);
            host.call(
                m,
                &ExtCall {
                    f: cfg::EXT_BIND,
                    this: This::Addr(p + base),
                    a: vec![Arg::U(resource), Arg::U(6), Arg::U(0)],
                },
            );
            for (offset, value) in [
                (0x10, gain),
                (0x14, 0x3f800000),
                (0x18, 0x49742400),
                (0x1c, 0x3f800000),
                (0x20, 0x3f800000),
            ] {
                m.w32(p + base + offset, value);
            }
        }
    }
    if m.r32(p + 0x85c) == 0 && m.r32(p + 0x86c) == 0 {
        let target = m.r32(p + 0x850);
        crate::native_part::bind_reference(m, p + 0x864, target);
        m.w32(p + 0x880, m.r32(p + 0x878));
    }
    transform_slots(m, k)?;
    if m.r32(k + 0x1150) != 0 {
        m.w32(k + 0x2e4c, m.r32(k + 0x2e4c) | 1);
    }
    let index = m.r32(k + 0xff0) as i32;
    if index >= 0 {
        if index >= 32 {
            return Err(
                "NativePostparseBoundary: selected body index outside original template".into(),
            );
        }
        for i in 0..3 {
            m.w32(
                k + 0x1eb8 + i * 4,
                m.r32(k + 0x128 + index as u32 * 0x74 + i * 4),
            );
        }
    }
    for index in 0..count.max(0) as u32 {
        for (offset, scale) in [(0x1f64, 0xd8), (0x1f68, 0xdc)] {
            let address = k + offset + index * 0xc8;
            m.wf(address, m.rf(p + scale) * m.rf(address));
        }
    }
    m.w32(p + 0xd8, 0x3f800000);
    m.w32(p + 0xdc, 0x3f800000);
    m.w32(k + 0x120, m.r32(k + 0x120).wrapping_add(1));
    Ok(())
}

/// `FUN_0046e570` tail after the cfg table: `FUN_0046e400` converts the
/// configured slots into the selected body's frame, then `K+0x120` increments.
/// The part template has no parameter block (`K+0x2e48`), so the car
/// `configure_finish` sound/curve/wheel stages do not run.
pub fn configure_finish_part(m: &mut Space, k: u32, _host: &mut dyn Host) -> Result<(), String> {
    transform_slots(m, k)?;
    m.w32(k + 0x120, m.r32(k + 0x120).wrapping_add(1));
    Ok(())
}

/// Original46e400: convert configured slots into the selected body's frame.
/// Matrix padding words are not meaningful float coefficients and are retained;
/// original54eab0 copies unspecified stack padding into those three words.
fn transform_slots(m: &mut Space, k: u32) -> Result<(), String> {
    if m.r32(k + 0xfa8) == 0 {
        return Ok(());
    }
    let index = m.r32(k + 0xff0) as i32;
    if index >= 32 {
        return Err(
            "NativePostparseBoundary: selected body index outside original template".into(),
        );
    }
    let frame = if index < 0 {
        k + 0x1000
    } else {
        k + 0x128 + index as u32 * 0x74
    };
    let body = m.r32(frame + 0x40);
    if body == 0 {
        return Ok(());
    }
    if index < 0 {
        m.w32(body + 0x9c, m.r32(body + 0x9c) | 4);
    }
    let mut origin = [m.rf(frame), m.rf(frame + 4), m.rf(frame + 8)];
    if m.rf(body + 0x14) > 0.0 {
        let definition = m.r32(body + 0x78);
        if definition == 0 {
            return Err("NativePostparseBoundary: massive body has no definition".into());
        }
        for (i, value) in origin.iter_mut().enumerate() {
            *value += m.rf(definition + 0x40 + i as u32 * 4);
        }
    }
    let matrix: Vec<f32> = (0..12).map(|i| m.rf(frame + 0xc + i * 4)).collect();
    let sentinel = m.r32(k + 0x115c);
    if sentinel == 0 {
        return Err("NativePostparseBoundary: missing configured slot sentinel".into());
    }
    let mut slot = if m.r32(sentinel + 4) == 0 {
        0
    } else {
        sentinel
    };
    let mut visited = std::collections::HashSet::new();
    while slot != 0 {
        if !visited.insert(slot) {
            return Err("NativePostparseBoundary: cyclic configured slot chain".into());
        }
        let position = [
            m.rf(slot + 0xc) - origin[0],
            m.rf(slot + 0x10) - origin[1],
            m.rf(slot + 0x14) - origin[2],
        ];
        for row in 0..3 {
            let value = (matrix[row * 4 + 1] * position[1] + matrix[row * 4 + 2] * position[2])
                + matrix[row * 4] * position[0];
            m.wf(slot + 0xc + row as u32 * 4, value);
        }
        //54eab0 postmultiplies the slot basis by the body's basis. Keep the
        //original per-coefficient expression ordering (PC24 after each op).
        let a: Vec<f32> = (0..12).map(|i| m.rf(slot + 0x18 + i * 4)).collect();
        let b = &matrix;
        let out = [
            (b[1] * a[4] + b[0] * a[0]) + b[2] * a[8],
            (a[9] * b[2] + b[1] * a[5]) + a[1] * b[0],
            (a[10] * b[2] + a[6] * b[1]) + b[0] * a[2],
            (b[4] * a[0] + b[6] * a[8]) + b[5] * a[4],
            (b[6] * a[9] + b[5] * a[5]) + b[4] * a[1],
            (b[4] * a[2] + a[10] * b[6]) + a[6] * b[5],
            (b[8] * a[0] + b[10] * a[8]) + b[9] * a[4],
            (b[10] * a[9] + b[9] * a[5]) + b[8] * a[1],
            (b[8] * a[2] + a[10] * b[10]) + a[6] * b[9],
        ];
        for (i, value) in out.into_iter().enumerate() {
            m.wf(slot + 0x18 + (i / 3 * 16 + i % 3 * 4) as u32, value);
        }
        let next = m.r32(slot + 4);
        slot = if next != 0 && m.r32(next + 4) != 0 {
            next
        } else {
            0
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ControlledHost {
        calls: Vec<serde_json::Value>,
    }
    impl Host for ControlledHost {
        fn call(&mut self, m: &mut Space, call: &ExtCall) -> u32 {
            let args: Vec<serde_json::Value> = call
                .a
                .iter()
                .map(|a| match a {
                    Arg::U(v) => serde_json::json!(v),
                    Arg::S(v) => serde_json::json!(v),
                })
                .collect();
            let this = match call.this {
                This::Addr(p) => Some(p),
                _ => None,
            };
            self.calls
                .push(serde_json::json!([format!("0x{:x}", call.f), this, args]));
            match call.f {
                cfg::EXT_LOOKUP => args[1].as_u64().unwrap() as u32 * 5 + 7,
                cfg::EXT_BIND => {
                    m.w32(this.unwrap() + 8, args[0].as_u64().unwrap() as u32);
                    0
                }
                _ => panic!("unexpected postparse collaborator"),
            }
        }
    }
    #[test]
    fn original_postparse_sound_geometry_wheels_and_loadcount() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-postparse-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-postparse-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let mut m = Space::new();
            let base = slr_physics::space::HEAP_BASE;
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(base + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            let mut host = ControlledHost { calls: vec![] };
            configure_finish(&mut m, row["k"].as_u64().unwrap() as u32, &mut host).unwrap();
            let ignored: Vec<u32> = row["ignored"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            for (i, value) in row["after"].as_array().unwrap().iter().enumerate() {
                let address = base + i as u32 * 4;
                if !ignored.contains(&address) {
                    assert_eq!(
                        m.r32(address) as u64,
                        value.as_u64().unwrap(),
                        "mode={} defaults={} address={address:#x}",
                        row["mode"],
                        row["defaults"]
                    );
                }
            }
            let expected: Vec<_> = row["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| {
                    serde_json::json!([
                        call["f"],
                        if call["f"] == "0x538440" {
                            serde_json::Value::Null
                        } else {
                            call["this"].clone()
                        },
                        call["a"]
                    ])
                })
                .collect();
            assert_eq!(host.calls, expected);
        }
    }
}
