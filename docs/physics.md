# Physics loop of the original (M3-01)

Ticket: issue #42 (plan id M3-01), part of spec #1. Reference binary: `StreetLegal_Redline.bak.exe`
(build 606, SHA-256 `7d77466a...e8edde`; image base `0x400000`; byte-identical to `stock/`'s `.bak.exe`,
and the stock `.exe` differs only inside the CD check). All addresses below are virtual addresses in
that image. `FUN_xxxxxxxx` names are Ghidra's, kept so the text can be grepped against
`re/out/all_pseudo.c` / `re/out/native_pseudo.c` (git-ignored, see Method).

Provenance tags on every claim:

- **[oracle: name]** a Unicorn function-oracle check in `re/oracle/physics_trace.py` calls the real exe
  code and asserts the stated value (`physics_trace.py all` = 7/7 pass; reproduced at the end).
- **[static]** read from decompiled/disassembled code at the cited address (Ghidra pseudocode, capstone
  listing, or a cross-reference query). Not executed.
- **[inferred]** a role deduced from names, offsets and data flow; the reasoning is given.
- **[not traced]** known to exist, not understood. Collected in "What could not be traced".

"Oracle" here always means the **Unicorn single-function oracle**; no whole-game run exists yet
(#12/#13), so nothing below was observed in a running game.

## 1. One-page summary

| Question | Answer | Where |
|---|---|---|
| Which thread? | The main thread, inside the frame loop `FUN_00428960`. No second physics thread exists; the only threads are DirectSound/audio and FMV timers. | 3, 4 |
| Control word | Game float code runs at x87 PC=24, RC=nearest (`0x007F`), as ADR 0007 states: D3D9 device created with flags `0x84` (fallback `0x24`), never `D3DCREATE_FPU_PRESERVE (0x2)`; no `FLDCW` exists in game code (all sites are in the linked libraries/CRT, sampled sites only change rounding). The 24-bit setting itself is not observed at run time. | 4 |
| Step | `FUN_004a5190` = `World::step(dt)` on the static world object at `0x6409c4`; fixed nominal step **0.02 s**, minimum **1e-4 s**. | 6 |
| Substeps | `FUN_00428450` splits the last frame's wall time (x time scale, clamped to **0.1 s**) into chunks of at most 0.02 s; a remainder in `[step, 2*step]` is halved. At most 6 steps per frame. | 5 |
| Solver loop | Each step runs up to **20** trial iterations of integrate / detect / resolve. | 6.2 |
| Integrator | `FUN_004a7c10`: **RK4** on a double-buffered 0xd4-byte body state (explicit Euler only for kinematic bodies, flag `0x100`). | 7 |
| Car model | Native class `car` (factory `0x442500`, vtable `0x5f0f18`): `control` (slot 11, once per frame), `drive` (slot 12, once per step), `animate` (slot 13, once per frame). Tyres = Pacejka magic formula (`0x43ba80`/`0x43bb30`). | 8, 9 |
| cfg handler consumers | `enginepower` (`0x443f90`) writes vehicle-parameter offsets `0xd0/0xd4/0xe0/0xe4/0xe8/0xf0/0xf4`; consumers are `0x454500` (drive), `0x448430`, `0x43c930`, `0x4518c0`. | 10 |

## 2. Method and why it is checkable

Three tools, all committed (`re/`), no derived output committed:

1. **Ghidra headless, function creation** (`re/ghidra_scripts/CreateNativeFunctions.java`). The original
   export (`ExportAll.java`) had no pseudocode for 258 of the 353 native-method implementations because
   they and every vtable method are reachable only through data pointers, so Ghidra never made functions
   there. The new script creates a function at every native entry in `re/out/native_api.tsv` and at every
   code address referenced by a `.rdata`/`.data` relocation that lies outside all existing functions
   (vtable slots, cfg-handler tables), then decompiles them. Result: **255 + 1,274 + 18 = 1,547 new
   functions** (255 native entries covering the 258 LAB_ rows, 1,274 vtable/handler targets, 18 callees
   found on the way); every one of the 353 native rows now has pseudocode, 0 decompile failures. The
   project is kept outside the repo (`~/slr-ghidra-m3`); `QueryExe.java` answers xrefs and mnemonic
   queries against it (`-process -noanalysis`). The choice between this, capstone-only and REA was made
   with `jev_decide` on stated priorities (checkable claims, reproducible by committed scripts, low
   effort): selected Ghidra headless creation, probability 1.0; REA `open_binary` works but its
   `procedure_pseudo_code` had no function at the LAB_ addresses either, and capstone gives no
   cross-references.
2. **`re/oracle/exe_xrefs.py`**: indexes the exe's base-relocation table, so every absolute address
   operand and data pointer is a known site. Answers "which vtable slot / code site holds address X",
   lists a vtable's slots, and finds code xrefs to a string (`str =gravity`). Ghidra's call graph misses
   vtable dispatch; this is what found the `car` vtable and the class registry.
3. **`re/oracle/physics_trace.py`**: the Unicorn checks cited as [oracle: ...].

Re-creating the working data (all git-ignored):

```sh
M=/Users/nicolas/Documents/github/games/street-legal
G=/opt/homebrew/Cellar/ghidra/12.1.4/libexec/support/analyzeHeadless
cd re && $G ~/slr-ghidra-m3 slr -import $M/reference/raw/StreetLegal_Redline.bak.exe -overwrite \
  -scriptPath $PWD/ghidra_scripts -postScript CreateNativeFunctions.java $PWD/out $PWD/out/native_api.tsv vtables
# writes re/out/native_pseudo.c + native_created.txt (about 1 minute). Existing re/out/all_pseudo.c is still needed.
$G ~/slr-ghidra-m3 slr -process StreetLegal_Redline.bak.exe -noanalysis -readOnly \
  -scriptPath $PWD/ghidra_scripts -postScript QueryExe.java xrefs 0x4a5190
~/slr-oracle-venv/bin/python re/oracle/exe_xrefs.py to 0x454500      # vtable slot / code sites
~/slr-oracle-venv/bin/python re/oracle/physics_trace.py all           # the oracle checks
```

Ghidra function boundaries for the created functions are Ghidra's guess (sizes in `native_created.txt`);
sizes quoted below for them are bytes of Ghidra's body, not a proof of the true extent.

## 3. Program structure around the loop

[static] unless marked.

- `entry` `0x5514c0` -> CRT start -> `0x551430` -> `FUN_0058c700` (the `WinMain`-equivalent; prints
  `SL2 build %d` with build `0x25e` = 606, loads `system.rpk`, reads `video_*` config, creates the audio
  threads via `FUN_005598c0`, calls the game manager init `FUN_00427980` at `0x58c7a9` with
  ECX = `0x636338`, then the frame loop `FUN_00428960` at `0x58c978`).
- **Game manager `M` = static object `0x636338`.** `FUN_00427980` initialises it: time scale `M+0x110 = 1.0`,
  the lists below, the world constructor `FUN_004a5580` (ECX = `[0x617650]`), and the config globals
  (`FUN_00427bf0`). Its destructor `FUN_004282c0` shows the list/counter pairs:

  | list head | count | used by | role |
  |---|---|---|---|
  | `M+0x08` | `M+0xc4` | `FUN_00428450` | objects receiving `control(dt)` |
  | `M+0x24` | `M+0xc8` | `FUN_00426f60` | objects receiving `drive(dt)` |
  | `M+0x40` | `M+0xcc` | `FUN_00426fd0` | objects receiving `animate(dt)` |
  | `M+0x5c`, `M+0x78` | `M+0xd0`, `M+0xd4` | `FUN_004277f0` | deferred deletion |
  | `M+0x94` | `M+0xd8` | `FUN_00427160` | timers (`GameType.addTimer`) |

- **Physics world `W` = static object `0x6409c4`**, reached everywhere through the pointer cell
  `PTR_DAT_00617650` (`0x617650` holds `0x6409c4`) [oracle: xhelp]. Fields used by the loop: `+0x00` sim
  time, `+0x04` carried accumulator, `+0x08` nominal step, `+0x0c` last step, `+0x10` iteration counter,
  `+0x104..0x10c` gravity vector, `+0x118` contact list, `+0x1ec` solver flag. `System.simTime()`
  (native `FUN_0047bfd0`) returns `W+0x00`.
- **Class registry** `0x60c848..0x60c8b8`: pairs (name, factory): `ground 0x4631f0`, `camera 0x437ca0`,
  `object 0x4643e0`, `car 0x442500`, `part 0x46b330`, `trigger 0x47b520`, `player 0x477bc0`,
  `button 0x436f30`, `cursor 0x45fd00`, `bot 0x42d310`, `road 0x42c1c0`, `walk 0x42ced0`,
  `trlight 0x42c570`, `ped 0x42b1e0`, `building 0x436ab0`. The `car` entry's factory sets vtable
  `0x5f0f18` and allocates a 0x2e98-byte object [oracle: xhelp checks the registry slot and vtable
  slots]. Script-driven objects (no native class, `obj+0x4c == 0`) are called through the script method
  of the same name instead (`FUN_00425a90(obj, "control" | "drive" | "animate")`, strings at
  `0x60cf38`, `0x60cadc`, `0x60cae4`).

## 4. Thread and control word

**Thread [static].** `FUN_00428960` (called once from `FUN_0058c700`) is the only caller chain to the
step: Ghidra xref query `4a5190 <- 428450 (0x42859a), 428790 (0x42892b)`;
`428450 <- 428960 (0x428ab7)`; `428960 <- 58c700 (0x58c978)`; `58c700 <- 551430 (0x5514a8)`. `FUN_00428790`
has no references at all (dead single-step variant). Thread-creating imports are used only at `0x554e10`
and `0x5598c0` (`CreateThread`, DirectSound code: `s_DSBuffers...` string), `0x55a1c0` (`timeSetEvent`,
audio, reached from `0x559ed0`), and `0x5d4fa0` (a `timeSetEvent` wrapper in the linked ActiveMovie/video
library, string `AMUnblock`). The script scheduler (`Thread.start` natives, `FUN_00418d10`) is cooperative
and runs on the main thread inside the same loop. So physics runs on the main (window/device) thread.

**Control word [static + documented D3D9 behaviour].** The device is created in `FUN_004b9180` with
behaviour flags `0x84` (`D3DCREATE_MIXED_VERTEXPROCESSING | D3DCREATE_MULTITHREADED`), falling back to
`0x24` (software VP | multithreaded); `D3DCREATE_FPU_PRESERVE` (`0x2`) is absent in both. By D3D9's
documented behaviour the creating thread's x87 control word is then set to 24-bit precision, nearest
rounding (`0x007F` with all exceptions masked), matching ADR 0007 and `oracle.py CW_D3D9`. Every
`FLDCW`/`FNSTCW` in the exe (Ghidra mnemonic query: 100+ sites) lies in the statically linked library
range `0x59f06e..0x5b1cdc` or in the CRT (`__ftol` at `0x5d6750`, `0x5d727d..`). The sampled sites
(`0x59f06e`, `0x5a20f3`, `0x5b01e6`, `__ftol`) save the word, OR in `0xC00` (rounding control only) for an
integer conversion and restore it, so precision control is never changed after device creation; not every
site was read. The 24-bit setting itself is **not observed** in the exe (no runtime run); it is the
documented D3D9 effect of the flags above. Consequence for the port: `__ftol` (`0x5d6750`) truncates, so
every `(int)float` in the loop must truncate; `drive`, the integrator and the tyre model use x87
`fsin/fpatan/f2xm1/fscale` (`0x43ba80`), pinned as APPROX in `docs/x87.md`.

## 5. Frame loop and substeps

### 5.1 `FUN_00428960` per iteration [static; addresses are the call sites]

1. `FUN_00552310(7)` (`0x428a91`): start timer 7 (`QueryPerformanceCounter`, `FUN_00552310/00552330`).
2. unless the skip flag `DAT_0062f270` is set (it is cleared otherwise):
   `FUN_0054dda0(DAT_0063c534)` (`0x428aad`, takes the last frame time; role [not traced]) ->
   **`FUN_00428450` (`0x428ab7`, physics: control + substeps)** -> script scheduler `FUN_00418d10(10.0)`
   (`0x428acb`, if `M+0xe4`) -> `animate` time `dt_a = min(now - last, 0.1)` from `FUN_005516c0` (QPC
   milliseconds, x 0.001 = seconds) and **`FUN_00426fd0(dt_a)` (`0x428b02`, `animate` for list `M+0x40`)** -> `FUN_004277f0`
   (`0x428b12`, deferred deletes) -> listener/camera update `FUN_005508f0`, `FUN_00550980`,
   `FUN_005378d0`, `FUN_00537b40` (`0x428cb6..0x428cd4`).
3. render: `FUN_004f9760` (`0x428ce1`) -> `FUN_004fca30` (`0x428cfe`) timed with timer 1; frontend hook
   `FUN_00408ce0` (`0x428dda`).
4. resource-queue pump `FUN_00505cd0/00505db0` (`0x428e51..0x428e9b`), bounded by the frame budget
   `_DAT_0060c8c4` (`System.setLdPriority` toggles it between 0.1 and -1, native `0x47bf30`).
5. `DAT_0063c534 = FUN_00552330(7)` (`0x428ed2`): **wall time of this whole iteration, the only writer of
   `0x63c534`**; `FUN_005522a0` (`0x428f58`) pumps window messages, `WM_QUIT (0x12)` sets the quit flag
   `DAT_0063c520`; `FUN_00554dc0` (`0x428f6b`) audio update.

So the physics of frame N advances by the wall time of frame N-1 (the first frame advances by 0). Time
enters through `QueryPerformanceCounter` only (and `timeGetTime`-style sources in the audio code): a
deterministic replay must supply the frame times, which is the `docs/call-log.md` seam.

### 5.2 `FUN_00428450(M)` (fastcall, ECX = M) [oracle: substeps; static]

```
local = DAT_0063c534 * M[0x110]            ; last frame time x time scale (System.timeWarp -> FUN_004283f0)
local = min(local, DAT_0060c8cc)           ; 0.1 s; 0x60c8cc has only reads (428450, 428960, 43a3b0)
DAT_0063c538 += local
if DAT_0063c538 > 1e-4 (0x5f09d4):         ; "control" once per frame, with the accumulated time
    if list M+8 non-empty: for each obj: FUN_004291e0(DAT_0063c538, obj)   -> control(dt)
    DAT_0063c538 = 0
FUN_00552310(0)                            ; timer 0
step = W[2]                                ; 0.02
FUN_004a5180()                             ; sets DAT_006439d8 = 1 ("clear accumulated forces on next step")
while local > 0 and not quit:
    chunk = local < step ? local
          : (local - step <= step ? local * 0.5 : step)      ; 0x5f09d0 = 0.5
    if FUN_004a5190(chunk) == 0: break      ; below the minimum step: remainder carried inside W
    local -= chunk; done += chunk
    FUN_00427160()                          ; timers with due-time <= W[0]
    FUN_004277f0()                          ; deferred deletes
```

[oracle: substeps] (exe code run with the world step stubbed to record its argument; float32 arithmetic):

| frame time (s) | steps (s) |
|---|---|
| 0 | none |
| 0.01 | 0.01 |
| 0.02 | 0.01, 0.01 |
| 0.021 | 0.0105, 0.0105 |
| 0.04 | 0.02, 0.01, 0.01 |
| 0.05 | 0.02, 0.015, 0.015 |
| 0.06 | 0.02, 0.02, 0.01, 0.01 |
| 0.1 | 0.02 x4, 0.01 x2 |
| 0.25 (clamped to 0.1) | 0.02 x4, 0.01 x2 |
| 0.05 at time scale 2.0 | same as 0.1 (scale applied before the clamp) |
| 0.05 with world step 0.01 | 0.01 x4, 0.005 x2 (step really comes from `W+8`) |

Note the exact-`step` case: a remainder equal to the step is split in half, because the test is
`local - step <= step`. The port must reproduce this in f32 (`0.5` is the constant at `0x5f09d0`).

After the loop: `DAT_0063c54c` frame counter (wraps at 1000), `W+0x20` physics-cost average, and the
replay-ring copy of up to `DAT_0063c550` body states (`0x636480..`, stride 0x58 bytes) from the focus
body, [static], not needed for physics results.

`control` is called **once per frame** with the accumulated time, **before** the substeps;
`drive` is called **once per step** (6.2); `animate` once per frame after all steps, with an
unscaled, separately measured `dt_a` (5.1).

## 6. The world step `FUN_004a5190`

### 6.1 Constructor and constants [oracle: ctor]

`FUN_004a5580(W)` (called from `FUN_00427980`) sets, verified by running it on a scratch object:
`+0x00 = +0x04 = 0`, `+0x08 = +0x0c = 0.02` (0x3ca3d70a), gravity `(+0x104,+0x108,+0x10c) =
(0, -9.81, 0)` (0xc11cf5c3), air density `+0x1b4 = 1.2928` (0x3fa57a78), two anchor bodies
(`FUN_0049ab60`/`FUN_0049b300`). Nothing else writes `W+8` (no relocation or pseudocode store; the world
is only reached through `PTR_DAT_00617650`), so **0.02 s is constant**. Gravity is overwritten when a map
config has a `gravity` key: `FUN_00463220` (the `ground` factory) stores `(0, -gravity, 0)` at
`W+0x104` if `-gravity <= 0` [static]. Other constants: minimum step `1e-4` (`0x5f09d4`), halving factor
`0.5` (`0x5f09d0`), frame clamp `0.1` (`0x60c8cc`), clock scale `0.001` (`0x5f09cc`).

### 6.2 Sequence [oracle: worldstep for the control flow; static for the order]

`FUN_004a5190` is `thiscall` on `W` with one float argument. Call sites (ordered by address):

| step | call | note |
|---|---|---|
| a | accumulate: `dt = dt + W[1]`; if `dt < 1e-4` store it back in `W[1]` and **return 0** | [oracle] 5e-5 -> 0, `W[1]` = 5e-5 |
| b | if `DAT_006439d8` (set by `FUN_004a5180`): zero the words `+0xa0/+0xa4/+0xa8` of every entry of the contact list `W[0x46]` (`W+0x118`); clear the flag | |
| c | `W[1] = 0` (the carried time was merged into `dt`), timer 4 start, `W[3] = dt`, counters reset | [oracle] 6e-5 carried + 0.02 -> sim time 0.02006 |
| d | for each body in `[W[0x21]+0x24]` chain: `FUN_004a3f90(body)` (`0x4a5235`) pre-step (contact-response pass; events to scripts via `FUN_00426800`, flag `0x200`) | role [inferred] |
| e | list `W[0x27]`: virtual `+0xc` per entry, entry removed when it returns < 0 | membership [not traced] |
| f | **`FUN_00426f60(dt)` (`0x4a52d0`, ECX = M): `drive(dt)` for list `M+0x24`** -> `FUN_00426e10` -> native `car` vtable slot 12 (`+0x30`), or script `drive` | |
| g | `FUN_004a3bc0(body, dt)` per body (`0x4a52e6`): **integrate** (section 7) | |
| h | list `W[0x2e]`: virtual `+0x18`; entries that return 0 are removed | membership [not traced] |
| i | **solver loop, up to 20 trial iterations**: `W[4]++`; flag `W[0x7b] = 1`; `FUN_004a3900` (`0x4a536b`, walks the list `W+0xb8`, vcalls `+0x1c/+0x20/+0x24`, sizes and fills solver vectors/matrices with `FUN_00552a40/00552460/00552d00`); if the flag is still 1: `FUN_004a2d10` (`0x4a537a`, contact-list pass over `W+0x118` calling `FUN_004af970`); then `FUN_00498dd0` (`0x4a53d2`), `FUN_004a2540` (`0x4a53f6`) and, if that returns non-zero, `FUN_004a35d0` (`0x4a5419`, takes contact entries from state 6 to 7 and calls the event dispatcher `FUN_00426800`); `FUN_004a3bc0` again for every body (`0x4a543d`); exit when `W[0x7b] != 0` or the counter exceeds `0x13` | iteration cap [oracle]; roles of the callees [inferred] |
| j | after the loop: `FUN_004a2b90` (`0x4a545d`, removes old/distant entries of the list `W+0x16c` through `FUN_004a2820`), `FUN_004a4ed0(body)` per body (`0x4a5476`, **commit**: flip the state buffer, damp, sleep), `W[0] += dt`, running mean of the iteration count in `_DAT_006439c4`, **return 1** | |

[oracle: worldstep] results (callees other than the `drive` hook stubbed): no contacts -> 1 iteration,
`drive` called first, `W[0]` += dt; persistent contacts (the narrow-phase stub keeps the flag clear and the
resolve stubs return non-zero) -> exactly **20** `FUN_004a3900` calls, counter 20, still returns 1.

`FUN_004a54b0` (`0x4a54b0`) is the single-body variant (control, pre, integrate, post for one body); its
only caller is `FUN_0043a3b0`, slot 13 of the `camera` class vtable `0x5f0e5c` (the camera is a body that
steps itself) [static].

Dead-code note: `FUN_00428790` (single `step` of `W[2]*M[0x110]` with `control`) has no callers or xrefs.

## 7. Integrator `FUN_004a7c10(this = body, src, dst, dt)` [oracle: rk4; static]

Bodies are double-buffered: two 0xd4-byte states at `body+0x64` and `body+0x64+0xd4`, current index
`body+0x20c`; `FUN_004a3bc0` reads state `idx`, writes `idx^1` and `FUN_004a4ed0` flips the index.
State layout used by the integrator [static; the first four also [oracle: rk4]]: `+0x00` position,
`+0x0c` velocity, `+0x18` offset added to the position after integration, `+0x24` time, `+0x28` force
accumulator (scaled in place by `invmass*dt`), `+0x40` 3x3 rotation matrix (rows at stride 16, rebuilt from
the quaternion by `FUN_0054eda0`), `+0x70` angular velocity, `+0x94` orientation quaternion. Body
parameters at `*(body+0x5c)`: `+0x14` inverse mass, `+0x84` linear damping (when negative, velocities are
scaled by `1 + invmass*damp*dt*k`, `k` = `_DAT_005f09c8`), `+0x9c` flags (`0x200` = plain Euler, `0x4` = static).

Branches by `body+0x58` flags: bit `0x04` compound/child (integrates the parent, derives the child);
bit `0x100` clear = **RK4** (the common case); bit `0x100` set = kinematic: explicit Euler
(`pos += vel*dt`).

The RK4 evidence is structural and numeric: four derivative evaluations through `FUN_004a58e0` with
half-step `0x3f000000`, combined by `FUN_004a9000` with weights `0x3e2aaaab` (1/6), `0x3eaaaaab` (1/3),
`0x3eaaaaab` (1/3), `0x3e2aaaab` (1/6). [oracle: rk4] Real code on a 1 kg-equivalent body
(`invmass = 1/1000`, force `(0,-9810,0)`, `v0 = (1,0,0)`, `dt = 0.02`): `pos = (0.02, -0.001962, 0)`
`vel = (1, -0.1962, 0)`, i.e. `pos.y = a*dt^2/2` exactly (explicit Euler would give 0, semi-implicit
-0.003924); `v0 = (3,2,1)`, `a = (10,0,0)`, `dt = 0.01` gives `pos.x = 0.0305`. The rotational state goes
through the same RK4 combination (the `0x1c/0x25` fields) but its numerics were not oracle-checked.
After integration the code applies the linear damping and clamps linear speed against `_DAT_005f1468` /
`_DAT_005f1464` (`FUN_004907e0` is the vector length) [static].

## 8. The `car` class: where the vehicle simulation runs

Class `car` (registry entry `0x60c864`, factory `FUN_00442500` allocates 0x2e98 bytes, vtable `0x5f0f18`)
[oracle: xhelp for slots 3, 4, 11, 12, 13, 14, 17].

| slot | offset | function | role | evidence |
|---|---|---|---|---|
| 3 | +0x0c | `0x44a250` | `car_add` (strings `car_add %d %d`, `wheelbone`): creates the chassis body and wheel bodies/joints | [static] |
| 4 | +0x10 | `0x44b920` | `car_rem` | [static] |
| 11 | +0x2c | `0x44e690` | **`control(dt)`**: driver input -> pedals/steering/gear/handbrake (helpers `0x44d040..0x44e2d0`, torque lookup `0x43be70`) | [static]; called by `FUN_004291e0` from `FUN_00428450` |
| 12 | +0x30 | `0x454500` | **`drive(car_class, car_state, dt)`**: engine, drivetrain, tyres, suspension, forces on the body | called by `FUN_00426e10` from `FUN_00426f60` |
| 13 | +0x34 | `0x4518c0` | **`animate(dt)`**: engine/skid/impact sound and effects (calls `FUN_0048cfb0`, `FUN_0048f0e0`); no force computation | [static] |
| 14 | +0x38 | `0x458c00` | large state/telemetry/AI-facing method (format strings, `AI_GoToTrafficSlow`) | [not traced] |
| 17 | +0x44 | `0x457850` | collision callback (string `pscoll`) | [static] |

Calling convention of slots 11-13: ECX = the class instance `K` (the object `0x442500` made, holding the
default parameter block at `K+0x2e48` and the wheel count at `K+0x1f40`), stack = (per-vehicle state `V` =
`obj+0x4c`, `dt`) [static: `FUN_00426e10`, `FUN_004291e0`, `FUN_00426fd0`]. Per-vehicle state `V` holds the
body pointer `V+0x13bc`, the wheel array `V+0x13e4` (stride **0x2b4** per wheel), the live parameter block
pointer `V+0x1fbc` (cloned from the default block by `FUN_0043e3e0`, see `FUN_00448430`) and the engine/
drivetrain state (`V+0x1dxx/0x1exx`: RPM at `+0x1e18`, wear `+0x1e38/0x1e48`, ...).

### 8.1 `drive` (`0x454500`) sections [static; call-site evidence]

In order of appearance (line numbers are positions in the pseudocode of `re/out/native_pseudo.c`):

1. entry guards: returns if `V+0x1fc0 != 0` or `dt < 1e-4` (`0x5f09d4`); parameter block `P = V+0x1fbc` (kept in a local; the register variable `iVar19` is reused for other pointers later in the function).
2. engine: torque curve `FUN_0043be70(rpm, P)` / `FUN_0043bee0` (piecewise-linear table: `P[0]` count, `P[1]` rpm/torque pairs, i.e. the `torque` cfg line); engine friction (`P+0xe0/0xf0/0xf4`, 10.1); starter torque `P+0xcc` added below idle `P+0xec`; power scale `V+0x1dd0/0x1dd4` (wear-scaled `P+0xd0/0xd4`); selected gear ratio `V+0x1dd8 = P[0x20 + gear*4]` and total ratio `P+0x40 * V+0x1dd8`; drivetrain term `P+0xe4 / ratio^2` and the engine angular acceleration `torque / P+0xe0`.
3. wheel loop (`*(K+8000)` wheels, stride 0x2b4): slip computation `FUN_0043bbd0` (relaxation-length blend with constants at `0x60e02c..0x60e048`, `atan` for the slip angle) and the **tyre force** `FUN_0043ba80`/`FUN_0043bb30` (section 9); brake term `P+0xdc * V+0x1dc0 ...` and `P+0xd8 * wheel+0xd8`; `FUN_00447830(K, V, dt)` is called twice (role [not traced]); `FUN_00491f80` (spring-object setter) is called with `P+0x44/+0x48` values.
4. gravity: `W+0x104..0x10c` is read here (`local_50 = m*g.x + ...`) and the result is applied to the chassis body through the vector helpers `0x4a6ff0`, `0x4a7190`, `0x4a6520`, `0x4a6e10`; free-flying parts (detached panels) apply gravity in `FUN_0043f850(V, dt)` instead.
5. wear/damage: `FUN_00551330("Invalid wear!")` guards and `FUN_0043c260` at the end (script call `updateDamage` through `FUN_00425a90`).

`drive` calls `_rand` (`0x5d7408`, the MSVC LCG already ported in `slr-platform::rng`) while walking the list
`V+0x1de4` under `V+0x1e18 > 0` (the engine-speed block), so **`drive` consumes the deterministic RNG
stream** and the call order matters for replay [static].

### 8.2 Natives that set the same state

`WheelRef` setters (35 natives) write the wheel record at `V+0x13e4 + i*0x2b4` [static; offsets from
the decompiled setters]: `setDrive +0xcc/+0xe0`, `setBrake +0xd0`, `setHBrake +0xd4`, `setForce +0x44`,
`setDamping +0x48/+0x4c`, `setRestLen +0x50(+0x5c)`, `setMaxLen +0x54`, `setMinLen +0x58`,
`setRadius +0x60(+0x5c)`, `setFriction +0x1f0/+0x64/+0x68`, `setFrictn_x +0x218`, `setSliction +0x1e8`,
`setStiffness +0x1f8`, `setRollRes +0x74`, `setBearing +0xf0`, `setMaxLoad +0x214`, `setLoadSmooth +0x220`,
`setWidth +0x1d4`, `setCPatch +0x88/+0x8c`, `setPacejka(i, v)` -> `+0x1e8 + 4*i` for `i < 17`,
`setArm/setHub/setInstantCenter` +0x234..+0x290 (suspension geometry), `setPos/setYpr` the wheel mount.
Force/damping/length setters re-derive the spring object through `FUN_00491f80`. `FUN_00448430` (reached
from `Chassis.forceUpdate`, native `0x43e320`) copies the 17 default Pacejka values `+0x2e50` of the class
into every wheel (`FUN_005d6780(wheel+0x1e8, K+0x2e50, 0x44)`).
`Chassis.setTorque` (`0x43c930`) writes `P+0xd0/+0xd4`; `getTorque` (`0x43c7f0`) evaluates the torque curve.
`PhysicsRef.create/createBox/createSphere` (`0x4807f0/0x4805b0/0x4806f0`) create bodies with
`FUN_0049a990` / `FUN_0049ab60` and attach them to a `GameRef` with `FUN_0048a1e0`; `setMatrix`
(`0x480920`) writes a body's pose and calls `FUN_004986f0`; `getPos` (`0x480b00`) reads the current
state. `Vehicle.getSpeedSquare` (`0x480500`) returns |v|^2 of the body. `System.timeWarp` (`0x47bf80`
-> `FUN_004283f0`) sets `M+0x110` and re-anchors `M+0x108`; `System.simTime` returns `W[0]`;
`GameType.addTimer(f, id)` (`0x47e1c0`) schedules at `W[0] + f` (`FUN_0048b750`).

## 9. Tyre model [oracle: pacejka; static]

`FUN_0043bb30(this = coef, slip, Fz)` (and the clamping twin `FUN_0043ba80`, which returns >= 0) is a
Pacejka magic formula; with `p = coef[0..10]`:

```
D  = Fz*p1 + p2                       ; peak factor
B  = (Fz*p3 + p4) * exp(-Fz*p5) / (D*p0)
x  = (Fz*p9 + p10 + slip) * B         ; horizontal shift p9*Fz + p10
E  = Fz^2*p6 + Fz*p7 + p8             ; curvature
F  = D * Fz * sin(p0 * atan(E*(atan(x) - x) + x))    ; p0 is the shape factor C
```
(returns 0 when `D*p0 == 0`). `exp` is `f2xm1` + `fscale` of `-Fz*p5*log2(e)`; `atan` is `fpatan`; `sin`
is `fsin` (x87 transcendentals, `docs/x87.md`). Oracle: 40 random coefficient sets, exe vs the closed form
in double precision: max relative error `3.9e-7`, i.e. the formula above is the function computed (not a
bit-exactness claim; Unicorn's x87 transcendentals are approximate). The 17 coefficients live at
`wheel+0x1e8` (`setPacejka`); only indices 0..10 are read by this function. `FUN_0043bbd0` (slip with
relaxation length and low-speed blending) feeds it [static].

## 10. cfg keyword table and consumers

Keyword table at `0x60e2e8` (keyword string, handler pairs), 45 keyword rows plus the `eof` terminator
[static; dumped from the image by walking the pairs]:
`enginepower 0x443f90`, `ratios 0x444080`, `torque 0x4440e0`, `rearendratio 0x444150`,
`brakepower 0x444190`, `clutch 0x444200`, `driver/controller 0x4445f0`, `ext_camera 0x443d40`,
`nodriver 0x444710`, `name 0x444730`, `seat 0x444740`, `pedals 0x444810`, `shifter 0x444990`,
`key 0x444a60`, `handbrake 0x444ae0`, `cockpit_rpm/speed`, `osd_*` (UI gauges `0x444e00..0x4456d0`),
`stockpart 0x46d970`, `material 0x445e00`, `damage 0x46cf50`, `damagedef 0x445ef0`, `velfactor 0x445fc0`,
`turn 0x445f00`, `roll 0x446000`, `pitch 0x446050`, `steerhelp 0x4460a0`, `cheat_control 0x446100`,
`notify 0x446160`. Handler convention: `(char *args, car_class K)`, runs only when `K+0x120 == 0`, writes
into the default parameter block `P = *(K+0x2e48)` using the exe's `sscanf` (`FUN_00551240`); `args`
points just past the keyword's NUL.

[oracle: cfg] five handler invocations run on the real exe (including its statically linked `sscanf`) against a
scratch block pre-filled with -1:

| keyword | handler | format (address) | writes | verified with |
|---|---|---|---|---|
| `enginepower a b c d [e f]` | `0x443f90` | `%f %f %f %f %f %f` (`0x60f60c`) | a -> `P+0xd0` **and** `P+0xd4`; b -> `+0xe0`; c -> `+0xf0`; d -> `+0xf4`; e -> `+0xe4`; f -> `+0xe8`; absent trailing values leave the old value | `135.0 6.5 0.00006 0.003` (stock taxi line) and `1 2 3 4 5 6` |
| `brakepower a b` | `0x444190` | `%f %f` | `+0xd8`, `+0xdc` | `1300.0 1800.0` |
| `rearendratio a` | `0x444150` | `%f` | `+0x40` | `3.33` |
| `clutch a` | `0x444200` | `%f` | `+0xc8` | `0.5` (arbitrary value; no stock line available) |
| `ratios n r1..r7` | `0x444080` | `%d %f x7` | `+0x1c` count, `+0x24..+0x3c` | [static only] |
| `torque n (rpm,torque)*n` | `0x4440e0` | `%#%d`, `%#%f:%f` | `P[0]` count, `P[1]` -> pairs of 8 bytes | [static only]; no stock text cfg carries a `torque` line |

### 10.1 Consumers of the `enginepower` fields [static]

Found by tracking every variable assigned from `V+0x1fbc` and listing the offsets dereferenced (offset ->
functions that read it; "w" = writes):

| offset | meaning (from use) | writers / readers |
|---|---|---|
| `+0xd0`, `+0xd4` | power scale factors | writers `0x43c930` (`Chassis.setTorque`, then derives `V+0x1dd0/0x1dd4`) and `0x448430` (resets to 1.0 via `piVar3[0x34/0x35]`); reader `0x454500` (`V+0x1dd0 = (1 - wear^2*0.6) * P+0xd0`, `V+0x1dd4` likewise with `P+0xd4`) |
| `+0xe0` | engine inertia | `0x448430` loads the script variable `engine_inertia` into it; `0x454500` divides torque by it (angular acceleration) and uses it in the speed-dependent friction term `rpm * P+0xe0 * 0x5f0cf8` |
| `+0xf0`, `+0xf4` | engine friction, forward / reverse | `0x448430` loads `engine_friction_fwd/rev`; `0x454500` uses `+0xf0` for positive and `+0xf4` for negative engine speed and scales both by `0x5f0c3c` when the engine is fully worn |
| `+0xe4` | drivetrain term | `0x454500` only: `1 / (total_ratio^2 / P+0xe4)` (reflected inertia) [inferred] |
| `+0xe8` | auxiliary | `0x454500` only: copied to a local at function start; meaning [not traced] |
| `+0xec` | idle speed (rad/s) | `0x448430` (`engine_rpm_idle * 0.10472`), `0x454500`, `0x4518c0` |
| `+0xcc` | starter torque | `0x448430` (`starter_torque`), `0x454500` (added below idle), `0x4518c0` |
| `+0xd8`, `+0xdc` | `brakepower` | `0x454500` (brake torque terms) |
| `+0x40` | rear-end ratio | `0x448430`, `0x454500` (`P+0x40 * gear ratio`), `0x458c00`, `0x4518c0` |
| `+0x1c`, `+0x20..` | gear count, ratio array | `0x448430` (from script `gears`/`ratio`), `0x454500` (`P[0x20 + gear*4]`), `0x457850`, `0x458c00`, `0x44e690`, `0x4518c0` |
| `+0xc8` | `clutch` | written by the handler and by `0x448430` (`ClutchF`); no reader found in the car methods [not traced] |

Method: every variable assigned from `V+0x1fbc` was tracked through each car method and the dereferenced
offsets listed; in `0x454500` the tracking was redone by hand after noticing that the register variable is
reused for wheel pointers from the wheel loop onwards, so only uses in the engine sections (before the wheel
loop, and through the dedicated local `P`) are listed. For `0x448430`, `0x4518c0`, `0x458c00`, `0x457850`,
`0x44e690` the readers come from the automatic tracking only (offset present, usage not read).

Open question: `FUN_00448430` stores 1.0 into `P+0xd0/+0xd4` after reading the script configuration,
so whether the cfg `enginepower` first value is ever visible to `drive` depends on whether
`Chassis.setTorque` or `forceUpdate` runs last; the order was not traced (see section 11).

Global physics tuning from the game config (`FUN_00427bf0`, loaded by `FUN_00427980`): `engine_inertia_factor`
-> `_DAT_0060e028`, `wheel_gndfeedback_factor` -> `_DAT_0060e020`, `wheel_brake_factor` -> `_DAT_0060e024`,
`steerhelp_turn` -> `_DAT_0063c7a8`, `esp_steer_scale/angvel_scale/yaw_allowed/brake_level/power_level` ->
`_DAT_0063c7ac, 0060e050..0060e05c`, `head_move_*` -> `0060e014..0060e01c`. Their consumers were not
traced except that `0x60e02c..0x60e048` are the slip-model constants read by `FUN_0043bbd0`.

### 10.2 Acceptance (issue #50)

Oracle that ran: **Unicorn function oracle, control word `0x007F` (x87 PC=24)** — a cfg-handler
differential over the **stock install**. `re/oracle/cfg_handler_vectors.py` builds the exe's own loader
state (the car class `K`, the parameter block `P`, the fixture's heap image, the keyword table and its
107 `[keyword, handler]` pairs walked from `0x60e0f8`), then for every line of every cfg it calls the
line's own handler at its table address **as exe code** and records the changed words (`diff`), the
hooked host calls and the heap pointer. Each line is run twice, with the stack refilled with `0x00` and
`0xCC` before the call, so a line whose handler reads uninitialised stack is flagged `nondet`. No Wine
and no booted engine: the compared facts are the exe's own writes and its own host-call arguments.

```
~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen handlers  # re/out/cfg_handlers/{init.json,handlers_{stock,modded}.jsonl}
~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen sscanf    # re/out/cfg_handlers/sscanf.jsonl
~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py verify        # re-run, require identical rows
cargo test -p slr-physics --test cfg_handlers_oracle   # the replay (paths overridable via SLR_CFG_HANDLER_VECTORS)
cargo test -p slr-fmt-cfg --test sscanf_oracle         # the shared `sscanf` port (FUN_00553890/00551240)
```

Observed on branch `m3-09-cfg-handlers` (2026-10-18), exe `reference/raw/StreetLegal_Redline.bak.exe`:

```
cfg_handlers_oracle: files=2037 exercised=42635 matched=42635 distinct_kw=53 nondet_skipped=14 unported_skipped=0 uncovered_unexpected=0
sscanf_oracle: rows=4410 mismatches=0
```

The 2,037 cfg files (998 in `stock/`, 1,039 in the modded tree) hold 44,685 keyword lines: 2,036 `eof`
terminators (no handler), 42,649 lines whose keyword is in the table, of which the 14 `nondet` rows below
are excluded — **42,635 replayed, 42,635 word-for-word identical**. The replay suite is wired into
`slr-ledger collect` as a `unicorn` suite, so the 53 annotated handler rows read `passing` in
`docs/fidelity.md`.

**Keyword coverage.** The table has **107** `[keyword, handler]` pairs. The stock and modded corpus
exercises **53** of them; the port has all 107. The other **54** never appear in any cfg, so they are
covered by synthetic cfg files instead (`cfg_handler_vectors.py gen leaves`): for each keyword, edge and
seeded-fuzz argument vectors, a preamble where the handler needs record state (`type`, `slot`, `body`,
`flap`), a duplicate-key re-entry and an overflow block for the bounded record lists, run through the
exe's own dispatch (car table `0x60e0f8`, part table `0x611368`) under the same Unicorn oracle, once with
the stack zeroed and once with `0xCC` to expose reads of uninitialised stack (those rows are re-applied,
not compared). The non-keyword helpers (`list_head_ctor`, `list_node_ctor`, `vec3_zero`, `vec3_set`,
`ref_release`, `stricmp_eq`) are called directly on garbage-filled cells (`gen direct`). Result on the
current tree: `cfg_leaves_oracle: exercised=1108 matched=1108`, `cfg_leaves_direct_oracle: matched=116
bad=0`, so every `slr-physics` ledger row reads `passing`. These vectors found one real port bug: the
part-table keyword `skinned` (`FUN_0046d840`) zeroed cells whose value did not convert, where the exe
scans straight into the car object and leaves them alone; fixed with a regression unit test. Fixture
recipe: `SLR_CFG_VECTORS_OUT=DIR ~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen all`
(about 70 s, deterministic except the `date` stamp of the direct rows), replayed with
`SLR_CFG_HANDLER_VECTORS=DIR cargo test -p slr-physics --test cfg_handlers_oracle`. Nothing derived from
the exe is committed. `acceptance/g4.py` (`check.py --execute G4`) regenerates all of it into an
isolated bundle and classifies every physics native (`passing`, `blocked_by_g11`, `no_test`,
`unimplemented`); the 122 physics natives of `WheelRef`, `Part`, `Chassis`, `GroundRef`, `DynoData`,
`SfxTable` and `Vehicle` without a ledger row are `unimplemented` in the port today (capture-only in
`slr-boot`), which keeps G4 open.

**The 14 skipped rows** are one `body` line each — line 2 of seven cfg files, in both corpora (7 × 2):
`frontend/scripts/mask.cfg`, `frontend/scripts/navigator_click.cfg`,
`misc/catalog/scripts/{body,decals,engine,rgear}.cfg`, `misc/garage/scripts/decals_button.cfg`. They
carry a single token (`body 0x00000029`), so
`%f %f %f %f %f %f %f %s %f %f %f` converts nothing and the exe continues from uninitialised stack
locals (the `0x00` run records the body constructor call `(0, 6, 1.0, 1.0, 1.0, "")`, the `0xCC` run
different values). Their behaviour is not reproducible by construction, so the harness counts them,
re-applies their recorded diff and fast-forwards the heap pointer, keeping every later line of the file
on the exact oracle state.

**Intentional divergence found.** One, in the port: 28 port sites called `FUN_00544590` with
`This::None` where all 30 exe sites pass `ECX = *(K+0x124)`. The oracle cannot see it (its recorded
receiver is the scratch manager `0x30000040`, which the harness accepts against `This::None`), so it was
found by the static audit in C6b and fixed there (commit `009b757`); `resolve` now always passes the
manager. The `body` uninitialised-stack rows above are recorded in the intentional-divergences table of
`docs/fidelity.md`.

## 11. What could not be traced (not guessed)

- **Roles of `FUN_004a3f90`, `FUN_004a3900`, `FUN_004a2d10`, `FUN_004a2540`, `FUN_00498dd0`, `FUN_004af970`.**
  Their position in the sequence and call/return behaviour is established (6.2), but the contact
  generation and constraint-solver mathematics were not read. They are what the port needs for collision
  response exactness (M3 follow-ups); the sequence and the 20-iteration bound are exact, the content is not.
- **Membership of world lists `W[0x27]`, `W[0x2e]`, `W[0x46]`, `W+0xb8`** (which constructors insert what;
  spring/joint objects built through `FUN_00491f80` are the likely `0x27/0x2e` members, unverified).
- **Order of `Chassis.setTorque` / `forceUpdate` / cfg parse** and therefore the effective `enginepower`
  first value (section 10.1), and the meaning of `P+0xe4/+0xe8`.
- **Car slot 14 `0x458c00`, slot 7 `0x44c460`, `0x44fa70`**: 74 KB / 13 KB / 30 KB methods with format
  strings (`car_add`-style commands, AI, bone updates); not read beyond callees.
- **Rotational RK4 numerics and the kinematic Euler branch** of `FUN_004a7c10`: only the translational
  RK4 was oracle-checked.
- **x87 control word at run time**: derived from flags + documented D3D9 behaviour; no whole-game or Wine
  run observed it (#12/#13). Only a sample of the `FLDCW` sites was read.
- **`torque` and `ratios` handlers** were not run (no stock text `torque` line; `ratios` not scripted).
- **Anything in the 5,903-function pseudocode that this document does not cite.** The tracing was driven
  from the loop outward; unreferenced physics code (e.g. `0x4b6xxx..0x4bxxxx` shape/material code) is
  untouched.

## 12. Reproduction record

`~/slr-oracle-venv/bin/python re/oracle/physics_trace.py all` (Unicorn 2.1.4, CW `0x007F`, exe
`reference/raw/StreetLegal_Redline.bak.exe` read-only) printed `7/7 checks passed`:
`ctor`, `substeps` (11 scenarios), `worldstep` (4), `rk4` (2), `pacejka` (40 random sets, max rel.
error 3.9e-7), `cfg` (5 lines), `xhelp` (vtable slots 3, 4, 11, 12, 13, 14, 17, registry, world pointer).
Oracle named: **Unicorn function oracle**, with callees outside each claim hooked as listed in the script.
