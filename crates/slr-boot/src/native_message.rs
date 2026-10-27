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
    if class == "player" && vtable == crate::native_player::VTABLE {
        return crate::native_player::get_info(m, template, object, message, argument);
    }
    if class == "cursor" && vtable == 0x5f11e4 {
        return crate::native_cursor::get_info(m, template, object, message, argument);
    }
    let values = match (class, vtable) {
        ("car", 0x5f0f18) => Some([1, 1, 1]),
        ("cursor", 0x5f11e4) => Some([0, 0, 1]),
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
        crate::native_player::VTABLE => {
            crate::native_player::get_info(m, template, object, message, argument)
        }
        0x5f0f18 => match message {
            7 => Ok(5),
            _ => Err(format!("NativeMessageBoundary: car45c510 query{message}")),
        },
        0x5f0f60 => match message {
            11 => part_mass(m, template, object, argument),
            60 => part_variant(m, template, object, argument),
            _ => part_scalar_query(m, template, object, message, argument)
                .ok_or_else(|| format!("NativeMessageBoundary: part4769e0 query{message}")),
        },
        vtable => Err(format!(
            "NativeMessageBoundary: native template query vtable {vtable:#x} unavailable"
        )),
    }
}
/// Actual resource access shared by the part's physics/render reference queries.
/// 48aea0 uses quality zero; 48c8c0 uses quality one. AccessAdapter executes the
/// recovered virtual minimum/request/get-data methods rather than bypassing load
/// state. The identity projection is synchronized when this is a live resource.
fn access_resource(m: &mut Space, raw: u32, flags: u32, quality: u32) -> Result<u32, String> {
    let payload = crate::native_resource_load::access(
        m,
        raw,
        crate::native_resource_load::Access {
            flags,
            quality,
            priority: 0,
            time: 0,
        },
        &mut crate::native_assets::AccessAdapter,
    )?;
    if crate::native_body::logical_resource(raw).is_some() {
        crate::native_assets::synchronize_loaded(m, raw)?;
    }
    Ok(payload)
}

/// Original29350 construction of GameLogic's deferred destroy list (+70).
/// Called once with the other GameLogic globals before runtime queries.
pub fn initialize_release_queue(m: &mut Space) {
    let list = 0x636338 + 0x70;
    for (offset, value) in [
        (0, 0x5f09b0),
        (4, 0x5f09ac),
        (8, list + 0x10),
        (12, 0),
        (16, 0x5f09ac),
        (20, 0),
        (24, list + 4),
    ] {
        m.w32(list + offset, value);
    }
    m.w32(0x636338 + 0xd4, 0);
}

/// Original427620's raw FIFO node and retained weak reference. GameLogic's
/// destroy list at +70 must already contain its original29350 sentinels.
/// The caller's reference remains bound; this queue retains a separate link.
pub(crate) fn defer_destroy(m: &mut Space, reference: u32) -> Result<(), String> {
    const GAME: u32 = 0x636338;
    let raw = m.r32(reference + 12);
    if raw != 0 && m.r32(raw + 0x4c) == 1 {
        access_resource(m, raw, 0x80000000, 1f32.to_bits())?;
    }
    let node = m.alloc(0x1c);
    m.w32(node, 0x5f09b4);
    crate::native_part::bind_reference(m, node + 12, m.r32(reference + 12));
    let tail = m.r32(GAME + 0x88);
    m.w32(tail + 4, node);
    m.w32(node + 8, tail);
    m.w32(node + 4, GAME + 0x80);
    m.w32(GAME + 0x88, node);
    m.w32(GAME + 0xd4, m.r32(GAME + 0xd4).wrapping_add(1));
    if let Some(logical) = crate::native_body::logical_resource(m.r32(node + 24)) {
        crate::factory::schedule_destroy_raw(logical, node)?;
    }
    Ok(())
}

/// The45f8b0 retained-reference node destructor, called at4277f0's cleanup
/// point after its associated logical resource has been processed. Resource
/// destruction may already have invalidated this node's reference in place.
pub fn retire_release_node(m: &mut Space, node: u32) {
    if m.r32(node + 24) != 0 {
        crate::native_part::bind_reference(m, node + 12, 0);
    } else {
        m.w32(node + 20, 0);
    }
    m.w32(node, 0x5f09ac);
    let previous = m.r32(node + 8);
    let next = m.r32(node + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
    m.w32(0x636338 + 0xd4, m.r32(0x636338 + 0xd4).wrapping_sub(1));
}

/// Original476b2e and complete48aea0 reference access. A successfully loaded
/// resource whose physics envelope has no entity is queued for destruction.
fn part_mass(m: &mut Space, template: u32, body: u32, output: u32) -> Result<u32, String> {
    let reference = body + 0x1c;
    let raw = m.r32(reference + 12);
    let entity = if m.r32(reference + 8) == 0 || raw == 0 {
        0
    } else {
        let payload = access_resource(m, raw, 0xa0000000, 0)?;
        if payload == 0 {
            0
        } else {
            let entity = m.r32(payload + 0x84);
            if entity == 0 && m.r32(reference + 8) != 0 && m.r32(reference + 12) != 0 {
                defer_destroy(m, reference)?;
            }
            entity
        }
    };
    if entity == 0 {
        return Ok(m.r32(template + 0x1150).wrapping_sub(m.r32(body + 0x74)));
    }
    let definition = m.r32(entity + 0x5c);
    let mass = m.r32(m.r32(definition + 0x78) + 12);
    if output != 0 {
        m.w32(output, mass);
    }
    let scaled = slr_x87::X87::from_f32(f32::from_bits(mass)).mul(
        slr_x87::X87::from_f32(m.rf(0x5f11d8)),
        slr_x87::Precision::F32,
        slr_x87::Rounding::NearestEven,
    );
    Ok(1u32.wrapping_sub(scaled.to_i64(slr_x87::Rounding::Zero) as u32))
}

/// Original477088, including every render link selected by the body's render
/// index array, and48c8c0's full access-before-color-store sequence. The signed
/// IDIV remainder is retained even for a negative variant.
pub(crate) fn part_variant(
    m: &mut Space,
    template: u32,
    body: u32,
    variant: u32,
) -> Result<u32, String> {
    if variant != 0 {
        m.w32(body + 0x58, variant);
    }
    let mut i = 0u32;
    while (i as i32) < m.r32(body + 0x54) as i32 {
        let index = m.r32(m.r32(body + 0x50) + i * 4);
        let descriptor = template
            .wrapping_add(0x128)
            .wrapping_add(index.wrapping_mul(0x74));
        let variants = m.r32(descriptor + 0x5c) as i32;
        if variants > 0 {
            let selected = (m.r32(body + 0x58) as i32) % variants;
            let colors = m.r32(descriptor + 0x60);
            let color = m.r32(colors.wrapping_add_signed(selected.wrapping_mul(4)));
            let link = m.r32(body + 0x4c) + i * 16;
            let raw = m.r32(link + 12);
            if raw != 0 {
                let payload = access_resource(m, raw, 0x80000001, 1f32.to_bits())?;
                if payload != 0 {
                    m.w32(payload + 0xcc, color);
                }
            }
        }
        i += 1;
    }
    Ok(m.r32(body + 0x58))
}

/// Remaining scalar/reference branches after the live physics/render dispatch.
/// `None` marks the still-unported installation/slot-compatibility messages;
/// the executable's default jump-table branch returns `Some(0)`.
///
/// The caller must supply a postparsed template (`template+120 != 0`). The
/// original initializes that template before entering its message switch.
fn part_scalar_query(
    m: &mut Space,
    template: u32,
    body: u32,
    message: u32,
    argument: u32,
) -> Option<u32> {
    use crate::native_part::bind_reference;
    match message {
        1 | 6 | 23 | 43 => {
            let (value, resource) = match message {
                1 => (m.r32(body + 0x24), m.r32(body + 0x28)),
                6 => (m.r32(template + 0xc), m.r32(template + 0x10)),
                23 => {
                    let render = m.r32(body + 0x4c);
                    (m.r32(render + 8), m.r32(render + 12))
                }
                _ => (m.r32(body + 0xc8), m.r32(body + 0xcc)),
            };
            if argument != 0 {
                bind_reference(m, argument, resource);
            }
            Some(value)
        }
        7 => Some(9),
        55 => Some(m.r32(template + 0x114c)),
        63 => Some(m.r32(body + 0xac)),
        39 => Some(m.r32(template + 0x1150).wrapping_sub(m.r32(body + 0x74))),
        47 => {
            if argument == 0 {
                return Some(0);
            }
            let id = m.r32(argument);
            let mut slot = m.r32(body + 0x78);
            let mut node = list_first(m, template + 0x1154);
            while node != 0 {
                if m.r32(node + 0x48) == id {
                    bind_reference(m, argument + 4, m.r32(slot + 0x24));
                    return Some(m.r32(argument + 0xc));
                }
                node = list_next(m, node);
                slot += 0x68;
            }
            // Build 606 dereferences NULL+24 if the requested id is absent;
            // such a request is outside valid constructed-part query states.
            None
        }
        49 => {
            if argument == 0 {
                return Some(0);
            }
            let old_id = m.r32(argument + 8);
            if m.r32(argument + 12) != 0 {
                bind_reference(m, argument, 0);
            } else {
                m.w32(argument + 8, 0);
            }
            let mut node = list_first(m, body + 0xd4);
            if node == 0 {
                return Some(m.r32(argument + 8));
            }
            if old_id == 0 {
                bind_reference(m, argument, m.r32(node + 0x18));
            } else {
                while node != 0 {
                    let matched = old_id == m.r32(node + 0x14);
                    // The original retains the raw next pointer at the match,
                    // then checks its sentinel before taking the embedded ref.
                    let next = m.r32(node + 4);
                    if matched {
                        if next != 0 && m.r32(next + 4) != 0 {
                            bind_reference(m, argument, m.r32(next + 0x18));
                        }
                        break;
                    }
                    node = list_next(m, node);
                }
            }
            Some(m.r32(argument + 8))
        }
        15 => {
            // `FMUL` under the game PC24, then 5d6750's truncate-to-i64;
            // EAX is the low word, including out-of-i32-range finite values.
            let product = slr_x87::X87::from_f32(m.rf(body + 0xb4)).mul(
                slr_x87::X87::from_f32(m.rf(0x5f09c8)),
                slr_x87::Precision::F32,
                slr_x87::Rounding::NearestEven,
            );
            Some(product.to_i64(slr_x87::Rounding::Zero) as u32)
        }
        73 => Some(m.r32(body + 0x5c)),
        40 | 41 | 58 | 71 | 72 => None,
        _ => Some(0),
    }
}

/// `4293a0`: lists contain an end sentinel whose `next` is NULL.
fn list_first(m: &Space, list: u32) -> u32 {
    let node = m.r32(list + 8);
    if m.r32(node + 4) != 0 {
        node
    } else {
        0
    }
}

/// `40cfc0`: an end sentinel is never returned as a live node.
fn list_next(m: &Space, node: u32) -> u32 {
    let next = m.r32(node + 4);
    if next != 0 && m.r32(next + 4) != 0 {
        next
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_part_queries_match_every_memory_change() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/oracle/native-part-query-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_query_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for pair in data["constants"]
                .as_array()
                .unwrap()
                .iter()
                .chain(row["initial"].as_array().unwrap())
            {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            let n = |key: &str| row[key].as_u64().unwrap() as u32;
            m.set_heap_ptr(n("heap_end"));
            m.begin_journal();
            assert_eq!(
                query(
                    &mut m,
                    n("template"),
                    n("body"),
                    true,
                    n("message"),
                    n("argument")
                )
                .unwrap(),
                n("eax"),
                "{} return",
                row["name"]
            );
            let changes: Vec<_> = row["changes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_u64().unwrap() as u32,
                        pair[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            assert_eq!(m.end_journal(), changes, "{} memory", row["name"]);
            assert_eq!(m.heap_ptr(), n("heap_after"), "{} allocation", row["name"]);
        }
    }
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
