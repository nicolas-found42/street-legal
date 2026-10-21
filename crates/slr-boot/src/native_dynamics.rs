//! Original4a5190 and its physics-step collaborators. Every unsupported active
//! graph stops at an explicit boundary; empty-list vectors are a separate gate.
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
fn add(a: u32, b: u32) -> u32 {
    stored(f(a).add(f(b), PC, RC))
}
fn sub(a: u32, b: u32) -> u32 {
    stored(f(a).sub(f(b), PC, RC))
}
/// Original498080 CRT spatial-update queue placement construction.
/// Process atexit registration does not alter the queue's runtime storage.
pub fn initialize_spatial_queue(m: &mut Space) {
    for (offset, value) in [
        (0, 0x5f1060),
        (4, 0x5f09ac),
        (8, 0x643740),
        (12, 0),
        (16, 0x5f09ac),
        (20, 0),
        (24, 0x643734),
    ] {
        m.w32(0x643730 + offset, value);
    }
}
pub trait Host: crate::native_spatial::IndexHost {
    /// Original426f60 dispatches class drive callbacks before integration.
    fn drive(&mut self, m: &mut Space, dt: u32) -> Result<(), String>;
    /// Original537790 request reached by swept resource bounds4986f0.
    fn spatial_request(
        &mut self,
        m: &mut Space,
        resource: u32,
        args: [u32; 3],
    ) -> Result<(), String> {
        crate::native_spatial::request_physics(m, resource, args)
    }
}
struct SpatialHost<'a>(&'a mut dyn Host);
impl crate::native_spatial::Host for SpatialHost<'_> {
    fn request(&mut self, m: &mut Space, resource: u32, args: [u32; 3]) -> Result<(), String> {
        self.0.spatial_request(m, resource, args)
    }
}
fn empty_list(m: &Space, head: u32) -> bool {
    head != 0 && m.r32(head + 4) == 0
}
/// Original4a5190 for independent/compound bodies and no-contact springs.
/// Nonempty contact/solver/index graphs stop at their unported collaborators.
pub fn step(m: &mut Space, dt: u32, host: &mut dyn Host) -> Result<bool, String> {
    let accumulated = add(dt, m.r32(WORLD + 4));
    m.w32(WORLD + 4, accumulated);
    if f32::from_bits(accumulated) < f32::from_bits(0x38d1b717) {
        return Ok(false);
    }
    let root = m.r32(WORLD + 0x84);
    if root == 0 {
        return Err("NativeDynamicsBoundary: world active-root storage is uninitialized".into());
    }
    let entities = active_entities(m, root)?;
    for entity in &entities {
        validate_independent_entity(m, *entity)?;
    }
    for offset in [0xb8, 0x118, 0x16c] {
        if !empty_list(m, m.r32(WORLD + offset)) {
            return Err(format!("NativeDynamicsBoundary: nonempty world list+{offset:x} requires original solver collaborators"));
        }
    }
    if m.r32(0x6439d8) != 0 {
        m.w32(0x6439d8, 0);
    }
    m.w32(WORLD + 4, sub(m.r32(WORLD + 4), accumulated));
    m.w32(WORLD + 0xc, accumulated);
    m.w32(0x6439c0, m.r32(0x6439c0).wrapping_add(1));
    for offset in [0x10, 0x14, 0x2c] {
        m.w32(WORLD + offset, 0);
    }
    for entity in &entities {
        prepare_entity(m, *entity)?;
    }
    spring_phase(m)?;
    host.drive(m, accumulated)?;
    if active_entities(m, root)? != entities {
        return Err("NativeDynamicsBoundary: drive callback changed active graph".into());
    }
    for entity in &entities {
        validate_independent_entity(m, *entity)?;
        integrate_entity(m, *entity, accumulated, host)?;
    }
    for offset in [0xb8, 0x118, 0x16c] {
        if !empty_list(m, m.r32(WORLD + offset)) {
            return Err(format!(
                "NativeDynamicsBoundary: drive/integration populated solver list+{offset:x}"
            ));
        }
    }
    for address in [0x643984, 0x643988] {
        m.w32(address, 0);
    }
    // Empty4a3900/4a2d10 leave the trial flag set. Actual spatial indexing
    // and collision candidates run before the final integration pass.
    m.w32(WORLD + 0x10, m.r32(WORLD + 0x10).wrapping_add(1));
    m.w32(WORLD + 0x1ec, 1);
    crate::native_spatial::refresh_queue(m, host)?;
    if crate::native_spatial::collide_queue(m, host)? != 0 {
        return Err(
            "NativeDynamicsBoundary: original4a35d0 collision constraint construction".into(),
        );
    }
    for entity in &entities {
        integrate_entity(m, *entity, accumulated, host)?;
    }
    for entity in &entities {
        commit_entity(m, *entity)?;
    }
    m.w32(WORLD, add(accumulated, m.r32(WORLD)));
    let count = X87::from_i32(m.r32(WORLD + 0x10) as i32);
    let average = f(m.r32(0x6439c4));
    let next = count
        .sub(average, PC, RC)
        .mul(f(0x3dcccccd), PC, RC)
        .add(average, PC, RC);
    m.w32(0x6439c4, stored(next));
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Driver {
        calls: Vec<u32>,
    }
    impl crate::native_spatial::IndexHost for Driver {
        fn access(&mut self, _m: &mut Space, _resource: u32, _flags: u32) -> Result<u32, String> {
            Err("fixture has no indexed resources".into())
        }
    }
    impl Host for Driver {
        fn drive(&mut self, _m: &mut Space, dt: u32) -> Result<(), String> {
            self.calls.push(dt);
            Ok(())
        }
    }
    #[test]
    fn original_empty_world_solver_and_index_passes_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-dynamics-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run native_dynamics_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["empty"].as_array().unwrap() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            let mut host = Driver::default();
            for (dt, expected) in row["dt"]
                .as_array()
                .unwrap()
                .iter()
                .zip(row["results"].as_array().unwrap())
            {
                let result = step(&mut m, dt.as_u64().unwrap() as u32, &mut host).unwrap();
                assert_eq!(
                    u32::from(result),
                    expected["result"].as_u64().unwrap() as u32
                );
                let words: Vec<_> = (WORLD..WORLD + 0x210)
                    .step_by(4)
                    .filter_map(|p| {
                        let v = m.r32(p);
                        (v != 0).then_some([p, v])
                    })
                    .collect();
                assert_eq!(serde_json::json!(words), expected["world"]);
                assert_eq!(serde_json::json!(host.calls), expected["callbacks"]);
            }
            let mut actual = std::collections::BTreeMap::new();
            for (start, len) in [
                (
                    slr_physics::space::HEAP_BASE,
                    m.heap_ptr() - slr_physics::space::HEAP_BASE,
                ),
                (WORLD, 0x210),
                (0x643730, 0x30),
                (0x643980, 0x60),
            ] {
                for p in (start..start + len).step_by(4) {
                    let v = m.r32(p);
                    if v != 0 {
                        actual.insert(p, v);
                    }
                }
            }
            let expected = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(actual, expected);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
}

fn mul(a: u32, b: u32) -> u32 {
    stored(f(a).mul(f(b), PC, RC))
}
fn vector(m: &Space, p: u32) -> [u32; 3] {
    std::array::from_fn(|i| m.r32(p + i as u32 * 4))
}
fn put_vector(m: &mut Space, p: u32, v: [u32; 3]) {
    for (i, v) in v.into_iter().enumerate() {
        m.w32(p + i as u32 * 4, v);
    }
}
fn quaternion(m: &Space, p: u32) -> [u32; 4] {
    std::array::from_fn(|i| m.r32(p + i as u32 * 4))
}
fn put_quaternion(m: &mut Space, p: u32, v: [u32; 4]) {
    for (i, v) in v.into_iter().enumerate() {
        m.w32(p + i as u32 * 4, v);
    }
}
const COMBINED_FIELDS: [u32; 18] = [
    0, 4, 8, 0xc, 0x10, 0x14, 0x28, 0x2c, 0x30, 0x70, 0x74, 0x78, 0x7c, 0x80, 0x84, 0x88, 0x8c,
    0x90,
];
//4367a0,54f730/54fb50/54f7e0 and4a9180, with original global output areas.
fn derivative(m: &mut Space, entity: u32, source: u32, dt: u32, dest: u32) {
    let inverse_mass = m.r32(m.r32(entity + 0x5c) + 0x14);
    let dt_mass = mul(dt, inverse_mass);
    put_vector(m, dest, vector(m, source + 0xc).map(|v| mul(dt, v)));
    put_vector(
        m,
        dest + 0xc,
        vector(m, source + 0x28).map(|v| mul(dt_mass, v)),
    );
    let w = vector(m, source + 0x70);
    let omega = [w[0], w[1], w[2], 0];
    put_quaternion(m, 0x644168, omega);
    let product = crate::native_entity::quaternion_product(omega, quaternion(m, source + 0x94));
    let half_dt = mul(dt, 0x3f000000);
    put_quaternion(m, dest + 0x94, product.map(|v| mul(half_dt, v)));
    put_vector(m, dest + 0x7c, vector(m, source + 0x88).map(|v| mul(dt, v)));
}
fn combine(m: &mut Space, a: u32, b: u32, scale: u32, dest: u32) {
    for offset in COMBINED_FIELDS {
        let value = stored(f(scale).mul(f(m.r32(b + offset)), PC, RC).add(
            f(m.r32(a + offset)),
            PC,
            RC,
        ));
        m.w32(dest + offset, value);
    }
    m.w8(0x7a0560, m.r8(0x7a0560) | 1);
    let scaled = quaternion(m, b + 0x94).map(|v| mul(scale, v));
    put_quaternion(m, 0x7a0570, scaled);
    let q = quaternion(m, a + 0x94);
    put_quaternion(
        m,
        dest + 0x94,
        std::array::from_fn(|i| add(q[i], scaled[i])),
    );
}
fn norm_squared(v: [u32; 3]) -> X87 {
    f(v[0])
        .mul(f(v[0]), PC, RC)
        .add(f(v[1]).mul(f(v[1]), PC, RC), PC, RC)
        .add(f(v[2]).mul(f(v[2]), PC, RC), PC, RC)
}
fn matrix_vector(matrix: [u32; 9], v: [u32; 3]) -> [u32; 3] {
    std::array::from_fn(|r| {
        let order = if r == 0 { [2, 1, 0] } else { [2, 0, 1] };
        stored(
            f(matrix[r * 3 + order[0]])
                .mul(f(v[order[0]]), PC, RC)
                .add(
                    f(matrix[r * 3 + order[1]]).mul(f(v[order[1]]), PC, RC),
                    PC,
                    RC,
                )
                .add(
                    f(matrix[r * 3 + order[2]]).mul(f(v[order[2]]), PC, RC),
                    PC,
                    RC,
                ),
        )
    })
}
/// Original4a7c10 RK4 and both kinematic Euler branches. Direct compound
/// integration remains a boundary;4a3bc0 projects children from the parent trial.
pub fn integrate(
    m: &mut Space,
    entity: u32,
    source: u32,
    dest: u32,
    dt: u32,
) -> Result<(), String> {
    let flags = m.r32(entity + 0x58);
    if flags & 4 != 0 {
        return Err(
            "NativeDynamicsBoundary: compound integrator4a7c10 requires parent projection".into(),
        );
    }
    let body = m.r32(entity + 0x5c);
    if flags & 0x100 != 0 && m.r32(body + 0x9c) & 0x200 != 0 {
        let factor = f(dt).mul(f(m.r32(body + 0x14)), PC, RC);
        for axis in 0..3 {
            let offset = axis * 4;
            m.w32(
                dest + offset,
                stored(f(dt).mul(f(m.r32(source + 0xc + offset)), PC, RC).add(
                    f(m.r32(source + offset)),
                    PC,
                    RC,
                )),
            );
            m.w32(
                dest + 0xc + offset,
                stored(factor.mul(f(m.r32(source + 0x28 + offset)), PC, RC).add(
                    f(m.r32(source + 0xc + offset)),
                    PC,
                    RC,
                )),
            );
        }
        m.w32(dest + 0x24, add(dt, m.r32(source + 0x24)));
        return Ok(());
    }
    if flags & 0x100 != 0 {
        derivative(m, entity, source, 0x3f800000, 0x643ab8);
        combine(m, source, 0x643ab8, dt, dest);
    } else {
        derivative(m, entity, source, dt, 0x643e18);
        combine(m, source, 0x643e18, 0x3f000000, 0x643d40);
        derivative(m, entity, 0x643d40, dt, 0x643eec);
        combine(m, source, 0x643eec, 0x3f000000, 0x643d40);
        derivative(m, entity, 0x643d40, dt, 0x643fc0);
        //4a8f00 adds the third derivative to the source without a scalar multiply.
        for offset in COMBINED_FIELDS {
            m.w32(
                0x643d40 + offset,
                add(m.r32(source + offset), m.r32(0x643fc0 + offset)),
            );
        }
        let qa = quaternion(m, source + 0x94);
        let qb = quaternion(m, 0x643fc0 + 0x94);
        put_quaternion(
            m,
            0x643d40 + 0x94,
            std::array::from_fn(|i| add(qa[i], qb[i])),
        );
        derivative(m, entity, 0x643d40, dt, 0x644094);
        combine(m, source, 0x643e18, 0x3e2aaaab, dest);
        combine(m, dest, 0x643eec, 0x3eaaaaab, dest);
        combine(m, dest, 0x643fc0, 0x3eaaaaab, dest);
        combine(m, dest, 0x644094, 0x3e2aaaab, dest);
    }
    for offset in [0, 4, 8] {
        m.w32(
            dest + offset,
            add(m.r32(source + 0x18 + offset), m.r32(dest + offset)),
        );
    }
    let q = quaternion(m, dest + 0x94);
    let length = f(q[3])
        .mul(f(q[3]), PC, RC)
        .add(f(q[2]).mul(f(q[2]), PC, RC), PC, RC)
        .add(f(q[1]).mul(f(q[1]), PC, RC), PC, RC)
        .add(f(q[0]).mul(f(q[0]), PC, RC), PC, RC)
        .sqrt(PC, RC);
    let normalized = if length.fcom(f(0)).0.c3 {
        [0, 0, 0, 0x3f800000]
    } else {
        let reciprocal = f(0x3f800000).div(length, PC, RC);
        q.map(|v| stored(reciprocal.mul(f(v), PC, RC)))
    };
    put_quaternion(m, dest + 0x94, normalized);
    crate::native_entity::write_matrix(
        m,
        dest + 0x40,
        crate::native_entity::quaternion_matrix(normalized),
    );
    let body = m.r32(entity + 0x5c);
    let damping = m.r32(body + 0x84);
    if f32::from_bits(damping) < 0. {
        let factor = f(m.r32(body + 0x14))
            .mul(f(damping), PC, RC)
            .mul(f(dt), PC, RC)
            .mul(f(0x42c80000), PC, RC)
            .add(f(0x3f800000), PC, RC);
        put_vector(
            m,
            dest + 0x7c,
            vector(m, dest + 0x7c).map(|v| stored(factor.mul(f(v), PC, RC))),
        );
    }
    m.w32(dest + 0x24, add(dt, m.r32(source + 0x24)));
    let rotation = crate::native_entity::read_matrix(m, dest + 0x40);
    let inertia = crate::native_entity::read_matrix(m, body + 0x18);
    let world_inverse = crate::native_entity::matrix_product(
        rotation,
        crate::native_entity::matrix_product(inertia, crate::native_entity::transpose(rotation)),
    );
    crate::native_entity::write_matrix(m, dest + 0xa4, world_inverse);
    let mut angular = matrix_vector(world_inverse, vector(m, dest + 0x7c));
    let squared = norm_squared(angular);
    let limited = squared.mul(f(0x3b23d70a), PC, RC);
    if limited.fcom(f(0x3f800000)).0
        == (slr_x87::ConditionCodes {
            c0: false,
            c2: false,
            c3: false,
        })
    {
        let scale = f(0x3f800000).div(limited, PC, RC);
        put_vector(
            m,
            dest + 0x7c,
            vector(m, dest + 0x7c).map(|v| stored(scale.mul(f(v), PC, RC))),
        );
        angular = matrix_vector(world_inverse, vector(m, dest + 0x7c));
    }
    put_vector(m, dest + 0x70, angular);
    let velocity = vector(m, dest + 0xc);
    let speed = norm_squared(velocity);
    if speed.fcom(f(0x46afc800)).0
        == (slr_x87::ConditionCodes {
            c0: false,
            c2: false,
            c3: false,
        })
    {
        let scale = f(0x43160000).div(speed.sqrt(PC, RC), PC, RC);
        put_vector(
            m,
            dest + 0xc,
            velocity.map(|v| stored(scale.mul(f(v), PC, RC))),
        );
    }
    Ok(())
}
#[cfg(test)]
mod integrator_tests {
    use super::*;
    #[test]
    fn original_rk4_full_state_and_scratch_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-dynamics-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run native_dynamics_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (index, row) in data["integrators"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
            integrate(
                &mut m,
                row["entity"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                row["destination"].as_u64().unwrap() as u32,
                row["dt"].as_u64().unwrap() as u32,
            )
            .unwrap();
            let expected = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x[0].as_u64().unwrap() as u32, x[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for range in row["ranges"].as_array().unwrap() {
                let start = range[0].as_u64().unwrap() as u32;
                let len = range[1].as_u64().unwrap() as u32;
                for p in (start..start + len).step_by(4) {
                    assert_eq!(
                        m.r32(p),
                        expected.get(&p).copied().unwrap_or(0),
                        "profile{index} address{p:#x}"
                    );
                }
            }
        }
    }
}

fn active_entities(m: &Space, root: u32) -> Result<Vec<u32>, String> {
    let mut out = vec![];
    let mut e = m.r32(root + 0x24);
    while e != 0 {
        if out.contains(&e) {
            return Err("NativeDynamicsBoundary: cyclic active entity list".into());
        }
        out.push(e);
        e = m.r32(e + 0x28);
    }
    Ok(out)
}
fn validate_independent_entity(m: &Space, e: u32) -> Result<(), String> {
    let mut visited = std::collections::HashSet::new();
    validate_entity_graph(m, e, &mut visited)
}
fn validate_entity_graph(
    m: &Space,
    e: u32,
    visited: &mut std::collections::HashSet<u32>,
) -> Result<(), String> {
    if !visited.insert(e) || visited.len() > 4096 {
        return Err("NativeDynamicsBoundary: cyclic/oversized entity graph".into());
    }
    if m.r32(e + 0x20c) > 1 {
        return Err("NativeDynamicsBoundary: invalid integration buffer index".into());
    }
    if m.r32(e + 0x58) & 0x10 != 0 {
        return Err(
            "NativeDynamicsBoundary: special/kinematic graph requires source projection".into(),
        );
    }
    if m.r32(e + 0x58) & 4 != 0 && (m.r32(e + 0x1c) == 0 || m.r32(e + 0x20) == 0) {
        return Err("NativeDynamicsBoundary: relative entity lacks parent/frame".into());
    }
    for child in active_entities(m, e)? {
        validate_entity_graph(m, child, visited)?;
    }
    if m.r32(WORLD + 0x80) != 0 && m.r32(m.r32(e + 0x5c) + 0x9c) & 0x800 != 0 {
        return Err(
            "NativeDynamicsBoundary: environmental contact forces require original4a3f90".into(),
        );
    }
    Ok(())
}
fn add_force(m: &mut Space, state: u32, force: [u32; 3]) {
    for (axis, force) in force.into_iter().enumerate() {
        let p = state + 0x28 + axis as u32 * 4;
        let v = if axis == 1 {
            add(force, m.r32(p))
        } else {
            add(m.r32(p), force)
        };
        m.w32(p, v);
    }
}
/// Original4a3f90 root damping/gravity/buoyancy force branches.
fn prepare_entity(m: &mut Space, e: u32) -> Result<(), String> {
    let flags = m.r32(e + 0x58);
    let body = m.r32(e + 0x5c);
    if flags & 1 == 0
        || flags & 0x80 != 0
        || m.r32(body + 0x9c) & 4 != 0
        || (flags & 4 == 0 && f32::from_bits(m.r32(body + 0x14)) == 0.)
    {
        return Ok(());
    }
    let index = m.r32(e + 0x20c);
    let source = e + 0x64 + index * 0xd4;
    let dest = e + 0x64 + (index ^ 1) * 0xd4;
    m.w32(dest + 0x3c, m.r32(dest + 0x3c) & !1);
    if flags & 4 == 0 {
        let damping = m.r32(body + 0x80);
        if f32::from_bits(damping) < 0. {
            add_force(m, source, vector(m, source + 0xc).map(|v| mul(damping, v)));
        }
        if m.r32(WORLD + 0x78) != 0 && flags & 2 == 0 {
            let mass = f(0x3f800000).div(f(m.r32(body + 0x14)), PC, RC);
            add_force(
                m,
                source,
                vector(m, WORLD + 0x104).map(|v| stored(mass.mul(f(v), PC, RC))),
            );
        }
        if m.r32(body + 0x9c) & 0x80 != 0 {
            let amount = f(m.r32(body + 0x88)).mul(f(m.r32(WORLD + 0x1b4)), PC, RC);
            let negative = amount.mul(f(0xbf800000), PC, RC);
            add_force(
                m,
                source,
                vector(m, WORLD + 0x104).map(|v| stored(negative.mul(f(v), PC, RC))),
            );
        }
    }
    for child in active_entities(m, e)? {
        prepare_entity(m, child)?;
    }
    Ok(())
}
/// Original4a3bc0 independent dynamic root and state-validity stores.
fn integrate_entity(m: &mut Space, e: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    let flags = m.r32(e + 0x58);
    if flags & 1 == 0 || flags & 0x80 != 0 {
        return Ok(());
    }
    let body = m.r32(e + 0x5c);
    let index = m.r32(e + 0x20c);
    let dest = e + 0x64 + (index ^ 1) * 0xd4;
    if flags & 4 != 0 {
        let parent = m.r32(e + 0x20);
        let relative = m.r32(e + 0x1c);
        let parent_state = parent + 0x64 + (m.r32(parent + 0x20c) ^ 1) * 0xd4;
        let rotation = crate::native_entity::read_matrix(m, parent_state + 0x40);
        let local = vector(m, relative + 4);
        let transformed = matrix_vector(rotation, local);
        put_vector(
            m,
            dest,
            std::array::from_fn(|i| add(m.r32(parent_state + i as u32 * 4), transformed[i])),
        );
        let q = crate::native_entity::quaternion_product(
            quaternion(m, parent_state + 0x94),
            quaternion(m, relative + 0x10),
        );
        put_quaternion(m, dest + 0x94, q);
        crate::native_entity::write_matrix(
            m,
            dest + 0x40,
            crate::native_entity::quaternion_matrix(q),
        );
        m.w32(dest + 0x24, m.r32(parent_state + 0x24));
        put_vector(m, dest + 0xc, point_velocity(m, parent, local)?);
        put_vector(m, dest + 0x70, vector(m, parent_state + 0x70));
    } else {
        if m.r32(body + 0x9c) & 4 != 0
            || m.r32(dest + 0x3c) & 1 != 0
            || f32::from_bits(m.r32(body + 0x14)) == 0.
        {
            return Ok(());
        }
        integrate(m, e, e + 0x64 + index * 0xd4, dest, dt)?;
        m.w32(WORLD + 0x2c, m.r32(WORLD + 0x2c).wrapping_add(1));
    }
    m.w32(dest + 0x3c, (m.r32(dest + 0x3c) & !2) | 1);
    for child in active_entities(m, e)? {
        integrate_entity(m, child, dt, host)?;
    }
    if m.r32(e + 0x50) != 0 {
        crate::native_spatial::swept_bounds(m, e, &mut SpatialHost(host))?;
    }
    let mut ancestor = e;
    while m.r32(ancestor + 0x58) & 4 != 0 {
        ancestor = m.r32(ancestor + 0x20);
    }
    crate::native_spatial::enqueue(m, m.r32(ancestor + 0x44))?;
    Ok(())
}
fn less(a: X87, b: X87) -> bool {
    let cc = a.fcom(b).0;
    cc.c0 && !cc.c2
}
/// Original4a4ed0 root state commit, averaging/sleep test and force clearing.
fn commit_entity(m: &mut Space, e: u32) -> Result<(), String> {
    let flags = m.r32(e + 0x58);
    if flags & 1 == 0 {
        return Ok(());
    }
    let body = m.r32(e + 0x5c);
    let index = m.r32(e + 0x20c);
    let source = e + 0x64 + index * 0xd4;
    let dest = e + 0x64 + (index ^ 1) * 0xd4;
    if flags & 0x84 == 0 && (m.r32(dest + 0x3c) & 1 == 0 || m.r32(body + 0x9c) & 4 != 0) {
        return Ok(());
    }
    if flags & 0x84 == 0 {
        for axis in 0..3 {
            let p = e + 0xc + axis * 4;
            let initial = mul(m.r32(p), 0x3f666666);
            let next = stored(
                f(m.r32(source + 0x28 + axis * 4))
                    .mul(f(0x3dcccccd), PC, RC)
                    .add(f(initial), PC, RC),
            );
            m.w32(p, next);
        }
        let inverse_mass = f(m.r32(body + 0x14));
        let inverse_squared = inverse_mass.mul(inverse_mass, PC, RC);
        let can_sleep = less(norm_squared(vector(m, dest + 0xc)), f(0x3dcccccd))
            && less(norm_squared(vector(m, dest + 0x70)), f(0x3a83126f))
            && less(
                inverse_squared.mul(norm_squared(vector(m, source + 0x88)), PC, RC),
                f(0x3c23d70a),
            )
            && less(
                inverse_squared.mul(norm_squared(vector(m, e + 0xc)), PC, RC),
                f(0x3c23d70a),
            );
        if can_sleep {
            crate::native_world::sleep(m, e)?;
            return Ok(());
        }
    }
    m.w32(e + 0x20c, index ^ 1);
    if flags & 4 == 0 {
        for off in [0x28, 0x2c, 0x30, 0x18, 0x1c, 0x20, 0x88, 0x8c, 0x90] {
            m.w32(dest + off, 0);
        }
        if flags & 0x100 == 0 {
            let rotation = crate::native_entity::read_matrix(m, dest + 0x40);
            let inertia = crate::native_entity::read_matrix(m, body + 0x18);
            let result = crate::native_entity::matrix_product(
                rotation,
                crate::native_entity::matrix_product(
                    inertia,
                    crate::native_entity::transpose(rotation),
                ),
            );
            crate::native_entity::write_matrix(m, dest + 0xa4, result);
        }
    }
    for child in active_entities(m, e)? {
        commit_entity(m, child)?;
    }
    Ok(())
}
#[cfg(test)]
mod active_tests {
    use super::*;
    #[derive(Default)]
    struct Driver {
        calls: Vec<u32>,
        requests: Vec<Vec<u32>>,
        real_requests: bool,
    }
    impl crate::native_spatial::IndexHost for Driver {
        fn access(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
            crate::native_resource_load::prepare(
                m,
                resource,
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
    impl Host for Driver {
        fn drive(&mut self, _m: &mut Space, dt: u32) -> Result<(), String> {
            self.calls.push(dt);
            Ok(())
        }
        fn spatial_request(
            &mut self,
            m: &mut Space,
            resource: u32,
            args: [u32; 3],
        ) -> Result<(), String> {
            if self.real_requests {
                return crate::native_spatial::request_physics(m, resource, args);
            }
            self.requests
                .push(std::iter::once(resource).chain(args).collect());
            Ok(())
        }
    }
    #[test]
    fn original_active_free_body_world_steps_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-dynamics-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run native_dynamics_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (index, row) in data["active"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            initialize_spatial_queue(&mut m);
            let mut host = Driver {
                real_requests: row["real_requests"].as_bool().unwrap(),
                ..Default::default()
            };
            let result = step(&mut m, 0x3ca3d70a, &mut host).unwrap();
            assert_eq!(u32::from(result), row["result"].as_u64().unwrap() as u32);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            assert_eq!(serde_json::json!(host.calls), row["callbacks"]);
            assert_eq!(serde_json::json!(host.requests), row["requests"]);
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
                        "profile{index} address{p:#x}"
                    );
                }
            }
        }
    }
}
fn project_position(matrix: [u32; 9], v: [u32; 3]) -> [u32; 3] {
    std::array::from_fn(|r| {
        let order = if r == 0 { [0, 1, 2] } else { [1, 0, 2] };
        stored(
            f(matrix[r * 3 + order[0]])
                .mul(f(v[order[0]]), PC, RC)
                .add(
                    f(matrix[r * 3 + order[1]]).mul(f(v[order[1]]), PC, RC),
                    PC,
                    RC,
                )
                .add(
                    f(matrix[r * 3 + order[2]]).mul(f(v[order[2]]), PC, RC),
                    PC,
                    RC,
                ),
        )
    })
}
/// Original4a73c0 world velocity at a local point, using the trial buffer.
fn point_velocity(m: &Space, e: u32, point: [u32; 3]) -> Result<[u32; 3], String> {
    if m.r32(e + 0x58) & 4 != 0 {
        let relative = m.r32(e + 0x1c);
        let rotation = crate::native_entity::read_matrix(m, relative + 0x20);
        let transformed = project_position(rotation, point);
        let local =
            std::array::from_fn(|i| add(transformed[i], m.r32(relative + 4 + i as u32 * 4)));
        return point_velocity(m, m.r32(e + 0x20), local);
    }
    let state = e + 0x64 + (m.r32(e + 0x20c) ^ 1) * 0xd4;
    let rotation = crate::native_entity::read_matrix(m, state + 0x40);
    let p = project_position(rotation, point);
    let w = vector(m, state + 0x70);
    let velocity = vector(m, state + 0xc);
    Ok(std::array::from_fn(|i| {
        let a = (i + 1) % 3;
        let b = (i + 2) % 3;
        stored(
            f(p[b])
                .mul(f(w[a]), PC, RC)
                .sub(f(p[a]).mul(f(w[b]), PC, RC), PC, RC)
                .add(f(velocity[i]), PC, RC),
        )
    }))
}
fn spring_phase(m: &mut Space) -> Result<(), String> {
    let mut current = m.r32(WORLD + 0x9c);
    if current == 0 {
        return Err("NativeDynamicsBoundary: spring list storage is uninitialized".into());
    }
    let mut visited = std::collections::HashSet::new();
    while current != 0 && m.r32(current + 4) != 0 {
        if !visited.insert(current) {
            return Err("NativeDynamicsBoundary: cyclic spring list".into());
        }
        let next = m.r32(current + 4);
        let next = if next != 0 && m.r32(next + 4) != 0 {
            next
        } else {
            0
        };
        if m.r32(current) != 0x5f0fcc {
            return Err(format!(
                "NativeDynamicsBoundary: spring-phase virtual+0c for vtable{:#x}",
                m.r32(current)
            ));
        }
        let result = crate::native_spring::pre_step(m, current)?;
        if result < 0 {
            return Err(
                "NativeDynamicsBoundary: spring destruction requires original virtual destructor"
                    .into(),
            );
        }
        current = next;
    }
    Ok(())
}
