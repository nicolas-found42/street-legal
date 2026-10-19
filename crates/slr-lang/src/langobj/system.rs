//! `java.lang.System` natives — M1-08 rows O5 (`exit` ×2), O6 (`log`), O7
//! (`buildNumber`, `currentTime`, `simTime`), O8 (the `analyze` trio), O9
//! (`info`), O10 (`gc`, `runFinalization`), and issue #35's five state rows
//! (`timeWarp`, `setLdPriority`, `syncGameTime`, `setMeasure`, `isLoading`).
//!
//! Compiler, package and config wrappers require an explicit
//! [`super::engine::Engine`] backend. Calling without one fails visibly.
//!
//! Three `V` natives, read off the machine code (`0x47be50-0x47bec6`) and
//! re-run under Unicorn (`re/oracle/langobj_vectors.py`, suite `exit`; the
//! `log` body was stepped in the same session):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`EXIT`] `0x0047be50` | `exit()V` | `[0x63c520] = 1`, then `ret` |
//! | [`EXIT_STRING`] `0x0047be60` | `exit(Ljava.lang.String;)V` | marshal, discard, `ret` |
//! | [`LOG`] `0x0047be80` | `log(Ljava.lang.String;)V` | marshal, append message + `"\n"` (null: `"<null>\n"`) |
//!
//! ```text
//! 47be50  mov dword ptr [0x63c520], 1   ; the exit request word
//! 47be5a  ret
//!
//! 47be60  mov ecx, [esp+4]              ; info
//! 47be64  lea eax, [esp+4]              ; the native's own argument slot
//! 47be68  push eax                      ; FUN_0045d910(info, 0, &slot):
//! 47be69  push 0                        ;   receiver out = 0 (the marshaller
//! 47be6b  push ecx                      ;   skips a null out, 0x45d938)
//! 47be6c  call FUN_0045d910             ;   parameter out = &slot
//! 47be74  ret                           ; nothing read back, no flag store
//!
//! 47be80  ... call FUN_0045d910          ; the same marshal; see [`log`]
//! 47be98  test eax, eax                 ; message word
//! 47be9a  je 47beb7                     ;   null -> one append, "<null>\n"
//! 47bea2  call FUN_0054bed0             ; console.append(message)
//! 47beb1  call FUN_0054bed0             ; console.append("\n")
//! ```
//!
//! # The scalar rows (O7)
//!
//! Three natives that ignore the whole `CallInfo` — no container, no marshal:
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`BUILD_NUMBER`] `0x0047c0b0` | `buildNumber()I` | `eax = 0x25e` (build 606) |
//! | [`SIM_TIME`] `0x0047bfd0` | `simTime()F` | `fld [[0x617650]]` (the physics world's `+0x00`) |
//! | [`CURRENT_TIME`] `0x0047bfe0` | `currentTime()F` | `FUN_005516c0() * 0.001f` (ms → s) |
//!
//! ```text
//! 47c0b0  mov eax, 0x25e               ; the build number
//! 47c0b5  ret
//!
//! 47bfd0  mov eax, dword ptr [0x617650] ; the world pointer cell
//! 47bfd5  fld dword ptr [eax]          ; W+0x00: the simulation time
//! 47bfd7  ret
//!
//! 47bfe0  call FUN_005516c0            ; the millisecond clock
//! 47bfe5  fmul dword ptr [0x5f10a4]    ; * 0.001f -> seconds
//! 47bfeb  ret
//! ```
//!
//! `buildNumber` is the word the entry point prints as `SL2 build %d`
//! (`FUN_0058c700`, `docs/physics.md` §1), 606.
//!
//! `currentTime` divides the clock: `FUN_005516c0` is
//! `(QPC - prev) / QPF * 1000.0f` — the very function the seam's clock seam
//! models ([`crate::LangHost::clock`], [`crate::LangHost::read_clock`]) — and
//! the native scales the reading by the f32 `0.001` at `0x5f10a4`. The
//! arithmetic is f32 on both sides: the exe's x87 product of two f32 values
//! is exact in extended precision, so the one rounding the 4-byte `F` return
//! cell performs *is* the f32 product the port computes (no double-rounding
//! difference is possible).
//!
//! `simTime` reads two levels: the pointer cell `0x617650` (initialized by
//! the file image to `0x6409c4`; no `mov`/`lea` to the cell exists anywhere
//! in the image) and the f32 at the world's `+0x00`. The port keeps that one
//! word in [`crate::LangHost::sim_time`], whose doc names both exe words; the
//! physics fixed-step loop is its writer (`docs/physics.md` §6, slice M3).
//!
//! Probe (this session, Unicorn on a hand-built `CallInfo`, `CW_D3D9`):
//! `buildNumber` → `eax = 0x25e` in 2 instructions; `simTime` with `W+0x00 =
//! {2.5, -1.0, 0.02}` → `st0 = 2.5, -1.0, 0.019999999552965164`, 3
//! instructions, no memory write; `currentTime` with `(QPF, prev, QPC)` =
//! `(1000, 0, 250)` → `st0 = 0.25` (250 ms), `(3579545, 1000, 895886)` →
//! `st0 = 0.24999994` (249.99993 ms).
//!
//! # The quirk: the `String` overload exits nothing
//!
//! `exit(Ljava.lang.String;)V` marshals its parameters through
//! `FUN_0045d910` — the receiver word into a **null** out (skipped) and the
//! `String` word into the native's own stack slot — and then returns. It
//! never writes `[0x63c520]`, so a script that calls the message overload
//! keeps running, and the message (and the would-be exit code) is dropped
//! (`exit(int)` is not in the registry at all; `re/out/native_api.tsv` 99-101
//! registers only these three `exit`/`log` rows). This is the handoff's
//! "exit code ignored" quirk, confirmed against the body above rather than
//! inferred; the port reproduces it, so [`exit_string`] leaves the flag alone.
//!
//! # The `analyze` trio (O8)
//!
//! Three more `V` natives that do nothing with their argument, one per type
//! (`re/out/native_api.tsv` 104-106):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`ANALYZE_OBJECT`] `0x0047bed0` | `analyze(Ljava.lang.Object;)V` | marshal into its own slot, `test` it, empty `if` body |
//! | [`ANALYZE_INT`] `0x0047bf10` | `analyze(I)V` | marshal into its own slot, no read-back |
//! | [`ANALYZE_FLOAT`] `0x0047bef0` | `analyze(F)V` | the same body; the `F` payload is the raw bit pattern |
//!
//! All three write the marshalled parameter word into the argument slot the
//! caller passed `info` in (the receiver out is the exe's `push 0`, so only
//! the parameter lands), and that is the whole of their effect: the `Object`
//! overload's `mov eax, [ebp+8]; test eax, eax` has a one-`nop` body (160 vs
//! 159 instructions over the two paths), and the `I`/`F` overloads never read
//! the word. The port routes all three through
//! [`crate::seam::AnalyzeSink`] ([`crate::seam::with_analyze`]), which
//! records the analysed word and the overload — see that type for the bodies
//! and the probe. Nothing reaches the exit word, the clock or the simulation
//! time; the `F` overload never touches the FPU.
//!
//! The `CallInfo.parseParameters` diagnostics the exe prints for a
//! *malformed* container (its marshaller logs them) are not part of this row:
//! the port stops at its own bound and records the zeroed slot. Ledger row
//! O5b owns that divergence.
//!
//! # The `info` readout (O9)
//!
//! `System.info(int)` is the one `System` native that *returns* a `String`:
//! it marshals its `I` parameter, switches on the word, formats one of six
//! JVM statistics through `sprintf(buf, "%d", …)` and interns the buffer
//! through `FUN_004174a0` — the same interner `Object.toString` uses.
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`INFO`] `0x0047c0c0` | `info(I)Ljava.lang.String;` | marshal, switch 0..=5, `%d`, intern |
//!
//! ```text
//! 47c0c0  sub esp, 0x104             ; the scratch buffer (uninitialized)
//! 47c0c6  push esi
//! 47c0c7  mov esi, [esp+0x10c]       ; info
//! 47c0ce  lea eax, [esp+4]           ; the buffer
//! 47c0d2  push eax                   ; out1 = &buffer: the `I` parameter
//! 47c0d3  push 0                     ; out0 = null (the receiver write
//! 47c0d5  push esi                   ;   is skipped, 0x45d938)
//! 47c0d6  call FUN_0045d910          ; FUN_0045d910(info, 0, &buffer)
//! 47c0db  mov eax, [esp+0x10]        ; eax = buf[0] = the marshalled param
//! 47c0df  add esp, 0xc
//! 47c0e2  cmp eax, 5
//! 47c0e5  ja 47c19e                  ; id > 5: NO format, still intern
//! 47c0eb  jmp dword ptr [eax*4 + 0x47c1b4]
//!        ; 0 -> 47c0f2  count the chain at [[jvm+0x18]+8] (+4 links)
//!        ; 1 -> 47c117  the same count at [[jvm+0x1c]+8]
//!        ; 2 -> 47c13c  the same count at [[jvm+0x10]+8]
//!        ; 3 -> 47c161  push [jvm+0x24]
//!        ; 4 -> 47c173  push [jvm+0x28]
//!        ; 5 -> 47c185  push [0x62f138] (the live-cell counter)
//! 47c196  call FUN_00551220          ; sprintf(&buffer+4, "%d", value)
//! 47c19b  add esp, 0xc
//! 47c19e  lea ecx, [esp+8]           ; = the `%d` target, buffer+4
//! 47c1a2  push ecx
//! 47c1a3  mov ecx, [esi]             ; jvm
//! 47c1a5  call FUN_004174a0          ; intern(jvm, buffer+4) -> String
//! 47c1aa  pop esi; add esp, 0x104; ret
//! ```
//!
//! The count loop (all three chain ids; `47c100`/`47c125`/`47c14a`) is
//! `n = 0; for (p = head; p != 0 && p->next != 0; p = p->next) n++` over the
//! `+0x04` link of the node `head = [[jvm+X]+0x08]` — so the head itself is
//! not counted and a null head reads 0. The port keeps the six readings in
//! the seam ([`crate::seam::SystemInfo`]), which documents each source; ids
//! 3/4/5 push their field whole.
//!
//! # The conversion is the exe's own `%d`
//!
//! `0x551220` is the exe's formatter, not the CRT's (`FUN_00553d70`, the
//! `vnprintf` it forwards to), and its `%d` case calls `FUN_00554170`. So the
//! port's conversion reproduces that body: the reading is **signed** (the
//! probe's `[jvm+0x28] = 0xffff_ffff` row interns `"-1"`), and — because the
//! exe negates before the digit loop — `0x8000_0000` overflows back to itself
//! and interns the characters `48 - |rem|` over the negative remainders:
//! `"-./,),(-*,("`. Both shapes are pinned by the oracle's `info` rows and
//! reproduced by [`decimal`]; nothing is "fixed" here.
//!
//! # The `id > 5` path has no format
//!
//! The `ja` branch skips the `sprintf` and interns from `buffer+4` — the word
//! the *formatted* paths target (`lea ecx, [esp+8]` at `0x47c19e` is
//! `sprintf`'s `lea ecx, [esp+0x10]` at `0x47c110` after its three pushes).
//! That word is uninitialized stack: the marshalled parameter went to
//! `buffer+0`, so nothing on this path writes `buffer+4` at all. The exe
//! therefore interns whatever residue that stack slot holds. The probe pins
//! the three shapes: a zeroed stack interns `""` (a fresh emulator run), a
//! `0x41` fill interns `"AAAA…"` up to the first NUL, and a formatted call
//! made first from the same frame interns that call's text again (`info(3)`
//! then `info(9)` → `"12345"`).
//!
//! The port cannot reproduce "whatever the stack held", so — like every
//! other stand-in here — it picks the deterministic reading: it interns the
//! **empty** string for every id outside 0..=5, which is what the exe's own
//! slot holds before any formatted call at that frame depth. Nothing else
//! changes: the native still returns an interned `java.lang.String`, never
//! null.
//!
//! # The GC pair (O10)
//!
//! The two `I` natives that bracket the collector: a tail call and a bare
//! `return 0` (`re/out/native_api.tsv` 102/103).
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`GC`] `0x0047c060` | `gc()I` | tail-call `FUN_00417f80(*(info+0))` |
//! | [`RUN_FINALIZATION`] `0x0047c070` | `runFinalization()I` | `xor eax, eax; ret` |
//!
//! ```text
//! 47c060  mov eax, [esp+4]        ; info
//! 47c064  mov ecx, [eax]          ; ecx = *info = the JVM word
//! 47c066  jmp 0x417f80            ; FUN_00417f80(jvm) — its eax is the return
//!
//! 47c070  xor eax, eax            ; the int 0
//! 47c072  ret
//! ```
//!
//! ## `runFinalization` returns zero and does nothing else
//!
//! Two instructions: it reads no argument (the row's descriptor is `()I` and
//! the body never touches `info`), calls nothing, writes no memory. The
//! probe pins it at 2 instructions, `eax = 0`, empty write list, `st0` empty.
//! The port returns that zero and touches no seam word.
//!
//! ## `gc` is a two-pass state machine over `[jvm+0x20]`
//!
//! `FUN_00417f80` (`0x417f80`-`0x418bff`) is the whole collector — a mark
//! container allocated at `0x417f97` and published at `[jvm+0x20]`, the root
//! walk over `[jvm+8]`'s 512 12-byte records at `0x417ff7`, `FUN_00418110`/
//! `FUN_00418290`/`FUN_00418970`, the sweep's `FUN_00408500` and the
//! finalizer diagnostics written to `0x62e018` — and the word it returns is a
//! **pass discriminator**, not a heap statistic:
//!
//! - `[jvm+0x20] == 0` (no mark context — a fresh JVM): this pass allocates
//!   one (`0x417f97`-`0x417fd7`), publishes the JVM at [`GC_JVM`] and returns
//!   **-1** (`or eax, 0xffffffff` at `0x418058`, [`GC_INITIALIZED`]).
//! - `[jvm+0x20] != 0`: `FUN_00418970` drains and frees that context and
//!   clears the word (`mov dword ptr [ebp+0x20], ebx` at `0x418b5b`), and the
//!   pass returns the number of objects the sweep freed (`mov eax, ebp` at
//!   `0x4180b2`) — 0 for a heap with nothing to collect ([`GC_COLLECTED_NONE`]).
//!
//! The probe (suite `sysgc`, hand-built `CallInfo` over a zeroed JVM object
//! with a 512-record root array) pins the alternation, so the value a script
//! sees is the native's contract rather than a probe artefact:
//!
//! | call | `[jvm+0x20]` before | `eax` | instructions | `[jvm+0x20]` after |
//! |---|---|---|---|---|
//! | 1 | 0 | `0xffffffff` | 4786 | the new context |
//! | 2 | the context | `0` | 74 | 0 |
//! | 3 | 0 | `0xffffffff` | 4786 | the new context |
//! | 4 | the context | `0` | 74 | 0 |
//!
//! The drain pass has one more observable effect, through a native this
//! module already ports: before freeing the container it publishes its byte
//! size at `[jvm+0x28]` (`mov edx, [esi]; shl edx, 2; mov dword ptr
//! [ebp+0x28], edx` at `0x418b2b`), the very word `System.info(4)` reports.
//! The probe reads it across a cycle: `info(4)` interns `"0"` before any
//! `gc` and `"256"` after one, and the initializing pass leaves the word
//! alone.
//!
//! ## What the port reproduces
//!
//! The port has no JVM heap, so it runs no collector and does not pretend to:
//! `FUN_00417f80`'s body is its own unit of work (the object-model rows own
//! it), and a fabricated sweep here would replace a measured value with a
//! guess. What the native *is* — a two-pass machine over one JVM word whose
//! whole `CallInfo`-level effect is its `eax` plus that word — is reproduced
//! exactly:
//!
//! - the context bit lives in [`crate::LangHost::gc_context`] (`false` on a
//!   fresh host, the port's `[jvm+0x20] == 0`);
//! - context clear → set it and return [`GC_INITIALIZED`];
//! - context set → clear it, publish [`GC_CONTEXT_BYTES`] at
//!   `SystemInfo::word_28` (the port's `[jvm+0x28]`, the one word the drain
//!   pass leaves where a later `info(4)` can read it) and return
//!   [`GC_COLLECTED_NONE`].
//!
//! The container's capacity is derived rather than assumed: the init pass
//! allocates a `0x100`-byte buffer and stores the capacity `0x40` words
//! (`0x417fa7`, `0x417fc0`-`0x417fcf`), and only the marking walker's grow
//! (`FUN_00405180`) can raise it — the port marks nothing, so
//! `[jvm+0x28] = 0x40 << 2` ([`GC_CONTEXT_BYTES`]).
//!
//! Two exe effects are deliberately left out, named rather than guessed:
//!
//! - the marked-object container itself — the port keeps the bit, not the
//!   array of marked words the walker fills;
//! - `[jvm+0x34] -= [jvm+0x38]` in the sweep's tail (`0x4180aa`), the JVM's
//!   heap byte counters. No ported reading exposes those fields, and
//!   [`GC_JVM`]'s only readers are `FUN_00417f80`, `FUN_00418290` and
//!   `FUN_00418d10` themselves (every relocation site the exe's `.reloc`
//!   table records for that word), so nothing outside the collector observes
//!   either write.
//!
//! # Seam
//!
//! The exit request is a plain process word in the exe, not per-thread
//! state, and the native does not terminate anything: it sets the word and
//! returns, and the main loop polls it (`cmp dword ptr [0x63c520], 0` at
//! `0x428a7d` inside `FUN_00428960`; `mov eax, [0x63c520]` at `0x428515` in
//! `FUN_00428450`). The port records the same request in
//! [`crate::LangHost::exit_flag`] (read back through
//! [`crate::LangHost::exit_requested`]) and **never** terminates the process —
//! there is no `exit()` call anywhere on this path. [`EXIT_FLAG`] names the
//! exe word the field stands in for.
//!
//! [`info`] reads the six JVM statistics the exe walks out of `info+0x00`;
//! the port has no JVM object, so they are the seam's
//! [`crate::seam::SystemInfo`] block, whose table names each exe chain.
//!
//! [`log`] has no file or console to write to headlessly, so it records the
//! appends the exe's console object would receive in the seam's
//! [`crate::seam::LogSink`] ([`with_log`]); see that native's docs for the
//! message-word rule and the build-606 finding.
//!
//! The `exit`/`log`/`analyze` rows are `VmNative`-shaped (they take the
//! [`CallInfo`]) because they marshal their own parameters and so need the
//! descriptor text; [`exit`] ignores both the VM and the info — the trio
//! needs the info but not the VM. The three O7 scalars need neither — they
//! read seam state only — so they are `LangNative`s (`NativeTable::set`), as
//! are the two O10 natives: [`gc`] moves one seam bit and [`run_finalization`]
//! returns a constant, so neither needs the object model. The exe's only use
//! of `gc`'s `info` is the JVM word it hands to the collector, and the port's
//! collector stand-in is the seam's [`crate::LangHost::gc_context`] — the same
//! rule the six [`crate::seam::SystemInfo`] readings follow.

use slr_vm::host::{marshal_args, CallInfo, VmHost};
use slr_vm::values::Value;

use super::object::intern_string;
use crate::seam::{
    with_analyze, with_lang_host, with_log, AnalyzeEvent, AnalyzeKind, ConsoleAppend, LangHost,
    NativeBinding, NativeResult, NativeTable,
};

/// Exe address of `java.lang.System.exit()V`.
pub const EXIT: u32 = 0x0047_be50;
/// Exe address of `java.lang.System.exit(Ljava.lang.String;)V`.
pub const EXIT_STRING: u32 = 0x0047_be60;
/// Exe address of `java.lang.System.log(Ljava.lang.String;)V`.
pub const LOG: u32 = 0x0047_be80;

/// Exe address of `java.lang.System.analyze(Ljava.lang.Object;)V`.
pub const ANALYZE_OBJECT: u32 = 0x0047_bed0;
/// Exe address of `java.lang.System.analyze(F)V`.
pub const ANALYZE_FLOAT: u32 = 0x0047_bef0;
/// Exe address of `java.lang.System.analyze(I)V`.
pub const ANALYZE_INT: u32 = 0x0047_bf10;

/// Exe address of `java.lang.System.buildNumber()I`.
pub const BUILD_NUMBER: u32 = 0x0047_c0b0;
/// Exe address of `java.lang.System.currentTime()F`.
pub const CURRENT_TIME: u32 = 0x0047_bfe0;
/// Exe address of `java.lang.System.simTime()F`.
pub const SIM_TIME: u32 = 0x0047_bfd0;
/// Exe address of `java.lang.System.info(I)Ljava.lang.String;`.
pub const INFO: u32 = 0x0047_c0c0;

/// Exe address of `java.lang.System.gc()I`.
pub const GC: u32 = 0x0047_c060;
/// Exe address of `java.lang.System.runFinalization()I`.
pub const RUN_FINALIZATION: u32 = 0x0047_c070;

/// `java.lang.System.timeWarp(F)F` — `0x47bf80`. Returns the scale word at
/// `[0x636448]` loaded *before* the branch, and on the taken arm stores the
/// argument there through `FUN_004283f0(0x636338, argument)`.
pub const TIME_WARP: u32 = 0x0047_bf80;

/// `java.lang.System.setLdPriority(I)V` — `0x47bf30`. Two stores, then `ret`:
/// a zero argument writes `0.1f`/`0`, anything else `-1.0f`/`1`.
pub const SET_LD_PRIORITY: u32 = 0x0047_bf30;

/// `java.lang.System.syncGameTime(F)V` — `0x47bff0`. Truncates, takes the
/// remainder modulo 86400, stores it, then writes one of two daylight flags
/// through a five-band ladder.
pub const SYNC_GAME_TIME: u32 = 0x0047_bff0;

/// `java.lang.System.setMeasure(F)V` — `0x47c4b0`. Stores the argument (or the
/// `1000.0f` seed when it never reached the local) and two derived words.
pub const SET_MEASURE: u32 = 0x0047_c4b0;

/// `java.lang.System.isLoading()I` — `0x47c3b0`: `[[0x618d48]+0x106ed4]`, the
/// loader object's word. The loader itself is not ported, so the port reads
/// [`crate::seam::LangHost::loading`], which nothing else writes yet.
pub const IS_LOADING: u32 = 0x0047_c3b0;

/// `java.lang.System.compileAll(Ljava.lang.String;)I` — `0x47c080`. Marshals
/// one `String` and returns `FUN_00418eb0([esi], name)`, the class compiler's
/// own value through the required engine backend.
pub const COMPILE_ALL: u32 = 0x0047_c080;

/// `java.lang.System.arraycopy` native entry.
pub const ARRAYCOPY: u32 = 0x487660;
/// `java.lang.System.openLib` native entry.
pub const OPEN_LIB: u32 = 0x487be0;
/// `java.lang.System.getConfigOptions` native entry.
pub const GET_CONFIG_OPTIONS: u32 = 0x47c500;
/// `java.lang.System.isLoadingReset` native entry.
pub const IS_LOADING_RESET: u32 = 0x47c3a0;

/// `[0x60c8c4]`: the loading-speed scale `setLdPriority` writes. The exe's
/// image value is `0.1f` (`0x3dcccccd`); a non-zero argument writes `-1.0f`.
pub const LD_PRIORITY_SCALE: u32 = 0x0060_c8c4;

/// `[0x640920]`: the flag `setLdPriority` writes beside the scale. It lies past
/// `.data`'s raw bytes, so the loader zero-fills it.
pub const LD_PRIORITY_FLAG: u32 = 0x0064_0920;

/// `[0x640988]`: the time-of-day seconds word `syncGameTime` writes.
pub const GAME_TIME_OF_DAY: u32 = 0x0064_0988;

/// `[0x6178d0]`: the daylight flag `syncGameTime` writes. The image holds `1`.
pub const DAYLIGHT: u32 = 0x0061_78d0;

/// `[0x60e04c]`: the measurement scale `setMeasure` stores verbatim. The image
/// holds `1610.0f` (`0x44c94000`).
pub const MEASURE_WORD: u32 = 0x0060_e04c;

/// `[0x63c770]`: `value * [0x5f0ee8]`, `setMeasure`'s first derived word.
pub const MEASURE_SCALED: u32 = 0x0063_c770;

/// `[0x63c73c]`: `[0x5e7334] / value`, `setMeasure`'s second derived word.
pub const MEASURE_RECIP: u32 = 0x0063_c73c;

/// `[0x5f0ee8]`: the constant `setMeasure` multiplies by, `0x3991a2b4`
/// — `1/3600`, the hours-per-second factor.
pub const MEASURE_K: f32 = 1.0 / 3600.0;

/// `[0x5e7334]`: the constant `setMeasure` divides by the value, `0x41200000`
/// — `10.0`.
pub const MEASURE_K2: f32 = 10.0;

/// `[0x5e73cc]`: the gate `timeWarp` compares against, `0.0`. The compare is
/// `fcomp` + `test ah,1`, i.e. C0 alone, so `<` and unordered (NaN) take the
/// same skip arm.
pub const TIME_WARP_GATE: f32 = 0.0;

/// `[0x636448]`'s loaded value: zero, because the address lies past `.data`'s
/// last initialised byte (`0x607000 + 0x27000 = 0x62e000`) and the loader
/// zero-fills it.
pub const TIME_WARP_INITIAL: f32 = 0.0;

/// `[0x636440]`: the rate `FUN_004283f0` derives beside the scale, as
/// `[0x5516c0] * [0x5f09cc] - [0x617650] / scale`.
pub const TIME_WARP_RATE: u32 = 0x0063_6440;

/// `[0x5f09cc]`: the seconds-per-millisecond factor `FUN_004283f0` applies to
/// the clock reading — `0x3a83126f`, `0.001`.
pub const CLOCK_MS_TO_S: f32 = 0.001;

/// The `eax` the collector's initializing pass returns: `or eax, 0xffffffff`
/// at `0x418058`, on the path that allocated the mark context (and, in the
/// exe, on the path where `FUN_00418970` left `[jvm+0x20]` set).
pub const GC_INITIALIZED: u32 = 0xffff_ffff;

/// The count the draining pass returns when the sweep freed nothing:
/// `mov eax, ebp` at `0x4180b2`, where `ebp` counted the freed objects. The
/// port's stand-in heap holds no unmarked object, so its sweep frees none —
/// the same 0 the exe returns for an empty heap.
pub const GC_COLLECTED_NONE: u32 = 0;

/// The word `runFinalization` returns: `xor eax, eax` at `0x47c070`. It is
/// the whole native — no argument, no call, no write.
pub const RUN_FINALIZATION_ZERO: u32 = 0;

/// `[jvm+0x20]`: the JVM's mark-context word. `FUN_00417f80` reads it first
/// (`0x417f87`), stores the context it allocates (`0x417fd7`) and clears it
/// when it drains that context (`0x418b5b`). The port keeps the one bit those
/// sites distinguish ([`crate::LangHost::gc_context`]).
pub const JVM_GC_CONTEXT: u32 = 0x20;

/// `[jvm+0x28]`: the byte size of the context the drain pass is about to
/// free (`mov edx, [esi]; shl edx, 2; mov dword ptr [ebp+0x28], edx` at
/// `0x418b2b`) — the word `System.info(4)` reports, so a `gc` cycle is
/// visible through that native's output.
pub const JVM_MARK_BYTES: u32 = 0x28;

/// The mark container's capacity in words: the initializing pass allocates a
/// `0x100`-byte data buffer (`push 0x100` at `0x417fa7`) and stores
/// `cap = 0x40` (`0x417fc0`-`0x417fcf`). Only the marking walker's grow
/// (`FUN_00405180`, called at `0x41815b`) can raise it, and the port marks
/// nothing.
pub const GC_CONTEXT_CAPACITY: u32 = 0x40;

/// `[jvm+0x28]` after the draining pass: `cap << 2`, the `0x100` bytes the
/// initializing pass allocated (`shl edx, 2` at `0x418b2d`). The probe's
/// post-`gc` `System.info(4)` row interns `"256"` — this is the number.
pub const GC_CONTEXT_BYTES: u32 = GC_CONTEXT_CAPACITY << 2;

/// `[0x62e004]`: the JVM word the collector's initializing pass publishes
/// for its own walkers (`0x417fe6`). Every relocation site the exe's `.reloc`
/// table records for the word is inside `FUN_00417f80` (`0x417fe8`,
/// `0x41802f`), `FUN_00418290` (`0x4185b9`, `0x418624`, `0x4187a3`) or
/// `FUN_00418d10` (`0x418d9c`, `0x418de3`), so no ported path can observe
/// it and the port keeps no field for it.
pub const GC_JVM: u32 = 0x0062_e004;

/// `"%d"` at `0x612ce4` — the format `info`'s ids 0..=5 all use (ids 1..5 sit
/// at `0x612ce8`/`0x612cec`/`0x612cf0`/`0x612cf4`/`0x612cf8`, the same text).
pub const INFO_FORMAT: u32 = 0x0061_2ce4;

/// The jump table `info`'s switch reads (`jmp dword ptr [eax*4 + 0x47c1b4]`):
/// six entries, ids 0..=5, in `0x47c0f2`, `0x47c117`, `0x47c13c`, `0x47c161`,
/// `0x47c173`, `0x47c185` order.
pub const INFO_TABLE: u32 = 0x0047_c1b4;

/// `[0x62f138]`, the live value-cell counter `info(5)` reports — the same
/// process word `crates/slr-vm` models as `ValuePool::live`/`Thread::live`
/// (`crates/slr-vm/src/values.rs`, `interp.rs`).
pub const LIVE_CELLS: u32 = 0x0062_f138;

/// The build number `buildNumber` returns: `mov eax, 0x25e` at `0x47c0b0`.
/// The entry point prints the same word (`SL2 build %d`, `FUN_0058c700`), so
/// the port's build identity is 606.
pub const BUILD_606: u32 = 0x25e;

/// The f32 `0.001` at `0x5f10a4`: the `fmul` at `0x47bfe5` that turns the
/// seam's millisecond clock into the seconds `currentTime` returns.
pub const MS_TO_SECONDS: f32 = 0.001;

/// The world pointer cell `simTime` loads through (`mov eax, dword ptr
/// [0x617650]` at `0x47bfd0`). The file image holds [`SIM_WORLD`] there and
/// no code rewrites the cell, so the port collapses the two-level read into
/// [`crate::LangHost::sim_time`].
pub const SIM_TIME_PTR: u32 = 0x0061_7650;

/// The physics world object [`SIM_TIME_PTR`] holds (`0x6409c4`); its first
/// f32 field (`+0x00`) is the simulation's time accumulator
/// (`FUN_004a5580` zeroes it, the fixed-step loop advances it —
/// `docs/physics.md` §6).
pub const SIM_WORLD: u32 = 0x0064_09c4;

/// The exe's exit request word, `[0x63c520]` (`mov dword ptr [0x63c520], 1`
/// at `0x47be50`). The port keeps its value in [`crate::LangHost::exit_flag`].
pub const EXIT_FLAG: u32 = 0x0063_c520;

/// The console object `log` appends to: `mov ecx, 0x62def8` at `0x47be9d`
/// and `0x47beac`. The port records the appends in
/// [`crate::seam::with_log`] instead of printing them.
pub const CONSOLE: u32 = 0x0062_def8;

/// `"\n"` at `0x612cd8`: the append `log` makes after a message.
pub const NEWLINE: u32 = 0x0061_2cd8;

/// `"<null>\n"` at `0x612cdc`: the single append a null message becomes.
pub const NULL_LINE: u32 = 0x0061_2cdc;

/// `java.lang.System` in the registry spelling.
pub const SYSTEM: &str = "java.lang.System";

/// The registry rows these ports answer (`re/out/native_api.tsv` 99-103,
/// 104-106, 108/109/113/114, in that file's order).
///
/// The family grows one row per `System` native; `exit` ×2, `log`, the
/// `analyze` trio, the three O7 scalars, `info` and the O10 GC pair have
/// landed.
pub const SYSTEM_NATIVE_BINDINGS: &[NativeBinding] = &[
    NativeBinding {
        class: SYSTEM,
        name: "exit",
        sig: "()V",
        impl_addr: EXIT,
    },
    NativeBinding {
        class: SYSTEM,
        name: "exit",
        sig: "(Ljava.lang.String;)V",
        impl_addr: EXIT_STRING,
    },
    NativeBinding {
        class: SYSTEM,
        name: "log",
        sig: "(Ljava.lang.String;)V",
        impl_addr: LOG,
    },
    NativeBinding {
        class: SYSTEM,
        name: "gc",
        sig: "()I",
        impl_addr: GC,
    },
    NativeBinding {
        class: SYSTEM,
        name: "runFinalization",
        sig: "()I",
        impl_addr: RUN_FINALIZATION,
    },
    NativeBinding {
        class: SYSTEM,
        name: "analyze",
        sig: "(Ljava.lang.Object;)V",
        impl_addr: ANALYZE_OBJECT,
    },
    NativeBinding {
        class: SYSTEM,
        name: "analyze",
        sig: "(I)V",
        impl_addr: ANALYZE_INT,
    },
    NativeBinding {
        class: SYSTEM,
        name: "analyze",
        sig: "(F)V",
        impl_addr: ANALYZE_FLOAT,
    },
    NativeBinding {
        class: SYSTEM,
        name: "arraycopy",
        sig: "(Ljava.lang.Object;ILjava.lang.Object;II)V",
        impl_addr: ARRAYCOPY,
    },
    NativeBinding {
        class: SYSTEM,
        name: "currentTime",
        sig: "()F",
        impl_addr: CURRENT_TIME,
    },
    NativeBinding {
        class: SYSTEM,
        name: "simTime",
        sig: "()F",
        impl_addr: SIM_TIME,
    },
    NativeBinding {
        class: SYSTEM,
        name: "timeWarp",
        sig: "(F)F",
        impl_addr: TIME_WARP,
    },
    NativeBinding {
        class: SYSTEM,
        name: "openLib",
        sig: "(Ljava.lang.String;)I",
        impl_addr: OPEN_LIB,
    },
    NativeBinding {
        class: SYSTEM,
        name: "compileAll",
        sig: "(Ljava.lang.String;)I",
        impl_addr: COMPILE_ALL,
    },
    NativeBinding {
        class: SYSTEM,
        name: "info",
        sig: "(I)Ljava.lang.String;",
        impl_addr: INFO,
    },
    NativeBinding {
        class: SYSTEM,
        name: "buildNumber",
        sig: "()I",
        impl_addr: BUILD_NUMBER,
    },
    NativeBinding {
        class: SYSTEM,
        name: "setLdPriority",
        sig: "(I)V",
        impl_addr: SET_LD_PRIORITY,
    },
    NativeBinding {
        class: SYSTEM,
        name: "syncGameTime",
        sig: "(F)V",
        impl_addr: SYNC_GAME_TIME,
    },
    NativeBinding {
        class: SYSTEM,
        name: "setMeasure",
        sig: "(F)V",
        impl_addr: SET_MEASURE,
    },
    NativeBinding {
        class: SYSTEM,
        name: "getConfigOptions",
        sig: "()V",
        impl_addr: GET_CONFIG_OPTIONS,
    },
    NativeBinding {
        class: SYSTEM,
        name: "isLoadingReset",
        sig: "()V",
        impl_addr: IS_LOADING_RESET,
    },
    NativeBinding {
        class: SYSTEM,
        name: "isLoading",
        sig: "()I",
        impl_addr: IS_LOADING,
    },
];

/// `java.lang.System.exit()V`
///
/// Records the exit request in the seam (`[0x63c520] = 1`). Nothing
/// terminates: the exe sets the word and returns, and its main loop polls it.
///
/// exe: LAB_0047be50
pub fn exit(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    with_lang_host(|host| host.exit_flag = 1);
    None
}

/// `java.lang.System.exit(Ljava.lang.String;)V`
///
/// Marshals the parameters exactly as the exe does (the words land in the
/// port's own `out` array and are dropped) and requests **no** exit: see the
/// module doc — the overload never writes `[0x63c520]`, so a message-carrying
/// `exit` is a no-op in build 606.
///
/// exe: LAB_0047be60
pub fn exit_string(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // The exe is handed one out word per value — the receiver out is null
    // here (so the marshaller drops that word) and the `String` out is its
    // own argument slot — and reads neither back. The port's two-word array
    // is the same shape; a descriptor declaring more parameters stops at its
    // bound (the exe writes on into its caller's frame instead), which is the
    // memory-safe port rule.
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    None
}

/// `java.lang.System.log(Ljava.lang.String;)V`
///
/// Marshals the message and appends it to the console object at [`CONSOLE`],
/// then appends a newline; a null message appends the literal `"<null>\n"`
/// instead (one append, no separate newline). Both appends are recorded in
/// the seam's [`crate::seam::LogSink`] through [`with_log`]; nothing is
/// printed and the native returns `V`.
///
/// ```text
/// 47be80  mov ecx, [esp+4]        ; info
/// 47be84  lea eax, [esp+4]        ; the native's own argument slot
/// 47be88  push eax                ; out[1] = &slot
/// 47be89  push 0                  ; out[0] = null (the marshaller skips it)
/// 47be8b  push ecx                ; FUN_0045d910(info, null, &slot)
/// 47be8c  call FUN_0045d910
/// 47be91  mov eax, [esp+0x10]     ; the slot: the message word
/// 47be95  add esp, 0xc
/// 47be98  test eax, eax
/// 47be9a  je 47beb7               ; null -> "<null>\n"
/// 47be9c  push eax                ; console.append(message)
/// 47be9d  mov ecx, 0x62def8
/// 47bea2  call FUN_0054bed0
/// 47bea7  push 0x612cd8           ; console.append("\n")
/// 47beac  mov ecx, 0x62def8
/// 47beb1  call FUN_0054bed0
/// 47beb6  ret
/// 47beb7  push 0x612cdc           ; console.append("<null>\n")
/// 47bebc  mov ecx, 0x62def8
/// 47bec1  call FUN_0054bed0
/// 47bec6  ret
/// ```
///
/// # The message word, not its text
///
/// The exe hands the console the marshalled *object word* (`push eax`); the
/// console is what renders a `java.lang.String` to bytes. The port's String
/// objects carry only their byte length (`VmHost::make_string`'s documented
/// stand-in), so [`ConsoleAppend::Message`] records the word itself and the
/// rendering waits for the object-model slice — the sink never invents text.
///
/// # Build 606 appends nothing
///
/// `FUN_0054bed0` is, in this build, a stream-semaphore pair and nothing
/// else: `mov eax,[esi+0x100]` then `WaitForSingleObject`/`ReleaseSemaphore`
/// around no write, and its `ret 4` drops the argument (the raw body is 40
/// bytes, `0x54bed0-0x54bef5`). Emulating the whole native under Unicorn
/// with the semaphore table populated confirms it: the only imports hit are
/// those two, and no message byte reaches memory. So the shipped game's
/// `System.log` discards its message; recording the appends is the port's
/// headless *stand-in*, not an observed file/console line.
///
/// # Malformed containers
///
/// A descriptor the container cannot satisfy stops [`marshal_args`] before
/// it writes the message out word; the exe's slot then still holds the
/// `info` pointer it started with, which it appends as if it were a message.
/// The port's out array starts zeroed, so it takes the null path — the same
/// rule the other marshalling natives state ("the port reads 0 there").
///
/// exe: LAB_0047be80
pub fn log(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // The exe reads only the message slot; `out[0]` is the receiver word.
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    let message = out[1];
    with_log(|sink| {
        if message == 0 {
            sink.append(ConsoleAppend::NullLine);
        } else {
            sink.append(ConsoleAppend::Message(message));
            sink.append(ConsoleAppend::Newline);
        }
    });
    None
}

/// `java.lang.System.analyze(Ljava.lang.Object;)V`
///
/// Marshals its one parameter exactly as the exe does and records the word it
/// marshalled in the seam's [`crate::seam::AnalyzeSink`]; the native returns
/// `V`. The exe's body is:
///
/// ```text
/// 47bed0  push ebp; mov ebp, esp
/// 47bed3  mov ecx, [ebp+8]     ; info
/// 47bed6  lea eax, [ebp+8]     ; the native's own argument slot (it holds
/// 47bed9  push eax             ;   the incoming info pointer)
/// 47beda  push 0               ; out0 = null: the receiver write is skipped
/// 47bedc  push ecx             ; FUN_0045d910(info, null, &slot)
/// 47bedd  call FUN_0045d910    ; -> the parameter word lands in [ebp+8]
/// 47bee2  mov eax, [ebp+8]     ; read the marshalled word back
/// 47bee5  add esp, 0xc
/// 47bee8  test eax, eax
/// 47beea  je 47beed            ; the `if (arg != 0)` whose body is one `nop`
/// 47beec  nop
/// 47beed  pop ebp; ret         ; eax = the analysed word
/// ```
///
/// The `test` is the whole of the native's use of the value: both paths run
/// the same code, so the body does not branch (the port therefore writes no
/// empty `if`), and the analysed word is simply the marshalled parameter —
/// the cell payload of the first declared parameter, `args[count-2]`. The
/// exe's `mov eax, [ebp+8]` leaves that word in `eax` as a scratch value (the
/// probe rows record it), but the descriptor's return type is `V`, so the
/// caller pushes no return cell and the port returns `None`, like every other
/// `V` native here.
///
/// # Malformed containers
///
/// A descriptor the container cannot satisfy stops [`marshal_args`] before it
/// writes the parameter word; the exe's slot then still holds the `info`
/// pointer it started with, and `mov eax, [ebp+8]` returns *that* as the
/// analysed word. The port's `out` array starts zeroed, so it records 0 — the
/// same "the port reads 0 there" rule the other marshalling natives state.
///
/// exe: FUN_0047bed0
pub fn analyze_object(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // The exe hands the marshaller a null receiver out and its own argument
    // slot as out1; only out1 is read back (there and here). `out[0]` is the
    // skipped receiver word, `out[1]` the parameter.
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    with_analyze(|sink| {
        sink.analyze(AnalyzeEvent {
            kind: AnalyzeKind::Object,
            value: out[1],
        });
    });
    None
}

/// `java.lang.System.analyze(I)V`
///
/// The int overload: the same marshal (`mov ecx,[esp+4]; lea eax,[esp+4];
/// push eax; push 0; push ecx; call FUN_0045d910; add esp,0xc; ret` at
/// `0x47bf10`) and **no read-back at all** — the exe never touches the
/// marshalled word, so the port records it and returns `V`. The payload is
/// copied verbatim by the marshaller (no sign or width handling), so the
/// recorded word is the argument cell's own word.
///
/// exe: LAB_0047bf10
pub fn analyze_int(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    analyze_payload(info, AnalyzeKind::Int);
    None
}

/// `java.lang.System.analyze(F)V`
///
/// The float overload, byte-for-byte the int overload's body at `0x47bef0`:
/// only the descriptor differs (`(F)V`), and the marshaller copies the `F`
/// payload word without converting it. The exe never pushes the value to the
/// FPU — a probe of this body leaves `st0` empty and writes no word but the
/// argument slot — so the port records the f32 **bit pattern**, byte for
/// byte, and never rounds it through an `f32` (a NaN or denormal argument
/// would not survive that).
///
/// exe: LAB_0047bef0
pub fn analyze_float(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    analyze_payload(info, AnalyzeKind::Float);
    None
}

/// The body the `I` and `F` overloads share: marshal the parameter into the
/// native's own slot (a null receiver out and one parameter out) and record
/// the word. Returns nothing — the exe reads neither `eax` nor the slot back.
fn analyze_payload(info: &CallInfo<'_>, kind: AnalyzeKind) {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    with_analyze(|sink| {
        sink.analyze(AnalyzeEvent {
            kind,
            value: out[1],
        });
    });
}

/// `java.lang.System.buildNumber()I`
///
/// The build constant 606: `mov eax, 0x25e; ret` at `0x47c0b0`. The native
/// touches neither the container nor the info pointer — it reads nothing —
/// so it takes no argument and leaves every seam word alone.
///
/// exe: LAB_0047c0b0
pub fn build_number(_host: &mut LangHost, _args: &[Value]) -> NativeResult {
    Some(BUILD_606)
}

/// `java.lang.System.currentTime()F`
///
/// The seam's clock reading in milliseconds ([`LangHost::read_clock`], the
/// supplied-reading replay seam) scaled by the f32 `0.001` at [`MS_TO_SECONDS`].
/// The original calls `FUN_005516c0` then multiplies by `[0x5f10a4]`; its
/// counter arithmetic is [`crate::clock::CounterClock`]. This native wrapper
/// consumes an already sampled f32 reading, without querying platform counters.
///
/// exe: LAB_0047bfe0
pub fn current_time(host: &mut LangHost, _args: &[Value]) -> NativeResult {
    Some((host.read_clock() * MS_TO_SECONDS).to_bits())
}

/// `java.lang.System.simTime()F`
///
/// The simulation's time word, `fld dword ptr [[0x617650]]` — the f32 at the
/// physics world's `+0x00`, kept in [`LangHost::sim_time`] (the exe's two
/// words and why the port keeps one of them are documented there). Its only
/// writer is the physics fixed-step loop, which is not ported yet, so the
/// native reads the word and nothing more.
///
/// exe: LAB_0047bfd0
pub fn sim_time(host: &mut LangHost, _args: &[Value]) -> NativeResult {
    Some(host.sim_time.to_bits())
}

/// `java.lang.System.info(I)Ljava.lang.String;`
///
/// The six-way JVM statistics readout: marshals its `I` parameter, switches
/// on the word (`0`, `1`, … `5`; anything else takes the no-format path) and
/// returns the interned `String` (`FUN_004174a0`), the same interner
/// `Object.toString` uses. The module doc has the body and the jump table.
///
/// The readings themselves come from the seam's
/// [`crate::seam::SystemInfo`] — the port has no JVM object to walk — and
/// each is formatted by [`decimal`], the port of the exe's own `%d`
/// (`FUN_00554170`): signed, and with the shipped build's `0x8000_0000`
/// quirk (`"-./,),(-*,("`) reproduced rather than corrected.
///
/// # The `id > 5` path
///
/// The exe skips the `sprintf` and interns from a scratch word nothing on
/// this path wrote, so the shipped build returns whatever that stack slot
/// held (probe: `""` on a zeroed stack, `"AAAA…"` over a `0x41` fill, and a
/// preceding formatted call's text from the same frame). The port interns the
/// **empty** string instead — the deterministic reading of that slot, and the
/// one a fresh frame holds. See the module doc.
///
/// # Malformed containers
///
/// The exe reads its parameter out of the scratch buffer the marshaller was
/// handed (`mov eax, [esp+0x10]`); a container the descriptor cannot satisfy
/// leaves that word holding the `info` pointer the native started with, so
/// the exe switches on an address and its unfilled buffer is what gets
/// interned. The port's out array starts zeroed, so it reads `0` there and
/// reports id 0's reading — the same "the port reads 0 there" rule the other
/// marshalling natives state.
///
/// exe: LAB_0047c0c0
pub fn info(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // `FUN_0045d910(info, 0, &buffer)`: a null receiver out (the exe's
    // marshaller skips it) and the parameter out at the bottom of the
    // native's own frame. `out[0]` is that skipped receiver word, `out[1]`
    // the marshalled `I` — the word the exe's switch reads back.
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    let id = out[1];
    let counters = with_lang_host(|host| host.info);
    let text = match id {
        0 => decimal(counters.list_18),
        1 => decimal(counters.list_1c),
        2 => decimal(counters.list_10),
        3 => decimal(counters.word_24),
        4 => decimal(counters.word_28),
        5 => decimal(counters.live_cells),
        // The exe has no `default:` case that formats: it reuses the frame's
        // uninitialized `%d` target. The port's buffer is empty.
        _ => String::new(),
    };
    Some(intern_string(vm, text.as_bytes()))
}

/// `java.lang.System.gc()I`
///
/// The exe is a three-instruction wrapper — `mov eax, [esp+4]; mov ecx,
/// [eax]; jmp FUN_00417f80` — that hands the JVM word (`*info`) to the
/// collector and returns whatever the collector left in `eax`. The module doc
/// has the collector's two passes; the port reproduces exactly the state
/// machine they form over [`crate::LangHost::gc_context`], the seam's stand-in
/// for `[jvm+0x20]`:
///
/// - context clear (the port's fresh JVM): set it and return
///   [`GC_INITIALIZED`] (`-1`, the `or eax, 0xffffffff` pass);
/// - context set: clear it, publish [`GC_CONTEXT_BYTES`] at
///   `SystemInfo::word_28` — the `[jvm+0x28]` write at `0x418b2b` that
///   `System.info(4)` reads back — and return [`GC_COLLECTED_NONE`].
///
/// It runs no marking and frees nothing, and it reads nothing from the
/// argument container (the exe reads only `info`'s first word, and the port's
/// collector stand-in is process-wide like every other seam word). The tail
/// call's target `FUN_00417f80` is the collector itself and is **not** ported
/// here (see the module doc for what the port models and what it leaves out);
/// the annotated symbol is the registered wrapper. The module doc has the
/// probe's four-call table.
///
/// exe: LAB_0047c060
pub fn gc(host: &mut LangHost, _args: &[Value]) -> NativeResult {
    if host.gc_context {
        host.gc_context = false;
        host.info.word_28 = GC_CONTEXT_BYTES;
        Some(GC_COLLECTED_NONE)
    } else {
        host.gc_context = true;
        Some(GC_INITIALIZED)
    }
}

/// `java.lang.System.runFinalization()I`
///
/// `xor eax, eax; ret` at `0x47c070`: the whole native. It does not marshal
/// its (absent) parameters, does not read `info`, calls nothing and writes no
/// memory — the probe pins it at 2 instructions, `eax = 0`, `st0` empty and
/// an empty write list. Build 606 therefore lets a script run finalization
/// nowhere: whatever finalizers the collector's own drain pass runs
/// (`FUN_00418970`, inside `FUN_00417f80`), this native triggers none. The
/// port returns the same zero and touches no seam word.
///
/// exe: LAB_0047c070
pub fn run_finalization(_host: &mut LangHost, _args: &[Value]) -> NativeResult {
    Some(RUN_FINALIZATION_ZERO)
}

/// `sprintf(buf, "%d", word)`: the exe's own decimal conversion, quirks
/// included.
///
/// `info`'s six format strings are all `"%d"` (`0x612ce4`..`0x612cf8`) and the
/// exe's formatter is its own — `FUN_00553d70`, whose `d`/`i` case calls
/// `FUN_00554170` with the word and a zero width — so the port reproduces
/// *that* rule, not the libc one:
///
/// - it is signed (`0xffff_ffff` → `"-1"`);
/// - it negates the value before the digit loop (`iVar4 = -iVar4` at
///   `0x55417e`), so `0x8000_0000` — the one word whose negation overflows
///   back to itself — runs the loop on a negative number and every
///   `v % 10` (C truncation) comes back negative, giving the characters
///   `48 - |rem|`: the shipped build interns `"-./,),(-*,("`.
///
/// The probe pins both (oracle rows `info-word-24-negative` → `"-2"`,
/// `info-word-28-negative` → the quirk text, `info-live-cells-max` →
/// `"2147483647"`), and the port matches the build rather than "fixing" it: a
/// `String` of the exe's bytes is what `info` returns.
///
/// exe: FUN_00554170
///
/// Reached from `info` through the `%d` conversion in `FUN_00553d70`.
fn decimal(word: u32) -> String {
    let mut v = word as i32;
    let negative = v < 0;
    if negative {
        // The exe's `-iVar4`, in 32-bit arithmetic: `INT_MIN` stays
        // `INT_MIN`.
        v = v.wrapping_neg();
    }
    // The exe stores the least-significant digit first (`acStack_20[iVar3]`)
    // and then prints the buffer forwards, so the text is the generation
    // order reversed. Its remainder keeps the dividend's sign (C `%`
    // truncates toward zero, like Rust's), which is why the quirk produces
    // characters below `'0'` rather than panicking.
    let mut digits = [0u8; 12];
    let mut len = 0;
    loop {
        digits[len] = (v % 10 + i32::from(b'0')) as u8;
        len += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut text = String::with_capacity(len + 1);
    if negative {
        text.push('-');
    }
    for digit in digits[..len].iter().rev() {
        text.push(char::from(*digit));
    }
    text
}

/// `java.lang.System.timeWarp(F)F` — `0x47bf80`.
///
/// ```text
/// 47bf81  mov ecx, [esp+8]        ; info
/// 47bf85  lea eax, [esp+8]        ; the native's own argument slot
/// 47bf89  push eax
/// 47bf8a  push 0                  ; receiver out = null
/// 47bf8c  push ecx
/// 47bf8d  call FUN_0045d910       ; marshal the F
/// 47bf92  fld dword ptr [esp+0x14]; the marshalled argument
/// 47bf96  fcomp dword ptr [0x5e73cc]  ; against the warp gate
/// 47bf9c  mov edx, [0x636448]     ; the scale word, loaded unconditionally
/// 47bfa5  mov [esp], edx
/// 47bfa9  fnstsw ax
/// 47bfab  test ah, 1              ; C0: argument < gate?
/// 47bfae  jne 47bfbf              ; yes -> skip the store
/// 47bfb0  push [esp+8]            ; no  -> FUN_004283f0(&this, scale)
/// 47bfba  call FUN_004283f0
/// 47bfbf  fld dword ptr [esp]     ; the scale word
/// 47bfc4  ret
/// ```
///
/// Two things the disassembly pins and a naive port misses:
///
/// - the comparison is `fcomp` + `test ah,1`, so the branch reads **C0**
///   alone. That is `argument < gate` for a normal compare *and* true when
///   either side is unordered, so a NaN argument takes the skip path, the
///   same as `-inf`.
/// - the return value is the scale word `[0x636448]` loaded *before* the
///   branch, so both paths return it — including the path that just stored a
///   new one. The port keeps the word in [`LangHost::time_warp`] and returns
///   it as `F`.
///
/// exe: FUN_0047bf80
pub fn time_warp(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    let argument = f32::from_bits(out[1]);
    let clock = with_lang_host(|host| host.read_clock());
    with_lang_host(|host| {
        // The word is loaded *before* the branch, so both paths return the
        // pre-call scale -- including the path that just stored a new one.
        let returned = host.time_warp;
        // `fcomp` sets C0 for `<` or unordered, so a NaN takes the skip path.
        if argument >= TIME_WARP_GATE {
            // `FUN_004283f0(this = 0x636338, scale)` stores the scale at
            // `this+0x110` (`0x636448`) and derives the rate at `this+0x108`
            // (`0x636440`): `clock * [0x5f09cc]` seconds, minus
            // `[0x617650] / scale` (the accumulated sim time over the scale)
            // when the scale is strictly above the gate.
            host.time_warp = argument;
            let mut rate = f64::from(clock) * 0.001_f64;
            if argument > TIME_WARP_GATE {
                rate -= f64::from(host.sim_time) / f64::from(argument);
            }
            host.time_warp_rate = rate as f32;
        }
        returned.to_bits()
    })
    .into()
}

/// `java.lang.System.setLdPriority(I)V` — `0x47bf30`.
///
/// ```text
/// ...     call FUN_0045d910       ; marshal the I
/// 47bf41  mov eax, [esp+0x10]
/// 47bf48  test eax, eax
/// 47bf4a  jne 47bf61
/// 47bf4c  mov dword ptr [0x60c8c4], 0x3dcccccd  ; 0.1f
/// 47bf56  mov dword ptr [0x640920], 0
/// 47bf60  ret
/// 47bf61  mov dword ptr [0x60c8c4], 0xbf800000  ; -1.0f
/// 47bf6b  mov dword ptr [0x640920], 1
/// 47bf75  ret
/// ```
///
/// Both stores are constants, so the argument only selects the pair; the
/// port keeps the two words in [`LangHost::ld_priority`].
///
/// exe: LAB_0047bf30
pub fn set_ld_priority(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    let word = out[1];
    with_lang_host(|host| {
        host.ld_priority = if word == 0 { (0.1, 0) } else { (-1.0, 1) };
    });
    None
}

/// `java.lang.System.syncGameTime(F)V` — `0x47bff0`.
///
/// ```text
/// ...     call FUN_0045d910       ; marshal the F
/// 47c001  fld dword ptr [esp+0x10]
/// 47c008  call 0x5d6750           ; __ftol: truncate to an int
/// 47c00d  cdq
/// 47c00e  mov ecx, 0x15180        ; 86400 seconds in a day
/// 47c013  idiv ecx
/// 47c015  cmp edx, 0x11940        ; 72000 (20:00)
/// 47c01b  mov [0x640988], edx     ; seconds-of-day
/// 47c021  jge 47c046              ; >= 20:00 -> night
/// 47c023  cmp edx, 0xe10          ; 3600 (01:00)
/// 47c029  jl 47c046               ; < 01:00  -> night
/// 47c02b  cmp edx, 0x4650         ; 18000 (05:00)
/// 47c031  jl 47c03b               ; < 05:00 -> day
/// 47c033  cmp edx, 0x6270         ; 25200 (07:00)
/// 47c039  jl 47c046               ; < 07:00 -> night
/// 47c03b  mov [0x6178d0], 0       ; day
/// 47c046  mov [0x6178d0], 1       ; night
/// ```
///
/// The `idiv` is **signed**, so the remainder keeps the dividend's sign: a
/// negative argument yields a negative seconds word, which fails both `jge
/// 72000` and the `jl 3600` test's complement and lands on night. The port
/// reproduces that rather than taking `rem_euclid`.
///
/// exe: LAB_0047bff0
pub fn sync_game_time(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    // `__ftol` truncates toward zero, then `idiv` by 86400 keeps the
    // dividend's sign in `edx`, so a negative argument yields a negative
    // seconds word -- which the ladder treats as night except in the
    // `-1`-like band, exactly as the exe does.
    let whole = f32::from_bits(out[1]).trunc() as i64;
    let seconds = (whole % 86_400) as i32;
    // The exe's ladder (`0x47c015`-`0x47c050`), read straight off its jumps:
    //   edx >= 0x11940 (72000)      -> 1   (`jge`)
    //   edx <  0x0e10  (3600)       -> 1   (`jl`)
    //   edx <  0x4650  (18000)      -> 0   (`jl`)
    //   edx <  0x6270  (25200)      -> 1   (`jl`)
    //   otherwise                   -> 0   (fallthrough)
    // The ladder, straight off the jumps (`0x47c015`-`0x47c050`), on the
    // signed remainder:
    //   s >= 72000 -> 1   (jge 0x47c046)
    //   s <  3600  -> 1   (jl  0x47c046)
    //   s < 18000  -> 0   (jl  0x47c03b)
    //   s < 25200  -> 1   (jl  0x47c046)
    //   else       -> 0   (fallthrough to 0x47c03b)
    // The oracle rows bracket every boundary: 3599/3600, 17999/18000,
    // 25199/25200 and 71999/72000.
    let daylight = !(3_600..72_000).contains(&seconds) || (18_000..25_200).contains(&seconds);
    with_lang_host(|host| {
        host.game_time_of_day = seconds;
        host.daylight = daylight;
    });
    None
}

/// `java.lang.System.setMeasure(F)V` — `0x47c4b0`.
///
/// ```text
/// 47c4b1  mov ecx, [esp+8]
/// 47c4b5  lea eax, [esp]          ; the local, pre-seeded:
/// 47c4bd  mov dword ptr [esp+0xc], 0x447a0000  ; local = 1000.0f
/// ...     call FUN_0045d910
/// 47c4ca  fld dword ptr [esp+0xc]
/// 47c4ce  fmul dword ptr [0x5f0ee8]
/// 47c4d4  mov [0x60e04c], edx     ; the raw word, pre-multiply
/// 47c4de  fstp dword ptr [0x63c770]; value * k
/// 47c4e4  fld dword ptr [0x5e7334]
/// 47c4ea  fdiv dword ptr [esp+0xc]
/// 47c4ee  fstp dword ptr [0x63c73c]; k2 / value
/// ```
///
/// The `1000.0f` seed means a marshal that writes nothing (or writes zero)
/// still leaves the divisor at 1000, so there is no division by zero on that
/// path — but a **zero argument** the marshaller does write *does* divide by
/// zero, and `fdiv` yields an infinity rather than trapping. The port keeps
/// the three words in [`LangHost::measure`].
///
/// exe: FUN_0047c4b0
pub fn set_measure(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    // The `1000.0f` at `0x47c4bd` writes the local at `[esp+0xc]`, which is
    // *not* the slot the marshaller fills (`lea eax,[esp]`), so it only shows
    // through on a container that never reaches that word. A zero argument
    // therefore stores zero, and `fdiv` then yields an infinity -- the oracle
    // row pins both.
    let value = f32::from_bits(out[1]);
    with_lang_host(|host| {
        host.measure = (value, value * MEASURE_K, MEASURE_K2 / value);
    });
    None
}

/// `java.lang.System.isLoading()I` — `0x47c3b0`.
///
/// ```text
/// 47c3b0  mov eax, [0x618d48]     ; the loader singleton
/// 47c3b5  mov eax, [eax+0x106ed4] ; its loading word
/// 47c3bb  ret
/// ```
///
/// No marshal, no container: the native reads one word out of the loader and
/// returns it. The port keeps it in [`LangHost::loading`].
///
/// exe: LAB_0047c3b0
pub fn is_loading(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    Some(with_lang_host(|host| host.loading))
}

/// Install the `java.lang.System` ports into a dispatch table.
pub fn install(table: &mut NativeTable) {
    table.set_vm(ARRAYCOPY, arraycopy);
    table.set_vm(COMPILE_ALL, compile_all);
    table.set_vm(GET_CONFIG_OPTIONS, get_config_options);
    table.set_vm(IS_LOADING_RESET, is_loading_reset);
    table.set_vm(OPEN_LIB, open_lib);
    table.set_vm(EXIT, exit);
    table.set_vm(EXIT_STRING, exit_string);
    table.set_vm(LOG, log);
    // The three `analyze` overloads marshal their own parameter, so they take
    // the object-model flavour too (they need the descriptor text).
    table.set_vm(ANALYZE_OBJECT, analyze_object);
    table.set_vm(ANALYZE_INT, analyze_int);
    table.set_vm(ANALYZE_FLOAT, analyze_float);
    // The three scalars take the language-state flavour: they marshal no
    // argument and need no object model, only the seam's words.
    table.set(BUILD_NUMBER, build_number);
    table.set(CURRENT_TIME, current_time);
    table.set(SIM_TIME, sim_time);
    // The O10 GC pair reads seam state only, like the three scalars above:
    // `gc` toggles the collector stand-in's context bit (and the mark-size
    // word `info(4)` reports), `runFinalization` is the constant zero.
    table.set(GC, gc);
    table.set(RUN_FINALIZATION, run_finalization);
    // `info` returns an interned `String`, so it takes the object-model
    // flavour (the interner lives on `VmHost`) *and* marshals its own
    // parameter.
    table.set_vm(INFO, info);
    // The remaining System rows (issue #35's non-timer half). Each marshals
    // its own parameter and reads only the seam's words.
    table.set_vm(TIME_WARP, time_warp);
    table.set_vm(SET_LD_PRIORITY, set_ld_priority);
    table.set_vm(SYNC_GAME_TIME, sync_game_time);
    table.set_vm(SET_MEASURE, set_measure);
    table.set_vm(IS_LOADING, is_loading);
}

/// Forward the compiler's result without inventing a successful compilation.
/// exe: FUN_0047c080
pub fn compile_all(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut out = [0; 2];
    marshal_args(info, &mut out).expect("compileAll arguments");
    Some(super::engine::with_engine(|e| {
        super::engine::invoke(e, 0x418eb0, info.jvm, &[out[1]])
    }))
}

/// Read options through the engine's config-tree implementation.
/// exe: LAB_0047c500
pub fn get_config_options(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    super::engine::with_engine(|e| super::engine::invoke(e, 0x427bf0, 0x636338, &[]));
    None
}

/// Reset the 32 loading samples and their wrapping sum.
/// exe: LAB_0047c3a0
pub fn is_loading_reset(_vm: &mut VmHost, _info: &CallInfo<'_>) -> Option<u32> {
    super::engine::with_engine(|e| {
        let seed = e.read(0x618d6c);
        for i in 0..32 {
            e.write(0x765e8c + i * 4, seed);
        }
        e.write(0x765f24, seed.wrapping_shl(5));
    });
    None
}

/// Preserve the package manager's fast path, load fallback and temporary unlink.
/// exe: LAB_00487be0
pub fn open_lib(_vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    use super::engine::{invoke, unlink, with_engine};
    let mut out = [0; 2];
    marshal_args(info, &mut out).expect("openLib arguments");
    Some(with_engine(|e| {
        let manager = e.read(0x618d48);
        let id = invoke(e, 0x538380, manager, &[out[1]]);
        let resource = id.wrapping_shl(16);
        let handle = invoke(e, 0x5383f0, manager, &[resource]);
        if handle != 0 {
            let handle = invoke(e, 0x5383f0, manager, &[resource]);
            if invoke(e, 0x544000, handle, &[]) >= 2 {
                return id;
            }
        }
        let link = e.scratch(4);
        invoke(e, 0x545fc0, link, &[resource | 1]);
        if e.read(link + 12) == 0 {
            return u32::MAX;
        }
        unlink(e, link);
        id
    }))
}

/// Copy only payload words, retaining destination cell types. Self copies use
/// a temporary payload buffer so forward and backward overlap match the exe.
/// Invalid indices that fault in the executable fail explicitly in the port.
/// exe: LAB_00487660
pub fn arraycopy(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut w = [0; 6];
    marshal_args(info, &mut w).expect("arraycopy arguments");
    let src = w[1].checked_sub(0x3000_0000).expect("array source") as usize;
    let dst = w[3].checked_sub(0x3000_0000).expect("array destination") as usize;
    let src_len = vm.objects[src].fields.len() as i32;
    let dst_len = vm.objects[dst].fields.len() as i32;
    let src_pos = w[2] as i32;
    let dst_pos = w[4] as i32;
    let count = w[5] as i32;
    if src_pos.wrapping_add(count) > src_len || dst_pos.wrapping_add(count) > dst_len {
        super::engine::with_engine(|e| {
            super::engine::invoke(e, 0x551140, 0, &[0x62e018, 0x613a60, 0x100]);
            super::engine::invoke(e, 0x551140, 0, &[0x62e018, 0x613a64, 0x100]);
            super::engine::invoke(e, 0x5513b0, 0, &[0x62e018]);
            let word = e.read(0x62e018) & !0xff;
            e.write(0x62e018, word);
        });
        return None;
    }
    // The executable checks upper bounds before entering its copy loop. A
    // zero count never dereferences either index, including negative ones.
    if count == 0 {
        return None;
    }
    assert!(
        src_pos >= 0 && dst_pos >= 0 && count >= 0,
        "arraycopy index would fault in executable"
    );
    if src == dst {
        let words: Vec<_> = vm.objects[src].fields[src_pos as usize..(src_pos + count) as usize]
            .iter()
            .map(|v| v.payload)
            .collect();
        for (i, word) in words.into_iter().enumerate() {
            vm.objects[dst].fields[dst_pos as usize + i].payload = word;
        }
    } else {
        for i in 0..count as usize {
            vm.objects[dst].fields[dst_pos as usize + i].payload =
                vm.objects[src].fields[src_pos as usize + i].payload;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::{self, bridge, with_lang_host, DispatchError, NativeTable, SystemInfo};
    use slr_vm::host::CallInfo;
    use slr_vm::values::Value;

    /// The bridge's own info for an `exit` row (the helper the `exit` rows
    /// use; `info` is the native this module ports).
    fn exit_info<'a>(args: &'a [Value], sig: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: SYSTEM,
            name: "exit",
            sig,
        }
    }

    fn flag() -> u32 {
        with_lang_host(|host| host.exit_flag)
    }

    /// A clock source that must not be reached: a native that reads no state
    /// would panic here if it sampled the seam.
    fn unreachable_clock() -> f32 {
        panic!("this native must not sample the clock seam")
    }

    /// The bridge's own info for a `()I`/`()F` row.
    fn scalar_info<'a>(name: &'a str, sig: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args: &[],
            class: SYSTEM,
            name,
            sig,
        }
    }

    /// `buildNumber` is the build constant (`mov eax, 0x25e; ret`), whatever
    /// the seam holds, and it reads no state at all.
    #[test]
    fn build_number_is_the_build_constant() {
        seam::reset();
        let mut host = crate::seam::LangHost::with_clock(unreachable_clock);
        assert_eq!(build_number(&mut host, &[]), Some(BUILD_606));
        assert_eq!(build_number(&mut host, &[]), Some(0x25e));
        assert_eq!(build_number(&mut host, &[]), Some(606));
        // No seam word moved.
        assert_eq!(host.clock, 0.0);
        assert_eq!(host.sim_time, 0.0);
        assert_eq!(host.exit_flag, 0);
        assert_eq!(host.last_error, None);
        // It also leaves the thread-local runtime alone.
        assert!(!with_lang_host(|h| h.exit_requested()));
        seam::reset();
    }

    /// `currentTime` is the seam's millisecond clock times the f32 `0.001`:
    /// 1500 ms is 1.5000001 s in f32 arithmetic (the f64 product would be
    /// exactly 1.5), which is the product the exe's 4-byte `F` cell keeps
    /// after its extended-precision multiply.
    #[test]
    fn current_time_scales_the_seam_clock_to_seconds() {
        fn ms_1500() -> f32 {
            1500.0
        }
        let mut host = crate::seam::LangHost::with_clock(ms_1500);
        let out = current_time(&mut host, &[]).expect("an F native returns bits");
        assert_eq!(out, (1500.0f32 * MS_TO_SECONDS).to_bits());
        assert_eq!(out, 0x3fc0_0001, "f32 1.5000001, not the f64 1.5");
        assert_ne!(out, 1.5f32.to_bits());
        // The sample is stored in the seam, exactly as the exe's clock
        // function stores its own reading.
        assert_eq!(host.clock, 1500.0);
        assert_eq!(current_time(&mut host, &[]), Some(out));
        assert_eq!(host.exit_flag, 0, "currentTime is not the exit overload");
        // Without a source the stored reading is what it scales.
        let mut plain = crate::seam::LangHost::new();
        plain.clock = 16.0;
        assert_eq!(current_time(&mut plain, &[]), Some(0.016f32.to_bits()));
        assert_eq!(
            current_time(&mut plain, &[]),
            Some(0x3c83_126f),
            "0.016f * 0.001f"
        );
    }

    /// `simTime` returns the seam's simulation word, and it never samples the
    /// clock seam (its source would panic).
    #[test]
    fn sim_time_reads_the_seam_word_and_samples_no_clock() {
        let mut host = crate::seam::LangHost::with_clock(unreachable_clock);
        assert_eq!(sim_time(&mut host, &[]), Some(0.0f32.to_bits()));
        for v in [0.02f32, 2.5, -1.0] {
            host.sim_time = v;
            assert_eq!(sim_time(&mut host, &[]), Some(v.to_bits()));
        }
        assert_eq!(host.clock, 0.0, "simTime does not touch the clock");
        // Read-only: the native leaves the word as it found it.
        assert_eq!(sim_time(&mut host, &[]), Some((-1.0f32).to_bits()));
        assert_eq!(host.sim_time, -1.0);
        assert_eq!(host.exit_flag, 0);
        // The two exe words the seam word stands in for are the ones the
        // native loads through.
        assert_eq!(SIM_TIME_PTR, 0x0061_7650);
        assert_eq!(SIM_WORLD, 0x0064_09c4);
    }

    /// `gc` is the collector's two-pass state machine: the pass that finds no
    /// context allocates one and returns `-1`, the pass that finds one drains
    /// it and returns the collected count (0 for the port's empty stand-in
    /// heap). The exe alternates exactly the same way — the oracle's `sysgc`
    /// suite pins four calls: `eax` = `ffffffff, 0, ffffffff, 0` with
    /// `[jvm+0x20]` clear, set, clear, set.
    #[test]
    fn gc_alternates_between_the_two_collector_passes() {
        let mut host = crate::seam::LangHost::new();
        assert!(!host.gc_context, "a fresh host holds no mark context");
        assert_eq!(host.info.word_28, 0, "no context has published a size");
        // `[jvm+0x28]` as the exe leaves it: 0 until a draining pass stores the
        // freed context's size, and the initializing passes never touch it
        // (oracle row `gc-init-keeps-mark-size`).
        let mut mark_bytes = 0u32;
        for call in 0..4 {
            if call % 2 == 0 {
                assert_eq!(gc(&mut host, &[]), Some(GC_INITIALIZED), "call {call}");
                assert!(host.gc_context, "call {call} stores the context");
                assert_eq!(
                    host.info.word_28, mark_bytes,
                    "call {call} leaves [jvm+0x28]"
                );
            } else {
                assert_eq!(gc(&mut host, &[]), Some(GC_COLLECTED_NONE), "call {call}");
                assert!(!host.gc_context, "call {call} clears the context word");
                mark_bytes = GC_CONTEXT_BYTES;
                assert_eq!(
                    host.info.word_28, mark_bytes,
                    "call {call} publishes the freed context's size"
                );
            }
        }
        // The drain pass's `[jvm+0x28]` is the word `System.info(4)` reads
        // back, so the pair is observable through that already-ported native.
        assert_eq!(GC_CONTEXT_BYTES, 0x100);
        assert_eq!(decimal(host.info.word_28), "256");
    }

    /// `gc` moves the two words the collector's passes own and leaves every
    /// other seam word alone — it samples no clock, marshals nothing and
    /// records no error.
    #[test]
    fn gc_touches_only_the_context_and_the_mark_size() {
        let mut host = crate::seam::LangHost::with_clock(unreachable_clock);
        host.clock = 12.5;
        host.sim_time = 2.5;
        host.exit_flag = 1;
        host.info = SystemInfo {
            list_18: 1,
            list_1c: 2,
            list_10: 3,
            word_24: 4,
            word_28: 5,
            live_cells: 6,
        };
        assert_eq!(gc(&mut host, &[]), Some(GC_INITIALIZED));
        assert_eq!(gc(&mut host, &[]), Some(GC_COLLECTED_NONE));
        assert_eq!(host.clock, 12.5, "no clock sample");
        assert_eq!(host.sim_time, 2.5);
        assert_eq!(host.exit_flag, 1);
        assert_eq!(host.info.list_18, 1);
        assert_eq!(host.info.list_1c, 2);
        assert_eq!(host.info.list_10, 3);
        assert_eq!(host.info.word_24, 4);
        assert_eq!(host.info.live_cells, 6);
        assert_eq!(
            host.info.word_28, GC_CONTEXT_BYTES,
            "only info(4) is written"
        );
        assert_eq!(host.last_error, None);
    }

    /// `runFinalization` is `xor eax, eax; ret`: the constant zero, with no
    /// argument, no call and no write (the probe pins 2 instructions, an
    /// empty write list and `eax = 0`).
    #[test]
    fn run_finalization_returns_zero_and_touches_no_state() {
        let mut host = crate::seam::LangHost::with_clock(unreachable_clock);
        host.clock = 12.5;
        host.sim_time = -1.0;
        host.exit_flag = 1;
        host.gc_context = true;
        host.info = SystemInfo {
            list_18: 2,
            list_1c: 0,
            list_10: 3,
            word_24: 12_345,
            word_28: 0xffff_ffff,
            live_cells: 42,
        };
        let before = (host.gc_context, host.info, host.exit_flag, host.sim_time);
        assert_eq!(
            run_finalization(&mut host, &[]),
            Some(RUN_FINALIZATION_ZERO)
        );
        assert_eq!(
            (host.gc_context, host.info, host.exit_flag, host.sim_time),
            before,
            "the native reads and writes nothing"
        );
        assert_eq!(host.clock, 12.5, "no clock sample");
        assert_eq!(host.last_error, None);
        // It is a word, not a one-shot: a second call returns the same zero
        // and still changes nothing.
        assert_eq!(run_finalization(&mut host, &[]), Some(0));
        assert_eq!((host.gc_context, host.info), (true, before.1));
    }

    /// The O10 addresses, return words and the JVM fields the docs cite.
    #[test]
    fn gc_constants_are_the_exe_words() {
        assert_eq!(GC, 0x0047_c060);
        assert_eq!(RUN_FINALIZATION, 0x0047_c070);
        assert_eq!(GC_INITIALIZED, 0xffff_ffff, "`or eax, -1` at 0x418058");
        assert_eq!(GC_COLLECTED_NONE, 0, "`mov eax, ebp` at 0x4180b2");
        assert_eq!(RUN_FINALIZATION_ZERO, 0, "`xor eax, eax` at 0x47c070");
        assert_eq!(JVM_GC_CONTEXT, 0x20, "[jvm+0x20]");
        assert_eq!(JVM_MARK_BYTES, 0x28, "[jvm+0x28]");
        assert_eq!(GC_CONTEXT_CAPACITY, 0x40, "0x417fc0-0x417fcf");
        assert_eq!(GC_CONTEXT_BYTES, 0x100, "`shl edx, 2` at 0x418b2d");
        assert_eq!(GC_JVM, 0x0062_e004, "the collector's own JVM word");
    }

    /// `exit()` writes the recorded request once and returns `V`; repeating
    /// it is idempotent, and nothing in the port terminates the process (the
    /// exe only sets the word — its main loop polls it).
    #[test]
    fn exit_records_the_request_and_never_terminates() {
        seam::reset();
        assert!(!with_lang_host(|host| host.exit_requested()));
        let mut vm = crate::test_strings::vm();
        assert_eq!(exit(&mut vm, &exit_info(&[], "()V")), None);
        assert_eq!(flag(), 1);
        assert!(with_lang_host(|host| host.exit_requested()));
        // The request is a word, not a one-shot: a second call keeps 1 and
        // the port is still usable (the next native port runs).
        assert_eq!(exit(&mut vm, &exit_info(&[], "()V")), None);
        assert_eq!(flag(), 1);
        seam::reset();
    }

    /// The `String` overload marshals and discards: it never sets the flag,
    /// not even when the container cannot satisfy the descriptor, and it does
    /// not clear a request the other overload made (build 606 quirk, pinned
    /// by the oracle's `exit` suite).
    #[test]
    fn exit_string_marshals_and_requests_no_exit() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        // receiver + the `java.lang.String` parameter, top of container last.
        let args = [Value::reference(0x2000_00c0), Value::reference(0x1000)];
        assert_eq!(
            exit_string(&mut vm, &exit_info(&args, "(Ljava.lang.String;)V")),
            None
        );
        assert_eq!(flag(), 0, "only exit()V writes [0x63c520]");
        // A container with no parameter left: the marshaller stops, the port
        // still drops the words and still sets no flag.
        assert_eq!(
            exit_string(&mut vm, &exit_info(&[], "(Ljava.lang.String;)V")),
            None
        );
        assert_eq!(flag(), 0);
        // A request made by the no-argument overload survives it.
        with_lang_host(|host| host.exit_flag = 1);
        assert_eq!(
            exit_string(&mut vm, &exit_info(&args, "(Ljava.lang.String;)V")),
            None
        );
        assert_eq!(flag(), 1);
        seam::reset();
    }

    fn log_info<'a>(args: &'a [Value]) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: SYSTEM,
            name: "log",
            sig: "(Ljava.lang.String;)V",
        }
    }

    fn logged() -> Vec<ConsoleAppend> {
        with_log(|sink| sink.take())
    }

    /// A live message appends the marshalled word and then the newline — two
    /// appends to the console object, exactly the exe's two calls at
    /// `0x47bea2`/`0x47beb1` — and the native returns `V`.
    #[test]
    fn log_records_the_message_word_and_the_newline() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        // receiver + the `java.lang.String` parameter, top of container last.
        let args = [
            Value::reference_named(0x2000_00c0, "Ljava.lang.String;"),
            Value::reference(0),
        ];
        assert_eq!(log(&mut vm, &log_info(&args)), None);
        // Each call appends its own pair; the sink is a stream, not a slot.
        assert_eq!(log(&mut vm, &log_info(&args)), None);
        assert_eq!(
            logged(),
            vec![
                ConsoleAppend::Message(0x2000_00c0),
                ConsoleAppend::Newline,
                ConsoleAppend::Message(0x2000_00c0),
                ConsoleAppend::Newline
            ]
        );
        assert!(logged().is_empty(), "take drains the stream");
        seam::reset();
    }

    /// A null message takes the exe's one-append path (`0x47beb7`): the
    /// literal `<null>\n` at `0x612cdc`, with no separate newline.
    #[test]
    fn log_null_appends_the_null_line() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let args = [Value::reference(0), Value::reference(0)];
        assert_eq!(log(&mut vm, &log_info(&args)), None);
        assert_eq!(logged(), vec![ConsoleAppend::NullLine]);
        seam::reset();
    }

    /// A container the descriptor cannot satisfy stops the marshaller before
    /// the message slot is written; the port's zeroed slot takes the null
    /// path (the exe's slot would still hold the `info` pointer — the
    /// malformed-container rule every marshalling native states).
    #[test]
    fn log_with_a_malformed_container_takes_the_null_path() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        assert_eq!(log(&mut vm, &log_info(&[])), None);
        assert_eq!(logged(), vec![ConsoleAppend::NullLine]);
        // A `V` return and no exit request: `log` is not the exit overload.
        assert_eq!(flag(), 0);
        seam::reset();
    }

    /// `log` touches only the console sink: the exit request word is
    /// neither set nor cleared (only `exit()V` writes it).
    #[test]
    fn log_never_touches_the_exit_flag() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let args = [Value::reference(0x2000_00c0), Value::reference(0)];
        assert_eq!(log(&mut vm, &log_info(&args)), None);
        assert_eq!(flag(), 0);
        with_lang_host(|host| host.exit_flag = 1);
        assert_eq!(log(&mut vm, &log_info(&args)), None);
        assert_eq!(flag(), 1);
        seam::reset();
    }

    fn analyze_info<'a>(args: &'a [Value], name: &'a str, sig: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: SYSTEM,
            name,
            sig,
        }
    }

    /// The `info(I)Ljava.lang.String;` row's own info, in the container shape
    /// the exe marshals from: `[param, receiver]` with the receiver last.
    fn info_info<'a>(args: &'a [Value]) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: SYSTEM,
            name: "info",
            sig: "(I)Ljava.lang.String;",
        }
    }

    /// One `info` argument container: the `I` parameter, then the (unread)
    /// receiver word, which is where `FUN_0045d910` reads `[count-1]`.
    fn info_args(id: u32) -> [Value; 2] {
        [Value::int(id as i32), Value::int(0)]
    }

    fn analyzed() -> Vec<AnalyzeEvent> {
        with_analyze(|sink| sink.take())
    }

    /// One `java.lang.System` value cell for the marshalled parameter slot:
    /// the container is `[receiver, param]` with the receiver last, which is
    /// where `FUN_0045d910` reads it.
    fn object_args(param: Value, receiver: Value) -> [Value; 2] {
        [param, receiver]
    }

    /// `analyze(Ljava.lang.Object;)V` records the marshalled parameter word —
    /// `[count-2]`, not the receiver — and is a `V` native: the exe's
    /// `test eax,eax` has a one-`nop` body, so nothing else happens.
    #[test]
    fn analyze_object_records_the_marshalled_word() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let param = Value::reference(0xcafe_babe);
        let args = object_args(param, Value::reference(0x2000_00c0));
        let info = analyze_info(&args, "analyze", "(Ljava.lang.Object;)V");
        assert_eq!(analyze_object(&mut vm, &info), None);
        assert_eq!(
            analyzed(),
            vec![AnalyzeEvent {
                kind: AnalyzeKind::Object,
                value: 0xcafe_babe
            }]
        );
        // A null parameter is recorded as 0 — the exe's `test` is the only
        // use of the word either way.
        let null = Value::reference(0);
        let args = object_args(null, Value::reference(0x2000_00c0));
        assert_eq!(
            analyze_object(
                &mut vm,
                &analyze_info(&args, "analyze", "(Ljava.lang.Object;)V")
            ),
            None
        );
        // The sink is a stream: each call appends its own event, in order.
        assert_eq!(
            analyze_object(
                &mut vm,
                &analyze_info(&args, "analyze", "(Ljava.lang.Object;)V")
            ),
            None
        );
        assert_eq!(
            analyzed(),
            vec![
                AnalyzeEvent {
                    kind: AnalyzeKind::Object,
                    value: 0
                },
                AnalyzeEvent {
                    kind: AnalyzeKind::Object,
                    value: 0
                }
            ]
        );
        assert!(analyzed().is_empty(), "take drains the stream");
        seam::reset();
    }

    /// `analyze(I)V` records the payload word verbatim: the marshaller copies
    /// the cell's word with no sign or width handling, and the exe never
    /// reads it back.
    #[test]
    fn analyze_int_records_the_payload_verbatim() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        for word in [0u32, 1, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff] {
            let param = Value::int(word as i32);
            let args = object_args(param, Value::int(0));
            assert_eq!(
                analyze_int(&mut vm, &analyze_info(&args, "analyze", "(I)V")),
                None
            );
            assert_eq!(
                analyzed(),
                vec![AnalyzeEvent {
                    kind: AnalyzeKind::Int,
                    value: word
                }],
                "{word:#x}"
            );
        }
        seam::reset();
    }

    /// `analyze(F)V` records the f32 **bit pattern**: the exe's body is the
    /// int overload's with an `(F)V` descriptor, the marshaller converts
    /// nothing and the native never pushes to the FPU — so even a denormal
    /// word is recorded as it stands instead of being rounded through an
    /// `f32`.
    #[test]
    fn analyze_float_records_the_bit_pattern() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        for (value, bits) in [
            (1.5f32, 0x3fc0_0000u32),
            (-1.5, 0xbfc0_0000),
            (0.0, 0x0000_0000),
            (-0.0, 0x8000_0000),
        ] {
            let param = Value::float(value);
            let args = object_args(param, Value::int(0));
            assert_eq!(
                analyze_float(&mut vm, &analyze_info(&args, "analyze", "(F)V")),
                None
            );
            assert_eq!(
                analyzed(),
                vec![AnalyzeEvent {
                    kind: AnalyzeKind::Float,
                    value: bits
                }],
                "{value:?}"
            );
            assert_eq!(bits, value.to_bits());
        }
        // A word that is not a normal f32 survives unrounded: the smallest
        // subnormal (1.4e-45) is the word 1, and a signalling NaN keeps its
        // payload.
        for word in [1u32, 0x7f80_0001, 0xffc0_0001] {
            let param = Value::float(f32::from_bits(word));
            let args = object_args(param, Value::int(0));
            assert_eq!(
                analyze_float(&mut vm, &analyze_info(&args, "analyze", "(F)V")),
                None
            );
            assert_eq!(
                analyzed(),
                vec![AnalyzeEvent {
                    kind: AnalyzeKind::Float,
                    value: word
                }],
                "{word:#010x} is the raw bit pattern"
            );
        }
        seam::reset();
    }

    /// A container the descriptor cannot satisfy stops the marshaller before
    /// the parameter slot is written; the port records 0 (the exe's slot
    /// would still hold the `info` pointer — the malformed-container rule the
    /// other marshalling natives state, ledger row O5b). No exit request
    /// either: none of the trio is the exit overload.
    #[test]
    fn analyze_records_zero_for_a_malformed_container() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let none: [Value; 0] = [];
        assert_eq!(
            analyze_object(
                &mut vm,
                &analyze_info(&none, "analyze", "(Ljava.lang.Object;)V")
            ),
            None
        );
        assert_eq!(
            analyze_int(&mut vm, &analyze_info(&none, "analyze", "(I)V")),
            None
        );
        assert_eq!(
            analyze_float(&mut vm, &analyze_info(&none, "analyze", "(F)V")),
            None
        );
        assert_eq!(
            analyzed(),
            vec![
                AnalyzeEvent {
                    kind: AnalyzeKind::Object,
                    value: 0
                },
                AnalyzeEvent {
                    kind: AnalyzeKind::Int,
                    value: 0
                },
                AnalyzeEvent {
                    kind: AnalyzeKind::Float,
                    value: 0
                }
            ]
        );
        assert_eq!(flag(), 0, "no analyze overload writes [0x63c520]");
        seam::reset();
    }

    /// The trio touches the `analyze` sink and nothing else: no seam word
    /// moves, and each overload's kind/descriptor is the registry row's own.
    #[test]
    fn analyze_kinds_and_descriptors_are_the_registry_rows() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let param = Value::int(7);
        let args = object_args(param, Value::int(0));
        let calls: [(u32, &str, AnalyzeKind); 3] = [
            (ANALYZE_OBJECT, "analyze", AnalyzeKind::Object),
            (ANALYZE_INT, "analyze", AnalyzeKind::Int),
            (ANALYZE_FLOAT, "analyze", AnalyzeKind::Float),
        ];
        for (addr, name, kind) in calls {
            let info = analyze_info(&args, name, kind.signature());
            let ret = match addr {
                ANALYZE_OBJECT => analyze_object(&mut vm, &info),
                ANALYZE_INT => analyze_int(&mut vm, &info),
                ANALYZE_FLOAT => analyze_float(&mut vm, &info),
                other => panic!("unexpected address {other:#x}"),
            };
            assert_eq!(ret, None, "{addr:#x} is a V native");
            assert_eq!(analyzed(), vec![AnalyzeEvent { kind, value: 7 }]);
            // The kind's descriptor is the row the registrar binds to this
            // address (`re/out/native_api.tsv` 104-106).
            let row = SYSTEM_NATIVE_BINDINGS
                .iter()
                .find(|row| row.impl_addr == addr)
                .expect("registered row");
            assert_eq!(row.sig, kind.signature());
            assert_eq!(row.name, name);
            assert_eq!(row.class, SYSTEM);
        }
        assert_eq!(flag(), 0);
        assert_eq!(with_lang_host(|host| host.sim_time), 0.0);
        assert_eq!(with_lang_host(|host| host.clock), 0.0);
        assert!(with_log(|sink| sink.is_empty()), "the trio logs nothing");
        seam::reset();
    }

    /// The six `info` ids read the six seam readings and intern the `%d`
    /// text of each — one `java.lang.String` per call, through the same
    /// interner `Object.toString` uses (`FUN_004174a0`).
    #[test]
    fn info_reports_the_six_seam_readings() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 2,
                list_1c: 0,
                list_10: 3,
                word_24: 12345,
                word_28: 0xffff_ffff,
                live_cells: 42,
            };
        });
        // The six rows of the probe's fixture: 0/1/2 are the chain counts
        // (2, 0 and 3 links after the head), 3/4/5 the pushed fields.
        for (id, text) in [
            (0, "2"),
            (1, "0"),
            (2, "3"),
            (3, "12345"),
            (4, "-1"),
            (5, "42"),
        ] {
            let args = info_args(id);
            let ret = info(&mut vm, &info_info(&args)).expect("the row returns a String");
            let index = (ret - 0x1000) as usize;
            assert_eq!(
                vm.objects[index].class_name(&vm.loader),
                "java.lang.String",
                "id {id}"
            );
            assert_eq!(
                vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
                text.len() as i32,
                "id {id} carries its byte length"
            );
            assert!(
                vm.strings.lookup(text.as_bytes()).is_some(),
                "id {id} interns {text:?}"
            );
            assert!(!with_lang_host(|host| host.exit_requested()));
        }
        assert_eq!(vm.strings.len(), 6, "six distinct texts");
        assert!(with_log(|sink| sink.is_empty()), "info logs nothing");
        assert!(
            with_analyze(|sink| sink.is_empty()),
            "info analyses nothing"
        );
        seam::reset();
    }

    /// The `%d` conversion is the exe's own (`FUN_00554170`): signed — the
    /// high bit survives as a leading `-` rather than a large positive — and
    /// with the shipped build's `0x8000_0000` quirk, where the pre-loop
    /// negation overflows back to itself and the characters come out as
    /// `48 - |rem|` over negative remainders.
    #[test]
    fn info_formats_each_reading_as_a_signed_decimal() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 0x7fff_ffff,
                list_1c: 0x8000_0000,
                list_10: 0xffff_ffff,
                word_24: 0xffff_fffe,
                word_28: 0x0000_0001,
                live_cells: 0,
            };
        });
        for (id, text) in [
            (0, "2147483647"),
            // `0x8000_0000`: the exe's own text, not "-2147483648".
            (1, "-./,),(-*,("),
            (2, "-1"),
            (3, "-2"),
            (4, "1"),
            (5, "0"),
        ] {
            let args = info_args(id);
            let ret = info(&mut vm, &info_info(&args)).expect("String");
            assert!(
                vm.strings.lookup(text.as_bytes()).is_some(),
                "id {id} interns {text:?}"
            );
            assert_eq!(
                vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
                text.len() as i32
            );
        }
        assert!(
            vm.strings.lookup(b"-2147483648").is_none(),
            "the exe does not produce the corrected INT_MIN text"
        );
        // `%d` of 0 is `"0"`, not the empty string.
        assert!(vm.strings.lookup(b"0").is_some());
        assert!(!vm.strings.is_empty());
        seam::reset();
    }

    /// The six values the oracle's `info-word-*` rows pin, through the port's
    /// own conversion: the ordinary signed decimals and the `INT_MIN` quirk.
    #[test]
    fn decimal_reproduces_the_exe_conversion() {
        for (word, text) in [
            (0x0000_0000u32, "0"),
            (0x0000_0001, "1"),
            (0x0000_002a, "42"),
            (0x0000_3039, "12345"),
            (0x7fff_ffff, "2147483647"),
            (0xffff_ffff, "-1"),
            (0xffff_fffe, "-2"),
            (0x8000_0000, "-./,),(-*,("),
            (0x8000_0001, "-2147483647"),
        ] {
            assert_eq!(decimal(word), text, "{word:#010x}");
        }
    }

    /// The row's addresses are the exe's own: the native, its `"%d"` text,
    /// the switch's jump table and the live-cell counter.
    #[test]
    fn info_constants_are_the_exe_words() {
        assert_eq!(INFO, 0x0047_c0c0);
        assert_eq!(INFO_FORMAT, 0x0061_2ce4);
        assert_eq!(INFO_TABLE, 0x0047_c1b4);
        assert_eq!(LIVE_CELLS, 0x0062_f138);
        let row = SYSTEM_NATIVE_BINDINGS
            .iter()
            .find(|row| row.impl_addr == INFO)
            .expect("the info row is registered");
        assert_eq!(row.class, SYSTEM);
        assert_eq!(row.name, "info");
        assert_eq!(row.sig, "(I)Ljava.lang.String;");
    }

    /// Every id outside 0..=5 skips the `sprintf` (`ja 47c19e`) and still
    /// interns: the shipped build returns whatever its frame's scratch word
    /// held, which the port cannot reproduce, so it interns the **empty**
    /// string — the reading of a fresh slot. A `String` still comes back.
    #[test]
    fn info_ids_outside_zero_to_five_intern_the_empty_string() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 2,
                list_1c: 0,
                list_10: 3,
                word_24: 12345,
                word_28: 0xffff_ffff,
                live_cells: 42,
            };
        });
        // `cmp eax,5; ja` is unsigned, so the negatives take this path too.
        for id in [6u32, 7, 0x8000_0000, 0xffff_ffff, 0x4141_4141] {
            let args = info_args(id);
            let ret = info(&mut vm, &info_info(&args)).expect("the row returns a String");
            let index = (ret - 0x1000) as usize;
            assert_eq!(
                vm.objects[index].class_name(&vm.loader),
                "java.lang.String",
                "id {id:#x}"
            );
            assert_eq!(
                vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
                0,
                "id {id:#x}"
            );
        }
        assert!(
            vm.strings.lookup(b"").is_some(),
            "the empty text is interned"
        );
        assert!(
            vm.strings.lookup(b"2").is_none() && vm.strings.lookup(b"42").is_none(),
            "no reading was formatted for these ids"
        );
        assert!(!with_lang_host(|host| host.exit_requested()));
        seam::reset();
    }

    /// The switch reads the marshalled parameter — `[count-2]`, the first
    /// declared parameter — not the receiver at `[count-1]` and not the
    /// container's first cell. With `params=(0xAAAA, 3)` the id is 3.
    #[test]
    fn info_switches_on_the_marshalled_parameter() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 2,
                word_24: 12345,
                ..SystemInfo::new()
            };
        });
        let args = [Value::int(0xaaaa), Value::int(3), Value::int(0)];
        assert_eq!(args.len(), 3);
        info(&mut vm, &info_info(&args)).expect("String");
        assert!(
            vm.strings.lookup(b"12345").is_some(),
            "the id is cells[count-2], the receiver is not read"
        );
        assert!(vm.strings.lookup(b"2").is_none(), "not cells[count-1]");
        seam::reset();
    }

    /// A container the descriptor cannot satisfy stops the marshaller before
    /// it writes the parameter out word, so the port reads 0 there and
    /// reports id 0's reading — the "the port reads 0 there" rule the other
    /// marshalling natives state (the exe's slot keeps the `info` pointer and
    /// its unfilled buffer is what it interns).
    #[test]
    fn info_with_a_malformed_container_reports_id_zero() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 2,
                word_24: 12345,
                ..SystemInfo::new()
            };
        });
        let none: [Value; 0] = [];
        let ret = info(&mut vm, &info_info(&none)).expect("String");
        let index = (ret - 0x1000) as usize;
        assert_eq!(vm.objects[index].class_name(&vm.loader), "java.lang.String");
        assert_eq!(
            vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
            1,
            "\"2\" is one byte"
        );
        assert!(vm.strings.lookup(b"2").is_some(), "id 0's reading");
        assert!(vm.strings.lookup(b"12345").is_none());
        assert!(vm.strings.lookup(b"").is_none(), "an id was reported");
        assert!(!with_lang_host(|host| host.exit_requested()));
        seam::reset();
    }

    /// Every row this module owns is known before its port lands, installs
    /// into the table and resolves end-to-end through the bridge.
    #[test]
    fn every_row_installs_and_dispatches_through_the_bridge() {
        seam::reset();
        for row in SYSTEM_NATIVE_BINDINGS {
            assert!(NativeTable::is_known(row.impl_addr), "{:?}", row);
        }
        assert_eq!(
            NativeTable::new().dispatch(&mut crate::seam::LangHost::new(), EXIT, &[]),
            Err(DispatchError::NotPorted { impl_addr: EXIT })
        );
        assert_eq!(
            NativeTable::new().dispatch(&mut crate::seam::LangHost::new(), BUILD_NUMBER, &[]),
            Err(DispatchError::NotPorted {
                impl_addr: BUILD_NUMBER
            })
        );

        let mut vm = crate::test_strings::vm();
        let mut table = NativeTable::new();
        install(&mut table);
        let mut expected: Vec<_> = SYSTEM_NATIVE_BINDINGS.iter().map(|b| b.impl_addr).collect();
        expected.sort_unstable();
        assert_eq!(
            table.ported(),
            expected,
            "all registered System rows installed"
        );
        seam::install(&mut vm, &table);
        for row in SYSTEM_NATIVE_BINDINGS {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
            assert!(vm.native_impls.contains_key(&row.impl_addr));
        }

        // `exit()` end to end: the flag is set through the bridge.
        assert_eq!(bridge(&mut vm, &exit_info(&[], "()V")), None);
        assert!(with_lang_host(|host| host.exit_requested()));

        // `exit(String)` end to end: no further change to the flag's value.
        let args = [Value::reference(0x2000_00c0), Value::reference(0x1000)];
        assert_eq!(
            bridge(&mut vm, &exit_info(&args, "(Ljava.lang.String;)V")),
            None
        );
        assert_eq!(flag(), 1);

        // `log(String)` end to end: the appends land in the seam sink.
        let log_args = [
            Value::reference_named(0x2000_00d0, "Ljava.lang.String;"),
            Value::reference(0),
        ];
        assert_eq!(bridge(&mut vm, &log_info(&log_args)), None);
        assert_eq!(
            logged(),
            vec![ConsoleAppend::Message(0x2000_00d0), ConsoleAppend::Newline]
        );
        assert_eq!(flag(), 1, "log never writes [0x63c520]");

        // The `analyze` trio end to end: each overload marshals its own
        // parameter and records the word in the seam sink.
        let analyze_args = [Value::int(0x1234_5678), Value::reference(0)];
        for (kind, name) in [
            (AnalyzeKind::Object, "analyze"),
            (AnalyzeKind::Int, "analyze"),
            (AnalyzeKind::Float, "analyze"),
        ] {
            let ret = bridge(
                &mut vm,
                &analyze_info(&analyze_args, name, kind.signature()),
            );
            assert_eq!(ret, None, "{kind:?} is a V native");
            assert_eq!(
                analyzed(),
                vec![AnalyzeEvent {
                    kind,
                    value: 0x1234_5678
                }]
            );
        }
        assert_eq!(flag(), 1, "no analyze overload writes [0x63c520]");

        // The three O7 scalars end to end, through the language-state path
        // of the bridge (they have no `CallInfo` port).
        assert_eq!(
            bridge(&mut vm, &scalar_info("buildNumber", "()I")),
            Some(BUILD_606)
        );
        with_lang_host(|host| host.sim_time = 12.5);
        assert_eq!(
            bridge(&mut vm, &scalar_info("simTime", "()F")),
            Some(12.5f32.to_bits())
        );
        fn ms_250() -> f32 {
            250.0
        }
        with_lang_host(|host| host.clock_source = Some(ms_250));
        assert_eq!(
            bridge(&mut vm, &scalar_info("currentTime", "()F")),
            Some((250.0f32 * MS_TO_SECONDS).to_bits())
        );
        assert_eq!(flag(), 1, "no scalar touches [0x63c520]");

        // `info(I)` end to end: it marshals its own parameter, reads the seam
        // readings and returns an interned `String` word.
        with_lang_host(|host| {
            host.info = SystemInfo {
                list_18: 2,
                list_1c: 0,
                list_10: 3,
                word_24: 12345,
                word_28: 0xffff_ffff,
                live_cells: 42,
            };
        });
        let info_words = info_args(4);
        let ret = bridge(&mut vm, &info_info(&info_words)).expect("a String return");
        let index = (ret - 0x1000) as usize;
        assert_eq!(vm.objects[index].class_name(&vm.loader), "java.lang.String");
        assert_eq!(
            vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
            2,
            "\"-1\""
        );
        assert!(vm.strings.lookup(b"-1").is_some());
        assert_eq!(flag(), 1, "info never writes [0x63c520]");

        // The O10 GC pair end to end. `gc` alternates the two passes through
        // the language-state path of the bridge, and its draining pass
        // overwrites the very `info(4)` reading the block above just left at
        // `0xffff_ffff`: the exe's `[jvm+0x28] = 0x40 << 2` at `0x418b2b`.
        assert_eq!(
            bridge(&mut vm, &scalar_info("gc", "()I")),
            Some(GC_INITIALIZED),
            "the initializing pass"
        );
        assert_eq!(
            bridge(&mut vm, &scalar_info("gc", "()I")),
            Some(GC_COLLECTED_NONE),
            "the draining pass"
        );
        let ret = bridge(&mut vm, &info_info(&info_args(4))).expect("a String return");
        assert_eq!(
            vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
            3,
            "\"256\""
        );
        assert!(
            vm.strings.lookup(b"256").is_some(),
            "info(4) after a gc cycle"
        );
        assert!(with_lang_host(|host| !host.gc_context), "drained");
        assert_eq!(flag(), 1, "neither O10 native writes [0x63c520]");

        // `runFinalization` end to end: the constant zero, nothing else.
        assert_eq!(
            bridge(&mut vm, &scalar_info("runFinalization", "()I")),
            Some(RUN_FINALIZATION_ZERO)
        );
        assert_eq!(flag(), 1);

        // The thread-local table is the one `install` published, so the plain
        // dispatch entry reaches the same ports.
        assert_eq!(seam::dispatch(BUILD_NUMBER, &[]), Ok(Some(BUILD_606)));
        assert_eq!(seam::dispatch(SIM_TIME, &[]), Ok(Some(12.5f32.to_bits())));
        assert_eq!(seam::dispatch(GC, &[]), Ok(Some(GC_INITIALIZED)));
        assert_eq!(seam::dispatch(RUN_FINALIZATION, &[]), Ok(Some(0)));
        seam::reset();
    }
}
