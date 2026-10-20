//! Headless resource factory identities, not render/physics payload execution.
//!
//! Original factories create registered resources even with a declared headless
//! device. These objects share the GameType ID allocator and participate in the
//! resource handle/parent graph. Dropping these effects shifts later primitive
//! ResourceRef.id results. See the passive original resource history in re/.
use crate::factory::{create_resource, ResourceDescriptor};
use slr_resource::node::{self, Container};
use slr_vm::host::{marshal_args, CallInfo, VmHost};

pub const RENDER_CREATE: u32 = 0x0048_0ee0;
pub const PHYSICS_BOX: u32 = 0x0048_05b0;
pub const CAMERA_CREATE: u32 = 0x0048_61e0;
pub const TEXT_CREATE: u32 = 0x0048_7050;

fn handle(vm: &mut VmHost, object: u32) -> Result<Option<u32>, String> {
    if object == 0 {
        return Ok(None);
    }
    let ptr = vm
        .read_field(object, "ptr")
        .map_err(|e| format!("Native.ptr: {e:?}"))?
        .payload;
    if ptr == 0 {
        return Ok(None);
    }
    node::with_res(|h| {
        h.node(ptr)
            .ok_or("Native.ptr handle missing")
            .map(|_| Some(ptr))
            .map_err(str::to_owned)
    })
}

fn resource(handle: u32) -> Option<(u32, Container)> {
    node::with_res(|h| {
        let word = h.node(handle)?.container;
        Some((word, h.container(word)?.clone()))
    })
}

fn bind(handle: u32, container: u32) -> Result<(), String> {
    node::with_res(|h| h.bind_created(handle, container))
}

fn name(vm: &VmHost, object: u32, default: &str) -> Result<String, String> {
    if object == 0 {
        return Ok(default.into());
    }
    let bytes = vm
        .string_bytes(object)
        .ok_or("factory name is not a String")?;
    String::from_utf8(bytes.to_vec()).map_err(|_| "non-UTF8 resource name".into())
}

fn instance(kind: i32, owner: u32, template: Option<u32>, name: &str) -> Result<u32, String> {
    create_resource(ResourceDescriptor {
        kind,
        flags: if kind == 3 { 0x301 } else { 0x201 },
        parent: Some(owner),
        template,
        name,
    })
}

fn camera_root() -> Result<u32, String> {
    if let Some(root) = node::with_res(|h| h.container_for_id(12)) {
        return Ok(root);
    }
    // Other system roots are installed as foreign parents when packages need
    // them. The original additionally retains a direct camera-root handle.
    let (kind, flags) = crate::RES.with(|r| {
        let r = r.borrow();
        let system = &r.packages.first().ok_or("system package missing")?.1;
        let root = system
            .entries
            .iter()
            .find(|e| e.type_id == 12 && e.super_id == 0)
            .ok_or("camera type root missing from system.rpk")?;
        if root.typeof_ != 0xc {
            return Err("unexpected camera root kind");
        }
        Ok((i32::from(root.typeof_), u32::from(root.flags)))
    })?;
    Ok(node::with_res(|h| {
        let mut root = Container::new(kind, 12);
        root.flags = flags;
        let root = h.add_container(root);
        h.bind_id(12, root);
        root
    }))
}

fn create(vm: &mut VmHost, info: &CallInfo<'_>) -> Result<(), String> {
    let mut w = [0; 11];
    marshal_args(info, &mut w).map_err(|e| format!("resource arguments: {e:?}"))?;
    match (info.class, info.name) {
        (node::RESOURCE_REF_CLASS, "duplicate") => {
            // LAB_004802c0 -> 48d860. Types e/d/5/7 clone; other types
            // alias the source container and consume no runtime ID.
            let (Some(target), Some(source)) = (handle(vm, w[0])?, handle(vm, w[1])?) else {
                return Ok(());
            };
            let Some((source_word, source)) = resource(source) else {
                return bind(target, 0);
            };
            let created = if matches!(source.type_id, 0xe | 0xd | 5 | 7) {
                create_resource(ResourceDescriptor {
                    kind: source.type_id,
                    flags: source.flags | 0x200,
                    parent: (source.parent != 0).then_some(source.parent),
                    // 545230 copies the payload into an independent resource;
                    // unlike 539880, it embeds no reference to the source.
                    template: None,
                    name: "<duplicate>",
                })?
            } else {
                source_word
            };
            bind(target, created)
        }
        ("java.util.resource.RenderRef", "create") => {
            // 480ee0 takes owner first, render type second. It requires a
            // bound owner and both argument handles before calling 539880.
            let (Some(target), Some(owner), Some(template)) =
                (handle(vm, w[0])?, handle(vm, w[1])?, handle(vm, w[2])?)
            else {
                return Ok(());
            };
            let Some((owner, _)) = resource(owner) else {
                return Ok(());
            };
            let template = resource(template).map(|(c, _)| c);
            let name = name(vm, w[3], "<renderinst>")?;
            let created = instance(3, owner, template, &name)?;
            bind(target, created)
        }
        ("java.util.resource.PhysicsRef", "createBox") => {
            let (Some(target), Some(owner)) = (handle(vm, w[0])?, handle(vm, w[1])?) else {
                return Ok(());
            };
            let Some((owner, _)) = resource(owner) else {
                return Err(
                    "unbound physics owner is outside the measured headless profile".into(),
                );
            };
            let name = name(vm, w[5], "<physinst>")?;
            let created = instance(2, owner, None, &name)?;
            bind(target, created)
        }
        ("java.render.Camera", "create") => {
            let Some(target) = handle(vm, w[0])? else {
                return Ok(());
            };
            let owner = handle(vm, w[1])?
                .and_then(resource)
                .map(|(c, _)| c)
                .ok_or("camera owner is outside the measured headless profile")?;
            // LAB_004861e0 creates a camera type under DAT_00636450 (system
            // resource 12), then a render instance under the supplied owner.
            let camera_root = camera_root()?;
            let camera = create_resource(ResourceDescriptor {
                kind: 0xc,
                flags: 2,
                parent: Some(camera_root),
                template: None,
                name: "<rendertype_camera>",
            })?;
            let created = instance(3, owner, Some(camera), "s_renderref_camera")?;
            bind(target, created)
        }
        ("java.render.Text", "create") => {
            // Text is not the ResourceRef receiver. 487050/42a010 resolve
            // Text.renderinst and create the instance into that object's ptr.
            let render = vm
                .read_field(w[0], "renderinst")
                .map_err(|e| format!("Text.renderinst: {e:?}"))?
                .payload;
            let (Some(target), Some(owner), Some(font)) =
                (handle(vm, render)?, handle(vm, w[1])?, handle(vm, w[2])?)
            else {
                return Ok(());
            };
            let Some((font, _)) = resource(font) else {
                return Ok(());
            };
            let Some((owner, _)) = resource(owner) else {
                return Err("text owner is outside the measured headless profile".into());
            };
            let created = instance(3, owner, Some(font), "r_text")?;
            bind(target, created)
        }
        _ => Err("unregistered resource identity native".into()),
    }
}

fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    if info.class == node::RESOURCE_REF_CLASS && info.name == "duplicate" {
        node::bridge(vm, info);
    }
    if let Err(error) = create(vm, info) {
        vm.native_failure = Some(format!(
            "{}.{}: ResourceIdentityBoundary: {error}",
            info.class, info.name
        ));
    }
    None
}

pub fn install(vm: &mut VmHost) {
    for address in [
        node::DUPLICATE,
        RENDER_CREATE,
        PHYSICS_BOX,
        CAMERA_CREATE,
        TEXT_CREATE,
    ] {
        vm.native_impls.insert(address, bridge);
    }
}
