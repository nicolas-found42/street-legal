# Boot to MainMenu: canonical state and recovery

Goal: unchanged stock scripts, run by the Rust runtime, reach MainMenu, with an
inspectable comparison against the strongest available original oracle.
This file is the handoff. Update it at every milestone; do not keep a second copy.

Last updated: 2026-10-20, branch `exact-port`.

## What runs now

```sh
cargo run -p slr-boot -- stock --config-instance-id 0xffff0080 --game-init-instance-id 0xffff007f
re/boot-compare.sh re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog
```

Unchanged stock scripts execute Config and GameInit, enter MainMenu at frame
179, finish the formerly failing OptionsDialog path, and run all 600 scheduler
frames without a script/native stop. A read-only observer checks the actual
`GameLogic.actualState` class and excludes pending MainMenu, MainMenuDialog or
OptionsDialog calls. It observes completed menu construction at frame 179 and
420 subsequent idle frames at frame 600. The driver exits successfully only
after >=300 idle frames; a 200-frame run rejects its 20 idle frames.
This is runtime progress, not boot parity acceptance.
Parity is still 0/30.

## Definition of done

MainMenu is entered, its dialog finishes building, and at least 300 idle frames
run without a stop. The filtered native log must match the headless original
through that point. Unfiltered/node divergences must be fixed or explicitly
listed here. A clean test suite alone does not satisfy this definition.

## Evidence and its limits

* **Original oracle (strongest available):** the stock exe under Unicorn with declared
  Win32/D3D9/DirectSound/DirectInput scenario shims (`re/headless/`, branch
  `worktree-agent-a20d568c5b681332f`, preserved at `33d8aa8` and merged
  into `exact-port` by approved merge `f144ecc`). It executes the original
  `Thread::run` over the same stock scripts and records the script-to-native call
  log. Run with `re/headless/run-boot.sh`; log at
  `re/out/headless/boot/observed/script-natives.slrlog`, facts in `original-facts.json`.
  Config ID 0xFFFF0080 was observed in the original. The controlled driver supplies
  GameInit ID 0xFFFF007F; original allocation history is not fully replayed.
* **Comparison (current checkpoint):** package header loading moves the first
  node divergence to #65018 (with `DROP=java.lang.GameType.getInfo`, field
  initializers skipped by default; common-prefix comparison only): the port
  runs `GameLogic.run` where the original runs `Osd.animate`. All 28 `openLib`
  results and the 48 registered package names/indices match the original
  discovery capture. Resource ID records #167–169 still match. The filtered
  native divergence is #446: original `System.currentTime`, port `Object.wait`.
  Unfiltered divergence is #646: original `Input.getInput`, port `Object.wait`.
  The device-name, Viewport, controller mapping and vehicle constructor-order
  frontiers have been superseded. This is not full-trace acceptance.
* **Validation:** the package/controller checkpoint passes 936 Rust workspace
  tests, with no failures or ignored tests, and workspace clippy. Checkpoint
  receipts and original-probe boundaries are listed below.
  `String.token` now has a real unchanged stock script-wrapper regression:
  earlier native-only tests supplied the argument cells in the wrong order.
* The scheduler, `Thread.start`, event/timer model, resource instance creation, header
  registration for other packages, `openLib`, `FindFile`, `File`, string concatenation
  and others were generalised from probed special cases **without per-function original
  vectors**. They are validated only by the whole-run comparison above, which is
  incomplete. They are not exactness claims.

## Declared scenario inputs (not observed in the original unless stated)

* Simulated clock: ms counter advancing 0.01 ms per read, +1000/60 ms per frame.
* Frame loop: fixed frame interval 1/60 s, world max step 0.01 s, time scale 1.0
  (`engine::FrameConfig`). Ask the oracle for the observed values (requested).
* Input devices: `SysKeyboard`, `SysMouse` only (`INPUT_DEVICES` in `slr-boot/src/main.rs`).
* Display scenario: 640x480, 800x600, 1024x768 at 32 and 16 bits, depth
  descending then width/height ascending, matching the oracle hypothetical HAL
  profile. Current mode: 800x600x32. Mode names are actual VM Strings.
* Viewport, Camera, Text, RenderRef.create, PhysicsRef.createBox and ResourceRef.duplicate
  now maintain resource identities/ownership. Camera creates a type-12 camera
  resource and a type-3 render instance; Text binds its `renderinst` field's
  handle. Renderer/physics payload creation, asset copies and their side effects
  remain unimplemented. Other headless render/sound/input natives answer zero; the 170 native rows no
  engine slice implements (`missing_rows.rs`) are capture-only too.
* Resource materialisation checks the descriptor's source file exists and marks the
  container loaded; nothing is decoded and original file I/O records are absent.
* Native object classes named by `native <name>` descriptors create no full engine
  object. The `player` command path now synchronously invokes script `setcontrol`
  (`FUN_0047a920`); other player commands remain capture-only.
* Missing linked packages (stock links `obstacles\*.rpk`, which the media lacks) get
  declared placeholder roots.

## Frontier (next stops, in order)

1. Filtered native #446 and node #65018: scheduler/time-budget ordering.
   The original uses the declared instruction-count-driven QPC scenario;
   the port still uses a per-read clock and fixed 1/60 frame interval. Resolve
   the actual time inputs and scheduling behavior before altering ordering.
2. Original finalizer calls start near filtered #451 in the preserved trace;
   the port's finalizer queues are empty. Full object/resource lifetime remains
   unaccepted. Do not drop finalizers or mask primitive IDs to force acceptance.
3. Unfiltered #646: original `Input.getInput` versus port `Object.wait`.
   The original input poll and native player `device` commands remain gaps.
4. General interface-array conversion remains an explicit VM boundary. The
   class/primitive-array candidate path now rejects unrelated class overloads
   rather than trying to load an array as a class file (original array CCDs have
   Object as their base). This is not general array covariance support.

## Known gaps behind the happy path

* Resource allocation/lifetime beyond the measured prefix through #167–169.
  The declared original profile's first allocations are Config `0xffff0080` and
  GameInit `0xffff007f`; the driver still supplies these IDs explicitly.
* Physics world, render/audio/input behaviour, SDAT saves, general descriptor parsing.
* `FUN_00428960` frame work not run: input poll, object-list step, `FUN_004277f0`
  reaping, camera/listener setup, render pass, pacing.
* GC/finalizers (`FinalizerQueues` empty), callback block of `FUN_00418d10`.

## Tooling added for this work

* `slr-dump <class>`, `slr-rpk <rpk> [id]` (`crates/slr-boot/src/bin`): class node streams and RPK
  descriptors. Diagnostics: `SLR_TRACE_CALLS`, `SLR_TRACE_NATIVES`, `SLR_TRACE_RES`,
  `SLR_TRACE_FIND` environment variables on `slr-boot`.
* Outputs under ignored `re/out/boot/`: `runtime.txt`, `runtime.calllog`, `engine-events.txt`,
  `materialisation.txt`, `factory-operations.txt`, `resource-registration.txt`, `compare.txt`.

## Recovery

* Remote `origin/exact-port` holds the integrated runtime and oracle source. The
  oracle worktree `.claude/worktrees/agent-a20d568c5b681332f` is inactive, clean and
  retained at `33d8aa8`, also pushed on its original branch. No old agent needs
  to be resumed. Do not delete that worktree or either branch without approval.
* `stock/` and `reference/` are read-only; derived outputs stay under `re/out/`.
* No Wine. Issue #1 and `docs/plan.md` remain the umbrella; #110 tracks VM completion.

## Checkpoint receipts (2026-10-20 resume)

Ignored `re/out/boot/steps-port-command.txt` reproduces node prefix improvement;
`steps-port-token.txt` records the next array-overload stop;
`array-stop.txt` records 600 completed frames after the array fix.
`menu-observer.txt` records frame 179 completion +420 idle frames;
`menu-observer-test.txt` records the positive/short-budget regression.
`token-red.txt` / `token-green.txt` show the argument-order regression and fix.
Oracle `re/out/headless/suite-accepted.txt` records the 169-test pass in its
worktree. That worktree and its derived captures must be retained. A second
ignored copy of the four comparison inputs is at
`re/out/headless/oracle-33d8aa8/observed/`; pass its `script-natives.slrlog` to
`re/boot-compare.sh` to reproduce this checkpoint without relying on a worktree path.

No per-function oracle vector yet covers synchronous command dispatch; its
current regression evidence is the integrated node/native comparison and
`cargo test -p slr-boot --test stock_menu_prefix`. That regression checks
Controller.setcontrol, both menu methods returning, and 600 completed frames;
it also rejects a short run, and intentionally does not claim original parity. Native
player mask initializes to observed EVENT_COMMAND (0x10) and uses script
set/clear operations; native object deletion/lifetime remains incomplete.

Review checkpoint: independent Standards review found no documented violations;
Spec review's command-mask bypass was fixed and re-reviewed. The final Jev gate
verified all three bounded runtime/test/gap claims without contradictions. Its
patch review still escalated (safe-to-apply 0.38, low test-gap confidence 0.07),
so this is not automatic quality approval or parity acceptance. Interface-array
coverage and the native-log mismatches remain explicit follow-up work.

### Viewport identity checkpoint (2026-10-20 continuation)

`re/oracle/viewport_create_probe.py` executes original `LAB_004814e0`,
`FUN_00538d60`, the real ID allocator and `ResourceRef.id` in three controlled
cases: fresh handle, already-bound handle with a retained sibling, and missing
handle. It supplies resource registration, virtual data/retain, Native.ptr
lookup and renderer registration. Original intrusive rebinds and five viewport
state stores execute. Two declared earlier allocations reserve Config/Init IDs;
this is not original startup allocation history or D3D execution.

The Rust replay verifies the resulting resource type/ID, unchanged handle,
old-list repair, viewport state and next allocator result. It also checks that
another wrapper bound to the same resource reads its viewport width. A temporary
restoration of the old create body failed on ID zero versus `0xffff007e`; the
new body passes. This validates the measured identity/state projection, not the
whole render factory. Camera and Text creation remained capture-only at this
checkpoint; the allocation-history continuation below supersedes that boundary.

Local receipts: `re/out/boot/viewport-create{,-repeat}.jsonl` (byte-identical),
`viewport-red-mutation.diff`, `viewport-create-red.txt`,
`viewport-create-green.txt`, `viewport-tests.txt`, `viewport-clippy.txt`, and
`viewport-workspace-tests.txt`; the final native/node comparisons are
`viewport-compare.txt` and `viewport-step-compare.txt`. Reproduce the focused replay with
`cargo test -p slr-boot --bin slr-boot viewport_creation_original_rows` after
generating the ignored oracle rows. Original Ghidra factory evidence is retained
in `re/out/boot/viewport-analysis.rea.json`, Evidence
`ev_ac8c111dd23106fcec6e70cd9cc6972844bb8b92a4e05ac83d18069677319f87`;
the native wrapper was also inspected directly from the executable's bytes.
The decompiler omits an unused factory argument; the probe follows the actual
six pushed arguments and confirms owner zero separately from the flag word.

Independent Standards and Spec reviews found no concrete issue within this
bounded projection. The automatic Jev gate escalated (safe-to-apply 0.15,
low test-gap confidence); it is not automatic quality approval. Its initial
test-claim contradictions mixed the deliberately restored old-body red receipt
with the final green run. Source/receipt inspection and a separate verification
resolved those bounded test claims (0.95/0.86 confidence). The remaining parity
claim was verified but still marked for review (0.56); the literal final
comparison receipts above retain both mismatches. No full-frontier completion
or original boot parity is claimed.

### Resource allocation history checkpoint (2026-10-20 continuation)

`re/headless/resource_history.py`, attached to the passive Observer, reads
allocator/recycler entries and returns plus runtime resource registration
descriptors. It never writes emulated state. The declared original startup was
run for 45M block-counted instructions with no native/factory replacement.
`re/out/headless/allocation-history/long-observed/original-facts.json` records
64 history events, no truncation and no pending allocator calls. There are no
recycles before the failing viewport allocation. Its ID is `0xffff0065`.
The captured 299-call unfiltered native prefix is byte-for-byte equal to the
preserved oracle prefix, including primitive results. The 64-event history
also repeats identically in a second run; see `repeat-verification.txt` in the
same directory (18,562 exact prefix bytes).

The eleven missing allocations before that viewport were two resource
duplicates (types 14 and 5), one RenderRef instance (type 3), two physics
instances (type 2), two text instances (type 3), and two camera type/instance
pairs (types 12 and 3). They now create registered containers, use the shared
allocator, repair existing handles, append parent-child links, and retain
instance templates. Duplicate's other-type alias branch consumes no ID.
The camera type root is resolved from the actual system.rpk header. Text uses
the native handle of its `renderinst` field, rather than a ptr on Text itself.
This is a headless identity projection; mesh/font copying, physics bodies,
renderer registration and full factory failure/lifetime branches are not exact.

The stock-menu regression first failed on record #167 (`0xffff0070` versus
`0xffff0065`). It now compares #167–169 against the preserved original and
also compares the original created-resource ID/type/parent prefix with
`re/out/boot/resource-identities.tsv`. MainMenu still completes at frame 179
and runs 420 idle frames. The next filtered mismatch is #202 `System.openLib`;
unfiltered #204 remains `Controller.user_Add`. Boot parity remains unaccepted.

Reproduce the history (the ignored graphics/media/desktop profiles must exist):

```sh
re/headless/run-boot.sh --limit 45000000 \
  --observe-dir re/out/headless/allocation-history/long-observed \
  --out re/out/headless/allocation-history/long.json \
  --calllog re/out/headless/allocation-history/long.slrlog
cargo test -p slr-boot --test stock_menu_prefix
re/boot-compare.sh re/out/headless/oracle-33d8aa8/observed/script-natives.slrlog
```

The new integration regression requires that local history capture as well as
the preserved native log. Its receipts are `re/out/boot/resource-history-red.txt`,
`resource-history-green.txt` and `resource-history-compare.txt`. Observer tests
exercise the real allocator/recycler with and without hooks, compare returned
IDs/free stacks, and check explicit truncation: receipt at
`re/out/headless/allocation-history/test-history.txt`.
The complete headless Python suite now passes 171 tests with no failures or
skips (`allocation-history/suite.txt`); workspace clippy also passes
(`re/out/boot/resource-history-clippy.txt`). Independent Standards and Spec
reviews found no concrete issue within the declared identity projection.
Workspace tests pass 932 tests, zero failures/ignored
(`re/out/boot/resource-history-workspace-tests.txt`, compact receipt
`resource-history-test-summary.txt`). Fresh node comparison remains at #19458
(`resource-history-steps.txt`). The final Jev gate verifies the bounded ID/graph
claim (0.83 confidence) but escalates patch quality on coverage/confidence
(safe-to-apply 0.25). Its history summary was flagged unsupported; that evidence
bundle omitted the explicit native-record count. The machine-checked complete
counts/repeat/prefix facts are now in `allocation-history/verification.json`.
No contradictions were reported. The gate's raw result is
`re/out/boot/resource-history-jev-gate.json`; it is not automatic quality approval.

## Fresh-agent verification and recovery pitfalls

The first continuation task is the filtered native mismatch in Frontier. Read `AGENTS.md`, `GLOSSARY.md`, ADRs 0004/0008/0009,
`docs/fidelity.md`, `docs/research/boot-sequence.md` and `docs/tufa-executor.md`
for constraints and implementation context. The temporary session handoff is
only an index to this canonical state.

`re/boot-compare.sh` prints the runtime's exit status and both comparisons but
its own shell exit status is not an acceptance gate. Read `compare.txt` and
`compare-all.txt`; runtime exit 0 currently coexists with native divergence.
`re/step-compare.py` compares only the common prefix, skips field initializers
unless `KEEP_FI=1`, and must use the documented DROP setting when reproducing
node #19458. Equal common prefixes alone do not establish complete trace parity.

All oracle captures, profiles, `re/out/native_api.tsv`, the stock install and
local oracle virtualenv are local-only dependencies; they are not recovered by
a fresh clone. Preserve `re/out/` and the retained oracle worktree. The four
copied oracle inputs were checked byte-for-byte against that worktree during
handoff preparation. Never commit captures or stock/reference data. The Python
suite is `~/slr-oracle-venv/bin/python -m unittest discover -s re/headless -v`;
missing local inputs can skip tests, so inspect its summary.

Runtime acceptance regression:
`cargo test -p slr-boot --test stock_menu_prefix`.
It runs both 600-frame success and 200-frame rejection and overwrites the
standard `re/out/boot/runtime*.calllog` outputs. Run the boot comparison again
after tests before interpreting those outputs.

### Package registration and controller mapping checkpoint (2026-10-20)

The passive `re/headless/package_history.py` observes original name registration,
package construction and header-load entry. It changes no emulated state.
`re/out/headless/package-investigation/full-observed/original-facts.json`
contains the discovery sequence; `repeat-observed` repeats its package history
identically. `headers-observed` records the earlier dependency chronology.
Original frontend links reserve garage/cars before `sl.rpk`. Loading police's
first foreign parent loads `parts.rpk`, reserving `cars/racers.rpk` at index 11
before the first engine package at index 12.

The port now reserves names independently of file availability, loads a named
resource's package headers during literal resolution, and interleaves each
header insertion with its required foreign-parent header load. It does not load
every linked package eagerly. `package-indices.tsv` exposes the resulting table.
The stock-menu regression compares all 48 names/indices and all 28 `openLib`
results directly with original captures. The original vehicle constructor order
now follows from the resource child lists; filesystem enumeration was unchanged.

`re/oracle/openlib_header_probe.py` executes the real native wrapper, package
registration/lookup and header loader over a declared empty valid RPK. Both
first and repeated open return the registered index without local resource 1.
Earlier wrapper-only vectors replaced the lookup helper and therefore did not
measure its own header-loading effect. The focused Rust replay also removes the
synthetic file after loading, checking the retained-header fast path. Original
file I/O and allocation remain controlled seams in this function probe.

`re/oracle/controller_mapping_probe.py` runs original user_Add/Del/Reset wrappers
and mapping-table operations in a declared loaded native player. The 166-operation
Rust replay checks mapping results, counts and retained slot words, including
duplicate keys, deletion/reuse, reset, invalid indices and 152-slot capacity.
Boot resolves the actual loaded native player resource; aliases share its table.
The pointer lookup, resource acquisition and virtual data accessor are controlled
probe seams; device samples, axis metadata, force/smoothing, player commands and
full lifetime behavior remain unaccepted.

Receipts: `re/out/boot/package-indices-red.txt`,
`package-controller-workspace-final.txt` (936 tests),
`package-controller-clippy-final.txt`, `package-controller-compare-final.txt`
and `package-steps-compare.txt`. The headless Python suite passes 185 tests with
no skips (`package-controller-python-final.txt`). Independent reviews checked
the package/header integration, controller projection and passive menu observer;
the original-helper and ready/waiting/bottom-frame issues found during review
were corrected before this checkpoint. The Jev gate verifies the bounded
runtime/frontier, original endpoint and suite claims but escalates patch quality
(safe-to-apply 0.23, low coverage confidence). Its package-count claim required
the actual regression source and table check, then verified at 0.66 confidence;
the literal test and table were reviewed directly. No contradictions were
reported by the gate. Raw receipts are `package-controller-jev-{gate,verify}.json`;
they are not automatic quality approval. The strengthened stock-menu test retains
the existing resource-ID/ownership checks and short-budget rejection. Native
comparison now reaches filtered #446/unfiltered #646; the node comparison reaches
#65018. MainMenu still completes at frame 179 with 420 idle frames. Full parity
remains unaccepted, and the timer/clock and finalizer frontiers above remain open.

The preserved oracle call logs extend beyond their stale `original-facts.json`
sidecar. Fresh isolated original runs reproduce their complete native prefix;
the receipt is `re/out/headless/menu-continuation/preserved-prefix-comparison.json`.
Treat that older sidecar as a shorter observation, not as the log's coverage
certificate.

The fresh paired `menu-continuation/menu-final` run now establishes the original
acceptance endpoint under the unchanged declared platform scenario. The passive
`menu_history.py` reads GameLogic's actualState, validates its class-variable
table, and walks both ready and waiting thread frame lists. It rejects unknown
frames except original-constructor-proven empty bottom frames and excludes
pending MainMenu/MainMenuDialog/OptionsDialog frames. The same MainMenu object is
complete after frame 31 and has 300 subsequent idle frames at frame 331.
`verification.json` records the matching native-log cutoffs: 6,508 filtered and
9,209 unfiltered records. The exact byte slices at `menu-final/accepted/` are the
current comparison inputs; their digests and source-run limits are recorded in
that verification file. Completed stock menu-construction return nodes corroborate
the observer in `menu-node-bounds.json`. This establishes the original endpoint,
not port parity. Keep all earlier captures; no oracle inputs were changed.
