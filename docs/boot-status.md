# Boot to MainMenu: canonical state and recovery

Goal: unchanged stock scripts, run by the Rust runtime, reach MainMenu, with an
inspectable comparison against the strongest available original oracle.
This file is the handoff. Update it at every milestone; do not keep a second copy.

Last updated: 2026-10-20, branch `exact-port`.

## What runs now

```sh
cargo run -p slr-boot -- stock --config-instance-id 0xffff0080 --game-init-instance-id 0xffff007f
re/boot-compare.sh [original.slrlog]     # boot + call-log diff against the oracle
```

Unchanged stock scripts execute Config and GameInit, enter MainMenu at frame
179, finish the formerly failing OptionsDialog path, and run all 600 scheduler
frames without a script/native stop. The driver still returns its conservative
`MainMenuBoundary` at the frame budget; automatic completed-dialog/idle detection
has not been implemented. This is runtime progress, not boot parity acceptance.
Parity is still 0/30.

## Definition of done

MainMenu is entered, its dialog finishes building, and at least 300 idle frames
run without a stop. The filtered native log must match the headless original
through that point. Unfiltered/node divergences must be fixed or explicitly
listed here. A clean test suite alone does not satisfy this definition.

## Evidence and its limits

* **Original oracle (strongest available):** the stock exe under Unicorn with declared
  Win32/D3D9/DirectSound/DirectInput scenario shims (`re/headless/`, branch
  `worktree-agent-a20d568c5b681332f`, preserved at `33d8aa8`; merge approved). It executes the original
  `Thread::run` over the same stock scripts and records the script-to-native call
  log. Run with `re/headless/run-boot.sh`; log at
  `re/out/headless/boot/observed/script-natives.slrlog`, facts in `original-facts.json`.
  The Config and GameInit resource IDs the original assigns are 0xFFFF0080 (Config)
  and are the values used above; this replaces the earlier supplied guess for Config.
* **Comparison (current checkpoint):** command delivery moves the first node
  divergence from #5061 to #19458 (with `DROP=java.lang.GameType.getInfo`, field initializers skipped by
  default; comparison covers a common prefix, not equal-length acceptance):
  `Prime_VT.<init>` in the port versus `Emer_VT.<init>` in the original.
  Filtered native divergence is #167: `ResourceRef.id`, original nonzero ID
  versus port zero (render-created resources are still absent). Unfiltered
  divergence is #204: `Controller.user_Add` answers 0 rather than mapping ID 1.
  The earlier device-name and Viewport frontiers have been superseded.
* **Validation:** oracle branch Python suite: 169 tests, no failures/skips.
  Rust workspace tests and clippy pass at the command/display/token checkpoint;
  array overload regression and final workspace recheck run before commit.
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
* Remaining headless render/sound/input natives are capture-only and answer zero; the 170 native rows no
  engine slice implements (`missing_rows.rs`) are capture-only too.
* Resource materialisation checks the descriptor's source file exists and marks the
  container loaded; nothing is decoded and original file I/O records are absent.
* Native object classes named by `native <name>` descriptors create no full engine
  object. The `player` command path now synchronously invokes script `setcontrol`
  (`FUN_0047a920`); other player commands remain capture-only.
* Missing linked packages (stock links `obstacles\*.rpk`, which the media lacks) get
  declared placeholder roots.

## Frontier (next stops, in order)

1. Implement acceptance observation (completed MainMenu dialog + >=300 idle
   frames); the driver currently reports only budget exhaustion after 600 frames.
2. Filtered native record #167: render-created resource IDs and original allocation
   history. Do not mask these primitive ID results to manufacture acceptance.
3. Unfiltered #204: controller device mapping creation (`user_Add` currently
   follows the no-device branch). Native player `device` commands remain capture-only.
4. Node #19458: vehicle-type enumeration order; determine its resource/package
   ordering before changing filesystem enumeration.
5. General interface-array conversion remains an explicit VM boundary. The
   class/primitive-array candidate path now rejects unrelated class overloads
   rather than trying to load an array as a class file (original array CCDs have
   Object as their base). This is not general array covariance support.

## Known gaps behind the happy path

* Original allocation history for resource IDs beyond Config; ours are `--game-init-instance-id`
  and a fresh allocator.
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

* Remote `origin/exact-port` holds all integrated work. The oracle agent works on a separate
  branch/worktree (`.claude/worktrees/agent-*`, ignored via `.git/info/exclude`).
* `stock/` and `reference/` are read-only; derived outputs stay under `re/out/`.
* No Wine. Issue #1 and `docs/plan.md` remain the umbrella; #110 tracks VM completion.

## Checkpoint receipts (2026-10-20 resume)

Ignored `re/out/boot/steps-port-command.txt` reproduces node prefix improvement;
`steps-port-token.txt` records the next array-overload stop;
`array-stop.txt` records 600 completed frames after the array fix.
`token-red.txt` / `token-green.txt` show the argument-order regression and fix.
Oracle `re/out/headless/suite-accepted.txt` records the 169-test pass in its
worktree. That worktree and its derived captures must be retained.

No per-function oracle vector yet covers synchronous command dispatch; its
current regression evidence is the integrated node/native comparison and
`cargo test -p slr-boot --test stock_menu_prefix`. That regression checks
Controller.setcontrol, both menu methods returning, and 600 completed frames;
it intentionally does not claim original parity. Native
player mask initializes to observed EVENT_COMMAND (0x10) and uses script
set/clear operations; native object deletion/lifetime remains incomplete.
