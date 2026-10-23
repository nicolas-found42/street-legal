//! Original instance-reference operations on the resource and physics trees:
//! render-instance access (48aea0), reparenting (48aba0/429c80) and world
//! transform assignment (48af20) with compound propagation (4a3a20).
use crate::native_fpu::Fpu;
use crate::native_install::{first, next, Install};
use crate::native_matrix as matrix;
use slr_physics::space::Space;

/// Original48aea0: the loaded payload of `link`'s resource when it carries a
/// physics/render entity at +84. A loaded payload without one schedules the
/// reference for destruction (427620).
pub fn instance(install: &mut Install<'_>, link: u32) -> Result<u32, String> {
    let m = &*install.m;
    if m.r32(link + 8) == 0 {
        return Ok(0);
    }
    let resource = m.r32(link + 12);
    if resource == 0 {
        return Ok(0);
    }
    let data = install.data(resource, 0xa000_0000, 0)?;
    if data == 0 {
        return Ok(0);
    }
    if install.m.r32(data + 0x84) != 0 {
        return Ok(data);
    }
    if install.m.r32(link + 8) != 0 && install.m.r32(link + 12) != 0 {
        crate::native_message::defer_destroy(install.m, link)?;
    }
    Ok(0)
}

/// Original48bc40: pin the caller's resource during ordered access, then
/// create/find its sorted bone name. Both the pin and scratch scope end even
/// when an unimplemented resource collaborator returns an explicit boundary.
/// exe: FUN_0048bc40
pub fn named_bone(install: &mut Install<'_>, link: u32, name: &str) -> Result<u32, String> {
    let saved = crate::native_stack::sp(install.m);
    let pin = crate::native_install::local_reference(install.m);
    crate::native_install::bind(install.m, pin, install.m.r32(link + 12));
    let result = (|| {
        let resource = install.m.r32(link + 12);
        if resource == 0 {
            return Ok(0);
        }
        let payload = install.data(resource, 0x8000_0000, 1f32.to_bits())?;
        if payload == 0 {
            return Ok(0);
        }
        Ok(crate::native_camera::bone(install.m, payload, name)?.0)
    })();
    crate::native_install::release(install.m, pin);
    crate::native_stack::set_sp(install.m, saved);
    result
}

/// Original48c0f0 with the original5413c0 ordered-id lookup. Null pose
/// pointers retain the matrix while still applying48be10 owner changes.
/// exe: FUN_0048c0f0
pub fn retarget_bone(
    install: &mut Install<'_>,
    link: u32,
    id: u32,
    owner: u32,
    position: u32,
    rotation: u32,
) -> Result<u32, String> {
    if install.m.r32(link + 8) == 0 {
        return Ok(0);
    }
    let resource = install.m.r32(link + 12);
    if resource == 0 {
        return Ok(1);
    }
    let payload = install.data(resource, 0x8000_0000, 1f32.to_bits())?;
    if payload == 0 {
        return Ok(1);
    }
    let mut bone = install.m.r32(payload + 0x12c);
    while bone != 0 && install.m.r32(bone + 4) != 0 {
        let candidate = install.m.r32(bone + 0x34);
        if candidate == id {
            break;
        }
        if candidate.wrapping_sub(id) as i32 >= 0 {
            bone = 0;
            break;
        }
        bone = install.m.r32(bone + 4);
    }
    if bone != 0 && install.m.r32(bone + 4) != 0 {
        let target = crate::native_camera::RenderBone {
            resource,
            body: payload,
            bone,
        };
        if position != 0 && rotation != 0 {
            let position = std::array::from_fn(|i| install.m.r32(position + i as u32 * 4));
            let rotation = crate::native_entity::read_matrix(install.m, rotation);
            crate::native_camera::assign_bone_matrix(install.m, target, position, rotation)?;
        }
        let host = &mut *install.host;
        crate::native_camera::retarget_owner_with(install.m, target, owner, |m, resource| {
            Install { m, host }.data(resource, 0xa000_0000, 1f32.to_bits())
        })?;
    }
    install.m.w32(payload + 0xf8, install.m.r32(0x6200a4));
    Ok(1)
}

/// Original48bc40 followed by48c0f0, used throughout installation.
pub fn named_bone_transform(
    install: &mut Install<'_>,
    link: u32,
    name: &str,
    owner: u32,
    position: u32,
    rotation: u32,
) -> Result<(), String> {
    let id = named_bone(install, link, name)?;
    retarget_bone(install, link, id, owner, position, rotation)?;
    Ok(())
}

/// Original48aaf0: bind `out` to the parent resource of `link`'s resource
/// (or, with `out` zero, return that parent's id).
pub fn parent(m: &mut Space, link: u32, out: u32) -> u32 {
    let resource = m.r32(link + 12);
    if m.r32(link + 8) == 0 || resource == 0 {
        return 0;
    }
    let parent = m.r32(resource + 0x14);
    if out == 0 {
        return m.r32(parent + 0x50);
    }
    if m.r32(out + 12) != parent {
        if m.r32(out + 12) != 0 {
            crate::native_install::unlink(m, out);
        }
        if parent == 0 {
            for offset in [0, 4, 8, 12] {
                m.w32(out + offset, 0);
            }
        } else {
            // 4290f0 on parent+44 links at the head and stores parent.
            let head = m.r32(parent + 0x48);
            if head != 0 {
                m.w32(head, out);
            }
            m.w32(out, 0);
            m.w32(out + 4, head);
            m.w32(parent + 0x48, out);
            m.w32(out + 12, parent);
            let id = m.r32(parent + 0x50);
            m.w32(out + 8, id);
            return id;
        }
    }
    m.r32(out + 8)
}

/// Original545120: re-derive the WT-root reference of every descendant.
pub fn propagate_root(m: &mut Space, resource: u32) {
    let mut child = first(m, resource + 0x18);
    while child != 0 {
        crate::native_resource_base::inherit_spatial_root(m, child, resource);
        propagate_root(m, child);
        child = next(m, child);
    }
}

/// The child-list move shared by 48aba0 (via 4a5d00) and 429c80.
fn attach(m: &mut Space, child: u32, parent: u32) {
    m.w32(child + 0x14, 0);
    let previous = m.r32(child + 8);
    let following = m.r32(child + 4);
    if previous != 0 && following != 0 {
        m.w32(following + 8, previous);
        m.w32(previous + 4, following);
    }
    m.w32(child + 4, 0);
    m.w32(child + 8, 0);
    m.w32(child + 0x14, parent);
    let tail = m.r32(parent + 0x38);
    m.w32(tail + 4, child);
    m.w32(child + 8, tail);
    m.w32(child + 4, parent + 0x30);
    m.w32(parent + 0x38, child);
    if parent != child {
        crate::native_resource_base::inherit_spatial_root(m, child, parent);
        propagate_root(m, child);
    }
}

/// Original429c80 (template vtable+20 for part-like classes): move the
/// body's own resource under `link`'s resource.
pub fn reparent_body(m: &mut Space, body: u32, link: u32) {
    let child = m.r32(body + 0xc);
    if m.r32(link + 8) == 0 || child == 0 {
        return;
    }
    if m.r32(link + 8) == m.r32(m.r32(child + 0x14) + 0x50) {
        return;
    }
    let parent = m.r32(link + 12);
    if parent == 0 {
        return;
    }
    attach(m, child, parent);
}

/// Original48aba0: make `child`'s resource a child of `link`'s resource.
pub fn set_parent(install: &mut Install<'_>, child: u32, link: u32) -> Result<u32, String> {
    let id = install.m.r32(link + 8);
    if id == 0 {
        return Ok(0);
    }
    let own = install.m.r32(child + 12);
    let current = if own == 0 {
        0
    } else {
        install.m.r32(install.m.r32(own + 0x14) + 0x50)
    };
    if id == current {
        return Ok(1);
    }
    let target = install.m.r32(link + 12);
    if own != 0 && (target == 0 || install.m.r32(own + 0xc8) != install.m.r32(target + 0xc8)) {
        return Err("NativeRenderBoundary: original545d20 spatial detach".into());
    }
    let target = install.m.r32(link + 12);
    if target != 0 {
        let wt = install.m.r32(target + 0xcc);
        if wt != 0 {
            install.m.w32(wt + 0x54, install.m.r32(wt + 0x54) | 0x40000);
        }
    }
    let own = install.m.r32(child + 12);
    if own == 0 {
        return Ok(0);
    }
    match install.m.r32(own + 0x4c) {
        1 => {
            let data = install.data(own, 0xa000_0000, 0x3f80_0000)?;
            if data == 0 {
                return Ok(0);
            }
            let secondary = install.m.r32(data + 0x44);
            if secondary != 0 {
                let template = install.data(secondary, 0xa000_0000, 0x3f80_0000)?;
                if template != 0 {
                    let k = install.m.r32(template + 0xc);
                    if k != 0 {
                        let body = install.m.r32(data + 0x4c);
                        match install.m.r32(k) {
                            0x5f0f18 => {
                                return Err(
                                    "NativeRenderBoundary: car template vtable+20 original44c170"
                                        .into(),
                                )
                            }
                            _ => reparent_body(install.m, body, link),
                        }
                    }
                }
            }
            let wt = install.m.r32(target + 0xcc);
            if wt != 0 {
                let data = install.data(wt, 0xa000_0000, 0)?;
                if data != 0
                    && install.m.r32(data + 0x40) == 0x35
                    && install.m.r32(data + 0x4c) != 0
                    && install.m.r32(data + 0x4c) + 0xb4 != 0
                {
                    return Err(
                        "NativeRenderBoundary: original4b3ee0 collision-root relocation".into(),
                    );
                }
            }
            Ok(1)
        }
        2 | 3 => {
            let m = &mut *install.m;
            if m.r32(link + 8) != m.r32(m.r32(own + 0x14) + 0x50) {
                let parent = m.r32(link + 12);
                if parent != 0 {
                    attach(m, own, parent);
                }
            }
            Ok(1)
        }
        _ => Ok(0),
    }
}

/// Original48af20: assign `link`'s instance a world position and rotation
/// (nullable pointers: origin / identity). Compound members are expressed
/// through their parents' relative records; both integration buffers,
/// quaternion, world inertia and descendants are refreshed. Returns whether
/// anything changed.
pub fn set_transform(
    install: &mut Install<'_>,
    link: u32,
    position: u32,
    rotation: u32,
) -> Result<u32, String> {
    if install.m.r32(link + 8) == 0 {
        return Ok(0);
    }
    let resource = install.m.r32(link + 12);
    if resource == 0 {
        return Ok(0);
    }
    let data = install.data(resource, 0xa000_0000, 0)?;
    if data == 0 {
        return Ok(0);
    }
    // push ebp; sub esp,b8; three pushes: frame locals relative to EBP.
    let entry = crate::native_stack::sp(install.m);
    let ebp = entry.wrapping_sub(4);
    let m = &mut *install.m;
    let p = ebp.wrapping_sub(0x18);
    let r = ebp.wrapping_sub(0x58);
    let offset = ebp.wrapping_sub(0xc);
    let t = ebp.wrapping_sub(0x28);
    let changed = ebp.wrapping_sub(0x1c);
    m.w32(changed, 0);
    for i in 0..3 {
        m.w32(
            p + i * 4,
            if position == 0 {
                0
            } else {
                m.r32(position + i * 4)
            },
        );
    }
    if rotation == 0 {
        matrix::identity(m, r);
    } else {
        for word in 0..12 {
            m.w32(r + word * 4, m.r32(rotation + word * 4));
        }
    }
    let mut entity = m.r32(data + 0x84);
    for i in 0..3 {
        m.w32(offset + i * 4, 0);
    }
    // One-argument calls made with three registers pushed below the frame:
    // the callee's entry ESP is below that argument and the return address.
    let call_frame = ebp.wrapping_sub(0xb8 + 0xc + 8);
    while m.r8(entity + 0x58) & 4 != 0 {
        let relative = m.r32(entity + 0x1c);
        let rotation = relative + 0x20;
        matrix::multiply_in_place(m, r, rotation, call_frame);
        matrix::rotate(m, offset, rotation, offset);
        matrix::add_vector(m, offset, relative + 4);
        entity = m.r32(entity + 0x20);
    }
    if entity == 0 {
        return Ok(0);
    }
    let mut fpu = Fpu::new();
    let row = |row: u32, column: u32| r + row * 0x10 + column * 4;
    let mut products = [0u32; 3];
    for (i, product) in products.iter_mut().enumerate() {
        let i = i as u32;
        fpu.fld(m, row(i, 2));
        fpu.fmul(m, offset + 8);
        fpu.fld(m, row(i, 1));
        fpu.fmul(m, offset + 4);
        fpu.faddp(1);
        fpu.fld(m, row(i, 0));
        fpu.fmul(m, offset);
        fpu.faddp(1);
        *product = fpu.fstp(m, t + i * 4);
    }
    for (i, product) in products.into_iter().enumerate() {
        m.w32(offset + i as u32 * 4, product);
    }
    for i in 0..3 {
        fpu.fld(m, p + i * 4);
        fpu.fsub(m, t + i * 4);
        fpu.fstp(m, p + i * 4);
    }
    let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
    let rotation_state = state + 0x40;
    let mut d = matrix::distance(m, r, rotation_state, call_frame);
    let global = 0x64097c;
    m.w32(
        global,
        d.to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven)
            .to_bits(),
    );
    if m.r32(global) == 0xffc0_0000 {
        d = matrix::distance(m, rotation_state, r, call_frame);
        m.w32(
            global,
            d.to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven)
                .to_bits(),
        );
    }
    fpu.fld(m, global);
    if !fpu.fcomp(m, 0x5f1104).test(0x41) {
        matrix::to_quaternion(m, state + 0x94, r, call_frame);
        matrix::from_quaternion(m, rotation_state, state + 0x94, call_frame);
        m.w32(changed, 1);
    }
    for i in [2u32, 1, 0] {
        fpu.fld(m, state + i * 4);
        fpu.fsub(m, p + i * 4);
    }
    // st0 = dx, st1 = dy, st2 = dz: (x*x + y*y) + z*z.
    fpu.fld_st(0);
    fpu.arith_st0(crate::native_fpu::Op::Mul, 1);
    fpu.fld_st(2);
    fpu.arith_st0(crate::native_fpu::Op::Mul, 3);
    fpu.faddp(1);
    fpu.fld_st(3);
    fpu.arith_st0(crate::native_fpu::Op::Mul, 4);
    fpu.faddp(1);
    let moved = !fpu.fcomp(m, 0x5f0ed4).test(0x41);
    if moved {
        for i in 0..3 {
            m.w32(state + i * 4, m.r32(p + i * 4));
        }
        m.w32(changed, 1);
    } else if m.r32(changed) == 0 {
        return Ok(0);
    }
    let definition = m.r32(entity + 0x5c);
    let l88 = ebp.wrapping_sub(0x88);
    let lb8 = ebp.wrapping_sub(0xb8);
    matrix::transpose(m, l88, rotation_state);
    matrix::product(m, lb8, definition + 0x18, l88);
    matrix::product(m, state + 0xa4, rotation_state, lb8);
    let other = entity + 0x64 + (m.r32(entity + 0x20c) ^ 1) * 0xd4;
    copy_state(m, other, state);
    let definition = m.r32(entity + 0x5c);
    matrix::transpose(m, lb8, other + 0x40);
    matrix::product(m, l88, definition + 0x18, lb8);
    matrix::product(m, other + 0xa4, other + 0x40, l88);
    propagate(install, entity)?;
    Ok(install.m.r32(changed))
}

/// Position, quaternion and the twelve rotation words (padding included).
fn copy_state(m: &mut Space, dst: u32, src: u32) {
    for i in 0..3 {
        m.w32(dst + i * 4, m.r32(src + i * 4));
    }
    for i in 0..4 {
        m.w32(dst + 0x94 + i * 4, m.r32(src + 0x94 + i * 4));
    }
    for i in 0..12 {
        m.w32(dst + 0x40 + i * 4, m.r32(src + 0x40 + i * 4));
    }
}

/// The placement step shared by 4a3a20 and 4a7880: world position and
/// orientation of a compound member from its parent and relative record.
fn relative_world(m: &mut Space, entity: u32) -> u32 {
    let parent = m.r32(entity + 0x20);
    let relative = m.r32(entity + 0x1c);
    let parent_state = parent + 0x64 + m.r32(parent + 0x20c) * 0xd4;
    let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
    let mut fpu = Fpu::new();
    let mut world = [0u32; 3];
    for (i, word) in world.iter_mut().enumerate() {
        let row = parent_state + 0x40 + i as u32 * 0x10;
        fpu.fld(m, row + 4);
        fpu.fmul(m, relative + 8);
        fpu.fld(m, row + 8);
        fpu.fmul(m, relative + 0xc);
        fpu.faddp(1);
        fpu.fld(m, relative + 4);
        fpu.fmul(m, row);
        fpu.faddp(1);
        *word = fpu.fstp_bits();
    }
    for (i, word) in world.into_iter().enumerate() {
        m.w32(state + i as u32 * 4, word);
    }
    fpu.fld(m, state);
    fpu.fadd(m, parent_state);
    fpu.fstp(m, state);
    for i in [1u32, 2] {
        fpu.fld(m, parent_state + i * 4);
        fpu.fadd(m, state + i * 4);
        fpu.fstp(m, state + i * 4);
    }
    let quaternion = |m: &Space, p: u32| std::array::from_fn(|i| m.r32(p + i as u32 * 4));
    let product = crate::native_entity::quaternion_product(
        quaternion(m, parent_state + 0x94),
        quaternion(m, relative + 0x10),
    );
    for (i, word) in product.into_iter().enumerate() {
        m.w32(state + 0x94 + i as u32 * 4, word);
    }
    let frame = crate::native_stack::sp(m);
    matrix::from_quaternion(m, state + 0x40, state + 0x94, frame);
    state
}

/// Original4a7880 (entity fastcall): re-place a compound member and all
/// descendants from their relative records.
pub fn relative_tree(m: &mut Space, entity: u32) {
    if m.r8(entity + 0x58) & 4 != 0 {
        relative_world(m, entity);
        let mut child = m.r32(entity + 0x24);
        while child != 0 {
            relative_tree(m, child);
            child = m.r32(child + 0x28);
        }
    }
}

/// Original4a3a20 (world thiscall): rebuild a compound member's world state
/// from its parent and relative record, then recurse and refresh bounds.
pub fn propagate(install: &mut Install<'_>, entity: u32) -> Result<(), String> {
    let m = &mut *install.m;
    if m.r8(entity + 0x58) & 4 != 0 {
        let parent = m.r32(entity + 0x20);
        let parent_state = parent + 0x64 + m.r32(parent + 0x20c) * 0xd4;
        let state = relative_world(m, entity);
        m.w32(state + 0x24, m.r32(parent_state + 0x24));
        let other = entity + 0x64 + (m.r32(entity + 0x20c) ^ 1) * 0xd4;
        copy_state(m, other, state);
    }
    let mut child = install.m.r32(entity + 0x24);
    while child != 0 {
        propagate(install, child)?;
        child = install.m.r32(child + 0x28);
    }
    if install.m.r32(entity + 0x5c) != 0 {
        if install.m.r32(entity + 0x50) != 0 {
            crate::native_spatial::swept_bounds(
                install.m,
                entity,
                &mut SpatialAdapter(install.host),
            )?;
        }
        let resource = install.m.r32(entity + 0x44);
        if resource != 0 {
            crate::native_spatial::aggregate_bounds(install.m, resource)?;
            crate::native_spatial::relocate(
                install.m,
                resource,
                &mut SpatialAdapter(install.host),
            )?;
        }
    }
    Ok(())
}

struct SpatialAdapter<'a>(&'a mut dyn crate::native_install::Host);
impl crate::native_spatial::Host for SpatialAdapter<'_> {
    fn request(&mut self, m: &mut Space, resource: u32, args: [u32; 3]) -> Result<(), String> {
        crate::native_spatial::request_physics(m, resource, args)
    }
}
impl crate::native_spatial::IndexHost for SpatialAdapter<'_> {
    fn access(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
        self.0.prepare(m, resource, flags)
    }
}

#[cfg(test)]
mod bone_oracle_tests {
    use super::*;
    use crate::native_install::{Host, ScriptWords};
    use serde_json::{json, Value};
    struct Callbacks<'a> {
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Callbacks<'_> {
        fn state(&self, m: &Space) -> Vec<u32> {
            let bone = m.r32(self.row["body"].as_u64().unwrap() as u32 + 0x12c);
            if m.r32(bone + 4) == 0 {
                vec![]
            } else {
                [0x4c, 0xac, 0x54, 0x84]
                    .map(|off| m.r32(bone + off))
                    .to_vec()
            }
        }
    }
    impl Host for Callbacks<'_> {
        fn minimum(&mut self, m: &mut Space, resource: u32, quality: u32) -> Result<(), String> {
            self.events
                .push(json!(["minimum", resource, quality, self.state(m)]));
            Ok(())
        }
        fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
            self.events
                .push(json!(["prepare", resource, flags, self.state(m)]));
            Ok(
                if self.row["mode"] == "blocked"
                    || (self.row["mode"] == "blocked-physics"
                        && resource != self.row["resource"].as_u64().unwrap() as u32)
                {
                    0x80000000
                } else {
                    0
                },
            )
        }
        fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
            self.events
                .push(json!(["payload", resource, self.state(m)]));
            Ok(
                if self.row["mode"] == "unavailable"
                    || (self.row["mode"] == "unavailable-physics"
                        && resource != self.row["resource"].as_u64().unwrap() as u32)
                {
                    0
                } else {
                    self.row["resources"][resource.to_string()]
                        .as_u64()
                        .unwrap() as u32
                },
            )
        }
        fn resolve(&mut self, _: &mut Space, id: u32, _: u32, _: u32) -> Result<u32, String> {
            Err(format!("detached native call: resolve {id:#x}"))
        }
        fn query(&mut self, _: &mut Space, _: u32, message: u32, _: u32) -> Result<u32, String> {
            Err(format!("detached native call: query{message}"))
        }
        fn script_query(&mut self, _: &mut Space, _: u32, message: u32) -> Result<u32, String> {
            Err(format!("detached native call: script query{message}"))
        }
        fn command(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            event: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            Err(format!("detached native call: command{event:#x}"))
        }
        fn script(
            &mut self,
            _: &mut Space,
            _: u32,
            method: &str,
            _: ScriptWords<'_>,
        ) -> Result<(), String> {
            Err(format!("detached native call: script {method}"))
        }
        fn deliver(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            event: u32,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            Err(format!("detached native call: deliver{event:#x}"))
        }
        fn vm_class(&mut self, _: &mut Space, name: &str) -> Result<u32, String> {
            Err(format!("detached native call: class {name}"))
        }
        fn vm_object(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            Err("detached native call: object".into())
        }
        fn vm_field(&mut self, _: &mut Space, _: u32, name: &str) -> Result<u32, String> {
            Err(format!("detached native call: field {name}"))
        }
    }
    #[test]
    fn original_named_bone_and_owner_changes_match_complete_buffers() {
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bytes = std::fs::read(root.join("re/out/boot/native-render-bone-original.json"))
            .expect("run re/oracle/native_render_bone_probe.py");
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        for (key, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_render_bone_probe.py"),
        ] {
            assert_eq!(
                data[key].as_str().unwrap(),
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 108);
        for row in rows {
            let n = |key: &str| row[key].as_u64().unwrap() as u32;
            let mut m = Space::new();
            m.w32(0x6200a4, 123);
            for (i, w) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, w.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(n("start"));
            crate::native_stack::set_sp(&mut m, 0x7fefff00);
            let mut host = Callbacks {
                row,
                events: vec![],
            };
            let mut install = Install {
                m: &mut m,
                host: &mut host,
            };
            let id = named_bone(&mut install, n("reference"), "bone00").unwrap();
            assert_eq!(id, n("lookup_id"), "row {row}");
            let pose = row["pose"] == "matrix";
            let result = retarget_bone(
                &mut install,
                n("reference"),
                n("id"),
                n("owner"),
                if pose { n("pos") } else { 0 },
                if pose { n("rot") } else { 0 },
            )
            .unwrap();
            assert_eq!(result, n("result"));
            assert_eq!(m.heap_ptr(), n("end"), "{} {}", row["mode"], row["pose"]);
            assert_eq!(
                host.events,
                row["events"].as_array().unwrap().clone(),
                "{} {}",
                row["mode"],
                row["pose"]
            );
            for (i, w) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    w.as_u64().unwrap() as u32,
                    "kind={} mode={} pose={} address={:#x}",
                    row["kind"],
                    row["mode"],
                    row["pose"],
                    0x20000000 + i as u32 * 4
                );
            }
        }
    }
}
