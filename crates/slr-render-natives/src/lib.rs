//! `java.render.*` natives (M1-12, issue #39): the rows whose bodies read and
//! write the exe's render globals, ported as call-log-writing backends.
//!
//! The D3D9 device itself is M5-02; the viewport/camera/text objects hang off
//! the resource node model (`0x42ab50`), which is M1-10. What is portable now
//! is the display-mode layer and the two `Text` colour rows: they are pure
//! global-word traffic, and each one is a registered native whose whole body
//! fits in a dozen instructions.
//!
//! # The rows
//!
//! | exe | row | body |
//! |---|---|---|
//! | [`NUM_DISPLAY_MODES`] `0x47c440` | `GfxEngine.numDisplayModes()I` | `[0x6495f4]` (`0x4b9ef0`) |
//! | [`CURR_DISPLAY_MODE`] `0x47c450` | `GfxEngine.currDisplayMode()I` | scan the mode table `0x6466a4` for the entry matching the current mode's three `WORD`s at `[0x6495dc]+0x1bc..0x1c0`, answer the index or `0` (`0x4b9ea0`) |
//! | [`DISPLAY_MODE_NAME`] `0x47c460` | `GfxEngine.displayModeName(I)Ljava.lang.String;` | format the mode entry (`0x4b9f00`) into a 0x100-byte buffer and intern it (`0x4174a0`) |
//! | [`CHANGE_VIDEO_MODE`] `0x47c3f0` | `GfxEngine.changeVideoMode(III)V` | marshal, then `[device+0x204]`'s vtable `+0x14` |
//! | [`TEXT_SET_DEFAULT_COLOR`] `0x487020` | `Text.setDefaultColor(I)V` | `[0x612c64] = arg` |
//! | [`TEXT_GET_DEFAULT_COLOR`] `0x487040` | `Text.getDefaultColor()I` | `[0x612c64]` |
//! | [`FORCE_RENDERING`] `0x47c1d0` | `GfxEngine.forceRendering()V` | two resource-manager calls, `[0x65aecc] = 0`, the render flush, `[0x6200a4]++` |
//! | [`FLUSH`] `0x47c2d0` | `GfxEngine.flush()V` | seven passes, then `0x537ed0(1)` and `0x4b7b90()` |
//! | [`IS_PLAYING_VIDEO`] `0x47c390` | `GfxEngine.isPlayingVideo()I` | `[0x64a394]` (`0x4f9750`) |
//! | [`VIEWPORT_ACTIVATE`] `0x481680` | `Viewport.activate(I)V` | the miss arm: `0x42ab50` answers 0 and the row returns |
//! | [`VIEWPORT_DEACTIVATE`] `0x4816c0` | `Viewport.deactivate()V` | the same miss arm |
//! | [`VIEWPORT_GET_ASPECT`] `0x4817b0` | `Viewport.getAspect()F` | `1.0` (`0x5f08f0`) on the miss arm |
//! | [`VIEWPORT_GET_WIDTH`] `0x4817f0` | `Viewport.getWidth()F` | `0.0` (`0x5e73cc`) on the miss arm |
//! | [`VIEWPORT_GET_HEIGHT`] `0x481870` | `Viewport.getHeight()F` | the same |
//! | [`VIEWPORT_GET_TOP`] `0x4818f0` | `Viewport.getTop()F` | the same |
//! | [`VIEWPORT_GET_LEFT`] `0x481970` | `Viewport.getLeft()F` | the same |
//! | [`VIEWPORT_RESIZE`] `0x481700` | `Viewport.resize(FFFF)V` | the same miss arm |
//!
//! # Divergences
//!
//! - `forceRendering`/`flush` reach the D3D9 device (`0x6187b0`) and the
//!   resource manager (`0x618d48`). The port performs the two global writes the
//!   bodies contain (`[0x65aecc] = 0` and the `[0x6200a4]` increment) and
//!   records the device calls as [`RenderEvent`]s instead of issuing them —
//!   those callees are not natives, so the call-log record is unaffected
//!   (`docs/adr/0007`).
//! - `changeVideoMode`'s vtable dispatch cannot be modelled without M5-02's
//!   device object; the port marshals and records the three arguments, and the
//!   device-side effect is left to that ticket. This is the same arm the exe
//!   takes when `[0x6187b0]` has no device: `0x47c40e` loads the pointer and
//!   dereferences `+0x204`, so a null device faults — the port instead records
//!   and returns.
//! - `displayModeName`'s formatting goes through the exe's `sprintf`
//!   (`0x5d782b`) with the image's literal `"%d %d %d"` at `0x617818`; the
//!   port renders the same three words with Rust's formatter, which is
//!   byte-identical for the integer ranges the table holds.
//!
//! # Headless capture contract
//!
//! [`install`] retains the seventeen modeled rows. [`install_headless`] adds
//! all remaining render registrations and writes every invocation to the
//! ordered [`slr_calllog::headless`] stream. Capture-only rows use explicit
//! deterministic return words (zero by default); their engine behavior is
//! unimplemented and they have no fidelity annotations. This provides the
//! logging backend required by M1-12, not a graphics or whole-boot parity claim.
//! Object identity normalization and live engine envelopes remain later work.

use std::cell::RefCell;
use std::collections::HashMap;

use slr_calllog::model::{LogRecord, Value as LogValue};
use slr_vm::host::{marshal_args, CallInfo, VmHost};
#[cfg(test)]
use slr_vm::values::Value;

// --------------------------------------------------------------------------
// The registry rows
// --------------------------------------------------------------------------

/// One `java.render.*` registration ported by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderBinding {
    /// The declaring class (`java.render.GfxEngine` or `java.render.Text`).
    pub class: &'static str,
    /// The registered method name.
    pub name: &'static str,
    /// The registered descriptor text.
    pub sig: &'static str,
    /// Exe address of the implementation (`record+8`).
    pub impl_addr: u32,
}

/// `java.render.GfxEngine.numDisplayModes()I`.
///
/// exe: 0x47c440
pub const NUM_DISPLAY_MODES: u32 = 0x0047_c440;
/// `java.render.GfxEngine.currDisplayMode()I`.
///
/// exe: 0x47c450
pub const CURR_DISPLAY_MODE: u32 = 0x0047_c450;
/// `java.render.GfxEngine.displayModeName(I)Ljava.lang.String;`.
///
/// exe: 0x47c460
pub const DISPLAY_MODE_NAME: u32 = 0x0047_c460;
/// `java.render.GfxEngine.changeVideoMode(III)V`.
///
/// exe: 0x47c3f0
pub const CHANGE_VIDEO_MODE: u32 = 0x0047_c3f0;
/// `java.render.GfxEngine.forceRendering()V`.
///
/// exe: 0x47c1d0
pub const FORCE_RENDERING: u32 = 0x0047_c1d0;
/// `java.render.GfxEngine.flush()V`.
///
/// exe: 0x47c2d0
pub const FLUSH: u32 = 0x0047_c2d0;
/// `java.render.Text.setDefaultColor(I)V`.
///
/// exe: 0x487020
pub const TEXT_SET_DEFAULT_COLOR: u32 = 0x0048_7020;
/// `java.render.Text.getDefaultColor()I`.
///
/// exe: 0x487040
pub const TEXT_GET_DEFAULT_COLOR: u32 = 0x0048_7040;
/// `java.render.GfxEngine.isPlayingVideo()I`.
///
/// exe: 0x47c390
///
/// The body is a `jmp` to `0x4f9750`, which reads `[0x64a394]`.
pub const IS_PLAYING_VIDEO: u32 = 0x0047_c390;
/// `java.render.Viewport.activate(I)V`.
///
/// exe: 0x481680
pub const VIEWPORT_ACTIVATE: u32 = 0x0048_1680;
/// `java.render.Viewport.deactivate()V`.
///
/// exe: 0x4816c0
pub const VIEWPORT_DEACTIVATE: u32 = 0x0048_16c0;
/// `java.render.Viewport.getAspect()F`.
///
/// exe: 0x4817b0
pub const VIEWPORT_GET_ASPECT: u32 = 0x0048_17b0;
/// `java.render.Viewport.getWidth()F`.
///
/// exe: 0x4817f0
pub const VIEWPORT_GET_WIDTH: u32 = 0x0048_17f0;
/// `java.render.Viewport.getHeight()F`.
///
/// exe: 0x481870
pub const VIEWPORT_GET_HEIGHT: u32 = 0x0048_1870;
/// `java.render.Viewport.getTop()F`.
///
/// exe: 0x4818f0
pub const VIEWPORT_GET_TOP: u32 = 0x0048_18f0;
/// `java.render.Viewport.getLeft()F`.
///
/// exe: 0x481970
pub const VIEWPORT_GET_LEFT: u32 = 0x0048_1970;
/// `java.render.Viewport.resize(FFFF)V`.
///
/// exe: 0x481700
pub const VIEWPORT_RESIZE: u32 = 0x0048_1700;

/// The aspect `getAspect` answers when the receiver has no engine envelope:
/// the `fld dword ptr [0x5f08f0]` at `0x4817d0`, which the image holds as
/// `1.0`.
pub const ASPECT_MISS: f32 = 1.0;
/// The word `getWidth`/`getHeight`/`getTop`/`getLeft` answer on the same arm:
/// the `fld dword ptr [0x5e73cc]` those rows take, `0.0` in the image.
pub const VIEWPORT_ZERO: f32 = 0.0;

/// `java.render.GfxEngine`.
pub const GFX_ENGINE_CLASS: &str = "java.render.GfxEngine";
/// `java.render.Text`.
pub const TEXT_CLASS: &str = "java.render.Text";
/// `java.render.Viewport`.
pub const VIEWPORT_CLASS: &str = "java.render.Viewport";

/// The eight rows, in the exe's registration order (`re/out/native_api.tsv`).
pub const RENDER_NATIVE_BINDINGS: &[RenderBinding] = &[
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "forceRendering",
        sig: "()V",
        impl_addr: FORCE_RENDERING,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "numDisplayModes",
        sig: "()I",
        impl_addr: NUM_DISPLAY_MODES,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "currDisplayMode",
        sig: "()I",
        impl_addr: CURR_DISPLAY_MODE,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "displayModeName",
        sig: "(I)Ljava.lang.String;",
        impl_addr: DISPLAY_MODE_NAME,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "changeVideoMode",
        sig: "(III)V",
        impl_addr: CHANGE_VIDEO_MODE,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "flush",
        sig: "()V",
        impl_addr: FLUSH,
    },
    RenderBinding {
        class: TEXT_CLASS,
        name: "setDefaultColor",
        sig: "(I)V",
        impl_addr: TEXT_SET_DEFAULT_COLOR,
    },
    RenderBinding {
        class: TEXT_CLASS,
        name: "getDefaultColor",
        sig: "()I",
        impl_addr: TEXT_GET_DEFAULT_COLOR,
    },
    RenderBinding {
        class: GFX_ENGINE_CLASS,
        name: "isPlayingVideo",
        sig: "()I",
        impl_addr: IS_PLAYING_VIDEO,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "activate",
        sig: "(I)V",
        impl_addr: VIEWPORT_ACTIVATE,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "deactivate",
        sig: "()V",
        impl_addr: VIEWPORT_DEACTIVATE,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "getAspect",
        sig: "()F",
        impl_addr: VIEWPORT_GET_ASPECT,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "getWidth",
        sig: "()F",
        impl_addr: VIEWPORT_GET_WIDTH,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "getHeight",
        sig: "()F",
        impl_addr: VIEWPORT_GET_HEIGHT,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "getTop",
        sig: "()F",
        impl_addr: VIEWPORT_GET_TOP,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "getLeft",
        sig: "()F",
        impl_addr: VIEWPORT_GET_LEFT,
    },
    RenderBinding {
        class: VIEWPORT_CLASS,
        name: "resize",
        sig: "(FFFF)V",
        impl_addr: VIEWPORT_RESIZE,
    },
];

/// Render natives captured only by [`install_headless`].
///
/// These rows have no engine implementation here. Their native calls are
/// recorded with supplied deterministic result words (zero by default), so
/// capture coverage does not claim exact graphics, video or object behavior.
/// They deliberately carry no fidelity annotations.
pub const HEADLESS_RENDER_BINDINGS: &[RenderBinding] = &[
    RenderBinding {
        class: "java.render.GfxEngine",
        name: "printScreen",
        sig: "(Ljava.lang.String;)V",
        impl_addr: 0x0047c3c0,
    },
    RenderBinding {
        class: "java.render.GfxEngine",
        name: "setGlobalEnvmap",
        sig: "(Ljava.util.resource.ResourceRef;)V",
        impl_addr: 0x0047c220,
    },
    RenderBinding {
        class: "java.render.GfxEngine",
        name: "openVideo",
        sig: "(Ljava.lang.String;II)I",
        impl_addr: 0x0047c330,
    },
    RenderBinding {
        class: "java.render.GfxEngine",
        name: "closeVideo",
        sig: "()V",
        impl_addr: 0x0047c380,
    },
    RenderBinding {
        class: "java.render.Camera",
        name: "create",
        sig: "(Ljava.util.resource.ResourceRef;Ljava.render.Viewport;IFFFFFII)V",
        impl_addr: 0x004861e0,
    },
    RenderBinding {
        class: "java.render.Camera",
        name: "destroy",
        sig: "()V",
        impl_addr: 0x004864c0,
    },
    RenderBinding {
        class: "java.render.Camera",
        name: "setFog",
        sig: "(IFF)V",
        impl_addr: 0x00486570,
    },
    RenderBinding {
        class: "java.render.Viewport",
        name: "create",
        sig: "(IFFFF)V",
        impl_addr: 0x004814e0,
    },
    RenderBinding {
        class: "java.render.Viewport",
        name: "destroy",
        sig: "()V",
        impl_addr: 0x00481600,
    },
    RenderBinding {
        class: "java.render.Viewport",
        name: "unproject",
        sig: "(Ljava.lang.Vector3;I)Ljava.lang.Vector3;",
        impl_addr: 0x00487a60,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "create",
        sig: "(Ljava.util.resource.ResourceRef;Ljava.util.resource.ResourceRef;FF)V",
        impl_addr: 0x00487050,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "changeColor",
        sig: "(I)V",
        impl_addr: 0x00487150,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "changeAlign",
        sig: "(I)V",
        impl_addr: 0x004871d0,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "setPos",
        sig: "(FF)V",
        impl_addr: 0x004870d0,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "getPos",
        sig: "()Ljava.lang.Vector3;",
        impl_addr: 0x00487250,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "update",
        sig: "()V",
        impl_addr: 0x00487350,
    },
    RenderBinding {
        class: "java.render.Text",
        name: "getWidthPixels",
        sig: "(Ljava.lang.String;Ljava.util.resource.ResourceRef;)F",
        impl_addr: 0x00487410,
    },
];

/// The mode-count word (`[0x6495f4]`, `0x4b9ef0`).
pub const DISPLAY_MODE_COUNT: u32 = 0x0064_95f4;
/// The current display mode's descriptor (`[0x6495dc]`, three `WORD`s at
/// `+0x1bc`, `+0x1be`, `+0x1c0`).
pub const CURRENT_MODE_DESC: u32 = 0x0064_95dc;
/// The mode table base (`0x6466a4`; entries are `0x14` bytes, the three
/// compared words at `+0`, `+4`, `+8`).
pub const DISPLAY_MODE_TABLE: u32 = 0x0064_66a4;
/// The current-mode index word (`0x4b9ea0` writes it when a match is found).
pub const DISPLAY_MODE_INDEX: u32 = 0x0064_a394;
/// `Text`'s default colour word (`0x612c64`), `-1` in the image.
pub const TEXT_DEFAULT_COLOR: u32 = 0x0061_2c64;
/// The render flush's dirty word (`0x65aecc`), cleared by both flush rows.
pub const RENDER_DIRTY: u32 = 0x0065_aecc;
/// The frame counter both flush rows increment (`0x6200a4`).
pub const FRAME_COUNTER: u32 = 0x0062_00a4;
/// The D3D9 device pointer (`0x6187b0`).
pub const D3D_DEVICE: u32 = 0x0061_87b0;
/// The resource manager (`0x618d48`).
pub const RESOURCE_MANAGER: u32 = 0x0061_8d48;
/// The number of passes both flush rows run (`mov esi,0x7`).
pub const FLUSH_PASSES: u32 = 7;

/// The mode-entry size in the table (`add ecx,0x14`).
pub const DISPLAY_MODE_STRIDE: u32 = 0x14;
/// The current-mode descriptor's first compared field (`+0x1bc`).
pub const MODE_DESC_WIDTH: u32 = 0x01bc;
/// The second compared field (`+0x1be`).
pub const MODE_DESC_HEIGHT: u32 = 0x01be;
/// The third compared field (`+0x1c0`).
pub const MODE_DESC_BPP: u32 = 0x01c0;

/// One render-layer effect, in the order the exe issues it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderEvent {
    /// `0x5378d0` on the resource manager (no stack arguments).
    ManagerSelect,
    /// `0x537b40` on the resource manager.
    ManagerFlush,
    /// `0x4fca30` on the D3D9 device: the frame flush.
    DeviceFlush,
    /// `0x537ed0(1)` after the seven flush passes.
    ManagerFinish,
    /// `0x4b7b90()` after the manager finishes.
    DeviceSync,
    /// `[device+0x204]`'s vtable `+0x14`: the video-mode change.
    ChangeVideoMode {
        /// The first marshalled word.
        first: u32,
        /// The second.
        second: u32,
        /// The third.
        third: u32,
    },
}

/// The render natives' runtime: the display-mode globals, the two flush words
/// and the text colour.
///
/// exe: `0x6495f4`/`0x6495dc`/`0x6466a4`/`0x64a394` (display modes),
/// `0x612c64` (text colour), `0x65aecc`/`0x6200a4` (the flush pair).
#[derive(Debug, Clone)]
pub struct RenderHost {
    modes: Vec<[i32; 3]>,
    current_desc: [i32; 3],
    current_index: Option<usize>,
    text_color: i32,
    video_playing: u32,
    dirty: u32,
    frame: u32,
    events: Vec<RenderEvent>,
    records: Vec<LogRecord>,
    headless_results: HashMap<u32, u32>,
    strings: Vec<(u32, String)>,
    next_string: u32,
}

impl Default for RenderHost {
    fn default() -> Self {
        RenderHost::new()
    }
}

impl RenderHost {
    /// A host at the image's initial state: no display modes enumerated (the
    /// table count is `0` before `0x4b9f60` fills it), text colour `-1`.
    #[must_use]
    pub fn new() -> RenderHost {
        RenderHost {
            modes: Vec::new(),
            current_desc: [0; 3],
            current_index: None,
            text_color: -1,
            video_playing: 0,
            dirty: 0,
            frame: 0,
            events: Vec::new(),
            records: Vec::new(),
            headless_results: HashMap::new(),
            strings: Vec::new(),
            next_string: 0,
        }
    }

    /// Install the enumerated display modes — the seam for the device layer
    /// (`0x4b9f60`), which is M5-02's to fill.
    pub fn set_display_modes(&mut self, modes: Vec<[i32; 3]>) {
        self.modes = modes;
    }

    /// Set the current mode's three descriptor words (`[0x6495dc]+0x1bc..`).
    pub fn set_current_desc(&mut self, desc: [i32; 3]) {
        self.current_desc = desc;
    }

    /// The mode count `numDisplayModes` answers.
    #[must_use]
    pub fn mode_count(&self) -> usize {
        self.modes.len()
    }

    /// The text default colour word.
    #[must_use]
    pub fn text_color(&self) -> i32 {
        self.text_color
    }

    /// Seed the two flush globals — the seam for a driver resuming a run, and
    /// for a test that starts from a known state.
    pub fn set_flush_state(&mut self, dirty: u32, frame: u32) {
        self.dirty = dirty;
        self.frame = frame;
    }

    /// Seed the `Text` default colour word (`0x612c64`).
    pub fn set_text_color(&mut self, color: i32) {
        self.text_color = color;
    }

    /// Seed the video-playback word (`0x64a394`) — the seam M5-09's player
    /// uses once it can open a video.
    pub fn set_video_playing(&mut self, word: u32) {
        self.video_playing = word;
    }

    /// The render dirty word (`0x65aecc`).
    #[must_use]
    pub fn dirty(&self) -> u32 {
        self.dirty
    }

    /// The frame counter (`0x6200a4`).
    #[must_use]
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// The render-layer effects so far, in order.
    #[must_use]
    pub fn events(&self) -> &[RenderEvent] {
        &self.events
    }

    /// The call records so far, in order.
    #[must_use]
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// Take the recorded calls.
    pub fn take_records(&mut self) -> Vec<LogRecord> {
        core::mem::take(&mut self.records)
    }

    /// Supply a deterministic return word for a capture-only render native.
    /// A non-void capture-only native otherwise returns zero (null or +0.0).
    /// This is a headless test input, never an oracle measurement.
    pub fn set_headless_result(&mut self, address: u32, word: u32) {
        self.headless_results.insert(address, word);
    }

    /// Run one row over the marshalled words (`words[0]` is the receiver
    /// slot), record the call and answer its result word.
    pub fn call(&mut self, row: &RenderBinding, words: &[u32]) -> Option<u32> {
        let word = match row.impl_addr {
            NUM_DISPLAY_MODES => Some(self.modes.len() as u32),
            CURR_DISPLAY_MODE => Some(self.curr_display_mode() as u32),
            DISPLAY_MODE_NAME => Some(self.display_mode_name(word_at(words, 1) as i32)),
            CHANGE_VIDEO_MODE => {
                let (first, second, third) =
                    (word_at(words, 1), word_at(words, 2), word_at(words, 3));
                self.events.push(RenderEvent::ChangeVideoMode {
                    first,
                    second,
                    third,
                });
                None
            }
            FORCE_RENDERING | FLUSH => {
                // `mov esi,7` at `0x47c1d1` / `0x47c2d1`: the sequence runs
                // seven times, `dec esi; jne` at `0x47c208`.
                for _ in 0..FLUSH_PASSES {
                    self.events.push(RenderEvent::ManagerSelect);
                    self.events.push(RenderEvent::ManagerFlush);
                    self.dirty = 0;
                    self.events.push(RenderEvent::DeviceFlush);
                    self.frame = self.frame.wrapping_add(1);
                }
                if row.impl_addr == FLUSH {
                    self.events.push(RenderEvent::ManagerFinish);
                    self.events.push(RenderEvent::DeviceSync);
                }
                None
            }
            TEXT_SET_DEFAULT_COLOR => {
                self.text_color = word_at(words, 1) as i32;
                None
            }
            TEXT_GET_DEFAULT_COLOR => Some(self.text_color as u32),
            // `0x4f9750` is `mov eax, [0x64a394]`: the video-playback word.
            IS_PLAYING_VIDEO => Some(self.video_playing),
            // The six Viewport rows take their "no engine envelope" arm when
            // `0x42ab50` misses, which is every port run: `activate` and
            // `deactivate` return without touching the device, and the four
            // getters answer the image's miss constants.
            VIEWPORT_ACTIVATE | VIEWPORT_DEACTIVATE => None,
            VIEWPORT_GET_ASPECT => Some(ASPECT_MISS.to_bits()),
            VIEWPORT_GET_WIDTH | VIEWPORT_GET_HEIGHT | VIEWPORT_GET_TOP | VIEWPORT_GET_LEFT => {
                Some(VIEWPORT_ZERO.to_bits())
            }
            VIEWPORT_RESIZE => None,
            addr if HEADLESS_RENDER_BINDINGS
                .iter()
                .any(|binding| binding.impl_addr == addr) =>
            {
                if return_type(row.sig) == b'V' {
                    None
                } else {
                    Some(self.headless_results.get(&addr).copied().unwrap_or(0))
                }
            }
            _ => return None,
        };
        let sig = row.sig;
        let args = param_types(sig)
            .into_iter()
            .enumerate()
            .map(|(index, ty)| log_value(ty, word_at(words, index + 1)))
            .collect();
        let result = word.map(|word| log_value(return_type(sig), word));
        self.records.push(LogRecord::NativeCall {
            class: row.class.to_owned(),
            name: row.name.to_owned(),
            args,
            result,
        });
        word
    }

    /// `0x4b9ea0`: the index of the table entry whose three words match the
    /// current mode's descriptor, or `0` when none does (the `xor eax,eax`
    /// at `0x4b9ee9`; the found arm falls into `0x4b9eeb`).
    fn curr_display_mode(&mut self) -> i32 {
        let index = self
            .modes
            .iter()
            .position(|entry| *entry == self.current_desc);
        match index {
            Some(index) => {
                self.current_index = Some(index);
                index as i32
            }
            None => 0,
        }
    }

    /// `0x4b9f00` + `0x4174a0`: format the mode entry and intern it. An index
    /// outside the table answers the empty string: `0x4b9f40` leaves the
    /// buffer as it found it and the native interns it regardless.
    fn display_mode_name(&mut self, index: i32) -> u32 {
        let name = usize::try_from(index)
            .ok()
            .and_then(|i| self.modes.get(i))
            // `0x617818` is the literal `"%d %d %d"` (space-separated).
            .map(|entry| format!("{} {} {}", entry[0], entry[1], entry[2]))
            .unwrap_or_default();
        // The port's string words are its own arena; see `SoundHost`'s note.
        self.next_string = self.next_string.wrapping_add(1);
        self.strings.push((self.next_string, name));
        self.next_string
    }

    /// The bytes behind an interned string word (tests and the script side).
    #[must_use]
    pub fn string(&self, word: u32) -> Option<&str> {
        self.strings
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, s)| s.as_str())
    }
}

/// The word at `index`, or `0` when the container ran short.
fn word_at(words: &[u32], index: usize) -> u32 {
    words.get(index).copied().unwrap_or(0)
}

/// The declared parameter types of `sig`, in declaration order.
#[must_use]
pub fn param_types(sig: &str) -> Vec<u8> {
    let bytes = sig.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    if bytes.first() == Some(&b'(') {
        i = 1;
    }
    while let Some(&ty) = bytes.get(i) {
        match ty {
            b')' => break,
            b'L' => {
                out.push(b'L');
                while bytes.get(i).is_some_and(|&c| c != b';') {
                    i += 1;
                }
                i += 1;
            }
            b'[' => {
                out.push(b'[');
                while bytes.get(i) == Some(&b'[') {
                    i += 1;
                }
                while bytes.get(i).is_some_and(|&c| c != b';') {
                    i += 1;
                }
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// The return type byte of `sig`.
#[must_use]
pub fn return_type(sig: &str) -> u8 {
    let bytes = sig.as_bytes();
    match bytes.iter().position(|&c| c == b')') {
        Some(close) => bytes.get(close + 1).copied().unwrap_or(b'V'),
        None => b'V',
    }
}

/// The call-log value for one marshalled word of type `ty`.
#[must_use]
pub fn log_value(ty: u8, word: u32) -> LogValue {
    match ty {
        b'F' => LogValue::FloatBits(word),
        b'D' => LogValue::DoubleBits(u64::from(word)),
        b'J' => LogValue::Long(i64::from(word as i32)),
        b'L' | b'[' => LogValue::Int(word as i32),
        _ => LogValue::Int(word as i32),
    }
}

thread_local! {
    static RENDER: RefCell<RenderHost> = RefCell::new(RenderHost::new());
}

/// Run `f` over this thread's [`RenderHost`].
pub fn with_render<R>(f: impl FnOnce(&mut RenderHost) -> R) -> R {
    RENDER.with(|r| f(&mut r.borrow_mut()))
}

/// Reset this thread's render state (tests isolate one scenario each).
pub fn reset_render() {
    RENDER.with(|r| *r.borrow_mut() = RenderHost::new());
}

/// Register every row with the VM's native registry.
pub fn register(vm: &mut VmHost) {
    for row in RENDER_NATIVE_BINDINGS {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`slr_vm::host::NativeFn`] installed at every address.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let row = RENDER_NATIVE_BINDINGS
        .iter()
        .find(|row| row.impl_addr == addr)?;
    let mut out = [0u32; slr_calllog::MAX_ARGS + 1];
    let written = marshal_args(info, &mut out).unwrap_or(0);
    with_render(|render| render.call(row, &out[..written]))
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in RENDER_NATIVE_BINDINGS {
        vm.native_impls
            .insert(row.impl_addr, bridge as slr_vm::host::NativeFn);
    }
}

/// Install every render native as a headless call-log backend.
///
/// Existing modeled rows retain their behavior. The rows in
/// [`HEADLESS_RENDER_BINDINGS`] capture arguments and return deterministic
/// placeholders supplied through [`RenderHost::set_headless_result`]. A boot
/// trace using those placeholders is not evidence of exact engine behavior.
pub fn install_headless(vm: &mut VmHost) {
    for row in RENDER_NATIVE_BINDINGS
        .iter()
        .chain(HEADLESS_RENDER_BINDINGS)
    {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
        vm.native_impls
            .insert(row.impl_addr, headless_bridge as slr_vm::host::NativeFn);
    }
}

fn headless_bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let row = RENDER_NATIVE_BINDINGS
        .iter()
        .chain(HEADLESS_RENDER_BINDINGS)
        .find(|row| row.impl_addr == addr)?;
    let mut words = [0; slr_calllog::MAX_ARGS + 1];
    let written = marshal_args(info, &mut words).expect("registered render ABI");
    with_render(|host| {
        let result = host.call(row, &words[..written]);
        slr_calllog::headless::record(host.records.last().expect("native call recorded").clone());
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIVER: u32 = 0x0050_0000;

    fn info<'a>(args: &'a [Value], row: &RenderBinding) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: row.class,
            name: row.name,
            sig: row.sig,
        }
    }

    fn row_named(name: &str) -> &'static RenderBinding {
        RENDER_NATIVE_BINDINGS
            .iter()
            .find(|row| row.name == name)
            .expect("row")
    }

    /// Call one row with `params` in declaration order: the container holds
    /// them reversed with the receiver last (`0x45d910`).
    fn call(vm: &mut VmHost, params: &[Value], name: &str) -> Option<u32> {
        let row = row_named(name);
        let mut args: Vec<Value> = params.iter().rev().copied().collect();
        args.push(Value::reference(RECEIVER));
        bridge(vm, &info(&args, row))
    }

    /// Every row installs and resolves through the registry.
    #[test]
    fn every_row_installs_and_resolves() {
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(RENDER_NATIVE_BINDINGS.len(), 17);
        for row in RENDER_NATIVE_BINDINGS {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{}.{}{}",
                row.class,
                row.name,
                row.sig
            );
            assert!(vm.native_impls.contains_key(&row.impl_addr));
        }
    }

    /// `numDisplayModes` is the table count; the image starts at zero.
    #[test]
    fn num_display_modes_reads_the_table_count() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(call(&mut vm, &[], "numDisplayModes"), Some(0));
        with_render(|r| {
            r.set_display_modes(vec![[640, 480, 16], [800, 600, 32]]);
        });
        assert_eq!(call(&mut vm, &[], "numDisplayModes"), Some(2));
    }

    /// `currDisplayMode` scans for the entry matching the current three words
    /// and answers `0` when none matches (`0x4b9ee9`).
    #[test]
    fn curr_display_mode_scans_for_the_matching_entry() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_render(|r| {
            r.set_display_modes(vec![[640, 480, 16], [800, 600, 32], [1024, 768, 32]]);
            r.set_current_desc([800, 600, 32]);
        });
        assert_eq!(call(&mut vm, &[], "currDisplayMode"), Some(1));
        with_render(|r| r.set_current_desc([123, 456, 78]));
        assert_eq!(call(&mut vm, &[], "currDisplayMode"), Some(0));
    }

    /// `displayModeName` formats the entry and interns it; an index outside
    /// the table interns the empty string.
    #[test]
    fn display_mode_name_formats_and_interns() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_render(|r| r.set_display_modes(vec![[800, 600, 32]]));
        let word = call(&mut vm, &[Value::int(0)], "displayModeName").expect("a word");
        with_render(|r| assert_eq!(r.string(word), Some("800 600 32")));
        let missing = call(&mut vm, &[Value::int(9)], "displayModeName").expect("a word");
        with_render(|r| assert_eq!(r.string(missing), Some("")));
    }

    /// The two flush rows run their sequence seven times (`0x47c1d1`), clearing
    /// the dirty word and bumping the frame counter each pass.
    #[test]
    fn force_rendering_and_flush_share_the_sequence() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        with_render(|r| r.dirty = 0xDEAD);
        call(&mut vm, &[], "forceRendering");
        with_render(|r| {
            assert_eq!(r.dirty(), 0);
            assert_eq!(r.frame(), 7, "seven passes per call");
            assert_eq!(r.events().len(), 3 * 7);
        });
        call(&mut vm, &[], "flush");
        with_render(|r| assert_eq!(r.frame(), 14));
    }

    /// `changeVideoMode` records its three marshalled words in order.
    #[test]
    fn change_video_mode_records_its_arguments() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        call(
            &mut vm,
            &[Value::int(1), Value::int(2), Value::int(3)],
            "changeVideoMode",
        );
        with_render(|r| {
            assert_eq!(
                r.events(),
                &[RenderEvent::ChangeVideoMode {
                    first: 1,
                    second: 2,
                    third: 3
                }]
            );
            let LogRecord::NativeCall { args, result, .. } = &r.records()[0] else {
                panic!("a native call");
            };
            assert_eq!(
                args,
                &vec![LogValue::Int(1), LogValue::Int(2), LogValue::Int(3)]
            );
            assert_eq!(result, &None);
        });
    }

    /// The text default colour round-trips and starts at `-1`.
    #[test]
    fn the_text_default_colour_round_trips() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(
            call(&mut vm, &[], "getDefaultColor"),
            Some((-1i32) as u32),
            "the image holds -1"
        );
        call(&mut vm, &[Value::int(0x00ff_00ff)], "setDefaultColor");
        assert_eq!(call(&mut vm, &[], "getDefaultColor"), Some(0x00ff_00ff));
    }

    /// Every recorded call carries the marshalled arguments and typed result.
    #[test]
    fn the_call_log_records_arguments_in_declaration_order() {
        reset_render();
        let mut vm = VmHost::new();
        install(&mut vm);
        call(&mut vm, &[Value::int(0)], "displayModeName");
        with_render(|r| {
            let LogRecord::NativeCall { args, result, .. } = &r.records()[0] else {
                panic!("a native call");
            };
            assert_eq!(args, &vec![LogValue::Int(0)]);
            assert!(matches!(result, Some(LogValue::Int(_))));
        });
    }

    #[test]
    fn the_binding_rows_are_the_render_natives() {
        assert_eq!(RENDER_NATIVE_BINDINGS.len(), 17);
        assert_eq!(RENDER_NATIVE_BINDINGS[0].name, "forceRendering");
        assert_eq!(RENDER_NATIVE_BINDINGS[0].impl_addr, FORCE_RENDERING);
        assert_eq!(RENDER_NATIVE_BINDINGS[3].name, "displayModeName");
        assert_eq!(RENDER_NATIVE_BINDINGS[3].impl_addr, DISPLAY_MODE_NAME);
        assert_eq!(RENDER_NATIVE_BINDINGS[4].name, "changeVideoMode");
        assert_eq!(RENDER_NATIVE_BINDINGS[4].sig, "(III)V");
        assert_eq!(RENDER_NATIVE_BINDINGS[6].class, TEXT_CLASS);
        assert_eq!(RENDER_NATIVE_BINDINGS[6].impl_addr, TEXT_SET_DEFAULT_COLOR);
        assert_eq!(RENDER_NATIVE_BINDINGS[7].impl_addr, TEXT_GET_DEFAULT_COLOR);
    }
}
