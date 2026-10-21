# Street Legal reconstruction: canonical state and recovery

Goal: the complete stock single-player reconstruction passes all P01–P30 and
the cross-cutting gates in [fidelity.md](fidelity.md#whole-game-completion-contract).
Boot to MainMenu is a required intermediate milestone.
This file is the handoff. Update it at every milestone; do not keep a second copy.

Last updated: 2026-10-21, branch `exact-port`, HEAD `35703ca`.

**Read the first four sections in order.** They are the current state of
work: the boot endpoint measurement, the record-620 frontier, the event
dispatch gap, and the two-level gate that explains it. The cfg-handler parity
section after them is closed work and can be skimmed. Everything under
"Active ownership" and below is older context; the "Frontier" list there is
explicitly marked superseded.

## Boot endpoint: 300 idle frames reached at a 600-frame budget (2026-10-21)

    ./target/debug/slr-boot stock --config-instance-id 0xffff0080 \
        --game-init-instance-id 0xffff007f --output /tmp/boot --run-id boot \
        --trace-level metadata --event-ring 4096 --frames 600

prints `BOOT MainMenu dialog complete at frame 179; idle frames=420`, so the
idle-frame condition is *reachable* at this budget. That run has no
`--clock-replay`, though, so it is not contract evidence: the replay run

    python3 re/boot-diagnose.py re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog \
        --frames 600 --replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl

still stops at the clock-replay thread-identity check
(`expected THRD-RUNVMI Dummy.getInfo, got THRD-INSTVAR-INI`) before reaching the
endpoint. Instrumenting `match_thread` pinned it to replay position 7375
(expected `java.lang.GameType.getInfo` on `THRD-RUNVMI Dummy.getInfo`, actual
`java.game.parts.Part.<instance field init>` on `THRD-INSTVAR-INI`). Both are
`Thread::run` entries that read `ClockSite::ThreadEnter`; the difference is
ordering — the port reaches a `Part` field-initializer thread before the
`getInfo` callback thread that the original reaches first. Aligning it means
matching the original's creation order inside `instantiate`/`register_created`,
not changing the clock layer.

The replay rows at that position give the exact original sequence, which is the
specification the port has to meet (`clock-inputs.jsonl`, positions 7373-7381):

    ThreadEnter THRD-CREATE  java.game.cars.Prime_DLH_500 <init>
    ThreadExit  THRD-CREATE
    ThreadEnter THRD-RUNVMI Dummy.getInfo  java.lang.GameType getInfo
    ThreadExit  THRD-RUNVMI Dummy.getInfo
    ThreadEnter THRD-INSTVAR-INI java.game.parts.Part <instance field init>
    ThreadExit  THRD-INSTVAR-INI
    ...

So the original runs the `Dummy.getInfo` callback thread between the car
constructor's exit and the first `Part` field initializer; the port creates the
`Part` containers first. Note the callback's resource is `Dummy`, not the car
being constructed, so this is about when `register_created` is reached, not
about which container it queries. A probe shows `register_created` *is* called
for these containers (`native = None` for most, `Some("player")` for one), so
the row is not being skipped wholesale. `instantiate` itself only *allocates*
the container and runs `run_created`; it never creates `Part` children. So the
port's `Part` field-initializer threads are produced by the script constructor
itself, i.e. by the `Part.addPart` path, before `register_created` is reached.
That makes this the same root cause as the `addPart` item in the 2026-10-20
list: until `Part.addPart` creates its part at the original point (bytecode
node 135 -> `addStockParts`/`setTexture`), the port will keep reaching a `Part`
initializer before the `getInfo` callback thread and the replay will keep
stopping here.

Counts over the whole accepted run make the size of that gap concrete
(original vs port, `script-natives.slrlog` vs the 600-frame port log):

| native | original | port |
|---|---|---|
| `setTexture` | 112 | 1 |
| `partOnSlot` | 119 | 0 |
| `getSlotID` | 76 | 0 |
| `disableSlot` | 20 | 0 |
| `getScriptInstance` | 58 | 0 |

So the port constructs `Part` objects but never reaches the slot/texture
assembly that the original performs after them. These counts are the observable
acceptance signal for the `addPart` fix; they should be reproduced exactly in
both logs.

The earlier note that the dialog completes at frame 31 with 300 idle frames
by 331 is stale: the current tree reports frame 179, so `--frames 331`
yields 151 idle frames. Both halves of the boot endpoint therefore remain open:
the replay mismatch blocks the run, and filtered native-log equality fails at
record 620 regardless.

## Gate B inventory: 245 of 353 natives implemented (2026-10-21)

Measured, not inferred. `SLR_NATIVE_AUDIT=1` runs `audit_natives` after every
install site has run and classifies each of the 353 registered rows by the
function pointer it actually dispatches to:

    NATIVE_AUDIT real=245 capture_only=113 unbound=0

| class family | capture-only rows |
|---|---|
| `java.util.resource` | 61 |
| `java.game.parts` | 44 |
| `java.game.Vehicle` | 3 |
| `java.game.Painter` | 3 |
| `java.game.Navigator` | 1 |
| `java.game.GameLogic` | 1 |
| **total** | **113** |

Gate B ("all 353 registered natives implemented and registered by original
address") reads 245/353. No row is unbound, so nothing fails to resolve; the
gap is behaviour, not wiring.

**A count of `missing_rows::MISSING` is not this number.** That table is the
input to `capture_only`, but `parts.rs`, `part_render.rs`, `wheel.rs` and
`resource_identity.rs` deliberately re-register a subset of those rows with real
implementations afterwards, overwriting both the record and `native_impls`.
Reading the table alone yields 170 and understates the port by 57 rows.
`re/out/boot/native-installed.tsv` and `native-missing.tsv` date from
2026-10-19, predate those installers, and should not be cited. `re/build_native_table.py`
still rebuilds `native_api.tsv` (the 353-row registry) from the binary.

## Two parallel native implementations; only one is wired (2026-10-21)

The port carries two independent models of the same natives, and the stock boot
can only execute one of them.

| | `slr-lang/src/langobj/*` | `slr-boot/src/{factory,node}.rs` |
|---|---|---|
| fidelity | exe-faithful branch/loop structure, calls `0x53a2a0`, `0x546070`, `0x404ea0`, `0x42ab50` by address | port-native reimplementation over `VmHost` objects |
| backend | `langobj::engine::Engine` trait | `VmHost` + `slr-resource::node` |
| installed in production | **no** — `langobj::engine::install` is called only from `slr-lang/tests/engine_oracle.rs` | yes |

`main.rs:621` runs `langobj::gametype::install(&mut table)` and `main.rs:622`
`langobj::install(vm, &table)`, which registers those bodies by original address.
`main.rs:633` then **overwrites** `0x00481a70` with
`factory::create_native_instance`, because the `langobj` body would call
`with_engine(...)`, which `expect`s an installed backend and would panic.

So the faithful `GameType.createNativeInstance` (`gametype.rs:148`, which passes
the receiver as the script identity to `0x53a2a0`) is dead code on the boot path,
and the `factory` version runs instead. That is the mechanical origin of the
record-620 divergence: `factory::create_native_instance` decodes the arguments
as `create` does — `[receiver, owner, template, parameters, name]` — but the
fifth slot of the real signature is a request id, so `ref_container` reads an
integer as a GameRef.

Closing this needs one of two things, and the choice should be recorded before
either is built:

1. Implement `langobj::engine::Engine` over the production `VmHost`/resource
   world so `slr-lang`'s faithful native bodies can run, then delete the
   `factory` overrides. This is the route that keeps ADR 0004's
   function-level-port requirement satisfied by construction.
2. Port the remaining `langobj` bodies into the `factory`/`node` idiom and stop
   registering the `langobj` table at all.

Option 1 is cheaper per native and does not duplicate behaviour, but it commits
the boot path to the `Engine` ABI for every `GameType`/`System`/`Thread` call.
Option 2 keeps one ABI but requires rewriting every `langobj` function that is
not yet expressed in `factory` form.

## Record 620 is a script branch, not a missing native (2026-10-21)

Re-measured on `33ccb95` with the fixed clock
(`--clock-replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl`):
filtered 620 with 4170 port records, unfiltered 909 with 6278.

The original's calls at ordinals 614-620:

    614 ResourceRef newNative
    615 GameRef     create              -> 559164976
    616 Part        setWear             (1.0)
    617 Part        setTear             (1.0)
    618 ResourceRef id
    619 GameRef     getScriptInstance  -> 559164976
    620 GameRef     queueEvent

The port at the same ordinals:

    614 GameType    createNativeInstance
    615 GameRef     setFlags
    616 GameRef     clearFlags
    617 GameType    setEventMask
    620 ResourceRef duplicate, inside `java.render.RectangleTemplate.<init>`

`GameRef.getScriptInstance` is **registered and installed**: it is absent from
`NATIVE_STUB` and `0x486f30` carries a body. The port is not failing to answer
it — it never reaches the script node that calls it, because the `Dummy.<init>`
path emits a `createNativeInstance` the original does not, and the following
calls diverge from there. That makes record 620 an object-construction ordering
defect first and an event-path symptom second.

Two edits were tried here and reverted, each measured:

- `node::install` re-run after `CREATE`/`CREATE_NATIVE` were bound. It uses
  `insert`, so it replaced `factory::game_ref_create` with the generic bridge and
  the run collapsed to 444 filtered records. The existing single install site is
  correct and must stay where it is.
- `createNativeInstance` rewritten to reuse its receiver as the script identity
  and to resolve the receiver class for the `resource == 0` template. It builds
  and starts, reaches MainMenu at 151 idle frames, but under the fixed clock the
  replay collapses to 444 and aborts at record 135 on `System.currentTime`
  (`0x41400000` against `0x41f00000`). The `Dummy` path needs the constructor
  call the original performs, not a shortcut around it.

## Ordinal 446: the refresher's first iteration is missing a clock read (2026-10-21)

Aligning the two logs by filtered ordinal removes the noise and leaves a
one-position offset:

| ordinal | original | port |
|---|---|---|
| 443 | `GameType.addTimer` | `GameType.addTimer` |
| 444 | `Object.wait` | `Object.wait` (`LoadingScreen;<1010>`) |
| 445 | `Thread.sleep` | `Thread.sleep` (`Hotkey watcher`) |
| 446 | `System.currentTime` | `Object.wait` (`Game time refresher`) |
| 447 | `Object.wait` | `System.currentTime` (`Osd;.animate`) |
| 448 | `System.currentTime` | `Thread.sleep` (`LoadingScreen;<1010>`) |

**Superseded.** The one-slot offset below was explained as a sleep/`currentTime`
inversion; the measurement in "Ruled out: the `currentTime` surplus is not a
script or VM defect" shows the port's `currentTime` calls are correct execution
of `idleFor(10.0)`. The offset is real but its stated cause is not.

Every call is present on both sides; the refresher thread's `wait` arrives one
slot early in the port, pushing the rest down. The port's own event stream shows
why: after `thread_run`, the refresher emits `ThreadEnter`, then one
`ThreadPoll` at `java.game.GameLogic.run()V` line 563 node 2, then `wait` at line
565 node 8 — no native call in between. The original performs a
`System.currentTime` in that window, and the record is filtered-true, so it comes
from script, not from VM bookkeeping.

`Thread.sleep` is confirmed correct: `0x47c650` forwards to `0x41f630`, which is
`deadline = 0x5516c0() + duration; flags |= 8` and does **not** move the thread
between queues. `Object.wait` is confirmed correct: `0x47c920` forwards to
`0x408b20`, whose thread half `0x41f650` unlinks from the run queue, sets `0x10`
and appends to the notify queue — matching `Monitor::wait` in `monitor.rs`. The
defect is therefore in which script nodes `GameLogic.run` executes between the
poll and the wait, not in either native.

### The port polls where the original sleeps

Counting thread primitives across each full run makes the shape of the
divergence unambiguous:

| native | original (6508 calls) | port (8208 calls) |
|---|---|---|
| `Thread.sleep` | 313 | 112 |
| `System.currentTime` | 16 | 338 |
| `Input.lastKey` | 0 | 336 |
| `Object.wait` | 7 | 4 |

**The port column is stale.** The current tree measures 336 `sleep` / 1014
`currentTime` / 12 `wait` on a 331-frame run
(`re/out/boot/runs/20261021T120239.910924Z-eac1ff00040f`), so the `sleep` side has
already converged and any conclusion drawn from the 112/338 snapshot no longer
holds. Re-derive the original column before using this table again.

The original's ordinal 447-531 run is an almost unbroken stretch of
`Thread.sleep` calls, which is what made this look like a sleep/`currentTime`
inversion. **That reading is superseded** — see "Ruled out: the `currentTime`
surplus is not a script or VM defect" below. The port's `currentTime` calls are
the correct execution of `idleFor(10.0)`, whose comparison cannot turn true
before ten simulated seconds.

`Input.lastKey` remains the open lead: **the original never calls it**, and the
port calls it ~993 times.

The guard is now decoded exactly. `Osd.animate` (body 76) nodes 275-283 compute

    275-280: System.resolveName inputQueue.lastElement()   (line 553)
    281-283: 0x1007/0x07  == local0                          (line 553)
    284:     0x1015 imm=191                               (line 650)

and `0x1015` jumps (to node 475) on a **zero** payload. So `lastElement() == local0`
being true skips the whole 285-474 region, `lastKey` included, and being false
falls into it.

Two runs disagree here, and that is the finding worth chasing:

- the 331-frame run recorded `java.render.Osd.animate()V` node **294** nine
  times on each of several threads (`lastKey` at line 557);
- a fresh 60-frame step trace shows node 284 receiving `payload=0x1` on all 60
  entries and never reaching node 294 (`grep -c "idx=294 op=101a"` = 0).

So `local0` (the comparison's right-hand side) takes different values across
runs, and the declaration graph explains how:

- node 27 (`0x1002 imm=1`, line 517) declares exactly **one** local, so `animate`
  has local 0 and nothing else;
- node 26 (`0x1015 imm=239`) jumps to 265, which steps over nodes 27-264 — the
  only region that would assign it (`0x002c`/`0x1006` at 28-29 open the scope,
  `getPos()` at 31-37 feeds it);
- nodes 257-264 then free locals 8 down to 1 at line 547, and nodes 275-283 read
  local 0 at line 553 — after that free.

`declare` (`host.rs:1952`) initialises every local to `payload: 0` with the
declared tag, so a read-before-write local is a stable zero. A 60-frame step
trace confirms the shape on both guards:

    idx=26  COND payload=0x0  (60 of 60)  -> jumps to 265, skipping 27-264
    idx=284 COND payload=0x1  (60 of 60)  -> falls through to 285

so local 0 stays 0, `inputQueue.lastElement() == 0` evaluates true, node 284
does **not** jump, and the 285-474 region — `lastKey` at node 294 included — is
entered. The trace shows node 285 entered 72 times, 6 of them from node 284;
`lastKey` therefore really does run, on both engines' script.

That leaves the sharpest open question in this whole area, stated plainly: the
port calls `Input.lastKey` ~993 times and the original's capture records
**zero**. The script path is the same, so either the original's `inputQueue` is
non-empty in a way the port's is not at node 280 (making the `==` false and
skipping the region), or the original's capture predates this path. Settling it
needs `inputQueue.lastElement()` compared between the two, which is a
one-line probe away on the port side and a capture on the original side.

### Current call sites for `currentTime`

| site | calls |
|---|---|
| `java.render.Osd.idleFor(F)I` line 259 | 987 |
| `Osd.show(I)V` line 793 | 12 |
| `Osd.activateText(...)` line 315 | 6 |
| `Osd.idleFor(F)I` line 267 | 6 |
| `Vehicle.<init>` line 68 | 3 |

(331-frame run, `20261021T120239.910924Z-eac1ff00040f`.) The `idleFor` line-259
share is the `idleFor(10.0)` branch described in the ruled-out section, not a
scheduling artefact.

Resolving that one site to `sleep` removes the `currentTime` flood and restores
the `sleep`-driven frame loop; it accounts for 329 of the 338 port `currentTime`
calls and is the single highest-value change available.

### Ruled out: the `Osd.iLevel`/`IL_TIPS` guard

Kept so the next agent does not repeat it. `idleFor` (body 69) node 3 calls
`Osd.isFocused()I`; the guard was suspected of being the divergence. It is not.

`Loader::field_reference_of` names the two references in `isFocused` (body 127)
exactly, and a temporary binop trace showed the comparison evaluating correctly:

    BINOP sub=0x9 second=Int/3 first=Int/1
    LE l=Int/3 r=Int/1 greater=true result=false

`iLevel <= IL_TIPS` is false, `0x0005` (`&&`) skips node 4, and the port executes
nodes 5-6. Re-reading `0x0005`: it skips the **next** node on the short-circuiting
value, and node 4 is the unconditional jump to 11, so taking 5-6 is correct.
Both engines reach `lastElement()` here.

`iLevel` is also not stuck. `Osd` declares `int iLevel = IL_MENU`, and its field
initialiser is body 43:

    body 43:  0x101b imm=498 (IL_MENU)  0x0010
    body 39:  IL_NONE = 0     body 40: IL_TIPS = 1
    body 41:  IL_KEYS = 2     body 42: IL_MENU = 3

so `iLevel == 3` on an `Osd` is the stock initial value, not corruption. Three of
the four `Osd`s read 0 because `alloc_object_id` runs a field initialiser only
when the layout slot carries one; those instances are not the animating one.

### Ruled out: the `currentTime` surplus is not a script or VM defect

This section records the full investigation so it is not repeated **and** so
nobody acts on it. `Osd.animate()V` (body 76) node 299 jumps to 316, then nodes
300-304 call `idleFor(10.0)` on every frame the `controlMode` flag is set:

    299: 0x1014 imm=8        <- jump over the first idleFor site (nodes 19-23)
    300: 0x1007 imm=28       <- literal marker
    301: 0x100b imm=10       <- the integer 10, converted to f32 1.0e1 by store_with
    302: 0x1027 imm=1
    303: 0x0024              <- call
    304: 0x101a idleFor

`idleFor(F)I` then evaluates `(lastUserActivityTimeStamp + 10.0f) < currentTime()`
at its line-259 exit. A temporary `Alu::lt` trace confirms the shape and the
values:

    LT l=0x41204dba(10.018976) r=0x3d12c5f9(0.035833333)
    LT l=0x41204dba(10.018976) r=0x3d57b204(0.052660003)
    LT l=0x41204dba(10.018976) r=0x3d8e63ff(0.069526665)

The left operand is `lastUserActivityTimeStamp` (0.018976 s, measured directly on
`0x1486`) plus the 10.0 argument; the right operand is the clock, advancing one
frame step per entry. The branch can therefore only turn true once the clock
passes ~10 s, which a 331-frame run at `1000/60` ms per frame never reaches.

**Both engines run the same script and the same operands, so this is not a port
defect.** `controlMode` stays set, `animate` enters `idleFor` on every frame, and
three `Osd` instances make it fire exactly three times per frame — measured as
`{per-frame: 3}` on all 331 frames.

That means the original's recorded `currentTime` total of 16 cannot be compared
against the port's 993 without re-deriving it: the same table's `sleep` column is
already stale (112 recorded, 336 measured on the current tree). Until the
original column is re-measured on the current tree, **do not treat the
`currentTime` count as evidence of a divergence**, and do not "fix" `controlMode`
or `idleFor` to reduce it.

What *is* worth checking, and is independent of this: whether
`configure_clock(step)` is the right model. It adds `step` on **every read**
(`sched::clock_ms`), so a boot phase that reads the clock heavily inflates
absolute time — the port makes 230 reads before frame 0. A wall clock does not
behave that way, and that difference is real regardless of the `idleFor` question.

### Why `lastKey` runs: a null check on `Osd.RID_ARROWRG`

331 of the 336 `lastKey` calls come from one site: `java.render.Osd.animate()V`
source line 557, body 76, node 291. The guard is three nodes earlier:

    281 op=0x1007 imm=27   line=553   load this
    282 op=0x1001 imm=0    line=553   push null
    283 op=0x1007 imm=7    line=553   load this.<field 7>
    284 op=0x1015 imm=191  line=650   if (payload == 0) goto node 191

Field 7 of `Osd` names `RID_ARROWRG` (own-field record 7 is string index 46, a
`Ljava.util.resource.ResourceRef;` slot). So line 557's block is
`if (this.RID_ARROWRG != null) { ... Input.lastKey() ... }`, and the port takes it
while the original does not. `SLR_TRACE_STEPS=1` confirms the port executes
nodes 285-296 and loops back to 281 without ever taking the `goto 191` edge.

That makes the remaining question concrete and testable: why does the port's
`Osd` hold a non-null `RID_ARROWRG` at animate time when the original's does not.
It is a resource-population difference in whichever setup assigns that slot —
not a scheduler, script-body or native issue.

`Osd.<init>(FFI)` alone does not assign it. Its whole executed trace is nodes
0,2,3,5,7,8,9,11,12,14,15,16,17,18,19,20 — node 15-16 is `push 7; CallInit`
(an object array) and node 20 is `0x002a` (return). The slot is filled later, by
`Osd.<init>(Viewport)` or by the `setup`/`createButton` path, so the next probe is
a write-watch on that field across the whole `Osd` lifetime rather than a read of
one constructor.



### The loop body, decoded

`GameLogic.run` is body 87 (43 nodes). `SLR_TRACE_STEPS=1` on the port shows only
four of them executed before the thread parks:

    idx=0 op=1007 line=563   load this
    idx=2 op=1015 line=575   conditional jump to node 0x27 (39)
    idx=3 op=1027 line=565   push int 0
    idx=4 op=0024 line=565   CallResolved -> Object.wait()

Node 1 (`0x100b`) never appears, and `idx=2` carries `imm=0x27` with the *loop's*
line number, so node 2 is the loop guard and nodes 39-40
(`0x1008`, `0x1014 imm=-0x28`) are its back edge. There is **no `currentTime`
node anywhere in this body** — the port executes the whole body correctly and
still parks in `wait` before any `currentTime` appears.

That rules out a missing script call and points at queue order instead: the
port's `thread_run` sequence is LoadingScreen (seq 113204), Hotkey watcher
(113217), Game time refresher (113241), `Osd;.animate` (113258). `Thread.start`
appends at the queue tail (`ThreadList::append`), so the refresher really does
sit ahead of `Osd;.animate`, and the port's first `currentTime` therefore lands
after the refresher's `wait`. The original has them the other way round: its
`currentTime` comes before the refresher's `wait`. The observable defect is the
order in which `GameLogic` starts those two threads, not the body of `run` and
not either native.

### Reading a stock script body

`slr_fmt_tufa::parse` plus `Module::trees` decodes every `TREE` body of a stock
class, and `module.strings` holds the name/type pool. A throwaway
`cargo run -p slr-boot --example ...` that prints `node.opword`, `operand` and
`imm` for one body answers "what does this script actually do" without a new
tool: `0x1027` pushes `imm` as an int, `0x0023` is `CallSuperInit`, `0x0024` is
`CallResolved`, `0x4025`/`0x4026` are instance/static calls, `0x0010` stops the
body. `Module::methods` returns raw `MTHD` records whose fields are not yet
decoded, so mapping a global `BodyId` back to a class-local body index still
needs `Host::body_label` or the loader's `body_id_of`. The classes have 100
bodies each; `GameLogic`'s two largest are body 71 (594 nodes, a constructor —
it opens with `CallSuperInit`) and body 96 (608 nodes).

## Two frontiers, and the reason they differ (2026-10-21)

The strict probe has two distinct stopping points and they must not be
conflated.

**Without `--clock-replay`** the run stops inside
`java.render.Osd.createButton(Ljava.render.osd.Style;FFILjava.lang.String;I)`
at source line 1050, node 122, after `PhysicsRef.createBox` (call 114) returns
successfully (`native_return` shows `error: null`). The failure is
`Unsupported(0x24)`, i.e. `HostError::NoBody`: a method resolved but has no body
in `native_impls`. The failing native is named in `Host::missing_native`
(`host.rs:2657`) but that field is not part of `failure.json`, so the report
shows only the enclosing script frame. The preceding run of the same path is
deterministic enough to reach MainMenu with 151 idle frames.

**With `--clock-replay`** the run instead stops at filtered 620 / unfiltered 909
with 4170 / 6278 records, and `capture-end.json` reports a replay-context
mismatch rather than a missing native:

    expected ThreadContext { name: "THRD-RUNVMI Dummy.getInfo",
                             class: "java.lang.GameType", method: "getInfo" }
    got      ThreadContext { name: "THRD-INSTVAR-INI",
                             class: "java.game.parts.Part",
                             method: "<instance field init>" }

So the clock-replay frontier is a **divergent script schedule** — the port
enters a `Part` instance-field initializer where the original is still inside
`Dummy.getInfo` — not a native gap. Fixing the record-620 call stream will not
fix the `createButton` `NoBody`, and vice versa.

Also fixed here: `ResHost::call` returned `None` for a bound row with no
modelled arm **without recording the call**, so a missing body appeared later
as an unexplained `Unsupported(0x24)` at an unrelated line. It now records
every bound row. Measured effect: none — filtered 620 / 4170 and unfiltered
909 / 6278 are unchanged, so the record stream is not perturbed; the change only
makes the gap visible.

**All three modes, measured on the same tree:**

| invocation | outcome |
|---|---|
| `slr-boot ... --frames 331`, no replay | reaches MainMenu; stops on `MainMenuBoundary` with 151 idle frames against the required 300 |
| `slr-boot ... --frames 600`, no replay | reaches MainMenu with 420 idle frames and clears the boundary |
| `boot-diagnose.py --replay clock-inputs.jsonl` | stops at 620 / 4170, replay-context mismatch as quoted above |

So the boot endpoint is not blocked by a missing native. The `Unsupported(0x24)`
in `createButton` appears only on the diagnostic harness path, and the honest
statement is that the no-replay run reaches the menu while the deterministic
replay does not yet reproduce the original's schedule.

### Corrected counts: the events are generated, the masks land on the wrong objects

Earlier text in this section used 1041 `queueEvent` calls. **That was a counting
error**: `events.jsonl` emits several record kinds per native call, and only the
`native_enter` rows are calls. Counting only `native_enter` on a full 700-frame
boot (`20261021T...`, run `final2`) gives:

| native | port | original |
|---|---|---|
| `queueEvent` | **68** | 69 |
| `setEventMask` | 27 | — |
| `clearEventMask` | 19 | — |
| `lastKey` | 705 | 0 |
| `currentTime` | 707 | 16 |
| `sleep` | 235 | 313 |

So event **generation** already matches the original almost exactly (68 against
69). Nothing is being swallowed at the call site. The loss is entirely on the
delivery side: `dispatch` is entered 134 times on that run, delivers twice, and
`DROP_MASK` fires **131** times while `DROP_TYPE` fires **zero**. The type gate
never rejects anything.

The mismatches are systematic, not incidental:

    SETMASK receiver=0x1016 container=0x614400 mask=0x10010
    SETMASK receiver=0x101d container=0x614700 mask=0x110000
    SETMASK receiver=0x1486 container=0x84f100 mask=0x110000

    DROP_MASK container=0x616200 mask=0x10 instance_mask=0x0
    DROP_MASK container=0x855000 mask=0x10 instance_mask=0x0
    DROP_MASK container=0x868a00 mask=0x2  instance_mask=0x0

The masks are registered on one set of containers and the events target a
**disjoint** set. Both sides resolve through `container_of`, and both are
self-consistent, so the defect is in *which* container each side names —
exactly the `FUN_0042ab50(this, DAT_0062e008)` indirection in `FUN_0048d3d0` that
the port collapses to `container_of(receiver)`.

### The mask bookkeeping is correct; the commands arrive after the subscriber exits

Container IDs settle the "wrong object" hypothesis — they match exactly:

    SETMASK   receiver=0x1016 container=0x614400 id=Some(4294901884) mask=0x10010
    DISPATCH  container=0x614400           id=Some(4294901884) mask=0x2 instance_mask=0x0

Same container, same id. The mask really was registered and really was `0` at
delivery time, because `Dialog.hide()V` line 184 clears it:

    CLEARMASK container=0x614400 mask=0xfffffff      <- Dialog.hide()V:184

The full lifecycle of `Osd` `0x1016`, by event sequence number:

| seq | call |
|---|---|
| 10138 | `setEventMask` from `osd.dialog.Dialog.show()V:177`, mask `0x10010` |
| 110310 | `clearEventMask` from `osd.dialog.Dialog.hide()V:184`, mask `0xfffffff` |
| 154164 | first `queueEvent(0x10)` from `GameRef.command()V:83` |

So the subscriber is gone 44 000 native calls before the first event it would
have wanted arrives. Dropping them is correct behaviour, not a mask bug.

The real defect is upstream, and it is a scheduling collapse rather than an
event bug. Every `queueEvent` in the whole run lands on **one frame**:

    queueEvent by frame: [(179, 64), (None, 4)]

and the `SplashScreen.handleEvent` thread has a total lifetime *inside* frame
179: its first record is sequence 146669 (`thread_run`, line 0, node 0) and its
last is 397705, with not one record on any other frame. `SplashScreen.exit()` runs
at 146691, inside that same window, so the thread starts, exits the screen, and
then keeps draining to 397705.

Before the first `GameRef.command` there is a burst of
`Part.addPart(ILjava.lang.String;Ljava.game.parts.Descriptor;)` on the same
thread, also on frame 179. So the whole resource-load / message sequence that the
original spreads across the boot runs is collapsed into the frame the main menu
completes, and by the time the commands are issued the dialogs that subscribed to
them have already hidden.

That is the defect to fix: **the SplashScreen handler thread is not being run
until frame 179.** Everything downstream — the 131 mask drops, the 2-versus-69
delivery count — follows from that.

Thread 596 is that handler, and its whole life is 31 522 records from sequence
146669 (`thread_run`, node 0) to 397705 (`thread_return`), every one of them on
frame 179. It runs `SplashScreen.exit()` at 146691 and then continues for another
251 000 sequence numbers. A single scheduler tick therefore executes the entire
remaining boot.

**The collapse is not a scheduler or clock effect.** Running the same boot with
`--clock-step` at 0.01, 0.5 and 5.0 - a 500-fold change in how fast the simulated
clock advances per read - leaves `queueEvent` at exactly `{None: 4, 179: 64}` in
all three runs. The scheduler's time slice is therefore not what parks the
handler.

The event stream shows what does. Frames 0-178 each carry 29-130 records; frame
179 carries **47 436**. The transition at sequence 146660-146669 is:

    146661  thread_return   Osd.animate()V               (frame 178)
    146662  render-frame-notify  monitor 0x100b, woken []
    146665  frame-enter      frame 179
    146666  native-player-frame  container 0x00615e00 phase=control
    146667  resource-retain  node 5245792 container 8711936
    146668  store            Object{word:5292, slot:0} = 5245792
    146669  thread_run       SplashScreen.handleEvent  thread 596

So on the first frame after the animating `Osd` retires, the player's `control`
phase retains a resource node and stores it into object `0x14AC`, and only *then*
is the `SplashScreen.handleEvent` thread created and run to completion. The
ordering is inverted relative to a live splash screen: the handler is created
after the screen's own exit path has already been reached, which is why it can
run `SplashScreen.exit()` at 146691 and still keep draining to 397705.

Next step: find why `SplashScreen.handleEvent` is created at that point rather
than when the screen's events are first queued - i.e. what in the
`native-player-frame` control phase triggers it, and whether the original creates
it during `enter()` instead.

### Part construction is incomplete: missing `setTexture`, `getSlots`, `getSlotID`

Comparing the two call logs by `class.name` exposes a concrete, non-timing gap.
The filtered oracle bundle `20261021T120133.016761Z-62ab81f49e10/inputs/` holds the
oracle log; the port's is `runtime/runtime.calllog` of the failing run.

| native | oracle | port |
|---|---|---|
| `java.game.parts.Part.setTexture` | 112 | **1** |
| `java.game.parts.Part.getSlots` | 1 | **0** |
| `java.game.parts.Part.getSlotID` | 76 | **0** |
| `java.game.parts.Part.setWear` | 116 | 58 |
| `java.game.parts.Part.setTear` | 116 | 58 |
| `java.util.resource.ResourceRef.destroy` | 19 | **135** |

The single surviving `setTexture` is the one `Chassis.addStockParts` line 582
issues for the chassis itself; every per-part application is gone. The 76
`getSlotID` calls pair with the single `getSlots()` that returned 76, i.e. one
full slot-table walk, and that walk never happens in the port.

**Where it is lost.** `Part.addPart` body line 719 carries a conditional on the
variable declared at line 686 (slot 5) and the `getParentID()` result:

    126  0x0024   CallResolved, resolves through 127-129 to getParentID
    130  0x1007/0x1b  silent no-op
    131  0x1001   load local 5
    132  0x1007/0x06  `!=`
    133  0x002d   store into slot 9
    134  0x1001   load local 9
    135  0x1015 imm=22  jump when false -> 157 (line 725, destroy)

A jump at 135 steps over 136-156, which contain both `addStockParts` (141, line
717) and `setTexture` (154, line 719). Confirmed with `SLR_TRACE_STEPS=1`: every
one of the 68 `addPart` calls either takes the early `getCar()` return at node 12
or jumps at 135; **none reaches node 141 or 154**.

The operands are equal, so `!=` is 0 and the jump fires. Instrumented for one
call:

    LOADLOCAL imm=5 len=10 slot=Var(VarId{index:43,g4}) value=Int 4294901877
    ASSIGN value=Int 0 -> local VarId{index:44,g4}
    LOADLOCAL imm=9 ... value=Int 0
    COND value Int 0x0  -> jump to 157

So slot 5 already holds `4294901877` (0xFFFF0075, the same value the immediately
preceding `getParentID` returned) at the point of comparison. Slot 5 is written
at line 691 by `0x1008/0x23`, and line 691 is where the oracle gets
`getParentID = -65419`. In other words the comparison is "did the parent id
change since line 691", and the original answers yes while the port answers no:
the resource created between lines 691 and 714 is not re-parented in the port.

That is the defect to chase next - `GameRef.create` / resource re-parenting, not
the VM's `!=` (verified correct: `0.0f != -65419` returns 1 in a unit probe) and
not `0x1015` (payload-zero jumps, matching `docs/tufa-executor.md`).

Corpus check backing `declare`: all 2081 `0x1002` nodes in `stock/` are followed
by a `0x1006` type node, so reading the type from the follower is right; the
`0x1002` immediate is the local slot index.

**The re-parenting gap is now measured directly.** `getParentID` result
distributions over the whole boot:

    oracle:  58 x -65426  (0xFFFF006E)   and  58 x -65419  (0xFFFF0075)
    port:   116 x -65419

and in the oracle the two strictly alternate by predecessor call:

    after ResourceRef.set(-65426)      -> getParentID = -65419
    after GameRef.queueEvent(..., 16)  -> getParentID = -65426

The port answers -65419 in both positions, so the node's container chain never
reaches the freshly created resource. `queueEvent` arguments and counts match
(69 vs 68), and `create` returns the same shape of id (oracle #291 -> 546735632,
port #291 -> 4389), so creation itself is not the divergence - the parent link
is.

Candidate sites, in the order to check them:

* `ResourceHost::create` (`node.rs:1494-1503`) relinks the node with
  `insert_head(container, node)` against `created_result`. If the original only
  rewrites the wrapper id (`nodes[node].id = containers[container].id`) and
  leaves `nodes[node].container` on the old container, this relink is the
  divergence - and it would explain why the port can never see -65426.
* `Container::parent` set at `factory.rs:159`/`346` is `root.container`, i.e.
  `owner_container` or `world.game`. `parent_id` (`node.rs:1802`) then returns
  `containers[containers[node.container].parent].id`, so a container whose parent
  is itself (or the new container) is what produces -65426.

Resolve it against the oracle wrapper vectors (`re/oracle/part_probe.py`,
`crates/slr-resource/tests/wrapper_oracle.rs`) before changing either site: the
distinguishing observation is a `getParentID` immediately after `queueEvent` on a
`create`d wrapper.

Existing oracle coverage does not settle it. `re/out/res/resnode.jsonl` has three
`getParentID` rows:

    parent_id_no_field  node.container=0  eax=0
    parent_id_unbound   node.container=0  eax=0
    parent_id_bound     node.container=3  eax=null  error UnmappedAccess

The only bound case faults reading `[[R+0xc]+0x14]+0x50`, which is the ADR 0007
original-bug divergence recorded in `node.rs:49`. So no oracle row observes a
*live* parent id, and the port's `-65419`-everywhere cannot be falsified against
the current vectors: **a new probe is needed**. The minimal one is a node whose
container word points at a container whose `+0x14` parent points at a second
container with a known `+0x50` id, which would give `getParentID` a definite
value for three shapes (parent == self, parent == sibling, parent == world).
`re/oracle/resnode_vectors.py` already lays out `R = {prev,next,id,container}`
and `X` with `+0x14 parent`, `+0x50 id`, so the row is a small addition, not new
tooling.

**The row now exists and settles the semantics.** `parent_id_with_parent` gives
the node a container whose `+0x14` points at a second container with `+0x50 =
0x2222`, and the original returns:

    eax = 8738 = 0x2222

So `getParentID` is `[[R+0xc]+0x14]`'s `+0x50`, i.e. the *parent container's own
id* - exactly what `node.rs:1802` implements (`containers[containers[node.container].parent].id`).
`parent_id_bound` keeps faulting `UnmappedAccess` with a null `+0x14`, which is
the ADR 0007 trigger, so it stays an intentional divergence and must not be
"fixed" to 0.

That removes `parent_id` itself from suspicion. The port returning `-65419`
everywhere means the container created by `GameRef.create` is given
`parent = -65419`'s container, while the original has `parent` pointing at the
new resource itself after `queueEvent` (hence `-65426`). The question is
therefore which container is written into `Container::parent` for a created
instance - `factory.rs:159` (`allocate_configured_instance`) or `factory.rs:346`
(`create_instance`) - and whether the original rewrites it after creation.

Two dead ends are now closed, so they need not be re-checked:

* `owner == 0` is *not* the difference. `game_ref_create_inner` maps it to
  `root = world.game`, and `factory.rs:80-99` binds `DEFAULT_RESOURCE_WORD`
  (`0x636460`, the exe's fallback owner) to a node over that *same* `game`
  container (id 4). So the port already uses the value the original would.
* `ResourceRef.set`/`queueEvent` argument shapes, `create` result ids and the
  three `Container::parent = root.container` sites (`factory.rs:159`, `:346`,
  `:216`) all match between the two runs.

What remains is that every creation path takes `root` from `owner_container` or
`world.game` *before* `instantiate` runs, so `parent` can never be the new
container. The original's `-65426` after `queueEvent` is the new resource's own
id, which is what a container whose `parent` points at itself would yield. Worth
checking next: whether the original's `53a5e0` resolves the owner *after*
allocating the instance identity, in which case a create whose owner argument is
the not-yet-existing id produces the self-parent.

**That hypothesis is refuted.** `re/oracle/game_ref_create_arguments_probe.py`
hooks `53a5e0`/`53a2a0` and runs the real `47d7b0`/`47d900` marshaller. Its
assertion `calls == [expected]` passes with `expected[0]` already resolved to
`0x636460` when the owner cell is null, and `handle+12 == 0` afterwards. So the
original passes the owner *into* `53a5e0` already decided - the port's ordering
(`root` fixed before `instantiate`) matches, and the self-parent cannot arise
from late owner resolution.

So the `-65426` must come from what `53a5e0` writes into the new container's
`+0x14`, not from how the owner is chosen. That is exactly the boundary the
existing probes stub (`boot_bridge_probe.py` declares "resource resolution" and
`boot_native_link_probe.py` supplies "instance storage"), so this is the next
thing to lift: make `53a5e0` write a real parent word and read the new
container's `+0x14`/`+0x50` back. Until that is measured, do not guess between
"parent = owner" and "parent = new container".

**Measured, and it rules out the `create` relink.** `boot_bridge_probe.py
real_link` now reads the created container back:

    created_container_is_node_container: false
    created_container_parent: 0
    created_container_id: 123
    owner_container: 536872368

A *bound* node keeps its own container; only the wrapper id changes. So
`0x47d7b0` does not move a bound node, and a variant of the port's
`insert_head(container, node)` restricted to bound nodes was tried and reverted:
it changed neither the `getParentID` distribution (still 116 x 4294901877) nor
the `setTexture`/`getSlots`/`getSlotID` counts.

The reason it could not have helped is measured too: every one of the 69
`create` calls in a boot run has `node_container=0x0`, i.e. an **unbound** node,
for which the port's unlink/insert path is the one the probe does not cover and
the original does bind the node to the new container. So the `create` site is
consistent with the evidence available and is not the divergence.

What is left: the parent id `-65419` reaches the created wrappers from the
container they end up in. Since they are unbound and join the newly created
container, the question is that container's `+0x14`. The port sets it to
`root.container` in `factory.rs:159`/`:346`; the original's own container has
`+0x14 = 0` in the probe. That difference - new container parent 0 versus
owner - is now the single unexplained link, and it is exactly what decides
`getParentID` for these wrappers.

**The probe now reads that container directly, and it does not match the
production path.** With `real_link` the probe reports:

    node_container:            536870976
      +0x14 parent:            0
      +0x50 id:                123        (= the new resource id)
    owner_container:           536872368
    created_container_in_list: false       (53a5e0's own 0x423c00 container differs)

So the original's created container has `+0x14 = 0` and its own id, while in the
boot run `getParentID` at `Part.addPart:714` returns `-65426`, the *new resource's
id*. `parent_id` over a `+0x14 = 0` container yields `containers[0].id`, which is 0
unless container 0 exists - so the two observations are only consistent if the
production path's container has a non-zero `+0x14`, i.e. the probe's unbound
scenario (`data+0x44 = 0`, node container 0) is *not* what the boot hits.

The controlling call sequence in the oracle log is unambiguous about the shape:

    #615 GameRef.create(...)          -> new resource
    #618 ResourceRef.id()             -> -65426   (the new id)
    #620 GameRef.queueEvent(0,16,...)
    #621 ResourceRef.getParentID()    -> -65426   (equals the new id)
    #624 Part.setTexture(196678)      <- first of the 112
    #630 ResourceRef.getParentID()    -> -65419   (the next wrapper)

`#621` returning the *new* id while `#630` returns the owner's is what makes
`!=` true at line 714 and lets `setTexture` run; the port answering `-65419` at
both points is what makes `!=` false and skips it.

Next: build the probe case that matches production - a *bound* node whose
`create` result is a fresh container - and read that container's `+0x14`. Keep
`parent_id` as it is until that row exists; the current evidence cannot decide
between "parent = the new container" and "parent = a container whose id is the
new resource", and they are different words.

### The internal `0x10000` dispatch is still missing, but it is not the count gap

The mask gate is not the problem. A `SLR_DISPATCH_TRACE` run over 60 frames
records every `dispatch` entry and the mask that was registered for the target:

    DISPATCHTRY container=0x615e00 type1=true mask=0x10 instance_mask=0x10 script=0x107b   <- delivered
    DISPATCHTRY container=0x614200 type1=true mask=0x2  instance_mask=0x0  script=0x1006   <- dropped
    DISPATCHTRY container=0x614400 type1=true mask=0x2  instance_mask=0x0  script=0x1016   <- dropped
    DISPATCHTRY container=0x614700 type1=true mask=0x2  instance_mask=0x0  script=0x101d   <- dropped
    DISPATCHTRY container=0x616200 type1=true mask=0x10 instance_mask=0x0  script=0x0      <- dropped

Every entry passes `type_id == 1`. What drops them is that `queueEvent` never
carries the mask the subscribers registered:

    SETMASK receiver=0x1016 container=0x614400 mask=0x10010 script=0x1016
    SETMASK receiver=0x101d container=0x614700 mask=0x110000 script=0x101d
    SETMASK receiver=0x1486 container=0x84f100 mask=0x110000 script=0x1486
    SETMASK receiver=0x1484 container=0x84ef00 mask=0x80     script=0x1484

`0x10010 & 0x2 == 0`, so the gate is closed — correctly. **`0x10000` is a real
event type, not a stray high bit.** The exe sends it from native code:

    FUN_00426800(param_1 + 0x13c, 0, 0x10000, local_238, 0);   // pseudocode 49375
    FUN_00426800(param_1,       0, 0x10000, local_238, 0);   // pseudocode 49376

and `FUN_00426800` is a bare wrapper over `FUN_004265c0` — the function the port
calls `dispatch`. The site is the camera/viewport update: it compares the stored
camera state against `PTR_DAT_00610a54` and, when they differ, formats a string
and dispatches `0x10000` twice, to `param_1 + 0x13c` and to `param_1`.

So the gap is not in `dispatch`, not in the mask bookkeeping, and not in
`queueEvent`: **the port never performs the engine-internal `0x10000` dispatch.**
`engine::dispatch` is only ever reached from `queue_event`.

The emitter is `FUN_00460140(int, float)` — a per-frame camera update that takes
the frame delta as its second argument and writes the camera transform into
`param_1 + 0x164..0x170` before comparing it against the stored state in
`PTR_DAT_00610a54`.

Scanning the image for `E8` relative calls finds exactly two, at `0x461c30` and
`0x461c4e`, and both lie **inside the range the decompiler attributed to
`FUN_00460140`**. Searching the whole file for the absolute value `0x00460140`
finds no hits at all, so the address is never stored as a function pointer in
the image either. `0x461c40` looks like a separate body (`8b 44 24 08` prologue,
stdcall style) nested inside that same decompiler range, and it has no direct
caller of its own.

The honest reading: this region is a vtable- or jump-table-driven update
pipeline, the decompiler merged several functions into one block, and none of it
is ported — `crates/slr-render-natives` has no camera model at all.

`0x461c30` is a two-instruction thunk (`call 0x460140; ret 8`). `0x461c40` is the
real wrapper, and hand-decoding its bytes gives the shape of the missing work:

    461c40  mov  eax,[esp+8]        ; the float delta
    461c44  sub  esp,0x12
    461c47  push esi                ; esi = the object (`this`)
    461c4c  push eax
    461c4d  push esi
    461c4e  call 0x460140           ; camera update -> dispatches 0x10000 twice
    461c53  mov  eax,[esi+0x7c]
    461c58  jz   0x461cf8           ; nothing to draw
    461c5e  mov  eax,[0x6495a4]
    461c65  jnz  0x461c9d
    461c67  mov  eax,[esi+0x8c]
    461c6f  jnz  0x461c9d
    461c98  call 0x48c4b0           ; formats from this+0x84 / +0xac / +0x2c
    461c9d  mov  eax,[0x63c83c]
    461ca4  jz   0x461d16
    461ca6  mov  eax,[esi+0x9c]
    461cae  jnz  0x461d16

So the wrapper is a viewport/camera per-frame update: it drives the camera
transform first (the step that emits the two `0x10000` events), then gates a
render-flag pass on `this+0x7c`, `this+0x8c` and `this+0x9c`, using the globals at
`0x6495a4` and `0x63c83c`.

`esi` is a **native container**, not a script object: `java.render.Viewport`
declares no instance fields at all, so those offsets live in the resource
record. `FUN_00460140` reads `*(byte *)(param_1 + 0x50) & 1` before doing any
work, and `+0x50` is the container `id` word in the port's own
`slr_resource::node::Container`, with `flags` at `+0x54`, `load_state` at `+0x64`
and `detail` at `+0x6c`. The camera state it reads lives in the global
`PTR_DAT_00610a54` (`+0xc` position, `+0x48` scale, `+0x6c`/`+0x74` clip planes),
and the transform it builds is written into `param_1 + 0x164..0x170`.

Implementing `0x460140` plus this wrapper is the bounded task. The port already
has the container layout it needs; what is missing is the camera-state global and
the transform step.

Adding that dispatch pair is the fix, and it is what unblocks the
`Osd`/`GameRef` `addpart` chain described above.

## Event dispatch: one delivery against the original's 69 (2026-10-21)

Instrumenting `engine::dispatch` on the stock boot shows the port delivers
**one** script event in 331 frames (`DISPATCH container=0x84ef00 type=0x80
method=handleEvent arg=Int(1)`), while the original's log has 69 `queueEvent`
rows and 58 `getScriptInstance` rows. `dispatch` is entered many times and the
type gate passes (`type_id = Some(1)`), but it stops at
`instance_mask & mask != 0`.

The masks the port holds come from `GameType.setEventMask` and land on the
*subscriber* containers the script passes as `this`:

    SET_MASK 0x614800 0x10000      SET_MASK 0x614400 0x10010
    SET_MASK 0x615200 0x10000      SET_MASK 0x844e00 0x10000
    SET_MASK 0x614700 0x110000     SET_MASK 0x628100 0x100080

but the delivery targets are `0x615e00` (mask `0x10`, ok) and `0x616200`
(mask `0x0`, blocked), so only the first ever reaches `call_script`. The
original's `0x426653 test [esi+0x70], ecx` reads the mask off the *event
target* it resolved through `0x42ab50`, which is evidently a container the port
does not put a mask on.

This is the same missing-link family as the `addPart` gap: without the delivery,
`GameRef.command`'s `addpart` message never reaches `handleEvent`, `isComplex`
stays 0, and `addPart`'s node 135 keeps branching to `destroy`. The counts in
the table above are the end-to-end signal.

Two candidate fixes were tried and reverted, each measured:

- resolving the target from the `ResourceRef` argument slot
  (`0x47da57`) instead of the receiver was behaviourally neutral — still 4170
  filtered records, first divergence at 620.
- letting a null argument fall back to `DEFAULT_RESOURCE_WORD` (`0x636460`,
  the bound game root) makes the port dispatch on that container and then
  abort: filtered records collapse from 4170 to 443 and the first divergence
  moves to record 156 with a `queueEvent` where the original has `File.open`.
  So the game root is not the container the original's `0x42ab50` returns for
  this call, and dispatching on it runs a script path the original does not
  take at that point. The abort is the clock-replay check firing on a different
  thread than expected: `expected THRD-RUNVMI Controller.setcontrol, got
  THRD-RUNVMI Ljava.io.MouseCursor;.getInfo`, i.e. the fallback delivers an
  event the original never delivers, which shifts the thread sequence by the
  time the replay reaches that request.

A further probe settles the input side: `marshal_args` reports the real
`queueEvent` words as `[this, 0, 16, <string>, 0, 0]` — the `ResourceRef`
argument slot is a genuine zero, not a masked pointer, and
`container_of(0)` is `None`. So the port never had a non-null argument to
resolve. `0x42ab5d` in `0x42ab50` dereferences `[edi+0x10]`, which means the
word it receives is an object.

A direct measurement on the headless original settles the input side. Hooking
`0x481a15` (after `0x42ab50` in `setEventMask`) and the `0x4265f5`/`0x426601`
window in one 200M-instruction run (`winmain_reached = true`) gives:

    setEventMask targets: 0x203030a0 0x204e6100 0x202f9e40 0x202ce300
                          0x20a9bb80 0x205c86b0 0x20b065a0 0x20b05a40 0x20b02870
    [esp+0x10] at 0x4265f5: 0x200045bc 0x20004874 0x20004874 0x20004874

`ecx` at `0x4265f5` is the four-word scratch record's own stack address, but
the word the very next instruction loads from `[esp+0x10]` is a **heap**
pointer (`0x2000_...`), i.e. the container that `0x4290bf` stored into
`[record+0xc]`. An earlier note in this file read the stale `ecx` at
`0x4265f5` and concluded the gate depends on stack residue; that was wrong and
is withdrawn. The gate really does test a container's `+0x4c`, as the
disassembly says.

Widening the hook to `0x481a15..0x426601` gives the container at the gate:
`ecx` is `0x200045bc` (once) and `0x20004874` (three times) — heap containers
again — and none of them appears among the nine `setEventMask` targets. So the
original's event targets and its mask subscribers are genuinely different
objects; the mask check therefore cannot be satisfied by the containers the
port stores masks on, which is what its `instance-mask = 0` readings show.

Nine mask targets against four gate hits in a 200M-instruction window also means
most `queueEvent` calls never reach the gate within that budget, so these counts
are not yet comparable to the 58/69 of the full accepted run.

Reading the mask test itself settles the delivery side. At `0x426653` on the
same run, `esi` is `0x20004694` (once) and `0x2000494c` (three times) — heap
containers — `ecx` is `0x10`, and `[esi+0x70]` is `0x10`. The test therefore
passes and the original delivers. Those containers are again disjoint from the
nine mask targets.

Tracing the store clarifies why: hooking `0x48d3d0` shows its `ecx` is always
`0x20066450` — one manager object — while the pushed word is the mask value
(`0x203030a0`, `0x204e6100`, `0x202f9e40`, `0x202ce300`, `0x20a9bb80`,
`0x205c86b0`, `0x20b065a0`, `0x20b05a40`, `0x20b02870`, plus two from other
sites). So `0x48d3d0` registers a mask *with a manager*, not onto the container
the port's `state.masks` models. `[X+0x70]` is written by some other path, and
the port's per-container map is not that path.

Reading `0x48d3d0` correctly settles the store. Its body is
`mov eax,[esp+4]; mov ecx,[eax+0xc]; ... call [vtable+0xc]; or [eax+0x70], mask`,
so it takes the *container* as its first stack argument, resolves the live
instance through `vtable+0xc`, and ORs the mask into that instance's `+0x70`.
Hooking it with `[esp+4]`/`[esp+8]` gives the container and mask pairs, and
resolving `[container+0xc]` gives the instance each call actually stamps:

    0x203030a0 / 0x10000  -> 0x200041a8     0x205662c0 / 0x10     -> 0x200045bc
    0x204e6100 / 0x10000  -> 0x20004460     0x2058fb60 / 0x10     -> 0x20004874
    0x202f9e40 / 0x110000 -> 0x2000404c     0x20a9bb80 / 0x10000  -> 0x200063a4
    0x202ce300 / 0x10010  -> 0x20003ef0     0x205c86b0 / 0x100080 -> 0x200049d0
    0x20b065a0 / 0x10000  -> 0x20006e84     0x20b05a40 / 0x110000 -> 0x20006d28
    0x20b02870 / 0x80     -> 0x20006bcc

Two of those instances, `0x200045bc` and `0x20004874`, are exactly the `ecx`
values read at `0x426601`. But the delivery test at `0x426653` reads `esi` =
`0x20004694` / `0x2000494c`, which is *another* object produced by the
`vtable+0xc` call at `0x426645`. So the original resolves two levels —
container → gate instance → delivery instance — and the port has one.

That two-level resolution, not the mask bookkeeping, is the remaining gap: the
port must model the intermediate object `0x426601` tests (whose `+0x4c` carries
the type) and the second `vtable+0xc` result that carries the mask at `+0x70`.
The measured table above is the acceptance input for that work; `0x203030a0`
etc. are the containers the script actually passes to `setEventMask`.

The writer of `[X+0x70]` is now known: `FUN_004819f0` (`GameType.setEventMask`)
unpacks its arguments with `0x45d910`, resolves `0x42ab50(field id,
this)` to a container at `0x481a10`, and calls `0x48d3d0(target, mask, 0)` at
`0x481a1d`. So the mask is stored on the container that `0x42ab50` derives from
`setEventMask`'s own receiver, and `0x4265c0` later tests that same word.

The port models both steps (`set_event_mask` -> `state.masks`,
`dispatch` -> `instance_mask`), so the discrepancy must be in which object the
script passes as `setEventMask`'s receiver versus which object owns the event.
The next probe should log, on the original side, the container `0x42ab50`
returns for each `setEventMask` (hook `0x481a15` and read `eax`) and the container
`0x4265f5` returns for the following `queueEvent`, then diff that pairing
against the port's. The oracle already records `receiver` and `caller` per
clock request, so a hook script is enough — no new instrumentation is needed in
the shipped code.

**Corrected 2026-10-21 (supersedes the argument-shape claim below).** A probe of
`createNativeInstance` on the stock boot shows the port already receives the
right shape: `marshal_args` yields `[receiver, GameRef, I, String, String]`,
which lines up with `create`'s `[receiver, owner, requested, parameters, name]`
slot for slot (`w=[4219, 4101, 50, 4222, 4223]` for the car case). So the
whole-number argument is read as `requested`, not as an owner, and that part of
the diagnosis below was wrong.

What does differ is the instance's script identity: `0x481b4c` passes the
*receiver itself* to `0x53a2a0`, while `create` allocates a fresh object. A
rewrite that reuses the receiver as the script identity and supplies the
receiver's class for the `resource == 0` default-template branch builds and
starts, but the replay then collapses from 4170 to 444 filtered records and
aborts at record 135 on `System.currentTime` (`0x41400000` vs
`0x41f00000`). It was reverted.

## Boot frontier: record 620, investigated 2026-10-21 (after `fe6f2e5`)

Reproduce:

    python3 re/boot-diagnose.py re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog --frames 331 --replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl

Result: filtered/actual 4170 vs 6508, unfiltered/actual 6278 vs 9209, first
divergence at filtered 620 / unfiltered 909. The port emits
`GameRef.queueEvent [0, 16, 5460]` where the original emits
`GameRef.getScriptInstance -> 559164976` and *then* `queueEvent`.

Findings this pass, each checked against the exe and the recorded traces:

- The row is registered. `re/out/native_api.tsv:299` has
  `java.util.resource.GameRef getScriptInstance ()Ljava.lang.Object; LAB_00486f30`,
  `GAME_REF_BINDINGS` carries it, and `node::install` registers it. It is not a
  missing-row gap.
- The port never executes the script call either: the whole run contains zero
  `getScriptInstance` records, and `missing_native` is never set to it. So the
  divergence is not a dropped log line.
- `0x486f30` has no direct call site in the image; its only reference is the
  registrar table entry at `0x48977e`. It is reached through the object vtable.
- The original records it from `FUN_004265c0` at `0x426642`, i.e. after the
  `cmp eax,1` instance gate (`0x426604`) and the `0x5447d0` envelope gate
  (`0x426629`), before the `0x426653` mask test.
- Emitting that record unconditionally from `engine::dispatch` when
  `script != 0` moved the first divergence *backwards* to record 156 (an
  original `File.open` position), adding 126 records that the original does not
  have. That edit was reverted. It shows the port's `script_of` is not
  equivalent to the original's `0x429060` + `vtable+0xc` pair: at record 156 the
  original's `[ecx+0x4c] != 1` or its `esi` is zero while the port has a live
  script.
- `queueEvent` argument slots in `queue_event` (`w[0]` receiver, `w[1]`
  ResourceRef, `w[2]` kind, `w[3]` String) match `marshal_args`, which places
  the receiver last. Changing the target to `w[1]` and adding a default-resource
  fallback changed nothing observable and was reverted.

`0x429060` is now disassembled and is the missing piece. It is an intrusive
list re-link with `ret 4`: `ecx` is a four-word record, `edi` the new parent.
It unlinks `ecx` from its old list, writes `[ecx+4] = [edi+0x44+4]`,
`[ecx+0xc] = edi`, `[ecx+8] = [edi+0x50]`, and links `ecx` under `edi`.
`0x4265c8` passes `edi = [target+0xc]` (the target's container `X`) and
`0x4265cc` passes `ecx = esp+8`, a scratch four-word record cleared at
`0x4265d0..0x4265e8`.

The follow-on read `0x4265f5 mov ecx,[esp+0x10]` was resolved empirically
(Unicorn, `Oracle.call(0x4265c0, [node,0,0,0,0])` with `[node+0xc] = X`,
`X+0x4c = 1`, `X+0x50 = 0xbeef`, `X = 0x20000000`): at `0x426601` `ecx` is
`X` itself, and `[X+0x4c]` reads `1`. So `0x4290bf mov [ecx+0xc], eax` does
write the container into the scratch record and the later load returns it. The
gate is a real type check on the event target's container (`X+0x4c == 1`), not
stack residue. An earlier note in this file claiming stack residue was wrong
and is corrected here.

So the port's `is_type1` check in `engine::dispatch` *is* the equivalent gate,
and the remaining discrepancy is the target's script-ness, not the gate.

Locating it further: a scoped `getScriptInstance` record emitted from
`queue_event` before `bridge` (type-1 target, per-container gate clear,
`script != 0`) produced 4233 filtered records and moved the first divergence
to record 156, where the original has `java.io.File.open`. Walking the two
lists there gives the reason: the `queueEvent` at port 157 corresponds to the
original's 156, and the original has **no** `getScriptInstance` before it. That
event's target came from `GameType.createNativeInstance` (port 154 / original
154) — a native instance, whose `vtable+0xc` returns 0, so the original skips the
whole block.

The port instead reports a non-zero script for that container: the scoped probe
read `script = 4219` for target `0x615e00`, which is the very word `queue_event`
received as its receiver. So the port's native instances carry a script where
the original's have none. That is a resource-model defect in how
`createNativeInstance` publishes its container, not a logging or ordering bug.

Both experiments were reverted; the tree is back at the `fe6f2e5` baseline.

main.rs:633` binds the `GameType.createNativeInstance` row (`0x00481a70`) to
`factory::create_native_instance`, which simply calls `factory::create`.
`create` unpacks its argument container as
`[receiver, owner, template, parameters, name]` (`factory.rs:985`), matching
`GameRef.create`'s `(GameRef, GameRef, String, String)`; but
`createNativeInstance`'s registered descriptor is
`(Ljava.util.resource.GameRef;ILjava.lang.String;Ljava.lang.String;)V`
(`slr-lang/src/langobj/gametype.rs:264`). So a whole-number second argument is
read as an `owner` GameRef, `instantiate` publishes a scripted container for a
call that creates a native instance, and `instance.script` is left holding the
receiver word (the `4219` the probe saw). That is why the port reports a live
script where the original's `vtable+0xc` returns 0.

The fix is to implement `0x481a70` on its own terms — `this` is the object whose
`Native.ptr` is rewritten in place, the second argument is the resource to
instantiate (not an owner), and no new script object is allocated — instead of
routing it through `create`. Until then, emitting the `getScriptInstance` row
from `queue_event` will keep moving the frontier backwards to record 156.

That rewrite was attempted and reverted: routing the call to
`instantiate(.., script = 0, ..)` with the receiver's class as the default
template fails closed at
`GameInit constructor: Unsupported(36)` inside
`Dummy.<init> -> Dummy.<init>(GameRef;I) -> Dummy.<init>(GameRef)`. `0x36` is
`HostError::NoBody`: `instantiate`'s default-template branch needs a script body
for the class, while `0x481ae8` caches a *native* template built by `0x53a1a0`
that deliberately has none. So `instantiate` needs an explicit native-template
path (cached per class at `X+0x1d4`, no script body, no `<init>` run) before
`0x481a70` can be ported. Adding that path without a real template registry
entry would be a shim, so it was not attempted.


The clock-replay `THRD-RUNVMI Dummy.getInfo` vs `THRD-INSTVAR-INI` mismatch
remains the other blocker at this frontier; it is independent of this one.

## cfg handler oracle: full parity, leaves and handlers (2026-10-21)

Pushed as `fe6f2e5`.

`cargo test -p slr-physics --test cfg_handlers_oracle` is green: 1091 leaf rows
exercised, 1080 replayed bit-exactly against the Unicorn vectors, 10 rows the
generator records as `nondet` (skipped and applied), 0 unknown keywords.
Reproduce the vectors first:

    ~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen leaves

Fixes in this pass (all verified against `re/out/cfg_handlers/leaves_*.jsonl`):

- `osd_rpmpin` / `osd_spdpin` / `osd_nospin`: the seven scanned floats are not
  contiguous in the object. Offsets now come from the exe (`0x444e80`,
  `0x444f30`, `0x444fd0`): `1a64,1a68,1a6c,1a9c,1aa0,1aa4,1aa8` (bind `1a80`);
  `1aac,1ab0,1ab4,1ae4,1ae8,1aec,1af0` (bind `1ac8`);
  `1af4,1af8,1afc,1b2c,1b30,1b34,1b38` (bind `1b10`).
- `osd_gear` / `osd_speed` / `osd_cond`: `%8s` stores the converted characters
  plus the `sscanf` NUL, capped at eight (matches `505240`).
- `sfx_engine_up` / `sfx_engine_down` / `sfx_exhaust`: the format is four `%f`
  (`0x60f8f4`), not five; defaults are `1.0, 1e6, 1.0, 1.0`.
- `sfx_gearbox_fwd` / `sfx_gearbox_rev`: the float slot is pre-set to `1000.0`
  and is written even when `%f` does not convert.
- `shadow` (`0x46c1a0`): `esi = K+0x1074`; matrix at `esi+0xc`, flags at
  `esi+0x3c` / `esi+0x58`, bind at `esi+0x48`.
- `bolt` (`0x46c4e0`) is a 0x58-byte list node (not the two-bind handler);
  `flapsfx` (`0x46ca70`) is the two-bind handler with slots `flap+0x3c` and
  `flap+0x4c`.
- `material` tail (`0x46cff0`): `esi = K+0x1170`, binds at `esi+0x20`, `esi`,
  `esi+0x30`.
- `logo` (`0x46d400`): bind slot is `K+0x1118`.
- `sfx_loop` (`0x46da30`): bind slot is `K+0x1250`, speed at `K+0x1260`.
- `wing` (`0x46d470`): the format is `%d %f` x9 (`0x61201c`), the record is
  selected by the body pointer at `+0x40`, and the position is re-based through
  the transposed matrix then multiplied by the stored matrix (`0x46d5c4`
  through `0x46d696`).

The wider run is green too:

    cargo test -p slr-physics --test cfg_handlers_oracle cfg_handlers_match -- --nocapture

reports `files=2037 exercised=42635 matched=42635 distinct_kw=53
nondet_skipped=14 unported_skipped=0 uncovered_unexpected=0`. Both cfg-frontier
obligations are closed; the rest of P01-P30 remain as described below.

## Active ownership and verified baseline (2026-10-21, superseding 2026-10-20 pause)

Work resumed under a single owner. HEAD/origin remain `6b63613`; the complete
inherited dirty tree is still uncommitted (do not reset it). Before new edits
the tree was archived at `re/out/boot/ownership-20261021T021707Z/`.

- **Stale-binary correction.** Earlier notes said the runtime stopped at
  type8 `lod_amp 3.000`. That was a stale `slr-resource` rlib linked by a build
  racing another cargo invocation; bundles `…T024444…` and `…T035108…` are
  affected. After a clean sequential rebuild the descriptor parses and the
  actual frontier was, and is, part-template construction. `ScriptData::apply`
  accepts the stock phase text (`script …class`, `native part …cfg`,
  `lod_amp 3.000`); the executed replay is in
  `slr-boot::native_message` tests.
- **Integrated correction: native-class postcreate queries.** Original
  `FUN_00426470` routes getInfo8/9/0x1c to the native payload (`vtable+0x3c`)
  for native-class containers, so no script `getInfo` native records are
  emitted. The port now does the same in `engine::register_created` using
  `factory::instance_template_container` and
  `native_body::template_payload`. Car template `45c510` messages 8/9/0x1c
  return 1 (executed rows: `re/out/boot/native-class-query-original.json`,
  probe `re/oracle/native_class_query_probe.py`; replay test
  `native_message::tests::original_car_template_queries_8_9_28_return_one`).
  Part template `4769e0` messages 8/9/0x1c take the default dispatch returning
  0; the executed part rows still require a real constructed payload and are
  reported as unavailable by the probe.
- **Verified strict frontier (2026-10-21, superseding 20261021T04…).** Bundle
  `re/out/boot/runs/20261021T101620.536318Z-ad4aa764504f` reaches the main
  loop (frame 31, `Osd.animate`/`MainMenuDialog.animate`) with 4,236 filtered
  and 6,346 unfiltered records (was 614/903). It matches the native trace to
  record 619 and then diverges: the original emits
  `GameRef.getScriptInstance` before `queueEvent` at that point, and the port
  does not.
  - Root cause: the original's `FUN_004265c0` reads the event target's live
    instance through `vtable+0xc` before the mask test, and the hook layer
    records that read. The port's `engine::dispatch` resolves the same
    instance without a call-log record.
  - A second, larger gap: the original runs `Part.addpart` 58 times (one per
    `getScriptInstance`); the port never runs it. `Part.addPart(
    I,String,Descriptor)` branches at bytecode node 135 on the created part
    being non-null and only then calls `addStockParts` (node 141) and
    `setTexture` (node 154). The port's executed lines for that method are
    685, 690, 691, 695, 698, 702, 703, 712, 714, 725 — no 717/719 — so the
    part it creates is the null that node 135 tests, and the original's
    part is not.
- **Resolved this checkpoint.** `slr-boot` unit tests pass, the whole
  workspace test suite passes, and `stock_menu_prefix` (the stock menu boot
  integration test) passes. Strict clippy and rustfmt are clean.
- **Open, in priority order.**
  1. `Part.addPart` must create a live part instance: fix the node-135 path so
     `addStockParts`, `setTexture`, `addpart`/`getScriptInstance`,
     `partOnSlot`, `getSlotID`, `disableSlot` and `setTexture` appear in the
     native trace at their original counts (58/112/119/76/20).
  2. Clock-replay thread identity. The original enters `THRD-RUNVMI
     Dummy.getInfo` (class `java.lang.GameType`) immediately after each
     `THRD-CREATE` constructor; the port goes straight into
     `THRD-INSTVAR-INI` part field initialization. `register_created` takes
     the native-template branch for a `native car` container, but the
     original still dispatches the `GameType.getInfo` script query.
  3. `getScriptInstance` before `queueEvent`. Original `4265c0` calls the
     event target's vtable `+0xc` (`486f30`) at `426642`, after the
     `cmp eax,1` type gate and before the `426647` mask test, and `41fbc0`
     records that nested native on return. The port now emits the same
     record from `engine::dispatch`. Original counts are 58
     `getScriptInstance` against 69 `queueEvent`; each of the 58 returns the
     script instance object that the preceding `GameRef.create` returned,
     and the 11 without one are events whose target is not type 1 or whose
     nested call is separated by `arraycopy`.
  4. Thread-interleaving at frame 31. After record 445 the original
     interleaves the frame thread's `Object.wait` with the "Game time
     refresher" thread's `System.currentTime`; the port runs `wait` first.
     This is the current first divergence (filtered 447, unfiltered 647).
  5. The endpoint remains 6,508/9,209 and 300 idle frames; no boot acceptance
     is claimed.
  6. The file worker now runs. `engine::step` drains one `native_file::pump`
     quantum per frame, which is where `428450` gives the file manager its
     turn, so `505c00` file completion and `544540` descriptor completion are
     live. The shape main-thread pass (`505cd0`/`505db0`/`505da0`) and the
     synchronous `505960` constructor still have no caller: every entry point
     needs a graphics-device collaborator that creates materials and finalizes
     objects, and `slr-d3d9` provides none. Stock boot issues no shape request
     before MainMenu. Those items are annotated with that reason and fail
     closed rather than report geometry as ready.
  7. `Part.addpart`/`addStockParts` never run. Original counts are 754 and
     1,508 executed steps; the port executes none. `GameRef.command` runs 174
     times and reaches its `queueEvent` node, but the resulting `dispatch`
     never delivers the `addpart` message that appends to `attachedParts`, so
     `isComplex` stays zero and `addPart`'s node 135 always branches to
     `destroy`. Fixing this is required before the part graph can match.

## Historical pause (2026-10-20)

- Before new edits, the complete tracked diff and untracked files were archived
  under ignored `re/out/boot/ownership-20261021T021707Z/`.
- Fresh strict baseline `re/out/boot/runs/20261021T021737.728442Z-036476be59fc/`
  stops at frame31 on the missing original file-worker request bridge for
  `cars\\racers\\Prime_data\\meshes\\chassis.scx`. Its 600 filtered/889 unfiltered
  native records match the accepted prefix; the full 6,508/9,209 endpoint remains
  incomplete. Use the reproduction command below.
- The inherited combined tree does not pass checkpoint validation. Initial
  Clippy receipt `re/out/boot/owner-clippy-initial.txt` reports unused unintegrated
  helpers and style failures. Initial workspace receipt
  `re/out/boot/owner-workspace-initial.txt` also identifies a stale mesh progress
  fixture. Preserve these failures; resolve their causes before committing.
- Validated checkpoint `6b63613`: raw particle oracle output is preserved; only
  confirmed malformed-input corrections use separately checked exceptions.
  Diagnostic bundles now archive untracked source bytes, including nested
  `manifest.json` files, and verify their integrity. Isolated workspace formatting,
  strict Clippy, workspace tests and the normal commit hook passed; diagnostics
  Python25/25, serialization Python3/3, collector470/470. Receipts are
  `re/out/boot/checkpoint-evidence-*`. The automated review exceeded its input
  limit; independent manual review found and corrected the nested-manifest
  defect before accepting this limited checkpoint. No whole-game credit follows.
- Current runtime strict bundle
  `re/out/boot/runs/20261021T024444.425865Z-0fc69beb141c/` stops frame31 at
  unsupported type8 descriptor `lod_amp 3.000` while adding stock parts.
  It records617 filtered/906 unfiltered calls, with the first mismatch at602/891.
  This is not a passing prefix claim. The completed outer creation is preceded
  by three extra `GameRef.getInfo` calls (8/9/28). Investigation traced these to
  `engine::register_created` dispatching scripts for a native car. Original
  `426470` routes native payload queries; direct original `45c510` observations
  return1 for all three. The production correction is not yet implemented.
- Integrated but not yet checkpoint-validated runtime changes include actual
  asynchronous resource requests (pending access returns null), synchronous
  descriptor read/reuse, live typee cloning, Part render/texture retargeting,
  callback-visible render initialization order, and active spatial processing.
  Focused original probes pass; full boot remains failed. The file/shape worker
  scheduling and synchronous shape decoding are still explicit integration gaps.
- Independent audit found that x87 probe input encoders discarded negative
  zero. The audit agent corrected both encoders and generated a fresh21,702-row
  Unicorn capture; old capture bytes were preserved. These changes remain
  uncommitted and need integrated review. **Authority correction:** ADR0008
  amends ADR0007: local Apple Silicon validation triangulates SoftFloat,
  Unicorn and Rosetta, and pins transcendentals to a documented implementation.
  Physical Intel hardware is not a required input. Earlier notes claiming it
  was required are withdrawn. Current Rosetta execution availability still
  needs assessment against that amended contract.
- Last agent work after the strict capture: type8 `lod_amp` parser/probes and
  logical-to-raw LOD projection were in progress; shape synchronous/retry
  helpers were tested but production decoding/material integration remained
  open; malformed-particle and x87 evidence corrections were in progress.
  Inspect the actual dirty tree and rerun checks before crediting any of these.
- Required final verification remains a separate run on frozen integrated
  source and unchanged stock inputs covering the entire acceptance contract,
  including human listening evidence and review of screenshot failures where
  required by ADR0009. No unavailable human evidence is assumed to have passed.

## Historical fresh-session handoff (2026-10-20)

The user requested a fresh session. The three delegated agents are interrupted;
do not assume their pending work or integration is complete. HEAD remains
`56d0663`; no new validated checkpoint was committed or pushed this session.
Preserve and assess the entire dirty tree before continuing.

The latest strict capture below predates subsequent asset-loader, template-cache,
shape/file and spatial work. In particular, `native_assets` is now connected,
but the real file-worker bridge and render initialization ordering still need
integration and fresh strict verification. Inspect the current sources rather
than treating focused helper receipts as production acceptance. The template
cache ownership regression passed in `re/out/boot/owner-template-cache-tests.txt`;
combined repository checks remain outstanding.

## Ownership continuation evidence (2026-10-20)

Whole-game authority is now operationally collected in
[fidelity.md](fidelity.md#whole-game-completion-contract). All P01–P30 remain
open. The active objective is the entire reconstruction; boot remains one
required milestone. No whole-game or boot acceptance is claimed here.

The inherited working tree was preserved before edits in ignored
`re/out/boot/ownership-start/` (tracked binary patch and untracked archive).
HEAD/origin remain `56d0663`; the following integration is uncommitted.

- Fresh strict bundle `re/out/boot/runs/20261020T194819.605586Z-b2a51aed66c9/`
  confirms the directory-registration fix passes the previous missing
  `Prime_models` frontier. All 600 filtered/889 unfiltered emitted calls still
  match, stopping at the configured native constructor boundary.
- Bundle `re/out/boot/runs/20261020T195637.258753Z-97e59e6a3f9b/` subsequently
  executes the actual Prime chassis template constructor/defaults, cfg handlers,
  mesh bodies, type-9 collision definition and postparse processing. It retains
  the same 600/889 native prefix and stops at the explicit runtime graph boundary.
  This bundle contains temporary mesh diagnostic stderr, subsequently removed.
- Latest strict bundle `re/out/boot/runs/20261020T205334.253198Z-3faae856a7cf/`
  retains the exact 600 filtered/889 unfiltered emitted native-call prefix.
  It passes configured Part/compound construction, slot query/add_mslot
  commands, wheel setup and camera attachment, then stops at the type-e render
  mesh bounds/get-data collaborator. The full 6,508/9,209 endpoint is still
  outstanding; this is not boot acceptance.
- The configured runtime now uses actual raw resource constructors, original
  loaded/unloaded registration lists, persistent template ownership and raw
  intrusive references. Successful publication occurs only after all car stages
  return successfully. Current production still stops on missing asset/file or
  physics work. Template and render payload unloads retain explicit boundaries
  until their actual native class destructors are integrated.
- Composed car tests cover the unhooked physics prefix, the constructor epilogue
  with declared render callbacks, and camera-bearing attachment arguments.
  They caught and corrected a root-versus-backbone transform bug and a camera
  caller that overwrote the backbone reference. Full raw Part, nested compound,
  world and repeated-spring tests retain every word, including original scratch
  provenance modeled by `native_stack`. These scoped tests do not grant whole
  constructor/render/physics acceptance.
- Additional original probes cover resource constructors and tree ownership,
  collision definitions, actual mesh inertia, native query/commands, render LOD
  retargeting, type-e descriptors/shadows/cloning and file/shape request queues.
  Probes are durable under `re/oracle/`; fixtures and qualified receipts remain
  in ignored `re/out/boot/`. No captured output replaces runtime logic.
- Original4a5190 empty-world stepping is connected. Independent and compound
  bodies, RK4, kinematic branches and no-contact spring phases have differential
  vectors. Real drive callbacks, spatial indexing, contacts and solver paths
  remain unfinished. Active bodies are never silently advanced by the former
  empty-world approximation.
- Passive original generation capture `re/out/headless/generation-investigation/`
  uses the accepted executable/profile identities and observes the original
  counter writes and car entry without replacing execution. It confirms image
  default6200a4=1, shared flush/normal-frame increments and car entry generation39.
  Production shares the render host counter with native memory; render native
  unit/oracle tests pass in `owner-generation-render-suite.txt`.
- Validation remains partial. Focused native suites pass at recorded source
  states, including `native-focused-combined-tests.txt` and later focused
  dynamics/LOD/resource receipts. `owner-workspace-rest-tests.txt` passed the
  workspace excluding boot earlier. A fresh combined formatting/clippy/workspace
  run and strict full endpoint are required before a checkpoint can be called
  validated. Current unintegrated helpers still produce warnings; suppressing
  them or weakening `stock_menu_prefix` is not an acceptance path.

Continue from the actual current tree, not just the historical handoff below.
Keep stock/reference read-only, preserve every failed capture, and retain the
strict full endpoint and workspace acceptance requirements.

## Earlier fresh-agent handoff (2026-10-20)

The user now intends to hand ownership of the **entire game decompilation** to a
fresh agent. Work so far remains focused on this boot milestone; neither boot
parity nor whole-game completion has been achieved. This milestone's Definition
of Done below is authoritative for boot, not a whole-game acceptance specification.

- **Committed and pushed:** branch `exact-port`, HEAD/origin `56d0663` (raw boot
  clocks and callback lifetimes), preceded by `71fd874` (VM String-backed thread
  names) and `2cd52dd` (preserved diagnostics/replay/regression tooling). The last
  committed checkpoint passed the repository hook's formatting, clippy and
  workspace tests; historical receipts are under `re/out/boot/`.
- **Last strict evidence:** bundle
  `re/out/boot/runs/20261020T192655.117211Z-12381134c173/` consumes7,011 raw clock
  requests and emits600 filtered/889 unfiltered native records, each an exact
  accepted-original prefix, then stops at frame31 vehicle creation. The diagnostic
  timeline identifies missing base class `java.game.cars.Prime_models`. An
  uncommitted original-derived directory-registration fix has focused tests, but
  has not yet been validated by a subsequent strict run.
- **Uncommitted work:** video COM failure behavior; deferred resource destruction,
  alias invalidation and ID recycling; cfg resource bindings; loader diagnostics
  and package directory registration; native car template/default initialization;
  shared native-body memory/ownership; geometry, Part/render and wheel projections;
  supporting original probes and passive lifecycle observers. Inspect `git diff`
  and untracked files: these are real work in progress, not an integrated release.
- **Validation limits:** current `cargo check -p slr-boot` passes with40 warnings
  (`re/out/boot/fresh-agent-handoff-check.txt`). Focused receipts pass video3,
  factory5, PartRender3, Parts3, wheel5, defaults1 and geometry2 tests. These were
  run at different intermediate source states. The combined tree has not passed
  fresh workspace formatting/clippy/tests or strict acceptance. Later resource
  unload/recycle ordering still needs strict comparison.
- **Principal remaining boot blocker:** integrate actual configured car/part
  runtime construction and its physics/resource graph. Original functions
  `46eae0` (base Part) and `44a250` (car instance) remain to be completed/integrated;
  raw physics entity/compound helpers were investigated but no `native_part.rs`
  or `native_entity.rs` implementation exists at handoff. Template postparse
  behavior and nonempty Part render retargeting also remain incomplete. Keep
  existing explicit boundaries until backed by original evidence.

Stock/reference inputs remain read-only. Oracle captures are ignored local
artifacts under `re/out/`; durable probe programs are under `re/oracle/` and
`re/headless/`. Available tools include `~/slr-oracle-venv/bin/python`, Kuna and
retained `re/out/all_pseudo.c`. No active subagent should be assumed to continue
work after this handoff.

Reproduce strict boot comparison with:

```sh
python3 re/boot-diagnose.py re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog --frames 331 --replay re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl
```

The runner verifies source/input identities; keep sources stable during capture.
The accepted endpoint contains6,508 filtered/9,209 unfiltered native records and
300 idle frames after completed MainMenu construction. Subsequent sections retain
historical baselines and evidence; use this handoff and fresh diagnostics for the
current frontier. Choose the implementation sequence freely, resolving underlying
systems rather than treating each historical mismatch as a separate task.

## Earlier runnable baseline

```sh
cargo run -p slr-boot -- stock --config-instance-id 0xffff0080 --game-init-instance-id 0xffff007f
re/boot-compare.sh re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog
```

Diagnostics now use unique run bundles; see [diagnostics.md](diagnostics.md).
`re/boot-compare.sh` invokes the fail-closed diagnostic runner. It returns
nonzero for the still-open parity divergence and prints the new bundle path.
The earlier fixed-path receipts below remain historical evidence, not the
output location of a new run. Direct `slr-boot` executions also select a unique
runtime directory unless `--output` is supplied; existing capture files are
never overwritten.

Unchanged stock scripts execute Config and GameInit, enter MainMenu at frame
179, finish the formerly failing OptionsDialog path, and run all 600 scheduler
frames without a script/native stop. A read-only observer checks the actual
`GameLogic.actualState` class and excludes pending MainMenu, MainMenuDialog or
OptionsDialog calls. It observes completed menu construction at frame 179 and
420 subsequent idle frames at frame 600. The driver exits successfully only
after >=300 idle frames; a 200-frame run rejects its 20 idle frames.
This is runtime progress, not boot parity acceptance.
Parity is still 0/30.

## Definition of done

MainMenu is entered, its dialog finishes building, and at least 300 idle frames
run without a stop. The filtered native log must match the headless original
through that point. Unfiltered/node divergences must be fixed or explicitly
listed here. A clean test suite alone does not satisfy this definition.

## Evidence and its limits

* **Original oracle (strongest available):** the stock exe under Unicorn with declared
  Win32/D3D9/DirectSound/DirectInput scenario shims (`re/headless/`, branch
  `worktree-agent-a20d568c5b681332f`, preserved at `33d8aa8` and merged
  into `exact-port` by approved merge `f144ecc`). It executes the original
  `Thread::run` over the same stock scripts and records the script-to-native call
  log. Run with `re/headless/run-boot.sh`; log at
  `re/out/headless/boot/observed/script-natives.slrlog`, facts in `original-facts.json`.
  Config ID 0xFFFF0080 was observed in the original. The controlled driver supplies
  GameInit ID 0xFFFF007F; original allocation history is not fully replayed.
* **Comparison (current checkpoint):** package header loading moves the first
  node divergence to #65018 (with `DROP=java.lang.GameType.getInfo`, field
  initializers skipped by default; common-prefix comparison only): the port
  runs `GameLogic.run` where the original runs `Osd.animate`. All 28 `openLib`
  results and the 48 registered package names/indices match the original
  discovery capture. Resource ID records #167–169 still match. The filtered
  native divergence is #446: original `System.currentTime`, port `Object.wait`.
  Unfiltered divergence is #646: original `Input.getInput`, port `Object.wait`.
  The device-name, Viewport, controller mapping and vehicle constructor-order
  frontiers have been superseded. This is not full-trace acceptance.
* **Validation:** the package/controller checkpoint passes 936 Rust workspace
  tests, with no failures or ignored tests, and workspace clippy. Checkpoint
  receipts and original-probe boundaries are listed below.
  `String.token` now has a real unchanged stock script-wrapper regression:
  earlier native-only tests supplied the argument cells in the wrong order.
* The scheduler, `Thread.start`, event/timer model, resource instance creation, header
  registration for other packages, `openLib`, `FindFile`, `File`, string concatenation
  and others were generalised from probed special cases **without per-function original
  vectors**. They are validated only by the whole-run comparison above, which is
  incomplete. They are not exactness claims.

## Declared scenario inputs (not observed in the original unless stated)

* Simulated clock: ms counter advancing 0.01 ms per read, +1000/60 ms per frame.
* Frame loop: fixed frame interval 1/60 s, world max step 0.01 s, time scale 1.0
  (`engine::FrameConfig`). Ask the oracle for the observed values (requested).
* Input devices: `SysKeyboard`, `SysMouse` only (`INPUT_DEVICES` in `slr-boot/src/main.rs`).
* Display scenario: 640x480, 800x600, 1024x768 at 32 and 16 bits, depth
  descending then width/height ascending, matching the oracle hypothetical HAL
  profile. Current mode: 800x600x32. Mode names are actual VM Strings.
* Viewport, Camera, Text, RenderRef.create, PhysicsRef.createBox and ResourceRef.duplicate
  now maintain resource identities/ownership. Camera creates a type-12 camera
  resource and a type-3 render instance; Text binds its `renderinst` field's
  handle. Renderer/physics payload creation, asset copies and their side effects
  remain unimplemented. Other headless render/sound/input natives answer zero; the 170 native rows no
  engine slice implements (`missing_rows.rs`) are capture-only too.
* Resource materialisation checks the descriptor's source file exists and marks the
  container loaded; nothing is decoded and original file I/O records are absent.
* Native object classes named by `native <name>` descriptors create no full engine
  object. The `player` command path now synchronously invokes script `setcontrol`
  (`FUN_0047a920`); other player commands remain capture-only.
* Missing linked packages (stock links `obstacles\*.rpk`, which the media lacks) get
  declared placeholder roots.

## Frontier (historical; superseded by the 2026-10-21 section at the top)

This list predates the current checkpoint. Read the top section first; the
current frontier is record 620 / 909 and the clock-replay thread identity.

1. Filtered native #446 and node #65018: scheduler/time-budget ordering.
   The original uses the declared instruction-count-driven QPC scenario;
   the port still uses a per-read clock and fixed 1/60 frame interval. Resolve
   the actual time inputs and scheduling behavior before altering ordering.
2. Incremental GC and finalizer queues now run. The first 317 queued allocation
   identities/classes/order and the first four finalizations match the original
   captures. Full object/resource lifetime, physical destruction and later
   collections remain unaccepted. Do not drop finalizers or mask primitive IDs
   to force acceptance.
3. Unfiltered #646: original `Input.lastKey` versus port `Object.wait` in the
   current accepted-log comparison (earlier receipts named `getInput`).
   The original input poll and native player `device` commands remain gaps.
4. General interface-array conversion remains an explicit VM boundary. The
   class/primitive-array candidate path now rejects unrelated class overloads
   rather than trying to load an array as a class file (original array CCDs have
   Object as their base). This is not general array covariance support.

## Known gaps behind the happy path

* Resource allocation/lifetime beyond the measured prefix through #167–169.
  The declared original profile's first allocations are Config `0xffff0080` and
  GameInit `0xffff007f`; the driver still supplies these IDs explicitly.
* Physics world, render/audio/input behaviour, SDAT saves, general descriptor parsing.
* `FUN_00428960` frame work not run: input poll, object-list step, `FUN_004277f0`
  reaping, camera/listener setup, render pass, pacing.
* GC/finalizer lifetime beyond the measured first sweep/drain; physical resource
  destruction and reaping remain incomplete.

## Tooling added for this work

* `slr-dump <class>`, `slr-rpk <rpk> [id]` (`crates/slr-boot/src/bin`): class node streams and RPK
  descriptors. Diagnostics: `SLR_TRACE_CALLS`, `SLR_TRACE_NATIVES`, `SLR_TRACE_RES`,
  `SLR_TRACE_FIND` environment variables on `slr-boot`.
* Outputs under ignored `re/out/boot/`: `runtime.txt`, `runtime.calllog`, `engine-events.txt`,
  `materialisation.txt`, `factory-operations.txt`, `resource-registration.txt`, `compare.txt`.

## Recovery

* Remote `origin/exact-port` holds the integrated runtime and oracle source. The
  oracle worktree `.claude/worktrees/agent-a20d568c5b681332f` is inactive, clean and
  retained at `33d8aa8`, also pushed on its original branch. No old agent needs
  to be resumed. Do not delete that worktree or either branch without approval.
* `stock/` and `reference/` are read-only; derived outputs stay under `re/out/`.
* No Wine. Issue #1 and `docs/plan.md` remain the umbrella; #110 tracks VM completion.

## Checkpoint receipts (2026-10-20 resume)

Ignored `re/out/boot/steps-port-command.txt` reproduces node prefix improvement;
`steps-port-token.txt` records the next array-overload stop;
`array-stop.txt` records 600 completed frames after the array fix.
`menu-observer.txt` records frame 179 completion +420 idle frames;
`menu-observer-test.txt` records the positive/short-budget regression.
`token-red.txt` / `token-green.txt` show the argument-order regression and fix.
Oracle `re/out/headless/suite-accepted.txt` records the 169-test pass in its
worktree. That worktree and its derived captures must be retained. A second
ignored copy of the four comparison inputs is at
`re/out/headless/oracle-33d8aa8/observed/`; pass its `script-natives.slrlog` to
`re/boot-compare.sh` to reproduce this checkpoint without relying on a worktree path.

No per-function oracle vector yet covers synchronous command dispatch; its
current regression evidence is the integrated node/native comparison and
`cargo test -p slr-boot --test stock_menu_prefix`. That regression checks
Controller.setcontrol, both menu methods returning, and 600 completed frames;
it also rejects a short run, and intentionally does not claim original parity. Native
player mask initializes to observed EVENT_COMMAND (0x10) and uses script
set/clear operations; native object deletion/lifetime remains incomplete.

Review checkpoint: independent Standards review found no documented violations;
Spec review's command-mask bypass was fixed and re-reviewed. The final Jev gate
verified all three bounded runtime/test/gap claims without contradictions. Its
patch review still escalated (safe-to-apply 0.38, low test-gap confidence 0.07),
so this is not automatic quality approval or parity acceptance. Interface-array
coverage and the native-log mismatches remain explicit follow-up work.

### Viewport identity checkpoint (2026-10-20 continuation)

`re/oracle/viewport_create_probe.py` executes original `LAB_004814e0`,
`FUN_00538d60`, the real ID allocator and `ResourceRef.id` in three controlled
cases: fresh handle, already-bound handle with a retained sibling, and missing
handle. It supplies resource registration, virtual data/retain, Native.ptr
lookup and renderer registration. Original intrusive rebinds and five viewport
state stores execute. Two declared earlier allocations reserve Config/Init IDs;
this is not original startup allocation history or D3D execution.

The Rust replay verifies the resulting resource type/ID, unchanged handle,
old-list repair, viewport state and next allocator result. It also checks that
another wrapper bound to the same resource reads its viewport width. A temporary
restoration of the old create body failed on ID zero versus `0xffff007e`; the
new body passes. This validates the measured identity/state projection, not the
whole render factory. Camera and Text creation remained capture-only at this
checkpoint; the allocation-history continuation below supersedes that boundary.

Local receipts: `re/out/boot/viewport-create{,-repeat}.jsonl` (byte-identical),
`viewport-red-mutation.diff`, `viewport-create-red.txt`,
`viewport-create-green.txt`, `viewport-tests.txt`, `viewport-clippy.txt`, and
`viewport-workspace-tests.txt`; the final native/node comparisons are
`viewport-compare.txt` and `viewport-step-compare.txt`. Reproduce the focused replay with
`cargo test -p slr-boot --bin slr-boot viewport_creation_original_rows` after
generating the ignored oracle rows. Original Ghidra factory evidence is retained
in `re/out/boot/viewport-analysis.rea.json`, Evidence
`ev_ac8c111dd23106fcec6e70cd9cc6972844bb8b92a4e05ac83d18069677319f87`;
the native wrapper was also inspected directly from the executable's bytes.
The decompiler omits an unused factory argument; the probe follows the actual
six pushed arguments and confirms owner zero separately from the flag word.

Independent Standards and Spec reviews found no concrete issue within this
bounded projection. The automatic Jev gate escalated (safe-to-apply 0.15,
low test-gap confidence); it is not automatic quality approval. Its initial
test-claim contradictions mixed the deliberately restored old-body red receipt
with the final green run. Source/receipt inspection and a separate verification
resolved those bounded test claims (0.95/0.86 confidence). The remaining parity
claim was verified but still marked for review (0.56); the literal final
comparison receipts above retain both mismatches. No full-frontier completion
or original boot parity is claimed.

### Resource allocation history checkpoint (2026-10-20 continuation)

`re/headless/resource_history.py`, attached to the passive Observer, reads
allocator/recycler entries and returns plus runtime resource registration
descriptors. It never writes emulated state. The declared original startup was
run for 45M block-counted instructions with no native/factory replacement.
`re/out/headless/allocation-history/long-observed/original-facts.json` records
64 history events, no truncation and no pending allocator calls. There are no
recycles before the failing viewport allocation. Its ID is `0xffff0065`.
The captured 299-call unfiltered native prefix is byte-for-byte equal to the
preserved oracle prefix, including primitive results. The 64-event history
also repeats identically in a second run; see `repeat-verification.txt` in the
same directory (18,562 exact prefix bytes).

The eleven missing allocations before that viewport were two resource
duplicates (types 14 and 5), one RenderRef instance (type 3), two physics
instances (type 2), two text instances (type 3), and two camera type/instance
pairs (types 12 and 3). They now create registered containers, use the shared
allocator, repair existing handles, append parent-child links, and retain
instance templates. Duplicate's other-type alias branch consumes no ID.
The camera type root is resolved from the actual system.rpk header. Text uses
the native handle of its `renderinst` field, rather than a ptr on Text itself.
This is a headless identity projection; mesh/font copying, physics bodies,
renderer registration and full factory failure/lifetime branches are not exact.

The stock-menu regression first failed on record #167 (`0xffff0070` versus
`0xffff0065`). It now compares #167–169 against the preserved original and
also compares the original created-resource ID/type/parent prefix with
`re/out/boot/resource-identities.tsv`. MainMenu still completes at frame 179
and runs 420 idle frames. The next filtered mismatch is #202 `System.openLib`;
unfiltered #204 remains `Controller.user_Add`. Boot parity remains unaccepted.

Reproduce the history (the ignored graphics/media/desktop profiles must exist):

```sh
re/headless/run-boot.sh --limit 45000000 \
  --observe-dir re/out/headless/allocation-history/long-observed \
  --out re/out/headless/allocation-history/long.json \
  --calllog re/out/headless/allocation-history/long.slrlog
cargo test -p slr-boot --test stock_menu_prefix
re/boot-compare.sh re/out/headless/oracle-33d8aa8/observed/script-natives.slrlog
```

The new integration regression requires that local history capture as well as
the preserved native log. Its receipts are `re/out/boot/resource-history-red.txt`,
`resource-history-green.txt` and `resource-history-compare.txt`. Observer tests
exercise the real allocator/recycler with and without hooks, compare returned
IDs/free stacks, and check explicit truncation: receipt at
`re/out/headless/allocation-history/test-history.txt`.
The complete headless Python suite now passes 171 tests with no failures or
skips (`allocation-history/suite.txt`); workspace clippy also passes
(`re/out/boot/resource-history-clippy.txt`). Independent Standards and Spec
reviews found no concrete issue within the declared identity projection.
Workspace tests pass 932 tests, zero failures/ignored
(`re/out/boot/resource-history-workspace-tests.txt`, compact receipt
`resource-history-test-summary.txt`). Fresh node comparison remains at #19458
(`resource-history-steps.txt`). The final Jev gate verifies the bounded ID/graph
claim (0.83 confidence) but escalates patch quality on coverage/confidence
(safe-to-apply 0.25). Its history summary was flagged unsupported; that evidence
bundle omitted the explicit native-record count. The machine-checked complete
counts/repeat/prefix facts are now in `allocation-history/verification.json`.
No contradictions were reported. The gate's raw result is
`re/out/boot/resource-history-jev-gate.json`; it is not automatic quality approval.

## Fresh-agent verification and recovery pitfalls

The first continuation task is the filtered native mismatch in Frontier. Read `AGENTS.md`, `GLOSSARY.md`, ADRs 0004/0008/0009,
`docs/fidelity.md`, `docs/research/boot-sequence.md` and `docs/tufa-executor.md`
for constraints and implementation context. The temporary session handoff is
only an index to this canonical state.

The historical comparison wrapper printed runtime status without reliably
propagating failure. Its replacement now records build/runtime/compare outcomes
and requires complete equality of both native logs. Read the bundle's
`report.json`; runtime exit 0 can still coexist with native divergence.
`re/step-compare.py` compares only the common prefix, skips field initializers
unless `KEEP_FI=1`, and must use the documented DROP setting when reproducing
node #19458. Equal common prefixes alone do not establish complete trace parity.

All oracle captures, profiles, `re/out/native_api.tsv`, the stock install and
local oracle virtualenv are local-only dependencies; they are not recovered by
a fresh clone. Preserve `re/out/` and the retained oracle worktree. The four
copied oracle inputs were checked byte-for-byte against that worktree during
handoff preparation. Never commit captures or stock/reference data. The Python
suite is `~/slr-oracle-venv/bin/python -m unittest discover -s re/headless -v`;
missing local inputs can skip tests, so inspect its summary.

Runtime acceptance regression:
`cargo test -p slr-boot --test stock_menu_prefix`.
It runs both 600-frame success and 200-frame rejection using unique output
directories. Its evidence no longer overwrites the standard historical logs.

### Package registration and controller mapping checkpoint (2026-10-20)

The passive `re/headless/package_history.py` observes original name registration,
package construction and header-load entry. It changes no emulated state.
`re/out/headless/package-investigation/full-observed/original-facts.json`
contains the discovery sequence; `repeat-observed` repeats its package history
identically. `headers-observed` records the earlier dependency chronology.
Original frontend links reserve garage/cars before `sl.rpk`. Loading police's
first foreign parent loads `parts.rpk`, reserving `cars/racers.rpk` at index 11
before the first engine package at index 12.

The port now reserves names independently of file availability, loads a named
resource's package headers during literal resolution, and interleaves each
header insertion with its required foreign-parent header load. It does not load
every linked package eagerly. `package-indices.tsv` exposes the resulting table.
The stock-menu regression compares all 48 names/indices and all 28 `openLib`
results directly with original captures. The original vehicle constructor order
now follows from the resource child lists; filesystem enumeration was unchanged.

`re/oracle/openlib_header_probe.py` executes the real native wrapper, package
registration/lookup and header loader over a declared empty valid RPK. Both
first and repeated open return the registered index without local resource 1.
Earlier wrapper-only vectors replaced the lookup helper and therefore did not
measure its own header-loading effect. The focused Rust replay also removes the
synthetic file after loading, checking the retained-header fast path. Original
file I/O and allocation remain controlled seams in this function probe.

`re/oracle/controller_mapping_probe.py` runs original user_Add/Del/Reset wrappers
and mapping-table operations in a declared loaded native player. The 166-operation
Rust replay checks mapping results, counts and retained slot words, including
duplicate keys, deletion/reuse, reset, invalid indices and 152-slot capacity.
Boot resolves the actual loaded native player resource; aliases share its table.
The pointer lookup, resource acquisition and virtual data accessor are controlled
probe seams; device samples, axis metadata, force/smoothing, player commands and
full lifetime behavior remain unaccepted.

Receipts: `re/out/boot/package-indices-red.txt`,
`package-controller-workspace-final.txt` (936 tests),
`package-controller-clippy-final.txt`, `package-controller-compare-final.txt`
and `package-steps-compare.txt`. The headless Python suite passes 185 tests with
no skips (`package-controller-python-final.txt`). Independent reviews checked
the package/header integration, controller projection and passive menu observer;
the original-helper and ready/waiting/bottom-frame issues found during review
were corrected before this checkpoint. The Jev gate verifies the bounded
runtime/frontier, original endpoint and suite claims but escalates patch quality
(safe-to-apply 0.23, low coverage confidence). Its package-count claim required
the actual regression source and table check, then verified at 0.66 confidence;
the literal test and table were reviewed directly. No contradictions were
reported by the gate. Raw receipts are `package-controller-jev-{gate,verify}.json`;
they are not automatic quality approval. The strengthened stock-menu test retains
the existing resource-ID/ownership checks and short-budget rejection. Native
comparison now reaches filtered #446/unfiltered #646; the node comparison reaches
#65018. MainMenu still completes at frame 179 with 420 idle frames. Full parity
remains unaccepted, and the timer/clock and finalizer frontiers above remain open.

The preserved oracle call logs extend beyond their stale `original-facts.json`
sidecar. Fresh isolated original runs reproduce their complete native prefix;
the receipt is `re/out/headless/menu-continuation/preserved-prefix-comparison.json`.
Treat that older sidecar as a shorter observation, not as the log's coverage
certificate.

The fresh paired `menu-continuation/menu-final` run now establishes the original
acceptance endpoint under the unchanged declared platform scenario. The passive
`menu_history.py` reads GameLogic's actualState, validates its class-variable
table, and walks both ready and waiting thread frame lists. It rejects unknown
frames except original-constructor-proven empty bottom frames and excludes
pending MainMenu/MainMenuDialog/OptionsDialog frames. The same MainMenu object is
complete after frame 31 and has 300 subsequent idle frames at frame 331.
`verification.json` records the matching native-log cutoffs: 6,508 filtered and
9,209 unfiltered records. The exact byte slices at `menu-final/accepted/` are the
current comparison inputs; their digests and source-run limits are recorded in
that verification file. Completed stock menu-construction return nodes corroborate
the observer in `menu-node-bounds.json`. This establishes the original endpoint,
not port parity. Keep all earlier captures; no oracle inputs were changed.

### Diagnostic history and replay checkpoint (2026-10-20)

The five observability recommendations are implemented in the diagnostic runner
and runtime; [diagnostics.md](diagnostics.md) documents commands, schemas,
retention and limits. Every new run has a unique directory, frozen oracle and
replay inputs, source/binary/input identities, explicit lifecycle, streamed native
and causal events, frame/failure state and machine-readable regression cases.
Offline analysis verifies retained artifacts; unfinished wrappers expose only
diagnostic prefixes with explicit unverified status. The comparison wrapper now
fails closed on build/runtime/evidence problems and complete-trace differences.

Original overload descriptors are trusted only after reconstructing the entire
selected original native stream from their captured argument/result words.
The current 9,209-record all-log reconstruction passes. Primitive IDs remain
significant; only declared references and the explicitly recorded legacy clock
policy are normalized. Historical replay freezes the effective trace environment
and preserves relative input provenance, including the external frame-timer
companion references. A changed implementation binary is reported, rather than
silently treated as the old run.

The VM's tracing records actual interpreter metadata without additional clock
reads. GC uses the measured 250-object incremental budget, allocation/write
barriers, queued-String revival, original root ordering and scoped finalizer
threads. The stock regression checks the 1,196 pre-frame classes/String contents,
317 first-sweep identities/classes/order, and first four finalizations. Mixed
String/number concatenation now performs the concatenation after conversion;
its four original probe rows also check allocation counts. Loaded Object.toString
uses the class descriptor; older function probes that directly supplied bare
name pointers retain those original inputs and check the formatter separately.
This is measured first-cycle progress, not full lifetime or boot parity.

The final verified legacy bundle is
`re/out/boot/runs/20261020T182738.219377Z-c150efb2a778/`.
Runtime exit is zero, while the runner correctly exits 2 for the existing native
differences: one-based filtered record 447 and unfiltered 647 (the earlier
zero-based frontiers 446/646). Its 91,278 events and 602 state checkpoints have
valid completion counts and no parse/missing-stream errors. The first divergence
maps to native call 647, `GameLogic.run`, source line 565/node 4, calling
`Object.wait`. The automatic rerun reproduces both complete raw native logs
byte-for-byte and captures the current/parked threads, native argument objects,
resources and recent events at that exact boundary. Offline inventory checking
and separate executable assertions verify these facts; see
`re/out/boot/diagnostics-smoke-verification.json`.

Raw clock replay has explicit request identities and computes time from raw
counter/frequency words, including the independently captured Timer7 frame-end
inputs. Its current embedding starts after GameInit. It stops on the actual
LoadingScreen thread-name disagreement: original `Ljava.render.LoadingScreen;`
versus an empty port name. Earlier pre-boundary legacy time readings also differ
under strict clock-result comparison. The paired negative diagnostic receipt is
`re/out/boot/runs/20261020T182115.340678Z-793ead4cd097/`; it retains the native
prefix and failure state. Resolve these actual metadata/timing differences next;
never scan ahead or substitute native results to consume the input tape.

Validation receipts under `re/out/boot/`: `diagnostics-workspace-tests.txt`
(969 Rust tests, no failures/ignored), `diagnostics-workspace-clippy.txt`,
`diagnostics-headless-tests.txt` (190 tests, no skips), and
`diagnostics-runner-tests.txt` (20 tests). Real process tests verify killed-child
capture survival, immutable output rejection, failure state, GC timeline events
and targeted-snapshot noninterference. Independent reviews found and corrected
missing timeline acceptance, wrong earliest-stream selection, inherited trace
configuration, descendant cleanup, overload masking and snapshot omissions.

The final Jev gate remains a low-confidence quality escalation (safe-to-apply
0.21). Its compact claim check initially contradicted the deliberately failing
parity run and lacked source evidence for two implementation claims. Direct
executable assertions and a separate source/receipt verification resolved those
bounded claims: four verified, none contradicted or unsupported. The literal
runtime/runner exit distinction and both complete raw-log comparisons were
checked directly. These advisory results are retained in
`diagnostics-root-{gate,verify}.json`; they are not automatic quality approval.
The diagnostic milestone is distinct from boot parity, which remains unaccepted.

### Thread-name and full-clock continuation (2026-10-20)

The preserved raw replay exposed an empty LoadingScreen thread name. The VM
Thread.init bridge now reads the real String.ptr bytes, copying through the
first NUL as the original constructor41f340 does. It retains the existing raw
pointer seam for controlled function probes. The focused regression checks the
name, queue flags/priority, script/handle identity and unchanged allocation count.
Receipts `re/out/boot/thread-name-{tests,lang-tests,clippy}.txt` record 36 focused
tests, 222 complete slr-lang tests and successful clippy. The bounded Jev gate
verified the implementation claim (0.92) but escalated test-gap confidence;
the literal test result counts were independently summed from all suite results.
This checkpoint does not claim boot parity.

The full raw tape also includes the earlier Config/initializer phase. Activating
strict replay there reveals its first actual mismatch: original startup uses
x87 control0x23f (53-bit), before device initialization switches to0x3f (24-bit).
The port currently starts every interpreter at24-bit. The diagnostic bundle is
`re/out/boot/runs/20261020T184427.970981Z-a32ecd9a60ef/`. Model the startup/device
precision boundary from execution evidence before consuming those early inputs;
never take precision or results from the tape to bypass a mismatch.

### Startup and frame replay checkpoint (2026-10-20)

Strict raw replay now starts at request1. The port consumes the first6,986
original requests, including Config, GameInit and frames0–30, with matching
clock roles, live thread/script locations and FPU precision. GameInit's end
asserts consumption of6,207 earlier requests without moving the tape. The
latest retained run is `re/out/boot/runs/20261020T190539.042320Z-2673fa4539fc/`.
It stops at request6,987 during frame31; this is not boot parity acceptance.

Creation now executes original427210's getInfo queries8,9,28 before registering
the returned callback capabilities. THRD-RUNVMI uses the original return-request
flags and lifetime branches. Its label comes from the embedded template
reference, including the original30-byte default template-name limit. Scoped
destruction and scheduler reaping retire actual interpreter identities, clear
script handles and unlink queues at the original destructor point. Initializer
native hooks retain their own thread identity; resolved native calls expose the
actual advanced PC. Frame completion drains the real Frontend.render monitor
after animation, without adding a script-native call or a clock read.

Nativeplayer control/animation implement the observed empty branches, including
the raw animation-dt store. A passive353-frame original capture observes1,685
entries with all relevant target/focus/render-link fields zero; its entire9,319
unfiltered native records are byte-identical to the retained original. Constructor
probes distinguish explicit zeroing from fields supplied by the declared zero
allocator scenario. Nonempty paths stop explicitly. Source and receipts are
`re/headless/player_frame_{history,probe}.py` and
`re/out/headless/player-frame-investigation/verification.json`; frame state is
also emitted on the shared diagnostic timeline.

The first317 GC sweep identities and all112 completed finalizers in the latest
strict run match the original prefix. Its earliest unfiltered difference at
one-based684 was a duplicate Viewport.destroy record: the backend and boot
observer both recorded it. Recording ownership now includes only the seven
Viewport methods actually overridden by boot, with a real backend/observer
regression. The subsequent video-open branch is under investigation; the
headless original reports a DirectShow class-registration failure where the
capture-only port currently returns zero. No oracle result has been substituted.

Validation: `re/out/boot/parity-continuation-workspace-tests.txt` records984 Rust
tests, zero failed/ignored; workspace clippy and formatting pass. The independent
Viewport recording regression also passes. The headless suite passes192 tests
with no skips, and the diagnostic runner suite passes20. Original callback,
constructor, empty-frame and render-monitor probes repeat exactly; their
boundaries and receipts remain under `re/out/boot/`.

The checkpoint Jev gate verifies all three bounded claims with no contradictions
or unsupported claims; it still escalates patch quality and two claim confidence
scores (safe-to-apply0.33). Independent source reviews and literal executable
checks support the measured prefixes and empty-branch projection. The advisory
gate is retained as `re/out/boot/parity-continuation-gate.json`; it does not
certify full parity or automatically approve the remaining implementation.

## Deferred resource lifetime integration (pending strict replay)

The passive `resource_lifetime_probe.py` preserves the standard startup scenario
and observes original queue/reference state without changing execution. Its
110M-instruction capture is retained under
`re/out/headless/resource-lifetime-investigation/observed/`; the verification
receipt compares all648 native records (39,444 bytes) exactly with the accepted
original menu capture's prefix. This is capture validation, not port parity.

Original `Camera.destroy` (`4864c0`) retains the private runtime camera template
first, then its render instance. `427620` appends real embedded four-word
references to the FIFO destroy queue. At instruction99,534,212, cleanup
`4277f0` (caller`4285f4`, before frame0 physics) sees the two pairs queued as
`ffff006b,ffff006a,ffff006f,ffff006e`. These values document the capture; the
implementation derives every ID from the live containers and never supplies
this list to the allocator. The four recycle entries occur at99,534,311,
99,535,592,99,536,081 and99,537,362, respectively.

Cleanup drains unload requests before destroy requests and repeats for requests
created during callbacks. Script resources receive event2 before unload.
Derived destructor `544e20` releases its embedded template reference; base
`544700` recycles the actual runtime ID through `536f90`, then `545f70` zeros
all four words of every retained alias. `5517f0` destroys children in list order
and unlinks the resource from its parent. Owning script `Native.ptr` cells keep
their existing reference allocation, now zeroed. Runtime ID recycling therefore
uses the existing allocator's LIFO free stack, following actual deletion order.
The port now executes cleanup after successful physics/timer passes, also when
no physics step succeeds, and after animation before render notification.

Focused factory regressions cover deferred alias validity, duplicate queued
requests, FIFO deletion/LIFO reuse, recursive parent-before-child deletion and
parent/tail-sentinel repair. The passive observer has two bounds/read-only tests.
Fixed package resource destruction remains an explicit boundary. The original
third callback cleanup queue and held-JVM-object free phase are not yet modeled;
they are not part of the captured first four resource recycles. A fresh strict
replay must establish the resulting native/resource frontier before accepting
this integration.

A second passive capture through360M instructions retains301 lifetime events
and810 native records (47,319 bytes), again an exact accepted-original prefix;
see `resource-lifetime-investigation/later/verification.json`. It distinguishes
loaded script unload from the generic base destructor. The observed type1
resource vtable `5f375c` points its unload slot to `53ec20`. That function first
recursively unloads script children, then deletes eligible owned children before
the enclosing resource's base destructor runs. The port now follows this order;
a focused regression checks leaf→script-child→parent recycling for loaded
scripts, while retaining parent→child base destruction for an unloaded graph.
`4274d0` is a bare return in this build, so unloading does not synthesize callback
unregistration. `Viewport.destroy` (`481600`) also queues its actual resource
through `48a8d0`; its release now participates in cleanup. These corrections
still require the next strict trace's later resource-recycle comparison.
