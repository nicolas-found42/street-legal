## Diagnostics

Before re-running anything for a boot, replay, hook or window-session failure, look at what was already recorded. These commands only read; none executes the game.

```sh
python3 re/boot-diagnose.py list   [--status failed] [--commit SHA]   # what ran
python3 re/boot-diagnose.py status --text                              # matched-prefix trend per commit; REGRESSION lines
python3 re/boot-diagnose.py digest RUN_ID_OR_BUNDLE --json             # bounded summary: provenance, first mismatch, causal chain, failure ring, exact replay commands
python3 re/boot-diagnose.py fixtures status                            # stale/missing local re/out fixtures, one line each
```

`digest` names the exact `--replay-from` and `--check-regression` commands and says, under `truncated`, what it cut. Bundles and the run index are local, may hold licensed stock-derived data and are never committed or uploaded. Runs live in `$SLR_RUNS_ROOT` (default `~/.local/share/street-legal`), not in a worktree. Details: `docs/diagnostics.md`.

## Agent skills

### Issue tracker

GitHub Issues via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `GLOSSARY.md` + `docs/adr/`. See `docs/agents/domain.md`.
