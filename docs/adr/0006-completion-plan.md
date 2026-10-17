# 0006: Definition of complete, milestone order, oracle policy and renderer

Amends 0004 (grilling round, 2026-10-17).

- **Complete** means stock single-player parity: every feature of the stock game is reachable and driven
  by the original scripts. That covers the menus and options; Career (dealer, used-car lot, garage, part
  catalog, install/tune, day street races, night club drags, pink slips, police); Quick Race and Free Ride on
  every stock map; and all stock cars and parts. Multiplayer (stock has none), mods, and the unreadable
  `Dialog.class` (version 0x108f5) are out of scope. The checklist lives in `docs/fidelity.md`, one row per
  item, each linked to the acceptance test of its tier.
- **Milestone order:** (1) VM plus headless boot: the original scripts run from engine start to the main menu,
  with render/sound/input natives recorded as a call log instead of executed; (2) physics, fed by the real
  vehicle and part scripts; (3) everything else (maps, textures, rendering, audio, saves).
- **Oracle policy:** spike the real exe under Wine in Docker first. If that fails, build a headless original
  (the exe's own code under Unicorn with Win32/D3D9/DirectSound stubbed) only as far as the VM and physics
  loop need. "Verified end-to-end" is claimed only where one of those two actually ran. Falling back to
  per-function tests alone is never silent.
- **Renderer:** wgpu with a shader generator that emulates the D3D9 fixed-function state the exe uses.
  macroquad is retired. Acceptance is by screenshot comparison.
- **Work tracking:** GitHub milestones per tier/feature and one issue per subsystem, labelled
  `ready-for-agent`, so separate sessions can work in parallel.
