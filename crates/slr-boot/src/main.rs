//! Headless stock bootstrap. All script bodies come from the read-only install.
mod engine;
mod factory;
mod gc_runtime;
mod ground_environment;
mod live_input;
mod machine_recording;
mod materialise;
mod missing_rows;
mod native_assets;
mod native_body;
mod native_button;
mod native_callbacks;
mod native_camera;
mod native_car;
mod native_car_command;
mod native_car_query;
mod native_car_update;
mod native_cursor;
mod native_cursor_pick;
mod native_cursor_runtime;
mod native_defaults;
mod native_definition;
mod native_dynamics;
mod native_entity;
mod native_factory;
mod native_file;
mod native_fpu;
mod native_frame;
mod native_geometry;
mod native_image;
mod native_install;
mod native_install_runtime;
mod native_instance;
mod native_instance_new;
mod native_instance_runtime;
mod native_matrix;
mod native_matrix_interval;
mod native_mesh_inertia;
mod native_mesh_resource;
mod native_message;
mod native_notifications;
mod native_part;
mod native_player;
mod native_player_command;
mod native_player_device;
mod native_player_runtime;
mod native_postparse;
mod native_query;
mod native_query_frame;
mod native_render_graph;
mod native_render_lod;
mod native_render_resource;
mod native_render_type;
mod native_resource_base;
// Raw manager lookup is currently exercised by the snapshot adapter; the
// production installation host still uses the live typed manager projection.
mod native_keyboard;
#[cfg(test)]
mod native_resource_index;
mod native_resource_load;
mod native_resource_manager;
mod native_script_access;
mod native_shape;
mod native_shape_v4;
mod native_spatial;
mod native_spring;
mod native_stack;
mod native_vm_abi;
mod native_vm_alu;
mod native_vm_array_class;
mod native_vm_array_object;
mod native_vm_bootstrap;
mod native_vm_callback;
mod native_vm_class_context;
mod native_vm_class_init;
mod native_vm_context;
mod native_vm_counter;
mod native_vm_dispatch;
mod native_vm_fpu;
mod native_vm_jvm;
mod native_vm_live;
mod native_vm_lookup;
mod native_vm_metadata;
mod native_vm_namespace;
mod native_vm_pool;
mod native_vm_resource;
mod native_vm_root;
mod native_vm_runtime;
mod native_vm_signature;
#[cfg(test)]
mod native_vm_test_store;
mod native_vm_thread_lifecycle;
mod native_vm_thread_run;
mod native_vm_values;
mod native_world;
mod part_render;
mod parts;
mod presentation;
mod resource_identity;
mod sched;
#[cfg(test)]
mod snapshot_harness;
mod telemetry;
mod video;
mod viewport;
mod wheel;
mod world_scene;
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
    pub(crate) packages: Vec<(String, Option<Package>)>,
    registered: std::collections::HashMap<usize, slr_resource::package::Installed>,
    loading: std::collections::HashSet<usize>,
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
        let i = self.packages.len();
        if i >= 0xffff {
            return Err("package manager indices exhausted".into());
        }
        // FUN_00538380 reserves a name independently of file availability.
        self.packages.push((name, None));
        Ok(i)
    }
    fn read_package(&mut self, index: usize) -> Result<(), String> {
        let (name, package) = self
            .packages
            .get_mut(index)
            .ok_or("unknown package index")?;
        if package.is_none() {
            *package = Some(
                Package::open(self.root.join(&name)).map_err(|e| format!("RPK {name}: {e:?}"))?,
            );
        }
        Ok(())
    }
    fn resolve(&mut self, package: Option<&str>, id: u32) -> Result<u32, String> {
        let i = if let Some(package) = package {
            self.package(package)?
        } else {
            0
        };
        self.request_headers(i)?;
        let foreign = id >> 16;
        let i = if foreign == 0 {
            i
        } else {
            let p = self.packages[i]
                .1
                .as_ref()
                .ok_or("package header missing")?;
            let link = p
                .links
                .iter()
                .find(|l| l.package_id_shifted == foreign << 16)
                .ok_or_else(|| format!("unknown RPK link {foreign}"))?
                .name
                .clone();
            let linked = self.package(&link)?;
            self.request_headers(linked)?;
            linked
        };
        Ok(((i as u32) << 16) | (id & 0xffff))
    }
    /// Register the package named by a mapped resource ID, if not yet done.
    fn request_metadata(&mut self, id: u32) -> Result<(), String> {
        let index = (id >> 16) as usize;
        self.request_headers(index)?;
        let package = self
            .packages
            .get(index)
            .and_then(|(_, package)| package.as_ref())
            .ok_or("mapped package is not available")?;
        if !package.entries.iter().any(|e| e.type_id == id & 0xffff) {
            return Err("mapped resource is outside the known package; original absence behavior is not asserted".into());
        }
        // Controlled system roots pre-exist, but individual descriptors must
        // still acquire their real header before ResourceRef.set can bind them.
        if index == 0 && slr_resource::node::with_res(|h| h.container_for_id(id).is_none()) {
            let entry_index = package
                .entries
                .iter()
                .position(|e| e.type_id == id)
                .unwrap();
            let parent = package.entries[entry_index].super_id;
            if parent == 0 {
                return Err("unprovided system root".into());
            }
            self.request_metadata(parent)?;
            slr_resource::node::with_res(|h| {
                slr_resource::package::install_header(h, &self.packages, 0, entry_index)
            })?;
        }
        Ok(())
    }
    /// Load links and register headers in their original interleaved order.
    /// Each header resolves its foreign parent before joining that parent's
    /// child list; loading all dependencies first would reorder those lists.
    fn request_headers(&mut self, index: usize) -> Result<(), String> {
        self.read_package(index)?;
        // System roots are still provided by the controlled boot driver.
        if index == 0 {
            return Ok(());
        }
        if self.registered.contains_key(&index) {
            return Ok(());
        }
        if !self.loading.insert(index) {
            return Err(format!("cyclic package header dependency at index {index}"));
        }
        let result = (|| {
            let package = self.packages[index].1.as_ref().unwrap();
            let links = package.links.clone();
            let parents: Vec<_> = package.entries.iter().map(|entry| entry.super_id).collect();
            for link in &links {
                self.package(&link.name)?;
            }
            let mut installed = slr_resource::package::Installed {
                ids: Vec::new(),
                declared_foreign_roots: Vec::new(),
            };
            for (entry_index, parent) in parents.into_iter().enumerate() {
                if parent >> 16 != 0 {
                    let link = links
                        .iter()
                        .find(|link| link.package_id_shifted == parent & 0xffff_0000)
                        .ok_or("foreign resource parent link missing")?;
                    let foreign = self.package(&link.name)?;
                    if foreign != 0 {
                        // Missing stock obstacles retain the declared empty-root
                        // boundary; their names still consume original indices.
                        let name = &self.packages[foreign].0;
                        if self.root.join(name).exists() {
                            self.request_headers(foreign)?;
                        }
                    }
                }
                let entry = slr_resource::node::with_res(|host| {
                    slr_resource::package::install_header(host, &self.packages, index, entry_index)
                })?;
                installed.ids.extend(entry.ids);
                installed
                    .declared_foreign_roots
                    .extend(entry.declared_foreign_roots);
            }
            println!("BOOT resource metadata {} entries={}; declared foreign roots {}; no descriptor materialization",
                self.packages[index].0, installed.ids.len(), installed.declared_foreign_roots.len());
            self.registered.insert(index, installed);
            Ok(())
        })();
        self.loading.remove(&index);
        result
    }
    fn write_trace(&self, output: &Path) -> Result<(), String> {
        let indices: String = self
            .packages
            .iter()
            .enumerate()
            .map(|(index, (name, _))| format!("{index}\t{name}\n"))
            .collect();
        std::fs::write(output.join("package-indices.tsv"), indices)
            .map_err(|e| format!("PackageTraceBoundary: {e}"))?;
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
            let p = p.as_ref().ok_or("template package header missing")?;
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
            .1
            .as_ref()
            .ok_or("missing RPK header")?;
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
    // The render/sound/input capture hosts record their own calls; natives the
    // driver overrides in those classes are recorded here instead.
    let driver_owned = (info.class == "java.render.Viewport"
        && matches!(
            info.name,
            "create"
                | "destroy"
                | "resize"
                | "getAspect"
                | "getWidth"
                | "getHeight"
                | "getTop"
                | "getLeft"
        ))
        || (info.class == "java.render.Camera" && matches!(info.name, "create" | "destroy"))
        || (info.class == "java.render.Text" && info.name == "create")
        || (info.class == "java.io.Input" && info.name == "getDeviceName")
        || (info.class == "java.render.GfxEngine"
            && matches!(info.name, "displayModeName" | "openVideo"));
    if !driver_owned
        && (info.class.starts_with("java.render.")
            || info.class.starts_with("java.sound.")
            || ["java.io.Input", "java.io.MouseCursor", "java.io.Controller"].contains(&info.class))
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
    slr_vm::diagnostics::with_context(slr_vm::diagnostics::thread_context(thread, vm), || {
        slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::Failure {
            error: format!("{phase}: {error:?}"),
            snapshot: slr_vm::diagnostics::failure_snapshot(vm, thread),
        });
    });
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

fn live_native_boundary(
    context: &slr_vm::host::NativeContext<'_>,
    info: &CallInfo<'_>,
) -> Result<(), String> {
    machine_recording::drain(context.host)?;
    native_boundary(info)
}

fn native_boundary(info: &CallInfo<'_>) -> Result<(), String> {
    if native_body::has_active_native_frame() {
        return Err("NativeAbiBoundary: nested VM native entry requires the original callback/interpreter/wrapper CALL context".into());
    }
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

/// Original47c3a0 resets the loading sample ring in the live native memory.
fn loading_reset(vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    if let Err(error) = native_body::with_world_memory(|m| {
        let seed = m.r32(0x618d6c);
        for i in 0..32 {
            m.w32(0x765e8c + i * 4, seed);
        }
        m.w32(0x765f24, seed.wrapping_shl(5));
    }) {
        vm.native_failure = Some(error);
    }
    None
}
#[cfg(test)]
mod native_abi_guard_tests {
    use super::*;

    #[test]
    fn nested_vm_native_boundary_preserves_parent_scratch_and_returns_an_error() {
        native_body::clear();
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[],
            class: "java.util.resource.GameRef",
            name: "create",
            sig: "()Ljava.lang.GameType;",
        };
        assert!(native_boundary(&info).is_ok());
        native_body::with_world_memory(|m| {
            crate::native_stack::function(m, &[0x12345678], None, |m| {
                let parent_sp = crate::native_stack::sp(m);
                m.w32(parent_sp.wrapping_sub(0x40), 0x5a123456);
                let error =
                    native_body::during_vm_callback(m, || native_boundary(&info)).unwrap_err();
                assert!(error.contains("NativeAbiBoundary"));
                assert!(crate::native_stack::active(m));
                assert_eq!(crate::native_stack::sp(m), parent_sp);
                assert_eq!(m.r32(parent_sp.wrapping_sub(0x40)), 0x5a123456);
            });
            assert!(!crate::native_stack::active(m));
        })
        .unwrap();
        assert!(native_boundary(&info).is_ok());
    }
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
/// and force its header load (`FUN_00544170`). A loaded header answers the
/// package index even without local resource 1; an unavailable file answers -1.
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
        let cached =
            r.registered.contains_key(&index) || (index == 0 && r.packages[index].1.is_some());
        if !cached && !r.root.join(&r.packages[index].0).exists() {
            return Ok(u32::MAX);
        }
        r.request_headers(index)?;
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
    let src_len = vm.array_cells(src).map(|c| c.len() as i32);
    let dst_len = vm.array_cells(dst).map(|c| c.len() as i32);
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
    let words: Vec<u32> = vm.array_cells(src)?[src_pos as usize..(src_pos + count) as usize]
        .iter()
        .map(|v| v.payload)
        .collect();
    for (i, word) in words.into_iter().enumerate() {
        if let Err(error) = vm.write_array_payload(dst, dst_pos as usize + i, word) {
            vm.native_failure = Some(format!("System.arraycopy: {error:?}"));
            return None;
        }
    }
    None
}
#[cfg(test)]
mod array_copy_machine_tests {
    use super::*;
    use slr_vm::{
        interp::{HeapSlot, Host},
        machine::Event,
    };
    #[test]
    fn overlapping_copy_records_ordered_typed_payload_stores() {
        let mut vm = VmHost::new();
        vm.start_machine_recording().unwrap();
        let array = vm.alloc_array("I", 3).unwrap();
        for (index, payload) in [10, 20, 30].into_iter().enumerate() {
            vm.write_cell(HeapSlot::Array { word: array, index }, Value::int(payload))
                .unwrap();
        }
        let trace = vm.machine_trace();
        let before = trace.pending(usize::MAX);
        trace.acknowledge(before.last().unwrap().sequence).unwrap();
        let args = [
            Value::int(2),
            Value::int(1),
            Value::reference(array),
            Value::int(0),
            Value::reference(array),
            Value::reference(0),
        ];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.System",
            name: "arraycopy",
            sig: "(Ljava.lang.Object;ILjava.lang.Object;II)V",
        };
        assert_eq!(array_copy(&mut vm, &info), None);
        assert!(vm.native_failure.is_none());
        assert_eq!(
            vm.array_cells(array)
                .unwrap()
                .iter()
                .map(|v| v.payload)
                .collect::<Vec<_>>(),
            [10, 10, 20]
        );
        let events = trace.pending(usize::MAX);
        assert_eq!(events.len(), 2);
        for (entry, (index, before, after)) in events.iter().zip([(1, 20, 10), (2, 30, 20)]) {
            assert_eq!(
                entry.event,
                Event::HeapStore {
                    slot: HeapSlot::Array { word: array, index },
                    before: Value::int(before),
                    after: Value::int(after)
                }
            );
        }
    }
}
/// `java.io.File` natives over real VM objects and Strings. The file layer
/// keeps its own object table (name, handle) and string words, so the VM's
/// values are mirrored into it before each call and String results are
/// re-interned as VM Strings afterwards.
fn file_native(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    use slr_io::file as f;
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    let mut w = [0u32; 3];
    let _ = slr_vm::host::marshal_args(info, &mut w);
    let text = |vm: &VmHost, word: u32| vm.string_bytes(word).map(<[u8]>::to_vec);
    match address {
        f::FILE_OPEN => {
            let name = vm
                .read_field(w[0], "name")
                .ok()
                .and_then(|v| text(vm, v.payload))
                .unwrap_or_default();
            f::with_files(|files| files.set_name(w[0], &String::from_utf8_lossy(&name)));
        }
        f::FILE_WRITE_STRING => {
            if let Some(bytes) = text(vm, w[1]) {
                f::with_files(|files| files.set_string(w[1], &bytes));
            }
        }
        f::FILE_COPY | f::FILE_MOVE => {
            for word in [w[1], w[2]] {
                if let Some(bytes) = text(vm, word) {
                    f::with_files(|files| files.set_string(word, &bytes));
                }
            }
        }
        _ => {}
    }
    let result = f::bridge(vm, info);
    if address == f::FILE_READ_STRING {
        let word = result?;
        if word == 0 {
            return Some(0);
        }
        let bytes = f::with_files(|files| files.string(word).map(<[u8]>::to_vec));
        return Some(match bytes {
            Some(bytes) => vm.intern_string(Some(&bytes)).ok()?,
            None => word,
        });
    }
    result
}
/// Declared scenario: the DirectInput devices the headless machine reports,
/// keyboard and mouse only (the oracle's platform profile). Names are the
/// scenario's, not observed instance names.
const INPUT_DEVICES: [&str; 2] = ["SysKeyboard", "SysMouse"];

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
    ground_environment::install(vm);
    slr_render_natives::install_headless(vm);
    slr_input::install_headless(vm);
    slr_sound::install_headless(vm);
    vm.native_impls
        .insert(0x00481a70, factory::create_native_instance);
    vm.native_impls
        .insert(slr_resource::node::SET_ID, resource_set);
    engine::install(vm);
    viewport::install(vm);
    video::install(vm);
    for row in slr_io::file::FILE_NATIVE_BINDINGS {
        vm.native_impls.insert(row.impl_addr, file_native);
    }
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
    parts::install(vm);
    vm.native_impls
        .insert(0x0043_e320, native_car_update::force_update);
    wheel::install(vm);
    resource_identity::install(vm);
    slr_lang::seam::with_lang_host(|h| {
        h.clock_source = Some(sched::clock_f32);
        // Observed in the original run: the first `System.timeWarp(0.0)` answers 1.0.
        h.time_warp = 1.0;
    });
    vm.native_impls
        .insert(slr_lang::thread::THREAD_START, sched::thread_start);
    vm.native_thread_hook = Some(sched::hook);
    vm.native_observer = Some(observe);
    vm.native_boundary = Some(live_native_boundary);
    vm.native_impls
        .insert(slr_lang::langobj::system::IS_LOADING_RESET, loading_reset);
    presentation::install(vm);
    live_input::install(vm);
    if std::env::var_os("SLR_NATIVE_AUDIT").is_some() {
        audit_natives(vm);
    }
}
/// Temporary probe: report `Osd.iLevel` and `Osd.IL_TIPS` on every live `Osd`.
fn probe_osd_levels(vm: &mut VmHost) {
    for index in 0..vm.objects.len() {
        let word = 0x1000 + index as u32;
        let is_osd = matches!(
            &vm.objects[index].class,
            slr_vm::vmclass::ClassRef::Loaded(id)
                if vm.loader.class_name(*id) == Some("java.render.Osd")
        );
        if !is_osd {
            continue;
        }
        let level = vm.read_field(word, "iLevel").map(|v| v.payload);
        let tips = vm.read_field(word, "IL_TIPS").map(|v| v.payload);
        let ptr = vm.read_field(word, "ptr").map(|v| v.payload);
        let ptr = ptr.ok();
        let container = ptr.and_then(|p| {
            slr_resource::node::try_observe(|host| host.node(p).map(|n| n.container)).flatten()
        });
        let registered = crate::engine::registered_animate(container.unwrap_or(0));
        println!(
            "OSD {word:#x} iLevel={level:?} IL_TIPS={tips:?} ptr={ptr:?} container={container:?} animates={registered:?}"
        );
    }
}

/// Report how many of the registered natives still dispatch to `capture_only`.
fn audit_natives(vm: &VmHost) {
    let placeholder: fn(&mut VmHost, &CallInfo<'_>) -> Option<u32> = capture_only;
    let (mut real, mut stub, mut unbound) = (0usize, 0usize, 0usize);
    let mut stubs: Vec<String> = Vec::new();
    for record in vm.natives.sorted() {
        match vm.native_impls.get(&record.impl_addr) {
            None => unbound += 1,
            Some(f) if std::ptr::fn_addr_eq(*f, placeholder) => {
                stub += 1;
                stubs.push(format!(
                    "{}.{}{} @ {:#010x}",
                    record.key.class, record.key.name, record.key.sig, record.impl_addr
                ));
            }
            Some(_) => real += 1,
        }
    }
    println!("NATIVE_AUDIT real={real} capture_only={stub} unbound={unbound}");
    for row in stubs {
        println!("NATIVE_STUB {row}");
    }
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
        let index = r.package("system.rpk")?;
        r.read_package(index)
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
                .as_ref()
                .unwrap()
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
    let config_object = object;
    telemetry::anchor("Config", object);
    let created=config_instance_id.ok_or("ResourceCreationBoundary: Config created resource ID is not pinned; supply --config-instance-id for controlled script replay")?;
    println!("CONTROLLED Config created resource ID={created:#010x}; original startup allocation history not replayed");
    let link = slr_resource::node::link_created_script(vm, object, created)
        .map_err(|e| format!("ConfigNativeLinkBoundary: {e}"))?;
    factory::attach_created(link, config)
        .map_err(|e| format!("ConfigChildRegistrationBoundary: {e}"))?;
    println!("BOOT Config native link {link:?}");
    gc_runtime::configure_config(object)?;
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
    engine::register_created(vm, link.container, object)?;
    slr_resource::node::finish_created_script(link)
        .map_err(|e| format!("Config retained link release: {e}"))?;
    println!("BOOT Config constructor retained native resource link");
    let mut settings = apply_settings(vm, object, output)?;
    println!(
        "BOOT settings global words={} final refresh requested",
        settings.globals.len()
    );
    settings.finish_refresh(false)?;
    // The original graphics activation creates its D3D9 device without
    // FPU_PRESERVE. This phase transition is independent of replay tape data.
    vm.runtime_precision = slr_x87::Precision::F32;
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
    factory::attach_created(link, init)
        .map_err(|e| format!("GameInitChildRegistrationBoundary: {e}"))?;
    println!("CONTROLLED GameInit created ID={created:#010x}; native link {link:?}");
    telemetry::anchor("GameInit", object);
    sched::run_created(
        vm,
        "THRD-CREATE",
        class,
        object,
        "<init>",
        &[Value::int(created as i32)],
    )
    .map_err(|(thread, e)| script_stop(vm, &thread, "GameInit constructor", e))?;
    engine::register_created(vm, link.container, object)?;
    slr_resource::node::finish_created_script(link)
        .map_err(|e| format!("GameInit retained link release: {e}"))?;
    println!("BOOT GameInit constructor returned; pumping scheduler for {frames} frames");
    gc_runtime::configure_roots(config_object, object, None)?;
    sched::begin_replay_after_game_init()?;
    vm.runtime_clock = Some(sched::clock_ms);
    if sched::replaying() {
        engine::begin_replayed_frames();
    }
    std::fs::write(
        output.join("objects-before-frames.tsv"),
        vm.objects
            .iter()
            .enumerate()
            .map(|(index, object)| {
                format!(
                    "{}\t{}\t{:?}\n",
                    index + 1,
                    object.class_name(&vm.loader),
                    vm.string_bytes(index as u32 + 0x1000)
                        .map(String::from_utf8_lossy)
                )
            })
            .collect::<String>(),
    )
    .map_err(|e| format!("ObjectTraceBoundary: {e}"))?;
    let mut pump = sched::Pump::new();
    let mut menu = None;
    let mut menu_since = 0;
    let mut menu_idle_frames = 0;
    telemetry::snapshot(vm, "before-frames", None)?;
    let mut frame = 0;
    loop {
        if frame >= frames && !live_input::window_open() {
            break;
        }
        let started = std::time::Instant::now();
        slr_vm::diagnostics::set_frame(Some(frame));
        live_input::poll(vm)?;
        if live_input::closed() {
            break;
        }
        telemetry::detail("frame-enter", vec![("frame".into(), frame.to_string())]);
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
        if sched::replaying() {
            engine::sample_replayed_animation_time();
        }
        engine::animate_pass(vm).map_err(|e| format!("animate frame {frame}: {e}"))?;
        factory::cleanup(vm).map_err(|e| format!("resource cleanup frame {frame}: {e}"))?;
        //428d91/428da0: the declared successful headless device frame
        //increments the same generation that GfxEngine.flush advances.
        slr_render_natives::with_render(|render| render.advance_frame_generation());
        engine::notify_render_waiters(vm)
            .map_err(|e| format!("render notify frame {frame}: {e}"))?;
        if let Some(seconds) = sched::replayed_frame_dt(frame + 1)? {
            engine::set_frame_dt(seconds);
        }
        telemetry::detail("frame-exit", vec![("frame".into(), frame.to_string())]);
        if std::env::var_os("SLR_SESSION_CAPTURE").is_none() || frame % 60 == 0 {
            telemetry::snapshot(vm, "frame-end", None)?;
        }
        if frame % 60 == 0 {
            slr_telemetry::journal::emit(
                "worker-heartbeat",
                serde_json::json!({"frame":frame,"input_id":live_input::input_id(),"step_ns":started.elapsed().as_nanos()}),
            )?;
        }
        machine_recording::drain(vm)?;
        telemetry::check()?;
        if let Some(error) = sched::replay_error() {
            return Err(error);
        }
        if std::env::var_os("SLR_OSD_PROBE").is_some() && frame % 40 == 0 {
            probe_osd_levels(vm);
        }
        let active_menu = active_main_menu(vm).filter(|_| !sched::pending_menu_call(vm));
        presentation::frame(vm, active_menu.is_some(), output)?;
        if active_menu.is_some() {
            live_input::ready();
        }
        live_input::record(vm, output)?;
        if slr_lang::seam::with_lang_host(|h| h.exit_requested()) {
            break;
        }
        if active_menu.is_some() && active_menu == menu {
            menu_idle_frames += 1;
        } else {
            menu = active_menu;
            menu_since = frame;
            menu_idle_frames = 0;
        }
        frame += 1;
        if live_input::window_open() {
            if let Some(delay) = std::time::Duration::from_millis(16).checked_sub(started.elapsed())
            {
                std::thread::sleep(delay);
            }
        }
    }
    let frames = pump.ticks;
    println!(
        "BOOT scheduler frames={frames} ticks={} thread runs={} simulated ms={:.3}",
        pump.ticks,
        pump.runs,
        sched::now_ms()
    );
    live_input::finish()?;
    if live_input::closed()
        || live_input::interacted()
        || slr_lang::seam::with_lang_host(|h| h.exit_requested())
    {
        Ok(())
    } else if menu.is_some() && menu_idle_frames >= 300 {
        sched::finish_accepted()?;
        println!("BOOT MainMenu dialog complete at frame {menu_since}; idle frames={menu_idle_frames}; native-log parity remains a separate comparison");
        Ok(())
    } else {
        Err(format!("MainMenuBoundary: frame budget exhausted; observed idle frames={menu_idle_frames}, require 300"))
    }
}

/// Inspect the authoritative script field after all callbacks for a frame.
/// This observer performs no script calls or state transitions.
fn active_main_menu(vm: &mut VmHost) -> Option<u32> {
    let index = vm
        .objects
        .iter()
        .position(|object| object.class_name(&vm.loader) == "java.game.GameLogic")?;
    let state = vm
        .read_field(index as u32 + 0x1000, "actualState")
        .ok()?
        .payload;
    let object = vm.objects.get(state.checked_sub(0x1000)? as usize)?;
    (object.class_name(&vm.loader) == "java.game.MainMenu").then_some(state)
}
/// Fresh writable filesystem projection. No script I/O points at stock.
///
/// Files are clone-linked where the filesystem supports it: a bundle must not
/// alias another bundle's bytes, but it also must not pay for a private copy of
/// a 458 MiB install on every run.
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
            slr_telemetry::clone_or_copy(&entry.path(), &target)?;
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
    let mut output = None;
    let mut run_id = None;
    let mut trace_level = "metadata".to_owned();
    let mut event_ring = 4096usize;
    let mut wall_clock = false;
    let mut snapshot_native = None;
    let mut config_instance_id = None;
    let mut game_init_instance_id = None;
    let mut frames = 600u64;
    let mut clock_step = 0.01f64;
    let mut clock_replay = None;
    let mut args = std::env::args().skip(2);
    while let Some(arg) = args.next() {
        if arg == "--snapshot-native" {
            snapshot_native = Some(
                args.next()
                    .ok_or("ArgumentBoundary: --snapshot-native needs an ID")?
                    .parse::<u64>()
                    .map_err(|e| format!("ArgumentBoundary: invalid native call ID: {e}"))?,
            );
        } else if matches!(
            arg.as_str(),
            "--output" | "--run-id" | "--trace-level" | "--event-ring"
        ) {
            let value = args
                .next()
                .ok_or_else(|| format!("ArgumentBoundary: {arg} needs a value"))?;
            match arg.as_str() {
                "--output" => output = Some(PathBuf::from(value)),
                "--run-id" => run_id = Some(value),
                "--trace-level" if matches!(value.as_str(), "metadata" | "full") => {
                    trace_level = value
                }
                "--event-ring" => {
                    event_ring = value
                        .parse()
                        .map_err(|e| format!("ArgumentBoundary: invalid ring capacity: {e}"))?
                }
                _ => return Err("ArgumentBoundary: trace level must be metadata or full".into()),
            }
        } else if arg == "--window" {
            presentation::enable();
        } else if arg == "--input-script" {
            presentation::enable();
            live_input::script(args.next().ok_or("--input-script needs a JSON path")?)?;
        } else if arg == "--menu-frame" {
            presentation::capture_path(args.next().ok_or("--menu-frame needs a PNG path")?);
        } else if arg == "--wall-clock" {
            wall_clock = true;
        } else if arg == "--config-instance-id" || arg == "--game-init-instance-id" {
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
        } else if arg == "--clock-replay" {
            clock_replay = Some(
                args.next()
                    .ok_or("ArgumentBoundary: --clock-replay needs a path")?,
            );
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
    if presentation::enabled() {
        config_instance_id.get_or_insert(0xffff0080);
        game_init_instance_id.get_or_insert(0xffff007f);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("ProjectionBoundary: {e}"))?
        .as_nanos();
    let run_id = run_id.unwrap_or_else(|| format!("{}-{stamp}", std::process::id()));
    if matches!(run_id.as_str(), "" | "." | "..")
        || run_id.len() > 128
        || !run_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err("ArgumentBoundary: run ID must contain 1..128 ASCII letters, digits, dots, hyphens or underscores and cannot be . or ..".into());
    }
    if event_ring == 0 || event_ring > 1_000_000 {
        return Err("ArgumentBoundary: event ring capacity must be 1..1000000".into());
    }
    let output = output.unwrap_or_else(|| {
        repository
            .join("re/out/boot/runs")
            .join(&run_id)
            .join("runtime")
    });
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
    telemetry::configure(
        &output,
        &run_id,
        event_ring,
        trace_level == "full",
        wall_clock,
    )?;
    if presentation::enabled() {
        slr_telemetry::journal::configure(&output, &run_id)?;
    }
    telemetry::target_native(snapshot_native);
    slr_telemetry::install_panic_hook(slr_telemetry::crash_context);
    println!("BOOT diagnostics {}", output.display());
    let files = output.join(format!("files/{}-{stamp}", std::process::id()));
    project_stock(&root, &files).map_err(|e| format!("ProjectionBoundary: {e}"))?;
    slr_io::reset(&files);
    slr_io::findfile::reset_finds(&files);
    native_body::configure_image(&root.join("StreetLegal_Redline.exe"))?;
    factory::configure(config_instance_id, game_init_instance_id);
    sched::configure_clock(clock_step);
    if let Some(path) = clock_replay {
        sched::configure_replay(Path::new(&path))?;
    }
    engine::configure(engine::FrameConfig::default());
    viewport::configure_display();
    let mut vm = VmHost::new();
    if std::env::var_os("SLR_TRACE_MACHINE").is_some() {
        machine_recording::start(&mut vm, &output, &run_id)?;
    }
    // Original Config/field initialization runs before D3D9 device creation
    // under CRT 53-bit precision (ADR 0007); the settings activation below
    // changes this same OS-thread state to 24-bit precision.
    vm.runtime_precision = slr_x87::Precision::F64;
    if sched::replaying() {
        vm.runtime_clock = Some(sched::clock_ms);
    }
    vm.native_diagnostics = Some(telemetry::native_snapshot);
    vm.call_trace = std::env::var_os("SLR_TRACE_CALLS").is_some();
    slr_vm::interp::set_step_trace(std::env::var_os("SLR_TRACE_STEPS").is_some());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slr_vm::clock::with_machine_trace(vm.machine_trace(), || {
            run(
                &root,
                &mut vm,
                config_instance_id,
                &output,
                game_init_instance_id,
                frames,
            )
        })
    }))
    .unwrap_or_else(|panic| {
        // The panic hook already published `crash.json`; keep the message in
        // the returned error so `failure.json` and the exit status agree.
        Err(format!(
            "RuntimePanic: {}",
            panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("non-string payload")
        ))
    });
    machine_recording::drain(&vm)?;
    factory::write_trace(&output)?;
    gc_runtime::write_trace(&output)?;
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
    if let Err(error) = &outcome {
        telemetry::snapshot(&vm, "failure", Some(error))?;
        if presentation::enabled() {
            presentation::failure_state(&vm)?;
            let capture = presentation::capture(&vm, &output.join("failure.png"));
            slr_telemetry::journal::emit(
                "incident-image",
                serde_json::json!({"path":"failure.png","error":capture.err()}),
            )?;
        }
    } else {
        telemetry::snapshot(&vm, "completed", None)?;
    }
    telemetry::finish(outcome.as_ref().err().map(String::as_str))?;
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
    // Supervise normal window launches so UI failures and hard exits still leave
    // an indexed bundle. Headless parity execution keeps its existing path.
    if std::env::args().any(|arg| arg == "--window")
        && std::env::var_os("SLR_SESSION_CAPTURE").is_none()
    {
        let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/menu-diagnose.py");
        let status = std::process::Command::new("python3")
            .arg(runner)
            .arg("record")
            .arg("--binary")
            .arg(std::env::current_exe().expect("current binary"))
            .arg("--")
            .args(std::env::args().skip(1))
            .status();
        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("SessionRunnerBoundary: {e}");
                std::process::exit(1)
            }
        }
    }
    if let (Some(root), Ok(run_id)) = (
        std::env::var_os("SLR_SESSION_RUNTIME"),
        std::env::var("SLR_SESSION_RUN_ID"),
    ) {
        if let Err(e) = slr_telemetry::journal::configure(Path::new(&root), &run_id) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    let outcome = if std::env::args().any(|arg| arg == "--window") {
        slr_d3d9::window(|sender, input| {
            live_input::connect(input);
            presentation::connect(sender.clone());
            let outcome = execute();
            let _ = sender.send(slr_d3d9::Message::Finished(outcome));
        })
    } else {
        execute()
    };
    if let Err(error) = outcome {
        eprintln!("STOP {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_backend_and_driver_overrides_each_record_exactly_once() {
        slr_calllog::headless::configure_stream(None, true);
        slr_calllog::headless::reset();
        slr_render_natives::reset_render();
        let mut vm = VmHost::new();
        slr_render_natives::install_headless(&mut vm);
        viewport::install(&mut vm);
        resource_identity::install(&mut vm);
        let args = [Value::reference(0x1000)];
        for (address, name, sig) in [
            (slr_render_natives::VIEWPORT_DEACTIVATE, "deactivate", "()V"),
            (resource_identity::VIEWPORT_DESTROY, "destroy", "()V"),
            (viewport::GET_WIDTH, "getWidth", "()F"),
        ] {
            let info = CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: "java.render.Viewport",
                name,
                sig,
            };
            let native = vm.native_impls[&address];
            let result = native(&mut vm, &info);
            observe(&info, result);
            let records = slr_calllog::headless::take_records();
            assert_eq!(records.len(), 1, "{name} backend plus observer");
            assert!(
                matches!(&records[0], slr_calllog::LogRecord::NativeCall { class, name: recorded, .. } if class == info.class && recorded == name)
            );
        }
    }
    #[test]
    fn open_lib_missing_local_one_loads_headers_before_original_fast_path() {
        use slr_vm::vmclass::{Class, Field};
        let oracle =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/boot/openlib-header.json");
        let Some(oracle) = crate::oracle_fixture::file(&oracle) else {
            return;
        };
        let row: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(oracle)
                .expect("generate re/oracle/openlib_header_probe.py output first"),
        )
        .unwrap();
        assert_eq!(row["header_loads"][0]["state"], 0);
        assert_eq!(row["final_header_state"], 2);
        assert!(row["fallbacks"].as_array().unwrap().is_empty());
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../re/out/boot/openlib-edge-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&output).unwrap();
        let mut data = b"RPAK".to_vec();
        for value in [0x200u32, 0, 0, 0, 0, 0, 0] {
            data.extend(value.to_le_bytes());
        }
        std::fs::write(output.join("empty.rpk"), data).unwrap();
        RES.with(|resources| {
            *resources.borrow_mut() = Resources {
                root: output.clone(),
                packages: vec![("system.rpk".into(), None)],
                ..Resources::default()
            };
        });
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: "java.lang.String".into(),
            base: None,
            interfaces: vec![],
            fields: vec![Field {
                class: "java.lang.String".into(),
                name: "ptr".into(),
                ty: "I".into(),
                is_static: false,
            }],
            methods: vec![],
            bodies: vec![],
        });
        let name = vm.intern_string(Some(b"empty.rpk")).unwrap();
        let args = [Value::reference(name), Value::reference(0)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            class: "java.lang.System",
            name: "openLib",
            sig: "(Ljava.lang.String;)I",
            args: &args,
        };
        // Real FUN_005383f0 loads headers before the wrapper reads their state,
        // including the first successful open of a package without local 1.
        assert_eq!(row["existence_checks"], 1);
        for expected in row["results"].as_array().unwrap() {
            assert_eq!(
                open_lib(&mut vm, &info),
                Some(expected.as_u64().unwrap() as u32)
            );
            if output.join("empty.rpk").exists() {
                std::fs::remove_file(output.join("empty.rpk")).unwrap();
            }
        }
        assert!(vm.native_failure.is_none());
        std::fs::remove_dir_all(output).unwrap();
        RES.with(|resources| *resources.borrow_mut() = Resources::default());
    }
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
            packages: vec![("system.rpk".into(), Some(package))],
            registered: Default::default(),
            loading: Default::default(),
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

#[cfg(test)]
mod oracle_fixture;
