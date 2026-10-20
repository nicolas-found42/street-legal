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
use std::cell::RefCell;

struct World {
    factory: Factory,
    ids: RuntimeIds,
    library: Root,
    game: Root,
    materialisation: Vec<(u32, String)>,
}
thread_local! { static WORLD: RefCell<Option<World>> = const { RefCell::new(None) }; }

pub fn configure(config: Option<u32>, init: Option<u32>) {
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
        std::fs::write(output.join("factory-operations.txt"), format!(
            "CONTROLLED loaded fresh-empty game parent; external descriptors supplied in RPK inheritance order; no general package manager parity\nmaterialisation={:#?}\nCONTROLLED seed=1; supplied Config/Init IDs reserved; original pre-Config allocation history not replayed\nroots replay-local system.rpk:1/type8 and system.rpk:4/type1\n{:#?}\ninstances={:#?}\n", w.materialisation,w.factory.events, w.factory.instances))
            .map_err(|e| format!("FactoryTraceBoundary: {e}"))
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
    let _parameters = string(vm, parameters)?;
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
        &name,
        Some(&class_name),
        Some(class),
    )?;
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

/// Resolve `requested` (0 = the receiver class's default template) and create
/// the type-1 instance for `script`.
pub fn instantiate(
    vm: &mut VmHost,
    owner_container: Option<(u32, u32)>,
    requested: u32,
    script: u32,
    name: &str,
    default_class_name: Option<&str>,
    default_class: Option<slr_vm::vmclass::ClassId>,
) -> Result<Created, String> {
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let w = world.as_mut().ok_or("factory world is not installed")?;
        let requested = if requested == 0 {
            let class_name = default_class_name.ok_or("default template needs a class name")?;
            let class = default_class.ok_or("default template needs a class")?;
            w.factory
                .default_for_class(w.library, class, class_name, &mut || w.ids.allocate())?
                .id
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

/// Original creation uses the same game-root owner for Config and GameInit.
pub fn attach_created(link: node::ScriptLink) -> Result<(), String> {
    WORLD.with(|w| {
        let w = w.borrow();
        let w = w.as_ref().ok_or("factory world not installed")?;
        let parent = w.game.container.ok_or("game root missing")?;
        node::with_res(|h| h.append_child(parent, link.container))
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
    let [_receiver, owner, template, name, _parameters] = w;
    let native_only = info.name == "create_native";
    let owner_container = if owner == 0 {
        None
    } else {
        Some(ref_container(owner).ok_or("owner GameRef has no resource")?)
    };
    let (_, template_id) = ref_container(template).ok_or("template GameRef has no resource")?;
    let name = string(vm, name)?.unwrap_or_default();
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
    let created = instantiate(vm, owner_container, template_id, script, &name, None, None)?;
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
