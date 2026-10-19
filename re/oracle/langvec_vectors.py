#!/usr/bin/env python3
"""Unicorn oracle vectors for the M1-07 `Vector3`/`Ypr` natives and the shared
3x3 helpers behind them (issue #34).

Every row is produced by running the **real exe code** under Unicorn with the
game's D3D9 control word (`0x007f`, 24-bit x87 precision) on synthetic objects
built in emulator memory — no game data, no JVM. The Rust consumer is
`crates/slr-lang/tests/vector_oracle.rs`; rows live in
`re/out/langvec/langvec.jsonl` (git-ignored).

Covered symbols:

    0x00482110 Vector3.rotate(Ypr)         0x004823e0 Vector3.length
    0x00482550 Vector3.distance            0x00482450 Vector3.normalize
    0x00482280 Vector3.rotate(Vector3,F)   0x00482020 Vector3.<init>(Ypr)
    0x00482610 Ypr.<init>(Vector3)         0x004826c0 Ypr.mul
    0x0054ecd0 ypr -> 3x3                  0x0054eab0 3x3 *= 3x3
    0x00551c90 3x3 -> ypr                  0x00551d20 vec -> ypr
    0x0054ee80 axis-angle 3x3

The argument protocol is the one `handoff-M1-07.md` documents (machine-verified
from `0x45d910`): the container is `data[0..count-1]` with the `this` cell on
top (`data[count-1]`) and the declared arguments below it in signature order;
`F`/`I` slots are one cell and the marshaller hands the native their payload
word. Everything else — the field helpers `0x42a560`/`0x42a040`, the
marshaller, the three 3x3 helpers and the natives — is real exe code.

Every input and output is an 8-hex-digit IEEE-754 `f32` bit pattern (exact:
NaN payloads and signed zeros included).

Usage (from the repo root; venv at `~/slr-oracle-venv`):

    ~/slr-oracle-venv/bin/python re/oracle/langvec_vectors.py gen
    ~/slr-oracle-venv/bin/python re/oracle/langvec_vectors.py verify

`gen` writes `re/out/langvec/langvec.jsonl`; `verify` re-runs each recorded
vector against the exe and checks it is byte-identical (a determinism check on
the harness, not the port). Override the exe with `--exe` or `SLR_REFERENCE`.

Never writes to `reference/` or `stock/`; the exe is only read.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import CW_D3D9, Oracle  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
OUT_DIR = os.path.join(REPO, "re", "out", "langvec")
OUT_FILE = os.path.join(OUT_DIR, "langvec.jsonl")

# --- exe addresses ---------------------------------------------------------

V3_ROTATE_YPR = 0x0048_2110
V3_LENGTH = 0x0048_23E0
V3_DISTANCE = 0x0048_2550
V3_NORMALIZE = 0x0048_2450
V3_ROTATE_AXIS = 0x0048_2280
V3_INIT_YPR = 0x0048_2020
YPR_INIT_VEC = 0x0048_2610
YPR_MUL = 0x0048_26C0

MAT3_FROM_YPR = 0x0054_ECD0
MAT3_MUL = 0x0054_EAB0
MAT3_TO_YPR = 0x0055_1C90
VEC_TO_YPR = 0x0055_1D20
MAT3_FROM_AXIS = 0x0054_EE80

# The synthetic field/class plumbing (handoff-M1-07 (b)).
VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}
LOG_SINKS = (0x551140, 0x5513B0)
MATRIX_OFFS = (0, 4, 8, 0x10, 0x14, 0x18, 0x20, 0x24, 0x28)


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _today() -> str:
    import datetime
    return datetime.date.today().isoformat()


UC_VER = "2.1.4"
CW = CW_D3D9


def f32(v: float) -> float:
    return struct.unpack("<f", struct.pack("<f", v))[0]


def bits(v: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f32(v)))[0]


def hx(value) -> str:
    if isinstance(value, int):
        return f"{value & 0xFFFFFFFF:08x}"
    return f"{bits(value):08x}"


def from_bits(word: str) -> float:
    return struct.unpack("<f", struct.pack("<I", int(word, 16)))[0]


# --- the Unicorn world -----------------------------------------------------


class World:
    """A fresh emulator with the synthetic object graph builders."""

    def __init__(self, exe: str):
        self.o = Oracle(exe, fpu_cw=CW)
        self.o.hook_alloc()
        self.fields: dict[int, dict[str, float]] = {}
        self.cells: dict[int, dict[str, int]] = {}
        for va in LOG_SINKS:
            self.o.hook(va, lambda oc, a: None, "cdecl", 3)

    def cstr(self, s: bytes) -> int:
        p = self.o.alloc(len(s) + 8)
        self.o.write(p, s + b"\0")
        return p

    def fcell(self, v: float, name: bytes) -> int:
        p = self.o.alloc(16)
        self.o.write_u32(p, VAL_VT)
        self.o.write_u32(p + 4, TC["F"])
        self.o.write_u32(p + 8, bits(v))
        self.o.write_u32(p + 0xC, self.cstr(name))
        return p

    def container(self, words: list[int]) -> int:
        o = self.o
        c = o.alloc(12)
        d = o.alloc(max(4 * len(words), 4))
        for i, w in enumerate(words):
            o.write_u32(d + 4 * i, w)
        o.write_u32(c, len(words))
        o.write_u32(c + 4, d)
        o.write_u32(c + 8, len(words))
        return c

    def obj(self, cls: bytes, values: list[float], names: tuple[bytes, ...]) -> int:
        o = self.o
        cells = [self.fcell(v, n) for v, n in zip(values, names)]
        fields = self.container(cells)
        holder = o.alloc(0x10)
        o.write_u32(holder + 0xC, fields)
        k = o.alloc(0x220)
        r = o.alloc(0x80)
        o.write_u32(r + 0x40, 0xFFFFFFFF)
        o.write_u32(k + 4, r)
        o.write_u32(k + 0x1D0, holder)
        ob = o.alloc(0x40)
        o.write_u32(ob + 0xC, k)
        self.fields[ob] = {n.decode(): f32(v) for n, v in zip(names, values)}
        self.cells[ob] = dict(zip((n.decode() for n in names), cells))
        return ob

    def vec3(self, x: float, y: float, z: float) -> int:
        return self.obj(b"java.lang.Vector3", [x, y, z], (b"x", b"y", b"z"))

    def ypr(self, y: float, p: float, r: float) -> int:
        return self.obj(b"java.lang.Ypr", [y, p, r], (b"y", b"p", b"r"))

    def lcell(self, obj_word: int) -> int:
        p = self.o.alloc(12)
        self.o.write_u32(p, VAL_VT)
        self.o.write_u32(p + 4, TC["L"])
        self.o.write_u32(p + 8, obj_word)
        return p

    def frame(self, sig: bytes, cells: list[int],
              cls: bytes = b"java.lang.Vector3", name: bytes = b"x") -> int:
        o = self.o
        stack = self.container(cells)
        callinfo = o.alloc(0x20)
        o.write_u32(callinfo + 4, self.cstr(sig))
        method = o.alloc(0x40)
        o.write_u32(method + 0xC, self.cstr(cls))
        o.write_u32(method + 0x10, self.cstr(name))
        o.write_u32(method + 0x14, callinfo)
        fr = o.alloc(0x40)
        o.write_u32(fr + 8, stack)
        o.write_u32(fr + 0xC, method)
        return fr

    def fields_of(self, ob: int) -> dict[str, str]:
        return {n: hx(self.o.read_u32(cell + 8)) for n, cell in self.cells[ob].items()}

    def triple(self, ob: int) -> list[str]:
        return [hx(self.o.read_u32(self.cells[ob][n] + 8)) for n in self.cells[ob]]

    def words3(self, values: list[float]) -> list[str]:
        return [hx(v) for v in values]


def native(w: World, addr: int, sig: bytes, cells: list[int],
           cls: bytes, name: bytes, limit: int = 2_000_000):
    fr = w.frame(sig, cells, cls, name)
    return w.o.call(addr, [fr], "cdecl", limit=limit)


# --- row builders ----------------------------------------------------------


def row_length(w: World, i: int, x: float, y: float, z: float) -> dict:
    recv = w.vec3(x, y, z)
    res = native(w, V3_LENGTH, b"()F", [w.lcell(recv)], b"java.lang.Vector3", b"length")
    return {"name": "length", "exe_addr": f"0x{V3_LENGTH:08x}", "sig": "()F",
            "this": [hx(x), hx(y), hx(z)], "args": [],
            "result": {"st0": hx(res.st0)}, "returns": "st0"}


def row_distance(w: World, i: int, this_v, arg_v) -> dict:
    recv = w.vec3(*this_v)
    arg = w.vec3(*arg_v)
    res = native(w, V3_DISTANCE, b"(Ljava.lang.Vector3;)F",
                 [w.lcell(arg), w.lcell(recv)], b"java.lang.Vector3", b"distance")
    return {"name": "distance", "exe_addr": f"0x{V3_DISTANCE:08x}",
            "sig": "(Ljava.lang.Vector3;)F",
            "this": [hx(v) for v in this_v], "args": [[hx(v) for v in arg_v]],
            "result": {"st0": hx(res.st0)}, "returns": "st0"}


def row_normalize(w: World, i: int, v) -> dict:
    recv = w.vec3(*v)
    native(w, V3_NORMALIZE, b"()V", [w.lcell(recv)], b"java.lang.Vector3", b"normalize")
    return {"name": "normalize", "exe_addr": f"0x{V3_NORMALIZE:08x}", "sig": "()V",
            "this": [hx(c) for c in v], "args": [],
            "result": {"writes": w.fields_of(recv)}, "returns": "none"}


def row_rotate_ypr(w: World, i: int, v, angles) -> dict:
    recv = w.vec3(*v)
    arg = w.ypr(*angles)
    res = native(w, V3_ROTATE_YPR, b"(Ljava.lang.Ypr;)Ljava.lang.Vector3;",
                 [w.lcell(arg), w.lcell(recv)], b"java.lang.Vector3", b"rotate")
    return {"name": "rotate_ypr", "exe_addr": f"0x{V3_ROTATE_YPR:08x}",
            "sig": "(Ljava.lang.Ypr;)Ljava.lang.Vector3;",
            "this": [hx(c) for c in v], "args": [[hx(a) for a in angles]],
            "result": {"writes": w.fields_of(recv)}, "returns": "this",
            "eax_is_receiver": res.eax == recv}


def row_rotate_axis(w: World, i: int, v, axis, angle: float) -> dict:
    recv = w.vec3(*v)
    ax = w.vec3(*axis)
    cells = [w.fcell(angle, b"a"), w.lcell(ax), w.lcell(recv)]
    res = native(w, V3_ROTATE_AXIS, b"(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
                 cells, b"java.lang.Vector3", b"rotate")
    return {"name": "rotate_axis", "exe_addr": f"0x{V3_ROTATE_AXIS:08x}",
            "sig": "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
            "this": [hx(c) for c in v], "args": [[hx(a) for a in axis], hx(angle)],
            "result": {"writes": w.fields_of(recv)}, "returns": "this",
            "eax_is_receiver": res.eax == recv}


def row_vec_init(w: World, i: int, angles) -> dict:
    recv = w.vec3(0.0, 0.0, 0.0)
    arg = w.ypr(*angles)
    native(w, V3_INIT_YPR, b"(Ljava.lang.Ypr;)",
           [w.lcell(arg), w.lcell(recv)], b"java.lang.Vector3", b"<init>")
    return {"name": "vec_init", "exe_addr": f"0x{V3_INIT_YPR:08x}",
            "sig": "(Ljava.lang.Ypr;)",
            "this": [hx(0.0)] * 3, "args": [[hx(a) for a in angles]],
            "result": {"writes": w.fields_of(recv)}, "returns": "none"}


def row_ypr_init(w: World, i: int, v) -> dict:
    recv = w.ypr(0.0, 0.0, 0.0)
    arg = w.vec3(*v)
    native(w, YPR_INIT_VEC, b"(Ljava.lang.Vector3;)",
           [w.lcell(arg), w.lcell(recv)], b"java.lang.Ypr", b"<init>")
    return {"name": "ypr_init", "exe_addr": f"0x{YPR_INIT_VEC:08x}",
            "sig": "(Ljava.lang.Vector3;)",
            "this": [hx(0.0)] * 3, "args": [[hx(c) for c in v]],
            "result": {"writes": w.fields_of(recv)}, "returns": "none"}


def row_ypr_mul(w: World, i: int, this_a, arg_a) -> dict:
    recv = w.ypr(*this_a)
    arg = w.ypr(*arg_a)
    native(w, YPR_MUL, b"(Ljava.lang.Ypr;)V",
           [w.lcell(arg), w.lcell(recv)], b"java.lang.Ypr", b"mul")
    return {"name": "ypr_mul", "exe_addr": f"0x{YPR_MUL:08x}", "sig": "(Ljava.lang.Ypr;)V",
            "this": [hx(a) for a in this_a], "args": [[hx(a) for a in arg_a]],
            "result": {"writes": w.fields_of(recv)}, "returns": "none"}


def row_mat3_from_ypr(w: World, i: int, angles) -> dict:
    src = w.o.alloc(16)
    w.o.write_f32s(src, [f32(a) for a in angles])
    dst = w.o.scratch(64)
    w.o.call(MAT3_FROM_YPR, [src], "thiscall", this=dst, limit=200_000)
    m = [hx(w.o.read_u32(dst + off)) for off in MATRIX_OFFS]
    return {"name": "mat3_from_ypr", "exe_addr": f"0x{MAT3_FROM_YPR:08x}",
            "sig": "(Ypr)Mat3", "this": None, "args": [[hx(a) for a in angles]],
            "result": {"m": m}, "returns": "none"}


def row_mat3_mul(w: World, i: int, left, self_m) -> dict:
    """`0x54eab0`: ecx = dest/self, stack arg = left; dest = left * self."""
    lm = w.o.alloc(0x40)
    for off, v in zip(MATRIX_OFFS, left):
        w.o.write_u32(lm + off, bits(v))
    dst = w.o.scratch(64)
    for off, v in zip(MATRIX_OFFS, self_m):
        w.o.write_u32(dst + off, bits(v))
    w.o.call(MAT3_MUL, [lm], "thiscall", this=dst, limit=200_000)
    m = [hx(w.o.read_u32(dst + off)) for off in MATRIX_OFFS]
    return {"name": "mat3_mul", "exe_addr": f"0x{MAT3_MUL:08x}",
            "sig": "(Mat3,Mat3)Mat3", "this": None,
            "args": [[hx(v) for v in left], [hx(v) for v in self_m]],
            "result": {"m": m}, "returns": "none"}


def row_mat3_to_ypr(w: World, i: int, m_in) -> dict:
    src = w.o.alloc(0x40)
    for off, v in zip(MATRIX_OFFS, m_in):
        w.o.write_u32(src + off, bits(v))
    dst = w.o.scratch(16)
    w.o.call(MAT3_TO_YPR, [src], "thiscall", this=dst, limit=200_000)
    triple = [hx(w.o.read_u32(dst + 4 * k)) for k in range(3)]
    return {"name": "mat3_to_ypr", "exe_addr": f"0x{MAT3_TO_YPR:08x}",
            "sig": "(Mat3)Ypr", "this": None, "args": [[hx(v) for v in m_in]],
            "result": {"triple": triple}, "returns": "none"}


def row_vec_to_ypr(w: World, i: int, v) -> dict:
    src = w.o.alloc(16)
    w.o.write_f32s(src, [f32(c) for c in v])
    dst = w.o.scratch(16)
    w.o.call(VEC_TO_YPR, [src], "thiscall", this=dst, limit=200_000)
    triple = [hx(w.o.read_u32(dst + 4 * k)) for k in range(3)]
    return {"name": "vec_to_ypr", "exe_addr": f"0x{VEC_TO_YPR:08x}",
            "sig": "(Vec3)Ypr", "this": None, "args": [[hx(c) for c in v]],
            "result": {"triple": triple}, "returns": "none"}


def row_mat3_from_axis(w: World, i: int, axis, angle: float) -> dict:
    dst = w.o.scratch(64)
    w.o.write(dst, b"\0" * 64)
    w.o.call(MAT3_FROM_AXIS, [f32(axis[0]), f32(axis[1]), f32(axis[2]), f32(angle)],
             "thiscall", this=dst, limit=200_000)
    m = [hx(w.o.read_u32(dst + off)) for off in MATRIX_OFFS]
    return {"name": "mat3_from_axis", "exe_addr": f"0x{MAT3_FROM_AXIS:08x}",
            "sig": "(Vec3,F)Mat3", "this": None,
            "args": [[hx(c) for c in axis], hx(angle)],
            "result": {"m": m}, "returns": "none"}


# --- the case sweep --------------------------------------------------------

VEC_EDGE = [
    (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0),
    (-1.0, -2.0, -3.0), (3.0, 4.0, 0.0), (0.0, 2.0, -2.0),
    (9.536_743e-7, 1.192_092_9e-6, 0.003_906_25),
    (-0.0, -0.0, -0.0), (1e30, -1e30, 1e-30),
]
ANGLE_EDGE = [
    (0.0, 0.0, 0.0), (0.25, 0.5, 0.75), (-1.0, 0.5, -2.0),
    (3.0, -3.0, 1.5), (0.0, 1.570_796_4, 0.0), (2.0, 0.0, -2.0),
    (-0.0, -0.0, -0.0), (6.0, -6.0, 3.0),
]
MATRIX_EDGE = [
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    [1.0, 0.0, 0.0, 0.0, -1.0, 0.5, 0.0, 0.0, 1.0],
    [0.6, -0.8, 0.0, 0.8, 0.6, 0.0, 0.0, 0.0, 1.0],
    [0.5, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0],
]


def cases():
    """Yield `(index, thunk)`; each thunk builds one row with a fresh World."""
    rng = random.Random(20261018)
    n = 0

    def emit(fn, *args):
        nonlocal n
        idx = n
        n += 1
        return lambda w, fn=fn, args=args, idx=idx: fn(w, idx, *args)

    out = []
    for v in VEC_EDGE:
        out.append(emit(row_length, *v))
    for _ in range(40):
        out.append(emit(row_length, *[f32(rng.uniform(-500.0, 500.0)) for _ in range(3)]))

    for a in VEC_EDGE:
        for b in VEC_EDGE[:4]:
            out.append(emit(row_distance, a, b))
    for _ in range(40):
        out.append(emit(row_distance,
                        tuple(f32(rng.uniform(-500.0, 500.0)) for _ in range(3)),
                        tuple(f32(rng.uniform(-500.0, 500.0)) for _ in range(3))))

    for v in VEC_EDGE:
        out.append(emit(row_normalize, v))
    for _ in range(40):
        out.append(emit(row_normalize, tuple(f32(rng.uniform(-50.0, 50.0)) for _ in range(3))))

    for v in VEC_EDGE:
        for a in ANGLE_EDGE[:4]:
            out.append(emit(row_rotate_ypr, v, a))
    for _ in range(40):
        out.append(emit(row_rotate_ypr,
                        tuple(f32(rng.uniform(-10.0, 10.0)) for _ in range(3)),
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3))))

    for v in VEC_EDGE[:5]:
        for axis, ang in [((0.0, 0.0, 1.0), 0.0), ((1.0, 0.0, 0.0), 0.5),
                          ((0.6, 0.8, 0.0), -1.25)]:
            out.append(emit(row_rotate_axis, v, axis, ang))
    for _ in range(40):
        out.append(emit(row_rotate_axis,
                        tuple(f32(rng.uniform(-10.0, 10.0)) for _ in range(3)),
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3)),
                        f32(rng.uniform(-3.2, 3.2))))

    for a in ANGLE_EDGE:
        out.append(emit(row_vec_init, a))
    for _ in range(40):
        out.append(emit(row_vec_init, tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3))))

    for v in VEC_EDGE:
        out.append(emit(row_ypr_init, v))
    for _ in range(40):
        out.append(emit(row_ypr_init, tuple(f32(rng.uniform(-50.0, 50.0)) for _ in range(3))))

    for a in ANGLE_EDGE:
        for b in ANGLE_EDGE[:4]:
            out.append(emit(row_ypr_mul, a, b))
    for _ in range(40):
        out.append(emit(row_ypr_mul,
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3)),
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3))))

    for a in ANGLE_EDGE:
        out.append(emit(row_mat3_from_ypr, a))
    for _ in range(30):
        out.append(emit(row_mat3_from_ypr,
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3))))

    for m in MATRIX_EDGE:
        out.append(emit(row_mat3_to_ypr, m))
    for _ in range(40):
        out.append(emit(row_mat3_to_ypr,
                        [f32(rng.uniform(-1.0, 1.0)) for _ in range(9)]))

    for m in MATRIX_EDGE:
        for other in MATRIX_EDGE[:4]:
            out.append(emit(row_mat3_mul, m, other))
    for _ in range(30):
        out.append(emit(row_mat3_mul,
                        [f32(rng.uniform(-1.0, 1.0)) for _ in range(9)],
                        [f32(rng.uniform(-1.0, 1.0)) for _ in range(9)]))

    for v in VEC_EDGE:
        out.append(emit(row_vec_to_ypr, v))
    for _ in range(40):
        out.append(emit(row_vec_to_ypr, tuple(f32(rng.uniform(-50.0, 50.0)) for _ in range(3))))

    for axis, ang in [((0.6, 0.8, 0.0), 0.0), ((0.6, 0.8, 0.0), 0.75),
                      ((0.0, 0.0, 0.0), 1.0), ((1e-4, 0.0, 0.0), 1.0)]:
        out.append(emit(row_mat3_from_axis, axis, ang))
    for _ in range(30):
        out.append(emit(row_mat3_from_axis,
                        tuple(f32(rng.uniform(-3.0, 3.0)) for _ in range(3)),
                        f32(rng.uniform(-3.2, 3.2))))
    return out


def build_rows(exe: str):
    rows = []
    for i, thunk in enumerate(cases()):
        w = World(exe)
        row = thunk(w)
        row["id"] = i
        row["oracle"] = "unicorn"
        row["unicorn"] = UC_VER
        row["cw"] = f"0x{CW:04x}"
        row["date"] = _today()
        rows.append(row)
    return rows


def gen(exe: str) -> int:
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = build_rows(exe)
    with open(OUT_FILE, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    kinds = {}
    for r in rows:
        kinds[r["name"]] = kinds.get(r["name"], 0) + 1
    for name, count in sorted(kinds.items()):
        print(f"{name}: {count} rows")
    print(f"total {len(rows)} rows -> {OUT_FILE}")
    return 0


# `verify` re-runs the sweep and compares against the recorded rows. The
# sweep is deterministic (fixed seed), so a byte-identical replay also pins
# the harness itself against an Unicorn/fpu drift.
def verify(exe: str) -> int:
    with open(OUT_FILE) as f:
        recorded = [json.loads(line) for line in f if line.strip()]
    fresh = build_rows(exe)
    if len(recorded) != len(fresh):
        print(f"row count differs: recorded {len(recorded)} fresh {len(fresh)}")
        return 1
    bad = 0
    for want, got in zip(recorded, fresh):
        rw = dict(want)
        rg = dict(got)
        rw.pop("date", None)
        rg.pop("date", None)
        if rw != rg:
            bad += 1
            print(f"MISMATCH id={want['id']} {want['name']}\n  want {rw}\n  got  {rg}")
    print(f"verify: {len(recorded) - bad}/{len(recorded)} byte-identical")
    return 1 if bad else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    v = sub.add_parser("verify")
    v.add_argument("--exe", default=None)
    a = ap.parse_args(argv)
    exe = a.exe or _default_exe()
    if a.cmd == "gen":
        return gen(exe)
    return verify(exe)


if __name__ == "__main__":
    sys.exit(main())
