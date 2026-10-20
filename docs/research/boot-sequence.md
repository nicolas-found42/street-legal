# Original boot sequence (static and controlled-function evidence)

Compiled from read-only Ghidra analysis of build 606 (`re/out/all_pseudo.c`, 4,356
functions). Addresses are evidence-backed unless marked `[INFERENCE]`. Static
evidence ends where noted; the headless oracle (`#13`) must pin the rest
dynamically. No Wine.

## Chain

1. `WinMain` `0x58c700`: CD check `FUN_00401000` (not ported; checksummed `stock/`
   replaces it), RPK load `FUN_00538380` (`s_system_rpk`), settings, then
   `FUN_00427980()`.
2. `FUN_00427980(param)`: allocate the 0x80-byte JVM (`FUN_0054f560`),
   construct it into the existing system context at `param+0xe4`
   (`FUN_0040fb50`), then in order:
   - `FUN_0040fe70` loader boot: 8 package roots, 5 primitive descriptors,
     eager requests for `java.lang.String` and `java.util.resource.GameRef`
     ONLY (everything else is requested lazily at runtime).
   - `FUN_00487f20`, `FUN_0046b530`, `FUN_004427e0`: the three native
     registration batches (all 353 rows).
   - Load `system.rpk` type `0x47` as a gametype, create the `Config` instance
     through `FUN_0053a5e0`, retain its script object at context `+0x100`, then
     apply native settings through `FUN_00427bf0`.
3. `FUN_00428960`, before its `do` loop: resolve the Config `init` field,
   load its resource link with `FUN_00546070`, then call
   `FUN_0053a5e0(&DAT_00636460, ..., "GameInit")` to create the game instance.
   This is a creation operation, not a lookup by alias. Link the returned node
   with `FUN_004290f0`, retaining data at system context `+0xf0/+0xf4`.
4. Main loop (gate `DAT_0062f270`), per frame:
   - `FUN_0054dda0` input/device poll.
   - `FUN_00428450` physics/object-list step + GC cleanup (internals
     `FUN_00427160`/`FUN_004277f0`).
   - `FUN_00418d10(jvm, 10.0f)` scheduler pump: walks the thread list
     through `FUN_00416940` into `Thread::run` `0x420ff0` with a float
     budget. The scheduler returns a status; `Thread::run` returns leftover time.
     Thread state words: `thread+0x2c` flags
     (0x4 running, 0x10 waiting, 0x20 notified, 0x40 stopped, 0x80 dead);
     frame+0x28 operand stack, frame+0x18 locals.
   - Read `CounterClock` `0x5516c0`, scale by the float at `0x5f09cc`,
     clamp elapsed time to `DAT_0060c8cc`, then call `FUN_00426fd0(dt)` for
     animation callbacks. This function is not a frame-pacing routine.
   - The loop's later default nonpositive `DAT_0060c8c4` timing branch calls
     `FUN_00551730(10.0f)`; its positive branch has separate timing logic.
     QPC scaling and frame pacing require their own behavioral validation.

## First script execution (controlled bridge established)

`FUN_0053a5e0` contains the script bridge itself. When the resolved gametype
resource data has a nonzero class at `+0x10`, it creates a `THRD-CREATE` thread
with `(jvm, 10, 0, name)` through `FUN_0041f340`, allocates the script instance
through `FUN_00404e20`, supplies the newly created resource ID as one argument through
`FUN_0041f9f0`, installs the native link via `FUN_0042a9e0`, and calls
`FUN_00408a70(instance, thread, class)`. That original wrapper dispatches
`FUN_004207c0(thread, class, instance, "<init>")`. A zero dispatch result
immediately runs `FUN_00420ff0(thread, 0)` before creation returns.

Oracle actually run: `re/oracle/boot_bridge_probe.py`, original build 606 under
Unicorn at PC=24. It executes the original resolver and constructor wrapper
with controlled resource nodes, allocation and recording boundaries for script
dispatch and interpreter. It asserts the order, thread arguments, `<init>`
receiver/class and zero budget. A null script class creates no script thread;
a nonzero constructor-dispatch result skips the immediate interpreter call.
All three scenarios repeated byte for byte in two fresh probe runs.
The evidence is in ignored `re/out/boot/bridge-checked.jsonl`. Reproduce:

```sh
~/slr-oracle-venv/bin/python re/oracle/boot_bridge_probe.py > re/out/boot/bridge-checked.jsonl
```

This is a controlled function oracle, not headless-original startup or actual
script-body execution. The controlled Rust replay now resolves Config's `init` and its RPK script
class; original startup allocation history, native/script links and execution
comparison remain open. The previous note's
claim that static evidence ended at list linking omitted the resolver's script
branch. The separate headless-original oracle still stops before script boot.

## Gametype mechanism

RPK text chunks `gametype 0xHHHHLLLL` (map id in the high word; 160 distinct in
stock, parsed in the `FUN_0053dee0` RSD branch via `FUN_00544590`/`FUN_00546070`)
build the gametype table. The main loop reads Config's `init` resource ID; `GameInit` is the name
passed to the instance-creation operation. The stock `sl.rpk` entry selected by Config's `init` names
`script java.game.Init`; the class is selected by data, not a native hardcoded
class name. The controlled Rust replay resolves it; original whole-boot
execution has not reached it.

## Port implications (boot driver)

- Replicate the `FUN_00427980` order: context/JVM construction, loader boot,
  three native batches; render/sound/input natives use the M1-12 logging hosts.
- RPK load via `slr-fmt-rpak`; create Config from `system.rpk` type `0x47`,
  apply `FUN_00427bf0`, read its `init` field, and create that gametype as
  `GameInit` through `FUN_0053a5e0`; link nodes like `FUN_004290f0`.
- Pump the scheduler with the 10.0f budget and QPC timing through
  `slr-platform` seams; record everything to the call log.
- Use the observed creation bridge above. Resolve the stock-selected class
  through RPK/Config data; run unchanged constructors and report each stop
  (`HostError::NoBody` must name the missing native exactly).

## Controlled VM and resource probes used during driver bring-up

- `re/oracle/vm_empty_dispatch_probe.py`: original `FUN_0041fbc0`, six
  supplied-metadata cases. A null body pointer consumes arguments without a
  new frame and returns 1. An absent return descriptor pushes nothing; a
  present descriptor pushes integer zero, including a declared `V`. This
  explains the unchanged empty `java.lang.Object` constructor.
- `re/oracle/field_flags_probe.py`: original `FUN_00404730`, seven supplied
  field-flag cases. Bit `0x08` selects static storage, including flags `0x29`;
  equality against `0x28` is insufficient for stock Sound constants.
- `re/oracle/resource_ids_probe.py`: original `FUN_00536ed0` and
  `FUN_00536f90`, two supplied seeds, 130 allocations and a recycled ID per
  seed. Refill appends 128 ushort IDs and allocation pops the last with high
  word `0xffff`. Constructor `FUN_00535a70` initializes the next-ID word to 1.
  Pre-Config allocation history remains unpinned; this does not determine the
  Config instance's ID during original startup.

All these original-function probes ran under Unicorn. Their JSONL and repeat
outputs are ignored under `re/out/boot/`. None runs whole-game startup, supplies
an original script-native trace, or establishes MainMenu.
