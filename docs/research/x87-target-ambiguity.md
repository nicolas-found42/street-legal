# G11: raw x87 target ambiguity

This is a target-definition and evidence question for eventual review. It does
not change the contract, accept G11, or describe the current boot blocker.

## What the fresh experiment establishes

At two exactly representable inputs, the independently compiled, pinned Bochs
standalone FCOS model and actual x86 FCOS instructions translated by Rosetta
produce adjacent extF80 values. Both values meet Intel's stated FCOS accuracy
bound at those inputs. The unchanged original build-606 function under Unicorn
produces a third value outside that bound. No physical Intel CPU ran.

| FCOS input (exact) | Bochs signed error, extF80 ulps | Rosetta signed error, extF80 ulps | Original under Unicorn |
|---|---:|---:|---:|
| 1/4 (`m32 0x3e800000`) | −0.514941974 | +0.485058026 | Outside ±1 ulp |
| 11/16 (`m32 0x3f300000`) | −0.484494362 | +0.515505638 | Outside ±1 ulp |

The error signs above are **computed minus mathematical value**. Each pair is
exactly one extF80 ulp apart. All three values round to the same m32 store in
these examples. The exact raw images, original observations and rational bounds
remain local in the [evidence receipt](../../re/out/checkpoints/mandate-20261022T072322Z/x87-target-ambiguity/evidence-replay.json);
no oracle vectors are copied into this document.

The referee uses exact rational arithmetic: for x∈{1/4,11/16}, the 32-term
alternating cosine sum is a lower bound, and adding x^64/64! gives an upper
bound. The enclosure width is below 2^-300. Both candidate errors are proved
strictly inside ±0.82 ulp; Unicorn's errors are proved outside ±1 ulp. These
claims use rational inequalities, not the rounded numbers displayed above,
mpmath, a host libm referee, or a comparison tolerance.

## Primary authority and pinned executions

Intel SDM Volume 1, order **253665-078US, December 2022**, §8.3.10, printed
pages 8-21–8-22 (PDF pages 223–224), specifies “less than 1 ulp when rounding to
the nearest (even)” for reduced transcendental arguments. It additionally gives
an FCOS error bound of 0.82 ulp for |x|<1.31. Both chosen inputs are inside that
domain. The local [PDF](../../re/out/checkpoints/20261021-campaign/x87-research/intel-sdm-vol1.pdf)
has SHA-256 `7aca6a3e847520c4a1b2f1230da74ae3837757bc49288f5c895a6b003371d051`;
its text extraction is hash-bound in the receipt. This proves that the two
images both meet the cited **value-accuracy condition for these inputs**; it
does not certify either implementation's complete SDM semantics, flags or
monotonicity over its whole domain.

- **Bochs model:** standalone FSIN/FCOS at upstream commit
  `d245c751e6a6fab4bf68e72823598a48001adb3c`, independently compiled from the
  upstream source. The full source inventory and compiler/harness identities
  are retained in the receipt. Its `fsincos.cc` SHA-256 is
  `4544bf8d862188edff834fd88a3186ad00360426d068f0dcecae07ff072c99ff`.
- **Rosetta observation:** an x86_64 Mach-O probe executes FNINIT, FLDCW
  `0x007f`, FLD m32, and standalone FCOS, then stores the complete ten-byte
  register image. `sysctl.proc_translated=1` was observed. This is Rosetta 2 on
  macOS 27.0.1, build **26A434**; the probe and all three runtime artifacts are
  SHA-256-bound in the receipt. It is translated instruction execution, not
  physical Intel execution and not the stock PE running under Wine.
- **Original observation:** unchanged build-606 `54ecd0` executes under
  Unicorn **2.1.4** with CW `0x007f`; a read-only hook at `54ecdf` observes the
  result of its real standalone FCOS. The original executable SHA-256 is
  `7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde`.
  The probe, oracle producer and loaded Unicorn library are also hash-bound.

The original-under-Unicorn result is consistent with its documented
binary64 host-sin/cos helper, whose narrowed result loses extF80 low bits. That
explains this oracle's observed precision deficit; it does not make either
other result a proved original-hardware answer.

Across the earlier 9,815-word corpus, all 332 differing scalar results in 308
Rosetta/Bochs rows differ by exactly one extF80 ulp. Testing a shared FSINCOS
path or substituting 64-bit/128-bit Pi does not explain those differences;
[retained experiments](../../re/out/checkpoints/mandate-20261022T072322Z/float-investigation/FINDINGS.md)
record the falsified hypotheses. No claim is made that physical Intel CPUs
necessarily differ this way: neither compared implementation is a physical CPU.

## Governing decision that remains unresolved

[ADR 0007](../adr/0007-float-model-bugfixes-platform.md) requires bit-exact x87 transcendentals.
[ADR 0008](../adr/0008-local-oracle-crates-natives.md) prohibits x86-machine execution, calls
for local triangulation and resolving disagreements from the SDM, and pins a
documented implementation. The [completion contract](../fidelity.md#exception-evidence-does-not-relax-the-contract)
explicitly says that pinning alone does not prove original parity; G11 cannot
pass on tolerance-based approval. The older tolerance-pinning language in
[x87.md](../x87.md) supplies no exception to that governing requirement.

Here the SDM accuracy condition admits two different raw images, so it cannot
uniquely select one. SoftFloat's basic arithmetic does not supply an independent
FSIN/FCOS implementation to break this tie. The unresolved issue is the
normative raw scalar target and evidence of its identity, rather than a reason
to choose whichever result makes a test pass. An algorithm pin, more sampling,
or successful m32 consumers alone cannot establish universal physical-hardware
identity or close the raw G11 obligation.

The bounded Euler memory path remains separate: it encloses the pinned model
and the exact declared Unicorn-host provider point and admits only unique m32
stores. Its full-consumer evidence can advance boot while G11 remains open.

## Exact obligations still open

The canonical [G11 inventory](../acceptance-inventory.md#cross-cutting-gates)
and completion contract still require:

- A justified operation-trust result and bit-exact implementation for every
  affected transcendental, pinned to documented semantics with original-parity
  evidence. The current `APPROX` FSIN/FCOS, FPATAN, F2XM1 and FYL2X evidence
  does not establish this.
- Completed triangulation and disagreement resolution, including the relevant
  main-thread/worker control state and required observable status. These two
  value examples establish ambiguity; they do not approve either model.
- Fresh complete function and affected-scenario comparisons using the resolved
  reference. No new tolerance, omitted raw observation, unsupported case, stale
  receipt or bounded consumer result may be relabelled as the missing scalar
  parity proof.
- Executable G11 evidence validation bound to the frozen source, original
  executable/stock, oracle/profile, inputs, tools and comparison policy, then
  fresh independent S13 execution. This decision package supplies no passing
  G11 or whole-game receipt.

## Smallest material decision/access needed

The parent should request a decision only when appropriate while independent
implementation continues. Two explicit resolutions are distinguishable:

1. **Preserve a physical-hardware interpretation:** identify the reference CPU
   family/model/stepping and microcode, and authorize bounded probe evidence
   from that target, an explicit exception to ADR 0008's no-x86-machine rule.
   The evidence must execute the original instruction/function at the required
   control states, retain raw results/status and CPU identity, and test the
   proposed implementation against that named target. A small triggered probe
   is the first needed observation; it is not sufficient whole-G11 acceptance.
2. **Define a versioned software target:** explicitly choose the normative x87
   implementation and revise the governing target definition. Bit-for-bit
   comparisons and all whole-game obligations would still be required against
   that definition. This is a material fidelity decision; it must not be
   presented as proof of original physical-hardware parity, an inferred
   permission, a tolerance adjustment, or automatic G11 acceptance.

No option is selected here. Access to an unnamed Intel machine would not by
itself define the target, and defining a target would not replace the remaining
implementation, full-corpus verification or frozen-candidate S13 execution.

## Reproduce locally

The complete local package contains the rational referee, original-function
capture and translated probe source, together with all dependency hashes.
Use a fresh output directory to preserve existing receipts. Existing inputs
and hashes are in the linked receipt; the two input words are specified above.

```sh
python3 re/x87probe/compare_intel_trig.py --upstream re/out/checkpoints/20261021-campaign/x87-research/Bochs --out re/out/x87-ambiguity-upstream-fresh
clang -arch x86_64 -O0 -Wall -Wextra -Werror re/out/checkpoints/mandate-20261022T072322Z/x87-target-ambiguity/rosetta_probe.c -o re/out/x87-ambiguity-rosetta-fresh
/Users/nicolas/slr-oracle-venv/bin/python re/out/checkpoints/mandate-20261022T072322Z/x87-target-ambiguity/reproduce_ambiguity.py --bochs-build re/out/x87-ambiguity-upstream-fresh --rosetta-probe re/out/x87-ambiguity-rosetta-fresh --out re/out/x87-ambiguity-fresh.json
```

The producer refuses reused output paths. Use another fresh name for each run.
The parent has not requested a user decision or changed acceptance.
