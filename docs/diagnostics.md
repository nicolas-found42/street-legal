# Boot diagnostics and historical replay

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
  artifact inventory. The tracked source diff is saved separately. Source,
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

## Verification and limits

```sh
python3 -m unittest discover -s re -p test_boot_diagnose.py -v
cargo test -p slr-boot --test diagnostic_capture
cargo test -p slr-vm diagnostics
```

These exercise immutable outputs, crash survival, causal snapshots, exact-word
preservation, replay provenance and false-acceptance rejection. The stock-menu
integration test also uses a unique output directory. Diagnostics do not turn
uncaptured historical runs into recoverable history, provide full memory
rewind, establish real-device fidelity, or establish boot parity without the
complete oracle comparison.
