import struct, sys, collections
from mpmath import mp, mpf, sin, cos, atan2, log, power
mp.prec = 300

def dec(b):
    sig, se = struct.unpack("<QH", b)
    s = se >> 15; e = se & 0x7FFF
    return s, e, sig

def val(b):
    s, e, sig = dec(b)
    if e == 0x7FFF: return None
    if e == 0 and sig == 0: return mpf(0)
    ee = (e if e else 1) - 16383 - 63
    v = mpf(sig) * mpf(2) ** ee
    return -v if s else v

def to_int_key(b):
    s, e, sig = dec(b)
    return (s, e, sig)

def ulp_dist(x, y):
    sx, ex, mx = dec(x); sy, ey, my = dec(y)
    if sx != sy: return None
    if ex == ey: return abs(mx - my)
    # adjacent binades
    if abs(ex - ey) == 1 and ex and ey:
        lo, hi = (x, y) if ex < ey else (y, x)
        _, elo, mlo = dec(lo); _, ehi, mhi = dec(hi)
        return (mhi - (1 << 63)) // 2 + ((1 << 64) - mlo) // 1  # coarse
    return None

def ref_round(v):
    """Correctly rounded (nearest even) 64-bit significand image of mpf v."""
    if v is None: return None
    if v == 0: return struct.pack("<QH", 0, 0)
    s = 1 if v < 0 else 0
    v = abs(v)
    e = int(mp.floor(mp.log(v, 2)))
    while mpf(2) ** e > v: e -= 1
    while mpf(2) ** (e + 1) <= v: e += 1
    m = v / mpf(2) ** (e - 63)
    mi = int(mp.nint(m))   # nint rounds half to even in mpmath
    if mi == 1 << 64: mi >>= 1; e += 1
    return struct.pack("<QH", mi, (s << 15) | (e + 16383))

def reference(op, a, b):
    x = val(a); y = val(b)
    if op == 'fyl2x' and x <= 0: return None
    if op == 'fsin': return sin(x)
    if op == 'fcos': return cos(x)
    if op == 'fpatan': return atan2(y, x)
    if op == 'f2xm1': return power(2, x) - 1
    if op == 'fyl2x': return y * log(x, 2)
    raise ValueError(op)
