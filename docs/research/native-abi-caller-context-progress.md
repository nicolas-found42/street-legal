# Raw native caller context: continuation evidence

This branch is in progress. The production ABI guard remains unchanged. The
preserved runtime still cannot supply the complete callback/interpreter caller
chain. None of the evidence below authorizes a guard exception, a captured stack
template, or substituting the typed native backend for raw caller reconstruction.

## Current source projections

`native_vm_dispatch` produces real pending CALL frames, rather than accepting
register or stack templates in its runtime interface. Its admitted continuations
cover:

- `41fbc0..41fc31`: method/string lookup and a cached signature, or a newly
  allocated method signature using an existing original canonical name.
- `41fc31..41fe04`: native registry traversal, including hash collisions,
  class/method comparisons, and canonical signature-pointer identity.
- `41fe04..420025`: cached zero-argument receiver allocation and argument vector.
- `420025` through native entry: existing `native_vm_abi::call_prepared`.
- The zero-argument integer native suffix: result ownership, value-cell release
  and reuse, operand insertion, thread flags, temporary-vector frees and return.
- The zero-argument instance script branch: original frame-pool allocation,
  frame construction/list insertion, `$this` local allocation, `()V` signature
  type materialization and return to the caller with the new current frame/PC.

`native_vm_signature` retains the raw CALL history of `407ac0`, `41dc30`, the
existing-name paths of `41e800/41e610`, and the `()V` path of `41dfd0/41e2d0`.
`native_vm_values` retains the three distinct frame/value/variable pool layouts
and register effects; they are not interchangeable semantic pool indices.

Other parameter/return paths, new canonical-name allocation, zero-local frame
construction through the dispatch environment, and general script parameter
transfer remain guarded or unproved. Additional source arms in these modules
have no parity credit merely because they compile.

## Verification

Local derived captures remain under ignored `re/out/boot`; no original pages,
bytecode, executable bytes, vectors or traces belong in Git.

```sh
cargo test -p slr-boot --bin slr-boot native_vm_dispatch::tests::
cargo test -p slr-boot --bin slr-boot native_abi_guard_tests::
cargo check --workspace --all-targets
```

The dispatch tests cover 80 explicit controlled caller/endpoint combinations,
including varied retained stack contents and GPR seeds, static/instance flags,
and class/method/signature collisions. They compare the complete 32 KiB heap,
2 KiB stack, selected globals and all eight GPRs at every allocator/DirectInput
leaf and endpoint. The complete integer cases execute original `47cc10/556e00`
with an explicitly quiescent DirectInput device contract.

The provenance-validated genuine first `Osd.animate` replay additionally proves:

| Original path/endpoint | Complete observed pages | Declared leaves |
| --- | ---: | ---: |
| `Input.lastKey`: dispatcher entry through dispatcher return | 18 | 5 |
| `Osd.animate`: uncached signature/shared prefix | 11 | 1 |
| `Osd.animate`: frame creation and receiver attachment | 17 | 2 |
| `Osd.animate`: script dispatch return/current-frame handoff | 20 | 5 |

All selected pages and all eight GPRs are compared at every declared leaf and
endpoint. The original executes its actual scripts and method/native lookup;
there are no VM or native-result hooks. The genuine startup capture has control
word `0x003f`; controlled vectors use `0x007f`. These are pinned independently.
This is not a claim of x87 status/register-stack or EFLAGS parity.

A negative control changed the retained dispatch return PC from `41fc27` to
`41fc28`: the controlled comparison failed on the exact retained stack byte.
The source was restored and the comparisons passed again.

Capture commands (the evidence root must contain the original validated stock
snapshot, its profiles and stock identity; it is read only):

```sh
~/slr-oracle-venv/bin/python re/oracle/native_vm_dispatch_prefix_probe.py
~/slr-oracle-venv/bin/python re/oracle/native_vm_dispatch_genuine_probe.py --evidence-root /path/to/evidence-checkout
~/slr-oracle-venv/bin/python re/oracle/native_vm_dispatch_genuine_probe.py --evidence-root /path/to/evidence-checkout --method animate --endpoint prefix --output re/out/boot/native-vm-dispatch-genuine-animate-prefix-original.json
~/slr-oracle-venv/bin/python re/oracle/native_vm_dispatch_genuine_probe.py --evidence-root /path/to/evidence-checkout --method animate --endpoint script-frame --output re/out/boot/native-vm-dispatch-genuine-animate-frame-original.json
~/slr-oracle-venv/bin/python re/oracle/native_vm_dispatch_genuine_probe.py --evidence-root /path/to/evidence-checkout --method animate --endpoint return --output re/out/boot/native-vm-dispatch-genuine-animate-return-original.json
```

The genuine probe's initial pages are comparison inputs only. Production
functions consume live addresses and actual pending CALLs. They do not read
these capture files.

## Remaining integration

The guard cannot be removed on these proofs alone. Production still needs:

1. A causal raw metadata/object/owner producer connected to the lossless VM
   machine stream, using the existing source-derived loader, name, pool and
   object construction paths. Semantic IDs are not raw pointers.
2. Source-derived callback wrapper/Thread creation and method selection frames.
3. The live `Thread.run` entry/opcode/native-dispatch context, including its
   persistent scratch and relevant FPU state, and nested callback ownership.
4. General argument transfer/conversion and the next native boundaries reached
   by the unchanged scripts, with strict composed original comparisons.
5. A production guard handoff based on actually produced/proved context, then
   the original 331-frame strict replay and menu/garage regression checks.
6. Fixture-free CI availability policy for the preserved raw tests, complete
   hook checks/review, feature-branch push, PR and merge only after CI passes.

A correct native kernel plus a genuine captured incoming caller proves that
kernel on those inputs. It does not prove that the live port produced the caller.

## Live callback ownership follow-up

The live callback path had a separate defect before raw caller reconstruction:
`run_callback` constructed a private `Thread::new()` while the host was recording
its shared machine stream. Both stock callback tests failed with `MachineContext`.
The callback now uses `VmHost::new_thread()` and records its actual priority,
flags and creator name before entering any frame.

Completed callbacks now invoke an explicit VM thread destructor. It records
retirement before destroying frames and releasing their value/local owners.
Repeated retirement does not replay releases, and retired threads cannot be
re-entered. Initializer retirement uses this same path. An unprotected void
callback is retained only while its actual continuation remains resumable;
a completed void callback disposes its queue immediately. Previously that
completed branch could leave its queue allocated and drop frames without the
machine stream recording destruction.

The embedding boundary now receives `NativeContext` with immutable borrows of
the actual host/thread, the selected defining class and method index, native
implementation and live argument owners. It runs before any native thread hook
or implementation. Nested invocation tests check that the inner context has its
own thread/cells and that returning restores the outer argument-owner scope.
These semantic identities are not raw executable addresses. The boot embedding
still delegates to the unchanged rejection guard.

Verification commands:

```sh
cargo test -p slr-vm
cargo clippy -p slr-vm --all-targets -- -D warnings
cargo test -p slr-boot --bin slr-boot callback_wrapper_tests::
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot native_vm_dispatch::tests::
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot native_abi_guard_tests::
```

The authored callback lifecycle and live native-boundary tests require no stock
files or oracle captures. Temporarily restoring the private `Thread::new()`
constructor makes the authored callback regression fail with `MachineContext`;
the source is restored before the green run. The stock scalar and waiting
callback tests also pass locally, with real original scripts.

This follow-up does **not** implement the raw callback/Thread.run producer. The
unchanged live original-script replay still stops at nested `Input.lastKey`;
the prior unfiltered prefix of 646 records remains the acceptance baseline.
Raw metadata/object ownership, the callback wrapper's actual interpreter entry,
Thread.run opcode scratch and relevant FPU history remain required. The isolated
dispatch comparisons are still kernel proofs with genuine incoming test inputs,
not proof that production supplied those inputs. The feature is not ready to
merge, and no PR or merge is justified by the ownership tests alone.

## Retained callback and interpreter continuation (2026-10-25)

The new `native_vm_callback` and `native_vm_thread_run` continuations now compose
from the actual pending `425a90` CALL to the first `47cc10` native entry. This is
still a kernel proof with genuine original incoming state; production has not
produced that incoming raw metadata/object/owner graph yet. The ABI guard remains.

The original callback probe executes the unchanged original scripts and captures
94 complete observed pages, all eight general registers, 63 opcode checkpoints,
and every declared allocator/counter leaf. There are no VM/native-result hooks
or retained-word exclusions. The composed comparison passes. The separate
callback-to-Thread.run comparison covers 62 observed pages; the separate
Thread.run-to-native-entry comparison covers 86. Production modules accept only
live addresses and pending CALLs; only tests read the ignored captures.

The reached source arms include qualified static fields and instance method
receivers, empty-parameter signature/method selection and cache insertion,
integer/reference equality, integer arithmetic, logical operations, array
lvalue access, local declaration/cleanup, script return/frame disposal, and
native registry/argument preparation. Unsupported source branches continue to
return explicit errors. This does not establish all script opcodes, live boot
ownership, callback completion, or the 331-frame replay.

`native_vm_abi::Entry` now retains the produced native CALL without unwinding its
parent dispatcher/interpreter. Its fields are private and execution requires
that its pending stack frame remain current. Existing complete dispatcher tests
pass after this refactor. A negative control changed the native return address
from `420080` to `420081`; the composed interpreter comparison failed on the
retained byte at `7feff7ec`. Restoring the source return address restored the pass.

The clock-entry comparisons cover 48 controlled combinations of caller fills,
seeds, budgets, x87 control words and preexisting stack depths. An independent
native/Rosetta x87 sequence agrees with the port on arithmetic precision flags;
Unicorn omits arithmetic PE in the reached clock path. Deadline bits agree.
That disagreement is explicit: this is not an assertion of full Unicorn x87
status parity. Complete heap/stack/GPR comparisons remain unmasked.

```sh
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot genuine_stock_osd_callback
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot genuine_stock_osd_complete_interpreter
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot original_run_entry_clock
SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot native_vm_dispatch::tests::
```

`native_vm_callback_genuine_probe.py` generates the upstream comparison with
`--evidence-root /path/to/preserved/evidence`; add `--native-entry` and a distinct
`--output` to generate the composed callback/interpreter/native-entry evidence.
The preserved evidence checkout is read only.

## Complete callback and source owner construction

The composed callback proof now runs all 128 original opcodes through
`Input.lastKey`, `System.currentTime`, `Thread.run` return and callback setup
return at `425aae`. It compares 107 complete observed pages and all eight GPRs.
The wrapper's final argument cleanup/RET and the live world caller remain
separate boundaries. The incoming snapshot is a test input, not a production
context producer. The ABI guard remains unchanged.

The raw owner path now includes complete `403e80` initialization of stock
`java.io.Input`: all 156 original scripted static fields, its controller-reference
array and float array. Two caller seeds compare the complete declared heap,
16 KiB of retained stack, eight GPRs, x87 control/status/tags and all physical
x87 registers. Every allocator/free and numeric QPF/QPC leaf is compared too.
The probe rejects CPU stores outside the declared observation, and uses full
original JVM construction and the original bootstrap prefix through `40ff66`
to establish valid primitive descriptors and array-slot globals. The stock runs
require no emulator state correction.

Additional original comparisons cover recursive empty class initialization,
static owner construction, initializer-thread/frame destruction, normal and
array class descriptors, fresh and occupied class-cache buckets, fresh and
cached array metadata, and reference/primitive array instances. Runtime kernels
consume source pointers and actual pending CALLs. Only test code reads captures.
The loader's causal metadata projection also binds `FileRead`, `ModuleRead` and
`ClassRegister` identities using the exact serialized bytes and module order;
eight stock profiles retain complete original heap parity.

A synthetic large I-to-F initializer exposes a Unicorn 2.1.4 defect: `FSTP`
omits precision exception PE, which later changes `FNSTSW AX`. The independent
native/Rosetta `class_init_fpu_referee.c` executes the same `FILD`/`FSTP`
conversion and agrees with the port. The qualified probe applies only that
measured FPSW exception/C1 transition at `417e35`; it never patches GPRs, memory,
opcodes or script/native results. `--unqualified-unicorn` preserves the raw
emulator observation separately. Assertions pin this qualification, including
the exact inexact input and exception bit. This qualification is not used by
the stock `Input` or callback comparisons.

The machine stream records dependency resolution before recursive class
initialization (`ClassDependencies`) and installed owners even when empty
(`ClassInstall`). Native array copies now emit typed payload stores, and GC
array clearing records the owned prior elements and length store rather than
an unresolved mutable-access marker.

Current verification: 53 raw VM tests pass with
`SLR_REQUIRE_ORACLE_FIXTURES=1 cargo test -p slr-boot --bin slr-boot native_vm_`.
Production still needs its causal raw object/owner bindings, callback wiring,
clock/input leaves and live FPU history. These kernel results do not establish
the live producer or the 331-frame replay, and do not authorize removing the
guard. No PR has been opened for this unfinished continuation.

## Ordinary owners, body identities, and full callback FP checks (2026-10-25)

The source407da0 constructor now handles ordinary instances as well as arrays.
Both404e60 (explicit length) and404e20 (ordinary allocation) retain their own
actual CALL sites. Instance field initializer frames run original TREE nodes
through420ff0, perform the source assignment, and remain owned until the single
instance-initializer Thread destructor. The Run environment now shares its
allocator/dispatcher collaborator directly; the nested allocator adapter was
removed so recursive source initializers do not create an infinitely nested
adapter type.

Four additional strict profiles cover empty instances, fields, initialized
fields, and the404e20 entry: each has both seeds and all six base/interface
combinations. All declared memory bytes, eight GPRs, heap cursor, and endpoint
FPU environment/registers are compared. The retained initializer profile uses
the already documented independent I-to-F PE qualification. The whole raw VM
suite now passes58 tests with `SLR_REQUIRE_ORACLE_FIXTURES=1` (41.71 seconds).

ClassRegister now carries the exact issued BodyId vector in serialized TREE
order. The causal metadata bridge binds each to the original loader's issued
body allocation and validates its count, opcode/operand words and present
immediates. It does not compare or fill unwritten immediate words. Duplicate,
empty and identical script bodies retain distinct owner identities; PC lookup
rejects a changed TREE owner or a node outside the body. Original loader
comparisons validate these bindings against source memory. No body address is
inferred from semantic identity or matching script bytes.

The stronger composed callback tests now also check the full retained x87
state at every opcode and endpoint. This exposed the known Unicorn arithmetic
PE omission at5516c0. `callback_clock_fpu_referee.c` independently executes the
six clock x87 instructions via Rosetta after restoring the entire incoming
x87 environment and physical registers. Each original clock observation must
match its live result, full tags and status except for a measured missing PE
bit. Only that independently measured flag is propagated in qualified local
captures; GPRs, memory, script/native results and retained physical FP bits are
not patched or masked. Raw unqualified captures and the failing comparison log
are preserved locally. Rosetta normalizes reserved FPCW bit6 and canonicalizes
some empty retained slots; these are explicitly outside this flag-only
qualification. This is not a claim that all Rosetta empty-register bits equal
Unicorn's representation. All128 callback script opcodes now pass the stronger
qualified FPU comparison. The guard remains unchanged.

The live diagnostic replay still stops at record647 (Input.lastKey), with the
first646 unfiltered records matching. Its latest durable machine capture has
465,984 contiguous events, 23,485 CellStores, zero CellWriteAccess markers and
zero UnresolvedMutation markers. The remaining CellAbandon is on fatal unwind.
The missing afterimages were arithmetic/boolean owned-cell mutation and the
initializer driver's source conversion; data-free tests cover each and the
initializer conversion was observed red before its fix. ClassRegister body
identities are included in that capture.

Trace now also offers a reader attached before owner creation. Independent
recorder/producer acknowledgements retain events until every consumer has
processed them. A late subscriber, stale acknowledgement or invented prefix is
rejected. Dropping a reader does not discard its unprocessed evidence. The
22 machine-stream tests pass, as do the VM and x87 regression suites. This
transport is still a prerequisite: the live source object/thread graph,
callback wiring and retained live FPU history are not finished. No callback
admission, PR or merge has been claimed.

## Numeric clock transport and constructor continuations (2026-10-25)

Clock replay now publishes each successfully consumed numeric counter input to
its scoped machine trace, including ordinal, frequency, counter, baseline,
control word, site, thread and driver position. Rejected reads remain poisoned
and publish no input. `native_vm_counter` requires the already established raw
baseline and source control word, then validates the actual QPF/QPC CALL order,
return PCs and argument owners. It cannot seed missing history from a packet.
The source clock's 48 profiles use this provider; three authored negative and
exact-arithmetic tests cover its ownership contract. The latest live replay
bundle, `20261025T091743.823854Z-638c697db1a2`, records 472,207 contiguous machine
events and all 6,223 consumed clock inputs in order. It still stops at the ABI
guard at record647, preserving the matching unfiltered646-record prefix.

The metadata bridge also pins each selected method ordinal to the actual
serialized static/instance MTHD record and owner. Runtime signature caches may
change without changing that declaration. The raw method lookup now executes
cached hits, including equal hash keys with different names or signatures;
six original profiles check complete heap, stack and GPR states.

Source Run now implements ordinary `new` and constructor invocation rather
than treating allocation as a completed constructor. Controlled initializer
profiles execute opcode21 through original408a30/4207c0, signature creation,
method selection, first cache-block allocation, frame dispatch, receiver
cleanup and opcode2a void return. A second profile supplies an integer argument
and checks signature construction, absent-return type parsing, parameter
conversion and inline variable-pool growth. Both have12 original comparisons
(two retained-register seeds and six base/interface combinations). Each was
observed failing before its missing source continuation was implemented.

Additional initializer profiles cover float literals and the four RID cases
(named package, null package, wrong constant kind and invalid member index).
Resource-table resolution is an explicitly declared domain collaborator reached
through its actual4122b0 CALL; VM, script and native results are not hooked.
After the zero-argument constructor addition, all69 raw VM tests passed with
`SLR_REQUIRE_ORACLE_FIXTURES=1` in43.39 seconds. The integer-argument profile
subsequently passed independently. Probe source changes invalidate old captures;
regeneration and the whole suite remain required after the ongoing stock
Vector constructor work.

The stock Vector profile loads hash-verified, unchanged Object/Vector class
files and executes the original Vector constructor chain. Its failures have
already exposed current/base constructor opcode mapping, ordinary rejection of
an overload with a different parameter count, and variable-argument copies.
This work is ongoing. It is not evidence of live object/thread graph production,
a complete bootstrap caller, or retained live FPU history. The guard remains;
no feature push, PR or merge is authorized by these bounded comparisons alone.

## Shipped constructors and retained native handoff (2026-10-25)

The unchanged stock Vector constructor now passes12 strict profiles. The source
interpreter executes its nested current/base constructor calls, overload
selection, named field accesses, parameter copies, field assignments and frame
teardown. The destructor releases operand values in reverse order, then owned
locals in declaration order; variable operands are not freed twice. No Vector
TREE node, method body, or native result is replaced.

A second stock profile executes the unchanged ResourceRef constructor with id0,
including its real47cea0 newNative and47cf80 set bodies. It loads hash-verified
Object, String, Native and ResourceRef classes. A probe setup error initially
used field ordinal3 from the old synthetic resource vectors; this was corrected
to the actual40fe70 bootstrap ordinal0 before accepting this profile. The two
seeds compare complete heap, stack, globals, all GPRs, heap cursor and FPU at
both native entries and the endpoint, plus every declared allocator leaf.
Native/script results and VM method calls are never hooks. Source545fc0 is
currently admitted only when the requested id already equals the node's id;
changed resource binding still needs its source resolver/list continuation.

Initializer driving now retains the actual Run native entry, validates its
private capability, executes the environment's source native body, and resumes
the dispatcher/interpreter. The embedding's production ABI guard remains
unchanged. This does not supply a live production caller merely by registering
a native implementation address.

These comparisons also exposed and fixed canonical-name collision insertion,
the retained EDI after an exhausted canonical-name chain, reference signature
parsing, the primitive-to-reference overload rejection via actual String module
inheritance lookup, missing constant-string insertion, the descriptor scan's
retained EDI, uncached native signature parsing, scalar native arguments and
statement result disposal. Stock ResourceRef's real native entries validate the
composed histories. Other conversion and interface-distance arms stay guarded.

After regenerating every affected class-initializer capture, all72 raw VM tests
passed with required fixtures in47.42 seconds. `cargo clippy -p slr-boot --tests
-- -D warnings` also passes. Production all-target Clippy still reports the raw
modules as unused because the live producer is not integrated; no blanket
suppression was added. The preserved worktree is unchanged and no PR is ready.

### Cold Osd parent callback and bootstrap class resolution (2026-10-25)

The ResourceRef source native now also covers its first changed binding from an
unbound node. A declared resource-manager lookup returns the physical container;
the source545fc0 branch links its native node into that container's reference
list. Two original profiles compare the positive-id constructor, not only id0.
Replacement of an existing bound node remains guarded.

The unchanged shipped Osd constructor now passes two complete cold-instance
profiles (2,593 declared checkpoints each). Its actual parent426fd0 animation
pass is also composed with425a90/4255e0, Thread construction, source Thread.run,
and both callback returns. That comparison passes all2,896 checkpoints per
seed, complete declared memory, all GPRs, heap cursor and full retained x87
state. Loading Frontend's original metadata was necessary: Osd.animate resolves
its input queue even for a newly constructed inactive Osd. This cold profile
returns without Input.lastKey; the independent genuine replay comparisons
continue to cover that native entry and callback return.

The parent comparison exposed an actual426fd0 cleanup error: bind_reference
cleared the stack-local reference words, whereas42712f..427159 only unlinks them.
The source cleanup now preserves those residual bytes and its volatile-register
history. Existing control/animation comparisons still pass. The source callback
and prepare seams retain their pending CALL, allowing a source VM to enter them
with its own frames and FPU owner. Default typed bridge behavior is retained.

Other newly composed source paths are uncached qualified fields, recursive
static initialization during qualified calls, zero-argument reference-return
signature parsing, and the inline null literal. The corresponding original
scripts and source native bodies execute; no script/native result is hooked.

After refreshing every affected class-initializer capture, all75 raw VM tests
passed with required fixtures in41.26 seconds. Test-target Clippy with warnings
as errors passed. Complete byte comparisons now use page-span equality through
Space::first_difference; no bytes are omitted. The missing/unmapped/wrapped
address cases have a focused regression test.

The next bootstrap continuation now reaches41044c, immediately before4165a0
cache preparation, through the original410890/411610/410630/41d290 calls and
lazy41d690 filesystem inventory. Eight profiles (fill0/A5, two seeds and two
heap gaps) compare all825 allocator/filesystem checkpoints and all35,938 ordered
stores each. Its explicit continuation retains the bootstrap, lookup, namespace
resolution and file-loader frames; none are unwound or replaced with a captured
caller. The failed-find-next -1/-2 sentinel branch is source-derived and
requires the actual preceding sentinel slot to be empty. Java compilation and
unproved inventory branches remain explicit errors. The class-resolution test
passes in9.69 seconds after replacing per-byte HashMap reads with complete
page-span comparisons.

This is still not the complete live production producer. Cache preparation,
actual source file/loader caller history, issued allocation bindings for live
objects/fields/cells, and retained live FPU history remain required. Production's
nested ABI guard is unchanged. No commit, PR or merge is ready yet.

Cache preparation4165a0 now also passes eight strict profiles through its actual
return410451. The source normalizes and checks the .java/.class paths through
Win32 CreateFileA/CloseHandle, constructs its real file-cache and OS-file slots,
acquires/releases the original semaphore nesting, reads the twelve-byte TUFA
header, decodes its words, releases the header allocation, and returns with the
loader frame still active. Every835 allocator/filesystem checkpoint and every
ordered store is compared. The source file-cache mode byte's full BL write
operand is retained in the observer (FFFFFF00), in addition to its actual low
byte in memory; no observation is masked. The original source/Java compilation,
archive and pre-existing file-cache replacement branches remain guarded.

The cache-header comparison passes in10.39 seconds and test-target Clippy with
warnings as errors passes. A new original-only file-input probe now measures
through411b20 after the whole class read; its Rust continuation is the next
work item. Production ABI admission and live ownership remain unfinished.

The whole-file path54c300 now also passes eight profiles through the actual
411b20 entry. It opens a distinct OS file slot, obtains the original stored size,
allocates the full buffer, reads the real stock class bytes, closes the slot
through its semaphore, and forms411b20's actual pending CALL. FileInput's source
and length come from that source allocation/read, not a capture or semantic id.
The complete heap/stack/global/GPR checkpoints and ordered stores pass; this
comparison takes10.52 seconds. All77 preceding raw VM tests passed in11.85
seconds before adding this78th test. Test-target Clippy with warnings as errors
also passed after the whole-file implementation.

The source file loader now continues through411b20's module/container
construction, CONS407660, FILD419d00, MTHD419b60, TREE decoding and403a90 class
construction. Its namespace publication and complete loader/namespace/lookup
returns pass eight strict profiles, followed by the second eager GameRef load
and the actual40fe70 RET. Each boundary compares the complete declared mutable
memory, stack, GPRs, allocation cursor, environmental checkpoints and ordered
stores. The full bootstrap comparison passed in19.79 seconds. Constant kinds
0/3/4/5/7 and zero interfaces are covered; unsupported paths remain errors.

A composed native_vm_live::Runtime now takes an actual pending constructor
CALL and the caller's complete x87 state, executes40fb50 and then40fe70 through
both eager source class loads, and retains immutable receipts for the inputs
actually read. No VM metadata is prebuilt. This comparison passed eight profiles
in20.18 seconds. Its metadata consumer binds FileRead/ModuleRead/ClassRegister
chronology to those previously issued source allocations without rerunning a
loader. The extended test independently drains the recorder before consuming
its retained reader, rejects forged sequence numbers and input bytes without
memory writes or acknowledgement, and verifies every method and TREE node
binding. It passed all eight profiles in75.46 seconds.

The next comparison adds actual410890 CALLs after boot for Native, Osd, Input,
Thread and a repeated Osd cache hit. The initial original-only capture exceeded
the boot harness's0x80000-byte heap; the extended profile explicitly uses a
0x100000-byte heap and still compares every declared byte. This profile is not
yet verified. The source lookup entry now consumes any matching actual pending
CALL and retains its return PC, rather than requiring a bootstrap-only parent.

These proofs still do not admit the production callback. Live object, field,
cell and thread ownership, the production root caller, source native
registration and retained live FPU history must be composed before removing
the ABI guard. No PR, push or merge has been performed. Source checkpoint
re/out/native-abi/checkpoint-20261025T131240Z archives186 changed source files;
all oracle captures and stock/reference files remain outside Git.

The post-bootstrap class-lookup profile now passes all eight configurations.
It loads Native, Osd, Input and Thread through original410890 CALLs, then repeats
Osd as a cache hit. Native omits its empty MTHD section, so the source dispatch
loop now retains an already consumed CLSS header when that section is absent.
Thread's nonempty CLSS interface array now executes the original allocation,
read loop, stores and GPR history. The final cached lookup's cache-match store
was already executed; its missing observer is now included in the ordered-store
comparison. All92670 source stores match in the first profile; no store or byte
is excluded. The extended capture is1.9GB and the full86-test raw-VM suite passed
with SLR_REQUIRE_ORACLE_FIXTURES=1 in341.37 seconds.

A native_vm_callback::Context now owns the retained wrapper and Thread.run
continuations, pauses at a validated native capability, resumes through its
actual source body, and permits the wrapper epilogue only after setup returns.
The cold-parent original comparison uses this composed owner and passes all
checkpoints. It still inherits the actual caller's FPU state. This is a source
continuation API, not production ABI admission; source-issued live object and
semantic ownership binding are still required.

The imported raw tests now use the existing optional-local/required-strict
fixture policy for their entry captures. Missing local captures may be skipped
in fixture-free CI. Once present, unreadable/corrupt/stale captures and missing
supporting inputs remain failures; required mode never silently skips them.

The next pending comparison composes cold JVM construction/boot with76 actual
416b00 native-registration CALLs. It includes duplicate and colliding names,
registry pool growth, empty names and signed high bytes. The original-only
capture has all eight profiles; the source caller implementation and its strict
comparison are in progress. The heap-only registration proof remains distinct.

The full416b00 source caller now passes eight cold constructor/boot profiles
with76 registrations each (duplicates,70 same-hash names, growth, empty and
signed-byte names). It uses the original canonical-name and41dc30 signature
bodies, actual registry node/pointer pools, allocator CALLs and return PCs;
complete memory, GPRs and ordered stores pass in40.60 seconds.

The487f20 caller's256 declarations are also composed and pass eight profiles
in54.80 seconds. re/oracle/native_vm_registry_catalog.py verifies the build606
image hash and each original MOV/PUSH/CALL sequence, then transcribes only its
return PCs, four pointer arguments and ECX-load position. The port retains the
original interleaving. These declarations are registration data, never native
ABI admission or result answers. The actual487f20 frame remains entered at
489e20 before its two resource-binding suffix calls, which are still required
for the production root caller.

The runtime now separates its source Loader from its owned FPU state and
exposes class lookup as an explicit collaborator. A new composed profile boots
a fresh JVM, loads Native through410890, enters403e80 on the actual returned
class, autoloads Object through the initializer's nested source CALL, initializes
both classes and returns. No class, namespace, loader or initializer body is
hooked. Eight profiles pass complete memory/register/checkpoint comparisons
and all48706 stores in20.08 seconds. Initializer/thread/variable helper stores
now participate in the stronger composed observation, including the full CL
operand during name copying. This adds a89th proved raw-VM case to the86-test
suite and two registration cases described above; a complete89-test suite has
not yet been rerun after the latest refactor.

The next integration slice is source-issued class, thread and frame lifecycle
receipts bound to the lossless semantic event chronology. Runtime.apply still
accepts metadata events only; other owners remain explicit errors. The full
production root/resource suffix, live object/field/cell bindings, retained live
callback/FPU integration, guard handoff,331-frame strict replay and menu/garage
regressions remain incomplete. The ABI guard is unchanged. No commit/push/PR
or merge has occurred.

Class dependencies and static installation now emit source-owned observations.
Thread/frame construction, pool release and destruction do the same. The
consumer binds the real VmHost initialization stream through those issued
records, with checked generations for reused raw addresses. It rejects altered
class dependencies, origins, frame contexts and retirement flags without memory
writes, sequence advancement or acknowledgement. Two initializer frames really
reuse one raw pool address with distinct generations; both retire before that
address can be exposed as live context. The eight-profile autoload/binding
comparison passes in19.79 seconds. OwnerHistory exposes these historical
bindings as diagnostic records, never as executable ABI capabilities. The
initial frame's raw+38 word remains unwritten by407b10: semantic absence never
clears or supplies that source word.

A fresh-object profile now composes cold JVM construction/boot, Native lookup,
Object dependency autoload, both class initializations,404e20/407da0 object
construction, the actual instance field and its temporary initializer thread.
The first84000-slot variable block is allocated by the source pool body; the
profile explicitly declares a0x200000-byte heap. Its machine driver now saves
and restores EBX explicitly, matching the caller model's preserved-register
contract; the original execution was recaptured after that harness correction.
Every mutable byte, GPR, allocator checkpoint and ordered CPU store passes all
eight profiles in70.79 seconds.

The same profile now also binds the complete real VmHost object/lifecycle
stream with no unacknowledged events. Object and field addresses come from the
source allocation/append receipts, with their actual declaring FILD record and
ordinal. FILD records and inventories now receive the same immutable-owner
validation as MTHD records. Forged object contexts and heap payloads are
rejected; raw addresses are not accepted as semantic object identities; changed
descriptor/declaring-record data invalidates field queries. All three temporary
thread generations retire correctly while reusing the source frame pool. This
combined comparison passes in70.52 seconds.

This is the90th raw-VM case (the last complete suite ran86 tests).31 existing
class-initialization regressions passed after the loader separation, before the
latest owner-observation additions. Script-bearing frames, opcode/cell and clock
history, the complete production root/resource caller, live callback handoff and
whole-menu/garage comparisons still remain. The production ABI guard remains
unchanged; no commit, push, PR or merge is ready.

## Scripted initializer lifecycle and registry return continuation (2026-10-25)

The reproducible cold Input initializer failure was missing construction and
operand-append observations in the inline literal path. A fixture-free regression
covers integer, float and null literals; omitting only the constructor observation
makes it fail immediately, and restoring that source observation makes it pass.
The original all-eight-profile comparison now passes the complete static
initializer lifecycle in229.39 seconds. Every successful value allocation must
have a constructor receipt and be released; all scripted frames and temporary
threads retire. Pops, copies, source conversions, static stores, releases, saved
PCs and frame restoration bind to actual source observations. The following
ordinary Input object allocation remains an explicitly rejected boundary: this
profile proves class initialization, not construction of that later object.

Absent serialized immediates have no semantic value to compare with unwritten
TREE scratch. The raw word remains in the source receipt and original complete
memory comparisons; the semantic event must carry zero when Node::has_imm is
false. No original byte or store is masked. Array allocation groups only its
validated synthetic-class/empty-thread support lifetimes, retains those source
receipts, and checks the outer caller, frame generation, descriptor and length.
Negative tests reject unrelated, missing, reordered and corrupted lifetimes.

The source487f20 registry caller now continues through its actual546070 resource
bindings and RET. Eight profiles cover empty, replacement, same-id and missing
bindings with two seeds, including head/interior unlink and insertion before an
existing head. Complete declared heap,8KiB stack, mutable image pages, heap
cursor, eight GPRs, full physical x87 state and ordered stores pass. Package lookup
538440 and resource resolution536820 remain declared domain collaborators;
their internals and the upstream427980 world caller are outside this proof.

The missing standalone-model rational proof producer and pinned model were
recovered from existing Git history. The model is retained under
re/x87probe/model, outside the Cargo build; no candidate scalar implementation
was linked into production. The rational proof always runs in fixture-free CI;
local retained receipts follow the optional-local/required-strict policy.
Missing original comparison captures were regenerated locally and remain ignored.

Validation confirms79 strict non-bootstrap raw VM tests, the eight-profile
initializer comparison, eight registry-return profiles, formatting and boot
unit-test-target Clippy. The non-boot workspace passes1009 tests across113
targets with the full read-only modded reference selected through SLR_REFERENCE.
Production all-target Clippy still fails with411 errors from unintegrated raw
code. No blanket warning suppression or artificial production consumer was added.

The fresh331-frame strict replay bundle is
re/out/boot/runs/20261025T175222.145393Z-6a2ebe2c7852. It stops at the unchanged
Input.lastKey ABI guard:446 filtered and646 unfiltered preceding records match;
records447/647 diverge on fatal unwind. Menu input, menu presentation and New
Career/garage regressions stop at that same boundary. The production callback
producer, complete root/resource caller and live FPU/ownership handoff remain
unfinished. The guard stays intact. No push, PR, CI merge or production-admission
claim is justified by these component proofs.

The array-owner audit additionally exposed a transactional failure: a malformed
or duplicate second element could consume the first element's owner generation
before rejection. Both cases were observed red. ObjectCommitted now validates
all elements and the token range before committing the ledger; correcting the
input and retrying succeeds, with no raw memory writes. The two regressions pass.

The final workspace run (full read-only SLR_REFERENCE) finished with1352 passing
and4 failing tests, including320 passing boot unit tests. Its failures are
failed_run_has_correlated_state_and_durable_end_marker (expects the later
MainMenuBoundary), stock_menu_input, stock_menu_presentation and stock_new_career;
all are consequences of the unchanged nested callback guard. The final
post-transactional-fix initializer comparison passed all eight profiles in229.39
seconds, and the current strict non-bootstrap suite passes79 tests. The latest
strict replay is re/out/boot/runs/20261025T180013.838727Z-084abbc059f7, with the
same446/646 exact-prefix limits and guard rejection. No release gate was waived.

## Startup caller and Input object continuation (2026-10-25)

The worktree is now based on current main `edaabaf`, including the consolidated
acceptance tooling. The prior staged/unstaged state is preserved in the named
stash and `re/out/continuation-20261025/` (tracked/index patches and the untracked
source archive). No x87 investigation branch was merged. The existing diagnostic
model remains outside Cargo; no experimental scalar provider is linked.

`native_vm_root::Startup` now retains the actual427980 entry and its source
resource bindings, malloc,40fb50 constructor, JVM publication at world+e4,
40fe70 bootstrap and487f20 return. The world singleton is636338, so this actual
publication writes63641c without a synthetic metadata publication in the host.
The next two source callers46b530 and4427e0 register34 and63 additional natives,
through their actual CALL/PUSH/MOV interleaving. The combined source startup
registers all353 entries and stops at427a0c, before the world/resource suffix.
Registration remains data construction, never permission to execute a native.

Three comparisons pass all eight fill/seed/heap-gap profiles each. They compare
the full declared heap,12KiB retained stack region, seven mutable image
regions, eight GPRs, heap cursor and ordered CPU stores at every declared leaf
and endpoint. Full saved FPU state is additionally compared at the endpoint;
the reached startup instructions contain no floating-point arithmetic. Half the profiles declare missing packages;
half supply two existing raw resource owners through explicit manager leaves.
No VM, method-selection, script or native implementation is hooked. The353-entry
profile explicitly declares a1MiB heap. Its earlier512KiB attempt failed at the
allocator bound and is retained; no heap bytes were excluded to make it pass.

A negative control changed the actual constructor CALL's return from4279ec to
4279ed. The comparison failed on the retained byte at7feffec8, at logger3. The
source was restored before the passing comparisons. These tests prove the named
startup prefix, not startup's remaining resource/world caller or production boot.

The new Input object profile also passes all eight original comparisons after
cold boot and all156 static initializers. It then executes404e20/407da0 and binds
the complete real VmHost chronology with no pending events. Input has zero
instance fields; Native's separate one-field profile remains distinct. Both
raw addresses masquerading as semantic object identities and changed lifetime,
metadata, descriptor or value observations remain rejected. This removes the
previously explicit unissued Input-allocation boundary from that component proof.

Thread.run now uses the previously isolated423ba0 operand implementation,
including source realloc/growth. The narrower duplicate implementation and its
unproved growth error were removed. The64 controlled profiles now also include
null operands, through direct and nested CALLs, all complete heap/stack/GPR
comparisons, without normalization. Their tests and the three composed genuine
Osd callback/Thread.run/Input.lastKey comparisons pass after integration.

The Runtime callback API retains its source callback, loader, owner ledger and
FPU state across native pauses, using private runtime/generation handles with
nested-owner checks. This is still work in progress: no production embedding
installs or admits this API yet. In particular, it is not a proof of semantic
NativeEnter/argument-owner binding or a substitute for the missing source caller.

The next production work is concrete:

- Finish startup427a0c through its resource/world/Config callers, preserving the
  actual stack and allocation order; do not replace this with a retained driver.
- Connect source-issued script objects to envelope+50 and provide the inverse
  live semantic-owner lookup. Existing typed envelope words must not become raw
  object pointers merely because their payload happens to match.
- Extend receipt binding through ordinary method frames and local-variable pool
  generations, then native entry/return and argument transfer. Field initializer
  frame proofs alone do not establish callback frame/local ownership.
- Install the same owned Runtime at the live callback/Thread.run boundary, retain
  nested native/FPU continuations, and acknowledge the lossless producer stream
  independently from optional durable recording.
- Only admit a native through its actually produced and validated Entry, then
  pass the complete strict331-frame comparisons and menu/New Career/garage runs.

The fresh331-frame bundle is
`re/out/boot/runs/20261026T013903.801414Z-64bdcd2698b3` (UTC identifier; local run
was2026-10-25). It still rejects Input.lastKey with exactly446 filtered and646
unfiltered preceding records matching; records447/647 differ on fatal unwind.
The ABI guard is byte-for-byte unchanged from the preserved worktree. The
production all-target Clippy check still fails on unintegrated raw code; no
blanket suppression or artificial production consumer was added. No PR, push or
merge is justified by the component proofs above.

Validation follow-up: the full strict workspace attempt completed its boot unit
suite with312 passes and14 missing-local-fixture failures. Those fixtures were
recovered read-only from main (copy/hash receipts are retained under the
continuation evidence root); the strict non-bootstrap rerun passes303 tests,
including all14 affected cases. The23 bootstrap cases passed in the full run.
The same recovery resolved six missing-fixture non-boot failures. No expected
oracle value, source tolerance or fixture availability policy was changed.

Across the workspace targets and affected-target reruns,1,356 tests pass and
four integration tests remain failing: diagnostic failure-marker expectations,
menu input, menu presentation and New Career/garage. All four reach the same
unchanged nested Input.lastKey guard. Formatting passes. Production workspace
Clippy still reports422 errors from unintegrated code, including the unfinished
callback ownership path. This is not a clean workspace verification result and
not readiness for PR/merge. Logs are retained in
`re/out/continuation-20261025/checks/`.
