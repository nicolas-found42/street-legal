# VM owner chronology and machine events

The opt-in `slr_vm::machine` stream records semantic allocation lifetimes and
execution order for a future native memory projection. Its identities are not
original x86 pointers. It does not remove the nested native ABI guard or establish
headless boot equality (#40), complete VM semantics (#110), or stock parity.

Start recording on a new `VmHost` with `start_machine_recording()` before loading
metadata, allocating objects, or creating threads. Create recorded threads with
`vm.new_thread()`, and attach frames through `Thread::push_frame`. Recording
rejects late starts, foreign thread pools, and frames transferred between threads.
Ordinary execution remains available with recording disabled.

`vm.machine_trace().pending(limit)` returns a repeatable prefix of entries, with
monotonic sequence numbers. `acknowledge(last_sequence)` removes only an existing
pending prefix. The queue has no ring eviction or filtering: consumers must drain
and acknowledge it to bound memory. The stream is currently an in-process API;
it is separate from the durable diagnostic archive and does not persist itself.

Events retain value/variable pool generations, frame and operand ownership,
current-frame switches, opcode PCs and errors, file/module/class origins, method
selection, native argument owners, and actual native results or boundary failures.
Mutable accesses lacking paired source stores emit explicit unresolved events;
consumers must not synthesize a complete memory projection from those paths.

Owned value cells, VM threads, and loaders cannot be cloned. Release a specific cell with
`thread.release(cell)`; equal payloads do not identify the owner to release.
Suspended callbacks transfer their existing thread into the scheduler. Abandoned
cells are retained as explicit observations rather than silently recycled.

The chronology repairs allocate and attach typed-zero parameter/receiver variables
before assignment conversion, and retain the original initializer thread/frame
lifetimes. These bounded behaviors have original build606 Unicorn comparisons;
the remaining raw loader/native ABI integration stays in the preserved worktree.

## Verification

Data-free recording contract tests always run:

```sh
cargo test -p slr-vm --test machine_stream
```

Regenerate the local original observations using the oracle environment:

```sh
~/slr-oracle-venv/bin/python re/oracle/vm_cell_identity_probe.py > re/out/vm-cell-identity-original.json
~/slr-oracle-venv/bin/python re/oracle/vm_parameter_chronology_probe.py
~/slr-oracle-venv/bin/python re/oracle/vm_instance_this_chronology_probe.py
~/slr-oracle-venv/bin/python re/oracle/initializer_thread_chronology_probe.py > re/out/boot/initializer-thread-chronology-attempt1.json
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-vm
```

The four new oracle suites skip only missing fixture files unless strict mode is
set. Existing but malformed fixtures, wrong executable/producer hashes, and
comparison mismatches fail. Derived fixtures stay under ignored `re/out/`.
