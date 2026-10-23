# Gameplay comparison: original-executable evidence

Investigation date: 2026-10-22. Applied the explicitly requested `reverse-engineer-anything` skill, using REA 6.7.0 and its Ghidra 12.1.4 provider. This adds static original-executable evidence to the [visual comparison](gameplay-comparison.md); it is not an original runtime session or a parity test.

## Target identity and retained evidence

Opened the project's stock `StreetLegal_Redline.bak.exe`, PE/x86, SHA-256:

`7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde`

The stock filename is relative to the repository, not a downloaded video artifact. Its identity was returned by REA and independently computed with `shasum -a 256`. Analysis used an ephemeral imported copy; the original was not modified or executed.

Retained under `re/out/gameplay-comparison-2026-10-22/`:

- `rea-evidence.json`: complete three-record Evidence bundle, including limitations.
- `rea-summary.json`: target, provider, evidence IDs and coverage summaries.
- `rea-analysis-snapshot.json`: exported session snapshot; the native session was then closed.
- `0x00475ec0.c`, `0x474600.c`, `0x472b30.c`: focused decompiler output for reading.
- `rea-render-query-error.json`: the unsuccessful renderer query, preserved separately.

## Findings

### Component commands are executable paths

Evidence **`ev_80ba1c22d2ef1e7f31c80e124e688db90b1482aa673556fec2124bc6c6bfc708`**, operation `analyze_function`, procedure `FUN_00475ec0`, analyzed body `0x475ec0–0x4769d2`.

Observed static control flow parses an incoming command and branches on `install`, `remove`, `repair`, `flap_toggle`, `texture`, `bigdecal`, `decal`, `paint`, `colorscheme`, `add_part`, `rem_part` and `filter`. This is stronger than merely finding strings: the decompiled branches contain argument parsing and calls to different implementation functions. The `install` path reaches `FUN_00473b10` then `FUN_00474600` when its result is nonzero; `remove` reaches `FUN_00472b30`; `repair` reaches `FUN_004748f0`; `paint` reaches `FUN_0048db00` under its branch conditions.

Inference: the exposed-component workshop in the gameplay references corresponds to distinct operations in the original runtime, rather than just different paint/preview screens. This static check does not establish that every command succeeds, which original script/UI event sends it, or the complete effect of every callee.

### Installation and removal involve object state

Evidence **`ev_1bffb3fd49480beff5c6c743ab261a10c6f12c4df403158ae7f173c5eeab90ac`**, operation `procedure_pseudo_code`, `FUN_00474600`.

Observed: calls to `FUN_0048aba0`, a helper returning a value (`FUN_00472e80`), calls to `FUN_0048aea0` and `FUN_0048af20`, traversal of object records, writes to flags and linked references, further resource queries, and a conditional `bone01` lookup/transform call. The [local render-graph module](../../crates/slr-boot/src/native_render_graph.rs) and [installation module](../../crates/slr-boot/src/native_install.rs) supply the current interpretation of these raw layouts as render attachment, slot and part operations. Those names are repository interpretation, not recovered source symbols.

Evidence **`ev_c284de699fac1c8756e23b4175d2e0425aa822ee7951445880be566da14d26d2`**, operation `procedure_pseudo_code`, `FUN_00472b30`.

Observed: a state check at object `+0x5c`, slot/record traversal, flag updates, resource preparation/queries, an accumulated condition that can cause an early return, and a final call to `FUN_004723f0`. Interpreting these fields as the part dependency/removal state follows the repository's existing native-install model. Complete removal behavior was not executed in this task.

Practical implication: reproducing the mechanical workshop requires the original script/resource/part graph plus its visible rendering. Adding component-looking controls to the retired frontend would not establish this behavior. The exact-port tree already contains installation and render-graph work; absence from the playable screenshots does not mean those modules are absent from the repository.

## Unresolved and bounded conclusions

- Ghidra reports **two unresolved indirect/computed call targets** in the command dispatcher. Its callee relationship coverage is partial. No whole-graph completeness claim is made.
- Decompiled code is a provider-derived representation with uncertain types/signatures, not original source. Raw layout field names are interpretations.
- A separate query for source-annotated renderer address `0x47c1d0` did not resolve to an admitted procedure in this Ghidra analysis. REA reported an invalid/unmapped fallback file-offset lookup. No renderer behavior is concluded from that failed query. It was not retried as though an empty result proved absence.
- This task did not run the original Windows executable, inspect live D3D calls, audit stock scripts end to end, or validate the new port against original executions.
- The current [boot status](../boot-status.md), [fidelity contract](../fidelity.md) and [ADR 0006](../adr/0006-completion-plan.md) remain authoritative. This note changes no acceptance status.
