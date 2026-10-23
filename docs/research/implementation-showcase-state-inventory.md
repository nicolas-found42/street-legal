# Playable implementation: finite showcase state inventory

Date: 2026-10-22. Source audit for preparing an external visual critique video.

## Scope and provenance

The recordable native application is the **legacy macroquad `slr-game` frontend**. Its top-level state machine has `Title`, `Garage`, and `Session`, with seven garage tabs ([app.rs:25](../../crates/slr-game/src/app.rs#L25), [app.rs:66](../../crates/slr-game/src/app.rs#L66)). ADR 0006 retires macroquad and specifies the newer exact stock-script/runtime effort ([ADR 0006](../adr/0006-completion-plan.md)). The current exact effort reports no complete-game/native-display acceptance ([boot status](../boot-status.md)). A video of the legacy app must identify this distinction; it cannot demonstrate the visual state of the exact runtime.

“Every state” here means every finite implemented screen, mode, outcome, warning and action family. Car/engine/tyre identities, paint choices, tuning values, money amounts and every physical position form combinatorial or continuous spaces. Show representative content plus catalog scrolling rather than asserting all combinations were captured.

This is a source inventory, **not a claim that every row has already been recorded or tested**. The final recording manifest must map IDs to timestamps and disclose omitted or simulated rows.

Legend: **UI** = reachable through ordinary implemented actions; **fixture** = isolated save/state seed to shorten setup, with subsequent real UI rendering/actions; **staged** = direct runtime mutation that must be called out beside the footage. Never describe staged wins, damage, penalties or lap times as naturally achieved gameplay.

## Recording checklist

### Launch, persistence and common UI

| ID | Distinct state/action | How to show it | Source |
|---|---|---|---|
| T01 | Title, no save; Continue disabled | Start with a fresh isolated save directory. Show content counts and build footer. | [app.rs:476](../../crates/slr-game/src/app.rs#L476) |
| T02 | New Game → worn starter garage | Click New Game; hold overview and new-game toast. Starter has $2,500, rep 0, day 1, worn tyres/body/engine/brakes. | [app.rs:291](../../crates/slr-game/src/app.rs#L291), [state.rs:210](../../crates/slr-core/src/state.rs#L210) |
| T03 | Save success | Click Garage Save and show “Game saved.” | [app.rs:229](../../crates/slr-game/src/app.rs#L229) |
| T04 | Title with Continue enabled | Garage Title saves before returning; show button enabled. | [app.rs:848](../../crates/slr-game/src/app.rs#L848) |
| T05 | Continue restores owned cars/setup/progress | Continue; show loaded toast and matching overview/car inventory. | [app.rs:302](../../crates/slr-game/src/app.rs#L302) |
| T06 | Load failure toast | Isolated fixture: preserve a deliberately invalid slot1.json and click Continue. Also optionally unsupported version and unknown-car save are separate text branches. Restore valid fixture afterward. | [app.rs:302](../../crates/slr-game/src/app.rs#L302), [state.rs:626](../../crates/slr-core/src/state.rs#L626) |
| T07 | Save failure toast | Optional fault fixture: isolated non-writable/invalid save destination; click Save. Preserve user saves and disclose deliberate fault. | [app.rs:229](../../crates/slr-game/src/app.rs#L229) |
| T08 | Keyboard focus / hover / disabled controls | Show selected button outline, hover highlight and dimmed disabled state; arrow keys/Enter and sliders Left/Right are supported. Number keys 1–7 switch garage tabs. | [ui.rs:57](../../crates/slr-game/src/ui.rs#L57), [ui.rs:154](../../crates/slr-game/src/ui.rs#L154), [app.rs:643](../../crates/slr-game/src/app.rs#L643) |
| T09 | Quit | Record only at end; closes app without another visual in-game screen. | [app.rs:497](../../crates/slr-game/src/app.rs#L497) |

An absent reference install produces a distinct title message and two-car fallback data/box visual, but this is a degraded environment, not another ordinary game mode. Keep the main showcase on loaded reference data. If shown, label it as a fallback appendix ([app.rs:99](../../crates/slr-game/src/app.rs#L99), [app.rs:503](../../crates/slr-game/src/app.rs#L503)). Failed clean-stock verification exits before a game screen ([main.rs](../../crates/slr-game/src/main.rs)).

### Garage, Parts and Tuning

| ID | Distinct state/action | How to show it | Source |
|---|---|---|---|
| G01 | Garage overview | Hold long enough to read specifications, simulated performance, condition bars, car name and odometer; let the car rotate. | [app.rs:562](../../crates/slr-game/src/app.rs#L562), [app.rs:751](../../crates/slr-game/src/app.rs#L751) |
| P01 | Engines: current installed vs unaffordable alternatives | Parts → Engines on original budget. Scroll catalog top/middle/bottom if over nine engines. Installed has its own highlight and disabled label. | [app.rs:930](../../crates/slr-game/src/app.rs#L930) |
| P02 | Engine swap affordable → installed | Seed money only if necessary, clearly labeled. Click actual Install; show payment/toast/current label and overview power change. Engine health restores and tune stage persists. | [app.rs:992](../../crates/slr-game/src/app.rs#L992), [state.rs:346](../../crates/slr-core/src/state.rs#L346) |
| P03 | Tyres: Both | Show prices/fit and installed row highlighting, scroll top/middle/bottom if over eight tyres; Fit resets wear. | [app.rs:1013](../../crates/slr-game/src/app.rs#L1013) |
| P04 | Tyres: Front only | Click Front then Fit a different tyre; show front/rear mismatch in overview and modified wheels. | [app.rs:1035](../../crates/slr-game/src/app.rs#L1035), [state.rs:369](../../crates/slr-core/src/state.rs#L369) |
| P05 | Tyres: Rear only | Repeat Rear; two-tyre versus four-tyre quote is distinct. | [app.rs:1013](../../crates/slr-game/src/app.rs#L1013) |
| P06 | Tyres insufficient funds | Fixture money 0; Fit dimmed. Fitting the current model remains possible when affordable because it replaces wear. | [app.rs:1094](../../crates/slr-game/src/app.rs#L1094) |
| P07 | Upgrades stock/no LSD | Show installed Stock, Stage 1–3 prices and LSD purchase. | [app.rs:1121](../../crates/slr-game/src/app.rs#L1121) |
| P08 | Stages 1, 2, 3 and downgrade | Use actual buttons; hold each installed state and resulting quote. Stage choices change boost; lower stages display “Downgrade (free).” Affordability guard uses computed UI cost. | [app.rs:1140](../../crates/slr-game/src/app.rs#L1140), [state.rs:394](../../crates/slr-core/src/state.rs#L394) |
| P09 | LSD installed | Actual $650 purchase, toast and Installed label; return to Tuning to show enabled lock slider. | [app.rs:1192](../../crates/slr-game/src/app.rs#L1192) |
| P10 | Unaffordable stage/LSD | Fixture money 0 and no LSD; show disabled purchases. | [app.rs:1168](../../crates/slr-game/src/app.rs#L1168) |
| U01 | Tuning without LSD | Diff lock disabled and install instruction; three enabled sliders; automatic gearbox state. | [app.rs:1217](../../crates/slr-game/src/app.rs#L1217) |
| U02 | Tuning edits | Move final drive, front brake bias and rear wing; show values/performance result. Ranges 0.7–1.4x, 30–80%, 0–100%; do not need every value. | [app.rs:1232](../../crates/slr-game/src/app.rs#L1232) |
| U03 | Tuning with LSD | Move enabled lock slider 0–100%. | [app.rs:1260](../../crates/slr-game/src/app.rs#L1260) |
| U04 | Manual / automatic gearbox | Toggle; capture both labels and later driving HUD in each mode. | [app.rs:1284](../../crates/slr-game/src/app.rs#L1284) |
| U05 | Reset tuning | Click after nondefault settings; show values return 1.0x/60%/30%, LSD 40% if installed. Gearbox toggle is not reset by this action. | [app.rs:1308](../../crates/slr-game/src/app.rs#L1308) |

### Mechanic, Painter and Cars

| ID | Distinct state/action | How to show it | Source |
|---|---|---|---|
| M01 | Worn/damaged service | Show engine, four body sides, brakes, four tyres and separate quotes on starter or labeled damage fixture. | [app.rs:1343](../../crates/slr-game/src/app.rs#L1343) |
| M02 | Repair engine/body/brakes | Click real repair; toast/zero quote/disabled repair and restored bars. | [app.rs:1398](../../crates/slr-game/src/app.rs#L1398), [state.rs:469](../../crates/slr-core/src/state.rs#L469) |
| M03 | Replace worn tyres | Click real replacement; show zero quote/disabled button; independent of other repairs. | [app.rs:1406](../../crates/slr-game/src/app.rs#L1406) |
| M04 | Service unaffordable warning | Fixture worn car, money 0; both disabled as applicable and full-repair money warning. | [app.rs:1414](../../crates/slr-game/src/app.rs#L1414) |
| A01 | Painter palette / current colour | Show all 12 swatches; current outlined green; click current colour → “Already that colour.” | [app.rs:1443](../../crates/slr-game/src/app.rs#L1443), [state.rs:18](../../crates/slr-core/src/state.rs#L18) |
| A02 | Successful respray | Click alternate swatch; visible car repaint, $150 reduction and toast. Multiple contrasting colours demonstrate rendering; every RGB value is unnecessary. | [app.rs:1481](../../crates/slr-game/src/app.rs#L1481) |
| A03 | Insufficient respray money + rejected click | Fixture money 0; warning; alternate swatch still clickable and produces actual “Not enough money” toast. Current swatch still returns Already that colour. | [app.rs:1477](../../crates/slr-game/src/app.rs#L1477), [state.rs:489](../../crates/slr-core/src/state.rs#L489) |
| C01 | Showroom unaffordable | Original budget; prices and dim buttons; scroll if more than nine cars. | [app.rs:1545](../../crates/slr-game/src/app.rs#L1545) |
| C02 | Showroom purchase | Labeled money fixture; actual Buy → new owned/active car and toast; condition mint. | [app.rs:1585](../../crates/slr-game/src/app.rs#L1585) |
| C03 | Used lot unaffordable / affordable | Show all six listings, condition bars, odometers and quotes. | [app.rs:1597](../../crates/slr-game/src/app.rs#L1597), [state.rs:275](../../crates/slr-core/src/state.rs#L275) |
| C04 | Used purchase → listing removed | Click Buy; selected car becomes active with listing condition. Capture list after removal and My cars. | [app.rs:1651](../../crates/slr-game/src/app.rs#L1651) |
| C05 | My cars, sole vehicle | Active button and Sell disabled. | [app.rs:1664](../../crates/slr-game/src/app.rs#L1664) |
| C06 | My cars, multiple vehicles | Active versus “Use this car”; click another; show preview/header change. Only first eight owned cars display; do not imply unlimited visible paging. | [app.rs:1682](../../crates/slr-game/src/app.rs#L1682) |
| C07 | Sale | Actual Sell while multiple owned; show balance/toast, inventory reduced and appropriate active state. No confirmation dialog. | [app.rs:1713](../../crates/slr-game/src/app.rs#L1713), [state.rs:321](../../crates/slr-core/src/state.rs#L321) |
| C08 | Next-day used lot | After race settlement or forfeit day increases; lot refreshes deterministically for seed/day. If day is directly staged, label that. | [app.rs:1535](../../crates/slr-game/src/app.rs#L1535), [state.rs:275](../../crates/slr-core/src/state.rs#L275) |

### Event selection and six event variants

Show the fresh rep-0 list, an unlocked fixture with money, and an unlocked no-money fixture. These distinguish rep locks from an unaffordable Race button. Show badly worn warning once (repair_cost > $600), and a recorded best time in an event row ([app.rs:1731](../../crates/slr-game/src/app.rs#L1731)). Entry fee is charged on race start. **E01–E06 require actual distinct starts**, because event name/opponent/lap count differ even when map geometry is shared.

| ID | Event ID | Display name | Geometry / light | Rep / fee | Source |
|---|---|---|---|---|---|
| E01 | drag_rookie | Street Drag: Rookie | 402 m drag / day | 0 / $0 | [state.rs:121](../../crates/slr-core/src/state.rs#L121) |
| E02 | circuit_club | Club Circuit Sprint (2 laps) | circuit 2 laps / day | 1 / $100 | [state.rs:133](../../crates/slr-core/src/state.rs#L133) |
| E03 | drag_night | Midnight Drag | 402 m drag / night | 3 / $250 | [state.rs:145](../../crates/slr-core/src/state.rs#L145) |
| E04 | circuit_night | Night Run (3 laps) | circuit 3 laps / night | 5 / $400 | [state.rs:157](../../crates/slr-core/src/state.rs#L157) |
| E05 | drag_pro | Pro Drag Challenge | 402 m drag / day | 8 / $800 | [state.rs:169](../../crates/slr-core/src/state.rs#L169) |
| E06 | circuit_champ | Championship Final (5 laps) | circuit 5 laps / day | 12 / $1,500 | [state.rs:181](../../crates/slr-core/src/state.rs#L181) |

Also capture selection variants **E07** rep locked, **E08** unlocked/affordable, **E09** unlocked/no money, **E10** wear warning, **E11** best-time row. A seeded reputation/money fixture is preferable to pretending progression happened off camera.

### Driving, HUD and overlays

| ID | Distinct state/action | How to show it | Source |
|---|---|---|---|
| D01 | Free test drive | Garage Test drive; lone player, day circuit, no position/prize; initial controls helper for first 8 seconds. Test circuit uses 999 laps internally. | [app.rs:337](../../crates/slr-game/src/app.rs#L337), [session.rs:401](../../crates/slr-game/src/session.rs#L401) |
| D02 | Four cameras | C cycles chase / close chase / hood / helicopter; show each with moving scenery and wheel motion. Modes are global to sessions; demonstrate all four in daylight and useful night views. | [session.rs:154](../../crates/slr-game/src/session.rs#L154), [render3d.rs:535](../../crates/slr-game/src/render3d.rs#L535) |
| D03 | Automatic launch, throttle/brake/steering/handbrake | Actual manual controls preferred; if autopilot substitutes, on-screen label. Observe speed/rpm/gears/acceleration/minimap. Show reverse and neutral if actual shifting produces them. | [session.rs:80](../../crates/slr-game/src/session.rs#L80), [session.rs:290](../../crates/slr-game/src/session.rs#L290) |
| D04 | Manual shifting | Toggle manual before start; Q/E for down/up shift; show MANUAL and changing gear. Reverse/neutral/limiter colours are HUD branches, not separate camera modes. | [session.rs:121](../../crates/slr-game/src/session.rs#L121), [session.rs:345](../../crates/slr-game/src/session.rs#L345) |
| D05 | Free-drive reset | R while Racing: “Car reset”; no time penalty. If harness calls respawn_player directly, label injected action. | [session.rs:139](../../crates/slr-game/src/session.rs#L139) |
| D06 | Free-drive pause / resume / leave | Esc: PAUSED, Resume and Back to garage. Resume then pause/leave to demonstrate return and wear persistence. | [app.rs:1820](../../crates/slr-game/src/app.rs#L1820), [app.rs:403](../../crates/slr-game/src/app.rs#L403) |
| D07 | Race countdown / GO | Capture full 3→2→1 and green GO (first 1.2 s). Cars held on grid; helper banner and opponent visible. | [race.rs:9](../../crates/slr-core/src/race.rs#L9), [session.rs:473](../../crates/slr-game/src/session.rs#L473) |
| D08 | Active drag HUD | Distance progress, P1/P2 position, opponent gap, straight minimap. | [session.rs:401](../../crates/slr-game/src/session.rs#L401) |
| D09 | Active circuit HUD | Lap x/n, circuit minimap, opponent; cross a complete lap for Best value. Seeded lap/progress is staged, not a completed driving lap. | [session.rs:418](../../crates/slr-game/src/session.rs#L418), [race.rs:200](../../crates/slr-core/src/race.rs#L200) |
| D10 | Night driving | Show dark sky, light poles, headlight ground glow, brake lights and opponent. Both drag and circuit night worlds exist. | [session.rs:224](../../crates/slr-game/src/session.rs#L224), [render3d.rs:316](../../crates/slr-game/src/render3d.rs#L316) |
| D11 | Race reset penalty | R during Racing → “Car reset (+5 s penalty).” | [session.rs:139](../../crates/slr-game/src/session.rs#L139) |
| D12 | Race pause/resume | Esc: Forfeit and leave replaces free-drive Back to garage. | [app.rs:1820](../../crates/slr-game/src/app.rs#L1820) |
| D13 | Forfeit | Real leave action; no results dialog; entry fee stays spent, damage/wear persists, day/races increment; returned garage autosaves. | [app.rs:403](../../crates/slr-game/src/app.rs#L403) |
| D14 | False start +1.0 s | Drag only; vehicle speed >2 m/s while Countdown. Ordinary held-grid controls may not reliably reach it. If velocity/false_start is directly staged, make the staging explicit. | [race.rs:168](../../crates/slr-core/src/race.rs#L168), [session.rs:491](../../crates/slr-game/src/session.rs#L491) |
| D15 | Collision/damage feedback | Actual barrier/opponent contact preferred; capture red impact flash, condition changes, changed body colour and post-drive mechanic. Direct seeded damage/flash must be labeled. Rendered damage changes paint shading, not mesh shape. | [session.rs:292](../../crates/slr-game/src/session.rs#L292), [render3d.rs:225](../../crates/slr-game/src/render3d.rs#L225), [condition.rs:57](../../crates/slr-core/src/condition.rs#L57) |
| D16 | Finish waiting | Player crosses before opponent; FINISH / waiting for the others. Session can simulate 30 extra physics steps each update while waiting, so natural screen may be brief. Direct phase/finish-time fixture must be labeled. | [session.rs:189](../../crates/slr-game/src/session.rs#L189), [session.rs:510](../../crates/slr-game/src/session.rs#L510) |
| D17 | Victory results | P1: Time, fee, prize, rep, repairs; actual settlement changes saved progression. A staged race boundary can still exercise real settlement but is not proof the car won naturally. | [app.rs:1838](../../crates/slr-game/src/app.rs#L1838), [state.rs:567](../../crates/slr-core/src/state.rs#L567) |
| D18 | Defeat results | P2 finishing pays 15% prize, +0 rep; same result table with DEFEAT. | [app.rs:1858](../../crates/slr-game/src/app.rs#L1858), [state.rs:595](../../crates/slr-core/src/state.rs#L595) |
| D19 | Improved personal-best banner | Must beat an **existing** record: first-ever saved best does not set new_record. Fixture previous slower record may shorten setup; resulting time must be labeled if staged. | [state.rs:575](../../crates/slr-core/src/state.rs#L575), [app.rs:1885](../../crates/slr-game/src/app.rs#L1885) |
| D20 | Return after results | Actual Back to garage; confirm changed money/day/rep, condition, saved time and enabled next event as applicable. | [app.rs:1888](../../crates/slr-game/src/app.rs#L1888) |

## Things that must not be invented as unseen states

- There are no confirmation modals for New Game, purchases, repair, paint, sale or forfeit in the frontend. Actions apply immediately; source-defined pause and result panels are the only modal-like overlays ([app.rs](../../crates/slr-game/src/app.rs)).
- Money/rep blockers generally disable the relevant action. The core error strings do not imply every error is reachable via the UI; painter's alternate swatch with no money is a concrete exposed rejected action ([app.rs:1477](../../crates/slr-game/src/app.rs#L1477)). Invalid IDs and last-car sale errors are guarded out of ordinary UI.
- Race ends when the **player** finishes. Opponent finishing alone does not naturally produce a timeout/DNF result; a Time “-” result would need a forced fixture ([race.rs:220](../../crates/slr-core/src/race.rs#L220)). No explicit crash/game-over or race restart dialog exists.
- Day/night are selected by event definitions, not a free-drive time switch. Free-drive is daylight circuit only ([app.rs:337](../../crates/slr-game/src/app.rs#L337)). The six events use two track types; they are not six independent city maps ([state.rs:523](../../crates/slr-core/src/state.rs#L523)).
- This frontend does not include audio calls/modules in its main module graph; a silent visual recording cannot validate engine, tyre, collision, ambient or UI sound ([main.rs](../../crates/slr-game/src/main.rs), [session.rs](../../crates/slr-game/src/session.rs)).
- No stock main menu, difficulty/options, player creation, component assembly, city free roam, police, club/pink-slip flow or original-script menu can be demonstrated by merely renaming legacy screens. Their coverage status should be stated as absent from this recorded frontend, with no claims about unseen exact-port internals ([ADR 0006](../adr/0006-completion-plan.md)).

## Harness implementation notes (isolated copy only)

A temporary child module of `app` can access the parent's private `parts_view`, `cars_view`, `axle`, `scroll`, `used_lot`, `used_day`, `perf_cache`, angle and state transition helpers. That placement permits actual `app.frame()`/real renderer execution without production edits. The existing `Ui.inject` applies virtual 1280×720 coordinates and actual button/slider handlers on the next begin; it is one-frame injection, not browser automation ([ui.rs:17](../../crates/slr-game/src/ui.rs#L17), [ui.rs:57](../../crates/slr-game/src/ui.rs#L57)). `Autotest::post_frame` demonstrates framebuffer capture after drawing, before presentation ([autotest.rs](../../crates/slr-game/src/autotest.rs)).

`RaceSession` fields race/event/cam/light/night/paused/outcome/settled/autopilot/elapsed/impact_flash/toast are public. Its accum, steer_in, thr_in and brk_in are private to the sibling `session` module and inaccessible from an app child. Use its public update/respawn methods or operate on public `race`/vehicles; do not assert keys were physically pressed when direct calls were used ([session.rs:13](../../crates/slr-game/src/session.rs#L13)). Four camera modes are `cam.mode` 0–3 ([render3d.rs:535](../../crates/slr-game/src/render3d.rs#L535)).

State setup is straightforward because GameState, OwnedCar, Setup, Condition and Race/Racer carry public state. Always isolate save paths, preserve the user's active game/process, reset performance cache after fixture replacement, and regenerate used lot when changing day/seed. A seeded rich/unlocked fixture should change money/reputation only, unless explicitly demonstrating a different car or damage fixture. For staged results, use internally consistent race times, progress, lap times, finish order and condition, then let the **real** settlement path produce outcome rather than painting a fictional result screen.

Suggested timeline: original-budget title/new game → every garage view and blocked action → labeled funded fixture and successful actions → real save/title/continue → free drive with four cameras and pause/reset → each of six event starts → full representative day/night driving → distinct penalty/damage overlays → finish/win/loss/record → forfeit/continuation → isolated fault-state appendix. Hold readable screens at least 3–5 seconds; maintain a timestamped manifest that distinguishes UI action, fixture setup and staged boundary. A video does not establish interaction latency or exact physics merely because these fields render correctly.

Jev verification tooling was not available to this audit subtask (`ALL_TOOLS` contained no `jev_verify`); source links were checked directly. No browser or production UI calls were made for this inventory.
