# Strict headless original startup

This is an incomplete M0-12 / issue #13 oracle prefix. It executes the original
build 606 `.bak.exe` at PE entry `0x5514c0` through the existing Unicorn loader.
The input SHA-256 is pinned. No Wine, executable patch, CRT replacement, game
function replacement, or native-return fixture is used.

```sh
~/slr-oracle-venv/bin/python re/headless/startup.py --limit 500000
~/slr-oracle-venv/bin/python -m unittest discover -s re/headless -v
```

The test suite needs the local executable and stock files; an absent executable
skips the suite. API regressions call platform handlers with Unicorn memory,
while prefix and worker tests execute original x86. These are local tests.

The executable and `stock/` must already exist locally; neither is distributed.
All CLI output is restricted to this worktree's ignored `re/out/headless/`.
Generated game files remain in an in-memory overlay. Stock files are read only.
Source paths are case insensitive within the stock tree; traversal, absolute
paths outside logical `C:\game`, and escaping symlinks stop the run.

The current C-only scenario reaches real `WinMain` (`0x58c700`) and stops at the
CD insertion dialog request after 466,336 startup counter increments. Two complete diagnostic
JSON prefixes compare byte for byte. This is **not** boot-to-MainMenu acceptance:
`status` is always `incomplete`, `main_menu_reached` is always false, and no D3D,
DirectSound, DirectInput, or native call stream has been established. JSON is a
platform diagnostic sidecar, not an SLRLOG stream or a ledger differential suite.

## Explicit platform scenario

- NT 5.1 build 2600; CP1252; one virtual CPU; one fixed C: drive; an empty desktop.
  The physical-memory report is 512 MiB. These are declared scenario inputs,
  not observations about the host or claims of full Windows XP emulation.
- Time starts at 2003-01-01 UTC. Each 1,000 increments of the startup instruction
  counter advances the virtual clock by one millisecond; the performance counter
  uses 1 MHz. Intercepted platform thunks each increment this counter once, so
  it does not count only instructions executed from the original executable.
- Threads execute original x86 code against shared memory. A deterministic
  round-robin scheduler switches every 1,000 instructions or on a blocking wait.
  Each thread has its own stack, saved CPU/FPU context, TEB, TLS, COM depth, and
  last-error value. Semaphore counts, recursive mutex ownership, and critical
  section ownership determine blocking and wakeup. If no thread is runnable, execution stops
  explicitly. Finite nonzero waits, arbitrary thread flags, and named semaphores
  are unsupported, rather than returning invented successes.
  Named mutexes retain ownership across alias handles and are destroyed after
  the last handle closes. Closing a handle with a pending wait stops explicitly;
  the model does not assign a result to Win32's undefined pending-close case.
- Heap allocation uses the existing bounded 64 MiB emulator heap. File reads are
  at most 16 MiB per call, files and the write overlay are bounded to 64 MiB, and
  API/scheduler events default to 10,000; `--event-limit` explicitly selects a
  budget from 1 to 100,000. The instruction limit must be positive.
  The overlay budget includes untouched stock bytes materialized by a first
  write. Zero-byte writes preserve the file and position without materializing
  a stock copy into the overlay.
- All implemented imports have an exact DLL/export match and explicit 32-bit
  `stdcall` argument count in `Startup.apis`. Unknown imports/exports stop by name
  before the Oracle's permissive default thunk can execute. Entering a thunk at
  an unexpected address also stops. Dialogs are recorded and stop before a reply.
- NLS is a bounded CP1252 scenario for CRT initialization, using Unicode simple
  case/category operations and explicit Microsoft CP1252 exceptions. It is not a
  full Windows NLS oracle. Unhandled code pages, combining text, and unknown
  CP1252 best-fit inputs stop explicitly; the CRT's U+039C default-character
  probe is supported. NLS/table parity beyond this startup is unproven.

The ABI reference is the installed MinGW-w64 14 headers (`heapapi.h`,
`synchapi.h`, `processthreadsapi.h`, `errhandlingapi.h`, `processenv.h`,
`sysinfoapi.h`, `fileapi.h`, `libloaderapi.h`, `winnls.h`, `objbase.h`). File
creation and synchronous position/count semantics follow Microsoft's
[CreateFileA](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea)
and [WriteFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile)
contracts. CP1252 conversion exceptions follow Microsoft's
[WindowsBestFit mapping](https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WindowsBestFit/bestfit1252.txt).
`SetHandleCount` is the NT no-op returning the supplied count, as in the
[Wine implementation](https://github.com/wine-mirror/wine/blob/master/dlls/kernel32/file.c).
Named mutex lifetime follows [CreateMutexA](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexa);
the pending-close stop follows [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject).

## Remaining media boundary

The original `0x401480` scans A:–Z: for `DRIVE_CDROM`. It uses `mciSendCommandA`
to open a CD-audio device, set milliseconds, query the track count, each track
length, and total length, then close. The total must be 4,470,000–4,830,000 ms;
the count and interior track lengths must match the executable's decrypted
check inputs. The available MSI/CAB installer tree provides no observed CD TOC.
The default harness exposes no CD drive and records the original failure.

## Optional hypothetical MCI scenario

An explicitly supplied JSON config can describe a simulated CD-audio device:

```sh
~/slr-oracle-venv/bin/python re/headless/startup.py \
  --media-profile re/out/headless/hypothetical-cd-toc.json \
  --event-limit 20000 --limit 5000000 \
  --out re/out/headless/media-startup.json
```

The config is external and bounded to 16 KiB. No game-derived TOC/profile vector
is committed. A small independent API-test example has these fields:

```json
{"label":"hypothetical-example","drive":"D","track_lengths_ms":[10,20,30],"total_ms":60}
```

`label` must start with `hypothetical-` and contain at most 80 ASCII letters,
digits, dots, underscores, or hyphens. `drive` is one uppercase non-C drive
letter. There must be 1–128 positive integer track lengths; `total_ms` must be
at least their sum and at most `2147483647`. Validated inputs are immutable.
Unknown fields and duplicate JSON keys are rejected. Optional `provenance` is
bounded explanatory text. Optional `failures` maps `open`, `set`, `close`,
`status:count`, `status:total`, or `status:track:N` to a declared MCI error code
from 256 to 1023 for controlled failure scenarios.

The profile enables only the exact `winmm.dll!mciSendCommandA` import with four
32-bit stdcall arguments. The adapter supports the original's CD-audio OPEN
(`0x803`, flags `0x3302`, type `516`, element `X:`), SET to milliseconds
(`0x80d`, flags `0x400`), STATUS count/track length/total length (`0x814`, flags
`0x100` or `0x110`), and CLOSE (`0x804`, zero flags and pointer). OPEN writes
only `wDeviceID`; STATUS writes only `dwReturn`. API failures preserve output
memory. Device ownership, time format, and close state are tracked; at most
16 devices may be open. Unsupported commands, flags, callbacks, and time
formats stop explicitly. Invalid device and track IDs return MCI errors.
ABI and status meanings follow the installed `mciapi.h` and Microsoft's
[MCI_OPEN](https://learn.microsoft.com/en-us/windows/win32/multimedia/mci-open)
and [MCI_STATUS](https://learn.microsoft.com/en-us/windows/win32/multimedia/mci-status)
contracts.

The diagnostic records the scenario label, canonical config hash, and
`physical_media_observed: false`. It mounts no CD filesystem and supplies no
D3D success model. Original machine code performs the CD predicate. A local
declared scenario has advanced its startup instruction counter to 3,848,705,
where it stops at `d3d9.dll!Direct3DCreate9(31)`. MainMenu remains unobserved.

`FindFirstFileA` additionally supports bounded `*.extension` patterns with
1–16 alphanumeric extension characters. Searches sort case-insensitively,
reject more than 4,096 real stock/overlay entries before adding synthetic dot
entries, and allow at most 128 active search handles.

Local original-code tests load ignored `re/out/headless/hypothetical-cd-toc.json`
and `cd-inputs.json` captures. They exercise predicate refusal paths and repeat
the declared scenario prefix; absent local captures skip those tests explicitly.
These tests and the pure synthetic API tests do not establish physical-disc
properties or boot-to-MainMenu acceptance.

## Optional hypothetical IDirect3D9 enumerator

`--graphics-profile re/out/headless/d3d/scenario.json` additionally enables the
exact `d3d9.dll!Direct3DCreate9` import. It is independent of the media option;
neither option changes the C-only default. The JSON config is bounded to 64 KiB,
rejects duplicate keys and unknown fields, and becomes immutable after validation.
An independent small API-test example is:

```json
{"label":"hypothetical-example","creation_sdk_version":31,"creation_result":"interface","adapter_count":1,"mode_lists":{"0:22":[[320,240,0,22]]},"display_modes":{"0":[320,240,0,22]},"type_checks":{"0,1,22,22,0":0}}
```

These values declare a scenario; they do not describe the host GPU. `label` has
the same hypothetical-name syntax as the media profile. `creation_sdk_version`
is explicitly 31 or 32, and `creation_result` is `interface` or `null`. The actual
original call supplies **31**; the installed SDK header macro is **32**. The
interface scenario assumes a non-null return for its declared input. It does
not establish that any physical Direct3D9 runtime accepts version 31.

`adapter_count` is 1–4. `mode_lists` has at most 12 `adapter:format` keys, for
adapters below that count and formats 22, 23, or 24. Each list has 0–32 four-word
`[width,height,refresh_hz,format]` records, with width/height 1–8192, refresh 0–1000,
and the format matching its key. The list length declares the mode count. An
optional `display_modes` map supplies a current mode for each declared adapter.
Optional `type_checks` has at most 128 exact comma-separated
`adapter,device_type,display_format,backbuffer_format,windowed` keys. Device types
are 1–3, display formats 22–24, backbuffer formats 21–25, and windowed is 0 or 1.
Each value explicitly supplies `D3D_OK` (0), `D3DERR_NOTAVAILABLE` (2289436778), or
`D3DERR_INVALIDCALL` (2289436780). Fullscreen success requires matching formats,
allowing the documented alpha backbuffer variants 22/21 and 24/25. Optional
`provenance` is at most 1,024 characters. Missing query entries stop by name;
there is no default mode list or successful device check.

The interface has all 17 SDK vtable slots and validates the explicit stack
`this` pointer. Implemented methods are `GetAdapterCount` (slot 4),
`GetAdapterModeCount` (6), `EnumAdapterModes` (7), `GetAdapterDisplayMode` (8), and
`CheckDeviceType` (9). Each uses 32-bit stdcall, including `this` on the stack.
Mode outputs write exactly the 16-byte `D3DDISPLAYMODE`; successful writes and
HRESULTs are recorded separately. Out-of-range adapters/indices and null mode
outputs have explicit refusal paths. An unmapped output stops with a named
error. At most four interfaces may exist. All other methods, including
`QueryInterface`, `AddRef`, `Release`, `GetDeviceCaps`, and `CreateDevice`, stop
before supplying a result. COM methods are not exposed as DLL exports.

ABI and format rules follow the installed MinGW-w64 14 `d3d9.h` and
`d3d9types.h`, and Microsoft's
[GetAdapterModeCount](https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3d9-getadaptermodecount),
[EnumAdapterModes](https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3d9-enumadaptermodes),
[GetAdapterDisplayMode](https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3d9-getadapterdisplaymode),
[CheckDeviceType](https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3d9-checkdevicetype),
and [D3DDISPLAYMODE](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3ddisplaymode)
contracts. This is a bounded enumerator, without window, cursor, caps, or device
creation models.

The trace records the config hash, declared and observed creation version,
`physical_gpu_observed: false`, and `device_created: false`. With local ignored
media and graphics configs, actual original constructor/enumeration code reaches
`user32.dll!LoadCursorA` at return site `0x4b7f44`, requesting cursor resource 116.
A declared single-mode scenario reaches an instruction counter of 3,849,360;
two complete diagnostic prefixes compare byte for byte. The original filters
its three format lists into two 640×480 records, at 16 and 32 bits. Controlled
refused device-type checks and empty mode lists yield zero accepted records.
A declared null creation result follows the original's null dereference at
`0x4b897c`; no recovery is invented. No actual `CreateDevice` call is reached in
these prefixes. Derived configs, expected mode records, and traces remain in
ignored `re/out/headless/d3d/`; original-code tests skip explicitly without them.
Every trace remains `incomplete` with `main_menu_reached: false`.

## Optional cursor/resource/window prefix

`--desktop-profile re/out/headless/window/desktop-profile.json` enables a bounded
hypothetical desktop independently of the media and graphics options. An example
of its explicit inputs is:

```json
{"label":"hypothetical-example","resource_language":1033,"cursor_size":[32,32],"icon_size":[32,32],"system_arrow_available":true,"black_brush_available":true}
```

The config is at most 8 KiB, has no duplicate or unknown keys, and is immutable
once validated. The hypothetical label has the same syntax as the other
profiles. `resource_language` is an integer from 0 to 65535; each image size has
two integer dimensions from 1 to 256. The two availability fields are explicit
booleans. Optional `provenance` is bounded to 1,024 characters. The trace records
these assumptions and the canonical config hash with
`physical_desktop_observed: false`. No host window or cursor is opened.

The exact DLL/export and 32-bit stdcall boundaries are `user32.dll!LoadCursorA`
(2 arguments), `LoadIconA` (2), `RegisterClassA` (1), `CreateWindowExA` (12), and
`gdi32.dll!GetStockObject` (1). Supported predefined objects are the system arrow
(`IDC_ARROW`, 32512) and black brush (4); their availability is the scenario
input, and successful lookups return shared symbolic handles. Other predefined
objects stop explicitly.

Module image lookups use the pinned executable's actual read-only PE resource
bytes. The requested language is used if present, otherwise neutral language 0;
other language selection is unmodeled. Numeric RT_GROUP_CURSOR/RT_CURSOR and
RT_GROUP_ICON/RT_ICON relationships are checked, with single-entry, unscaled,
40-byte BI_RGB bitmap headers, supported palette/bitmap lengths, and valid cursor
hotspots. Group/payload SHA-256 values, selected resource IDs, language,
dimensions, and bit depth are recorded. This validates resource identity and
layout; it supplies no pixel renderer. Named resources, scaling, multiple image
choices, and other encodings stop. Missing group lookups return null with the
scenario's resource-name-not-found error 1814. The resource catalog allows at
most 512 payloads totaling 1 MiB; at most 64 symbolic objects may be allocated.

Class registration reads the 40-byte `WNDCLASSA`, supports ASCII names up to 128
characters, style/extra bytes zero, the original module instance, known image
handles or null, and no menu or an empty menu string. Names are case-insensitive;
a duplicate returns zero/error 1410. At most 16 classes may be registered.
Creation supports only parentless, menuless popup styles `0x80000000` and
`0x90000000`, extended styles zero or `0x8`, coordinates within ±8192, and sizes
0–8192. Unknown classes return zero/error 1407. Other shapes stop explicitly;
there are at most 16 creation attempts.

A known popup receives a provisional symbolic HWND. The shim marshals the
48-byte `CREATESTRUCTA` and transfers execution to its registered x86 WndProc for
`WM_NCCREATE` (`0x81`, zero wParam). It uses the current thread's stack and a
platform return trampoline; it does not hook or replace the WndProc. There is
at most one pending callback per thread and four overall. Dispatch and callback
returns share the existing bounded event/instruction budgets. The callback
contract follows the local i686 `winuser.h` structures and Microsoft's
[WM_NCCREATE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccreate)
reference. Both a nonzero callback return and a refusal stop at a named next
creation phase, before unmodeled later messages or cleanup. No completed HWND
result is supplied by this prefix. `DefWindowProcA` remains unmodeled unless the
optional creation continuation below is explicitly configured.

With the local declared media/graphics/desktop inputs, actual original code
loads ten cursor groups starting with **116 (`0x74`)**, icon 107, the arrow and
brush, then registers `INVICTUS` with WndProc `0x4b8000`. Its popup request has
extended style `0x8`, style `0x90000000`, and zero initial size. The original
WndProc executes for `WM_NCCREATE` and delegates to `user32.dll!DefWindowProcA`,
where the prefix stops at return site `0x4b829c`, startup counter **3,849,524**,
with one callback pending. Two complete prefixes compare byte for byte. No
window creation has returned, and no device/caps, physical desktop, or MainMenu
acceptance is claimed. Default C-only, MCI-only, and graphics-only inputs retain
their prior diagnostic traces. Original resource/prefix captures stay ignored
under `re/out/headless/window/`; their local tests skip explicitly if absent.

## Optional declared creation messages

The existing desktop JSON may additionally contain:

```json
"creation": {"default_nccreate_result":1,"after_nccreate":"nccalcsize_rect"}
```

This nested object requires these two fields and permits the optional
`nccalcsize` and `wm_create` objects described below. The result is the integer
0 or 1; `after_nccreate` is `stop` or `nccalcsize_rect`. These are explicit
hypothetical platform inputs, recorded in scenario metadata and its config hash.
They are not observations of Windows default processing. Omitting `creation`
preserves the earlier strict `DefWindowProcA` boundary and complete trace.

The option enables only exact `user32.dll!DefWindowProcA`, with four 32-bit
stdcall arguments. It accepts only the owned active callback's exact parameters
and unchanged `CREATESTRUCTA`, and supplies the declared result only for
`WM_NCCREATE`. The result flows through the registered x86 WndProc. A zero
callback result stops before refusal cleanup; nonzero with `stop` stops before
another message. Neither returns a completed HWND.

With nonzero WM_NCCREATE and `nccalcsize_rect`, the shim sends the same WndProc
`WM_NCCALCSIZE` (`0x83`), wParam zero, and a 16-byte RECT containing the requested
popup's proposed screen rectangle. This is a declared delivery scenario, not a
complete Windows creation-message sequence. The rectangle follows Microsoft's
[WM_NCCALCSIZE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize)
FALSE-wParam shape. Callback arguments, input RECT words and any returned RECT
are recorded; a callback return stops before further creation processing.
[DefWindowProcA](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-defwindowproca)
has message-dependent results. Without the optional identity scenario below,
default processing of WM_NCCALCSIZE, other
messages and other parameter shapes is unmodeled and stops before supplying a
result. The callback bridge checks four-word stdcall stack cleanup and retains
the existing per-thread, event, instruction and creation-attempt bounds.

Local original-code scenarios execute the original WndProc at `0x4b8000` with
both declared NCCREATE results. Those stop after the actual callback return at
startup counter **3,849,530**. Explicit next-RECT delivery reaches its real
WM_NCCALCSIZE delegation to DefWindowProcA, return site `0x4b829c`, at counter
**3,849,551**, and stops with that callback pending. Each of the three scenarios
repeats identically; expected inputs and receipts stay ignored under
`re/out/headless/window-next/`. No HWND success, later message handling, D3D
device/caps model, physical desktop or MainMenu acceptance is established.

## Optional identity client rectangle

The creation object may additionally contain this explicit scenario:

```json
"nccalcsize": {"rect_output":"identity","default_result":0,"after":"wm_create"}
```

All three nested fields are required. `rect_output` must be `identity`,
`default_result` must be integer zero, and `after` is `stop` or `wm_create`.
The parent `after_nccreate` must be `nccalcsize_rect`. Unknown fields, duplicate
keys, nulls, boolean results and unsupported choices are rejected. Inputs are
immutable and included in the existing hypothetical desktop metadata/config
hash. Omitting this object preserves every earlier diagnostic profile.

Only the owned active WM_NCCALCSIZE callback with its exact false-wParam RECT
arguments is eligible. The adapter records its four input words, writes those
same four words back, records the 16-byte output span, and returns the declared
zero through the original x86 WndProc. Identity output is an explicit zero-inset
popup assumption; it is not an observed Windows default rectangle calculation.
With `stop`, processing stops after that actual callback return. With
`wm_create`, only an actual zero callback return delivers WM_CREATE (`0x1`),
unused wParam zero, and the original CREATESTRUCT pointer to the same WndProc.
This is a declared delivery scenario, not the complete Windows creation sequence.

The [WM_CREATE contract](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-create)
defines CREATESTRUCT lParam and an application return of zero to continue or
minus one to refuse. Default processing of that message remains unmodeled:
the adapter records its input structure and stops before supplying a result,
unless the optional CREATE scenario below is explicitly configured.
Without the optional CREATE continuation, an application that handles it itself
also stops after its actual return, before completed HWND or refusal cleanup.
Other default messages remain strict
unknown boundaries; no generic success result is used.

Local original-code controlled scenarios return from WM_NCCALCSIZE at startup
counter **3,849,557**, or reach the original WM_CREATE delegation at counter
**3,849,578**, return site `0x4b829c`, with that callback pending. Both complete
traces repeat identically. Their independent scratch expectations and explicit
profile JSON stay ignored under `re/out/headless/window-calc/`. No window
creation result, physical desktop, full Windows message sequence, device/caps
or MainMenu acceptance is established.

## Optional declared CREATE result and visible notification

The creation object may additionally contain:

```json
"wm_create": {"default_result":0,"after":"wm_showwindow"}
```

Both fields are required. The signed `default_result` is integer zero or minus
one; `after` is `stop` or `wm_showwindow`. The parent `nccalcsize.after` must be
`wm_create`. Unknown/missing fields, nulls, boolean results and unsupported choices
are rejected. The validated inputs are immutable and recorded in the existing
hypothetical scenario metadata/config hash. Omitting this object preserves the
earlier strict WM_CREATE boundary and its complete diagnostic trace.

The adapter accepts only the owned active WM_CREATE callback's exact arguments
and unchanged CREATESTRUCT. It supplies the explicitly declared signed result
as raw 32-bit bits through the original x86 WndProc; minus one is recorded as
`4294967295` (`0xffffffff`). With `stop`, the actual callback return stops before
any Win32 creation result. Refusal always stops before unsupported cleanup and
before a NULL API return. No provisional handle is promoted to completed HWND.

With `wm_showwindow`, only an actual zero CREATE callback return and an original
WS_VISIBLE style can deliver WM_SHOWWINDOW (`0x18`), wParam one, lParam zero to
the same WndProc. Requesting that visible path for an invisible popup stops
explicitly. Microsoft's [WM_SHOWWINDOW](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-showwindow)
reference documents a notification after creation and before display for
WS_VISIBLE windows. This adapter's exact delivery parameters remain declared
scenario inputs; a full Windows message sequence is unproven. Default processing
of that notification remains unknown and stops before supplying a result unless
the optional SHOW scenario below is configured. Without that optional scenario,
an application-handled return also stops before further creation processing.

The local original-code scenarios record actual CREATE zero/refusal returns at
startup counter **3,849,584**. The explicit visible notification executes the
original WndProc's state updates and stops at its DefWindowProcA delegation,
return site `0x4b829c`, at counter **3,849,620**. In the local declared graphics
scenario, original code changes the four words at `0x649588` from `[0,0,0,0]`
to `[0,0,800,600]`. The shim does not write those game-state fields. For each
profile, two independent runs produce identical full JSON. Local expected state,
raw callback inputs, profiles and receipts remain ignored under
`re/out/headless/window-visible/`. No window creation completion, physical
desktop, full notification sequence, device/caps or MainMenu acceptance is
established.

## Optional SHOW result and finite declared transcript

The creation object may additionally contain:

```json
"wm_showwindow": {
  "default_result":0, "expected_callback_result":0, "after":"transcript",
  "transcript":{"messages":[],"api_return":"owned_provisional_handle"}
}
```

The parent `wm_create.after` must be `wm_showwindow`. All displayed fields are
required. `after:"stop"` instead forbids the transcript object and stops after
the actual SHOW callback return, before any CreateWindowExA result. Both
results must be integer zero. One is the explicit platform default assumption;
the other is the expected actual application result. The trace records whether
the original WndProc consumed the default. An application-handled result still
runs through the original callback; a mismatched result stops before advancing.
Actual CREATE refusal stops before SHOW or transcript delivery, regardless of
unused continuation inputs. The actual CREATE result also remains authoritative
when application handling does not consume its configured default.

The optional transcript is a finite supplied scenario with at most eight extra
messages. Each requires `message`, raw 32-bit `wparam`, `default_result:0`,
`expected_callback_result:0`, and raw 32-bit `lparam`. Only recovered WM_MOVE,
WM_SIZE, WM_ACTIVATEAPP, WM_ACTIVATE, WM_SETFOCUS and WM_KILLFOCUS are supported
with that scalar shape. WM_WINDOWPOSCHANGING and WM_WINDOWPOSCHANGED instead
require `wparam:0`, `windowpos:["topmost",x,y,cx,cy,flags]` (or `"top"`), and
`default_memory:"identity"`. Coordinates/dimensions are bounded signed words;
flags are bounded 16-bit inputs. The adapter materializes and records the exact
seven-word WINDOWPOS, including the same owned HWND. Changed structure words
stop before another result. Unsupported fields, IDs, pointer shapes, repeated
default calls, and unknown later imports stop explicitly. No generic default
zero or additional notification is supplied.

Every configured notification executes the registered original WndProc. Its
known viewport/activation stores run as original instructions; the adapter does
not inject game-state writes. This order and each default result are hypothetical
inputs. Default processing that would generate other messages is not modeled,
and messages without direct game writes are not proven safe to omit from a
physical Windows sequence. Metadata records those limits, immutable configuration
and its canonical hash. Omitting the option preserves all earlier diagnostic
profiles and their complete outputs.

The transcript must end with explicit `api_return:"stop"` or
`"owned_provisional_handle"`. The latter supplies a declared platform result
only for the exact suspended CreateWindowExA call: same owner, thunk, stack,
return site, twelve arguments, CREATESTRUCT and active callback context.
Every expected actual callback result must match before that one-shot return.
The temporary stdcall adapter is removed immediately. The trace records the raw
supplied result and `declared_api_return_supplied` state; it does not establish
a completed physical HWND. Original code then executes until the next unknown
boundary, currently ShowWindow with the supplied handle and SW_HIDE.

Independent exploratory expectations and repeated original-executable captures
stay ignored under `re/out/headless/window-transcript-native/`. All scenarios
remain incomplete, with physical desktop observation and Windows notification
sequence fidelity unestablished and MainMenu false. No ShowWindow success,
later default processing, D3D device/caps model or boot acceptance is supplied.
