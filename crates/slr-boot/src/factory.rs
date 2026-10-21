//! Controlled fresh resource world for the measured default GameType branch.
use slr_resource::{
    factory::{Factory, Root, RuntimeIds, ScriptData},
    node::{self, Container, GameInstance},
};
use slr_vm::{
    host::{marshal_args, CallInfo, VmHost},
    values::Value,
    vmclass::ClassRef,
};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
};

struct World {
    factory: Factory,
    ids: RuntimeIds,
    library: Root,
    game: Root,
    materialisation: Vec<(u32, String)>,
    resources: Vec<RuntimeResource>,
    identity_history: Vec<(u32, i32, u32, u32)>,
    attached_names: HashMap<u32, String>,
    template_names: HashMap<u32, String>,
    unload_queue: VecDeque<u32>,
    destroy_queue: VecDeque<u32>,
}

/// Identity/ownership projection of a headless engine resource. The retained
/// template link models the factory's embedded resource reference; payload
/// decoding, renderer objects and physics bodies remain engine boundaries.
#[derive(Debug)]
pub struct RuntimeResource {
    pub container: u32,
    pub id: u32,
    pub kind: i32,
    pub parent: Option<u32>,
    pub template_link: Option<u32>,
    pub name: String,
}

pub fn game_root() -> Result<u32, String> {
    WORLD.with(|world| {
        world
            .borrow()
            .as_ref()
            .and_then(|world| world.game.container)
            .ok_or_else(|| "game resource root unavailable".into())
    })
}

pub struct ResourceDescriptor<'a> {
    pub kind: i32,
    pub flags: u32,
    pub parent: Option<u32>,
    pub template: Option<u32>,
    pub name: &'a str,
}
thread_local! { static WORLD: RefCell<Option<World>> = const { RefCell::new(None) }; }

pub fn configure(config: Option<u32>, init: Option<u32>) {
    crate::native_body::clear();
    crate::native_body::with_world_memory(|memory| {
        let manager = crate::native_resource_load::initialize_projection(memory);
        crate::native_resource_manager::initialize(memory, manager);
        crate::native_world::initialize_storage(memory);
        crate::native_dynamics::initialize_spatial_queue(memory);
        crate::native_shape::initialize_pending_queue(memory);
        crate::native_shape::initialize_retry_queue(memory);
        crate::native_shape::initialize_ready_queue(memory);
        crate::native_shape::initialize_result_queues(memory);
        crate::native_file::initialize_runtime(memory);
        // Original image scalar controlling asynchronous mesh requests.
        memory.w32(0x619994, 1);
        crate::native_world::initialize(memory)
    })
    .expect("fresh native memory lease")
    .expect("original static physics world construction");
    node::reset_res();
    let (library, game) = node::with_res(|h| {
        let mut library = Container::new(8, 1);
        library.flags = 0;
        let library = h.add_container(library);
        let mut game = Container::new(1, 4);
        game.flags = 1;
        game.load_state = 1;
        game.instance = Some(GameInstance::default());
        let game = h.add_container(game);
        h.bind_id(1, library);
        h.bind_id(4, game);
        // The exe's game-root handle (`0x636460`), the default owner of
        // `GameRef.create` when the caller passes none.
        let handle = h.add_bound_node(4, game);
        h.bind_object(node::DEFAULT_RESOURCE_WORD, handle);
        (library, game)
    });
    let mut ids = RuntimeIds::new(1);
    for id in [config, init].into_iter().flatten() {
        ids.reserve(id);
    }
    let mut factory = Factory::default();
    factory.fresh_empty_parents.insert(game);
    WORLD.with(|w| {
        *w.borrow_mut() = Some(World {
            factory,
            materialisation: vec![],
            resources: vec![],
            identity_history: vec![],
            attached_names: HashMap::new(),
            template_names: HashMap::new(),
            unload_queue: VecDeque::new(),
            destroy_queue: VecDeque::new(),
            ids,
            library: Root {
                resource_id: 1,
                container: Some(library),
            },
            game: Root {
                resource_id: 4,
                container: Some(game),
            },
        })
    });
}

pub fn write_trace(output: &std::path::Path) -> Result<(), String> {
    WORLD.with(|w| {
        let w = w.borrow();
        let Some(w) = w.as_ref() else { return Ok(()); };
        let identities = w.identity_history.iter().map(|(id,kind,parent,template)|
            format!("{id}\t{kind}\t{parent}\t{template}\n")).collect::<String>();
        std::fs::write(output.join("resource-identities.tsv"), identities)
            .map_err(|e| format!("ResourceIdentityTraceBoundary: {e}"))?;
        std::fs::write(output.join("factory-operations.txt"), format!(
            "CONTROLLED loaded fresh-empty game parent; external descriptors supplied in RPK inheritance order; no general package manager parity\nmaterialisation={:#?}\nCONTROLLED seed=1; supplied Config/Init IDs reserved; original pre-Config allocation history not replayed\nroots replay-local system.rpk:1/type8 and system.rpk:4/type1\n{:#?}\ninstances={:#?}\nresources={}\n", w.materialisation,w.factory.events, w.factory.instances, w.resources.iter().map(|r| format!("id={:#010x} container={:#x} kind={} parent={:?} template_link={:?} name={:?}",r.id,r.container,r.kind,r.parent,r.template_link,r.name)).collect::<Vec<_>>().join("\n")))
            .map_err(|e| format!("FactoryTraceBoundary: {e}"))
    })
}

/// Snapshot the identities and ownership actually modeled by this headless host.
pub fn diagnostic_state() -> serde_json::Value {
    WORLD.with(|world| {
        let Ok(world)=world.try_borrow() else{return serde_json::json!({"unavailable":"factory mutation in progress"})};
        let Some(world)=world.as_ref() else{return serde_json::Value::Null};
        node::try_observe(|host|serde_json::json!({"projection":"headless-identity",
            "resources":world.resources.iter().map(|resource| {
                let container=host.container(resource.container);
                serde_json::json!({"container":resource.container,"id":resource.id,"type":resource.kind,
                    "parent":container.map(|c|c.parent),"head":container.map(|c|c.head),
                    "template_link":resource.template_link,"name":resource.name})
            }).collect::<Vec<_>>()})).unwrap_or_else(||serde_json::json!({"unavailable":"resource mutation in progress"}))
    })
}

fn string(vm: &VmHost, word: u32) -> Result<Option<String>, String> {
    if word == 0 {
        return Ok(None);
    }
    let bytes = vm.string_bytes(word).ok_or("invalid string object")?;
    Ok(Some(
        std::str::from_utf8(bytes)
            .map_err(|_| "non-UTF8 factory name/parameters")?
            .into(),
    ))
}

fn create(vm: &mut VmHost, info: &CallInfo<'_>) -> Result<(), String> {
    let mut words = [0; 5];
    marshal_args(info, &mut words).map_err(|e| format!("factory argument marshalling: {e:?}"))?;
    let [receiver, owner, requested, parameters, name] = words;
    let class = match receiver
        .checked_sub(0x1000)
        .and_then(|i| vm.objects.get(i as usize))
        .map(|o| &o.class)
    {
        Some(ClassRef::Loaded(class)) => *class,
        _ => return Err("factory receiver has no retained class".into()),
    };
    let class_name = vm
        .loader
        .class_name(class)
        .ok_or("stale factory class")?
        .to_owned();
    let name = string(vm, name)?.unwrap_or(class_name.clone());
    // Default-class compiled constructor is null, so parameters are not consumed
    // by an engine constructor. Validate the actual nullable script string only.
    let parameters = string(vm, parameters)?.unwrap_or_default();
    let owner_container = if owner == 0 {
        None
    } else {
        let ptr = vm
            .read_field(owner, "ptr")
            .map_err(|e| format!("owner Native.ptr: {e:?}"))?
            .payload;
        if ptr == 0 {
            None
        } else {
            Some(
                node::with_res(|h| h.node(ptr).map(|n| (n.container, n.id)))
                    .ok_or("owner Native.ptr is not a resource link")?,
            )
        }
    };
    let instance = instantiate(
        vm,
        owner_container,
        requested,
        receiver,
        (&name, &parameters),
        Some(&class_name),
        Some(class),
    )?;
    // 53a2a0 registers callback capabilities before its native wrapper
    // replaces the script's Native.ptr with the newly returned resource link.
    crate::engine::register_created(vm, instance.container, receiver)?;
    let link = node::bind_existing_script(vm, receiver, instance.container)?;
    println!(
        "BOOT native factory class={class_name} template={:#010x} instance={:#010x} ptr={:#x}",
        instance.template, instance.id, link.node
    );
    Ok(())
}

/// One native instance of a template under an owner (default: the game root),
/// as `FUN_0053a2a0` builds it for an already existing script object.
pub struct Created {
    pub id: u32,
    pub container: u32,
    pub template: u32,
}

/// Native descriptor inherited by an instantiated template, if any.
pub fn native_class(container: u32) -> Option<String> {
    WORLD.with(|world| {
        let world = world.borrow();
        let w = world.as_ref()?;
        let instance = w
            .factory
            .instances
            .values()
            .find(|i| i.container == container)?;
        let template_container =
            node::with_res(|h| h.node(instance.template_link).map(|n| n.container))?;
        w.factory
            .templates
            .values()
            .find(|t| t.container == template_container)?
            .native_class
            .clone()
    })
}

/// Template container retained by a factory instance's own template link.
pub fn instance_template_container(container: u32) -> Option<u32> {
    WORLD.with(|world| {
        let world = world.borrow();
        let w = world.as_ref()?;
        let instance = w
            .factory
            .instances
            .values()
            .find(|i| i.container == container)?;
        node::with_res(|h| h.node(instance.template_link).map(|n| n.container))
    })
}

/// Resolve `requested` (0 = the receiver class's default template) and create
/// the type-1 instance for `script`.
pub fn instantiate(
    vm: &mut VmHost,
    owner_container: Option<(u32, u32)>,
    requested: u32,
    script: u32,
    text: (&str, &str),
    default_class_name: Option<&str>,
    default_class: Option<slr_vm::vmclass::ClassId>,
) -> Result<Created, String> {
    let (name, parameters) = text;
    let (requested, root, configured) = WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let w = world.as_mut().ok_or("factory world is not installed")?;
        let requested = if requested == 0 {
            let class_name = default_class_name.ok_or("default template needs a class name")?;
            let class = default_class.ok_or("default template needs a class")?;
            let descriptor = format!("L{class_name};");
            let template =
                w.factory
                    .default_for_class(w.library, class, &descriptor, &mut || w.ids.allocate())?;
            // 53a1a0 passes31 to551120/5548c0, whose bound includesNUL.
            let mut end = descriptor.len().min(30);
            while !descriptor.is_char_boundary(end) {
                end -= 1;
            }
            w.template_names
                .insert(template.container, descriptor[..end].to_owned());
            template.id
        } else {
            if !w.factory.templates.contains_key(&requested) {
                let phases = super::RES.with(|r| r.borrow().template_phases(requested))?;
                let mut data = ScriptData::default();
                for (id, text) in phases {
                    data.apply(&text, &mut |name| request_script(vm, name))?;
                    w.materialisation.push((id, text));
                }
                w.factory
                    .install_script_template(requested, w.library, &data)?;
            }
            requested
        };
        let root = match owner_container {
            Some((container, id)) if container != 0 => Root {
                resource_id: id,
                container: Some(container),
            },
            _ => w.game,
        };
        let template = &w.factory.templates[&requested];
        let configured = template.native_config.clone().map(|path| {
            (
                template.native_class.clone().unwrap_or_default(),
                template.container,
                path,
            )
        });
        Ok::<_, String>((requested, root, configured))
    })?;
    if let Some((class, container, path)) = configured {
        // Loading a configured native template can acquire other package
        // resources. Do not hold WORLD's mutation borrow across that work.
        let prepared =
            crate::native_body::prepare_template(vm, requested, container, &class, &path)?;
        let instance = WORLD.with(|world| {
            let mut world = world.borrow_mut();
            let world = world.as_mut().ok_or("factory world is not installed")?;
            world
                .factory
                .allocate_configured_instance(root, requested, script, name, &mut || {
                    world.ids.allocate()
                })?
                .ok_or_else(|| "configured native instance allocation returned zero".to_owned())
        })?;
        if class == "part" {
            crate::native_factory::begin_part(vm, prepared, instance.container, parameters)?;
        } else {
            crate::native_factory::begin_car(vm, prepared, instance.container, parameters)?;
        }
        return Ok(Created {
            id: instance.id,
            container: instance.container,
            template: requested,
        });
    }
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let w = world.as_mut().ok_or("factory world is not installed")?;
        let instance = w
            .factory
            .create_instance(root, requested, script, name, &mut || w.ids.allocate())?
            .ok_or("native instance allocation returned zero")?;
        if w.factory.templates[&requested].native_class.as_deref() == Some("player") {
            crate::engine::player_created(instance.container);
        }
        Ok(Created {
            id: instance.id,
            container: instance.container,
            template: requested,
        })
    })
}

pub fn create_native_instance(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    if let Err(error) = create(vm, info) {
        vm.native_failure = Some(format!("{}.{}{}: {error}", info.class, info.name, info.sig));
    }
    None
}

/// Headless identity part of FUN_00538d60: a type-0x12 resource, using the
/// same allocator as GameType templates/instances. Renderer registration and
/// virtual materialisation are not implemented by this resource model.
pub fn create_viewport(handle: u32) -> Result<u32, String> {
    let container = create_resource(ResourceDescriptor {
        kind: 0x12,
        flags: 0,
        parent: None,
        template: None,
        name: "<viewport>",
    })?;
    node::with_res(|h| h.bind_created(handle, container))?;
    Ok(container)
}

/// Register a fresh headless resource with real identity, parent/child links
/// and a retained template. The descriptor flags and allocation order follow
/// 538d60/539500/539880/539fe0/545230; this does not execute their payload tails.
pub fn create_resource(descriptor: ResourceDescriptor<'_>) -> Result<u32, String> {
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let world = world.as_mut().ok_or("factory world is not installed")?;
        node::with_res(|h| {
            for container in [descriptor.parent, descriptor.template]
                .into_iter()
                .flatten()
            {
                h.container(container)
                    .ok_or("resource parent/template missing")?;
            }
            let id = world.ids.allocate();
            let mut resource = Container::new(descriptor.kind, id);
            resource.flags =
                descriptor.flags | crate::native_resource_base::type_flags(descriptor.kind as u32);
            let container = h.add_container(resource);
            h.bind_id(id, container);
            if let Some(parent) = descriptor.parent {
                h.append_child(parent, container)?;
            }
            let template_link = descriptor.template.map(|template| {
                let id = h.container(template).unwrap().id;
                h.add_bound_node(id, template)
            });
            world.identity_history.push((
                id,
                descriptor.kind,
                descriptor
                    .parent
                    .and_then(|word| h.container(word))
                    .map_or(0, |entry| entry.id),
                descriptor
                    .template
                    .and_then(|word| h.container(word))
                    .map_or(0, |entry| entry.id),
            ));
            world.resources.push(RuntimeResource {
                container,
                id,
                kind: descriptor.kind,
                parent: descriptor.parent,
                template_link,
                name: descriptor.name.into(),
            });
            Ok(container)
        })
    })
}

/// Name text supplied to the original resource descriptor by the live factory.
/// Package resources with no installed descriptor name remain unspecified.
pub fn resource_name(container: u32) -> Option<String> {
    WORLD.with(|world| {
        let world = world.borrow();
        let world = world.as_ref()?;
        world
            .resources
            .iter()
            .find(|entry| entry.container == container)
            .map(|entry| entry.name.clone())
            .or_else(|| {
                world
                    .factory
                    .instances
                    .values()
                    .find(|entry| entry.container == container)
                    .map(|entry| entry.name.clone())
            })
            .or_else(|| world.template_names.get(&container).cloned())
    })
}

/// A real retained four-word reference owned by one native configuration slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CfgBinding {
    pub id: u32,
    pub container: u32,
    pub link: u32,
}

/// Original544590: resolve a cfg word in its template package's foreign table.
/// A runtime package manager (indexffff) treats the supplied word as canonical.
pub fn resolve_cfg(template_id: u32, id: u32) -> Result<u32, String> {
    if template_id >> 16 == 0xffff {
        return Ok(id);
    }
    if id & 0xffff == 0 {
        return Ok(0);
    }
    crate::RES.with(|resources| {
        let mut resources = resources.borrow_mut();
        let package = resources
            .packages
            .get((template_id >> 16) as usize)
            .ok_or("CfgResourceBoundary: template package missing")?
            .0
            .clone();
        resources.resolve(Some(&package), id)
    })
}

/// Original5383f0 returns the package descriptor; its+c names the package,
/// not the resource entry alias. Runtime-package descriptor text is unobserved.
pub fn package_for_resource(id: u32) -> Result<String, String> {
    if id >> 16 == 0xffff {
        return Err("CfgResourceBoundary: runtime package descriptor name".into());
    }
    crate::RES.with(|resources| {
        let mut resources = resources.borrow_mut();
        let index = (id >> 16) as usize;
        resources.request_headers(index)?;
        resources
            .packages
            .get(index)
            .map(|entry| entry.0.clone())
            .ok_or_else(|| "CfgResourceBoundary: resource package missing".into())
    })
}

/// Actual external RPK descriptor, retaining its type and flags. Callers parse
/// their own original resource-class grammar instead of guessing a file token.
pub fn cfg_descriptor(id: u32) -> Result<(i32, u8, String), String> {
    if id >> 16 == 0xffff {
        return Err("CfgResourceBoundary: runtime descriptor has no external RPK entry".into());
    }
    crate::RES.with(|resources| {
        let mut resources = resources.borrow_mut();
        resources.request_metadata(id)?;
        let package = resources.packages[(id >> 16) as usize]
            .1
            .as_ref()
            .ok_or("cfg package missing")?;
        let entry = package
            .entries
            .iter()
            .find(|entry| entry.type_id == id & 0xffff)
            .ok_or("cfg descriptor missing")?;
        Ok((
            i32::from(entry.typeof_),
            entry.flags,
            package.descriptor(entry),
        ))
    })
}

/// Read an actual descriptor-selected stock input, preserving its bytes. The
/// path is rooted in stock, as the original virtual working directory is.
pub fn open_cfg_file(path: &str) -> Result<std::fs::File, String> {
    let relative = path.trim_end_matches('\0').replace('\\', "/");
    let relative = std::path::Path::new(&relative);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("CfgResourceBoundary: external file escapes stock".into());
    }
    crate::RES.with(|resources| {
        let resources = resources.borrow();
        let root = resources
            .root
            .canonicalize()
            .map_err(|error| format!("cfg stock root: {error}"))?;
        let path = root
            .join(relative)
            .canonicalize()
            .map_err(|error| format!("CfgResourceBoundary: {relative:?}: {error}"))?;
        if !path.starts_with(&root) {
            return Err("CfgResourceBoundary: external symlink escapes stock".into());
        }
        std::fs::File::open(path)
            .map_err(|error| format!("CfgResourceBoundary: external file open: {error}"))
    })
}

pub fn read_cfg_file(path: &str) -> Result<Vec<u8>, String> {
    let mut file = open_cfg_file(path)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)
        .map_err(|error| format!("CfgResourceBoundary: external file read: {error}"))?;
    Ok(bytes)
}

/// Original538440: open/find a named package and resolve one package-local word.
pub fn lookup_cfg(package: &str, id: u32) -> Result<u32, String> {
    if id & 0xffff == 0 {
        return Ok(0);
    }
    crate::RES.with(|resources| resources.borrow_mut().resolve(Some(package), id))
}

/// Original546070→536820: bind an already canonical ID, check its exact requested
/// kind and insert a retained reference into its head list. This acquires the
/// resource identity; it does not claim its mesh/physics payload was decoded.
pub fn bind_cfg(template_id: u32, id: u32, kind: i32, flag: u32) -> Result<CfgBinding, String> {
    if id == 0 {
        return Ok(CfgBinding::default());
    }
    if id >> 16 != 0xffff {
        crate::RES.with(|resources| resources.borrow_mut().request_metadata(id))?;
    }
    node::with_res(|host| {
        let container = host.container_for_id(id).ok_or_else(|| {
            format!("CfgResourceBoundary: resource {id:#010x} header unavailable")
        })?;
        let resource = host
            .container(container)
            .ok_or("CfgResourceBoundary: canonical container missing")?;
        if kind != 0 && resource.type_id != kind {
            return Ok(CfgBinding::default());
        }
        let link = host.add_bound_node(id, container);
        slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
            category: "cfg-resource-bind".into(),
            fields: vec![
                ("template".into(), format!("{template_id:#010x}")),
                ("id".into(), format!("{id:#010x}")),
                ("kind".into(), kind.to_string()),
                ("flag".into(), format!("{flag:#010x}")),
                ("container".into(), container.to_string()),
                ("link".into(), link.to_string()),
            ],
        });
        Ok(CfgBinding {
            id,
            container,
            link,
        })
    })
}

pub fn release_cfg(binding: CfgBinding) {
    if binding.link != 0 {
        node::with_res(|host| host.release_embedded_link(binding.link));
    }
}

/// Camera.destroy follows the render instance's live embedded template link.
pub fn template_container(container: u32) -> Option<u32> {
    WORLD.with(|world| {
        let world = world.borrow();
        let world = world.as_ref()?;
        let link = world
            .resources
            .iter()
            .find(|resource| resource.container == container)?
            .template_link?;
        node::with_res(|host| host.node(link).map(|entry| entry.container))
            .filter(|word| *word != 0)
    })
}

/// Keep the retained identity link synchronized after original5439e0 changes
/// the native render instance's embedded template reference.
pub fn synchronize_render_template(container: u32, template: u32) -> Result<(), String> {
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let world = world.as_mut().ok_or("factory world is not installed")?;
        let resource = world
            .resources
            .iter_mut()
            .find(|r| r.container == container)
            .ok_or("render instance identity missing")?;
        if resource.kind != 3 {
            return Err("template synchronization requires render instance".into());
        }
        node::with_res(|host| {
            if let Some(link) = resource.template_link {
                host.bind_created(link, template)
            } else {
                let id = host.container(template).map_or(0, |r| r.id);
                resource.template_link = Some(host.add_bound_node(id, template));
                Ok(())
            }
        })
    })
}

/// Original427620/427720 retain a real reference in FIFO deferred queues.
pub fn schedule_release(container: u32, destroy: bool) -> Result<(), String> {
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let world = world.as_mut().ok_or("factory world missing")?;
        let link = node::with_res(|host| {
            let id = host
                .container(container)
                .ok_or("release container missing")?
                .id;
            Ok::<_, String>(host.add_bound_node(id, container))
        })?;
        if destroy {
            world.destroy_queue.push_back(link);
        } else {
            world.unload_queue.push_back(link);
        }
        slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
            category: "resource-defer-release".into(),
            fields: vec![
                ("container".into(), container.to_string()),
                ("destroy".into(), destroy.to_string()),
                ("retained_link".into(), link.to_string()),
            ],
        });
        Ok(())
    })
}

pub fn unload_resource(container: u32) -> Result<(), String> {
    fn unload(container: u32, depth: usize) -> Result<(), String> {
        if depth > 256 {
            return Err("ResourceLifecycleBoundary: unload child depth".into());
        }
        let (resource, children) = node::with_res(|host| {
            let resource = host
                .container(container)
                .ok_or("unload container missing")?
                .clone();
            let mut children = Vec::new();
            let mut child = resource.first_child;
            while child != 0 && child != resource.children_sentinel {
                if children.contains(&child) {
                    return Err("resource child list cycle".into());
                }
                children.push(child);
                child = host
                    .container(child)
                    .ok_or("unload child missing")?
                    .next_sibling;
            }
            Ok::<_, String>((resource, children))
        })?;
        if crate::native_assets::owns_payload(container) {
            return Err("NativeAssetBoundary: loaded render payload requires its original unload/destructor".into());
        }
        if resource.type_id == 8 && crate::native_body::has_loaded_template(container) {
            return Err("NativeTemplateBoundary: configured template unload requires original546c60/native class destructor".into());
        }
        // 53ec20 first recursively unloads script children, even when this
        // script is already unloaded. No event2 is synthesized for children.
        if resource.type_id == 1 {
            for child in &children {
                let script = node::with_res(|host| {
                    host.container(*child)
                        .is_some_and(|entry| entry.type_id == 1)
                });
                if script {
                    unload(*child, depth + 1)?;
                }
            }
            if resource.load_state & 1 != 0 {
                // Forced unload flag1 deletes owned children before the
                // enclosing resource's base destructor recycles its own ID.
                for child in children {
                    let eligible = node::with_res(|host| {
                        host.container(child).is_some_and(|entry| {
                            ((entry.id ^ resource.id) & 0xffff0000) == 0 || entry.id >> 16 == 0xffff
                        })
                    });
                    if eligible {
                        destroy_resource(child)?;
                    }
                }
            }
        }
        node::with_res(|host| {
            let resource = host
                .container_mut(container)
                .ok_or("unload container disappeared")?;
            resource.load_state &= !1;
            resource.instance = None;
            Ok(())
        })
    }
    unload(container, 0)
}

/// Deleting resource destructors recycle their actual ID before invalidating
/// references and recursively deleting children (544700/544e20→5517f0).
pub fn destroy_resource(container: u32) -> Result<(), String> {
    fn destroy(
        world: &mut World,
        host: &mut node::ResHost,
        container: u32,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 256 {
            return Err("ResourceLifecycleBoundary: resource child depth".into());
        }
        let resource = host
            .container(container)
            .ok_or("destroy container missing")?
            .clone();
        if resource.id >> 16 != 0xffff {
            return Err("ResourceLifecycleBoundary: fixed package resource destruction".into());
        }
        let links = world
            .resources
            .iter()
            .filter(|entry| entry.container == container)
            .filter_map(|entry| entry.template_link)
            .chain(
                world
                    .factory
                    .instances
                    .values()
                    .filter(|entry| entry.container == container)
                    .map(|entry| entry.template_link),
            )
            .collect::<Vec<_>>();
        for link in links {
            host.release_embedded_link(link);
        }
        world.ids.recycle(resource.id);
        slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
            category: "resource-recycle".into(),
            fields: vec![
                ("id".into(), format!("{:#010x}", resource.id)),
                ("container".into(), container.to_string()),
            ],
        });
        host.invalidate_container_references(container)?;
        while let Some(child) = host
            .container(container)
            .and_then(|entry| (entry.first_child != 0).then_some(entry.first_child))
        {
            destroy(world, host, child, depth + 1)?;
        }
        host.remove_empty_container(container)?;
        world.resources.retain(|entry| entry.container != container);
        world
            .factory
            .instances
            .retain(|_, entry| entry.container != container);
        world
            .factory
            .templates
            .retain(|_, entry| entry.container != container);
        world.factory.defaults.retain(|_, id| *id != resource.id);
        world.attached_names.remove(&container);
        world.template_names.remove(&container);
        Ok(())
    }
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let world = world.as_mut().ok_or("factory world missing")?;
        node::with_res(|host| destroy(world, host, container, 0))
    })
}

/// Original4277f0 drains unload then destroy, repeating for reentrant requests.
pub fn cleanup(vm: &mut VmHost) -> Result<(), String> {
    loop {
        let mut processed = false;
        for destroy in [false, true] {
            loop {
                let link = WORLD.with(|world| {
                    let mut world = world.borrow_mut();
                    let world = world.as_mut()?;
                    if destroy {
                        world.destroy_queue.pop_front()
                    } else {
                        world.unload_queue.pop_front()
                    }
                });
                let Some(link) = link else { break };
                processed = true;
                let resource = node::with_res(|host| {
                    host.node(link).and_then(|entry| {
                        host.container(entry.container)
                            .map(|resource| (entry.container, resource.type_id))
                    })
                });
                if let Some((container, kind)) = resource {
                    if kind == 1 {
                        crate::engine::dispatch(
                            vm,
                            container,
                            None,
                            2,
                            &crate::engine::EventArg::Str(None),
                            None,
                        )?;
                    }
                    unload_resource(container)?;
                    if destroy {
                        destroy_resource(container)?;
                    }
                }
                node::with_res(|host| host.release_embedded_link(link));
            }
        }
        if !processed {
            break;
        }
    }
    Ok(())
}

/// Original creation uses the same game-root owner for Config and GameInit.
pub fn attach_created(link: node::ScriptLink, template: u32) -> Result<(), String> {
    let name = package_name(template).ok_or("created template has no package name")?;
    WORLD.with(|w| {
        let mut w = w.borrow_mut();
        let w = w.as_mut().ok_or("factory world not installed")?;
        let parent = w.game.container.ok_or("game root missing")?;
        node::with_res(|h| h.append_child(parent, link.container))?;
        w.attached_names.insert(link.container, name);
        Ok(())
    })
}

fn package_name(id: u32) -> Option<String> {
    crate::RES.with(|resources| {
        let resources = resources.borrow();
        let package = resources.packages.get((id >> 16) as usize)?.1.as_ref()?;
        package
            .entries
            .iter()
            .find(|entry| entry.type_id == id & 0xffff)
            .map(|entry| entry.alias.trim_end_matches('\0').to_owned())
    })
}

/// Callback4255e0 uses the data's embedded template reference (+0x38), not
/// the instance name. At42568b ECX is data+0x38;48adb0 reads its container's
/// name at+0x60. Thus a "dummy" instance is labeled "Dummy.getInfo".
pub fn runtime_name(container: u32) -> Option<String> {
    WORLD.with(|world| {
        let world = world.borrow();
        let world = world.as_ref()?;
        world.attached_names.get(&container).cloned().or_else(|| {
            let instance = world
                .factory
                .instances
                .values()
                .find(|instance| instance.container == container)?;
            let template = node::with_res(|host| {
                host.node(instance.template_link).map(|node| node.container)
            })?;
            world.template_names.get(&template).cloned().or_else(|| {
                let template = world
                    .factory
                    .templates
                    .values()
                    .find(|entry| entry.container == template)?;
                package_name(template.id)
            })
        })
    })
}

/// The type-8 parser's `script <name>` request (`FUN_005461a0`): a name ending
/// in `.class` is a path, split into directory and basename and loaded
/// directly (`FUN_00410750`); anything else is a class request
/// (`FUN_00410890`).
fn request_script(vm: &mut VmHost, name: &str) -> Result<Option<slr_vm::vmclass::ClassId>, String> {
    if let Some(stem) = name.strip_suffix(".class") {
        let (directory, basename) = match stem.rfind('\\') {
            Some(i) => (&stem[..=i], &stem[i + 1..]),
            None => ("", stem),
        };
        return vm
            .loader
            .request_path(directory, basename)
            .map_err(|e| format!("template script {name}: {e:?}"));
    }
    vm.loader
        .request(name)
        .map_err(|e| format!("template script {name}: {e:?}"))
}

/// The engine's post-construction owner notification. On the scripted branch
/// `53a5e0` runs `owner.addpart(created)` through the callback wrapper before
/// `0x427210` registers the instance; the owner is the script executing the
/// `GameRef.create` call, not the created instance.
fn notify_created_owner(
    vm: &mut VmHost,
    created: u32,
    owner_container: Option<(u32, u32)>,
) -> Result<(), String> {
    let Some((owner, _)) = owner_container else {
        return Ok(());
    };
    let candidates = node::with_res(|h| {
        let mut found = Vec::new();
        for container in std::iter::once(owner).chain(h.child_containers(owner)) {
            let Some(c) = h.container(container) else {
                continue;
            };
            if let Some(script) = c.instance.as_ref().map(|i| i.script) {
                if script != 0 && script != created {
                    found.push((container, script));
                }
            }
        }
        found
    });
    for (container, script) in candidates {
        let Some(class) = object_class(vm, script) else {
            continue;
        };
        if !class_has_method(&vm.loader, class, "addpart") {
            continue;
        }
        let name = runtime_name(container).unwrap_or_default();
        let (arg_node, ty) = node::with_res(|h| {
            let n = h.object_node(created);
            let ty = h
                .node(n)
                .and_then(|entry| h.container(entry.container))
                .map_or(1, |c| c.type_id);
            (n, ty)
        });
        let wrapper = node::wrap_node(vm, arg_node, ty as u32);
        crate::sched::run_callback(
            vm,
            &name,
            class,
            script,
            "addpart",
            &[Value::reference_named(
                wrapper,
                "Ljava.util.resource.GameRef;",
            )],
            false,
        )
        .map_err(|(thread, error)| {
            crate::script_stop(vm, &thread, "create owner notification", error)
        })?;
        return Ok(());
    }
    Ok(())
}

fn class_has_method(
    loader: &slr_vm::vmclass::Loader,
    mut class: slr_vm::vmclass::ClassId,
    name: &str,
) -> bool {
    for _ in 0..64 {
        let Some(entry) = loader.find_by_id(class) else {
            return false;
        };
        if entry.methods.iter().any(|method| method.name == name) {
            return true;
        }
        let Some(base) = entry.base.as_deref() else {
            return false;
        };
        let Some(next) = loader.lookup(base) else {
            return false;
        };
        class = next;
    }
    false
}

fn object_class(vm: &VmHost, word: u32) -> Option<slr_vm::vmclass::ClassId> {
    match vm.objects.get(word.checked_sub(0x1000)? as usize)?.class {
        ClassRef::Loaded(id) => Some(id),
        ClassRef::Array(_) => None,
    }
}

fn ref_container(word: u32) -> Option<(u32, u32)> {
    node::with_res(|h| {
        let n = h.object_node(word);
        let container = h.node(n)?.container;
        h.container(container).map(|c| (container, c.id))
    })
}

fn game_ref_create_inner(vm: &mut VmHost, info: &CallInfo<'_>) -> Result<(), String> {
    let mut w = [0; 5];
    marshal_args(info, &mut w).map_err(|e| format!("create argument marshalling: {e:?}"))?;
    //47d813..47d824 passes the third formal argument as53a5e0's
    //constructor text and the fourth as its instance name.
    let [_receiver, owner, template, parameters, name] = w;
    let native_only = info.name == "create_native";
    let owner_container = if owner == 0 {
        None
    } else {
        Some(ref_container(owner).ok_or("owner GameRef has no resource")?)
    };
    let (_, template_id) = ref_container(template).ok_or("template GameRef has no resource")?;
    let name = string(vm, name)?.unwrap_or_default();
    let parameters = string(vm, parameters)?.unwrap_or_default();
    // Install the template first so its script class is known.
    let class = WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let w = world.as_mut().ok_or("factory world is not installed")?;
        if !w.factory.templates.contains_key(&template_id) {
            let phases = super::RES.with(|r| r.borrow().template_phases(template_id))?;
            let mut data = ScriptData::default();
            for (id, text) in phases {
                data.apply(&text, &mut |name| request_script(vm, name))?;
                w.materialisation.push((id, text));
            }
            w.factory
                .install_script_template(template_id, w.library, &data)?;
        }
        Ok::<_, String>(w.factory.templates[&template_id].class)
    })?;
    let script = match class {
        Some(class) if !native_only => vm
            .alloc_object_id(class)
            .map_err(|e| format!("script instance allocation: {e:?}"))?,
        _ => 0,
    };
    let created = instantiate(
        vm,
        owner_container,
        template_id,
        script,
        (&name, &parameters),
        None,
        None,
    )?;
    if script != 0 {
        let node = node::with_res(|h| {
            let n = h.add_bound_node(created.id, created.container);
            h.bind_object(script, n);
            n
        });
        vm.write_field(script, "ptr", Value::int(node as i32))
            .map_err(|e| format!("Native.ptr store: {e:?}"))?;
        let class = class.expect("scripted instance has a class");
        crate::sched::run_created(
            vm,
            "THRD-CREATE",
            class,
            script,
            "<init>",
            &[Value::int(created.id as i32)],
        )
        .map_err(|(thread, e)| {
            crate::script_stop(vm, &thread, "created instance constructor", e)
        })?;
        notify_created_owner(vm, script, owner_container)?;
        crate::engine::register_created(vm, created.container, script)?;
    }
    println!(
        "BOOT created instance template={template_id:#010x} id={:#010x} script={script:#x} native_only={native_only}",
        created.id
    );
    node::with_res(|h| h.set_created_result(created.container));
    Ok(())
}

/// `GameRef.create` / `GameRef.create_native` (`FUN_0047d7b0`/`FUN_0047d900`):
/// the resource manager's instance creation (`FUN_0053a5e0` for the scripted
/// branch), then the recorded wrapper relink.
pub fn game_ref_create(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    if let Err(error) = game_ref_create_inner(vm, info) {
        vm.native_failure = Some(format!("{}.{}{}: {error}", info.class, info.name, info.sig));
        return None;
    }
    node::bridge(vm, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_game_ref_create_keeps_constructor_parameters_and_name_distinct() {
        use slr_vm::vmclass::{Class, Field};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/game-ref-create-arguments-original.json");
        let capture: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/game_ref_create_arguments_probe.py"),
        )
        .unwrap();
        for row in capture["rows"].as_array().unwrap() {
            configure(None, None);
            let mut vm = VmHost::new();
            node::install(&mut vm);
            for class_name in ["java.util.resource.GameRef", "java.lang.String"] {
                vm.loader.insert_test_class(Class {
                    kind: 0,
                    name: class_name.into(),
                    base: None,
                    interfaces: vec![],
                    fields: vec![Field {
                        class: class_name.into(),
                        name: "ptr".into(),
                        ty: "I".into(),
                        is_static: false,
                    }],
                    methods: vec![],
                    bodies: vec![],
                });
            }
            let template_container = WORLD.with(|world| {
                let mut world = world.borrow_mut();
                let world = world.as_mut().unwrap();
                world
                    .factory
                    .install_script_template(0x100, world.library, &ScriptData::default())
                    .unwrap()
                    .container
            });
            let mut reference = |container: u32| {
                let object = vm.alloc_object("java.util.resource.GameRef").unwrap();
                let handle = node::with_res(|host| {
                    let id = host.container(container).map_or(0, |c| c.id);
                    let handle = host.add_bound_node(id, container);
                    host.bind_object(object, handle);
                    handle
                });
                vm.write_field(object, "ptr", Value::int(handle as i32))
                    .unwrap();
                object
            };
            let receiver = reference(0);
            let template = reference(template_container);
            let owner_container = node::with_res(|host| host.container_for_id(4).unwrap());
            let owner = if row["default_owner"].as_bool().unwrap() {
                0
            } else {
                reference(owner_container)
            };
            let parameters = vm
                .intern_string(row["parameters"].as_str().map(str::as_bytes))
                .unwrap();
            let name = vm
                .intern_string(row["name"].as_str().map(str::as_bytes))
                .unwrap();
            let method = row["method"].as_str().unwrap();
            let native = method == "create_native";
            let sig = if native {
                "(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)V"
            } else {
                "(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)Ljava.lang.GameType;"
            };
            let args = [
                Value::reference_named(name, "Ljava.lang.String;"),
                Value::reference_named(parameters, "Ljava.lang.String;"),
                Value::reference(template),
                Value::reference(owner),
                Value::reference(receiver),
            ];
            let info = CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: "java.util.resource.GameRef",
                name: method,
                sig,
            };
            let result = game_ref_create(&mut vm, &info);
            assert!(vm.native_failure.is_none(), "{:?}", vm.native_failure);
            assert_eq!(result, if native { None } else { Some(0) });
            assert!(row["parameters_forwarded"].as_bool().unwrap());
            assert!(row["name_forwarded"].as_bool().unwrap());
            // Original wrapper constructor spy verifies formal3->parameters,
            // formal4->name; the VM path must use that name for the actual
            // factory instance while retaining both words in its collaborator.
            WORLD.with(|world| {
                let world = world.borrow();
                let world = world.as_ref().unwrap();
                assert_eq!(world.factory.instances.len(), 1);
                let instance = world.factory.instances.values().next().unwrap();
                assert_eq!(instance.name, row["name"].as_str().unwrap_or_default());
                assert_ne!(
                    instance.name,
                    row["parameters"]
                        .as_str()
                        .unwrap_or("null-constructor-text")
                );
            });
            node::with_res(|host| {
                let forwarded = host
                    .events()
                    .iter()
                    .find_map(|event| match event {
                        node::ResourceEvent::Create { native: kind, args } if *kind == native => {
                            Some(args)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(&forwarded[forwarded.len() - 2..], &[parameters, name]);
                if native {
                    assert_eq!(forwarded[2], 0);
                }
            });
        }
    }

    #[test]
    fn cfg_binding_retains_canonical_runtime_container_and_checks_exact_kind() {
        configure(None, None);
        let container = runtime(7, None, None);
        let id = node::with_res(|host| host.container(container).unwrap().id);
        assert_eq!(resolve_cfg(0xffff0001, id).unwrap(), id);
        assert_eq!(
            bind_cfg(0xffff0001, id, 9, 0).unwrap(),
            CfgBinding::default()
        );
        let bound = bind_cfg(0xffff0001, id, 7, 1).unwrap();
        assert_eq!((bound.id, bound.container), (id, container));
        node::with_res(|host| assert_eq!(host.node(bound.link).unwrap().container, container));
        release_cfg(bound);
        node::with_res(|host| {
            assert!(host.node(bound.link).is_none());
            assert_eq!(host.container(container).unwrap().head, 0);
        });
        assert_eq!(
            bind_cfg(0xffff0001, 0, 0, 0).unwrap(),
            CfgBinding::default()
        );
    }

    fn runtime(kind: i32, parent: Option<u32>, template: Option<u32>) -> u32 {
        create_resource(ResourceDescriptor {
            kind,
            flags: 0,
            parent,
            template,
            name: "lifecycle-test",
        })
        .unwrap()
    }

    #[test]
    fn render_template_retarget_survives_old_template_destruction_and_can_clear() {
        configure(None, None);
        let old = runtime(14, None, None);
        let new = runtime(14, None, None);
        let render = runtime(3, None, Some(old));
        synchronize_render_template(render, new).unwrap();
        destroy_resource(old).unwrap();
        assert_eq!(template_container(render), Some(new));
        synchronize_render_template(render, 0).unwrap();
        assert_eq!(template_container(render), None);
        destroy_resource(new).unwrap();
        destroy_resource(render).unwrap();
    }

    #[test]
    fn deferred_destroy_invalidates_aliases_and_recycles_in_actual_fifo_order() {
        configure(None, None);
        let parent = node::with_res(|host| host.container_for_id(4).unwrap());
        let template = runtime(12, Some(parent), None);
        let instance = runtime(3, Some(parent), Some(template));
        let (template_id, instance_id, alias) = node::with_res(|host| {
            let a = host.container(template).unwrap().id;
            let b = host.container(instance).unwrap().id;
            (a, b, host.add_bound_node(b, instance))
        });
        schedule_release(template, true).unwrap();
        schedule_release(instance, true).unwrap();
        // A duplicate request retains a link that the first destructor zeros.
        schedule_release(instance, true).unwrap();
        node::with_res(|host| assert_eq!(host.node(alias).unwrap().id, instance_id));
        cleanup(&mut VmHost::new()).unwrap();
        node::with_res(|host| {
            assert!(host.container(template).is_none());
            assert!(host.container(instance).is_none());
            let reference = host.node(alias).unwrap();
            assert_eq!(
                (
                    reference.prev,
                    reference.next,
                    reference.id,
                    reference.container
                ),
                (0, 0, 0, 0)
            );
            let parent = host.container(parent).unwrap();
            assert_eq!((parent.first_child, parent.last_child), (0, 0));
            assert_ne!(parent.children_sentinel, 0);
        });
        WORLD.with(|world| assert_eq!(world.borrow().as_ref().unwrap().identity_history.len(), 2));
        let first = runtime(3, Some(parent), None);
        let second = runtime(3, Some(parent), None);
        node::with_res(|host| {
            assert_eq!(host.container(first).unwrap().id, instance_id);
            assert_eq!(host.container(second).unwrap().id, template_id);
            assert_eq!(host.container(first).unwrap().next_sibling, second);
        });
    }

    #[test]
    fn loaded_script_unload_destroys_children_before_parent_recycling() {
        configure(None, None);
        let parent = runtime(1, None, None);
        let script = runtime(1, Some(parent), None);
        let leaf = runtime(3, Some(script), None);
        node::with_res(|host| {
            host.container_mut(parent).unwrap().load_state = 1;
            host.container_mut(script).unwrap().load_state = 1;
        });
        let ids = node::with_res(|host| {
            [parent, script, leaf].map(|word| host.container(word).unwrap().id)
        });
        schedule_release(parent, true).unwrap();
        cleanup(&mut VmHost::new()).unwrap();
        let recreated = [
            runtime(3, None, None),
            runtime(3, None, None),
            runtime(3, None, None),
        ];
        node::with_res(|host| {
            assert_eq!(recreated.map(|word| host.container(word).unwrap().id), ids)
        });
    }

    #[test]
    fn recursive_destroy_recycles_parent_before_children_and_keeps_siblings_linked() {
        configure(None, None);
        let owner = node::with_res(|host| host.container_for_id(4).unwrap());
        let parent = runtime(3, Some(owner), None);
        let child = runtime(3, Some(parent), None);
        let sibling = runtime(3, Some(owner), None);
        let (parent_id, child_id) = node::with_res(|host| {
            (
                host.container(parent).unwrap().id,
                host.container(child).unwrap().id,
            )
        });
        schedule_release(parent, true).unwrap();
        cleanup(&mut VmHost::new()).unwrap();
        node::with_res(|host| {
            let root = host.container(owner).unwrap();
            assert_eq!((root.first_child, root.last_child), (sibling, sibling));
            assert_eq!(host.container(sibling).unwrap().previous_sibling, owner);
        });
        let a = runtime(3, None, None);
        let b = runtime(3, None, None);
        node::with_res(|host| {
            assert_eq!(host.container(a).unwrap().id, child_id);
            assert_eq!(host.container(b).unwrap().id, parent_id);
        });
    }

    #[test]
    fn callback_label_uses_bounded_template_descriptor_instead_of_instance_name() {
        configure(None, None);
        let mut vm = VmHost::new();
        let created = instantiate(
            &mut vm,
            None,
            0,
            7,
            ("arbitrary instance", ""),
            Some("java.render.SimpleLoadingDialog"),
            slr_vm::vmclass::ClassId::from_raw(17),
        )
        .unwrap();
        // Original53a1a0→551120(copy limit31)→5548c0 stores30 bytes+NUL.
        // Original42568b then reads that template name through data+0x38.
        assert_eq!(
            runtime_name(created.container).as_deref(),
            Some("Ljava.render.SimpleLoadingDial")
        );
        WORLD.with(|world| {
            let world = world.borrow();
            assert_eq!(
                world.as_ref().unwrap().factory.instances[&created.id].name,
                "arbitrary instance"
            );
        });
    }
}
