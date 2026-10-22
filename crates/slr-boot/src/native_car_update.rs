//! Build-606 chassis update: callback, installed-part fold and live parameters.
use slr_physics::space::Space;
use slr_vm::host::VmHost;
use slr_x87::{Precision, Rounding, X87};

#[derive(Debug, PartialEq, Eq)]
pub struct Update {
    pub script: u32,
    pub parameters: u32,
    pub callback: bool,
}

/// Original448430..448562. Release the native memory lease before entering
/// updatevariables: that script calls natives against this same live body.
pub fn begin(
    m: &mut Space,
    template: u32,
    body: u32,
    mut clone: impl FnMut(&mut Space, u32) -> Result<u32, String>,
    mut suspended: impl FnMut(u32) -> Result<bool, String>,
    mut payload: impl FnMut(&mut Space, u32) -> Result<u32, String>,
) -> Result<Option<Update>, String> {
    m.w32(body + 0x70, m.r32(body + 0x70) & !0x0080_0000);
    if m.r32(body + 0x1fbc) == m.r32(template + 0x2e48) {
        let parameters = clone(m, m.r32(template + 0x2e48))?;
        m.w32(body + 0x1fbc, parameters);
    }
    let parameters = m.r32(body + 0x1fbc);
    let resource = m.r32(body + 0xc);
    if resource == 0 {
        return Ok(None);
    }
    let data = payload(m, resource)?;
    if data == 0 {
        return Ok(None);
    }
    let script = m.r32(data + 0x50);
    let callback = script != 0 && !suspended(script)?;
    if callback {
        let wheels = m.r32(template + 0x1f40) as i32;
        let base = m.r32(body + 0x13e4);
        if wheels > 0 && base == 0 {
            return Err("chassis update: invalid live wheel storage".into());
        }
        for i in 0..wheels.max(0) as u32 {
            let wheel = base + i * 0x2b4;
            for offset in [0x164, 0x168, 0x15c, 0x160, 0x16c, 0x170] {
                m.w32(wheel + offset, 0);
            }
            for offset in (0..0x44).step_by(4) {
                m.w32(wheel + 0x1e8 + offset, m.r32(template + 0x2e50 + offset));
            }
        }
    }
    Ok(Some(Update {
        script,
        parameters,
        callback,
    }))
}

/// Original4479d0: project wheel wear and the overall torque multiplier.
/// Each arithmetic instruction rounds at the live device thread's precision.
/// exe: FUN_004479d0
pub fn rebuild(m: &mut Space, template: u32, body: u32) {
    const PC: Precision = Precision::F32;
    const RC: Rounding = Rounding::NearestEven;
    fn product(a: u32, b: u32) -> u32 {
        X87::from_f32(f32::from_bits(a))
            .mul(X87::from_f32(f32::from_bits(b)), PC, RC)
            .to_f32(PC, RC)
            .to_bits()
    }
    fn wear(value: u32, factor: u32) -> u32 {
        let v = f32::from_bits(value);
        let x = X87::from_f32(v);
        let factor = X87::from_f32(f32::from_bits(factor));
        let attenuation = if v < 1.0 || v.is_nan() {
            X87::from_f32(1.0).sub(
                x.mul(x, PC, RC)
                    .mul(X87::from_f32(f32::from_bits(0x3f19_999a)), PC, RC),
                PC,
                RC,
            )
        } else {
            X87::from_f32(f32::from_bits(0x3dcc_ccd0))
        };
        attenuation.mul(factor, PC, RC).to_f32(PC, RC).to_bits()
    }
    let scale = m.r32(body + 0x1f24);
    // Original CF test also returns for an unordered comparison.
    if f32::from_bits(scale).is_nan() || f32::from_bits(scale) < 0.0 {
        return;
    }
    let count = (m.r32(template + 0x1f40) as i32).max(0) as u32;
    let base = m.r32(body + 0x13e4);
    for i in 0..count {
        let wheel = base + i * 0x2b4;
        let brake = m.r32(body + if i < 2 { 0x1f28 } else { 0x1f2c });
        if f32::from_bits(brake) > 0.0 {
            m.w32(wheel + 0xcc, brake);
            m.w32(wheel + 0xe0, brake);
        }
        let multiplier = m.r32(body + 0x1f14 + i * 4);
        if f32::from_bits(multiplier) > 0.0 {
            m.w32(wheel + 0x64, product(m.r32(wheel + 0x64), multiplier));
            m.w32(wheel + 0x68, product(m.r32(wheel + 0x68), multiplier));
        }
        m.w32(
            wheel + 0x70,
            wear(m.r32(wheel + 0x15c), m.r32(wheel + 0x68)),
        );
        m.w32(
            wheel + 0x6c,
            wear(m.r32(wheel + 0x15c), m.r32(wheel + 0x64)),
        );
    }
    let parameters = m.r32(body + 0x1fbc);
    m.w32(parameters + 0xd0, product(m.r32(parameters + 0xd0), scale));
    m.w32(parameters + 0xd4, product(m.r32(parameters + 0xd4), scale));
}

/// Original script/resource collaborators; parameter arithmetic and graph edits
/// remain in the function port so controlled original runs exercise the same path.
pub trait Host {
    fn field(&mut self, object: u32, name: &str, ty: &str) -> u32;
    fn array(&mut self, array: u32) -> Vec<u32>;
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    fn bind_sound(&mut self, m: &mut Space, slot: u32, id: u32) -> Result<(), String>;
}
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn x(word: u32) -> X87 {
    X87::from_f32(f32::from_bits(word))
}
fn bits(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}
fn attenuation(word: u32) -> X87 {
    let value = f32::from_bits(word);
    if value < 1.0 || value.is_nan() {
        X87::from_f32(1.).sub(
            x(word).mul(x(word), PC, RC).mul(x(0x3f19999a), PC, RC),
            PC,
            RC,
        )
    } else {
        x(0x3dcc_ccd0)
    }
}
/// Clear a weak reference, including its cached ID when already detached.
fn clear_reference(m: &mut Space, link: u32) {
    crate::native_part::bind_reference(m, link, 0);
    m.w32(link + 8, 0);
}
/// exe: FUN_0043c520
pub fn fold_part(m: &mut Space, part: u32, template: u32, body: u32) {
    let kind = m.r32(part + 0xb0);
    let index = kind >> 16;
    let destination = match kind & 0xffff {
        5..=7 if (index as i32) < m.r32(template + 0x1f40) as i32 => {
            m.r32(body + 0x13e4) + index * 0x2b4 + 0x15c + ((kind & 0xffff) - 5) * 8
        }
        8 => body + 0x1e38,
        9 => body + 0x1e40,
        10 => body + 0x1e50,
        12 => body + 0x1e48,
        _ => return,
    };
    m.w32(destination, m.r32(part + 0xb8));
    m.w32(destination + 4, m.r32(part + 0xbc));
    let link = match kind & 0xffff {
        9 => body + 0x203c,
        10 => body + 0x202c,
        12 => body + 0x204c,
        _ => return,
    };
    crate::native_part::bind_reference(m, link, m.r32(part + 0xc));
}
/// exe: FUN_004707d0
fn wheel_slot(m: &Space, template: u32, body: u32, id: u32) -> u32 {
    let mut descriptor = m.r32(template + 0x115c);
    let mut slot = m.r32(body + 0x78);
    while descriptor != 0 && m.r32(descriptor + 4) != 0 {
        if m.r32(descriptor + 0x48) == id {
            return slot;
        }
        descriptor = m.r32(descriptor + 4);
        slot += 0x68;
    }
    0
}
fn allocate_curves(m: &mut Space, parameters: u32, count: u32) {
    let forward = m.malloc(count.wrapping_mul(8));
    let reverse = m.malloc(count.wrapping_mul(8));
    m.w32(parameters, count);
    m.w32(parameters + 4, forward);
    m.w32(parameters + 8, count);
    m.w32(parameters + 12, reverse);
}
fn empty_curves(m: &mut Space, parameters: u32) {
    allocate_curves(m, parameters, 1);
    for base in [m.r32(parameters + 4), m.r32(parameters + 12)] {
        m.w32(base, 0);
        m.w32(base + 4, 0);
    }
}
/// Original448562..448ced, after the real synchronous updatevariables callback.
/// Keep the pre-fold torque source read: installed parts can change the pointer.
/// exe: FUN_00448430
pub fn project(
    m: &mut Space,
    template: u32,
    body: u32,
    update: &Update,
    host: &mut dyn Host,
) -> Result<(), String> {
    let parameters = update.parameters;
    let script = update.script;
    let torque = m.r32(parameters + 0xc18);
    clear_reference(m, body + 0x202c);
    let mut cursor = m.r32(body + 0xdc);
    while cursor != 0 && m.r32(cursor + 4) != 0 {
        let resource = m.r32(cursor + 0x18);
        if resource != 0 {
            let data = host.payload(m, resource)?;
            if data != 0 {
                fold_part(m, m.r32(data + 0x4c), template, body);
            }
        }
        cursor = m.r32(cursor + 4);
    }
    m.w32(parameters + 0xe0, host.field(script, "engine_inertia", "F"));
    m.wf(parameters + 0xd0, 1.);
    m.wf(parameters + 0xd4, 1.);
    for (offset, name) in [
        (0xcc, "starter_torque"),
        (0xf0, "engine_friction_fwd"),
        (0xf4, "engine_friction_rev"),
    ] {
        m.w32(parameters + offset, host.field(script, name, "F"));
    }
    m.w32(
        parameters + 0xec,
        bits(x(host.field(script, "engine_rpm_idle", "F")).mul(x(0x3dd67750), PC, RC)),
    );
    let mass = x(host.field(script, "engine_mass", "F")).mul(x(0x45fa0000), PC, RC);
    let momentum = x(m.r32(body + 0x1e58)).mul(x(m.r32(body + 0x1e64)), PC, RC);
    m.w32(body + 0x1e58, bits(mass));
    m.w32(body + 0x1e64, bits(momentum.div(mass, PC, RC)));
    for (dst, src) in [(0x1dd0, 0xd0), (0x1dd4, 0xd4)] {
        m.w32(
            body + dst,
            bits(attenuation(m.r32(body + 0x1e38)).mul(x(m.r32(parameters + src)), PC, RC)),
        );
    }
    if torque == 0 {
        m.w32(parameters + 0x10, 0);
        empty_curves(m, parameters);
    } else {
        // fild -> fstp rounds the integer RPM step once before each product.
        let step = x(bits(X87::from_i32(m.r32(torque + 0x1d0) as i32)));
        let count = m.r32(torque + 0x1d4);
        if count != m.r32(parameters) {
            allocate_curves(m, parameters, count);
        }
        m.w32(parameters + 0x10, m.r32(torque + 0x1c0));
        let forward = m.r32(torque + 0x1dc);
        let reverse = m.r32(torque + 0x1e0);
        if forward == 0 {
            empty_curves(m, parameters);
        } else {
            for i in 0..(count as i32).max(0) as u32 {
                let rpm = bits(X87::from_i32(i as i32).mul(step, PC, RC));
                let a = m.r32(parameters + 4) + i * 8;
                let b = m.r32(parameters + 12) + i * 8;
                m.w32(a + 4, m.r32(forward + i * 4));
                m.w32(
                    b + 4,
                    m.r32(if reverse == 0 { forward } else { reverse } + i * 4),
                );
                m.w32(a, rpm);
                m.w32(b, rpm);
            }
        }
    }
    let ratio = host.field(script, "ratio", "[F");
    for (i, value) in host.array(ratio).into_iter().take(8).enumerate() {
        m.w32(parameters + 0x20 + i as u32 * 4, value);
    }
    m.w32(parameters + 0x1c, host.field(script, "gears", "I"));
    for (offset, name) in [
        (0x40, "rearend_ratio"),
        (0x14, "maxRPM"),
        (0x18, "RPM_limit"),
        (0xc8, "ClutchF"),
    ] {
        m.w32(parameters + offset, host.field(script, name, "F"));
    }
    let sounds = [
        host.field(script, "SFX_trans_fwd", "I"),
        host.field(script, "SFX_trans_rev", "I"),
        host.field(script, "SFX_ignition", "I"),
    ];
    for (offset, name) in [
        (0x804, "rpm_trans_fwd"),
        (0x808, "rpm_trans_rev"),
        (0x80c, "sfx_starter_rpm"),
    ] {
        m.w32(parameters + offset, host.field(script, name, "F"));
    }
    for (offset, id) in [0x7d4, 0x7e4, 0x7f4].into_iter().zip(sounds) {
        host.bind_sound(m, parameters + offset, id)?;
    }
    m.w32(body + 0x1dcc, 0);
    m.w32(parameters + 0xa38, host.field(script, "tank_nitro", "F"));
    m.w32(
        parameters + 0xa44,
        host.field(script, "consumption_nitro", "F"),
    );
    let capacity = m.r32(parameters + 0xa38);
    // Original CF/ZF test selects capacity for unordered comparisons too.
    if !matches!(
        f32::from_bits(capacity).partial_cmp(&m.rf(body + 0x1ef8)),
        Some(std::cmp::Ordering::Greater)
    ) {
        m.w32(body + 0x1ef8, capacity);
    }
    for i in 0..(m.r32(template + 0x1f40) as i32).max(0) as u32 {
        let slot = wheel_slot(m, template, body, i + 0x65);
        if slot == 0 {
            continue;
        }
        let link = m.r32(body + 0x13e4) + i * 0x2b4 + 0x1d8;
        if m.r32(slot + 8) == 0 {
            clear_reference(m, link);
        } else {
            crate::native_part::bind_reference(m, link, m.r32(slot + 12));
        }
    }
    let center = host.field(script, "drag_center", "java.lang.Vector3");
    for (offset, name) in [(0x8fc, "x"), (0x900, "y"), (0x904, "z")] {
        m.w32(parameters + offset, host.field(center, name, "F"));
    }
    m.w32(parameters + 0x908, host.field(script, "C_drag", "F"));
    Ok(())
}
struct RuntimeHost<'a> {
    vm: &'a mut VmHost,
    config: crate::native_body::ConfigHost,
}
impl Host for RuntimeHost<'_> {
    fn field(&mut self, object: u32, name: &str, ty: &str) -> u32 {
        self.vm
            .read_field(object, name)
            .ok()
            .filter(|v| {
                v.tag.name() == ty
                    || (ty == "java.lang.Vector3"
                        && matches!(v.tag, slr_vm::values::TypeTag::Ref(_)))
            })
            .map_or(0, |v| v.payload)
    }
    fn array(&mut self, array: u32) -> Vec<u32> {
        self.vm
            .array_cells_mut(array)
            .map_or_else(Vec::new, |v| v.iter().map(|v| v.payload).collect())
    }
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        crate::native_assets::access(
            m,
            resource,
            crate::native_resource_load::Access {
                flags: 0x80000000,
                quality: 1f32.to_bits(),
                priority: 0,
                time: 0,
            },
        )
    }
    fn bind_sound(&mut self, m: &mut Space, slot: u32, id: u32) -> Result<(), String> {
        use slr_physics::cfg::{Arg, ExtCall, Host, This};
        self.config.call(
            m,
            &ExtCall {
                f: slr_physics::cfg::EXT_BIND,
                this: This::Addr(slot),
                a: vec![Arg::U(id), Arg::U(6), Arg::U(0)],
            },
        );
        self.config.finish()
    }
}

/// Complete43e320 resource guards before dispatching448430. The VM marshaller
/// supplies its receiver's ResourceRef; both accesses preserve original flags.
/// exe: LAB_0043e320
pub fn resolve_update(
    m: &mut Space,
    reference: u32,
    mut access: impl FnMut(&mut Space, u32, crate::native_resource_load::Access) -> Result<u32, String>,
) -> Result<Option<(u32, u32)>, String> {
    if reference == 0 {
        return Ok(None);
    }
    let resource = m.r32(reference + 12);
    if resource == 0 {
        return Ok(None);
    }
    let args = |flags| crate::native_resource_load::Access {
        flags,
        quality: 0,
        priority: 0,
        time: 0,
    };
    let data = access(m, resource, args(0xa0000000))?;
    if data == 0 {
        return Ok(None);
    }
    let secondary = m.r32(data + 0x44);
    let body = m.r32(data + 0x4c);
    if secondary == 0 {
        return Ok(None);
    }
    let template = access(m, secondary, args(0x20000000))?;
    if template == 0 {
        return Ok(None);
    }
    Ok(Some((m.r32(template + 12), body)))
}

fn run(vm: &mut VmHost, receiver: u32) -> Result<(), String> {
    let container =
        slr_resource::node::with_res(|h| h.node(h.object_node(receiver)).map(|n| n.container))
            .unwrap_or(0);
    if container == 0 {
        return Ok(());
    }
    let target = crate::native_body::with_world_memory(|m| {
        let raw = crate::native_body::raw_resource(m, container)?;
        let reference = m.alloc(16);
        crate::native_part::bind_reference(m, reference, raw);
        let result = resolve_update(m, reference, crate::native_assets::access);
        crate::native_part::bind_reference(m, reference, 0);
        result
    })??;
    let Some((template, body)) = target else {
        return Ok(());
    };
    let update = crate::native_body::with_body(container, |state| {
        if state.template != template || state.body != body {
            return Err("chassis update resolved a different native body".into());
        }
        let mut host = crate::native_body::ConfigHost::new(state.memory.r32(state.template + 0xc));
        let result = begin(
            &mut state.memory,
            state.template,
            state.body,
            |m, source| crate::native_car::clone_parameters(m, source, &mut host),
            |script| match vm.read_field(script, "suspend_update") {
                Ok(value) => Ok(value.tag == slr_vm::values::TypeTag::Int && value.payload != 0),
                Err(slr_vm::host::HostError::MissingMember) => Ok(false),
                Err(error) => Err(format!("chassis update: suspend_update: {error:?}")),
            },
            |m, resource| {
                crate::native_assets::access(
                    m,
                    resource,
                    crate::native_resource_load::Access {
                        flags: 0xa0000000,
                        quality: 0,
                        priority: 0,
                        time: 0,
                    },
                )
            },
        )?;
        host.finish()?;
        state.resource_bindings.extend(host.take_bindings());
        Ok::<_, String>(result)
    })
    .ok_or("chassis update: chassis has no live native body")??;
    let Some(update) = update else { return Ok(()) };
    if !update.callback {
        crate::native_body::with_body(container, |state| {
            rebuild(&mut state.memory, state.template, state.body);
        })
        .ok_or("chassis update: body disappeared before rebuild")?;
        return Ok(());
    }
    if update.callback {
        let class = match vm
            .objects
            .get(
                update
                    .script
                    .checked_sub(0x1000)
                    .ok_or("chassis update: invalid script identity")? as usize,
            )
            .map(|o| &o.class)
        {
            Some(slr_vm::vmclass::ClassRef::Loaded(class)) => *class,
            _ => return Err("chassis update: script class unavailable".into()),
        };
        let name = crate::factory::runtime_name(container)
            .ok_or("chassis update: callback template name unavailable")?;
        crate::sched::run_callback(
            vm,
            &name,
            class,
            update.script,
            "updatevariables",
            &[],
            false,
        )
        .map_err(|(thread, error)| crate::script_stop(vm, &thread, "car updatevariables", error))?;
    }
    crate::native_body::with_body(container, |state| {
        let mut host = RuntimeHost {
            vm,
            config: crate::native_body::ConfigHost::new(state.memory.r32(state.template + 0xc)),
        };
        let result = project(
            &mut state.memory,
            state.template,
            state.body,
            &update,
            &mut host,
        );
        let finish = host.config.finish();
        for (slot, binding) in host.config.take_bindings() {
            if let Some(old) = state.resource_bindings.insert(slot, binding) {
                crate::factory::release_cfg(old);
            }
        }
        result?;
        finish?;
        rebuild(&mut state.memory, state.template, state.body);
        Ok(())
    })
    .ok_or("chassis body disappeared during updatevariables")?
}

pub fn force_update(vm: &mut VmHost, info: &slr_vm::host::CallInfo<'_>) -> Option<u32> {
    let mut args = [0];
    let result = slr_vm::host::marshal_args(info, &mut args)
        .map_err(|e| format!("chassis update: arguments {e:?}"))
        .and_then(|_| run(vm, args[0]));
    if let Err(error) = result {
        vm.native_failure = Some(error);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_update_prefix_matches_all_memory_words() {
        compare_original("car-update-prefix-original.json", 4);
    }
    #[test]
    fn original_suspended_update_completes_with_all_memory_words_equal() {
        compare_original("car-update-suspended-original.json", 24);
    }
    #[test]
    fn original_force_update_resource_guards_and_target_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/car-update-wrapper-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/car_update_wrapper_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["rows"].as_array().unwrap().len(), 9);
        for row in data["rows"].as_array().unwrap() {
            let address = |key: &str| row["addresses"][key].as_u64().unwrap() as u32;
            let case = row["case"].as_u64().unwrap();
            let mut m = Space::new();
            for (key, values) in row["before"].as_object().unwrap() {
                for (i, value) in values.as_array().unwrap().iter().enumerate() {
                    m.w32(address(key) + i as u32 * 4, value.as_u64().unwrap() as u32);
                }
            }
            let mut accesses = Vec::new();
            let result = resolve_update(
                &mut m,
                row["reference"].as_u64().unwrap() as u32,
                |_, raw, args| {
                    assert_eq!([args.quality, args.priority, args.time], [0; 3]);
                    accesses.push(serde_json::json!([raw, args.flags]));
                    Ok(if raw == address("resource") {
                        if case == 2 || case == 3 {
                            0
                        } else {
                            address("data")
                        }
                    } else {
                        assert_eq!(raw, address("template"));
                        if case == 5 || case == 6 {
                            0
                        } else {
                            address("tdata")
                        }
                    })
                },
            )
            .unwrap();
            assert_eq!(serde_json::json!(accesses), row["accesses"], "case {case}");
            let actual: Vec<_> = result
                .into_iter()
                .map(|(template, body)| vec![template, body])
                .collect();
            assert_eq!(serde_json::json!(actual), row["result"], "case {case}");
        }
    }
    #[test]
    fn original_active_update_matches_complete_memory_and_call_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/car-update-active-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/car_update_active_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 48);
        struct FakeHost<'a> {
            row: &'a serde_json::Value,
            calls: &'a std::cell::RefCell<Vec<serde_json::Value>>,
        }
        impl Host for FakeHost<'_> {
            fn field(&mut self, object: u32, name: &str, ty: &str) -> u32 {
                let key = match ty {
                    "I" => "ints",
                    "F" => "floats",
                    _ => "refs",
                };
                let kind = if key == "refs" { "R" } else { ty };
                self.calls
                    .borrow_mut()
                    .push(serde_json::json!(["field", object, name, kind]));
                self.row[key][name].as_u64().unwrap() as u32
            }
            fn array(&mut self, _: u32) -> Vec<u32> {
                self.row["ratios"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect()
            }
            fn payload(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
                Ok(resource + 0xd8)
            }
            fn bind_sound(&mut self, _: &mut Space, slot: u32, id: u32) -> Result<(), String> {
                self.calls
                    .borrow_mut()
                    .push(serde_json::json!(["sound", slot, id]));
                Ok(())
            }
        }
        for row in rows {
            let address = |name: &str| row["addresses"][name].as_u64().unwrap() as u32;
            let template = address("template");
            let body = address("body");
            let mut m = Space::new();
            m.set_heap_ptr(row["heap_start"].as_u64().unwrap() as u32);
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(template + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            let calls = std::cell::RefCell::new(Vec::new());
            let mut host = FakeHost { row, calls: &calls };
            let update = begin(
                &mut m,
                template,
                body,
                |_, source| {
                    calls
                        .borrow_mut()
                        .push(serde_json::json!(["clone", source]));
                    Ok(address("clone"))
                },
                |script| Ok(host.field(script, "suspend_update", "I") != 0),
                |m, resource| crate::native_factory::loaded_game_payload(m, resource),
            )
            .unwrap()
            .unwrap();
            assert!(update.callback);
            calls
                .borrow_mut()
                .push(serde_json::json!(["updatevariables"]));
            m.w32(
                update.parameters + 0xc18,
                row["callback_torque"].as_u64().unwrap() as u32,
            );
            project(&mut m, template, body, &update, &mut host).unwrap();
            rebuild(&mut m, template, body);
            assert_eq!(
                m.heap_ptr() as u64,
                row["heap_end"].as_u64().unwrap(),
                "case {}",
                row["case"]
            );
            for (i, value) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(template + i as u32 * 4),
                    value.as_u64().unwrap() as u32,
                    "case {}, address {:#x}",
                    row["case"],
                    template + i as u32 * 4
                );
            }
            assert_eq!(
                serde_json::json!(calls.into_inner()),
                row["calls"],
                "case {}",
                row["case"]
            );
        }
    }
    fn compare_original(filename: &str, expected_rows: usize) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot")
            .join(filename);
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/car_update_prefix_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            data["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["rows"].as_array().unwrap().len(), expected_rows);
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for (key, values) in row["before"].as_object().unwrap() {
                let base = row["addresses"][key].as_u64().unwrap() as u32;
                for (i, value) in values.as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, value.as_u64().unwrap() as u32);
                }
            }
            let template = row["addresses"]["template"].as_u64().unwrap() as u32;
            let body = row["addresses"]["body"].as_u64().unwrap() as u32;
            let mut calls = Vec::new();
            let result = begin(
                &mut m,
                template,
                body,
                |_, source| {
                    assert_eq!(
                        source,
                        row["addresses"]["parameters"].as_u64().unwrap() as u32
                    );
                    calls.push("clone");
                    Ok(row["addresses"]["clone"].as_u64().unwrap() as u32)
                },
                |script| {
                    assert_eq!(script, row["script"].as_u64().unwrap() as u32);
                    Ok(row["suspended"].as_bool().unwrap())
                },
                |m, resource| crate::native_factory::loaded_game_payload(m, resource),
            )
            .unwrap()
            .unwrap();
            if result.callback {
                calls.push("updatevariables");
            } else {
                rebuild(&mut m, template, body);
            }
            assert_eq!(serde_json::json!(calls), row["calls"]);
            assert_eq!(result.script, row["script"].as_u64().unwrap() as u32);
            for (key, values) in row["after"].as_object().unwrap() {
                let base = row["addresses"][key].as_u64().unwrap() as u32;
                for (i, value) in values.as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(base + i as u32 * 4),
                        value.as_u64().unwrap() as u32,
                        "{key}+{:#x}, wheels={}",
                        i * 4,
                        row["wheels"]
                    );
                }
            }
        }
    }
}
