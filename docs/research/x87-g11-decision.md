# G11 decision file: x87 transcendentals cannot be pinned without Intel silicon

Status: **blocked on a user decision and on device access.** Nothing here relaxes ADR 0007, ADR
0008, ADR 0009 or G11. `APPROX` operations stay `APPROX`; no tolerance was changed.

## Question

ADR 0007 requires bit-exact x87 transcendentals "pinned to a documented implementation". ADR 0008
names the triangulation: Berkeley SoftFloat, Unicorn, Rosetta 2, with disagreements settled by the
Intel SDM. The SDM only bounds the error of `FSIN`, `FCOS`, `FPATAN`, `F2XM1` and `FYL2X`; it does
not define result bits. So where the legs disagree inside the bound, the SDM cannot decide.

## What the original exe uses

Capstone sweep of `StreetLegal_Redline.bak.exe` (linear, so counts include some mis-decoded data;
game-code range `0x401000..0x59f000`):

| instruction | sites in game code | note |
|---|---|---|
| `fsin` / `fcos` | 84 / 83 | trig of angles throughout |
| `fpatan` | 50 (+3 CRT) | Pacejka tyre model (`0x43ba80`, `0x43bb30`), slip angles |
| `f2xm1` + `fscale` + `frndint` + `fldl2e` | 5 | the `exp` inside the Pacejka load term |
| `fyl2x`, `fprem`, `fptan`, `fxam` | CRT only | `log`/`pow`/`fmod`/`tan` |

All transcendentals execute at x87 control word `0x007F` (PC24, round-to-nearest) per ADR 0007;
the SDM says PC does not apply to them (Rosetta and Bochs both confirm PC-independence here).

## Historical evidence from the original investigation

Harnesses are in `re/x87probe/g11/` (tooling only, outside the workspace). Three result sources
run over one deterministic, synthetic corpus (no original data): `corpus.in`, 1,043,510 records,
sha256 `620111abc1e3e0967345c356de945d1f978d0a33f309c50814e6a0091a3870a4`, from
`python3 re/x87probe/g11/make_corpus.py --out DIR --n 20000`.

- `bochs-stream`: the pinned Bochs FPU (`d245c751e6a6fab4bf68e72823598a48001adb3c`). Its source
  states that it models the hardware's internal roundings so as to match the last bit. That is an
  upstream claim, not ours.
- `x87-stream`: the same instructions as host x87 inline assembly in an x86_64 binary. Run here
  under Rosetta 2 (`sysctl.proc_translated = 1`, Apple M5 Pro). **Not Intel silicon.**
- `slrx87`: the shipped `slr-x87` `approx_*` functions (`crates/slr-x87/examples/stream.rs`).

Raw outputs: `/Users/nicolas/Documents/street-legal-evidence/x87-g11-20261026T2230Z/validator-runs/g11-2c8c768fcb8b/` (a full `acceptance/g11.py` run).

### Bochs vs Rosetta, bit for bit on the 80-bit result (in-domain rows)

| op | rows | identical 80-bit result | identical after rounding to f32 (what `FSTP m32` stores) |
|---|---|---|---|
| fsin | 220,065 | 94.6% | **100%** |
| fcos | 220,065 | 96.4% | **100%** |
| f2xm1 (\|x\| <= 1) | 120,031 | 92.1% | **100%** |
| fyl2x (x > 0) | 120,384 | 91.0% | **100%** |
| fpatan | 120,780 | 94.3% | **100%** |
| fscale | 120,780 | **100%** | 100% |
| frndint | 120,065 | **100%** | 100% |

C2 agrees on every row. Out-of-domain operands (`F2XM1` with \|x\| > 1, `FYL2X` with x <= 0) are
SDM-undefined and reported separately by the tool; Bochs returns the operand, Rosetta computes.
`FYL2XP1`, `FPTAN` and `FSINCOS` also differ; see `adjudicate.json`.

So the two non-silicon references are **not** the same implementation: 5-9% of 80-bit results
differ, by a few ulp of the 64-bit significand. They agree after the f32 rounding in all
1,043,510 rows. Neither is Intel silicon, and a disagreement cannot be resolved from the SDM.

### Each leg against a correctly rounded 200-bit mpmath reference

Aggregated over 24,000 mild-range vectors per op (`|x|` below 2^30; `re/x87probe/g11/run_compare.py`):

| op | Bochs exact | Rosetta exact | both exact | neither exact |
|---|---|---|---|---|
| fsin | 94.9% | 93.0% | 91.9% | 4.0% |
| fcos | 94.5% | 93.3% | 92.6% | 4.8% |
| f2xm1 | 93.5% | 98.2% | 91.9% | 0.2% |
| fyl2x | 92.6% | 95.8% | 89.7% | 1.2% |
| fpatan | 97.4% | 96.3% | 94.1% | 0.3% |

Neither leg is uniformly closer to the correctly rounded value, and neither is correctly rounded
(Intel hardware is not correctly rounded either), so accuracy cannot choose the pin.

### The shipped `slr-x87` `approx_*` against both references (f32-store level)

For f32/f64-class operands up to magnitude 2^30, and for in-domain `f2xm1`/`fyl2x`/`fpatan`,
`slr-x87` rounds to the same f32 as both references on every sampled vector except a few dozen special-operand rows (NaN/infinity payload conventions). At the 80-bit level it
matches almost never (73 of 200,046 `fsin` rows), as its documentation says.

It is **materially wrong for large arguments**: for f32 inputs with magnitude 2^30..2^127 only 14%
of `fsin`/`fcos` results match even after rounding to f32 (Bochs and Rosetta agree with each other
on 100% of those rows). The cause is the 66-bit internal pi of the x87 reduction, which libm in
f64 does not reproduce. Whether the game ever reaches such angles is not established.

### `fscale` and `frndint`: closed

Both are exact operations (no transcendental approximation). Bochs, Rosetta and the new
`X87::fscale` (`crates/slr-x87/src/fscale.rs`; `frndint` already existed) agree bit for bit on
120,780 and 120,065 vectors under all four rounding controls and PC24/53/64, including NaN
payloads, infinities, zeros, denormals and overflow/underflow rounding. `acceptance/g11.py`
reports both `pinned_bit_exact`.

## What an Intel run would need

Only a physical Intel x87 can say which implementation (if either) is bit-exact. A run on Apple
Silicon, including through Rosetta, is not it; the tool records `sysctl_proc_translated` and the CPU
brand/vendor so that cannot be confused.

1. An **Intel** Mac (x86_64 CPU; `GenuineIntel`). AMD silicon implements `FSIN`/`FCOS` differently
   and would need a separate named target. Name the target CPU generation in the record.
2. Files to copy over: `re/x87probe/g11/x87_stream.c`, `make_corpus.py`, `xcorpus.py`, `adjudicate.py`.
3. On that Mac (Xcode command-line tools and python3; no Rosetta involved):

   ```sh
   python3 make_corpus.py --out out --n 20000      # prints sha256; must equal the one above
   clang -arch x86_64 -O0 -o x87-stream x87_stream.c
   ./x87-stream < out/corpus.in > out/intel.out 2> out/intel.stderr
   ```

   `intel.stderr` must show `"sysctl_proc_translated":0` and `"cpu_vendor":"GenuineIntel"`.
4. Return the whole `out/` directory (`corpus.json`, `intel.out` = 1,043,510 x 22 bytes = 22.9 MB, `intel.stderr`).
   Format: per input record (21 bytes: op, 10-byte operand `a`, 10-byte operand `b`) one 22-byte
   record: 10-byte result, 10-byte second result (`FSINCOS`/`FPTAN` only), 16-bit status word.
5. Back here: `python3 re/x87probe/g11/adjudicate.py corpus.json corpus.in bochs=bochs.out
   rosetta=rosetta.out intel=intel.out` prints, per op and per source spec, which model equals the
   silicon bit for bit. The full validator, `python3 acceptance/check.py --execute G11 --bochs PATH
   --intel-observation out/`, rechecks the observation (untranslated `GenuineIntel`, same corpus
   hash) and adds it as a fourth leg; an operation is `pinned_bit_exact` only if every leg and
   `slr-x87` agree on every in-domain row. Only control word `007f` is observed on the Intel side.

The same kit measures any other candidate implementation: write its results in the same format.

## Options

- **A. Obtain an Intel observation, then pin (recommended).** If Bochs equals silicon on an op, it
  is the documented implementation to port; the Rust port of the Bochs algorithm is licence-limited
  (Bochs is LGPL 2.1 with SoftFloat-derived notices; the project is MIT OR Apache-2.0), so a
  licence review comes first. If neither equals silicon, the next step is recovering the algorithm
  from silicon data. Cost: access to one Intel Mac for a few seconds of runtime.
- **B. Treat game-visible equivalence as sufficient.** Both references agree on 100% of f32-store
  results across 1,043,510 vectors; with a 64-bit significand and a 24-bit consumer, an implementation
  within a few ulp of the 64-bit significand changes a stored f32 about once per 10^11 operations (an order-of-magnitude estimate, not measured; the sampled zero-difference result is consistent with it).
  That is **not** bit-exact transcendentals; it replaces ADR 0007's rule with a consumer-level one.
  It is a material fidelity decision and is the user's to make.
- **C. Pin to Bochs (or Rosetta) without silicon.** Documented, deterministic, reproducible, but it
  claims no original parity and the two disagree on 5-9% of 80-bit results. G11 as written does not
  pass.

## Not done

`fxam`/`ftst` (CRT classification, no probe yet), `FPREM` partial reductions (Unicorn and Rosetta
disagree on 15 of 70 probe rows; SoftFloat implements FPREM1), hardware observation, any change to `slr-x87` transcendentals (still `APPROX`), flag/exception
parity for transcendentals, PC/RC profiles other than `0x007F`, and original-consumer parity for
the 84 + 83 + 50 + 5 call sites.


## Integration review corrections (2026-10-27)

G11 now requires `FSINCOS`, which is present in the original library routines,
and compares both defined outputs of `FSINCOS` and `FPTAN`. Defined exceptional
`FYL2X` operands and `F2XM1` NaNs remain in the trust denominator; only genuinely
undefined `F2XM1` operands are reported separately. Binary edge cases now cover
special values in either operand. These corrections supersede the historical
corpus and counts above; fresh integration receipts remain local. They do not
resolve the disagreement between reference models or permit approximate results.


## Native process detection (2026-10-27)

The probe retains the raw `sysctl.proc_translated` return status and captures
`errno` immediately, before querying the CPU brand/vendor. Apple’s
[translation detection example](https://developer.apple.com/documentation/apple-silicon/about-the-rosetta-translation-environment)
classifies an absent flag (`ENOENT`) as native execution. G11 therefore accepts
that precise query outcome with `sysctl_proc_translated=0`, or an ordinary
successful native query. Other query failures, translated execution, missing
error metadata for a failed query and non-Intel identities remain rejected.
Existing successful observations without an errno field remain readable.
This corrects observation admission; it supplies no hardware capture and
promotes no x87 operation.
