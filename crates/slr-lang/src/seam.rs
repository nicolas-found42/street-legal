//! The `slr-lang` dispatch seam (M1-06 slice 1).
//!
//! The exe reaches a native through its registry record: `FUN_0041fbc0`
//! resolves `(class, name, sig)` to `record+8` and calls that address with
//! one `CallInfo` stack argument. The port keeps that shape in two layers:
//!
//! - [`NativeTable`] maps `impl_addr -> LangNative`, a plain
//!   `fn(&mut LangHost, &[Value]) -> NativeResult`. It is the table the
//!   language natives are written against; it knows the 13 M1-06 addresses
//!   ([`LANG_NATIVE_BINDINGS`]), and every row another module lists in
//!   [`KNOWN_FAMILIES`] (`slr_lang::thread` lists its 13
//!   `java.lang.Thread` / `java.lang.Object` rows there), so an address
//!   that exists in the exe but has no port yet reports
//!   [`DispatchError::NotPorted`] instead of a made-up value.
//! - [`NativeTable`] also carries **VM-host ports** ([`NativeTable::set_vm`]):
//!   natives that take the whole [`slr_vm::host::CallInfo`] rather than the
//!   bare container — either because they need the object model (the
//!   `java.lang.Object` natives, M1-08 rows O1..O4) or because they marshal
//!   their own parameters and need the descriptor text
//!   (`java.lang.System.exit`, row O5). They return a native payload.
//! - [`bridge`] is the single [`slr_vm::host::NativeFn`] installed into
//!   `VmHost::native_impls`. It resolves the record's `record+8` address
//!   through the host's own registry ([`slr_vm::natives::Natives::resolve`]),
//!   takes the already-marshalled [`slr_vm::host::CallInfo::args`] container
//!   and dispatches. The `NativeFn` signature carries no `Result`, so the
//!   bridge only records an error in [`LangHost::last_error`]; [`install`]
//!   therefore installs it **only** for addresses that actually have a port
//!   ([`NativeTable::set`] or [`NativeTable::set_vm`]), and every other
//!   address keeps the VM's `HostError::NoBody`.
//!
//! State shared by the natives lives in [`LangHost`]: the CRT `rand` word
//! (`ptd+0x14`, [`slr_platform::MsvcRand`]), the `clock()` reading in
//! milliseconds (`FUN_005516c0`, sampled through a [`LangHost`] clock seam so
//! replay runs stay deterministic), the simulation's time word
//! (`[[0x617650]]`, the physics world's `+0x00`) and the exit request
//! (`[0x63c520]`). It is process state, not per-VM state, so it lives in a
//! thread-local exactly like the exe's per-thread data block.
//!
//! Two port flavours share the table because the exe has one native table:
//! [`NativeTable::set`] installs a [`LangNative`] (this thread's [`LangHost`]
//! alone) and [`NativeTable::set_vm`] a [`VmNative`], which also takes the
//! [`VmHost`]. The Integer/Float `toString` rows need the world: their
//! characters go to [`intern_string`], the port of the exe's string interner
//! (`FUN_004174a0`, the "seam string path"), and the interned
//! `java.lang.String` object word it returns is the native's `eax`.
//!
//! The console output the natives produce is **not** [`LangHost`] state: it
//! is an unbounded stream, so it lives in its own thread-local as a
//! [`LogSink`] ([`with_log`]). `java.lang.System.log` appends there
//! (`ConsoleAppend`), which is the headless stand-in for the exe's console
//! object at `0x62def8` (`FUN_0054bed0`).
//!
//! The analysed values the three `java.lang.System.analyze` natives marshal
//! are the same kind of stream and get the same treatment: an [`AnalyzeSink`]
//! ([`with_analyze`]) records one [`AnalyzeEvent`] per call. In build 606 the
//! natives' only effect is the write of the marshalled word into their own
//! argument slot, so the sink is the port's record of a value the exe drops
//! (see [`AnalyzeSink`] for the bodies).

use std::cell::RefCell;
use std::collections::HashMap;

use slr_platform::MsvcRand;
use slr_vm::host::{CallInfo, NativeFn, VmHost};
use slr_vm::values::Value;

/// The return payload of a ported native: `eax`, or the `st0` `f32` bits for
/// an `F` return. `None` supplies no payload for a `V` native; the VM dispatcher still pushes `V:0`.
pub type NativeResult = Option<u32>;

/// A ported language native: the target of one `record+8` address.
pub type LangNative = fn(&mut LangHost, &[Value]) -> NativeResult;

/// A ported native that takes the whole [`CallInfo`] instead of the bare
/// container: the target of one `record+8` address that reads [`VmHost`] state
/// (the object heap, the string interner, the loader) or that marshals its own
/// parameters itself and so needs the descriptor text (`info.sig`, e.g.
/// `System.exit(Ljava.lang.String;)V`). Same payload contract as
/// [`LangNative`]; it is a separate type because the world, not the per-thread
/// language state, is what it needs.
pub type VmNative = fn(&mut VmHost, &CallInfo<'_>) -> NativeResult;

/// Intern bytes through the VM's loaded String class and authoritative ptr cell.
/// Missing class/field metadata is an explicit embedding error.
pub fn intern_string(vm: &mut VmHost, bytes: &[u8]) -> u32 {
    vm.intern_string(Some(bytes))
        .expect("loaded String class with Native.ptr required")
}

/// Read String characters through this VM's actual ptr field, including words
/// produced by literals, conversions, and other native producers.
#[must_use]
pub fn string_bytes(vm: &VmHost, word: u32) -> Option<Vec<u8>> {
    vm.string_bytes(word).map(<[u8]>::to_vec)
}

/// Process (per-thread) state the language natives share.
///
/// - `rng`: the CRT `rand` word the exe keeps at `ptd+0x14`
///   (`FUN_005dd8c0` initializes it to 1; `srand`/`rand` at `FUN_005d73fb`/
///   `FUN_005d7408` are [`slr_platform::MsvcRand`]).
/// - `clock`: the last `clock()` sample in milliseconds — what
///   `FUN_005516c0` computes as `(QPC - start) / QPF * 1000.0f` (constant
///   `0x5f0910` = f32 `1000.0`). Sampling goes through `clock_source` so a
///   replay can feed a fixed reading.
/// - `sim_time`: `[[0x617650]]`, the simulation's time word — the same
///   process-global kind of state, read by `java.lang.System.simTime`
///   (`0x47bfd0`) and written by the physics loop (not ported yet).
/// - `exit_flag`: `[0x63c520]`, the exit request
///   [`crate::langobj::system::EXIT`] writes. The exe never terminates from
///   inside the native: it sets the word and returns, and the main loop polls
///   it (`cmp dword ptr [0x63c520], 0` at `0x428a7d` inside
///   `FUN_00428960`; `mov eax, [0x63c520]` at `0x428515`). The port records
///   the same request here and it is read back through
///   [`LangHost::exit_requested`].
/// - `last_error`: the bridge's only error channel (see [`bridge`]); `None`
///   on every successful dispatch.
#[derive(Clone, Copy, Debug)]
pub struct LangHost {
    /// Per-thread CRT `rand` state word.
    pub rng: MsvcRand,
    /// Last `clock()` reading in milliseconds.
    pub clock: f32,
    /// Clock seam: `Some` replaces the wall clock for replay.
    pub clock_source: Option<fn() -> f32>,
    /// The simulation's time word, `[[0x617650]]`: the f32 at `0x6409c4+0`,
    /// the physics world's first field.
    ///
    /// `java.lang.System.simTime` (`0x47bfd0`) reads it through the pointer
    /// cell `0x617650`, which the file image initializes to `0x6409c4` and no
    /// code rewrites (no `mov`/`lea` to the cell anywhere in the image), so
    /// the port keeps the one word the two-level read ends at. The physics
    /// fixed-step loop is its only writer (`docs/physics.md` §6; slice M3),
    /// so nothing advances it yet; `GameType.addTimer` schedules against the
    /// same cell.
    pub sim_time: f32,
    /// `[0x63c520]`: the exit request. `java.lang.System.exit()V` writes 1;
    /// nothing in the exe clears it.
    pub exit_flag: u32,
    /// The JVM heap-bookkeeping readings `java.lang.System.info(int)`
    /// reports (`0x47c0c0`, M1-08 row O9).
    pub info: SystemInfo,
    /// The JVM's GC-context word, `[jvm+0x20]`: `true` while the context
    /// `java.lang.System.gc` allocated on its previous pass is still
    /// allocated.
    ///
    /// `FUN_00417f80` — the collector `gc` tail-calls — reads that word
    /// first (`mov eax, [edi+0x20]; cmp eax, 0; jne 0x41805e` at `0x417f87`)
    /// and stores the freshly allocated context back (`0x417fd7`); the port
    /// keeps the one bit those two sites distinguish, because the context
    /// itself is a heap container (`{+0 capacity, +4 data, +8 count}`,
    /// `0x417fa7`-`0x417fd1`) the port has no heap to fill. `gc` reproduces
    /// both of the exe's passes over this flag — see
    /// [`crate::langobj::system::gc`], which also names the two `eax` values
    /// the probe pinned.
    pub gc_context: bool,
    /// `[0x636448]`: the time-warp scale `java.lang.System.timeWarp` returns
    /// and overwrites when its argument clears the gate at `[0x5e73cc]`.
    pub time_warp: f32,
    /// `[0x636440]`: the sim-clock rate `timeWarp` derives when its argument
    /// clears the gate -- `clock * 0.001 - sim_time / scale`, the seconds of
    /// real time one second of simulation costs at that scale.
    pub time_warp_rate: f32,
    /// `[0x60c8c4]` and `[0x640920]`: the loading-speed scale and flag
    /// `setLdPriority` writes (0.1f/0, or -1.0f/1 for a non-zero argument).
    pub ld_priority: (f32, u32),
    /// `[0x640988]`: seconds-of-day `syncGameTime` writes.
    pub game_time_of_day: i32,
    /// `[0x6178d0]`: the daylight flag `syncGameTime` writes.
    pub daylight: bool,
    /// `setMeasure`'s three words: the input value, `value * k` at
    /// `[0x63c770]` and `k2 / value` at `[0x63c73c]`.
    pub measure: (f32, f32, f32),
    /// `[[0x618d48]+0x106ed4]`, the word `isLoading` returns. The loader
    /// object itself is unported, so this stands in for its field and nothing
    /// writes it yet -- the value the exe reports before a loader exists.
    pub loading: u32,
    /// Last failed bridge dispatch, if any.
    pub last_error: Option<DispatchError>,
}

/// The six readings `java.lang.System.info(int)` returns.
///
/// The exe's native marshals its `I` argument and switches on it
/// (`cmp eax,5; ja` at `0x47c0e2`, jump table `0x47c1b4`); ids 0..=5 read six
/// words of JVM state through the calling VM object `info+0x00` and format
/// each through `sprintf(buf, "%d", …)` — all six format strings at
/// `0x612ce4`..`0x612cf8` are the same `"%d"` — and any other id skips the
/// format entirely and interns whatever the frame's scratch buffer already
/// held (see [`crate::langobj::system::info`]).
///
/// The port has no JVM object, so the readings live here in the seam — the
/// same stand-in the `sim_time`/`exit_flag` words get — and the native keeps
/// the switch, the `%d` conversion and the interning:
///
/// | id | exe source | field |
/// |---|---|---|
/// | 0 | chain at `[[jvm+0x18]+0x08]` | [`Self::list_18`] |
/// | 1 | chain at `[[jvm+0x1c]+0x08]` | [`Self::list_1c`] |
/// | 2 | chain at `[[jvm+0x10]+0x08]` | [`Self::list_10`] |
/// | 3 | `[jvm+0x24]` | [`Self::word_24`] |
/// | 4 | `[jvm+0x28]` | [`Self::word_28`] |
/// | 5 | `[0x62f138]` | [`Self::live_cells`] |
///
/// Nothing else writes this block yet: the JVM object's own bookkeeping is
/// not ported, exactly as the physics loop has still to write `sim_time`. The
/// one writer so far is `java.lang.System.gc`, whose draining pass publishes
/// `[jvm+0x28]` — [`Self::word_28`] — before it frees the mark context; see
/// [`crate::langobj::system::gc`]. The Unicorn probe in
/// `re/oracle/langobj_vectors.py` (suites `info` and `sysgc`) drives the real
/// natives over a hand-built JVM object and pins both the mapping above and
/// that write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemInfo {
    /// info(0): the length the exe counts at `[[jvm+0x18]+0x08]`.
    ///
    /// The count rule is the same for all three chain ids: starting at the
    /// head word, the exe increments once per `+0x04` (next) link that is
    /// non-null and stops at the first null — so the head itself is not
    /// counted and a null head (or a head with no successor) reads 0.
    pub list_18: u32,
    /// info(1): the same count at `[[jvm+0x1c]+0x08]`.
    pub list_1c: u32,
    /// info(2): the same count at `[[jvm+0x10]+0x08]` — the chain
    /// `System.gc`'s walker (`FUN_00417f80`, `0x417fd0`) walks.
    pub list_10: u32,
    /// info(3): `[jvm+0x24]`, taken whole (the exe pushes the field).
    pub word_24: u32,
    /// info(4): `[jvm+0x28]`, taken whole.
    ///
    /// `System.gc` is its only ported writer: the collector's draining pass
    /// stores the size in bytes of the mark context it is about to free
    /// (`mov dword ptr [ebp+0x28], edx` at `0x418b2b`, with `edx` the
    /// container's capacity in words scaled by 4), which is
    /// [`crate::langobj::system::GC_CONTEXT_BYTES`] for the context the
    /// initializing pass allocates. The probe reads the pair end to end:
    /// `info(4)` interns `"0"` before any `gc` and `"256"` after one.
    pub word_28: u32,
    /// info(5): `[0x62f138]`, the live value-cell counter — the word
    /// `crates/slr-vm` already models per pool
    /// (`slr_vm::values::ValuePool::live`, `slr_vm::interp::Thread::live`)
    /// and which no path publishes into `CallInfo` yet.
    pub live_cells: u32,
}

impl SystemInfo {
    /// Zeroed readings: every `info` id reports `0` until the JVM object's
    /// bookkeeping is published here. The port's own live-cell counter starts
    /// at 0 (`crates/slr-vm` `Thread::live`). Only `java.lang.System.gc`
    /// writes this block so far (its draining pass, see the field docs).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl LangHost {
    /// Fresh per-thread state: `rand` word 1, clock reading 0, wall clock.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rng: MsvcRand::new(),
            clock: 0.0,
            clock_source: None,
            sim_time: 0.0,
            exit_flag: 0,
            info: SystemInfo::new(),
            gc_context: false,
            // The exe's image values for the words the new System rows own.
            time_warp: 0.0,
            time_warp_rate: 0.0,
            ld_priority: (0.1, 0),
            game_time_of_day: 0,
            daylight: true,
            measure: (1610.0, 0.0, 0.0),
            loading: 0,
            last_error: None,
        }
    }

    /// The exit request state: `true` once `java.lang.System.exit()V` has
    /// run (`[0x63c520] != 0`).
    ///
    /// The exe's main loop polls the word; a port driver polls this. No
    /// ported path terminates the process, and nothing clears the word.
    #[must_use]
    pub fn exit_requested(&self) -> bool {
        self.exit_flag != 0
    }

    /// State with a fixed clock source (deterministic replay).
    #[must_use]
    pub fn with_clock(clock_source: fn() -> f32) -> Self {
        let mut host = Self::new();
        host.clock_source = Some(clock_source);
        host
    }

    /// Sample the supplied clock reading, store it and return it. This replay
    /// seam supplies the whole reading. The original helper queries persistent
    /// counter words without advancing its start counter; its arithmetic is
    /// [`crate::clock::CounterClock::read_milliseconds`]. A platform driver must
    /// explicitly connect that helper if it wants counter-based samples.
    pub fn read_clock(&mut self) -> f32 {
        if let Some(source) = self.clock_source {
            self.clock = source();
        }
        self.clock
    }
}

impl Default for LangHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a dispatch did not call a port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// No exe native exists at this address (a bad or mistyped `record+8`).
    NoImplementation {
        /// The requested `record+8` address.
        impl_addr: u32,
    },
    /// The address is a registered language native (one of the 13
    /// [`LANG_NATIVE_BINDINGS`] rows, or a row another module lists in
    /// [`KNOWN_FAMILIES`]), but its port has not landed yet.
    NotPorted {
        /// The requested `record+8` address.
        impl_addr: u32,
    },
}

/// One registry row read off `re/out/native_api.tsv` (the exe's own
/// registrar): `(class, name, sig)` binds to `impl_addr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeBinding {
    /// Native class name, dotted as registered (`java.lang.Math`).
    pub class: &'static str,
    /// Method name (`sqrt`).
    pub name: &'static str,
    /// Descriptor text (`(F)F`).
    pub sig: &'static str,
    /// Exe address of the implementation (`record+8`).
    pub impl_addr: u32,
}

/// The 13 M1-06 registrations, in the handoff's table order.
pub const LANG_NATIVE_BINDINGS: &[NativeBinding] = &[
    NativeBinding {
        class: "java.lang.Math",
        name: "randomize",
        sig: "()V",
        impl_addr: 0x0047_c7d0,
    },
    NativeBinding {
        class: "java.lang.Math",
        name: "setrandseed",
        sig: "(I)V",
        impl_addr: 0x0047_c7f0,
    },
    NativeBinding {
        class: "java.lang.Math",
        name: "random",
        sig: "()F",
        impl_addr: 0x0047_c820,
    },
    NativeBinding {
        class: "java.lang.Math",
        name: "sqrt",
        sig: "(F)F",
        impl_addr: 0x0047_c850,
    },
    NativeBinding {
        class: "java.lang.Integer",
        name: "toString",
        sig: "(I)Ljava.lang.String;",
        impl_addr: 0x0048_1e30,
    },
    NativeBinding {
        class: "java.lang.Integer",
        name: "toHexString",
        sig: "(I)Ljava.lang.String;",
        impl_addr: 0x0048_1e80,
    },
    NativeBinding {
        class: "java.lang.Float",
        name: "toString",
        sig: "(FLjava.lang.String;)Ljava.lang.String;",
        impl_addr: 0x0048_1dd0,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "finalize",
        sig: "()V",
        impl_addr: 0x0048_6190,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "token",
        sig: "(ILjava.lang.String;)Ljava.lang.String;",
        impl_addr: 0x0048_1cf0,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "intValue",
        sig: "()I",
        impl_addr: 0x0048_1ed0,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "floatValue",
        sig: "()F",
        impl_addr: 0x0048_1f10,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "append",
        sig: "(I)Ljava.lang.String;",
        impl_addr: 0x0048_1f50,
    },
    NativeBinding {
        class: "java.lang.String",
        name: "length",
        sig: "()I",
        impl_addr: 0x0048_1fe0,
    },
];

/// The `impl_addr -> LangNative` table.
///
/// `known` is the set of exe addresses the registry can hand out for the
/// language classes: the 13 [`LANG_NATIVE_BINDINGS`] addresses plus every row
/// another module lists in [`KNOWN_FAMILIES`] (`slr_lang::thread` lists the 13
/// `java.lang.Thread` / `java.lang.Object` rows there); `impls` holds the ports
/// that have landed. An address in neither set is
/// [`DispatchError::NoImplementation`], an address only known is
/// [`DispatchError::NotPorted`]: the two are distinct so a typo in a native
/// port is never mistaken for unfinished work.
///
/// Two port flavours share the table because the exe has one native table:
/// [`NativeTable::set`] installs a [`LangNative`] (language state only) and
/// [`NativeTable::set_vm`] a [`VmNative`] (object model).
#[derive(Clone, Debug, Default)]
pub struct NativeTable {
    impls: HashMap<u32, LangNative>,
    vm_impls: HashMap<u32, VmNative>,
}

impl NativeTable {
    /// An empty table (every known address is still unported).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the port for one exe address.
    pub fn set(&mut self, impl_addr: u32, native: LangNative) {
        self.impls.insert(impl_addr, native);
    }

    /// Install a port that needs the object model ([`VmHost`]).
    pub fn set_vm(&mut self, impl_addr: u32, native: VmNative) {
        self.vm_impls.insert(impl_addr, native);
    }

    /// The port for `impl_addr`, if it has landed.
    #[must_use]
    pub fn get(&self, impl_addr: u32) -> Option<LangNative> {
        self.impls.get(&impl_addr).copied()
    }

    /// The VM-host port for `impl_addr`, if it has landed.
    #[must_use]
    pub fn get_vm(&self, impl_addr: u32) -> Option<VmNative> {
        self.vm_impls.get(&impl_addr).copied()
    }

    /// Ported addresses of both flavours, sorted (test/debug convenience).
    #[must_use]
    pub fn ported(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .impls
            .keys()
            .chain(self.vm_impls.keys())
            .copied()
            .collect();
        out.sort_unstable();
        out
    }

    /// `true` when `impl_addr` is a registered language native of either
    /// family (`LANG_NATIVE_BINDINGS` plus the rows in [`KNOWN_FAMILIES`]).
    #[must_use]
    pub fn is_known(impl_addr: u32) -> bool {
        LANG_NATIVE_BINDINGS
            .iter()
            .chain(known_rows())
            .any(|b| b.impl_addr == impl_addr)
    }

    /// Call the port at `impl_addr` with the already-marshalled argument
    /// container (`CallInfo::args`, receiver/return slot last).
    pub fn dispatch(
        &self,
        host: &mut LangHost,
        impl_addr: u32,
        args: &[Value],
    ) -> Result<NativeResult, DispatchError> {
        if let Some(native) = self.get(impl_addr) {
            return Ok(native(host, args));
        }
        if Self::is_known(impl_addr) {
            return Err(DispatchError::NotPorted { impl_addr });
        }
        Err(DispatchError::NoImplementation { impl_addr })
    }
}

/// Rows owned by other `slr-lang` slices that this table must recognise as
/// *known but unported* even when their module is not compiled in here: one
/// entry per class family, in the order the families landed (M1-05 for the
/// thread rows, then the M1-08 set). A family appends its own array as its
/// rows land.
pub const KNOWN_FAMILIES: &[&[NativeBinding]] = &[
    crate::langobj::gametype::GAMETYPE_NATIVE_BINDINGS,
    crate::thread::THREAD_NATIVE_BINDINGS,
    crate::langobj::object::OBJECT_NATIVE_BINDINGS,
    crate::langobj::system::SYSTEM_NATIVE_BINDINGS,
];

/// Every row of every [`KNOWN_FAMILIES`] entry, flattened: the `slr-lang`
/// known set this table answers for, `LANG_NATIVE_BINDINGS` aside.
pub fn known_rows() -> impl Iterator<Item = &'static NativeBinding> {
    KNOWN_FAMILIES.iter().flat_map(|family| family.iter())
}

/// Dispatch through a table with the object model in hand: a
/// [`NativeTable::get_vm`] port runs, every other known address is
/// [`DispatchError::NotPorted`].
///
/// This is the entry [`bridge`] uses; it exists separately so tests can
/// drive a table without a VM's registry.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn dispatch_on_vm(
    table: &NativeTable,
    vm: &mut VmHost,
    impl_addr: u32,
    info: &CallInfo<'_>,
) -> Result<NativeResult, DispatchError> {
    if let Some(native) = table.get_vm(impl_addr) {
        return Ok(native(vm, info));
    }
    let known = table.get(impl_addr).is_some() || NativeTable::is_known(impl_addr);
    if known {
        return Err(DispatchError::NotPorted { impl_addr });
    }
    Err(DispatchError::NoImplementation { impl_addr })
}

/// One append to the exe's console object: `mov ecx, 0x62def8; call
/// FUN_0054bed0` (the object word is the thiscall's stack argument).
///
/// [`crate::langobj::system::LOG`] makes every one of these
/// (`0x47be9c`/`0x47bea7`/`0x47beb7`); the headless port records the appends
/// a live console would receive instead of printing them. The *text* of a
/// [`ConsoleAppend::Message`] lives in the word's `java.lang.String`, whose
/// character data the port's object model does not carry yet (it stores the
/// byte length, see `VmHost::make_string`), so the word is recorded and a
/// later object-model slice can render it without changing this record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleAppend {
    /// `push <word>` (`0x47be9c`): the `java.lang.String` message word the
    /// native marshalled out of its argument slot.
    Message(u32),
    /// `push 0x612cd8` (`0x47bea7`): the `"\n"` appended after a message.
    Newline,
    /// `push 0x612cdc` (`0x47beb7`): the literal `"<null>\n"` a null message
    /// becomes (one append, no separate newline).
    NullLine,
}

/// The recorded console appends: the headless stand-in for the exe's console
/// object at `0x62def8`.
///
/// The exe's object is process-wide and guarded by a stream semaphore
/// (`FUN_0054bed0` locks `[0x62def8+0x100]`'s table entry across the write);
/// the port keeps one sink per thread next to the rest of the seam runtime.
/// [`crate::langobj::system::log`] is the only ported writer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogSink {
    appends: Vec<ConsoleAppend>,
}

impl LogSink {
    /// An empty sink (nothing logged yet).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one console append, in call order.
    pub fn append(&mut self, append: ConsoleAppend) {
        self.appends.push(append);
    }

    /// The appends recorded so far, in order.
    #[must_use]
    pub fn appends(&self) -> &[ConsoleAppend] {
        &self.appends
    }

    /// Take the recorded appends (tests replay one native at a time).
    pub fn take(&mut self) -> Vec<ConsoleAppend> {
        core::mem::take(&mut self.appends)
    }

    /// Number of recorded appends.
    #[must_use]
    pub fn len(&self) -> usize {
        self.appends.len()
    }

    /// `true` when nothing has been logged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.appends.is_empty()
    }
}

/// Which `java.lang.System.analyze` overload produced an [`AnalyzeEvent`].
///
/// The exe has one native per type (`re/out/native_api.tsv` 104-106), so the
/// overload — not a runtime tag — is what distinguishes them; each row's
/// descriptor text is the only thing that differs in the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzeKind {
    /// `analyze(Ljava.lang.Object;)V`, `0x0047bed0`.
    Object,
    /// `analyze(I)V`, `0x0047bf10`.
    Int,
    /// `analyze(F)V`, `0x0047bef0`.
    Float,
}

impl AnalyzeKind {
    /// The registry descriptor of this overload
    /// (`re/out/native_api.tsv` 104-106).
    #[must_use]
    pub fn signature(self) -> &'static str {
        match self {
            Self::Object => "(Ljava.lang.Object;)V",
            Self::Int => "(I)V",
            Self::Float => "(F)V",
        }
    }
}

/// One `java.lang.System.analyze` call: the word the exe's marshaller wrote
/// into the native's own argument slot, and which overload was called.
///
/// The native is a no-op in build 606 — see [`AnalyzeSink`] — so this record
/// is the port's headless stand-in for the value the exe drops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalyzeEvent {
    /// The overload the called address belongs to.
    pub kind: AnalyzeKind,
    /// The marshalled parameter word: the value `FUN_0045d910` wrote into
    /// `[ebp+8]` (`analyze(Object)`) or `[esp+4]` (`analyze(I)`/`(F)`), byte
    /// for byte as the argument cell carried it. For the `F` overload this is
    /// the f32 **bit pattern** (the marshaller converts nothing and the
    /// native never touches the FPU), so the record is the raw word, not an
    /// `f32`.
    pub value: u32,
}

/// The recorded `System.analyze` calls: the headless stand-in for a native
/// that, in build 606, does nothing with its argument.
///
/// All three overloads marshal their parameter and drop it:
///
/// ```text
/// 47bed0 (Object)        push ebp; mov ebp, esp
/// 47bed3                 mov ecx, [ebp+8]     ; info
/// 47bed6                 lea eax, [ebp+8]     ; the native's own argument
/// 47bed9                 push eax             ;   slot, which still holds
/// 47beda                 push 0               ;   the incoming info pointer
/// 47bedc                 push ecx             ; FUN_0045d910(info, 0, &slot)
/// 47bedd                 call FUN_0045d910    ;   -> the parameter word
/// 47bee2                 mov eax, [ebp+8]     ;   lands in that slot
/// 47bee8                 test eax, eax
/// 47beea                 je 47beed
/// 47beec                 nop                  ; the empty `if (arg != 0)` body
/// 47beed                 pop ebp; ret         ; eax = the analysed word
///
/// 47bef0 (F) / 47bf10 (I)  the same marshal with no frame and no read-back:
///                          mov ecx,[esp+4]; lea eax,[esp+4]; push eax;
///                          push 0; push ecx; call FUN_0045d910;
///                          add esp,0xc; ret
/// ```
///
/// The `Object` overload's only use of the marshalled word is that `test`:
/// both branches run the same code (the `nop` is the whole taken body), so
/// the native's whole effect is the slot write. The `I`/`F` overloads do not
/// even read the word back. Unicorn probe (this session): the slot word at
/// `0x7feffef0` after each call is the argument cell payload
/// (`0xcafebabe`, `0x3fc00000`, `0x12345678`), `analyze(Object)` returns it
/// in `eax`, `analyze(F)` leaves `st0` empty and writes no other word, and
/// the `Object` overload executes one extra instruction (the `nop`) when the
/// word is non-zero — 160 vs 159.
///
/// So the sink is the port's record of the analysed value: a native whose
/// whole observable effect in the shipped build is "the value went into the
/// argument slot" records it here instead of into a caller frame.
///
/// The `CallInfo.parseParameters` diagnostics a *malformed* container prints
/// are not recorded here — the port has no out-slot overrun to report and
/// stops at its own bound (ledger row O5b owns that divergence).
///
/// The port keeps one sink per thread next to the rest of the seam runtime,
/// like [`LogSink`]: the natives record from wherever the VM calls them
/// (including inside a [`NativeTable::dispatch`] borrow).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalyzeSink {
    events: Vec<AnalyzeEvent>,
}

impl AnalyzeSink {
    /// An empty sink (nothing analysed yet).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one analysed value, in call order.
    pub fn analyze(&mut self, event: AnalyzeEvent) {
        self.events.push(event);
    }

    /// The events recorded so far, in order.
    #[must_use]
    pub fn events(&self) -> &[AnalyzeEvent] {
        &self.events
    }

    /// Take the recorded events (tests replay one native at a time).
    pub fn take(&mut self) -> Vec<AnalyzeEvent> {
        core::mem::take(&mut self.events)
    }

    /// Number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// `true` when nothing has been analysed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// The thread-local language runtime: the shared [`LangHost`] state, the
/// table the bridge dispatches through. String identity and buffers belong
/// to the calling VmHost, shared with literals and numeric conversions.
struct LangRuntime {
    host: LangHost,
    table: NativeTable,
}

thread_local! {
    static LANG: RefCell<LangRuntime> = RefCell::new(LangRuntime {
        host: LangHost::new(),
        table: NativeTable::new(),
    });
    /// The console sink: its own thread-local (not a [`LangRuntime`] field) so
    /// a native may record from anywhere, including inside the
    /// [`NativeTable::dispatch`] borrow.
    static LOG: RefCell<LogSink> = RefCell::new(LogSink::new());
    /// The `System.analyze` sink: the same shape and the same reason as
    /// [`LOG`] — the natives record through it from anywhere.
    static ANALYZE: RefCell<AnalyzeSink> = RefCell::new(AnalyzeSink::new());
}

/// Run `f` over this thread's [`LangHost`] (the exe's per-thread data block).
pub fn with_lang_host<R>(f: impl FnOnce(&mut LangHost) -> R) -> R {
    LANG.with(|rt| f(&mut rt.borrow_mut().host))
}

/// Run `f` over this thread's [`NativeTable`] (install ports here).
pub fn with_native_table<R>(f: impl FnOnce(&mut NativeTable) -> R) -> R {
    LANG.with(|rt| f(&mut rt.borrow_mut().table))
}

/// Run `f` over this thread's [`LogSink`] (the headless console).
///
/// This is the recording side of `java.lang.System.log`: the native appends
/// and a test reads back, instead of text reaching a console or log file.
/// The sink has its own thread-local, so this is callable from any native,
/// including one running inside a [`NativeTable::dispatch`] borrow.
pub fn with_log<R>(f: impl FnOnce(&mut LogSink) -> R) -> R {
    LOG.with(|log| f(&mut log.borrow_mut()))
}

/// Run `f` over this thread's [`AnalyzeSink`] (the recorded `analyze` values).
///
/// This is the recording side of `java.lang.System.analyze`: the three ports
/// push an [`AnalyzeEvent`] and a test reads them back, instead of writing
/// into a native's caller frame (which is the whole of the exe's effect). Its
/// own thread-local lets a native record from inside a
/// [`NativeTable::dispatch`] borrow, exactly like [`with_log`].
pub fn with_analyze<R>(f: impl FnOnce(&mut AnalyzeSink) -> R) -> R {
    ANALYZE.with(|analyze| f(&mut analyze.borrow_mut()))
}

/// Reset this thread's runtime to fresh state: an empty language state, an
/// empty table, an empty console sink and an empty `analyze` sink. Existing
/// VmHost String objects and buffers retain their lifetime.
pub fn reset() {
    LANG.with(|rt| {
        let mut rt = rt.borrow_mut();
        rt.host = LangHost::new();
        rt.table = NativeTable::new();
    });
    LOG.with(|log| *log.borrow_mut() = LogSink::new());
    ANALYZE.with(|analyze| *analyze.borrow_mut() = AnalyzeSink::new());
}

/// Dispatch through this thread's table and host.
pub fn dispatch(impl_addr: u32, args: &[Value]) -> Result<NativeResult, DispatchError> {
    LANG.with(|rt| {
        let mut rt = rt.borrow_mut();
        let LangRuntime { host, table, .. } = &mut *rt;
        table.dispatch(host, impl_addr, args)
    })
}

/// The single [`NativeFn`] installed into `VmHost::native_impls`.
///
/// The VM has already resolved the record and marshalled the container;
/// this recovers `record+8` from the same registry (the `NativeFn` ABI has
/// no address parameter) and dispatches. A VM-host port
/// ([`NativeTable::set_vm`]) runs with the object model; a language port runs
/// against this thread's [`LangHost`].
///
/// A failed dispatch has nowhere to report to — the ABI returns a bare
/// payload — so it is stored in [`LangHost::last_error`] and the return is
/// `None`. [`install`] only attaches this bridge to addresses with a port,
/// so the failure path is unreachable in a correctly installed VM.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let impl_addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    // A VM-host port first: it needs `vm`, which the thread-local dispatch
    // below cannot lend out.
    let vm_port = LANG.with(|rt| rt.borrow().table.get_vm(impl_addr));
    if let Some(native) = vm_port {
        return native(vm, info);
    }
    match dispatch(impl_addr, info.args) {
        Ok(result) => result,
        Err(err) => {
            with_lang_host(|host| host.last_error = Some(err));
            None
        }
    }
}

/// Register every row this module knows: the 13 M1-06 rows plus the
/// M1-08 family rows (`FUN_00416b00`). Idempotent: the exe's registrar
/// overwrites a record.
pub fn register(vm: &mut VmHost) {
    for row in LANG_NATIVE_BINDINGS.iter().chain(known_rows()) {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// Wire the language natives into the VM: register the rows and install
/// [`bridge`] at every ported address of `table`.
///
/// [`bridge`] dispatches through the thread-local table (it has no other way to
/// reach a port), so the ports in `table` are published there as part of the
/// install. A caller that installs a scratch table twice does not need to
/// publish it separately.
///
/// Addresses without a port are deliberately left out of
/// `VmHost::native_impls`: the VM then reports `HostError::NoBody`, which is
/// the truth ("this native is not ported"), instead of a fabricated return.
pub fn install(vm: &mut VmHost, table: &NativeTable) {
    register(vm);
    with_native_table(|installed| {
        for (&addr, &native) in &table.impls {
            installed.set(addr, native);
        }
        for (&addr, &native) in &table.vm_impls {
            installed.set_vm(addr, native);
        }
    });
    for impl_addr in table.ported() {
        vm.native_impls.insert(impl_addr, bridge as NativeFn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(_host: &mut LangHost, args: &[Value]) -> NativeResult {
        Some(
            args.first()
                .map_or(0, |v| v.as_i32() as u32)
                .wrapping_add(1),
        )
    }

    fn fake_void(_host: &mut LangHost, _args: &[Value]) -> NativeResult {
        None
    }

    /// The seam does what a native needs: a fake implementation dispatches
    /// through the table and its payload comes back untouched.
    #[test]
    fn a_fake_native_dispatches_through_the_table() {
        let mut table = NativeTable::new();
        table.set(0xdead_beef, fake);
        let mut host = LangHost::new();
        assert_eq!(
            table.dispatch(&mut host, 0xdead_beef, &[Value::int(41)]),
            Ok(Some(42))
        );
        assert_eq!(table.dispatch(&mut host, 0xdead_beef, &[]), Ok(Some(1)));
        assert_eq!(table.ported(), vec![0xdead_beef]);
    }

    /// `V` natives return `None` (the exe pushes no return cell).
    #[test]
    fn a_fake_void_native_returns_none() {
        let mut table = NativeTable::new();
        table.set(0x0047_c7f0, fake_void);
        let mut host = LangHost::new();
        assert_eq!(table.dispatch(&mut host, 0x0047_c7f0, &[]), Ok(None));
    }

    /// A known-but-unported address and an unknown address are different
    /// failures.
    #[test]
    fn known_and_unknown_addresses_error_differently() {
        let table = NativeTable::new();
        let mut host = LangHost::new();
        assert_eq!(
            table.dispatch(&mut host, 0x0047_c850, &[]),
            Err(DispatchError::NotPorted {
                impl_addr: 0x0047_c850
            })
        );
        assert_eq!(
            table.dispatch(&mut host, 0x0047_c851, &[]),
            Err(DispatchError::NoImplementation {
                impl_addr: 0x0047_c851
            })
        );
    }

    /// The table lists exactly the 13 exe addresses with no duplicates.
    #[test]
    fn the_binding_list_is_the_thirteen_rows() {
        assert_eq!(LANG_NATIVE_BINDINGS.len(), 13);
        for (i, row) in LANG_NATIVE_BINDINGS.iter().enumerate() {
            assert!(NativeTable::is_known(row.impl_addr));
            assert!(
                !LANG_NATIVE_BINDINGS[..i]
                    .iter()
                    .any(|b| b.impl_addr == row.impl_addr),
                "duplicate address {:08x}",
                row.impl_addr
            );
        }
    }

    /// A row of a declared family is `NotPorted` while its port is missing,
    /// `install` still registers it in the registry, and once a port lands the
    /// bridge dispatches it. An address in no family stays `NoImplementation`.
    #[test]
    fn a_family_row_is_known_without_a_port() {
        const ROW: NativeBinding = NativeBinding {
            class: "java.lang.Thread",
            name: "init",
            sig: "(Ljava.lang.String;)V",
            impl_addr: 0x0047_c510,
        };
        let mut table = NativeTable::new();
        let mut host = LangHost::new();
        assert!(NativeTable::is_known(ROW.impl_addr));
        assert!(!NativeTable::is_known(0x0047_c511));
        assert_eq!(
            table.dispatch(&mut host, ROW.impl_addr, &[]),
            Err(DispatchError::NotPorted {
                impl_addr: ROW.impl_addr
            })
        );
        assert_eq!(
            table.dispatch(&mut host, 0x0047_c511, &[]),
            Err(DispatchError::NoImplementation {
                impl_addr: 0x0047_c511
            })
        );

        // `install` registers a known row in the registry even when no port
        // has landed (the VM still answers the record's `record+8`).
        reset();
        let mut vm = crate::test_strings::vm();
        install(&mut vm, &table);
        assert_eq!(
            vm.natives
                .resolve(ROW.class, ROW.name, ROW.sig)
                .expect("registered"),
            ROW.impl_addr
        );
        assert!(vm.native_impls.is_empty(), "no port -> no bridge entry");

        // And a landed port dispatches through the bridge.
        table.set(ROW.impl_addr, fake_void);
        install(&mut vm, &table);
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[],
            class: ROW.class,
            name: ROW.name,
            sig: ROW.sig,
        };
        assert_eq!(bridge(&mut vm, &info), None);
        with_lang_host(|h| assert_eq!(h.last_error, None));
        reset();
    }

    /// The bridge resolves `record+8` from the registry and dispatches to the
    /// port; `install` attaches it only where a port exists.
    #[test]
    fn the_bridge_dispatches_and_install_is_selective() {
        reset();
        let mut vm = crate::test_strings::vm();
        register(&mut vm);
        assert_eq!(
            vm.natives
                .resolve("java.lang.Math", "sqrt", "(F)F")
                .expect("registered"),
            0x0047_c850
        );

        let mut table = NativeTable::new();
        assert_eq!(table.ported(), Vec::<u32>::new());
        install(&mut vm, &table);
        assert!(vm.native_impls.is_empty(), "no port -> no bridge entry");

        table.set(0x0047_c850, fake);
        install(&mut vm, &table);
        assert!(vm.native_impls.contains_key(&0x0047_c850));

        with_native_table(|t| t.set(0x0047_c850, fake));
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &[Value::int(7)],
            class: "java.lang.Math",
            name: "sqrt",
            sig: "(F)F",
        };
        assert_eq!(bridge(&mut vm, &info), Some(8));
        with_lang_host(|h| assert_eq!(h.last_error, None));
        reset();
    }

    /// A `VmNative` port is reachable through `get_vm`/`ported`, runs with the
    /// world, and the seam's string path interns into that same world.
    #[test]
    fn a_vm_port_receives_the_world_and_the_string_path_interns() {
        fn fake_vm(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
            assert_eq!(info.name, "toString");
            Some(intern_string(vm, b"42"))
        }

        reset();
        let mut table = NativeTable::new();
        table.set_vm(0x0048_1e30, fake_vm);
        assert_eq!(table.ported(), vec![0x0048_1e30]);
        assert!(table.get(0x0048_1e30).is_none(), "not a language port");
        assert!(table.get_vm(0x0048_1e30).is_some());

        let mut vm = crate::test_strings::vm();
        install(&mut vm, &table);
        let args = [Value::int(42), Value::int(0)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: "java.lang.Integer",
            name: "toString",
            sig: "(I)Ljava.lang.String;",
        };
        assert_eq!(
            vm.natives
                .resolve(info.class, info.name, info.sig)
                .expect("registered"),
            0x0048_1e30
        );
        let word = bridge(&mut vm, &info).expect("the row returns a String");
        assert_eq!(word, 0x1000, "actual VM object word");
        assert_eq!(vm.strings.lookup(b"42").expect("interned").bytes, b"42");
        assert_eq!(vm.objects.len(), 1);
        assert_eq!(vm.objects[0].class_name(&vm.loader), "java.lang.String");
        assert_eq!(vm.string_bytes(word), Some(&b"42"[..]));
        with_lang_host(|h| assert_eq!(h.last_error, None));
        reset();
    }

    /// The intern path records the bytes against the word it returns, so a
    /// native handed that word as a `String` parameter can read the text back
    /// through its actual VM ptr field. An unknown object word and null have
    /// no bytes.
    #[test]
    fn the_intern_word_resolves_to_its_bytes() {
        reset();
        let mut vm = crate::test_strings::vm();
        let first = intern_string(&mut vm, b"%.3f");
        let second = intern_string(&mut vm, b"7.0");
        assert_eq!(first, 0x1000);
        assert_eq!(second, 0x1001);
        assert_eq!(string_bytes(&vm, first).as_deref(), Some(&b"%.3f"[..]));
        assert_eq!(string_bytes(&vm, second).as_deref(), Some(&b"7.0"[..]));
        assert_eq!(string_bytes(&vm, 0), None);
        assert_eq!(string_bytes(&vm, 0x2000_0002), None);
        reset();
        assert_eq!(
            string_bytes(&vm, first).as_deref(),
            Some(&b"%.3f"[..]),
            "language reset preserves VM-owned strings"
        );
    }

    /// The clock seam feeds `randomize`; without a source the reading stays
    /// at its stored value.
    #[test]
    fn the_clock_seam_is_sampled() {
        fn fixed() -> f32 {
            1234.5
        }
        let mut host = LangHost::with_clock(fixed);
        assert_eq!(host.read_clock(), 1234.5);
        assert_eq!(host.clock, 1234.5);
        let mut plain = LangHost::new();
        plain.clock = 9.0;
        assert_eq!(plain.read_clock(), 9.0);
    }
}
