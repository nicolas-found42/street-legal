# Gameplay comparison: six reference videos, batch B

Research date: 2026-10-22. Local evidence root: `re/out/gameplay-comparison-2026-10-22/`.

## Method and limits

Inspected all 24 timestamped screenshots in each of six contact sheets, 144 reference samples in total, plus ten of those source frames separately for readable details. Also inspected the local eleven-screenshot contact sheet. This is sampled visual analysis of downloaded videos, not a claim to have watched six videos continuously. Static frames establish visible UI, parts, scenery and scene states; they do not establish handling fidelity, audio quality, smoothness, latency, or the causal sequence of an interaction. A feature absent from these samples is not necessarily absent from the game. Timestamps below use the extracted frame filename's time in seconds, converted to minutes and seconds, with fractional seconds retained where useful.

Source attribution: each video link identifies the uploader's primary gameplay capture; each local frame path identifies the pixels inspected. Video titles and descriptions are uploader statements, distinguished below from observations. Edition and mod differences must be preserved: the local title's data label and `README.md` identify the source data as build 606, v2.2.1, while some references show later or modified editions. This data label does not establish that the pictured local frontend reproduces the stock game's behavior.

## 1. Someguy — first car and turbocharger

[Street legal racing redline letsplay #1 [first car plus TURBOCHARGER??]](https://www.youtube.com/watch?v=wE_AwIAe5tQ), Someguy, 25:26.

Evidence: `frames/wE_AwIAe5tQ/contact-1.jpg`, metadata and description in `videos/wE_AwIAe5tQ.*`.

Uploader claims: first episode; acquiring a turbocharger. Description does not specify an edition or establish an unmodified installation. The modern white/red interface and green garage resemble another Steam-era capture in this batch, but that resemblance does not prove version identity or absence of mods.

Observed samples:

- **00:31.79**: advanced game options expose several driving/movement sliders, traffic density and pedestrian density, plus a race nitro toggle. This establishes configurable systems in this reference UI, not their measured effect.
- **01:35.38**: garage interior contains a recessed service pit, jacks, ramps, safety markings, benches and wall decorations. An icon toolbar spans the top; a wide inventory area spans the bottom.
- **02:38.96–04:46.14**: dealer purchase dialog and outdoor lot present individual vehicles spatially, including a visibly damaged hatchback; price and vehicle model are displayed.
- **05:49.72**: blue hatchback sits in the garage. **06:53.31** shows the body hidden/removed and the chassis, wheels and engine visible in their physical positions. Separate frame `07-00413.31.jpg` confirms the structure clearly.
- **07:56.89, 09:00.48, 10:04.06**: full-screen parts catalogs depict tyres, intake systems and engine performance systems as illustrated products, with prices and categories.
- **11:07.65**: body has open/missing panels; camera is close enough to inspect the cabin and engine area.
- **12:11.23**: Valo City loading card explicitly tells the player to win night races to advance club rating and access a later championship. This is in-game text, not proof that the sampled session completed that progression.
- **13:14.82–16:25.58**: street driving views show lane markings, sidewalks, buildings, a construction site, street lamps, trees, pedestrians, and other cars. Lower-left minimap depicts a street network; lower-right analog instruments dominate the HUD.
- **17:29.16**: event page shows a named race, track outline, conditions and requirements.
- **18:32.75–20:39.92**: night scenes show lit street lamps, other vehicles and a participation/watch-race affordance. A later side angle shows extensive body damage/deformation.
- **21:43.50**: performance catalog includes nitrous-related components. **22:47.09–23:50.68** show exposed engine/chassis again. **24:54.26**, separately enlarged, shows a “TUNE PART” dialog with a nitrous pressure slider and units.

Comparison priority: make the garage a place where the vehicle's assemblies are visible and inspectable; make tuning specific to the selected component. Catalog richness and a city environment are larger gaps than the mere presence of an event list or tuning sliders, which the local frontend already has.

## 2. Zerga — S2 EP01, Toyota Supra

[Let's Play Street Legal Racing Redline S2 - EP01 - Toyota Supra](https://www.youtube.com/watch?v=RaYM7cUZqgQ), Zerga, 23:12.

Evidence: `frames/RaYM7cUZqgQ/contact-1.jpg`, metadata and description in `videos/RaYM7cUZqgQ.*`.

Uploader claims: title identifies a Toyota Supra; description says the car is not doing well. This is not a stock-vehicle reference. Pixels independently show blue UI labeled **“slrr by Jack”** in catalog screens. Treat this capture as modified, with exact installed patch/mod inventory unknown.

Observed samples:

- **00:29.00**, separately enlarged: textured garage pit and blue icon toolbar; top status strip includes prestige, day, time and money.
- **01:27.01**: illustrated used-car-dealer loading art gives in-world opening hours and describes buying/selling cars and parts.
- **02:25.01**: financial information panel shows a ledger-like table.
- **03:23.01–06:17.03**: dealer lot and inspected cars include mismatched body colors. **05:19.02**, separately enlarged, shows engine information with an RPM-based performance graph and numeric specifications. It establishes a diagnostic display; curve correctness was not measured.
- **07:15.03**: mismatched white/blue coupe in the workshop; **08:13.03–09:11.04** show category menus for suspension, shock absorbers, springs, brake systems, rims and tyres.
- **11:07.05**: wheel-related items are laid out in an inventory strip under a close rear view.
- **12:05.05** and **14:01.06**: “TUNE PART” overlays appear above the vehicle. **13:03.05** shows a different wheel configuration with loose tyre/rim icons underneath. Samples alone do not identify the full installation sequence.
- **15:57.06**: night street scene with rear chase camera, street junction and traffic light. Instruments use a compact lower horizontal arrangement, materially different from the large white dials in other captures.
- **16:55.07–17:53.07**: illustrated tyre and wheel catalogs show multiple products.
- **19:49.08**: body is hidden/removed, showing wheels and chassis in the workshop.
- **20:47.08–22:43.09**: nighttime street intersections and road-driving views include trees, lighting and multiple cars gathered together.

Comparison priority: add diagnostic views for engine power/torque and component-specific tuning, while avoiding copying this mod's custom HUD as the canonical stock target. The repeated visible vehicle assemblies support a more general reconstruction requirement across editions.

## 3. ninjaplease41 — Part 1

[Let's Play Street Legal Racing Redline Part 1](https://www.youtube.com/watch?v=LA0C_XpIeQE), ninjaplease41, 09:25.

Evidence: `frames/LA0C_XpIeQE/contact-1.jpg`, metadata and description in `videos/LA0C_XpIeQE.*`.

Uploader claims: first let's play; inspecting and upgrading a new race car. No explicit edition or mod inventory is supplied. Orange/green UI styling alone is insufficient to certify a stock install or exact patch.

Observed samples:

- **00:11.76**: workshop pit, grey industrial textures, green walls, tool cabinets, jacks and cones. Toolbar sits above the 3D scene, six inventory cells below it.
- **00:35.29**: used-car lot shows a white compact car from close front view.
- **00:58.82**, separately enlarged: orange “CAR INFORMATION” screen combines numeric specifications with front/rear/side vehicle diagrams and geometry measurements.
- **01:22.35**: intact compact hatchback in the garage; **01:45.88** exposes its front engine bay, and **02:09.41** exposes the cabin with body panels in inventory cells.
- **02:32.94**: “SELL PART” confirmation specifically names the passenger seat and its value. **02:56.47**: repair dialog reports that the car is already repaired as far as possible. These samples establish individual part trading and repair feedback.
- **03:20.00**: body shell and exposed engine bay visible; **03:43.53–04:07.06** show suspension/chassis only, with wheels/brake components represented as individual inventory items.
- **04:30.59–04:54.12**: green illustrated engine catalog; **05:17.65** shows an engine block icon in inventory.
- **05:41.18–07:38.83**: catalog pages include crankshafts, rods, manifolds, heads and other components; the category index explicitly separates main components, air/fuel delivery, exhaust, transmission/drivetrain, accessory systems, pro tuning and engine kits.
- **08:02.36**: engine block appears mounted in the workshop scene. **08:25.89–09:12.94**: additional detailed engine components in the catalog.

No street-driving frame occurs in this 24-sample set; this does not prove the whole video contains no driving. The samples are particularly useful for comparing the assembly experience rather than road rendering.

Comparison priority: moving from engine-package purchases to an assembly graph of small, inspectable parts would recover a central visible activity. The local repair screen has aggregate condition bars; the reference workshop depicts seats, body panels, brakes and engine internals as separate objects.

## 4. Vladik :3 — Part #2

[Street Legal Racing Redline Letsplay | Part #2](https://www.youtube.com/watch?v=GsKiUBC7zX8), Vladik :3, 31:32.

Evidence: `frames/GsKiUBC7zX8/contact-1.jpg`, metadata and description in `videos/GsKiUBC7zX8.*`.

Uploader claims: playing a “900hp duhen sunstrip” and links their own **Street Legal Laced Edition** mod. Do not treat 900 hp as a measurement established by these screenshots or treat the capture as a clean stock reference. The installed edition is not independently enumerated.

Observed samples:

- **00:39.42**: yellow modified/damaged compact car at a nighttime meeting with many parked cars, minimap and participate/watch-race options.
- **01:58.26**: first-person cockpit with rendered steering wheel, dashboard and driver's hands. **03:17.11** returns to rear chase view on a lit road; this establishes multiple presented camera modes, not the exact switching control.
- **04:35.95–05:54.79**: side views show missing/open panels and mismatched exterior surfaces at the race gathering.
- **07:13.64**, separately enlarged: rival-selection panel presents portraits, names, wager amounts and opponent vehicles. This makes race participation social and economic within the visible interface.
- **08:32.48–09:51.32**: nighttime street-road views show changing instrument values and lights; frames do not establish tyre grip or handling quality.
- **11:10.17**: illustrated engine catalog. **12:29.01–13:47.85**: garage and inventory with spray-can icons, followed by a more uniformly yellow visible body. The exact operation between frames is unobserved.
- **15:06.70–17:44.38**: street racing/driving environment includes intersections, street lighting and surrounding buildings.
- **19:03.22**: loading/matchup card shows driver portraits and a **$2000** bet.
- **20:22.07–21:40.91**: damaged hatchback in nighttime street scenes. **22:59.75**: red body-parts catalog with separate panels and bumpers.
- **24:18.60**: workshop front view shows damaged body and more spray cans.
- **25:37.44**: daytime first-person view; **26:56.28** daytime chase view with sidewalk, trees, cars and traffic lights.
- **28:15.13**: red catalog with racing body components. **29:33.97–30:52.81**: nighttime car meeting and road views.

Comparison priority: rival selection and visible wagers provide stronger race context than a standalone event launch button. Cockpit view is a useful optional feature, but this modded sample should not dictate stock power levels or particular racing-body assets.

## 5. Zerga — S4 EP1, Steam Version

[Let's Play Street Legal Racing Redline S4 - EP1 - Steam Version!](https://www.youtube.com/watch?v=orCRa9TVieE), Zerga, 14:23.

Evidence: `frames/orCRa9TVieE/contact-1.jpg`, metadata and description in `videos/orCRa9TVieE.*`.

Uploader claims: Steam version, **“no mods.”** Pixels independently identify **v2.3.1 (build 925)** in the lower-left of the menu at **00:17.97**, inspected separately. This is the clearest edition anchor in this six-video batch; the no-mods assertion remains an uploader claim, not an installation audit. Local data is v2.2.1, so UI and content changes between versions may be intentional differences.

Observed samples:

- **00:17.97–00:53.92**: silver/white main menu and new-player-name dialog.
- **01:29.87–02:41.77**: outdoor used-car dealer; dialogs show individual asking prices and prose vehicle specifications. The Enivagen 110 GTK dialog lists price **$15,948**.
- **03:17.71**: purchased green hatchback in textured green workshop; top strip tracks prestige, day, clock and money.
- **03:53.66**: body hidden/removed, exposing wheels and chassis; engine icon in bottom inventory. **04:29.61–06:53.40**: body/glass/interior components appear as individually selectable parts, with names and percentages in the hover/status text and thumbnails in inventory. Doors, windows, steering wheel and seats appear across these samples.
- **07:29.35–08:05.30**: city street view of a visibly stripped vehicle; multi-lane junction, bus, sidewalks, trees, buildings, large analogue gauges and a street-grid minimap.
- **08:41.25**, separately enlarged: welcome dialog directs the player to find club opponents, race for money/prestige, learn suitable night-race opponents and deal with police. These are explicit in-game instructions, not proof that the sampled clip demonstrates police interaction.
- **09:17.20–09:53.14**: daytime city road with overhead traffic lights, pedestrians and buildings.
- **11:05.04–12:52.88**: junctions, underpass/elevated structure, waterfront-like open spaces, surrounding cars and roadside buildings.
- **13:28.83**: side view exposes the stripped passenger compartment; **14:04.78** returns to chase view with the large speed/RPM gauges and minimap.

Comparison priority: use this clip for later-edition, reportedly unmodified confirmation of the same core garage disassembly and city exploration seen in older/custom captures. Prioritize those cross-edition behaviors ahead of reproducing v2.3.1's white menu design.

## 6. Zerga — EP00 test

[Let's Play Street Legal Racing Redline - EP00 - test](https://www.youtube.com/watch?v=CENtWCepqFA), Zerga, 14:31.

Evidence: `frames/CENtWCepqFA/contact-1.jpg`, metadata and description in `videos/CENtWCepqFA.*`.

Uploader claims: a test episode seeking format suggestions. The main menu at **00:18.15**, separately enlarged, visibly says **“slrr by Jack V2.”** Treat as modified. Exact additional vehicle packs and engine parts are not established by the provided metadata.

Observed samples:

- **00:18.15**: illustrated blue custom menu with career, quickrace, free ride and select-track entries.
- **00:54.46**: textured garage pit and blue icon toolbar. **01:30.76**: illustrated showroom loading screen. **02:07.07**: indoor dealership with car, price display and kiosk-like props.
- **03:19.67**: chassis only in workshop, many component thumbnails along the bottom. **03:55.98–05:44.89**: white four-door car with progressively different visible body/panel states; detailed wheels, exhaust and cooling pipes are visible.
- **06:21.20**, separately enlarged: exposed front assembly includes radiator/intercooler-like heat exchanger, prominent pipes and turbo-like hardware, with colored component thumbnails below. These visual shapes do not establish engine displacement, boost or power.
- **06:57.50**: “TUNE PART” dialog over the workshop.
- **07:33.80–08:10.11**: dark/black body and exposed engine structure in close, rotatable-looking workshop viewpoints. Static frames establish viewpoint variation, not the input mechanism.
- **09:22.72**: nighttime rear chase view, minimap, building-lined streets, lighting and a compact bottom HUD.
- **09:59.02–10:35.33**: opponent portraits and race-loading/matchup cards with wager text.
- **11:11.63**: exposed chassis, engine, wheels and front plumbing. **11:47.94**: catalog of colored nitrous bottles.
- **12:24.24**: partially assembled car in workshop. **13:00.54**: night street view with smoke/mist around the car; screenshots cannot determine the audible engine behavior or cause of the smoke.
- **13:36.85–14:13.15**: another opponent/wager matchup and stylized loading art.

Comparison priority: a clear installed-part visualization and assembly inventory are stronger shared targets than this mod's custom menu, branding or car assets. Race matchup presentation adds identity and stakes absent from the local event list screenshot.

## Cross-video comparison with the local frontend

Local evidence: `local/contact-sheet.png`, representing `01_title.png` through `11_test_drive.png` under `local/shots/`. Conclusions below concern the pictured frontend states, not an exhaustive audit of source code or hidden features.

Repository scope: the pictured `slr-game` macroquad frontend is retired under [ADR 0006](../adr/0006-completion-plan.md); the stock-script exact port, including `slr-boot`, is a distinct implementation path. Priorities below describe visible target gaps, not a recommendation to extend the retired approximate frontend. Implementation should follow the repository's current stock-port milestone and acceptance order, with stock single-player parity and screenshot comparison as specified by that ADR.

| Area | Reference pixels across this batch | Local pictured state | Practical priority |
| --- | --- | --- | --- |
| Vehicle workshop | Individual removable seats, doors, glass, wheels, suspension and engine components; exposed chassis and close views | Low-detail complete coupe preview alongside tabs and lists | Build a selectable component model and assembly/disassembly view; reveal mechanical structure and dependencies |
| Parts shopping | Illustrated products and subcategories including engine internals, tyres, body panels and accessories | Text rows for engine/tyre/upgrade choices and Fit buttons | Add visible part identity and compatibility, while preserving concise cost/fit information |
| Information/tuning | Engine performance graph, vehicle measurements, per-part sliders and condition percentages | Aggregate specification, estimated performance, condition bars; final-drive/brake-bias/wing/differential sliders | Add component-scoped settings and diagnostics that explain their effect |
| World | City junctions, sidewalks, buildings, traffic lights, parked/moving vehicles, pedestrians; day and night | Sparse circuit, red-white barriers, repeated block trees, simple road backdrop | Build a recognizable street environment with intersections and urban landmarks before increasing scenery count |
| Race context | Named rivals, portraits, wagers, night meets and street-grid navigation | Event rows gated by reputation; race HUD and results overlay | Connect events to opponents, stakes and places; preserve the already-clear entry/prize/reputation display |
| Car appearance | Model-specific body silhouettes, visible interiors, detailed wheels, glass and damage states | Same simple coupe-like preview pictured with different colors | Improve vehicle silhouette and subassemblies; make damage and installed parts legible |
| HUD | Map of city streets, instrument styling varies substantially by edition/mod | Simple route/track map, combined gauge, aggregate condition bars | Target the information required by the game loop; choose a canonical edition before copying visual styling |

The highest-confidence shared reference identity is **buy a used car → inspect/strip/build its actual assemblies → tune and diagnose components → drive through a city and find races with stakes**. The local screenshots already demonstrate a compact buy/upgrade/repair/paint/tune/event/race/results loop, but visually compress much of the workshop into lists and much of the world into a circuit. Handling, collision physics, traffic behavior, audio and performance need separate interactive or motion-based checks; these screenshots cannot establish their fidelity.
