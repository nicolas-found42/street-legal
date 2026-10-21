//! Original resource tree/base constructors551750/544620/544d40. Host IDs
//! remain supplied by the live factory; these helpers initialize their raw graph.
use slr_physics::space::Space;
/// Original619d10 type flags, before dynamic instance flags are ORed in by
/// the resource factory539880/539fe0.
pub fn type_flags(kind: u32) -> u32 {
    match kind {
        2 | 3 | 5 => 0x100,
        _ => 0,
    }
}
/// Original551750 tree node with two parallel child-list links.
fn tree(m: &mut Space, p: u32) {
    for off in [4, 8, 0xc, 0x10] {
        m.w32(p + off, 0);
    }
    m.w32(p + 0x1c, 0x5f2af8);
    m.w32(p + 0x30, 0x5f2af8);
    for index in 0..2 {
        m.w32(p + 0x20 + index * 8, p + 0x30);
        m.w32(p + 0x24 + index * 8, 0);
        m.w32(p + 0x34 + index * 8, 0);
        m.w32(p + 0x38 + index * 8, p + 0x1c);
    }
    m.w32(p + 0x18, 0x5f2af4);
    m.w32(p + 0x14, 0);
}
/// Original544620. Descriptor words are kind,id,flags,data,length; optional
/// name is the sixth descriptor word's C string. Unwritten bytes are preserved.
pub fn base(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    tree(m, p);
    m.w32(p + 0x44, p + 0x44);
    m.w32(p + 0x48, 0);
    m.w32(p + 0x60, 0);
    m.w32(p, 0x5f3830);
    for (index, value) in descriptor.into_iter().enumerate() {
        m.w32(p + 0x4c + index as u32 * 4, value);
    }
    if let Some(name) = name {
        let address = m.alloc(32);
        m.w32(p + 0x60, address);
        let end = name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name.len())
            .min(30);
        for (index, byte) in name[..end].iter().enumerate() {
            m.w8(address + index as u32, *byte);
        }
        m.w8(address + end as u32, 0);
    }
    let frame = m.r32(0x6200a4);
    let previous = frame.wrapping_sub(1);
    m.w32(p + 0x78, previous);
    m.w32(p + 0x7c, previous);
    m.w32(p + 0x54, m.r32(p + 0x54) | type_flags(descriptor[0]));
    m.w32(p + 0x68, 0);
    for off in [0x6c, 0x70] {
        if previous < frame || f32::from_bits(m.r32(p + off)) < 0. {
            m.w32(p + off, 0);
        }
    }
    m.w32(p + 0x74, 0);
    m.w32(p + 0x64, 0);
    m.w32(0x76639c, m.r32(0x76639c).wrapping_add(1));
}
/// Original544d40 spatial resource base, optional supplied AABB copied as7words.
pub fn spatial(
    m: &mut Space,
    p: u32,
    descriptor: [u32; 5],
    name: Option<&[u8]>,
    bounds: Option<[u32; 7]>,
) {
    base(m, p, descriptor, name);
    for off in (0x84..=0x98).step_by(4) {
        m.w32(p + off, 0);
    }
    m.w32(p + 0x80, 2);
    for off in (0xa0..=0xb4).step_by(4) {
        m.w32(p + off, 0);
    }
    m.w32(p + 0x9c, 2);
    for off in [0xc0, 0xc4, 0xc8, 0xcc] {
        m.w32(p + off, 0);
    }
    m.w32(p, 0x5f3860);
    if let Some(bounds) = bounds {
        for (index, value) in bounds.into_iter().enumerate() {
            m.w32(p + 0x80 + index as u32 * 4, value);
        }
    }
    for off in [0xb8, 0xd0, 0xd4] {
        m.w32(p + off, 0);
    }
    m.w32(p + 0xbc, 1f32.to_bits());
}
/// Original53dd80 instance envelope. Unknown/reserved words retain their input.
pub fn initialize_instance(m: &mut Space, p: u32) {
    for index in [
        1, 8, 9, 10, 11, 14, 15, 16, 17, 3, 4, 5, 6, 7, 12, 13, 19, 20, 28, 18, 29, 31, 32,
    ] {
        m.w32(p + index * 4, 0);
    }
    m.w32(p, 0x5f3750);
    m.w32(p + 0x54, 0x5f09a8);
    m.w32(p + 0x58, 0x5f09ac);
    m.w32(p + 0x5c, p + 0x64);
    m.w32(p + 0x60, 0);
    m.w32(p + 0x64, 0x5f09ac);
    m.w32(p + 0x68, 0);
    m.w32(p + 0x6c, p + 0x58);
    m.w32(p + 0x78, 1f32.to_bits());
}
/// Original53e6e0 raw kind1 instance constructor, before load callback53e860.
pub fn instance(
    m: &mut Space,
    p: u32,
    descriptor: [u32; 5],
    name: Option<&[u8]>,
    bounds: Option<[u32; 7]>,
) {
    spatial(m, p, descriptor, name, bounds);
    initialize_instance(m, p + 0xd8);
    m.w32(p + 0xe0, m.r32(p + 0x50));
    m.w32(p, 0x5f375c);
}
/// Original53f8b0 physics resource constructor, before load/retainedentity binding.
pub fn physics(
    m: &mut Space,
    p: u32,
    descriptor: [u32; 5],
    name: Option<&[u8]>,
    bounds: Option<[u32; 7]>,
) {
    spatial(m, p, descriptor, name, bounds);
    m.w32(p + 0xd8, 0x5f3794);
    m.w32(p + 0xdc, 0);
    m.w32(p + 0xe0, m.r32(p + 0x50));
    for offset in (0xe4..=0x124).step_by(4) {
        m.w32(p + offset, 0);
    }
    for offset in [0xe8, 0xfc, 0x110, 0x124] {
        m.w32(p + offset, 1f32.to_bits());
    }
    m.w32(p + 0x140, 0x5f09a8);
    m.w32(p + 0x144, 0x5f09ac);
    m.w32(p + 0x148, p + 0x150);
    m.w32(p + 0x14c, 0);
    m.w32(p + 0x150, 0x5f09ac);
    m.w32(p + 0x154, 0);
    m.w32(p + 0x158, p + 0x144);
    m.w32(p + 0x15c, 0);
    m.w32(p + 0x160, 0);
    m.w32(p, 0x5f37a4);
}
/// Original543000 render resource constructor.
pub fn render(
    m: &mut Space,
    p: u32,
    descriptor: [u32; 5],
    name: Option<&[u8]>,
    bounds: Option<[u32; 7]>,
) {
    spatial(m, p, descriptor, name, bounds);
    crate::native_camera::initialize_render(m, p + 0xd8);
    m.w32(p, 0x5f37f4);
    m.w32(p + 0xe0, m.r32(p + 0x50));
    m.w32(p + 0x1c4, descriptor[1]);
    m.w32(p + 0x1c8, descriptor[1]);
}
/// Original547f60 camera resource constructor, before539500 parameters.
pub fn camera(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    base(m, p, descriptor, name);
    for offset in [0x84, 0xa4, 0xac, 0xb4] {
        m.w32(p + offset, 0);
    }
    m.w32(p + 0x80, 0x5f399c);
    m.w32(p + 0x88, m.r32(p + 0x50));
    for (offset, value) in [
        (0x8c, 0x3f400000),
        (0x90, 0x42b40000),
        (0x94, 0x3f800000),
        (0x98, 0x42c80000),
        (0xb8, 0x3f800000),
    ] {
        m.w32(p + offset, value);
    }
    m.w32(p, 0x5f39a8);
}
/// Original544fe0 WT-root reference inheritance after attaching a spatial child.
pub fn inherit_spatial_root(m: &mut Space, child: u32, parent: u32) {
    let target = if !matches!(m.r32(parent + 0x4c), 1 | 2 | 3 | 15 | 16 | 17)
        || m.r32(child + 0x54) & 0x10 != 0
    {
        child
    } else {
        m.r32(parent + 0xcc)
    };
    crate::native_part::bind_reference(m, child + 0xc0, target);
}
/// Original549010/5480e0 material descriptor family (kind13decimal).
pub fn material(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    base(m, p, descriptor, name);
    let e = p + 0x80;
    for index in 1..=0x47 {
        if index != 2 {
            m.w32(e + index * 4, 0);
        }
    }
    for index in [4, 5, 6, 8, 9, 10, 12, 13, 14, 0x18, 0x1e, 0x43] {
        m.w32(e + index * 4, 1f32.to_bits());
    }
    for (index, value) in [
        (0x21, 1),
        (0x2a, 4),
        (0x2b, 1),
        (0x2c, 3),
        (0x2e, 1),
        (0x36, 0x41200000),
        (0x38, 0xffff00ff),
        (0x47, 2),
    ] {
        m.w32(e + index * 4, value);
    }
    m.w32(e, 0x5f39dc);
    m.w32(p, 0x5f39e8);
    m.w32(p + 0x88, m.r32(p + 0x50));
}
/// Original54a5b0/549580 mesh render type family (kind14decimal).
pub fn mesh_render_type(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    base(m, p, descriptor, name);
    let e = p + 0x80;
    for index in [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0x11] {
        m.w32(e + index * 4, 0);
    }
    for index in 0x13..=0x2f {
        m.w32(e + index * 4, 0);
    }
    m.w32(e + 0x88, 0x41200000);
    m.w32(e + 0x9c, 0x41200000);
    m.w32(e + 0x48, 1f32.to_bits());
    m.w32(e, 0x5f3a5c);
    m.w32(p, 0x5f3a68);
    m.w32(p + 0x88, m.r32(p + 0x50));
    crate::native_part::bind_reference(m, p + 0x130, p);
}
/// Original53b520 mesh source resource constructor.
pub fn mesh_source(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    base(m, p, descriptor, name);
    m.w32(p + 0x80, 0x5f35f0);
    m.w32(p + 0x84, 0);
    m.w32(p + 0x88, m.r32(p + 0x50));
    for offset in [0x8c, 0x98, 0x9c] {
        m.w32(p + offset, 0);
    }
    for offset in (0xa8..=0xf8).step_by(4) {
        m.w32(
            p + offset,
            if matches!(offset, 0xa8 | 0xc4 | 0xe0) {
                2
            } else {
                0
            },
        );
    }
    m.w32(p + 0xa4, descriptor[1]);
    m.w32(p, 0x5f3608);
}
/// Original546ae0 native game-type template resource constructor.
pub fn game_type(m: &mut Space, p: u32, descriptor: [u32; 5], name: Option<&[u8]>) {
    base(m, p, descriptor, name);
    for offset in [0x84, 0x8c, 0x90, 0x94, 0x9c] {
        m.w32(p + offset, 0);
    }
    m.w32(p + 0x80, 0x5f389c);
    m.w32(p + 0x88, m.r32(p + 0x50));
    m.w32(p + 0x98, 1f32.to_bits());
    m.w32(p, 0x5f38a8);
}
/// Complete raw constructor dispatch admitted by original536a10. Resource
/// loading and manager insertion are separate operations.
pub fn construct(
    m: &mut Space,
    p: u32,
    d: [u32; 5],
    name: Option<&[u8]>,
    bounds: Option<[u32; 7]>,
) -> Result<(), String> {
    match d[0] {
        1 => instance(m, p, d, name, bounds),
        2 => physics(m, p, d, name, bounds),
        3 => render(m, p, d, name, bounds),
        5 => mesh_source(m, p, d, name),
        8 => game_type(m, p, d, name),
        12 => camera(m, p, d, name),
        13 => material(m, p, d, name),
        14 => mesh_render_type(m, p, d, name),
        kind @ (6 | 7 | 9 | 10 | 0x12 | 0x13 | 0x14 | 0x15) => {
            base(m, p, d, name);
            m.w32(p + 0x84, 0);
            m.w32(p + 0x88, m.r32(p + 0x50));
            let (rawvt, payloadvt) = match kind {
                6 => (0x5f3658, 0x5f364c),
                7 => (0x5f36dc, 0x5f36d0),
                9 => (0x5f38e8, 0x5f38dc),
                10 => (0x5f3928, 0x5f391c),
                0x12 => (0x5f371c, 0x5f3710),
                0x13 => (0x5f3578, 0x5f356c),
                0x14 => (0x5f3a28, 0x5f3a1c),
                0x15 => (0x5f3698, 0x5f368c),
                _ => unreachable!(),
            };
            m.w32(p, rawvt);
            m.w32(p + 0x80, payloadvt);
            if matches!(kind, 6 | 7 | 9 | 0x12 | 0x13 | 0x14) {
                m.w32(p + 0x8c, 0);
            }
            if kind == 7 {
                for offset in [0x90, 0x98, 0x9c, 0xa4] {
                    m.w32(p + offset, 0);
                }
                m.w32(p + 0xa8, 1f32.to_bits());
            }
            if kind == 9 {
                m.w32(p + 0x90, 0);
            }
            if kind == 0x14 {
                m.w32(p + 0x90, 1f32.to_bits());
            }
            if matches!(kind, 0x12 | 0x15) {
                let t = p + if kind == 0x12 { 0xa0 } else { 0x8c };
                m.w32(t, 0x5f2af4);
                m.w32(t + 4, 0x5f2af8);
                m.w32(t + 0x18, 0x5f2af8);
                for i in 0..2 {
                    m.w32(t + 8 + i * 8, t + 0x18);
                    m.w32(t + 0xc + i * 8, 0);
                    m.w32(t + 0x1c + i * 8, 0);
                    m.w32(t + 0x20 + i * 8, t + 4);
                }
                if kind == 0x12 {
                    m.w32(p + 0x90, 0);
                    m.w32(p + 0x94, 0);
                    m.w32(p + 0x98, 1f32.to_bits());
                    m.w32(p + 0x9c, 1f32.to_bits());
                }
            }
        }
        kind => {
            return Err(format!(
                "NativeResourceBoundary: original536a10 constructor kind{kind} unavailable"
            ))
        }
    }
    Ok(())
}
/// Resource object allocation sizes selected by original536a10.
pub fn size(kind: u32) -> Option<u32> {
    Some(match kind {
        1 => 0x15c,
        2 => 0x164,
        3 => 0x26c,
        5 => 0xfc,
        6 => 0x90,
        7 => 0xac,
        8 => 0xa0,
        9 => 0x94,
        10 => 0x90,
        0xc => 0xbc,
        0xd => 0x1a0,
        0xe => 0x140,
        0x12 => 0xcc,
        0x13 => 0x90,
        0x14 => 0x94,
        0x15 => 0xb8,
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_resource_base_memory_and_name_allocations_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-base-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_resource_base_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let p = m.alloc(0x26c);
            let d = m.alloc(24);
            let name_address = m.alloc(48);
            let bounds_address = m.alloc(28);
            let fill = row["fill"].as_u64().unwrap() as u8;
            for i in 0..0x26c {
                m.w8(p + i, fill);
            }
            let descriptor = std::array::from_fn(|i| row["descriptor"][i].as_u64().unwrap() as u32);
            for i in 0..5 {
                m.w32(d + i * 4, descriptor[i as usize]);
            }
            let name = row["name"].as_str().map(str::as_bytes);
            if let Some(name) = name {
                for (index, byte) in name.iter().enumerate() {
                    m.w8(name_address + index as u32, *byte);
                }
                m.w32(d + 20, name_address);
            }
            let bounds = row["bounds"]
                .as_array()
                .map(|values| std::array::from_fn(|i| values[i].as_u64().unwrap() as u32));
            if let Some(bounds) = bounds {
                for (index, value) in bounds.into_iter().enumerate() {
                    m.w32(bounds_address + index as u32 * 4, value);
                }
            }
            m.w32(0x6200a4, row["frame"].as_u64().unwrap() as u32);
            m.w32(0x76639c, 123);
            if row["spatial"].as_bool().unwrap() {
                construct(&mut m, p, descriptor, name, bounds).unwrap();
            } else {
                base(&mut m, p, descriptor, name);
            }
            let actual = (0..0x26c / 4).map(|i| m.r32(p + i * 4)).collect::<Vec<_>>();
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected,
                "spatial{} frame{} fill{}",
                row["spatial"], row["frame"], row["fill"]
            );
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            assert_eq!(m.r32(0x76639c), 124);
            if let Some(values) = row["name_words"].as_array() {
                let actual = (0..8)
                    .map(|i| m.r32(m.r32(p + 0x60) + i * 4))
                    .collect::<Vec<_>>();
                let expected = values
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected);
            }
        }
    }
    #[test]
    fn original_spatial_root_reference_inheritance_matches() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-base-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_resource_base_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["roots"].as_array().unwrap() {
            let mut m = Space::new();
            let p = m.alloc(0x164);
            let c = m.alloc(0x164);
            let d = m.alloc(24);
            let old = m.alloc(0x164);
            m.w32(0x6200a4, 1);
            let pk = row["parentkind"].as_u64().unwrap() as u32;
            let flags = row["childflags"].as_u64().unwrap() as u32;
            for (a, desc) in [(p, [pk, 101, 0, 0, 0]), (c, [2, 102, flags, 0, 0])] {
                for (i, v) in desc.iter().enumerate() {
                    m.w32(d + i as u32 * 4, *v);
                }
                spatial(&mut m, a, desc, None, None);
            }
            if row["parentroot"].as_bool().unwrap() {
                m.w32(p + 0xcc, p);
            }
            if row["previous"].as_bool().unwrap() {
                m.w32(old + 0x50, 103);
                m.w32(c + 0xc0, 0);
                m.w32(c + 0xc4, 0);
                m.w32(c + 0xc8, 103);
                m.w32(c + 0xcc, old);
                m.w32(old + 0x48, c + 0xc0);
            }
            inherit_spatial_root(&mut m, c, p);
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            let actual = (p..m.heap_ptr())
                .step_by(4)
                .map(|a| m.r32(a))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{row}");
        }
    }
}
