//! Original segment/sphere and resource/physical mesh ray kernels.
//! Results retain x87 operation order and single-precision store boundaries.
//! CPU exception flags and native scratch/register ABI are outside this slice.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn value(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn store(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}
fn ray_lt(a: X87, b: X87) -> bool {
    a.fcom(b).0.c0
}
fn ray_le(a: X87, b: X87) -> bool {
    let c = a.fcom(b).0;
    c.c0 || c.c3
}
fn ray_neg(v: X87) -> X87 {
    let mut b = v.to_bytes();
    b[9] ^= 0x80;
    X87::from_bytes(b)
}
/// Original498580: directed finite ray/sphere broadphase fraction.
/// Exact returned x87 value only; native scratch ABI/exception flags are separate.
pub fn segment_sphere(
    start: [u32; 3],
    direction: [u32; 3],
    length: u32,
    center: [u32; 3],
    radius: u32,
) -> X87 {
    let delta = [0, 1, 2].map(|i| value(center[i]).sub(value(start[i]), PC, RC));
    let stored_delta = delta.map(|d| value(store(d)));
    // Z is stored without popping, then used live for the first product.
    let projection = value(store(
        delta[2]
            .mul(value(direction[2]), PC, RC)
            .add(stored_delta[1].mul(value(direction[1]), PC, RC), PC, RC)
            .add(stored_delta[0].mul(value(direction[0]), PC, RC), PC, RC),
    ));
    let radius = value(radius);
    let miss = value(0xbf800000);
    if !ray_le(ray_neg(radius), projection) {
        return miss;
    }
    if ray_lt(value(length).add(radius, PC, RC), projection) {
        return miss;
    }
    let projection_squared = value(store(projection.mul(projection, PC, RC)));
    let perpendicular_squared = value(store(
        stored_delta[1]
            .mul(stored_delta[1], PC, RC)
            .add(stored_delta[2].mul(stored_delta[2], PC, RC), PC, RC)
            .add(stored_delta[0].mul(stored_delta[0], PC, RC), PC, RC)
            .sub(projection_squared, PC, RC),
    ));
    let radius_squared = value(store(radius.mul(radius, PC, RC)));
    if !ray_le(perpendicular_squared, radius_squared) {
        return miss;
    }
    if ray_lt(projection, value(0))
        && !ray_le(
            projection_squared.add(perpendicular_squared, PC, RC),
            radius_squared,
        )
    {
        return miss;
    }
    if !ray_le(projection, value(length)) {
        let excess = projection.sub(value(length), PC, RC);
        if ray_lt(
            radius_squared,
            excess
                .mul(excess, PC, RC)
                .add(perpendicular_squared, PC, RC),
        ) {
            return miss;
        }
    }
    let result = value(length)
        .sub(projection, PC, RC)
        .sub(
            radius_squared
                .sub(perpendicular_squared, PC, RC)
                .sqrt(PC, RC),
            PC,
            RC,
        )
        .div(value(length), PC, RC);
    if !ray_le(result, value(0)) {
        result
    } else {
        value(0)
    }
}
fn ray_abs(v: X87) -> X87 {
    let mut b = v.to_bytes();
    b[9] &= 0x7f;
    X87::from_bytes(b)
}
fn projection_axes(n: [u32; 3]) -> (usize, usize) {
    let a = n.map(|v| ray_abs(value(v)));
    if ray_lt(a[1], a[0]) {
        if ray_le(a[2], a[0]) {
            (1, 2)
        } else {
            (0, 1)
        }
    } else if !ray_lt(a[1], a[2]) {
        (0, 2)
    } else {
        (0, 1)
    }
}
/// Original49fa60 with null entity: game resource triangle selection only.
/// The nonnull physical collision publication path is a separate collaborator.
pub fn game_triangle(vertices: [[u32; 3]; 3], normal: [u32; 3], ray: [u32; 6]) -> X87 {
    let zero = value(0);
    let one = value(0x3f800000);
    let epsilon = value(0x3ca3d70a);
    let a = [0, 1, 2].map(|i| value(store(value(ray[i]).sub(value(vertices[0][i]), PC, RC))));
    let b = [0, 1, 2].map(|i| value(store(value(ray[i + 3]).sub(value(vertices[0][i]), PC, RC))));
    let dot = |p: [X87; 3]| {
        p[2].mul(value(normal[2]), PC, RC)
            .add(p[1].mul(value(normal[1]), PC, RC), PC, RC)
            .add(p[0].mul(value(normal[0]), PC, RC), PC, RC)
    };
    let d0 = value(store(dot(a)));
    let d1_live = dot(b);
    let d1 = value(store(d1_live));
    if ray_le(d1_live, zero) || !ray_le(d0, zero) {
        return zero;
    }
    if !ray_le(d1.mul(d0, PC, RC), zero)
        && !ray_le(ray_abs(d0), epsilon)
        && !ray_le(ray_abs(d1), epsilon)
    {
        return zero;
    }
    let (x, y) = projection_axes(normal);
    let sub = |vertex: usize, axis: usize| {
        value(vertices[vertex][axis]).sub(value(vertices[0][axis]), PC, RC)
    };
    if !ray_le(ray_abs(d0), epsilon) || !ray_le(ray_abs(d1), epsilon) {
        let fraction = value(store(ray_abs(d0.div(d0.sub(d1, PC, RC), PC, RC))));
        let delta = [0, 1, 2].map(|i| b[i].sub(a[i], PC, RC));
        let point = [0, 1, 2].map(|i| {
            value(store(
                (if i == 0 {
                    delta[i]
                } else {
                    value(store(delta[i]))
                })
                .mul(fraction, PC, RC)
                .add(a[i], PC, RC),
            ))
        });
        let bx = value(store(sub(1, x)));
        let by = value(store(sub(1, y)));
        let cx = sub(2, x);
        let cy = sub(2, y);
        let area = value(store(cy.mul(bx, PC, RC).sub(cx.mul(by, PC, RC), PC, RC)));
        let u = cy
            .mul(point[x], PC, RC)
            .sub(cx.mul(point[y], PC, RC), PC, RC)
            .div(area, PC, RC);
        let u_word = value(store(u));
        if ray_lt(u, zero) || !ray_le(u_word, one) {
            return zero;
        }
        let v = bx
            .mul(point[y], PC, RC)
            .sub(by.mul(point[x], PC, RC), PC, RC)
            .div(area, PC, RC);
        if ray_lt(v, zero) || !ray_le(v.add(u_word, PC, RC), one) {
            return zero;
        }
        fraction
    } else {
        let area_live =
            sub(2, y)
                .mul(sub(1, x), PC, RC)
                .sub(sub(1, y).mul(sub(2, x), PC, RC), PC, RC);
        let area = value(store(area_live));
        if ray_lt(ray_abs(area_live), value(0x3727c5ac)) {
            return zero;
        }
        let mut lower = zero;
        let mut upper = one;
        for current in 0..3 {
            let next = (current + 1) % 3;
            let dx = value(vertices[next][x])
                .sub(value(vertices[current][x]), PC, RC)
                .mul(area, PC, RC);
            let dy = ray_neg(
                value(vertices[next][y])
                    .sub(value(vertices[current][y]), PC, RC)
                    .mul(area, PC, RC),
            );
            let edge = |offset: usize| {
                value(ray[offset + x])
                    .sub(value(vertices[current][x]), PC, RC)
                    .mul(dy, PC, RC)
                    .add(
                        value(ray[offset + y])
                            .sub(value(vertices[current][y]), PC, RC)
                            .mul(dx, PC, RC),
                        PC,
                        RC,
                    )
            };
            let f0 = value(store(edge(0)));
            let f1 = edge(3);
            if ray_lt(f0, zero) {
                if ray_lt(f1, zero) {
                    return zero;
                }
                let fraction = f0.div(f0.sub(f1, PC, RC), PC, RC);
                if !ray_le(upper, fraction) {
                    upper = value(store(fraction));
                }
            } else {
                let fraction = f0.div(f0.sub(f1, PC, RC), PC, RC);
                if ray_lt(lower, fraction) {
                    lower = value(store(fraction));
                }
            }
            if !ray_lt(lower, upper) {
                return zero;
            }
        }
        lower
    }
}
/// Original49f7b0 physical mesh ray, including the live fraction limit test.
pub fn mesh_triangle(vertices: [[u32; 3]; 3], normal: [u32; 3], ray: [u32; 6], limit: u32) -> X87 {
    let zero = value(0);
    let one = value(0x3f800000);
    let epsilon = value(0x3ca3d70a);
    let a = [0, 1, 2].map(|i| value(store(value(ray[i]).sub(value(vertices[0][i]), PC, RC))));
    let b = [0, 1, 2].map(|i| value(store(value(ray[i + 3]).sub(value(vertices[0][i]), PC, RC))));
    let d0 = value(store(
        a[2].mul(value(normal[2]), PC, RC)
            .add(a[0].mul(value(normal[0]), PC, RC), PC, RC)
            .add(a[1].mul(value(normal[1]), PC, RC), PC, RC),
    ));
    let d1 = value(store(
        b[0].mul(value(normal[0]), PC, RC)
            .add(b[1].mul(value(normal[1]), PC, RC), PC, RC)
            .add(b[2].mul(value(normal[2]), PC, RC), PC, RC),
    ));
    if ray_le(d1, zero) || !ray_le(d0, zero) {
        return zero;
    }
    let fraction_live = ray_abs(d1.div(d0.sub(d1, PC, RC), PC, RC));
    let fraction = value(store(fraction_live));
    if !ray_le(fraction_live, value(limit)) {
        return zero;
    }
    if !ray_le(d1.mul(d0, PC, RC), zero)
        && !ray_le(ray_abs(d0), epsilon)
        && !ray_le(ray_abs(d1), epsilon)
    {
        return zero;
    }
    if ray_le(ray_abs(d0), epsilon) && ray_le(ray_abs(d1), epsilon) {
        return zero;
    }
    let delta = [0, 1, 2].map(|i| a[i].sub(b[i], PC, RC));
    let point = [0, 1, 2].map(|i| {
        value(store(
            (if i == 0 {
                delta[i]
            } else {
                value(store(delta[i]))
            })
            .mul(fraction, PC, RC)
            .add(b[i], PC, RC),
        ))
    });
    let (x, y) = projection_axes(normal);
    let sub = |vertex: usize, axis: usize| {
        value(vertices[vertex][axis]).sub(value(vertices[0][axis]), PC, RC)
    };
    let bx = value(store(sub(1, x)));
    let by = value(store(sub(1, y)));
    let cx = sub(2, x);
    let cy = sub(2, y);
    let area = value(store(cy.mul(bx, PC, RC).sub(cx.mul(by, PC, RC), PC, RC)));
    let u = cy
        .mul(point[x], PC, RC)
        .sub(cx.mul(point[y], PC, RC), PC, RC)
        .div(area, PC, RC);
    let u_word = value(store(u));
    if ray_lt(u, zero) || !ray_le(u_word, one) {
        return zero;
    }
    let v = bx
        .mul(point[y], PC, RC)
        .sub(by.mul(point[x], PC, RC), PC, RC)
        .div(area, PC, RC);
    if ray_lt(v, zero) || !ray_le(v.add(u_word, PC, RC), one) {
        return zero;
    }
    fraction
}

/// Original49fa60 null-entity call: read live indices, normal and ray storage.
/// A physical contact publication call has a different, nonnull entity contract.
pub(crate) fn game_resource_triangle(m: &Space, vertices: u32, triangle: u32, ray: u32) -> X87 {
    let positions = [0, 1, 2].map(|i| {
        let p = vertices + m.r32(triangle + 4 + i * 4) * 12;
        [0, 1, 2].map(|j| m.r32(p + j * 4))
    });
    game_triangle(
        positions,
        [0, 1, 2].map(|i| m.r32(triangle + 0x10 + i * 4)),
        [0, 1, 2, 3, 4, 5].map(|i| m.r32(ray + i * 4)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_ray_test_support as proof;
    #[test]
    fn original_sphere_kernel_matches_all_80_bits() {
        let Some(data) = proof::load(
            "SLR_NATIVE_RAY_SPHERE_VECTORS",
            "native-ray-sphere-original.json",
            "sphere",
            &[
                "re/oracle/native_ray_sphere_probe.py",
                "re/oracle/oracle.py",
            ],
        ) else {
            return;
        };
        let sha = "e0d91c43a7fd7dc2662c5b16c62f76a79633952338d3e3493b9f59c8dac26477";
        proof::kernel_rows(&data, 653, 11, sha);
        proof::negative_kernel_controls(&data, 653, 11, sha);
        for row in data["rows"].as_array().unwrap() {
            let w = proof::words(&row["words"]);
            proof::returned(
                row,
                segment_sphere(
                    w[..3].try_into().unwrap(),
                    w[3..6].try_into().unwrap(),
                    w[6],
                    w[7..10].try_into().unwrap(),
                    w[10],
                ),
            );
        }
    }
    #[test]
    fn original_game_triangle_kernel_matches_all_80_bits() {
        let Some(data) = proof::load(
            "SLR_NATIVE_RAY_GAME_TRIANGLE_VECTORS",
            "native-ray-game-triangle-original.json",
            "game-triangle",
            &[
                "re/oracle/native_ray_game_triangle_probe.py",
                "re/oracle/oracle.py",
            ],
        ) else {
            return;
        };
        let sha = "060c11ff7c22c3fd8adaa2944907217eca1066371fce3f1d8b11d281dba6ac4d";
        proof::kernel_rows(&data, 904, 18, sha);
        proof::negative_kernel_controls(&data, 904, 18, sha);
        for row in data["rows"].as_array().unwrap() {
            let w = proof::words(&row["words"]);
            proof::returned(
                row,
                game_triangle(
                    [
                        w[..3].try_into().unwrap(),
                        w[3..6].try_into().unwrap(),
                        w[6..9].try_into().unwrap(),
                    ],
                    w[9..12].try_into().unwrap(),
                    w[12..18].try_into().unwrap(),
                ),
            );
        }
    }
    #[test]
    fn original_mesh_triangle_kernel_matches_all_80_bits() {
        let Some(data) = proof::load(
            "SLR_NATIVE_RAY_MESH_TRIANGLE_VECTORS",
            "native-ray-mesh-triangle-original.json",
            "mesh-triangle",
            &[
                "re/oracle/native_ray_game_triangle_probe.py",
                "re/oracle/native_ray_mesh_triangle_probe.py",
                "re/oracle/oracle.py",
            ],
        ) else {
            return;
        };
        let sha = "67a1b01ddfd3df0536bd47565e6eca6f7c6a040cfe5a0f9781dbac22ee0696ed";
        proof::kernel_rows(&data, 3616, 19, sha);
        proof::negative_kernel_controls(&data, 3616, 19, sha);
        for row in data["rows"].as_array().unwrap() {
            let w = proof::words(&row["words"]);
            proof::returned(
                row,
                mesh_triangle(
                    [
                        w[..3].try_into().unwrap(),
                        w[3..6].try_into().unwrap(),
                        w[6..9].try_into().unwrap(),
                    ],
                    w[9..12].try_into().unwrap(),
                    w[12..18].try_into().unwrap(),
                    w[18],
                ),
            );
        }
    }
}
