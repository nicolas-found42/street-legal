# Acceptance validation

The existing whole-game contract in `docs/fidelity.md` remains authoritative:
P01–P30, G1–G11 and S1–S13 are all required. This tooling computes executable
results separately from inventory progress and supporting paths. A document,
receipt, test count, source annotation or equal trace prefix cannot assert a pass.

Use Python 3.11 or newer:

```sh
python3 acceptance/selftest.py
sh acceptance/run.sh --table --json
sh acceptance/run.sh --json
```

Exit 0 means every obligation passed, or that a valid `--table` report was
requested. Exit 1 means acceptance remains incomplete. Exit 2 means invalid
inventory or unavailable/untrustworthy evidence. `--table` always identifies
itself as report-only. The committed inventory includes local ignored evidence
paths; restoring those paths is required before using that inventory. Data-free
self-tests construct their own complete inventory and synthetic inputs.

Schema 2 preserves the existing obligation IDs, kinds, titles and feature tiers.
Inventory statuses cannot be `passed`: the previous S11 annotation is partial
until its fresh validator succeeds. No historical source-tree status is imported.

Supported live checks are explicit:

```sh
sh acceptance/run.sh --execute S1 --execute S11 --execute G1 --json
```

S1 builds in an isolated target and reruns the named full boot endpoint. It
recompares both complete native streams, records node differences, and checks
source, stock/executable, oracle, replay and policy identities. An existing equal
bundle still requires a fresh replay. This supporting boot milestone does not
close P01's intro/render requirements or another feature scenario.

S11 freezes source and available original fixtures, observes collector Cargo
children with required-fixture mode, and checks current ledger equality plus
idempotent regeneration. Missing/skipped/ignored/filtered observations cannot
silently pass. Validation output directories are excluded from fixture freezing
so a candidate cannot copy its own validation bundle recursively. The freezer
uses main's input requirements; older runtime-only snapshots and experimental
Euler captures are not prerequisites.

G1's stock checksum check is partial: it does not prove the real window/display
or unchanged-script runtime. S10 additionally requires fresh S1, removal of the
approximate slice, replacement-host execution and integration checks; it is
available through `--execute S10`. These validators inspect and execute existing
hosts; they do not replace runtime code. Other obligations report unsupported
until complete validators and applicable human reviews exist.

No original captures, generated inventories, model candidates or runtime
replacements belong in this tooling change. CI runs the data-free adversarial
self-tests, not a whole-game acceptance claim.
