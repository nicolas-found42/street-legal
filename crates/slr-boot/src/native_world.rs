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
    if m.r32(WORLD + 0x84) != 0 || m.r32(WORLD + 0x88) != 0 {
        return Err("NativeWorldBoundary: world is already initialized".into());
    }
    crate::native_stack::function(m, &[], Some(WORLD), |m| {
        crate::native_vm_root::world::initialize_body(m, WORLD, &mut PrimitiveWorld)
    })
}
struct PrimitiveWorld;
impl crate::native_vm_root::world::Environment for PrimitiveWorld {
    fn body(&mut self, m: &mut Space) -> Result<(), String> {
        use crate::native_stack::{self as stack, Reg};
        let body = crate::native_geometry::body(m, 0, 0, [0; 3], None)?;
        stack::set_reg(m, Reg::Eax, body);
        Ok(())
    }
    fn entity(&mut self, m: &mut Space) -> Result<(), String> {
        use crate::native_stack::{self as stack, Reg};
        let body = stack::argument(m, 0);
        let position = stack::argument(m, 1);
        let rotation = stack::argument(m, 2);
        let position = std::array::from_fn(|i| m.r32(position + i as u32 * 4));
        let entity = crate::native_entity::root(m, body, position, Some(rotation))?;
        stack::set_reg(m, Reg::Eax, entity);
        Ok(())
    }
}

/// Original sleeping transition, including relative teardown and reattachment
/// when moving a member from the active root to the sleeping root.
/// exe: FUN_004a7a80
pub fn sleep(m: &mut Space, entity: u32) -> Result<(), String> {
    fn walk(m: &mut Space, e: u32, depth: usize) -> Result<(), String> {
        crate::native_stack::function(m, &[], Some(e), |m| walk_inner(m, e, depth))
    }
    fn walk_inner(m: &mut Space, e: u32, depth: usize) -> Result<(), String> {
        use crate::native_stack::{self as stack, Reg};
        if e == 0 || depth > 1024 {
            return Err("NativeWorldBoundary: invalid sleeping entity graph".into());
        }
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::set_reg(m, Reg::Ebp, e);
        stack::set_reg(m, Reg::Ebx, 0x80);
        stack::push_reg(m, Reg::Esi);
        if m.r32(e + 0x58) & 0x80 != 0 {
            return Ok(());
        }
        let active = m.r32(WORLD + 0x84);
        let sleeping = m.r32(WORLD + 0x88);
        if active == 0 || sleeping == 0 {
            return Err("NativeWorldBoundary: physics roots unavailable".into());
        }
        if m.r32(e + 0x20) == active {
            stack::call(m, 0x4a7aac, &[e], Some(WORLD), |m| detach(m, e))?;
            stack::call(m, 0x4a7abf, &[e, sleeping], Some(WORLD), |m| {
                attach(m, e, sleeping)
            })?;
        }
        m.w32(e + 0x58, m.r32(e + 0x58) | 0x80);
        stack::set_reg(m, Reg::Ebx, 0);
        let index = m.r32(e + 0x20c);
        if index > 1 {
            return Err("NativeWorldBoundary: invalid integration buffer index".into());
        }
        let state = e + 0x64 + index * 0xd4;
        stack::set_reg(m, Reg::Esi, state);
        for offset in [
            0xc, 0x10, 0x14, 0x28, 0x2c, 0x30, 0x70, 0x74, 0x78, 0x7c, 0x80, 0x84, 0x88, 0x8c, 0x90,
        ] {
            m.w32(state + offset, 0);
        }
        m.w32(state + 0x3c, m.r32(state + 0x3c) & !2);
        stack::push_reg(m, Reg::Edi);
        m.copy(e + 0x64 + (index ^ 1) * 0xd4, state, 0xd4);
        stack::add_sp(m, 4);
        let mut child = m.r32(e + 0x24);
        while child != 0 {
            stack::set_reg(m, Reg::Esi, child);
            stack::call(m, 0x4a7b46, &[], Some(child), |m| walk(m, child, depth + 1))?;
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

/// Original4a6470 (state thiscall): store angular velocity `w` and derive
/// angular momentum through the inverse of the world inverse inertia.
/// exe: FUN_004a6470
pub fn angular_momentum(m: &mut Space, state: u32, w: u32, frame: u32) {
    for i in 0..3 {
        m.w32(state + 0x70 + i * 4, m.r32(w + i * 4));
    }
    let local = frame.wrapping_sub(0x30);
    crate::native_matrix::inverse(m, local, state + 0xa4, frame.wrapping_sub(0x44 + 8));
    let mut fpu = crate::native_fpu::Fpu::new();
    let mut out = [0u32; 3];
    for (i, word) in out.iter_mut().enumerate() {
        let row = local + i as u32 * 0x10;
        fpu.fld(m, row);
        fpu.fmul(m, state + 0x70);
        fpu.fld(m, row + 8);
        fpu.fmul(m, state + 0x78);
        fpu.faddp(1);
        fpu.fld(m, row + 4);
        fpu.fmul(m, state + 0x74);
        fpu.faddp(1);
        *word = fpu.fstp_bits();
    }
    for (i, word) in out.into_iter().enumerate() {
        m.w32(state + 0x7c + i as u32 * 4, word);
    }
}

/// Original49a760 (world thiscall): detach a member from its parent; a
/// relative record is freed and the old parent's compound recomputed.
/// exe: FUN_0049a760
pub fn detach(m: &mut Space, entity: u32) -> Result<(), String> {
    crate::native_stack::function(m, &[entity], Some(WORLD), |m| detach_inner(m, entity))
}
fn detach_inner(m: &mut Space, entity: u32) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, entity);
    stack::set_reg(m, Reg::Ebx, WORLD);
    let parent = m.r32(entity + 0x20);
    if parent != 0 {
        let mut previous = 0;
        let mut current = m.r32(parent + 0x24);
        while current != 0 {
            if current == entity {
                let next = m.r32(entity + 0x28);
                if previous == 0 {
                    m.w32(parent + 0x24, next);
                } else {
                    m.w32(previous + 0x28, next);
                }
                break;
            }
            previous = current;
            current = m.r32(current + 0x28);
        }
    }
    if m.r32(entity + 0x1c) != 0 {
        // 54f5b0 frees the record; the bump allocator never reuses it.
        stack::external(m, 0x49a7a6, &[m.r32(entity + 0x1c)], None, |_| ());
        m.w32(entity + 0x58, m.r32(entity + 0x58) & !4);
        m.w32(entity + 0x1c, 0);
        stack::call(m, 0x49a7c4, &[parent], Some(WORLD), |m| {
            crate::native_entity::compound(m, parent)
        })?;
        stack::call(m, 0x49a7cb, &[], Some(entity), |m| {
            crate::native_entity::world_inertia(m, entity)
        });
        let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
        stack::external(m, 0x49a7f5, &[state + 0x70], Some(state), |m| {
            angular_momentum(m, state, state + 0x70, stack::sp(m))
        });
    }
    m.w32(entity + 0x20, 0);
    Ok(())
}

/// Original49a810 (world thiscall): attach a member to a parent.
/// exe: FUN_0049a810
pub fn attach(m: &mut Space, entity: u32, parent: u32) -> Result<(), String> {
    crate::native_stack::function(m, &[entity, parent], Some(WORLD), |m| {
        attach_inner(m, entity, parent)
    })
}
fn attach_inner(m: &mut Space, entity: u32, parent: u32) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    stack::push_reg(m, Reg::Esi);
    m.w32(entity + 0x28, m.r32(parent + 0x24));
    m.w32(parent + 0x24, entity);
    m.w32(entity + 0x20, parent);
    stack::add_sp(m, 4);
    if m.r32(m.r32(parent + 0x5c) + 0x9c) & 0x100 == 0 {
        stack::call(m, 0x49a83b, &[entity], Some(WORLD), |m| {
            crate::native_entity::update_relative(m, entity)
        })?;
    }
    Ok(())
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

    #[test]
    fn original_world_attachment_teardown_and_angular_momentum_match() {
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/native-world-detach-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            assert!(
                std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
                "required vectors missing: run re/oracle/native_world_detach_probe.py"
            );
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_world_detach_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema_version"], 1);
        assert_eq!(data["fpu_cw"], 0x007f);
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        let producer = std::fs::read(root.join("re/oracle/native_world_detach_probe.py")).unwrap();
        assert_eq!(
            data["producer_sha256"],
            format!("{:x}", Sha256::digest(producer))
        );
        let executable =
            std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap();
        assert_eq!(
            data["exe_sha256"],
            format!("{:x}", Sha256::digest(executable))
        );
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 67);
        for (index, row) in rows.iter().enumerate() {
            let mut m = Space::new();
            let word = |v: &serde_json::Value| v.as_u64().unwrap() as u32;
            let before = &row["before"];
            for (i, v) in before["words"].as_array().unwrap().iter().enumerate() {
                m.w32(slr_physics::space::HEAP_BASE + i as u32 * 4, word(v));
            }
            for (i, v) in before["world"].as_array().unwrap().iter().enumerate() {
                m.w32(WORLD + i as u32 * 4, word(v));
            }
            for (i, v) in row["stack"].as_array().unwrap().iter().enumerate() {
                m.w32(0x7fef_f000 + i as u32 * 4, word(v));
            }
            m.set_heap_ptr(word(&before["heap"]));
            m.w32(0x5f08f0, 1f32.to_bits());
            m.w32(0x617650, WORLD);
            m.w32(0x640bd0, 1);
            crate::native_matrix::identity(&mut m, 0x643638);
            let args = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(word)
                .collect::<Vec<_>>();
            let this = word(&row["this"]);
            let frame = ((0x7ff0_0000 - 0x100 - args.len() as u32 * 4 - 4) & !15) - 4;
            crate::native_stack::set_sp(&mut m, frame);
            match word(&row["address"]) {
                0x4a6470 => angular_momentum(&mut m, this, args[0], frame),
                0x49a760 => detach(&mut m, args[0]).unwrap(),
                0x49a810 => attach(&mut m, args[0], args[1]).unwrap(),
                0x4a7a80 => sleep(&mut m, this).unwrap(),
                address => panic!("unknown world vector {address:#x}"),
            }
            let after = &row["after"];
            assert_eq!(m.heap_ptr(), word(&after["heap"]), "row {index} heap");
            for (i, v) in after["words"].as_array().unwrap().iter().enumerate() {
                let address = slr_physics::space::HEAP_BASE + i as u32 * 4;
                assert_eq!(
                    m.r32(address),
                    word(v),
                    "row {index} {} heap {address:#x}",
                    row["kind"]
                );
            }
            for (i, v) in after["world"].as_array().unwrap().iter().enumerate() {
                let address = WORLD + i as u32 * 4;
                assert_eq!(m.r32(address), word(v), "row {index} world {address:#x}");
            }
        }
    }
}
