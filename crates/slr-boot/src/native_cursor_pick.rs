//! Original48d450 viewport/camera traversal and ray orchestration. The ray
//! query remains a live collaborator; an empty/missing renderer is never
//! substituted for a constructed renderer's actual intrusive list.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn f(word: u32) -> X87 {
    X87::from_f32(f32::from_bits(word))
}
fn st(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}
#[cfg(test)]
fn read3(m: &Space, p: u32) -> [u32; 3] {
    [m.r32(p), m.r32(p + 4), m.r32(p + 8)]
}

pub trait Host {
    /// Original49e3d0. The six contiguous words are the two unprojected ray
    /// endpoints. The callback owns its real hit-global writes and may mutate
    /// renderer/camera lists before the traversal reads its next node.
    fn ray(
        &mut self,
        m: &mut Space,
        ray: [u32; 6],
        selection: u32,
        limit: &mut u32,
    ) -> Result<u32, String>;
}

fn previous(m: &Space, current: u32) -> u32 {
    let previous = m.r32(current + 8);
    if previous == 0 || m.r32(previous + 8) == 0 {
        0
    } else {
        previous
    }
}
///4fdc40 and516c20: reverse list traversal, starting from the retained tail.
fn first(m: &Space, owner: u32, first_offset: u32, tail_offset: u32) -> Result<u32, String> {
    let node = m.r32(owner + first_offset);
    if node == 0 {
        return Err(
            "NativeCursorPickBoundary: original renderer/camera list header is not constructed"
                .into(),
        );
    }
    if m.r32(node + 4) == 0 {
        Ok(0)
    } else {
        Ok(m.r32(owner + tail_offset))
    }
}

/// exe: FUN_00513840
pub fn unproject(m: &Space, camera: u32, point: [u32; 3]) -> [u32; 3] {
    let r = |offset| f(m.r32(camera + offset));
    let [x, y, z] = point.map(f);
    // The first numerator stays in an x87 register; the other two and inverse
    // W are explicitly stored to single precision before the world matrix.
    let x = r(0x364)
        .mul(y, PC, RC)
        .add(r(0x374).mul(z, PC, RC), PC, RC)
        .add(r(0x354).mul(x, PC, RC), PC, RC)
        .add(r(0x384), PC, RC);
    let yy = st(r(0x368)
        .mul(y, PC, RC)
        .add(r(0x358).mul(f(point[0]), PC, RC), PC, RC)
        .add(r(0x378).mul(z, PC, RC), PC, RC)
        .add(r(0x388), PC, RC));
    let zz = st(r(0x36c)
        .mul(y, PC, RC)
        .add(r(0x35c).mul(f(point[0]), PC, RC), PC, RC)
        .add(r(0x37c).mul(z, PC, RC), PC, RC)
        .add(r(0x38c), PC, RC));
    let w = r(0x370)
        .mul(y, PC, RC)
        .add(r(0x360).mul(f(point[0]), PC, RC), PC, RC)
        .add(r(0x380).mul(z, PC, RC), PC, RC)
        .add(r(0x390), PC, RC);
    let inverse = f(st(f(0x3f800000).div(w, PC, RC)));
    let x = x.mul(inverse, PC, RC);
    let y = f(yy).mul(inverse, PC, RC);
    let z = f(zz).mul(inverse, PC, RC);
    std::array::from_fn(|i| {
        let i = i as u32 * 4;
        st(z.mul(r(0x2b4 + i), PC, RC)
            .add(y.mul(r(0x2a4 + i), PC, RC), PC, RC)
            .add(x.mul(r(0x294 + i), PC, RC), PC, RC)
            .add(r(0x2c4 + i), PC, RC))
    })
}

fn inside(word: u32) -> bool {
    let lo = f(word).fcom(f(0xbf800000)).0;
    let hi = f(word).fcom(f(0x3f800000)).0;
    !lo.c0 && (hi.c0 || hi.c3)
}
fn normalize(point: u32, origin: u32, extent: u32) -> u32 {
    let offset = f(origin).add(f(origin), PC, RC).sub(f(0x3f800000), PC, RC);
    st(f(point)
        .sub(offset, PC, RC)
        .div(f(extent), PC, RC)
        .sub(f(0x3f800000), PC, RC))
}
fn counted(m: &mut Space, link: u32, target: u32) {
    crate::native_car_command::unlink_entity(m, link);
    m.w32(link, target);
    if target == 0 {
        m.w32(link + 4, 0);
    } else {
        m.w32(link + 4, m.r32(target));
        m.w32(target, link);
        m.w32(target + 4, m.r32(target + 4).wrapping_add(1));
    }
}

/// exe: FUN_0048d450
pub fn pick(m: &mut Space, a: [u32; 10], host: &mut dyn Host) -> Result<u32, String> {
    // Read the input position lazily, after each callback, as the original
    // does. Output arguments may alias ordinary caller-owned state.
    if a[1] != 0 {
        crate::native_install::release(m, a[1]);
    }
    if a[2] != 0 {
        crate::native_car_command::unlink_entity(m, a[2]);
    }
    for p in [a[3], a[4]] {
        if p != 0 {
            for i in 0..3 {
                m.w32(p + i * 4, 0);
            }
        }
    }
    if a[5] != 0 {
        m.w32(a[5], u32::MAX);
    }
    if a[6] != 0 {
        m.w32(a[6], 0);
    }
    if a[9] != 0 {
        m.w32(a[9], 0);
    }
    let renderer = m.r32(0x6187b0);
    if renderer == 0 {
        return Err("NativeCursorPickBoundary: original renderer6187b0 is absent".into());
    }
    let mut viewport = first(m, renderer, 0xac, 0xbc)?;
    while viewport != 0 {
        let x = normalize(m.r32(a[0]), m.r32(viewport + 0x5c), m.r32(viewport + 0x64));
        let y = normalize(
            m.r32(a[0] + 4),
            m.r32(viewport + 0x60),
            m.r32(viewport + 0x68),
        );
        if inside(x) && inside(y) {
            let mut camera = first(m, viewport, 0x2c, 0x3c)?;
            while camera != 0 {
                let neg_y = y ^ 0x80000000;
                let far = unproject(m, camera, [x, neg_y, 0x3f800000])
                    .map(|v| st(f(v).mul(f(0x3dcccccd), PC, RC)));
                let near = unproject(m, camera, [x, neg_y, 0])
                    .map(|v| st(f(v).mul(f(0x3dcccccd), PC, RC)));
                let ray = [far[0], far[1], far[2], near[0], near[1], near[2]];
                let mut limit = 0x3f800000;
                let result = host.ray(m, ray, camera + 0x658, &mut limit)?;
                if result != 0 {
                    if a[1] != 0 {
                        crate::native_part::bind_reference(m, a[1], m.r32(0x6435b8));
                    }
                    if a[2] != 0 {
                        counted(m, a[2], m.r32(0x6435a4));
                    }
                    for (dst, source) in [(a[3], 0x643584), (a[4], 0x643590)] {
                        if dst != 0 {
                            for i in 0..3 {
                                m.w32(dst + i * 4, m.r32(source + i * 4));
                            }
                        }
                    }
                    if a[5] != 0 {
                        m.w32(a[5], m.r32(0x64359c));
                    }
                    if a[6] != 0 {
                        m.w32(a[6], m.r32(0x643580));
                    }
                    for (dst, vector) in [(a[7], far), (a[8], near)] {
                        if dst != 0 {
                            for (i, word) in vector.into_iter().enumerate() {
                                m.w32(dst + i as u32 * 4, word);
                            }
                        }
                    }
                    if a[9] != 0 {
                        m.w32(a[9], m.r32(0x6435a0));
                    }
                    return Ok(result);
                }
                camera = previous(m, camera);
            }
        }
        viewport = previous(m, viewport);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    const GLOBALS: [u32; 17] = [
        0x6187b0, 0x617650, 0x643580, 0x643584, 0x643588, 0x64358c, 0x643590, 0x643594, 0x643598,
        0x64359c, 0x6435a0, 0x6435a4, 0x6435a8, 0x6435ac, 0x6435b0, 0x6435b4, 0x6435b8,
    ];
    fn u(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn load(m: &mut Space, v: &Value) {
        for (i, word) in v.as_array().unwrap().iter().enumerate() {
            m.w32(0x20000000 + i as u32 * 4, u(word));
        }
        m.set_heap_ptr(0x20000000 + v.as_array().unwrap().len() as u32 * 4);
    }
    fn compare(m: &Space, v: &Value, label: &str) {
        assert_eq!(
            (m.heap_ptr() - 0x20000000).div_ceil(4) as usize,
            v.as_array().unwrap().len(),
            "{label} heap size"
        );
        for (i, word) in v.as_array().unwrap().iter().enumerate() {
            assert_eq!(
                m.r32(0x20000000 + i as u32 * 4),
                u(word),
                "{label} heap+{:x}",
                i * 4
            );
        }
    }
    fn global_shape(v: &Value) -> bool {
        v.as_object().is_some_and(|v| {
            v.len() == GLOBALS.len()
                && GLOBALS.iter().all(|p| {
                    v.get(&p.to_string())
                        .and_then(Value::as_u64)
                        .is_some_and(|n| n <= u32::MAX as u64)
                })
        })
    }
    fn globals(m: &mut Space, v: &Value) {
        assert!(global_shape(v));
        for (address, word) in v.as_object().unwrap() {
            m.w32(address.parse().unwrap(), u(word));
        }
    }
    fn compare_globals(m: &Space, v: &Value) {
        assert!(global_shape(v));
        for (address, word) in v.as_object().unwrap() {
            assert_eq!(m.r32(address.parse().unwrap()), u(word), "global {address}");
        }
    }
    fn vectors() -> Value {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-cursor-pick-original.json"))
                .expect("run native_cursor_pick_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["fpu_cw"], 0x7f);
        assert_eq!(data["unicorn"], "2.1.4");
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("stock_exe_sha256", "stock/StreetLegal_Redline.exe"),
            ("producer_sha256", "re/oracle/native_cursor_pick_probe.py"),
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
        assert_eq!(
            data["profile_sha256"],
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&data["profile"]).unwrap())
            )
        );
        assert_eq!(data["profile"]["hooks"], json!([0x49e3d0]));
        data
    }
    #[test]
    fn original_unprojection_matches_full_heap_including_aliased_input() {
        let data = vectors();
        let rows = data["unprojections"].as_array().unwrap();
        assert_eq!(rows.len(), 48);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["fpu_depth"], 0);
            assert_eq!(row["fpu_cw"], 0x7f);
            assert_eq!(row["esp_delta"], 8);
            let mut m = Space::new();
            load(&mut m, &row["before"]);
            let expected_len = if row["alias"] == 1 { 415 } else { 419 };
            assert_eq!(row["before"].as_array().unwrap().len(), expected_len);
            let result = unproject(&m, u(&row["camera"]), read3(&m, u(&row["point"])));
            for (j, word) in result.into_iter().enumerate() {
                m.w32(u(&row["out"]) + j as u32 * 4, word);
            }
            compare(&m, &row["after"], &format!("unprojection{i}"));
        }
    }
    struct Controlled<'a> {
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Host for Controlled<'_> {
        fn ray(
            &mut self,
            m: &mut Space,
            ray: [u32; 6],
            selection: u32,
            limit: &mut u32,
        ) -> Result<u32, String> {
            let index = self.events.len();
            let expected = self.row["events"]
                .as_array()
                .unwrap()
                .get(index)
                .ok_or("unexpected ray callback")?;
            let result = self.row["responses"].get(index).map(u).unwrap_or(0);
            assert_eq!(result, u(&expected["result"]));
            let heap: Vec<_> = (0..(m.heap_ptr() - 0x20000000).div_ceil(4))
                .map(|i| m.r32(0x20000000 + i * 4))
                .collect();
            self.events.push(json!({"ray":ray,"selection":selection,"limit":*limit,"context":m.r32(0x617650),"heap":heap,"result":result}));
            if result != 0 {
                m.w32(0x6435b8, u(&self.row["resources"][1 + index % 2]));
                m.w32(0x6435a4, u(&self.row["entities"][1]));
                for (p, values) in [
                    (
                        0x643584,
                        [0.125f32.to_bits(), (-0.25f32).to_bits(), 0.5f32.to_bits()],
                    ),
                    (0x643590, [(-1f32).to_bits(), 0, 1f32.to_bits()]),
                ] {
                    for (i, word) in values.into_iter().enumerate() {
                        m.w32(p + i as u32 * 4, word);
                    }
                }
                m.w32(0x64359c, 0xabcd1234 + index as u32);
                m.w32(0x643580, 0.375f32.to_bits());
                m.w32(0x6435a0, 0xf00d0000 + index as u32);
            }
            if self.row["mutate"] == true && index == 0 {
                if let Some(camera) = self.row["cameras"].as_array().unwrap().first() {
                    m.w32(u(camera) + 0x2c4, 17f32.to_bits());
                }
            }
            *limit = 0.5f32.to_bits();
            Ok(result)
        }
    }
    #[test]
    fn original_picking_matches_all_heaps_callbacks_globals_and_nullable_outputs() {
        let data = vectors();
        let rows = data["picks"].as_array().unwrap();
        assert_eq!(rows.len(), 34);
        let mut total_callbacks = 0;
        for row in rows {
            assert_eq!(row["fpu_depth"], 0);
            assert_eq!(row["fpu_cw"], 0x7f);
            assert_eq!(row["esp_delta"], 0);
            let mut m = Space::new();
            load(&mut m, &row["before"]);
            globals(&mut m, &row["before_globals"]);
            let heap_words = row["before"].as_array().unwrap().len();
            assert_eq!(row["after"].as_array().unwrap().len(), heap_words);
            for event in row["events"].as_array().unwrap() {
                assert_eq!(event["heap"].as_array().unwrap().len(), heap_words);
                assert_eq!(event["ray"].as_array().unwrap().len(), 6);
            }
            let args = std::array::from_fn(|i| u(&row["args"][i]));
            let mut host = Controlled {
                row,
                events: vec![],
            };
            assert_eq!(
                pick(&mut m, args, &mut host).unwrap(),
                u(&row["result"]),
                "{}",
                row["label"]
            );
            assert_eq!(
                host.events,
                *row["events"].as_array().unwrap(),
                "{} callbacks",
                row["label"]
            );
            total_callbacks += host.events.len();
            compare(&m, &row["after"], row["label"].as_str().unwrap());
            compare_globals(&m, &row["globals"]);
        }
        assert_eq!(total_callbacks, 37);
    }
}
