"""Deterministic operand corpora for the x87 transcendental legs (binary 21-byte records)."""
import random
import struct

OPS = {"fsin": 0, "fcos": 1, "fpatan": 2, "f2xm1": 3, "fyl2x": 4, "fyl2xp1": 5, "fptan": 6, "fsincos": 7, "fscale": 8, "frndint": 9}
BINARY = {"fpatan", "fyl2x", "fyl2xp1", "fscale"}


def f80(sign, exp, sig):
    return struct.pack("<QH", sig, (sign << 15) | exp)


def from_f32_bits(w):
    sign, e, m = w >> 31, (w >> 23) & 0xFF, w & 0x7FFFFF
    if e == 0xFF:
        return f80(sign, 0x7FFF, 0x8000000000000000 | (m << 40))
    if e == 0:
        if m == 0:
            return f80(sign, 0, 0)
        sh = 24 - m.bit_length()
        return f80(sign, 0x3FFF - 126 - sh, (m << sh) << 40)
    return f80(sign, e - 127 + 0x3FFF, (0x800000 | m) << 40)


def from_f64_bits(w):
    sign, e, m = w >> 63, (w >> 52) & 0x7FF, w & ((1 << 52) - 1)
    if e == 0x7FF:
        return f80(sign, 0x7FFF, 0x8000000000000000 | (m << 11))
    if e == 0:
        if m == 0:
            return f80(sign, 0, 0)
        sh = 53 - m.bit_length()
        return f80(sign, 0x3FFF - 1022 - sh, (m << sh) << 11)
    return f80(sign, e - 1023 + 0x3FFF, (0x10000000000000 | m) << 11)


def gen_operand(rng, cls, lo, hi):
    """lo/hi: unbiased binary exponent range of the magnitude."""
    e = rng.randint(lo, hi)
    sign = rng.getrandbits(1)
    if cls == "f32":
        return f80(sign, e + 0x3FFF, (1 << 63) | (rng.getrandbits(23) << 40))
    if cls == "f64":
        return f80(sign, e + 0x3FFF, (1 << 63) | (rng.getrandbits(52) << 11))
    return f80(sign, e + 0x3FFF, (1 << 63) | rng.getrandbits(63))


def corpus(op, cls, n, seed, lo, hi, lo2=None, hi2=None):
    rng = random.Random(seed)
    opc = OPS[op]
    out = bytearray()
    for _ in range(n):
        a = gen_operand(rng, cls, lo, hi)
        if op in ("fyl2x",):
            a = a[:9] + bytes([a[9] & 0x7F])  # log domain: x > 0
        b = gen_operand(rng, cls, lo if lo2 is None else lo2, hi if hi2 is None else hi2) if op in BINARY else bytes(10)
        out += bytes([opc]) + a + b
    return bytes(out)


# (op, class, lo, hi, lo2, hi2): unbiased exponent ranges of the operands
SPECS = []
for cls in ("f32", "f64", "ext"):
    for lo, hi in ((-30, -2), (-2, 2), (2, 30)):
        SPECS.append(("fsin", cls, lo, hi, None, None))
        SPECS.append(("fcos", cls, lo, hi, None, None))
    SPECS.append(("f2xm1", cls, -40, -2, None, None))
    SPECS.append(("f2xm1", cls, -2, -1, None, None))
    SPECS.append(("fyl2x", cls, -20, 20, -6, 6))
    SPECS.append(("fyl2x", cls, -1, 1, -6, 6))
    SPECS.append(("fpatan", cls, -20, 20, -20, 20))
    SPECS.append(("fpatan", cls, -2, 2, -2, 2))
for cls in ("f32", "f64", "ext"):
    SPECS.append(("fscale", cls, -20, 20, -2, 6))
    SPECS.append(("fscale", cls, -20, 20, 6, 15))
    SPECS.append(("frndint", cls, -4, 4, None, None))
    SPECS.append(("frndint", cls, 4, 70, None, None))
SPECS.append(("fsin", "f32", 30, 127, None, None))
SPECS.append(("fcos", "f32", 30, 127, None, None))
SPECS.append(("fsin", "ext", 30, 62, None, None))
SPECS.append(("fcos", "ext", 30, 62, None, None))
