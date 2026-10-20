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
    /// Receiver word -> `[flags, left, top, width, height]`.
    static VIEWPORTS: RefCell<HashMap<u32, [u32; 5]>> = RefCell::new(HashMap::new());
}

fn words(info: &CallInfo<'_>) -> [u32; 6] {
    let mut out = [0; 6];
    let _ = marshal_args(info, &mut out);
    out
}

pub fn create(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    VIEWPORTS.with(|v| v.borrow_mut().insert(w[0], [w[1], w[2], w[3], w[4], w[5]]));
    None
}

pub fn resize(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    VIEWPORTS.with(|v| {
        if let Some(state) = v.borrow_mut().get_mut(&w[0]) {
            state[1..5].copy_from_slice(&w[1..5]);
        }
    });
    None
}

fn field(info: &CallInfo<'_>, index: usize) -> Option<u32> {
    let w = words(info);
    Some(VIEWPORTS.with(|v| v.borrow().get(&w[0]).map_or(0, |s| s[index])))
}

pub fn get_left(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(info, 1)
}
pub fn get_top(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(info, 2)
}
pub fn get_width(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(info, 3)
}
pub fn get_height(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    field(info, 4)
}

pub fn get_aspect(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    let state = VIEWPORTS.with(|v| v.borrow().get(&w[0]).copied());
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
