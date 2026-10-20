//! Headless stock bootstrap. All script bodies come from the read-only install.
use slr_fmt_rpak::Package;
use slr_vm::{
    host::{CallInfo, VmHost},
    interp::{Thread, VmError},
    values::Value,
    vmclass::FileReader,
};
use slr_x87::X87;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Resources {
    root: PathBuf,
    packages: Vec<(String, Package)>,
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
thread_local! {static RES: RefCell<Resources> = RefCell::new(Resources::default());}
fn resolve(_: &mut VmHost, package: Option<&str>, id: u32) -> Result<u32, VmError> {
    RES.with(|r| r.borrow_mut().resolve(package, id))
        .map_err(|e| {
            eprintln!("RESOURCE STOP {e}");
            VmError::Unsupported(0x100e)
        })
}
fn observe(info: &CallInfo<'_>, result: Option<u32>) {
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
    vm.native_observer = Some(observe);
}
fn run(root: &Path, vm: &mut VmHost, config_instance_id: Option<u32>) -> Result<(), String> {
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
    let mut thread = Thread::new();
    vm.enter_method(
        &mut thread,
        config_class,
        object,
        "<init>",
        &[Value::int(created as i32)],
    )
    .map_err(|e| format!("Config constructor dispatch: {e:?}"))?;
    thread
        .run(vm, 0.0, &mut || X87::from_f64(0.0))
        .map_err(|e| format!("Config constructor: {e:?} pc={:?}", thread.pc))?;
    let init = vm
        .read_field(object, "init")
        .map_err(|e| format!("Config.init: {e:?}"))?
        .payload;
    println!("BOOT Config.init={init:#010x}");
    let script = RES.with(|r| r.borrow().script(init))?;
    println!("BOOT GameInit resource={init:#010x} class={script}");
    Err(format!(
        "NativeSettingsBoundary: FUN_00427bf0 has not run; resolved Config.init={init:#010x}"
    ))
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
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        if arg == "--config-instance-id" {
            let text = args
                .next()
                .ok_or("ArgumentBoundary: --config-instance-id needs a value")?;
            let value = if let Some(text) = text.strip_prefix("0x") {
                u32::from_str_radix(text, 16)
            } else {
                text.parse()
            }
            .map_err(|e| format!("ArgumentBoundary: invalid instance ID: {e}"))?;
            config_instance_id = Some(value);
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
    let mut vm = VmHost::new();
    let outcome = run(&root, &mut vm, config_instance_id);
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
        if let Some(native) = vm.missing_native {
            format!("{error}; NoBody: {native}")
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
