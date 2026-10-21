//! Bounded native class query dispatch, ahead of any script fallback.
use slr_physics::space::Space;

/// Original car45c510 template payload: message7 (jump45c53a) returns category5.
/// Messages8/9/0x1c take the shared default branch45d6c4 returning1
/// (executed rows: re/out/boot/native-class-query-original.json).
///
/// Other recovered payload queries are executed over the same controlled
/// layout: cursor4625f0 (vtable5f11e4) 0/0/1, player47b220 (5f12f0) 1/0/1,
/// trigger47bb70 (5f133c) 0/0/0, button4379d0 (5f0e10) 0/0/0, part4769e0
/// (5f0f60) 0/0/0 (part still needs a real constructed payload for the
/// executed rows; its dispatch default branch returns0).
pub fn template_query(
    m: &mut Space,
    template: u32,
    class: &str,
    message: u32,
    object: u32,
    argument: u32,
) -> Result<u32, String> {
    let vtable = m.r32(template);
    let values = match (class, vtable) {
        ("car", 0x5f0f18) => Some([1, 1, 1]),
        ("cursor", 0x5f11e4) => Some([0, 0, 1]),
        ("player", 0x5f12f0) => Some([1, 0, 1]),
        ("trigger", 0x5f133c) => Some([0, 0, 0]),
        ("button", 0x5f0e10) => Some([0, 0, 0]),
        ("part", 0x5f0f60) => Some([0, 0, 0]),
        _ => None,
    };
    let Some(values) = values else {
        return Err(format!(
            "NativeMessageBoundary: template query for {class} vtable{vtable:#x}"
        ));
    };
    match message {
        7 if class == "car" => query(m, template, object, true, message, argument),
        8 => Ok(values[0]),
        9 => Ok(values[1]),
        0x1C => Ok(values[2]),
        _ => Err(format!(
            "NativeMessageBoundary: {class} template query{message}"
        )),
    }
}

/// Original native payload query dispatch `FUN_00426470`'s direct branch:
/// the template vtable selects the class handler (`5f0f18` car45c510,
/// `5f0f60` part4769e0, `5f11e4` cursor, `5f12f0` player, `5f133c` trigger,
/// `5f0e10` button). `object` is the live engine body for a constructed
/// instance.
pub fn query(
    m: &mut Space,
    template: u32,
    object: u32,
    direct: bool,
    message: u32,
    argument: u32,
) -> Result<u32, String> {
    if !direct {
        return Err(
            "NativeMessageBoundary: indirect426470 query requires live instance payload".into(),
        );
    }
    match m.r32(template) {
        0x5f0f18 => match message {
            7 => Ok(5),
            _ => Err(format!("NativeMessageBoundary: car45c510 query{message}")),
        },
        0x5f0f60 => part_query(m, template, object, message, argument)
            .ok_or_else(|| format!("NativeMessageBoundary: part4769e0 query{message}")),
        vtable => Err(format!(
            "NativeMessageBoundary: native template query vtable {vtable:#x} unavailable"
        )),
    }
}
/// Original part4769e0 (`0x477100` jump table over `message-1`, `0x477150`
/// index bytes): the constructed part body answers the runtime's information
/// queries. Returns `(handled, value)`; an argument that the original writes
/// through is reported as `None` because the ported caller passes a reference.
///
/// | msg | target | value |
/// |---|---|---|
/// | 1 | `476a3c` | category ref |
/// | 6 | `476ad4` | resource id |
/// | 7 | `476a1d` | 9 (interaction kind) |
/// | 11 | `476b2e` | mass-derived flag |
/// | 15 | `477069` | `round(body+0xb4 * 0x5f09c8)` |
/// | 23 | `476af2` | first render reference id |
/// | 39 | `476b71` | `K+0x1150 - body+0x74` |
/// | 40 | `476cac` | slot lookup |
/// | 41 | `476d20` | slot scan |
/// | 43 | `476f40` | `body+0xc8` |
/// | 47 | `476a5e` | slot scan by id |
/// | 49 | `476f79` | clear reference |
/// | 55 | `476a2c` | `K+0x114c` |
/// | 58 | `476b86` | `0x545fc0` slot string |
/// | 60 | `477088` | `body+0x58` and render selection |
/// | 63 | `476b1a` | `body+0xac` |
/// | 71/72/73 | `476da0`/`476e7f`/`476f68` | box/extent queries |
///
/// The default branch (`476d87`) returns 0.
pub fn part_query(
    m: &mut Space,
    template: u32,
    body: u32,
    message: u32,
    argument: u32,
) -> Option<u32> {
    match message {
        7 => Some(9),
        55 => Some(m.r32(template + 0x114c)),
        63 => Some(m.r32(body + 0xac)),
        43 => Some(m.r32(body + 0xc8)),
        39 => Some(m.r32(template + 0x1150).wrapping_sub(m.r32(body + 0x74))),
        6 => Some(m.r32(body + 0x24)),
        // 47 walks the template's 0x1154 slot list for the id at +0x48 and
        // returns that slot's +0xc.
        47 => slot_by_id(m, template, m.r32(body + 0x78), argument),
        // 60/40/41/58/71/72/73 write through their argument or need the
        // original slot-string and box collaborators; keep them explicit.
        _ if argument == 0 => None,
        23 => Some(m.r32(m.r32(body + 0x4c) + 8)),
        15 => {
            let value = m.rf(body + 0xb4) * f32::from_bits(m.r32(0x5f09c8));
            // 5d6750 rounds to the nearest integer into EAX.
            Some(value.round() as i32 as u32)
        }
        11 => {
            // 48aea0: the first child entity of the body's physics reference.
            let entity = m.r32(body + 0x1c);
            let child = if entity == 0 { 0 } else { m.r32(entity + 0x84) };
            if child == 0 {
                return Some(m.r32(template + 0x1150).wrapping_sub(m.r32(body + 0x74)));
            }
            let definition = m.r32(m.r32(child + 0x84) + 0x5c);
            let mass = m.rf(m.r32(definition + 0x78) + 0xc) * f32::from_bits(m.r32(0x5f11d8));
            // 5d6750 returns the rounded value; `1 - result` is the flag.
            Some(1u32.wrapping_sub(mass.round() as i32 as u32))
        }
        _ => None,
    }
}

/// `FUN_004293a0` over the template's `0x1154` list: the node whose `+0x48`
/// equals `id`, with the loop bounded by the configured slot count.
fn slot_by_id(m: &Space, template: u32, first: u32, id: u32) -> Option<u32> {
    let count = m.r32(template + 0x1150);
    let mut node = m.r32(template + 0x1154);
    for _ in 0..count {
        if node == 0 {
            break;
        }
        if m.r32(node + 0x48) == id {
            return Some(m.r32(node + 0xc));
        }
        let next = m.r32(node + 4);
        node = if next != 0 && m.r32(next + 4) != 0 {
            next
        } else {
            0
        };
    }
    let _ = first;
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_template_queries_8_9_28_match_per_class_rows() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-class-query-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_class_query_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut part_vtable = 0u32;
        for entry in data["vtables"]["part"].as_array().unwrap() {
            part_vtable = entry["vtable"].as_u64().unwrap() as u32;
        }
        assert_eq!(
            part_vtable, 0x5f0f60,
            "part payload vtable recovered from ctor store"
        );
        for row in data["queries"].as_array().unwrap() {
            let class = row["cls"].as_str().unwrap();
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            m.w32(template, row["vtable"].as_u64().unwrap() as u32);
            assert_eq!(
                template_query(
                    &mut m,
                    template,
                    class,
                    row["kind"].as_u64().unwrap() as u32,
                    0,
                    0,
                )
                .unwrap(),
                row["eax"].as_u64().unwrap() as u32,
                "{} kind{}",
                class,
                row["kind"]
            );
        }
    }
    #[test]
    fn original_car_query7_category_and_optional_argument_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-message-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_message_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let resource = m.alloc(0x100);
            let output = m.alloc(16);
            m.w32(template, 0x5f0f18);
            m.w32(template + 0xc, row["id"].as_u64().unwrap() as u32);
            m.w32(template + 0x10, resource);
            m.w32(resource + 0x50, 0xffff0080);
            let result = query(
                &mut m,
                template,
                object,
                true,
                7,
                if row["output"].as_bool().unwrap() {
                    output
                } else {
                    0
                },
            )
            .unwrap();
            assert_eq!(result, row["result"].as_u64().unwrap() as u32);
            for (i, value) in row["words"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    value.as_u64().unwrap() as u32,
                    "word{i}"
                );
            }
        }
    }
}
