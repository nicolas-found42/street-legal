//! `java.sound.Sound` natives (M1-12, issue #39): the ten registered rows,
//! ported against the music object the exe keeps at `0x64091c`.
//!
//! The exe's sound layer is a DirectSound wrapper (`FUN_004776f0` and the
//! `0x477xxx` block) that only exists after a device is acquired, which is
//! M6-01/M6-02. What the natives themselves do, however, is small and fully
//! readable: they either read/write a word of the music object, read one of
//! three **global volume words**, or tail-call a two-instruction capability
//! probe over a DirectSound global. This module ports exactly that, and the
//! call-log record each row emits (`LogRecord::NativeCall`, the record the
//! M0-11 hook layer emits for the same call) is the headless backend M1-12
//! asks for.
//!
//! # The rows
//!
//! | exe | row | body |
//! |---|---|---|
//! | [`CHANGE_MUSIC_SET`] `0x487460` | `changeMusicSet(I)V` | select one of four static set objects (`0x63c878`, `0x640908`, `0x6408a8`, `0x640890`), release the current one (`0x550f30`) and install the new one at `0x64091c` |
//! | [`NEXT_TRACK`] `0x487500` | `nextTrack()V` | `0x550f40(current)`: `[obj+4]++` then `0x550e50` reload |
//! | [`PREV_TRACK`] `0x487510` | `prevTrack()V` | `0x550f60(current)`: `[obj+4]--` then the same reload |
//! | [`GET_MODE`] `0x487560` | `getMode()I` | `current ? [current+0x10] : -1` |
//! | [`SET_MODE`] `0x487520` | `setMode(I)I` | when `current` and `0 <= arg <= 3`, store `arg` at `[current+0x10]`; **returns the stored (or current) mode** |
//! | [`SET_VOLUME`] `0x487580` | `setVolume(IF)V` | channel `0` → `0x612c58`, `1` → `0x612c5c` (and `0x550fe0` when a music object exists), `2` → `0x612c60`; any other channel is ignored |
//! | [`GET_VOLUME`] `0x4875f0` | `getVolume(I)F` | the same three words; an unknown channel answers the zero float `0x5e73cc` |
//! | [`HAS_3D_HARDWARE`] `0x487640` | `has3DHardware()I` | `[0x77c9b8] < 4 ? 0 : 1` (`cmp/sbb/inc`, `0x559e20`) |
//! | [`HAS_MIX_HARDWARE`] `0x487650` | `hasMixHardware()I` | the same over `[0x77c9a0]` (`0x559e30`) |
//! | [`ENABLE_DEBUG_DUMP`] `0x487f00` | `enableDebugDump(ResourceRef)I` | marshals, discards the argument, answers `0` |
//!
//! `nextTrack`/`prevTrack` are `void` even though the helpers they tail-call
//! return a word: the native signature discards it, so the port stores nothing.
//!
//! # Divergences
//!
//! - The exe's `0x550f40`/`0x550f60`/`0x550fe0` reach the DirectSound wrapper
//!   after updating their word. The port updates the same words and records the
//!   reload as [`SoundEvent::Reload`] instead of calling into an absent device
//!   (`docs/adr/0007`). The observable call-log record is unaffected: those
//!   helpers are not natives and emit no `NativeCall`.
//! - `has3DHardware`/`hasMixHardware` read DirectSound globals the port cannot
//!   acquire, so they read this host's modelled words, which start at the
//!   exe's zero-initialised value `0` — the answer the real exe gives when no
//!   device was created.

use std::cell::RefCell;
use std::collections::HashMap;

use slr_calllog::model::{LogRecord, Value as LogValue};
use slr_vm::host::{marshal_args, CallInfo, VmHost};
#[cfg(test)]
use slr_vm::values::Value;

// --------------------------------------------------------------------------
// The registry rows
// --------------------------------------------------------------------------

/// One `java.sound.Sound` registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundBinding {
    /// `java.sound.Sound` for every row of this module.
    pub class: &'static str,
    /// The registered method name.
    pub name: &'static str,
    /// The registered descriptor text.
    pub sig: &'static str,
    /// Exe address of the implementation (`record+8`).
    pub impl_addr: u32,
}

/// `java.sound.Sound.changeMusicSet(I)V`.
///
/// exe: 0x487460
pub const CHANGE_MUSIC_SET: u32 = 0x0048_7460;
/// `java.sound.Sound.nextTrack()V`.
///
/// exe: 0x487500
pub const NEXT_TRACK: u32 = 0x0048_7500;
/// `java.sound.Sound.prevTrack()V`.
///
/// exe: 0x487510
pub const PREV_TRACK: u32 = 0x0048_7510;
/// `java.sound.Sound.setMode(I)I`.
///
/// exe: 0x487520
pub const SET_MODE: u32 = 0x0048_7520;
/// `java.sound.Sound.getMode()I`.
///
/// exe: 0x487560
pub const GET_MODE: u32 = 0x0048_7560;
/// `java.sound.Sound.setVolume(IF)V`.
///
/// exe: 0x487580
pub const SET_VOLUME: u32 = 0x0048_7580;
/// `java.sound.Sound.getVolume(I)F`.
///
/// exe: 0x4875f0
pub const GET_VOLUME: u32 = 0x0048_75f0;
/// `java.sound.Sound.has3DHardware()I`.
///
/// exe: 0x487640
pub const HAS_3D_HARDWARE: u32 = 0x0048_7640;
/// `java.sound.Sound.hasMixHardware()I`.
///
/// exe: 0x487650
pub const HAS_MIX_HARDWARE: u32 = 0x0048_7650;
/// `java.sound.Sound.enableDebugDump(Ljava.util.resource.ResourceRef;)I`.
///
/// exe: 0x487f00
pub const ENABLE_DEBUG_DUMP: u32 = 0x0048_7f00;

/// The class every row below is registered under.
pub const SOUND_CLASS: &str = "java.sound.Sound";

/// The ten rows, in the exe's registration order (`re/out/native_api.tsv`).
pub const SOUND_NATIVE_BINDINGS: &[SoundBinding] = &[
    SoundBinding {
        class: SOUND_CLASS,
        name: "changeMusicSet",
        sig: "(I)V",
        impl_addr: CHANGE_MUSIC_SET,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "nextTrack",
        sig: "()V",
        impl_addr: NEXT_TRACK,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "prevTrack",
        sig: "()V",
        impl_addr: PREV_TRACK,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "getMode",
        sig: "()I",
        impl_addr: GET_MODE,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "setMode",
        sig: "(I)I",
        impl_addr: SET_MODE,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "setVolume",
        sig: "(IF)V",
        impl_addr: SET_VOLUME,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "getVolume",
        sig: "(I)F",
        impl_addr: GET_VOLUME,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "has3DHardware",
        sig: "()I",
        impl_addr: HAS_3D_HARDWARE,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "hasMixHardware",
        sig: "()I",
        impl_addr: HAS_MIX_HARDWARE,
    },
    SoundBinding {
        class: SOUND_CLASS,
        name: "enableDebugDump",
        sig: "(Ljava.util.resource.ResourceRef;)I",
        impl_addr: ENABLE_DEBUG_DUMP,
    },
];

/// The four static music-set objects `changeMusicSet` selects by index
/// (`0x4874e4`'s jump table: `0x487489`, `0x487490`, `0x487497`, `0x48749e`).
pub const MUSIC_SETS: [u32; 4] = [0x0063_c878, 0x0064_0908, 0x0064_08a8, 0x0064_0890];

/// The three global volume words: channel 0, 1 and 2 (`0x612c58`,
/// `0x612c5c`, `0x612c60`), all initialised to `1.0` in the image.
pub const VOLUME_WORDS: [u32; 3] = [0x0061_2c58, 0x0061_2c5c, 0x0061_2c60];

/// The zero float `getVolume` answers for an unknown channel (`0x5e73cc`).
pub const ZERO_FLOAT: u32 = 0x005e_73cc;

/// The `getMode` answer when no music set is installed (`or eax,-1`,
/// `0x487566`).
pub const NO_MUSIC_MODE: i32 = -1;

/// The DirectSound capability globals the two `has*Hardware` rows read
/// (`0x77c9b8`, `0x77c9a0`), plus the threshold `4` (`cmp ...,0x4`).
pub const DS_3D_GLOBAL: u32 = 0x0077_c9b8;
/// The mix-capability global.
pub const DS_MIX_GLOBAL: u32 = 0x0077_c9a0;
/// The threshold both probes compare against.
pub const DS_CAPABILITY_THRESHOLD: u32 = 4;

/// One sound-layer effect, in the order the exe issues it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundEvent {
    /// `0x550f30`: the previous music set was released.
    Release {
        /// The released set object.
        object: u32,
    },
    /// `0x550e50`: a set was installed. The exe calls it on every selection
    /// (`0x4874c6`) and at the end of every track move (`0x550f51`); its body
    /// is gated on the set's loaded-buffer pointer and count, so for a bare
    /// descriptor it returns immediately — but the *call* is the observable
    /// fact the oracle rows record.
    Install {
        /// The set object that installed.
        object: u32,
    },
    /// `0x550f40`/`0x550f60`: the track index moved and the set reloaded.
    Reload {
        /// The set object that reloaded.
        object: u32,
        /// Its track index after the move.
        track: i32,
    },

    /// `0x550fe0`: channel 1's volume was pushed into the music set.
    MusicVolume {
        /// The new volume bits.
        bits: u32,
    },
}

/// The `Sound` natives' runtime: the music object, the three volume words, the
/// two capability globals and the call records.
///
/// exe: `0x64091c` (the current music set), `0x612c58`/`0x612c5c`/`0x612c60`
/// (the volumes), `0x77c9b8`/`0x77c9a0` (the capability words).
#[derive(Debug, Clone)]
pub struct SoundHost {
    current: Option<u32>,
    tracks: HashMap<u32, i32>,
    modes: HashMap<u32, i32>,
    volumes: [u32; 3],
    ds_3d: u32,
    ds_mix: u32,
    events: Vec<SoundEvent>,
    records: Vec<LogRecord>,
}

impl Default for SoundHost {
    fn default() -> Self {
        SoundHost::new()
    }
}

impl SoundHost {
    /// A host at the image's initial state: no music set, every volume `1.0`,
    /// both capability words `0`.
    #[must_use]
    pub fn new() -> SoundHost {
        SoundHost {
            current: None,
            tracks: HashMap::new(),
            modes: HashMap::new(),
            volumes: [1.0f32.to_bits(); 3],
            ds_3d: 0,
            ds_mix: 0,
            events: Vec::new(),
            records: Vec::new(),
        }
    }

    /// The current music-set object (`[0x64091c]`), if one is installed.
    #[must_use]
    pub fn current(&self) -> Option<u32> {
        self.current
    }

    /// The current set's mode word (`[current+0x10]`).
    #[must_use]
    pub fn mode(&self) -> Option<i32> {
        self.current
            .and_then(|object| self.modes.get(&object).copied())
    }

    /// The current set's track index (`[current+4]`).
    #[must_use]
    pub fn track(&self) -> Option<i32> {
        self.current
            .and_then(|object| self.tracks.get(&object).copied())
    }

    /// One of the three global volume words, as bits.
    #[must_use]
    pub fn volume_bits(&self, channel: usize) -> Option<u32> {
        self.volumes.get(channel).copied()
    }

    /// The modelled DirectSound capability words: `(3D, mix)`.
    #[must_use]
    pub fn capabilities(&self) -> (u32, u32) {
        (self.ds_3d, self.ds_mix)
    }

    /// Set the modelled capability words — the seam a device-acquiring test
    /// or a future `slr-audio` uses.
    pub fn set_capabilities(&mut self, ds_3d: u32, ds_mix: u32) {
        self.ds_3d = ds_3d;
        self.ds_mix = ds_mix;
    }

    /// Install a music set without going through the native — the seam a
    /// resumed driver or a replay test uses to reach a known state.
    pub fn set_current(&mut self, object: Option<u32>) {
        self.current = object;
    }

    /// Set one of the three global volume words, as bits.
    pub fn set_volume_bits(&mut self, channel: usize, bits: u32) {
        if let Some(slot) = self.volumes.get_mut(channel) {
            *slot = bits;
        }
    }

    /// Set the set's own track (`+4`) and mode (`+0x10`) words.
    pub fn set_set_words(&mut self, object: u32, track: i32, mode: i32) {
        self.tracks.insert(object, track);
        self.modes.insert(object, mode);
        // the words are read back from the same map, so seeding them here is
        // also what the getters observe
        let _ = (track, mode);
    }

    /// The sound-layer effects so far, in order.
    #[must_use]
    pub fn events(&self) -> &[SoundEvent] {
        &self.events
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

    /// Run one row over the marshalled words (`words[0]` is the receiver
    /// slot), record the call and answer its result word.
    pub fn call(&mut self, row: &SoundBinding, words: &[u32]) -> Option<u32> {
        let word = match row.impl_addr {
            CHANGE_MUSIC_SET => {
                self.change_music_set(word_at(words, 1) as i32);
                None
            }
            NEXT_TRACK => {
                self.move_track(1);
                None
            }
            PREV_TRACK => {
                self.move_track(-1);
                None
            }
            GET_MODE => Some(self.get_mode() as u32),
            SET_MODE => Some(self.set_mode(word_at(words, 1) as i32) as u32),
            SET_VOLUME => {
                self.set_volume(word_at(words, 1) as i32, word_at(words, 2));
                None
            }
            GET_VOLUME => Some(self.get_volume(word_at(words, 1) as i32)),
            HAS_3D_HARDWARE => Some(u32::from(self.ds_3d >= DS_CAPABILITY_THRESHOLD)),
            HAS_MIX_HARDWARE => Some(u32::from(self.ds_mix >= DS_CAPABILITY_THRESHOLD)),
            ENABLE_DEBUG_DUMP => Some(0),
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

    /// `0x487460`: select one of the four static set objects, release the
    /// current one and install the new one. An index outside `0..=3` selects
    /// the null object, and the same release/install sequence still runs
    /// (`xor esi,esi` at `0x4874a5`).
    fn change_music_set(&mut self, index: i32) {
        let selected = usize::try_from(index)
            .ok()
            .and_then(|i| MUSIC_SETS.get(i).copied());
        if selected == self.current {
            return;
        }
        if let Some(object) = self.current.take() {
            self.events.push(SoundEvent::Release { object });
        }
        self.current = selected;
        // `0x4874ba`: store the selection, then, when it is not null,
        // `0x550e50` (install) and `0x550fe0` (push channel 1's volume).
        if let Some(object) = selected {
            self.events.push(SoundEvent::Install { object });
            self.events.push(SoundEvent::MusicVolume {
                bits: self.volumes[1],
            });
        }
    }

    /// `0x550f40`/`0x550f60`: move the track index by `delta` and reload.
    fn move_track(&mut self, delta: i32) {
        let Some(object) = self.current else {
            return;
        };
        let track = self.tracks.entry(object).or_insert(0);
        *track = track.wrapping_add(delta);
        // `0x550f40`/`0x550f60`: bump the word, then `0x550e50` install.
        self.events.push(SoundEvent::Reload {
            object,
            track: *track,
        });
        self.events.push(SoundEvent::Install { object });
    }

    /// `0x487560`: the current mode, or `-1` when no set is installed.
    fn get_mode(&self) -> i32 {
        self.mode().unwrap_or(NO_MUSIC_MODE)
    }

    /// `0x487520`: store the mode when a set exists and the value is in
    /// `0..=3`; answer the stored mode (or the current one when the guard
    /// rejects the argument; or `-1` with no set — `0x487556` answers the
    /// marshalled argument in that arm).
    fn set_mode(&mut self, mode: i32) -> i32 {
        let Some(object) = self.current else {
            // `0x487556`: no set — the answer is the argument word itself.
            return mode;
        };
        if (0..=3).contains(&mode) {
            self.modes.insert(object, mode);
            mode
        } else {
            self.modes.get(&object).copied().unwrap_or(0)
        }
    }

    /// `0x487580`: channel 0 and 2 store the word; channel 1 stores it and
    /// pushes it into the music set when one exists. Any other channel is
    /// ignored.
    fn set_volume(&mut self, channel: i32, bits: u32) {
        match channel {
            0 => self.volumes[0] = bits,
            1 => {
                self.volumes[1] = bits;
                if self.current.is_some() {
                    self.events.push(SoundEvent::MusicVolume { bits });
                }
            }
            2 => self.volumes[2] = bits,
            _ => {}
        }
    }

    /// `0x4875f0`: the channel's word, or the zero float for an unknown one.
    fn get_volume(&self, channel: i32) -> u32 {
        usize::try_from(channel)
            .ok()
            .and_then(|i| self.volumes.get(i).copied())
            .unwrap_or(0)
    }
}

/// The word at `index`, or `0` when the container ran short (the exe reads a
/// null cell there).
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

/// The return type byte of `sig`: the first byte after `)`.
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
    static SOUND: RefCell<SoundHost> = RefCell::new(SoundHost::new());
}

/// Run `f` over this thread's [`SoundHost`].
pub fn with_sound<R>(f: impl FnOnce(&mut SoundHost) -> R) -> R {
    SOUND.with(|s| f(&mut s.borrow_mut()))
}

/// Reset this thread's sound state (tests isolate one scenario each).
pub fn reset_sound() {
    SOUND.with(|s| *s.borrow_mut() = SoundHost::new());
}

/// Register every row with the VM's native registry.
pub fn register(vm: &mut VmHost) {
    for row in SOUND_NATIVE_BINDINGS {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`slr_vm::host::NativeFn`] installed at every address.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let row = SOUND_NATIVE_BINDINGS
        .iter()
        .find(|row| row.impl_addr == addr)?;
    let mut out = [0u32; 8];
    let written = marshal_args(info, &mut out).unwrap_or(0);
    with_sound(|sound| sound.call(row, &out[..written]))
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in SOUND_NATIVE_BINDINGS {
        vm.native_impls
            .insert(row.impl_addr, bridge as slr_vm::host::NativeFn);
    }
}

/// Install every Sound row and append each call to the ordered headless log.
/// The same [`SoundHost`] and sound behavior are used by [`install`].
pub fn install_headless(vm: &mut VmHost) {
    register(vm);
    for row in SOUND_NATIVE_BINDINGS {
        vm.native_impls
            .insert(row.impl_addr, headless_bridge as slr_vm::host::NativeFn);
    }
}

fn headless_bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    SOUND_NATIVE_BINDINGS
        .iter()
        .find(|row| row.impl_addr == address)?;
    let result = bridge(vm, info);
    with_sound(|host| {
        slr_calllog::headless::record(host.records.last().expect("sound call recorded").clone())
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(args: &'a [Value], name: &'a str, sig: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: SOUND_CLASS,
            name,
            sig,
        }
    }

    /// The receiver cell every `Sound` row reads at `[count-1]`.
    const RECEIVER: u32 = 0x0050_0000;

    fn row(addr: u32) -> &'static SoundBinding {
        SOUND_NATIVE_BINDINGS
            .iter()
            .find(|row| row.impl_addr == addr)
            .expect("row")
    }

    /// Call one row with `params` in declaration order. The exe's container
    /// holds the parameters *reversed* with the receiver at `[count-1]`
    /// (`0x45d910` reads `[count-1]` first, then walks backwards), so the
    /// helper builds exactly that shape.
    fn call(vm: &mut VmHost, params: &[Value], name: &str, sig: &str) -> Option<u32> {
        let mut args: Vec<Value> = params.iter().rev().copied().collect();
        args.push(Value::reference(RECEIVER));
        bridge(vm, &info(&args, name, sig))
    }

    /// Every row installs and resolves through the registry.
    #[test]
    fn every_row_installs_and_resolves() {
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(SOUND_NATIVE_BINDINGS.len(), 10);
        for row in SOUND_NATIVE_BINDINGS {
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

    /// `changeMusicSet` selects the static set, releases the previous one, and
    /// an out-of-range index still releases and installs null.
    #[test]
    fn change_music_set_selects_releases_and_tolerates_range() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        call(&mut vm, &[Value::int(2)], "changeMusicSet", "(I)V");
        with_sound(|s| assert_eq!(s.current(), Some(MUSIC_SETS[2])));
        call(&mut vm, &[Value::int(0)], "changeMusicSet", "(I)V");
        with_sound(|s| {
            assert_eq!(s.current(), Some(MUSIC_SETS[0]));
            assert_eq!(
                s.events(),
                &[
                    SoundEvent::Install {
                        object: MUSIC_SETS[2]
                    },
                    SoundEvent::MusicVolume {
                        bits: 1.0f32.to_bits()
                    },
                    SoundEvent::Release {
                        object: MUSIC_SETS[2]
                    },
                    SoundEvent::Install {
                        object: MUSIC_SETS[0]
                    },
                    SoundEvent::MusicVolume {
                        bits: 1.0f32.to_bits()
                    },
                ]
            );
        });
        call(&mut vm, &[Value::int(9)], "changeMusicSet", "(I)V");
        with_sound(|s| {
            assert_eq!(s.current(), None, "an out-of-range index selects null");
            // a null selection releases but installs nothing
            assert_eq!(
                s.events().last(),
                Some(&SoundEvent::Release {
                    object: MUSIC_SETS[0]
                })
            );
        });
    }

    /// `setMode` stores 0..=3 and answers the stored mode; with no set it
    /// answers the argument; `getMode` answers -1 without a set.
    #[test]
    fn mode_round_trips_through_the_current_set() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(call(&mut vm, &[], "getMode", "()I"), Some((-1i32) as u32));
        assert_eq!(
            call(&mut vm, &[Value::int(2)], "setMode", "(I)I"),
            Some(2),
            "no set: the answer is the argument"
        );
        call(&mut vm, &[Value::int(1)], "changeMusicSet", "(I)V");
        assert_eq!(call(&mut vm, &[Value::int(3)], "setMode", "(I)I"), Some(3));
        assert_eq!(call(&mut vm, &[], "getMode", "()I"), Some(3));
        // Out of range: the guard rejects the store and the current mode stands.
        assert_eq!(call(&mut vm, &[Value::int(7)], "setMode", "(I)I"), Some(3));
        assert_eq!(call(&mut vm, &[], "getMode", "()I"), Some(3));
    }

    /// The track helpers move the index and emit the reload; with no set they
    /// do nothing.
    #[test]
    fn next_and_prev_move_the_track_only_with_a_set() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        call(&mut vm, &[], "nextTrack", "()V");
        with_sound(|s| assert!(s.events().is_empty(), "no set: nothing happens"));
        call(&mut vm, &[Value::int(0)], "changeMusicSet", "(I)V");
        call(&mut vm, &[], "nextTrack", "()V");
        call(&mut vm, &[], "nextTrack", "()V");
        call(&mut vm, &[], "prevTrack", "()V");
        with_sound(|s| {
            assert_eq!(s.track(), Some(1));
            assert_eq!(
                s.events().last(),
                Some(&SoundEvent::Install {
                    object: MUSIC_SETS[0]
                })
            );
            assert!(s.events().contains(&SoundEvent::Reload {
                object: MUSIC_SETS[0],
                track: 1
            }));
        });
    }

    /// The three volume channels round-trip as float bits; an unknown channel
    /// is ignored on write and answers 0.0 on read; channel 1 notifies the set.
    #[test]
    fn volumes_round_trip_per_channel() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        let half = 0.5f32.to_bits() as i32;
        for channel in 0..3 {
            call(
                &mut vm,
                &[Value::int(channel), Value::int(half)],
                "setVolume",
                "(IF)V",
            );
            assert_eq!(
                call(&mut vm, &[Value::int(channel)], "getVolume", "(I)F"),
                Some(0.5f32.to_bits())
            );
        }
        // Channel 1 with no set: stored but no event.
        with_sound(|s| assert!(s.events().is_empty()));
        call(&mut vm, &[Value::int(0)], "changeMusicSet", "(I)V");
        call(
            &mut vm,
            &[Value::int(1), Value::int(0.25f32.to_bits() as i32)],
            "setVolume",
            "(IF)V",
        );
        with_sound(|s| {
            assert_eq!(
                s.events().last(),
                Some(&SoundEvent::MusicVolume {
                    bits: 0.25f32.to_bits()
                })
            );
        });
        // Unknown channel: write ignored, read answers the zero float.
        call(
            &mut vm,
            &[Value::int(7), Value::int(half)],
            "setVolume",
            "(IF)V",
        );
        assert_eq!(
            call(&mut vm, &[Value::int(7)], "getVolume", "(I)F"),
            Some(0.0f32.to_bits())
        );
        assert_eq!(ZERO_FLOAT, 0x005e_73cc, "the exe's zero float");
    }

    /// The capability probes answer 1 exactly at the threshold 4.
    #[test]
    fn capability_probes_compare_against_four() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(call(&mut vm, &[], "has3DHardware", "()I"), Some(0));
        assert_eq!(call(&mut vm, &[], "hasMixHardware", "()I"), Some(0));
        with_sound(|s| s.set_capabilities(3, 4));
        assert_eq!(call(&mut vm, &[], "has3DHardware", "()I"), Some(0));
        assert_eq!(call(&mut vm, &[], "hasMixHardware", "()I"), Some(1));
        with_sound(|s| s.set_capabilities(4, 3));
        assert_eq!(call(&mut vm, &[], "has3DHardware", "()I"), Some(1));
        assert_eq!(call(&mut vm, &[], "hasMixHardware", "()I"), Some(0));
    }

    /// `enableDebugDump` discards its argument and answers 0, but still emits
    /// the call record with the argument.
    #[test]
    fn enable_debug_dump_records_and_answers_zero() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        let args = [Value::reference(0xAB)];
        assert_eq!(
            call(
                &mut vm,
                &args,
                "enableDebugDump",
                "(Ljava.util.resource.ResourceRef;)I"
            ),
            Some(0)
        );
        with_sound(|s| {
            let records = s.records();
            assert_eq!(records.len(), 1);
            let LogRecord::NativeCall {
                name, args, result, ..
            } = &records[0]
            else {
                panic!("a native call");
            };
            assert_eq!(name, "enableDebugDump");
            assert_eq!(args.len(), 1);
            assert_eq!(result, &Some(LogValue::Int(0)));
        });
    }

    /// Every recorded call carries the marshalled arguments in declaration
    /// order and the typed result.
    #[test]
    fn the_call_log_records_arguments_in_declaration_order() {
        reset_sound();
        let mut vm = VmHost::new();
        install(&mut vm);
        call(
            &mut vm,
            &[Value::int(1), Value::int(0.5f32.to_bits() as i32)],
            "setVolume",
            "(IF)V",
        );
        with_sound(|s| {
            let LogRecord::NativeCall { args, result, .. } = &s.records()[0] else {
                panic!("a native call");
            };
            assert_eq!(
                args,
                &vec![LogValue::Int(1), LogValue::FloatBits(0.5f32.to_bits())]
            );
            assert_eq!(result, &None, "void");
        });
    }

    #[test]
    fn the_binding_rows_are_the_sound_natives() {
        assert_eq!(SOUND_NATIVE_BINDINGS.len(), 10);
        assert_eq!(SOUND_NATIVE_BINDINGS[0].name, "changeMusicSet");
        assert_eq!(SOUND_NATIVE_BINDINGS[0].sig, "(I)V");
        assert_eq!(SOUND_NATIVE_BINDINGS[0].impl_addr, CHANGE_MUSIC_SET);
        assert_eq!(SOUND_NATIVE_BINDINGS[5].name, "setVolume");
        assert_eq!(SOUND_NATIVE_BINDINGS[5].sig, "(IF)V");
        assert_eq!(SOUND_NATIVE_BINDINGS[5].impl_addr, SET_VOLUME);
        assert_eq!(SOUND_NATIVE_BINDINGS[6].name, "getVolume");
        assert_eq!(SOUND_NATIVE_BINDINGS[6].sig, "(I)F");
        assert_eq!(SOUND_NATIVE_BINDINGS[7].name, "has3DHardware");
        assert_eq!(SOUND_NATIVE_BINDINGS[7].impl_addr, HAS_3D_HARDWARE);
        assert_eq!(SOUND_NATIVE_BINDINGS[8].name, "hasMixHardware");
        assert_eq!(SOUND_NATIVE_BINDINGS[8].impl_addr, HAS_MIX_HARDWARE);
        assert_eq!(SOUND_NATIVE_BINDINGS[9].name, "enableDebugDump");
        assert_eq!(SOUND_NATIVE_BINDINGS[9].impl_addr, ENABLE_DEBUG_DUMP);
        let _ = row(CHANGE_MUSIC_SET);
    }
}
