# re/x87probe

Probe suite for ticket M0-07 (plan id M0-07, issue #8): x87 behavior probes
with two oracles — Berkeley SoftFloat-3e `extF80` (C harness) and Unicorn
2.1.4 (x86 stub runner) — plus an `mpmath` tiebreaker for transcendentals.

## Layout

- `gen_vectors.py` — deterministic corpus generator. Writes
  `re/out/x87/corpus.jsonl` + `re/out/x87/manifest.json` (both git-ignored).
- `softfloat_harness.c` — C harness over SoftFloat-3e `extF80_*`. Reads the
  corpus, prints one JSON result per line. Precision control is emulated by
  setting the SoftFloat global `extF80_roundingPrecision` to 32/64/80 for
  PC=24/53/64 (basic arithmetic + `fsqrt` only); all other ops run at
  precision 80 with `"pc_applied": false`. Transcendentals print
  `"unsupported": true` (SoftFloat has none). `fprem` uses `extF80_rem`
  (FPREM1/nearest-quotient semantics — see docs/x87.md for the resolution).
- `run_unicorn.py` — Unicorn 2.1.4 runner. Fresh 32-bit instance per vector:
  sets FPCW from (PC, RC), `FLD tbyte` operands in, one instruction stub,
  reads ST(0)/memory/flags back exactly.
- `compare.py` — referee. Joins both oracles (+ `mpmath` at 200-bit precision
  for `fsin/fcos/fpatan/f2xm1/fyl2x`) by vector id; writes
  `re/out/x87/results.json` + `re/out/x87/disagreements.json`.

## Regenerate everything from scratch

```sh
# 0. one-time setup (outside the repo):
#    SoftFloat source + build:
mkdir -p re/out/x87/softfloat
git clone --depth 1 https://github.com/ucb-bar/berkeley-softfloat-3.git \
  re/out/x87/softfloat/sf3
make -C re/out/x87/softfloat/sf3/build/Linux-x86_64-GCC -j8
#    Python deps (shared oracle venv):
~/slr-oracle-venv/bin/pip install mpmath

# 1. corpus (21,702 vectors):
~/slr-oracle-venv/bin/python re/x87probe/gen_vectors.py

# 2. SoftFloat oracle:
SF=re/out/x87/softfloat/sf3
clang -O2 -o re/out/x87/sf_oracle \
  -I$SF/source/include -I$SF/build/Linux-x86_64-GCC \
  -DSOFTFLOAT_FAST_INT64 -DINLINE_LEVEL=5 -DSOFTFLOAT_ROUND_ODD \
  -DSOFTFLOAT_FAST_DIV32TO16 -DSOFTFLOAT_FAST_DIV64TO32 \
  re/x87probe/softfloat_harness.c $SF/build/Linux-x86_64-GCC/softfloat.a
./re/out/x87/sf_oracle re/out/x87/corpus.jsonl > re/out/x87/sf_results.jsonl

# 3. Unicorn oracle:
~/slr-oracle-venv/bin/python re/x87probe/run_unicorn.py \
  re/out/x87/corpus.jsonl re/out/x87/uc_results.jsonl

# 4. referee:
~/slr-oracle-venv/bin/python re/x87probe/compare.py
```

Nothing under `re/out/` is committed (git-ignored via `/re/out`).
Committed outputs of this suite: this directory + `docs/x87.md`.
