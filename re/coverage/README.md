# Original-address coverage inventory

This tooling inventories original addresses and their evidence. It does not
award parity, regenerate the fidelity ledger, or change acceptance results.
Rust annotations, emitted C, discovery counts and legacy test results are
reported separately from validated semantics and differential evidence.

Use Python 3.11 or newer with the pinned dependencies:

```sh
python3 -m pip install -r re/coverage/requirements.txt
python3 re/coverage/test_inventory.py
```

With the read-only original executable and local analysis inputs available:

```sh
python3 re/coverage/inventory.py
# Optional: reproduce Kuna entry discovery, without a C export.
python3 re/coverage/discover_kuna.py
python3 re/coverage/inventory.py \
  --kuna-inventory re/out/coverage/kuna-functions.json \
  --kuna-manifest re/out/coverage/kuna-functions.manifest.json
```

The inventory reads `reference/raw/StreetLegal_Redline.bak.exe`,
`re/out/index.txt`, `re/out/all_pseudo.c` and `re/out/native_api.tsv`.
`re/out/test-results.json` and focused cached exports are optional claims.
Outputs default to ignored `re/out/coverage/`. CLI options select another
executable, repository or output path; generated outputs must remain under
that repository's ignored `re/out/` directory.

The source manifest hashes tracked and untracked nonignored files and rejects a
candidate that changes during collection. Kuna discovery records the command and tool version and checks executable,
exit-status and content identities before publishing its cache.
A changed or failed observation cannot overwrite a successful cache.

The instruction graph follows declared entries, direct calls and branches.
It stops at returns and terminal jumps; register/memory indirect transfers stay
unresolved. An IAT slot identifies an import, and relocation pointers identify
potential targets, without proving game dispatch reachability. Discovery seeds
can decode data as instructions. Ghidra body-address counts are never treated as
contiguous instruction ranges.

Historical exports without binary-bound producer provenance remain unverified.
Neither clean pseudocode nor an annotation demonstrates recovered semantics.
Legacy test-result rows do not become fresh validated evidence. The tool reports
no completion percentage while the supported reachable-address denominator and
indirect dispatch remain unresolved. Existing ADR exclusions remain narrowly
scoped; platform replacement still carries the Tier D obligations.
