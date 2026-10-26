//! Original callback registrations over intrusive nodes with embedded weak refs.
//! Registration, retirement and traversal stay live across callbacks; a snapshot
//! of logical resource IDs cannot preserve later-node mutations or destruction.
use slr_physics::space::Space;

pub const CONTEXT: u32 = 0x636338;

fn layout(kind: u32) -> Option<(u32, u32)> {
    match kind {
        8 => Some((0, 0xc4)),
        9 => Some((0x1c, 0xc8)),
        0x1c => Some((0x38, 0xcc)),
        _ => None,
    }
}

///428fd0 headers in the game context, plus the three registration counts.
pub fn initialize(m: &mut Space, context: u32) {
    for kind in [8, 9, 0x1c] {
        let (offset, count) = layout(kind).unwrap();
        let head = context + offset;
        for (at, value) in [
            (0, 0x5f09a8),
            (4, 0x5f09ac),
            (8, head + 16),
            (12, 0),
            (16, 0x5f09ac),
            (20, 0),
            (24, head + 4),
        ] {
            m.w32(head + at, value);
        }
        m.w32(context + count, 0);
    }
}

pub fn reset(m: &mut Space, context: u32) {
    for kind in [8, 9, 0x1c] {
        let (offset, _) = layout(kind).unwrap();
        let mut node = crate::native_install::first(m, context + offset);
        while node != 0 {
            let next = crate::native_install::next(m, node);
            destroy(m, node);
            node = next;
        }
    }
    initialize(m, context);
}

/// exe: FUN_00427370
pub fn register(m: &mut Space, context: u32, resource: u32, kind: u32) {
    let Some((offset, count)) = layout(kind) else {
        return;
    };
    let node = m.malloc(0x1c);
    m.w32(node, 0x5f09b4);
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
    for at in [12, 16, 20, 24] {
        m.w32(node + at, 0);
    }
    crate::native_part::bind_reference(m, node + 12, resource);
    let head = context + offset;
    let tail = m.r32(head + 24);
    m.w32(tail + 4, node);
    m.w32(node + 8, tail);
    m.w32(node + 4, head + 16);
    m.w32(head + 24, node);
    m.w32(context + count, m.r32(context + count).wrapping_add(1));
}

///4274e0 retires only the first matching ID. It leaves the list node and
/// count in place until the corresponding callback pass destroys it.
/// exe: FUN_004274e0
pub fn unregister(m: &mut Space, context: u32, id: u32, kind: u32) {
    let Some((offset, _)) = layout(kind) else {
        return;
    };
    let mut node = crate::native_install::first(m, context + offset);
    while node != 0 {
        if m.r32(node + 20) == id {
            if m.r32(node + 24) != 0 {
                crate::native_part::bind_reference(m, node + 12, 0);
            } else {
                m.w32(node + 20, 0);
            }
            break;
        }
        node = crate::native_install::next(m, node);
    }
}

fn destroy(m: &mut Space, node: u32) {
    crate::native_part::bind_reference(m, node + 12, 0);
    //429150 base destructor; freeing storage does not change retained bytes
    // in the oracle's documented bump allocator scenario.
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

///426f60 traversal. Control426c90 accepts non-type1 registrations too;
/// animation427035 retains its type1 predicate. Snapshot next per node.
/// Returns the blocks passed to free in the original allocator scenario.
/// exe: FUN_00426f60
pub fn walk(
    m: &mut Space,
    context: u32,
    kind: u32,
    mut callback: impl FnMut(&mut Space, u32) -> Result<(), String>,
) -> Result<Vec<u32>, String> {
    let mut retired = vec![];
    let Some((offset, count)) = layout(kind) else {
        return Ok(retired);
    };
    let mut node = crate::native_install::first(m, context + offset);
    while node != 0 {
        let next = crate::native_install::next(m, node);
        if m.r32(node + 20) == 0 {
            destroy(m, node);
            retired.push(node);
            m.w32(context + count, m.r32(context + count).wrapping_sub(1));
        } else {
            let resource = m.r32(node + 24);
            if kind == 9 || (resource != 0 && (kind == 8 || m.r32(resource + 0x4c) == 1)) {
                callback(m, node + 12)?;
            }
        }
        node = next;
    }
    Ok(retired)
}

pub fn contains(m: &Space, context: u32, id: u32, kind: u32) -> bool {
    let Some((offset, _)) = layout(kind) else {
        return false;
    };
    let mut node = crate::native_install::first(m, context + offset);
    while node != 0 {
        if id != 0 && m.r32(node + 20) == id {
            return true;
        }
        node = crate::native_install::next(m, node);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};

    #[test]
    fn original_registration_retirement_and_drive_traversal_match_complete_heap() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-callbacks-original.json"))
                .expect("run native_callbacks_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_callbacks_probe.py"),
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
        assert_eq!(data["fpu_cw"], 0x7f);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 36);
        for (index, row) in rows.iter().enumerate() {
            let word = |name: &str| row[name].as_u64().unwrap() as u32;
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            let mut initialized = Space::new();
            initialize(&mut initialized, word("context"));
            for base in [0, 0x1c, 0x38] {
                for at in (0..0x1c).step_by(4) {
                    assert_eq!(
                        initialized.r32(word("context") + base + at),
                        before[((base + at) / 4) as usize].as_u64().unwrap() as u32,
                        "original428fd0 header initialization"
                    );
                }
            }
            for (i, v) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(0x20000000 + before.len() as u32 * 4);
            let kind = word("kind");
            let context = word("context");
            let compare = |m: &Space, expected: &Value| {
                let expected = expected.as_array().unwrap();
                assert_eq!(
                    (m.heap_ptr() - 0x20000000).div_ceil(4) as usize,
                    expected.len()
                );
                for (i, v) in expected.iter().enumerate() {
                    assert_eq!(
                        m.r32(0x20000000 + i as u32 * 4),
                        v.as_u64().unwrap() as u32,
                        "case{index} heap word{i}"
                    );
                }
            };
            for action in row["actions"].as_array().unwrap() {
                let resource_index = action["index"].as_u64().unwrap() as usize;
                let resource = row["resources"]
                    .get(resource_index)
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32;
                if action["op"] == 1 {
                    register(&mut m, context, resource, kind);
                } else {
                    let reference = row["references"][resource_index].as_u64().unwrap() as u32;
                    let id = m.r32(reference + 8);
                    unregister(&mut m, context, id, kind);
                }
                compare(&m, &action["after"]);
            }
            let mut calls = vec![];
            let retired = if kind == 9 {
                walk(&mut m, context, kind, |m, reference| {
                    calls.push(json!({"dt": 0x3c888889u32, "reference":reference,
                        "words": (0..4).map(|i| m.r32(reference+i*4)).collect::<Vec<_>>() }));
                    Ok(())
                })
                .unwrap()
            } else {
                vec![]
            };
            assert_eq!(json!(calls), row["calls"], "case{index} callbacks");
            assert_eq!(json!(retired), row["frees"], "case{index} deferred frees");
            compare(&m, &row["after"]);
        }
        let mutations = data["mutations"].as_array().unwrap();
        assert_eq!(mutations.len(), 8);
        for (index, row) in mutations.iter().enumerate() {
            let context = row["context"].as_u64().unwrap() as u32;
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            for (i, word) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(0x20000000 + before.len() as u32 * 4);
            let mut mutated = false;
            for (pass, expected) in row["passes"].as_array().unwrap().iter().enumerate() {
                let mut calls = vec![];
                let retired = walk(&mut m, context, 9, |m, reference| {
                    calls.push(json!({"dt": 0x3c888889u32, "reference":reference,
                        "words":(0..4).map(|i| m.r32(reference+i*4)).collect::<Vec<_>>() }));
                    if !mutated {
                        mutated = true;
                        for action in row["mutation"].as_array().unwrap() {
                            let target = action[1].as_u64().unwrap() as usize;
                            if action[0] == 1 {
                                register(
                                    m,
                                    context,
                                    row["resources"][target].as_u64().unwrap() as u32,
                                    9,
                                );
                            } else {
                                let reference = row["references"][target].as_u64().unwrap() as u32;
                                unregister(m, context, m.r32(reference + 8), 9);
                            }
                        }
                    }
                    Ok(())
                })
                .unwrap();
                assert_eq!(
                    json!(calls),
                    expected["calls"],
                    "mutation{index} pass{pass} callback order/identity"
                );
                assert_eq!(
                    json!(retired),
                    expected["frees"],
                    "mutation{index} pass{pass} frees"
                );
                let after = expected["after"].as_array().unwrap();
                assert_eq!(
                    (m.heap_ptr() - 0x20000000).div_ceil(4) as usize,
                    after.len()
                );
                for (i, word) in after.iter().enumerate() {
                    assert_eq!(
                        m.r32(0x20000000 + i as u32 * 4),
                        word.as_u64().unwrap() as u32,
                        "mutation{index} pass{pass} full heap word{i}"
                    );
                }
            }
        }
    }
    #[test]
    fn control_traverses_non_type1_resources_while_animation_rejects_them() {
        let mut m = Space::new();
        let context = m.alloc(0xd0);
        initialize(&mut m, context);
        let resource = m.alloc(0x100);
        m.w32(resource + 0x4c, 2);
        m.w32(resource + 0x50, 0xffff1234);
        for kind in [8, 0x1c] {
            register(&mut m, context, resource, kind);
        }
        let mut calls = Vec::new();
        for kind in [8, 0x1c] {
            walk(&mut m, context, kind, |m, link| {
                calls.push((kind, m.r32(link + 12)));
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(
            calls,
            [(8, resource)],
            "original426c90 has no type1 predicate;427035 does"
        );
    }
}
