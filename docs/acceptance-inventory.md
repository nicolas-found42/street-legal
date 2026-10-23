# Acceptance inventory

Single index of every **remaining** acceptance obligation. Authority is
[fidelity.md](fidelity.md#whole-game-completion-contract); the P01–P30 feature
table there stays the one feature-status inventory and is not duplicated here.
This file tracks: cross-cutting gates, supporting evidence gaps that block
rows, and the final-verification obligations.

The machine-readable obligation/progress inventory is
[`acceptance/matrix.json`](../acceptance/matrix.json). `./acceptance/run.sh`
computes separate **executable results** over exactly 54 obligations: P01–P30,
G1–G11 and S1–S13. Inventory status (`open`, `partial`, `passed`) and existing
file paths cannot establish a passing result. The gate rejects omitted rows,
changed feature tiers, and unsupported passing claims. `--table` reports only;
`--json` emits computed results and an explicit `accepted` value.

Initial audit on 2026-10-22: **0/54 executable obligations accepted**. A later
source-bound isolated run passed S11 alone (499 rows, two identical regenerations);
its frozen archive is `re/out/checkpoints/20261021-campaign/acceptance-solo-frozen/ledger-2651bd9e1250`.
The boot validator's failure-reporting status collision was then reproduced and
fixed;34 data-free regressions pass. Current results must come from the new
live execution in `acceptance-solo-frozen-results-2.json`, not that older receipt. The default
report has 51 unsupported scenarios, S1/S11 not run and S10 failed (the old
approximate crates remain). Inventory progress is 42 open and 12 partial.
`--execute G1` verifies the extraction-pinned manifest, all 7,858 stock files,
no unexplained additions and the original exe; G1 remains partial because its
runtime/window/removal requirements are unmet. `--execute S11` retains an
isolated candidate source tree, binary source patch and untracked source copies,
frozen oracle fixtures, private Cargo target and full per-test outputs. It
freshly collects ledger evidence, rejects skipped/ignored/empty test runs,
regenerates twice, compares to candidate docs and verifies source/stock/fixture
stability. A prior receipt JSON cannot claim success for this validator.

`--execute S1` builds and runs the strict boot runner with `--isolated-build`,
then reanalyzes its complete raw native streams and MainMenu + 300 idle-frame
endpoint. It checks candidate dirty/untracked source identity, stock tree,
original exe, pinned complete declared-profile oracle identities and counts,
strict captured-clock policy, artifact integrity and build provenance. Full STEP
tracing records executed-node counts and the first divergence through the pinned
original endpoint, including field initializers and length differences. The port
label cannot distinguish static/instance initializer bodies; that normalization
is explicit and proves no parity of that distinction. Node differences are
recorded only under S1’s narrower contract; no node-parity claim follows. The
headless original oracle is named by its retained `source-run.json` (Unicorn,
`nt51-hypothetical-window-prefix-v1`); this headless profile proves no physical
device behavior. `--boot-bundle PATH` supports raw evidence reanalysis; stale
source is `stale`, and even equal stored evidence is `replay_required` until
live execution. P01 still needs its separate intro/render acceptance.

The first live S11 attempt was retained and failed because the isolated fixture
inventory omitted clock companion captures; the inventory was repaired. The
second isolated run correctly exposed four production failures in three
`native_car::graph_tests` and the `native_world` detach graph (143 bin tests
passed, 4 failed; 14 lib tests passed), so S11 remains failing. The source and
outputs are in
`re/out/checkpoints/20261021-campaign/acceptance-audit/live/ledger-b255ce1381c9`.
This is a bounded implementation check, and closes no feature row.

The earlier S11 graph failures are historical: subsequent full-buffer snapshot repairs pass22 installation checks and the latest bin checkpoint passed180 tests. A new isolated S11 execution is still required for the frozen candidate; those component results do not change S11 acceptance. The solo event update,44 original callback-list cases, resource-ID-reuse regression, genuine snapshot refresh after producer changes and experimental trig investigation are described in `docs/research/native-runtime-gaps.md`.

Complete validators for the remaining scenarios are still required. Each must
reproduce its entire obligation, validate candidate source (including preserved
dirty/untracked bytes), original exe, stock input corpus, oracle configuration,
replay schedule, comparison policy and applicable human review receipts. A
component test return code or editable counter cannot substitute for that
implementation. Missing fixtures, skipped observations and stale receipts must
fail closed.

The prior S11 `passed` claim is now `partial`: its retained checkpoint names
revision `7e2d2cb`, 486 evidence rows and 504 annotated symbols, and cannot
establish fresh collection/regeneration for a changing candidate. Historical
bundles remain linked in the matrix. The `acceptance-evidence` worktree is clean
at ancestor `6b63613`, with no unique commits or unfinished tracked/untracked
work; it was inspected and preserved. The campaign's starting-state archive is
under `re/out/checkpoints/20261021-campaign/starting-state-20261022T032015Z`.

## Cross-cutting gates

| Gate | Obligation | Current evidence | Status |
|---|---|---|---|
| G1 stock inputs/runtime | Clean stock install unchanged; runtime reads original assets and runs unchanged TUFA scripts; native macOS arm64 windowed/fullscreen; no approximate logic in shipped host; old slice removed when boot passes | `stock/` read-only + SHA-256 manifest check (`slr_io::stock`); `slr-boot` runs unchanged scripts. Windowed/fullscreen/display path not exercised; `slr-core`/`slr-game` still present | partial |
| G2 formats (A) | Every stock file parses with corpus counts and no unexplained trailing bytes; loader logic differential-tested; writers round-trip | `slr-ledger` corpus rows for RPAK/SCX/TUFA/cfg/SDAT/TEXTURE/skeleton; writer coverage partial (SDAT/config only). No single corpus-count report over all 7,858 files | partial |
| G3 VM+natives (B) | Per-opcode/value-helper/native differential evidence; all 353 registered natives implemented and registered by original address; complete boot native-log equality; end-to-end logs per row | Current candidate matches619 filtered /908 unfiltered records in frame31, then stops at the explicit nested VM-native ABI boundary in stock Part.addpart. Starting source stopped at54ecd0 input0x40491687; the bounded memory-consumer path now passes that input without closing scalar G11. The historical4,892/7,001 prefix used inherited approximate Euler conversion and later missed the cursor/Group event. Endpoint remains unmet; historical620/909 baseline is retained below. Ledger component evidence does not establish complete native/runtime parity | partial |
| G4 physics level 1 (C) | Every physics native passes original-function differential tests over random+edge vectors, 0 ulp on trusted ops; float trust/control-word evidence | `slr-physics` cfg native vectors pass; several cfg rows still `ported (no test yet)`; x87 trust triangulation in progress (see G9) | partial |
| G5 physics level 2 (C) | Deterministic drag/braking/collision per-frame vehicle state bit-exact for every stock car (P25) and every stock part (P26) | Active free-body stepping exists; drive callbacks, contacts, solver, and per-car scenario traces absent | open |
| G6 rendering (D) | Per-frame D3D9-equivalent states/stages/transforms/draw streams equal hooked logs; named-scene screenshot review; real display path | `slr-d3d9` fixed-function emulator exists; boot emits no D3D9-equivalent stream log; no screenshot review | open |
| G7 audio (D) | DirectSound-equivalent call streams equal hooked logs; human listening per item | `slr-sound` native rows replay; no call-stream comparison or listening evidence | open |
| G8 input (D) | Keyboard/mouse/gamepad through original ControlSet scripts; deterministic DirectInput call streams match | Controller mapping native vectors pass; `Input` device enumeration/mapping and `device` command remain declared gaps | partial |
| G9 saves/config (E) | Same scripted session produces byte-identical SDAT/config; cross-load both ways | SDAT readers/writers and file natives have vectors; no end-to-end byte-identical session against the original | partial |
| G10 exceptions/evidence | Each original-bug fix confirmed in build 606 with trigger inputs and excluded only from those inputs; annotations and regenerated ledger agree; vectors/traces ignored; oracle tooling outside workspace | Intentional-divergence table maintained; particle raw-word exceptions enforced by schema; historical ledger regeneration is retained, but must be repeated and validated against the candidate source; exception/whole-game evidence remains partial | partial |
| G11 float parity (ADR0007/0008) | Bit-exact x87 transcendentals pinned to a documented implementation; SoftFloat/Unicorn/Rosetta triangulation; no tolerance-based approval | SoftFloat vendored; Unicorn capture regenerated after negative-zero encoder fix; operation trust and bit-exact transcendental evidence remain incomplete; see docs/x87.md | partial |

## Supporting obligations blocking the gates

| # | Obligation | Status |
|---|---|---|
| S1 | Boot milestone (P01 intermediate): completed MainMenu + ≥300 idle frames, filtered native log equal to the 6,508-record endpoint, unfiltered/node divergences fixed or explicitly recorded | open — current matching prefix619/908; explicit nested VM-native ABI boundary in frame31; historical later Group callback failure remains unresolved; endpoint not reached |
| S2 | Full 353-native registration by original address, including registrations not statically referenced by stock scripts | partial — complete verified implementation/registration is not demonstrated |
| S3 | Production shape/file pipeline: real SCX v3/v4 decode, material/resource binding, worker pump at original frame position | open — integrated decode/material/worker behavior is not accepted |
| S4 | Per-car deterministic physics scenarios (24 models) and per-part effect scenarios | open |
| S5 | D3D9-equivalent call stream + screenshot harness against hooked original | open |
| S6 | DirectSound call stream + listening protocol | open |
| S7 | DirectInput call stream + ControlSet script exercise | partial |
| S8 | SDAT/config end-to-end byte-identity and cross-load | partial |
| S9 | Real window/display path (windowed + fullscreen) on macOS arm64 | open |
| S10 | Remove old approximate slice (`slr-core`, `slr-game`) after boot passes (plan M1-14) | open — blocked by S1 |
| S11 | Ledger regeneration from current tree and test receipts | partial — executable isolated validator exists; historical7e2d2cb checkpoint and failed live collection retained; latest changing candidate awaits coherent freeze and rerun |
| S12 | Human visual/listening review receipts for affected rows | open |
| S13 | Independent frozen-revision verification of the entire contract | open |

## Reproduced integrated failure

Current-source replay is `re/out/boot/runs/20261022T075801.098749Z-a0b09acb3e17/`:619/908 matching native records, then the explicit nested VM-native ABI boundary in stock Part.addpart. The scalar float-trust gate remains open despite the bounded Euler memory-consumer implementation. A193-test full bin run passes after genuine snapshot recapture; no feature/boot acceptance follows. See the canonical [boot status](boot-status.md) for commands and current dependencies.

Historical later integration using an inherited approximate Euler path is
`re/out/boot/runs/20261022T041215.985487Z-8ad90dd15a38/`: it matches the
original through 4,892 filtered / 7,001 unfiltered records, then differs at
4,893/7,002. The runtime reaches frame32 and consumes13,071 clock requests
before expecting `THRD-RUNVMI Dummy.handleEvent` on `java.render.Group` while
the port begins `THRD-RUNVMI Ljava.render.Osd;.animate`. Emitted lengths are
4,895/6,508 filtered and7,005/9,209 unfiltered. The later currentTime NaN is
a consequence of the latched missing-callback error. This is an incomplete
production run; it is not a frozen-candidate acceptance pass. Original capture
pins the Group event to native cursor460140 (caller460539), mask10000; the
coherent native frame subsystem remains under implementation. Current campaign
state and next implementation work are maintained in
[boot-status.md](boot-status.md).

Historical reproduced starting state:

The campaign baseline was built and replayed in
`re/out/boot/runs/20261022T032015.415079Z-61591a6515d2`. Its strict runtime exits
1 on a thread-context mismatch: expected `THRD-RUNVMI Dummy.getInfo`, observed
`THRD-INSTVAR-INI` in `java.game.parts.Part` instance-field initialization.
First native-call differences are filtered record 620 and unfiltered record
909; emitted log lengths are 4,399/6,508 and 6,507/9,209 respectively. These are
mismatching, incomplete sessions. They do not establish a passing boot or P01.
The independent reanalysis receipt is
`re/out/checkpoints/20261021-campaign/acceptance-audit/reproduced-baseline-analysis.json`.
This is the reproduced campaign starting state; subsequent runtime progress is
recorded in [boot-status.md](boot-status.md).

## Gate regressions

`python3 acceptance/selftest.py` exercises the former single-row/document-only
false pass, each omitted obligation, fabricated passing statuses, modified tier
requirements, malformed inputs, missing/path-escaping evidence and report-only
semantics. `acceptance/observations.py` supplies a strict component-test
observation helper: even exit 0 cannot pass if `SLR_ORACLE_SKIPPED:` appears in
stdout/stderr, a legacy `SKIP:`/`SKIP ` marker appears, no tests ran, tests failed
or tests were ignored. A filtered suite must name its expected tests and every
required identity must have executed successfully. A valid
component observation still cannot establish a complete acceptance obligation
or prove that a comparison policy is semantically valid. Existing snapshot
exclusion policies require their separate provenance/policy audit; S11 only
validates fresh execution and faithful ledger regeneration.

## Reproduction commands

```sh
python3 acceptance/selftest.py
./acceptance/run.sh --json # exits 1 while contract remains incomplete
./acceptance/run.sh --json --execute G1 --execute S11
./acceptance/run.sh --json --execute S1 # fresh isolated boot rebuild/replay
./acceptance/run.sh --table # report only, not acceptance
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
