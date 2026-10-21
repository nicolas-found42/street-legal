//! Source spatial resource bounds and index collaborators4986f0..498dd0.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn f(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn stored(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}
fn add(a: u32, b: u32) -> u32 {
    stored(f(a).add(f(b), PC, RC))
}
fn sub(a: u32, b: u32) -> u32 {
    stored(f(a).sub(f(b), PC, RC))
}
fn mul(a: u32, b: u32) -> u32 {
    stored(f(a).mul(f(b), PC, RC))
}
fn lt(a: u32, b: u32) -> bool {
    f32::from_bits(a) < f32::from_bits(b)
}
fn le(a: u32, b: u32) -> bool {
    f32::from_bits(a) <= f32::from_bits(b)
}
fn abs(v: u32) -> u32 {
    v & 0x7fffffff
}
/// Original54b850: classify a sphere against a sphere/AABB bounds descriptor.
pub fn classify_sphere(bounds: [u32; 7], center: [u32; 3], radius: u32) -> u32 {
    let delta: [u32; 3] = std::array::from_fn(|i| sub(bounds[1 + i], center[i]));
    if bounds[0] == 0 {
        let d = add(
            add(mul(delta[2], delta[2]), mul(delta[1], delta[1])),
            mul(delta[0], delta[0]),
        );
        let sum = add(radius, bounds[4]);
        if lt(mul(sum, sum), d) {
            return 1;
        }
        let difference = sub(bounds[4], radius);
        if le(mul(difference, difference), d) {
            return 2;
        }
        if lt(bounds[4], radius) {
            return 4;
        }
    } else if bounds[0] == 1 {
        let d = delta.map(abs);
        if (0..3).any(|i| lt(add(radius, bounds[4 + i]), d[i])) {
            return 1;
        }
        if (0..3).any(|i| le(sub(bounds[4 + i], radius), d[i])) {
            let sum = add(
                add(
                    add(
                        add(
                            add(mul(bounds[6], bounds[6]), mul(bounds[5], bounds[5])),
                            mul(bounds[4], bounds[4]),
                        ),
                        mul(d[0], d[0]),
                    ),
                    mul(d[1], d[1]),
                ),
                mul(d[2], d[2]),
            );
            return if le(mul(radius, radius), sum) { 2 } else { 4 };
        }
    } else {
        return 0;
    }
    8
}
/// Original54ba20: classify an AABB against a sphere/AABB descriptor.
pub fn classify_box(bounds: [u32; 7], center: [u32; 3], half: [u32; 3]) -> u32 {
    let d: [u32; 3] = std::array::from_fn(|i| abs(sub(center[i], bounds[1 + i])));
    if bounds[0] == 0 {
        if (0..3).any(|i| lt(add(half[i], bounds[4]), d[i])) {
            return 1;
        }
        if (0..3).all(|i| lt(d[i], sub(half[i], bounds[4]))) {
            return 4;
        }
        let sum = add(
            add(
                add(
                    add(
                        add(mul(half[0], half[0]), mul(half[2], half[2])),
                        mul(half[1], half[1]),
                    ),
                    mul(d[0], d[0]),
                ),
                mul(d[1], d[1]),
            ),
            mul(d[2], d[2]),
        );
        if lt(sum, mul(bounds[4], bounds[4])) {
            return 8;
        }
    } else if bounds[0] == 1 {
        if (0..3).any(|i| lt(add(half[i], bounds[4 + i]), d[i])) {
            return 1;
        }
        if (0..3).all(|i| lt(d[i], sub(half[i], bounds[4 + i]))) {
            return 4;
        }
        if (0..3).all(|i| lt(d[i], sub(bounds[4 + i], half[i]))) {
            return 8;
        }
    } else {
        return 0;
    }
    2
}
pub trait Host {
    /// Original537790 schedules the resource's real vcall+10 request.
    fn request(&mut self, m: &mut Space, resource: u32, args: [u32; 3]) -> Result<(), String>;
}
/// Original4986f0: resource AABB encloses both integration-buffer centers.
pub fn swept_bounds(m: &mut Space, entity: u32, host: &mut dyn Host) -> Result<(), String> {
    let resource = m.r32(entity + 0x54);
    if resource == 0 {
        return Ok(());
    }
    let body = m.r32(entity + 0x5c);
    let definition = m.r32(body + 0x78);
    if body == 0 || definition == 0 {
        return Err("NativeSpatialBoundary: swept bounds require actual body definition".into());
    }
    let active = m.r32(entity + 0x20c);
    if active > 1 {
        return Err("NativeSpatialBoundary: invalid state buffer".into());
    }
    let a = entity + 0x64 + active * 0xd4;
    let b = entity + 0x64 + (active ^ 1) * 0xd4;
    let radius = m.r32(definition + 0xc);
    for axis in 0..3 {
        let offset = axis * 4;
        let av = m.r32(a + offset);
        let bv = m.r32(b + offset);
        let sum = if axis == 0 { add(bv, av) } else { add(av, bv) };
        m.w32(resource + 0x84 + offset, mul(sum, 0x3f000000));
        let difference = abs(sub(av, bv));
        m.w32(
            resource + 0x90 + offset,
            add(mul(difference, 0x3f000000), radius),
        );
    }
    m.w32(resource + 0x80, 1);
    if m.r32(resource + 0x54) & 0x100 != 0 {
        host.request(m, resource, [0x3dcccccd, 0, 0x3f800000])?;
    }
    Ok(())
}
/// Original4988a0: union the bounded type2 children in the first tree list.
pub fn aggregate_bounds(m: &mut Space, resource: u32) -> Result<(), String> {
    let head = m.r32(resource + 0x20);
    if head == 0 {
        return Err("NativeSpatialBoundary: resource child-list storage is uninitialized".into());
    }
    let mut node = head;
    let mut low = [0; 3];
    let mut high = [0; 3];
    let mut first = true;
    let mut seen = std::collections::HashSet::new();
    while node != 0 && m.r32(node + 4) != 0 {
        if !seen.insert(node) {
            return Err("NativeSpatialBoundary: cyclic resource children".into());
        }
        if m.r32(node + 0x4c) == 2 && m.r32(node + 0x80) != 2 {
            for axis in 0..3 {
                let p = m.r32(node + 0x84 + axis as u32 * 4);
                let r = m.r32(node + 0x90 + axis as u32 * 4);
                let l = sub(p, r);
                let h = add(r, p);
                if first || lt(l, low[axis]) {
                    low[axis] = l;
                }
                if first || lt(high[axis], h) {
                    high[axis] = h;
                }
            }
            first = false;
        }
        node = m.r32(node + 4);
    }
    if !first {
        m.w32(resource + 0x80, 1);
        for axis in 0..3 {
            let extent = mul(sub(high[axis], low[axis]), 0x3f000000);
            m.w32(resource + 0x84 + axis as u32 * 4, add(low[axis], extent));
            m.w32(resource + 0x90 + axis as u32 * 4, extent);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Requests(Vec<Vec<u32>>);
    impl Host for Requests {
        fn request(&mut self, _m: &mut Space, r: u32, args: [u32; 3]) -> Result<(), String> {
            self.0.push(std::iter::once(r).chain(args).collect());
            Ok(())
        }
    }
    #[test]
    fn original_spatial_bounds_kernels_and_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spatial-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run native_spatial_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (i, row) in data["kernels"].as_array().unwrap().iter().enumerate() {
            let bounds = std::array::from_fn(|i| row["bounds"][i].as_u64().unwrap() as u32);
            let center = std::array::from_fn(|i| row["center"][i].as_u64().unwrap() as u32);
            let half = std::array::from_fn(|i| row["half"][i].as_u64().unwrap() as u32);
            assert_eq!(
                classify_sphere(bounds, center, half[0]),
                row["sphere"].as_u64().unwrap() as u32,
                "sphere{i}"
            );
            assert_eq!(
                classify_box(bounds, center, half),
                row["box"].as_u64().unwrap() as u32,
                "box{i}"
            );
        }
        for field in ["aggregate", "swept"] {
            for row in data[field].as_array().unwrap() {
                let mut m = Space::new();
                for pair in row["input_words"].as_array().unwrap() {
                    m.w32(
                        pair[0].as_u64().unwrap() as u32,
                        pair[1].as_u64().unwrap() as u32,
                    );
                }
                m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
                if field == "aggregate" {
                    aggregate_bounds(&mut m, row["resource"].as_u64().unwrap() as u32).unwrap();
                } else {
                    let mut host = Requests::default();
                    swept_bounds(&mut m, row["entity"].as_u64().unwrap() as u32, &mut host)
                        .unwrap();
                    assert_eq!(serde_json::json!(host.0), row["calls"]);
                }
                let expected = row["output_words"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32))
                    .collect::<std::collections::BTreeMap<_, _>>();
                for p in (slr_physics::space::HEAP_BASE..m.heap_ptr()).step_by(4) {
                    assert_eq!(
                        m.r32(p),
                        expected.get(&p).copied().unwrap_or(0),
                        "{field} address{p:#x}"
                    );
                }
            }
        }
    }
}

/// Original535a70/535e70 request queue storage component.
/// Other manager components are initialized by their owning subsystems.
pub fn initialize_request_pool(m: &mut Space, manager: u32) {
    for offset in [0xde8, 0xe04] {
        let list = manager + offset;
        for (offset, value) in [
            (0, 0x5f09a8),
            (4, 0x5f09ac),
            (8, list + 0x10),
            (12, 0),
            (16, 0x5f09ac),
            (20, 0),
            (24, list + 4),
        ] {
            m.w32(list + offset, value);
        }
    }
    for index in 0..0x4000 {
        let node = manager + 0xe20 + index * 0x3c;
        m.w32(node, 0x5f3558);
        for offset in [4, 8, 0x18, 0x1c, 0x20, 0x24] {
            m.w32(node + offset, 0);
        }
        let previous = m.r32(manager + 0xe1c);
        m.w32(previous + 4, node);
        m.w32(node + 4, manager + 0xe14);
        m.w32(node + 8, previous);
        m.w32(manager + 0xe1c, node);
    }
}
/// Original537790 with the original53fce0 physical-resource virtual request.
pub fn request_physics(m: &mut Space, resource: u32, args: [u32; 3]) -> Result<(), String> {
    if m.r32(resource) != 0x5f37a4 {
        return Err(format!(
            "NativeSpatialBoundary: request virtual+10 for vtable{:#x}",
            m.r32(resource)
        ));
    }
    //53fce0 calls53fcb0, whose get-data returns the embedded+d8 payload.
    let needed = if m.r32(resource + 0x54) & 0x4000000 != 0 {
        false
    } else {
        let head = m.r32(resource + 0x148);
        if head == 0 {
            return Err(
                "NativeSpatialBoundary: physical request child-list storage unavailable".into(),
            );
        }
        m.r32(head + 4) != 0
    };
    schedule_request(m, resource, needed, args)
}
/// Original537790 after the resource's actual virtual+10 call. Records retain
/// the second and third request arguments at+c/+10 (verified from x86 stack
/// offsets and oracle; decompiler stack recovery mislabels these fields).
pub fn schedule_request(
    m: &mut Space,
    resource: u32,
    needed: bool,
    args: [u32; 3],
) -> Result<(), String> {
    if !needed || m.r32(resource + 0x54) & 0x4000000 != 0 {
        return Ok(());
    }
    let manager = m.r32(0x618d48);
    if manager == 0 {
        return Err("NativeSpatialBoundary: request manager unavailable".into());
    }
    let node = m.r32(manager + 0xe0c);
    if node == 0 {
        return Err("NativeSpatialBoundary: request pool storage unavailable".into());
    }
    let next = m.r32(node + 4);
    if next == 0 {
        return Ok(());
    } //Original does nothing when its fixed pool is exhausted.
    let tail = m.r32(manager + 0xe00);
    if tail == 0 {
        return Err("NativeSpatialBoundary: pending request list storage unavailable".into());
    }
    let previous = m.r32(node + 8);
    if previous != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
    crate::native_part::bind_reference(m, node + 0x18, resource);
    m.w32(node + 0xc, args[1]);
    m.w32(node + 0x10, args[2]);
    m.w32(resource + 0x54, m.r32(resource + 0x54) | 0x4000000);
    m.w32(tail + 4, node);
    m.w32(node + 4, manager + 0xdf8);
    m.w32(node + 8, tail);
    m.w32(manager + 0xe00, node);
    Ok(())
}
#[cfg(test)]
mod request_tests {
    use super::*;
    #[test]
    fn original_request_pool_and_physics_scheduling_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spatial-request-original.json");
        let bytes = std::fs::read(path).expect("run native_spatial_request_probe.py");
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let manager = data["manager"].as_u64().unwrap() as u32;
        let mut m = Space::new();
        initialize_request_pool(&mut m, manager);
        let expected: std::collections::BTreeMap<u32, u32> = data["pool_words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                )
            })
            .collect();
        for p in (manager + 0xde8..manager + 0xf0e20).step_by(4) {
            assert_eq!(
                m.r32(p),
                expected.get(&p).copied().unwrap_or(0),
                "pool address{p:#x}"
            );
        }
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            m.w32(0x618d48, manager);
            m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            for resource in row["sequence"].as_array().unwrap() {
                request_physics(
                    &mut m,
                    resource.as_u64().unwrap() as u32,
                    [0x3dcccccd, 0, 0x3f800000],
                )
                .unwrap();
            }
            let expected: std::collections::BTreeMap<u32, u32> = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_u64().unwrap() as u32,
                        pair[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            for range in row["ranges"].as_array().unwrap() {
                for p in (range[0].as_u64().unwrap() as u32..range[1].as_u64().unwrap() as u32)
                    .step_by(4)
                {
                    assert_eq!(
                        m.r32(p),
                        expected.get(&p).copied().unwrap_or(0),
                        "{} address{p:#x}",
                        row["mode"]
                    );
                }
            }
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
}

/// Live loading, identity and collision-engine collaborators for spatial work.
pub trait IndexHost {
    fn access(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String>;
    fn relocated(&mut self, _m: &mut Space, _engine: u32, _resource: u32) -> Result<(), String> {
        Err("NativeSpatialBoundary: original4b3ee0 collision-root relocation".into())
    }
    fn reidentify(&mut self, _m: &mut Space, _resource: u32) -> Result<(), String> {
        Err("NativeSpatialBoundary: original536ed0/5447b0 live resource-ID reassignment".into())
    }
    fn submit(&mut self, _m: &mut Space, _engine: u32, _resource: u32) -> Result<(), String> {
        Err("NativeSpatialBoundary: original4b3c40 collision candidate submission".into())
    }
    fn collide(&mut self, _m: &mut Space, _engine: u32) -> Result<i32, String> {
        Err("NativeSpatialBoundary: original4b4250 collision detection".into())
    }
}
/// Original53efa0/53fcb0 embedded game/physics get-data methods.
fn embedded_payload(m: &Space, resource: u32) -> Result<u32, String> {
    match m.r32(resource) {
        0x5f375c | 0x5f37a4 => Ok(resource + 0xd8),
        v => Err(format!(
            "NativeSpatialBoundary: get-data virtual+c for vtable{v:#x}"
        )),
    }
}
/// Original48ae40 through its resource access collaborator.
fn collision_owner(m: &mut Space, resource: u32, host: &mut dyn IndexHost) -> Result<u32, String> {
    if resource == 0 {
        return Ok(0);
    }
    if m.r32(resource + 0x4c) != 1 {
        minimum_detail(m, resource, 0)?;
    }
    if host.access(m, resource, 0xa0000000)? & 0x80000000 != 0 {
        return Ok(0);
    }
    let payload = embedded_payload(m, resource)?;
    Ok(
        if m.r32(payload + 0x40) == 0x35 && m.r32(payload + 0x4c) != 0 {
            m.r32(payload + 0x4c) + 0xb4
        } else {
            0
        },
    )
}
/// Original544780 for native game/physics resources.
fn minimum_detail(m: &mut Space, resource: u32, value: u32) -> Result<(), String> {
    if !matches!(m.r32(resource), 0x5f375c | 0x5f37a4) {
        return Err("NativeSpatialBoundary: unported minimum-detail virtual+14".into());
    }
    if m.r32(resource + 0x7c) < m.r32(0x6200a4) || f(m.r32(resource + 0x6c)).fcom(f(value)).0.c0 {
        m.w32(resource + 0x6c, value);
    }
    Ok(())
}
fn contains_resource(m: &Space, parent: u32, resource: u32) -> u32 {
    if m.r32(parent + 0x80) == 2 || m.r32(resource + 0x80) == 2 {
        return 0xe;
    }
    let bounds = std::array::from_fn(|i| m.r32(parent + 0x80 + i as u32 * 4));
    let center = std::array::from_fn(|i| m.r32(resource + 0x84 + i as u32 * 4));
    match m.r32(resource + 0x80) {
        0 => classify_sphere(bounds, center, m.r32(resource + 0x90)),
        1 => classify_box(
            bounds,
            center,
            std::array::from_fn(|i| m.r32(resource + 0x90 + i as u32 * 4)),
        ),
        _ => 0,
    }
}
fn unlink(m: &mut Space, node: u32) {
    let next = m.r32(node + 4);
    let previous = m.r32(node + 8);
    if next != 0 && previous != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}
fn attach(m: &mut Space, parent: u32, resource: u32) -> Result<(), String> {
    let tail = m.r32(parent + 0x38);
    if tail == 0 {
        return Err("NativeSpatialBoundary: parent tree storage unavailable".into());
    }
    m.w32(resource + 0x14, 0);
    unlink(m, resource);
    m.w32(resource + 0x14, parent);
    m.w32(tail + 4, resource);
    m.w32(resource + 4, parent + 0x30);
    m.w32(resource + 8, tail);
    m.w32(parent + 0x38, resource);
    Ok(())
}
/// Original498b10 resource relocation, including ancestor search and descent.
pub fn relocate(m: &mut Space, resource: u32, host: &mut dyn IndexHost) -> Result<bool, String> {
    if m.r32(resource + 0x80) == 2 {
        return Ok(false);
    }
    let original_parent = m.r32(resource + 0x14);
    if original_parent == 0 {
        return Err("NativeSpatialBoundary: bounded resource has no spatial parent".into());
    }
    let mut parent = original_parent;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(parent) {
            return Err("NativeSpatialBoundary: cyclic spatial ancestors".into());
        }
        let above = m.r32(parent + 0x14);
        if above == 0
            || m.r32(parent + 0x54) & 0x30 != 0
            || contains_resource(m, parent, resource) & 8 != 0
        {
            break;
        }
        parent = above;
    }
    let mut changed = false;
    if m.r32(parent + 0x50) != m.r32(original_parent + 0x50) {
        attach(m, parent, resource)?;
        changed = true;
        let engine = collision_owner(m, resource, host)?;
        if engine != 0 {
            host.relocated(m, engine, resource)?;
        }
    }
    let mut child = m.r32(m.r32(resource + 0x14) + 0x20);
    if child == 0 {
        return Err("NativeSpatialBoundary: spatial child-list unavailable".into());
    }
    seen.clear();
    while child != 0 && m.r32(child + 4) != 0 {
        if !seen.insert(child) {
            return Err("NativeSpatialBoundary: cyclic spatial children".into());
        }
        if m.r32(child + 0x50) != m.r32(resource + 0x50)
            && m.r32(child + 0x54) & 0x20 == 0
            && m.r32(child + 0x64) & 1 != 0
            && m.r32(child + 0x4c) == 1
        {
            let payload = embedded_payload(m, child)?;
            if (payload == 0 || m.r32(payload + 0x30) == 0)
                && contains_resource(m, child, resource) & 8 != 0
            {
                attach(m, child, resource)?;
                changed = true;
                break;
            }
        }
        child = m.r32(child + 4);
    }
    if changed && m.r32(resource + 0x50) & 0xffff0000 != 0xffff0000 {
        host.reidentify(m, resource)?;
    }
    Ok(changed)
}
#[cfg(test)]
mod index_tests {
    use super::*;
    struct Loaded;
    impl IndexHost for Loaded {
        fn access(&mut self, m: &mut Space, r: u32, flags: u32) -> Result<u32, String> {
            crate::native_resource_load::prepare(
                m,
                r,
                crate::native_resource_load::Access {
                    flags,
                    quality: 0,
                    priority: 0,
                    time: 0,
                },
                &mut crate::native_assets::AccessAdapter,
            )
        }
    }
    #[test]
    fn original_spatial_resource_tree_relocation_matches() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spatial-index-original.json");
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(p).expect("run native_spatial_index_probe.py"))
                .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            let result = relocate(
                &mut m,
                row["resource"].as_u64().unwrap() as u32,
                &mut Loaded,
            )
            .unwrap();
            assert_eq!(u32::from(result), row["result"].as_u64().unwrap() as u32);
            let expected: std::collections::BTreeMap<u32, u32> = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_u64().unwrap() as u32,
                        pair[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            for p in (slr_physics::space::HEAP_BASE..m.heap_ptr()).step_by(4) {
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

/// Original498810 and4a3bc0's identical queue insertion, with raw references.
pub fn enqueue(m: &mut Space, resource: u32) -> Result<(), String> {
    if resource == 0 || m.r32(resource + 0x54) & 0x10000000 != 0 {
        return Ok(());
    }
    let tail = m.r32(0x643748);
    if tail == 0 {
        return Err("NativeSpatialBoundary: spatial queue storage unavailable".into());
    }
    m.w32(resource + 0x54, m.r32(resource + 0x54) | 0x10000000);
    let node = m.alloc(0x1c);
    m.w32(node, 0x5f09b4);
    for off in [4, 8, 12, 16, 20, 24] {
        m.w32(node + off, 0);
    }
    crate::native_part::bind_reference(m, node + 12, resource);
    m.w32(tail + 4, node);
    m.w32(node + 4, 0x643740);
    m.w32(node + 8, tail);
    m.w32(0x643748, node);
    Ok(())
}
fn destroy_queue_entry(m: &mut Space, node: u32) -> Result<(), String> {
    if m.r32(node) != 0x5f09b4 {
        return Err("NativeSpatialBoundary: unknown spatial queue destructor".into());
    }
    unlink(m, node);
    crate::native_part::bind_reference(m, node + 12, 0);
    m.w32(node, 0x5f09ac);
    //The shared native address-space allocator, like the oracle allocator,
    //does not reuse freed addresses. All destructor writes remain observable.
    Ok(())
}
fn first_child(m: &Space, resource: u32) -> Result<u32, String> {
    let node = m.r32(resource + 0x20);
    if node == 0 {
        return Err("NativeSpatialBoundary: resource child-list unavailable".into());
    }
    Ok(node)
}
fn refresh_tree(
    m: &mut Space,
    resource: u32,
    host: &mut dyn IndexHost,
    depth: usize,
) -> Result<(), String> {
    if depth > 1024 {
        return Err("NativeSpatialBoundary: cyclic/oversized spatial hierarchy".into());
    }
    let mut child = first_child(m, resource)?;
    let mut seen = std::collections::HashSet::new();
    while child != 0 && m.r32(child + 4) != 0 {
        if !seen.insert(child) {
            return Err("NativeSpatialBoundary: cyclic resource children".into());
        }
        if m.r32(child + 0x4c) == 1 {
            refresh_tree(m, child, host, depth + 1)?;
        }
        //498d70 reads the link after recursion: relocation can move this
        //child to a different parent and replace its next pointer.
        child = m.r32(child + 4);
    }
    aggregate_bounds(m, resource)?;
    relocate(m, resource, host)?;
    Ok(())
}
/// Original498dd0 refreshes queued trees and removes invalid/deleted references.
pub fn refresh_queue(m: &mut Space, host: &mut dyn IndexHost) -> Result<(), String> {
    let mut node = m.r32(0x643738);
    if node == 0 {
        return Err("NativeSpatialBoundary: spatial queue unavailable".into());
    }
    let mut seen = std::collections::HashSet::new();
    while node != 0 && m.r32(node + 4) != 0 {
        if !seen.insert(node) {
            return Err("NativeSpatialBoundary: cyclic spatial queue".into());
        }
        let flags = if m.r32(node + 20) == 0 {
            0x200000
        } else {
            let resource = m.r32(node + 24);
            if resource == 0 {
                0
            } else {
                refresh_tree(m, resource, host, 0)?;
                m.r32(resource + 0x54)
            }
        };
        let next = m.r32(node + 4);
        if flags & 0x200000 != 0 {
            destroy_queue_entry(m, node)?;
        }
        node = next;
    }
    Ok(())
}
/// Original4a2430 active physical child eligibility; unordered bounds follow
/// the original x87 condition flags instead of a host-language <= comparison.
fn collision_eligible(
    m: &mut Space,
    resource: u32,
    host: &mut dyn IndexHost,
) -> Result<bool, String> {
    if m.r32(resource + 0x54) & 0x200000 != 0 {
        return Ok(false);
    }
    let radius: [u32; 3] = std::array::from_fn(|i| m.r32(resource + 0x90 + i as u32 * 4));
    let norm = f(radius[1])
        .mul(f(radius[1]), PC, RC)
        .add(f(radius[2]).mul(f(radius[2]), PC, RC), PC, RC)
        .add(f(radius[0]).mul(f(radius[0]), PC, RC), PC, RC);
    let cc = norm.fcom(f(0x461c4000)).0;
    if !cc.c0 && !cc.c3 {
        return Ok(false);
    }
    let mut child = first_child(m, resource)?;
    let mut seen = std::collections::HashSet::new();
    while child != 0 && m.r32(child + 4) != 0 {
        if !seen.insert(child) {
            return Err("NativeSpatialBoundary: cyclic resource children".into());
        }
        if m.r32(child + 0x54) & 0x200000 == 0 && m.r32(child + 0x4c) == 2 {
            minimum_detail(m, child, 0x3f800000)?;
            if host.access(m, child, 0xa0000000)? & 0x80000000 == 0 {
                let payload = embedded_payload(m, child)?;
                let entity = m.r32(payload + 0x84);
                if entity != 0 && m.r32(entity + 0x58) & 0xa0 == 0 {
                    return Ok(true);
                }
            }
        }
        child = m.r32(child + 4);
    }
    Ok(false)
}
/// Original4a2540 consumes eligibility-tested queued resources and invokes each
/// participating collision engine once, in original reverse collected order.
pub fn collide_queue(m: &mut Space, host: &mut dyn IndexHost) -> Result<i32, String> {
    use slr_physics::cfg::PHYSICS_WORLD as WORLD;
    m.w32(WORLD + 0x30, m.r32(WORLD + 0x30).wrapping_add(1));
    let mut node = m.r32(0x643738);
    if node == 0 {
        return Err("NativeSpatialBoundary: spatial queue unavailable".into());
    }
    let mut engines = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while node != 0 && m.r32(node + 4) != 0 {
        if !seen.insert(node) {
            return Err("NativeSpatialBoundary: cyclic spatial queue".into());
        }
        let resource = m.r32(node + 24);
        if resource != 0 {
            m.w32(resource + 0x54, m.r32(resource + 0x54) & !0x10000000);
            if collision_eligible(m, resource, host)? {
                m.w32(WORLD + 0x50, m.r32(WORLD + 0x50).wrapping_add(1));
                let engine = collision_owner(m, m.r32(resource + 0xcc), host)?;
                if engine != 0 {
                    host.submit(m, engine, resource)?;
                    if !engines.contains(&engine) && engines.len() < 16 {
                        engines.push(engine);
                    }
                }
            }
        }
        let next = m.r32(node + 4);
        destroy_queue_entry(m, node)?;
        node = next;
    }
    let mut count = 0i32;
    for engine in engines.into_iter().rev() {
        count = count.wrapping_add(host.collide(m, engine)?);
    }
    m.w32(0x643530, m.r32(0x6200a4));
    Ok(count)
}
#[cfg(test)]
mod queue_tests {
    use super::*;
    struct Loaded;
    impl IndexHost for Loaded {
        fn access(&mut self, m: &mut Space, r: u32, flags: u32) -> Result<u32, String> {
            crate::native_resource_load::prepare(
                m,
                r,
                crate::native_resource_load::Access {
                    flags,
                    quality: 0,
                    priority: 0,
                    time: 0,
                },
                &mut crate::native_assets::AccessAdapter,
            )
        }
    }
    fn memory(words: &serde_json::Value, heap: u32) -> Space {
        let mut m = Space::new();
        m.set_heap_ptr(heap);
        for p in words.as_array().unwrap() {
            m.w32(p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32);
        }
        m
    }
    fn compare(m: &Space, expected: &serde_json::Value, mode: &str) {
        let expected: std::collections::BTreeMap<u32, u32> = expected
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                )
            })
            .collect();
        for (a, b) in [
            (slr_physics::space::HEAP_BASE, m.heap_ptr()),
            (0x643730, 0x643750),
            (0x6409c4, 0x640a24),
            (0x643530, 0x643534),
        ] {
            for p in (a..b).step_by(4) {
                assert_eq!(
                    m.r32(p),
                    expected.get(&p).copied().unwrap_or(0),
                    "{mode} address{p:#x}"
                );
            }
        }
    }
    #[test]
    fn original_spatial_queue_refresh_and_consumption_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-spatial-queue-original.json");
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run native_spatial_queue_probe.py"),
        )
        .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let heap = row["heap"].as_u64().unwrap() as u32;
            let mode = row["mode"].as_str().unwrap();
            let resource = row["resource"].as_u64().unwrap() as u32;
            let mut m = memory(
                &row["input_words"],
                row["heap_before"].as_u64().unwrap() as u32,
            );
            enqueue(&mut m, resource).unwrap();
            if mode == "duplicate" {
                enqueue(&mut m, resource).unwrap();
            }
            assert_eq!(m.heap_ptr(), heap);
            compare(&m, &row["queued_words"], mode);
            let mut m = memory(&row["refresh_input"], heap);
            refresh_queue(&mut m, &mut Loaded).unwrap();
            compare(&m, &row["refreshed_words"], mode);
            assert_eq!(
                collide_queue(&mut m, &mut Loaded).unwrap() as u32,
                row["result"].as_u64().unwrap() as u32
            );
            compare(&m, &row["output_words"], mode);
        }
    }
}
