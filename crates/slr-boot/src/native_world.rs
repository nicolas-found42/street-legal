//! Original4a5580 physics-world startup and4a7a80 sleeping transitions.
use slr_physics::{cfg::PHYSICS_WORLD as WORLD, space::Space};

/// Original CRT initializer497b60 constructs the world's intrusive list
/// sentinels. Its45dc70 call is a no-op;498010 zeroes the final three words.
/// Process atexit registration is outside this storage initializer's scope;
/// the original world destructor still requires separate lifecycle acceptance.
pub fn initialize_storage(m: &mut Space) {
    for offset in [0x94, 0xb0, 0xcc, 0xe8, 0x110, 0x12c, 0x148, 0x164, 0x180] {
        let list = WORLD + offset;
        for (delta, value) in [
            (0, 0x5f09a8),
            (4, 0x5f09ac),
            (8, list + 0x10),
            (12, 0),
            (16, 0x5f09ac),
            (20, 0),
            (24, list + 4),
        ] {
            m.w32(list + delta, value);
        }
    }
    for offset in [0x1f0, 0x1f4, 0x1f8, 0x1fc, 0x200, 0x204, 0x208] {
        m.w32(WORLD + offset, 0);
    }
}

/// Create the two static list roots before any configured body is loaded.
/// exe: FUN_004a5580
pub fn initialize(m: &mut Space) -> Result<(), String> {
    crate::native_stack::function(m, &[], Some(WORLD), initialize_inner)
}
fn initialize_inner(m: &mut Space) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    if m.r32(WORLD + 0x84) != 0 || m.r32(WORLD + 0x88) != 0 {
        return Err("NativeWorldBoundary: world is already initialized".into());
    }
    //4a5580 prologue and live registers; stack bytes survive later calls.
    stack::alloc(m, 0x3c);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, WORLD);
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Edi, 0);
    for offset in [
        0, 4, 0x20, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x80, 0x104, 0x10c,
        0x1b8, 0x1c0, 0x1c4, 0x1cc, 0x1d0, 0x1d4, 0x1d8, 0x1dc, 0x1e0,
    ] {
        m.w32(WORLD + offset, 0);
    }
    for (offset, value) in [
        (8, 0x3ca3d70a),
        (12, 0x3ca3d70a),
        (0x78, 1),
        (0x7c, 1),
        (0x108, 0xc11cf5c3),
        (0x1b4, 0x3fa57a78),
        (0x1bc, 0xc0e00000),
        (0x1c8, 0xbf800000),
        (0x1e4, 0x43960000),
        (0x1e8, 0x44098000),
    ] {
        m.w32(WORLD + offset, value);
    }
    let position = stack::sp(m) + 8;
    let rotation = stack::sp(m) + 0x14;
    for offset in [8, 12, 16] {
        stack::store(m, offset, 0);
    }
    stack::external(m, 0x4a565b, &[], Some(rotation), |m| {
        crate::native_entity::write_matrix(
            m,
            rotation,
            [
                1f32.to_bits(),
                0,
                0,
                0,
                1f32.to_bits(),
                0,
                0,
                0,
                1f32.to_bits(),
            ],
        );
    });
    for (offset, body_return, entity_return) in
        [(0x84, 0x4a5672, 0x4a567a), (0x88, 0x4a56a4, 0x4a56ac)]
    {
        //The caller forms the root-entity arguments before the body call.
        stack::push(m, rotation);
        stack::push(m, position);
        for _ in 0..6 {
            stack::push(m, 0);
        }
        let body = stack::call(m, body_return, &[], Some(WORLD), |m| {
            crate::native_geometry::body(m, 0, 0, [0; 3], None)
        })?;
        stack::add_sp(m, 24);
        stack::push(m, body);
        let entity = stack::call(m, entity_return, &[], Some(WORLD), |m| {
            crate::native_entity::root(m, body, [0; 3], Some(rotation))
        })?;
        stack::add_sp(m, 12);
        m.w32(WORLD + offset, entity);
        m.w32(body + 0x9c, m.r32(body + 0x9c) | 0x100);
    }
    for (address, offset) in [(0x62dcac, 0x1f8), (0x62dcb0, 0x200), (0x62dca8, 0x1f0)] {
        m.w32(address, WORLD + offset);
    }
    Ok(())
}

/// Original calls this before its newly-created Part compound is populated.
/// The active/sleeping roots have body flag100, so moving roots between them
/// does not create relative transforms or recompute a compound.
/// exe: FUN_004a7a80
pub fn sleep(m: &mut Space, entity: u32) -> Result<(), String> {
    fn walk(m: &mut Space, e: u32, depth: usize) -> Result<(), String> {
        if e == 0 || depth > 1024 {
            return Err("NativeWorldBoundary: invalid sleeping entity graph".into());
        }
        if m.r32(e + 0x58) & 0x80 != 0 {
            return Ok(());
        }
        let active = m.r32(WORLD + 0x84);
        let sleeping = m.r32(WORLD + 0x88);
        if active == 0 || sleeping == 0 {
            return Err("NativeWorldBoundary: physics roots unavailable".into());
        }
        if m.r32(e + 0x20) == active {
            if m.r32(e + 0x1c) != 0 {
                return Err(
                    "NativeWorldBoundary: root sleep requires relative-transform teardown49a760"
                        .into(),
                );
            }
            let mut current = m.r32(active + 0x24);
            let mut previous = 0;
            while current != 0 && current != e {
                previous = current;
                current = m.r32(current + 0x28);
            }
            if current == 0 {
                return Err("NativeWorldBoundary: active parent lacks entity".into());
            }
            let next = m.r32(e + 0x28);
            m.w32(
                if previous == 0 {
                    active + 0x24
                } else {
                    previous + 0x28
                },
                next,
            );
            if m.r32(m.r32(sleeping + 0x5c) + 0x9c) & 0x100 == 0 {
                return Err(
                    "NativeWorldBoundary: sleeping root requires relative transform49a840".into(),
                );
            }
            m.w32(e + 0x28, m.r32(sleeping + 0x24));
            m.w32(sleeping + 0x24, e);
            m.w32(e + 0x20, sleeping);
        }
        m.w32(e + 0x58, m.r32(e + 0x58) | 0x80);
        let index = m.r32(e + 0x20c);
        if index > 1 {
            return Err("NativeWorldBoundary: invalid integration buffer index".into());
        }
        let state = e + 0x64 + index * 0xd4;
        for offset in [
            0xc, 0x10, 0x14, 0x28, 0x2c, 0x30, 0x70, 0x74, 0x78, 0x7c, 0x80, 0x84, 0x88, 0x8c, 0x90,
        ] {
            m.w32(state + offset, 0);
        }
        m.w32(state + 0x3c, m.r32(state + 0x3c) & !2);
        m.copy(e + 0x64 + (index ^ 1) * 0xd4, state, 0xd4);
        let mut child = m.r32(e + 0x24);
        while child != 0 {
            walk(m, child, depth + 1)?;
            child = m.r32(child + 0x28);
        }
        Ok(())
    }
    walk(m, entity, 0)
}

/// Original4a7990: activate a sleeping root and its compound descendants.
/// Pending world force callbacks require their actual virtual handlers.
pub fn activate(m: &mut Space, entity: u32) -> Result<(), String> {
    fn walk(m: &mut Space, e: u32, depth: usize) -> Result<(), String> {
        if e == 0 || depth > 1024 {
            return Err("NativeWorldBoundary: invalid activation graph".into());
        }
        let flags = m.r32(e + 0x58);
        if flags & 0x80 == 0 {
            return Ok(());
        }
        let queued = m.r32(WORLD + 0xd4);
        if queued != 0 && m.r32(queued + 4) != 0 {
            return Err(
                "NativeWorldBoundary: activation requires original world force callbacks".into(),
            );
        }
        m.w32(e + 0x58, flags & !0x80);
        if flags & 4 == 0 {
            m.w32(e + 0xc, 0);
            m.w32(e + 0x10, 0x47c35000);
            m.w32(e + 0x14, 0);
            if m.r32(e + 0x20) == m.r32(WORLD + 0x88) {
                let active = m.r32(WORLD + 0x84);
                if active == 0 {
                    return Err("NativeWorldBoundary: active root unavailable".into());
                }
                crate::native_car::reparent_root(m, e, active)?;
            }
        }
        let mut child = m.r32(e + 0x24);
        while child != 0 {
            walk(m, child, depth + 1)?;
            child = m.r32(child + 0x28);
        }
        Ok(())
    }
    walk(m, entity, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compare(m: &Space, row: &serde_json::Value) {
        assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        let world = (0..0x210 / 4)
            .map(|i| m.r32(WORLD + i * 4))
            .collect::<Vec<_>>();
        assert_eq!(serde_json::json!(world), row["world"]);
        let words = (slr_physics::space::HEAP_BASE..m.heap_ptr())
            .step_by(4)
            .filter_map(|p| {
                let v = m.r32(p);
                (v != 0).then_some([p, v])
            })
            .collect::<Vec<_>>();
        let expected = row["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let actual = words
            .into_iter()
            .map(|[a, v]| (a, v))
            .collect::<std::collections::BTreeMap<_, _>>();
        for address in actual.keys().chain(expected.keys()) {
            assert_eq!(
                actual.get(address).copied().unwrap_or(0),
                expected.get(address).copied().unwrap_or(0),
                "heap {address:#x}"
            );
        }
    }
    #[test]
    fn original_world_startup_and_sleep_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-world-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_world_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            if row["storage"].as_bool().unwrap_or(false) {
                initialize_storage(&mut m);
            }
            initialize(&mut m).unwrap();
            compare(&m, &row["initial"]);
            if row["sleeping"].as_bool().unwrap() {
                let body = crate::native_geometry::body(
                    &mut m,
                    100f32.to_bits(),
                    5,
                    [2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits()],
                    None,
                )
                .unwrap();
                let entity = crate::native_entity::root(&mut m, body, [0; 3], None).unwrap();
                sleep(&mut m, entity).unwrap();
                if row["activating"].as_bool().unwrap_or(false) {
                    activate(&mut m, entity).unwrap();
                }
            }
            compare(&m, &row["after"]);
        }
    }
}
