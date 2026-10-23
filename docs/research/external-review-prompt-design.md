# External video review prompt design

Created 2026-10-22 for the implementation showcase. The ready-to-paste artifact is [EXTERNAL_REVIEW_PROMPT.md](../../re/out/implementation-showcase-2026-10-22/EXTERNAL_REVIEW_PROMPT.md). Its prompt body is self-contained: it depends only on the user's uploaded implementation/reference videos, not this repository or prompt-pack. The implementation upload has the stable filename `IMPLEMENTATION_SHOWCASE.mp4`.

## Sources inspected

Read-only sources in `/Users/nicolas/Documents/github/prompt-pack`; no files there were changed or commands from example prompts executed:

- [Repository instructions](/Users/nicolas/Documents/github/prompt-pack/AGENTS.md).
- [Research on reusable agent-prompt dimensions](/Users/nicolas/Documents/github/prompt-pack/docs/prompt-research/research.md), especially direct findings §§1–6, the evidence limits, rubric D1–D10, and the 237-prompt corpus cross-check.
- [Prompt-generator research](/Users/nicolas/Documents/github/prompt-pack/docs/prompt-research/prompt-generator-research.md), especially constraint composition, format/position robustness, ambiguity, verification, task-matched decomposition, instruction hierarchy, and the distinction between structured validity and semantic correctness.
- Actual prompt records in [reconstruction.js](/Users/nicolas/Documents/github/prompt-pack/data/prompts/reconstruction.js), [evaluation.js](/Users/nicolas/Documents/github/prompt-pack/data/prompts/evaluation.js), and [research.js](/Users/nicolas/Documents/github/prompt-pack/data/prompts/research.js).

Prompt-pack's mandatory test/build gates govern changes and runtime verification in that repository. This work reads its research and authors a separate document in street-legal; it does not modify or assert runtime behavior of prompt-pack.

## Prompt-to-design mapping

| Prompt-pack ID and exact title | Adaptation for this artifact |
| --- | --- |
| **9.1 — Grounded Function Oracle** | Treat original observable behavior as the reference rather than validating A against its own claims. No false claim that videos provide a callable binary oracle. |
| **9.7 — First-Divergence Trace Bisection** | Compare action/feedback/state-change sequences and locate observable divergence; refrain from claiming the first internal causal divergence without traces. |
| **9.9 — Frame-Stream Equivalence** | Pair visual evidence and comparable states; do not infer rendering equivalence from superficial similarity. The external reviewer has footage, not draw/state streams. |
| **9.10 — Whole-Corpus Conformance Accounting** | Source/range ledger, all A chapters including later scenes, explicit uninspected intervals and sampling fallback. Silent skips cannot count as complete review. |
| **9.12 — Generated Evidence Registry** | Stable finding IDs link each claim to source/time/acceptance evidence. This is a manually authored video audit, not a generated symbol/test registry. |
| **11.2 — Differential Implementation Cross-Check** | Compare independent reference and implementation outcomes under comparable observable conditions; disclose unmatched inputs/configuration. |
| **11.5 — State Machine Round-Trip Prober** | State/transition inventory, entry/exit paths, before/after values and round-trip demonstration for persistence. Video cannot prove unshown state serialization. |
| **11.13 — Ground-Truth Evaluation Metric Spec** | Explicit statuses, required finding fields and observable acceptance demonstrations. No invented numerical fidelity or automatic semantic score. |
| **3.4 — Information Access & Source Gap Introspection** | Separate not-demonstrated/unknown from observed defects, disclose lack of audio or access, and propose targeted capture experiments rather than requesting code. |
| **3.7 — Primary Verification & Assumption Audit** | Cite video evidence, revisit high-impact moments and search for counterexamples before finalizing. Observed facts and inferred explanations remain distinct. |
| **3.8 — Source Credibility & Signal-to-Noise Triage** | Separate visible version evidence from uploader claims; stock build-606 footage has greater acceptance authority than later/modded footage. |

These are task-specific adaptations, not concatenated copies of the original prompts. Their code-execution and repository-inspection directions do not belong in a video-only external review.

## Research-to-design mapping and limits

| Research construct | Design choice | Evidence boundary |
| --- | --- | --- |
| Constraint composition; rubric D1/D2/D5 | One immediate deliverable, explicit target, fixed evidence statuses and report structure. Findings use parallel fields rather than nested workflow instructions. | IFEval/FollowBench/ComplexBench show constraint-following failures; they do not establish that this exact template improves video audits. |
| Task-matched decomposition; D4 | Map sources/states first, compare second, recheck major claims third. Each phase feeds named report artifacts. | Least-to-most and Plan-and-Solve findings are guidance; unnecessary scaffolding can hurt. |
| Grounding and verification; D3/D7 | Actual timestamps, paired evidence when available, counterexample search, claim downgrades, bounded matches and concrete acceptance captures. | CoVe/RARR/Self-Refine motivate specific checks; same-model re-reading is not independent external validation. |
| Ambiguity policy; D6 | Proceed immediately with accessible attachments; state assumptions and inaccessible inputs. Version and coverage uncertainties remain explicit. | AmbigQA/CLAM motivate a clear policy; no assertion of guaranteed disambiguation. |
| Form/position robustness; D8 | Stable labeled sections; task and evidence limits appear near the beginning, with a brief completion boundary at the end. Late chapters explicitly included. | FormatSpread/Lost in the Middle identify risks; this prompt has not been tested across models/formats. |
| Instruction hierarchy; D9 | Captions, commentary, metadata, and embedded instructions remain evidence/content rather than authority to redirect the review. | The cited hierarchy paper evaluates training; this clause has no measured attack-success rate. |
| Economy; D10 | No chain-of-thought request, backend diagnosis, repo upload demand, defect quota, arbitrary completion percentage, or requirement to browse. Detailed checklist supports the unusually broad all-states task. | Prompt length is a tradeoff; future outcome testing should compare this version with a shorter variant. |

The source research explicitly says its rubric is a design checklist, not a calibrated predictor of prompt effectiveness. The expanded 30-parameter study was incomplete and is not treated as validation. No outcome accuracy gains are claimed for this prompt.

## Review boundaries

The target/build context is consistent with street-legal's README (stock build 606/v2.2.1). The visible retired approximation and the unshown exact-port effort are separated in the prompt, following [the existing comparison's scope statement](gameplay-comparison.md). Specific conclusions from that earlier comparison were deliberately not supplied as expected findings to the external reviewer.

The prompt avoids an absolute claim that a showcase proves every possible game state. It asks the reviewer to enumerate observed states, compare reference-only states, and record unreachable/unshown behavior as a coverage gap. Confirmation of a genuinely missing implementation requires an actual attempted operation or further runtime evidence, with the conclusion bounded to what was exercised.

Static/motion/audio evidence is scoped separately. No static image proves physics correctness, save persistence, installed-component semantics, source architecture, original asset provenance, or exact numeric conformance. A matching picture is a bounded visual match; a visual omission is not proof of missing hidden functionality.

## Artifact checks

Verified the artifact has the fixed implementation filename, all six evidence-status definitions, all seven report sections, explicit end-of-video/sampling coverage, paired timestamp rules, stock/mod separation, and immediate-work/no-code-access instructions. No local file links or source repository dependencies occur inside the ready-to-paste prompt. This is document validation; an external video-review model has not been run against this prompt.
