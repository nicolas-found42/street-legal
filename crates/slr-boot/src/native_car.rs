//! Bounded scalar stage of the original car runtime constructor.
//! Base Part construction and resource/physics graph ownership are caller duties;
//! this module does not stand in for the whole44a250 constructor.
use slr_physics::space::Space;

/// Original44a288..44a3fd placement construction, before base46eae0.
/// The caller allocates exactly0x212c bytes. No resource collaborator is involved.
pub fn initialize_storage(m: &mut Space, object: u32) {
    let zero = |m: &mut Space, offset: u32, bytes: u32| {
        for i in (0..bytes).step_by(4) {
            m.w32(object + offset + i, 0);
        }
    };
    //45e910 base storage (unwritten gaps intentionally preserved).
    for (offset, bytes) in [
        (0, 0x4c),
        (0x60, 0x10),
        (0x7c, 0x30),
        (0xc0, 0x10),
        (0xf4, 8),
    ] {
        zero(m, offset, bytes);
    }
    list(m, object + 0xd4, 0x5f1060);
    //45e7d0 constructs four embedded records. The final word at+0x6c
    //and the record index at+0 remain untouched until later initialization.
    for index in 0..4 {
        let record = 0x11c + index * 0x4a8;
        zero(m, record + 4, 0x68);
        zero(m, record + 0x70, 0x400);
    }
    zero(m, 0x13bc, 8);
    for offset in [
        0x13c4, 0x13d4, 0x1e98, 0x1ea8, 0x1eb8, 0x1ee0, 0x1fc4, 0x1fd4, 0x1fe4, 0x1ff4, 0x2004,
        0x202c, 0x203c, 0x204c, 0x20d8,
    ] {
        zero(m, offset, 16);
    }
    for index in 0..16 {
        zero(m, 0x13e8 + index * 0x10, 16);
    }
    //45e9e0 initializes only four words in each0x8c record.
    for index in 0..16 {
        zero(m, 0x14ec + index * 0x8c + 0x30, 16);
    }
    list(m, object + 0x1ddc, 0x5f09a8);
    zero(m, 0x1f90, 16);
    list(m, object + 0x1fa0, 0x5f09a8);
    zero(m, 0x2014, 8);
    for (offset, vtable) in [
        (0x205c, 0x5f1064),
        (0x2078, 0x5f1068),
        (0x2094, 0x5f106c),
        (0x20b0, 0x5f1054),
    ] {
        list(m, object + offset, vtable);
    }
}
pub(crate) fn list(m: &mut Space, address: u32, vtable: u32) {
    for (offset, value) in [
        (0, vtable),
        (4, 0x5f09ac),
        (8, address + 16),
        (12, 0),
        (16, 0x5f09ac),
        (20, 0),
        (24, address + 4),
    ] {
        m.w32(address + offset, value);
    }
}

/// Original44a7de..44a845 (zero-count branch44b419): wheel array and its
/// placement constructors. This does not substitute for wheel physics setup.
pub fn prepare_wheel_storage(m: &mut Space, template: u32, object: u32) -> Result<u32, String> {
    let count = m.r32(template + 0x1f40);
    if count > 16 {
        return Err("NativeCarBoundary: wheel count exceeds sixteen records".into());
    }
    let wheels = if count == 0 {
        0
    } else {
        let allocation = m.alloc(count * 0x2b4 + 4);
        m.w32(allocation, count);
        for index in 0..count {
            crate::wheel::construct_wheel(m, allocation + 4 + index * 0x2b4);
        }
        allocation + 4
    };
    m.w32(object + 0x13e4, wheels);
    Ok(wheels)
}

/// Original44a472..44a776 scalar stage; not a whole-function ledger annotation.
#[cfg(test)]
pub fn initialize_scalars(
    m: &mut Space,
    template: u32,
    object: u32,
    world: u32,
) -> Result<(), String> {
    initialize_scalars_impl(m, template, object, world, None)
}

/// Scalar stage with original43e3e0 cloning enabled. Resource bindings emitted
/// through `host` must remain owned for the runtime object's lifetime.
pub fn initialize_scalars_with_host(
    m: &mut Space,
    template: u32,
    object: u32,
    world: u32,
    host: &mut dyn slr_physics::cfg::Host,
) -> Result<(), String> {
    initialize_scalars_impl(m, template, object, world, Some(host))
}
fn initialize_scalars_impl(
    m: &mut Space,
    template: u32,
    object: u32,
    world: u32,
    host: Option<&mut dyn slr_physics::cfg::Host>,
) -> Result<(), String> {
    if template == 0 || object == 0 || world == 0 {
        return Err(
            "NativeCarBoundary: scalar initialization requires live template/body/world".into(),
        );
    }
    let parameters = m.r32(template + 0x2e48);
    if parameters == 0 {
        return Err("NativeCarBoundary: missing configured parameter block".into());
    }
    if m.r8(template + 0x2e4c) & 1 != 0 && host.is_none() {
        return Err(
            "NativeCarBoundary: parameter clone43e3e0 requires a live resource host".into(),
        );
    }
    let count = m.r32(template + 0x177c) as i32;
    if count > 4 {
        return Err("NativeCarBoundary: embedded part count exceeds four original records".into());
    }
    // Literal stores from44a472..44a72c; zero stores intentionally overwrite
    // reused/nonzero memory. Unwritten fields retain their previous contents.
    for &(offset, value) in &[
        (0x14e8, 0x00000000),
        (0x1e58, 0x489c4000),
        (0x1e64, 0x38c49ba6),
        (0x1e60, 0x41a00000),
        (0x1e5c, 0x41a00000),
        (0x1e38, 0x00000000),
        (0x1e3c, 0x3356bf95),
        (0x1e44, 0x3356bf95),
        (0x1e40, 0x00000000),
        (0x1e54, 0x3356bf95),
        (0x1e50, 0x00000000),
        (0x1e48, 0x00000000),
        (0x1e4c, 0x36a7c5ac),
        (0x1e2c, 0x00000000),
        (0x1e30, 0x00000000),
        (0x1e34, 0x00000000),
        (0x1dac, 0x00000000),
        (0x1db4, 0x00000000),
        (0x1db8, 0x00000000),
        (0x1dc0, 0x00000000),
        (0x1dbc, 0x00000000),
        (0x1dc4, 0x00000000),
        (0x1dc8, 0x00000000),
        (0x1dcc, 0x00000000),
        (0x1f44, 0x00000000),
        (0x1dd8, 0x00000000),
        (0x1e18, 0x00000000),
        (0x1e14, 0x00000000),
        (0x1e1c, 0x00000000),
        (0x70, 0x00000000),
        (0x1df8, 0x00000000),
        (0x1e00, 0x00000000),
        (0x1e04, 0xbf800000),
        (0x1e0c, 0xbf800000),
        (0x1e10, 0xbf800000),
        (0x1f44, 0x00000000),
        (0x1f48, 0x00000000),
        (0x1f4c, 0x00000000),
        (0x1f50, 0x00000000),
        (0x1f54, 0x00000000),
        (0x1f58, 0x00000000),
        (0x1f30, 0x00000000),
        (0xb4, 0x3f800000),
        (0x1ef0, 0x3f800000),
        (0xb8, 0x00000000),
        (0x1f04, 0x00000000),
        (0x1f08, 0x00000000),
        (0x1f00, 0x00000000),
        (0x1f0c, 0x00000000),
        (0x1f10, 0x00000000),
        (0x1ed8, 0x00000000),
        (0x1edc, 0x00000000),
        (0x1f3c, 0x00000000),
        (0x1f40, 0x00000000),
        (0x1f5c, 0x00000000),
        (0x1f60, 0x00000000),
        (0x1f64, 0x00000000),
        (0x1f68, 0x00000000),
        (0x1f6c, 0x00000000),
        (0x1f70, 0x00000000),
        (0x1f74, 0x00000000),
        (0x1f78, 0x00000000),
        (0x1f7c, 0x00000000),
        (0x1f80, 0x00000000),
        (0x1f84, 0x00000000),
        (0x1f88, 0x00000000),
        (0x1f8c, 0x00000000),
        (0x1fc0, 0x00000000),
        (0x20e8, 0x00000000),
        (0x20ec, 0x00000000),
        (0x20f0, 0x00000000),
        (0x20f4, 0x00000000),
        (0x20f8, 0x00000000),
        (0x1ef8, 0x00000000),
        (0x1f24, 0xbf800000),
        (0x1f14, 0xbf800000),
        (0x1f18, 0xbf800000),
        (0x1f1c, 0xbf800000),
        (0x1f20, 0xbf800000),
        (0x1f28, 0xbf800000),
        (0x1f2c, 0xbf800000),
        (0x20cc, 0x00000000),
        (0x20d0, 0x00000000),
        (0x20d4, 0x00000000),
        (0x2124, 0x00000000),
        (0x2128, 0x00000000),
        (0x20fc, 0x00000000),
        (0x2104, 0x00000000),
        (0x2108, 0x00000000),
        (0x210c, 0x00000000),
        (0x2110, 0x00000000),
        (0x2114, 0x00000000),
        (0x2118, 0x00000000),
        (0x211c, 0x00000000),
        (0x2120, 0x00000000),
    ] {
        m.w32(object + offset, value);
    }
    m.w32(object + 0x70, 0x60);
    let inverse = slr_x87::X87::from_f32(1.0)
        .div(
            slr_x87::X87::from_f32(m.rf(template + 0x11dc)),
            slr_x87::Precision::F32,
            slr_x87::Rounding::NearestEven,
        )
        .to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven);
    m.wf(object + 0xbc, inverse);
    m.w32(object + 0xf0, m.r32(template + 0x11e0));
    m.w32(object + 0x1ef4, m.r32(parameters + 0xa34));
    m.w32(object + 0x1efc, m.r32(parameters + 0xa3c));
    m.w32(object + 0x2100, m.r32(world));
    let runtime_parameters = if m.r8(template + 0x2e4c) & 1 != 0 {
        clone_parameters(
            m,
            parameters,
            host.expect("clone resource host checked above"),
        )?
    } else {
        parameters
    };
    m.w32(object + 0x1fbc, runtime_parameters);
    for index in 0..count.max(0) as u32 {
        let record = object + 0x11c + index * 0x4a8;
        m.w32(record, index);
        m.w32(record + 0x64, 0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_wheel_force_instruction_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["wheel_force"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let wheel = m.alloc(0x2b4);
            m.alloc(0xec);
            let world = m.alloc(0x400);
            m.w32(template + 0x1f40, 4);
            m.wf(template + 0x1ec4, row["mass"].as_f64().unwrap() as f32);
            for (i, value) in row["gravity"].as_array().unwrap().iter().enumerate() {
                m.wf(world + 0x104 + i as u32 * 4, value.as_f64().unwrap() as f32);
            }
            m.wf(wheel + 0x3c, 0.17);
            m.wf(wheel + 0x40, 2.);
            wheel_force_coefficients(&mut m, template, wheel, world).unwrap();
            assert_eq!(
                serde_json::json!([
                    m.r32(wheel + 0x44),
                    m.r32(wheel + 0x48),
                    m.r32(wheel + 0x4c)
                ]),
                row["words"]
            );
        }
    }
    #[test]
    fn original_wheel_frame_and_spring_anchor() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["wheel_frame"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for row in rows {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let wheels = m.alloc(0x568);
            let entity = m.alloc(0x218);
            let physical = m.alloc(0xa0);
            let definition = m.alloc(0xac);
            let relative = m.alloc(0x50);
            let index = row["index"].as_u64().unwrap() as u32;
            m.w32(template + 0x1f40, 2);
            m.w32(object + 0x13e4, wheels);
            m.w32(object + 0x13bc, entity);
            m.w32(entity + 0x5c, physical);
            m.w32(physical + 0x78, definition);
            m.w32(entity + 0x1c, relative);
            for (i, (center, local)) in [(0.23, 0.11), (-0.41, -0.13), (0.67, 0.17)]
                .into_iter()
                .enumerate()
            {
                m.wf(definition + 0x40 + i as u32 * 4, center);
                m.wf(relative + 4 + i as u32 * 4, local);
            }
            let end = template + 0x1ffc + index * 0xc8;
            let wheel = wheels + index * 0x2b4;
            for offset in 0..0xc8 / 4 {
                m.w32(end - 0xb8 + offset * 4, 0x3e000000 + offset * 0x10001);
            }
            for (i, angle) in row["angles"].as_array().unwrap().iter().enumerate() {
                m.wf(end - 0xac + i as u32 * 4, angle.as_f64().unwrap() as f32);
            }
            for offset in 0..0x2b4 {
                m.w8(wheel + offset, 0xa5);
            }
            m.wf(wheel + 0x5c, 0.93);
            m.wf(wheel + 0x60, 0.31);
            let anchor = prepare_wheel_frame(&mut m, template, object, index).unwrap();
            let words: Vec<_> = (0..0x2b4 / 4).map(|i| m.r32(wheel + i * 4)).collect();
            assert_eq!(serde_json::json!(words), row["words"]);
            assert_eq!(serde_json::json!(anchor), row["anchor"]);
        }
    }
    #[test]
    fn original_wheel_tail_arm_links_and_scalars() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["wheel_tail"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let wheels = m.alloc(0x568);
            let index = row["index"].as_u64().unwrap() as u32;
            let other = row["other"].as_i64().unwrap() as i32;
            for offset in (0..0x2e98).step_by(4) {
                m.w32(template + offset, 0x3e000000 + offset * 15);
            }
            m.w32(template + 0x1f40, 2);
            m.w32(object + 0x13e4, wheels);
            m.w32(template + 0x1ffc + index * 0xc8 - 0x34, other as u32);
            let wheel = wheels + index * 0x2b4;
            for offset in 0..0x2b4 {
                m.w8(wheel + offset, 0xa5);
            }
            finish_wheel_fields(&mut m, template, object, index).unwrap();
            let words: Vec<_> = (0..0x2b4 / 4).map(|i| m.r32(wheel + i * 4)).collect();
            assert_eq!(serde_json::json!(words), row["words"]);
        }
    }
    #[test]
    fn original_wheel_array_placement_construction() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["wheel_storage"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for row in rows {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let count = row["count"].as_u64().unwrap() as u32;
            m.w32(template + 0x1f40, count);
            let wheels = prepare_wheel_storage(&mut m, template, object).unwrap();
            assert_eq!(wheels as u64, row["pointer"].as_u64().unwrap());
            assert_eq!(m.heap_ptr() as u64, row["heap"].as_u64().unwrap());
            let words: Vec<_> = if wheels == 0 {
                vec![]
            } else {
                (0..1 + count * (0x2b4 / 4))
                    .map(|i| m.r32(wheels - 4 + i * 4))
                    .collect()
            };
            assert_eq!(serde_json::json!(words), row["words"]);
        }
    }
    #[test]
    fn original_storage_constructors_preserve_unwritten_gaps() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["shell_rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            let mut m = Space::new();
            let object = m.alloc(0x212c);
            assert_eq!(object as u64, row["body"].as_u64().unwrap());
            for offset in 0..0x212c {
                m.w8(object + offset, row["fill"].as_u64().unwrap() as u8);
            }
            initialize_storage(&mut m, object);
            let words: Vec<_> = (0..0x212c)
                .step_by(4)
                .map(|offset| m.r32(object + offset))
                .collect();
            assert_eq!(serde_json::json!(words), row["words"]);
        }
    }
    #[test]
    fn original_scalar_stage_preserves_surrounding_state() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-original; run re/oracle/native_car_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(data["rows"].as_array().unwrap().len(), 48);
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let template = m.alloc(0x2e98);
            let body = m.alloc(0x212c);
            let parameters = m.alloc(0xb00);
            let world = m.alloc(0x400);
            let val = |key: &str| row[key].as_u64().unwrap() as u32;
            assert_eq!(
                (template, body, parameters, world),
                (
                    val("template"),
                    val("body"),
                    val("parameters"),
                    val("world")
                )
            );
            for offset in 0..0x212c {
                m.w8(body + offset, val("fill") as u8);
            }
            m.w32(template + 0x11dc, val("mass"));
            m.w32(template + 0x11e0, 0x3ec12345);
            m.w32(template + 0x2e48, parameters);
            m.w32(template + 0x177c, val("count"));
            m.w32(parameters + 0xa34, 0x41c80000);
            m.w32(parameters + 0xa3c, 0x3e800000);
            m.w32(world, 0x12345678);
            initialize_scalars(&mut m, template, body, world).unwrap();
            let words: Vec<_> = (0..0x212c)
                .step_by(4)
                .map(|offset| m.r32(body + offset))
                .collect();
            assert_eq!(
                serde_json::json!(words),
                row["words"],
                "mass={:#x} fill={} count={}",
                val("mass"),
                val("fill"),
                val("count")
            );
        }
    }
}

/// Original43e3e0 parameter clone. Curve buffers and weak links are copied
/// through their real allocation/link operations, never as pointer snapshots.
/// The caller retains cfg Host ownership for the three lazy global bindings.
pub fn clone_parameters(
    m: &mut Space,
    source: u32,
    host: &mut dyn slr_physics::cfg::Host,
) -> Result<u32, String> {
    use slr_physics::cfg::{self, Arg, ExtCall, This};
    if source == 0 {
        return Err("NativeCarBoundary: cannot clone null parameters".into());
    }
    for offset in [0, 8] {
        let count = m.r32(source + offset);
        if count > 65536 || (count != 0 && m.r32(source + offset + 4) == 0) {
            return Err("NativeCarBoundary: invalid engine curve count/buffer".into());
        }
    }
    for offset in [0x338, 0x580, 0x7c8, 0xa54] {
        if m.r32(source + offset) > 16 {
            return Err("NativeCarBoundary: parameter record count exceeds sixteen".into());
        }
    }
    let clone = m.alloc(0xc1c);
    // Placement constructors42cec0/45dc30 touch zero weak links and the
    // default sound scales. Fresh storage is zero; all three scales are
    // overwritten from the source immediately below.
    for offset in [0, 8] {
        let count = m.r32(source + offset);
        m.w32(clone + offset, count);
        let array = m.malloc(count * 8);
        m.w32(clone + offset + 4, array);
    }
    for offset in [0, 8] {
        copy(
            m,
            m.r32(clone + offset + 4),
            m.r32(source + offset + 4),
            m.r32(source + offset) * 8,
        );
    }
    copy(m, clone + 0x10, source + 0x10, 0xe8);
    for (table, count_offset) in [(0xf8, 0x338), (0x340, 0x580), (0x588, 0x7c8)] {
        let count = m.r32(source + count_offset);
        copy(m, clone + count_offset, source + count_offset, 8);
        for index in 0..count {
            let from = source + table + index * 0x24;
            let to = clone + table + index * 0x24;
            crate::native_part::bind_reference(m, to, m.r32(from + 12));
            copy(m, to + 16, from + 16, 20);
        }
    }
    m.w32(clone + 0x7d0, m.r32(source + 0x7d0));
    for offset in [0x7d4, 0x7e4, 0x7f4] {
        crate::native_part::bind_reference(m, clone + offset, m.r32(source + offset + 12));
    }
    copy(m, clone + 0x804, source + 0x804, 16);
    for offset in [0x814, 0x824] {
        crate::native_part::bind_reference(m, clone + offset, m.r32(source + offset + 12));
    }
    for index in 0..4 {
        let offset = 0x834 + index * 16;
        crate::native_part::bind_reference(m, clone + offset, m.r32(source + offset + 12));
        m.w32(clone + 0x874 + index * 4, m.r32(source + 0x874 + index * 4));
    }
    for offset in [0x894, 0x8a4, 0x884, 0x8b4, 0x8c4] {
        crate::native_part::bind_reference(m, clone + offset, m.r32(source + offset + 12));
    }
    copy(m, clone + 0x8d4, source + 0x8d4, 8);
    for offset in [0x8dc, 0x8ec] {
        crate::native_part::bind_reference(m, clone + offset, m.r32(source + offset + 12));
    }
    copy(m, clone + 0x8fc, source + 0x8fc, 0x15c);
    copy(
        m,
        clone + 0xa58,
        source + 0xa58,
        m.r32(source + 0xa54) * 0x1c,
    );
    m.w32(clone + 0xc18, 0);
    for (slot, name, index, kind) in [
        (0x63c680, "sl.rpk", 0x6e, 8),
        (0x63c6a0, "particles.rpk", 0x106, 14),
        (0x63c788, "particles.rpk", 0x106, 14),
    ] {
        if m.r32(slot + 8) == 0 {
            let id = host.call(
                m,
                &ExtCall {
                    f: cfg::EXT_LOOKUP,
                    this: This::None,
                    a: vec![Arg::S(name.into()), Arg::U(index)],
                },
            );
            host.call(
                m,
                &ExtCall {
                    f: cfg::EXT_BIND,
                    this: This::Addr(slot),
                    a: vec![Arg::U(id), Arg::U(kind), Arg::U(0)],
                },
            );
        }
    }
    Ok(clone)
}
fn copy(m: &mut Space, destination: u32, source: u32, size: u32) {
    for offset in 0..size {
        m.w8(destination + offset, m.r8(source + offset));
    }
}

#[cfg(test)]
mod clone_tests {
    use super::*;
    use slr_physics::cfg::{self, Arg, ExtCall, Host, This};
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
                _ => panic!("unexpected clone collaborator"),
            }
        }
    }
    #[test]
    fn original_parameter_clone_deep_curves_links_and_lazy_globals() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-clone-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-clone-original");
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
            for slot in [0x63c680, 0x63c6a0, 0x63c788] {
                m.w32(slot + 8, u32::from(row["cached"].as_bool().unwrap()));
            }
            m.set_heap_ptr(row["start"].as_u64().unwrap() as u32);
            let mut host = ControlledHost { calls: vec![] };
            let cloned =
                clone_parameters(&mut m, row["source"].as_u64().unwrap() as u32, &mut host)
                    .unwrap();
            assert_eq!(cloned as u64, row["result"].as_u64().unwrap());
            assert_eq!(m.heap_ptr() as u64, row["end"].as_u64().unwrap());
            for (i, value) in row["after"].as_array().unwrap().iter().enumerate() {
                let address = base + i as u32 * 4;
                assert_eq!(
                    m.r32(address) as u64,
                    value.as_u64().unwrap(),
                    "count={} cached={} address={address:#x}",
                    row["count"],
                    row["cached"]
                );
            }
            for entry in row["cache_after"].as_array().unwrap() {
                let slot = entry[0].as_u64().unwrap() as u32;
                for (i, value) in entry[1].as_array().unwrap().iter().enumerate() {
                    assert_eq!(m.r32(slot + i as u32 * 4) as u64, value.as_u64().unwrap());
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

/// Original44b2a8..44b3e2: wheel arm/hub scalar configuration after optional
/// physical wheel entities have been attached. Physics state is not fabricated.
pub fn finish_wheel_fields(
    m: &mut Space,
    template: u32,
    object: u32,
    index: u32,
) -> Result<(), String> {
    let count = m.r32(template + 0x1f40);
    if index >= count || count > 16 {
        return Err("NativeCarBoundary: wheel index/count outside configured records".into());
    }
    let wheels = m.r32(object + 0x13e4);
    if wheels == 0 {
        return Err("NativeCarBoundary: missing runtime wheel array".into());
    }
    let wheel = wheels + index * 0x2b4;
    let end = template + 0x1ffc + index * 0xc8;
    copy(m, wheel + 0x234, end - 0x50, 0x1c);
    let other = m.r32(end - 0x34) as i32;
    if other >= count as i32 {
        return Err("NativeCarBoundary: configured wheel link index outside array".into());
    }
    m.w32(
        wheel + 0x250,
        if other < 0 {
            0
        } else {
            wheels + other as u32 * 0x2b4
        },
    );
    copy(m, wheel + 0x254, end - 0x30, 0x28);
    for (offset, value) in [
        (0x184, 0x47435000),
        (0x190, 0x3a83126f),
        (0x18c, 0x41a00000),
        (0x188, 0x41a00000),
        (0x17c, 0x41a00000),
        (0x178, 0x41a00000),
        (0x174, 0x46ea6000),
        (0x180, 0x3ada740e),
        (0x164, 0),
        (0x168, 0),
        (0x15c, 0),
        (0x160, 0),
        (0x16c, 0),
        (0x170, 0),
        (0x2b0, 0),
    ] {
        m.w32(wheel + offset, value);
    }
    Ok(())
}

//48aea0 resolves the object's backbone resource to its actual physics payload.
//The owning entity graph already carries the corresponding resource identity.
#[cfg(test)]
fn backbone_entity(m: &Space, object: u32) -> Result<u32, String> {
    let target = m.r32(object + 0x38);
    let root = m.r32(object + 0x13bc);
    let mut pending = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(e) = pending.pop() {
        if e == 0 || !seen.insert(e) {
            continue;
        }
        if m.r32(e + 0x54) == target {
            return Ok(e);
        }
        let mut child = m.r32(e + 0x24);
        let mut siblings = std::collections::HashSet::new();
        while child != 0 {
            if seen.contains(&child) || !siblings.insert(child) {
                return Err("NativeCarBoundary: cyclic backbone graph".into());
            }
            pending.push(child);
            child = m.r32(child + 0x28);
        }
    }
    Err("NativeCarBoundary: backbone resource has no bound physics entity".into())
}
/// Original44ab58..44ac71: wheel positions, hub/arm anchors and basis before
/// spring allocation. Returns the original local spring anchor stack vector.
#[cfg(test)]
pub fn prepare_wheel_frame(
    m: &mut Space,
    template: u32,
    object: u32,
    index: u32,
) -> Result<[u32; 3], String> {
    let backbone = backbone_entity(m, object)?;
    prepare_wheel_frame_for_backbone(m, template, object, index, backbone)
}
fn prepare_wheel_frame_for_backbone(
    m: &mut Space,
    template: u32,
    object: u32,
    index: u32,
    backbone: u32,
) -> Result<[u32; 3], String> {
    let count = m.r32(template + 0x1f40);
    let wheels = m.r32(object + 0x13e4);
    if index >= count || count > 16 || wheels == 0 {
        return Err("NativeCarBoundary: invalid wheel storage/index".into());
    }
    let entity = m.r32(object + 0x13bc);
    let body = m.r32(entity + 0x5c);
    let definition = m.r32(body + 0x78);
    let relative = m.r32(backbone + 0x1c);
    if entity == 0 || body == 0 || definition == 0 || relative == 0 {
        return Err(
            "NativeCarBoundary: wheel frame needs live body definition and relative transform"
                .into(),
        );
    }
    let wheel = wheels + index * 0x2b4;
    let end = template + 0x1ffc + index * 0xc8;
    let center = std::array::from_fn::<_, 3, _>(|axis| {
        m.rf(definition + 0x40 + axis as u32 * 4) - m.rf(relative + 4 + axis as u32 * 4)
    });
    let offset = [
        center[0],
        (m.rf(wheel + 0x5c) - m.rf(wheel + 0x60)) + center[1],
        center[2],
    ];
    let mut anchor = [0; 3];
    for axis in 0..3 {
        let value = offset[axis] + m.rf(end - 0xb8 + axis as u32 * 4);
        m.wf(wheel + 0x78 + axis as u32 * 4, value);
        m.wf(
            wheel + 0x27c + axis as u32 * 4,
            offset[axis] + m.rf(end - 8 + axis as u32 * 4),
        );
        m.wf(
            wheel + 0x288 + axis as u32 * 4,
            offset[axis] + m.rf(end + 4 + axis as u32 * 4),
        );
        anchor[axis] = (value + m.rf(relative + 4 + axis as u32 * 4)).to_bits();
    }
    let matrix = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
        m.rf(end - 0xac),
        m.rf(end - 0xa8),
        m.rf(end - 0xa4),
    ));
    for (i, value) in matrix.m.into_iter().enumerate() {
        m.wf(wheel + 0x90 + (i / 3 * 16 + i % 3 * 4) as u32, value);
    }
    copy(m, wheel + 0x110, wheel + 0x90, 48);
    Ok(anchor)
}

/// Original49a760/49a810 fresh-root reparenting used by44a250. A root with
/// an existing relative record needs destruction/reaggregation first; preserve
/// that explicit boundary instead of dropping its lifetime effects.
pub fn reparent_root(m: &mut Space, entity: u32, parent: u32) -> Result<(), String> {
    if entity == 0 || parent == 0 {
        return Err("NativeCarBoundary: cannot reparent null entity".into());
    }
    if m.r32(entity + 0x1c) != 0 {
        return Err("NativeCarBoundary: reparenting an existing relative record requires original49a760 destruction".into());
    }
    let old = m.r32(entity + 0x20);
    if old != 0 {
        let mut current = m.r32(old + 0x24);
        let mut previous = 0;
        let mut seen = std::collections::HashSet::new();
        while current != 0 {
            if !seen.insert(current) {
                return Err("NativeCarBoundary: cyclic physics child list".into());
            }
            if current == entity {
                let next = m.r32(entity + 0x28);
                m.w32(
                    if previous == 0 {
                        old + 0x24
                    } else {
                        previous + 0x28
                    },
                    next,
                );
                break;
            }
            previous = current;
            current = m.r32(current + 0x28);
        }
    }
    m.w32(entity + 0x20, 0);
    m.w32(entity + 0x28, m.r32(parent + 0x24));
    m.w32(parent + 0x24, entity);
    m.w32(entity + 0x20, parent);
    let body = m.r32(parent + 0x5c);
    if body == 0 {
        return Err("NativeCarBoundary: physics parent has no physical body".into());
    }
    if m.r32(body + 0x9c) & 0x100 == 0 {
        if crate::native_stack::active(m) {
            crate::native_stack::call(m, 0x49a83b, &[entity], None, |m| {
                crate::native_entity::update_relative(m, entity)
            })?;
        } else {
            crate::native_entity::update_relative(m, entity)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod reparent_tests {
    use super::*;
    #[test]
    fn original_fresh_roots_reparent_with_and_without_previous_parent() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-reparent-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-reparent-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            let mut m = Space::new();
            let base = slr_physics::space::HEAP_BASE;
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(base + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(row["start"].as_u64().unwrap() as u32);
            reparent_root(
                &mut m,
                row["entity"].as_u64().unwrap() as u32,
                row["parent"].as_u64().unwrap() as u32,
            )
            .unwrap();
            assert_eq!(m.heap_ptr() as u64, row["end"].as_u64().unwrap());
            let words: Vec<_> = (base..m.heap_ptr())
                .step_by(4)
                .map(|address| m.r32(address))
                .collect();
            assert_eq!(serde_json::json!(words), row["after"]);
        }
    }
}

/// Live collaborators required by44a250. Existing PhysicalHost creates real
/// resource identities; cfg Host owns lazy bindings. Activation is4a7990,
/// distinct from4a7a80 (the PhysicalHost method currently named `wake`).
pub trait CarHost: crate::native_part::PhysicalHost + slr_physics::cfg::Host {
    fn physics_entity(&mut self, _m: &mut Space, _resource: u32) -> Result<u32, String> {
        Err("NativeCarBoundary: physical resource get-data collaborator missing".into())
    }
    fn activate(&mut self, m: &mut Space, entity: u32) -> Result<(), String>;
    fn attach_parameter(
        &mut self,
        m: &mut Space,
        object: u32,
        destination: u32,
        record: u32,
    ) -> Result<(), String>;
    fn set_detail_window(
        &mut self,
        _m: &mut Space,
        _resource: u32,
        _lower: u32,
        _upper: u32,
    ) -> Result<(), String> {
        Err("NativeCarBoundary: LOD payload update requires original resource vcall+0c".into())
    }
    fn refresh_render(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _lod: u32,
        wheels: bool,
    ) -> Result<(), String> {
        Err(format!(
            "NativeCarBoundary: render refresh requires original{}",
            if wheels { "44c1f0" } else { "46f9e0" }
        ))
    }
    fn notify_car_add(&mut self, _m: &mut Space, _resource: u32, _id: u32) -> Result<(), String> {
        Err("NativeCarBoundary: car_add requires original426800 message dispatch".into())
    }
    fn or_native_payload_flags(
        &mut self,
        _m: &mut Space,
        _resource: u32,
        _flags: u32,
    ) -> Result<(), String> {
        Err("NativeCarBoundary: final native payload requires original resource vcall+0c".into())
    }
}
/// Result of actual construction through44a95e. This is deliberately a stage,
/// not a publishable configured car: springs, wheel entities, render retargeting
/// and final flags/lifetimes still have to complete.
pub struct CarStage {
    pub base_entity: Option<u32>,
    pub root_entity: Option<u32>,
    native_frame: crate::native_stack::Frame,
    backbone: u32,
}
/// The source template and newly allocated car instance being constructed.
struct CarInstance {
    template: u32,
    object: u32,
}
/// Construct44a250 through the car_root reparenting stage using the verified
/// base Part graph and real resource callbacks. Caller allocates0x212c bytes.
pub fn begin_runtime(
    m: &mut Space,
    template: u32,
    parent: u32,
    parameters: &str,
    object: u32,
    world: u32,
    host: &mut dyn CarHost,
) -> Result<CarStage, String> {
    use crate::native_stack::{self as stack, Reg};
    const PARAMETERS: u32 = 0x3000_fa00;
    let frame = stack::enter(m, &[parent, PARAMETERS], Some(template));
    stack::alloc(m, 0xf8);
    for r in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
        stack::push_reg(m, r);
    }
    stack::set_reg(m, Reg::Edi, template);
    stack::set_reg(m, Reg::Ebx, 0);
    stack::store(m, 0x10, template);
    stack::external(m, 0x44a27b, &[0x212c], None, |_| object);
    stack::set_reg(m, Reg::Esi, object);
    let result = begin_runtime_inner(
        m,
        CarInstance { template, object },
        parent,
        parameters,
        world,
        host,
        frame,
    );
    if result.is_err() {
        stack::leave(m, frame);
    }
    result
}
fn begin_runtime_inner(
    m: &mut Space,
    instance: CarInstance,
    parent: u32,
    parameters: &str,
    world: u32,
    host: &mut dyn CarHost,
    frame: crate::native_stack::Frame,
) -> Result<CarStage, String> {
    use crate::native_stack::{self as stack, Reg};
    let CarInstance { template, object } = instance;
    initialize_storage(m, object);
    let parameters_ptr = stack::load(m, 0x110);
    let base = stack::call(
        m,
        0x44a415,
        &[parent, parameters_ptr, object],
        Some(template),
        |m| crate::native_part::initialize_phys(m, template, parent, parameters, object, host),
    )?;
    stack::set_reg(m, Reg::Ebp, object);
    stack::store(m, 0x28, object);
    crate::native_part::bind_reference(m, object + 0x13c4, m.r32(object + 0x28));
    if let Some(entity) = base {
        if m.r32(object + 0x13bc) != 0 {
            return Err(
                "NativeCarBoundary: nonfresh entity alias destruction requires original45fb80"
                    .into(),
            );
        }
        m.w32(object + 0x13bc, entity);
        m.w32(object + 0x13c0, m.r32(entity));
        m.w32(entity, object + 0x13bc);
        m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
    }
    initialize_scalars_with_host(m, template, object, world, host)?;
    let source = m.r32(template + 0x2e48);
    let count = m.r32(source + 0xa54);
    if count > 16 {
        return Err("NativeCarBoundary: parameter attachment count exceeds sixteen".into());
    }
    for index in 0..count {
        host.attach_parameter(
            m,
            object,
            object + 0x13e8 + index * 0x10,
            source + 0xa58 + index * 0x1c,
        )?;
    }
    let wheels = prepare_wheel_storage(m, template, object)?;
    if wheels == 0 {
        return Ok(CarStage {
            base_entity: base,
            root_entity: None,
            native_frame: frame,
            backbone: 0,
        });
    }
    let backbone = host.physics_entity(m, m.r32(object + 0x38))?;
    if backbone == 0 {
        return Err("NativeCarBoundary: backbone resource has null physics payload".into());
    }
    let mut entity = backbone;
    stack::store(m, 0x2c, backbone);
    //44a858 resolves compound-relative aliases to their root.
    let mut seen = std::collections::HashSet::new();
    while m.r32(entity + 0x58) & 4 != 0 {
        if !seen.insert(entity) {
            return Err("NativeCarBoundary: cyclic compound parent chain".into());
        }
        entity = m.r32(entity + 0x20);
        if entity == 0 {
            return Err("NativeCarBoundary: relative entity has no parent".into());
        }
    }
    if m.r32(entity + 0x58) & 0x80 != 0 {
        host.activate(m, entity)?;
    }
    let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
    let position = std::array::from_fn(|axis| m.r32(state + axis as u32 * 4));
    stack::set_reg(m, Reg::Esi, entity);
    stack::set_reg(m, Reg::Ebx, 0);
    stack::push(m, state + 0x40);
    stack::push(m, state);
    stack::push(m, 0);
    let body = stack::call(
        m,
        0x44a8d0,
        &[
            1f32.to_bits(),
            5,
            1f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
        ],
        Some(world),
        |m| crate::native_geometry::body(m, 1f32.to_bits(), 5, [1f32.to_bits(); 3], None),
    )?;
    //49ab60 is cdecl: the six body arguments include the retained zero shape.
    //The CALL helper removed five words, leaving the explicit shape word.
    stack::add_sp(m, 4);
    let root = stack::call(m, 0x44a8dc, &[body], Some(world), |m| {
        crate::native_entity::root(m, body, position, Some(state + 0x40))
    })?;
    stack::add_sp(m, 8);
    stack::set_reg(m, Reg::Edi, root);
    let resource = host.create_physics(m, object, root, "car_root")?;
    if resource == 0 {
        return Err("NativeCarBoundary: car_root resource construction returned null".into());
    }
    crate::native_part::bind_reference(m, root + 0x48, resource);
    crate::native_part::bind_reference(m, root + 0x38, m.r32(object + 0xc));
    stack::external(m, 0x44a95e, &[entity, root], Some(world), |m| {
        reparent_root(m, entity, root)
    })?;
    //44a975 derives an unspecified configured total mass from the actual
    //base body's inverse mass; this is performed before per-wheel forces.
    if m.rf(template + 0x1ec4) <= 0.0 {
        let physical = m.r32(m.r32(object + 0x13bc) + 0x5c);
        let value = slr_x87::X87::from_f32(1.)
            .div(
                slr_x87::X87::from_f32(m.rf(physical + 0x14)),
                slr_x87::Precision::F32,
                slr_x87::Rounding::NearestEven,
            )
            .to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven);
        m.wf(template + 0x1ec4, value);
    }
    Ok(CarStage {
        base_entity: base,
        root_entity: Some(root),
        native_frame: frame,
        backbone,
    })
}

/// Original44ad54..44adb0 force and damping coefficients. Keep the x87 value
/// live between the three stores, as the original does; no captured coefficient
/// is used by construction.
pub fn wheel_force_coefficients(
    m: &mut Space,
    template: u32,
    wheel: u32,
    world: u32,
) -> Result<(), String> {
    use slr_x87::{Precision, Rounding, X87};
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let count = m.r32(template + 0x1f40) as i32;
    if count <= 0 || count > 16 {
        return Err(
            "NativeCarBoundary: wheel force requires a positive configured wheel count".into(),
        );
    }
    let f = |value: u32| X87::from_f32(f32::from_bits(value));
    let x = f(m.r32(world + 0x104));
    let y = f(m.r32(world + 0x108));
    let z = f(m.r32(world + 0x10c));
    let value = x
        .mul(x, pc, rc)
        .add(y.mul(y, pc, rc), pc, rc)
        .add(z.mul(z, pc, rc), pc, rc)
        .sqrt(pc, rc)
        .mul(f(m.r32(template + 0x1ec4)), pc, rc)
        .div(X87::from_f32(count as f32), pc, rc)
        .div(f(m.r32(wheel + 0x3c)), pc, rc);
    m.w32(wheel + 0x44, value.to_f32(pc, rc).to_bits());
    let value = value.mul(f(m.r32(wheel + 0x40)), pc, rc);
    m.w32(wheel + 0x48, value.to_f32(pc, rc).to_bits());
    let value = value.mul(f(0x3f333333), pc, rc);
    m.w32(wheel + 0x4c, value.to_f32(pc, rc).to_bits());
    Ok(())
}
/// Construct one wheel through44adee using configured cells, real physical
/// spring graph and actual gravity/mass. Optional wheelbone/hub/arm entities
/// and final scalar fields are subsequent constructor stages.
fn initialize_wheel_core(
    m: &mut Space,
    template: u32,
    object: u32,
    index: u32,
    root: u32,
    world: u32,
    backbone: u32,
) -> Result<u32, String> {
    let count = m.r32(template + 0x1f40);
    let wheels = m.r32(object + 0x13e4);
    if index >= count || count > 16 || wheels == 0 {
        return Err("NativeCarBoundary: invalid wheel storage/index".into());
    }
    let wheel = wheels + index * 0x2b4;
    crate::wheel::copy_template_fields(m, wheel, object, template, index);
    let anchor = prepare_wheel_frame_for_backbone(m, template, object, index, backbone)?;
    if crate::native_stack::active(m) {
        use crate::native_stack::{self as stack, Reg};
        stack::set_reg(m, Reg::Ebp, wheel);
        stack::set_reg(m, Reg::Ebx, template + 0x1ffc + index * 0xc8);
        stack::set_reg(m, Reg::Esi, wheel + 0x90);
        stack::set_reg(m, Reg::Edi, wheel + 0x140);
    }
    let spring = crate::native_spring::create(
        m,
        root,
        object,
        anchor,
        m.r32(wheel + 0x5c),
        m.r32(wheel + 0x64),
    )?;
    if m.r32(wheel + 0x30) != 0 {
        return Err("NativeCarBoundary: wheel spring replacement requires original4776f0".into());
    }
    m.w32(wheel + 0x30, spring);
    m.w32(wheel + 0x34, m.r32(spring + 0xc));
    m.w32(spring + 0xc, wheel + 0x30);
    m.w32(spring + 0x10, m.r32(spring + 0x10).wrapping_add(1));
    wheel_force_coefficients(m, template, wheel, world)?;
    crate::native_spring::configure_from_wheel(m, spring, wheel)?;
    Ok(spring)
}

fn transform_point(m: &Space, entity: u32, position: [u32; 3]) -> [u32; 3] {
    use slr_x87::{Precision, Rounding, X87};
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
    let f = |v: u32| X87::from_f32(f32::from_bits(v));
    std::array::from_fn(|row| {
        let order = if row == 0 { [1, 2, 0] } else { [0, 1, 2] };
        let product = |column: usize| {
            f(m.r32(state + 0x40 + row as u32 * 16 + column as u32 * 4)).mul(
                f(position[column]),
                pc,
                rc,
            )
        };
        let rotated = product(order[0])
            .add(product(order[1]), pc, rc)
            .add(product(order[2]), pc, rc)
            .to_f32(pc, rc);
        f(m.r32(state + row as u32 * 4))
            .add(X87::from_f32(rotated), pc, rc)
            .to_f32(pc, rc)
            .to_bits()
    })
}
fn wheel_entity(
    m: &mut Space,
    root: u32,
    physical: u32,
    position: [u32; 3],
    rotation: [u32; 9],
    return_pc: u32,
) -> Result<u32, String> {
    const MATRIX: u32 = 0x3000_fe00;
    let saved: [u32; 12] = std::array::from_fn(|i| m.r32(MATRIX + i as u32 * 4));
    crate::native_entity::write_matrix(m, MATRIX, rotation);
    let result = if crate::native_stack::active(m) {
        use crate::native_stack as stack;
        let (p, r) = if return_pc == 0x44af34 {
            (stack::sp(m) + 0x3c, stack::sp(m) + 0xc8)
        } else {
            (stack::sp(m) + 0x30, stack::sp(m) + 0x98)
        };
        for (i, v) in position.into_iter().enumerate() {
            m.w32(p + i as u32 * 4, v);
        }
        crate::native_entity::write_matrix(m, r, rotation);
        stack::call(m, return_pc, &[root, physical, p, r], None, |m| {
            crate::native_entity::child(m, root, physical, position, Some(r))
        })
    } else {
        crate::native_entity::child(m, root, physical, position, Some(MATRIX))
    };
    for (i, value) in saved.into_iter().enumerate() {
        m.w32(MATRIX + i as u32 * 4, value);
    }
    let entity = result?;
    m.w32(entity + 0x58, m.r32(entity + 0x58) | 0x8020);
    Ok(entity)
}
fn attach_wheel_resource(
    m: &mut Space,
    object: u32,
    wheel_link: u32,
    entity: u32,
    name: &str,
    host: &mut dyn crate::native_part::PhysicalHost,
) -> Result<(), String> {
    let resource = host.create_physics(m, object, entity, name)?;
    if resource == 0 {
        return Err(format!(
            "NativeCarBoundary: {name} resource factory returned null"
        ));
    }
    crate::native_part::bind_reference(m, entity + 0x48, resource);
    crate::native_part::bind_reference(m, wheel_link, resource);
    Ok(())
}
/// Original44adee..44b2a8 optional physical wheelbone, hub and arm entities.
/// Geometry pointers are actual configured bodies, not resource IDs.
pub fn initialize_wheel_entities(
    m: &mut Space,
    template: u32,
    object: u32,
    index: u32,
    root: u32,
    host: &mut dyn crate::native_part::PhysicalHost,
) -> Result<(), String> {
    let count = m.r32(template + 0x1f40);
    let wheels = m.r32(object + 0x13e4);
    if index >= count || count > 16 || wheels == 0 {
        return Err("NativeCarBoundary: invalid wheel storage/index".into());
    }
    let base = m.r32(object + 0x13bc);
    let state = base + 0x64 + m.r32(base + 0x20c) * 0xd4;
    let wheel = wheels + index * 0x2b4;
    let end = template + 0x1ffc + index * 0xc8;
    if m.r32(end - 0x70) != 0 {
        let parameters = m.r32(object + 0x1fbc);
        let spring = m.r32(wheel + 0x30);
        let relative = m.r32(base + 0x1c);
        if parameters == 0 || spring == 0 || relative == 0 {
            return Err("NativeCarBoundary: wheelbone requires runtime parameters, spring and relative frame".into());
        }
        let angle = (m.rf(parameters + 0xa0c) * m.rf(wheel + 0xc8)) * m.rf(object + 0x1dac);
        let matrix = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(angle, 0., 0.))
            .m
            .map(f32::to_bits);
        let rotation = crate::native_entity::matrix_inplace_product(
            matrix,
            crate::native_entity::read_matrix(m, state + 0x40),
        );
        let extent = if m.rf(spring + 0xd8) < m.rf(spring + 0x38) {
            m.rf(spring + 0xd8)
        } else {
            m.rf(spring + 0x38)
        };
        let delta = m.rf(wheel + 0x60) - extent;
        let local = std::array::from_fn(|axis| {
            let position =
                m.rf(wheel + 0x78 + axis as u32 * 4) + m.rf(relative + 4 + axis as u32 * 4);
            (m.rf(wheel + 0x94 + axis as u32 * 16) * delta + position).to_bits()
        });
        let position = transform_point(m, base, local);
        let entity = wheel_entity(m, root, m.r32(end - 0x70), position, rotation, 0x44af34)?;
        attach_wheel_resource(m, object, wheel, entity, "wheelbone", host)?;
    }
    if m.r32(end - 0x68) != 0 {
        let local = std::array::from_fn(|axis| m.r32(end - 0xb8 + axis as u32 * 4));
        let position = transform_point(m, base, local);
        let identity = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(0., 0., 0.))
            .m
            .map(f32::to_bits);
        let rotation = crate::native_entity::matrix_inplace_product(
            identity,
            crate::native_entity::read_matrix(m, state + 0x40),
        );
        let physical = m.r32(end - 0x68);
        let hub = wheel_entity(m, root, physical, position, rotation, 0x44b151)?;
        attach_wheel_resource(m, object, wheel + 0x10, hub, "hub", host)?;
        let arm = wheel_entity(m, root, physical, position, rotation, 0x44b20c)?;
        attach_wheel_resource(m, object, wheel + 0x20, arm, "a_arm", host)?;
    }
    Ok(())
}

#[cfg(test)]
mod point_tests {
    use super::*;
    #[test]
    fn original_local_to_world_point_instruction_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-point-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-point-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 12);
        for row in rows {
            let mut m = Space::new();
            let entity = m.alloc(0x218);
            let active = row["active"].as_u64().unwrap() as u32;
            m.w32(entity + 0x20c, active);
            let state = entity + 0x64 + active * 0xd4;
            for (i, value) in row["position"].as_array().unwrap().iter().enumerate() {
                m.w32(state + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            for (i, value) in row["matrix"].as_array().unwrap().iter().enumerate() {
                m.w32(
                    state + 0x40 + (i / 3 * 16 + i % 3 * 4) as u32,
                    value.as_u64().unwrap() as u32,
                );
            }
            let local = std::array::from_fn(|i| row["local"][i].as_u64().unwrap() as u32);
            assert_eq!(
                serde_json::json!(transform_point(&m, entity, local)),
                row["result"]
            );
        }
    }
}

/// Continue actual44a250 wheel construction, compound refresh and physical
/// flags through44b47e. Successful return still requires camera/resource/slot
/// and render finalization before the car may be published.
pub fn finish_physics(
    m: &mut Space,
    template: u32,
    object: u32,
    world: u32,
    stage: &CarStage,
    host: &mut dyn CarHost,
) -> Result<(), String> {
    let count = m.r32(template + 0x1f40);
    if count > 16 {
        return Err("NativeCarBoundary: wheel count exceeds sixteen".into());
    }
    if count != 0 {
        let root = stage
            .root_entity
            .ok_or("NativeCarBoundary: missing constructed car_root")?;
        for index in 0..count {
            initialize_wheel_core(m, template, object, index, root, world, stage.backbone)?;
            initialize_wheel_entities(m, template, object, index, root, host)?;
            finish_wheel_fields(m, template, object, index)?;
        }
        host.or_flags(m, object, 0x20)?;
    }
    if let Some(entity) = stage.base_entity {
        crate::native_stack::call(m, 0x44b435, &[entity], Some(world), |m| {
            crate::native_entity::compound(m, entity)
        })?;
    }
    let flags = m.r32(object + 0x70);
    m.w32(
        object + 0x70,
        if m.r32(template + 0x1780) as i32 >= 0 {
            flags | 0x4000
        } else {
            flags & !0x4000
        },
    );
    if let Some(mut entity) = stage.base_entity {
        if m.r32(entity + 0x58) & 0x80 == 0 {
            let mut visited = std::collections::HashSet::new();
            while m.r32(entity + 0x58) & 4 != 0 {
                if !visited.insert(entity) {
                    return Err("NativeCarBoundary: cyclic compound parent chain".into());
                }
                entity = m.r32(entity + 0x20);
                if entity == 0 {
                    return Err("NativeCarBoundary: relative entity has no parent".into());
                }
            }
            host.wake(m, entity)?;
        }
    }
    Ok(())
}

/// Original44b486..44b512 car camera follower records. Each record owns a real
/// intrusive entity reference and points to its configured0x28-byte descriptor.
/// This is distinct from48b940's earlier camera resource attachments.
pub fn install_aux_cameras(
    m: &mut Space,
    template: u32,
    object: u32,
    entity: u32,
) -> Result<(), String> {
    if entity == 0 {
        return Ok(());
    }
    let count = m.r32(template + 0x2bc4) as i32;
    if count > 16 {
        return Err("NativeCarBoundary: auxiliary camera count exceeds sixteen".into());
    }
    for index in 0..count.max(0) as u32 {
        let record = m.alloc(0x1c);
        m.w32(record, 0x5f0fc8);
        m.w32(record + 4, 0);
        m.w32(record + 8, 0);
        m.w32(record + 0xc, template + 0x2bc8 + index * 0x28);
        m.w32(record + 0x10, entity);
        m.w32(record + 0x14, m.r32(entity));
        m.w32(entity, record + 0x10);
        m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
        m.w32(record + 0x18, 0);
        let previous = m.r32(object + 0x2090);
        if previous == 0 {
            return Err("NativeCarBoundary: auxiliary camera list sentinel missing".into());
        }
        m.w32(previous + 4, record);
        m.w32(record + 8, previous);
        m.w32(record + 4, object + 0x2088);
        m.w32(object + 0x2090, record);
    }
    Ok(())
}
/// Original44b512..44b79d resource pose/type and configured slot retargeting.
/// Owner/resource pointers must name actual raw resource projections created
/// by the factory. LOD and render-engine finalization follow this stage.
pub fn finish_layout(m: &mut Space, template: u32, object: u32) -> Result<(), String> {
    let resource = m.r32(object + 0xc);
    if resource != 0 {
        let entity = m.r32(object + 0x13bc);
        if entity == 0 {
            return Err("NativeCarBoundary: resource pose requires base physics entity".into());
        }
        let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
        copy(m, resource + 0x84, state, 12);
        for offset in [0x90, 0x94, 0x98] {
            m.w32(resource + offset, 10f32.to_bits());
        }
        m.w32(resource + 0x80, 0);
    }
    m.w32(object + 0x5c, 3);
    m.w32(object + 0xb0, 3);
    let count = m.r32(template + 0x1150);
    if count > 65536 {
        return Err("NativeCarBoundary: configured slot count is invalid".into());
    }
    let slots = m.r32(object + 0x78);
    if count != 0 && slots == 0 {
        return Err("NativeCarBoundary: configured slots have not been constructed".into());
    }
    let mut descriptor = m.r32(template + 0x115c);
    let wheels = m.r32(object + 0x13e4);
    for index in 0..count {
        if descriptor == 0 || m.r32(descriptor + 4) == 0 {
            return Err("NativeCarBoundary: configured slot list shorter than count".into());
        }
        let slot = slots + index * 0x68;
        let kind = m.r32(descriptor + 0x4c) & 0xffff;
        let target = match kind {
            0 => Some(m.r32(object + 0x38)),
            11 => {
                let reference = m.r32(object + 0xf4);
                if reference == 0 {
                    None
                } else {
                    Some(m.r32(reference + 0x54))
                }
            }
            101..=400 => {
                let (wheel_index, offset) = if kind <= 200 {
                    (kind - 101, 0xc)
                } else if kind <= 300 {
                    (kind - 201, 0x1c)
                } else {
                    (kind - 301, 0x2c)
                };
                if wheels == 0 || wheel_index >= m.r32(template + 0x1f40) {
                    return Err("NativeCarBoundary: slot targets a missing wheel".into());
                }
                Some(m.r32(wheels + wheel_index * 0x2b4 + offset))
            }
            _ => None,
        };
        if let Some(target) = target {
            crate::native_part::bind_reference(m, slot + 0x18, target);
        }
        copy(m, slot + 0x2c, descriptor + 0xc, 12);
        copy(m, slot + 0x38, descriptor + 0x18, 48);
        let next = m.r32(descriptor + 4);
        descriptor = if next != 0 && m.r32(next + 4) != 0 {
            next
        } else {
            0
        };
    }
    Ok(())
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn original_auxiliary_camera_nodes_and_slot_layout() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-layout-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-layout-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(data["cameras"].as_array().unwrap().len(), 3);
        assert_eq!(data["layouts"].as_array().unwrap().len(), 2);
        for group in ["cameras", "layouts"] {
            for row in data[group].as_array().unwrap() {
                let mut m = Space::new();
                let base = slr_physics::space::HEAP_BASE;
                for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, value.as_u64().unwrap() as u32);
                }
                m.set_heap_ptr(row["start"].as_u64().unwrap() as u32);
                let template = row["template"].as_u64().unwrap() as u32;
                let object = row["body"].as_u64().unwrap() as u32;
                if group == "cameras" {
                    install_aux_cameras(
                        &mut m,
                        template,
                        object,
                        row["entity"].as_u64().unwrap() as u32,
                    )
                    .unwrap();
                } else {
                    finish_layout(&mut m, template, object).unwrap();
                }
                assert_eq!(m.heap_ptr() as u64, row["end"].as_u64().unwrap());
                for (i, value) in row["after"].as_array().unwrap().iter().enumerate() {
                    let address = base + i as u32 * 4;
                    assert_eq!(
                        m.r32(address) as u64,
                        value.as_u64().unwrap(),
                        "{group} count={} address={address:#x}",
                        row["count"]
                    );
                }
            }
        }
    }
}

/// Original44b79d..44b90f final LOD/render/payload operations. The factory's
/// collaborators must execute original resource and render behavior; defaults
/// return explicit boundaries, so this cannot manufacture a finished car.
pub fn finalize_render(
    m: &mut Space,
    template: u32,
    object: u32,
    host: &mut dyn CarHost,
) -> Result<(), String> {
    let resource = m.r32(object + 0xc);
    let count = m.r32(template + 0xfac) as i32;
    if count > 16 {
        return Err("NativeCarBoundary: LOD count exceeds original sixteen levels".into());
    }
    let mut lod = 0;
    if count > 0 {
        let distance = if resource == 0 {
            0.
        } else {
            m.rf(resource + 0x70)
        };
        while lod < count as u32 {
            if !matches!(
                m.rf(template + 0xfb0 + lod * 4).partial_cmp(&distance),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ) {
                break;
            }
            lod += 1;
        }
        if resource != 0 {
            let lower = if lod >= 1 {
                m.r32(template + 0xfac + lod * 4)
            } else {
                0
            };
            let upper = m.r32(template + 0xfb0 + lod * 4);
            host.set_detail_window(m, resource, lower, upper)?;
        }
    }
    host.refresh_render(m, template, object, lod, false)?;
    if m.r32(template + 0x1f40) != 0 {
        host.refresh_render(m, template, object, lod, true)?;
    }
    if m.r8(template + 0x2e4c) & 2 != 0
        && m.r32(object + 8) & 0xffff0000 != 0xffff0000
        && resource != 0
    {
        host.notify_car_add(m, resource, m.r32(object + 8))?;
    }
    if resource != 0 {
        host.or_native_payload_flags(m, resource, 0x4000)?;
    }
    Ok(())
}
/// Finish all constructor stages after begin_runtime. Publication remains the
/// caller's responsibility and is allowed only after this returns successfully.
pub fn finish_runtime(
    m: &mut Space,
    template: u32,
    object: u32,
    world: u32,
    stage: &CarStage,
    host: &mut dyn CarHost,
) -> Result<(), String> {
    let result = (|| {
        finish_physics(m, template, object, world, stage, host)?;
        install_aux_cameras(m, template, object, stage.base_entity.unwrap_or(0))?;
        finish_layout(m, template, object)?;
        finalize_render(m, template, object, host)
    })();
    leave_runtime_stack(m, stage);
    result
}

#[cfg(test)]
mod finalization_tests {
    use super::*;
    struct ControlledHost {
        payload: u32,
        events: Vec<serde_json::Value>,
    }
    impl slr_physics::cfg::Host for ControlledHost {
        fn call(&mut self, _: &mut Space, _: &slr_physics::cfg::ExtCall) -> u32 {
            panic!("finalization does not call cfg host")
        }
    }
    impl crate::native_part::PhysicalHost for ControlledHost {
        fn create_physics(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: &str,
        ) -> Result<u32, String> {
            panic!("finalization does not construct physics")
        }
        fn wake(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            panic!("finalization does not sleep physics")
        }
        fn or_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            panic!("finalization does not update physical flags")
        }
        fn or_resource_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            panic!("finalization does not update resource flags")
        }
    }
    impl CarHost for ControlledHost {
        fn activate(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            panic!("no activation in final stage")
        }
        fn attach_parameter(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            panic!("no camera attachment in final stage")
        }
        fn set_detail_window(
            &mut self,
            m: &mut Space,
            _: u32,
            lower: u32,
            upper: u32,
        ) -> Result<(), String> {
            self.events
                .push(serde_json::json!(["window", lower, upper]));
            m.w32(self.payload + 0x74, lower);
            m.w32(self.payload + 0x78, upper);
            Ok(())
        }
        fn refresh_render(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            lod: u32,
            wheels: bool,
        ) -> Result<(), String> {
            self.events.push(serde_json::json!(["render", wheels, lod]));
            Ok(())
        }
        fn notify_car_add(&mut self, _: &mut Space, _: u32, id: u32) -> Result<(), String> {
            self.events.push(serde_json::json!([
                "notify",
                format!("car_add {} 0", id as i32)
            ]));
            Ok(())
        }
        fn or_native_payload_flags(
            &mut self,
            m: &mut Space,
            _: u32,
            flags: u32,
        ) -> Result<(), String> {
            self.events.push(serde_json::json!(["flags", flags]));
            m.w32(self.payload + 0xc, m.r32(self.payload + 0xc) | flags);
            Ok(())
        }
    }
    #[test]
    fn original_final_lod_render_and_notification_sequence() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-finalize-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-car-finalize-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for row in rows {
            let mut m = Space::new();
            let k = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let resource = m.alloc(0x200);
            m.alloc(0x100);
            let payload = m.alloc(0x100);
            m.w32(k + 0xfac, 3);
            for (i, distance) in [0., 2., 10., 1000.].into_iter().enumerate() {
                m.wf(k + 0xfb0 + i as u32 * 4, distance);
            }
            m.w32(k + 0x1f40, 4);
            m.w32(k + 0x2e4c, 2);
            m.w32(object + 8, 0x12345678);
            m.w32(
                object + 0xc,
                if row["has_resource"].as_bool().unwrap() {
                    resource
                } else {
                    0
                },
            );
            m.wf(resource + 0x70, row["distance"].as_f64().unwrap() as f32);
            m.w32(payload + 0xc, 4);
            m.w32(payload + 0x74, 0xaaaaaaaa);
            m.w32(payload + 0x78, 0xbbbbbbbb);
            let mut host = ControlledHost {
                payload,
                events: vec![],
            };
            finalize_render(&mut m, k, object, &mut host).unwrap();
            assert_eq!(serde_json::json!(host.events), row["events"]);
            let words: Vec<_> = (0..64).map(|i| m.r32(payload + i * 4)).collect();
            assert_eq!(serde_json::json!(words), row["payload_words"]);
        }
    }
}

/// Leave the actual44a250 frame after all staged constructor work succeeds.
pub fn leave_runtime_stack(m: &mut Space, stage: &CarStage) {
    crate::native_stack::leave(m, stage.native_frame);
}

#[cfg(test)]
include!("native_car_graph_tests.rs");
