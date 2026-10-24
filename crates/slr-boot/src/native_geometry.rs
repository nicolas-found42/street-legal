//! Original49ab60 primitive body/definition graph and4a6010 inertia.
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
fn constant(v: f32) -> X87 {
    X87::from_f32(v)
}
fn matrix_zero(m: &mut Space, p: u32) {
    for i in [0, 1, 2, 4, 5, 6, 8, 9, 10] {
        m.w32(p + i * 4, 0);
    }
}
/// The intrusive body list initialized by the world constructor. Space is a
/// fresh configured world; subsequent allocations retain its counters/links.
fn prepare_world(m: &mut Space) {
    if m.r32(WORLD + 0x198) == 0 {
        m.w32(WORLD + 0x188, WORLD + 0x190);
        m.w32(WORLD + 0x198, WORLD + 0x184);
    }
}
fn allocate_body(m: &mut Space, mass: u32) -> u32 {
    prepare_world(m);
    let p = m.alloc(0xa0);
    m.w32(p, 0x5f1444);
    let previous = m.r32(WORLD + 0x198);
    m.w32(previous + 4, p);
    m.w32(p + 8, previous);
    m.w32(p + 4, WORLD + 0x190);
    m.w32(WORLD + 0x198, p);
    for offset in [0x70, 0x74] {
        m.w32(WORLD + offset, m.r32(WORLD + offset).wrapping_add(1));
    }
    let flags = if f32::from_bits(mass) == 0.0 || f32::from_bits(mass).is_nan() {
        5
    } else {
        m.w32(p + 0x14, stored(constant(1.).div(f(mass), PC, RC)));
        m.w32(p + 0x80, stored(f(mass).mul(f(0xbba3d70a), PC, RC)));
        m.w32(p + 0x84, stored(f(mass).mul(f(0xbc23d70a), PC, RC)));
        1
    };
    m.w32(p + 0x9c, flags);
    p
}
/// Original54e510 for the diagonal inertia matrices formed by primitives.
/// Cofactors and determinant retain the instruction-order rounding, rather
/// than replacing the calculation with three unrelated reciprocals.
fn inverse_diagonal(m: &mut Space, body: u32, d: [X87; 3]) {
    use crate::native_stack as stack;
    let zero = constant(0.);
    let det = d[2]
        .mul(d[0], PC, RC)
        .mul(d[1], PC, RC)
        .add(zero, PC, RC)
        .add(zero, PC, RC)
        .sub(zero, PC, RC)
        .sub(zero, PC, RC)
        .sub(zero, PC, RC);
    let inv = constant(1.).div(det, PC, RC);
    let cof = [
        d[1].mul(d[2], PC, RC),
        d[0].mul(d[2], PC, RC),
        d[0].mul(d[1], PC, RC),
    ];
    let cofactors = std::array::from_fn(|i| {
        if i / 3 == i % 3 {
            stored(cof[i / 3])
        } else {
            0
        }
    });
    stack::inverse_copy(m, body + 0x18, body + 0x48, stored(det), cofactors);
    for r in 0..3 {
        for c in 0..3 {
            m.w32(
                body + 0x48 + r * 16 + c * 4,
                stored(inv.mul(
                    if r == c {
                        f(stored(cof[r as usize]))
                    } else {
                        zero
                    },
                    PC,
                    RC,
                )),
            );
        }
    }
}
fn inertia(m: &mut Space, body: u32, definition: u32, kind: u32, dims: [u32; 3]) {
    crate::native_stack::function(m, &[], Some(body), |m| {
        inertia_inner(m, body, definition, kind, dims)
    });
}
fn inertia_inner(m: &mut Space, body: u32, definition: u32, kind: u32, dims: [u32; 3]) {
    use crate::native_stack::{self as stack, Reg};
    stack::push_reg(m, Reg::Ecx);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, body);

    if f32::from_bits(m.r32(body + 0x14)) == 0.0 {
        matrix_zero(m, body + 0x18);
        matrix_zero(m, body + 0x48);
        return;
    }
    stack::push_reg(m, Reg::Edi);
    stack::store(
        m,
        8,
        stored(constant(1.).div(f(m.r32(body + 0x14)), PC, RC)),
    );
    stack::set_reg(m, Reg::Edi, body + 0x18);
    matrix_zero(m, body + 0x18);
    let mass = constant(1.).div(f(m.r32(body + 0x14)), PC, RC);
    let [x, y, z] = dims.map(f);
    let sq = |x: X87| x.mul(x, PC, RC);
    let diag = match kind {
        0 | 1 => {
            let v = constant(2.5).div(sq(x).mul(mass, PC, RC), PC, RC);
            [v; 3]
        }
        2..=4 => {
            let axial = constant(2.).div(sq(y).mul(mass, PC, RC), PC, RC);
            let length = x.add(x, PC, RC);
            let radial = constant(1.).div(
                sq(y)
                    .mul(constant(0.25), PC, RC)
                    .add(sq(length).mul(f(0x3daaaaab), PC, RC), PC, RC)
                    .mul(mass, PC, RC),
                PC,
                RC,
            );
            match kind {
                2 => [axial, radial, radial],
                3 => [radial, axial, radial],
                _ => [radial, radial, axial],
            }
        }
        5 => [
            constant(3.).div(sq(y).add(sq(z), PC, RC).mul(mass, PC, RC), PC, RC),
            constant(3.).div(sq(x).add(sq(z), PC, RC).mul(mass, PC, RC), PC, RC),
            constant(3.).div(sq(x).add(sq(y), PC, RC).mul(mass, PC, RC), PC, RC),
        ],
        _ => [constant(2.5).div(mass, PC, RC); 3],
    };
    for (i, value) in diag.into_iter().enumerate() {
        m.w32(body + 0x18 + i as u32 * 0x14, stored(value));
    }
    let volume = match kind {
        0 | 1 => x.mul(x, PC, RC).mul(x, PC, RC).mul(f(0x40860a92), PC, RC),
        5 => z.mul(y, PC, RC).mul(x, PC, RC).mul(constant(8.), PC, RC),
        6 | 7 => f(0x40860a92),
        _ => constant(0.),
    };
    m.w32(definition + 0x4c, stored(volume));
    // 54e510 reloads the stored diagonal values, not the calculation registers.
    let diagonal = std::array::from_fn(|i| f(m.r32(body + 0x18 + i as u32 * 0x14)));
    let return_pc = match kind {
        0 | 1 => 0x4a60e7,
        3 => 0x4a61bb,
        5 => 0x4a62ed,
        _ => 0x4a6233,
    };
    stack::call(m, return_pc, &[body + 0x18], Some(body + 0x48), |m| {
        inverse_diagonal(m, body, diagonal)
    });
}
/// Original4a6350 uses the supplied radius directly in its inertia divisor.
fn sphere_inertia(m: &mut Space, body: u32, definition: u32, radius: u32) {
    crate::native_stack::function(m, &[1, radius], Some(body), |m| {
        sphere_inertia_inner(m, body, definition, radius)
    });
}
fn sphere_inertia_inner(m: &mut Space, body: u32, definition: u32, radius: u32) {
    use crate::native_stack::{self as stack, Reg};
    stack::push_reg(m, Reg::Ecx);
    stack::push_reg(m, Reg::Esi);
    stack::set_reg(m, Reg::Esi, body);

    if f32::from_bits(m.r32(body + 0x14)) == 0.0 {
        matrix_zero(m, body + 0x18);
        matrix_zero(m, body + 0x48);
        return;
    }
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Edi, body + 0x18);
    let mass = constant(1.).div(f(m.r32(body + 0x14)), PC, RC);
    stack::store(m, 8, stored(mass));
    let value = constant(2.5).div(mass.mul(f(radius), PC, RC), PC, RC);
    matrix_zero(m, body + 0x18);
    for offset in [0x18, 0x2c, 0x40] {
        m.w32(body + offset, stored(value));
    }
    for offset in [0x40, 0x44, 0x48] {
        m.w32(definition + offset, 0);
    }
    let r = f(radius);
    m.w32(
        definition + 0x4c,
        stored(r.mul(r, PC, RC).mul(r, PC, RC).mul(f(0x40860a92), PC, RC)),
    );
    stack::call(m, 0x4a63d8, &[body + 0x18], Some(body + 0x48), |m| {
        inverse_diagonal(m, body, [f(stored(value)); 3])
    });
}
/// Execute original49ab60 using the supplied collision mesh bytes.
pub fn body(
    m: &mut Space,
    mass: u32,
    kind: u32,
    dimensions: [u32; 3],
    shape: Option<&[u8]>,
) -> Result<u32, String> {
    let path = if shape.is_some() { 0x30001000 } else { 0 };
    crate::native_stack::function(
        m,
        &[
            mass,
            kind,
            dimensions[0],
            dimensions[1],
            dimensions[2],
            path,
        ],
        Some(WORLD),
        |m| body_inner(m, mass, kind, dimensions, shape),
    )
}
fn body_inner(
    m: &mut Space,
    mass: u32,
    kind: u32,
    dimensions: [u32; 3],
    shape: Option<&[u8]>,
) -> Result<u32, String> {
    use crate::native_stack::{self as stack, Reg};
    stack::push_reg(m, Reg::Ebx);
    stack::push_reg(m, Reg::Esi);
    stack::push_reg(m, Reg::Edi);
    stack::set_reg(m, Reg::Esi, WORLD);
    if kind > 7 {
        return Err(format!(
            "NativeGeometryBoundary: unsupported body kind{kind}"
        ));
    }
    let body = stack::external(m, 0x49ab6f, &[0xa0], None, |m| allocate_body(m, mass));
    stack::set_reg(m, Reg::Edi, body);
    stack::set_reg(m, Reg::Ebx, 0);
    stack::external(m, 0x49ab81, &[], Some(body), |_| ());
    let definition = stack::external(m, 0x49ac3f, &[0xac], None, |m| m.alloc(0xac));
    stack::set_reg(m, Reg::Esi, definition);
    for (return_pc, offset) in [(0x49ac67, 0x5c), (0x49ac6f, 0x6c), (0x49ac77, 0x7c)] {
        stack::external(m, return_pc, &[], Some(definition + offset), |_| ());
    }
    stack::push_reg(m, Reg::Ebp);
    stack::add_sp(m, 4);
    m.w32(body + 0x78, definition);
    m.w32(definition, body + 0x78);
    m.w32(definition + 4, 1);
    m.w32(definition + 8, kind);
    m.w32(definition + 0x50, 0x3f19999a);
    m.w32(definition + 0x58, 0x3e4ccccd);
    m.w32(body + 0x9c, m.r32(body + 0x9c) | 0x400);
    let [x, y, z] = dimensions.map(f);
    let margin = f(0x3d4ccccd);
    match kind {
        0 => {
            matrix_zero(m, body + 0x18);
            matrix_zero(m, body + 0x48);
        }
        1..=4 => {
            m.w32(definition + 0x9c, dimensions[0]);
            m.w32(definition + 0x10, dimensions[if kind == 1 { 0 } else { 1 }]);
            let radius = if kind == 1 {
                x.add(margin, PC, RC)
            } else {
                x.add(y, PC, RC).add(margin, PC, RC)
            };
            m.w32(definition + 0xc, stored(radius));
            let return_pc = match kind {
                1 => 0x49ae00,
                2 => 0x49ae4f,
                3 => 0x49aea4,
                _ => 0x49aef9,
            };
            stack::call(m, return_pc, &[], Some(body), |m| {
                inertia(m, body, definition, kind, dimensions)
            });
            let minor = if kind == 1 {
                radius
            } else {
                y.add(margin, PC, RC)
            };
            for axis in 0..3 {
                m.w32(
                    definition + 0x14 + axis * 4,
                    stored(if kind == 1 || axis == kind - 2 {
                        radius
                    } else {
                        minor
                    }),
                );
            }
        }
        5 => {
            for (i, value) in dimensions.into_iter().enumerate() {
                m.w32(definition + 0x9c + i as u32 * 4, value);
            }
            let radius = z
                .mul(z, PC, RC)
                .add(y.mul(y, PC, RC), PC, RC)
                .add(x.mul(x, PC, RC), PC, RC)
                .sqrt(PC, RC)
                .add(margin, PC, RC);
            m.w32(definition + 0xc, stored(radius));
            stack::call(m, 0x49ada1, &[], Some(body), |m| {
                inertia(m, body, definition, kind, dimensions)
            });
            for (i, value) in [x, y, z].into_iter().enumerate() {
                m.w32(
                    definition + 0x14 + i as u32 * 4,
                    stored(value.add(margin, PC, RC)),
                );
            }
        }
        6 | 7 => {
            if let Some(bytes) = shape {
                load_mesh(m, definition, bytes)?;
                if kind == 6 {
                    if f32::from_bits(m.r32(body + 0x14)) == 0.0 {
                        matrix_zero(m, body + 0x18);
                        matrix_zero(m, body + 0x48);
                    } else {
                        crate::native_mesh_inertia::compute(m, body, definition)?;
                        crate::native_mesh_inertia::scale_mass(m, body);
                    }
                }
                let mut radius = constant(0.);
                for i in 0..m.r32(definition + 0x9c) {
                    let v = m.r32(definition + 0xa0) + i * 12;
                    let x = f(m.r32(v));
                    let y = f(m.r32(v + 4));
                    let z = f(m.r32(v + 8));
                    let squared = x.mul(x, PC, RC).add(y.mul(y, PC, RC), PC, RC).add(
                        z.mul(z, PC, RC),
                        PC,
                        RC,
                    );
                    if radius.to_f32(PC, RC) < squared.to_f32(PC, RC) {
                        radius = squared;
                    }
                }
                let radius = radius.sqrt(PC, RC).add(margin, PC, RC);
                m.w32(definition + 0xc, stored(radius));
                if kind == 7 {
                    stack::call(m, 0x49b06d, &[1, stored(radius)], Some(body), |m| {
                        sphere_inertia(m, body, definition, stored(radius))
                    });
                    // Original54e3b0 copies the three padding slots from its
                    // stack frame. 4b5c30 previously wrote saved EBX, the
                    // exhausted V3 face counter, and the bounds maximum X.
                    let vertices = m.r32(definition + 0xa0);
                    let mut maximum = m.r32(vertices);
                    for i in 0..m.r32(definition + 0x9c).saturating_sub(1) {
                        let v = m.r32(vertices + i * 12);
                        if f32::from_bits(maximum) <= f32::from_bits(v) {
                            maximum = v;
                        }
                    }
                    if f32::from_bits(m.r32(body + 0x14)) != 0.0 {
                        m.w32(body + 0x54, 0);
                        m.w32(body + 0x64, 0);
                        m.w32(body + 0x74, maximum);
                    }
                }
            } else {
                stack::call(
                    m,
                    if kind == 6 { 0x49afc7 } else { 0x49b06d },
                    &[1, 1f32.to_bits()],
                    Some(body),
                    |m| sphere_inertia(m, body, definition, 1f32.to_bits()),
                );
            }
            if kind == 7 {
                m.w32(definition + 0x34, m.r32(definition + 0x34) | 1);
                m.w32(definition + 0x4c, 0);
            }
        }
        _ => unreachable!(),
    }
    m.w32(body + 0x88, m.r32(definition + 0x4c));
    Ok(body)
}
/// Original4b5c30 mesh loading into an existing zeroed collision definition.
/// Positions retain centimetre-to-metre x87 scaling, triangle ordering, normal
/// arithmetic, and the original bounds loop's exclusion of the last vertex.
pub fn load_mesh(m: &mut Space, definition: u32, bytes: &[u8]) -> Result<(), String> {
    let decoded = slr_fmt_scx::parse(bytes)
        .map_err(|error| format!("NativeGeometryBoundary: collision mesh {error:?}"))?;
    let groups: Vec<(Vec<[f32; 3]>, Vec<u32>)> = match decoded {
        slr_fmt_scx::Scx::V3 { groups, .. } => groups
            .into_iter()
            .map(|g| (g.mesh.positions, g.mesh.indices))
            .collect(),
        slr_fmt_scx::Scx::V4 { meshes, .. } => {
            let mut base = 0;
            let mut groups = vec![];
            for mesh in meshes {
                if mesh.flags & 1 == 0 {
                    return Err(
                        "NativeGeometryBoundary: collision vertex chunk lacks positions".into(),
                    );
                }
                let indices = mesh.indices.into_iter().map(|i| i as u32 + base).collect();
                base += mesh.positions.len() as u32;
                groups.push((mesh.positions, indices));
            }
            groups
        }
    };
    // File54c300 owns this temporary allocation; free54f5b0 does not reclaim
    // addresses in the declared bump-allocator scenario used by Space.
    let input = m.alloc(bytes.len() as u32);
    for (i, byte) in bytes.iter().enumerate() {
        m.w8(input + i as u32, *byte);
    }
    let mut vertices = 0u32;
    let mut faces = 0u32;
    let mut vertex_address = 0;
    let mut face_address = 0;
    for (positions, indices) in groups {
        if indices.len() % 3 != 0 {
            return Err("NativeGeometryBoundary: incomplete collision triangle".into());
        }
        let new_vertices = vertices + positions.len() as u32;
        let next = m.malloc(new_vertices * 12);
        if vertices != 0 {
            m.copy(next, vertex_address, vertices * 12);
        }
        vertex_address = next;
        for (i, position) in positions.iter().enumerate() {
            for (j, coord) in position.iter().enumerate() {
                m.w32(
                    vertex_address + (vertices + i as u32) * 12 + j as u32 * 4,
                    stored(constant(*coord).mul(f(0x3c23d70a), PC, RC)),
                );
            }
        }
        vertices = new_vertices;
        let new_faces = faces + indices.len() as u32 / 3;
        let next = m.malloc(new_faces * 32);
        if faces != 0 {
            m.copy(next, face_address, faces * 32);
        }
        face_address = next;
        for (i, triangle) in indices.as_chunks::<3>().0.iter().enumerate() {
            let face = face_address + (faces + i as u32) * 32;
            m.w32(face, 3);
            for (j, index) in triangle.iter().enumerate() {
                if *index >= vertices {
                    return Err("NativeGeometryBoundary: collision index out of bounds".into());
                }
                m.w32(face + 4 + j as u32 * 4, *index);
            }
        }
        faces = new_faces;
    }
    if vertices == 0 || faces == 0 {
        return Err(
            "NativeGeometryBoundary: original collision mesh has no vertices or faces".into(),
        );
    }
    for (offset, value) in [
        (0x9c, vertices),
        (0xa0, vertex_address),
        (0xa4, faces),
        (0xa8, face_address),
    ] {
        m.w32(definition + offset, value);
    }
    for i in 0..faces {
        let face = face_address + i * 32;
        let p: [u32; 3] =
            std::array::from_fn(|j| vertex_address + m.r32(face + 4 + j as u32 * 4) * 12);
        let a: [X87; 3] = std::array::from_fn(|j| {
            f(m.r32(p[1] + j as u32 * 4)).sub(f(m.r32(p[0] + j as u32 * 4)), PC, RC)
        });
        let b: [X87; 3] = std::array::from_fn(|j| {
            f(m.r32(p[2] + j as u32 * 4)).sub(f(m.r32(p[1] + j as u32 * 4)), PC, RC)
        });
        let n = [
            b[2].mul(a[1], PC, RC).sub(b[1].mul(a[2], PC, RC), PC, RC),
            a[2].mul(b[0], PC, RC).sub(b[2].mul(a[0], PC, RC), PC, RC),
            b[1].mul(a[0], PC, RC).sub(a[1].mul(b[0], PC, RC), PC, RC),
        ];
        let length = n[1]
            .mul(n[1], PC, RC)
            .add(n[2].mul(n[2], PC, RC), PC, RC)
            .add(n[0].mul(n[0], PC, RC), PC, RC)
            .sqrt(PC, RC);
        let inverse = if length.to_f32(PC, RC) != 0.0 {
            constant(1.).div(length, PC, RC)
        } else {
            constant(1.)
        };
        for (j, value) in n.into_iter().enumerate() {
            m.w32(
                face + 0x14 + j as u32 * 4,
                stored(value.mul(inverse, PC, RC)),
            );
        }
    }
    let mut minimum: [u32; 3] = std::array::from_fn(|j| m.r32(vertex_address + j as u32 * 4));
    let mut maximum = minimum;
    let mut radius_squared = constant(0.);
    for i in 0..vertices.saturating_sub(1) {
        let p: [u32; 3] = std::array::from_fn(|j| m.r32(vertex_address + i * 12 + j as u32 * 4));
        for j in 0..3 {
            if f32::from_bits(p[j]) <= f32::from_bits(minimum[j]) {
                minimum[j] = p[j];
            }
            if f32::from_bits(maximum[j]) <= f32::from_bits(p[j]) {
                maximum[j] = p[j];
            }
        }
        let square = |j: usize| f(p[j]).mul(f(p[j]), PC, RC);
        let squared = square(1).add(square(2), PC, RC).add(square(0), PC, RC);
        if radius_squared.to_f32(PC, RC) < squared.to_f32(PC, RC) {
            radius_squared = squared;
        }
    }
    for j in 0..3 {
        let extent = f(maximum[j])
            .sub(f(minimum[j]), PC, RC)
            .mul(constant(0.5), PC, RC);
        m.w32(definition + 0x14 + j as u32 * 4, stored(extent));
        m.w32(
            definition + 0x40 + j as u32 * 4,
            stored(f(minimum[j]).add(f(stored(extent)), PC, RC)),
        );
    }
    m.w32(
        definition + 0xc,
        stored(radius_squared.sqrt(PC, RC).add(f(0x3d4ccccd), PC, RC)),
    );
    Ok(())
}
/// Original49a990 after the resource acquisition guard has returned its real
/// collision definition. Ownership binds to that same definition and increments
/// its reference list, rather than allocating a replacement primitive.
pub fn body_from_definition(m: &mut Space, mass: u32, definition: u32) -> Result<u32, String> {
    if definition == 0 {
        return Err("NativeGeometryBoundary: missing collision definition".into());
    }
    let body = allocate_body(m, mass);
    let inv_mass = m.r32(body + 0x14);
    if f32::from_bits(inv_mass) == 0.0 {
        matrix_zero(m, body + 0x18);
    } else {
        let value = stored(constant(2.5).div(f(mass), PC, RC));
        for offset in [0x18, 0x2c, 0x40] {
            m.w32(body + offset, value);
        }
    }
    matrix_zero(m, body + 0x48);
    m.w32(body + 0x78, definition);
    m.w32(body + 0x7c, m.r32(definition));
    m.w32(definition, body + 0x78);
    m.w32(definition + 4, m.r32(definition + 4).wrapping_add(1));
    m.w32(body + 0x88, m.r32(definition + 0x4c));
    Ok(body)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_mesh_body_mass_integrals_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-geometry-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/native_geometry_probe.py"),
        )
        .unwrap();
        for row in rows["mesh_bodies"].as_array().unwrap() {
            let hex = row["data_hex"].as_str().unwrap();
            let data: Vec<_> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            let mut memory = Space::new();
            let p = body(
                &mut memory,
                row["mass"].as_u64().unwrap() as u32,
                row["kind"].as_u64().unwrap() as u32,
                [0; 3],
                Some(&data),
            )
            .unwrap();
            let d = memory.r32(p + 0x78);
            for (name, address, count) in [
                ("body_words", p, 40),
                ("definition_words", d, 43),
                ("vertices", memory.r32(d + 0xa0), memory.r32(d + 0x9c) * 3),
                ("faces", memory.r32(d + 0xa8), memory.r32(d + 0xa4) * 8),
                ("world_words", WORLD + 0x170, 16),
            ] {
                for i in 0..count {
                    let actual = memory.r32(address + i * 4);
                    let expected = row[name][i as usize].as_u64().unwrap() as u32;
                    assert_eq!(
                        actual,
                        expected,
                        "{} kind{} mass{} {name}+{:#x}: {actual:#010x} vs {expected:#010x}",
                        row["label"],
                        row["kind"],
                        row["mass"],
                        i * 4
                    );
                }
            }
        }
    }
    #[test]
    fn original_primitive_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-geometry-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_primitive_graphs_match; run re/oracle/native_geometry_probe.py");
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows["rows"].as_array().unwrap() {
            let mut memory = Space::new();
            let dims = std::array::from_fn(|i| row["dimensions"][i].as_u64().unwrap() as u32);
            let p = body(
                &mut memory,
                row["mass"].as_u64().unwrap() as u32,
                row["kind"].as_u64().unwrap() as u32,
                dims,
                None,
            )
            .unwrap();
            assert_eq!(p, row["body"].as_u64().unwrap() as u32);
            for (name, p, count) in [
                ("body_words", p, 40),
                ("definition_words", memory.r32(p + 0x78), 43),
                ("world_words", WORLD + 0x170, 16),
            ] {
                let actual = (0..count)
                    .map(|i| memory.r32(p + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual, expected,
                    "kind{} mass{} {name}",
                    row["kind"], row["mass"]
                );
            }
        }
    }
    #[test]
    fn original_mesh_definition_and_body_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-geometry-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_mesh_definition_and_body_graphs_match; run re/oracle/native_geometry_probe.py");
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows["mesh"].as_array().unwrap() {
            let mut memory = Space::new();
            let definition = memory.alloc(0xac);
            memory.alloc(16);
            let hex = row["data_hex"].as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            load_mesh(&mut memory, definition, &bytes).unwrap();
            for (name, address, count) in [
                ("words", definition, 43),
                (
                    "vertices",
                    memory.r32(definition + 0xa0),
                    memory.r32(definition + 0x9c) * 3,
                ),
                (
                    "faces",
                    memory.r32(definition + 0xa8),
                    memory.r32(definition + 0xa4) * 8,
                ),
            ] {
                let actual = (0..count)
                    .map(|i| memory.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "mesh {name}");
            }
        }
        for row in rows["definition_bodies"].as_array().unwrap() {
            let mut memory = Space::new();
            let definition = memory.alloc(0xac);
            memory.wf(definition + 0x4c, 123.);
            for _ in 0..4 {
                memory.alloc(0x100);
            }
            let body = body_from_definition(
                &mut memory,
                row["mass"].as_u64().unwrap() as u32,
                definition,
            )
            .unwrap();
            assert_eq!(body, row["body"].as_u64().unwrap() as u32);
            for (name, address, count) in [
                ("body_words", body, 40),
                ("definition_words", definition, 43),
                ("world_words", WORLD + 0x170, 16),
            ] {
                let actual = (0..count)
                    .map(|i| memory.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "definition body {name}");
            }
        }
    }
}
