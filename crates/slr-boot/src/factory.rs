//! Controlled fresh resource world for the measured default GameType branch.
use slr_resource::{
    factory::{Factory, Root, RuntimeIds, ScriptData},
    node::{self, Container, GameInstance},
};
use slr_vm::{
    host::{marshal_args, CallInfo, VmHost},
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
    WORLD.with(|world| {
        let mut world = world.borrow_mut();
        let w = world.as_mut().ok_or("factory world is not installed")?;
        let requested = if requested == 0 {
            w.factory.default_for_class(w.library, class, &class_name, &mut || w.ids.allocate())?.id
        } else {
            if !w.factory.templates.contains_key(&requested) {
                let phases=super::RES.with(|r| r.borrow().template_phases(requested))?;
                let mut data=ScriptData::default();
                for (id,text) in phases {
                    data.apply(&text,&mut |name| vm.loader.request(name).map_err(|e|format!("template script {name}: {e:?}"))?.ok_or_else(||format!("template script class {name} missing")))?;
                    w.materialisation.push((id,text));
                }
                w.factory.install_script_template(requested,w.library,&data)?;
            }
            requested
        };
        let root = match owner_container {
            Some((container, id)) if container != 0 => Root {resource_id:id,container:Some(container)},
            _ => w.game,
        };
        let instance = w.factory.create_instance(root, requested, receiver, &name, &mut || w.ids.allocate())?
            .ok_or("native instance allocation returned zero")?;
        let link = node::bind_existing_script(vm, receiver, instance.container)?;
        println!("BOOT native factory class={class_name} template={requested:#010x} instance={:#010x} ptr={:#x}", instance.id,link.node);
        Ok(())
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
