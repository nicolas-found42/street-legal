//! Camera attachment and render transform records from48b940 and its callees.
use slr_physics::{cfg::PHYSICS_WORLD as WORLD, space::Space};
use slr_x87::{Precision, Rounding, X87};
fn single_list(m: &mut Space, p: u32) {
    m.w32(p, 0x5f09a8);
    for off in [4, 0x10] {
        m.w32(p + off, 0x5f09ac);
    }
    m.w32(p + 8, p + 0x10);
    m.w32(p + 0xc, 0);
    m.w32(p + 0x14, 0);
    m.w32(p + 0x18, p + 4);
}
fn double_list(m: &mut Space, p: u32) {
    m.w32(p, 0x5f2af4);
    for off in [4, 0x18] {
        m.w32(p + off, 0x5f2af8);
        for k in 1..=4 {
            m.w32(p + off + k * 4, 0);
        }
    }
    for i in 0..2 {
        m.w32(p + 8 + i * 8, p + 0x18);
        m.w32(p + 0xc + i * 8, 0);
        m.w32(p + 0x1c + i * 8, 0);
        m.w32(p + 0x20 + i * 8, p + 4);
    }
}
/// Original5404d0, invoked by543000 at rawresource+d8. Unwritten gaps
/// retain incoming bytes, including the owner id subsequently assigned543031.
pub fn initialize_render(m: &mut Space, p: u32) {
    m.w32(p + 4, 0);
    for off in [0x24, 0x40, 0x5c] {
        single_list(m, p + off);
    }
    double_list(m, p + 0x90);
    double_list(m, p + 0x124);
    double_list(m, p + 0x150);
    m.w32(p + 0xd0, 0x5f355c);
    m.w32(p + 0xd4, 0);
    m.w32(p + 0xd8, 0);
    for off in [0xdc, 0xe0, 0xe4, 0xe8] {
        m.w32(p + off, 0);
    }
    for off in [
        0x78, 0x7c, 0x80, 0x84, 0x110, 0x114, 0x118, 0x11c, 0x120, 0xbc, 0xc, 0x10, 0x14, 0x18,
        0x1c, 0x20, 0xc0, 0xc4, 0x8c, 0xfc, 0x100, 0x104, 0x108, 0x10c, 0x17c, 0x184, 0x188, 0x88,
    ] {
        m.w32(p + off, 0);
    }
    m.w32(p, 0x5f37e4);
    m.w32(p + 0xcc, u32::MAX);
    m.w32(p + 0xf4, m.r32(WORLD));
    m.w32(p + 0xf8, m.r32(0x6200a4));
    m.w32(p + 0x18c, m.r32(0x6200a4).wrapping_sub(1));
    m.w32(p + 0x180, 1f32.to_bits());
}
/// Original540270 skeleton transform constructor. Strings use the original
/// bounded copy; both matrices start as4x4 identity.
pub fn initialize_bone(m: &mut Space, p: u32, id: u32, name: &str) {
    m.w32(p, 0x5f37dc);
    for off in [4, 8, 0xc, 0x10, 0x44, 0x48, 0x4c, 0x50] {
        m.w32(p + off, 0);
    }
    m.w32(p + 0x34, id);
    let len = name.len().min(31);
    for (i, byte) in name.as_bytes()[..len].iter().enumerate() {
        m.w8(p + 0x14 + i as u32, *byte);
    }
    m.w8(p + 0x14 + len as u32, 0);
    m.w32(p + 0x38, u32::MAX);
    for base in [0x54, 0xb0] {
        for i in 0..16 {
            m.w32(
                p + base + i * 4,
                if i % 5 == 0 { 1f32.to_bits() } else { 0 },
            );
        }
    }
    for off in [0x94, 0x98, 0x9c, 0xa0, 0xa4, 0xa8, 0xac] {
        m.w32(p + off, 0);
    }
    m.w32(p + 0xf0, 1);
    m.w32(p + 0x3c, 2);
}
/// Original5411b0 sorted bone-name insertion, returning its16-bit search id.
pub fn bone(m: &mut Space, body: u32, name: &str) -> Result<(u32, u32), String> {
    let mut cursor = m.r32(body + 0x12c);
    if cursor == 0 {
        return Err("NativeCameraBoundary: render bone list is not constructed".into());
    }
    let mut low = 1u32;
    let mut high = 0xffffu32;
    let mut before = 0;
    let mut visited = vec![];
    while cursor != 0 && m.r32(cursor + 4) != 0 {
        if visited.contains(&cursor) {
            return Err("NativeCameraBoundary: cyclic render bone list".into());
        }
        visited.push(cursor);
        let old: Vec<u8> = (0..32)
            .map(|i| m.r8(cursor + 0x14 + i))
            .take_while(|b| *b != 0)
            .collect();
        match old.as_slice().cmp(name.as_bytes()) {
            std::cmp::Ordering::Equal => return Ok((m.r32(cursor + 0x34), cursor)),
            std::cmp::Ordering::Greater => {
                high = m.r32(cursor + 0x34);
                before = cursor;
                break;
            }
            std::cmp::Ordering::Less => {
                low = m.r32(cursor + 0x34);
                cursor = m.r32(cursor + 4);
            }
        }
    }
    let id = low.wrapping_add(high) >> 1;
    let bone = m.alloc(0xf4);
    initialize_bone(m, bone, id, name);
    let previous = if before == 0 {
        m.r32(body + 0x144)
    } else {
        m.r32(before + 8)
    };
    if previous == 0 {
        return Err("NativeCameraBoundary: render bone tail is not constructed".into());
    }
    let next = if before == 0 { body + 0x13c } else { before };
    m.w32(previous + 4, bone);
    m.w32(bone + 4, next);
    m.w32(bone + 8, previous);
    m.w32(next + 8, bone);
    m.w32(body + 0xbc, m.r32(body + 0xbc) | 0x1800);
    Ok((id, bone))
}
fn remove_active(m: &mut Space, bone: u32) {
    let next = m.r32(bone + 0x10);
    let previous = m.r32(bone + 0xc);
    if next != 0 && previous != 0 {
        m.w32(previous + 0x10, next);
        m.w32(next + 0xc, previous);
    }
    m.w32(bone + 0xc, 0);
    m.w32(bone + 0x10, 0);
}
/// A render bone and the resource/payload that own its controller.
#[derive(Clone, Copy)]
pub struct RenderBone {
    pub resource: u32,
    pub body: u32,
    pub bone: u32,
}

/// Original48bf50/48be10 camera bone transformation and controller replacement.
pub fn retarget(
    m: &mut Space,
    target: RenderBone,
    physics_ref: u32,
    physics_body: u32,
    position: [u32; 3],
    angles: [u32; 3],
) -> Result<(), String> {
    let matrix = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
        f32::from_bits(angles[0]),
        f32::from_bits(angles[1]),
        f32::from_bits(angles[2]),
    ))
    .m
    .map(f32::to_bits);
    retarget_matrix(m, target, physics_ref, physics_body, position, matrix)
}
/// Original48c0f0 matrix variant, sharing48be10 owner/controller retargeting.
pub fn retarget_matrix(
    m: &mut Space,
    target: RenderBone,
    physics_ref: u32,
    physics_body: u32,
    position: [u32; 3],
    matrix: [u32; 9],
) -> Result<(), String> {
    let RenderBone {
        resource: render_resource,
        body: render_body,
        bone,
    } = target;
    for i in 0..16 {
        let value = if i < 12 {
            if i % 4 < 3 {
                matrix[(i % 4 * 3 + i / 4) as usize]
            } else {
                0
            }
        } else if i < 15 {
            X87::from_f32(f32::from_bits(position[(i - 12) as usize]))
                .mul(X87::from_f32(10.), Precision::F32, Rounding::NearestEven)
                .to_f32(Precision::F32, Rounding::NearestEven)
                .to_bits()
        } else {
            1f32.to_bits()
        };
        m.w32(bone + 0x54 + i * 4, value);
    }
    m.w32(bone + 0xf0, 1);
    let identity = [0x54, 0x68, 0x7c]
        .into_iter()
        .all(|off| m.r32(bone + off) == 1f32.to_bits())
        && position.into_iter().all(|word| f32::from_bits(word) == 0.);
    m.w32(
        bone + 0x3c,
        if identity {
            m.r32(bone + 0x3c) | 2
        } else {
            m.r32(bone + 0x3c) & !2
        },
    );
    remove_active(m, bone);
    let previous = m.r32(render_body + 0x178);
    if previous == 0 {
        return Err("NativeCameraBoundary: render active-bone list is not constructed".into());
    }
    m.w32(previous + 0xc, bone);
    m.w32(bone + 0xc, render_body + 0x168);
    m.w32(bone + 0x10, previous);
    m.w32(render_body + 0x178, bone);
    m.w32(render_body + 0xbc, m.r32(render_body + 0xbc) | 0x1800);
    if (physics_ref == 0 && m.r32(bone + 0x4c) != 0)
        || (physics_ref != 0 && m.r32(bone + 0x4c) != m.r32(physics_ref + 8))
    {
        let controller = m.r32(bone + 0xac);
        if controller != 0 {
            crate::native_part::bind_reference(m, controller + 0xc, 0);
            m.w32(controller, 0x5f09ac);
            let next = m.r32(controller + 8);
            let previous = m.r32(controller + 4);
            if next != 0 && previous != 0 {
                m.w32(previous + 8, next);
                m.w32(next + 4, previous);
            }
            m.w32(controller + 4, 0);
            m.w32(controller + 8, 0);
            m.w32(
                render_resource + 0x54,
                m.r32(render_resource + 0x54) & !0x4000,
            );
        }

        remove_active(m, bone);
        crate::native_part::bind_reference(m, bone + 0x44, m.r32(physics_ref + 0xc));
        let previous = m.r32(render_body + 0x178);
        if previous == 0 {
            return Err(
                "NativeCameraBoundary: physics attached-bone list is not constructed".into(),
            );
        }
        m.w32(previous + 0xc, bone);
        m.w32(bone + 0xc, render_body + 0x168);
        m.w32(bone + 0x10, previous);
        m.w32(render_body + 0x178, bone);
        if physics_ref == 0 {
            //48be10 null-owner branch retains the now-deleted controller pointer.
            m.w32(render_body + 0xf8, m.r32(0x6200a4));
            return Ok(());
        }
        let flags = m.r32(render_resource + 0x54);
        if flags != 0 && flags & 0x4000 == 0 {
            let controller = m.alloc(0x1c);
            m.w32(controller, 0x5f355c);
            crate::native_part::bind_reference(m, controller + 0xc, render_resource);
            m.w32(render_resource + 0x54, flags | 0x4000);
            let previous = m.r32(physics_body + 0x80);
            if previous == 0 {
                return Err(
                    "NativeCameraBoundary: physics controller list is not constructed".into(),
                );
            }
            m.w32(previous + 4, controller);
            m.w32(controller + 8, previous);
            m.w32(controller + 4, physics_body + 0x78);
            m.w32(physics_body + 0x80, controller);
            m.w32(bone + 0xac, controller);
        } else {
            m.w32(bone + 0xac, 0);
        }
    }
    m.w32(render_body + 0xf8, m.r32(0x6200a4));
    Ok(())
}
#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub aspect: u32,
    pub fov: u32,
    pub near: u32,
    pub far: u32,
    pub flags: u32,
}
impl Parameters {
    /// Original539500 tail after real resource allocation/materialization.
    pub fn initialize_payload(self, m: &mut Space, body: u32) {
        let pc = Precision::F32;
        let rc = Rounding::NearestEven;
        let f = |v: u32| X87::from_f32(f32::from_bits(v));
        m.w32(
            body + 0xc,
            X87::from_f32(1.)
                .div(f(self.aspect), pc, rc)
                .to_f32(pc, rc)
                .to_bits(),
        );
        for (off, value) in [
            (0x10, self.fov),
            (
                0x14,
                f(self.near)
                    .mul(X87::from_f32(10.), pc, rc)
                    .to_f32(pc, rc)
                    .to_bits(),
            ),
            (
                0x18,
                f(self.far)
                    .mul(X87::from_f32(10.), pc, rc)
                    .to_f32(pc, rc)
                    .to_bits(),
            ),
            (0x30, self.flags),
            (0x34, 0),
            (0x38, 1f32.to_bits()),
        ] {
            m.w32(body + off, value);
        }
    }
}
pub trait Host {
    fn setting(&mut self, name: &str) -> Result<u32, String>;
    fn body(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    fn create_camera(&mut self, m: &mut Space, parameters: Parameters) -> Result<u32, String>;
    fn create_render(
        &mut self,
        m: &mut Space,
        owner: Option<u32>,
        camera: u32,
        name: &str,
    ) -> Result<(u32, u32), String>;
}
/// Addresses of the live three-word position and angle arrays supplied by cfg.
pub struct PosePointers {
    pub position: u32,
    pub angles: u32,
}

/// Original48b940. Destination andphysics_ref are raw16-byte resource refs;
/// position andangles are live3-word parameter arrays, fov is supplied bycfg.
pub fn create(
    m: &mut Space,
    destination: u32,
    physics_ref: u32,
    pose: PosePointers,
    fov: u32,
    flags: u32,
    host: &mut dyn Host,
) -> Result<bool, String> {
    let PosePointers { position, angles } = pose;
    let physics_resource = m.r32(physics_ref + 0xc);
    if physics_resource == 0 {
        return Ok(false);
    }
    let physics_body = host.body(m, physics_resource)?;
    let entity = m.r32(physics_body + 0x84);
    if entity == 0 {
        return Ok(false);
    }
    let near = host.setting("camera_int_clipplane")?;
    let far = host.setting("camera_int_viewrange")?;
    let camera = host.create_camera(
        m,
        Parameters {
            aspect: 0x3f400000,
            fov,
            near,
            far,
            flags,
        },
    )?;
    let owner = if m.r32(entity + 0x40) != 0 && m.r32(entity + 0x44) != 0 {
        let raw = m.r32(m.r32(entity + 0x44) + 0xcc);
        (raw != 0).then_some(raw)
    } else {
        None
    };
    let (render, body) = host.create_render(m, owner, camera, "cam")?;
    crate::native_part::bind_reference(m, destination, render);
    let (_, bone) = bone(m, body, "bone00")?;
    retarget(
        m,
        RenderBone {
            resource: render,
            body,
            bone,
        },
        physics_ref,
        physics_body,
        std::array::from_fn(|i| m.r32(position + i as u32 * 4)),
        std::array::from_fn(|i| m.r32(angles + i as u32 * 4)),
    )?;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_camera_bone_retarget_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-camera-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_camera_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["retarget"].as_array().unwrap() {
            let mut m = Space::new();
            m.w32(0x6200a4, 123);
            let reference = m.alloc(16);
            let resource = m.alloc(0x100);
            let body = m.alloc(0x190);
            let physref = m.alloc(16);
            let physres = m.alloc(0x100);
            let physbody = m.alloc(0x90);
            let pos = m.alloc(12);
            let angles = m.alloc(12);
            let name = m.alloc(16);
            for (i, b) in b"bone00\0".iter().enumerate() {
                m.w8(name + i as u32, *b);
            }
            initialize_render(&mut m, body);
            for (r, id) in [(resource, 0xffff0100), (physres, 0xffff0080)] {
                m.w32(r, 0x30000000);
                m.w32(r + 0x4c, 1);
                m.w32(r + 0x50, id);
                m.w32(r + 0x54, 0x201);
            }
            crate::native_part::bind_reference(&mut m, reference, resource);
            crate::native_part::bind_reference(&mut m, physref, physres);
            m.w32(physbody + 0x80, physbody + 0x6c);
            m.w32(physbody + 0x70, physbody + 0x78);
            let position = std::array::from_fn(|i| row["position"][i].as_u64().unwrap() as u32);
            let rotation = std::array::from_fn(|i| row["angles"][i].as_u64().unwrap() as u32);
            for i in 0..3 {
                m.w32(pos + i * 4, position[i as usize]);
                m.w32(angles + i * 4, rotation[i as usize]);
            }
            let (id, bone) = bone(&mut m, body, "bone00").unwrap();
            assert_eq!(id, row["id"].as_u64().unwrap() as u32);
            retarget(
                &mut m,
                RenderBone {
                    resource,
                    body,
                    bone,
                },
                physref,
                physbody,
                position,
                rotation,
            )
            .unwrap();
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            let actual = (slr_physics::space::HEAP_BASE..m.heap_ptr())
                .step_by(4)
                .filter_map(|p| {
                    let value = m.r32(p);
                    (value != 0).then_some((p, value))
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for address in actual.keys().chain(expected.keys()) {
                assert_eq!(
                    actual.get(address).copied().unwrap_or(0),
                    expected.get(address).copied().unwrap_or(0),
                    "{address:#x}"
                );
            }
        }
    }
    #[test]
    fn original_controller_replacement_and_null_retarget_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-camera-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_camera_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["replacement"].as_array().unwrap() {
            let mut m = Space::new();
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            m.w32(0x6200a4, 123);
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            let a = |key: &str| row[key].as_u64().unwrap() as u32;
            retarget(
                &mut m,
                RenderBone {
                    resource: a("resource"),
                    body: a("body"),
                    bone: a("bone"),
                },
                a("target_ref"),
                a("target_body"),
                std::array::from_fn(|i| row["position"][i].as_u64().unwrap() as u32),
                std::array::from_fn(|i| row["angles"][i].as_u64().unwrap() as u32),
            )
            .unwrap();
            assert_eq!(m.heap_ptr(), a("heap"));
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for address in (slr_physics::space::HEAP_BASE..m.heap_ptr()).step_by(4) {
                assert_eq!(
                    m.r32(address),
                    expected.get(&address).copied().unwrap_or(0),
                    "{} {address:#x}",
                    row["mode"]
                );
            }
        }
    }
    #[test]
    fn original_render_and_bone_constructors_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-camera-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_camera_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let is_render = row["kind"] == "render";
            let size = if is_render { 0x190 } else { 0xf4 };
            let p = m.alloc(size);
            for i in 0..size {
                m.w8(p + i, row["fill"].as_u64().unwrap() as u8);
            }
            m.w32(WORLD, 0x3f123456);
            m.w32(0x6200a4, 123);
            if is_render {
                initialize_render(&mut m, p);
            } else {
                initialize_bone(&mut m, p, 0x8000, "bone00");
            }
            let actual = (0..size / 4).map(|i| m.r32(p + i * 4)).collect::<Vec<_>>();
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{} fill{}", row["kind"], row["fill"]);
        }
    }
}
