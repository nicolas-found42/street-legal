# Original-address coverage inventory

`inventory.py` produces a deterministic, read-only coverage assessment. It does
not regenerate `docs/fidelity.md`, edit acceptance results, or award parity from
source emission, Rust annotations, or legacy test-result claims. It requires the
existing oracle venv's `pefile` and `capstone` packages. Data-free integrity tests:

```sh
~/slr-oracle-venv/bin/python re/coverage/test_inventory.py
```

Generate the Ghidra/native/Rust/legacy-evidence report without another tool run:

```sh
~/slr-oracle-venv/bin/python re/coverage/inventory.py
```

Add independently reproduced Kuna entry discovery (not C emission):

```sh
~/slr-oracle-venv/bin/python re/coverage/discover_kuna.py
~/slr-oracle-venv/bin/python re/coverage/inventory.py \
  --kuna-inventory re/out/checkpoints/20261021-campaign/re-inventory/kuna-functions.json \
  --kuna-manifest re/out/checkpoints/20261021-campaign/re-inventory/kuna-functions.manifest.json
```

The report is `re/out/checkpoints/20261021-campaign/re-inventory/coverage.json`.
All output stays ignored. Each address records entry sources, C emission and
diagnostics, semantics status, source-hashed Rust annotations, raw differential
claims, native registrations, relocation reference sites, direct candidate edges
and unresolved indirect transfers. The candidate source manifest includes tracked
and untracked nonignored files, with exact content hashes; HEAD alone cannot
identify a dirty candidate. A fresh run identifies current files, not a frozen
source archive. Source retention and validated execution receipts remain required.
If the candidate changes during collection, generation fails and preserves the
previous report; rerun after the edits settle.

The graph follows x86 instructions from declared entries and discovered direct
CALL targets. It follows direct branch paths until returns or terminal jumps,
stops at other declared entries, and never interprets Ghidra body-address counts
as contiguous bounds. A register or memory indirect transfer remains unresolved;
an absolute IAT slot identifies only an import. Relocation pointers into executable
sections are potential targets, not proof of dispatch reachability. Incorrect
discovery seeds can decode data as instructions. Consequently even zero decode
gaps does not establish full control-flow recovery or supported reachability.

`source_emission` is separate from `semantics`. Historical Ghidra C has no local
binary-bound producer manifest; current file hashes identify it without certifying
its original production. Kuna discovery requires an executable/content-bound
manifest and rejects missing, duplicated, altered or failed cache records. The
historical 4,356-body Kuna export is absent locally and is not recreated merely
to improve a count. Reuse it only after its original manifest can be validated.
Focused `.kuna` project exports and JSON observations in `re/out/boot` are
retained separately by address and artifact hash, with their missing binary-bound
producer provenance explicit. They supply useful inspection material without
replacing the missing full-export cache or certifying recovered semantics.

Legacy `test-results.json` is retained as claims, with **zero imported validated
evidence**: its rows lack the candidate/executable/stock/oracle/comparison identity
required by the whole-game contract. This does not invalidate every historical
test; it refuses to treat their count as current verified parity. No semantic
dossier validator exists in this inventory, so their status is explicitly
unverified. The acceptance runner owns evidence validation; this tool is an
inventory and investigation aid.

No completion percentage is meaningful while the supported reachable address
denominator and indirect dispatch remain unresolved. Scope accounting recognizes
the existing CD-check replacement, compiler exclusion and data-only Dialog
exclusion. Platform adaptation retains Tier D obligations. No blanket CRT,
platform or unreachable-code exclusions are inferred.
