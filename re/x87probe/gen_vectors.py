"""Deterministic x87 probe corpus generator.

Writes one JSONL corpus plus one JSON manifest under re/out/x87/ (git-ignored).
Each vector carries exact 80-bit operands as hex, the op name, and the list of
(PC, RC) control-word settings the runners must execute it under.

Operand classes (per vector family): small ints, f32-exact, f32-inexact,
denormals (f32/f64/ext), huge/tiny exponents, pi/2 region (trig), log-domain
edges (fyl2x/f2xm1), fprem reduction cases, NaN/+-0/+-inf where defined, and
double-rounding sequences (a+b then store-to-f32 semantics).

f80 encoding: dict(sign: 0/1, exp: 15-bit biased int, sig: 64-bit int with the
explicit integer bit). Vectors are hand-constructed from bit patterns, so no
host-float rounding leaks into the corpus.
"""
from __future__ import annotations

import hashlib
import json
import math
import os

OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "out", "x87")
CORPUS_PATH = os.path.join(OUT_DIR, "corpus.jsonl")
MANIFEST_PATH = os.path.join(OUT_DIR, "manifest.json")

BIAS = 16383
F32_BIAS = 127


def f80(sign: int, exp_unbiased: int | None, sig: int) -> dict:
    """Build an f80 operand. exp_unbiased=None -> subnormal/zero encoding raw."""
    return {"sign": sign, "exp": exp_unbiased, "sig": sig}


def from_f64_bits(bits: int) -> dict:
    sign = (bits >> 63) & 1
    exp = ((bits >> 52) & 0x7FF) - 1023
    frac = bits & ((1 << 52) - 1)
    if exp == 1024 - 1023 + 0 and False:
        pass
    if ((bits >> 52) & 0x7FF) == 0x7FF:
        # inf/nan: map to ext with all-1s exponent
        if frac == 0:
            return {"sign": sign, "exp": "inf", "sig": 0x8000000000000000}
        # quiet nan payload: keep top payload bits, set integer bit
        return {"sign": sign, "exp": "nan", "sig": 0xC000000000000000 | (frac << 11) & 0xFFFFFFFFFFFFFFFF}
    if ((bits >> 52) & 0x7FF) == 0:
        if frac == 0:
            return {"sign": sign, "exp": "zero", "sig": 0}
        # f64 subnormal -> normalize into ext
        e = -1022
        f = frac
        while not (f & (1 << 52)):
            f <<= 1
            e -= 1
        f &= (1 << 52) - 1
        sig = (1 << 63) | (f << 11)
        return {"sign": sign, "exp": e, "sig": sig}
    sig = (1 << 63) | (frac << 11)
    return {"sign": sign, "exp": exp, "sig": sig}


def f64(v: float) -> dict:
    return from_f64_bits(int.from_bytes(__import__("struct").pack("<d", v), "little"))


def f32_exact(v: float) -> dict:
    import struct
    bits = struct.unpack("<I", struct.pack("<f", v))[0]
    sign = (bits >> 31) & 1
    eb = (bits >> 23) & 0xFF
    frac = bits & 0x7FFFFF
    if eb == 0xFF:
        if frac == 0:
            return {"sign": sign, "exp": "inf", "sig": 0x8000000000000000}
        return {"sign": sign, "exp": "nan", "sig": 0xC000000000000000 | (frac << 40)}
    if eb == 0:
        if frac == 0:
            return {"sign": sign, "exp": "zero", "sig": 0}
        e = -126
        f = frac
        while not (f & (1 << 23)):
            f <<= 1
            e -= 1
        f &= 0x7FFFFF
        return {"sign": sign, "exp": e, "sig": (1 << 63) | (f << 40)}
    return {"sign": sign, "exp": eb - 127, "sig": (1 << 63) | (frac << 40)}


def ext_raw(sign: int, biased: int, sig: int) -> dict:
    return {"sign": sign, "biased": biased, "sig": sig}


def ints() -> list[dict]:
    return [
        {"sign": 0, "exp": 0, "sig": 0x8000000000000000},   # 1.0
        {"sign": 0, "exp": 1, "sig": 0x8000000000000000},   # 2.0
        {"sign": 0, "exp": 2, "sig": 0x8000000000000000},   # 4.0
        {"sign": 1, "exp": 0, "sig": 0x8000000000000000},   # -1.0
        {"sign": 0, "exp": 3, "sig": 0x8000000000000000},   # 8.0
        {"sign": 0, "exp": 10, "sig": 0x8000000000000000},  # 1024.0
        {"sign": 0, "exp": 31, "sig": 0x8000000000000000},  # 2^31
        {"sign": 1, "exp": 31, "sig": 0x8000000000000000},  # -2^31
        {"sign": 0, "exp": 63, "sig": 0x8000000000000000},  # 2^63 (i64 edge)
        {"sign": 1, "exp": 63, "sig": 0x8000000000000000},  # -2^63 (i32/i64 edge)
        {"sign": 0, "exp": 32, "sig": 0xFFFFFFFE00000000},  # 2^32-2ish integer w/ low bits
    ]


def fractions() -> list[dict]:
    return [
        f64(0.1), f64(0.5), f64(0.25), f64(1.5), f64(2.5), f64(-2.5),
        f64(1.0 / 3.0), f64(2.0 / 3.0), f64(math.pi), f64(-math.pi),
        f64(math.e), f64(1e-10), f64(1e10), f64(123.456), f64(-987.654),
        f32_exact(0.1),  # f32-inexact decimal stored exactly
        f32_exact(1.5), f32_exact(-2.75), f32_exact(100.25),
    ]


def denormals() -> list[dict]:
    return [
        # smallest f32 subnormal as ext
        {"sign": 0, "exp": -149, "sig": 0x8000000000000000},
        # a mid f32 subnormal
        {"sign": 0, "exp": -130, "sig": 0xA000000000000000},
        # smallest f64 subnormal (5e-324)
        from_f64_bits(0x0000000000000001),
        # ext subnormal encodings (biased=0, no integer bit)
        {"sign": 0, "biased": 0, "sig": 0x0000000000000001},
        {"sign": 0, "biased": 0, "sig": 0x4000000000000000},
        {"sign": 1, "biased": 0, "sig": 0x0000000000000001},
        # pseudo-denormal (biased=0 WITH integer bit set; 8087-class encoding)
        {"sign": 0, "biased": 0, "sig": 0x8000000000000000},
        # unnormal (biased!=0, no integer bit)
        {"sign": 0, "biased": 1, "sig": 0x4000000000000000},
    ]


def huge_tiny() -> list[dict]:
    return [
        {"sign": 0, "exp": 100, "sig": 0x8000000000000000},
        {"sign": 0, "exp": 1000, "sig": 0x8000000000000000},
        {"sign": 0, "exp": 10000, "sig": 0x8000000000000000},
        {"sign": 0, "exp": 16382, "sig": 0x8000000000000000},  # near ext max
        {"sign": 0, "exp": 16383, "sig": 0x7FFFFFFFFFFFFFFF},  # max finite ext
        {"sign": 0, "exp": 127, "sig": 0x8000000000000000},   # f32 overflow edge 2^127
        {"sign": 0, "exp": 128, "sig": 0x8000000000000000},   # beyond f32 max
        {"sign": 0, "exp": 1023, "sig": 0x8000000000000000},  # f64 edge
        {"sign": 0, "exp": 1024, "sig": 0x8000000000000000},  # beyond f64 max
        {"sign": 0, "exp": -100, "sig": 0x8000000000000000},
        {"sign": 0, "exp": -1000, "sig": 0x8000000000000000},
        {"sign": 0, "exp": -1074, "sig": 0x8000000000000000},  # f64 min-normal-ish region
        {"sign": 0, "exp": -126, "sig": 0x8000000000000000},  # f32 min normal 2^-126
        {"sign": 0, "exp": -149, "sig": 0x8000000000000000},  # f32 min subnormal 2^-149
        {"sign": 0, "exp": -16444, "sig": 0x8000000000000000},  # below ext min normal
        {"sign": 1, "exp": 16382, "sig": 0x8000000000000000},
    ]


def specials() -> list[dict]:
    return [
        {"sign": 0, "exp": "zero", "sig": 0},
        {"sign": 1, "exp": "zero", "sig": 0},
        {"sign": 0, "exp": "inf", "sig": 0x8000000000000000},
        {"sign": 1, "exp": "inf", "sig": 0x8000000000000000},
        {"sign": 0, "exp": "nan", "sig": 0xC000000000000000},  # canonical qnan
        {"sign": 1, "exp": "nan", "sig": 0xC000000000000000},  # -qnan
        {"sign": 0, "exp": "nan", "sig": 0x8000000000000001},  # snan-ish payload
        {"sign": 0, "exp": "nan", "sig": 0xC000000000000001},  # qnan + payload
        {"sign": 0, "exp": "nan", "sig": 0xFFFFFFFFFFFFFFFF},  # all-bits nan
        {"sign": 0, "biased": 0x7FFF, "sig": 0x8000000000000000},  # infinity encoding alt
        {"sign": 0, "biased": 0x7FFF, "sig": 0xBFFFFFFFFFFFFFFF},  # indefinite-ish
    ]


def pi_region() -> list[dict]:
    out = []
    for k in (f64(math.pi / 2), f64(math.pi), f64(3 * math.pi / 2), f64(2 * math.pi)):
        out.append(k)
    # just above/below pi/2 (argument-reduction stress)
    import struct
    for f in (math.pi / 2, math.pi, 2 * math.pi, 1e8, 1e10, 1e15, 1e18, 1e19, 2**63 * math.pi):
        b = struct.pack("<d", f)
        for delta in (-2, -1, 1, 2):
            bits = int.from_bytes(b, "little") + delta
            out.append(from_f64_bits(bits & 0xFFFFFFFFFFFFFFFF))
    out += [f64(0.0), f64(1.0), f64(-1.0), f64(0.5), f64(100.0)]
    return out


def log_edges() -> list[dict]:
    return [
        f64(0.5), f64(1.0), f64(2.0), f64(math.e), f64(10.0),
        f64(1.0 + 2**-40), f64(1.0 - 2**-33), f64(0.9999999), f64(1.0000001),
        f64(1e-300), f64(1e300), f64(3.0), f64(0.0), f64(-1.0),
        {"sign": 0, "exp": "inf", "sig": 0x8000000000000000},
        {"sign": 0, "exp": "zero", "sig": 0},
        {"sign": 0, "exp": "nan", "sig": 0xC000000000000000},
    ]


def double_rounding_pairs() -> list[tuple[dict, dict]]:
    """Operand pairs whose infinitely-precise result sits near a rounding boundary
    at 24 or 53 bits, so single-vs-double rounding is observable after fst."""
    import struct
    out: list[tuple[dict, dict]] = []
    # 1 + 2^-25: exactly between two f32 values (tie at 24 bits)
    out.append((f64(1.0), {"sign": 0, "exp": -25, "sig": 0x8000000000000000}))
    out.append((f64(1.0), {"sign": 0, "exp": -24, "sig": 0x8000000000000000}))
    # near-halfway cases at 53 bits
    out.append((f64(1.0), {"sign": 0, "exp": -54, "sig": 0x8000000000000000}))
    out.append((f64(1.0), {"sign": 0, "exp": -53, "sig": 0xC000000000000000}))
    # classic: 0.1f32 + 0.2f32 style accumulation
    out.append((f32_exact(0.1), f32_exact(0.2)))
    out.append((f32_exact(100000.0), f64(0.123456789)))
    # catastrophic cancellation then re-scale
    out.append((f64(1e16), f64(-(1e16 - 2.0))))
    out.append((f64(3.14159265), f64(-3.14159260)))
    # product just over a power of two (exponent-boundary rounding)
    out.append(({"sign": 0, "exp": 0, "sig": 0xFFFFFFFFFFFFFF00}, f64(1.0)))
    out.append(({"sign": 0, "exp": 11, "sig": 0xFFFFFFFFFFFFFF00}, f64(1.5)))
    for a, b in list(out):
        out.append((b, a))
    # dedupe preserving order
    seen: dict[str, tuple[dict, dict]] = {}
    for a, b in out:
        seen.setdefault(json.dumps([a, b], sort_keys=True), (a, b))
    return list(seen.values())


PCS = (24, 53, 64)
RCS = (0, 1, 2, 3)

# Which (pc, rc) cells each op family runs. "honors_pc_rc=False" means the op
# ignores PC/RC per the SDM; we still run all RC x PC=64 cells to prove it.
BINARY_OPS = ["fadd", "fsub", "fmul", "fdiv"]
UNARY_OPS = ["fsqrt", "fsin", "fcos", "f2xm1"]


def cells_for(op: str) -> list[tuple[int, int]]:
    if op in ("fadd", "fsub", "fmul", "fdiv", "fsqrt"):
        return [(pc, rc) for pc in PCS for rc in RCS]
    if op in ("fsin", "fcos", "fpatan", "f2xm1", "fyl2x", "fprem", "fcom",
              "fcomp", "fucom", "fucomi", "fld", "fst", "fistp"):
        # transcendentals/fprem/compares/moves: PC-independent per SDM; run
        # every RC at PC=64 plus one PC=24/RC=0 witness to prove PC-independence.
        return [(64, rc) for rc in RCS] + [(24, 0)]
    raise ValueError(op)


def build_corpus() -> list[dict]:
    vecs: list[dict] = []
    vid = 0

    def emit(op: str, a: dict, b: dict | None = None, tag: str = ""):
        nonlocal vid
        for pc, rc in cells_for(op):
            vecs.append({"id": vid, "op": op, "a": a, "b": b, "pc": pc, "rc": rc, "tag": tag})
            vid += 1

    base = ints() + fractions()
    edge = denormals() + huge_tiny() + specials()

    for op in BINARY_OPS:
        for a, b in double_rounding_pairs():
            emit(op, a, b, "dblround")
        for a in base:
            for b in (f64(1.0), f64(-1.0), f64(0.5), f64(3.0)):
                emit(op, a, b, "base")
        for a in edge:
            for b in (f64(1.0), f64(2.0), {"sign": 0, "exp": 100, "sig": 0x8000000000000000}):
                emit(op, a, b, "edge")
        # zero-divisor / zero-dividend explicitly for fdiv
        if op == "fdiv":
            for z in ({"sign": 0, "exp": "zero", "sig": 0}, {"sign": 1, "exp": "zero", "sig": 0}):
                for v in (f64(1.0), f64(-1.0), {"sign": 0, "exp": "inf", "sig": 0x8000000000000000}):
                    emit(op, v, z, "divzero")
                    emit(op, z, v, "zerodiv")

    for a in base + edge:
        emit("fsqrt", a, None, "sqrt")
    for a in (f64(-1.0), f64(-0.0), {"sign": 1, "exp": "zero", "sig": 0},
              {"sign": 1, "exp": "inf", "sig": 0x8000000000000000}):
        emit("fsqrt", a, None, "sqrtneg")

    for a in pi_region():
        emit("fsin", a, None, "trig")
        emit("fcos", a, None, "trig")
    for a in denormals() + huge_tiny()[:6] + specials():
        emit("fsin", a, None, "trigedge")
        emit("fcos", a, None, "trigedge")

    for a in log_edges():
        emit("f2xm1", a, None, "exp")
    for x in log_edges():
        for y in (f64(2.0), f64(10.0), f64(0.5), f64(math.e), f64(-3.0)):
            emit("fyl2x", x, y, "log")
    for y in log_edges():
        for x in (f64(1.0), f64(-1.0), f64(3.0)):
            emit("fpatan", y, x, "atan")

    # fprem: dividend x divisor, incl. big-ratio reductions and C2-partial cases
    prem_pairs = [
        (f64(5.5), f64(2.0)), (f64(7.0), f64(2.0)), (f64(-5.5), f64(2.0)),
        (f64(1e19), f64(3.0)), (f64(1e300), f64(7.0)),
        ({"sign": 0, "exp": 100, "sig": 0x8000000000000000}, f64(3.0)),
        ({"sign": 0, "exp": 1000, "sig": 0x8000000000000000}, f64(3.0)),
        (f64(1.0), {"sign": 0, "exp": "zero", "sig": 0}),
        ({"sign": 0, "exp": "inf", "sig": 0x8000000000000000}, f64(2.0)),
        (f64(1.0), {"sign": 0, "exp": "inf", "sig": 0x8000000000000000}),
        ({"sign": 0, "exp": "nan", "sig": 0xC000000000000000}, f64(2.0)),
        (f64(6.5), f64(0.5)), (f64(1e-300), f64(1e-310)),
        (from_f64_bits(0x0000000000000001), f64(1.0)),
    ]
    for a, b in prem_pairs:
        emit("fprem", a, b, "prem")

    for cmp in ("fcom", "fcomp", "fucom", "fucomi"):
        vals = base + edge
        for a in vals:
            for b in (f64(0.0), f64(1.0), f64(-1.0), a,
                      {"sign": 0, "exp": "nan", "sig": 0xC000000000000000}):
                emit(cmp, a, b, "cmp")

    for a in base + edge:
        emit("fld", a, None, "load")
        emit("fst", a, None, "store")
    # fistp operands: in-range, fractional (RC matters), out-of-range, special
    fistp_vals = (
        base + [f64(2147483647.0), f64(2147483648.0), f64(-2147483648.0),
                f64(-2147483649.0), f64(1e19), f64(-1e19), f64(2.5), f64(-2.5),
                f64(0.5), f64(-0.5), f64(1.5), f64(8388609.0)]
        + specials() + denormals()[:4]
    )
    for a in fistp_vals:
        # fistp honors RC (all 4) but not PC; still run PC=64 + PC=24 witness.
        for pc, rc in [(64, r) for r in RCS] + [(24, 0)]:
            vecs.append({"id": vid, "op": "fistp", "a": a, "b": None, "pc": pc, "rc": rc, "tag": "int"})
            vid += 1

    return vecs


def main() -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    vecs = build_corpus()
    with open(CORPUS_PATH, "w") as f:
        for v in vecs:
            f.write(json.dumps(v) + "\n")
    counts: dict[str, int] = {}
    for v in vecs:
        counts[v["op"]] = counts.get(v["op"], 0) + 1
    digest = hashlib.sha256(open(CORPUS_PATH, "rb").read()).hexdigest()
    with open(MANIFEST_PATH, "w") as f:
        json.dump({"vectors": len(vecs), "sha256": digest, "per_op": counts,
                   "pcs": list(PCS), "rcs": list(RCS)}, f, indent=2)
    print(f"wrote {len(vecs)} vectors -> {CORPUS_PATH}")
    for op in sorted(counts):
        print(f"  {op:8s} {counts[op]}")


if __name__ == "__main__":
    main()
