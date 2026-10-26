# Exact-port completion mandate

Work in `/Users/nicolas/Documents/github/games/street-legal`.

Own delivery of the complete native macOS arm64 exact port of stock **Street Legal Racing: Redline v2.2.1, build 606**. Implement, integrate, run, compare against the original, and independently verify the finished game. This is an execution request: carry the work through acceptance, rather than returning a plan or stopping after a component milestone.

## Definition of done

Use the whole-game completion contract in `docs/fidelity.md`, its governing ADRs and `acceptance/matrix.json` as the authority. Completion requires fresh passing evidence for **every P01–P30, G1–G11 and S1–S13 obligation**, all required visual/listening reviews, and independent verification of the frozen final candidate. Preserve the complete contract; resolve inconsistencies against the governing decisions rather than selecting the easier interpretation.

The delivered game must run from a clean build using the unchanged stock scripts and original assets, support the required native display/input/audio paths, and satisfy all stock single-player workflows, models, parts, physics, persistence and presentation obligations. Booting, a playable approximation, passing unit tests, recovered source, or a long matching trace prefix is an intermediate result.

## Ownership and autonomy

You own the critical path, implementation strategy, reverse engineering, missing tooling, integration, regression repair and acceptance execution. Choose tools and task sizes according to the uncertainty they resolve. Create or complete executable validators wherever the existing acceptance machinery lacks them. Fix the implementation and its evidence pipeline together.

This mandate authorizes bounded subagent delegation, superseding the earlier solo preference for this campaign. Use agents where independent investigation or verification improves delivery. You remain responsible for integration, shared-file ownership, reviewing their evidence and resolving disagreements. Agent count is not a target. Reserve independent verification for a verifier that did not implement the candidate; its fresh execution must satisfy S13, not merely endorse your summary.

Proceed through routine implementation decisions, reversible local work, scoped local commits and dependency fixes without asking me to approve each step. Preserve existing dirty/staged work and user data; keep unrelated changes out of your commits. Ask only for genuinely unavailable access, a required human observation, a material scope/fidelity decision, or a destructive action beyond this mandate. Bring a concrete decision or ready-to-review evidence package when asking, and continue independent work while awaiting the answer.

## Establish the live starting point

Read the repository instructions, `docs/boot-status.md`, `docs/plan.md`, the completion contract and acceptance inventory. Inspect the current tree and reproduce the earliest integrated failure with current-source evidence. Treat historical counts and receipts as context until reproduced. Preserve the source bytes, inputs and outputs needed to recover the starting state.

Use the dependency order that closes whole-game acceptance fastest. Continue beyond the first repaired boundary through complete boot, simulation/world, presentation/devices, career/persistence and the full parity sweep. Reconsider the implementation strategy when experiments repeatedly fail to reduce uncertainty; keep a concise record so repeated attempts do not become the work product.

## Incorporate the external critique

Read `.scratch/report/comparison_report.md`. Map every G001–G028 finding and C1–C9 capture experiment to the relevant canonical acceptance obligations, retaining unresolved evidence questions. Those G identifiers belong to the report and are distinct from the canonical G1–G11 gates.

The report evaluates the retired application in the showcase. Use its original-game observations and proposed demonstrations to expose acceptance blind spots. Re-establish applicability against stock build 606 before making them requirements. Keep the exact-port effort on its governing architecture and retirement plan. Resolve the report's applicable concerns in the final port and produce fresh evidence; a patch to the retired approximation does not close exact-port acceptance.

## Evidence integrity

Execute original behavior through the permitted oracle and name what actually ran. Bind each result to the candidate source, stock/exe identities, oracle/shim profile, replay inputs, tools and comparison policy. Keep original-derived assets, traces and vectors local and ignored as required. Preserve failed attempts.

Maintain the contract's comparison strength. Repair divergences instead of weakening tolerances, trimming sessions, skipping cases, inserting captured answers into runtime behavior, or relabeling stale evidence. Original-bug exceptions require the specified trigger-specific proof. Fixtures may establish bounded conditions; ordinary end-to-end workflows still need their required execution evidence.

Exercise the actual display, audio and input paths in addition to deterministic comparisons. Prepare required human review packages as soon as they are usable, with exact observations to judge and explicit outstanding receipts. A missing review or device observation keeps its obligation open.

## Integration, persistence and delivery

Keep the canonical status and issue dependencies current without creating competing progress inventories. Report concise updates about demonstrated behavior, the active blocker and the next experiment. Continue useful work through context boundaries and checkpoints; leave reproducible recovery instructions if execution is interrupted. A checkpoint records continuity, not completion.

When the implementation is ready, freeze the complete rebuildable candidate and required inputs. Have the independent verifier freshly rebuild and execute the entire acceptance matrix, required repository checks and applicable human reviews. Fix failures and repeat verification against the corrected candidate with accurate source provenance.

Deliver the working game and launch instructions, the frozen source identity, the complete computed acceptance results and linked reproducible evidence, the independent verification report, and a final video with game audio demonstrating the accepted workflows. State “100% complete” only when every required obligation has passed. If an external dependency prevents a required observation, identify the exact open obligation and needed action while completing all remaining independent work.

Start now by reproducing the current integrated failure and advancing the production port.
