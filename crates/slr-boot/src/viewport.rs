//! `java.render.Viewport` natives over the created viewport's own state.
//!
//! The exe's `create` (`0x4814e0` -> `FUN_00538d60`) stores the flag word and four
//! floats into the viewport resource data at `+0x0c`, `+0x10` (left), `+0x14`
//! (top), `+0x18` (width) and `+0x1c` (height); `resize` (`0x481700`) overwrites the
//! four floats; the getters read them back, answering the marshalling slot (0.0)
//! when the viewport has no data. `getAspect` (`FUN_0048cda0`) is
//! `width * W / (width * H)` for the current display mode `W x H`
//! (`FUN_004fd580`). Activation has no state the scripts can read.
use slr_vm::host::{marshal_args, CallInfo, VmHost};
use std::{cell::RefCell, collections::HashMap};

pub const CREATE: u32 = 0x0048_14e0;
pub const RESIZE: u32 = 0x0048_1700;
pub const GET_ASPECT: u32 = 0x0048_17b0;
pub const GET_WIDTH: u32 = 0x0048_17f0;
pub const GET_HEIGHT: u32 = 0x0048_1870;
pub const GET_TOP: u32 = 0x0048_18f0;
pub const GET_LEFT: u32 = 0x0048_1970;

/// Declared scenario: the current display mode. The original read 800x600x32 from
/// the stock `save/game/options` file during Config construction.
pub const DISPLAY_MODE: (i32, i32) = (800, 600);

/// Same declared adapter modes as the headless original's hypothetical SM2 HAL
/// profile, after FUN_004b8970's duplicate elimination and LAB_004b8650 sort
/// (depth descending, width then height ascending). This is scenario data,
/// not enumeration of the physical host GPU.
pub fn configure_display() {
    VIEWPORTS.with(|v| v.borrow_mut().clear());
    slr_render_natives::reset_render();
    slr_render_natives::with_render(|h| {
        h.set_display_modes(
            [32, 16]
                .into_iter()
                .flat_map(|depth| {
                    [(640, 480), (800, 600), (1024, 768)]
                        .into_iter()
                        .map(move |(w, h)| [w, h, depth])
                })
                .collect(),
        );
        h.set_current_desc([DISPLAY_MODE.0, DISPLAY_MODE.1, 32]);
    });
}

/// The render collaborator interns in its own arena; the VM must receive a
/// real Java string, as LAB_0047c460 does through FUN_004174a0.
fn display_mode_name(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let word = slr_render_natives::bridge(vm, info)?;
    let name = slr_render_natives::with_render(|h| h.string(word).map(str::to_owned))?;
    match vm.intern_string(Some(name.as_bytes())) {
        Ok(word) => Some(word),
        Err(e) => {
            vm.native_failure = Some(format!("displayModeName string: {e:?}"));
            None
        }
    }
}

thread_local! {
    /// Resource container -> `[flags, left, top, width, height]`.
    static VIEWPORTS: RefCell<HashMap<u32, [u32; 5]>> = RefCell::new(HashMap::new());
}

fn words(info: &CallInfo<'_>) -> [u32; 6] {
    let mut out = [0; 6];
    let _ = marshal_args(info, &mut out);
    out
}

pub fn create(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut w = [0; 6];
    if let Err(error) = marshal_args(info, &mut w) {
        vm.native_failure = Some(format!("Viewport.create arguments: {error:?}"));
        return None;
    }
    // LAB_004814e0 checks the existing Native.ptr before allocating. It
    // reuses that handle, including when it was bound to another resource.
    let ptr = vm.read_field(w[0], "ptr").ok()?.payload;
    if !slr_resource::node::with_res(|h| h.node(ptr).is_some()) {
        return None;
    }
    match crate::factory::create_viewport(ptr) {
        Ok(container) => {
            VIEWPORTS.with(|v| {
                v.borrow_mut()
                    .insert(container, [w[1], w[2], w[3], w[4], w[5]])
            });
        }
        Err(error) => vm.native_failure = Some(format!("Viewport.create: {error}")),
    }
    None
}

fn container(vm: &mut VmHost, receiver: u32) -> Option<u32> {
    let ptr = vm.read_field(receiver, "ptr").ok()?.payload;
    slr_resource::node::with_res(|h| h.node(ptr).map(|n| n.container))
}

pub fn resize(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    let resource = container(vm, w[0])?;
    VIEWPORTS.with(|v| {
        if let Some(state) = v.borrow_mut().get_mut(&resource) {
            state[1..5].copy_from_slice(&w[1..5]);
        }
    });
    None
}

fn field(vm: &mut VmHost, info: &CallInfo<'_>, index: usize) -> Option<u32> {
    let w = words(info);
    let resource = container(vm, w[0]).unwrap_or(0);
    Some(VIEWPORTS.with(|v| v.borrow().get(&resource).map_or(0, |s| s[index])))
}

pub fn get_left(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(vm, info, 1)
}
pub fn get_top(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(vm, info, 2)
}
pub fn get_width(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(vm, info, 3)
}
pub fn get_height(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(vm, info, 4)
}

pub fn get_aspect(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    let resource = container(vm, w[0]).unwrap_or(0);
    let state = VIEWPORTS.with(|v| v.borrow().get(&resource).copied());
    Some(match state {
        None => 1.0f32.to_bits(),
        Some(s) => {
            let width = f32::from_bits(s[3]);
            let (screen_w, screen_h) = DISPLAY_MODE;
            ((width * screen_w as f32) / (width * screen_h as f32)).to_bits()
        }
    })
}

pub fn install(vm: &mut VmHost) {
    vm.native_impls
        .insert(slr_render_natives::DISPLAY_MODE_NAME, display_mode_name);
    for (address, native) in [
        (CREATE, create as slr_vm::host::NativeFn),
        (RESIZE, resize),
        (GET_ASPECT, get_aspect),
        (GET_WIDTH, get_width),
        (GET_HEIGHT, get_height),
        (GET_TOP, get_top),
        (GET_LEFT, get_left),
    ] {
        vm.native_impls.insert(address, native);
    }
}

/// Normalized display rectangle of a live viewport, for presentation/input.
pub(crate) fn bounds(container: u32) -> Option<[f32; 4]> {
    VIEWPORTS.with(|v| {
        v.borrow().get(&container).map(|w| {
            [
                f32::from_bits(w[1]),
                f32::from_bits(w[2]),
                f32::from_bits(w[3]),
                f32::from_bits(w[4]),
            ]
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_resource::node::{self, Container};
    use slr_vm::{
        values::Value,
        vmclass::{Class, Field},
    };

    fn invoke(vm: &mut VmHost, object: u32, name: &str, sig: &str, args: &[Value]) -> Option<u32> {
        let mut cells: Vec<_> = args.iter().rev().copied().collect();
        cells.push(Value::reference(object));
        let class = if matches!(name, "create" | "getWidth") {
            "java.render.Viewport"
        } else {
            node::RESOURCE_REF_CLASS
        };
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            class,
            name,
            sig,
            args: &cells,
        };
        match name {
            "create" => create(vm, &info),
            "getWidth" => get_width(vm, &info),
            _ => node::bridge(vm, &info),
        }
    }

    #[test]
    fn viewport_creation_original_rows() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/viewport-create.jsonl");
        let Ok(rows) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: viewport_creation_original_rows; run re/oracle/viewport_create_probe.py");
            return;
        };
        let mut count = 0;
        for row in rows.lines() {
            let row: serde_json::Value = serde_json::from_str(row).unwrap();
            let mut vm = VmHost::new();
            node::install(&mut vm);
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: "java.render.Viewport".into(),
                base: None,
                interfaces: vec![],
                fields: vec![Field {
                    class: "java.render.Viewport".into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }],
                methods: vec![],
                bodies: vec![],
            });
            crate::factory::configure(Some(0xffff0080), Some(0xffff007f));
            VIEWPORTS.with(|v| v.borrow_mut().clear());
            let object = vm.alloc_object("java.render.Viewport").unwrap();
            let missing = row["missing_handle"].as_bool().unwrap();
            let bound = row["initially_bound"].as_bool().unwrap();
            if !missing {
                invoke(&mut vm, object, "newNative", "()V", &[]);
            }
            let ptr = vm.read_field(object, "ptr").unwrap().payload;
            let old = bound.then(|| {
                node::with_res(|h| {
                    let old = h.add_container(Container::new(0x12, 123));
                    h.bind_id(123, old);
                    old
                })
            });
            let other = if let Some(old) = old {
                let other = node::with_res(|h| h.add_bound_node(123, old));
                invoke(&mut vm, object, "set", "(I)V", &[Value::int(123)]);
                Some(other)
            } else {
                None
            };
            let state: Vec<u32> = row["state"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let args: Vec<_> = state
                .iter()
                .enumerate()
                .map(|(i, bits)| {
                    if i == 0 {
                        Value::int(*bits as i32)
                    } else {
                        Value::float(f32::from_bits(*bits))
                    }
                })
                .collect();
            invoke(&mut vm, object, "create", "(IFFFF)V", &args);
            assert_eq!(vm.native_failure, None);
            let id = invoke(&mut vm, object, "id", "()I", &[]).unwrap();
            assert_eq!(id, row["id"].as_u64().unwrap() as u32);
            if !missing {
                assert_eq!(vm.read_field(object, "ptr").unwrap().payload, ptr);
                node::with_res(|h| {
                    let n = h.node(ptr).unwrap();
                    assert_eq!(n.id, id);
                    let c = h.container(n.container).unwrap();
                    assert_eq!((c.id, c.type_id, c.head), (id, 0x12, ptr));
                    assert_eq!(
                        VIEWPORTS.with(|v| v.borrow()[&n.container]),
                        state.as_slice()
                    );
                    if let Some(old) = old {
                        assert_eq!(h.container(old).unwrap().head, other.unwrap());
                        assert_eq!(h.node(other.unwrap()).unwrap().next, 0);
                        assert_eq!(h.node(other.unwrap()).unwrap().prev, 0);
                    }
                });
                // A second wrapper of the same resource sees its viewport data.
                let alias = vm.alloc_object("java.render.Viewport").unwrap();
                invoke(&mut vm, alias, "newNative", "()V", &[]);
                invoke(&mut vm, alias, "set", "(I)V", &[Value::int(id as i32)]);
                assert_eq!(
                    invoke(&mut vm, alias, "getWidth", "()F", &[]),
                    Some(state[3])
                );
            } else {
                assert_eq!(invoke(&mut vm, object, "getWidth", "()F", &[]), Some(0));
                assert!(VIEWPORTS.with(|v| v.borrow().is_empty()));
            }
            // The missing-handle branch must consume no ID; a successful
            // creation consumes exactly one from the shared free stack.
            let next = vm.alloc_object("java.render.Viewport").unwrap();
            invoke(&mut vm, next, "newNative", "()V", &[]);
            invoke(&mut vm, next, "create", "(IFFFF)V", &args);
            assert_eq!(
                invoke(&mut vm, next, "id", "()I", &[]),
                Some(row["next_id"].as_u64().unwrap() as u32)
            );
            count += 1;
        }
        assert_eq!(count, 3);
    }
}
