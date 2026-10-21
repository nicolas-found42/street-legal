//! Original45e5f0/490dc0 wheel spring and its child physical entity.
use slr_physics::{cfg::PHYSICS_WORLD as WORLD, space::Space};
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn f(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn stored(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}

/// Original car caller allocates0xec and installs the derived vtable after
///45e5f0, then490dc0 creates a capsule child and attaches its resource handle.
/// The final formal argument is present in the ABI but490dc0 never reads it.
pub fn create(
    m: &mut Space,
    parent: u32,
    car_handle: u32,
    position: [u32; 3],
    length: u32,
    friction: u32,
) -> Result<u32, String> {
    use crate::native_stack::{self as stack, Reg};
    let in_car = stack::active(m);
    let spring = if in_car {
        stack::external(m, 0x44ac76, &[0xec], None, |m| m.alloc(0xec))
    } else {
        m.alloc(0xec)
    };
    if in_car {
        stack::set_reg(m, Reg::Esi, spring);
    }
    let constructor = |m: &mut Space| {
        for offset in [4, 8, 0xc, 0x10, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c] {
            m.w32(spring + offset, 0);
        }
        m.w32(spring, 0x5f1010);
    };
    if in_car {
        stack::external(m, 0x44acb0, &[], Some(spring), constructor);
    } else {
        stack::function(m, &[], Some(spring), constructor);
    }
    for offset in [0x7c, 0x80, 0x94, 0x98, 0xb8, 0xbc] {
        m.w32(spring + offset, 0);
    }
    m.w32(spring, 0x5f0fcc);
    let (point, direction) = if in_car {
        stack::set_reg(m, Reg::Edi, 0);
        let point = stack::sp(m) + 0x48;
        let direction = stack::sp(m) + 0x68;
        stack::store(m, 0x54, length);
        stack::store(m, 0x64, friction);
        for (i, v) in [0, 0xbf800000, 0].into_iter().enumerate() {
            m.w32(direction + i as u32 * 4, v);
        }
        for (i, v) in position.into_iter().enumerate() {
            m.w32(point + i as u32 * 4, v);
        }
        (point, direction)
    } else {
        (0x30001010, 0x30001020)
    };
    let args = [car_handle, parent, point, direction, length, friction];
    let invoke = |m: &mut Space| {
        stack::function(m, &args, Some(spring), |m| {
            create_inner(m, spring, parent, car_handle, position, length)
        })
    };
    if in_car {
        stack::call(m, 0x44acfe, &args, Some(spring), invoke)
    } else {
        invoke(m)
    }
}
fn create_inner(
    m: &mut Space,
    spring: u32,
    parent: u32,
    car_handle: u32,
    position: [u32; 3],
    length: u32,
) -> Result<u32, String> {
    use crate::native_stack::{self as stack, Reg};
    if parent == 0 || car_handle == 0 {
        return Err("NativeSpringBoundary: missing car entity or resource handle".into());
    }
    let active = m.r32(parent + 0x20c);
    if active > 1 {
        return Err("NativeSpringBoundary: invalid parent integration buffer".into());
    }
    stack::alloc(m, 0x48);
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Ebp);
    stack::push_reg(m, Reg::Esi);
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Edi, parent);
    stack::set_reg(m, Reg::Esi, spring);
    stack::set_reg(m, Reg::Ebp, 0);
    stack::store(m, 0x10, position[0]);
    let state = parent + active * 0xd4;
    let local = [
        f(position[0]),
        f(length)
            .mul(f(0xbf000000), PC, RC)
            .add(f(position[1]), PC, RC),
        f(position[2]),
    ];
    let world_position: [u32; 3] = std::array::from_fn(|r| {
        let v = local[2]
            .mul(f(m.r32(state + 0xa4 + r as u32 * 16 + 8)), PC, RC)
            .add(
                local[1].mul(f(m.r32(state + 0xa4 + r as u32 * 16 + 4)), PC, RC),
                PC,
                RC,
            )
            .add(
                local[0].mul(f(m.r32(state + 0xa4 + r as u32 * 16)), PC, RC),
                PC,
                RC,
            );
        // X/Y projections are stored before adding the parent's position; Z
        // remains in its x87 register until the translation addition.
        let v = if r < 2 { f(stored(v)) } else { v };
        stored(v.add(f(m.r32(state + 0x64 + r as u32 * 4)), PC, RC))
    });
    let identity = [
        1f32.to_bits(),
        0,
        0,
        0,
        1f32.to_bits(),
        0x80000000,
        0,
        0,
        1f32.to_bits(),
    ];
    let rotation = crate::native_entity::matrix_inplace_product(
        identity,
        crate::native_entity::read_matrix(m, state + 0xa4),
    );
    let point = stack::sp(m) + 0x10;
    let angles = stack::sp(m) + 0x1c;
    let rotation_ptr = stack::sp(m) + 0x28;
    for (i, v) in world_position.into_iter().enumerate() {
        m.w32(point + i as u32 * 4, v);
    }
    stack::external(m, 0x490e86, &[0, 0, 0], Some(angles), |m| {
        for i in 0..3 {
            m.w32(angles + i * 4, 0);
        }
    });
    stack::external(m, 0x490e94, &[angles], Some(rotation_ptr), |m| {
        crate::native_entity::write_matrix(m, rotation_ptr, identity)
    });
    stack::external(m, 0x490eb5, &[state + 0xa4], Some(rotation_ptr), |m| {
        crate::native_entity::write_matrix(m, rotation_ptr, rotation)
    });
    let half = stored(f(length).mul(f(0x3f000000), PC, RC));
    stack::store(m, 0x64, half);
    let body = stack::call(
        m,
        0x490ee1,
        &[0x3c23d70a, 3, half, 0x3d4ccccd, 0, 0],
        Some(WORLD),
        |m| crate::native_geometry::body(m, 0x3c23d70a, 3, [half, 0x3d4ccccd, 0], None),
    )?;
    let entity = stack::call(
        m,
        0x490ef8,
        &[parent, body, point, rotation_ptr],
        Some(WORLD),
        |m| crate::native_entity::child(m, parent, body, world_position, Some(rotation_ptr)),
    )?;
    stack::set_reg(m, Reg::Edi, entity);
    stack::set_reg(m, Reg::Ebx, spring + 0x7c);
    m.w32(entity + 0x58, m.r32(entity + 0x58) | 0x40);
    m.w32(spring + 0x7c, entity);
    m.w32(spring + 0x80, m.r32(entity));
    m.w32(entity, spring + 0x7c);
    m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
    let resource = m.r32(car_handle + 0xc);
    stack::external(m, 0x490f3a, &[resource], Some(entity + 0x38), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        if resource != 0 {
            let link = entity + 0x38;
            let head = m.r32(resource + 0x48);
            if head != 0 {
                m.w32(head, link);
            }
            m.w32(link, 0);
            m.w32(link + 4, head);
            m.w32(resource + 0x48, link);
            m.w32(link + 8, m.r32(resource + 0x50));
            m.w32(link + 0xc, resource);
        }
    });
    m.w32(spring + 0x58, half);
    for offset in [
        0x54, 0x5c, 0x60, 0x64, 0x68, 0x6c, 0x70, 0x74, 0xc0, 0xc4, 0xc8, 0x78, 0x3c, 0x40,
    ] {
        m.w32(spring + offset, 0);
    }
    for (offset, value) in [
        (0x30, length),
        (0x38, length),
        (0x50, length),
        (0x44, 0x44480000),
        (0x34, 0x3f666667),
        (0x48, 0x43480000),
        (0x4c, 0x430c0000),
    ] {
        m.w32(spring + offset, value);
    }
    let positive = f32::from_bits(length) > 0.;
    m.w32(spring + 0x38, if positive { length } else { 0x3f666667 });
    m.w32(spring + 0x50, if positive { length } else { 0x3f666667 });
    m.w32(spring + 0x40, if positive { length } else { 0 });
    // The world constructor's pair of intrusive spring-list sentinels.
    if m.r32(WORLD + 0xac) == 0 {
        m.w32(WORLD + 0x9c, WORLD + 0xa4);
        m.w32(WORLD + 0xac, WORLD + 0x98);
    }
    stack::external(m, 0x490ff6, &[spring], Some(WORLD), |m| {
        let previous = m.r32(WORLD + 0xac);
        m.w32(previous + 4, spring);
        m.w32(spring + 8, previous);
        m.w32(spring + 4, WORLD + 0xa4);
        m.w32(WORLD + 0xac, spring);
        m.w32(WORLD + 0x58, m.r32(WORLD + 0x58).wrapping_add(1));
        stack::push_reg(m, Reg::Esi);
    });
    m.w32(spring + 0xd8, m.r32(spring + 0x38));
    m.w32(
        spring + 0x9c,
        stored(
            f(m.r32(spring + 0x38))
                .mul(f(m.r32(spring + 0x38)), PC, RC)
                .add(f(0x3dcccccd), PC, RC),
        ),
    );
    for offset in [
        0x90, 0xe0, 0xdc, 0xcc, 0xd0, 0xd4, 0xe4, 0xe8, 0x84, 0x88, 0x8c,
    ] {
        m.w32(spring + offset, 0);
    }
    Ok(spring)
}

/// Original491f80 parameter projection followed by the car caller's d8 store.
pub fn configure_from_wheel(m: &mut Space, spring: u32, wheel: u32) -> Result<(), String> {
    if spring == 0 || wheel == 0 {
        return Err("NativeSpringBoundary: missing spring or wheel".into());
    }
    let plus = |a: u32, b: u32| stored(f(a).add(f(b), PC, RC));
    let radius = m.r32(wheel + 0x60);
    let a = [
        m.r32(wheel + 0x44),
        m.r32(wheel + 0x48),
        m.r32(wheel + 0x5c),
        plus(m.r32(wheel + 0x54), radius),
        plus(m.r32(wheel + 0x58), radius),
        m.r32(wheel + 0x4c),
        radius,
    ];
    configure(m, spring, a);
    m.w32(spring + 0xd8, plus(m.r32(wheel + 0x38), radius));
    Ok(())
}
fn configure(m: &mut Space, spring: u32, a: [u32; 7]) {
    m.w32(spring + 0x44, a[0]);
    m.w32(spring + 0x48, a[1]);
    m.w32(spring + 0x34, a[2]);
    m.w32(
        spring + 0x4c,
        if f32::from_bits(a[5]) <= 0. {
            stored(f(a[1]).mul(f(0x3f333333), PC, RC))
        } else {
            a[5]
        },
    );
    m.w32(
        spring + 0x38,
        if f32::from_bits(a[3]) > 0. {
            a[3]
        } else {
            a[2]
        },
    );
    m.w32(
        spring + 0x3c,
        if f32::from_bits(a[4]) > 0. { a[4] } else { 0 },
    );
    if f32::from_bits(a[6]) <= 0. {
        m.w32(spring + 0x50, a[2]);
        m.w32(spring + 0x40, 0);
    } else {
        m.w32(spring + 0x50, a[6]);
        m.w32(spring + 0x40, a[6]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_spring_parameter_projection_matches() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spring-original.json");
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/native_spring_probe.py"),
        )
        .unwrap();
        for row in data["configure"].as_array().unwrap() {
            let mut m = Space::new();
            let spring = m.alloc(0xec);
            for i in 0..0xec / 4 {
                m.w32(spring + i * 4, 0x12340000 + i);
            }
            configure(
                &mut m,
                spring,
                std::array::from_fn(|i| row["args"][i].as_u64().unwrap() as u32),
            );
            let words: Vec<_> = (0..0xec / 4).map(|i| m.r32(spring + i * 4)).collect();
            assert_eq!(serde_json::json!(words), row["words"]);
        }
    }

    #[test]
    fn original_spring_constructor_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spring-original.json");
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/native_spring_probe.py"),
        )
        .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for (offset, value) in [
                (0x188, WORLD + 0x190),
                (0x198, WORLD + 0x184),
                (0x9c, WORLD + 0xa4),
                (0xac, WORLD + 0x98),
            ] {
                m.w32(WORLD + offset, value);
            }
            m.w32(0x617650, WORLD);
            let b = crate::native_geometry::body(&mut m, 0, 0, [0; 3], None).unwrap();
            let parent = crate::native_entity::root(&mut m, b, [0; 3], None).unwrap();
            let handle = m.alloc(0x10);
            let resource = m.alloc(0x60);
            m.w32(handle + 0xc, resource);
            m.w32(resource + 0x50, 0x12345678);
            let p = std::array::from_fn(|i| row["position"][i].as_u64().unwrap() as u32);
            for (i, length) in row["lengths"].as_array().unwrap().iter().enumerate() {
                let s = create(
                    &mut m,
                    parent,
                    handle,
                    p,
                    length.as_u64().unwrap() as u32,
                    0.17f32.to_bits(),
                )
                .unwrap();
                assert_eq!(s, row["springs"][i].as_u64().unwrap() as u32);
            }
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            let actual: Vec<_> = (0..0x210 / 4).map(|i| m.r32(WORLD + i * 4)).collect();
            assert_eq!(serde_json::json!(actual), row["world"]);
            let expected: std::collections::BTreeMap<_, _> = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (v[0].as_u64().unwrap() as u32, v[1].as_u64().unwrap() as u32))
                .collect();
            for address in (slr_physics::space::HEAP_BASE..m.heap_ptr()).step_by(4) {
                let actual = m.r32(address);
                let expected = *expected.get(&address).unwrap_or(&0);
                assert_eq!(
                    actual, expected,
                    "length{} heap {address:#x}: {actual:#x}/{expected:#x}",
                    row["lengths"]
                );
            }
        }
    }
}

/// Original491100 no-contact spring reset and sleeping-list relocation.
/// A live contact below full extension requires its source force/constraint
/// branch and remains an explicit boundary here.
pub fn pre_step(m: &mut Space, spring: u32) -> Result<i32, String> {
    let entity = m.r32(spring + 0x7c);
    if entity == 0 {
        return Ok(0);
    }
    let other = m.r32(spring + 0xb8);
    let active = m.r32(m.r32(entity + 0x5c) + 0x9c) & 4 == 0 && m.r32(entity + 0x58) & 0x80 == 0;
    let other_active =
        other != 0 && m.r32(m.r32(other + 0x5c) + 0x9c) & 4 == 0 && m.r32(other + 0x58) & 0x80 == 0;
    let length = m.r32(spring + 0x38);
    if active || other_active {
        let square = f(length).mul(f(length), PC, RC);
        let cc = square.fcom(f(m.r32(spring + 0x9c))).0;
        if other != 0 && !cc.c0 && !cc.c3 {
            return Err("NativeSpringBoundary: loaded contact491100 requires original force/constraint response".into());
        }
        for offset in [0xcc, 0xd0, 0xd4, 0xe4, 0xdc, 0x6c, 0x70, 0x74, 0x78, 0xe0] {
            m.w32(spring + offset, 0);
        }
        m.w32(spring + 0xd8, length);
        m.w32(spring + 0x9c, stored(square.add(f(0x3dcccccd), PC, RC)));
        return Ok(0);
    }
    for offset in [0xcc, 0xd0, 0xd4, 0xe4] {
        m.w32(spring + offset, 0);
    }
    let previous = m.r32(spring + 8);
    let next = m.r32(spring + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(spring + 4, 0);
    m.w32(spring + 8, 0);
    let tail = m.r32(WORLD + 0xe4);
    if tail == 0 {
        return Err("NativeSpringBoundary: sleeping spring-list storage is uninitialized".into());
    }
    m.w32(tail + 4, spring);
    m.w32(spring + 4, WORLD + 0xdc);
    m.w32(spring + 8, tail);
    m.w32(WORLD + 0xe4, spring);
    Ok(0)
}
#[cfg(test)]
mod step_tests {
    use super::*;
    #[test]
    fn original_no_contact_and_sleeping_spring_step_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spring-step-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run native_spring_step_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
            let result = pre_step(&mut m, row["spring"].as_u64().unwrap() as u32).unwrap();
            assert_eq!(result, row["result"].as_i64().unwrap() as i32);
            let expected = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for (start, len) in [
                (
                    slr_physics::space::HEAP_BASE,
                    m.heap_ptr() - slr_physics::space::HEAP_BASE,
                ),
                (WORLD, 0x210),
            ] {
                for p in (start..start + len).step_by(4) {
                    assert_eq!(
                        m.r32(p),
                        expected.get(&p).copied().unwrap_or(0),
                        "{} address{p:#x}",
                        row["mode"]
                    );
                }
            }
        }
    }
}
