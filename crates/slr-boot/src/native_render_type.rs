//! Original549720 render-type descriptors,54a190payload cloning and54a450
//! resource replacement. Skeleton/particle/skid/welding backends and active
//! renderer notification callbacks retain explicit boundaries.
use slr_physics::{cfg, space::Space};
/// Package-relative ID resolution and retained resource binding use the live
/// render template's package authority, supplied by its resource factory.
pub trait Host {
    fn resolve(&mut self, m: &mut Space, id: u32) -> Result<u32, String>;
    fn bind(&mut self, m: &mut Space, slot: u32, id: u32, kind: u32) -> Result<(), String>;
}
fn bind(m: &mut Space, slot: u32, id: u32, kind: u32, host: &mut dyn Host) -> Result<(), String> {
    let resolved = host.resolve(m, id)?;
    host.bind(m, slot, resolved, kind)
}
pub fn load(m: &mut Space, body: u32, bytes: &[u8], host: &mut dyn Host) -> Result<(), String> {
    let mut textures = Vec::new();
    for line in bytes.split(|b| *b == b'\n' || *b == 0) {
        let mut input = b"_\0".to_vec();
        input.extend_from_slice(line);
        let keyword = cfg::scan(&input, "%s");
        let Some(key) = keyword.s(0) else { continue };
        match key {
            b"mesh" => {
                let scan = cfg::scan(&input, "%*s %d");
                let id = scan
                    .d(0)
                    .ok_or("NativeRenderTypeBoundary: malformed mesh resource record")?
                    as u32;
                bind(m, body + 0xc, id, 5, host)?;
            }
            b"numbones" | b"flags" => {
                let scan = cfg::scan(&input, "%*s %d");
                if let Some(v) = scan.d(0) {
                    m.w32(
                        body + if key == b"numbones" { 0x1c } else { 0x34 },
                        v as u32,
                    );
                }
            }
            b"lod_bias" | b"lod_amp" => {
                let scan = cfg::scan(&input, "%*s %f");
                if let Some(v) = scan.f(0) {
                    m.w32(
                        body + if key == b"lod_bias" { 0x44 } else { 0x48 },
                        v.to_bits(),
                    );
                }
            }
            b"texture" => {
                let scan = cfg::scan(&input, "%*s %d");
                let id = scan
                    .d(0)
                    .ok_or("NativeRenderTypeBoundary: malformed texture resource record")?
                    as u32;
                if textures.len() == 1024 {
                    return Err(
                        "NativeRenderTypeBoundary: texture list exceeds original stack array"
                            .into(),
                    );
                }
                textures.push(id);
                m.w32(body + 0x20, m.r32(body + 0x20).wrapping_add(1));
            }
            b"shd_texture" => {
                let scan = cfg::scan(&input, "%*s %d");
                let id = scan
                    .d(0)
                    .ok_or("NativeRenderTypeBoundary: malformed shadow texture")?
                    as u32;
                bind(m, body + 0x4c, id, 7, host)?;
                m.w32(body + 0x34, m.r32(body + 0x34) | 0x20000);
            }
            b"shd_vbase" | b"shd_center" | b"shd_diru" | b"shd_dirv" | b"shd_vup" => {
                use slr_x87::{Precision, Rounding, X87};
                let pc = Precision::F32;
                let rc = Rounding::NearestEven;
                let scan = cfg::scan(&input, "%*s %f %f %f");
                let mut values = [0u32; 3];
                for (i, value) in values.iter_mut().enumerate() {
                    *value = scan
                        .f(i)
                        .ok_or("NativeRenderTypeBoundary: incomplete shadow vector")?
                        .to_bits();
                }
                let scaled = values
                    .map(|v| X87::from_f32(f32::from_bits(v)).mul(X87::from_f32(10.), pc, rc));
                let bits = scaled.map(|v| v.to_f32(pc, rc).to_bits());
                let (offset, flags) = match key {
                    b"shd_vbase" => (0x5c, 0x40000),
                    b"shd_center" => (0x7c, 0x30000),
                    b"shd_diru" => (0x88, 0x30000),
                    b"shd_dirv" => (0x94, 0x30000),
                    _ => (0x68, 0x40000),
                };
                if key == b"shd_vup" {
                    let x = X87::from_f32(f32::from_bits(bits[0]));
                    let y = X87::from_f32(f32::from_bits(bits[1]));
                    let z = scaled[2];
                    let length = x
                        .mul(x, pc, rc)
                        .add(y.mul(y, pc, rc), pc, rc)
                        .add(z.mul(z, pc, rc), pc, rc)
                        .sqrt(pc, rc);
                    m.w32(body + 0x74, length.to_f32(pc, rc).to_bits());
                    let inverse = X87::from_f32(1.).div(length, pc, rc);
                    for (i, word) in bits.into_iter().enumerate() {
                        m.w32(
                            body + offset + i as u32 * 4,
                            X87::from_f32(f32::from_bits(word))
                                .mul(inverse, pc, rc)
                                .to_f32(pc, rc)
                                .to_bits(),
                        );
                    }
                } else {
                    for (i, word) in bits.into_iter().enumerate() {
                        m.w32(body + offset + i as u32 * 4, word);
                    }
                }
                m.w32(body + 0x34, m.r32(body + 0x34) | flags);
            }
            b"shd_radius" => {
                let scan = cfg::scan(&input, "%*s %f");
                if let Some(value) = scan.f(0) {
                    m.w32(body + 0x78, (value * 10.).to_bits());
                }
            }
            b"shd_uv" => {
                let scan = cfg::scan(&input, "%*s %f %f %f %f");
                for i in 0..4 {
                    if let Some(value) = scan.f(i) {
                        m.w32(body + 0xa0 + i as u32 * 4, value.to_bits());
                    }
                }
            }
            b"skeleton" | b"particle" | b"skidmark" | b"welding" => {
                return Err(format!(
                    "NativeRenderTypeBoundary: original549720 feature{} requires live loader",
                    String::from_utf8_lossy(key)
                ))
            }
            _ => (),
        }
    }
    let count = m.r32(body + 0x20);
    if count != textures.len() as u32 {
        return Err("NativeRenderTypeBoundary: repeated load has existing texture records".into());
    }
    if count != 0 {
        let allocation = m.malloc(count * 16 + 4);
        m.w32(allocation, count);
        let array = allocation + 4;
        m.w32(body + 0x24, array);
        for (index, id) in textures.into_iter().enumerate() {
            for word in 0..4 {
                m.w32(array + index as u32 * 16 + word * 4, 0);
            }
            bind(m, array + index as u32 * 16, id, 7, host)?;
        }
    }
    Ok(())
}
/// Original54a190 copy into a freshly constructed typee payload. The skeleton
/// pointer is shared by the original; particle/skid/weld pointers stay at their
/// destination constructor defaults. Every resource reference is freshly bound.
pub fn clone_payload(m: &mut Space, source: u32, target: u32) -> Result<(), String> {
    if m.r32(target + 0x24) != 0 || m.r32(target + 0x58) != 0 {
        return Err("NativeRenderTypeBoundary: clone destination is not fresh".into());
    }
    crate::native_part::bind_reference(m, target + 0xc, m.r32(source + 0x18));
    m.w32(target + 0x1c, m.r32(source + 0x1c));
    let count = m.r32(source + 0x20);
    if count > 1024 {
        return Err(
            "NativeRenderTypeBoundary: clone texture count exceeds original input bound".into(),
        );
    }
    let allocation = m.malloc(count * 16 + 4);
    m.w32(allocation, count);
    let array = allocation + 4;
    m.w32(target + 0x24, array);
    m.w32(target + 0x20, count);
    for i in 0..count {
        for word in 0..4 {
            m.w32(array + i * 16 + word * 4, 0);
        }
        crate::native_part::bind_reference(
            m,
            array + i * 16,
            m.r32(m.r32(source + 0x24) + i * 16 + 12),
        );
    }
    for offset in [0x34, 0x38] {
        m.w32(target + offset, m.r32(source + offset));
    }
    let shadow = m.r32(source + 0x58);
    crate::native_part::bind_reference(m, source + 0x4c, 0);
    for offset in (0x4c..0xb0).step_by(4) {
        m.w32(target + offset, m.r32(source + offset));
    }
    crate::native_part::bind_reference(m, source + 0x4c, shadow);
    crate::native_part::bind_reference(m, target + 0x4c, shadow);
    for offset in [0x44, 0x48] {
        m.w32(target + offset, m.r32(source + offset));
    }
    Ok(())
}
/// Original52d2a0 resource-change notification. The actual hash/list lookup
/// executes; a registered renderer callback needs its real listener body.
fn notify_change(m: &mut Space, reference: u32) -> Result<(), String> {
    m.w32(0x65c870, 1);
    let id = m.r32(reference + 8);
    let mut node = 0x65c690 + (id & 31) * 12;
    if m.r32(node + 4) != 0 {
        let mut found = false;
        for _ in 0..4096 {
            if m.r32(node) == id {
                found = true;
                break;
            }
            node = m.r32(node + 8);
            if node == 0 {
                break;
            }
        }
        if node != 0 && !found {
            return Err("NativeRenderTypeBoundary: renderer listener hash cycle".into());
        }
        if found {
            m.w32(0x65c838, node);
            let listener = m.r32(node + 4);
            if listener != 0 && m.r32(listener) != 0 {
                return Err(
                    "NativeRenderTypeBoundary: original52d2a0 active renderer listener virtual+4"
                        .into(),
                );
            }
        }
    }
    m.w32(0x65c870, 0);
    Ok(())
}
/// Original54a450 typee virtual+8: replace mesh or the first matching texture
/// reference, then issue the actual resource-change notification.
pub fn replace_resource(
    m: &mut Space,
    body: u32,
    old_reference: u32,
    new_reference: u32,
) -> Result<bool, String> {
    let id = m.r32(old_reference + 8);
    let slot = if m.r32(body + 0x14) == id {
        Some(body + 0xc)
    } else {
        let count = m.r32(body + 0x20);
        if count > 1024 {
            return Err("NativeRenderTypeBoundary: replacement texture count exceeds bound".into());
        }
        let array = m.r32(body + 0x24);
        (0..count)
            .find(|i| m.r32(array + i * 16 + 8) == id)
            .map(|i| array + i * 16)
    };
    let Some(slot) = slot else { return Ok(false) };
    crate::native_part::bind_reference(m, slot, m.r32(new_reference + 12));
    notify_change(m, body + 0xb0)?;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Resources {
        resources: Vec<u32>,
        calls: Vec<Vec<u32>>,
    }
    impl Host for Resources {
        fn resolve(&mut self, _m: &mut Space, id: u32) -> Result<u32, String> {
            Ok(id | 0x50000)
        }
        fn bind(&mut self, m: &mut Space, slot: u32, id: u32, kind: u32) -> Result<(), String> {
            let raw = self.resources[(id & 0xffff) as usize - 1];
            crate::native_part::bind_reference(m, slot, raw);
            self.calls.push(vec![slot, id, kind, 0]);
            Ok(())
        }
    }
    #[test]
    fn original_render_type_mesh_texture_and_scalar_records_match_complete_heap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-type-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_render_type_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let body = m.alloc(0xc0);
            let resources = (0..5).map(|_| m.alloc(0x90)).collect();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut host = Resources {
                resources,
                calls: Vec::new(),
            };
            load(
                &mut m,
                body,
                row["text"].as_str().unwrap().as_bytes(),
                &mut host,
            )
            .unwrap();
            assert_eq!(serde_json::to_value(&host.calls).unwrap(), row["calls"]);
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "row={} word{i}",
                    row["text"]
                );
            }
        }
    }
    #[test]
    fn original_typee_clone_matches_resource_links_arrays_shared_skeleton_and_shadow_words() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-type-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_render_type_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["clones"].as_array().unwrap() {
            let mut m = Space::new();
            let source = m.alloc(0xc0);
            let target = m.alloc(0xc0);
            for _ in 0..5 {
                m.alloc(0x90);
            }
            let count = row["count"].as_u64().unwrap() as u32;
            if count != 0 {
                m.alloc(count * 16 + 4);
            }
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            clone_payload(&mut m, source, target).unwrap();
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "count={count} shadow={} word{i}",
                    row["shadow"]
                );
            }
        }
    }
    #[test]
    fn original_typee_reference_replacement_matches_complete_resource_graph() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-replace-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_render_replace_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            for (i, v) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            for (address, value) in row["globals_before"].as_object().unwrap() {
                m.w32(address.parse().unwrap(), value.as_u64().unwrap() as u32);
            }
            assert_eq!(
                replace_resource(
                    &mut m,
                    row["body"].as_u64().unwrap() as u32,
                    row["old"].as_u64().unwrap() as u32,
                    row["new"].as_u64().unwrap() as u32
                )
                .unwrap(),
                row["result"].as_bool().unwrap()
            );
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "profile={} word{i}",
                    row["profile"]
                );
            }
            for (address, value) in row["globals_after"].as_object().unwrap() {
                assert_eq!(
                    m.r32(address.parse().unwrap()),
                    value.as_u64().unwrap() as u32,
                    "global{address}"
                );
            }
            assert_eq!(m.r32(0x65c870), 0);
        }
    }
}
