//! Original53af40 mesh-source descriptor load. Render shape creation505960 is
//! an explicit collaborator, carrying live source-file/resource identity.
use slr_physics::{cfg, space::Space};
use slr_x87::{Precision, Rounding, X87};
pub trait Host {
    #[allow(clippy::too_many_arguments)] // The original call site passes all eight.
    fn create_shape(
        &mut self,
        m: &mut Space,
        kind: u32,
        asynchronous: u32,
        source: &str,
        skinned: u32,
        resource_id: u32,
        slot: u32,
    ) -> Result<u32, String>;
}
fn scalar(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn half_sum(a: u32, b: u32, subtract: bool) -> u32 {
    let v = if subtract {
        scalar(a).sub(scalar(b), Precision::F32, Rounding::NearestEven)
    } else {
        scalar(a).add(scalar(b), Precision::F32, Rounding::NearestEven)
    };
    v.mul(X87::from_f32(0.5), Precision::F32, Rounding::NearestEven)
        .to_f32(Precision::F32, Rounding::NearestEven)
        .to_bits()
}
pub fn load(m: &mut Space, body: u32, bytes: &[u8], host: &mut dyn Host) -> Result<(), String> {
    m.w32(body + 0x20, 0);
    for line in bytes.split(|byte| *byte == b'\n' || *byte == 0) {
        let mut input = b"_\0".to_vec();
        input.extend_from_slice(line);
        let key = cfg::scan(&input, "%s");
        let Some(key) = key.s(0) else { continue };
        match key {
            b"sourcefile" => {
                let start = line
                    .iter()
                    .position(|b| !b.is_ascii_whitespace())
                    .unwrap_or(line.len());
                let remainder = &line[(start + 10).min(line.len())..];
                let start = remainder
                    .iter()
                    .position(|b| !b.is_ascii_whitespace())
                    .unwrap_or(remainder.len());
                let file = &remainder[start..];
                let end = file
                    .iter()
                    .position(|b| *b == b'\r' || *b == 0)
                    .unwrap_or(file.len());
                let file = &file[..end];
                if !file.is_empty() {
                    let p = m.alloc(file.len() as u32 + 1);
                    for (index, byte) in file.iter().take(255).enumerate() {
                        m.w8(p + index as u32, *byte);
                    }
                    m.w8(p + file.len().min(255) as u32, 0);
                    m.w32(body + 0xc, p);
                }
            }
            b"flags" | b"slot" => {
                let scan = cfg::scan(&input, "%*s %d");
                if let Some(v) = scan.d(0) {
                    m.w32(body + if key == b"flags" { 0x1c } else { 0x20 }, v as u32);
                }
            }
            b"radius" => {
                let scan = cfg::scan(&input, "%*s %f");
                let radius = scan
                    .f(0)
                    .ok_or(
                        "NativeMeshResourceBoundary: radius record lacks original numeric input",
                    )?
                    .to_bits();
                m.w32(body + 0x28, 0);
                for offset in [0x2c, 0x30, 0x34] {
                    m.w32(body + offset, 0);
                }
                for offset in [0x38, 0x3c, 0x40] {
                    m.w32(body + offset, radius);
                }
            }
            b"oc_rect" => {
                let scan = cfg::scan(&input, "%*s %f %f %f %f");
                if scan.f(1).is_none() {
                    m.w32(body + 0x1c, m.r32(body + 0x1c) | 4);
                    continue;
                }
                let [a, b, c, d] = std::array::from_fn(|i| scan.f(i).unwrap_or(0.).to_bits());
                m.w32(body + 0x60, 1);
                m.w32(body + 0x64, half_sum(c, a, false));
                m.w32(body + 0x68, 1f32.to_bits());
                m.w32(body + 0x6c, half_sum(d, b, false));
                m.w32(body + 0x70, half_sum(a, c, true) & 0x7fff_ffff);
                m.w32(body + 0x74, 0);
                m.w32(body + 0x78, half_sum(b, d, true) & 0x7fff_ffff);
                m.w32(body + 0x1c, m.r32(body + 0x1c) | 2);
            }
            _ => (),
        }
    }
    let old = m.r32(body + 0x1c);
    m.w32(body + 0x1c, old | 4);
    let p = m.r32(body + 0xc);
    if p == 0 {
        return Err("NativeMeshResourceBoundary: original sourcefile is absent".into());
    }
    let source = (0..256)
        .map(|i| m.r8(p + i))
        .take_while(|b| *b != 0)
        .collect::<Vec<_>>();
    let is_y = source
        .last()
        .is_some_and(|byte| matches!(byte, b'y' | b'Y'));
    if is_y {
        m.w32(body + 0x1c, old | 5);
    }
    let source = std::str::from_utf8(&source).map_err(|_| {
        "NativeMeshResourceBoundary: non-UTF8 sourcefile requires original byte path"
    })?;
    let shape = host.create_shape(
        m,
        if is_y { 4 } else { 3 },
        m.r32(0x619994),
        source,
        u32::from(m.r32(body + 0x1c) & 8 != 0),
        m.r32(body + 8),
        if is_y { 0 } else { m.r32(body + 0x20) },
    )?;
    m.w32(body + 0x18, shape);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Shape {
        calls: Vec<serde_json::Value>,
    }
    impl Host for Shape {
        fn create_shape(
            &mut self,
            _: &mut Space,
            kind: u32,
            asynchronous: u32,
            source: &str,
            skinned: u32,
            id: u32,
            slot: u32,
        ) -> Result<u32, String> {
            self.calls.push(
                serde_json::json!({"args":[kind,asynchronous,skinned,id,slot],"source":source}),
            );
            Ok(0x30004000)
        }
    }
    #[test]
    fn original_mesh_descriptor_fields_heap_and_shape_arguments_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-mesh-resource-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_mesh_resource_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let raw = row["raw"].as_u64().unwrap() as u32;
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            m.w32(0x619994, 1);
            for (i, value) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(raw + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            let mut host = Shape { calls: vec![] };
            load(
                &mut m,
                row["body"].as_u64().unwrap() as u32,
                row["text"].as_str().unwrap().as_bytes(),
                &mut host,
            )
            .unwrap();
            assert_eq!(serde_json::json!(host.calls), row["calls"]);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            for (i, value) in row["output"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(raw + i as u32 * 4),
                    value.as_u64().unwrap() as u32,
                    "{} word{i}",
                    row["text"]
                );
            }
        }
    }
}
/// Loaded branch53b7a1..53b7fd. Returns whether the original537790 manager
/// notification must be dispatched; the caller must execute it before treating
/// the operation as complete. Descriptor radius/bounds stay unchanged here.
pub fn refresh_progress(m: &mut Space, raw: u32) -> Result<bool, String> {
    if m.r32(raw + 0x4c) != 5 || m.r32(raw + 0x64) & 1 == 0 {
        return Err("NativeMeshResourceBoundary: progress refresh requires loaded kind5".into());
    }
    //53b74b..53b76f also choose the actual file-load mode on loaded calls.
    m.w32(
        0x619994,
        u32::from(m.rf(raw + 0x6c) != 1000. && !m.rf(raw + 0x6c).is_nan()),
    );
    m.w32(raw + 0x68, 0);
    let shape = m.r32(raw + 0x98);
    if shape == 0 {
        m.w32(raw + 0x64, m.r32(raw + 0x64) | 0x1000_0000);
        return Ok(false);
    }
    let flags = m.r32(shape + 0x20);
    if flags & 0x8000_0000 == 0 && flags & 8 != 0 {
        m.w32(raw + 0x68, 1f32.to_bits());
        return Ok(m.r32(raw + 0x54) & 0x100 != 0);
    }
    Ok(false)
}
#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn original_loaded_mesh_progress_errors_notification_and_bounds_preservation_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-mesh-resource-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_mesh_resource_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["progress"].as_array().unwrap() {
            let mut m = Space::new();
            let raw = row["raw"].as_u64().unwrap() as u32;
            let shape = row["shape"].as_u64().unwrap() as u32;
            for (i, v) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(raw + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            m.w32(shape + 0x20, row["shapeflags"].as_u64().unwrap() as u32);
            let notify = refresh_progress(&mut m, raw).unwrap();
            assert_eq!(notify, !row["calls"].as_array().unwrap().is_empty());
            assert_eq!(m.r32(0x619994), row["mode"].as_u64().unwrap() as u32);
            for (i, v) in row["output"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(raw + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "rawflags{} shapeflags{} detail{} null{} word{i}",
                    row["rawflags"],
                    row["shapeflags"],
                    row["detail"],
                    row["null"]
                );
            }
        }
    }
}
