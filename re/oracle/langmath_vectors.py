#!/usr/bin/env python3
"""Unicorn oracle vectors for the M1-06 `java.lang.Math` / `Integer` / `Float`
natives (issue #33).

Every row runs the **real exe code** under Unicorn with the game's D3D9 control
word (`0x007f`, 24-bit x87 precision) on a hand-built `CallInfo` — no game
data, no JVM. The Rust consumer is `crates/slr-lang/tests/langmath_oracle.rs`;
rows land in `re/out/langmath/langmath.jsonl` (git-ignored).

Covered symbols:

    0x0047c7d0  Math.randomize()V       0x0047c7f0  Math.setrandseed(I)V
    0x0047c820  Math.random()F          0x0047c850  Math.sqrt(F)F
    0x0048_1e30 Integer.toString(I)     0x0048_1e80 Integer.toHexString(I)
    0x0048_1dd0 Float.toString(F,String)

The natives resolve their `java.lang.String` receiver/parameter through
`FUN_0042ab50`, which is hooked to answer from the row's planted buffer (a
`java.lang.String` value is its char buffer, the same shape `System.log`
relies on). The interner `FUN_004174a0` is also replaced: its callback records
the produced NUL-terminated bytes and returns 0 instead of a String object.
The rows therefore validate native formatting and the bytes handed to that
boundary, not String object identity, deduplication or lifetime. Actual String
interning remains unverified under issue #110. The `sprintf`/`sscanf` CRT
helpers and native bodies execute original code; allocation, log, clock and
thread-local-data boundaries are modeled as described below.

`Math.random` and `randomize` read CRT `rand` state (`[ptd+0x14]`, reached
through `FUN_005d7408`/`FUN_005d73fb`). The probe installs the exe's own
`rand`/`srand` bodies, so a row records the draw the exe produced from the
seed the row set, which is what the port's [`MsvcRand`] mirror reproduces.

Usage (from the repo root; venv at `~/slr-oracle-venv`):

    ~/slr-oracle-venv/bin/python re/oracle/langmath_vectors.py gen
    ~/slr-oracle-venv/bin/python re/oracle/langmath_vectors.py verify

Never writes to `reference/` or `stock/`; the exe is only read.
"""
from __future__ import annotations

import argparse
import datetime
import json
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import CW_D3D9, Oracle  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
OUT_DIR = os.path.join(REPO, "re", "out", "langmath")
OUT_FILE = os.path.join(OUT_DIR, "langmath.jsonl")

MATH_RANDOMIZE = 0x0047_C7D0
MATH_SETRANDSEED = 0x0047_C7F0
MATH_RANDOM = 0x0047_C820
MATH_SQRT = 0x0047_C850
INTEGER_TO_STRING = 0x0048_1E30
INTEGER_TO_HEX = 0x0048_1E80
FLOAT_TO_STRING = 0x0048_1DD0

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}
LOG_SINKS = (0x551140, 0x5513B0)
INTERN = 0x4174A0
STRING_FIELD = 0x42AB50
RAND = 0x5D7408
SRAND = 0x5D73FB
PER_THREAD_DATA = 0x5DD8D3
CLOCK = 0x5516C0


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


UC_VER = "2.1.4"
CW = CW_D3D9


def _today() -> str:
    return datetime.date.today().isoformat()


def f32(v: float) -> float:
    return struct.unpack("<f", struct.pack("<f", v))[0]


def bits(v: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f32(v)))[0]


def st0_f32(raw: bytes) -> int:
    """The `FSTP m32` image of an 80-bit ST0, exactly.

    `CallResult.st0` is `_ext_to_float`, which renders every NaN as positive
    `float("nan")` and so loses the sign of x87's indefinite. The native
    returns `F`, so the observable is the 32-bit store of ST0; decode the raw
    register instead. `fsqrt` runs at PC=24, so every finite result already
    has a 24-bit significand and narrowing to f32 is exact.
    """
    mant, se = struct.unpack("<QH", raw)
    sign = (se >> 15) & 1
    exp = se & 0x7FFF
    if exp == 0x7FFF:
        # inf keeps its sign; NaN narrows quiet with the payload's top 22
        # significand bits (indefinite 0xC000000000000000 -> 0x00400000).
        frac = 0 if mant == 0x8000000000000000 else (mant >> 40) & 0x7FFFFF
        return (sign << 31) | 0x7F80_0000 | frac
    # mant is the explicit-integer-bit significand; scale it with ldexp so
    # no intermediate underflows for the deep-denormal inputs.
    value = math.ldexp(mant, -(63 + (16383 - exp if exp else 16382)))
    if sign:
        value = -value
    return bits(value)


def hx(value) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, int):
        return f"{value & 0xFFFFFFFF:08x}"
    return f"{bits(value):08x}"


class World:
    """A fresh emulator with the synthetic `CallInfo` builders."""

    def __init__(self, exe: str):
        self.o = Oracle(exe, fpu_cw=CW)
        self.o.hook_alloc()
        self.interned: list[bytes] = []
        self.buffers: dict[int, int] = {}
        self.clock_ms: int = 0
        for va in LOG_SINKS:
            self.o.hook(va, lambda oc, a: None, "cdecl", 3)
        # The CRT `rand` word lives at `[ptd+0x14]`, and `FUN_005dd8d3` reaches
        # `ptd` through the thread-local storage import. Plant one block so the
        # exe's own `srand`/`rand`/`__ftol` run unmodified.
        self.ptd = self.o.alloc(0x100)
        self.o.hook(PER_THREAD_DATA, lambda oc, a: self.ptd, "cdecl", 0)
        # `randomize` seeds from `clock()`; a row names the reading instead of
        # measuring the wall clock.
        def clock(oc, a):
            return struct.unpack("<f", struct.pack("<I", self.clock_ms))[0]
        self.o.hook(CLOCK, clock, "cdecl", 0)

        def intern(oc, args):
            self.interned.append(oc.read_cstr(args[0], 0x400))
            return 0

        # `481e6b mov ecx,[esi] / call 0x4174a0`: the interner is thiscall
        # with the JVM word in `ecx` and the char buffer as its stack argument.
        self.o.hook(INTERN, intern, "thiscall", 1)

        def string_field(oc, args):
            return self.buffers.get(args[0], 0)

        self.o.hook(STRING_FIELD, string_field, "cdecl", 2)

    def cstr(self, s: bytes) -> int:
        p = self.o.alloc(len(s) + 8)
        self.o.write(p, s + b"\0")
        return p

    def plant(self, text: bytes, seed: int = 0) -> int:
        """Plant `text` as a `java.lang.String` char buffer.

        Returns the buffer pointer, which is the value the exe's marshalled
        `Ljava.lang.String;` word holds: `Float.toString` pushes slot 0
        straight into `sprintf` (`481dff push edx`), exactly like `token`
        hands its charset to `strtok` and `System.log` pushes its message.
        The object word is registered too, for any row that resolves it
        through `FUN_0042ab50`.
        """
        buf = self.cstr(text)
        self.buffers[0x2000_0000 | (0x40 + seed)] = buf
        return buf

    def cell(self, kind: str, word: int) -> int:
        p = self.o.alloc(12)
        self.o.write_u32s(p, [VAL_VT, TC[kind], word & 0xFFFFFFFF])
        return p

    def call_info(self, sig: bytes, cells: list[int], cls: bytes, name: bytes,
                  needs_jvm: bool = False) -> int:
        """A hand-built `CallInfo`.

        `needs_jvm` fills `+0` with a synthetic JVM object whose `+0x68` names
        the intern hook's `this`, which is what the exe's `FUN_004174a0` reads
        (`mov ecx,[edi+0x68]` where `edi` is the `[CallInfo+0]` word).
        """
        o = self.o
        jvm_word = 0
        if needs_jvm:
            jvm = o.alloc(0x80)
            o.write_u32(jvm + 0x68, 0x5E7A00)
            jvm_word = jvm
        sigrec = o.alloc(16)
        text = o.alloc(64)
        o.write(text, sig + b"\0")
        o.write_u32s(sigrec, [0, text, 0, 0])
        rec = o.alloc(0x20)
        o.write_u32s(rec, [0x5F08FC, 0, 0, self.cstr(cls), self.cstr(name), sigrec])
        data = o.alloc(4 * max(len(cells), 1))
        o.write_u32s(data, cells or [0])
        cont = o.alloc(16)
        o.write_u32s(cont, [max(len(cells), 1), data, len(cells), 0])
        info = o.alloc(32)
        o.write_u32s(info, [jvm_word, 0, cont, rec])
        return info

    def rand_state(self) -> int:
        """The CRT `rand` word `[ptd+0x14]`."""
        return self.o.read_u32(self.ptd + 0x14)


# --- row builders ----------------------------------------------------------


def row_randomize(w: World, i: int, clock_bits: int) -> dict:
    w.clock_ms = clock_bits
    w.o.call(MATH_RANDOMIZE, [], "cdecl")
    return {"name": "randomize", "exe_addr": f"0x{MATH_RANDOMIZE:08x}", "sig": "()V",
            "clock": hx(clock_bits), "result": {"rand_state": f"0x{w.rand_state():08x}"}}


def row_setrandseed(w: World, i: int, seed: int) -> dict:
    cells = [w.cell("I", seed), w.cell("I", 0)]
    info = w.call_info(b"(I)V", cells, b"java.lang.Math", b"setrandseed")
    w.o.call(MATH_SETRANDSEED, [info], "cdecl")
    return {"name": "setrandseed", "exe_addr": f"0x{MATH_SETRANDSEED:08x}", "sig": "(I)V",
            "seed": hx(seed), "result": {"rand_state": f"0x{w.rand_state():08x}"}}


def row_random(w: World, i: int, seed: int, draws: int) -> dict:
    cells = [w.cell("I", seed), w.cell("I", 0)]
    info = w.call_info(b"(I)V", cells, b"java.lang.Math", b"setrandseed")
    w.o.call(MATH_SETRANDSEED, [info], "cdecl")
    cells = [w.cell("I", 0)]
    info = w.call_info(b"()F", cells, b"java.lang.Math", b"random")
    out = []
    for _ in range(draws):
        res = w.o.call(MATH_RANDOM, [info], "cdecl")
        out.append(hx(res.st0))
    return {"name": "random", "exe_addr": f"0x{MATH_RANDOM:08x}", "sig": "()F",
            "seed": hx(seed), "draws": out, "result": {"rand_state": f"0x{w.rand_state():08x}"}}


def row_sqrt(w: World, i: int, value: float) -> dict:
    cells = [w.cell("F", bits(value)), w.cell("F", 0)]
    info = w.call_info(b"(F)F", cells, b"java.lang.Math", b"sqrt")
    res = w.o.call(MATH_SQRT, [info], "cdecl")
    f32 = st0_f32(res.st0_raw)
    return {"name": "sqrt", "exe_addr": f"0x{MATH_SQRT:08x}", "sig": "(F)F",
            "arg": hx(value), "result": {"st0": f"{f32:08x}",
                                         "st0_ext": res.st0_raw.hex()}}


def row_int_to_string(w: World, i: int, value: int) -> dict:
    w.interned.clear()
    cells = [w.cell("I", value), w.cell("I", 0)]
    info = w.call_info(b"(I)Ljava.lang.String;", cells, b"java.lang.Integer", b"toString",
                       needs_jvm=True)
    res = w.o.call(INTEGER_TO_STRING, [info], "cdecl")
    return {"name": "int_to_string", "exe_addr": f"0x{INTEGER_TO_STRING:08x}",
            "sig": "(I)Ljava.lang.String;", "arg": hx(value),
            "interned": [s.decode("latin1") for s in w.interned],
            "result": {"eax": f"0x{res.eax:08x}"}}


def row_int_to_hex(w: World, i: int, value: int) -> dict:
    w.interned.clear()
    cells = [w.cell("I", value), w.cell("I", 0)]
    info = w.call_info(b"(I)Ljava.lang.String;", cells, b"java.lang.Integer", b"toHexString",
                       needs_jvm=True)
    res = w.o.call(INTEGER_TO_HEX, [info], "cdecl")
    return {"name": "int_to_hex", "exe_addr": f"0x{INTEGER_TO_HEX:08x}",
            "sig": "(I)Ljava.lang.String;", "arg": hx(value),
            "interned": [s.decode("latin1") for s in w.interned],
            "result": {"eax": f"0x{res.eax:08x}"}}


def row_float_to_string(w: World, i: int, value: float, fmt: bytes) -> dict:
    w.interned.clear()
    # `481df4 mov edx,[esp+0x14]` reads the format word from slot 0 and the
    # `F` from slot 1 (`481df0 fld dword [esp+0x18]`), i.e. declaration order.
    fmt_word = w.plant(fmt, seed=1)
    cells = [w.cell("L", fmt_word), w.cell("F", bits(value)), w.cell("L", 0)]
    info = w.call_info(b"(FLjava.lang.String;)Ljava.lang.String;", cells,
                       b"java.lang.Float", b"toString", needs_jvm=True)
    res = w.o.call(FLOAT_TO_STRING, [info], "cdecl")
    return {"name": "float_to_string", "exe_addr": f"0x{FLOAT_TO_STRING:08x}",
            "sig": "(FLjava.lang.String;)Ljava.lang.String;",
            "arg": hx(value), "format": fmt.decode("latin1"),
            "interned": [s.decode("latin1") for s in w.interned],
            "result": {"eax": f"0x{res.eax:08x}"}}


def cases():
    rng = __import__("random").Random(20261018)
    out = []

    for clock in (0.0, 1.0, 1234.0, 1e6, -5.0, 2147483648.0):
        out.append((row_randomize, (bits(clock),)))

    for seed in (0, 1, 0xFFFFFFFF, 0x12345678, 0x80000000, 0x7FFFFFFF, 42):
        out.append((row_setrandseed, (seed,)))

    for seed, draws in ((1, 8), (0, 8), (0x12345678, 16), (0xFFFFFFFF, 8)):
        out.append((row_random, (seed, draws)))
    for _ in range(6):
        out.append((row_random, (rng.getrandbits(32), 16)))

    for v in (0.0, -0.0, 1.0, 2.0, 4.0, 0.5, 0.25, 3.0, 9.0, 10.0, 100.0,
              float("inf"), float("-inf"), float("nan"), -1.0, -2.0,
              1e-30, 1e30, 3.4028234663852886e38, 1.1754943508222875e-38):
        out.append((row_sqrt, (v,)))
    for _ in range(20):
        out.append((row_sqrt, (f32(rng.uniform(0.0, 1e6)),)))

    for v in (0, 1, -1, 42, -42, 0x7FFFFFFF, -0x80000000, 1234567890, -1234567890):
        out.append((row_int_to_string, (v,)))
    for _ in range(8):
        out.append((row_int_to_string, (rng.getrandbits(32) - 2**31,)))
    for v in (0, 1, -1, 0x7FFFFFFF, -0x80000000, 0xDEADBEEF, 0x1234, 0xFF):
        out.append((row_int_to_hex, (v,)))
    for _ in range(8):
        out.append((row_int_to_hex, (rng.getrandbits(32) - 2**31,)))

    # `%g`/`%e` are outside the exe's dispatch table: they write nothing and
    # consume no cell (deterministic, so they pin the default arm). `%d` is
    # deliberately absent: `Float.toString` pushes one promoted double and no
    # integer cell, so a `%d` conversion reads uninitialized caller-frame
    # bytes (`0x60000000`/`0xa0000000` observed) — undefined stack residue
    # that pins the harness, not the native.
    for fmt in (b"%.2f", b"%f", b"%g", b"%e", b"%8.2f", b"%08.3f"):
        for v in (0.0, 1.5, -2.25, 3.1415927, 1e-6, 1e6, 123456.789):
            out.append((row_float_to_string, (v, fmt)))
    for _ in range(10):
        out.append((row_float_to_string, (f32(rng.uniform(-1e5, 1e5)), b"%.3f")))

    return out


def build_rows(exe: str):
    rows = []
    for i, (fn, args) in enumerate(cases()):
        w = World(exe)
        row = fn(w, i, *args)
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
    kinds: dict[str, int] = {}
    for r in rows:
        kinds[r["name"]] = kinds.get(r["name"], 0) + 1
    for name, count in sorted(kinds.items()):
        print(f"{name}: {count} rows")
    print(f"total {len(rows)} rows -> {OUT_FILE}")
    return 0


def verify(exe: str) -> int:
    rows = build_rows(exe)
    bad = 0
    with open(OUT_FILE) as f:
        recorded = [json.loads(line) for line in f if line.strip()]
    if len(recorded) != len(rows):
        print(f"row count changed: {len(recorded)} -> {len(rows)}")
        bad += 1
    for was, now in zip(recorded, rows):
        if was != now:
            print(f"row {was.get('id')} drifted")
            bad += 1
    print("identical" if not bad else f"{bad} drifted rows")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("command", choices=("gen", "verify"))
    ap.add_argument("--exe", default=_default_exe())
    args = ap.parse_args()
    return gen(args.exe) if args.command == "gen" else verify(args.exe)


if __name__ == "__main__":
    raise SystemExit(main())
