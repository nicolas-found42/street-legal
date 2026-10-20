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

Unchanged stock scripts execute from `Config.<init>` through `java.game.Init.<init>`
(Frontend, LoadingScreen/SimpleLoadingDialog, input controllers, mouse cursor,
`GameLogic.<init>` with its package scans and vehicle types), then the scheduler
pump runs frames. By frame 179 a timer event reaches
`SplashScreen.handleEvent -> GameLogic.changeActiveSection -> MainMenu.enter ->
MainMenuDialog.show` (stock MainMenu entered). The run currently stops inside
`OptionsDialog.show` building a menu (see "Frontier").

MainMenu is **entered, not reached**: the menu is not fully built and the loop has not
settled in its idle state. Parity is still 0/30.

## Evidence and its limits

* **Original oracle (strongest available):** the stock exe under Unicorn with declared
  Win32/D3D9/DirectSound/DirectInput scenario shims (`re/headless/`, branch
  `worktree-agent-a20d568c5b681332f` until merged). It executes the original
  `Thread::run` over the same stock scripts and records the script-to-native call
  log. Run with `re/headless/run-boot.sh`; log at
  `re/out/headless/boot/observed/script-natives.slrlog`, facts in `original-facts.json`.
  The Config and GameInit resource IDs the original assigns are 0xFFFF0080 (Config)
  and are the values used above; this replaces the earlier supplied guess for Config.
* **Comparison:** `slr-calllog-diff --mask-refs re/out/native_api.tsv orig port`. Reference
  words (objects, Strings) are masked because they are allocator addresses in the
  original; primitives are compared bit for bit. Both streams exclude the render,
  sound and input capture classes. Last comparison: first divergence at record 97
  (`ResourceRef.scaleMesh` floats: the port's Viewport getters return 0).
* The scheduler, `Thread.start`, event/timer model, resource instance creation, header
  registration for other packages, `openLib`, `FindFile`, `File`, string concatenation
  and others were generalised from probed special cases **without per-function original
  vectors**. They are validated only by the whole-run comparison above, which is
  incomplete. They are not exactness claims.

## Declared scenario inputs (not observed in the original unless stated)

* Simulated clock: ms counter advancing 0.01 ms per read, +1000/60 ms per frame.
* Frame loop: fixed frame interval 1/60 s, world max step 0.01 s, time scale 1.0
  (`engine::FrameConfig`). Ask the oracle for the observed values (requested).
* Input devices: `Keyboard`, `Mouse` only (`INPUT_DEVICES` in `slr-boot/src/main.rs`).
* Headless render/sound/input natives are capture-only and answer zero; the 170 native rows no
  engine slice implements (`missing_rows.rs`) are capture-only too.
* Resource materialisation checks the descriptor's source file exists and marks the
  container loaded; nothing is decoded and original file I/O records are absent.
* Native object classes named by `native <name>` descriptors (`ground`, `player`, ...)
  create no engine object.
* Missing linked packages (stock links `obstacles\*.rpk`, which the media lacks) get
  declared placeholder roots.

## Frontier (next stops, in order)

1. `Viewport` natives must return the created viewport's values (port of the hit arm of
   `0x4817b0..0x481970`, state set by `Viewport.create`), so Osd layout is non-degenerate.
   This is also the first call-log divergence.
2. `OptionsDialog.show -> MultiChoice.setValue -> Vector.elementAt(negative)`: an option list
   is empty or an index native answers 0 (display modes, texture quality, ...).
3. Then whatever stops next; `re/boot-compare.sh` prints the stop and the first divergence.

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
