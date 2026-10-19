# Hook layer (`re/hook`, M0-11)

A Win32 proxy DLL that runs the stock exe under Wine and records what it does as
an `SLRLOG` call log (`docs/call-log.md`, `crates/slr-calllog` is normative).

Status: **statically verified only.** Four defects were root-caused and fixed
(`00c80af` registrar split push; `96085b4` NULL trampoline read; `e90c4dd`
publish window; `0e83685` lock span), three of them latent — but **no Wine
launch has been made since `00c80af`/`96085b4`, and none at all since
`e90c4dd`/`0e83685`**. Nothing in this document is an end-to-end result; see §6
for the exact per-row classification, §7 for what is proven and the single
pending live step (§7.1).

## 1. What it emits

Two files per run (paths from `SLR_HOOK_LOG` / `SLR_HOOK_TEXT`):

- `hook.slrlog` — the call log: 8-byte header (`SLRLOG` + u16 version 1 + pad),
  then framed records `tag u8, body length u32 LE, body`, written under a lock
  from every thread, buffered 256 KiB and flushed on close. An empty run is
  exactly 8 bytes.
- `hook.txt` — human-readable install/trace lines (`attach:`, `install:`,
  `native: hooked …`, `com:`/`files:`/`input:`, `finish:`). Diagnostic only,
  off unless `SLR_HOOK_TEXT` is set (`run.sh` always sets it).

Record kinds the hook actually writes (tags 0x05 VM snapshot and 0x06 per-frame
vehicle have **no writer here** — they belong to the Rust backends):

| Tag | Source | Body |
|---|---|---|
| 0x00 native | one detour per registered native | `str class, str name, u32 argc(=0), option<value> result` |
| 0x01 D3D9 | generated vtable thunks on `IDirect3D*` | `u8 kind, str method, u32 argc + values` |
| 0x02 DirectSound | same, on `IDirectSound*` | `u8 kind, str method, u32 argc + values` |
| 0x03 DirectInput | `GetKeyboardState`/`GetAsyncKeyState`/`GetCursorPos` + wrapped IDirectInput8 devices | `u8 kind, u32 n + bytes report` (keyboard: 256 state bytes; mouse: 12-byte `dx,dy,buttons`) |
| 0x04 file | `CreateFileA/W, ReadFile, WriteFile, CloseHandle, FindFirstFileA, FindNextFileA` | `u8 op, str path, u32 n + values detail` (open 0, read 1, write 2, close 3, find 4) |

Details worth knowing:

- **Native results are typed from the signature's last character**: `V` = no
  result; `F`/`D` = ST0 float/double *bit pattern* (sampled then restored);
  `J` = EDX:EAX long; `L`/`[` = object pointer replaced by a per-run sequential
  id (0 = null); everything else = EAX as int. Raw pointers never reach the log.
- **Native arguments are not logged** (`argc = 0`). Their values live in TUFA
  heap objects whose layout is M1-02 work; reading them now would be guessing.
  This is the largest content gap.
- File reads/writes log a CRC32 of the payload unless
  `SLR_HOOK_FILE_PAYLOAD=1`, which logs the bytes instead.
- `MessageBoxA` is wrapped: auto-dismissed with `IDOK` so a headless replay
  cannot block, and logged as a D3D9-kind-0 record (`method = MessageBoxA:<flags>`,
  two string values) because a dialog is a boot outcome.
- All records carry no host timestamps, only values the game itself produced.
  Order is call order; records never interleave mid-record.

## 2. Mechanism

- **Proxy DLL** (chosen by `jev_decide: proxy-dll`): `build.sh` produces a
  32-bit `dinput8.dll`; `WINEDLLOVERRIDES=dinput8=n,b` makes Wine load it
  because `DINPUT8.dll!DirectInput8Create` is already an import of the exe
  (verified in the PE import table). It therefore runs before the exe entry
  point, in the exe's process.
- `DllMain` opens the log, then patches the exe entry point (`0x005514C0` at
  link base) with a 5-byte `E9` to an asm stub. The stub calls `slr_install`,
  which restores the original entry bytes, installs every hook (the exe's
  imports are fully bound by then) and jumps to the real entry.
- **IAT patching** walks the exe's import descriptors and overwrites the
  `FirstThunk` slot. A hook's original function is resolved with
  `slr_real_import()` (`GetProcAddress` on the loaded DLL) — *never* by
  re-reading the IAT slot, which now holds the hook itself.
- **Inline detours** (`slr_detour`) copy whole instructions (≥5 bytes, no
  relative branch inside; sized by the small x86 length decoder in
  `hook_native.c`) into a trampoline, `E9`-jump back to `target+stolen`, and
  `E9`-jump the target to the detour. Code is taken from a 4 MiB RWX pool
  (`slr_alloc_exec`). The signature is
  `slr_detour(target, detour, stolen, publish)`: the trampoline is stored into
  `*publish` **before** the target's entry byte is patched, so a hook that hops
  through that slot (`call [e->tramp]`, `jmp *g_reg_tramp`) can never be entered
  with a NULL slot. A hook that reads no slot (the `E9`-alias tail thunk, which
  decodes its target from the stub itself) passes `NULL`.
- **COM logging** patches the real vtables in place (Wine's d3d9 asserts on
  vtable identity, so they are not copied) with generated stdcall thunks that
  recreate the frame, log, call the original, and wrap any interface the call
  produced. Tables (slot order, argument classes) are generated from the
  mingw-w64 `d3d9.h`/`dsound.h` at build time.
- **Natives**: `FUN_00416b00` (the TUFA registrar) is detoured once; the
  static 353-row table derived from the exe (`re/hook/native_table.h`, generated
  from `re/out/native_api.tsv`) is hooked at install, and late registrations
  seen by the registrar are hooked on capture. `java.io.Controller`'s 7
  registrations are absent from the table (Ghidra has no owner for
  `0x477cb0`); the live registrar is what would recover them.

## 3. Build

Prerequisites (see `docs/wine-spike.md` for the Wine install):

- `i686-w64-mingw32-gcc` (mingw-w64; `brew install mingw-w64`).
- mingw-w64 headers for `gen_com.py` (`/opt/homebrew/Cellar/mingw-w64/*/toolchain-i686/i686-w64-mingw32/include`).
- `re/out/native_api.tsv` (git-ignored) from `python3 re/build_native_table.py`
  (reads `reference/raw/StreetLegal_Redline.bak.exe` and the Ghidra export
  `re/out/all_pseudo.c`, asserts 353 rows). `gen_native.py` regenerates
  `re/hook/native_table.h` from it.

```sh
re/hook/build.sh                 # -> re/out/wine/hook/dinput8.dll
re/hook/build.sh /tmp/hookout    # or an explicit output directory
```

Build flags that matter: `-mno-sse -mfpmath=387` (the DLL must not disturb the
game's x87 state), `-mstackrealign`, `-static-libgcc`, `-Wl,--kill-at`.

Data-free tests (no Wine, no game data):

```sh
re/hook/tests/run_tests.sh
# cc-compiled slrlog.h emitter == Python reference emitter, byte for byte;
# the Rust reader accepts both and slr-calllog-diff reports identical.
```

## 4. Run

`run.sh` refuses to start without explicit authorization:

```sh
SLR_ALLOW_WINE=1 re/hook/run.sh <run-name> [timeout-seconds]
```

`SLR_ALLOW_WINE` **hard gate**: the script exits 4 with a message unless the
variable is exactly `1`. This is a refusal, not a retry prompt. The gate exists
because both live sessions produced user-visible Wine error dialogs and the
working rule since then has been static-only analysis
(`slr-notes/decision-m0-11-static-only.md`). Per-invocation authorization only;
do not put it in a wrapper or CI.

A run needs this layout under `re/out/wine` (git-ignored):

```text
wine64/Wine Stable.app   pinned Wine 11.0 (Gcenx 11.0_1)
game-pristine/           untouched COPY of stock/ (never stock/ itself)
prefix-pristine/         wineboot-initialised prefix, HKLM AeDebug Auto=0
hook/dinput8.dll         build.sh output
runs/<name>/             per-run output
```

Each run `cp -cR`-clones `game-pristine` and `prefix-pristine` (APFS copy-on-write,
so two runs start from byte-identical inputs), copies the DLL in as
`game/dinput8.dll`, then launches exactly once under a `perl alarm` timeout and
writes `stdout.log`, `stderr.log`, `exit.code`, plus the two hook logs from
`SLR_HOOK_LOG` / `SLR_HOOK_TEXT`. `wineserver -k` runs afterwards.

Environment (all read once in `config_init`, `re/hook/hook.h`):

| Variable | Default | Meaning |
|---|---|---|
| `SLR_HOOK_LOG` | `hook.slrlog` | SLRLOG output path (Wine `Z:` paths work) |
| `SLR_HOOK_TEXT` | `""` (off) | human-readable diagnostic log |
| `SLR_HOOK_TRACE` | `""` | import trace: a DLL substring, `all` (minus hot CRT calls), or anything starting with `c` = trace only (`nat>`/`com>` lines) |
| `SLR_HOOK_INPUT` | `""` | scripted input: `<frame> key <vk> <up\|down>`, `<frame> mouse <dx> <dy> <buttons>` |
| `SLR_HOOK_SEED` | `0x2005020b` | pinned epoch/seed (build-606 release date) |
| `SLR_HOOK_STEP_US` | `50` | virtual microseconds added per clock read |
| `SLR_HOOK_DT_US` | `16667` | fixed per-frame step; currently parsed and reported at `attach:` only — the virtual clock advances per read, not per frame |
| `SLR_HOOK_MAX_MS` | `0` | exit once virtual time reaches this |
| `SLR_HOOK_IDLE_MS` | `0` | wall-clock idle watchdog → `slr_finish("idle")` |
| `SLR_HOOK_MAX_RECORDS` | `0` | exit after N records |
| `SLR_HOOK_FILE_PAYLOAD` | `0` | `1` logs payload bytes instead of CRC32 |
| `SLR_HOOK_DISABLE` | `""` | comma list; tokens implemented: `natives`, `files`, `input`, `vtable`, `devwrap` |
| `SLR_HOOK_WINDOWED` | `1` | rewrite a requested full-screen `D3DPRESENT_PARAMETERS` to windowed (the log keeps what the game asked for) |

Determinism measures already in place: virtual clock for
`GetTickCount`/`QueryPerformanceCounter`/`GetSystemTime*`/`timeGetTime`/`Sleep`
(`Sleep(0)` is a no-op; `Sleep(N)` advances the virtual clock and does not
really wait), input only from the script, sequential object ids assigned in
first-appearance order (both for native returns and COM objects), fresh clones
per run, and one lock per record.

## 5. Crash root causes

Four defects, in fix order; the first two were live crashes diagnosed from
captured logs, the last two are static residuals found by reading the emission
path. No relaunch was needed for any of them, and no fix has been run under
Wine (the pending step, §7.1).

| § | Defect | Commit | Kind |
|---|---|---|---|
| 5.1 | registrar trampoline split the `push 0x18` immediate (steal 5 → 6) | `00c80af` | observed crash `c0000096` |
| 5.2 | alias tail thunk read `e->tramp` before it was assigned | `96085b4` (W1) | observed page fault at `0x1` |
| 5.4 | non-alias publish window: entry patched before the slot store | `e90c4dd` (W4) | static residual |
| 5.5 | `g_nat_cs` released between the `hooked` check and the patch | `0e83685` (W5) | static residual |

### 5.1 `c0000096` at `0x01790006` — split instruction in the registrar trampoline

`sev1`: `EXCEPTION_PRIV_INSTRUCTION c0000096` at `0x01790006`. The hook DLL was
based at `0x01790000`, so the fault was in our own RWX pool at `pool+6`.
`slr_detour(FUN_00416b00, …, stolen=5)` copied 5 bytes of the registrar
prologue `53 55 56 57 6A 18` (four pushes then `push 0x18`), splitting the
2-byte immediate. The trampoline therefore began with a stray `6A`, followed by
the appended `E9` whose first `rel32` byte `0xFB` is `STI` — privileged in user
mode.

Fix (`00c80af`): size the steal with `stolen_bytes()`, the same length decoder
used for natives (whole instructions, ≥5 bytes, no relative branch). The
registrar's steal becomes 6 bytes (`install` log line reports
`353 static rows, 6 stolen bytes`).

### 5.2 Page fault reading `0x1` at `0x7B0459FB` — alias thunk read a NULL trampoline

`v1` (after `00c80af`): page fault on read `0x1` at `0x7B0459FB`, i.e. our DLL
RVA `0x59FB`, inside `slr_native_hook_all`, immediately after
`native: registrar hooked (353 static rows, 6 stolen bytes)` and before the
hook-all summary — `hook.slrlog` was 8 bytes (header only).

The 6 entries whose first byte is `E9` (alias stubs to a shared body) take the
tail-detour path. `make_tail_thunk()` ran *before* `slr_detour()` returned and
read `*(int32_t *)(e->tramp + 1)`, but `e->tramp` is only assigned by that
`slr_detour()` call, so it was still `NULL`: a deterministic read at `0x1` on
the first alias entry. The arithmetic was also wrong with a live trampoline —
an `E9` `rel32` is relative to the stub, so adding it to `tramp+5` aims at
`target + (tramp - stub)`, a wild jump.

Fix (`96085b4`, ledger W1): decode the target from the stub's own bytes
(`*(int32_t *)(stub + 1)` rebased onto `stub + 5`), compute the thunk's `E9`
relative to the thunk, and stop recording a trampoline for alias entries. The
path no longer reads `e->tramp` at all, so it has no detour-order dependency —
but the *non-alias* path still did: see §5.4.
Single hop is provably complete for this exe: none of the six alias targets is
itself an `E9` stub or another table entry.

### 5.3 Also fixed in `00c80af` (same live run)

- **Unbounded recursion** (the earlier `7B043B30` stack-overflow signature):
  every lazy hook did `real = slr_iat_hook(..., my_hook)` on first call — after
  its own slot was installed — and got itself back. Replaced by
  `slr_real_import()` in `hook_file.c` / `hook_input.c`.
- **x87 corruption**: `make_thunk` unconditionally saved/restored ST0, destroying
  live x87 values for the ~300 int/void natives. The x87 path is now emitted only
  for `F`/`D` result signatures.
- **Natives never hooked**: the `g_regs == 360` trigger could not fire for a
  353-row table; the static rows are now hooked at install, late ones on capture.
- **Non-deterministic object ids**: ids came from pointer-hash slot order; now
  first-appearance order.
- Removed the `GetSystemInfo` hook (faking a 1-CPU box changes the thread pool).
- `run.sh`: `winedbg=d` in `WINEDLLOVERRIDES` so a fault cannot spawn winedbg or
  raise the "serious problem" dialog, plus `AeDebug\Auto=0` in the pristine
  prefix. Note: the user-visible Wine error box appeared anyway in the two live
  sessions; this suppression is not a guarantee that a GUI box cannot appear.

### 5.4 Non-alias trampoline publish window (ledger W4, static residual)

The residual of §5.2 on the *non-alias* path. Every hooked entry gets a thunk
that ends by calling the real implementation through the slot:

```
FF 15 &e->tramp      ; call dword ptr [slot]  -- read at call time
```

`slr_detour` wrote the entry's `E9` jmp (making the thunk reachable) and only
*then* returned the trampoline, which the caller stored into `e->tramp`. Any
thread already running inside the game that called the entry inside that window
— after the patch, before the store — reached the thunk with `e->tramp == NULL`
and transferred through address `0` (the same fault class as §5.2's `0x1` read).
The registrar had the identical window on `g_reg_tramp` (`jmp *g_reg_tramp`).

Fix (ledger W4): `slr_detour` takes the slot and publishes the
trampoline before patching the entry, with a `__atomic_thread_fence` between the
store and the patch (`lock or [esp],0` on i686); it no longer returns the
trampoline, so the store order cannot be re-broken by a later assignment. The
non-alias call site passes `&e->tramp`, the registrar passes `&g_reg_tramp`, and
the alias tail path passes `NULL` (it reads no slot, §5.2).

### 5.5 `slr_native_hook_all` released `g_nat_cs` before the patch (ledger W5, static)

Latent double-patch. The per-entry loop read `e->hooked` under `g_nat_cs`,
released the lock, and only then built the thunk and called `slr_detour`; the
`hooked` store was taken again after the call. Two callers can overlap:
`slr_native_install` calls the pass directly, and `slr_reg_capture` calls it
from the registrar thread the moment the 360th interception lands (§2). Both
could read `hooked == 0` for the same entry and patch it twice — the second
`slr_detour` copies the *already patched* `E9` (plus NOP padding) into a fresh
trampoline and re-points the thunk at the hook entry itself, so calls to that
native would recurse into the logger instead of the real body. Today the two
callers are DllMain-time and effectively sequential, so no run has exercised
it; it is a correctness hole in the "idempotent" contract, not an observed
crash.

Fix (ledger W5): the loop body moved into `nat_hook_one(e, &live, &skip,
&preonly)`, and `slr_native_hook_all` now takes `g_nat_cs` immediately before
that call and releases it immediately after — one `Enter`/`Leave` pair per
entry with the detour inside. `nat_hook_one` documents the contract (called
with `g_nat_cs` held), takes no lock of its own, and commits `e->hooked`
(`-1`/`1`/`2`) before every return, so the check-then-patch sequence is atomic
against every other pass. Holding the lock across `slr_detour` is safe: it only
calls `slr_alloc_exec` (`VirtualAlloc`/pool bump), `memcpy`, `VirtualProtect`
and `FlushInstructionCache`, none of which re-enters `g_nat_cs` (the only other
takers are `obj_id` and `slr_reg_capture`, and neither runs under `g_text_cs`,
so there is no lock-order cycle). The hold is short: one entry is a few hundred
bytes of `memcpy` plus one `VirtualProtect` pair.

Both directions are asserted in `re/hook/tests/run_tests.sh`: `slr_native_hook_all`
has exactly one `EnterCriticalSection(&g_nat_cs)`, one `LeaveCriticalSection`
and one `nat_hook_one(...)` call strictly between them; `nat_hook_one` contains
neither lock call, both `slr_detour` sites, and each detour is followed by its
`e->hooked =` store before the branch ends; and `!e->hooked` no longer appears
anywhere in the file (the released-lock re-check pattern). The pre-W5 source
fails this check. In the built artifact (`-O1` inlines the static helper) the
compiled `_slr_native_hook_all` has a single `EnterCriticalSection` call site at
the loop head and a single `LeaveCriticalSection` call site at the loop-back
block, with both `_slr_detour` calls between them.

## 6. Row classification (static, exact)

Source of truth is the generated table itself: `re/hook/native_table.h` holds
**353 rows over 350 distinct impl VAs** — `0x47ddf0` (`GameRef.getInfo` twice),
`0x480440` (`GameRef.getDetail` / `RenderRef.getDetail`) and `0x483750`
(`GroundRef.getRouteLength` / `findRoute`) each appear twice.
`slr_native_install` copies all 353 rows into `g_nat`, so the
`native: hooked N live` counter counts *rows*; the second row of a duplicate VA
bumps the counter only (the first pass already set `hooked`), it is never
patched twice.

Every row was classified by the **compiled** `lde_len()` / `stolen_bytes()`
from `re/hook/hook_native.c` — extracted verbatim and linked into a host
program, run on the row's VA bytes (`re/out/oracle/lde_crosscheck.py` in
`~/slr-oracle-venv`). A byte-for-byte Python port
(`re/out/oracle/classify_rows.py`) agrees with the compiled decoder on all 353
rows, and capstone agrees with the decoder's instruction boundaries on every
row (0 mismatches). Reproduction (prints the class, steal size and head bytes
of all 353 rows, plus the alias targets and the steal histogram):

```sh
~/slr-oracle-venv/bin/python3 re/out/oracle/classify_rows.py          # Python port over all rows
~/slr-oracle-venv/bin/python3 re/out/oracle/lde_crosscheck.py         # compiled C decoder vs the port
```

Both scripts are git-ignored (`re/out`); they read only the exe and
`re/hook/native_table.h`, never `reference/` writes. Result:

| Class | Rows | VAs | Path in `nat_hook_one` |
|---|---|---|---|
| hookable | 343 | 340 | normal thunk; `slr_detour(..., st, &e->tramp)`; `hooked = 1` |
| `E9` alias stub | 6 | 6 | tail thunk; `slr_detour(..., 5, NULL)`; pre-log only; `hooked = 2` |
| skipped | 4 | 4 | not hooked; `hooked = -1` |
| unmapped / undecodable | 0 | 0 | — |

All 350 VAs lie in `.text` (`0x401000`–`0x5e6815`), so `IsBadCodePtr` rejects
none of them and "unmapped" is 0 by measurement, not by assumption.

Steal sizes over the 343 hookable rows: 5×157, 6×40, 7×60, 8×83, 10×3. The
three 10-byte steals are legal and `slr_detour` handles them exactly as any
other whole-instruction window (the decoder only crosses 5 bytes on an
instruction boundary): `System.exit` `0x47be50` (one 10-byte
`c7 05 20 c5 63 00 01 00 00 00` = `mov dword [0x63c520], 1`),
`FindFile.next` `0x487da0` and `GameRef.isScripted` `0x486da0` (both
`8b 4c 24 04` + `81 ec 08 01 00 00` = 4+6).

**The 6 `E9` alias rows** (head `E9 <rel32>` + NOP padding; decoded target):

| Row | Stub | Target |
|---|---|---|
| `GfxEngine.closeVideo` | `0x47c380` | `0x4f9320` |
| `GfxEngine.isPlayingVideo` | `0x47c390` | `0x4f9750` |
| `GfxEngine.numDisplayModes` | `0x47c440` | `0x4b9ef0` |
| `GfxEngine.currDisplayMode` | `0x47c450` | `0x4b9ea0` |
| `Sound.has3DHardware` | `0x487640` | `0x559e20` |
| `Sound.hasMixHardware` | `0x487650` | `0x559e30` |

None of the six targets is itself an `E9` stub or another table row, so the
single hop is complete for this exe. They are counted as "pre-only" in the
`native: hooked N live + M pre-only, K skipped` line and produce no result.

**The 4 skipped rows** (head starts with a relative call/branch, so
`stolen_bytes()` returns 0 and the entry is left alone). Calls to them are
invisible in the log:

| Row | VA | Head bytes | Why |
|---|---|---|---|
| `System.currentTime` | `0x47bfe0` | `e8 db 56 0d 00` | `call` first |
| `Math.randomize` | `0x47c7d0` | `e8 eb 4e 0d 00` | `call` first |
| `Math.random` | `0x47c820` | `51 e8 e2 ab 15 00` | `push ecx` then `call` inside 5 bytes |
| `Input.lastKey` | `0x47cc10` | `e8 eb a1 0d 00` | `call` first |

### 6.1 Correction to the earlier 339/6/5 figures

The W1 handoff and the first version of this document reported 336/339 hookable
and **5** skipped, counting `System.runFinalization` `0x47c070` as skipped
("only 3 bytes before `ret`"). That is not what the shipped code does: the head
is `33 C0 C3 90 90` (`xor eax,eax` / `ret` / `nop` / `nop`) — whole
instructions, no relative branch — so the compiled `stolen_bytes()` returns 5
and the row takes the normal thunk path. The copied `ret` returns into the
thunk (the `E9` appended after the copied bytes is dead), which is the correct
behaviour for a `xor eax,eax; ret` body. Nothing in `hook_native.c` treats
`ret` as a terminator. With it re-classified the figures are **343/6/4 rows**
(**340/6/4 distinct VAs**), measured as in the table above.

Remaining gaps:

- **7 `java.io.Controller` natives** are not in the static table (registrar-only;
  see §2). Until a live run proves the registrar fires, treat them as absent.
- **Native argument values** (`argc = 0`, needs the M1-02 value layout).
- **`SLR_HOOK_DT_US` is not consumed**; only `STEP_US` drives the clock.
- **`SLR_HOOK_DISABLE` tokens `com` and `time` are not checked** (only
  `natives`, `files`, `input`, `vtable`, `devwrap`), despite the field comment.
- **`FindNextFileA` logs `find:%p`** (the raw handle) as its path, which is not
  stable across runs; the parent `FindFirstFileA` pattern lookup by handle is
  the intended replacement. This is a known determinism suspect.
- **Thread interleaving**: records are atomic, but their order across threads is
  not forced; identical logs require the game's own scheduling to be identical.

## 7. Verification status

Proven statically:

- `re/hook/build.sh` builds `re/out/wine/hook/dinput8.dll` (i686 mingw, no errors).
- `re/hook/tests/run_tests.sh`: C emitter byte-identical to the Python reference
  (324-byte log), 7 records identical under the Rust reader and diff tool.
- Row classification above (343/6/4 rows = 340/6/4 distinct VAs, 0 unmapped):
  the *compiled* `lde_len()`/`stolen_bytes()` from `hook_native.c` was extracted
  verbatim into a host program and run over all 353 rows
  (`re/out/oracle/lde_crosscheck.py`), a byte-for-byte Python port agrees with
  it on all 353 rows, and capstone agrees with its instruction boundaries on
  every row (0 mismatches). See §6 for the enumerated exceptions and the
  correction to the earlier 339/6/5 count.
- Alias-thunk emission under Unicorn: 6/6 stubs jump to the decoded target with
  no fault, `ebp` preserved, `ret` returns to the caller; the old instruction
  with `edx = 0` reproduces the reported read at `0x1`.
- Publish order (ledger W4, `re/out/oracle/publish_order.py`, capstone 5.0.7 +
  Unicorn from `~/slr-oracle-venv`): the source order, the *compiled*
  `dinput8.dll` (`slr_detour`: `if (publish)` guard → slot store → `lock or
  [esp],0` → patched entry byte) and an emulated reader entering the target
  between the two operations. Declared order: reader completes
  (`eax = 0xC0FFEE`) after either operation; reversed (pre-W4) order: the reader
  transfers through the NULL slot and faults on fetch at address `0`. Every
  call site is asserted to pass the slot its hook reads (`&e->tramp`,
  `&g_reg_tramp`) or `NULL` (alias), and no call site assigns a trampoline after
  `slr_detour` returns. Re-run after W5 with the same result (the oracle's
  source anchors — the two `slr_detour` call lines and the thunk template — are
  unchanged by the move into `nat_hook_one`).
- Publish-order guard test (ledger W4, `re/hook/tests/run_tests.sh`, third
  section, no Wine and no game data): asserts `*publish = tramp;` precedes
  `t[0] = 0xE9;` in `slrhook.c`; that no line of `hook_native.c` assigns the
  result of `slr_detour` (the `= slr_detour(` pattern); that there are exactly
  three `slr_detour(` call sites; and that those sites pass `&e->tramp`,
  `&g_reg_tramp` and `NULL` respectively. Observed output (2026-10-18):

  ```text
  OK: C emitter == Python reference (324 bytes)
  OK: 7 records identical
  OK: W4 publish order (slot before patch, 3 call sites)
  OK: W5 lock span (1 lock region per entry, both detour sites and their state stores inside the locked helper)
  ```

  The guard is a source-shape check, not a re-derivation: it fails on a
  reordered `slr_detour` and on any call site that goes back to reading a
  trampoline after the call returns.
- Lock span (ledger W5): `re/hook/tests/run_tests.sh` asserts the shape of §5.5
  (one lock region per entry with the patch inside the helper, no lock call in
  the helper, state committed before each return, no `!e->hooked` re-check);
  the pre-W5 source fails it. In the built DLL the inlined helper leaves exactly
  one `EnterCriticalSection` call site (loop head) and one `LeaveCriticalSection`
  call site (loop-back) around both `_slr_detour` calls. Not exercised at
  runtime: no two passes have ever overlapped in a live run.

**Not verified — no Wine run has been made after `00c80af` or `96085b4`.** The
last live sessions were `fix1`/`sev1` (before `00c80af`) and `v1` (before W1).
Consequently neither the acceptance criterion (two identical replays from boot
to the main menu, byte-equal logs) nor even a clean boot is established.

### 7.1 The single pending step (explicitly authorized Wine only)

Everything static is done; exactly one live action remains and it is
**gated on the user's per-invocation authorization**:

```sh
cd slr-wt/m0-11 && ./re/hook/build.sh && SLR_ALLOW_WINE=1 ./re/hook/run.sh v1 90
```

Expected first-run shape:

```text
hook.txt ends with:   install: done
                      native: hooked <N> live + <M=6> pre-only, <K=4> skipped (of 353 regs, ...)
hook.slrlog:          > 8 bytes (records present)
stderr.log:           no "Unhandled" line
```

The `N live` count is over 353 *rows*, so with a clean pass it reads 343
(`343 live + 6 pre-only, 4 skipped`); a duplicate VA bumps the counter without
a second patch (§6).

**Crash-dialog hazard.** Both live sessions so far raised a user-visible Wine
"serious problem" error box even though `run.sh` sets `winedbg=d` and the
pristine prefix has `HKLM\Software\Wine\AeDebug\Auto=0`; those measures are not
a guarantee that a GUI dialog cannot appear. An authorized run is therefore not
fully unattended-safe: expect that a modal Wine box may need a human to dismiss
it, and that `run.sh`'s `perl alarm` timeout only bounds the launched `wine`
process, not a human. Keep the prefix's `Auto=0`; do not remove it. Wine must
not be relaunched inside the same authorization — `run.sh`
launches exactly once and does not retry; a crash is evidence to keep and read
from `stderr.log`, not a reason for another launch.

**Acceptance criterion (the ticket's, unchanged):** two runs with identical
environment produce byte-equal call logs from boot to the main menu:

```sh
SLR_ALLOW_WINE=1 ./re/hook/run.sh a1 90   # separately authorized
SLR_ALLOW_WINE=1 ./re/hook/run.sh a2 90   # separately authorized
cmp re/out/wine/runs/a1/hook.slrlog re/out/wine/runs/a2/hook.slrlog
```

`cmp` silent (exit 0) passes. On divergence, the reporter is:

```sh
cargo run -p slr-calllog --bin slr-calllog-diff -- \
  re/out/wine/runs/a1/hook.slrlog re/out/wine/runs/a2/hook.slrlog
```

Known divergence suspects (§6 gaps): `FindNextFileA` logs the raw handle
(`find:%p`), and the order of records from different threads is not forced.

## 8. Files

| File | Role |
|---|---|
| `slrhook.c` | config, log sink, exec pool, IAT patch, inline detour, import trace, entry stub, watchdog |
| `hook.h` | shared config/API |
| `slrlog.h` | header-only SLRLOG v1 emitter (byte-compatible with `slr-calllog`) |
| `hook_native.c` | registrar detour, per-native thunks, result typing, gap handling |
| `native_table.h` | generated 353-row static table (class, name, signature, impl VA) |
| `hook_com.c` | D3D9/DirectSound vtable thunks + creation entry points |
| `hook_file.c` | file-I/O hooks and handle→path table |
| `hook_input.c` | keyboard/mouse hooks, DirectInput8 wrapping, script replay, `MessageBoxA` |
| `hook_time.c` | deterministic virtual clock |
| `stubs.c` | `DirectInput8Create` export forwarding |
| `gen_com.py`, `gen_native.py` | generators for `com_tables.h` and `native_table.h` |
| `build.sh`, `run.sh`, `tests/` | build, gated launch harness, data-free emitter tests |
