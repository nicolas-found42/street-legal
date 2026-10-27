//! Original renderer list construction and viewport registration. These are
//! retained native records; no incoming oracle state supplies production lists.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};

pub const STATIC_RENDERER: u32 = 0x64a4d8;

/// Original4fb8c0. Device installation and subsequent camera registration are
/// separate operations; the constructor deliberately preserves unwritten gaps.
/// exe: FUN_004fb8c0
pub fn construct(m: &mut Space, renderer: u32) {
    for offset in [0, 0x2c, 0x114, 0x140, 0x16c, 0x198, 0x1c4] {
        crate::native_camera::double_list(m, renderer + offset);
    }
    for offset in [0x6c, 0x88, 0xa4, 0xc0, 0xdc, 0xf8] {
        crate::native_camera::single_list(m, renderer + offset);
    }
    for offset in [0x58, 0x5c, 0x60, 0x64, 0x218] {
        m.w32(renderer + offset, 0);
    }
    m.w32(renderer + 0x68, 0x3c23d70a);
}

pub trait Host {
    /// The normal allocator may fail; caller-provided dirty storage is retained.
    fn allocate(&mut self, m: &mut Space, size: u32) -> Result<u32, String> {
        Ok(m.malloc(size))
    }
    /// Original4fd580 calls the installed display device's virtual+100.
    fn dimensions(&mut self, m: &mut Space, renderer: u32) -> Result<[u32; 2], String>;
}

fn product(dimension: X87, value: u32) -> u32 {
    dimension
        .mul(
            X87::from_f32(f32::from_bits(value)),
            Precision::F32,
            Rounding::NearestEven,
        )
        .to_f32(Precision::F32, Rounding::NearestEven)
        .to_bits()
}

/// Original515f60, with its explicit resource-data argument from4fce00.
/// exe: FUN_00515f60
pub fn viewport(
    m: &mut Space,
    object: u32,
    reference: u32,
    data: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    m.w32(object + 4, 0);
    m.w32(object + 8, 0);
    for offset in [0x14, 0x18, 0x1c, 0x20] {
        m.w32(object + offset, 0);
    }
    crate::native_camera::single_list(m, object + 0x24);
    crate::native_camera::single_list(m, object + 0x40);
    m.w32(object, 0x5f3064);
    m.w32(object + 0xc, 0);
    let raw = m.r32(reference + 0xc);
    crate::native_part::bind_reference(m, object + 0x14, raw);
    let flags = m.r32(data + 0xc);
    if flags != u32::MAX {
        m.w32(object + 0x10, flags);
    }
    for offset in [0x10, 0x14, 0x18, 0x1c] {
        m.w32(object + offset + 0x4c, m.r32(data + offset));
    }
    let renderer = m.r32(0x6187b0);
    if renderer != 0 {
        let [width, height] = host.dimensions(m, renderer)?;
        let width = X87::from_f64(width as f64);
        let height = X87::from_f64(height as f64);
        //516027 stores a single copy without popping ST0. The final height
        //multiply reloads that rounded copy at516037; the first uses FILD's
        //exact unsigned integer still retained in ST0.
        let stored_height = X87::from_f32(height.to_f32(Precision::F32, Rounding::NearestEven));
        for (offset, dimension) in [
            (0x5c, width),
            (0x60, height),
            (0x64, width),
            (0x68, stored_height),
        ] {
            m.w32(
                object + offset + 0x10,
                product(dimension, m.r32(object + offset)),
            );
        }
    }
    Ok(())
}

fn append(m: &mut Space, list: u32, node: u32) {
    let previous = m.r32(list + 0x18);
    m.w32(previous + 4, node);
    m.w32(node + 4, list + 0x10);
    m.w32(node + 8, previous);
    m.w32(list + 0x18, node);
}
fn unlink(m: &mut Space, node: u32) {
    let previous = m.r32(node + 8);
    let next = m.r32(node + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}
fn find(m: &Space, list: u32, id: u32) -> Result<u32, String> {
    let mut node = m.r32(list + 8);
    let mut visited = std::collections::HashSet::new();
    while node != 0 && m.r32(node + 4) != 0 {
        if !visited.insert(node) {
            return Err("NativeRendererBoundary: cyclic viewport list".into());
        }
        if m.r32(node + 0x1c) == id {
            return Ok(node);
        }
        node = m.r32(node + 4);
    }
    Ok(0)
}

/// Original4fce00's supplied-data arm. The implicit virtual materialization
/// arm remains explicit until the resource loader supplies it.
/// exe: FUN_004fce00
pub fn register(
    m: &mut Space,
    renderer: u32,
    reference: u32,
    data: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    if data == 0 {
        return Err(
            "NativeRendererBoundary: viewport registration needs original resource get-data".into(),
        );
    }
    let object = host.allocate(m, 0x7c)?;
    if object == 0 {
        return Ok(0);
    }
    viewport(m, object, reference, data, host)?;
    append(m, renderer + 0xc0, object);
    Ok(object)
}

/// Original4fd020/4fd280: move one retained registration into the sorted active
/// list. Already-active registrations keep their existing order and flags.
/// exe: FUN_004fd020
/// exe: FUN_004fd280
pub fn activate(m: &mut Space, renderer: u32, reference: u32, flags: u32) -> Result<u32, String> {
    let raw = m.r32(reference + 0xc);
    if raw == 0 || m.r32(raw + 0x4c) != 0x12 {
        return Ok(0);
    }
    let id = m.r32(reference + 8);
    let object = find(m, renderer + 0xc0, id)?;
    if object == 0 {
        find(m, renderer + 0xa4, id)?;
        return Ok(0);
    }
    if flags != u32::MAX {
        m.w32(object + 0x10, flags);
    }
    unlink(m, object);
    m.w32(object + 0xc, m.r32(object + 0xc) | 1);
    let list = renderer + 0xa4;
    let mut next = m.r32(list + 8);
    let mut visited = std::collections::HashSet::new();
    while next != 0 && m.r32(next + 4) != 0 {
        if !visited.insert(next) {
            return Err("NativeRendererBoundary: cyclic active viewport list".into());
        }
        if m.r32(next + 0x10) >= m.r32(object + 0x10) {
            let previous = m.r32(next + 8);
            m.w32(object + 4, next);
            m.w32(object + 8, previous);
            m.w32(previous + 4, object);
            m.w32(next + 8, object);
            return Ok(0);
        }
        next = m.r32(next + 4);
    }
    append(m, list, object);
    Ok(0)
}

/// Original4fd050 viewport arm: deactivate into the retained registration list.
/// exe: FUN_004fd050
pub fn deactivate(m: &mut Space, renderer: u32, reference: u32) -> Result<u32, String> {
    let raw = m.r32(reference + 0xc);
    if raw == 0 {
        return Ok(0);
    }
    if m.r32(raw + 0x4c) != 0x12 {
        return Err("NativeRendererBoundary: deactivation of a non-viewport resource".into());
    }
    let object = find(m, renderer + 0xa4, m.r32(reference + 8))?;
    if object != 0 {
        m.w32(object + 0xc, m.r32(object + 0xc) & !1);
        unlink(m, object);
        append(m, renderer + 0xc0, object);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    struct Device {
        fill: u8,
        dimensions: [u32; 2],
        events: Vec<Value>,
    }
    impl Host for Device {
        fn allocate(&mut self, m: &mut Space, size: u32) -> Result<u32, String> {
            let pointer = m.malloc(size);
            for offset in 0..size {
                m.w8(pointer + offset, self.fill);
            }
            self.events.push(json!(["allocate", size, pointer]));
            Ok(pointer)
        }
        fn dimensions(&mut self, _: &mut Space, _: u32) -> Result<[u32; 2], String> {
            self.events.push(json!([
                "dimensions",
                self.dimensions[0],
                self.dimensions[1]
            ]));
            Ok(self.dimensions)
        }
    }
    fn word(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn load(m: &mut Space, base: u32, words: &Value) {
        for (i, value) in words.as_array().unwrap().iter().enumerate() {
            m.w32(base + i as u32 * 4, word(value));
        }
    }
    fn state(m: &Space, renderer: u32) -> Value {
        json!({"heap_ptr":m.heap_ptr(),"heap":(slr_physics::space::HEAP_BASE..m.heap_ptr()).step_by(4).map(|a|m.r32(a)).collect::<Vec<_>>(),"renderer":(0..0x240).step_by(4).map(|offset|m.r32(renderer+offset)).collect::<Vec<_>>()})
    }
    fn verify_inventory(rows: &[Value]) -> Result<(), &'static str> {
        let ordinary = [0x3e800000, 0xbf000000, 0x3f400000, 0x3fc00000];
        let precision = [0, 0, 0x3f800000, 0x3f800001];
        let mut expected = std::collections::BTreeSet::new();
        for fill in [0, 0xa5] {
            for dimensions in [[800, 600], [960, 540], [u32::MAX, 0x80000001]] {
                for flags in [[127, 20, 127], [u32::MAX, 0, 0x80000000]] {
                    expected.insert(
                        [
                            vec![fill],
                            dimensions.to_vec(),
                            flags.to_vec(),
                            ordinary.to_vec(),
                        ]
                        .concat(),
                    );
                }
            }
            expected.insert(
                [
                    vec![fill],
                    vec![800, 0x01000001],
                    vec![127, 20, 127],
                    precision.to_vec(),
                ]
                .concat(),
            );
        }
        let plan = json!([
            ["construct", -1, 0],
            ["register", 0, 0],
            ["register", 1, 0],
            ["register", 2, 0],
            ["activate", 0, 4294967295u32],
            ["activate", 1, 4294967295u32],
            ["activate", 2, 4294967295u32],
            ["activate", 1, 5],
            ["deactivate", 1, 0],
            ["deactivate", 1, 0],
            ["deactivate", 0, 0],
            ["deactivate", 2, 0],
            ["activate", 2, 2147483648u32],
            ["activate", 0, 0],
            ["activate", 1, 4294967295u32]
        ]);
        let mut actual = std::collections::BTreeSet::new();
        for row in rows {
            let mut identity = vec![word(&row["fill"])];
            for (field, length) in [("dimensions", 2), ("flags", 3), ("rectangle", 4)] {
                let values = row[field].as_array().ok_or("missing profile field")?;
                if values.len() != length {
                    return Err("wrong profile field length");
                }
                identity.extend(values.iter().map(word));
            }
            if !actual.insert(identity) {
                return Err("duplicate profile");
            }
            let steps = row["steps"]
                .as_array()
                .ok_or("missing operation schedule")?;
            let schedule = steps
                .iter()
                .map(|step| json!([step["action"], step["index"], step["flag"]]))
                .collect::<Vec<_>>();
            if json!(schedule) != plan {
                return Err("altered operation schedule");
            }
        }
        if actual != expected {
            return Err("missing or unexpected profile");
        }
        Ok(())
    }
    #[test]
    fn original_renderer_constructor_and_retained_viewport_lifecycle() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/native-renderer-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(fixture["schema"], 1);
        assert_eq!(
            fixture["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        let sources = fixture["sources"].as_object().unwrap();
        assert_eq!(sources.len(), 2);
        for source in ["re/oracle/native_renderer_probe.py", "re/oracle/oracle.py"] {
            assert_eq!(
                sources[source],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(source)).unwrap())
                )
            );
        }
        let rows = fixture["rows"].as_array().unwrap();
        verify_inventory(rows).unwrap();
        let mut duplicated = rows.clone();
        duplicated[12] = duplicated[0].clone();
        assert!(
            verify_inventory(&duplicated).is_err(),
            "ordinary row replaced the precision case"
        );
        let mut missing = rows.clone();
        missing.pop();
        assert!(
            verify_inventory(&missing).is_err(),
            "missing precision case accepted"
        );
        let mut altered = rows.clone();
        altered[0]["steps"][5]["index"] = json!(0);
        assert!(
            verify_inventory(&altered).is_err(),
            "altered activation order accepted"
        );
        let mut altered = rows.clone();
        altered[0]["steps"][14]["flag"] = json!(5);
        assert!(
            verify_inventory(&altered).is_err(),
            "altered operation flag accepted"
        );
        for row in rows {
            let renderer = word(&row["renderer"]);
            assert_eq!(renderer, STATIC_RENDERER);
            let mut m = Space::new();
            load(
                &mut m,
                slr_physics::space::HEAP_BASE,
                &row["before"]["heap"],
            );
            load(&mut m, renderer, &row["before"]["renderer"]);
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            m.w32(0x6187b0, renderer);
            for index in 0..3 {
                let data = word(&row["payload"][index]);
                assert_eq!(m.r32(data + 0xc), word(&row["flags"][index]));
                for component in 0..4 {
                    assert_eq!(
                        m.r32(data + 0x10 + component as u32 * 4),
                        word(&row["rectangle"][component])
                    );
                }
            }
            let mut host = Device {
                fill: word(&row["fill"]) as u8,
                dimensions: [word(&row["dimensions"][0]), word(&row["dimensions"][1])],
                events: vec![],
            };
            assert_eq!(row["steps"].as_array().unwrap().len(), 15);
            for (step_index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
                host.events.clear();
                let action = step["action"].as_str().unwrap();
                let index = step["index"].as_i64().unwrap();
                let reference = if index >= 0 {
                    word(&row["reference"][index as usize])
                } else {
                    0
                };
                let result = match action {
                    "construct" => {
                        construct(&mut m, renderer);
                        renderer
                    }
                    "register" => register(
                        &mut m,
                        renderer,
                        reference,
                        word(&row["payload"][index as usize]),
                        &mut host,
                    )
                    .unwrap(),
                    "activate" => {
                        activate(&mut m, renderer, reference, word(&step["flag"])).unwrap()
                    }
                    "deactivate" => deactivate(&mut m, renderer, reference).unwrap(),
                    _ => panic!("unknown original action"),
                };
                assert_eq!(
                    result,
                    word(&step["result"]),
                    "result {step_index} {action}"
                );
                assert_eq!(
                    json!(host.events),
                    step["events"],
                    "events {step_index} {action}"
                );
                let actual = state(&m, renderer);
                assert_eq!(actual["heap_ptr"], step["state"]["heap_ptr"]);
                for (region, base) in [
                    ("heap", slr_physics::space::HEAP_BASE),
                    ("renderer", renderer),
                ] {
                    let actual = actual[region].as_array().unwrap();
                    let expected = step["state"][region].as_array().unwrap();
                    assert_eq!(actual.len(), expected.len());
                    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
                        assert_eq!(
                            got,
                            want,
                            "memory {:#x}, step {step_index} {action}, fill={} dimensions={}",
                            base + index as u32 * 4,
                            row["fill"],
                            row["dimensions"]
                        );
                    }
                }
            }
        }
    }
}
