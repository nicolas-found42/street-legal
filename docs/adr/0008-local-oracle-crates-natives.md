# 0008: Local-only oracle, ~20 subsystem crates, all 353 natives

Amends 0004, 0006 and 0007 (grilling round 3, 2026-10-17).

- **Everything runs on the developer's Apple Silicon Mac. No VMs, no Docker, no x86 machine.** This
  replaces the Wine-in-Docker spike of 0006. There is no real x87 hardware to compare against, so the x87
  reference is triangulated: Berkeley SoftFloat's 80-bit type with precision control (independent of QEMU),
  Unicorn, and Rosetta 2 (the exe under macOS Wine, which is not a VM) run the same probe suite. An
  operation is trusted where they agree with the Intel SDM; disagreements are resolved from the SDM and
  documented. Transcendentals are pinned to one documented implementation.
- **Whole-game oracle:** the real stock exe under macOS Wine with a hook layer, recording call logs and VM
  state. Float values in those traces count as evidence only for operations the probes trust. Unicorn
  remains the per-function oracle.
- **Crates, by subsystem (~20):** format crates per family (`slr-fmt-rpak`, `-scx`, `-cfg`, `-tufa`,
  `-sdat`, `-texture`, `-sound`); `slr-x87`; `slr-vm`; native crates per group (`slr-lang`, `slr-io`,
  `slr-resource`, `slr-physics`, `slr-world`, `slr-render-natives`, `slr-sound`); engines (`slr-d3d9`
  wgpu fixed-function emulator, `slr-audio`, `slr-input`, `slr-platform` with window/clock/rng seams);
  `slr-calllog` (trace format and headless backends); `slr-ledger` (generates the fidelity ledger from
  exe-address annotations plus local test results); `slr-game` (the binary). Oracle tooling stays under
  `re/`, outside the workspace.
- **All 353 registered natives are ported**, not only the 216 that stock scripts reference. (The earlier
  338-row table missed 15 registrations because of a regex bug.)
- **Input:** keyboard, mouse and gamepad/wheel through the original ControlSet scripts; no force feedback.
