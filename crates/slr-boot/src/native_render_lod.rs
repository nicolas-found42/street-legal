//! Part46f9e0 and wheel44c1f0 render-instance selection and retargeting.
//! Resource factories and script-visible messages are live collaborators;
//! unsupported paths remain explicit and never yield fabricated render objects.
use slr_physics::space::Space;

pub trait Host {
    fn create_instance(
        &mut self,
        m: &mut Space,
        object: u32,
        source_ref: u32,
        name: &str,
    ) -> Result<u32, String>;
    fn render_body(&mut self, m: &mut Space, resource: u32) -> Result<Option<u32>, String>;
    fn physics_body(&mut self, m: &mut Space, resource: u32) -> Result<Option<u32>, String>;
    fn destroy_instance(&mut self, m: &mut Space, reference: u32) -> Result<(), String>;
    fn resource_flags(
        &mut self,
        m: &mut Space,
        resource: u32,
        clear: u32,
        set: u32,
    ) -> Result<(), String>;
    fn deformable_instance(
        &mut self,
        _m: &mut Space,
        _object: u32,
        _source: u32,
    ) -> Result<u32, String> {
        Err("NativeRenderBoundary: deformable clone requires original48d860/render-type vcall+8/539880".into())
    }
    fn interaction_state(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _direct: bool,
    ) -> Result<i32, String> {
        Err("NativeRenderBoundary: interaction query requires original message7".into())
    }
    fn message(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _direct: bool,
        _text: &str,
    ) -> Result<(), String> {
        Err(
            "NativeRenderBoundary: original template/resource message dispatch is unavailable"
                .into(),
        )
    }
    fn missing_clickdef(&mut self, _id: u32) -> Result<(), String> {
        Err("NativeRenderBoundary: missing clickdef requires original5513b0 diagnostic".into())
    }
    fn linked_bones(
        &mut self,
        _m: &mut Space,
        _template: u32,
        _object: u32,
        _descriptor: u32,
        _reference: u32,
        _direct: bool,
    ) -> Result<(), String> {
        Err(
            "NativeRenderBoundary: linked-slot retargeting requires original471f10/message path"
                .into(),
        )
    }
}
fn eligible(flags: u32, lod: u32) -> bool {
    (flags >> 16) & 15 <= lod && lod <= (flags >> 20) & 15
}
fn body(m: &mut Space, host: &mut dyn Host, reference: u32) -> Result<Option<(u32, u32)>, String> {
    let resource = m.r32(reference + 12);
    if resource == 0 {
        return Ok(None);
    }
    Ok(host
        .render_body(m, resource)?
        .map(|payload| (resource, payload)))
}
fn retarget(
    m: &mut Space,
    host: &mut dyn Host,
    reference: u32,
    bone: u32,
    physics_ref: u32,
    position: [u32; 3],
    matrix: [u32; 9],
) -> Result<(), String> {
    if m.r32(reference + 8) == 0 {
        return Ok(());
    }
    let Some((resource, payload)) = body(m, host, reference)? else {
        return Ok(());
    };
    let physics_resource = m.r32(physics_ref + 12);
    let physics = host
        .physics_body(m, physics_resource)?
        .ok_or("NativeRenderBoundary: bone physics payload unavailable")?;
    crate::native_camera::retarget_matrix(
        m,
        crate::native_camera::RenderBone {
            resource,
            body: payload,
            bone,
        },
        physics_ref,
        physics,
        position,
        matrix,
    )
}
fn find_bone(
    m: &mut Space,
    host: &mut dyn Host,
    reference: u32,
    name: &str,
) -> Result<Option<u32>, String> {
    let Some((_, payload)) = body(m, host, reference)? else {
        return Ok(None);
    };
    Ok(Some(crate::native_camera::bone(m, payload, name)?.1))
}
fn matrix_from_frame(m: &Space, p: u32) -> [u32; 9] {
    crate::native_entity::read_matrix(m, p)
}
fn configured_origin(m: &Space, frame: u32) -> Result<[f32; 3], String> {
    let physical = m.r32(frame + 0x40);
    if physical == 0 {
        return Err("NativeRenderBoundary: configured origin has no body".into());
    }
    let mut origin = [m.rf(frame), m.rf(frame + 4), m.rf(frame + 8)];
    if m.rf(physical + 0x14) > 0. {
        let definition = m.r32(physical + 0x78);
        if definition == 0 {
            return Err("NativeRenderBoundary: massive configured body has no definition".into());
        }
        for (i, v) in origin.iter_mut().enumerate() {
            *v += m.rf(definition + 0x40 + i as u32 * 4);
        }
    }
    Ok(origin)
}
/// Base46f9e0 refresh. Plain configured render paths execute here; special
/// deformable/link/script paths require the host's actual original behavior.
pub fn part(
    m: &mut Space,
    template: u32,
    object: u32,
    lod: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let count = m.r32(template + 0xfa8);
    if count > 32 {
        return Err("NativeRenderBoundary: descriptor count exceeds thirty-two".into());
    }
    let scratch = m.malloc(count * 4);
    for i in 0..count {
        m.w32(scratch + i * 4, 0);
    }
    let references = m.r32(object + 0x4c);
    let indices = m.r32(object + 0x50);
    let active = m.r32(object + 0x54);
    if active > m.r32(template + 0xff8) {
        return Err("NativeRenderBoundary: active render count exceeds capacity".into());
    }
    for index in (0..active).rev() {
        let ordinal = m.r32(indices + index * 4);
        if ordinal >= count {
            return Err("NativeRenderBoundary: active descriptor index outside template".into());
        }
        let flags = m.r32(template + 0x180 + ordinal * 0x74);
        let reference = references + index * 16;
        if !eligible(flags, lod) {
            let last = m.r32(object + 0x54) - 1;
            m.w32(object + 0x54, last);
            if let Some((_, payload)) = body(m, host, reference)? {
                let configured = m.r32(template + 0x178 + ordinal * 0x74);
                if m.r32(payload + 0x80) != configured
                    && m.r32(payload + 0x80) != m.r32(object + 0xa4)
                {
                    host.destroy_instance(m, reference)?;
                }
            }
            host.destroy_instance(m, reference)?;
            if index < last {
                crate::native_part::bind_reference(
                    m,
                    reference,
                    m.r32(references + last * 16 + 12),
                );
                m.w32(indices + index * 4, m.r32(indices + last * 4));
            }
            crate::native_part::bind_reference(m, references + last * 16, 0);
            m.w32(indices + last * 4, u32::MAX);
        } else {
            m.w32(scratch + ordinal * 4, 1);
        }
    }
    let direct = m.r32(object + 0xc8) == m.r32(object + 8);
    let interaction = if direct {
        host.interaction_state(m, template, object, true)?
    } else if m.r32(object + 0xc8) == 0 {
        0
    } else {
        host.interaction_state(m, template, object, false)?
    };
    for ordinal in 0..count {
        let descriptor = template + 0x128 + ordinal * 0x74;
        let flags = m.r32(descriptor + 0x58);
        if m.r32(descriptor + 0x3c) != 3
            || m.r32(scratch + ordinal * 4) != 0
            || !eligible(flags, lod)
            || flags & 0x1000 != 0
        {
            continue;
        }
        let active = m.r32(object + 0x54);
        if active >= m.r32(template + 0xff8) {
            break;
        }
        let reference = references + active * 16;
        let source = if m.r32(template + 0x1110) == m.r32(descriptor + 0x50) {
            object + 0x9c
        } else {
            descriptor + 0x48
        };
        m.w32(indices + active * 4, ordinal);
        m.w32(object + 0x54, active + 1);
        let resource = if source != object + 0x9c && flags & 0x800 != 0 {
            host.deformable_instance(m, object, source)?
        } else {
            host.create_instance(m, object, source, "part_r_o")?
        };
        crate::native_part::bind_reference(m, reference, resource);
        let bone = find_bone(m, host, reference, "bone00")?;
        if let Some((_, payload)) = body(m, host, reference)? {
            for offset in [0xec, 0xf0] {
                m.w32(payload + offset, m.r32(object + 0xac));
            }
            for axis in 0..3 {
                m.w32(payload + 0x17c + axis * 4, m.r32(object + 0x108 + axis * 4));
            }
            // Instruction-order projection is checked independently before the
            // resulting clipping plane participates in renderer acceptance.
            let dot = (m.rf(object + 0xfc) * m.rf(object + 0x108)
                + m.rf(object + 0x100) * m.rf(object + 0x10c))
                + m.rf(object + 0x104) * m.rf(object + 0x110);
            m.wf(payload + 0x188, dot * 10.);
        }
        if let Some(bone) = bone {
            if flags & 0x8000 == 0 {
                let selected = m.r32(template + 0xff0) as i32;
                let frame = if selected < 0 {
                    template + 0x1000
                } else {
                    if selected >= 32 {
                        return Err(
                            "NativeRenderBoundary: selected body index outside template".into()
                        );
                    }
                    template + 0x128 + selected as u32 * 0x74
                };
                if m.r32(frame + 0x40) == 0 {
                    host.missing_clickdef(m.r32(template + 0xc))?;
                } else {
                    let inverse = crate::native_entity::transpose(matrix_from_frame(m, frame + 12));
                    let rotation = crate::native_entity::matrix_inplace_product(
                        inverse,
                        matrix_from_frame(m, descriptor + 12),
                    );
                    let origin = configured_origin(m, frame)?;
                    let neg = origin.map(|v| -v);
                    let mut position = [0; 3];
                    for (axis, component) in position.iter_mut().enumerate() {
                        let a = axis;
                        let product = |i: usize| neg[i] * f32::from_bits(inverse[a * 3 + i]);
                        let order = if axis == 0 { [1, 2, 0] } else { [0, 1, 2] };
                        let projection =
                            (product(order[0]) + product(order[1])) + product(order[2]);
                        *component = (projection + m.rf(descriptor + axis as u32 * 4)).to_bits();
                    }
                    retarget(m, host, reference, bone, object + 0x2c, position, rotation)?;
                    if m.r32(template + 0x11b8) != 0 {
                        for (axis, component) in position.iter_mut().enumerate() {
                            *component = (f32::from_bits(*component)
                                + m.rf(template + 0x11bc + axis as u32 * 4))
                            .to_bits();
                        }
                        if let Some(second) = find_bone(m, host, reference, "bone01")? {
                            retarget(
                                m,
                                host,
                                reference,
                                second,
                                object + 0x2c,
                                position,
                                rotation,
                            )?;
                        }
                    }
                }
            } else if m.r32(object + 0xf4) != 0 {
                let physics_ref = m.r32(object + 0xf4) + 0x48;
                retarget(
                    m,
                    host,
                    reference,
                    bone,
                    physics_ref,
                    std::array::from_fn(|i| m.r32(descriptor + i as u32 * 4)),
                    matrix_from_frame(m, descriptor + 12),
                )?;
            }
        }
        let variants = m.r32(descriptor + 0x5c) as i32;
        if variants > 0 {
            let variant = m.r32(object + 0x58) as i32 % variants;
            if variant < 0 {
                return Err(
                    "NativeRenderBoundary: negative color variant indexes before the array".into(),
                );
            }
            let colors = m.r32(descriptor + 0x60);
            if let Some((_, payload)) = body(m, host, reference)? {
                m.w32(payload + 0xcc, m.r32(colors + variant as u32 * 4));
            }
        }
        if (flags & 0x13f != 0 && flags & 0x40 == 0) || m.r32(object + 0x70) & 0x10000000 != 0 {
            host.resource_flags(m, resource, 0, 0x400000)?;
        }
        if interaction == 5 && flags & 0x13f != 0 {
            host.resource_flags(m, resource, 0x400000, 0)?;
            host.message(
                m,
                template,
                object,
                direct,
                &format!("add_light {reference} {flags}"),
            )?;
        }
        let mut attached = false;
        let slots = m.r32(object + 0x78);
        for i in 0..m.r32(template + 0x1150) {
            if m.r32(slots + i * 0x68 + 0x14) & 1 != 0 {
                attached = true;
                break;
            }
        }
        if (attached || (direct && interaction == 5))
            && flags & 0x4000 != 0
            && m.r32(descriptor + 0x6c) != 0
        {
            host.linked_bones(m, template, object, descriptor, reference, direct)?;
        }
    }
    Ok(())
}
/// Original44c1f0 wheel-render refresh; removal belongs to the base refresh.
pub fn wheels(
    m: &mut Space,
    template: u32,
    object: u32,
    lod: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let count = m.r32(template + 0xfa8);
    if count > 32 {
        return Err("NativeRenderBoundary: descriptor count exceeds thirty-two".into());
    }
    let scratch = m.malloc(count * 4);
    for index in 0..count {
        m.w32(scratch + index * 4, 0);
    }
    let references = m.r32(object + 0x4c);
    let indices = m.r32(object + 0x50);
    for index in (0..m.r32(object + 0x54)).rev() {
        let ordinal = m.r32(indices + index * 4);
        if ordinal >= count {
            return Err("NativeRenderBoundary: active descriptor index outside template".into());
        }
        let flags = m.r32(template + 0x180 + ordinal * 0x74);
        if flags & 0x1000 != 0 && eligible(flags, lod) {
            m.w32(scratch + ordinal * 4, 1);
        }
    }
    for ordinal in 0..count {
        let descriptor = template + 0x128 + ordinal * 0x74;
        let flags = m.r32(descriptor + 0x58);
        if m.r32(descriptor + 0x3c) != 3
            || m.r32(scratch + ordinal * 4) != 0
            || !eligible(flags, lod)
            || flags & 0x1000 == 0
        {
            continue;
        }
        let active = m.r32(object + 0x54);
        if active >= m.r32(template + 0xff8) {
            break;
        }
        let index = flags >> 24;
        if index >= m.r32(template + 0x1f40) {
            return Err("NativeRenderBoundary: wheel render index outside configured array".into());
        }
        let wheel = m.r32(object + 0x13e4) + index * 0x2b4;
        let reference = references + active * 16;
        m.w32(indices + active * 4, ordinal);
        m.w32(object + 0x54, active + 1);
        let resource = host.create_instance(m, object, descriptor + 0x48, "r_wheel")?;
        crate::native_part::bind_reference(m, reference, resource);
        if let Some(bone) = find_bone(m, host, reference, "bone00")? {
            retarget(
                m,
                host,
                reference,
                bone,
                wheel,
                std::array::from_fn(|i| m.r32(descriptor + i as u32 * 4)),
                matrix_from_frame(m, descriptor + 12),
            )?;
        }
        if let Some((_, payload)) = body(m, host, reference)? {
            m.w32(payload + 0xec, m.r32(object + 0xac));
            m.w32(payload + 0xf0, m.r32(object + 0xac));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ControlledHost {
        resource: u32,
        payload: u32,
        physics: u32,
        calls: Vec<serde_json::Value>,
    }
    impl Host for ControlledHost {
        fn create_instance(
            &mut self,
            _: &mut Space,
            object: u32,
            source: u32,
            name: &str,
        ) -> Result<u32, String> {
            self.calls
                .push(serde_json::json!(["create", object, source, name]));
            Ok(self.resource)
        }
        fn render_body(&mut self, _: &mut Space, resource: u32) -> Result<Option<u32>, String> {
            assert_eq!(resource, self.resource);
            Ok(Some(self.payload))
        }
        fn physics_body(&mut self, _: &mut Space, _: u32) -> Result<Option<u32>, String> {
            assert_ne!(
                self.physics, 0,
                "nonretargeting branch requires no physics getter"
            );
            Ok(Some(self.physics))
        }
        fn destroy_instance(&mut self, _: &mut Space, reference: u32) -> Result<(), String> {
            self.calls.push(serde_json::json!(["destroy", reference]));
            Ok(())
        }
        fn resource_flags(
            &mut self,
            m: &mut Space,
            resource: u32,
            clear: u32,
            set: u32,
        ) -> Result<(), String> {
            m.w32(resource + 0x54, (m.r32(resource + 0x54) & !clear) | set);
            Ok(())
        }
    }
    #[test]
    fn original_part_and_wheel_lod_retarget_graphs_colors_flags_and_removal() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-lod-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-render-lod-original");
            return;
        };
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let mut m = Space::new();
            let base = slr_physics::space::HEAP_BASE;
            m.w32(0x6200a4, 123);
            for (i, value) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(base + i as u32 * 4, value.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(row["start"].as_u64().unwrap() as u32);
            let val = |key: &str| row[key].as_u64().unwrap() as u32;
            let mut host = ControlledHost {
                resource: val("resource"),
                payload: val("payload"),
                physics: val("physbody"),
                calls: vec![],
            };
            if row["wheel_index"].is_number() {
                wheels(&mut m, val("k"), val("body"), val("lod"), &mut host).unwrap();
            } else {
                part(&mut m, val("k"), val("body"), val("lod"), &mut host).unwrap();
            }
            assert_eq!(m.heap_ptr() as u64, row["end"].as_u64().unwrap());
            assert_eq!(serde_json::json!(host.calls), row["calls"]);
            for (i, value) in row["after"].as_array().unwrap().iter().enumerate() {
                let address = base + i as u32 * 4;
                assert_eq!(
                    m.r32(address) as u64,
                    value.as_u64().unwrap(),
                    "lod={} address={address:#x}",
                    row["lod"]
                );
            }
        }
    }
}
