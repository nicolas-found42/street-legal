# Architecture

```
slr-formats  (bytes -> typed data, no game logic)
     ^
slr-core     (catalog, vehicle, condition, tyre, engine, track, ai, race, state, perf, visual)
     ^
slr-game     (macroquad: render3d, ui, session, app, autotest)
```

- Core is headless and deterministic: fixed-step vehicle sim (60 Hz outer, 360 Hz inner substeps
  with a semi-implicit wheel integrator), all state serialisable (serde JSON).
- The frontend only converts input to `Controls`, draws state, and calls `GameState` actions.
- `--autotest` drives the real UI with injected clicks and asserts on `GameState`
  (11 checks: new game, tyre fit, repair, paint, tuning slider, event race, settle, autosave
  round-trip, test drive).
- Decisions: see `docs/adr/`.
