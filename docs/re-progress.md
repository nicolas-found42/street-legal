# Reverse-engineering progress (stock build 606)

Assessment: **incomplete**. Snapshot: 2026-10-19, integration branch
`exact-port`. The old approximate `slr-core`/`slr-game` slice remains until
headless boot passes (M1-14). No stock feature has passed end-to-end parity.

## Measured progress

Reproduce from local artifacts:

```sh
cargo run -q -p slr-ledger -- collect
cargo run -q -p slr-ledger
python3 re/progress.py
```

| Measure | Handoff baseline (`7caab32`) | Current integration |
|---|---:|---:|
| Generated ledger symbol rows | 430 | 487 |
| Rows with passing evidence | 321 | 369 |
| Ported rows without registered passing evidence | 109 | 118 |
| Unique annotated addresses | 423 | 485 |
| Ghidra function entries with an annotation | 200 / 4,356 | 238 / 4,356 |
| Ghidra function entries with passing evidence | 151 / 4,356 | 179 / 4,356 |
| Native registrations with exact ledger symbol | 108 / 353 | 178 / 353 |
| Native registrations with exact symbol and passing evidence | 85 / 353 | 158 / 353 |
| Native registrations with any annotation at the same address | 173 / 353 | 205 / 353 |
| Parity checklist rows passing | 0 / 30 | 0 / 30 |

Annotation alignment adds no implemented behavior. It removes four duplicate
hex-address aliases of already tested SDAT File functions and uses the native
table's exact FUN/LAB spellings for File/FindFile annotations and evidence.
Unique annotation counts are unchanged. An independent review also found that
File.move had been assigned passing evidence despite having no replay case.
That unsupported claim was removed; [#107](https://github.com/nicolas-found42/street-legal/issues/107) now adds real native/layer/helper coverage: 13 fresh Unicorn scenarios, including 12 differential cases and the separately asserted missing-parent bug correction recorded in the fidelity ledger. The per-file gate requested stronger review (composite 0.729; safe-to-apply 0.36); parent source/ABI review found no additional concrete defect. A stale local progress snapshot in its evidence was replaced with a fresh recount before publishing metrics. File.copy also used a misidentified helper hook; [#108](https://github.com/nicolas-found42/street-legal/issues/108) replaces that evidence with 17 real native/layer/helper scenarios, byte-preserving overwrite, actual 0/UINT_MAX returns and destination cache semantics. Fresh copy and move corpora re-verify under Unicorn.

The handoff's 437 rows combine 430 generated symbol rows and seven manually
recorded divergences. Its "25 parity rows" is a miscount: P01–P30 are 30 rows,
all open. LAB labels may be interior to Ghidra functions; these measures cannot
be added together. Passing rows evidence only cases actually asserted. Some
existing tests exercise null/error envelopes or injected engine seams; inspect
them before extending any exactness claim to live engine paths.

The baseline `cargo test --workspace` passed (106 successful test-result
summaries). Current ledger collection passed 466/466 evidence rows. These are test-to-symbol evidence rows, distinct from the 369 passing symbol rows. Language wrapper additions and real File.move coverage increase evidence; removal of duplicate aliases and unexercised File.move/File.copy claims corrects earlier overstatements.

An interner audit also removed `FUN_004174a0` from the math oracle's passing
claims. That probe replaces the helper with a callback that records output bytes
and returns zero; the native formatting evidence remains valid, but String
object identity, deduplication and lifetime are not compared. The helper remains
annotated as ported without passing evidence. Its actual port validation belongs
to #110; controlled original-interner recovery alone does not validate the Rust
implementation.

## Current frontier

- **Stock boot bring-up:** request-driven loading replays all 13 recovered
  original selection rows. `slr-boot` installs native resource links and script
  back-references before unchanged Config/GameInit constructors. Original
  creation probes compare the temporary retained successor during dispatch,
  post-return link state, and newNative/set-same-ID/id without allocation or
  pointer replacement. Config's full constructor preserves its installed link.
  A controlled run with `--config-instance-id 0xffff0080
  --game-init-instance-id 0xffff007f` applies 37 stock settings words, takes the
  measured absent-renderer refresh branch, resolves `Config.init`, then enters
  unchanged `java.game.Init` and completes `Sound.init`. The factory now reads
  stock `system.rpk:0x1b`, applies root1's `lod_amp 1.000` then its script-only
  descriptor, requests the real class and attaches the native Dummy to the
  existing script/Native.ptr. It accepts the measured declared loaded fresh-empty
  game parent only, preserving inherited zero range words and append-order
  child links. Config and Init also register under the game root. Other parent,
  descriptor or package branches stop explicitly. Original Unicorn rows compare
  two supplied text-parser phases, a supplied loaded-empty parent and explicit
  parent registration; general package-manager loading remains unverified.
  The next named stop is `InitializerBoundary` at
  `java.render.Frontend.defLoadingPic` (body419), before Frontend.init.
  The stop includes script frames
  and source line; no engine return is fabricated. Unwired Thread.start is also
  guarded before dropping a pending frame. The one-ID command stops at
  `GameInitCreationBoundary`; the default stops at `ResourceCreationBoundary`.
  Supplied IDs and replay-local package indices do not establish original
  startup allocation history. Script file I/O uses a fresh ignored writable
  projection, never `stock/`. Native calls and settings operations remain
  inspectable under `re/out/boot/`. The installed surface remains 192 native
  keys/191 implementation addresses, including seven Controller rows outside
  the original 353 rows. Original Unicorn probes cover controlled creation,
  links/native queries, empty bodies, field flags, IDs, settings/refresh and
  File.exists/delete; none executes original script bodies or establishes an
  original script-call prefix. Workspace checks: 924 tests, formatting/Clippy,
  ledger 466/466, regeneration without drift. General resource loading,
  original allocation history, present-backend rendering, scheduler/thread/
  clock wiring and MainMenu remain open; parity is still 0/30. See
  `docs/research/boot-sequence.md` and `crates/slr-boot/README.md`.

- **Fresh headless-original replay:** recovered ignored profiles now pass the
  parent-run 109-test Python suite with zero skips. The declared notification
  profile reaches `ShowWindow(HWND, SW_HIDE)` before stock script execution;
  this is the furthest shared startup evidence, not a script-call oracle.
  Recovery receipts remain under `re/out/headless/recovery/`.

- **Request-driven loader:** the recovered original-executable selection oracle
  now replays all 13 path/directory/direct-file rows (5/6/2) against unchanged
  stock class bytes. Lazy requests compare compile/read event order, diagnostics,
  registration counts and selected record identities. Package nodes and basenames
  match exact case; the first matching directory wins, a stale first entry never
  falls back, and cache hits cause no read, registration, event or diagnostic.
  Direct-file rows deliberately bypass the request cache and register all 12
  records across the two Dialog file orders. Stock directory enumeration and
  successful compiler checks remain explicit seams, matching the controlled
  original oracle's boundary; this is not original filesystem, compiler,
  complete boot or MainMenu evidence. Fixtures remain ignored in `re/out/`.

- **#35 / M1-08:** all 40 Object/System/GameType registrations are bound. Fresh executable vectors cover language wrappers, array payload/type behavior and Thread init/start/stop queue state. Compiler, package manager, timers and callback engines are explicit required collaborators; their implementation remains subsystem work. Class has no registered native. Independent review caught and corrected zero-length arraycopy handling with negative indices and strengthened ordinary timer-delay inputs.
  The final per-file Jev gate verified all five scoped claims but escalated
  (composite 0.6832; safe-to-apply 0.33), chiefly on review confidence.
  Independent and parent source review repaired the concrete arraycopy/input
  defects and found no remaining issue in this wrapper scope. This is manual
  review disposition, not automatic approval or whole-engine certification.
- **#37 / M1-10:** all 40 ResourceRef/GameRef registrations have native-boundary
  coverage, including both getInfo descriptors sharing one address. Fresh
  original-executable verification matches 592 native and 14 wrapper-factory
  rows; Rust compares 584 nonfault native cases and 56 returned-wrapper cases.
  Eight precisely identified crash inputs are fixed and separately ledgered;
  all other fault locations, operations and input states fail the replay.
  Loaded-stock tests cover returned VM objects, inherited Native.ptr storage,
  fresh newNative/deleteNative, reference arguments and native method roundtrips.
  Parent and independent reviews repaired multiple identity/bridge defects;
  a mutation demonstrated and corrected overly broad fault exclusion.
  Engine queries, creation, spatial results and VM string inputs remain explicit
  backend state. The localized current VM slot representation does not claim
  original physical field layout; #109 now centralizes base-first instance-only
  layout with actual script field access. Further script execution is #110.
  Final root gates escalated on low review confidence (resource composite
  0.6514/safe 0.24; copy 0.733/safe 0.46). An inaccurate "264 ResourceRef"
  shorthand was corrected: that replay also includes GameRef flags. Supplemental
  copy evidence verified the bridge/platform limits; the final fault-policy
  gate verified all three corrected claims (composite 0.794/safe 0.49), still
  requesting stronger review. Parent and independent source/ABI review plus
  fresh executable runs and eight crash-fix expectation mutations supply the
  manual disposition; no automatic gate approval is claimed.
- **#39 / M1-12:** headless capture implemented and validated: all 64 bindings
  (34 render, 10 Sound, 20 input/controller) emit schema-valid native logs.
  Fresh Unicorn runs verify 22 modeled render and 30 Sound cases. Seventeen
  render bindings capture calls with explicit deterministic result inputs;
  their engine behavior remains unimplemented and unclaimed in the ledger.
  The stream is ordered per thread; cross-thread merging and object identity
  normalization remain boot/replay work. Parent manual review found no further
  concrete defects after the Jev gate requested stronger review.
- **#12 / M0-11:** whole-game Wine oracle. Static fixes are integrated, but
  boot-to-main-menu replay acceptance has not run. Wine launches stay forbidden
  without explicit per-invocation authorization (user directive; the
  `re/hook/run.sh` gate enforces it). The cited note
  `../slr-notes/decision-m0-11-static-only.md` no longer exists on disk; the
  gate it recorded is preserved here and in the hook script. #13 / M0-12 is the
  documented Unicorn fallback.
- **#13 / M0-12:** strict startup infrastructure remains an incomplete oracle
  prefix. The default C-only scenario reaches real WinMain and requests the
  original disk-insertion dialog at startup instruction counter 466,336;
  its complete diagnostic JSON is unchanged by the media extension.
  Intercepted platform thunks increment this counter, so it does not count
  exclusively original executable instructions.
  An optional, explicitly hypothetical MCI profile now exercises the original
  CD predicate and advances to counter 3,848,705 with 9,102 events, stopping at
  the named `d3d9.dll!Direct3DCreate9` import. Two complete declared-profile
  prefixes match exactly. The profile identifies its assumed inputs and
  records `physical_media_observed: false`; no derived TOC vector is committed.
  Original CRT/game functions are not replaced and no Wine launch occurs.
  This remains platform diagnostic JSON, without an SLRLOG native stream,
  observed MainMenu, or D3D/DirectSound/DirectInput success model.
  All 48 local tests pass, including 15 original-predicate scenarios; 19
  targeted media/config/search mutations are rejected and source is restored.
  The earlier overlay-budget, mutex-lifetime and zero-byte-write repairs are
  retained. Independent root tests pass. Review also fixed aggregate directory
  bounds before adding synthetic dot entries and tested actual wrong-DLL
  rejection and stdcall cleanup for the optional WinMM import.
  A follow-up regression repairs malformed MCI OPEN diagnostics: the diagnostic
  field `kind` collided with the boundary constructor's argument. Unsupported
  callback, device-type, drive and alias inputs now raise the intended named
  stop without creating a device or writing the packet. All 59 local headless
  tests pass, and complete C-only, MCI-only and D3D-only diagnostic bytes remain
  unchanged under matching options.
  The author media gate verified all four claims but escalated on review
  confidence (composite 0.722/safe 0.30). Stronger source/ABI review and fresh
  local execution supply manual disposition. The six-file root gate also
  escalated (composite 0.7634/safe 0.37), including a low-confidence
  repeatability contradiction. Direct complete-file comparisons and focused
  raw CLI/test evidence resolved it; supplemental verification supported the
  repeatability and test facts at 0.91 and 0.86 confidence. No automatic patch
  approval or #13 completion is claimed.
  An optional hypothetical IDirect3D9 enumeration profile now continues through
  original graphics constructor/enumeration code to `user32.dll!LoadCursorA`,
  return site `0x4b7f44` (cursor resource 116). A declared single-mode scenario
  produces two 640x480 records at 16 and 32 bits, with startup counter 3,849,360.
  Complete accepted prefixes repeat byte for byte. Declared refused checks and
  empty lists produce no accepted records; declared null creation follows the
  original null read at `0x4b897c`. That enumeration slice supplies no window,
  cursor, caps or device creation model. The observed creation input 31 is
  distinct from the installed
  header macro 32; actual runtime acceptance is not claimed.
  All 58 local headless tests pass without skips, including six original graphics
  prefix runs. Sixteen isolated implementation mutations and 28 expectation
  perturbations are rejected. The C-only and MCI-only complete diagnostics remain
  byte-identical under matching options. Independent review checks all 17 COM
  slot names and argument counts against the installed SDK header, including
  stack `this`. Unsupported methods preserve the stopped call's registers and
  stack and supply no result. The five-file author gate escalated on review
  confidence (composite 0.75785/safe 0.46), including an unsupported test claim.
  Fresh root logs verify that test fact at 0.83 confidence. Source, ABI and raw
  byte comparisons provide manual disposition of the remaining uncertain
  judgments; no automatic approval or #13 completion is claimed.
  A second follow-up fixes rejected known imports: validation now precedes
  simulated stdcall return cleanup, preserving entry EIP, ESP, EAX and stack
  arguments on both named Boundary and platform-error stops. The regression
  first failed both paths; all 60 local headless tests now pass. Complete
  default, MCI-only and D3D-only diagnostics remain byte-identical. This is
  harness error-state preservation, not additional game parity.

  A separate optional hypothetical desktop profile now reads the pinned PE's
  ten cursor groups (starting at resource 116) and icon 107, supplies declared
  arrow/black-brush availability, and registers the original INVICTUS class.
  CreateWindowExA marshals a 48-byte CREATESTRUCTA and executes original WndProc
  `0x4b8000` for WM_NCCREATE. The accepted scenario stops at the still-unmodeled
  DefWindowProcA, return site `0x4b829c`, startup counter 3,849,524. One callback
  remains pending; window creation has supplied no result. Independent x86
  probes verify saved integer/flags/x87 context restoration when a synthetic
  callback returns, and direct PE reads check all eleven resource identities.
  All 71 integrated headless tests pass without skips. The author's 27
  implementation mutations and 316 expectation/stack perturbations are rejected;
  independent review checks their receipts, restored source and key failure logs.
  Four fresh integrated CLI outputs match their complete pinned captures.
  The author gate escalated on review confidence (composite 0.71955/safe 0.37).
  Root and independent source/ABI review plus actual CPU/resource probes provide
  manual disposition; no automatic approval or #13 completion is claimed.
  This remains an incomplete oracle prefix with MainMenu false; no physical
  desktop, completed window, later messages, caps or device model is claimed.

  An additional creation-message scenario can explicitly supply WM_NCCREATE's
  default result as 0 or 1. The real WndProc returns that result, then stops at
  the next unsupported creation phase (counter 3,849,530). Optional declared
  RECT-form WM_NCCALCSIZE delivery reaches its original delegation to
  DefWindowProcA at counter 3,849,551 and stops without a second default result
  or completed HWND. These are declared platform inputs, not observations of
  Windows default processing or its complete message sequence. The callback
  bridge now rejects incorrect four-word stdcall cleanup before restoring the
  suspended API context. Existing desktop inputs retain their strict stop.
  All 83 integrated tests pass without skips; root also independently runs
  the 12 added tests. All four earlier complete traces remain byte-identical,
  and the new CLI trace equals its repeated capture. The author's 20 isolated
  implementation mutations and 138 expectation perturbations are rejected;
  root checks the restored hashes and relevant ABI/default-result failure logs.
  The author gate escalated (composite 0.703208/safe 0.37), including an
  unsupported mutation claim. Source, raw execution and receipt review provide
  manual disposition; no automatic approval or whole-window parity is claimed.

  The next optional scenario declares identity RECT output and a zero default
  WM_NCCALCSIZE result. The original callback returns zero at counter 3,849,557;
  the rectangle-only scenario stops there with no pending callback. An explicit
  continuation delivers WM_CREATE using the same CREATESTRUCTA, reaching its
  original DefWindowProcA delegation at counter 3,849,578 with one callback
  pending and no default result. Neither scenario completes window creation.
  Identity output is a hypothetical zero-inset input, not observed Windows
  processing. All 91 integrated headless tests pass without skips; root also
  runs the eight focused tests independently. Seven earlier complete JSON
  outputs retain their hashes, and both new CLI outputs equal repeated captures.
  Twenty-one implementation mutations and 205 expected-value perturbations
  are rejected. The author gate escalated (composite 0.79775; safe-to-apply
  0.37), with an unexplained compound trace/hash contradiction. Independent
  source, fresh complete-byte and raw-boundary checks resolve that uncertainty.
  A later root report incorrectly combined the two scenarios' pending states;
  it was corrected to zero versus one before integration. No automatic approval,
  physical desktop, complete creation-message sequence or MainMenu is claimed.

  A subsequent optional CREATE scenario supplies declared zero or minus-one
  defaults through the original WndProc, then stops at counter 3,849,584 before
  any API result or refusal cleanup. Only an actual zero callback result and
  WS_VISIBLE style permit the explicitly requested WM_SHOWWINDOW notification.
  Original code then writes the viewport globals to `[0,0,800,600]` in the
  declared graphics scenario and reaches its unknown default at counter
  3,849,620. One callback is pending there; the three other profiles stop after
  CREATE with none pending. The shim does not write those game-state fields.
  All 98 integrated tests pass without skips. Thirteen fresh complete CLI
  outputs match their pinned captures, including all nine earlier profiles.
  Nineteen isolated implementation mutations and 534 expected-output scalar
  perturbations are rejected; profile/measurement-address inputs are excluded
  from the latter. The author gate verified all six claims but escalated
  (composite 0.7789375; safe-to-apply 0.49). Root and independent source review,
  fresh native memory readings and a separate 40-row original WndProc replay
  provide manual disposition of low-confidence state judgments. A README scope
  qualifier now distinguishes the optional continuation from the earlier stop.
  No automatic approval, completed window, physical notification sequence,
  device/caps model or MainMenu acceptance is claimed.
  The recovered unpublished window-transcript slice (declared finite
  WM_SHOWWINDOW/notification transcript plus a one-shot owned CreateWindowExA
  return) was rebuilt from the frozen recovery archive and integrated at merge
  `1497cac` after a fresh native-prefix replay: 109 tests with no skips, all 13
  prior matching-input prefixes byte-identical, and all 5 new CLI outputs equal
  to native outputs. Seven files match the frozen manifest hashes; the merged
  diff equals the frozen 52,537-byte patch. The integration gate verified all
  three claims (none contradicted) but escalated on review confidence
  (composite 0.75475; safe-to-apply 0.30); parent hash/byte checks provide
  manual disposition. Inputs remain declared hypothetical platform data, every
  trace stays incomplete with MainMenu false, and the read-only next frontier
  is ShowWindow/SW_HIDE before its API result.

- **#109:** typed persistent field and array lvalues now share authoritative VM
  host storage with vector and resource natives. The loader retains class pools,
  body owners and field initializer indices; supported numeric initializers run,
  while unsupported construction/call/literal initializers fail explicitly.
  Actual unchanged stock Vector3 multiplication and addition bodies run through
  public Thread.run. Independent root runs reproduce all 64 controlled original
  cases and pass all 21 focused Rust cases. The author workspace run passes 841
  tests across 114 suites; 565 expectation perturbations and 20 implementation
  mutations are rejected, with source restored.
  Review found and repaired pool rebinding, class replacement behind issued body
  handles, and invalid initializer indices silently becoming zero. Conflicting
  class definitions now fail; identical reloads are idempotent. The structural
  corpus test loads each file in an isolated namespace: 1,452 files, 1,451 linked,
  one existing exclusion and 1,491 class modules. It does not prove a globally
  linked stock world or select among conflicting Dialog definitions.
  Evidence review removed four unsupported structural-test credits for method
  resolution, assignability and class lookup. This correction lowers passing
  symbol counts despite new field coverage. String interning and the complete
  original object constructor remain without registered passing evidence.
  Both final split Jev gates escalated (functional composite 0.5782/safe 0.22;
  generated ledger 0.748/safe 0.49). Low-confidence test and mutation contradiction
  labels supplied no concrete defect. Parent and independent source review,
  exact source hashes, raw successful logs, behavioral mutation failures and
  fresh original replay resolve those labels through manual review. No automatic
  patch approval or complete VM certification is claimed.
- **Clock helper:** `CounterClock` now implements `0x5516c0` over explicit
  performance-counter query callbacks. Independent original execution matches
  108 cases: nine inputs across three x87 precisions and four rounding modes.
  Rust compares the raw extended return, persistent frequency/start/now qwords
  and API query order. Both BOOL results are ignored, matching the original;
  failed queries without writes retain old output words. Start is fixed by this
  helper, correcting the prior comment that each read advanced it.
  Twelve implementation defects and 432 altered return/state expectations are
  rejected, with restored source and fixture bytes. Missing, empty, partial and
  duplicated vector controls are checked. The seven-file gate verified all four
  scoped claims but escalated (composite 0.76746/safe 0.42). Independent source,
  instruction, original-run and behavioral-failure review provides manual
  disposition; no automatic approval is claimed.
  `CounterClock.initialize` also ports `0x551690`: frequency and start are
  queried, the old now word is preserved, and the return is always zero.
  Thirteen original sequences contain 19 helper calls, including reinitialization
  and reads after failed queries. Both Rust clock replays pass; nine initializer
  implementation defects and 95 altered output expectations are rejected.
  Empty, partial, duplicate and shortened sequences fail, and absent initializer
  vectors prevent ledger collection without replacing prior passing evidence.
  The four-file initializer gate verified all four claims but escalated on
  review confidence (composite 0.796/safe 0.48). Independent original instruction,
  source, replay and mutation-log review provides manual disposition. A real
  macOS counter provider and connecting these helpers to LangHost's existing
  supplied-reading seam remain open. Complete scheduler and boot fidelity are
  not established.
- **#110:** a bounded execution slice now runs script frames, native returns,
  dynamic overload selection, constructor routing, typed arrays and argument
  copies, resource/String literals, and runtime casts/type checks. String
  literals and native producers share actual per-VM heap objects and ptr
  storage. Invalid UTF-8 literal bytes remain bytes; wrong-kind or signed-negative
  literal indices yield typed null, while absent nonnegative indices error.
  Independent review repaired subclass ptr access, byte decoding and these pool
  distinctions. The shared numeric formatter moved without algorithm changes.
  Fresh root replay matches 107 original rows across 17 groups; 24
  focused Rust tests pass without skips. The author workspace run passes 864
  cases across 118 libtest reports. Twenty-six implementation mutations and 13
  vector perturbations are rejected; missing fixtures prevent collection and
  preserve prior evidence. Sixteen new comparison registrations increase the
  collector to 402 rows without expanding helper-symbol passing claims.
  Review also retained the interner's untested annotation at its new VM host
  location; moving the implementation must not erase that remaining obligation.
  Split source gates and the final repair gate requested stronger review
  (final repair composite 0.7018; safe-to-apply 0.34). Root and independent
  source review, exact manifests, fresh original replay and public tests supply
  manual disposition; atomic factual verification supports the counts and
  controls. No automatic patch approval or full interpreter claim is made.
  A further bounded initializer slice now follows base/interface declaration
  order and installs each typed-zero field immediately before its body. Class
  initialization precedes base-first instance allocation. Defining-class pools,
  runtime receivers, static self-descriptor skipping and diagnostic-plus-raw-copy
  assignment follow controlled original observations. Genuine execution errors
  preserve the field prefix and current writes without invented sticky poison.
  Root replay reproduces all 50 original rows across eight groups; Rust compares
  49 distinct observations, excluding recursive instance self-new behind the
  original supplied allocator. All 22 initializer and six wrapper tests pass.
  Fourteen source mutations and 14 expectation perturbations are rejected;
  25 evidence controls pass their expected outcomes. Review repaired an input-manifest
  validation gap; missing or invalid evidence preserves prior collector output.
  The author workspace run passes 886 cases across 119 libtest reports. Twenty-eight
  new comparison registrations bring collection to 430 rows and add bounded
  passing evidence only for the class-initialization and allocation drivers.
  The final source gate verified all four claims but requested stronger review
  (composite 0.72278; safe-to-apply 0.31). Parent and independent source review,
  exact frozen hashes, fresh original replay and public tests supply manual
  disposition; no automatic approval or whole-driver certification is claimed.
  Later-field failure comparisons assert partial state and error presence:
  the original invalid-memory stop and Rust managed error are not physically
  identical. Original unwind, rollback and continuation are unproved.
  Full initialization remains open: current-body preflight still guards argument
  arrays, constructor/direct calls and array/type-test families. Earlier fields
  and current zero survive rejection, but the rejected body's instruction prefix
  is not executed. A configurable depth bound limits nested field bodies.
  Supplied/stale Type5 caches, versioned global class selection, full array-class
  loading, active String GC, scheduler completion and machine CallInfo context
  remain unimplemented. These boundaries still block stock-script boot;
  #110 remains open.
- Then **#40 / M1-13** and the subsystem/parity work in `docs/plan.md`. A
  successful unit-vector run cannot close the boot ticket.

The current native patches are integrated for final source/evidence review.
Ticket closure is scoped to their native-boundary acceptance; stock boot remains open. The
118 untested port rows also remain work; no scoped exception has been granted.

## Tooling and provenance

- `re/ghidra_scripts/ExportAll.java` exports **4,356** non-external, non-thunk
  entries to ignored `re/out/`. Index field three is body-address count, not
  a contiguous size from which an end address can be inferred.
- `re/build_native_table.py` retains the 360 registrar-site check and 353-row
  Ghidra pseudocode table. Seven Controller registrations are separately known
  but absent from that reference table.
- Original exe SHA-256:
  `7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde`.
  `reference/` and `stock/` stay read-only; derived artifacts stay ignored.
- TUFA findings are in `docs/tufa-executor.md` and format docs. The old progress
  page's "no differential test yet" statement was stale.
- `re/progress.py` separates exact-symbol and canonical-address coverage and
  counts every parity row, making this assessment reproducible.

## Kuna evaluation (1.906)

Verdict: **incorporate as a second opinion and address lookup tool**. Retain
Ghidra's export, inventory, names, and native-table builder. The executable
oracle remains the behavioral authority. No Ghidra installation was replaced.

The CLI reports `kuna 1.906`; its embedded skill was installed with
`kuna install-skill --agent codex` at
`~/.codex/skills/kuna-decompiler/SKILL.md`. Source tooling is in `re/kuna/`;
generated data is in `re/out/kuna/`.

### Recovery and budgets

| Experiment | Result |
|---|---|
| Default `kuna functions --json` | 2,076 entries |
| Exact address overlap with Ghidra | 1,568 shared; 2,788 Ghidra-only; 508 Kuna-only |
| Declare every Ghidra entry; 3 s/function export | All 4,356 attempted; 4,352 emit C; four timeouts |
| Initial recognized diagnostics | 95 C bodies with warn/jump-as-call markers; 4,257 without these markers |
| Retry the four errors at 20 s/function | All four emit C; one carries diagnostics |
| Combined observations | 4,356 emit C; 96 carry recognized diagnostics; 74 contain jump-as-call; no remaining emission failure |

Initial failures: `FUN_00408f60`, `FUN_00412b10`, `FUN_00458c00`,
`FUN_00487f20`. Original attempts and successful retries remain separate local
records. Commands and lookup usage are in `re/kuna/README.md`.

Initial bounded batches took about 93 seconds in aggregate; peak sampled
process-group RSS was 684,640 KiB. This is one local measurement, not a speed
comparison with Ghidra. C emission for every reference entry does **not** prove
complete control-flow recovery or exact decompilation. The extra 508 default
Kuna entries have not been independently classified as real functions.

### Hard-target checks

- `setPriority` at `0x47c790`: exposes the expected field +0x28 store, useful
  corroboration of the native wrapper disassembly.
- `addTimer` at `0x47e1c0`: exposes the world-time addition and 0x48b750 call.
- `Thread::run` at `0x420ff0`: Kuna represents three opcode-table dispatches
  as indirect calls followed by returns, marked jump-as-call; Ghidra recovers
  switches. The original instructions at 0x4210e5 and 0x422e91 are indirect
  **jumps**, checked with capstone. This C cannot be the sole porting spec.
  Jev's comparison was uncertain overall and on control flow, while its
  table-reconstruction aspect leaned toward agreement; actual instructions
  resolve the narrow jump-versus-call question.
- Physics 0x428450, 0x4a5190, 0x4a7c10 emit C exposing step, accumulator,
  and integration arithmetic. Their long-double expressions do not encode
  x87 PC=24 rounding. This is static corroboration only: neither decompiler
  is a float oracle. Use `docs/x87.md` and independent oracle legs.

### Integration limits and follow-up

REA 6.7.0's Kuna adapter rejects PE (its admitted formats are ELF/thin Mach-O),
although the CLI handles this PE. Kuna's memory watchdog also rejects macOS
because its resident-set implementation is unavailable. The wrapper therefore
uses an external process-group RSS/wall-time watchdog and preserves failed
addresses. Nine data-free tooling tests cover missing/error output, diagnostic
dispatch, address mapping, noncontiguous bodies, stalled-process/worker termination, and recovery from an interrupted inventory.

Upstream risks were rechecked on 2026-10-19: issues
[#299](https://github.com/Noelo-Lab/kuna/issues/299),
[#1157](https://github.com/Noelo-Lab/kuna/issues/1157),
[#1180](https://github.com/Noelo-Lab/kuna/issues/1180), and
[#1034](https://github.com/Noelo-Lab/kuna/issues/1034) were open. These are
reported risks, not claims that each reproduces in this game.
The upstream [headless instructions](https://github.com/Noelo-Lab/kuna#headless-ghidra--scripting)
describe swapping kuna_ghidra and report testing with Ghidra 12.1.2.
The local workflow leaves Ghidra 12.1.4 intact.

Follow-up: [#106: validate jump-table recovery and diagnostic functions](https://github.com/nicolas-found42/street-legal/issues/106).

Review of this checkpoint found and fixed orphaned-worker cleanup and invalid
inventory-cache resume defects. The gate verified the numerical/test claims but
requested stronger review; the independent code review supplied these concrete
fixes. It did not certify whole-game completion.
