#!/usr/bin/env python3
"""Unicorn oracle vectors for the exe's MSVC `rand`/`srand` (ticket M0-13).

Calls the real exe code per draw: `srand` at VA 0x5d73fb and `_rand` at VA
0x5d7408, with `_getptd` (0x5dd8d3) hooked to return a caller-owned scratch
block per simulated thread (separate `[block+0x14]` state words, exactly how
separate OS threads get separate RNG states).

One stdout line per (seed, thread) pair (flushed as produced):

    SEED 0x1 THREAD 0 41 18467 6334 ...

Usage (from the repo root, venv at ~/slr-oracle-venv):

    ~/slr-oracle-venv/bin/python re/oracle/rand_vectors.py \
        --seeds 0,1,42,0x12345678 --threads 4 --draws 62500

Default totals 4 x 4 x 62500 = 1,000,000 draws in ~1 min (per-call harness;
see issue #14). The Rust consumer is `crates/slr-platform/tests/rand_oracle.rs`.

Never writes to reference/ or stock/; the exe is only read.
"""
from __future__ import annotations

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle  # noqa: E402

MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

RAND_ADDR = 0x5D7408
SRAND_ADDR = 0x5D73FB
GETPTD_ADDR = 0x5DD8D3
STATE_OFF = 0x14


def _default_exe() -> str:
    for root in (
        os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")),
        MAIN_CHECKOUT,
        os.environ.get("SLR_REFERENCE", ""),
    ):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _parse_seeds(s: str) -> list[int]:
    return [int(x, 0) for x in s.split(",") if x.strip()]


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seeds", default="0,1,42,0x12345678")
    ap.add_argument("--threads", type=int, default=4)
    ap.add_argument("--draws", type=int, default=62500,
                    help="draws per (seed, thread) pair")
    ap.add_argument("--exe", default=None)
    args = ap.parse_args(argv)
    seeds = _parse_seeds(args.seeds)
    exe = args.exe or _default_exe()
    t0 = time.perf_counter()
    total = 0
    for seed in seeds:
        o = Oracle(exe=exe)
        blocks = [o.scratch(0x100) for _ in range(args.threads)]
        target = {"b": blocks[0]}
        o.hook(GETPTD_ADDR, lambda oo, a: target["b"], "cdecl", 0)
        for t in range(args.threads):
            target["b"] = blocks[t]
            o.call(SRAND_ADDR, args=[seed])
            got = o.read_u32(blocks[t] + STATE_OFF)
            assert got == (seed & 0xFFFFFFFF), f"srand state {got:#x} != {seed:#x}"
            out = []
            for _ in range(args.draws):
                v = o.call(RAND_ADDR, args=[]).eax
                assert 0 <= v <= 0x7FFF, f"rand out of range: {v}"
                out.append(str(v))
            total += len(out)
            print(f"SEED {seed:#x} THREAD {t} " + " ".join(out), flush=True)
    dt = time.perf_counter() - t0
    print(f"# draws={total} seeds={len(seeds)} threads={args.threads} "
          f"per_pair={args.draws} seconds={dt:.1f} oracle=unicorn", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
