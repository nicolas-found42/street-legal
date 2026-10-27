#!/usr/bin/env python3
"""Unicorn per-function vectors for the vehicle physics leaf functions (G4, tier C level 1).

    SLR_PHYS_FN_OUT=DIR ~/slr-oracle-venv/bin/python re/oracle/physics_fn_vectors.py gen

Everything written is git-ignored derived data; nothing here embeds exe bytes. The functions run as
exe code at x87 control word 0x007F (PC24, RC nearest, ADR 0007), with random + edge operands:

  torque_curve      FUN_0043be70   piecewise-linear lookup over an {count, pairs*} curve
  torque_curve_obj  FUN_0043bee0   the same over an object holding the curve at +8/+0xc
  pacejka_clamped   FUN_0043ba80   Pacejka magic formula, force clamped to >= 0
  pacejka           FUN_0043bb30   the same without the clamp
  slip              FUN_0043bbd0   tyre slip blend with relaxation length

Each row records the raw ST0 image the function returns (all five return in ST0 or through
pointers), the FPU stack depth, written memory cells, and the global constants the exe reads
(randomised per row for `slip`, whose tuning constants are runtime config in the original).
`pacejka*` calls the x87 transcendentals; Unicorn's are double-based, so those rows are
diagnostic for G11 and the Rust replay reports (not asserts) mismatches only where the port's
own APPROX path is implicated.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.environ.get("SLR_PHYS_FN_OUT") or os.path.join(REPO, "re", "out", "physics_fn")
CW = CW_D3D9

TORQUE = 0x43BE70
TORQUE_OBJ = 0x43BEE0
PACEJKA_CLAMPED = 0x43BA80
PACEJKA = 0x43BB30
SLIP = 0x43BBD0
# runtime-config globals the slip function reads (docs/physics.md sections 9/10.1)
SLIP_CONSTS = [0x60E02C, 0x60E030, 0x60E034, 0x60E038, 0x60E03C, 0x60E040, 0x60E044, 0x60E048]


def bits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def fl(w: int) -> float:
    return struct.unpack("<f", struct.pack("<I", w & 0xFFFFFFFF))[0]


EDGE = [0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x3DCCCCCD, 0xBDCCCCCD, 0x3F000000,
        0x40000000, 0x40490FDB, 0x00000001, 0x007FFFFF, 0x00800000, 0x7F7FFFFF, 0xFF7FFFFF,
        0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00000, 0x7FA00000, 0x42C80000, 0x447A0000,
        0x3727C5AC, 0x4E6E6B28, 0x1E3CE508, 0x5F000000]


def rand_word(r: random.Random, lo_exp: int, hi_exp: int) -> int:
    """A finite f32 with a uniformly chosen exponent in [lo_exp, hi_exp] and random sign/mantissa."""
    if r.random() < 0.12:
        return r.choice(EDGE)
    e = r.randint(lo_exp, hi_exp) + 127
    return (r.getrandbits(1) << 31) | (max(1, min(254, e)) << 23) | r.getrandbits(23)


def st0(res):
    return res.st0_raw.hex() if res.st0_raw is not None else None


def write_jsonl(name: str, rows: list) -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    with open(os.path.join(OUT_DIR, name), "w") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":")) + "\n")


def gen_torque(o: Oracle, r: random.Random) -> list:
    rows = []
    reset = o.scratch_ptr
    for variant, va in (("torque_curve", TORQUE), ("torque_curve_obj", TORQUE_OBJ)):
        for _ in range(1500):
            o.scratch_ptr = reset
            count = r.choice([1, 1, 2, 2, 3, 4, 5, 8, 12])
            xs = sorted(r.uniform(-5, 9000) if r.random() < 0.8 else r.choice([0, 0.5, 1000, 1000.5, 6500]) for _ in range(count))
            if r.random() < 0.15 and count > 1:
                xs[1] = xs[0]  # duplicate knot: divide by zero path
            pairs = []
            for x in xs:
                pairs += [bits(x), rand_word(r, -4, 10) if r.random() < 0.85 else r.choice(EDGE)]
            pa = o.scratch(8 * count + 16)
            o.write_u32s(pa, pairs)
            if variant == "torque_curve":
                hdr = o.scratch(8)
                o.write_u32s(hdr, [count, pa])
                target = hdr
            else:
                hdr = o.scratch(32)
                o.write_u32s(hdr, [0, 0, count, pa])
                target = hdr
            for x in ([pairs[0], pairs[-2]] + [bits(v) for v in (xs[0] - 1, xs[0], xs[-1], xs[-1] + 1)]
                      + [rand_word(r, -3, 13) for _ in range(6)] + [r.choice(EDGE)]):
                res = o.call(va, [("u32", x), ("u32", target)], "cdecl", fpu_cw=CW)
                rows.append({"fn": variant, "x": x, "count": count, "pairs": pairs,
                             "st0": st0(res), "depth": res.fpu_top_depth, "esp": res.esp_delta})
    return rows


def gen_pacejka(o: Oracle, r: random.Random) -> list:
    rows = []
    reset = o.scratch_ptr
    for name, va in (("pacejka_clamped", PACEJKA_CLAMPED), ("pacejka", PACEJKA)):
        for i in range(12000):
            o.scratch_ptr = reset
            if i % 3 == 0:  # realistic coefficient neighbourhood
                base = [1.4, 0.0007, 1.3, 6000.0, 400.0, 0.0006, -1e-7, 1e-3, 0.4, 0.0, 0.0]
                coef = [bits(b * r.uniform(0.2, 3.0)) if b else bits(r.uniform(-0.02, 0.02)) for b in base]
                fz = bits(r.uniform(0, 12000))
                slip = bits(r.uniform(-1.5, 1.5))
            else:
                coef = [rand_word(r, -8, 14) for _ in range(11)]
                fz = rand_word(r, -4, 14)
                slip = rand_word(r, -6, 6)
            cell = o.scratch(0x40)
            o.write_u32s(cell, coef)
            res = o.call(va, [("u32", slip), ("u32", fz)], "thiscall", this=cell, fpu_cw=CW)
            rows.append({"fn": name, "coef": coef, "slip": slip, "fz": fz, "st0": st0(res),
                         "depth": res.fpu_top_depth, "esp": res.esp_delta, "fpsw": res.fpsw})
    return rows


def gen_slip(o: Oracle, r: random.Random) -> list:
    rows = []
    reset = o.scratch_ptr
    for i in range(20000):
        o.scratch_ptr = reset
        consts = [rand_word(r, -4, 4) if i % 4 else bits(v) for v, _ in zip(
            (1.0, 1.0, 0.3, 0.5, 0.1, 0.1, 0.1, 0.2), SLIP_CONSTS)]
        for va, w in zip(SLIP_CONSTS, consts):
            o.write_u32(va, w)
        a = [rand_word(r, -6, 6) for _ in range(4)]
        if i % 5 == 0:
            a[1] = bits(r.uniform(-30, 30))
        out_x0, out_y0 = rand_word(r, -4, 4), rand_word(r, -4, 4)
        cell = o.scratch(32)
        o.write_u32s(cell, [out_x0, out_y0])
        res = o.call(SLIP, [("u32", a[0]), ("u32", a[1]), ("u32", a[2]), ("u32", a[3]),
                            ("u32", cell), ("u32", cell + 4)], "cdecl", fpu_cw=CW)
        rows.append({"fn": "slip", "args": a, "consts": consts, "out0": [out_x0, out_y0],
                     "out": o.read_u32s(cell, 2), "depth": res.fpu_top_depth, "esp": res.esp_delta,
                     "st0": st0(res)})
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["gen"])
    ap.add_argument("--exe", default=os.path.join(REPO, "stock", "StreetLegal_Redline.bak.exe"),
                    help="path to the stock executable")
    args = ap.parse_args(argv)
    o = Oracle(args.exe, fpu_cw=CW)
    r = random.Random(0x7A11)
    for name, rows in (("torque.jsonl", gen_torque(o, r)), ("pacejka.jsonl", gen_pacejka(o, r)),
                       ("slip.jsonl", gen_slip(o, r))):
        write_jsonl(name, rows)
        print(name, len(rows))
    return 0


if __name__ == "__main__":
    sys.exit(main())
