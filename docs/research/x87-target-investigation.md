# Experimental x87 target investigation

Keep the extracted experiment diagnostic-only and archive it from production
consideration pending the evidence below. It can test whether a Rust adaptation
matches a particular upstream algorithm; that is useful investigation, but does
not currently justify changing the game's floating-point provider or passing G11.
This document changes no ADR, tolerance, acceptance obligation or runtime path.

## Numerical target under the existing decisions

[ADR 0007](../adr/0007-float-model-bugfixes-platform.md) requires bit-exact x87
transcendentals and the game's thread-specific control state. Its default is
PC24/round-to-nearest on the device thread, with PC53 on workers; the relevant
state must be observed at the operation, not inferred from the process default.
[ADR 0008](../adr/0008-local-oracle-crates-natives.md) explicitly amends the
physical-x87 comparison proposal: execution stays on the Apple Silicon Mac,
with SoftFloat, Unicorn and Rosetta triangulation, disagreement resolution from
the Intel SDM, and a documented implementation pin for transcendentals.

The normative project target is therefore the existing local, triangulated,
documented software reference, subject to operation trust and original-executable
parity. It is not an unnamed physical Intel CPU, nor automatically Bochs,
Unicorn, Rosetta, correctly rounded mathematical sine/cosine, or the value most
convenient for an existing test. An exact implementation/operation trust decision
still has to be justified and recorded. Selecting a pin identifies an algorithm;
it does not establish original parity. The
[fidelity contract](../fidelity.md#exception-evidence-does-not-relax-the-contract)
expressly preserves that distinction and disallows tolerance-based approval.

There is a real unresolved limitation, rather than a permission to choose:
where the permitted software references disagree and the SDM admits both raw
results, the SDM cannot resolve their low bits. Do not reinterpret ADR 0008 as
proof that every disagreement is decidable from its accuracy bounds. Preserve
the failed comparison and leave that operation untrusted. Hardware access would
require a separate authorized departure from ADR 0008 and a named target; this
investigation neither needs nor requests that change.

[docs/x87.md](../x87.md) contains older statements that physical confirmation
remains open, tolerance-pinned transcendentals suffice, and transcendentals
ignore PC/RC. Those statements are historical implementation/probe reports,
not amendments to the later acceptance contract. Physical silicon is not a
prerequisite silently restored by that document; tolerance does not meet G11;
and individual instruction semantics must be checked rather than generalized
from observed PC/RC independence in a corpus.

## Primary references and their limits

The locally preserved Intel SDM Volume 1 is order **253665-078US, December
2022**, SHA-256
`7aca6a3e847520c4a1b2f1230da74ae3837757bc49288f5c895a6b003371d051`.
It lives under the source worktree's ignored
`re/out/checkpoints/20261021-campaign/x87-research/intel-sdm-vol1.pdf`.
Its identity and text were inspected for this investigation; this is not a new
execution receipt. [Intel's published Volume 1](https://cdrdv2-public.intel.com/789574/253665-sdm-vol-1.pdf),
§§8.3.8 and 8.3.10, distinguishes instruction approximations from mathematical
functions. It describes a 66-significant-bit internal Pi and argument-dependent
error growth, especially near reduction cancellation and at large arguments.
For reduced arguments the round-to-nearest error is below one ulp; FCOS has the
more specific 0.82-ulp double-extended bound for |x| < 1.31. These are accuracy
bounds, not an algorithm or unique result-bit specification. They do not by
themselves certify flags, stack behavior, or every operand class. Intel also
[explains these instruction/mathematical-function differences](https://www.intel.com/content/www/us/en/developer/articles/technical/the-difference-between-x87-instructions-and-mathematical-functions.html).

[Berkeley SoftFloat's documented API](https://www.jhauser.us/arithmetic/SoftFloat.html)
provides extF80 arithmetic and rounding/exception support. Its basic arithmetic
leg is valuable but does not provide an independent FSIN/FCOS implementation
to break a transcendental tie. A high-precision mathematical referee can bound
accuracy; it does not identify a particular instruction approximation's bits.
Rosetta's execution of x86 instructions is translated execution. A standalone
Mach-O probe is neither a physical CPU observation nor the original PE under
Wine. Original PE execution under Unicorn is executable-backed evidence, but
its float results are trustworthy only for operations whose emulator semantics
have passed the required trust work.

The preserved upstream checkout is at Bochs commit
`d245c751e6a6fab4bf68e72823598a48001adb3c` (HEAD inspected locally and source
checked against the [immutable upstream file](https://github.com/bochs-emu/Bochs/blob/d245c751e6a6fab4bf68e72823598a48001adb3c/bochs/cpu/fpu/fsincos.cc)).
Its distinct standalone FSIN/FCOS and FSINCOS paths must not be substituted for
one another. Upstream hardware-equivalence comments are upstream claims, not
this project's executable-backed proof. The repository carries an
[LGPL 2.1 license text](https://github.com/bochs-emu/Bochs/blob/d245c751e6a6fab4bf68e72823598a48001adb3c/LICENSE);
`fsincos.cc` separately retains the older SoftFloat extension notice with its
own attribution/derivative-work terms. Preserve per-file notices and dependency
provenance. Do not assume the project's MIT/Apache licensing covers a translation
or vendored dependency. Keep upstream builds outside the shipped workspace;
any future production adaptation needs a specific provenance/license review.

## What the preserved experiment does and does not establish

The older `docs/research/x87-target-ambiguity.md` in
`/Users/nicolas/.codex/worktrees/native-abi-continuation/street-legal` reports
adjacent extF80 Bochs and Rosetta FCOS results at the exactly representable
inputs 1/4 and 11/16, both inside the cited accuracy bound. It also reports an
original build-606 FCOS result under Unicorn outside that bound. Its rational
referee and archived receipts are useful reproduction inputs. Those historical
numerical claims have not been freshly executed by this document's author;
they are not promoted to a current trust result. In particular, the older
claim that a new software-target contract must be chosen is too broad: ADR
0008 already establishes software triangulation and pinning. The missing piece
is a defensible per-operation identity/trust result satisfying the unchanged
original-parity requirement.

The preserved `compare_intel_trig.py` checks adaptation equality to the pinned
Bochs build, not original parity. Its generated corpus is finite m32 inputs;
its harness selects PC24/default nearest rounding, emits raw ten-byte sine and
cosine results, and combines both C2 return indicators into one bit. That scope
does not cover arbitrary extF80 inputs, all required control states, complete
status/exception flags, independent per-instruction C2, pending exceptions,
stack effects, or full original consumers. A zero-mismatch result therefore
cannot close G11. Likewise, a bounded Euler path yielding one uniquely possible
m32 store can establish that bounded store only; it cannot establish raw scalar
agreement, unaffected consumers, or whole-game physics parity.

## Required evidence before a production proposal

An integration proposal must contain all of these, with unsupported observations
reported explicitly and every failed attempt retained:

1. **Frozen inputs and rebuildable source.** Record the candidate Git revision,
   tracked patch and untracked source archive, input generator and corpus hashes,
   original build-606 executable hash/size and stock manifest identity. Preserve
   the actual source bytes needed to reproduce the candidate; hashes alone are
   insufficient. Include wrapper, generated config, native/C/Rust source,
   dependency sources and pin, build scripts and comparison policy. Hash inputs
   before and after execution and reject concurrent mutation.
2. **Observed execution profile.** Name the original address, exact instruction
   bytes, operand/load/store widths and evaluation order. Record complete input
   CW (PC, RC, masks), initial status/stack/tags, observed output CW, raw extF80
   images and memory words, output status/exception/condition flags and stack
   effects. Observe each operation separately; a combined C2 or rounded m32
   word cannot replace missing raw state. Cover the actually used device and
   worker profiles and document any unreachable operand classes with evidence.
3. **Oracle and tool identity.** Record Unicorn library bytes, SoftFloat build
   and specialization, Bochs revision/source inventory, compiler/linker flags,
   generated harnesses, binaries, OS build, Rosetta translation observation,
   and Wine/hook/shim profiles when used. Retain exact commands, exit codes,
   complete stdout/stderr and output hashes. A direct Mach-O probe and a hooked
   original PE are separate profiles. No silent fallback or cached success.
4. **Operation trust and disagreement dossier.** For each affected operation,
   state the trusted reference, instruction semantics, supported domain, raw
   comparisons and primary SDM revision/sections. Independently test encoders,
   signed zero, NaN/infinity/subnormal handling, range reduction boundaries and
   rounding ties. Retain raw disagreement witnesses. Bound mathematical error
   separately from raw implementation equality; no new tolerance or NaN
   normalization may hide an unresolved result.
5. **Strict original consumers and acceptance.** After resolving trust, rebuild
   the unchanged original-function oracle and candidate from frozen inputs,
   compare complete outputs at zero ulp on trusted operations, and rerun all
   affected deterministic scenarios and their required tiers. Bind results to
   the immutable candidate and complete endpoint, not a common prefix. Then
   perform the independent frozen-candidate verification required by S13. An
   absent fixture, skipped case, unknown flag, unavailable profile or stale
   receipt is missing evidence, never a pass.

## Disposition

**Archive the experimental provider from production; retain its diagnostic
sources and reproduction tooling on this investigation branch.** Its immediate
use is to expose model/oracle disagreements and measure adaptation fidelity.
No present evidence package establishes all affected operation trust, strict
original-function equality, or complete scenario acceptance. Do not wire it into
`slr-x87`, `slr-lang`, boot, or physics, and do not relax G11 to obtain a green
comparison. A future proposal can be reconsidered when the checklist above is
satisfied; if the references remain irreducibly inconsistent, record the
limitation and keep the experiment archived under the existing contract.

## Fresh diagnostic replay on this branch (2026-10-25)

The parent reran the extraction against main `edaabaf` with the source-hash
pinned model, a freshly compiled independent Bochs build at the pin above,
the clean `stock/StreetLegal_Redline.exe`, and freshly compiled x86_64 inline
assembly under observed Rosetta translation. The retained local package is
`re/out/x87-target-investigation-20261025/`:

- `model-2/receipt.json`: 9,815 finite-m32 adaptation comparisons, zero
  mismatches against Bochs. This proves sampled adaptation equality only.
- `ambiguity-final.json`: fresh execution of original function `0x54ecd0`,
  observing its FCOS register at `0x54ecdf`, CW `0x007f`, at 1/4 and 11/16.
  Exact rational enclosures reproduce adjacent Bochs/Rosetta extF80 values
  inside 0.82 ulp and the original-under-Unicorn value outside one ulp.
  All images share the same m32 rounding bin. Raw register images, original
  memory, observed status words, executable/tool/source hashes and commands
  remain ignored local evidence, not committed vectors or an operation pass.
- `negative-control.log`: a changed candidate-source hash is rejected before
  any result is written. The producer also refuses reused output paths.
- The rational model majorant proof passes against the unchanged model hash.
  Five operand-encoder tests and strict-fixture `slr-x87` tests pass, as do
  diagnostic-package Clippy and formatting. Existing approximate tests do
  not become strict transcendental evidence by passing.

The first independent comparison attempt (`model/`) could not link the
standalone debug candidate: SoftFloat's inline C helpers need the optimized
build configuration used by main. Its failed attempt is retained. The isolated
package now explicitly uses dev optimization level 1; `model-2` is the successful
fresh attempt. No production build setting changed.

This replay resolves the disposition: **retain the diagnostic archive; do not
ship the candidate**. It does not resolve a unique trusted raw implementation,
prove PC53/other rounding profiles or full status semantics, execute the stock
PE under Wine, or satisfy G11/S13. The provenance checklist above remains the
minimum for reconsidering production.
