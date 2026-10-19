# Kuna second-opinion tooling

Keep Ghidra's `ExportAll.java`, `re/out/index.txt`, and `all_pseudo.c` as the
reference inventory and native-table input. C emission does not establish
behavioral parity; the executable oracle and `docs/x87.md` still govern it.

With Kuna 1.906 on PATH:

```sh
kuna install-skill --agent codex
python3 re/kuna/export.py export
python3 re/kuna/export.py lookup FUN_00420ff0
python3 re/kuna/test_export.py
python3 re/progress.py
```

The exporter declares Ghidra entries without inventing contiguous ends from
body-address counts. It uses bounded batches, three seconds per function, and
an external process-group RSS/time watchdog (768 MiB default). Kuna's own
`--max-fn-memory` rejects this macOS host. A killed batch leaves explicit errors
for each affected entry.

Commands, hashes, C, errors, and definitions stay under ignored
`re/out/kuna/ghidra-entries/`. `summary.json` records discovery overlap,
attempted/pending entries, failures, and diagnostics. `by-address.json` contains
C keyed by canonical address. A matching binary/index/tool/budget manifest
permits resuming. A changed profile requires a fresh output directory.
Errors or pending entries produce a nonzero exit. `warn:` and `jump-as-call`
diagnostics remain visible even when Kuna reports success.

Retry selected failures while retaining all reference entry declarations:

```sh
python3 re/kuna/export.py export --out re/out/kuna/retry-20s \
  --fn-seconds 20 --batch-size 1 \
  --addr FUN_00408f60 --addr FUN_00412b10 \
  --addr FUN_00458c00 --addr FUN_00487f20
python3 re/kuna/export.py lookup FUN_00412b10 --out re/out/kuna/retry-20s
```

Default and retry exports are separate observations. Combining them must not
erase the original timeouts or imply body equivalence.

Lookup accepts `FUN_`, `LAB_`, `sub_`, and hex spellings, resolving exported
entries. A native LAB label can be interior to a Ghidra function and absent
from its index: query these directly, e.g.
`kuna decompile reference/raw/StreetLegal_Redline.bak.exe 0x47e1c0 --addr`.
Keep exe annotations; map names by address. See `docs/re-progress.md` for
measured results and limitations.

## Thread dispatch option check (2026-10-19)

A bounded follow-up for #106 compared six profiles on `FUN_00420ff0`: the
existing `auto` baseline; `auto` plus `switchguardbound on`; `auto` plus
`readonly on`; both options together; `reliable`; and `aggressive`. Every run
used the full Ghidra entry declarations, one selected function, one worker,
20 seconds per function, and the existing external 45-second/768-MiB watchdog.
All six returned one C record without a reported error. Every record retains
three `jump-as-call` markers. The range-guard-only C is byte-identical to the
baseline; the read-only option changes a global-zero comparison but leaves the
three markers. The other presets also retain them. No option is promoted into
the reference pipeline.

The original first dispatch at `0x4210e5` is an indirect jump preceded by an
unsigned range check against `0x48` and a byte-map lookup. The dispatch at
`0x422e91` similarly guards 14 selector inputs and reads a byte map. These two
bounds are below the default 1,024-entry limit; merely increasing that limit
would not address their admitted selector counts. The installed option docs
identify `switchguardbound` as a per-program recovery experiment and warn
against making it a global default.

Exact argv, exit status, timing/RSS observations, baseline/variant JSON, and C
hash comparisons remain under ignored `re/out/kuna/thread-dispatch-options/`.
Use each `*.result.json` command array to repeat that profile. This is a
negative recovery result, not behavior validation. Existing original-backed VM
vectors remain the oracle; #106 stays open for table recovery, the other
diagnostic functions and unclassified inventory entries.
