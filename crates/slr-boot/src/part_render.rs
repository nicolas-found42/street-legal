//! Original Part texture/render changes over live resource references and payloads.
use crate::factory::{self, CfgBinding};
use crate::native_body::NativeBody;
use slr_physics::space::Space;
use slr_resource::node;
use slr_vm::host::{marshal_args, CallInfo, VmHost};

trait RenderHost {
    fn clone_type(&mut self, m: &mut Space, source: u32) -> Result<u32, String>;
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    fn replace(&mut self, m: &mut Space, payload: u32, old: u32, new: u32) -> Result<(), String>;
    fn defer(&mut self, m: &mut Space, resource: u32) -> Result<(), String>;
    fn synchronize_template(&mut self, _m: &Space, _resource: u32) -> Result<(), String> {
        Ok(())
    }
}

fn retarget_active(
    m: &mut Space,
    template: u32,
    body: u32,
    replacement: u32,
    host: &mut dyn RenderHost,
) -> Result<(), String> {
    let count = m.r32(body + 0x54) as i32;
    let indices = m.r32(body + 0x50);
    let refs = m.r32(body + 0x4c);
    for index in 0..count.max(0) as u32 {
        let ordinal = m.r32(indices + index * 4);
        if m.r32(template + 0x178 + ordinal * 0x74) == m.r32(template + 0x1110) {
            let resource = m.r32(refs + index * 16 + 12);
            if resource != 0 {
                let payload = host.payload(m, resource)?;
                if payload != 0 {
                    crate::native_render_resource::bind_template(
                        m,
                        payload,
                        m.r32(replacement + 12),
                    );
                    host.synchronize_template(m, resource)?;
                }
            }
            break;
        }
    }
    Ok(())
}

/// exe: FUN_00467ff0
fn texture(
    m: &mut Space,
    template: u32,
    body: u32,
    target: u32,
    host: &mut dyn RenderHost,
) -> Result<(), String> {
    if m.r32(body + 0x84) == m.r32(target + 8) || m.r32(template + 0x10f0) == 0 {
        return Ok(());
    }
    if m.r32(body + 0xa4) == m.r32(template + 0x1110) {
        let source = m.r32(template + 0x1114);
        let clone = if source != 0 && matches!(m.r32(source + 0x4c), 5 | 7 | 0xd | 0xe) {
            host.clone_type(m, source)?
        } else {
            source
        };
        crate::native_part::bind_reference(m, body + 0x9c, clone);
        retarget_active(m, template, body, body + 0x9c, host)?;
    }
    let resource = m.r32(body + 0xa8);
    if resource != 0 {
        let payload = host.payload(m, resource)?;
        if payload != 0 {
            let current = m.r32(body + 0x84);
            if m.r32(target + 8) != 0
                && current >> 16 == 0xffff
                && current != m.r32(template + 0x10f0)
            {
                host.defer(m, m.r32(body + 0x88))?;
            }
            host.replace(m, payload, body + 0x7c, target)?;
        }
    }
    crate::native_part::bind_reference(m, body + 0x7c, m.r32(target + 12));
    Ok(())
}

/// exe: FUN_004682a0
fn render_type(
    m: &mut Space,
    template: u32,
    body: u32,
    target: u32,
    host: &mut dyn RenderHost,
) -> Result<(), String> {
    if m.r32(body + 0xa4) == m.r32(target + 8) {
        return Ok(());
    }
    retarget_active(m, template, body, target, host)?;
    let resource = m.r32(target + 12);
    if resource != 0 {
        let payload = host.payload(m, resource)?;
        if payload != 0 {
            crate::native_part::bind_reference(m, body + 0x8c, m.r32(payload + 0x18));
        }
    }
    crate::native_part::bind_reference(m, body + 0x9c, m.r32(target + 12));
    Ok(())
}

fn live_id(state: &mut NativeBody, slot: u32) -> u32 {
    state.resource(slot).id
}

struct LiveRender;
impl RenderHost for LiveRender {
    fn clone_type(&mut self, m: &mut Space, source: u32) -> Result<u32, String> {
        crate::native_assets::clone_render_type(m, source)
    }
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        crate::native_assets::access(
            m,
            resource,
            crate::native_resource_load::Access {
                flags: 0xa0000001,
                quality: 1f32.to_bits(),
                priority: 0,
                time: 0,
            },
        )
    }
    fn replace(&mut self, m: &mut Space, payload: u32, old: u32, new: u32) -> Result<(), String> {
        crate::native_render_type::replace_resource(m, payload, old, new)?;
        crate::native_assets::synchronize_bindings(m, payload - 0x80)
    }
    fn defer(&mut self, _m: &mut Space, resource: u32) -> Result<(), String> {
        let container = crate::native_body::logical_resource(resource)
            .ok_or("PartRenderBoundary: deferred texture lacks live identity")?;
        factory::schedule_release(container, true)
    }
    fn synchronize_template(&mut self, m: &Space, resource: u32) -> Result<(), String> {
        let container = crate::native_body::logical_resource(resource)
            .ok_or("PartRenderBoundary: active render lacks live identity")?;
        let template_raw = m.r32(resource + 0xd8 + 0x84);
        let template = if template_raw == 0 {
            0
        } else {
            crate::native_body::logical_resource(template_raw)
                .ok_or("PartRenderBoundary: active render template lacks live identity")?
        };
        factory::synchronize_render_template(container, template)
    }
}

fn change(state: &mut NativeBody, target: CfgBinding, is_texture: bool) -> Result<(), String> {
    let target_raw = crate::native_body::raw_resource(&mut state.memory, target.container)?;
    let reference = state.memory.malloc(16);
    crate::native_part::bind_reference(&mut state.memory, reference, target_raw);
    let outcome = if is_texture {
        texture(
            &mut state.memory,
            state.template,
            state.body,
            reference,
            &mut LiveRender,
        )
    } else {
        render_type(
            &mut state.memory,
            state.template,
            state.body,
            reference,
            &mut LiveRender,
        )
    };
    crate::native_part::bind_reference(&mut state.memory, reference, 0);
    // Preserve logical lifetimes for every raw Part reference changed by the
    // original helper. Rebinding an unchanged raw target is a real no-op.
    for offset in [0x7c, 0x8c, 0x9c] {
        let slot = state.body + offset;
        let id = state.memory.r32(slot + 8);
        if state
            .resource_bindings
            .get(&slot)
            .is_none_or(|binding| binding.id != id)
        {
            state.bind_resource(slot, id, 0, 0)?;
        }
    }
    outcome
}

pub fn set_texture(state: &mut NativeBody, target: CfgBinding) -> Result<(), String> {
    if state.memory.r32(state.body + 0x84) == target.id
        || state.memory.r32(state.template + 0x10f0) == 0
    {
        return Ok(());
    }
    change(state, target, true)
}
pub fn set_render_type(state: &mut NativeBody, target: CfgBinding) -> Result<(), String> {
    if state.memory.r32(state.body + 0xa4) == target.id {
        return Ok(());
    }
    change(state, target, false)
}

pub fn native(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut args = [0; 2];
    if let Err(error) = marshal_args(info, &mut args) {
        vm.native_failure = Some(format!("PartRenderBoundary: {error:?}"));
        return None;
    }
    let container = vm
        .read_field(args[0], "ptr")
        .ok()
        .and_then(|value| {
            node::with_res(|host| {
                host.node(value.payload)
                    .map(|reference| reference.container)
            })
        })
        .unwrap_or(0);
    if container == 0 {
        return Some(if matches!(info.name, "getTexture" | "getRenderType") {
            0
        } else {
            1
        });
    }
    let result = crate::native_body::with_body(container, |state| -> Result<Option<u32>, String> {
        let texture = info.name.ends_with("Texture");
        let slot = state.body + if texture { 0x7c } else { 0x9c };
        let current = live_id(state, slot);
        if info.name.starts_with("get") {
            return Ok(Some(current));
        }
        //469b80/469fa0 bind the input first, but call the helper only when
        // the body's existing corresponding reference has a nonzero ID.
        let target = factory::bind_cfg(
            state.memory.r32(state.template + 0xc),
            args[1],
            if texture { 7 } else { 14 },
            0,
        )?;
        let outcome = if current == 0 {
            Ok(())
        } else if texture {
            set_texture(state, target)
        } else {
            set_render_type(state, target)
        };
        factory::release_cfg(target);
        outcome.map(|()| Some(1))
    })
    .unwrap_or_else(|| {
        Err(format!(
            "PartRenderBoundary: resource {container:#x} has no loaded native body"
        ))
    });
    match result {
        Ok(result) => result,
        Err(error) => {
            vm.native_failure = Some(format!("{}.{}: {error}", info.class, info.name));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_physics::space::Space;
    use std::collections::HashMap;
    struct OriginalCallbacks<'a> {
        row: &'a serde_json::Value,
        calls: Vec<serde_json::Value>,
    }
    impl OriginalCallbacks<'_> {
        fn record(&mut self, name: &str, args: &[u32]) {
            self.calls
                .push(serde_json::json!({"name":name,"args":args}));
        }
    }
    impl RenderHost for OriginalCallbacks<'_> {
        fn clone_type(&mut self, _: &mut Space, source: u32) -> Result<u32, String> {
            self.record("clone", &[source]);
            Ok(self.row["clone"].as_u64().unwrap() as u32)
        }
        fn payload(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
            self.record("payload", &[resource]);
            Ok(if self.row["profile"] == "null_payload" {
                0
            } else {
                resource
                    + if self.row["active"].as_u64() == Some(resource as u64) {
                        0xd8
                    } else {
                        0x80
                    }
            })
        }
        fn replace(
            &mut self,
            _: &mut Space,
            payload: u32,
            old: u32,
            new: u32,
        ) -> Result<(), String> {
            self.record("replace", &[payload, old, new]);
            Ok(())
        }
        fn defer(&mut self, _: &mut Space, resource: u32) -> Result<(), String> {
            self.record("defer", &[resource]);
            Ok(())
        }
    }
    #[test]
    fn original_texture_and_render_changes_match_complete_reference_graphs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-part-render-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_part_render_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let mut host = OriginalCallbacks { row, calls: vec![] };
            let template = row["template"].as_u64().unwrap() as u32;
            let body = row["body"].as_u64().unwrap() as u32;
            let target = row["target"].as_u64().unwrap() as u32;
            if row["operation"] == "texture" {
                texture(&mut m, template, body, target, &mut host)
            } else {
                render_type(&mut m, template, body, target, &mut host)
            }
            .unwrap();
            assert_eq!(
                serde_json::json!(host.calls),
                row["calls"],
                "{} {}",
                row["operation"],
                row["profile"]
            );
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "{} {} word{i}",
                    row["operation"],
                    row["profile"]
                );
            }
        }
    }
    #[test]
    fn setters_return_one_even_with_unbound_receiver() {
        let mut vm = VmHost::new();
        let args = [
            slr_vm::values::Value::reference(0),
            slr_vm::values::Value::int(77),
        ];
        for name in ["setTexture", "setRenderType"] {
            assert_eq!(
                native(
                    &mut vm,
                    &CallInfo {
                        jvm: 0,
                        thread: 0,
                        args: &args,
                        class: "java.game.parts.Part",
                        name,
                        sig: "(I)I"
                    }
                ),
                Some(1)
            );
        }
    }
    fn setup() -> NativeBody {
        factory::configure(None, None);
        let mut memory = Space::new();
        let template = memory.alloc(0x2e98);
        let body = memory.alloc(0x180);
        memory.w32(template + 0xc, 0xffff0001);
        NativeBody {
            memory,
            template,
            body,
            container: 0,
            resource_bindings: HashMap::new(),
        }
    }
    fn resource(kind: i32) -> u32 {
        let container = factory::create_resource(factory::ResourceDescriptor {
            kind,
            flags: 0,
            parent: None,
            template: None,
            name: "part-render-test",
        })
        .unwrap();
        node::with_res(|host| host.container(container).unwrap().id)
    }
    #[test]
    fn texture_default_zero_is_a_real_noop_and_keeps_the_retained_reference() {
        let mut state = setup();
        let old = resource(7);
        let new = resource(7);
        let slot = state.body + 0x7c;
        state.bind_resource(slot, old, 7, 0).unwrap();
        let retained = state.resource(slot);
        let target = factory::bind_cfg(0xffff0001, new, 7, 0).unwrap();
        set_texture(&mut state, target).unwrap();
        factory::release_cfg(target);
        assert_eq!(state.resource(slot), retained);
    }
    #[test]
    fn null_render_replacement_preserves_mesh_and_clears_only_render_reference() {
        let mut state = setup();
        let render = resource(14);
        let mesh = resource(5);
        let render_slot = state.body + 0x9c;
        let mesh_slot = state.body + 0x8c;
        state.bind_resource(render_slot, render, 14, 0).unwrap();
        state.bind_resource(mesh_slot, mesh, 5, 0).unwrap();
        set_render_type(&mut state, CfgBinding::default()).unwrap();
        assert_eq!(state.resource(render_slot).id, 0);
        assert_eq!(state.resource(mesh_slot).id, mesh);
    }
}
