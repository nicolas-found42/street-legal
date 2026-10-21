//! Live resource-identity collaborators for configured native constructors.
use crate::{factory, native_body, native_camera, native_car, native_part, native_world};
use slr_physics::{
    cfg::{self, ExtCall, Host},
    space::Space,
};
use slr_resource::node;

/// Original game-instance resource virtual get-data, limited to an already
/// loaded envelope. Loading/constructing the owner is the factory's operation.
fn loaded_game_payload(m: &Space, resource: u32) -> Result<u32, String> {
    if m.r32(resource + 0x4c) != 1 || m.r32(resource + 0x64) & 1 == 0 {
        return Err(
            "NativeResourceBoundary: owner get-data needs loaded original game-instance envelope"
                .into(),
        );
    }
    Ok(resource + 0xd8)
}

fn mark_loaded(m: &mut Space, raw: u32) -> Result<(), String> {
    crate::native_resource_load::mark_loaded(m, raw, 1)?;
    let logical = native_body::logical_resource(raw)
        .ok_or("NativeResourceBoundary: loaded resource has no live identity")?;
    node::with_res(|host| {
        let resource = host
            .container_mut(logical)
            .ok_or("NativeResourceBoundary: loaded resource disappeared")?;
        resource.load_state = m.r32(raw + 0x64);
        Ok(())
    })
}

pub struct RuntimeHost {
    pub config: native_body::ConfigHost,
    settings: std::collections::HashMap<&'static str, u32>,
}
impl RuntimeHost {
    /// Resource-only collaborators do not read camera settings or invoke cfg handlers.
    pub fn for_resources(template_id: u32) -> Self {
        Self {
            config: native_body::ConfigHost::new(template_id),
            settings: Default::default(),
        }
    }
}
impl Host for RuntimeHost {
    fn call(&mut self, m: &mut Space, call: &ExtCall) -> u32 {
        self.config.call(m, call)
    }
}
impl native_part::PhysicalHost for RuntimeHost {
    fn command(
        &mut self,
        m: &mut Space,
        template: u32,
        object: u32,
        direct: bool,
        sender: u32,
        message: u32,
        text: &str,
    ) -> Result<(), String> {
        crate::native_car_command::command(m, template, object, direct, sender, message, text)
    }
    fn query(
        &mut self,
        m: &mut Space,
        template: u32,
        object: u32,
        direct: bool,
        message: u32,
        arg: u32,
    ) -> Result<u32, String> {
        crate::native_message::query(m, template, object, direct, message, arg)
    }
    fn create_physics(
        &mut self,
        m: &mut Space,
        owner: u32,
        entity: u32,
        name: &str,
    ) -> Result<u32, String> {
        let parent = native_body::logical_resource(m.r32(owner + 0xc))
            .ok_or("NativeResourceBoundary: physics owner lacks a live resource")?;
        let logical = factory::create_resource(factory::ResourceDescriptor {
            kind: 2,
            flags: 0x201,
            parent: Some(parent),
            template: None,
            name,
        })?;
        let raw = native_body::raw_resource(m, logical)?;
        mark_loaded(m, raw)?;
        m.w32(raw + 0x68, 1f32.to_bits());
        //53f8b0's envelope is at+d8 (53fcb0);53a0de retains the
        //entity through its+84 link, located at container+15c.
        m.w32(raw + 0x15c, entity);
        m.w32(raw + 0x160, m.r32(entity));
        m.w32(entity, raw + 0x15c);
        m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
        let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
        let definition = m.r32(m.r32(entity + 0x5c) + 0x78);
        for axis in 0..3 {
            m.w32(raw + 0x84 + axis * 4, m.r32(state + axis * 4));
            m.w32(
                raw + 0x90 + axis * 4,
                if definition == 0 {
                    0
                } else {
                    m.r32(definition + 0xc)
                },
            );
        }
        m.w32(raw + 0x80, 0);
        Ok(raw)
    }
    fn wake(&mut self, m: &mut Space, entity: u32) -> Result<(), String> {
        native_world::sleep(m, entity)
    }
    fn or_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String> {
        m.w32(object + 0x70, m.r32(object + 0x70) | flags);
        Ok(())
    }
    fn parent_payload(&mut self, m: &mut Space, reference: u32) -> Result<Option<u32>, String> {
        // 48aaf0: a reference with a bound resource yields that resource's
        // +0x14 payload word; a zero-bound reference yields nothing.
        let resource = m.r32(reference + 12);
        if resource == 0 {
            return Ok(None);
        }
        Ok(Some(m.r32(resource + 0x14)))
    }
    fn or_resource_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String> {
        let raw = m.r32(object + 0xc);
        if raw != 0 {
            let logical = native_body::logical_resource(raw)
                .ok_or("NativeResourceBoundary: resource flag target has no live identity")?;
            let value = node::with_res(|host| {
                let resource = host
                    .container_mut(logical)
                    .ok_or("NativeResourceBoundary: resource disappeared")?;
                resource.flags |= flags;
                Ok::<_, String>(resource.flags)
            })?;
            m.w32(raw + 0x54, value);
        }
        Ok(())
    }
}
impl native_car::CarHost for RuntimeHost {
    fn physics_entity(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        if m.r32(resource + 0x4c) != 2 || m.r32(resource + 0x64) & 1 == 0 {
            return Err(
                "NativeCarBoundary: backbone get-data needs a loaded physics resource".into(),
            );
        }
        let entity = m.r32(resource + 0x15c);
        if entity == 0 {
            return Err("NativeCarBoundary: backbone physics payload has no entity".into());
        }
        Ok(entity)
    }
    fn set_detail_window(
        &mut self,
        m: &mut Space,
        resource: u32,
        lower: u32,
        upper: u32,
    ) -> Result<(), String> {
        let payload = loaded_game_payload(m, resource)?;
        m.w32(payload + 0x74, lower);
        m.w32(payload + 0x78, upper);
        Ok(())
    }
    fn or_native_payload_flags(
        &mut self,
        m: &mut Space,
        resource: u32,
        flags: u32,
    ) -> Result<(), String> {
        let payload = loaded_game_payload(m, resource)?;
        m.w32(payload + 0xc, m.r32(payload + 0xc) | flags);
        Ok(())
    }
    fn refresh_render(
        &mut self,
        m: &mut Space,
        template: u32,
        object: u32,
        lod: u32,
        wheels: bool,
    ) -> Result<(), String> {
        if wheels {
            crate::native_render_lod::wheels(m, template, object, lod, self)
        } else {
            crate::native_render_lod::part(m, template, object, lod, self)
        }
    }
    fn activate(&mut self, m: &mut Space, entity: u32) -> Result<(), String> {
        native_world::activate(m, entity)
    }
    fn attach_parameter(
        &mut self,
        m: &mut Space,
        object: u32,
        destination: u32,
        record: u32,
    ) -> Result<(), String> {
        native_camera::create(
            m,
            destination,
            object + 0x2c,
            native_camera::PosePointers {
                position: record,
                angles: record + 12,
            },
            m.r32(record + 24),
            1,
            self,
        )?;
        Ok(())
    }
}
impl crate::native_shape::Host for RuntimeHost {
    fn request_file(
        &mut self,
        m: &mut Space,
        path: &str,
        callback: u32,
        shape: u32,
    ) -> Result<i32, String> {
        crate::native_file::request_shape(m, path, callback, shape)
    }
}
impl crate::native_mesh_resource::Host for RuntimeHost {
    fn create_shape(
        &mut self,
        m: &mut Space,
        kind: u32,
        asynchronous: u32,
        source: &str,
        skinned: u32,
        resource_id: u32,
        slot: u32,
    ) -> Result<u32, String> {
        if asynchronous == 0 {
            return Err(
                "NativeShapeBoundary: synchronous505960 decode path is not integrated".into(),
            );
        }
        crate::native_shape::request_async(m, kind, source, skinned, resource_id, slot, self)
    }
}
impl native_camera::Host for RuntimeHost {
    fn setting(&mut self, name: &str) -> Result<u32, String> {
        self.settings
            .get(name)
            .copied()
            .ok_or_else(|| format!("NativeCameraBoundary: Config.{name} unavailable"))
    }
    fn body(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        match m.r32(resource + 0x4c) {
            2 | 3 => Ok(resource + 0xd8),
            0xc => Ok(resource + 0x80),
            kind => Err(format!(
                "NativeCameraBoundary: payload for resource kind{kind}"
            )),
        }
    }
    fn create_camera(
        &mut self,
        m: &mut Space,
        parameters: native_camera::Parameters,
    ) -> Result<u32, String> {
        let root = crate::resource_identity::camera_root()?;
        let logical = factory::create_resource(factory::ResourceDescriptor {
            kind: 0xc,
            flags: 2,
            parent: Some(root),
            template: None,
            name: "<rendertype_camera>",
        })?;
        let raw = native_body::raw_resource(m, logical)?;
        parameters.initialize_payload(m, raw + 0x80);
        mark_loaded(m, raw)?;
        Ok(raw)
    }
    fn create_render(
        &mut self,
        m: &mut Space,
        owner: Option<u32>,
        camera: u32,
        name: &str,
    ) -> Result<(u32, u32), String> {
        let parent = match owner {
            Some(owner) => native_body::logical_resource(owner)
                .ok_or("NativeCameraBoundary: render owner has no live identity")?,
            None => factory::game_root()?,
        };
        let template = native_body::logical_resource(camera)
            .ok_or("NativeCameraBoundary: camera type has no live identity")?;
        let logical = factory::create_resource(factory::ResourceDescriptor {
            kind: 3,
            flags: 0x301,
            parent: Some(parent),
            template: Some(template),
            name,
        })?;
        let raw = native_body::raw_resource(m, logical)?;
        let payload = raw + 0xd8;
        crate::native_render_resource::finish_instance(m, raw, camera, self)?;
        crate::native_assets::synchronize_loaded(m, raw)?;
        Ok((raw, payload))
    }
}

impl crate::native_render_resource::Host for RuntimeHost {
    fn mark_loaded(&mut self, m: &mut Space, resource: u32) -> Result<(), String> {
        mark_loaded(m, resource)
    }
    fn mesh_payload(&mut self, m: &mut Space, template: u32) -> Result<u32, String> {
        let args = crate::native_resource_load::Access {
            flags: 0x80000000,
            quality: 1f32.to_bits(),
            priority: 0,
            time: 10f32.to_bits(),
        };
        let source = crate::native_assets::access(m, template, args)?;
        if source == 0 {
            return Ok(0);
        }
        let mesh = m.r32(source + 0x18);
        if mesh == 0 {
            Ok(0)
        } else {
            crate::native_assets::access(m, mesh, args)
        }
    }
}

impl crate::native_render_lod::Host for RuntimeHost {
    fn interaction_state(
        &mut self,
        m: &mut Space,
        template: u32,
        object: u32,
        direct: bool,
    ) -> Result<i32, String> {
        crate::native_message::query(m, template, object, direct, 7, 0).map(|value| value as i32)
    }
    fn create_instance(
        &mut self,
        m: &mut Space,
        object: u32,
        source_ref: u32,
        name: &str,
    ) -> Result<u32, String> {
        let owner = m.r32(object + 0xc);
        let source = m.r32(source_ref + 12);
        if source == 0 {
            return Err("NativeRenderBoundary: render source has no live resource".into());
        }
        <Self as native_camera::Host>::create_render(
            self,
            m,
            if owner == 0 { None } else { Some(owner) },
            source,
            name,
        )
        .map(|(resource, _)| resource)
    }
    fn render_body(&mut self, m: &mut Space, resource: u32) -> Result<Option<u32>, String> {
        if resource == 0 {
            return Ok(None);
        }
        if m.r32(resource + 0x4c) != 3 {
            return Err("NativeRenderBoundary: render instance has wrong resource kind".into());
        }
        if m.r32(resource + 0x64) & 1 == 0 {
            return Err(
                "NativeRenderBoundary: render instance needs original load/get-data path".into(),
            );
        }
        Ok(Some(resource + 0xd8))
    }
    fn physics_body(&mut self, m: &mut Space, resource: u32) -> Result<Option<u32>, String> {
        if resource == 0 {
            return Ok(None);
        }
        if m.r32(resource + 0x4c) != 2 {
            return Err("NativeRenderBoundary: physics instance has wrong resource kind".into());
        }
        if m.r32(resource + 0x64) & 1 == 0 {
            return Err(
                "NativeRenderBoundary: physics instance needs original load/get-data path".into(),
            );
        }
        Ok(Some(resource + 0xd8))
    }
    fn destroy_instance(&mut self, m: &mut Space, reference: u32) -> Result<(), String> {
        let raw = m.r32(reference + 12);
        if m.r32(reference + 8) == 0 || raw == 0 {
            return Ok(());
        }
        if m.r32(raw + 0x4c) == 1 {
            loaded_game_payload(m, raw)?;
        }
        let logical = native_body::logical_resource(raw)
            .ok_or("NativeRenderBoundary: deferred release target lacks live identity")?;
        //48a8d0 ->427620: retain a separate weak queue node. The caller's
        //reference remains bound until its own subsequent rebind/clear.
        //This projects the live queue's identity/order; its raw GameLogic list
        //is not yet represented in Space and has no full-heap parity credit.
        factory::schedule_release(logical, true)
    }
    fn resource_flags(
        &mut self,
        m: &mut Space,
        resource: u32,
        clear: u32,
        set: u32,
    ) -> Result<(), String> {
        let logical = native_body::logical_resource(resource)
            .ok_or("NativeRenderBoundary: flag target has no live resource identity")?;
        let flags = node::with_res(|host| {
            let resource = host
                .container_mut(logical)
                .ok_or("NativeRenderBoundary: flag target disappeared")?;
            resource.flags = (resource.flags & !clear) | set;
            Ok::<_, String>(resource.flags)
        })?;
        m.w32(resource + 0x54, flags);
        Ok(())
    }
}

/// Begin actual native runtime construction. Keep this nonpublishable until
/// all remaining constructor stages and their callbacks have executed.
pub fn begin_car(
    vm: &mut slr_vm::host::VmHost,
    mut state: native_body::NativeBody,
    container: u32,
    parameters: &str,
) -> Result<(), String> {
    state.container = container;
    let raw = native_body::raw_resource(&mut state.memory, container)?;
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: "native-car-enter".into(),
        fields: vec![
            ("generation".into(), state.memory.r32(0x6200a4).to_string()),
            (
                "template".into(),
                format!("{:#010x}", state.memory.r32(state.template + 0xc)),
            ),
            ("container".into(), container.to_string()),
        ],
    });
    //53a2a0 retains the requested template before calling its native class.
    let template_resource = state.memory.r32(state.template + 0x10);
    native_part::bind_reference(&mut state.memory, raw + 0x110, template_resource);
    mark_loaded(&mut state.memory, raw)?;
    let reference = state.memory.alloc(16);
    native_part::bind_reference(&mut state.memory, reference, raw);
    state.body = state.memory.alloc(0x212c);
    let config = crate::gc_runtime::config_script()
        .ok_or("NativeCameraBoundary: Config root unavailable")?;
    let mut settings = std::collections::HashMap::new();
    for name in [
        "camera_int_clipplane",
        "camera_int_viewrange",
        "camera_ext_clipplane",
        "camera_ext_viewrange",
    ] {
        if let Ok(value) = vm.read_field(config, name) {
            settings.insert(name, value.payload);
        }
    }
    let mut host = RuntimeHost {
        config: native_body::ConfigHost::new(state.memory.r32(state.template + 0xc)),
        settings,
    };
    let stage = native_car::begin_runtime(
        &mut state.memory,
        state.template,
        reference,
        parameters,
        state.body,
        cfg::PHYSICS_WORLD,
        &mut host,
    )?;
    native_car::finish_runtime(
        &mut state.memory,
        state.template,
        state.body,
        cfg::PHYSICS_WORLD,
        &stage,
        &mut host,
    )?;
    host.config.finish()?;
    state.resource_bindings.extend(host.config.take_bindings());
    //53a2a0 stores the native result and script only after the constructor
    //returns. The script's externally visible resource link is installed by
    //the caller after callback registration.
    state.memory.w32(raw + 0x124, state.body);
    let script = node::with_res(|host| {
        host.container(container)
            .and_then(|resource| resource.instance.as_ref())
            .map(|instance| instance.script)
    })
    .ok_or("NativeResourceBoundary: constructed car has no script envelope")?;
    state.memory.w32(raw + 0x128, script);
    crate::wheel::bind_native_body(container, &state.memory, state.body, state.template)?;
    native_part::bind_reference(&mut state.memory, reference, 0);
    native_body::insert(state);
    Ok(())
}

/// Begin actual native **part** runtime construction from `FUN_0046e930`.
///
/// `46e930` allocates the `0x11c` part body, runs `4369e0` + the eight `42cec0`
/// Ref/message constructors + `4774a0`, calls the base constructor `46eae0`
/// with (parent Ref, parameters, body), derives the LOD mode via message 7,
/// picks the render detail window from `K+0xfac/0xfb0`, calls the render stage
/// `46f9e0`, and finally stores the body into the caller's result reference.
///
/// `owner` is the owning instance's raw resource: the original passes the
/// parent Ref whose `+0xc` is that resource, which `46eae0` binds at `body+0x0c`
/// and which `46e930` also hands to `48aaf0` for the part's own reference.
pub fn begin_part(
    vm: &mut slr_vm::host::VmHost,
    mut state: native_body::NativeBody,
    container: u32,
    parameters: &str,
) -> Result<(), String> {
    state.container = container;
    let raw = native_body::raw_resource(&mut state.memory, container)?;
    let template_resource = state.memory.r32(state.template + 0x10);
    native_part::bind_reference(&mut state.memory, raw + 0x110, template_resource);
    mark_loaded(&mut state.memory, raw)?;
    // The owning instance is the container's factory parent. `53a2a0` passed
    // the caller's parent reference straight through, and for a part created
    // by `Part.addPart` that parent is the car instance's raw resource.
    let owner = node::with_res(|host| {
        host.container(container)
            .and_then(|resource| (resource.parent != 0).then_some(resource.parent))
    })
    .unwrap_or(0);
    let parent = if owner != 0 {
        native_body::raw_resource(&mut state.memory, owner)?
    } else {
        raw
    };
    let reference = state.memory.alloc(16);
    native_part::bind_reference(&mut state.memory, reference, parent);
    state.body = state.memory.alloc(0x11c);
    let mut host = RuntimeHost::for_resources(state.memory.r32(state.template + 0xc));
    let entity = crate::native_part::construct(
        &mut state.memory,
        state.template,
        reference,
        parameters,
        state.body,
        &mut host,
    )?;
    let _ = entity;
    host.config.finish()?;
    state.resource_bindings.extend(host.config.take_bindings());
    state.memory.w32(raw + 0x124, state.body);
    let script = node::with_res(|host| {
        host.container(container)
            .and_then(|resource| resource.instance.as_ref())
            .map(|instance| instance.script)
    })
    .ok_or("NativeResourceBoundary: constructed part has no script envelope")?;
    state.memory.w32(raw + 0x128, script);
    let _ = vm;
    native_part::bind_reference(&mut state.memory, reference, 0);
    native_body::insert(state);
    Ok(())
}

#[cfg(test)]
mod release_tests {
    use super::*;
    use crate::native_render_lod::Host as _;

    #[test]
    fn deferred_instance_release_destroys_and_invalidates_live_aliases() {
        factory::configure(None, None);
        let resource = factory::create_resource(factory::ResourceDescriptor {
            kind: 3,
            flags: 0,
            parent: None,
            template: None,
            name: "deferred-render-instance",
        })
        .unwrap();
        let (id, alias) = node::with_res(|host| {
            let id = host.container(resource).unwrap().id;
            (id, host.add_bound_node(id, resource))
        });
        native_body::with_world_memory(|memory| {
            let raw = native_body::raw_resource(memory, resource).unwrap();
            let reference = memory.alloc(16);
            native_part::bind_reference(memory, reference, raw);
            let mut host = RuntimeHost::for_resources(0);
            host.destroy_instance(memory, reference).unwrap();
            assert_eq!(memory.r32(reference + 12), raw);
            assert_eq!(memory.r32(reference + 8), id);
            // A second request has its own retained reference; destroying the
            // first must invalidate it before the cleanup loop sees it.
            host.destroy_instance(memory, reference).unwrap();
        })
        .unwrap();
        node::with_res(|host| assert!(host.container(resource).is_some()));
        factory::cleanup(&mut slr_vm::host::VmHost::new()).unwrap();
        node::with_res(|host| {
            assert!(host.container(resource).is_none());
            let alias = host.node(alias).unwrap();
            assert_eq!((alias.id, alias.container), (0, 0));
        });
    }
}
