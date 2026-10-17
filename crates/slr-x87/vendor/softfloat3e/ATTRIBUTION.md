# SoftFloat vendoring attribution (slr-x87)

`vendor/softfloat3e/source/` (+ `8086-SSE` specialize) and
`vendor/softfloat3e/build/{platform.h,Makefile}` are vendored verbatim from:

- **Berkeley SoftFloat, Release 3e** — John R. Hauser,
  `https://github.com/ucb-bar/berkeley-softfloat-3`
  (upstream commit `a0c6494`, "Merge pull request #34", cloned 2026-10-17).
- License: **BSD 3-Clause** (UC Regents). The full text is preserved at
  `vendor/softfloat3e/COPYING.txt` (redistribution condition 1: source
  redistributions retain the copyright notice, conditions, disclaimer —
  satisfied: `COPYING.txt` plus the per-file headers are untouched).
- Upstream README preserved at `vendor/softfloat3e/README-upstream.md`.

## Why vendored (ADR 0008)

ADR 0008 requires local-only oracles and crates: everything must build on the
developer's Mac with no VMs/Docker/x86 machine, and CI is data-free. The x87
reference is triangulated from SoftFloat's 80-bit type with precision control
(independent of QEMU), Unicorn, and Rosetta 2. Depending on a system SoftFloat
or downloading it at build time would break hermetic/offline builds, so the C
sources ship inside the crate and compile via `build.rs` + the `cc` crate
(GPL tooling never enters the workspace — only `cc` + the platform C
compiler are used).

## What is vendored vs written

- Vendored (BSD-3, untouched): all of `source/*.c`, `source/8086-SSE/*.c`,
  `source/include/*.h`, `build/platform.h`, `build/Makefile`, `COPYING.txt`.
- Written for this crate (MIT, workspace license): `build.rs` (drives `cc`
  with the M0-07 oracle flags: 8086-SSE specialize, `SOFTFLOAT_ROUND_ODD`,
  `INLINE_LEVEL=5`, `SOFTFLOAT_FAST_INT64`, `SOFTFLOAT_FAST_DIV32TO16`,
  `SOFTFLOAT_FAST_DIV64TO32`), `src/slr_softfloat_shim.c` (global-state
  accessors + truncated-quotient FPREM whose division core is adapted from
  SoftFloat's `extF80_rem.c`), and all Rust sources.
- `src/slr_softfloat_shim.c` includes SoftFloat headers and adapts its long
  division; it is our file under the workspace MIT license and carries its
  own header noting the BSD-3 origin of the adapted algorithm.

## Verification

`cargo test -p slr-x87` compiles this tree and asserts bit-exact agreement
with the SoftFloat oracle (`re/out/x87/sf_results.jsonl`, git-ignored,
regenerated via `re/x87probe/README.md`) for every TRUSTED cell; the test
prints per-(op × PC × RC) match counts.
