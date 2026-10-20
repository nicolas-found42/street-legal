//! `java.io.Input` / `java.io.MouseCursor` / `java.io.Controller` natives
//! (M1-09 slice I7): device-less logging plus loaded-player mapping identities.
//!
//! `InputHost::call_controller` additionally models the original loaded-player
//! add/delete/reset mapping table. Boot resolves the native resource and uses
//! that path; `call` retains the original no-handle constants below. Original
//! sequence evidence: `re/oracle/controller_mapping_probe.py`. Device sampling,
//! axis metadata, force/smoothing behavior and player commands remain boundaries.
//! The historical device-less slice described below applies to `call`.
//!
//! The 20 rows below are the exe's input-facing natives. None of them is
//! portable yet: every one of them reads the DirectInput device table the
//! port does not model — the device array `0x76f9b0`, its count `0x777434`
//! and the per-device records at `0x770104` + `index * 0x104` — or the
//! controller object a device handler installs at `0x636338` (a class
//! descriptor whose property lookup is `0x426470`). Both only exist after the
//! game has acquired a device (`FUN_004776f0` and the `0x477xxx` block), which
//! is M6-04's `slr-input`, not this slice.
//!
//! What the port *can* reproduce exactly is the arm the exe itself takes when
//! nothing has been enumerated — the state of every port run. So these rows
//! are ported as *logging stubs*:
//!
//! - the declared arguments are marshalled through the shared ABI walk
//!   ([`marshal_args`], the exe's `FUN_0045d910`), exactly as
//!   [`crate::file`] / [`crate::findfile`] do;
//! - the call is recorded as [`LogRecord::NativeCall`] with the marshalled
//!   argument values and the returned one. This is the canonical call-log
//!   record of `docs/call-log.md` §2, i.e. the record the M0-11 hook layer
//!   emits for the same call — with `args` filled, which that layer cannot do
//!   without M1-02's value layout (`re/hook/hook_native.c` logs `argc = 0`);
//! - the native returns the constant the exe's device-less arm returns, and
//!   no state is touched.
//!
//! # The constants (probe: `re/oracle/io_vectors.py`, `input.jsonl`)
//!
//! Each value below is the return of the *real* exe code for a call whose
//! device lookup misses, measured under Unicorn with the field plumbing faked
//! (`0x42a690`/`0x42ab50`/`0x42a010` answer 0, so every arm that needs an
//! object takes its own "no handle" path) and the device globals left at their
//! zero-initialised values (`0x777434 == 0`, `0x76f9b0 == 0`, `0x636338` bound
//! as the exe binds it).
//!
//! | stub | row | result |
//! |---|---|---|
//! | [`INPUT_GET_AXIS`] `0x47c990` | `getAxis(II)F` | `0.0` — [`INPUT_ZERO_FLOAT`] `0x5e73cc`, the `fld` `0x557464`/`0x557b09` both take when the device/axis misses |
//! | [`INPUT_GET_DEVICE_NAME`] `0x47c9c0` | `getDeviceName(I)String` | the interned empty string — see the divergence below |
//! | [`INPUT_MAP_AXIS`] `0x47c9f0` | `mapAxis(GameRef;IIIFFFF)V` | none (`V`) |
//! | [`INPUT_SET_AXIS_SMOOTH`] `0x47ca70` | `setAxisSmooth(GameRef;IFFFF)V` | none (`V`) |
//! | [`INPUT_ACTIVE_AXIS`] `0x47cae0` | `activeAxis(I)I` | `-1` — `0x557ffa` (`or eax, 0xffffffff`), the index/count guard `0x557aab`/`0x557ab5` both jump to |
//! | [`INPUT_AXIS_NAME`] `0x47cb00` | `axisName(II)String` | the interned empty string — `0x5579f0` zeroes the caller's buffer (`0x5579fd`) and answers `-1`; the native interns the buffer regardless (`0x4174a0`) |
//! | [`INPUT_LAST_KEY`] `0x47cc10` | `lastKey()I` | `0` — the poll word `0x556e00` with no key, and the `test eax, 0xff0000` arm returns it |
//! | [`INPUT_CREATE_HOTKEY`] `0x47cc50` | `createHotkey(IIHotkey;GameRef;Osd;I)V` | none (`V`) |
//! | [`INPUT_DELETE_HOTKEY`] `0x47cd00` | `deleteHotkey(Hotkey)V` | none (`V`) |
//! | [`INPUT_CHECK_HOTKEYS`] `0x47cd40` | `checkHotkeys(GameRef;Osd;)V` | none (`V`) |
//! | [`INPUT_FLUSH_HOTKEYS`] `0x47ce80` | `flushHotkeys()V` | none (`V`) |
//! | [`CURSOR_GET_PICKED_POS`] `0x487870` | `getPickedPos()Vector3` | null, after the log appends below |
//! | [`CURSOR_GET_POS`] `0x487970` | `getPos()Vector3` | null, after the log appends below |
//! | [`CONTROLLER_USER_ADD`] `0x477740` | `user_Add(IIIFFFF)I` | `0` (`mov eax, edi`, `edi` zeroed at `0x477772`) |
//! | [`CONTROLLER_USER_DEL`] `0x477810` | `user_Del(III)I` | `0` (`xor eax, eax`, `0x4778a4`) |
//! | [`CONTROLLER_USER_SET_AXIS_SMOOTH`] `0x4778b0` | `user_SetAxisSmooth(IFFFFF)I` | `1` (`mov eax, 1`, `0x477962`) |
//! | [`CONTROLLER_USER_SET_AXIS_SPEED`] `0x477970` | `user_SetAxisSpeed(IF)V` | none (`V`) |
//! | [`CONTROLLER_USER_RESET`] `0x477a00` | `user_Reset()V` | none (`V`) |
//! | [`CONTROLLER_USER_GET_AXIS_VAL`] `0x477a80` | `user_GetAxisVal(I)F` | `0.0` |
//! | [`CONTROLLER_USER_SET_AXIS_FORCE`] `0x477b20` | `user_SetAxisForce(IFF)V` | none (`V`) |
//!
//! # The annotations
//!
//! Every row carries a `/// exe:` line in its address constant's docs, in the
//! ledger's exact form. `Input`/`MouseCursor` use the Ghidra label
//! `re/out/native_api.tsv` records (`FUN_0047c990` for `getAxis`,
//! `LAB_0047cae0` for `activeAxis`, ...); the `Controller` block has no Ghidra
//! function (see the divergences), so those seven carry the plain hex call-site
//! address the registrar holds.
//!
//! # The two log appends
//!
//! Both `MouseCursor` rows test the object's `cursor` field (`0x42a010` with
//! the field name `0x613a8c`/`0x613acc`) and, when it is absent, append `"!"`
//! then their diagnostic to the log sink `0x62e018` (`0x551140(dst, text,
//! 0x100)` twice, then the flush `0x5513b0` and the NUL `0x487961`), and only
//! then answer null (`xor eax, eax`). The two diagnostics are
//! [`MOUSE_ERROR`] `"Mighty Mouse ERROR"` (`0x613ab8`) and [`MOUSE_ERROR_2`]
//! `"Mighty Mouse ERROR 2"` (`0x613af4`). [`InputHost::log`] records them the
//! way [`crate::file::FileHost::log`] records `File`'s diagnostics: the `"!"`
//! prefix as its own entry, then the message.
//!
//! # Divergences (all "unmodelled or unspecified in the original" direction)
//!
//! - The device layer itself is absent (M6-04). Every arm that would read a
//!   device record is unreachable in the port, so the constants above are the
//!   complete behaviour, not a subset of it. In the exe those same arms bind
//!   the device's vtable and call it (`0x5447d0`, `0x54d650`..`0x54d830`).
//! - `getDeviceName` interns a *null* device-name pointer: `0x54de20` is
//!   `0x558100(index)`, which answers `0` (the `xor eax, eax` at `0x558123`)
//!   for an index outside `0..0x777434`, and `0x47c9e2` hands that 0 straight
//!   to the interner. Interning a null C string is undefined in the exe (the
//!   probe faults inside the exe's own interner); the port interns the empty
//!   string instead, the same word [`INPUT_AXIS_NAME`] returns.
//! - `mapAxis` / `setAxisSmooth` / `checkHotkeys` reach `0x426470` with the
//!   marshalled object word and dereference `[word+8]` before any null check;
//!   with no controller object that is a null read inside the exe, so there is
//!   no constant to return — the port records the call and returns nothing.
//!   The probe reproduces the fault (read of 4 bytes at `0x8`, `eip
//!   0x426474`).
//! - The `Controller` rows have no Ghidra function name: they live in the
//!   unattributed block `0x477740..0x477bbf`, between `FUN_004776f0` and
//!   `FUN_00477bc0` (`re/out/index.txt`), and their registrar at `0x477cb0`
//!   sits in Ghidra's *other* gap (`0x477ca6..0x477e00`). `re/out/native_api.tsv`
//!   therefore has no row for them and `Natives::CONTROLLER_EXCLUDED` carries
//!   the seven class/name/signature triples; the addresses below are the call
//!   sites `0x477d22/41/60/7f/9e/bd/dc`.
//! - A `L`/`[` argument is recorded as the raw marshalled word
//!   ([`LogValue::Int`]), where the M0-11 hook layer records a per-run object
//!   id (it never logs raw pointers). The port has no script-object identity
//!   to hand out, so the word is the honest payload; a differential replay of
//!   the port against a hook-layer log must normalise it the way
//!   `re/oracle/io_vectors.py` already normalises the file layer's object word.
//! - A short argument container leaves the missing parameters at 0 (the ABI
//!   walk's own rule, [`marshal_args`]), so the record still has one value per
//!   declared parameter.

use std::cell::RefCell;
use std::collections::HashMap;

use slr_calllog::{LogRecord, Value as LogValue};
use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};

use crate::file::{IoBinding, LOG_PREFIX};

/// The class the eleven `Input` rows register under.
pub const INPUT_CLASS: &str = "java.io.Input";
/// The class the two cursor rows register under.
pub const MOUSE_CURSOR_CLASS: &str = "java.io.MouseCursor";
/// The class the seven ownerless rows register under.
pub const CONTROLLER_CLASS: &str = "java.io.Controller";

/// Exe address of `java.io.Input.getAxis(II)F`.
/// exe: FUN_0047c990
pub const INPUT_GET_AXIS: u32 = 0x0047_c990;
/// Exe address of `java.io.Input.getDeviceName(I)Ljava.lang.String;`.
/// exe: FUN_0047c9c0
pub const INPUT_GET_DEVICE_NAME: u32 = 0x0047_c9c0;
/// Exe address of `java.io.Input.mapAxis(Ljava.util.resource.GameRef;IIIFFFF)V`.
/// exe: LAB_0047c9f0
pub const INPUT_MAP_AXIS: u32 = 0x0047_c9f0;
/// Exe address of `java.io.Input.setAxisSmooth(Ljava.util.resource.GameRef;IFFFF)V`.
/// exe: LAB_0047ca70
pub const INPUT_SET_AXIS_SMOOTH: u32 = 0x0047_ca70;
/// Exe address of `java.io.Input.activeAxis(I)I`.
/// exe: LAB_0047cae0
pub const INPUT_ACTIVE_AXIS: u32 = 0x0047_cae0;
/// Exe address of `java.io.Input.axisName(II)Ljava.lang.String;`.
/// exe: LAB_0047cb00
pub const INPUT_AXIS_NAME: u32 = 0x0047_cb00;
/// Exe address of `java.io.Input.lastKey()I`.
/// exe: LAB_0047cc10
pub const INPUT_LAST_KEY: u32 = 0x0047_cc10;
/// Exe address of
/// `java.io.Input.createHotkey(IILjava.io.Hotkey;Ljava.util.resource.GameRef;Ljava.render.Osd;I)V`.
/// exe: LAB_0047cc50
pub const INPUT_CREATE_HOTKEY: u32 = 0x0047_cc50;
/// Exe address of `java.io.Input.deleteHotkey(Ljava.io.Hotkey;)V`.
/// exe: LAB_0047cd00
pub const INPUT_DELETE_HOTKEY: u32 = 0x0047_cd00;
/// Exe address of
/// `java.io.Input.checkHotkeys(Ljava.util.resource.GameRef;Ljava.render.Osd;)V`.
/// exe: FUN_0047cd40
pub const INPUT_CHECK_HOTKEYS: u32 = 0x0047_cd40;
/// Exe address of `java.io.Input.flushHotkeys()V`.
/// exe: LAB_0047ce80
pub const INPUT_FLUSH_HOTKEYS: u32 = 0x0047_ce80;
/// Exe address of `java.io.MouseCursor.getPickedPos()Ljava.lang.Vector3;`.
/// exe: LAB_00487870
pub const CURSOR_GET_PICKED_POS: u32 = 0x0048_7870;
/// Exe address of `java.io.MouseCursor.getPos()Ljava.lang.Vector3;`.
/// exe: LAB_00487970
pub const CURSOR_GET_POS: u32 = 0x0048_7970;
/// Exe address of `java.io.Controller.user_Add(IIIFFFF)I`.
/// exe: 0x477740
pub const CONTROLLER_USER_ADD: u32 = 0x0047_7740;
/// Exe address of `java.io.Controller.user_Del(III)I`.
/// exe: 0x477810
pub const CONTROLLER_USER_DEL: u32 = 0x0047_7810;
/// Exe address of `java.io.Controller.user_SetAxisSmooth(IFFFFF)I`.
/// exe: 0x4778b0
pub const CONTROLLER_USER_SET_AXIS_SMOOTH: u32 = 0x0047_78b0;
/// Exe address of `java.io.Controller.user_SetAxisSpeed(IF)V`.
/// exe: 0x477970
pub const CONTROLLER_USER_SET_AXIS_SPEED: u32 = 0x0047_7970;
/// Exe address of `java.io.Controller.user_Reset()V`.
/// exe: 0x477a00
pub const CONTROLLER_USER_RESET: u32 = 0x0047_7a00;
/// Exe address of `java.io.Controller.user_GetAxisVal(I)F`.
/// exe: 0x477a80
pub const CONTROLLER_USER_GET_AXIS_VAL: u32 = 0x0047_7a80;
/// Exe address of `java.io.Controller.user_SetAxisForce(IFF)V`.
/// exe: 0x477b20
pub const CONTROLLER_USER_SET_AXIS_FORCE: u32 = 0x0047_7b20;

/// The float `0x5e73cc` the axis lookups load when the device or the axis
/// misses (`0x557464`, `0x557b09`); the image holds `0.0`.
pub const INPUT_ZERO_FLOAT: f32 = 0.0;

/// The device array base (`0x76f9b0`), one record per enumerated device.
pub const DEVICE_TABLE: u32 = 0x0076_f9b0;
/// The enumerated-device count (`0x777434`); 0 in every port run.
pub const DEVICE_COUNT: u32 = 0x0077_7434;
/// The device records' base (`0x770104`), `index * 0x104` apart.
pub const DEVICE_RECORDS: u32 = 0x0077_0104;
/// One device record's stride (`0x558110`..`0x55811b`).
pub const DEVICE_STRIDE: u32 = 0x104;
/// The input controller class descriptor `0x636338` `0x426470` is bound to.
pub const CONTROLLER_DESCRIPTOR: u32 = 0x0063_6338;
/// The log sink buffer (`0x62e018`) the `MouseCursor` arms append to.
pub const LOG_SINK: u32 = 0x0062_e018;
/// `0x613ab8`: `getPickedPos`'s diagnostic.
pub const MOUSE_ERROR: &str = "Mighty Mouse ERROR";
/// `0x613af4`: `getPos`'s diagnostic.
pub const MOUSE_ERROR_2: &str = "Mighty Mouse ERROR 2";

/// The most parameter words any row of this module declares
/// (`mapAxis`'s `L` + `III` + `FFFF` = 8), plus the receiver slot.
pub const MAX_WORDS: usize = 10;

const _: () = assert!(MAX_WORDS >= 9);

/// What a stub's device-less arm returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StubResult {
    /// `V`: nothing is pushed on the operand stack.
    Void,
    /// The constant integer word the exe returns.
    Int(i32),
    /// The constant float the exe returns (`F`).
    Float(f32),
    /// `0x4174a0` interned the empty string.
    EmptyString,
    /// Null, after the exe appends `"!"` and `text` to [`LOG_SINK`].
    NullWithLog {
        /// The diagnostic (`0x613ab8` / `0x613af4`).
        text: &'static str,
    },
}

/// One stubbed row: the registry binding it answers and the constant its
/// device-less arm returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stub {
    /// The registry row (`re/out/native_api.tsv` 159-171 for `Input` /
    /// `MouseCursor`; the call sites' names for `Controller`, see the module
    /// docs).
    pub binding: IoBinding,
    /// The constant.
    pub result: StubResult,
}

/// One stubbed row, spelled out.
const fn stub(
    class: &'static str,
    name: &'static str,
    sig: &'static str,
    impl_addr: u32,
    result: StubResult,
) -> Stub {
    Stub {
        binding: IoBinding {
            class,
            name,
            sig,
            impl_addr,
        },
        result,
    }
}

/// The eleven `java.io.Input` rows, in the exe's registration order
/// (`re/out/native_api.tsv` 159-169).
pub const INPUT_STUBS: [Stub; 11] = [
    stub(
        INPUT_CLASS,
        "getAxis",
        "(II)F",
        INPUT_GET_AXIS,
        StubResult::Float(INPUT_ZERO_FLOAT),
    ),
    stub(
        INPUT_CLASS,
        "getDeviceName",
        "(I)Ljava.lang.String;",
        INPUT_GET_DEVICE_NAME,
        StubResult::EmptyString,
    ),
    stub(
        INPUT_CLASS,
        "mapAxis",
        "(Ljava.util.resource.GameRef;IIIFFFF)V",
        INPUT_MAP_AXIS,
        StubResult::Void,
    ),
    stub(
        INPUT_CLASS,
        "setAxisSmooth",
        "(Ljava.util.resource.GameRef;IFFFF)V",
        INPUT_SET_AXIS_SMOOTH,
        StubResult::Void,
    ),
    stub(
        INPUT_CLASS,
        "activeAxis",
        "(I)I",
        INPUT_ACTIVE_AXIS,
        StubResult::Int(-1),
    ),
    stub(
        INPUT_CLASS,
        "axisName",
        "(II)Ljava.lang.String;",
        INPUT_AXIS_NAME,
        StubResult::EmptyString,
    ),
    stub(
        INPUT_CLASS,
        "lastKey",
        "()I",
        INPUT_LAST_KEY,
        StubResult::Int(0),
    ),
    stub(
        INPUT_CLASS,
        "createHotkey",
        "(IILjava.io.Hotkey;Ljava.util.resource.GameRef;Ljava.render.Osd;I)V",
        INPUT_CREATE_HOTKEY,
        StubResult::Void,
    ),
    stub(
        INPUT_CLASS,
        "deleteHotkey",
        "(Ljava.io.Hotkey;)V",
        INPUT_DELETE_HOTKEY,
        StubResult::Void,
    ),
    stub(
        INPUT_CLASS,
        "checkHotkeys",
        "(Ljava.util.resource.GameRef;Ljava.render.Osd;)V",
        INPUT_CHECK_HOTKEYS,
        StubResult::Void,
    ),
    stub(
        INPUT_CLASS,
        "flushHotkeys",
        "()V",
        INPUT_FLUSH_HOTKEYS,
        StubResult::Void,
    ),
];

/// The two `java.io.MouseCursor` rows (`re/out/native_api.tsv` 170-171).
pub const MOUSE_CURSOR_STUBS: [Stub; 2] = [
    stub(
        MOUSE_CURSOR_CLASS,
        "getPickedPos",
        "()Ljava.lang.Vector3;",
        CURSOR_GET_PICKED_POS,
        StubResult::NullWithLog { text: MOUSE_ERROR },
    ),
    stub(
        MOUSE_CURSOR_CLASS,
        "getPos",
        "()Ljava.lang.Vector3;",
        CURSOR_GET_POS,
        StubResult::NullWithLog {
            text: MOUSE_ERROR_2,
        },
    ),
];

/// The seven `java.io.Controller` rows, in registration order. The exe
/// registers these from the unattributed registrar at `0x477cb0`, so they are
/// absent from `re/out/native_api.tsv` and carried by
/// `slr_vm::natives::CONTROLLER_EXCLUDED`; the addresses are the binary's own
/// call sites `0x477d22/41/60/7f/9e/bd/dc` (see the module docs).
pub const CONTROLLER_STUBS: [Stub; 7] = [
    stub(
        CONTROLLER_CLASS,
        "user_Add",
        "(IIIFFFF)I",
        CONTROLLER_USER_ADD,
        StubResult::Int(0),
    ),
    stub(
        CONTROLLER_CLASS,
        "user_Del",
        "(III)I",
        CONTROLLER_USER_DEL,
        StubResult::Int(0),
    ),
    stub(
        CONTROLLER_CLASS,
        "user_SetAxisSmooth",
        "(IFFFFF)I",
        CONTROLLER_USER_SET_AXIS_SMOOTH,
        StubResult::Int(1),
    ),
    stub(
        CONTROLLER_CLASS,
        "user_SetAxisSpeed",
        "(IF)V",
        CONTROLLER_USER_SET_AXIS_SPEED,
        StubResult::Void,
    ),
    stub(
        CONTROLLER_CLASS,
        "user_Reset",
        "()V",
        CONTROLLER_USER_RESET,
        StubResult::Void,
    ),
    stub(
        CONTROLLER_CLASS,
        "user_GetAxisVal",
        "(I)F",
        CONTROLLER_USER_GET_AXIS_VAL,
        StubResult::Float(INPUT_ZERO_FLOAT),
    ),
    stub(
        CONTROLLER_CLASS,
        "user_SetAxisForce",
        "(IFF)V",
        CONTROLLER_USER_SET_AXIS_FORCE,
        StubResult::Void,
    ),
];

/// Every row this module ports, the exe's registration order.
pub fn all_stubs() -> impl Iterator<Item = &'static Stub> {
    INPUT_STUBS
        .iter()
        .chain(MOUSE_CURSOR_STUBS.iter())
        .chain(CONTROLLER_STUBS.iter())
}

/// The stub registered at `addr`, `None` for an address this module does not
/// answer.
#[must_use]
pub fn stub_by_addr(addr: u32) -> Option<&'static Stub> {
    all_stubs().find(|stub| stub.binding.impl_addr == addr)
}

/// The `Input` natives' runtime: the call records, the exe's log sink and the
/// string words the interners hand out.
///
/// exe: the device table `0x76f9b0`/`0x777434`/`0x770104` plus the log sink
/// `0x62e018` — none of which the port models, so the host holds only what the
/// stubs produce.
#[derive(Debug, Clone, PartialEq)]
pub struct InputHost {
    records: Vec<LogRecord>,
    log: Vec<String>,
    strings: HashMap<u32, Vec<u8>>,
    next_string: u32,
    controllers: HashMap<u32, ControllerMappings>,
}

/// Identity/state projection of original `FUN_0054d650`'s 152 mapping slots.
/// Samples remain zero in the declared headless input scenario.
#[derive(Debug, Clone, PartialEq)]
struct ControllerMappings {
    slots: [[u32; 8]; 152],
    count: u32,
}

impl Default for ControllerMappings {
    fn default() -> Self {
        Self {
            slots: [[0; 8]; 152],
            count: 0,
        }
    }
}

impl ControllerMappings {
    fn call(&mut self, address: u32, words: &[u32]) -> Option<u32> {
        match address {
            CONTROLLER_USER_ADD => {
                let args: [u32; 7] = std::array::from_fn(|i| word_at(words, i + 1));
                if (args[1] as i32) < 0 || (args[2] as i32) < 0 {
                    return Some(u32::MAX);
                }
                let Some(index) = self.slots.iter().position(|slot| slot[0] == 0) else {
                    return Some(0);
                };
                self.slots[index] = [
                    args[0], args[1], args[2], 0, args[3], args[4], args[5], args[6],
                ];
                self.count = self.count.wrapping_add(1);
                Some(index as u32 + 1)
            }
            CONTROLLER_USER_DEL => {
                let key: [u32; 3] = std::array::from_fn(|i| word_at(words, i + 1));
                if let Some(slot) = self.slots.iter_mut().find(|slot| slot[..3] == key) {
                    slot[0] = 0;
                    self.count = self.count.wrapping_sub(1);
                }
                Some(0)
            }
            CONTROLLER_USER_RESET => {
                // Original reset clears only each mapping's action sentinel.
                for slot in &mut self.slots {
                    slot[0] = 0;
                }
                self.count = 0;
                None
            }
            _ => None,
        }
    }
}

impl Default for InputHost {
    fn default() -> Self {
        InputHost::new()
    }
}

impl InputHost {
    /// A host that has recorded nothing.
    #[must_use]
    pub fn new() -> InputHost {
        InputHost {
            records: Vec::new(),
            log: Vec::new(),
            strings: HashMap::new(),
            next_string: 0,
            controllers: HashMap::new(),
        }
    }

    /// The call records so far, in order.
    #[must_use]
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// Take the recorded calls (tests replay one native at a time).
    pub fn take_records(&mut self) -> Vec<LogRecord> {
        core::mem::take(&mut self.records)
    }

    /// The message the exe's log sink (`0x62e018`) received, in order: the
    /// `"!"` prefix as its own entry, then the diagnostic. Empty for every
    /// stub but the two `MouseCursor` rows.
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Register the bytes behind a string word — the seam for a script-side
    /// `java.lang.String` argument, the way [`crate::file::FileHost::set_string`]
    /// is for `File`.
    pub fn set_string(&mut self, word: u32, bytes: &[u8]) {
        self.strings.insert(word, bytes.to_vec());
    }

    /// The bytes behind a string word: `None` for the null word 0 and for a
    /// word nothing registered.
    #[must_use]
    pub fn string(&self, word: u32) -> Option<&[u8]> {
        if word == 0 {
            return None;
        }
        self.strings.get(&word).map(Vec::as_slice)
    }

    /// Run one stub over the marshalled words `marshal_args` wrote
    /// (`words[0]` is the receiver slot): record the call and answer its
    /// constant.
    pub fn call(&mut self, stub: &Stub, words: &[u32]) -> Option<u32> {
        // The exe appends before it returns (`0x487929`, `0x487a15`).
        if let StubResult::NullWithLog { text } = stub.result {
            self.log.push(LOG_PREFIX.to_owned());
            self.log.push(text.to_owned());
        }
        self.call_with_result(stub, words, None)
    }

    /// Run a mapping operation for a verified loaded native player. `owner`
    /// is the resource identity, shared by all script wrappers of that player.
    /// Other controller operations retain their explicit headless constants.
    pub fn call_controller(&mut self, stub: &Stub, words: &[u32], owner: u32) -> Option<u32> {
        let override_result = matches!(
            stub.binding.impl_addr,
            CONTROLLER_USER_ADD | CONTROLLER_USER_DEL | CONTROLLER_USER_RESET
        )
        .then(|| {
            self.controllers
                .entry(owner)
                .or_default()
                .call(stub.binding.impl_addr, words)
        });
        self.call_with_result(stub, words, override_result)
    }

    fn call_with_result(
        &mut self,
        stub: &Stub,
        words: &[u32],
        override_result: Option<Option<u32>>,
    ) -> Option<u32> {
        let word = override_result.unwrap_or_else(|| match stub.result {
            StubResult::Void => None,
            StubResult::Int(value) => Some(value as u32),
            StubResult::Float(value) => Some(value.to_bits()),
            StubResult::EmptyString => Some(self.intern(b"")),
            StubResult::NullWithLog { .. } => Some(0),
        });
        let sig = stub.binding.sig;
        let args = param_types(sig)
            .into_iter()
            .enumerate()
            .map(|(index, ty)| log_value(ty, word_at(words, index + 1)))
            .collect();
        let result = word.map(|word| log_value(return_type(sig), word));
        self.records.push(LogRecord::NativeCall {
            class: stub.binding.class.to_owned(),
            name: stub.binding.name.to_owned(),
            args,
            result,
        });
        word
    }

    /// `0x4174a0`: intern `bytes` and return this host's word for the string
    /// (the exe returns the interned `String` object).
    fn intern(&mut self, bytes: &[u8]) -> u32 {
        self.next_string = self.next_string.wrapping_add(1);
        self.strings.insert(self.next_string, bytes.to_vec());
        self.next_string
    }
}

/// The declared parameter types of `sig`, in declaration order: the bytes of
/// the descriptor between `(` and `)`, with `L...;` collapsed to its `L` and
/// `[[L...;` to its `[`.
#[must_use]
pub fn param_types(sig: &str) -> Vec<u8> {
    let bytes = sig.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => i += 1,
            b')' => break,
            b'L' => {
                out.push(b'L');
                while i < bytes.len() && bytes[i] != b';' {
                    i += 1;
                }
                i += 1;
            }
            b'[' => {
                while i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                }
                // The element type: `L...;` or one primitive byte.
                if i < bytes.len() && bytes[i] == b'L' {
                    while i < bytes.len() && bytes[i] != b';' {
                        i += 1;
                    }
                }
                i += 1;
                out.push(b'[');
            }
            ty => {
                out.push(ty);
                i += 1;
            }
        }
    }
    out
}

/// `sig`'s return type: its last byte after the parameter list's `)`.
#[must_use]
pub fn return_type(sig: &str) -> u8 {
    let bytes = sig.as_bytes();
    let close = bytes.iter().rposition(|&b| b == b')').unwrap_or(0);
    bytes.get(close + 1).copied().unwrap_or(b'V')
}

/// The marshalled word of the `index`th parameter (`words[0]` is the receiver
/// slot); a word the container did not reach reads 0, the ABI walk's own rule.
fn word_at(words: &[u32], index: usize) -> u32 {
    words.get(index).copied().unwrap_or(0)
}

/// The canonical call-log value of one marshalled word, typed by its
/// descriptor byte exactly as the M0-11 hook layer types its arguments and
/// results (`re/hook/hook_native.c`): `F` is the raw `f32` bits, `L`/`[` the
/// object word, everything else the integer.
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
    static INPUT: RefCell<InputHost> = RefCell::new(InputHost::new());
}

/// Run `f` over this thread's [`InputHost`].
pub fn with_input<R>(f: impl FnOnce(&mut InputHost) -> R) -> R {
    INPUT.with(|cell| f(&mut cell.borrow_mut()))
}

/// Reset this thread's [`InputHost`] to a fresh one.
pub fn reset_input() {
    INPUT.with(|cell| *cell.borrow_mut() = InputHost::new());
}

/// Register every row in the host's native registry (`FUN_00416b00`).
/// Idempotent: the exe's registrar overwrites a record.
pub fn register(vm: &mut VmHost) {
    for stub in all_stubs() {
        let row = stub.binding;
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`NativeFn`] installed at every address.
///
/// The VM has already resolved the record and built the `CallInfo`; this
/// recovers `record+8` from the same registry (the `NativeFn` ABI carries no
/// address), marshals the container with the shared `FUN_0045d910` walk and
/// dispatches into the thread-local [`InputHost`]. The stub records the
/// marshalled arguments and returns its constant.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let stub = stub_by_addr(addr)?;
    let mut out = [0u32; MAX_WORDS];
    let written = marshal_args(info, &mut out).unwrap_or(0);
    with_input(|input| input.call(stub, &out[..written]))
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for stub in all_stubs() {
        vm.native_impls
            .insert(stub.binding.impl_addr, bridge as NativeFn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::values::Value;

    /// The receiver word every test uses.
    const RECEIVER: u32 = 0x4000;

    /// A synthetic call into `bridge`: the container is
    /// `[last param .. first param, receiver]`, which is the order
    /// `marshal_args` reads.
    fn call(vm: &mut VmHost, stub: &Stub, params: &[Value]) -> Option<u32> {
        let mut args: Vec<Value> = params.iter().rev().copied().collect();
        args.push(Value::reference(RECEIVER));
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: stub.binding.class,
            name: stub.binding.name,
            sig: stub.binding.sig,
        };
        bridge(vm, &info)
    }

    /// A distinct cell per parameter, so a swapped or dropped argument is
    /// visible in the record.
    fn param_cells(sig: &str) -> Vec<Value> {
        param_types(sig)
            .into_iter()
            .enumerate()
            .map(|(i, ty)| match ty {
                b'F' => Value::float(i as f32 + 0.5),
                b'L' | b'[' => Value::reference(0x100 + i as u32),
                _ => Value::int(i as i32 + 1),
            })
            .collect()
    }

    /// The record `call` must produce for `stub`.
    fn expected_record(stub: &Stub, result: Option<u32>) -> LogRecord {
        let sig = stub.binding.sig;
        LogRecord::NativeCall {
            class: stub.binding.class.to_owned(),
            name: stub.binding.name.to_owned(),
            args: param_types(sig)
                .into_iter()
                .enumerate()
                .map(|(i, ty)| {
                    let word = match ty {
                        b'F' => (i as f32 + 0.5).to_bits(),
                        b'L' | b'[' => 0x100 + i as u32,
                        _ => i as u32 + 1,
                    };
                    log_value(ty, word)
                })
                .collect(),
            result: result.map(|word| log_value(return_type(sig), word)),
        }
    }

    /// Every row registers under its class and `install` wires the bridge at
    /// exactly those addresses.
    #[test]
    fn register_and_install_wire_the_rows() {
        reset_input();
        let mut vm = VmHost::new();
        register(&mut vm);
        register(&mut vm);
        let stubs: Vec<&Stub> = all_stubs().collect();
        assert_eq!(stubs.len(), 20);
        assert_eq!(vm.natives.len(), stubs.len());
        for stub in &stubs {
            let row = stub.binding;
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
        }
        install(&mut vm);
        for stub in &stubs {
            assert!(
                vm.native_impls.contains_key(&stub.binding.impl_addr),
                "{} {}{} uninstalled",
                stub.binding.class,
                stub.binding.name,
                stub.binding.sig
            );
        }
    }

    /// Every one of the twenty rows records its exact marshalled arguments
    /// and returns the exe's constant, in the canonical call-log shape.
    #[test]
    fn every_row_records_its_arguments_and_returns_the_exe_constant() {
        let mut vm = VmHost::new();
        install(&mut vm);
        reset_input();
        for stub in all_stubs().collect::<Vec<_>>() {
            let params = param_cells(stub.binding.sig);
            let got = call(&mut vm, stub, &params);
            let want = match stub.result {
                StubResult::Void => None,
                StubResult::Int(value) => Some(value as u32),
                StubResult::Float(value) => Some(value.to_bits()),
                StubResult::EmptyString => Some(1),
                StubResult::NullWithLog { .. } => Some(0),
            };
            assert_eq!(
                got, want,
                "{} {}{} result",
                stub.binding.class, stub.binding.name, stub.binding.sig
            );
            let records = with_input(InputHost::take_records);
            assert_eq!(
                records,
                vec![expected_record(stub, want)],
                "{} {}{} record",
                stub.binding.class,
                stub.binding.name,
                stub.binding.sig
            );
            reset_input();
        }
    }

    /// The constants themselves, pinned row by row (probed; see the module
    /// docs).
    #[test]
    fn the_constants_are_the_exes() {
        assert_eq!(INPUT_STUBS[0].result, StubResult::Float(0.0), "getAxis");
        assert_eq!(
            INPUT_STUBS[1].result,
            StubResult::EmptyString,
            "getDeviceName"
        );
        assert_eq!(INPUT_STUBS[2].result, StubResult::Void, "mapAxis");
        assert_eq!(INPUT_STUBS[3].result, StubResult::Void, "setAxisSmooth");
        assert_eq!(INPUT_STUBS[4].result, StubResult::Int(-1), "activeAxis");
        assert_eq!(INPUT_STUBS[5].result, StubResult::EmptyString, "axisName");
        assert_eq!(INPUT_STUBS[6].result, StubResult::Int(0), "lastKey");
        for stub in &INPUT_STUBS[7..] {
            assert_eq!(stub.result, StubResult::Void, "{}", stub.binding.name);
        }
        assert_eq!(
            MOUSE_CURSOR_STUBS[0].result,
            StubResult::NullWithLog { text: MOUSE_ERROR }
        );
        assert_eq!(
            MOUSE_CURSOR_STUBS[1].result,
            StubResult::NullWithLog {
                text: MOUSE_ERROR_2
            }
        );
        assert_eq!(CONTROLLER_STUBS[0].result, StubResult::Int(0), "user_Add");
        assert_eq!(CONTROLLER_STUBS[1].result, StubResult::Int(0), "user_Del");
        assert_eq!(
            CONTROLLER_STUBS[2].result,
            StubResult::Int(1),
            "user_SetAxisSmooth"
        );
        assert_eq!(CONTROLLER_STUBS[3].result, StubResult::Void);
        assert_eq!(CONTROLLER_STUBS[4].result, StubResult::Void);
        assert_eq!(
            CONTROLLER_STUBS[5].result,
            StubResult::Float(0.0),
            "user_GetAxisVal"
        );
        assert_eq!(CONTROLLER_STUBS[6].result, StubResult::Void);
    }

    /// A `V` row pushes nothing and records no result.
    #[test]
    fn a_void_row_pushes_nothing() {
        let mut vm = VmHost::new();
        install(&mut vm);
        reset_input();
        let stub = stub_by_addr(INPUT_FLUSH_HOTKEYS).expect("flushHotkeys");
        assert_eq!(call(&mut vm, stub, &[]), None);
        assert_eq!(
            with_input(InputHost::take_records),
            vec![LogRecord::NativeCall {
                class: INPUT_CLASS.to_owned(),
                name: "flushHotkeys".to_owned(),
                args: vec![],
                result: None,
            }]
        );
    }

    /// Both `MouseCursor` arms append `"!"` then their diagnostic to the log
    /// sink, and still answer null (`0x487929`, `0x487a15`).
    #[test]
    fn the_mouse_cursor_rows_log_the_mighty_mouse_error() {
        let mut vm = VmHost::new();
        install(&mut vm);
        for (addr, text) in [
            (CURSOR_GET_PICKED_POS, MOUSE_ERROR),
            (CURSOR_GET_POS, MOUSE_ERROR_2),
        ] {
            reset_input();
            let stub = stub_by_addr(addr).expect("a cursor row");
            assert_eq!(call(&mut vm, stub, &[]), Some(0));
            with_input(|input| {
                assert_eq!(input.log(), &["!".to_owned(), text.to_owned()]);
                assert_eq!(input.records().len(), 1);
                match &input.records()[0] {
                    LogRecord::NativeCall { args, result, .. } => {
                        assert!(args.is_empty());
                        assert_eq!(result, &Some(LogValue::Int(0)));
                    }
                    other => panic!("not a native call: {other:?}"),
                }
            });
        }
        reset_input();
        assert!(with_input(|input| input.log().is_empty()));
    }

    /// The two string rows intern the empty string, one fresh word per call.
    #[test]
    fn the_string_rows_intern_the_empty_string() {
        let mut vm = VmHost::new();
        install(&mut vm);
        reset_input();
        let first = call(
            &mut vm,
            stub_by_addr(INPUT_AXIS_NAME).expect("axisName"),
            &param_cells("(II)Ljava.lang.String;"),
        );
        let second = call(
            &mut vm,
            stub_by_addr(INPUT_GET_DEVICE_NAME).expect("getDeviceName"),
            &param_cells("(I)Ljava.lang.String;"),
        );
        assert_eq!(first, Some(1));
        assert_eq!(second, Some(2));
        with_input(|input| {
            assert_eq!(input.string(1), Some(&b""[..]));
            assert_eq!(input.string(2), Some(&b""[..]));
            assert_eq!(input.string(0), None);
            assert_eq!(input.string(3), None);
        });
    }

    /// A short container leaves the missing parameters at 0 — the ABI walk's
    /// rule, so the record keeps one value per declared parameter.
    #[test]
    fn a_short_container_records_zero_for_the_missing_parameters() {
        let mut vm = VmHost::new();
        install(&mut vm);
        reset_input();
        let stub = stub_by_addr(INPUT_ACTIVE_AXIS).expect("activeAxis");
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[Value::int(2), Value::reference(RECEIVER)],
            class: stub.binding.class,
            name: stub.binding.name,
            sig: stub.binding.sig,
        };
        // The receiver slot and the one declared parameter both come from the
        // container here (`count = 2`), so the call is complete.
        assert_eq!(bridge(&mut vm, &info), Some(-1i32 as u32));

        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[Value::reference(RECEIVER)],
            class: stub.binding.class,
            name: stub.binding.name,
            sig: stub.binding.sig,
        };
        reset_input();
        assert_eq!(bridge(&mut vm, &info), Some(-1i32 as u32));
        assert_eq!(
            with_input(InputHost::take_records),
            vec![LogRecord::NativeCall {
                class: INPUT_CLASS.to_owned(),
                name: "activeAxis".to_owned(),
                args: vec![LogValue::Int(0)],
                result: Some(LogValue::Int(-1)),
            }]
        );
    }

    /// A row this module does not answer records nothing and returns nothing.
    #[test]
    fn an_unregistered_row_is_not_answered() {
        let mut vm = VmHost::new();
        install(&mut vm);
        reset_input();
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[Value::reference(RECEIVER)],
            class: "java.io.Input",
            name: "notARow",
            sig: "()V",
        };
        assert_eq!(bridge(&mut vm, &info), None);
        assert!(with_input(|input| input.records().is_empty()));
    }

    #[test]
    fn controller_mappings_replay_original_sequence() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/controller-mapping.json");
        let source = std::fs::read_to_string(&path)
            .expect("generate re/oracle/controller_mapping_probe.py output first");
        let row: serde_json::Value = serde_json::from_str(&source).unwrap();
        let mut input = InputHost::new();
        for (index, operation) in row["operations"].as_array().unwrap().iter().enumerate() {
            let address = match operation["name"].as_str().unwrap() {
                "add" => CONTROLLER_USER_ADD,
                "del" => CONTROLLER_USER_DEL,
                "reset" => CONTROLLER_USER_RESET,
                name => panic!("unknown operation {name}"),
            };
            let stub = stub_by_addr(address).unwrap();
            let mut words = vec![123];
            words.extend(
                operation["params"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32),
            );
            let result = input.call_controller(stub, &words, 17);
            assert_eq!(
                result,
                operation["result"].as_u64().map(|v| v as u32),
                "operation {index}"
            );
            let state = input.controllers.get(&17).unwrap();
            assert_eq!(
                state.count as u64,
                operation["count"].as_u64().unwrap(),
                "count {index}"
            );
            assert_eq!(
                serde_json::to_value(&state.slots[..3]).unwrap(),
                operation["slots"],
                "slots {index}"
            );
        }
        // Resource identities isolate mapping tables, and missing handles keep
        // the original device-less result.
        let stub = stub_by_addr(CONTROLLER_USER_ADD).unwrap();
        assert_eq!(input.call_controller(stub, &[123, 7, 0, 1], 18), Some(1));
        assert_eq!(input.call(stub, &[123, 7, 0, 1]), Some(0));
    }

    /// The descriptor walk behind the record: parameter types, return type.
    #[test]
    fn the_descriptor_walk_reads_the_row_spellings() {
        assert_eq!(
            param_types("(Ljava.util.resource.GameRef;IIIFFFF)V"),
            vec![b'L', b'I', b'I', b'I', b'F', b'F', b'F', b'F']
        );
        assert_eq!(
            param_types("(IILjava.io.Hotkey;Ljava.util.resource.GameRef;Ljava.render.Osd;I)V"),
            vec![b'I', b'I', b'L', b'L', b'L', b'I']
        );
        assert_eq!(param_types("[[Ljava.lang.String;I)V"), vec![b'[', b'I']);
        assert_eq!(param_types("()V"), Vec::<u8>::new());
        assert_eq!(return_type("(II)F"), b'F');
        assert_eq!(return_type("()Ljava.lang.String;"), b'L');
        assert_eq!(return_type("(I)V"), b'V');
    }
}
