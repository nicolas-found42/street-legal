# Street Legal Racing: Redline — gameplay comparison

Research date: **2026-10-22**. The currently playable Rust build reproduces a compact upgrade/repair/paint/tune/race loop, but the reference footage presents a substantially richer mechanical workshop, vehicle assembly model and city-racing world. The biggest differences are in reachable activities and scene content as well as visual finish.

## Open the evidence

- [Offline gallery: twenty videos, screenshots and timestamps](../../re/out/gameplay-comparison-2026-10-22/index.html)
- [Four visual comparisons](../../re/out/gameplay-comparison-2026-10-22/comparison.jpg)
- [Local screenshot contact sheet](../../re/out/gameplay-comparison-2026-10-22/local/contact-sheet.png)
- [Local capture findings](gameplay-comparison-local.md)
- Per-video observations: [six videos A](gameplay-video-notes-a.md), [six videos B](gameplay-video-notes-b.md), [eight videos C](gameplay-video-notes-c.md)
- [Original-executable investigation with REA](gameplay-comparison-rea.md)

All media, full uploader metadata/descriptions, timestamped frames, collection scripts, source identity and verification receipts are retained locally under `re/out/gameplay-comparison-2026-10-22/`. That directory is ignored by Git; the research notes can be versioned without committing 1.31 GB of downloaded media. Keep this bundle if the images and gallery should remain usable.

## What was compared

Downloaded **20 distinct YouTube gameplay/Let's Play videos**, **1,313,359,983 bytes** of MP4 media, representing **6:54:40.71** of source footage. Each file has a video stream and audio stream; video heights are at most 480 pixels. Download identity, codecs, dimensions and SHA-256 values are in `verification.json`. All twenty files passed complete FFmpeg video/audio decoding with exit status zero and empty error logs; `audit/check-results.json` retains those results and validates all 504 JPEGs, their timestamp links and 318 timestamp references in the per-video notes. Audio decoding checks file integrity, not its perceptual fidelity.

Reviewed **504 timestamped video frames**: 24 uniform midpoint samples per recording, with 48 for the uploader-declared v2.2.1/build-606 speedrun. These appear in **21 contact sheets**; detailed follow-up frames were also enlarged where useful. Reviewed **11 captured local screens**, spanning title, garage, tyres, mechanic, painter, tuning, cars, events, race HUD, results and test drive. The isolated native autotest reports **11 checks passed, zero failed**, with completed, untruncated diagnostics. This is evidence for that local sequence, not whole-game acceptance. The local agent's report records any additional capture attempts and limits.

This was **sampled visual analysis**, not seven hours of continuous real-time viewing. Downloaded audio was retained but not comprehensively listened to or transcribed. Screenshots do not validate handling feel, collision equations, sound, frame pacing, full progression, police behavior or save compatibility. Different cars, camera positions, times of day and race states make the montage a feature/presentation comparison rather than a matched-frame rendering test.

### The playable app and exact-port project are different

The app launched for this investigation is `slr-game`/`street-legal`, the older macroquad approximation. The repository explicitly retires it in [ADR 0006](../adr/0006-completion-plan.md); [fidelity.md](../fidelity.md) and [boot-status.md](../boot-status.md) distinguish it from the newer exact stock-script/native-runtime work. Finding a feature missing in its screenshots does **not** establish that no parser, native implementation or bounded oracle evidence exists elsewhere in the repo.

The reference closest to the target is [te :C's run](https://www.youtube.com/watch?v=aKLtzcZUPbE): its uploader explicitly declares **v2.2.1 / SL2 build 606**. This is an uploader statement, not an audited installation. Other recordings include explicitly modded builds and later/earlier versions. For example, [Zerga's Steam episode](https://www.youtube.com/watch?v=orCRa9TVieE&t=17s) visibly identifies **v2.3.1/build 925**, while its description claims no mods. Jack, Exhaustive, Laced Edition and real-car replacement footage supply supplemental experience references; their cars, tuning values, UI skins and mod behavior are not stock requirements.

## Concrete differences

| Area | Reference evidence | Current playable capture | Assessment |
| --- | --- | --- | --- |
| Workshop and assembly | Build-606 reference at [01:28](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=88s): textured workshop, inspection pit, exposed cabin/body structure, loose component inventory and toolbar. Similar states recur across editions. | `02_garage_overview.png`: intact exterior preview on checkerboard next to tabbed specification panel. | **Major interaction and scene gap.** The physical car is the workspace in the reference; local controls largely operate on aggregate values. |
| Component shopping/compatibility | Build-606 [30:08](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=1808s) shows a specific spring purchase and compatibility warning; [LA0C at 05:41](https://www.youtube.com/watch?v=LA0C_XpIeQE&t=341s) illustrates small engine components. | `03_parts_tyres.png`: text rows and Fit buttons; other upgrade choices are simplified packages. | **Major mechanical-depth gap.** Need actual item identity, component dependencies and visible installed/loose state, driven by stock scripts. |
| Tuning and diagnostics | Build-606 [02:28](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=148s) uses a component modal; [hqaX at 01:03](https://www.youtube.com/watch?v=hqaXbFUNDPg&t=63s) shows an engine curve/specification screen in an unspecified variant. | `06_tuning.png`: final drive, brake bias, wing and diff sliders; aggregate acceleration/top-speed estimates. | **Different scope and presentation.** Local has working adjustment controls but lacks the pictured component/diagnostic context. Exact stock parameter coverage needs script-based checks. |
| Cars and materials | Reference workshop views reveal cabin, seats, engine, glass, detailed wheels, individual panels and condition differences. | Local model has recognizable exterior geometry, flat surface colors, dark windows and generated spoke wheels. | **Large visible fidelity gap.** Local [render3d.rs](../../crates/slr-game/src/render3d.rs) submits untextured colored triangles and creates wheels procedurally; asset count alone does not recover material/assembly appearance. |
| Roads and world | Build-606 [27:10](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=1630s) shows a building-lined city; [32:07](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=1927s) shows a hilly ROC road. Other references show junctions, sidewalks, lights, traffic and pedestrians. | `09_race_hud.png` / `11_test_drive.png`: bounded procedural road, repeated block trees, red/white barriers and simple distant boxes. | **Largest immediate visual gap.** The local route is not a reconstruction of the stock city or ROC environment. [Track/world code](../../crates/slr-core/src/track.rs) and [renderer](../../crates/slr-game/src/render3d.rs) confirm procedural construction. |
| Lighting and atmosphere | Build-606 [08:23](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=503s): night sky, lit road, surrounding cars and people; later samples show sunlit hills and sky. | Captured local drive/race is daytime with flat background and simple directional shading. | **Large presentation gap.** Local code does have a night lighting path; lack of a night capture is not proof that night mode is absent. Texture/light/material equivalence remains untested. |
| Race setup and stakes | Build-606 [04:26](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=266s): portrait-based rival list; [08:23](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=503s): street meet and countdown; ROC has a different start/venue. | `08_events.png`: reputation-gated rows; race launches on generic track, then `10_results.png` presents outcome, money and reputation. | **Core loop exists; context is compressed.** Original rivals, location, staging, club progression and wagers need their original event flow rather than only event buttons. |
| HUD and cameras | Build-606 uses lower-left street map, lower instrument strip and car-condition diagram. Several other recordings show dashboard, steering wheel and hands in cockpit view. | Track map top-right, round gauge bottom-right, condition bars bottom-left; pictured rear chase view. | **Layout and camera-content gap.** Local source already contains camera modes; the reference cockpit's interior content and original HUD remain separate requirements. Modded gauges should not define the baseline. |
| Damage and repair | Build-606 [33:06](https://www.youtube.com/watch?v=aKLtzcZUPbE&t=1986s) shows a tilted vehicle with missing/deformed structure; other videos show stripped and damaged panels. | Local condition bars and repair transaction work. Its render path darkens/tints painted surfaces based on aggregate damage while retaining panel vertex positions. | **Visible structural-damage gap.** A lower condition bar or darker paint does not reproduce detached/deformed components. This is not a measured comparison of collision physics. |
| Menus, dealers and transitions | Reference clips show spatial showrooms/used lots, illustrated character/rival/loading art, per-part dialogs and options screens. | Simple title buttons, list-based car shopping, tabbed garage and results overlay. | **Reachability and identity gap.** Restore original script-driven screens/assets; do not mistake a generic menu with similar labels for stock behavior. |

Source-based render observations above concern the retired frontend. In particular, `MeshSink::flush` uses `texture: None`; `CarRender::emit` retains transformed part vertices while changing paint color for damage; `World::build` explicitly constructs procedural surroundings. These are direct implementation explanations for pictured differences, not conclusions reverse-engineered from screenshots alone.

## What the original executable adds

Using the requested **reverse-engineer-anything** skill, REA/Ghidra inspected the stock backup executable with SHA-256 `7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde`. The native dispatcher at **0x475ec0** contains actual branches for install, remove, repair, paint, texture and decal operations. Focused decompilations at **0x474600** and **0x472b30** show object/slot flags, references, traversal and attachment-related calls. This supports treating the workshop gap as a state/assembly problem as well as a rendering problem. [The REA note](gameplay-comparison-rea.md) records three exact Evidence IDs, decompiler limitations, partial indirect-call coverage and a failed renderer lookup. No original executable runtime was observed in this task.

## Recommended acceptance targets

These are concrete visual/interaction targets within the existing stock-port plan, **not** an instruction to add more approximations to macroquad or change the agreed milestone order. Resolve the currently documented boot/runtime blockers first.

1. **A genuine stock workshop session:** enter garage through unchanged scripts, rotate/inspect the car, remove and reinstall a body panel, wheel and engine component, preserve loose inventory/compatibility/condition, and capture before/after states. Relevant P06/P07/P24/P26.
2. **Recognizable original scenes:** one named garage, dealer, city junction, night meet, test track and ROC start; load their actual geometry/materials and compare with controlled original captures. Relevant P04/P05/P11/P12/P15/P17–P23.
3. **Original navigation and race context:** rival selection, wager/club entry, starter/countdown, race, outcome and return to garage through the real event flow. Relevant P03/P11–P16.
4. **Component diagnostics and visible effects:** stock part tuning, engine performance view, per-panel paint/damage and camera/HUD assets. Relevant P08–P10/P24/P27.
5. **Measured parity after visual coverage:** deterministic original-versus-port driving/collision states, native render/input/audio streams, and save/config byte compatibility. These require the existing oracle and human-review gates; YouTube cannot close them.

No P01–P30 status was changed. Existing runtime work and user changes were preserved. Research artifacts are the only authored project changes for this comparison.

## Video inventory

Every entry links the uploader's source; local media, metadata and frames are indexed by video ID in the gallery and manifest. Edition labels record explicit declarations or visible variant evidence and preserve unknowns.

| # | Source / title | Channel | Duration | Edition/variant evidence |
| --- | --- | --- | --- | --- |
| 1 | [Street Legal Racing: Redline 2.2.1 - Any% in 44:06](https://www.youtube.com/watch?v=aKLtzcZUPbE) | te :C | 47:26 | Uploader: v2.2.1 / build 606; closest target anchor |
| 2 | [Let's Play Street Legal Racing Redline - EP01 - Preparing the Car.](https://www.youtube.com/watch?v=Y1v2YIcNpAk) | Zerga | 21:45 | Customized Jack UI; exact build unknown |
| 3 | [Let's Play Street Legal Racing Redline - EP02 - First Race.](https://www.youtube.com/watch?v=2p77_4gKzs8) | Zerga | 13:03 | Customized Jack UI/real car; exact build unknown |
| 4 | [Let's Play Street Legal Racing: Redline! Part 1](https://www.youtube.com/watch?v=hqaXbFUNDPg) | gamextrmnatr | 17:00 | Build/mod inventory unspecified |
| 5 | [Street legal racing redline  gameplay part 1: Creating a profile and first races...](https://www.youtube.com/watch?v=MurqdfEkgsM) | RacingGamerSlovenia | 27:08 | Build/mod inventory unspecified |
| 6 | [Street Legal Racing Redline - Career Mode Ep.1](https://www.youtube.com/watch?v=jv5gTguHVVM) | Getrag | 20:45 | Uploader: 2.2.1 Exhaustive by Amilmand |
| 7 | [Street Legal Racing - Redline "Fast and Furious"](https://www.youtube.com/watch?v=313tW_mMGqw) | TheDevildogGamer | 30:19 | Custom UI/assets; exact pack unknown |
| 8 | [Street legal racing redline letsplay #1 [first car plus TURBOCHARGER??]](https://www.youtube.com/watch?v=wE_AwIAe5tQ) | Someguy | 25:26 | Build/mod inventory unspecified |
| 9 | [Let's Play Street Legal Racing Redline S2 - EP01 - Toyota Supra](https://www.youtube.com/watch?v=RaYM7cUZqgQ) | Zerga | 23:12 | Visible Jack UI; Toyota replacement car |
| 10 | [Let's Play Street Legal Racing Redline Part 1](https://www.youtube.com/watch?v=LA0C_XpIeQE) | ninjaplease41 | 9:25 | Build/mod inventory unspecified |
| 11 | [Street Legal Racing Redline Letsplay \| Part #2](https://www.youtube.com/watch?v=GsKiUBC7zX8) | Vladik :3 | 31:32 | Uploader links Laced Edition; install not audited |
| 12 | [Let's Play Street Legal Racing Redline S4 - EP1 - Steam Version!](https://www.youtube.com/watch?v=orCRa9TVieE) | Zerga | 14:23 | Visible v2.3.1 / build 925; uploader says no mods |
| 13 | [Let's Play Street Legal Racing Redline - EP00 - test](https://www.youtube.com/watch?v=CENtWCepqFA) | Zerga | 14:31 | Visible slrr by Jack V2 |
| 14 | [Let's Play Street Legal Racing Redline S6 - EP1 - S14 AS A FIRST CAR!?!](https://www.youtube.com/watch?v=CwDwrmKRLMg) | Zerga | 13:17 | Customized UI/real car; exact pack unknown |
| 15 | [WHERE MY CHANNEL BEGAN! - Street Legal Racing: Redline - V8 Twin Turbo Integra DC5](https://www.youtube.com/watch?v=2q5pHaPi2jA) | Brett Charles Bass | 20:42 | Uploader links Steam v2.3.1 and four mods |
| 16 | [Lets Play SLRR - Episode 1 - Street Legal  Racing Redline](https://www.youtube.com/watch?v=06AnTjt6P3E) | YokohamaPlease | 12:44 | Visible Redline 2.2.0; mods unknown |
| 17 | [Let's Play Street Legal Racing Redline - EP08 - Flying Miata, literally.](https://www.youtube.com/watch?v=Nc7J2WQf2Ig) | Zerga | 16:08 | Visible Jack UI; real-car replacement |
| 18 | [Street Legal Racing Redline Exhaustive Mod EP-1](https://www.youtube.com/watch?v=gkAY7Xr68RE) | Cobby Blobby | 14:37 | Explicit Exhaustive Mod / real-car replacements |
| 19 | [Street legal racing redline Live commentary Lets play Part 1](https://www.youtube.com/watch?v=AxEfAO3InCU) | TacoGamerGuy | 9:40 | Build/mod inventory unspecified |
| 20 | [Street Legal Racing Redline Letsplay \| Part #1](https://www.youtube.com/watch?v=0DzsE9zk5iA) | Vladik :3 | 31:38 | Uploader links Laced Edition; install not audited |
