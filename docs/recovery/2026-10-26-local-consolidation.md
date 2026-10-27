# Local worktree consolidation — 2026-10-26

This is a recovery consolidation of unfinished Street Legal work, not a passing
acceptance checkpoint. No remote branch or pull request was changed.

The latest `codex/recovery-20261026-raw-native` snapshot supplies the working
implementation. Earlier divergent versions are retained exactly in the other
merge parents, including `codex/recovery-20261026-native-abi` and
`codex/recovery-20261026-primary`. Research-only x87 code stays outside Cargo.
Use `git show <recovery-branch>:<path>` to recover any earlier file version.

Original staged/unstaged patches, binary indexes, source identities, the recovery
stash identity, and an all-ref Git bundle are under
`.git/recovery/20261026-consolidation/`. The detailed resolution manifest and
ignored-evidence relocation receipts are retained there too. The original
recovery stash is retained. Build caches are reproducible and were removed.

Implementation status at interruption: production raw Runtime startup/callback
installation remains incomplete and the ABI guard stays enabled. The last
331-frame replay fails at Input.lastKey. Clippy and full acceptance do not pass.
The last parameter-transfer edit adds ParameterAssignment observations but its
owner-consumption arms are unfinished; repair that before expecting compilation.
The in-flight full workspace and cold-owner tests were stopped for disk cleanup.
No interrupted test is credited as passing.
