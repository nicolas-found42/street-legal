# Street Legal Racing: Redline, in Rust

A playable recreation of *Street Legal Racing: Redline* built from the shipped game data
(build 606, v2.2.1) in `reference/raw` (read-only; never modified by this project).

**Honest status: this is a faithful-data, approximate-simulation vertical slice, not a complete
port.** Real files drive the content (24 cars, 29 tyres, 14 engine blocks, real body meshes,
real drivetrain numbers); the physics, career and tracks are new implementations. See
[docs/fidelity.md](docs/fidelity.md) for exactly what is decoded, approximated or missing.

## Run

```bash
cargo run --release -p slr-game            # needs reference/raw next to the workspace
cargo run --release -p slr-game -- --autotest --time-scale 4   # scripted UI end-to-end test + screenshots
cargo test --workspace                     # unit + corpus tests (corpus tests skip without reference/raw)
cargo run -p slr-core --example calibrate  # performance table for all 24 cars
```

Flags/env: `--reference DIR` / `SLR_REFERENCE`, `--save-dir DIR` / `SLR_SAVE_DIR` (default `./saves`).

## Controls

W/S throttle and brake (hold S when stopped for reverse), A/D steer, Space handbrake,
Q/E shift (manual gearbox, set in Tuning), C camera, R reset car, Esc pause. Garage: mouse or
Up/Down/Enter, number keys 1-7 switch tabs.

## Play loop

Garage (overview, parts, tuning, mechanic, painter, car lot) -> test drive or event race ->
results -> repair or modify -> autosave; Continue on the title screen reloads the save.

## Layout

- `crates/slr-formats`: parsers for RPAK, SCX, TUFA, cfg, SDAT, verified on all 8,411 reference files.
- `crates/slr-core`: catalog, vehicle simulation, garage/economy, races, AI, saves. Headless.
- `crates/slr-game`: macroquad frontend (3D world, HUD, garage UI, self-test).
- `docs/`: inventory, formats, fidelity matrix, architecture, ADRs.
