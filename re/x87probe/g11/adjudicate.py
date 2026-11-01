#!/usr/bin/env python3
"""Compare streamed result files leg by leg against the shared corpus (no mpmath needed).

  adjudicate.py corpus.json corpus.in bochs=bochs.out rosetta=rosetta.out intel=intel.out

Reports, per op, bit-exact agreement of every pair of legs on the 80-bit result and on the
result rounded to f32 (the value an `FSTP m32` stores), plus the C2 flag. A leg that executed
real Intel x87 (stderr `cpu_vendor` GenuineIntel, `sysctl_proc_translated` 0) is what decides
which software model, if any, is hardware-exact for an operation.
"""
import collections, itertools, json, struct, sys
from pathlib import Path


def to_f32_bits(rec):
    sig, se = struct.unpack("<QH", rec[:10])
    sign, e = se >> 15, se & 0x7FFF
    if e == 0x7FFF:
        return (sign << 31) | 0x7F800000 | (0x400000 if sig << 1 & (2**64 - 1) else 0)
    if e == 0 and sig == 0:
        return sign << 31
    # round 64-bit significand to 24 bits, nearest even, handle f32 range
    exp = (e if e else 1) - 16383
    if sig == 0:
        return sign << 31
    shift = 40
    if exp < -126:
        shift += -126 - exp
    if shift > 64:
        return sign << 31
    q, r = sig >> shift, sig & ((1 << shift) - 1)
    half = 1 << (shift - 1)
    if r > half or (r == half and q & 1):
        q += 1
    if exp < -126:
        return (sign << 31) | q
    if q == 1 << 24:
        q >>= 1; exp += 1
    if exp > 127:
        return (sign << 31) | 0x7F800000
    return (sign << 31) | ((exp + 127) << 23) | (q & 0x7FFFFF)


BINARY_OPS = {"fpatan", "fyl2x", "fyl2xp1", "fscale"}


def _special(sig, se):
    exp = se & 0x7FFF
    return exp == 0x7FFF or (exp == 0 and sig == 0)


def split_records(buf, rows, name):
    if len(buf) != rows * 22:
        raise ValueError(f"{name}: expected {rows} records of 22 bytes, got {len(buf) / 22}")
    return [buf[i:i + 22] for i in range(0, len(buf), 22)]


def compare(meta, corpus, legs):
    """Per-op and per-spec agreement counters for every pair of legs (name -> list of records)."""
    report = {}
    by_spec = {}
    for spec in meta["index"]:
        op = spec["op"]
        for k in range(spec["first"], spec["first"] + spec["rows"]):
            a_sig, a_se = struct.unpack("<QH", corpus[k * 21 + 1:k * 21 + 11])
            # SDM-undefined operand domain: F2XM1 needs |x| <= 1 (x < 0 for FYL2X is defined: invalid)
            a_exp = a_se & 0x7FFF
            a_nan = a_exp == 0x7FFF and (a_sig << 1) & (2**64 - 1) != 0
            # the SDM defines F2XM1 only on [-1, 1]; infinities are outside it (Rosetta returns -0.5 for
            # -inf, Bochs -1, and the SDM table does not settle it), so they make no claim
            ood = (op == "f2xm1" and a_exp >= 0x3FFF and (a_exp, a_sig) != (0x3FFF, 1 << 63) and not a_nan)
            # zeros, infinities and NaNs have SDM-fixed results; finite operands are the open class
            b_sig, b_se = struct.unpack("<QH", corpus[k * 21 + 11:k * 21 + 21])
            special = _special(a_sig, a_se) or (op in BINARY_OPS and _special(b_sig, b_se))
            tag = ":out-of-domain" if ood else ":special" if special else ""
            cell = report.setdefault(op + tag, collections.Counter())
            cell["rows"] += 1
            spec_cell = by_spec.setdefault(spec["spec"] + tag, collections.Counter())
            spec_cell["rows"] += 1
            for a, b in itertools.combinations(sorted(legs), 2):
                x, y = legs[a][k], legs[b][k]
                c2 = (x[20] | x[21] << 8) & 0x400 == (y[20] | y[21] << 8) & 0x400
                width = 20 if op in ("fsincos", "fptan") else 10  # both results of the two-result ops
                for tgt in (cell, spec_cell):
                    tgt[f"{a}=={b}:ext80"] += x[:width] == y[:width]
                    tgt[f"{a}=={b}:f32"] += to_f32_bits(x) == to_f32_bits(y)
                    tgt[f"{a}=={b}:c2"] += c2
    return {"by_op": {op: dict(c) for op, c in report.items()},
            "by_spec": {k: dict(c) for k, c in by_spec.items()}}


def main(argv):
    meta = json.loads(Path(argv[0]).read_text())
    corpus = Path(argv[1]).read_bytes()
    if len(corpus) != meta["rows"] * 21:
        sys.exit("corpus length differs from manifest")
    legs = {}
    for item in argv[2:]:
        name, path = item.split("=", 1)
        legs[name] = split_records(Path(path).read_bytes(), meta["rows"], name)
    print(json.dumps(compare(meta, corpus, legs), indent=1))


if __name__ == "__main__":
    main(sys.argv[1:])
