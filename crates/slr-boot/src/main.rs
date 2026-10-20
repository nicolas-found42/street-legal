//! Headless stock bootstrap. All script bodies come from the read-only install.
mod engine;
mod factory;
mod materialise;
mod missing_rows;
mod sched;
use slr_boot::settings::{Kind, Settings, Source};
use slr_fmt_rpak::Package;
use slr_vm::{
    host::{CallInfo, VmHost},
    interp::{Thread, VmError},
    values::Value,
    vmclass::FileReader,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Resources {
    pub(crate) root: PathBuf,
    pub(crate) packages: Vec<(String, Package)>,
    registered: std::collections::HashMap<usize, slr_resource::package::Installed>,
}
impl Resources {
    fn package(&mut self, name: &str) -> Result<usize, String> {
        let name = name
            .trim_end_matches('\0')
            .replace('\\', "/")
            .to_ascii_lowercase();
        if let Some(i) = self.packages.iter().position(|(n, _)| *n == name) {
            return Ok(i);
        }
        let p = Package::open(self.root.join(&name)).map_err(|e| format!("RPK {name}: {e:?}"))?;
        let i = self.packages.len();
        self.packages.push((name, p));
        Ok(i)
    }
    fn resolve(&mut self, package: Option<&str>, id: u32) -> Result<u32, String> {
        let i = if let Some(package) = package {
            self.package(package)?
        } else {
            0
        };
        let foreign = id >> 16;
        let i = if foreign == 0 {
            i
        } else {
            let p = &self.packages[i].1;
            let link = p
                .links
                .iter()
                .find(|l| l.package_id_shifted == foreign << 16)
                .ok_or_else(|| format!("unknown RPK link {foreign}"))?
                .name
                .clone();
            self.package(&link)?
        };
        Ok(((i as u32) << 16) | (id & 0xffff))
    }
    /// Register the package named by a mapped resource ID, if not yet done.
    fn request_metadata(&mut self, id: u32) -> Result<(), String> {
        let index = (id >> 16) as usize;
        let (name, package) = self
            .packages
            .get(index)
            .ok_or("mapped package is not available")?;
        if !package.entries.iter().any(|e| e.type_id == id & 0xffff) {
            return Err("mapped resource is outside the known package; original absence behavior is not asserted".into());
        }
        if index == 0 {
            return Err("the system package is registered by the boot driver".into());
        }
        if self.registered.contains_key(&index) {
            return Ok(());
        }
        let name = name.clone();
        let links: Vec<String> = package.links.iter().map(|l| l.name.clone()).collect();
        for link in links {
            // A missing linked file is tolerated: its entries stay absent.
            let _ = self.package(&link);
        }
        let installed = slr_resource::node::with_res(|host| {
            slr_resource::package::install_package(host, &self.packages, index)
        })?;
        println!(
            "BOOT resource metadata {name} entries={}; declared foreign roots {}; no descriptor materialization",
            installed.ids.len(),
            installed.declared_foreign_roots.len()
        );
        self.registered.insert(index, installed);
        Ok(())
    }
    fn write_trace(&self, output: &Path) -> Result<(), String> {
        let snapshot = slr_resource::node::with_res(|host| {
            let mut text = String::from("CONTROLLED package header registration; foreign roots declared empty, their own hierarchy and other children unmodeled; no descriptor parsing/materialization\n");
            let mut indices: Vec<_> = self.registered.keys().copied().collect();
            indices.sort_unstable();
            for index in indices {
                let installed = &self.registered[&index];
                text.push_str(&format!(
                    "package {index} {}: entries={} declared_foreign_roots={:?}\n",
                    self.packages[index].0,
                    installed.ids.len(),
                    installed.declared_foreign_roots
                ));
                for id in &installed.ids {
                    let container = host
                        .container_for_id(*id)
                        .and_then(|word| host.container(word));
                    text.push_str(&format!("  {id:#010x} {container:?}\n"));
                }
            }
            text.push_str(&format!("events={:#?}\n", host.events()));
            text
        });
        std::fs::write(output.join("resource-registration.txt"), snapshot)
            .map_err(|e| format!("ResourceTraceBoundary: {e}"))
    }
    /// The descriptor chain of a type-8 template, root first, following
    /// `super_id` across package links (`id` is `package << 16 | local`).
    fn template_phases(&self, id: u32) -> Result<Vec<(u32, String)>, String> {
        let mut next = id;
        let mut visited = std::collections::HashSet::new();
        let mut phases = vec![];
        while next & 0xffff != 0 {
            if !visited.insert(next) {
                return Err("type8 inheritance cycle".into());
            }
            let (_, p) = self
                .packages
                .get((next >> 16) as usize)
                .ok_or_else(|| format!("template package of {next:#x} is not installed"))?;
            let e = p
                .entries
                .iter()
                .find(|e| e.type_id == next & 0xffff)
                .ok_or_else(|| format!("missing type8 resource {next:#x}"))?;
            if e.typeof_ != 8 || e.flags != 0 {
                return Err(format!(
                    "unsupported external template {next:#x} kind={} flags={}",
                    e.typeof_, e.flags
                ));
            }
            phases.push((next, p.descriptor(e)));
            let parent = e.super_id;
            next = if parent >> 16 == 0 {
                (next & 0xffff_0000) | parent
            } else {
                let link = p
                    .links
                    .iter()
                    .find(|l| l.package_id_shifted == (parent >> 16) << 16)
                    .ok_or_else(|| format!("unknown RPK link {} for {next:#x}", parent >> 16))?
                    .name
                    .trim_end_matches('\0')
                    .replace('\\', "/")
                    .to_ascii_lowercase();
                let index = self
                    .packages
                    .iter()
                    .position(|(n, _)| *n == link)
                    .ok_or_else(|| format!("linked package {link} is not installed"))?;
                ((index as u32) << 16) | (parent & 0xffff)
            };
        }
        phases.reverse();
        if phases.is_empty() {
            return Err(format!("template {id:#x} has no descriptor chain"));
        }
        Ok(phases)
    }
    fn script(&self, id: u32) -> Result<String, String> {
        let p = &self
            .packages
            .get((id >> 16) as usize)
            .ok_or("missing RPK")?
            .1;
        let e = p
            .entries
            .iter()
            .find(|e| e.type_id == id & 0xffff)
            .ok_or_else(|| format!("missing resource {id:#x}"))?;
        let d = p.descriptor(e);
        d.lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("script ")
                    .map(|s| s.trim().trim_end_matches('\0').to_owned())
            })
            .ok_or_else(|| format!("resource {id:#x} has no script: {d:?}"))
    }
}
thread_local! {pub(crate) static RES: RefCell<Resources> = RefCell::new(Resources::default());}
fn resolve(_: &mut VmHost, package: Option<&str>, id: u32) -> Result<u32, VmError> {
    RES.with(|r| r.borrow_mut().resolve(package, id))
        .map_err(|e| {
            eprintln!("RESOURCE STOP {e}");
            VmError::Unsupported(0x100e)
        })
}
fn observe(info: &CallInfo<'_>, result: Option<u32>) {
    if std::env::var_os("SLR_TRACE_NATIVES").is_some() {
        eprintln!(
            "NATIVE {}.{}{} -> {result:?}",
            info.class, info.name, info.sig
        );
    }
    if info.class.starts_with("java.render.")
        || info.class.starts_with("java.sound.")
        || ["java.io.Input", "java.io.MouseCursor", "java.io.Controller"].contains(&info.class)
    {
        return;
    }

    slr_calllog::headless::record(slr_calllog::LogRecord::NativeCall {
        class: info.class.into(),
        name: info.name.into(),
        args: info
            .args
            .iter()
            .rev()
            .skip(1)
            .map(|v| {
                if v.tag == slr_vm::values::TypeTag::Float {
                    slr_calllog::Value::FloatBits(v.payload)
                } else {
                    slr_calllog::Value::Int(v.payload as i32)
                }
            })
            .collect(),
        result: result.map(|v| {
            if slr_vm::host::return_type(info.sig) == Some(slr_vm::values::TypeTag::Float) {
                slr_calllog::Value::FloatBits(v)
            } else {
                slr_calllog::Value::Int(v as i32)
            }
        }),
    });
}
pub(crate) fn script_stop(vm: &VmHost, thread: &Thread, phase: &str, error: VmError) -> String {
    let mut frames = Vec::new();
    for (index, frame) in thread.frames.iter().enumerate() {
        let pc = if index + 1 == thread.frames.len() {
            thread.pc
        } else {
            frame.saved_pc
        };
        let Some(pc) = pc else { continue };
        let Some(owner) = vm.loader.body_owner(pc.body) else {
            continue;
        };
        let Some(class) = vm.loader.find_by_id(owner) else {
            continue;
        };
        let method = class.methods.iter().find(|m| {
            !m.is_native && vm.loader.body_id_of(owner, m.body as usize) == Some(pc.body)
        });
        frames.push(format!(
            "{}.{}{} node{}",
            class.name,
            method.map_or("<field>", |m| m.name.as_str()),
            method.map_or("", |m| m.sig.as_str()),
            pc.idx
        ));
    }
    format!(
        "{phase}: {error:?}; script frames [{}]; source line {}",
        frames.join(" -> "),
        thread.line
    )
}

fn native_boundary(info: &CallInfo<'_>) -> Result<(), String> {
    if info.class == "java.lang.GameType"
        && !matches!(
            info.name,
            "createNativeInstance"
                | "setEventMask"
                | "clearEventMask"
                | "addNotification"
                | "remNotification"
                | "addTimer"
                | "removeAllTimers"
                | "registerCallback"
                | "unregisterCallback"
                | "unregisterCallbacks"
        )
    {
        return Err("GameType engine collaborator is not installed".into());
    }
    Ok(())
}

fn resource_set(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut words = [0; 2];
    if let Err(error) = slr_vm::host::marshal_args(info, &mut words) {
        vm.native_failure = Some(format!(
            "{}.{}{}: resource arguments {error:?}",
            info.class, info.name, info.sig
        ));
        return None;
    }
    let ptr = vm
        .read_field(words[0], "ptr")
        .ok()
        .map(|value| value.payload);
    let requires_load = words[1] != 0
        && slr_resource::node::with_res(|h| {
            ptr.and_then(|ptr| h.node(ptr))
                .is_some_and(|node| node.id != words[1] && h.container_for_id(words[1]).is_none())
        });
    if std::env::var_os("SLR_TRACE_RES").is_some() {
        eprintln!(
            "RES set receiver={:#x} id={:#010x} requires_load={requires_load}",
            words[0], words[1]
        );
    }
    if requires_load {
        if let Err(error) = RES.with(|r| r.borrow_mut().request_metadata(words[1])) {
            vm.native_failure = Some(format!(
                "{}.{}{}: ResourceLoadingBoundary: mapped resource {:#010x}: {error}",
                info.class, info.name, info.sig, words[1]
            ));
            return None;
        }
    }
    slr_resource::node::bridge(vm, info)
}
/// `java.lang.System.openLib(String)I` (`LAB_00487be0`): register the package
/// by name with the package manager (`FUN_00538380`, index = first-use order)
/// and force its header load by resolving local resource 1 (`FUN_00545fc0`).
/// The answer is the package index, or -1 when that resolution fails.
fn open_lib(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut words = [0; 2];
    if let Err(error) = slr_vm::host::marshal_args(info, &mut words) {
        vm.native_failure = Some(format!("System.openLib: {error:?}"));
        return None;
    }
    let Some(name) = vm
        .string_bytes(words[1])
        .map(|b| String::from_utf8_lossy(b).into_owned())
    else {
        vm.native_failure = Some("System.openLib: null or invalid package name".into());
        return None;
    };
    let result = RES.with(|r| {
        let mut r = r.borrow_mut();
        let index = match r.package(&name) {
            Ok(index) => index,
            Err(_) => return Ok(u32::MAX),
        };
        let has_entry_one = r.packages[index].1.entries.iter().any(|e| e.type_id == 1);
        if !has_entry_one {
            return Ok(u32::MAX);
        }
        if index != 0 {
            r.request_metadata(((index as u32) << 16) | 1)?;
        }
        Ok::<u32, String>(index as u32)
    });
    match result {
        Ok(index) => Some(index),
        Err(error) => {
            vm.native_failure = Some(format!("System.openLib {name:?}: {error}"));
            None
        }
    }
}
/// `java.lang.System.arraycopy(Object,int,Object,int,int)V` (`LAB_00487660`) over
/// the VM's array cells: payload words only, destination cell types retained,
/// a same-array copy staged through a temporary buffer. An out-of-range request
/// is the original's logged no-op; negative indices fault in the original.
fn array_copy(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut w = [0u32; 6];
    if let Err(error) = slr_vm::host::marshal_args(info, &mut w) {
        vm.native_failure = Some(format!("System.arraycopy: {error:?}"));
        return None;
    }
    let (src, src_pos, dst, dst_pos, count) = (w[1], w[2] as i32, w[3], w[4] as i32, w[5] as i32);
    let src_len = vm.array_cells_mut(src).map(|c| c.len() as i32);
    let dst_len = vm.array_cells_mut(dst).map(|c| c.len() as i32);
    let (Some(src_len), Some(dst_len)) = (src_len, dst_len) else {
        vm.native_failure = Some("System.arraycopy: source or destination is not an array".into());
        return None;
    };
    if src_pos.wrapping_add(count) > src_len || dst_pos.wrapping_add(count) > dst_len {
        vm.method_diagnostics.push("System.arraycopy: out of range");
        return None;
    }
    if count == 0 {
        return None;
    }
    if src_pos < 0 || dst_pos < 0 || count < 0 {
        vm.native_failure =
            Some("System.arraycopy: negative index faults in the executable".into());
        return None;
    }
    let words: Vec<u32> = vm.array_cells_mut(src)?[src_pos as usize..(src_pos + count) as usize]
        .iter()
        .map(|v| v.payload)
        .collect();
    let cells = vm.array_cells_mut(dst)?;
    for (i, word) in words.into_iter().enumerate() {
        cells[dst_pos as usize + i].payload = word;
    }
    None
}
/// Declared scenario: the DirectInput devices the headless machine reports,
/// keyboard and mouse only (the oracle's platform profile). Names are the
/// scenario's, not observed instance names.
const INPUT_DEVICES: [&str; 2] = ["Keyboard", "Mouse"];

/// `java.io.Input.getDeviceName(I)String` (`LAB_0047c9c0`): the declared
/// device's name, or null past the last device (the loop terminator scripts
/// rely on; the device-less port interns the empty string forever instead).
fn input_device_name(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut words = [0; 2];
    slr_vm::host::marshal_args(info, &mut words).ok()?;
    let name = INPUT_DEVICES.get(words[1] as usize);
    let result = match name {
        Some(name) => vm.intern_string(Some(name.as_bytes())).ok()?,
        None => 0,
    };
    Some(result)
}
/// Capture-only body for a registered row nothing implements yet: the call is
/// logged by the native observer and the row answers the zero word.
fn capture_only(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    (!info.sig.ends_with('V')).then_some(0)
}
fn install(vm: &mut VmHost) {
    let mut table = slr_lang::NativeTable::new();
    slr_lang::math::install(&mut table);
    slr_lang::number::install(&mut table);
    slr_lang::string::install(&mut table);
    slr_lang::object::install(&mut table);
    slr_lang::system::install(&mut table);
    slr_lang::langobj::gametype::install(&mut table);
    slr_lang::install(vm, &table);
    slr_lang::thread::install(vm);
    slr_lang::vec::install(vm);
    slr_io::install(vm);
    slr_io::findfile::install(vm);
    slr_io::resid::install(vm);
    slr_resource::node::install(vm);
    slr_render_natives::install_headless(vm);
    slr_input::install_headless(vm);
    slr_sound::install_headless(vm);
    vm.native_impls
        .insert(0x00481a70, factory::create_native_instance);
    vm.native_impls
        .insert(slr_resource::node::SET_ID, resource_set);
    engine::install(vm);
    vm.native_impls
        .insert(slr_io::input::INPUT_GET_DEVICE_NAME, input_device_name);
    vm.native_impls
        .insert(slr_lang::langobj::system::OPEN_LIB, open_lib);
    vm.native_impls
        .insert(slr_lang::langobj::system::ARRAYCOPY, array_copy);
    vm.native_impls
        .insert(slr_resource::node::CREATE, factory::game_ref_create);
    vm.native_impls
        .insert(slr_resource::node::CREATE_NATIVE, factory::game_ref_create);
    for &(class, name, sig, address) in missing_rows::MISSING {
        if vm.natives.resolve(class, name, sig).is_none() && !vm.native_impls.contains_key(&address)
        {
            vm.natives.register(class, name, sig, address);
            vm.native_impls.insert(address, capture_only);
        }
    }
    for address in [
        slr_resource::node::LOAD,
        slr_resource::node::CACHE,
        slr_resource::node::PRECACHE,
    ] {
        vm.native_impls.insert(address, materialise::load_like);
    }
    slr_lang::seam::with_lang_host(|h| h.clock_source = Some(sched::clock_f32));
    vm.native_impls
        .insert(slr_lang::thread::THREAD_START, sched::thread_start);
    vm.native_thread_hook = Some(sched::hook);
    vm.native_observer = Some(observe);
    vm.native_boundary = Some(native_boundary);
}
struct ConfigSource<'a> {
    vm: &'a mut VmHost,
    object: u32,
}
impl Source for ConfigSource<'_> {
    fn read(&mut self, kind: Kind, name: &'static str) -> Result<u32, String> {
        let value = match self.vm.read_field(self.object, name) {
            Ok(value) => value,
            Err(slr_vm::host::HostError::MissingMember) if kind == Kind::Has => return Ok(0),
            Err(error) => return Err(format!("Config.{name}: {error:?}")),
        };
        if kind == Kind::Has {
            return Ok(1);
        }
        let expected = if kind == Kind::Float {
            slr_vm::values::TypeTag::Float
        } else {
            slr_vm::values::TypeTag::Int
        };
        if value.tag != expected {
            return Err(format!(
                "Config.{name} expected {expected:?}, found {:?}",
                value.tag
            ));
        }
        Ok(value.payload)
    }
}
fn apply_settings(vm: &mut VmHost, object: u32, output: &Path) -> Result<Settings, String> {
    use std::io::Write;
    let mut settings = Settings::default();
    let outcome = settings.apply(&mut ConfigSource { vm, object });
    let mut log = std::io::BufWriter::new(
        std::fs::File::create(output.join("settings-operations.txt"))
            .map_err(|e| format!("SettingsLogBoundary: {e}"))?,
    );
    for event in &settings.events {
        writeln!(log, "{event:?}").map_err(|e| format!("SettingsLogBoundary: {e}"))?;
    }
    log.flush()
        .map_err(|e| format!("SettingsLogBoundary: {e}"))?;
    outcome?;
    Ok(settings)
}
fn run(
    root: &Path,
    vm: &mut VmHost,
    config_instance_id: Option<u32>,
    output: &Path,
    game_init_instance_id: Option<u32>,
    frames: u64,
) -> Result<(), String> {
    RES.with(|r| {
        let mut r = r.borrow_mut();
        r.root = root.into();
        r.package("system.rpk")
    })
    .map_err(|e| e.to_string())?;
    println!("BOOT RPK system.rpk");
    vm.boot();
    vm.loader.set_reader(FileReader::stock(root.into()));
    vm.loader.build_package_cache(root);
    for name in slr_vm::vmclass::boot::BOOT_CLASSES {
        vm.loader
            .request(name)
            .map_err(|e| format!("eager {name}: {e:?}"))?
            .ok_or_else(|| format!("missing eager class {name}"))?;
        println!("BOOT eager {name}");
    }
    install(vm);
    vm.resource_resolver = Some(resolve);
    println!(
        "BOOT native bindings={} implementations={}",
        vm.natives.len(),
        vm.native_impls.len()
    );
    let config = RES
        .with(|r| {
            r.borrow().packages[0]
                .1
                .entries
                .iter()
                .find(|e| e.alias.trim_end_matches('\0') == "Config")
                .map(|e| e.type_id)
        })
        .ok_or("system.rpk has no Config")?;
    let class = RES.with(|r| r.borrow().script(config))?;
    let config_class = vm
        .loader
        .request(&class)
        .map_err(|e| format!("Config load {e:?}"))?
        .ok_or("Config class not found")?;
    println!("BOOT Config resource={config:#010x} class={class}");
    let object = vm
        .alloc_object_id(config_class)
        .map_err(|e| format!("Config allocation: {e:?}"))?;
    println!("BOOT Config object={object:#x}");
    let created=config_instance_id.ok_or("ResourceCreationBoundary: Config created resource ID is not pinned; supply --config-instance-id for controlled script replay")?;
    println!("CONTROLLED Config created resource ID={created:#010x}; original startup allocation history not replayed");
    let link = slr_resource::node::link_created_script(vm, object, created)
        .map_err(|e| format!("ConfigNativeLinkBoundary: {e}"))?;
    factory::attach_created(link).map_err(|e| format!("ConfigChildRegistrationBoundary: {e}"))?;
    println!("BOOT Config native link {link:?}");
    sched::run_created(
        vm,
        "THRD-CREATE",
        config_class,
        object,
        "<init>",
        &[Value::int(created as i32)],
    )
    .map_err(|(thread, e)| script_stop(vm, &thread, "Config constructor", e))?;
    let retained = slr_resource::node::link_created_script(vm, object, created)
        .map_err(|e| format!("Config constructor changed native link: {e}"))?;
    if retained != link {
        return Err("Config constructor replaced native link".into());
    }
    slr_resource::node::finish_created_script(link)
        .map_err(|e| format!("Config retained link release: {e}"))?;
    println!("BOOT Config constructor retained native resource link");
    let mut settings = apply_settings(vm, object, output)?;
    println!(
        "BOOT settings global words={} final refresh requested",
        settings.globals.len()
    );
    settings.finish_refresh(false)?;
    println!("CONTROLLED original refresh absent-backend branch: no handle writes or engine calls");
    let init = vm
        .read_field(object, "init")
        .map_err(|e| format!("Config.init: {e:?}"))?
        .payload;
    println!("BOOT Config.init={init:#010x}");
    let script = RES.with(|r| r.borrow().script(init))?;
    println!("BOOT GameInit resource={init:#010x} class={script}");
    let created = game_init_instance_id.ok_or("GameInitCreationBoundary: supply --game-init-instance-id for controlled creation replay; original allocation history remains unpinned")?;
    let class = vm
        .loader
        .request(&script)
        .map_err(|e| format!("GameInit class: {e:?}"))?
        .ok_or("GameInit class missing")?;
    let object = vm
        .alloc_object_id(class)
        .map_err(|e| format!("GameInit allocation: {e:?}"))?;
    let link = slr_resource::node::link_created_script(vm, object, created)
        .map_err(|e| format!("GameInitNativeLinkBoundary: {e}"))?;
    factory::attach_created(link).map_err(|e| format!("GameInitChildRegistrationBoundary: {e}"))?;
    println!("CONTROLLED GameInit created ID={created:#010x}; native link {link:?}");
    sched::run_created(
        vm,
        "THRD-CREATE",
        class,
        object,
        "<init>",
        &[Value::int(created as i32)],
    )
    .map_err(|(thread, e)| script_stop(vm, &thread, "GameInit constructor", e))?;
    slr_resource::node::finish_created_script(link)
        .map_err(|e| format!("GameInit retained link release: {e}"))?;
    println!("BOOT GameInit constructor returned; pumping scheduler for {frames} frames");
    let mut pump = sched::Pump::new();
    for frame in 0..frames {
        engine::step(vm).map_err(|e| format!("engine frame {frame}: {e}"))?;
        pump.tick(vm).map_err(|f| {
            script_stop(
                vm,
                &f.thread,
                &format!("scheduler frame {frame} thread {} {:?}", f.id, f.name),
                f.error,
            )
        })?;
        sched::advance_ms(1000.0 / 60.0);
        engine::animate_pass(vm).map_err(|e| format!("animate frame {frame}: {e}"))?;
    }
    println!(
        "BOOT scheduler frames={frames} ticks={} thread runs={} simulated ms={:.3}",
        pump.ticks,
        pump.runs,
        sched::now_ms()
    );
    Err("MainMenuBoundary: frame budget exhausted before MainMenu was observed".into())
}
/// Fresh writable filesystem projection. No script I/O points at stock.
fn project_stock(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_symlink() {
            return Err(std::io::Error::other(
                "stock projection rejects symbolic links",
            ));
        } else if kind.is_dir() {
            project_stock(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn execute() -> Result<(), String> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| format!("RepositoryBoundary: {e}"))?;
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| repository.join("stock"))
        .canonicalize()
        .map_err(|e| format!("StockBoundary: {e}"))?;
    // Validate before creating any projection or output directories.
    slr_io::stock::verify_stock(&root).map_err(|e| format!("StockBoundary: {e}"))?;
    let output = repository.join("re/out/boot");
    if output.starts_with(&root) {
        return Err("ProjectionBoundary: output would be inside the source install".into());
    }
    std::fs::create_dir_all(&output).map_err(|e| format!("OutputBoundary: {e}"))?;
    let output = output
        .canonicalize()
        .map_err(|e| format!("OutputBoundary: {e}"))?;
    if output.starts_with(&root) {
        return Err("ProjectionBoundary: canonical output is inside the source install".into());
    }
    let mut config_instance_id = None;
    let mut game_init_instance_id = None;
    let mut frames = 600u64;
    let mut clock_step = 0.01f64;
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        if arg == "--config-instance-id" || arg == "--game-init-instance-id" {
            let text = args
                .next()
                .ok_or("ArgumentBoundary: --config-instance-id needs a value")?;
            let value = if let Some(text) = text.strip_prefix("0x") {
                u32::from_str_radix(text, 16)
            } else {
                text.parse()
            }
            .map_err(|e| format!("ArgumentBoundary: invalid instance ID: {e}"))?;
            if arg == "--config-instance-id" {
                config_instance_id = Some(value);
            } else {
                game_init_instance_id = Some(value);
            }
        } else if arg == "--frames" || arg == "--clock-step" {
            let text = args
                .next()
                .ok_or_else(|| format!("ArgumentBoundary: {arg} needs a value"))?;
            if arg == "--frames" {
                frames = text
                    .parse()
                    .map_err(|e| format!("ArgumentBoundary: invalid frame count: {e}"))?;
            } else {
                clock_step = text
                    .parse()
                    .map_err(|e| format!("ArgumentBoundary: invalid clock step: {e}"))?;
            }
        } else {
            return Err(format!("ArgumentBoundary: unknown argument {arg}"));
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("ProjectionBoundary: {e}"))?
        .as_nanos();
    let files = output.join(format!("files/{}-{stamp}", std::process::id()));
    project_stock(&root, &files).map_err(|e| format!("ProjectionBoundary: {e}"))?;
    slr_io::reset(&files);
    slr_io::findfile::reset_finds(&files);
    factory::configure(config_instance_id, game_init_instance_id);
    sched::configure_clock(clock_step);
    engine::configure(engine::FrameConfig::default());
    let mut vm = VmHost::new();
    let outcome = run(
        &root,
        &mut vm,
        config_instance_id,
        &output,
        game_init_instance_id,
        frames,
    );
    factory::write_trace(&output)?;
    RES.with(|r| r.borrow().write_trace(&output))?;
    std::fs::write(
        output.join("engine-events.txt"),
        engine::take_trace().join("\n"),
    )
    .map_err(|e| format!("EngineTraceBoundary: {e}"))?;
    std::fs::write(
        output.join("materialisation.txt"),
        materialise::take_trace().join("\n"),
    )
    .map_err(|e| format!("MaterialiseTraceBoundary: {e}"))?;
    std::fs::write(
        output.join("initializer-constructors.txt"),
        format!("{:#?}", vm.constructor_calls),
    )
    .map_err(|e| format!("ConstructorTraceBoundary: {e}"))?;
    let file = std::fs::File::create(output.join("runtime.calllog"))
        .map_err(|e| format!("CallLogBoundary: {e}"))?;
    let mut writer =
        slr_calllog::LogWriter::create(file).map_err(|e| format!("CallLogBoundary: {e}"))?;
    for record in slr_calllog::headless::take_records() {
        writer
            .record(&record)
            .map_err(|e| format!("CallLogBoundary: {e}"))?;
    }
    writer
        .flush()
        .map_err(|e| format!("CallLogBoundary: {e}"))?;
    outcome.map_err(|error| {
        if let Some(failure) = vm.native_failure {
            return format!("{error}; NativeBackendBoundary: {failure}");
        }
        if let Some(failure) = vm.initializer_failure {
            return format!("{error}; InitializerBoundary: {failure}");
        }
        if let Some(native) = vm.missing_native {
            format!("{error}; NoBody: {native}")
        } else if let Some(detail) = slr_vm::host::last_host_error() {
            format!("{error}; last host error: {detail}")
        } else {
            error
        }
    })
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("STOP {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn template_chains_follow_super_ids_and_missing_packages_fail_closed() {
        let mut bytes = b"RPAK".to_vec();
        for word in [0x200u32, 0, 0, 0, 0, 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        let mut package = Package::parse(bytes).unwrap();
        for (id, parent) in [(1, 0), (27, 1), (28, 27), (2, 0), (3, 2)] {
            package.entries.push(slr_fmt_rpak::Entry {
                super_id: parent,
                type_id: id,
                typeof_: 8,
                flags: 0,
                is_parent_compatible: 1.0,
                rsd_offset: 0,
                rsd_size: 0,
                alias: String::new(),
                spatial: None,
            });
        }
        let resources = Resources {
            root: PathBuf::new(),
            packages: vec![("system.rpk".into(), package)],
            registered: Default::default(),
        };
        assert_eq!(
            resources
                .template_phases(27)
                .unwrap()
                .iter()
                .map(|p| p.0)
                .collect::<Vec<_>>(),
            vec![1, 27]
        );
        let chain = |id| {
            resources
                .template_phases(id)
                .unwrap()
                .iter()
                .map(|p| p.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(chain(28), vec![1, 27, 28]);
        assert_eq!(chain(3), vec![2, 3]);
        assert_eq!(chain(1), vec![1]);
        // A package that is not installed fails closed.
        assert!(resources.template_phases(0x1001b).is_err());
    }
    #[test]
    fn resource_set_stops_on_unpopulated_id_without_clearing_existing_link() {
        use slr_vm::{
            values::Value,
            vmclass::{Class, Field},
        };
        let mut vm = VmHost::new();
        for (name, base, fields) in [
            (
                "java.lang.Native",
                None,
                vec![Field {
                    class: "java.lang.Native".into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }],
            ),
            (
                slr_resource::node::RESOURCE_REF_CLASS,
                Some("java.lang.Native"),
                vec![],
            ),
        ] {
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: name.into(),
                base: base.map(str::to_owned),
                interfaces: vec![],
                fields,
                methods: vec![],
                bodies: vec![],
            });
        }
        slr_resource::node::reset_res();
        slr_resource::node::install(&mut vm);
        let object = vm
            .alloc_object(slr_resource::node::RESOURCE_REF_CLASS)
            .unwrap();
        slr_resource::node::bridge(
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                class: slr_resource::node::RESOURCE_REF_CLASS,
                name: "newNative",
                sig: "()V",
                args: &[Value::reference(object)],
            },
        );
        slr_resource::node::with_res(|h| {
            let c = h.add_container(slr_resource::node::Container::new(7, 123));
            h.bind_id(123, c);
        });
        for id in [123, 0x100a8] {
            resource_set(
                &mut vm,
                &CallInfo {
                    jvm: 0,
                    thread: 0,
                    class: slr_resource::node::RESOURCE_REF_CLASS,
                    name: "set",
                    sig: "(I)V",
                    args: &[Value::int(id), Value::reference(object)],
                },
            );
            if id == 123 {
                assert!(vm.native_failure.is_none());
                let ptr = vm.read_field(object, "ptr").unwrap().payload;
                let before = slr_resource::node::with_res(|h| {
                    h.bind_id(123, 0);
                    (
                        h.node(ptr).unwrap().clone(),
                        h.node_count(),
                        h.events().len(),
                    )
                });
                resource_set(
                    &mut vm,
                    &CallInfo {
                        jvm: 0,
                        thread: 0,
                        class: slr_resource::node::RESOURCE_REF_CLASS,
                        name: "set",
                        sig: "(I)V",
                        args: &[Value::int(123), Value::reference(object)],
                    },
                );
                assert!(vm.native_failure.is_none());
                slr_resource::node::with_res(|h| {
                    assert_eq!(
                        (
                            h.node(ptr).unwrap().clone(),
                            h.node_count(),
                            h.events().len()
                        ),
                        before
                    )
                });
            } else {
                assert!(vm
                    .native_failure
                    .as_deref()
                    .unwrap()
                    .contains("ResourceLoadingBoundary: mapped resource 0x000100a8"));
            }
        }
        let ptr = vm.read_field(object, "ptr").unwrap().payload;
        slr_resource::node::with_res(|h| assert_eq!(h.node(ptr).unwrap().id, 123));
        vm.native_failure = None;
        resource_set(
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                class: slr_resource::node::RESOURCE_REF_CLASS,
                name: "set",
                sig: "(I)V",
                args: &[Value::int(0), Value::reference(object)],
            },
        );
        assert!(vm.native_failure.is_none());
        slr_resource::node::with_res(|h| assert_eq!(h.node(ptr).unwrap().id, 0));
    }
}
