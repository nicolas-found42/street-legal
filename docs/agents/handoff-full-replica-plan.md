# Handoff: grill the user into a full plan for the exact Rust replica

You are a fresh agent. Your job is to **run a grilling session with the user** (skills: `grill-with-docs`, which loads
`grilling` + `domain-modeling`) that ends in a **complete, written plan for building the full exact Rust replica of
Street Legal Racing: Redline**, then (only after the user confirms shared understanding) file it as GitHub milestones
and issues. You are not implementing the replica in this session.

Repo: `/Users/nicolas/Documents/github/games/street-legal` (remote `nicolas-found42/street-legal`, default branch
`main`; current branch `exact-port`, **no commits yet**, everything is untracked).

## Hard rules
- **Never modify `reference/`** (`reference/raw` and `reference/raw-stock`). It is git-ignored and read-only.
- Follow `AGENTS.md`: GitHub Issues via `gh` (`docs/agents/issue-tracker.md`), triage labels
  (`docs/agents/triage-labels.md`), domain docs = `GLOSSARY.md` + `docs/adr/` (`docs/agents/domain.md`).
- The user's global agent instructions require Jev checkpoints (`mcp__jev__*`): `jev_decide` when 2–6 approaches compete,
  `jev_noul` for unevidenced assumptions, `jev_screen` on web/pasted text, `jev_gate`/`jev_review` before calling code
  work done. Load the `jev` skill before the first Jev call.
- Facts are your job (dispatch Explore sub-agents); decisions are the user's. Ask a whole frontier per round,
  numbered, each with a recommendation worded so "yes" accepts it.
- Be honest about status. The previous agent over-claimed early on; the user wants a *full* replica, not a slice.

## Read first (in this order)
1. `docs/adr/0004-exact-function-level-port.md` — the governing decision, tiered exactness, round-1 plan decisions.
2. `GLOSSARY.md` — canonical terms (Stock media, Clean stock install, Modded install, Complete, Exact, Oracle,
   Differential test, Native method, ...).
3. `docs/re-progress.md` — what is exact and corpus-tested vs open.
4. `docs/tufa-executor.md` — inferred model of the script VM (`Thread::run`), opcodes, operators, frames/calls.
5. `docs/research/rescope.md` — external research (prior art, Unicorn/x87 caveats, recompilers, Rust float/VM
   patterns, phased plan). Every claim is URL-cited; "(unverified)" items are unverified.
6. `docs/fidelity.md`, `docs/formats.md`, `docs/architecture.md`, ADRs 0001–0003 (0001/0003 superseded by 0004,
   0002 superseded: saves are exact SDAT).

## Target and assets (facts)
- Game: SL2 build 606 (Feb 7 2005), v2.2.1, 32-bit Windows x86 PE (D3D9, DirectSound, DirectInput8).
- `reference/raw` = **Modded install**. Contains `StreetLegal_Redline.bak.exe` (2005-08-16, SHA-256
  `7d77466a…8edde`, used for all RE so far) and a patched `StreetLegal_Redline.exe` (2006). Mods (MW_Mod,
  DestructionTools, later edits to 2008) **overwrote 94 stock `.class` files outside mod folders** (incl.
  `sl/Scripts/game/{Vehicle,GameLogic,Garage,Track,...}`, `parts/scripts/{Chassis,Transmission,Brake,Wheel}`),
  plus `parts.rpk`, `particles.rpk`, car scripts/cfgs; added `maps/Addon_Maps`, `ROC_cars`, etc. No backups.
  → Compatibility corpus only, not the behavioural reference.
- `reference/raw-stock/Street Legal Racing - Redline v2.2.1/` = **Stock media** (installer, not an install):
  `Street Legal Racing - Redline.msi` + `Data1.cab` (280 MB MS-CAB, 7,858 flat-named entries, 783 `.class`),
  `setup.exe`, `Setup.ini` (ProductVersion 2.2.1). Directory layout lives in the MSI tables. macOS `bsdtar`
  lists the cab but fails to decompress ("Invalid CFDATA", likely LZX). The media ships **both**
  `StreetLegal_Redline.exe` and `StreetLegal_Redline.bak.exe`, so `.bak.exe` is a stock name; which one is the
  real build-606 binary is **unconfirmed** until a clean extraction (compare SHA-256 with the two in `reference/raw`).
- Host: macOS arm64 (Apple M5 Pro), Rust 1.99, Docker 29.8 available, no Wine. Ghidra 12.1.4 at
  `/opt/homebrew/Cellar/ghidra/12.1.4/libexec`, JDK 21 at `/opt/homebrew/opt/openjdk@21/...`. REA MCP tools
  (`mcp__rea__*`) available.

## What exists (code)
- Cargo workspace, crates:
  - `slr-formats` — **exact, corpus-tested** decoders: RPAK, SCX v4 (v3 map pieces, 441 files, **not decoded**),
    cfg (incl. `_main_*.cfg` vehicle defs), SDAT header only, and the **whole TUFA class file**
    (`tufa.rs`: `OPWORDS` table from exe VA 0x5f0914, `Module::trees` — 1,546 modules / 5,816 bodies /
    320,522 nodes, zero trailing bytes; `pool` — 102,711 constant-pool entries; `methods`/`fields`/`class_rec`).
    The 11 modules in `system/Scripts/render/osd/Dialog.class` are an older format (0x108f5) the exe can't read.
  - `slr-core`, `slr-game` — the **old approximate playable slice** (macroquad, invented physics/AI/economy,
    procedural tracks). Not exact. Decision pending (Q13) on when to delete.
- Tests: `cargo test --workspace` green (51 + 3 + 8). Corpus tests currently point at `reference/raw`
  (override with `SLR_REFERENCE`).
- RE tooling: `re/ghidra_scripts/ExportAll.java` (headless export of all 4,356 functions' pseudocode to
  `re/out/all_pseudo.c` + `index.txt`; `re/out` is git-ignored and must be regenerated, see `docs/re-progress.md`),
  `re/out/native_api_fixed.tsv` (338 native method registrations: class, method, signature, impl address;
  registered via `FUN_00416b00`), `re/oracle/oracle.py` (Unicorn single-function oracle: loads the PE, calls a
  function with a calling convention, returns eax/st0). Needs a venv with `unicorn capstone pefile`
  (previous one lived in a session scratchpad and may be gone; recreate with `uv venv` outside the repo).

## Key RE facts already established
- Script VM = `Thread::run` `FUN_00420ff0`; class loader `FUN_00411b20`; callMethod `FUN_0041fbc0`;
  call-by-name `FUN_004207c0`; constant-pool reader `0x407660`; package roots set up in `FUN_0040fe70`
  (`java.lang`→`system/scripts/lang`, `java.game`→`sl/scripts/game`, parts, accessories, render, io, util, sound).
- 23 operator helpers identified by their own error strings (see `docs/tufa-executor.md`). Still open: negation
  direction for `!=`/`<=`/`<`, float NaN compare, frame fields `+0x30/+0x34/+0x1c8`.
- `enginepower` cfg handler at VA 0x00443f90 writes 6 floats into the vehicle struct (consumers untraced).
- `Direct3DCreate9` is called at pseudocode line ~87735; the **CreateDevice behaviour flags (FPU_PRESERVE?)
  are not yet determined** — this decides x87 precision for every float comparison (open risk in ADR 0004).
- `texture.dat` XOR decryptor `FUN_00401260`; CD check `FUN_00401480` (from earlier artifacts in
  `/var/folders/f_/7sw_z59s17qfpkf_0jzcs4hc0000gn/T/street-legal-re-artifacts/` — may no longer exist).
- Prior art: `EliasFD3S/slrr-reverse` (C++ host rewrite, MIT, no verification), Connor-Moyle SCX Blender
  importer (SCX v3–v6), CookiePLMonster SLRR-Proofix (original bugs, build 936).

## Settled decisions (do not re-litigate)
From ADR 0004 + grilling round 1 (user: "yes to all"):
1. Exact = function-by-function port of the stock exe; same outputs for same inputs incl. x87 behaviour where it
   matters. Tiers: A formats (byte-exact), B VM + natives (same stack/values/native-call sequence),
   C physics natives (0 ulp where oracle shows it), D rendering/audio/input (behavioural, screenshot),
   E saves (now exact SDAT).
2. Original scripts run unchanged in a Rust TUFA VM; only exe-native methods are hand-ported.
3. Assets read at runtime from the user's own copy; nothing derived from the original is committed.
4. A function is exact only when a differential test backs it; `docs/fidelity.md` becomes a per-function ledger.
5. **Complete** = stock single-player feature parity: written checklist in `docs/fidelity.md` (menus/options;
   career: dealer, used lot, garage, catalog, install/tune, day street races, night club drags, pink slips,
   police; quick race; free ride on every stock map; all stock cars/parts), each item passing its tier's test.
   Out of scope: mods, multiplayer, `Dialog.class`.
6. Order: VM + **headless boot to main menu** (render/sound/input natives log calls) → physics fed by real
   scripts → everything else.
7. Oracle: spike Wine-in-Docker running the stock exe; fall back to a "headless original" (stock exe under
   Unicorn with Win32/D3D9/DSound stubs). Claim end-to-end verification only where one actually ran.
8. Saves: exact SDAT read/write (ADR 0002 superseded).
9. Rendering: wgpu + a D3D9 fixed-function state emulator (shader generation); screenshot acceptance.
10. Stock media in `reference/raw-stock` is the source of truth; modded install is a compatibility corpus.
11. Tracking: commit current work on a branch; GitHub milestone per tier/feature, issue per subsystem,
    `ready-for-agent`.

## Open: round 2 (asked, NOT yet answered — start here)
Re-ask these first (recommendations in brackets):
- **Q8** Installer → files: (a) Rust `slr extract-stock` reading MSI tables + cab via crates `msi`/`cab`;
  (b) brew `msitools`/`cabextract`; (c) run installer under Wine. [(a)]
- **Q9** Extraction target: git-ignored `stock/`, regenerable; all acceptance tests switch to `stock/`,
  `reference/raw` remains a compatibility check. [yes]
- **Q10** Script compiler: the exe has a Java-subset compiler (`JVM::compileSource`) that recompiles when a
  `.java` is newer than its `.class`. Port it only if stock boot actually triggers recompilation (fact to check
  after extraction); else defer to a mod-support milestone out of scope. [yes]
- **Q11** Time/RNG: `Clock` and RNG seams; exact ports of the exe's timing and RNG (likely MSVC `rand` LCG —
  confirm); deterministic replay mode for all tests. [yes]
- **Q12** Platforms: macOS primary + Windows + Linux via wgpu; CI runs synthetic tests only, data-dependent
  tests stay local. [yes]
- **Q13** Old approximate slice: keep until the VM boots to the main menu, then delete in one commit. [yes]

## Remaining design-tree branches (later rounds — none settled yet)
Work these into frontiers as prerequisites settle; dispatch Explore agents for facts first:
- **Inventory/scope facts to gather**: full stock file inventory (after extraction): counts by format, which
  formats are still undecoded (SCX v3 maps, PTX/RDB textures, `texture.dat`, sound formats, particles, fonts,
  `.cfg` variants, SDAT bodies), stock script count and which natives they actually call (static scan of
  constant pools vs `native_api_fixed.tsv`).
- **x87 precision**: determine CreateDevice flags and the FPU control word in effect; decide the float model
  (plain f32/f64 vs an `X87` softfloat type, where) and how Unicorn's x87 fidelity is validated.
- **Crate architecture**: e.g. `slr-formats`, `slr-vm`, `slr-natives` (by class group), `slr-physics`,
  `slr-render` (wgpu D3D9 emu), `slr-audio`, `slr-platform` (window/input/clock), `slr-game` (host);
  seams for oracle comparison; how natives bind to VM objects (GameRef/ResourceRef handles).
- **Native porting strategy**: 338 natives grouped by class (WheelRef 29, Part 26, GroundRef 25, Chassis 23,
  System 22, ResourceRef 20, GameRef 18, ...); classify pure / stateful / I/O; acceptance per class;
  the native engine subsystems behind them (physics world, resource manager, GfxEngine, Osd, Sound, Input).
- **Physics**: rigid body, wheels/tyres, suspension, drivetrain, collision, damage; integration step and
  substeps; which parts are native vs scripted; oracle vectors and tolerances.
- **Maps/world**: SCX v3 decoding, map rpk structure, ground/collision meshes, traffic and pedestrians,
  police, day/night, weather/fog.
- **Rendering detail**: texture formats, materials, lighting, shadows, particles, OSD/frontend (script-driven
  UI), fonts, cameras, resolution/aspect handling; screenshot oracle source.
- **Audio**: formats, engine sound synthesis (SfxTable natives), 3D audio, music.
- **Input**: DirectInput mapping, ControlSet scripts, force feedback (in/out of scope?).
- **Saves/config**: SDAT body format, compatibility with original save files, config/options persistence.
- **Oracle engineering**: Docker image (amd64 Wine, Xvfb, software D3D), trace format (native-call log,
  per-frame physics state), Frida/hook layer, Unicorn headless-original scope, storage of oracle vectors.
- **Verification ledger**: per-function status keyed by exe address, how `docs/fidelity.md` is generated,
  definition of done per issue, Jev gate usage.
- **Known original bugs** (Proofix list): reproduce or flag?
- **Legal/licence hygiene**: no assets or exe bytes committed, dev-only GPL tools (Unicorn) not linked.
- **Work breakdown**: milestones, issue granularity, dependency graph, which issues can run in parallel
  sessions, acceptance test per issue, rough sizing.

## Deliverables of your session
1. Grilling until the frontier is empty and the user confirms shared understanding.
2. Update `GLOSSARY.md` inline as terms resolve; add ADRs only when hard-to-reverse + surprising + real
   trade-off (e.g. float model, crate architecture, oracle choice if it changes).
3. Write the plan: `docs/plan.md` (milestones, ordered issues with acceptance tests, dependency graph) and the
   parity checklist in `docs/fidelity.md`.
4. With the user's go-ahead: commit current work on `exact-port` (attribution lines per the system reminder),
   then create milestones and issues with `gh` per `docs/agents/issue-tracker.md`, labelled `ready-for-agent`.
