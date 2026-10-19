# Original boot sequence (static evidence)

Compiled from read-only Ghidra analysis of build 606 (`re/out/all_pseudo.c`, 4,356
functions). Addresses are evidence-backed unless marked `[INFERENCE]`. Static
evidence ends where noted; the headless oracle (`#13`) must pin the rest
dynamically. No Wine.

## Chain

1. `WinMain` `0x58c700`: CD check `FUN_00401000` (not ported; checksummed `stock/`
   replaces it), RPK load `FUN_00538380` (`s_system_rpk`), settings, then
   `FUN_00427980()`.
2. `FUN_00427980(param)`: allocate 0x80-byte game context (`FUN_0054f560`),
   construct JVM into `param+0xe4` (`FUN_0040fb50`), then in order:
   - `FUN_0040fe70` loader boot: 8 package roots, 5 primitive descriptors,
     eager requests for `java.lang.String` and `java.util.resource.GameRef`
     ONLY (everything else is requested lazily at runtime).
   - `FUN_00487f20`, `FUN_0046b530`, `FUN_004427e0`: the three native
     registration batches (all 353 rows).
3. Main loop (gate `DAT_0062f270`), per frame:
   - `FUN_0054dda0` input/device poll.
   - `FUN_00428450` physics/object-list step + GC cleanup (internals
     `FUN_00427160`/`FUN_004277f0`).
   - `FUN_00418d10(jvm, 10.0f)` scheduler pump: walks the thread list
     (jvm+8..), dispatches each thread through `FUN_004198b0` (reference
     propagation/queue helper) into `Thread::run` `0x420ff0` with a float
     budget; returns leftover time. Thread state words: `thread+0x2c` flags
     (0x4 running, 0x10 waiting, 0x20 notified, 0x40 stopped, 0x80 dead);
     frame+0x28 operand stack, frame+0x18 locals.
   - `FUN_00426fd0` frame pacing (QPC-scaled clock float at context+0x108;
     `CounterClock` `0x551690`/`0x5516c0` behind it).
   - First iteration only: `FUN_0053a5e0(&DAT_00636460, ..., "GameInit")`
     resolves the RPK config entry named `GameInit`; `FUN_004290f0` links the
     returned node into the game context list (context+0x4/8/0xc; slot
     +0xf0/+0xf4). Missing GameInit ⇒ slot 0 ⇒ no game context; the loop still
     pumps (zero threads).

## First script execution (open boundary)

Static evidence ends at GameInit resolution + list link. No call in the boot
window dispatches script code (`41fbc0`/`4207c0`/`404d40`) or Thread
`init`/`start` (`LAB_0047c510`/`LAB_0047c570`); those natives have no static
call sites at all — `GameRef.create` (`LAB_0047d7b0`) / `create_native`
(`LAB_0047d900`) and `Thread` natives are invoked from script frames only. The
first script thread is therefore reached through a dynamically selected path
(candidates: `FUN_00428450`'s object processing, RPK node activation).
Dynamic evidence (headless oracle reaching script execution) must pin it.

## Gametype mechanism

RPK text chunks `gametype 0xHHHHLLLL` (map id in the high word; 160 distinct in
stock, parsed in the `FUN_0053dee0` RSD branch via `FUN_00544590`/`FUN_00546070`)
build the gametype table. The main loop's `GameInit` config key selects one
entry. No static reference to `java.game.Init` or `java.game.GameState` exists
in native code; `sl/Scripts/game/Init.class` is plausibly script-referenced only
`[INFERENCE]`.

## Port implications (boot driver)

- Replicate the `FUN_00427980` order: context/JVM construction, loader boot,
  three native batches; render/sound/input natives use the M1-12 logging hosts.
- RPK load via `slr-fmt-rpak`; resolve config `GameInit` the way `FUN_0053a5e0`
  does; link the node like `FUN_004290f0`.
- Pump the scheduler with the 10.0f budget and QPC pacing through
  `slr-platform` seams; record everything to the call log.
- The GameInit→first-thread bridge is the first runtime question: run and
  observe (`HostError::NoBody` reports name the missing native exactly).
