# Boot diagnostics and historical replay

For the exact-port native window, see [interactive session diagnostics](menu-diagnostics.md).
Window launches automatically create supervised, indexed input/presentation bundles.
The boot comparison runner below retains its separate parity verdict.

Use the unattended runner for boot investigations. It builds before running and
creates a new evidence bundle every time; it never reuses an old runtime log.

```sh
python3 re/boot-diagnose.py \
  re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog
```

`re/boot-compare.sh` is a compatibility entry point to this command. Its exit
status now matters: zero requires runtime success, valid complete captures, and
equality of both complete native streams, including their lengths. A successful
MainMenu construction alone does not satisfy this condition. A divergence is an
expected nonzero result while parity remains open.

## Evidence bundles

Bundles live under `re/out/boot/runs/<UTC-and-unique-ID>/`. The runner prints the
bundle path and a machine-readable result. `--runs-root PATH` selects another
archive root. Stock and reference inputs remain read-only. Each bundle contains:

* `manifest.json`: lifecycle status, Git revision and dirty source hash, binary
  identity, arguments, trace configuration, input/profile hashes and final
  artifact inventory. The tracked source diff is saved separately; untracked
  source bytes are retained under `source-untracked/`, with their original
  paths, modes and identities in `source_untracked`. Source,
  binary and input mutation during execution invalidates acceptance.
* `inputs/`, replay inputs and `stock-inputs.json`: frozen oracle/signature/profile
  inputs and a stock file inventory. Licensed stock files are still required to
  replay; the runner verifies their content, rather than changing them.
* `runtime.stdout.txt`, `runtime.stderr.txt` and build output: execution evidence,
  retained even when the process fails.
* `report.json`: runtime result, complete filtered/unfiltered comparisons, first
  mismatch and its correlated execution context.
* `runtime/`: the streams and snapshots described below.

The manifest starts as `running` and is finalized as `completed`, `failed` or
`interrupted`. `completed` means comparison acceptance, not merely child exit
zero. Runtime `capture.json` and `capture-end.json` separately describe runtime
capture completion. A missing end marker is incomplete evidence. Changing an
archived artifact makes verification fail. Hashes detect changes; they are not
cryptographic signatures against a party able to rewrite the entire bundle.

All bundles are local, gitignored and retained until explicitly removed. They
are not uploaded or backed up automatically. Preserve the whole bundle, not
just a call log, when archiving an investigation. Never commit stock, reference
files or original-derived captures.

To reconstruct a candidate, check out the manifest's source commit in a separate
directory, apply `source.diff`, then restore each `source_untracked` snapshot to
its recorded repository-relative path and mode. Obtain the unchanged licensed
stock inputs separately and verify their inventory. Do not restore over ongoing
work. Older bundles that contain only untracked hashes cannot reconstruct those
files; retain their original checkout/archive. Untracked source symlinks are
rejected rather than silently copied as regular files.

## Shared timeline and state

`runtime/events.jsonl` carries versioned events with a run ID, increasing event
sequence, frame, actual interpreter thread and script location. Native calls
have a separate identity that survives nested script/native callbacks. Native
log records expose zero-based filtered/unfiltered ordinals for joining the
timeline to comparison records. Reports display comparison records one-based.
Parent sequence links expose causal nesting; these are execution ordering
identities, not wall-clock durations.

The stream covers native entry/return, clock requests/results, scheduler queues,
GC phases, resource identity/ownership and successful VM stores. Floating values
retain their payload words or x87 register bytes. Observers do not sample the
clock, invoke scripts or change scheduling decisions.

Native call logs and JSON events are written incrementally without a userspace
history buffer. Each frame synchronizes streams and state; completion synchronizes
all streams before writing its durable marker. A killed process can leave a
partial final record, but previous written records remain available. A capture
with a partial tail cannot pass complete-trace acceptance.

`analyze` can inspect an unfinished bundle even if the wrapper was killed
before finalizing its manifest. That result is diagnostic-only, retains valid
record prefixes and reports unverified inventory and unknown process liveness.
It neither declares acceptance nor restarts the child.

`states.jsonl` records frame checkpoints: queue flags, wake deadlines, parked
script stacks, native roots, selected runtime object fields, resource ownership
and GC state. `failure.json` adds object fields and the recent event ring on
runtime failure. Targeted thread snapshots include locals, operand cells,
referenced objects and bounded String/array contents; total counts expose every
limit. Resources currently describe the headless identity projection, not
unimplemented graphics or physics payloads. Busy state in a nested callback is
explicitly reported unavailable rather than re-entering a mutable host.

On a complete native divergence, the runner ordinarily repeats the same case
and asks for a snapshot at the exact mismatching native call ID. The resulting
`divergence.json` contains the current thread as well as parked threads,
resources and recent events. It accepts this snapshot only when the rerun's
complete raw call logs reproduce the first run. A nondeterministic rerun is
reported as such. `--no-divergence-rerun` disables this extra execution.

## Controls and replay

```sh
# Full structured instruction history; metadata is the default.
python3 re/boot-diagnose.py ORIGINAL --trace-level full --event-ring 8192

# Validate and inspect an archived run without executing the game.
python3 re/boot-diagnose.py analyze BUNDLE

# Reproduce a historical case using its frozen configuration and inputs.
python3 re/boot-diagnose.py --replay-from BUNDLE

# Check whether a later build passes the recorded failing prefix.
python3 re/boot-diagnose.py --check-regression BUNDLE

# Replay the captured raw counter inputs, with strict clock-result comparison.
python3 re/boot-diagnose.py ORIGINAL --frames 331 \
  --replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl
```

`--replay-from` permits a new implementation binary and reports the code/binary
change. It requires the same stock contents and reconstructs prior trace,
scenario and oracle inputs. It does not require the original oracle/profile
files to remain at their former paths. Raw counter replay validates its paired
clock/frame inputs and source provenance; missing inputs or mismatched runtime
requests stop replay. It never substitutes recorded native results or scans
ahead to make an execution fit the tape.

The original-clock embedding starts at the earliest captured request, including
Config and field initialization. Startup uses the CRT's 53-bit x87 precision;
graphics activation changes the live embedding to 24-bit precision before
GameInit. The GameInit boundary asserts that all earlier requests were consumed;
it does not seek the tape. Thread names, script locations, precision and thread
lifetimes come from live execution and must match the original request context.
Any remaining mismatch stops replay and retains the failing prefix.

`regression.json` records the frozen case, earliest mismatch, comparison policy
and reproduction/verification commands. `--check-regression` reports whether
all records through that prior frontier now match. Its report distinguishes
this prefix assertion from complete parity; the command still exits nonzero
when a later divergence or any other acceptance condition remains.

Metadata tracing keeps a bounded recent instruction ring but omits instruction
events from the disk stream. Full tracing persists them too. Ring eviction,
suppressed steps, snapshot limits and diagnostic I/O errors are explicit.
`--event-ring N` sets the ring capacity. The legacy `SLR_TRACE_*` text switches
remain available through `--trace NAME=VALUE`; their effective values are
recorded. `--timeout SECONDS` bounds a child and cleans up its process group.

The legacy simulated-clock scenario explicitly masks `System.currentTime` and
`System.simTime` return values for comparison. Raw-clock replay defaults to
`--clock-mask strict`, preserving those primitive results. Reference identities
are normalized according to captured native signatures. The runner discovers
`source-original-facts.json` beside an accepted oracle log, or accepts
`--oracle-facts PATH`. It reconstructs every original record through the chosen
endpoint before trusting its per-call overload descriptors. The facts file is
frozen and hashed with the other inputs. Without this authority, ambiguous
primitive/reference positions retain raw words for diagnosis and prevent full
acceptance. Primitive resource IDs remain significant. Always read the recorded
comparison policy before interpreting a match.

## The run index

Every bundle is summarized in one line of `<runs-root>/../runs-index.jsonl`:

```sh
python3 re/boot-diagnose.py list   --runs-root re/out/boot/runs
python3 re/boot-diagnose.py query  --runs-root re/out/boot/runs --status failed
python3 re/boot-diagnose.py query  --runs-root re/out/boot/runs --commit 6b63613 --limit 5
python3 re/boot-diagnose.py status --runs-root re/out/boot/runs
```

A record carries the run ID and bundle path, start/finish, status and error,
source commit and dirty flag, binary hash, the effective configuration, both
comparisons with their first mismatching record, the correlated native call
ID, whether the targeted rerun reproduced the first run, the artifact count and
total bytes, and the flake summary for repeated runs. `status` additionally
names the newest failure and every failure with no later completed run of the
same commit.

The index is append-only: pruning bundles leaves the history readable, so
`list` still answers "what ran, and what happened" after the bytes are gone.
Bundles that predate the index, or that a killed wrapper never registered, are
added explicitly:

```sh
python3 re/boot-diagnose.py index --runs-root re/out/boot/runs
python3 re/boot-diagnose.py index --runs-root re/out/boot/runs --rebuild
```

`index` skips bundles already recorded and is safe to rerun. A bundle whose
wrapper manifest is missing because the process was killed is reconstructed
from its crash-surviving `capture.json`/`capture-end.json`/`failure.json`, and
the record is marked `reconstructed` — those runs are the ones most worth
finding. Records without a recorded start fall back to the bundle directory's
mtime, so `status` still means "newest" rather than "last appended".

## Retention and disk

Bundles are still local and explicit, but they are no longer unbounded:

```sh
python3 re/boot-diagnose.py prune --runs-root re/out/boot/runs --keep 20 --dry-run
python3 re/boot-diagnose.py prune --runs-root re/out/boot/runs --keep 20
# or prune automatically after a run:
python3 re/boot-diagnose.py ORIGINAL --keep 20
```

`prune` deletes the oldest finished bundles beyond `--keep`; a bundle whose
manifest still says `running` is never removed, because its writer may still
be appending. The index is untouched.

Inputs are content-addressed in `<runs-root>/../inputs-store/<sha256>`. Each
bundle still holds a real file of its own bytes — verification hashes the
bundle, so two bundles must never share a mutable inode — and on a filesystem
with reflinks (APFS, btrfs, XFS) those files share blocks, costing no
additional disk. The store entry is removed once no bundle references it.

The headless projection of `stock/` is clone-linked the same way. Three
successive 30-frame runs cost about 100 MiB in total rather than three copies
of the 458 MiB install.

## Repeated runs and flakiness

```sh
python3 re/boot-diagnose.py ORIGINAL --repeat 5
```

Each iteration gets its own bundle and index record. Every record carries a
`flake` block with the iteration number, the failure count over the batch, the
rate, and `nondeterministic` when some but not all iterations failed. The
existing divergence rerun already proves whether a single failing case
reproduces; `--repeat` turns that into a rate over identical invocations. The
command exits nonzero if any iteration failed.

## Filing the failure

```sh
python3 re/boot-diagnose.py ORIGINAL --file-issue dry-run   # print the gh command
python3 re/boot-diagnose.py ORIGINAL --file-issue            # create the issue
```

On a failure, the runner builds the issue from the index record it just wrote:
status, error, commit and dirty flag, binary hash, the first mismatch with its
comparison label and one-based record number, the correlated native call ID,
and the two commands that reproduce it (`--replay-from`) and re-check the
frontier (`--check-regression`). It is labelled `needs-triage`, per
`docs/agents/issue-tracker.md`. `dry-run` records the exact argv under
`report.json`'s `issue_command` instead of calling `gh`, so the command can be
inspected and reviewed before anything is published. A failure to reach `gh`
becomes `report.issue`; it never changes the run's own exit status.

## Wall clock and the environment

The timeline is execution ordering, never duration, unless wall time is asked
for:

```sh
python3 re/boot-diagnose.py ORIGINAL --wall-clock
```

`--wall-clock` adds `wall_ns` (monotonic, nanoseconds since process start) to
every `events.jsonl` line and to divergence artifacts. It is off by default
because it is the one field that legitimately differs between identical runs;
comparison never reads it.

Every bundle also carries `environment.json` inside `runtime/` and an
`environment` block in the manifest: OS, release, machine, Python, `rustc`,
`cargo`, `wine` (when present), CPU count, and the diagnostic `SLR_*`/`RUST_*`
environment variables the child saw. Unrelated variables — credentials included
— are never recorded.

## The interactive frontend

The macroquad frontend has the same streams behind an explicit flag:

```sh
cargo run -p slr-game -- --diagnostics re/out/frontend/runs/session-1
cargo run -p slr-game -- --autotest --diagnostics re/out/frontend/runs/autotest-1
```

* `events.jsonl` — screen changes, new game, save, load, test drive, event
  start, session finish, every checkpoint, and the autotest verdict.
* `states.jsonl` — the player-visible state at each checkpoint: money, day,
  reputation, car condition, and for a driving session the race phase, elapsed
  time, and every racer's position, velocity, gear, RPM, lap, progress, finish
  time and penalty, as exact `f32` bit patterns.
* `file-io.jsonl` — the save and load operations, with the resulting byte count
  and the state that was written.
* `manifest.json` — written from the first frame (`"status":"running"`) and
  rewritten atomically on exit, so a killed session is still discoverable and
  its file identities are still recorded. It carries the same
  `schema_version`/`run_id`/`started`/`finished`/`status`/`configuration`/
  `artifacts` fields the boot runner writes, plus `final`, the closing
  checkpoint. `embedding` is `slr-game`, which is how the archive tools tell it
  from a boot bundle.
* `autotest.json` — the pass/fail lists, written when `--autotest` finishes.
* `crash.json` — written by the panic hook, naming the panic and the last
  checkpoint. A fatal signal (`SIGSEGV`, `SIGABRT`, `SIGBUS`, `SIGILL`,
  `SIGFPE`) writes a minimal `{"reason":"signal","signal":N}` marker from an
  async-signal-safe handler before re-raising, so an abort mid-frame is
  distinguishable from a clean exit.

`--diagnostics-interval N` sets the checkpoint period in frames (default 30;
`0` checkpoints only on transitions). The directory must be empty or absent: a
diagnostic run never overwrites evidence.

A frontend bundle indexes, lists, queries and prunes through the same tools as
a boot bundle:

```sh
python3 re/boot-diagnose.py index  --runs-root re/out/frontend/runs
python3 re/boot-diagnose.py list   --runs-root re/out/frontend/runs
python3 re/boot-diagnose.py status --runs-root re/out/frontend/runs
python3 re/boot-diagnose.py analyze re/out/frontend/runs/session-1
```

Its `comparisons` are `null`, not `false`: there is no original-side native
stream to match, so a frontend session neither passes nor fails parity.

## Original-side evidence

The original's own file layer and its `diag.log` are now captured with oracle
runs rather than reconstructed:

* `observed/script-files.slrlog` — the original's `CreateFileA`/`ReadFile`/
  `WriteFile`/`CloseHandle`/`FindFirstFileA` operations, as `file` records in the
  existing format. They are deliberately a separate stream from
  `script-natives*.slrlog`, so a native-record comparison never meets a
  platform record.
* `observed/files/` — the bytes the original wrote, by repository-relative path,
  with sizes and hashes in the result's `observed_files` block.
* `observed/original-diag.log` — the original's `diag.log`, copied for
  convenience. It is the exe's own output: the log sink at `0x5513b0` reaches
  `0x5585e0`, which appends through the intercepted file layer.

## Verification and limits

```sh
python3 -m unittest discover -s re -p test_boot_diagnose.py -v
cargo test -p slr-boot --test diagnostic_capture
cargo test -p slr-telemetry
cargo test -p slr-vm diagnostics
python3 -m unittest discover -s re/headless -p test_boot_native.py -v   # needs the licensed install
```

These exercise immutable outputs, crash survival (including a real fatal
signal raised in a child process), causal snapshots, exact-word preservation,
replay provenance, false-acceptance rejection, the index, pruning, content
sharing, repeated-run flake accounting, wall-clock isolation and the frontend
manifest. The stock-menu integration test also uses a unique output directory.

`.github/workflows/diagnostics.yml` runs formatting, linting, the workspace
tests, the runner's own suite and an archive smoke test on every push. Suites
that need the licensed install or the original executable print an explicit
`SKIP:` line and pass when that data is absent, which is what CI sees; the job
therefore proves the code builds and its data-free tests pass, never that stock
parity holds. Parity runs stay on the developer machine, where the data exists.

Diagnostics do not turn uncaptured historical runs into recoverable history,
provide full memory rewind, establish real-device fidelity, or establish boot
parity without the complete oracle comparison. The frontend does not run the
original VM, so its native call log stays an empty valid header; its value is
the gameplay state history and the crash record, not script parity.

## Optional local oracle captures in workspace tests

GitHub CI does not contain licensed installs or original-derived captures. The
passive clock/timer tests use the local capture directory
`re/out/headless/clock-investigation/menu1800M/`. They print an explicit `SKIP:`
when that directory is absent. An existing directory is always passed to the
strict loaders: missing member files, malformed manifests, digest failures and
invalid provenance remain failures. Runtime replay loading has no skip policy.
The parser's hash-tampering regression uses synthetic test data and runs in CI.

The boot unit tests also check local `re/out/boot/` vectors individually and
stock-dependent unit tests require `stock/`. These tests skip only absent
fixtures; installed files are still read and parsed normally. Both process
capture integration tests use the existing verified-stock availability check.

Require the fixtures covered by this policy during a local oracle check:

```sh
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test --workspace
```

Any value of `SLR_REQUIRE_ORACLE_FIXTURES` enables this requirement for those
unit tests converted to this policy (passive clock/timer, boot vectors, and
stock callback/finalizer tests). Controller mapping, LOD, GC, string addition,
and stock String wrapper tests also honor this flag. This test-only flag does
not change older optional suites or integration availability checks; it does not establish
that every oracle suite has run.
CI's workspace command uses `--show-output` so its fixture skips are visible.
A green fixture-free CI job verifies code/tests, not stock parity.

On macOS, process-group cleanup confirms that an `EPERM` group has no live
members before treating it as already exited. A live member, failed process
inspection, or a permission error on another platform still fails the run.
