# Original boot sequence (static and controlled-function evidence)

Compiled from read-only Ghidra analysis of build 606 (`re/out/all_pseudo.c`, 4,356
functions). Addresses are evidence-backed unless marked `[INFERENCE]`. Static
evidence ends where noted; the headless oracle (`#13`) must pin the rest
dynamically. No Wine.

## Chain

1. `WinMain` `0x58c700`: CD check `FUN_00401000` (not ported; checksummed `stock/`
   replaces it), RPK load `FUN_00538380` (`s_system_rpk`), settings, then
   `FUN_00427980()`.
2. `FUN_00427980(param)`: first resolve `system.rpk` local IDs 1 and 4
   into the gametype-library handle (`0x62f260`, type 8) and game-root handle
   (`0x636460`, type 1). Then allocate the 0x80-byte JVM (`FUN_0054f560`),
   construct it into the existing system context at `param+0xe4`
   (`FUN_0040fb50`), then in order:
   - `FUN_0040fe70` loader boot: 8 package roots, 5 primitive descriptors,
     eager requests for `java.lang.String` and `java.util.resource.GameRef`
     ONLY (everything else is requested lazily at runtime).
   - `FUN_00487f20`, `FUN_0046b530`, `FUN_004427e0`: the three native
     registration batches (all 353 rows).
   - Load `system.rpk` type `0x47` as a gametype, create the `Config` instance
     through `FUN_0053a5e0`, retain its resource ID at context `+0x100` and
     its native node at `+0x104`, then
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
class and installs the measured native/script links before unchanged constructors.
Original startup allocation history and execution comparison remain open. The previous note's
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

- `re/oracle/boot_settings_probe.py`: original `FUN_00427bf0`, two supplied
  Config-getter profiles, recording exact arithmetic/global writes and final
  refresh request. The port replays getter/write order and bits; this does not
  establish missing-field diagnostics or whole renderer settings behavior.
- `re/oracle/boot_refresh_probe.py`: original `FUN_004fc590`, four declared
  backend/retained-handle profiles. An absent backend executes without helper
  hooks and preserves both handle words. Present-backend profiles record
  release/free calls and virtual slot `0x134` with supplied results; no GPU
  work executes. The Rust headless profile implements only the absent branch.

- `re/oracle/boot_native_link_probe.py`: original `FUN_0053a5e0`,
  `FUN_00408a70`, `FUN_00429060`, `FUN_0042a9e0` and field storage, with
  supplied resources/decoded instance storage and script-dispatch boundaries.
  Two dispatch results preserve a temporary retained successor during the
  constructor callback and leave the script handle alone after return. Real
  ResourceRef `newNative`, `set(123)` and `id` then execute against that link;
  they preserve its identity and allocate nothing. These native checks occur
  after resolver return, and cover the already-linked/same-ID case only.
- `re/oracle/boot_gametype_factory_probe.py`: original `FUN_0053a1a0`,
  three supplied-ID/resource-constructor cases. Root/child resource descriptors
  begin `[8, created_id, 2, 0, 0]`; the live data's `+0x10` receives argument
  **2**, the script class. The old decompiler parameter label is misleading;
  raw `0x53a279`/`0x53a27d` and fresh execution agree. A zero allocated ID
  returns null before resource construction.
- `re/oracle/boot_native_factory_probe.py`: original `FUN_0053a2a0`,
  three supplied resource/virtual/event profiles. With a script class and no
  compiled constructor in the template, it creates a type-1 instance, retains
  the template link, stores the existing script object from argument 3 at
  live data `+0x50`, and releases its temporary instance handle. Passive PC
  observation records no script allocation/constructor/interpreter entry in
  this branch. Zero requested/allocated IDs return null. Resource constructors,
  virtual loads/refcounts and event/physics callbacks remain controlled seams;
  this does not validate a complete native factory backend.

- `re/oracle/boot_order_probe.py`: original `FUN_00427980` orchestration,
  with supplied resource/JVM/loader/native-registration/Config/settings seams.
  It records the root resource links before JVM construction, the three native
  batch addresses in order, other root links, Config creation, and settings.
  Original context stores retain Config's resource ID at `+0x100` and its
  native node pointer at `+0x104`; neither word is the script object pointer.
  Package-index translation is explicitly supplied by this probe.
- `re/oracle/resource_constructor_probe.py`: original type-8 `FUN_00546ae0`
  and type-1 `FUN_0053e6e0` with their base constructors and supplied descriptors,
  starting from poisoned object storage. Fresh type-8 data has null compiled
  constructor/class and words `+0x18 = 0x3f800000`, `+0x1c = 0`. The class is
  installed later by `FUN_0053a1a0`. Fresh type-1 bounds kind is 2; its child
  sentinel is empty, script back-reference and physics owner are null. In this
  precise state, real `FUN_004988a0`, `FUN_00498b10` and `FUN_0048ae40` make no
  object writes, and the latter two return zero. Resource manager registration
  and nonempty/spatial/physics cases are outside this probe.

- `re/oracle/boot_native_factory_fresh_probe.py`: real type-1 constructor
  followed by the original native factory and all three empty post-create
  helpers. The constructor's flags `0x21` survive the factory's OR with `0x200`,
  yielding `0x221`. The older controlled zero-initial-flags case yielding
  `0x200` is insufficient evidence for a fresh runtime instance.
- `re/oracle/gametype_materialize_probe.py`: real type-8 constructor and
  `FUN_005461a0` parser on actual read-only `system.rpk` text: root 1's
  `lod_amp 1.000` and Dummy `0x1b`'s `script java.util.resource.Dummy`.
  With the class-request identity supplied at `FUN_00410890`, the parser stores
  that class at data `+0x10`, leaves the compiled constructor null, and retains
  words `+0x18 = 0x3f800000`, `+0x1c = 0`. These are two supplied parser phases;
  package inheritance and original class loading are not executed. The external
  template flags remain zero.

- `re/oracle/boot_native_factory_child_probe.py`: the fresh native factory
  with real type-1 parent/child constructors, a declared already-materialized
  empty parent, original inherited-range setters and empty post-create helpers.
  A supplied child-registration boundary retains the parent pointer. Parent and
  child range words are `[0, 0]` and final instance flags are `0x221`. This
  does not execute parent loading or parent-child list registration. The declared
  parent retains flags `1` and load-state `0`. Production supports this measured
  empty-parent profile; other parent profiles stop explicitly.
- `re/oracle/resource_child_registration_probe.py`: real `FUN_00536a10`
  explicit-parent insertion with real type-8 and type-1 constructors. Each of
  three sequential inserts checks parent, head, tail, previous, next and sentinel
  links. Allocation is supplied; manager lookup and package hierarchy loading
  are not executed.

All these original-function probes ran under Unicorn. Their JSONL and repeat
outputs are ignored under `re/out/boot/`. None runs whole-game startup, supplies
an original script-native trace, or establishes MainMenu.

## Bounded continuation evidence

These fresh original-function probes prepare the next boot boundaries; their
results do not establish original startup or a stock-script call-log prefix.

- `re/oracle/vminit_constructor_probe.py`: four static/instance initializer
  cases with zero or one integer constructor argument. Original initializer,
  argument collector and null-body constructor dispatcher execute. Allocation,
  pool/type identity, method lookup and cached argument signature are supplied.
  Passive observations check the actual argument `17`, receiver identity and
  immediate return `1`; no constructor script body is executed by these rows.
- `re/oracle/thread_start_continuation_probe.py`: original `Thread.start`,
  typed target read and constructor-independent call-by-name dispatch, with
  field/type/method lookup and native handle storage supplied. A null body
  returns `1`, kills the thread and clears its script and handle. A controlled
  void-return body instead returns `0` from dispatch with a pending frame and
  retained handle/script. A later original interpreter pump executes it. The
  existing `RunFailed` name alone cannot distinguish this pending continuation.
- `re/oracle/resource_header_registration_probe.py`: three actual frontend
  headers (3, 210, 168), through original header conversion, registration,
  constructors and inactive-list insertion, followed by reference binding.
  RID mapping, parent/container lookup, foreign root and queue anchor are
  supplied. Binding the loading-image reference changes the retained-reference
  head without materializing its descriptor; setting the same ID has no lookup.
- `re/oracle/resource_package_registration_probe.py`: all 279 actual frontend
  headers, in file order, through the same original registration path. Every
  sibling chain is checked after each insertion. Type 3 and type 5 constructors
  add `0x100` to descriptor flags; every registered entry has load-state `0x40`
  before materialization. The eight foreign system roots are real empty
  constructors with declared identities. This does not execute original package
  file loading, foreign-root hierarchy loading or sourcefile parsing.
- `re/oracle/boot_pacing_probe.py`: original `FUN_00551730`, real `__ftol`
  and CRT wrapper with only the Sleep import supplied. Seven float bit patterns
  check the requested integer milliseconds. The default `10.0f` requests
  `Sleep(10)`, while `10.9f` truncates to `10`. This measures conversion and the
  API request, not actual host waiting or whole-loop timing.

Each probe ran under Unicorn and repeated byte-identically in ignored
`re/out/boot/`. The constructor, Thread and resource probes also received an
independent ABI/source review and fresh replay. Low-confidence Jev results
were manually checked against those strict original-execution assertions;
none certifies an end-to-end boot comparison.
