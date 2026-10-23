# Playable frontend evidence for the gameplay comparison

Captured for the 2026-10-22 comparison of this repository with *Street Legal Racing: Redline* gameplay videos. This note covers the local playable frontend only; the [main comparison](gameplay-comparison.md) and video notes carry the external-video evidence.

## What was actually run

The executable is `target/release/street-legal`, package `slr-game`, the macroquad frontend. This distinction matters: its source explicitly calls it the legacy frontend retired by ADR 0006. The repository's exact reconstruction is a separate, incomplete effort; the canonical status reports that no complete game, native display/audio, or human review pass is claimed. These captures establish the visible behavior of the playable approximation, not stock parity. Sources: [frontend entry point](../../crates/slr-game/src/main.rs#L1), [ADR 0006](../adr/0006-completion-plan.md), [canonical state](../boot-status.md), [fidelity checklist](../fidelity.md).

The release build succeeded. The native end-to-end autotest then exited successfully with **11 checks passed, zero failed**, exporting **11 distinct 2560 × 1440 screenshots**. The diagnostics receipt reports `status: completed`, `truncated: false`, 43 recorded states, and 1,173 recorded events. The launch log reports verification of the clean stock install: 7,858 files and 458,923,903 bytes. The title screen separately reports content loaded from the modded compatibility install `reference/raw`: 24 cars, 14 engines, and 29 tyres. Verification of stock files does not mean the playable frontend runs the stock scripts. Sources: [run log](../../re/out/gameplay-comparison-2026-10-22/local/autotest.log), [autotest receipt](../../re/out/gameplay-comparison-2026-10-22/local/diagnostics/autotest.json), [completion receipt](../../re/out/gameplay-comparison-2026-10-22/local/diagnostics/capture-end.json), [title capture](../../re/out/gameplay-comparison-2026-10-22/local/shots/01_title.png), [loading code](../../crates/slr-game/src/main.rs#L102), [glossary](../../GLOSSARY.md).

All observations below were checked against the actual image contact sheet. Garage overview, tuning, race HUD, results, and test drive were also opened individually at large size. The contact sheet is a derived browsing aid, not an additional gameplay screenshot: [view all eleven captures](../../re/out/gameplay-comparison-2026-10-22/local/contact-sheet.png).

## Screenshot inventory and observations

All image links below are local ignored evidence under `re/out/`; the files are intentionally outside version control.

| Capture | What the image establishes | Relevant source |
|---|---|---|
| [01 Title](../../re/out/gameplay-comparison-2026-10-22/local/shots/01_title.png) | Minimal title layout with Continue, New Game and Quit; loaded-content counts and build-606 label. | [draw_title](../../crates/slr-game/src/app.rs#L476) |
| [02 Garage overview](../../re/out/gameplay-comparison-2026-10-22/local/shots/02_garage_overview.png) | Red Duhen SunStrip in an otherwise empty tiled presentation space. Right panel holds specifications, simulated performance, four condition bars, and Test drive / Save / Title. Initial money is $2,500, reputation 0, day 1. | [garage scene](../../crates/slr-game/src/app.rs#L562), [overview panel](../../crates/slr-game/src/app.rs#L751), [new game](../../crates/slr-core/src/state.rs#L210) |
| [03 Parts / tyres](../../re/out/gameplay-comparison-2026-10-22/local/shots/03_parts_tyres.png) | Text rows with size, rim and Fit price; Front / Rear / Both choice. This is a list-based upgrade purchase interface. | [parts tabs](../../crates/slr-game/src/app.rs#L878), [tyre list](../../crates/slr-game/src/app.rs#L1013) |
| [04 Mechanic](../../re/out/gameplay-comparison-2026-10-22/local/shots/04_mechanic.png) | Separate engine, four body-side, brake and four tyre condition bars; grouped repair and tyre-replacement buttons. | [mechanic panel](../../crates/slr-game/src/app.rs#L1343) |
| [05 Painter](../../re/out/gameplay-comparison-2026-10-22/local/shots/05_painter.png) | Blue car after a $150 respray. Twelve named swatches; whole-car repaint selection. | [painter panel](../../crates/slr-game/src/app.rs#L1443), [paint table](../../crates/slr-core/src/state.rs#L18) |
| [06 Tuning](../../re/out/gameplay-comparison-2026-10-22/local/shots/06_tuning.png) | Four sliders: final drive, front brake bias, rear wing and differential lock. Automatic/manual toggle, reset button and simulated performance feedback. Final drive has changed to 0.89×; differential control is disabled until an LSD is bought. | [tuning panel](../../crates/slr-game/src/app.rs#L1217) |
| [07 Cars / showroom](../../re/out/gameplay-comparison-2026-10-22/local/shots/07_cars.png) | Showroom, Used lot and My cars subtabs. Captured showroom is a scrollable list of models with Buy prices. It is not a captured physical dealer environment. | [car tabs](../../crates/slr-game/src/app.rs#L1494), [showroom](../../crates/slr-game/src/app.rs#L1545) |
| [08 Events](../../re/out/gameplay-comparison-2026-10-22/local/shots/08_events.png) | Six fixed events, beginning with Street Drag: Rookie. Rows show opponents, fees, prizes, reputation rewards and locks. | [event panel](../../crates/slr-game/src/app.rs#L1731), [event definitions](../../crates/slr-core/src/state.rs#L115) |
| [09 Race HUD](../../re/out/gameplay-comparison-2026-10-22/local/shots/09_race_hud.png) | Daytime two-car drag on a flat, barrier-lined straight with block trees. Rear chase camera, top-left race timing/position, top-right route, bottom-left condition bars and bottom-right gear/RPM/speed gauge. Captured player speed is 62 km/h. | [session drawing](../../crates/slr-game/src/session.rs#L224), [HUD](../../crates/slr-game/src/session.rs#L290) |
| [10 Results](../../re/out/gameplay-comparison-2026-10-22/local/shots/10_results.png) | Defeat, P2 of 2, 18.62 seconds, $0 fee, $53 prize, +0 reputation, and Back to garage. The test exercises a completed race and settlement rather than requiring a win. | [overlays](../../crates/slr-game/src/app.rs#L1820), [race settlement](../../crates/slr-core/src/state.rs#L567) |
| [11 Test drive](../../re/out/gameplay-comparison-2026-10-22/local/shots/11_test_drive.png) | Driving alone around the procedural circuit at 90 km/h; the route map differs from the drag strip. | [test drive](../../crates/slr-game/src/app.rs#L337), [circuit construction](../../crates/slr-core/src/track.rs#L79) |

## Comparison implications supported by local evidence

**There is a working condensed loop.** New game, tyres, repair, paint, tuning, event entry, driving, results, return to garage and save/reload all ran in one native session. The test asserts money decreases for tyres and repairs, repaint changes the stored colour, final drive changes, race count advances, tyre wear persists, the saved state reloads identically, and test-drive speed exceeds 20 km/h. This is useful functionality to retain while judging the much larger gap to original-game footage. Source: [the actual autotest script](../../crates/slr-game/src/autotest.rs#L34) and its [receipt](../../re/out/gameplay-comparison-2026-10-22/local/diagnostics/autotest.json).

**The current visual result is deliberately simplified.** The garage is an orbiting car on checker tiles with a large uniform information panel. Driving scenery consists of generated road strips, barriers, ground tiles, cuboid trees and light poles. The renderer creates untextured meshes (`texture: None`) and emits colour-shaded car triangles; tyres and spoked wheel faces are generated geometry. Therefore adding more video samples will not explain away the visible absence of texture detail and the sparse environment. Sources: [garage image](../../re/out/gameplay-comparison-2026-10-22/local/shots/02_garage_overview.png), [race image](../../re/out/gameplay-comparison-2026-10-22/local/shots/09_race_hud.png), [mesh pipeline](../../crates/slr-game/src/render3d.rs#L60), [car and wheel emission](../../crates/slr-game/src/render3d.rs#L224), [world construction](../../crates/slr-game/src/render3d.rs#L315).

**Car silhouettes use real body-panel data, with a reduced material model.** The visual loader assembles panel SCX geometry at CFG attachment slots. Its material categories are Paint, Glass, Light and Dark, inferred from part names. The render path has no full original material or texture stage reconstruction. Body damage in this drawing path darkens the paint colour; it does not deform vertices. Sources: [visual assembly](../../crates/slr-core/src/visual.rs#L1), [material types](../../crates/slr-core/src/visual.rs#L15), [paint damage effect](../../crates/slr-game/src/render3d.rs#L224).

**The garage interaction model is menu-based.** The frontend has seven tabs, not a general part-slot workshop interaction model. Parts offers whole-engine swaps, tyre fitment and staged power/LSD upgrades; mechanic performs grouped condition restoration; painter changes the car's paint index. Individual part installation/removal, decals and all stock tunable parameters remain open in the project's stock-parity checklist. Sources: [tabs](../../crates/slr-game/src/app.rs#L43), [engine swaps](../../crates/slr-game/src/app.rs#L930), [upgrade stages](../../crates/slr-game/src/app.rs#L1121), [mechanic](../../crates/slr-game/src/app.rs#L1343), [painter](../../crates/slr-game/src/app.rs#L1443), [P06–P10 and P26](../fidelity.md).

**The driving environment and progression are new implementations.** The local tracks are the generated drag strip and circuit; the event list is six static definitions. Vehicle dynamics in this frontend are planar. Camera, speed and timing in the screenshots are useful visual evidence, but they cannot establish original physics fidelity, original-city navigation, traffic, police, pedestrians, or the original career progression. The original single-player obligations remain separately open. Sources: [track construction](../../crates/slr-core/src/track.rs#L69), [event list](../../crates/slr-core/src/state.rs#L115), [vehicle model](../../crates/slr-core/src/vehicle.rs#L1), [ADR 0003](../adr/0003-procedural-tracks-and-engines.md), [P11–P25](../fidelity.md).

**Current saves establish frontend persistence only.** The successful reload check compares this frontend's JSON save with its in-memory state. The final isolated save has one race, zero wins, day 2, blue paint, changed tyre sizes, and $140. It does not prove original SDAT compatibility. Sources: [save/load implementation](../../crates/slr-core/src/state.rs#L616), [isolated save](../../re/out/gameplay-comparison-2026-10-22/local/saves/slot1.json), [save-reload assertion](../../crates/slr-game/src/autotest.rs#L178), [P03 and save gate](../fidelity.md).

## Limits and process handling

The capture script covers the title, seven garage tabs, one daytime drag race through its loss result, and a short circuit test drive. It does not capture engine/upgrades subtabs, used-lot/My cars subtab contents, purchasing a car, night driving, a circuit race result, pause, alternate cameras, collision damage, manual shifts, keyboard/controller driving quality, or audio. Source inspection can describe implemented paths, but those paths were not demonstrated by this run. The AI autopilot replaces human driving input and `--time-scale 4` accelerates the session; these are not normal-play handling measurements. Sources: [autotest schedule](../../crates/slr-game/src/autotest.rs), [autopilot input](../../crates/slr-game/src/session.rs#L88).

An additional isolated manual process was briefly launched to seek more native views. Computer Use could not bind the unbundled executable by either its window title or full binary path, and the app inventory did not expose it. No additional screenshots or manual-control observations were claimed. That process was stopped via its own exec session (exit 130). A subsequent exact process-name check found no running `street-legal` executable. PID 9274 from the earlier task was already absent before this capture work; no user process or user save was stopped, overwritten or edited.

No production source was changed for capture, and no workspace-wide tests were claimed. A Jev verification call was not available in the active tool inventory; the parent also reported that its configured Jev model endpoint was failing with 403. The concrete runtime receipts and direct visual review above are the evidence used here.

## Reproduce

Run from the repository root, choosing a fresh output directory to preserve this receipt:

```bash
cargo build --release -p slr-game
target/release/street-legal \
  --autotest --time-scale 4 \
  --save-dir re/out/gameplay-comparison-2026-10-22/local/saves \
  --shots re/out/gameplay-comparison-2026-10-22/local/shots \
  --diagnostics re/out/gameplay-comparison-2026-10-22/local/diagnostics \
  > re/out/gameplay-comparison-2026-10-22/local/autotest.log 2>&1
```

The local checkout was at HEAD `7e2d2cb56b7e4976b15ee45ffd48c635d061ce30` with existing dirty runtime work. This run is observational research on that checkout, not a frozen candidate or the final parity acceptance run. The capture diagnostics retain binary path, size and platform metadata in [environment.json](../../re/out/gameplay-comparison-2026-10-22/local/diagnostics/environment.json).
