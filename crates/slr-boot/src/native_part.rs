//! Base Part runtime construction from original46eae0. Resource acquisition
//! and physical construction remain explicit collaborators, not fabricated
//! successful constructors.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};

/// Original429010/4290f0 intrusive weak resource-reference assignment. The
/// caller supplies an actual raw resource container, whose id lives at +50.
pub fn bind_reference(m: &mut Space, link: u32, target: u32) {
    let old = m.r32(link + 12);
    if old == target {
        return;
    }
    if old != 0 {
        let previous = m.r32(link);
        let next = m.r32(link + 4);
        if previous == 0 {
            m.w32(old + 0x48, next);
        } else {
            m.w32(previous + 4, next);
        }
        if next != 0 {
            m.w32(next, previous);
        }
    }
    for i in 0..4 {
        m.w32(link + i * 4, 0);
    }
    if target != 0 {
        let next = m.r32(target + 0x48);
        if next != 0 {
            m.w32(next, link);
        }
        m.w32(link + 4, next);
        m.w32(target + 0x48, link);
        m.w32(link + 8, m.r32(target + 0x50));
        m.w32(link + 12, target);
    }
}
/// First phase, ending immediately before46ec72 chooses the physical graph.
/// Original nullable parameter text defaults to zero position/euler/variant.
/// Nonzero Euler angles are bounded until54ecd0 is integrated here.
pub fn prepare(
    m: &mut Space,
    k: u32,
    parent: u32,
    parameters: &str,
    object: u32,
) -> Result<(), String> {
    if m.r32(k + 0x120) == 0 {
        return Err("NativePartBoundary: template postparse has not run".into());
    }
    let input = format!("_\0{parameters}");
    let scan = slr_physics::cfg::scan(input.as_bytes(), "%f,%f,%f,%f,%f,%f,%d");
    bind_reference(m, object, m.r32(parent + 12));
    m.w32(object + 0x114, 1f32.to_bits());
    m.w32(object + 0x118, 1f32.to_bits());
    m.w32(object + 0xac, m.r32(parent + 8));
    let count = m.r32(k + 0xff8);
    let bytes = count
        .checked_mul(16)
        .and_then(|n| n.checked_add(4))
        .ok_or("NativePartBoundary: render array overflow")?;
    let allocation = m.malloc(bytes);
    m.w32(allocation, count);
    m.w32(object + 0x4c, allocation + 4);
    let indices = m.malloc(
        count
            .checked_mul(4)
            .ok_or("NativePartBoundary: render index overflow")?,
    );
    m.w32(object + 0x50, indices);
    m.w32(object + 0x58, scan.d(6).unwrap_or(0) as u32);
    for i in 0..3 {
        m.w32(
            object + 0xfc + i * 4,
            scan.f(i as usize).unwrap_or(0.).to_bits(),
        );
    }
    let orientation = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
        scan.f(3).unwrap_or(0.),
        scan.f(4).unwrap_or(0.),
        scan.f(5).unwrap_or(0.),
    ));
    for i in 0..3 {
        m.w32(
            object + 0x108 + i * 4,
            orientation.m[i as usize * 3 + 1].to_bits(),
        );
    }
    for offset in [0x78, 0x74, 0x70, 0x54] {
        m.w32(object + offset, 0);
    }
    m.w32(object + 0xf0, m.r32(k + 0x11e0));
    Ok(())
}
/// Phase after physical graph creation: configured slots and resource links.
/// Raw node/container reference semantics are retained; resources themselves
/// must already exist and have been materialized by the factory.
pub fn finish(m: &mut Space, k: u32, object: u32) -> Result<(), String> {
    bind_reference(m, object + 0xc0, m.r32(object + 0xc));
    m.w32(object + 0xd0, 0);
    let count = m.r32(k + 0x1150);
    if count != 0 {
        let bytes = count
            .checked_mul(0x68)
            .and_then(|n| n.checked_add(4))
            .ok_or("NativePartBoundary: slot array overflow")?;
        let allocation = m.malloc(bytes);
        m.w32(allocation, count);
        let slots = allocation + 4;
        m.w32(object + 0x78, slots);
        let mut descriptor = m.r32(k + 0x115c);
        for i in 0..count {
            if descriptor == 0 || m.r32(descriptor + 4) == 0 {
                return Err("NativePartBoundary: configured slot list shorter than count".into());
            }
            let slot = slots + i * 0x68;
            //42cec0 twice plus explicit flags/index words46f2d2..46f2d8.
            for offset in (0..=0x28).step_by(4) {
                m.w32(slot + offset, 0);
            }
            bind_reference(m, slot + 0x18, m.r32(object + 0x38));
            m.copy(slot + 0x2c, descriptor + 0xc, 12);
            m.copy(slot + 0x38, descriptor + 0x18, 48);
            descriptor = m.r32(descriptor + 4);
        }
    }
    m.w32(object + 0xb4, 1f32.to_bits());
    m.w32(object + 0xb8, 0);
    m.w32(object + 0xb0, 0);
    let value = X87::from_f32(1.)
        .div(
            X87::from_f32(f32::from_bits(m.r32(k + 0x11dc))),
            Precision::F32,
            Rounding::NearestEven,
        )
        .to_f32(Precision::F32, Rounding::NearestEven);
    m.w32(object + 0xbc, value.to_bits());
    for (dst, src) in [(0x7c, 0x10e8), (0x8c, 0x10f8), (0x9c, 0x1108)] {
        bind_reference(m, object + dst, m.r32(k + src + 12));
    }
    Ok(())
}
/// Complete currently verified no-physical-graph branch. Call prepare/finish
/// around real graph construction for templates with a backbone/click body.
#[cfg(test)]
pub fn initialize(
    m: &mut Space,
    k: u32,
    parent: u32,
    parameters: &str,
    object: u32,
) -> Result<(), String> {
    if (m.r32(k + 0xff0) as i32) >= 0 || m.r32(k + 0x1040) != 0 {
        return Err("NativePartBoundary: configured physical graph requires original49b300/539fe0/49b230/49a050".into());
    }
    prepare(m, k, parent, parameters, object)?;
    finish(m, k, object)
}
/// `FUN_0046e570` head: after `46e320` base defaults the part constructor binds
/// `sound.rpk:11` into `K+0x11a0` with type 6 before the cfg table runs.
pub fn bind_template_sound(m: &mut Space, k: u32, host: &mut dyn slr_physics::cfg::Host) {
    use slr_physics::cfg::{Arg, ExtCall, This};
    let handle = host.call(
        m,
        &ExtCall {
            f: slr_physics::cfg::EXT_LOOKUP,
            this: This::None,
            a: vec![Arg::S("sound.rpk".into()), Arg::U(0xb)],
        },
    );
    host.call(
        m,
        &ExtCall {
            f: slr_physics::cfg::EXT_BIND,
            this: This::Addr(k + 0x11a0),
            a: vec![Arg::U(handle), Arg::U(6), Arg::U(0)],
        },
    );
}

/// Load a part template through its original constructor chain: `46b330`
/// storage, `46e570` (`46e320` defaults, `sound.rpk` bind, the `0x611368` cfg
/// table), then `46e400` slot transform and the `+0x120` increment.
///
/// The part template has no `K+0x2e48` parameter block, so the car postparse
/// stage does not apply; only the shared `46e400` slot-frame transform and the
/// constructor's own finalisation run.
/// `FUN_0046e930`: the native part class's vtable slot 3 (create).
///
/// Allocates the `0x11c` body with the storage constructors `46e930` runs
/// (`4369e0`, the eight `42cec0` Ref/message nodes, `4774a0`), constructs the
/// base through `46eae0`, derives the LOD mode from message 7 on the body's
/// parent reference, selects the render detail window from `K+0xfac/0xfb0`,
/// and runs the render stage `46f9e0`.
pub fn construct<H: PhysicalHost + crate::native_render_lod::Host>(
    m: &mut Space,
    k: u32,
    parent: u32,
    parameters: &str,
    object: u32,
    host: &mut H,
) -> Result<Option<u32>, String> {
    // 46e94e..46e9b5 storage constructors.
    for offset in (0..0x1c).step_by(4) {
        m.w32(object + offset, 0);
    }
    for base in [0x1c, 0x2c, 0x3c, 0x60, 0x7c, 0x8c, 0x9c, 0xc0] {
        for offset in (0..0x10).step_by(4) {
            m.w32(object + base + offset, 0);
        }
    }
    // 4774a0 list at 0xd4.
    for (offset, value) in [
        (0, 0x5f1060),
        (4, 0x5f09ac),
        (8, object + 0xd4 + 0x10),
        (12, 0),
        (0x10, 0x5f09ac),
        (0x14, 0),
        (0x18, object + 0xd4 + 4),
    ] {
        m.w32(object + 0xd4 + offset, value);
    }
    m.w32(object + 0xf4, 0);
    m.w32(object + 0xf8, 0);
    let entity = initialize_phys(m, k, parent, parameters, object, host)?;
    // 48aaf0(body, &ref) binds the local reference to *(body+0xc)+0x14 and
    // returns its id; an unlinked parent leaves the reference zeroed.
    let resource = m.r32(object + 0xc);
    let owner = if resource == 0 {
        0
    } else {
        m.r32(resource + 0x14)
    };
    let reference = m.alloc(16);
    for offset in (0..16).step_by(4) {
        m.w32(reference + offset, 0);
    }
    if owner != 0 {
        bind_reference(m, reference, owner);
    }
    // 426470(&ref, 7, 0): interaction category of the parent.
    let mode = if m.r32(reference + 8) == 0 {
        0
    } else {
        host.reference_query(m, reference, 7, 0)?
    };
    m.w32(object + 0x5c, if mode == 0xb { 2 } else { 3 });
    let index = select_render_lod(m, k, object, host)?;
    crate::native_render_lod::part(m, k, object, index, host)?;
    Ok(entity)
}

/// Original46ea10..46ea90. Resource distance is read from body.Ref.resource,
/// and the returned payload of that same resource receives the chosen range.
/// FCOMP's C0 branch also selects the first threshold on unordered input.
pub fn select_render_lod(
    m: &mut Space,
    k: u32,
    object: u32,
    host: &mut (impl PhysicalHost + ?Sized),
) -> Result<u32, String> {
    let count = m.r32(k + 0xfac) as i32;
    if count <= 0 {
        return Ok(0);
    }
    let resource = m.r32(object + 12);
    let distance = if resource == 0 {
        0.
    } else {
        m.rf(resource + 0x70)
    };
    let mut index = count as u32;
    for i in 0..count as u32 {
        let (status, _) = X87::from_f32(distance).fcom(X87::from_f32(m.rf(k + 0xfb0 + i * 4)));
        if status.c0 {
            index = i;
            break;
        }
    }
    if resource != 0 {
        let payload = host.game_payload(m, resource)?;
        if payload != 0 {
            let lower = if index == 0 {
                m.r32(0x5e73cc)
            } else {
                m.r32(k + 0xfac + index * 4)
            };
            m.w32(
                payload + 0x74,
                X87::from_f32(f32::from_bits(lower))
                    .to_f32(Precision::F32, Rounding::NearestEven)
                    .to_bits(),
            );
            m.w32(payload + 0x78, m.r32(k + 0xfb0 + index * 4));
        }
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_no_physics_part_graphs_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-part-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let k = m.alloc(0x2e98);
            let p = m.alloc(0x120);
            m.alloc(64);
            let o = m.alloc(0x212c);
            m.w32(k + 0x120, 1);
            m.w32(k + 0xff0, u32::MAX);
            m.w32(k + 0xff8, row["count"].as_u64().unwrap() as u32);
            m.w32(k + 0x11dc, 2f32.to_bits());
            m.w32(k + 0x11e0, 0x12345678);
            m.w32(p + 8, 0xffff1234);
            initialize(&mut m, k, p, row["parameters"].as_str().unwrap(), o).unwrap();
            for (field, address, count) in [
                ("words", o, 0x120 / 4),
                (
                    "render_words",
                    m.r32(o + 0x4c) - 4,
                    row["count"].as_u64().unwrap() as u32 * 4 + 1,
                ),
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
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
}

/// Resource identity, live physics instance payloads, and world wake queues are
/// supplied by the factory. This collaborator must execute original semantics;
/// it cannot return an invented success or replay an oracle capture.
pub trait PhysicalHost {
    fn create_physics(
        &mut self,
        m: &mut Space,
        owner: u32,
        entity: u32,
        name: &str,
    ) -> Result<u32, String>;
    fn wake(&mut self, m: &mut Space, entity: u32) -> Result<(), String>;
    fn or_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String>;
    fn or_resource_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String>;
    /// Original426470 on an actual weak resource reference, including script
    /// getInfo. Distinct from a template's direct native vtable query.
    fn reference_query(
        &mut self,
        _m: &mut Space,
        _reference: u32,
        _message: u32,
        _argument: u32,
    ) -> Result<u32, String> {
        Err("NativePartBoundary: original426470 reference query unavailable".into())
    }
    /// Original419860(0xa0000000,0,0,0) on the body's own resource.
    fn game_payload(&mut self, _m: &mut Space, _resource: u32) -> Result<u32, String> {
        Err("NativePartBoundary: original419860 game payload access unavailable".into())
    }
    fn query(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _direct: bool,
        _message: u32,
        _arg: u32,
    ) -> Result<u32, String> {
        Err("NativePartBoundary: original native/weak query collaborator unavailable".into())
    }
    #[allow(clippy::too_many_arguments)] // The original call site passes all eight.
    fn command(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _direct: bool,
        _sender: u32,
        _message: u32,
        _text: &str,
    ) -> Result<(), String> {
        Err("NativePartBoundary: original native/weak command collaborator unavailable".into())
    }
}
/// Original46f768..46f86c native/weak message selection after slot construction.
/// Query7 is dispatched through the actual object's class and can be entirely
/// native. Only the original result5 authorizes add_mslot messages.
pub fn dispatch_slots(
    m: &mut Space,
    k: u32,
    object: u32,
    host: &mut (impl PhysicalHost + ?Sized),
) -> Result<(), String> {
    let count = m.r32(k + 0x1150);
    if count == 0 {
        return Ok(());
    }
    let direct = m.r32(object + 0xc8) == m.r32(object + 8);
    if !direct && m.r32(object + 0xc8) == 0 {
        return Ok(());
    }
    if host.query(m, k, object, direct, 7, 0)? != 5 {
        return Ok(());
    }
    let mut descriptor = m.r32(k + 0x115c);
    if descriptor != 0 && m.r32(descriptor + 4) == 0 {
        descriptor = 0;
    }
    let mut slot = m.r32(object + 0x78);
    for _ in 0..count {
        if descriptor == 0 {
            return Err("NativePartBoundary: dispatched slot list shorter than count".into());
        }
        if m.r32(descriptor + 0xc4) != 0 {
            let text = format!("add_mslot {} {}", slot as i32, descriptor as i32);
            host.command(m, k, object, direct, 0, 16, &text)?;
        }
        descriptor = m.r32(descriptor + 4);
        if descriptor != 0 && m.r32(descriptor + 4) == 0 {
            descriptor = 0;
        }
        slot = slot.wrapping_add(0x68);
    }
    Ok(())
}
fn orientation(parameters: &str) -> [u32; 9] {
    let input = format!("_\0{parameters}");
    let scan = slr_physics::cfg::scan(input.as_bytes(), "%f,%f,%f,%f,%f,%f,%d");
    slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
        scan.f(3).unwrap_or(0.),
        scan.f(4).unwrap_or(0.),
        scan.f(5).unwrap_or(0.),
    ))
    .m
    .map(f32::to_bits)
}
fn translated(
    m: &Space,
    position: [u32; 3],
    rotation: [u32; 9],
    descriptor: u32,
    body: u32,
) -> [u32; 3] {
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let value = |v: u32| X87::from_f32(f32::from_bits(v));
    let local: [u32; 3] = std::array::from_fn(|axis| {
        let center = if body == 0 {
            [0; 3]
        } else {
            let d = m.r32(body + 0x78);
            std::array::from_fn(|i| m.r32(d + 0x40 + i as u32 * 4))
        };
        if body == 0 {
            return m.r32(descriptor + axis as u32 * 4);
        }
        let r = crate::native_entity::read_matrix(m, descriptor + 12);
        let order = if axis == 0 { [2, 1, 0] } else { [2, 0, 1] };
        let product = |i: usize| value(r[axis * 3 + i]).mul(value(center[i]), pc, rc);
        product(order[0])
            .add(product(order[1]), pc, rc)
            .add(product(order[2]), pc, rc)
            .add(value(m.r32(descriptor + axis as u32 * 4)), pc, rc)
            .to_f32(pc, rc)
            .to_bits()
    });
    std::array::from_fn(|axis| {
        value(rotation[axis * 3 + 1])
            .mul(value(local[1]), pc, rc)
            .add(
                value(rotation[axis * 3 + 2]).mul(value(local[2]), pc, rc),
                pc,
                rc,
            )
            .add(
                value(rotation[axis * 3]).mul(value(local[0]), pc, rc),
                pc,
                rc,
            )
            .add(value(position[axis]), pc, rc)
            .to_f32(pc, rc)
            .to_bits()
    })
}
fn attach_physics(
    m: &mut Space,
    object: u32,
    entity: u32,
    name: &str,
    host: &mut dyn PhysicalHost,
    owner_link: bool,
) -> Result<u32, String> {
    let resource = host.create_physics(m, object, entity, name)?;
    if resource == 0 {
        return Err("NativePartBoundary: physics resource factory returned null".into());
    }
    if owner_link {
        bind_reference(m, object + 0x1c, resource);
    }
    bind_reference(m, entity + 0x48, resource);
    if owner_link {
        bind_reference(m, entity + 0x38, m.r32(object + 0xc));
    }
    Ok(resource)
}
/// Original46eae0 including its physical graph, bounded to identity-oriented
/// configured bones/click transforms. The factory supplies real resources and
/// wake semantics; the graph itself is constructed from the live cfg template.
pub fn initialize_phys(
    m: &mut Space,
    k: u32,
    parent: u32,
    parameters: &str,
    object: u32,
    host: &mut dyn PhysicalHost,
) -> Result<Option<u32>, String> {
    use crate::native_stack::{self as stack, Reg};
    const PARAMETERS: u32 = 0x3000_fa00;
    stack::function(m, &[parent, PARAMETERS, object], Some(k), |m| {
        if stack::argument(m, 1) == PARAMETERS {
            for (i, byte) in parameters
                .as_bytes()
                .iter()
                .chain(std::iter::once(&0))
                .enumerate()
            {
                m.w8(PARAMETERS + i as u32, *byte);
            }
        }
        stack::alloc(m, 0x1a8);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Esi);
        stack::set_reg(m, Reg::Ebx, k);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Edi, 0);
        stack::set_reg(m, Reg::Esi, parent);
        stack::set_reg(m, Reg::Ebp, object);
        initialize_phys_inner(m, k, parent, parameters, object, host)
    })
}
#[allow(clippy::too_many_arguments)] // The original frame passes all nine words.
fn root_physics(
    m: &mut Space,
    object: u32,
    body: u32,
    position: [u32; 3],
    position_ptr: u32,
    rotation: u32,
    name: &str,
    return_pc: u32,
    host: &mut dyn PhysicalHost,
) -> Result<u32, String> {
    use crate::native_stack::{self as stack, Reg};
    let name_ptr = if name == "part_comp" {
        0x612240
    } else {
        0x61226c
    };
    stack::set_reg(m, Reg::Esi, object + 0x1c);
    stack::call(
        m,
        return_pc,
        &[object, body, position_ptr, rotation, name_ptr],
        Some(object + 0x1c),
        |m| {
            stack::function(
                m,
                &[object, body, position_ptr, rotation, name_ptr],
                Some(object + 0x1c),
                |m| {
                    stack::push_reg(m, Reg::Ebx);
                    stack::push_reg(m, Reg::Esi);
                    stack::set_reg(m, Reg::Esi, object);
                    stack::set_reg(m, Reg::Ebx, 0);
                    stack::push_reg(m, Reg::Edi);
                    stack::set_reg(m, Reg::Edi, object + 0x1c);
                    stack::push_reg(m, Reg::Ebp);
                    let entity = stack::call(
                        m,
                        0x48a216,
                        &[body, position_ptr, rotation],
                        Some(slr_physics::cfg::PHYSICS_WORLD),
                        |m| crate::native_entity::root(m, body, position, Some(rotation)),
                    )?;
                    stack::set_reg(m, Reg::Ebp, entity);
                    stack::external(m, 0x48a225, &[object, 0, entity, name_ptr], None, |m| {
                        attach_physics(m, object, entity, name, host, true)
                    })?;
                    Ok(entity)
                },
            )
        },
    )
}
fn initialize_phys_inner(
    m: &mut Space,
    k: u32,
    parent: u32,
    parameters: &str,
    object: u32,
    host: &mut dyn PhysicalHost,
) -> Result<Option<u32>, String> {
    use crate::native_stack::{self as stack, Reg};
    let frame = stack::sp(m);
    stack::external(m, 0x46ebb8, &[], Some(frame + 0x70), |m| {
        for i in 0..3 {
            m.w32(frame + 0x70 + i * 4, 0);
        }
    });
    let parameter_address = stack::load(m, 0x1c0);
    let scanner_args = [
        parameter_address,
        0x612228,
        frame + 0x34,
        frame + 0x38,
        frame + 0x3c,
        frame + 0x70,
        frame + 0x74,
        frame + 0x78,
        object + 0x58,
    ];
    stack::call(m, 0x46ebe9, &scanner_args, None, |m| {
        stack::function(m, &scanner_args, None, |m| {
            let arguments = stack::sp(m) + 0xc;
            stack::call(
                m,
                0x551254,
                &[parameter_address, 0x612228, arguments],
                None,
                |m| {
                    stack::function(m, &[parameter_address, 0x612228, arguments], None, |m| {
                        stack::alloc(m, 0x18);
                        stack::push_reg(m, Reg::Ebx);
                        stack::push_reg(m, Reg::Ebp);
                        stack::push_reg(m, Reg::Esi);
                        stack::push_reg(m, Reg::Edi);
                    })
                },
            );
        });
    });
    let count = m.r32(k + 0xfa8);
    let click = m.r32(k + 0x1040);
    prepare(m, k, parent, parameters, object)?;
    if m.r32(k + 0xfac) > 0 {
        host.or_resource_flags(m, object, 0x100)?;
    }
    let position = std::array::from_fn(|axis| m.r32(object + 0xfc + axis as u32 * 4));
    let rotation = orientation(parameters);
    // Stack temporaries in the original have no heap/resource allocation effect.
    let frame = stack::sp(m);
    let position_ptr = frame + 0x34;
    for i in 0..3 {
        m.w32(position_ptr + i * 4, position[i as usize]);
    }
    let matrix = frame + 0x40;
    let child_matrix = frame + 0x80;
    crate::native_entity::write_matrix(m, matrix, rotation);
    let backbone = m.r32(k + 0xff0) as i32;
    let entity = if backbone < 0 {
        if click == 0 {
            None
        } else {
            crate::native_entity::write_matrix(
                m,
                child_matrix,
                crate::native_entity::matrix_product(
                    rotation,
                    crate::native_entity::read_matrix(m, k + 0x100c),
                ),
            );
            let entity = root_physics(
                m,
                object,
                click,
                position,
                position_ptr,
                child_matrix,
                "click",
                0x46f22b,
                host,
            )?;
            bind_reference(m, object + 0x2c, m.r32(object + 0x28));
            bind_reference(m, object + 0x3c, m.r32(object + 0x38));
            if m.r32(entity + 0x58) & 0x80 == 0 {
                let mut root = entity;
                while m.r32(root + 0x58) & 4 != 0 {
                    root = m.r32(root + 0x20);
                }
                stack::call(m, 0x46f271, &[], Some(root), |m| host.wake(m, root))?;
            }
            Some(entity)
        }
    } else {
        let body = stack::call(
            m,
            0x46ec94,
            &[1f32.to_bits(), 6, 0, 0, 0, 0],
            Some(slr_physics::cfg::PHYSICS_WORLD),
            |m| crate::native_geometry::body(m, 1f32.to_bits(), 6, [0; 3], None),
        )?;
        let entity = root_physics(
            m,
            object,
            body,
            position,
            position_ptr,
            matrix,
            "part_comp",
            0x46ecaf,
            host,
        )?;
        if m.r32(entity + 0x58) & 0x80 == 0 {
            let mut root = entity;
            while m.r32(root + 0x58) & 4 != 0 {
                root = m.r32(root + 0x20);
            }
            stack::call(m, 0x46ece3, &[], Some(root), |m| host.wake(m, root))?;
        }
        m.w32(entity + 0x58, m.r32(entity + 0x58) | 8);
        for index in 0..count {
            let record = k + 0x128 + index * 0x74;
            let kind = m.r32(record + 0x3c);
            if !matches!(kind, 1 | 2) {
                continue;
            }
            let body = m.r32(record + 0x40);
            crate::native_entity::write_matrix(
                m,
                child_matrix,
                crate::native_entity::matrix_product(
                    rotation,
                    crate::native_entity::read_matrix(m, record + 12),
                ),
            );
            let child_position = translated(m, position, rotation, record, body);
            let child_pos_ptr = frame + 0x30;
            for i in 0..3 {
                m.w32(child_pos_ptr + i * 4, child_position[i as usize]);
            }
            stack::set_reg(m, Reg::Esi, record + 0x10);
            let child = stack::call(
                m,
                0x46ee39,
                &[entity, body, child_pos_ptr, child_matrix],
                Some(slr_physics::cfg::PHYSICS_WORLD),
                |m| {
                    crate::native_entity::child(m, entity, body, child_position, Some(child_matrix))
                },
            )?;
            stack::set_reg(m, Reg::Edi, child);
            bind_reference(m, child + 0x38, m.r32(object + 0xc));
            if index == backbone as u32 {
                let resource = attach_physics(m, object, child, "bbone", host, false)?;
                bind_reference(m, object + 0x2c, resource);
            }
            if index == m.r32(k + 0xff4) {
                if m.r32(object + 0xf4) != 0 {
                    return Err("NativePartBoundary: replacing retained steering body requires original45fb80".into());
                }
                m.w32(object + 0xf4, child);
                m.w32(object + 0xf8, m.r32(child));
                m.w32(child, object + 0xf4);
                m.w32(child + 4, m.r32(child + 4).wrapping_add(1));
                attach_physics(m, object, child, "steer", host, false)?;
                m.w32(child + 0x58, m.r32(child + 0x58) | 0x8020);
            }
            if click != 0 || m.r32(record + 0x58) & 0x200 != 0 {
                m.w32(child + 0x58, m.r32(child + 0x58) | 0x4000);
            }
            if m.r32(record + 0x58) & 0x100 != 0 {
                m.w32(child + 0x58, m.r32(child + 0x58) | 0x20);
            }
            if kind == 2 {
                m.w32(child + 0x58, m.r32(child + 0x58) | 0x11040);
                host.or_flags(m, object, 0x60)?;
            }
        }
        if click != 0 {
            m.w32(click + 0x9c, m.r32(click + 0x9c) & !4);
            crate::native_entity::write_matrix(
                m,
                child_matrix,
                crate::native_entity::matrix_product(
                    rotation,
                    crate::native_entity::read_matrix(m, k + 0x100c),
                ),
            );
            let child_position = translated(m, position, rotation, k + 0x1000, 0);
            let child_pos_ptr = frame + 0x30;
            for i in 0..3 {
                m.w32(child_pos_ptr + i * 4, child_position[i as usize]);
            }
            let child = stack::call(
                m,
                0x46f0d2,
                &[entity, click, child_pos_ptr, child_matrix],
                Some(slr_physics::cfg::PHYSICS_WORLD),
                |m| {
                    crate::native_entity::child(
                        m,
                        entity,
                        click,
                        child_position,
                        Some(child_matrix),
                    )
                },
            )?;
            let name = if m.r32(object + 0x34) == 0 {
                "cbone"
            } else {
                "click"
            };
            bind_reference(m, child + 0x38, m.r32(object + 0xc));
            let resource = attach_physics(m, object, child, name, host, false)?;
            if name == "cbone" {
                bind_reference(m, object + 0x2c, resource);
            }
        }
        stack::call(
            m,
            0x46f1c7,
            &[entity],
            Some(slr_physics::cfg::PHYSICS_WORLD),
            |m| crate::native_entity::compound(m, entity),
        )?;
        if m.r32(object + 0x34) == 0 {
            bind_reference(m, object + 0x2c, m.r32(object + 0x28));
        }
        Some(entity)
    };
    finish(m, k, object)?;
    host.or_flags(m, object, 0x10295)?;
    dispatch_slots(m, k, object, host)?;
    Ok(entity)
}

#[cfg(test)]
mod physics_tests {
    use super::*;
    struct DeclaredResources {
        calls: Vec<serde_json::Value>,
    }
    impl PhysicalHost for DeclaredResources {
        fn create_physics(
            &mut self,
            m: &mut Space,
            owner: u32,
            entity: u32,
            name: &str,
        ) -> Result<u32, String> {
            let index = self.calls.len() as u32;
            let resource = 0x30020000 + index * 0x1000;
            let payload = resource + 0x100;
            m.w32(resource, 0x30000000);
            m.w32(resource + 0x4c, 1);
            m.w32(resource + 0x50, 0xffff0100 + index);
            m.w32(resource + 0x54, 0x200);
            m.w32(payload + 0x84, entity);
            m.w32(payload + 0x88, m.r32(entity));
            m.w32(entity, payload + 0x84);
            m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
            self.calls.push(
                serde_json::json!({"owner":owner,"entity":entity,"name":name,"resource":resource}),
            );
            Ok(resource)
        }
        fn wake(&mut self, m: &mut Space, entity: u32) -> Result<(), String> {
            crate::native_world::sleep(m, entity)
        }
        fn or_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String> {
            m.w32(object + 0x70, m.r32(object + 0x70) | flags);
            Ok(())
        }
        fn or_resource_flags(
            &mut self,
            m: &mut Space,
            object: u32,
            flags: u32,
        ) -> Result<(), String> {
            let resource = m.r32(object + 0xc);
            m.w32(resource + 0x54, m.r32(resource + 0x54) | flags);
            Ok(())
        }
    }
    #[test]
    fn original_physical_part_graphs_match_with_declared_resource_factory() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-part-physics-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_physics_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            let mut host = DeclaredResources { calls: vec![] };
            initialize_phys(
                &mut m,
                row["k"].as_u64().unwrap() as u32,
                row["parent"].as_u64().unwrap() as u32,
                "0,0,0,0,0,0,7",
                row["object"].as_u64().unwrap() as u32,
                &mut host,
            )
            .unwrap();
            assert_eq!(serde_json::json!(host.calls), row["calls"]);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            let mut ranges = vec![
                (
                    slr_physics::space::HEAP_BASE,
                    m.heap_ptr() - slr_physics::space::HEAP_BASE,
                ),
                (slr_physics::cfg::PHYSICS_WORLD, 0x210),
                (0x30010000, 0x100),
                (0x30001000, 0x4000),
            ];
            for call in host.calls {
                ranges.push((call["resource"].as_u64().unwrap() as u32, 0x200));
            }
            let mut actual = std::collections::BTreeMap::new();
            for (address, bytes) in ranges {
                for p in (address..address + bytes).step_by(4) {
                    let value = m.r32(p);
                    if value != 0 {
                        actual.insert(p, value);
                    }
                }
            }
            let expected = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for address in actual.keys().chain(expected.keys()) {
                assert_eq!(
                    actual.get(address).copied().unwrap_or(0),
                    expected.get(address).copied().unwrap_or(0),
                    "count{} address{address:#x}",
                    row["count"]
                );
            }
        }
    }
}

#[cfg(test)]
mod slot_tests {
    use super::*;
    struct Messages {
        result: u32,
        events: Vec<serde_json::Value>,
    }
    impl PhysicalHost for Messages {
        fn create_physics(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: &str,
        ) -> Result<u32, String> {
            unreachable!()
        }
        fn wake(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn or_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn or_resource_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn query(
            &mut self,
            _: &mut Space,
            k: u32,
            o: u32,
            d: bool,
            msg: u32,
            arg: u32,
        ) -> Result<u32, String> {
            assert!(d);
            self.events
                .push(serde_json::json!({"kind":"query","this":k,"args":[o,msg,arg]}));
            Ok(self.result)
        }
        fn command(
            &mut self,
            _: &mut Space,
            k: u32,
            o: u32,
            d: bool,
            sender: u32,
            msg: u32,
            text: &str,
        ) -> Result<(), String> {
            assert!(d);
            self.events.push(
                serde_json::json!({"kind":"command","this":k,"args":[o,sender,msg],"text":text}),
            );
            Ok(())
        }
    }
    #[test]
    fn original_slot_virtual_message_selection_matches() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-part-slots-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_slots_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let k = row["k"].as_u64().unwrap() as u32;
            let object = row["object"].as_u64().unwrap() as u32;
            let count = row["count"].as_u64().unwrap() as u32;
            let ds = row["descriptors"].as_array().unwrap();
            m.w32(k + 0x1150, count);
            m.w32(k + 0x115c, ds[0].as_u64().unwrap() as u32);
            m.w32(object + 8, 5);
            m.w32(object + 0xc8, 5);
            m.w32(object + 0x78, row["slots"].as_u64().unwrap() as u32);
            for index in 0..count as usize {
                let d = ds[index].as_u64().unwrap() as u32;
                m.w32(d + 4, ds[index + 1].as_u64().unwrap() as u32);
                m.w32(d + 0xc4, (index % 2 + 1) as u32);
            }
            let mut host = Messages {
                result: row["result"].as_u64().unwrap() as u32,
                events: vec![],
            };
            dispatch_slots(&mut m, k, object, &mut host).unwrap();
            assert_eq!(serde_json::json!(host.events), row["events"]);
        }
    }
}

#[cfg(test)]
mod lod_selection_tests {
    use super::*;
    use serde_json::{json, Value};
    struct ResourceAccess {
        loaded: bool,
        payload: u32,
        calls: Vec<Value>,
    }
    impl PhysicalHost for ResourceAccess {
        fn create_physics(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: &str,
        ) -> Result<u32, String> {
            unreachable!()
        }
        fn wake(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn or_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn or_resource_flags(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
            unreachable!()
        }
        fn game_payload(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
            self.calls
                .push(json!({"resource":resource,"args":[0xa0000000u32,0,0,0]}));
            Ok(if self.loaded { self.payload } else { 0 })
        }
    }
    #[test]
    fn original_part_lod_reads_own_resource_and_updates_its_payload() {
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("re/out/boot/native-part-lod-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            assert!(
                std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
                "missing original part LOD vectors"
            );
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_lod_probe.py");
            return;
        };
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["fpu_cw"], 0x7f);
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(
            data["producer_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/oracle/native_part_lod_probe.py")).unwrap()
                )
            )
        );
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 128);
        for (index, row) in rows.iter().enumerate() {
            let mut memory = Space::new();
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                memory.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut host = ResourceAccess {
                loaded: row["loaded"].as_bool().unwrap(),
                payload: row["payload"].as_u64().unwrap() as u32,
                calls: vec![],
            };
            let result = select_render_lod(
                &mut memory,
                row["template"].as_u64().unwrap() as u32,
                row["body"].as_u64().unwrap() as u32,
                &mut host,
            )
            .unwrap();
            assert_eq!(
                result as u64,
                row["result"].as_u64().unwrap(),
                "row{index} LOD"
            );
            assert_eq!(json!(host.calls), row["events"], "row{index} access order");
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    memory.r32(0x20000000 + i as u32 * 4) as u64,
                    v.as_u64().unwrap(),
                    "row{index} word{i}"
                );
            }
        }
    }
}
