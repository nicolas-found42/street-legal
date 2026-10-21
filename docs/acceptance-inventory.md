# Acceptance inventory

Single index of every **remaining** acceptance obligation. Authority is
[fidelity.md](fidelity.md#whole-game-completion-contract); the P01–P30 feature
table there stays the one feature-status inventory and is not duplicated here.
This file tracks: cross-cutting gates, supporting evidence gaps that block
rows, and the final-verification obligations.

Status vocabulary: `open` (no evidence), `partial` (bounded evidence, gate not
met), `passed` (gate evidence linked, frozen revision). Nothing in this file
grants acceptance without the fidelity.md evidence requirements.

Last updated: 2026-10-21, branch `exact-port`, verified baseline `6b63613` plus
uncommitted integration.

## Cross-cutting gates

| Gate | Obligation | Current evidence | Status |
|---|---|---|---|
| G1 stock inputs/runtime | Clean stock install unchanged; runtime reads original assets and runs unchanged TUFA scripts; native macOS arm64 windowed/fullscreen; no approximate logic in shipped host; old slice removed when boot passes | `stock/` read-only + SHA-256 manifest check (`slr_io::stock`); `slr-boot` runs unchanged scripts. Windowed/fullscreen/display path not exercised; `slr-core`/`slr-game` still present | partial |
| G2 formats (A) | Every stock file parses with corpus counts and no unexplained trailing bytes; loader logic differential-tested; writers round-trip | `slr-ledger` corpus rows for RPAK/SCX/TUFA/cfg/SDAT/TEXTURE/skeleton; writer coverage partial (SDAT/config only). No single corpus-count report over all 7,858 files | partial |
| G3 VM+natives (B) | Per-opcode/value-helper/native differential evidence; all 353 registered natives implemented and registered by original address; complete boot native-log equality; end-to-end logs per row | 371 ledger `passing` rows, 118 `ported (no test yet)`; boot native log matches 614/6,508 filtered and 903/9,209 unfiltered at frame31; 170 native rows unimplemented in `missing_rows.rs` (capture-only) | partial |
| G4 physics level 1 (C) | Every physics native passes original-function differential tests over random+edge vectors, 0 ulp on trusted ops; float trust/control-word evidence | `slr-physics` cfg native vectors pass; several cfg rows still `ported (no test yet)`; x87 trust triangulation in progress (see G9) | partial |
| G5 physics level 2 (C) | Deterministic drag/braking/collision per-frame vehicle state bit-exact for every stock car (P25) and every stock part (P26) | Active free-body stepping exists; drive callbacks, contacts, solver, and per-car scenario traces absent | open |
| G6 rendering (D) | Per-frame D3D9-equivalent states/stages/transforms/draw streams equal hooked logs; named-scene screenshot review; real display path | `slr-d3d9` fixed-function emulator exists; boot emits no D3D9-equivalent stream log; no screenshot review | open |
| G7 audio (D) | DirectSound-equivalent call streams equal hooked logs; human listening per item | `slr-sound` native rows replay; no call-stream comparison or listening evidence | open |
| G8 input (D) | Keyboard/mouse/gamepad through original ControlSet scripts; deterministic DirectInput call streams match | Controller mapping native vectors pass; `Input` device enumeration/mapping and `device` command remain declared gaps | partial |
| G9 saves/config (E) | Same scripted session produces byte-identical SDAT/config; cross-load both ways | SDAT readers/writers and file natives have vectors; no end-to-end byte-identical session against the original | partial |
| G10 exceptions/evidence | Each original-bug fix confirmed in build 606 with trigger inputs and excluded only from those inputs; annotations and regenerated ledger agree; vectors/traces ignored; oracle tooling outside workspace | Intentional-divergence table maintained; particle raw-word exceptions enforced by schema; ledger regeneration is manual (`cargo run -p slr-ledger -- regen`) and currently stale vs tree | partial |
| G11 float parity (ADR0007/0008) | Bit-exact x87 transcendentals pinned to a documented implementation; SoftFloat/Unicorn/Rosetta triangulation; no tolerance-based approval | SoftFloat vendored; Unicorn capture regenerated after negative-zero encoder fix; Rosetta assessment pending (agent X87Audit) | partial |

## Supporting obligations blocking the gates

| # | Obligation | Status |
|---|---|---|
| S1 | Boot milestone (P01 intermediate): completed MainMenu + ≥300 idle frames, filtered native log equal to the 6,508-record endpoint, unfiltered/node divergences fixed or explicitly recorded | open — frontier at frame31, 614/903 matching records |
| S2 | Full 353-native registration by original address, including 170 rows not statically referenced by stock scripts | partial — `missing_rows.rs` retains capture-only rows |
| S3 | Production shape/file pipeline: real SCX v3/v4 decode, material/resource binding, worker pump at original frame position | open — decoder host has no production implementation; agent slice in progress |
| S4 | Per-car deterministic physics scenarios (24 models) and per-part effect scenarios | open |
| S5 | D3D9-equivalent call stream + screenshot harness against hooked original | open |
| S6 | DirectSound call stream + listening protocol | open |
| S7 | DirectInput call stream + ControlSet script exercise | partial |
| S8 | SDAT/config end-to-end byte-identity and cross-load | partial |
| S9 | Real window/display path (windowed + fullscreen) on macOS arm64 | open |
| S10 | Remove old approximate slice (`slr-core`, `slr-game`) after boot passes (plan M1-14) | open — blocked by S1 |
| S11 | Ledger regeneration from current tree and test receipts | open — stale |
| S12 | Human visual/listening review receipts for affected rows | open |
| S13 | Independent frozen-revision verification of the entire contract | open |

## Known frontier (boot, as of 2026-10-21T0409Z)

Strict diagnose bundle
`re/out/boot/runs/20261021T041614.363437Z-14de472bd89f`: filtered 602→615
frontier improved by routing native-class payload queries in
`engine::register_created` (per-class executed rows in
`re/out/boot/native-class-query-original.json`: car 1/1/1, cursor 0/0/1,
player 1/0/1, trigger 0/0/0, button 0/0/0; part 0/0/0 pending a constructed
payload; replay test
`native_message::tests::original_template_queries_8_9_28_match_per_class_rows`).
Stop: `NativeTemplateBoundary: part template construction is not integrated`
(`native_body::prepare_template`, class != car). Delegated slice: part
template constructor chain + executed part query rows.

## Reproduction commands

```sh
cargo run -p slr-ledger -- regen
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets
python3 re/boot-diagnose.py re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog --frames 331 --replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl --isolated-build
~/slr-oracle-venv/bin/python -m unittest discover -s re/headless -v
```

`--isolated-build` compiles into the run bundle's private target directory.
It exists because a shared `target/` can satisfy cargo while another build is
mid-flight and link a stale dependency rlib; that race produced the wrongly
reported `lod_amp` frontier in earlier bundles. Acceptance runs must use it.
