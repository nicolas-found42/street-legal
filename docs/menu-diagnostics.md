# Interactive exact-port diagnostics

Normal window launches now run under the session supervisor automatically:

```sh
cargo run -p slr-boot -- stock --window
```

The terminal prints the bundle location under `re/out/menu/runs/`. Python 3 is
required alongside the native binary. For a window launch, `--output PATH`
selects a new **bundle directory**; streams are inside `PATH/runtime/`. A direct
headless invocation retains its existing `--output` runtime-directory semantics.
The supervisor captures stdout/stderr, records source, binary, stock and host
identities, freezes input scripts and a runnable binary copy, and registers the
session immediately in the shared append-only index. It never overwrites an
existing bundle. `manifest.json` starts as running and is finalized only after
the child exits and the evidence is checked. Source/stock/binary changes during
a run invalidate its verdict. The source identity describes the observed
workspace, not an attestation that a manually supplied binary was built from it.

All data is local and ignored by Git. No upload or remote service is involved.
Typed text is part of the input tape. Bundles can contain licensed projected
stock files; do not commit or publish them.

## Causal evidence

`runtime/session-00000.jsonl` and subsequent segments are a shared UI/worker
journal. Every record carries schema, run ID, global sequence, monotonic elapsed
nanoseconds, host thread and current VM context. Segments rotate at 8 MiB
(with an oversized individual record kept intact); each record is synchronized
before the operation continues. No history is evicted from this journal.

The chain is:

```text
input-received (UI event, source ID, drawable size, scale and window)
  input-apply (all events, exact batch tick, source ID, focused OSD)
    input-begin (VM sequence linked as callback parent)
      input-route (binding / pick / cursor callback / ignored-key reason)
        input-result (success or error and focus after callback)
          menu-state / render-state (original script state, picking and viewport data)
            frame-enqueue / frame-submitted / frame-presented
```

Raw keyboard events/modifiers and unsupported mouse, wheel and touch events
are also recorded, so a missing translation is visible. The replay tape uses
the translated events the VM actually consumed. Pre-menu input is recorded as
ignored with its reason. Every applied input is
written **before** invoking scripts; a crash leaves a discoverable unmatched
input ID. Events arriving in the same frame retain their complete order. Input
is applied with the current shared VM frame, rather than the previous frame.
The original `input-state.json` remains an atomically replaced, bounded convenience
summary; the segmented journal is the historical authority.

The journal also includes window creation, focus, resize, scale, GPU adapter,
surface recovery/errors, device loss, UI/worker heartbeats and stage timing.
Worker heartbeats occur every 60 VM frames. The UI records a worker stall after
five seconds without a new frame **after first presentation** and captures
`stall.png`. The supervisor independently checks UI and worker liveness (default
60 seconds), terminates a stuck process group, and registers failure. Total run
time defaults to one hour. SIGINT/SIGTERM finalize an interrupted bundle; if the
supervisor itself is killed, `status` identifies its orphaned running manifest
using the recorded process identity. Such evidence is diagnostic and unverified.

`frame-presented` means submission through the window surface's present call,
not physical display scanout or GPU timing-query completion. Frame/input IDs
join the UI thread to worker records; enqueue time and blocked-send duration
expose queue delay. CPU-rendered state images use the same offscreen OSD recipe
at 800×600, rather than screenshots of the compositor's window decorations.

The latest eight distinct state images are retained. Every image's render
recipe and PNG SHA-256 persist in the journal even after image-byte eviction;
eviction is explicit. VM failures add `failure.png` and `ui-failure.json` with
UI physics boxes and active viewport bounds. A rendering failure may prevent a
screenshot; that error is recorded rather than replaced with a success image.

## Find, inspect and replay

These commands emit JSON suitable for an agent or script:

```sh
python3 re/menu-diagnose.py status
python3 re/menu-diagnose.py latest-failure
python3 re/menu-diagnose.py inspect BUNDLE
python3 re/menu-diagnose.py timeline BUNDLE --event 123 --radius 10 --vm --limit 200
python3 re/menu-diagnose.py replay BUNDLE
python3 re/menu-diagnose.py verify BUNDLE --repeat 5
```

`timeline` reads compressed/segmented streams, limits returned rows and reports
total matching counts. `--vm` includes native/script events from the selected
journal records' VM frames. Input-route records link to VM callback parent
sequences for further causal inspection. `inspect` verifies the complete
artifact inventory for finalized bundles; a running/orphaned capture has no
completed verification claim. An unfinished final journal line can be inspected
as a diagnostic prefix, while malformed complete records fail parsing.

`replay` uses the frozen binary. `verify` uses the current debug binary by
default; build it first, or supply `--binary PATH`. Both restore the frozen input
tape, preserve application ticks/batch order, require identical stock content,
and run headlessly through the original scripts. `--repeat N` creates an
independent bundle for each attempt and reports the observed failure rate.
An explicit recorded `--frames` budget is preserved during replay. These tools
never substitute recorded native results into the VM.

For a previously completed session, verification compares exact menu-state,
render-recipe and offscreen PNG-hash transitions. It reports the first changed
transition and both sequence lengths. For a failed session, verification checks
the prior successful prefix and requires clean completion, with no unresolved
input callbacks, GPU errors or stalls. That narrower verdict establishes
progress past the recorded failure; specify scenario assertions to verify the
intended final behavior. GPU/backend differences can change PNG hashes and
should be investigated using the recorded environment.

For example, record an existing input script with explicit final assertions:

```sh
python3 re/menu-diagnose.py record --headless --input-script events.json \
  --expect expected.json --timeout 120 -- stock
```

`expected.json` supports `labels_include`, `selected`, `exit`, `game_state` and
`pending_state_call`. State checkpoints also run every 60 VM frames, so autonomous
state entry and completion remain visible after input callbacks return:

```json
{"labels_include":["$20000"],"game_state":"java.game.Garage","pending_state_call":false,"exit":false}
```

The assertion file is frozen and automatically reused during replay/verification;
`verify --expect PATH` can specify a new expected outcome for a fix. Input
scripts use the format in `crates/slr-boot/README.md`. `inputs/recorded-input.json`
is generated from every applied event, so a live session becomes a replay case.
If an abrupt failure prevented tape finalization, inspect its durable journal
prefix; the tool refuses to invent a complete replay tape.

The existing archive index can also list these bundles:

```sh
python3 re/boot-diagnose.py list --runs-root re/out/menu/runs
```

Interactive reports explicitly say parity was not evaluated. They do not change
boot oracle comparisons or accept any stock-parity campaign obligation.

## Cost, retention and preservation

Supervised sessions use metadata tracing, suppress persisted per-store events
(explicitly counted in `capture-end.json`), retain the VM's bounded recent event
ring, and take full VM checkpoints every 60 frames plus failure/completion.
Menu state and render transitions remain in the journal. `record --full` keeps
instruction/store events for an escalated investigation. Ordinary headless
parity tracing and checkpoints are unchanged.

Finished JSONL streams larger than 1 MiB are gzip-compressed. The default policy
keeps at most 20 unprotected bundles within a 2 GiB **logical artifact** budget;
projected stock and binary snapshots count even when APFS shares their blocks.
Failures are pinned; running/orphaned sessions and the newest managed bundle
are protected, so a successful capture remains inspectable even when older
failures fill the budget. Over-budget
protected evidence is reported instead of deleted. The append-only index
survives pruning. Explicit output bundles outside the managed runs root remain
indexed and discoverable but are not automatically pruned. Unreferenced content-store entries are collected by hash,
rather than by inode (reflink copies have distinct inodes).

```sh
python3 re/menu-diagnose.py prune --keep 20 --max-bytes 2147483648
python3 re/menu-diagnose.py export BUNDLE /chosen/local/incident.tar.gz
python3 re/menu-diagnose.py unpin BUNDLE
```

Export verifies the bundle and creates a local archive without overwriting a
file or writing into the bundle itself. It enables explicit backup; it does not
configure automatic off-machine backup. Unpin only after preserving or resolving
an incident. Removing a bundle removes its detailed bytes, not its index entry.

No system can recover events from historical runs that were never captured.
These captures also do not provide full-memory rewind, a native crash dump,
physical-display fidelity, or complete OS/controller input emulation.

## Checks

```sh
python3 -m unittest discover -s re -p 'test_*diagnose.py' -v
cargo test -p slr-telemetry
cargo test -p slr-boot --test stock_menu_input
cargo test -p slr-boot --test stock_menu_presentation
```

The data-free runner tests cover same-frame input retention, frozen scripts,
provenance, tampering, incomplete callback/crash evidence, heartbeat termination,
changed render detection, successful-prefix fix verification, explicit scenario
assertions, orphan discovery and retention. Journal tests exercise concurrent
writers and rotation without sequence gaps. Licensed-stock checks remain local;
CI exercises the runner and telemetry without licensed assets.

## New Career coverage

`cargo test -p slr-boot --test stock_new_career` drives the shipped menu,
name entry, character selection and welcome dialog into `java.game.Garage`.
It checks entry completion and several idle checkpoints, original player/money
HUD labels, and toolbar geometry. It requires verified local stock.

Camera template registration, native player camera references and loading reset
now support this script path. Garage toolbar render/body resources use a Tier D
static OSD projection from the shipped CFG and SCX data. Full 3D garage rendering,
car control and all garage operations remain outside this scenario's coverage;
this is not native parity acceptance. Stock scripts are unchanged.
