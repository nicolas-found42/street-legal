//! Physics entities created by original 49b300/49b230. The currently verified
//! path uses the original null rotation argument (identity orientation).
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

/// Construct and bind a root entity to an existing physical body. Original
/// 49b0b0 links the body reference at +5c, computes its threshold and copies
/// the active state to its second integration buffer. Rotation matrices are
/// deliberately rejected until the matrix/quaternion path is differential tested.
pub fn root(
    m: &mut Space,
    body: u32,
    position: [u32; 3],
    rotation: Option<u32>,
) -> Result<u32, String> {
    entity_entry(m, None, body, position, rotation)
}
fn entity_entry(
    m: &mut Space,
    parent: Option<u32>,
    body: u32,
    position: [u32; 3],
    rotation: Option<u32>,
) -> Result<u32, String> {
    use crate::native_stack::{self as stack, Reg};
    // Direct host calls carry their value-array position through an explicit
    // scratch carrier; nested calls retain the caller's actual stack pointer.
    const POSITION: u32 = 0x3000_fc00;
    let args = if let Some(parent) = parent {
        vec![parent, body, POSITION, rotation.unwrap_or(0)]
    } else {
        vec![body, POSITION, rotation.unwrap_or(0)]
    };
    stack::function(m, &args, Some(WORLD), |m| {
        let position_ptr = stack::argument(m, if parent.is_some() { 2 } else { 1 });
        if position_ptr == POSITION {
            for (i, value) in position.into_iter().enumerate() {
                m.w32(POSITION + i as u32 * 4, value);
            }
        }
        stack::push_reg(m, Reg::Ecx);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Ebx, WORLD);
        stack::push(m, 0x218);
        stack::store(m, 0x10, WORLD);
        let entity = stack::external(
            m,
            if parent.is_some() { 0x49b244 } else { 0x49b314 },
            &[],
            None,
            |m| m.alloc(0x218),
        );
        stack::add_sp(m, 4);
        stack::set_reg(m, Reg::Esi, entity);
        stack::set_reg(m, Reg::Edi, 0);
        stack::push_reg(m, Reg::Ebp);
        for index in 0..2 {
            stack::set_reg(m, Reg::Ebx, entity + 0x64 + index * 0xd4);
            stack::set_reg(m, Reg::Ebp, 2 - index);
            stack::external(
                m,
                if parent.is_some() { 0x49b282 } else { 0x49b34e },
                &[],
                Some(entity + 0x64 + index * 0xd4),
                |_| (),
            );
        }
        stack::set_reg(m, Reg::Ebp, stack::load(m, 0));
        stack::add_sp(m, 4);
        stack::set_reg(m, Reg::Ebx, WORLD);
        stack::call(
            m,
            if parent.is_some() { 0x49b2d8 } else { 0x49b3a4 },
            &[entity, body, position_ptr, rotation.unwrap_or(0)],
            Some(WORLD),
            |m| {
                stack::function(
                    m,
                    &[entity, body, position_ptr, rotation.unwrap_or(0)],
                    Some(WORLD),
                    |m| {
                        stack::push_reg(m, Reg::Ecx);
                        stack::push_reg(m, Reg::Ebx);
                        stack::push_reg(m, Reg::Ebp);
                        stack::set_reg(m, Reg::Ebp, entity);
                        stack::store(m, 8, WORLD);
                        stack::push_reg(m, Reg::Esi);
                        stack::set_reg(m, Reg::Esi, 0);
                        stack::set_reg(m, Reg::Ebx, 0);
                        stack::push_reg(m, Reg::Edi);
                        stack::set_reg(m, Reg::Edi, body);
                        construct(m, entity, body, position, rotation, parent.is_none())
                    },
                )
            },
        )?;
        if let Some(parent) = parent {
            m.w32(entity + 0x28, m.r32(parent + 0x24));
            m.w32(parent + 0x24, entity);
            m.w32(entity + 0x20, parent);
            stack::call(m, 0x49b2f0, &[entity], Some(WORLD), |m| {
                update_relative(m, entity)
            })?;
        }
        Ok(entity)
    })
}
fn construct(
    m: &mut Space,
    e: u32,
    body: u32,
    position: [u32; 3],
    rotation: Option<u32>,
    attach_world: bool,
) -> Result<u32, String> {
    if body == 0 {
        return Err("NativeEntityBoundary: missing physical body".into());
    }
    m.w32(e + 0x5c, body);
    m.w32(e + 0x60, m.r32(body + 0xc));
    m.w32(body + 0xc, e + 0x5c);
    m.w32(body + 0x10, m.r32(body + 0x10).wrapping_add(1));
    m.w32(e + 8, 0x3ca3d70a);
    let inverse_mass = m.r32(body + 0x14);
    m.w32(
        e + 0x18,
        if f32::from_bits(inverse_mass) > 0. {
            stored(f(0x3dcccccd).div(f(inverse_mass), PC, RC))
        } else {
            1000f32.to_bits()
        },
    );
    for (i, word) in position.into_iter().enumerate() {
        m.w32(e + 0x64 + i as u32 * 4, word);
    }
    if let Some(rotation) = rotation {
        let q = matrix_quaternion(read_matrix(m, rotation));
        for (i, word) in q.into_iter().enumerate() {
            m.w32(e + 0xf8 + i as u32 * 4, word);
        }
        write_matrix(m, e + 0xa4, quaternion_matrix(q));
    } else {
        for offset in [0xa4, 0xb8, 0xcc, 0x104] {
            m.w32(e + offset, 1f32.to_bits());
        }
    }
    m.w32(body + 0x98, m.r32(body + 0x98).wrapping_add(1));
    crate::native_stack::call(m, 0x49b1c1, &[], Some(e), |m| world_inertia(m, e));
    let flags = m.r32(body + 0x9c);
    m.w32(
        e + 0x58,
        u32::from(flags & 4 == 0)
            | if flags & 8 != 0 { 0x20 } else { 0 }
            | if flags & 0x40 != 0 { 0x100 } else { 0 }
            | (flags & 2),
    );
    m.w32(e + 0x88, m.r32(WORLD));
    m.w32(e + 0xa0, 1);
    m.copy(e + 0x138, e + 0x64, 0xd4);
    m.w32(WORLD + 0x54, m.r32(WORLD + 0x54).wrapping_add(1));
    // 49b300 optionally attaches dynamic roots to the world's parent entity.
    let parent = m.r32(WORLD + 0x84);
    if attach_world && parent != 0 && flags & 4 == 0 {
        m.w32(e + 0x28, m.r32(parent + 0x24));
        m.w32(parent + 0x24, e);
        m.w32(e + 0x20, parent);
    }
    Ok(e)
}
/// Original49b230 identity-orientation compound member. Relative position,
/// body references, intrusive child list and local transform are retained.
pub fn child(
    m: &mut Space,
    parent: u32,
    body: u32,
    position: [u32; 3],
    rotation: Option<u32>,
) -> Result<u32, String> {
    entity_entry(m, Some(parent), body, position, rotation)
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
fn div(a: u32, b: u32) -> u32 {
    stored(f(a).div(f(b), PC, RC))
}
fn inverse_parts(a: [u32; 9]) -> (u32, [u32; 9], [u32; 9]) {
    let d = sub(
        sub(
            sub(
                add(
                    add(mul(mul(a[6], a[1]), a[5]), mul(mul(a[2], a[3]), a[7])),
                    mul(mul(a[8], a[0]), a[4]),
                ),
                mul(mul(a[0], a[7]), a[5]),
            ),
            mul(mul(a[8], a[1]), a[3]),
        ),
        mul(mul(a[6], a[2]), a[4]),
    );
    let reciprocal = div(1f32.to_bits(), d);
    let cof = [
        sub(mul(a[4], a[8]), mul(a[7], a[5])),
        sub(mul(a[7], a[2]), mul(a[1], a[8])),
        sub(mul(a[1], a[5]), mul(a[2], a[4])),
        sub(mul(a[6], a[5]), mul(a[3], a[8])),
        sub(mul(a[0], a[8]), mul(a[6], a[2])),
        sub(mul(a[3], a[2]), mul(a[0], a[5])),
        sub(mul(a[7], a[3]), mul(a[6], a[4])),
        sub(mul(a[6], a[1]), mul(a[7], a[0])),
        sub(mul(a[4], a[0]), mul(a[3], a[1])),
    ];
    (d, transpose(cof), cof.map(|v| mul(reciprocal, v)))
}
/// Original49a050 through49a3f4: combine child mass and inertia and return
/// the compound center of mass. Bounds regeneration/recentering is a separate
/// remaining phase; callers must not treat this as a finished49a050.
/// This measured path accepts identity relative transforms.
fn compound_frame(m: &mut Space, entity: u32) {
    use crate::native_stack::{self as stack, Reg};
    stack::alloc(m, 0xcc);
    stack::push_reg(m, Reg::Ebx);
    stack::set_reg(m, Reg::Ebx, entity);
    stack::push_reg(m, Reg::Ebp);
    let body = m.r32(entity + 0x5c);
    stack::set_reg(m, Reg::Ebp, body);
    stack::push_reg(m, Reg::Esi);
    stack::store(m, 0x40, stack::reg(m, Reg::Ecx));
    stack::store(m, 0x44, body);
    stack::push_reg(m, Reg::Edi);
    for offset in [0x10, 0x14, 0x18, 0x1c] {
        stack::store(m, offset, 0);
    }
    let tensor = stack::sp(m) + 0x7c;
    stack::external(m, 0x49a0a4, &[], Some(tensor), |m| {
        write_matrix(m, tensor, [0; 9])
    });
}
#[cfg(test)]
pub fn compound_mass(m: &mut Space, entity: u32) -> Result<[u32; 3], String> {
    crate::native_stack::function(m, &[entity], None, |m| {
        compound_frame(m, entity);
        compound_mass_inner(m, entity)
    })
}
fn compound_mass_inner(m: &mut Space, entity: u32) -> Result<[u32; 3], String> {
    use crate::native_stack::{self as stack, Reg};
    let body = m.r32(entity + 0x5c);
    if body == 0 {
        return Err("NativeEntityBoundary: compound has no body".into());
    }
    if m.r32(body + 0x9c) & 0x100 != 0 {
        return Ok([0; 3]);
    }
    if m.r32(entity + 0x58) & 0x8000 != 0 {
        return Err(
            "NativeEntityBoundary: fixed-mass compound requires original49a050 bounds branch"
                .into(),
        );
    }
    let mut total = 0;
    let mut center = [0; 3];
    let mut inertia = [0; 9];
    let mut children = vec![];
    let mut child = m.r32(entity + 0x24);
    while child != 0 {
        if children.contains(&child) {
            return Err("NativeEntityBoundary: cyclic compound child list".into());
        }
        children.push(child);
        child = m.r32(child + 0x28);
    }
    for child in children {
        let child_body = m.r32(child + 0x5c);
        let inv = m.r32(child_body + 0x14);
        if f32::from_bits(inv) == 0. {
            continue;
        }
        let relative = m.r32(child + 0x1c);
        let mass = div(1f32.to_bits(), inv);
        let reciprocal = div(1f32.to_bits(), add(mass, total));
        let p: [u32; 3] = std::array::from_fn(|i| m.r32(relative + 4 + i as u32 * 4));
        let next: [u32; 3] =
            std::array::from_fn(|i| mul(add(mul(mass, p[i]), mul(center[i], total)), reciprocal));
        let old_delta: [u32; 3] = std::array::from_fn(|i| sub(next[i], center[i]));
        let new_delta: [u32; 3] = std::array::from_fn(|i| sub(next[i], p[i]));
        let relative_rotation = read_matrix(m, relative + 0x20);
        let child_inertia = read_matrix(m, child_body + 0x48);
        let intermediate = matrix_inplace_product(transpose(relative_rotation), child_inertia);
        let mut own = matrix_product(relative_rotation, intermediate);
        stack::set_reg(m, Reg::Esi, child);
        stack::set_reg(m, Reg::Edi, relative + 0x20);
        let temp = stack::sp(m) + 0xac;
        stack::external(m, 0x49a251, &[relative + 0x20], Some(temp), |m| {
            write_matrix(m, temp, transpose(relative_rotation))
        });
        stack::external(m, 0x49a264, &[child_body + 0x48], Some(temp), |m| {
            stack::alloc(m, 0x30);
            stack::push_reg(m, Reg::Esi);
            stack::push_reg(m, Reg::Edi);
            let scratch = stack::sp(m) + 8;
            write_matrix(m, scratch, intermediate);
            m.copy(temp, scratch, 0x30);
        });
        let own_ptr = stack::sp(m) + 0x4c;
        stack::external(m, 0x49a276, &[relative + 0x20, temp], Some(own_ptr), |m| {
            write_matrix(m, own_ptr, own)
        });
        for r in 0..3 {
            for c in 0..3 {
                if r == c {
                    let a = (r + 1) % 3;
                    let b = (r + 2) % 3;
                    inertia[r * 3 + c] = add(
                        mul(
                            add(
                                mul(old_delta[a], old_delta[a]),
                                mul(old_delta[b], old_delta[b]),
                            ),
                            total,
                        ),
                        inertia[r * 3 + c],
                    );
                    own[r * 3 + c] = add(
                        mul(
                            add(
                                mul(new_delta[a], new_delta[a]),
                                mul(new_delta[b], new_delta[b]),
                            ),
                            mass,
                        ),
                        own[r * 3 + c],
                    );
                } else {
                    inertia[r * 3 + c] = sub(
                        inertia[r * 3 + c],
                        mul(mul(old_delta[r], old_delta[c]), total),
                    );
                    own[r * 3 + c] =
                        sub(own[r * 3 + c], mul(mul(new_delta[r], new_delta[c]), mass));
                }
                inertia[r * 3 + c] = add(own[r * 3 + c], inertia[r * 3 + c]);
            }
        }
        center = next;
        total = add(div(1f32.to_bits(), inv), total);
    }
    if f32::from_bits(total) <= 0. {
        m.w32(body + 0x14, 0);
        m.w32(body + 0x9c, m.r32(body + 0x9c) | 4);
        m.w32(body + 0x80, 0);
        m.w32(body + 0x84, 0);
        for r in 0..3 {
            for c in 0..3 {
                m.w32(body + 0x18 + r * 16 + c * 4, 0);
                m.w32(body + 0x48 + r * 16 + c * 4, 0);
            }
        }
    } else {
        m.w32(body + 0x14, div(1f32.to_bits(), total));
        let tensor = stack::sp(m) + 0x7c;
        write_matrix(m, tensor, inertia);
        stack::set_reg(m, Reg::Edi, body + 0x48);
        stack::set_reg(m, Reg::Esi, tensor + 0x30);
        m.copy(body + 0x48, tensor, 0x30);
        let (det, cof, inv) = inverse_parts(inertia);
        stack::call(m, 0x49a391, &[tensor], Some(body + 0x18), |m| {
            stack::inverse_copy(m, tensor, body + 0x18, det, cof)
        });
        for i in 0..9 {
            m.w32(
                body + 0x48 + (i / 3) as u32 * 16 + (i % 3) as u32 * 4,
                inertia[i],
            );
            m.w32(
                body + 0x18 + (i / 3) as u32 * 16 + (i % 3) as u32 * 4,
                inv[i],
            );
        }
        m.w32(body + 0x9c, m.r32(body + 0x9c) & !4);
        m.w32(body + 0x80, mul(total, 0xbba3d70a));
        m.w32(body + 0x84, mul(total, 0xbc23d70a));
    }
    Ok(center)
}

/// Complete49a050 for identity-oriented compound graphs whose bounds derive
/// from boxes or explicit collision vertices. The parent may have no children.
pub fn compound(m: &mut Space, entity: u32) -> Result<(), String> {
    compound_recursive(m, entity, &mut vec![])
}
fn compound_recursive(m: &mut Space, entity: u32, ancestors: &mut Vec<u32>) -> Result<(), String> {
    crate::native_stack::function(m, &[entity], None, |m| {
        compound_frame(m, entity);
        compound_recursive_inner(m, entity, ancestors)
    })
}
fn compound_recursive_inner(
    m: &mut Space,
    entity: u32,
    ancestors: &mut Vec<u32>,
) -> Result<(), String> {
    if ancestors.contains(&entity) {
        return Err("NativeEntityBoundary: cyclic compound ancestor graph".into());
    }
    ancestors.push(entity);
    let body = m.r32(entity + 0x5c);
    let definition = m.r32(body + 0x78);
    if m.r32(body + 0x9c) & 0x100 != 0 {
        return Ok(());
    }
    if !matches!(m.r32(definition + 8), 5 | 6) {
        return Err(
            "NativeEntityBoundary: compound bounds currently require definition kind6".into(),
        );
    }
    let mut children = vec![];
    let mut child = m.r32(entity + 0x24);
    while child != 0 {
        if children.contains(&child) {
            return Err("NativeEntityBoundary: cyclic compound child list".into());
        }
        children.push(child);
        child = m.r32(child + 0x28);
    }
    // Validate every fallible branch before mass-phase mutation.
    for child in &children {
        let d = m.r32(m.r32(*child + 0x5c) + 0x78);
        if !matches!(m.r32(d + 8), 0..=7) {
            return Err(
                "NativeEntityBoundary: compound child bounds kind requires original498f50".into(),
            );
        }
    }
    let fixed_mass = m.r32(entity + 0x58) & 0x8000 != 0;
    let mut center = if fixed_mass {
        [0; 3]
    } else {
        compound_mass_inner(m, entity)?
    };
    let mut minimum = [0; 3];
    let mut maximum = [0; 3];
    let bounds = crate::native_stack::sp(m) + 0x14;
    let kind = m.r32(definition + 8);
    crate::native_stack::external(
        m,
        if fixed_mass { 0x49a60b } else { 0x49a406 },
        &[entity, kind, bounds],
        None,
        |m| {
            use crate::native_stack::{self as stack, Reg};
            stack::alloc(m, 0x144);
            stack::push_reg(m, Reg::Ebx);
            stack::set_reg(m, Reg::Ebx, entity);
            stack::push_reg(m, Reg::Esi);
            stack::set_reg(m, Reg::Esi, body);
            stack::store(m, 0x124, body);
            stack::push_reg(m, Reg::Ebp);
            stack::push_reg(m, Reg::Edi);
            for child in &children {
                if m.r32(*child + 0x58) & 0x20 != 0 {
                    continue;
                }
                let d = m.r32(m.r32(*child + 0x5c) + 0x78);
                let delta: [u32; 3] = std::array::from_fn(|i| {
                    sub(
                        m.r32(*child + 0x64 + i as u32 * 4),
                        m.r32(entity + 0x64 + i as u32 * 4),
                    )
                });
                let margin = if m.r32(*child + 0x24) != 0 {
                    0
                } else {
                    0x3c23d70a
                };
                let radius = add(margin, m.r32(d + 0x10));
                let kind = m.r32(d + 8);
                let points: Vec<[u32; 3]> = if kind == 0 {
                    vec![]
                } else if kind == 1 {
                    vec![[0; 3]]
                } else if (2..=4).contains(&kind) {
                    let axis = (kind - 2) as usize;
                    let mut positive = [0; 3];
                    positive[axis] = m.r32(d + 0x9c);
                    let mut negative = positive;
                    negative[axis] ^= 0x80000000;
                    vec![positive, negative]
                } else if kind == 5 {
                    (0..8)
                        .map(|bits| {
                            std::array::from_fn(|axis| {
                                mul(
                                    if bits & (1 << axis) == 0 {
                                        (-1f32).to_bits()
                                    } else {
                                        1f32.to_bits()
                                    },
                                    m.r32(d + 0x9c + axis as u32 * 4),
                                )
                            })
                        })
                        .collect()
                } else {
                    let (n, p) = if m.r32(d + 0x9c) != 0 {
                        (m.r32(d + 0x9c), m.r32(d + 0xa0))
                    } else if m.r32(d + 8) == 6 {
                        (m.r32(d + 0x20), m.r32(d + 0x24))
                    } else {
                        (0, 0)
                    };
                    (0..n)
                        .map(|i| std::array::from_fn(|axis| m.r32(p + i * 12 + axis as u32 * 4)))
                        .collect()
                };
                let relative_rotation = matrix_inplace_product(
                    transpose(read_matrix(m, entity + 0xa4)),
                    read_matrix(m, *child + 0xa4),
                );
                for point in points {
                    let transformed: [u32; 3] = std::array::from_fn(|axis| {
                        let order = if axis == 2 { [1, 2, 0] } else { [2, 0, 1] };
                        add(
                            add(
                                mul(relative_rotation[order[0] * 3 + axis], point[order[0]]),
                                mul(relative_rotation[order[1] * 3 + axis], point[order[1]]),
                            ),
                            mul(relative_rotation[order[2] * 3 + axis], point[order[2]]),
                        )
                    });
                    if kind == 5 {
                        crate::native_stack::store(m, 0x11c, transformed[2]);
                    }
                    for axis in 0..3 {
                        let position = add(transformed[axis], delta[axis]);
                        let low = sub(position, radius);
                        let high = add(position, radius);
                        if f32::from_bits(low) < f32::from_bits(minimum[axis]) {
                            minimum[axis] = low;
                        }
                        if f32::from_bits(maximum[axis]) < f32::from_bits(high) {
                            maximum[axis] = high;
                        }
                    }
                }
            }
        },
    );
    let length_squared = |v: [u32; 3]| add(add(mul(v[0], v[0]), mul(v[1], v[1])), mul(v[2], v[2]));
    let a = length_squared(minimum);
    let b = length_squared(maximum);
    let radius = if f32::from_bits(a) < f32::from_bits(b) {
        b
    } else {
        a
    };
    if f32::from_bits(m.r32(body + 0x14)) == 0. {
        center =
            std::array::from_fn(|i| add(minimum[i], mul(sub(maximum[i], minimum[i]), 0x3f000000)));
    }
    for axis in 0..3 {
        let negative = center[axis] ^ 0x80000000;
        m.w32(definition + 0x40 + axis as u32 * 4, negative);
        minimum[axis] = add(minimum[axis], negative);
        maximum[axis] = add(maximum[axis], negative);
    }
    let vertices = m.alloc(0x60);
    m.w32(definition + 0x20, 8);
    m.w32(definition + 0x24, vertices);
    m.w32(definition + 0x34, m.r32(definition + 0x34) | 4);
    for i in 0..8 {
        for axis in 0..3 {
            m.w32(
                vertices + i * 12 + axis * 4,
                if i & (1 << axis) == 0 {
                    minimum[axis as usize]
                } else {
                    maximum[axis as usize]
                },
            );
        }
    }
    for offset in [0x10, 0x9c, 0xa0, 0xa4, 0xa8] {
        m.w32(definition + offset, 0);
    }
    m.w32(definition + 8, 6);
    m.w32(definition + 0xc, stored(f(radius).sqrt(PC, RC)));
    for axis in 0..3 {
        m.w32(
            definition + 0x14 + axis as u32 * 4,
            if f32::from_bits(minimum[axis]).abs() <= f32::from_bits(maximum[axis]).abs() {
                maximum[axis] & 0x7fff_ffff
            } else {
                minimum[axis] & 0x7fff_ffff
            },
        );
    }
    if !fixed_mass && f32::from_bits(length_squared(center)) > f32::from_bits(0x3727c5ac) {
        for child in &children {
            let relative = m.r32(*child + 0x1c);
            for axis in 0..3 {
                m.w32(
                    relative + 4 + axis * 4,
                    sub(m.r32(relative + 4 + axis * 4), center[axis as usize]),
                );
            }
        }
        let active = m.r32(entity + 0x20c);
        for axis in 0..3 {
            let position = add(
                m.r32(entity + 0x64 + active * 0xd4 + axis * 4),
                center[axis as usize],
            );
            m.w32(entity + 0x64 + active * 0xd4 + axis * 4, position);
            m.w32(entity + 0x64 + (active ^ 1) * 0xd4 + axis * 4, position);
        }
    }
    let relative = m.r32(entity + 0x1c);
    if !fixed_mass
        && relative != 0
        && f32::from_bits(length_squared(center)) > f32::from_bits(0x3727c5ac)
    {
        let rotation = read_matrix(m, relative + 0x20);
        for axis in 0..3 {
            let value = add(
                add(
                    mul(center[0], rotation[axis * 3]),
                    mul(center[1], rotation[axis * 3 + 1]),
                ),
                mul(center[2], rotation[axis * 3 + 2]),
            );
            m.w32(
                relative + 4 + axis as u32 * 4,
                add(value, m.r32(relative + 4 + axis as u32 * 4)),
            );
        }
    }
    let active = m.r32(entity + 0x20c);
    compound_world_inertia(m, entity, false);
    m.w32(entity + 0x20c, active ^ 1);
    compound_world_inertia(m, entity, true);
    m.w32(entity + 0x20c, active);
    let inverse_mass = m.r32(body + 0x14);
    m.w32(
        entity + 0x18,
        if f32::from_bits(inverse_mass) > 0. {
            div(0x3dcccccd, inverse_mass)
        } else {
            1000f32.to_bits()
        },
    );
    crate::native_stack::add_sp(m, 4); // original49a6a9 POP EDI before ancestor recursion.
    if relative != 0 {
        let parent = m.r32(entity + 0x20);
        let ecx = crate::native_stack::load(m, 0x3c);
        crate::native_stack::call(m, 0x49a6df, &[parent], Some(ecx), |m| {
            compound_recursive(m, parent, ancestors)
        })?;
    }
    let resource = m.r32(entity + 0x54);
    if resource != 0 {
        for axis in 0..3 {
            m.w32(
                resource + 0x84 + axis * 4,
                m.r32(entity + 0x64 + m.r32(entity + 0x20c) * 0xd4 + axis * 4),
            );
            m.w32(resource + 0x90 + axis * 4, m.r32(definition + 0xc));
        }
        m.w32(resource + 0x80, 0);
    }
    ancestors.pop();
    Ok(())
}
pub fn read_matrix(m: &Space, p: u32) -> [u32; 9] {
    std::array::from_fn(|i| m.r32(p + (i / 3) as u32 * 16 + (i % 3) as u32 * 4))
}
pub fn write_matrix(m: &mut Space, p: u32, a: [u32; 9]) {
    for (i, word) in a.into_iter().enumerate() {
        m.w32(p + (i / 3) as u32 * 16 + (i % 3) as u32 * 4, word);
    }
}
pub fn transpose(a: [u32; 9]) -> [u32; 9] {
    std::array::from_fn(|i| a[(i % 3) * 3 + i / 3])
}
/// Original54e9c0 multiplication, retaining each output's x87 instruction order.
pub fn matrix_product(a: [u32; 9], b: [u32; 9]) -> [u32; 9] {
    std::array::from_fn(|i| {
        let r = i / 3;
        let c = i % 3;
        let order = match i {
            0 => [0, 1, 2],
            1 | 2 => [1, 0, 2],
            6 => [0, 2, 1],
            _ => [2, 1, 0],
        };
        add(
            add(
                mul(a[r * 3 + order[0]], b[order[0] * 3 + c]),
                mul(a[r * 3 + order[1]], b[order[1] * 3 + c]),
            ),
            mul(a[r * 3 + order[2]], b[order[2] * 3 + c]),
        )
    })
}
/// Original54eab0: in-place product with its distinct operation ordering.
pub fn matrix_inplace_product(a: [u32; 9], b: [u32; 9]) -> [u32; 9] {
    std::array::from_fn(|i| {
        let r = i / 3;
        let c = i % 3;
        let order = match i {
            0 => [1, 0, 2],
            1 | 2 | 4 | 7 => [2, 1, 0],
            _ => [0, 2, 1],
        };
        add(
            add(
                mul(b[r * 3 + order[0]], a[order[0] * 3 + c]),
                mul(b[r * 3 + order[1]], a[order[1] * 3 + c]),
            ),
            mul(b[r * 3 + order[2]], a[order[2] * 3 + c]),
        )
    })
}
fn square_root(v: u32) -> u32 {
    stored(f(v).sqrt(PC, RC))
}
fn matrix_quaternion(a: [u32; 9]) -> [u32; 4] {
    let trace = add(add(a[4], a[0]), a[8]);
    if f32::from_bits(trace) > 0. {
        let root = square_root(add(trace, 1f32.to_bits()));
        let scale = div(0x3f000000, root);
        [
            mul(sub(a[7], a[5]), scale),
            mul(sub(a[2], a[6]), scale),
            mul(sub(a[3], a[1]), scale),
            mul(root, 0x3f000000),
        ]
    } else {
        let mut i = usize::from(f32::from_bits(a[0]) < f32::from_bits(a[4]));
        if f32::from_bits(a[i * 4]) < f32::from_bits(a[8]) {
            i = 2;
        }
        let next = [1, 2, 0];
        let j = next[i];
        let k = next[j];
        let root = square_root(add(sub(a[i * 4], add(a[k * 4], a[j * 4])), 1f32.to_bits()));
        let scale = if f32::from_bits(root) != 0. {
            div(0x3f000000, root)
        } else {
            root
        };
        let mut q = [0; 4];
        q[i] = mul(root, 0x3f000000);
        q[3] = mul(sub(a[k * 3 + j], a[j * 3 + k]), scale);
        q[j] = mul(add(a[j * 3 + i], a[i * 3 + j]), scale);
        q[k] = mul(add(a[k * 3 + i], a[i * 3 + k]), scale);
        q
    }
}
pub(crate) fn quaternion_matrix(q: [u32; 4]) -> [u32; 9] {
    let [x, y, z, w] = q;
    let xx = add(x, x);
    let yy = add(y, y);
    let zz = add(z, z);
    let x2 = mul(xx, x);
    let xw = mul(xx, w);
    let one = 1f32.to_bits();
    [
        sub(one, add(mul(zz, z), mul(yy, y))),
        sub(mul(yy, x), mul(zz, w)),
        add(mul(yy, w), mul(zz, x)),
        add(mul(zz, w), mul(yy, x)),
        sub(one, add(mul(zz, z), x2)),
        sub(mul(zz, y), xw),
        sub(mul(zz, x), mul(yy, w)),
        add(xw, mul(zz, y)),
        sub(one, add(mul(yy, y), x2)),
    ]
}
fn quaternion_inverse(q: [u32; 4]) -> [u32; 4] {
    let [x, y, z, w] = q;
    let inv = div(
        1f32.to_bits(),
        add(add(add(mul(z, z), mul(y, y)), mul(w, w)), mul(x, x)),
    );
    [
        mul(inv, x ^ 0x80000000),
        mul(inv, y ^ 0x80000000),
        mul(inv, z ^ 0x80000000),
        mul(inv, w),
    ]
}
pub(crate) fn quaternion_product(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let half = 0x3f000000;
    let p = mul(add(b[0], b[1]), add(a[0], a[2]));
    let q = mul(sub(b[0], b[1]), sub(a[0], a[2]));
    let r = mul(add(a[3], a[1]), sub(b[3], b[2]));
    let s = mul(sub(a[3], a[1]), add(b[3], b[2]));
    [
        sub(
            mul(add(a[3], a[0]), add(b[3], b[0])),
            mul(add(add(add(s, r), q), p), half),
        ),
        add(
            mul(sub(add(sub(p, q), r), s), half),
            mul(add(b[2], b[1]), sub(a[3], a[0])),
        ),
        add(
            mul(add(sub(sub(p, q), r), s), half),
            mul(add(a[1], a[2]), sub(b[3], b[0])),
        ),
        add(
            mul(add(sub(s, add(q, p)), r), half),
            mul(sub(b[1], b[2]), sub(a[2], a[1])),
        ),
    ]
}
/// Original4a76d0: world inverse inertia for one integration buffer.
pub fn world_inertia(m: &mut Space, entity: u32) {
    use crate::native_stack::{self as stack, Reg};
    stack::function(m, &[], Some(entity), |m| {
        stack::alloc(m, 0x60);
        stack::push_reg(m, Reg::Ebx);
        stack::set_reg(m, Reg::Ebx, m.r32(entity + 0x5c));
        stack::push_reg(m, Reg::Esi);
        let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
        stack::set_reg(m, Reg::Esi, state);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Edi, state + 0x40);
        let rotation = read_matrix(m, state + 0x40);
        let inertia = read_matrix(m, m.r32(entity + 0x5c) + 0x18);
        let transpose_ptr = stack::sp(m) + 0xc;
        stack::external(m, 0x4a76fa, &[state + 0x40], Some(transpose_ptr), |m| {
            write_matrix(m, transpose_ptr, transpose(rotation))
        });
        let temporary = stack::sp(m) + 0x3c;
        let intermediate = matrix_product(inertia, transpose(rotation));
        stack::set_reg(m, Reg::Ebx, m.r32(entity + 0x5c) + 0x18);
        stack::external(
            m,
            0x4a770c,
            &[m.r32(entity + 0x5c) + 0x18, transpose_ptr],
            Some(temporary),
            |m| write_matrix(m, temporary, intermediate),
        );
        stack::external(
            m,
            0x4a771d,
            &[state + 0x40, temporary],
            Some(state + 0xa4),
            |m| write_matrix(m, state + 0xa4, matrix_product(rotation, intermediate)),
        );
    });
}
fn compound_world_inertia(m: &mut Space, entity: u32, second: bool) {
    use crate::native_stack::{self as stack, Reg};
    let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
    stack::set_reg(m, Reg::Esi, state);
    stack::set_reg(m, Reg::Edi, state + 0x40);
    let rotation = read_matrix(m, state + 0x40);
    let body = m.r32(entity + 0x5c);
    let inertia = read_matrix(m, body + 0x18);
    let transposed = stack::sp(m) + 0xac;
    let temporary = stack::sp(m) + 0x4c;
    stack::external(
        m,
        if second { 0x49a67f } else { 0x49a632 },
        &[state + 0x40],
        Some(transposed),
        |m| write_matrix(m, transposed, transpose(rotation)),
    );
    stack::set_reg(m, Reg::Ebp, body + 0x18);
    let intermediate = matrix_product(inertia, transpose(rotation));
    stack::external(
        m,
        if second { 0x49a691 } else { 0x49a647 },
        &[body + 0x18, transposed],
        Some(temporary),
        |m| write_matrix(m, temporary, intermediate),
    );
    stack::external(
        m,
        if second { 0x49a6a2 } else { 0x49a658 },
        &[state + 0x40, temporary],
        Some(state + 0xa4),
        |m| write_matrix(m, state + 0xa4, matrix_product(rotation, intermediate)),
    );
}
/// Original49a840: update the existing entity's relative record.
pub fn update_relative(m: &mut Space, entity: u32) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    stack::function(m, &[entity], None, |m| {
        stack::alloc(m, 0x1c);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        stack::set_reg(m, Reg::Esi, entity);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Edi, m.r32(entity + 0x20));
        update_relative_inner(m, entity)
    })
}
fn update_relative_inner(m: &mut Space, entity: u32) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    let parent = m.r32(entity + 0x20);
    if parent == 0 {
        return Err("NativeEntityBoundary: relative entity has no parent".into());
    }
    m.w32(parent + 0x58, m.r32(parent + 0x58) | 8);
    let mut relative = m.r32(entity + 0x1c);
    if relative == 0 {
        relative = m.alloc(0x50);
        m.w32(entity + 0x1c, relative);
        m.w32(entity + 0x58, m.r32(entity + 0x58) | 4);
    }
    m.w32(relative, entity);
    let ps = parent + m.r32(parent + 0x20c) * 0xd4;
    let es = entity + m.r32(entity + 0x20c) * 0xd4;
    let delta: [u32; 3] = std::array::from_fn(|i| {
        sub(
            m.r32(es + 0x64 + i as u32 * 4),
            m.r32(ps + 0x64 + i as u32 * 4),
        )
    });
    let rotation = read_matrix(m, ps + 0xa4);
    for axis in 0..3 {
        let value = add(
            add(
                mul(delta[0], rotation[axis]),
                mul(delta[2], rotation[6 + axis]),
            ),
            mul(rotation[3 + axis], delta[1]),
        );
        m.w32(relative + 4 + axis as u32 * 4, value);
    }
    let parent_q = std::array::from_fn(|i| m.r32(ps + 0xf8 + i as u32 * 4));
    let entity_q = std::array::from_fn(|i| m.r32(es + 0xf8 + i as u32 * 4));
    stack::set_reg(m, Reg::Ebx, relative);
    let temporary = stack::sp(m) + 0x18;
    let inverse = quaternion_inverse(parent_q);
    stack::external(m, 0x49a957, &[ps + 0xf8], Some(temporary), |m| {
        for (i, v) in inverse.into_iter().enumerate() {
            m.w32(temporary + i as u32 * 4, v);
        }
    });
    let relative_q = quaternion_product(inverse, entity_q);
    stack::set_reg(m, Reg::Edi, relative + 0x10);
    stack::external(
        m,
        0x49a97e,
        &[temporary, es + 0xf8],
        Some(relative + 0x10),
        |m| {
            stack::alloc(m, 0x10);
            let a = inverse;
            let b = entity_q;
            let p = mul(add(b[0], b[1]), add(a[0], a[2]));
            let q = mul(sub(b[0], b[1]), sub(a[0], a[2]));
            let r = mul(add(a[3], a[1]), sub(b[3], b[2]));
            let s = mul(sub(a[3], a[1]), add(b[3], b[2]));
            stack::store(m, 0xc, mul(sub(b[3], b[0]), add(a[1], a[2])));
            stack::store(m, 4, p);
            stack::store(m, 0, q);
            stack::store(m, 0x18, r);
            stack::store(m, 0x14, s);
            stack::store(m, 8, sub(p, q));
            for (i, v) in relative_q.into_iter().enumerate() {
                m.w32(relative + 0x10 + i as u32 * 4, v);
            }
        },
    );
    stack::external(
        m,
        0x49a987,
        &[relative + 0x10],
        Some(relative + 0x20),
        |m| {
            stack::alloc(m, 0x1c);
            let [x, y, z, _] = relative_q;
            let xx = add(x, x);
            let yy = add(y, y);
            let zz = add(z, z);
            for (offset, v) in [
                (0, yy),
                (0x20, zz),
                (0x18, mul(xx, x)),
                (4, mul(yy, x)),
                (0xc, mul(zz, x)),
                (0x14, mul(yy, y)),
                (0x10, mul(zz, y)),
                (8, mul(zz, z)),
            ] {
                stack::store(m, offset, v);
            }
            write_matrix(m, relative + 0x20, quaternion_matrix(relative_q));
        },
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_rotated_child_entity_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-entity-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_entity_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rotated_children"].as_array().unwrap() {
            let mut m = Space::new();
            let b = m.alloc(0xa0);
            m.alloc(16);
            let rotation = m.alloc(48);
            m.w32(b + 0x14, 0.5f32.to_bits());
            m.w32(b + 0x9c, 1);
            for i in 0..9 {
                m.w32(
                    rotation + (i / 3) * 16 + (i % 3) * 4,
                    row["matrix"][i as usize].as_u64().unwrap() as u32,
                );
                m.w32(
                    b + 0x18 + (i / 3) * 16 + (i % 3) * 4,
                    row["inertia"][i as usize].as_u64().unwrap() as u32,
                );
            }
            let parent = root(
                &mut m,
                b,
                [1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits()],
                Some(rotation),
            )
            .unwrap();
            let b2 = m.alloc(0xa0);
            m.w32(b2 + 0x14, 0.25f32.to_bits());
            m.w32(b2 + 0x9c, 1);
            for i in 0..9 {
                m.w32(
                    b2 + 0x18 + (i / 3) * 16 + (i % 3) * 4,
                    row["inertia"][i as usize].as_u64().unwrap() as u32,
                );
            }
            m.w32(WORLD + 0x84, 0x640000);
            let e = child(
                &mut m,
                parent,
                b2,
                [4f32.to_bits(), 6f32.to_bits(), 9f32.to_bits()],
                Some(rotation),
            )
            .unwrap();
            for (field, address, count) in [
                ("entity_words", e, 0x218 / 4),
                ("parent_words", parent, 0x218 / 4),
                ("relative_words", m.r32(e + 0x1c), 20),
                ("body_words", b2, 40),
                ("world_words", WORLD + 0x50, 12),
            ] {
                let actual = (0..count)
                    .map(|i| m.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{field}");
            }
        }
    }
    #[test]
    fn original_rotated_root_entity_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-entity-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_entity_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rotations"].as_array().unwrap() {
            let mut m = Space::new();
            let b = m.alloc(0xa0);
            m.alloc(16);
            let rotation = m.alloc(48);
            m.w32(b + 0x14, 0.5f32.to_bits());
            m.w32(b + 0x9c, 1);
            for i in 0..9 {
                m.w32(
                    rotation + (i / 3) * 16 + (i % 3) * 4,
                    row["matrix"][i as usize].as_u64().unwrap() as u32,
                );
                m.w32(
                    b + 0x18 + (i / 3) * 16 + (i % 3) * 4,
                    row["inertia"][i as usize].as_u64().unwrap() as u32,
                );
            }
            let e = root(
                &mut m,
                b,
                [1.25f32.to_bits(), (-2.5f32).to_bits(), 10f32.to_bits()],
                Some(rotation),
            )
            .unwrap();
            for (field, address, count) in [
                ("entity_words", e, 0x218 / 4),
                ("body_words", b, 40),
                ("world_words", WORLD + 0x50, 12),
            ] {
                let actual = (0..count)
                    .map(|i| m.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{field},matrix {}", row["matrix"]);
            }
        }
    }
    #[test]
    fn original_full_compound_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-compound-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_compound_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["full"].as_array().unwrap() {
            let mut m = Space::new();
            let parent = m.alloc(0x218);
            let body = m.alloc(0xa0);
            let definition = m.alloc(0xac);
            m.w32(parent + 0x5c, body);
            m.w32(body + 0x9c, 0x405);
            m.w32(body + 0x78, definition);
            m.w32(definition + 8, 6);
            for base in [0, 0xd4] {
                for offset in [0xa4, 0xb8, 0xcc, 0x104] {
                    m.w32(parent + base + offset, 1f32.to_bits());
                }
            }
            let mut relatives = vec![];
            for child in row["children"].as_array().unwrap() {
                let e = m.alloc(0x218);
                let b = m.alloc(0xa0);
                let rel = m.alloc(0x50);
                let d = m.alloc(0xac);
                relatives.push(rel);
                m.w32(e + 0x20, parent);
                m.w32(e + 0x5c, b);
                m.w32(e + 0x1c, rel);
                m.w32(b + 0x78, d);
                let kind = row["kind"].as_u64().unwrap() as u32;
                m.w32(d + 8, kind);
                m.w32(
                    d + 0x10,
                    if kind == 1 {
                        2f32.to_bits()
                    } else if (2..=4).contains(&kind) {
                        1f32.to_bits()
                    } else {
                        0
                    },
                );
                m.w32(e + 0x28, m.r32(parent + 0x24));
                m.w32(parent + 0x24, e);
                m.w32(b + 0x14, child["inverse_mass"].as_u64().unwrap() as u32);
                for i in 0..3 {
                    let p = child["position"][i as usize].as_u64().unwrap() as u32;
                    m.w32(rel + 4 + i * 4, p);
                    m.w32(e + 0x64 + i * 4, p);
                    m.w32(
                        b + 0x48 + i * 0x14,
                        child["inertia"][i as usize].as_u64().unwrap() as u32,
                    );
                    m.w32(rel + 0x20 + i * 0x14, 1f32.to_bits());
                    m.w32(
                        d + 0x9c + i * 4,
                        child["dimensions"][i as usize].as_u64().unwrap() as u32,
                    );
                }
                for base in [0, 0xd4] {
                    for offset in [0xa4, 0xb8, 0xcc, 0x104] {
                        m.w32(e + base + offset, 1f32.to_bits());
                    }
                }
            }
            let mut cursor = m.r32(parent + 0x24);
            while cursor != 0 {
                let relative = m.r32(cursor + 0x1c);
                for i in 0..9 {
                    let word = row["matrix"][i as usize].as_u64().unwrap() as u32;
                    m.w32(relative + 0x20 + (i / 3) * 16 + (i % 3) * 4, word);
                    for base in [0, 0xd4] {
                        m.w32(cursor + base + 0xa4 + (i / 3) * 16 + (i % 3) * 4, word);
                    }
                }
                cursor = m.r32(cursor + 0x28);
            }
            let ancestor = if row["nested"].as_bool().unwrap() {
                let e = m.alloc(0x218);
                let b = m.alloc(0xa0);
                let d = m.alloc(0xac);
                let relative = m.alloc(0x50);
                m.w32(e + 0x5c, b);
                m.w32(b + 0x78, d);
                m.w32(b + 0x9c, 0x405);
                m.w32(d + 8, 5);
                for base in [0, 0xd4] {
                    for off in [0xa4, 0xb8, 0xcc, 0x104] {
                        m.w32(e + base + off, 1f32.to_bits());
                    }
                }
                m.w32(parent + 0x20, e);
                m.w32(parent + 0x1c, relative);
                m.w32(parent + 0x58, 4);
                m.w32(e + 0x24, parent);
                m.w32(relative, parent);
                for off in [0x1c, 0x20, 0x34, 0x48] {
                    m.w32(relative + off, 1f32.to_bits());
                }
                Some((e, b, d, relative))
            } else {
                None
            };
            compound(&mut m, parent).unwrap();
            for (field, address, count) in [
                ("parent_words", parent, 0x218 / 4),
                ("body_words", body, 40),
                ("definition_words", definition, 43),
                ("vertices", m.r32(definition + 0x24), 24),
            ] {
                let actual = (0..count)
                    .map(|i| m.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{field}, count {}", row["count"]);
            }
            for (index, relative) in relatives.into_iter().enumerate() {
                let actual = (0..20).map(|i| m.r32(relative + i * 4)).collect::<Vec<_>>();
                let expected = row["relative_words"][index]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected);
            }
            if let Some((e, b, d, r)) = ancestor {
                for (field, address, count) in [
                    ("ancestor_words", e, 0x218 / 4),
                    ("ancestor_body_words", b, 40),
                    ("ancestor_definition_words", d, 43),
                    ("ancestor_vertices", m.r32(d + 0x24), 24),
                    ("ancestor_relative_words", r, 20),
                ] {
                    let actual = (0..count)
                        .map(|i| m.r32(address + i * 4))
                        .collect::<Vec<_>>();
                    let expected = row[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u32)
                        .collect::<Vec<_>>();
                    assert_eq!(actual, expected, "{field},count{}", row["count"]);
                }
            }
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
    #[test]
    fn original_compound_mass_phase_matches() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-compound-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_compound_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let parent = m.alloc(0x218);
            let body = m.alloc(0xa0);
            m.w32(parent + 0x5c, body);
            m.w32(body + 0x9c, 0x405);
            for child in row["children"].as_array().unwrap() {
                let e = m.alloc(0x218);
                let b = m.alloc(0xa0);
                let rel = m.alloc(0x50);
                m.w32(e + 0x5c, b);
                m.w32(e + 0x1c, rel);
                m.w32(e + 0x28, m.r32(parent + 0x24));
                m.w32(parent + 0x24, e);
                m.w32(b + 0x14, child["inverse_mass"].as_u64().unwrap() as u32);
                for i in 0..3 {
                    m.w32(
                        rel + 4 + i * 4,
                        child["position"][i as usize].as_u64().unwrap() as u32,
                    );
                    m.w32(
                        b + 0x48 + i * 0x14,
                        child["inertia"][i as usize].as_u64().unwrap() as u32,
                    );
                    m.w32(rel + 0x20 + i * 0x14, 1f32.to_bits());
                }
            }
            let center = compound_mass(&mut m, parent).unwrap();
            let expected_center: Vec<u32> = row["center"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(center.as_slice(), expected_center);
            let actual = (0..40).map(|i| m.r32(body + i * 4)).collect::<Vec<_>>();
            let expected = row["body_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "count {}", row["count"]);
        }
    }
    #[test]
    fn original_compound_child_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-entity-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_entity_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["children"].as_array().unwrap() {
            let mut m = Space::new();
            let parent = m.alloc(0x218);
            let body = m.alloc(0xa0);
            m.alloc(16);
            for (axis, v) in [1f32, 2., 3.].into_iter().enumerate() {
                m.w32(parent + 0x64 + axis as u32 * 4, v.to_bits());
            }
            for offset in [0xa4, 0xb8, 0xcc, 0x104] {
                m.w32(parent + offset, 1f32.to_bits());
            }
            let mass = row["mass"].as_u64().unwrap() as u32;
            m.w32(body + 0x14, mass);
            m.w32(body + 0x9c, if f32::from_bits(mass) == 0. { 5 } else { 1 });
            for offset in [0x18, 0x2c, 0x40] {
                m.w32(body + offset, mass);
            }
            let entity = child(
                &mut m,
                parent,
                body,
                [3f32.to_bits(), 6f32.to_bits(), 9f32.to_bits()],
                None,
            )
            .unwrap();
            assert_eq!(entity, row["child"].as_u64().unwrap() as u32);
            for (field, address, count) in [
                ("entity_words", entity, 0x218 / 4),
                ("parent_words", parent, 0x218 / 4),
                ("relative_words", m.r32(entity + 0x1c), 20),
                ("body_words", body, 40),
                ("world_words", WORLD + 0x50, 12),
            ] {
                let actual = (0..count)
                    .map(|i| m.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{field}");
            }
        }
    }
    #[test]
    fn original_root_entity_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-entity-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_entity_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let b = crate::native_geometry::body(
                &mut m,
                row["mass"].as_u64().unwrap() as u32,
                5,
                [2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits()],
                None,
            )
            .unwrap();
            m.alloc(16);
            let e = root(
                &mut m,
                b,
                std::array::from_fn(|i| row["position"][i].as_u64().unwrap() as u32),
                None,
            )
            .unwrap();
            assert_eq!(e, row["entity"].as_u64().unwrap() as u32);
            for (field, address, count) in [
                ("entity_words", e, 0x218 / 4),
                ("body_words", b, 40),
                ("world_words", WORLD + 0x50, 12),
            ] {
                let actual = (0..count)
                    .map(|i| m.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{field}: mass {}", row["mass"]);
            }
        }
    }
}
