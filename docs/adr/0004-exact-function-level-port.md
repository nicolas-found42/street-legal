# 0004: Exact function-level port, verified by an oracle

Supersedes 0001 and 0003. The goal is an exact replica, not a faithful-data approximation.

- **Exact** means a function-by-function port of `StreetLegal_Redline.bak.exe` (build 606, v2.2.1):
  same results for the same inputs, including x87 float behaviour where it affects the simulation.
  Byte-matching recompilation is out of scope. The patched `.exe` and mods are out of scope.
- **Scripts run unchanged.** A Rust TUFA VM executes the original `.class` modules. Only methods
  that are native in the exe are ported by hand, so game logic comes from data, not from us.
- **Assets are read at runtime** from a user-supplied install (`--reference`). Nothing derived
  from the original is committed.
- **A function is exact only when a differential test backs it.** Layer 1: emulate the exe
  function with Unicorn and compare bit for bit against the Rust port. Layer 2 (best effort):
  run the real exe under Wine in Docker and compare whole-game traces.
- **Order:** TUFA VM and native stubs, vehicle simulation, collision and damage, game flow, rendering,
  audio. `slr-formats` is kept; the approximate `slr-core` modules are replaced as ports land.
- `docs/fidelity.md` becomes a per-function ledger (address, Rust function, status, evidence).

Why: the previous slice was playable but invented its physics, AI, economy and tracks. That cannot
be called a replica, and the trade-off (less visible progress early) is accepted for provable fidelity.

## Rescope (after research, see docs/research/rescope.md)

Exactness is tiered; each tier has its own acceptance test.

| Tier | Areas | Meaning of exact | Acceptance |
|---|---|---|---|
| A | RPAK, SCX, TUFA .class, cfg, PTX/RDB/SDAT readers | byte-for-byte parse, round trip | whole `reference/raw` corpus |
| B | TUFA VM (`Thread::run`) and the 339 natives | same stack/values/native-call sequence for the same script | per-opcode and per-native differential tests keyed by exe address |
| C | vehicle physics natives (`WheelRef`, `Part`, `Chassis`, `PhysicsRef`, `GroundRef`) | 0 ulp where the oracle can show it, else documented tolerance | Unicorn per-function vectors; x87 precision modelled only where f32 diverges |
| D | rendering, audio, input, windowing | behaviourally equivalent, not bit-exact | screenshot/behaviour tolerance |
| E | saves | JSON by design (ADR 0002) | round trip |

Open risk to settle first: x87 precision. D3D9 may leave the FPU at 24-bit precision unless the device is created
with FPU_PRESERVE; check the exe's CreateDevice flags before trusting any float comparison.
Dev-only tools (Unicorn, reccmp ideas) are never linked into the shipped crates (licence hygiene).

## Plan decisions (grilling round 1, all recommendations accepted)

- **Complete** = stock single-player feature parity: every item of a written checklist in `docs/fidelity.md`
  passes its tier's acceptance test. Mods, multiplayer and the unreadable `Dialog.class` are out of scope.
- **Order changed:** VM plus a headless boot to the main menu (render/sound/input natives log calls) comes
  first, then physics fed by the real scripts, then the rest.
- **Oracle:** spike Wine in Docker running the stock exe; fall back to a "headless original" (stock exe code
  under Unicorn with Win32/D3D9/DirectSound stubbed). "Verified end-to-end" is only claimed where one of
  them ran; never silently fall back to per-function-only.
- **Saves:** exact SDAT read/write (supersedes ADR 0002), because the scripts save through natives.
- **Rendering:** wgpu with a D3D9 fixed-function state emulator (shader generation), accepted by screenshot.
- **Source of truth for data:** the clean v2.2.1 installer media in `reference/raw-stock`. The modded
  `reference/raw` install is a compatibility corpus only (its core scripts and `parts.rpk` were replaced
  by mods without backups).
- **Tracking:** current work committed on a branch; one GitHub milestone per tier/feature and one issue per
  subsystem, labelled `ready-for-agent`.
