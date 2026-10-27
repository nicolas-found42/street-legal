#!/usr/bin/env python3
"""Unicorn vectors for the native cfg keyword handlers and the exe's sscanf (M3-09, issue #50).

    ~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen sscanf     # re/out/cfg_handlers/sscanf.jsonl
    ~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py gen handlers   # re/out/cfg_handlers/handlers_*.jsonl
    ~/slr-oracle-venv/bin/python re/oracle/cfg_handler_vectors.py verify         # re-run, require identical rows

Everything written is git-ignored derived data (re/out/); nothing here embeds exe bytes.

sscanf  FUN_00553890 (wrapper FUN_00551240): the game's own sscanf, run as exe code on fuzzed and
        hand-written inputs for every format string the cfg handlers use. Each destination is a
        0x440-byte cell pre-filled with 0xAA; the row records the return value and the final cells
        (trailing 0xAA bytes trimmed).

handlers (see the second half of this file) run the keyword handlers of the table at 0x60e0f8 as exe
code against a car-class object K (0x2e98 bytes), its parameter block P (0xc1c bytes) and the heap
nodes the handlers build.  Externals are hooked and their calls/effects recorded (see HOOKS).
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import random
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.environ.get("SLR_CFG_VECTORS_OUT") or os.path.join(REPO, "re", "out", "cfg_handlers")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
UC_VER = "2.1.4"
CW = CW_D3D9           # game float code runs at x87 PC=24 on the device thread (ADR 0007)


def _exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    sys.exit("exe not found (reference/raw/StreetLegal_Redline.bak.exe)")


def _today() -> str:
    return datetime.date.today().isoformat()


def _write_jsonl(name: str, rows: list) -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    with open(os.path.join(OUT_DIR, name), "w") as f:
        for r in rows:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")


# ====================================================================== sscanf
SSCANF = 0x553890
FORMATS = [  # every distinct format string the keyword handlers pass to FUN_00551240 + probes
    "%f", "%f %f", "%f %f %f", "%f %f %f %f", "%f %f %f %f %f", "%f %f %f %f %f %f",
    "%f %f %f %f %f %f %f", "%f %f %f %f %f %f %f %f %f", "%f %f %f %f %f %f %f %f %f %f",
    "%f %f %f %f %f %f %f %f %f %f %f %f %f %f %f",
    "%f %f,%f,%f %f,%f,%f %f,%f,%f %f", "%f %s %f %f %f",
    "%d", "%d %d", "%d %d %d", "%d %f", "%d %f %d", "%d %d %f %f", "%d %d %d %f",
    "%d %f %f %f", "%d %f %f %f %f", "%d %f %f %f %f %f", "%d %f %f %f %f %f %f",
    "%d %f %f %f %f %f %f %f", "%d %f %f %f %f %f %f %f %f %f %f %f %f %f",
    "%d %d %d %d %d %d %d %d", "%f %f %f %f %f %f %d %f %f %f %f",
    "%f %f %f %f %f %f %f %s %f %f %f", "%f %f %f %f %f %f %d", "%s %f %f %f %f %f %f",
    "%d %f %f %f %d %8s", "%d %f %f %f %d %8s %f %f %f %f",
    "%#%d", "%#%f", "%#%f:%f", "%c", "%x", "%u", "%s", "%d %x", "%5s %d", "%*d %d", "a%d",
    "%d,%d", "%f%f",
]
# input fragments the fuzzer glues together (delimiters, numbers in every shape the cfg files use)
_NUM = ["0", "1", "-1", "12", "-12", "007", "255", "65535", "123456789", "4294967295", "4294967296",
        "-2147483648", "2147483648", "0x10", "0X1f", "0xffffffff", "0x", "0xg", "0x100000000",
        "0.5", "-0.5", "1.", ".5", "-.5", "+1", "+.5", "1e5", "1e-3", "3.14159", "-0", "-0.0",
        "0.1", "0.3", "9.99", "100.0", "1234.5678", "0.00006", "16777217", "16777219",
        "8388609.5", "0.000000001", "99999999999", "1.5.5", "--1", "-", ".", "abc", "x1", "nan", "inf"]
_SEP = [" ", "  ", "\t", " \t ", ",", " ,", ", ", ":", "\r", "\n"]


def _fuzz_inputs(rng: random.Random, n: int) -> list:
    out = []
    for _ in range(n):
        parts = []
        for _ in range(rng.randint(0, 16)):
            parts.append(rng.choice(_NUM))
            parts.append(rng.choice(_SEP) if rng.random() < 0.85 else "")
        s = "".join(parts)
        if rng.random() < 0.1:
            s = s.strip()
        out.append(s)
    return out


def _n_dests(fmt: str) -> int:
    # count '%' conversions that store (not '%#' and not '%*')
    n = 0
    i = 0
    while i < len(fmt):
        if fmt[i] == "%":
            if fmt[i + 1] == "#":
                i += 2
                continue
            if fmt[i + 1] == "*":
                i += 2
                while i < len(fmt) and fmt[i].isdigit():
                    i += 1
                i += 1
                continue
            n += 1
        i += 1
    return n


CELL = 0x440      # a %s without width can store up to 0x400 bytes


def run_sscanf(o: Oracle, fmt: str, inp: bytes) -> dict:
    n = _n_dests(fmt)
    o.scratch_ptr = 0x30000000                       # inputs live in the scratch arena; reuse it per row
    buf = o.scratch(len(inp) + 8)
    o.write(buf, inp + b"\0\0\0\0")
    f = o.scratch(len(fmt) + 1)
    o.write(f, fmt.encode() + b"\0")
    cells = [o.scratch(CELL) for _ in range(max(n, 1))]
    for c in cells:
        o.write(c, b"\xaa" * CELL)
    argv = o.scratch(4 * max(n, 1))
    o.write_u32s(argv, cells)
    r = o.call(SSCANF, [buf, f, argv], "cdecl", fpu_cw=CW, limit=2_000_000)
    return {"ret": r.eax - (1 << 32) if r.eax & 0x80000000 else r.eax,
            "cells": [o.read(c, CELL).rstrip(b"\xaa").hex() for c in cells[:n]]}


def gen_sscanf() -> list:
    rng = random.Random(0x5C4F)
    rows = []
    o = Oracle(_exe(), fpu_cw=CW)
    for fmt in FORMATS:
        inputs = ["", " ", "1", "1 2", "1 2 3 4 5 6 7 8 9 10 11 12 13 14 15", "ab cd", "7,8", "0x1F 0x20"]
        inputs += _fuzz_inputs(rng, 90)
        for s in inputs:
            inp = s.encode("latin-1")
            res = run_sscanf(o, fmt, inp)
            rows.append({"id": f"sscanf_{len(rows):05d}", "exe_addr": "0x553890", "name": "sscanf",
                         "fmt": fmt, "input": inp.hex(), "ret": res["ret"], "cells": res["cells"],
                         "oracle": "unicorn", "unicorn": UC_VER, "cw": hex(CW), "date": _today()})
    return rows


# ====================================================================== handlers
from oracle import HEAP_BASE, SCRATCH_BASE, STACK_TOP  # noqa: E402

TABLE_VA = 0x60E0F8
FACTORY = 0x442500        # class factory: allocates the 0x2e98-byte object K and runs its constructors
CONFIGURE = 0x446180      # car-class configure method: defaults, then the keyword table parse
DISPATCH_MEM = 0x4B6480   # FUN_004b6480(text, len, table, K): parse a text buffer through the table
DELIMS = b" \r\n\t"       # the strtok delimiter set of FUN_004b6340 (PTR_DAT_006176a4)
MASK = 0xFFFFFFFF


def f32bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bitsf32(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b & MASK))[0]


def read_table(o: Oracle, va: int = TABLE_VA):
    """(keyword, handler) pairs of a keyword table, in exe order.

    TABLE_VA (0x60e0f8) is the car-class table; 0x611368 is the part-class table
    (`FUN_0046e570` parses part cfgs into the same K through `FUN_004b6480`)."""
    rows = []
    a = va
    while True:
        k, h = struct.unpack("<II", o.read(a, 8))
        name = o.read_cstr(k).decode("latin-1")
        if name == "eof":
            break
        rows.append((name, h))
        a += 8
    return rows


class Hooks:
    """Python stand-ins for the exe functions outside the handlers' own logic.

    Every call is appended to `log` as {"f": va, "this": ecx-or-null, "a": [...], "r": ret, "w": [[addr, u32]]}
    so the Rust port can replay it (`ReplayHost`): the port must issue the same calls with the
    same arguments in the same order; the recorded return value and memory effects are applied.
    Hook results are deterministic functions of the arguments (never of arena addresses), except
    that heap objects are bump-allocated exactly like the exe's allocator wrapper.
    """

    def __init__(self, o: Oracle):
        self.o = o
        self.log: list = []
        o.hook_alloc()
        o.hook(0x544590, self.h_resolve, "thiscall", 1)        # resource id -> handle
        o.hook(0x546070, self.h_bind, "thiscall", 3)           # bind a resource ref slot
        o.hook(0x538440, self.h_lookup, "thiscall", 2)         # (name, index) -> resource id
        o.hook(0x5383F0, self.h_resname, "thiscall", 1)        # resource id -> name (diagnostics)
        o.hook(0x5513B0, self.h_log, "cdecl", 4)               # printf-style diagnostics (diag.log)
        o.hook(0x49AB60, self.h_body, "thiscall", 6)           # physics world: create primitive body
        o.hook(0x49A990, self.h_bodydef, "thiscall", 2)        # physics world: create body from click def
        o.hook(0x54ECD0, self.h_euler_to_matrix, "thiscall", 1)
        o.hook(0x551C90, self.h_matrix_to_euler, "thiscall", 1)
        self.name_buf = None

    def _rec(self, f, this, a, r=None, w=None):
        if this is not None and STACK_TOP - 0x100000 <= this < STACK_TOP + 0x1000:
            # a stack temporary (e.g. the throw-away resource ref of `click`): not state, symbolic here
            this = "local"
            w = [x for x in (w or []) if not (STACK_TOP - 0x100000 <= x[0] < STACK_TOP + 0x1000)]
        self.log.append({"f": f"0x{f:x}", "this": this, "a": a, "r": r, "w": w or []})

    def h_resolve(self, oc, a):
        r = (a[0] * 3 + 1) & MASK
        self._rec(0x544590, oc.reg("ecx"), [a[0]], r)
        return r

    def h_bind(self, oc, a):
        this = oc.reg("ecx")
        oc.write_u32(this + 8, a[0])          # model: slot[2] = handle (resource side effects not modelled)
        self._rec(0x546070, this, list(a), None, [[this + 8, a[0]]])

    def h_lookup(self, oc, a):
        name = oc.read_cstr(a[0]).decode("latin-1")
        r = (a[1] * 5 + 7) & MASK
        self._rec(0x538440, oc.reg("ecx"), [name, a[1]], r)
        return r

    def h_resname(self, oc, a):
        if self.name_buf is None:
            self.name_buf = oc.scratch(16)
            oc.write(self.name_buf, b"resname\0")
        self._rec(0x5383F0, None, [a[0]], 0)
        return self.name_buf

    def _norm(self, oc, w):
        """A stack pointer argument (a local C string) is recorded as the string."""
        if STACK_TOP - 0x100000 <= w < STACK_TOP + 0x1000:
            return oc.read_cstr(w).decode("latin-1")
        return w

    def h_log(self, oc, a):
        fmt = oc.read_cstr(a[0]).decode("latin-1")
        nw = sum(2 if m == "f" else 1 for m in re.findall(r"%[-+ #0-9.]*([a-zA-Z])", fmt))
        self._rec(0x5513B0, None, [fmt] + [self._norm(oc, w) for w in a[1:1 + nw]], 0)
        return 0

    def h_body(self, oc, a):
        this = oc.reg("ecx")
        p = oc.alloc(0xA0)
        self._rec(0x49AB60, this, [self._norm(oc, w) for w in a], p)
        return p

    def h_bodydef(self, oc, a):
        this = oc.reg("ecx")
        p = oc.alloc(0xA0)
        self._rec(0x49A990, this, [a[0]] + (oc.read_u32s(a[1], 4) if a[1] else [0, 0, 0, 0]), p)
        return p

    def h_euler_to_matrix(self, oc, a):
        import math
        this = oc.reg("ecx")
        ang = oc.read_f32s(a[0], 3)
        sx, cx = math.sin(ang[0]), math.cos(ang[0])
        sy, cy = math.sin(ang[1]), math.cos(ang[1])
        sz, cz = math.sin(ang[2]), math.cos(ang[2])
        m = [cz * cx + sz * sy * sx, cz * sy * sx - sz * cx, cy * sx, 0.0,
             sz * cy, cz * cy, -sy, 0.0,
             sz * sy * cx - cz * sx, sz * sx + cz * sy * cx, cy * cx]
        w = []
        for idx, v in zip((0, 1, 2, 4, 5, 6, 8, 9, 10), (m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10])):
            b = f32bits(v)
            oc.write_u32(this + 4 * idx, b)
            w.append([this + 4 * idx, b])
        self._rec(0x54ECD0, this, [f32bits(x) for x in ang], None, w)

    def h_matrix_to_euler(self, oc, a):
        import math
        this = oc.reg("ecx")
        m = oc.read_f32s(a[0], 11)
        out = [0.0, 0.0, 0.0]
        out[0] = math.atan2(m[2], m[10])
        out[2] = math.atan2(m[4], m[5])
        out[1] = math.atan2(-m[6], m[5])
        w = []
        for i, v in enumerate(out):
            b = f32bits(v)
            oc.write_u32(this + 4 * i, b)
            w.append([this + 4 * i, b])
        self._rec(0x551C90, this, [f32bits(x) for x in m], None, w)


def heap_words(o: Oracle, base_bytes: bytes, base_hp: int):
    """(changed 32-bit words of the heap vs `base_bytes`, current heap pointer)."""
    cur = o.read(HEAP_BASE, o.heap_ptr - HEAP_BASE)
    out = []
    n = len(cur)
    for i in range(0, n - (n % 4), 4):
        c = cur[i:i + 4]
        b = base_bytes[i:i + 4] if i + 4 <= len(base_bytes) else b"\0\0\0\0"
        if c != b:
            out.append([HEAP_BASE + i, struct.unpack("<I", c)[0]])
    return out, o.heap_ptr


class Fixture:
    """The initial state every cfg is parsed from: K from the exe's own factory + constructors, then
    FUN_00446180's default-initialisation (parameter block P etc.), captured at the moment it would
    start the keyword-table parse."""

    def __init__(self, exe: str):
        self.exe = exe
        o = Oracle(exe, fpu_cw=CW)
        self.hk = Hooks(o)
        o.hook(0x4261F0, lambda oc, a: 1.0, "cdecl", 1)          # script float variable lookup (factory)
        o.hook(0x426100, lambda oc, a: 0, "cdecl", 2)
        o.hook(0x4427E0, lambda oc, a: None, "cdecl", 0)
        txt = o.alloc(16)
        o.write(txt, b"eof\n\0")
        arg = o.scratch(64)
        o.write(arg, f"x {txt} 4\0".encode())                    # "%s %d %d": path, text pointer, length
        r = o.call(FACTORY, [arg, 0], "cdecl", fpu_cw=CW, limit=20_000_000)
        self.k = r.eax
        snap = {}

        def grab(oc, a):
            snap["mem"] = oc.read(HEAP_BASE, oc.heap_ptr - HEAP_BASE)
            snap["hp"] = oc.heap_ptr
            return 1

        o.hook(DISPATCH_MEM, grab, "cdecl", 4)
        o.call(CONFIGURE, [self.k], "fastcall", fpu_cw=CW, limit=20_000_000)
        self.mem, self.hp = snap["mem"], snap["hp"]
        self.p = struct.unpack_from("<I", self.mem, self.k - HEAP_BASE + 0x2E48)[0]
        self.table = read_table(o)
        self.o = o
        self.hk.log.clear()
        self.restore()

    def words(self):
        """Non-zero words of the initial heap image (what the Rust test loads)."""
        out = []
        for i in range(0, len(self.mem), 4):
            if self.mem[i:i + 4] != b"\0\0\0\0":
                out.append([HEAP_BASE + i, struct.unpack_from("<I", self.mem, i)[0]])
        return out

    def restore(self, stack_fill: int = 0):
        o = self.o
        if o.heap_ptr > self.hp:
            o.write(self.hp, b"\0" * (o.heap_ptr - self.hp))
        o.write(HEAP_BASE, self.mem)
        o.heap_ptr = self.hp
        o.scratch_ptr = SCRATCH_BASE
        o.write(SCRATCH_BASE, b"\0" * 0x4000)
        o.write(STACK_TOP - 0x8000, bytes([stack_fill]) * 0x7F00)   # stack garbage (nondeterminism probe)
        self.hk.log.clear()
        self.hk.name_buf = None


def split_lines(text: bytes):
    """The line loop of FUN_004b6480: each line runs up to and including its LF (or NUL)."""
    p = 0
    while p < len(text):
        q = text.find(b"\n", p)
        n = (len(text) - p) if q < 0 else (q - p + 1)
        end = p + n - (1 if q >= 0 else 0)
        yield text[p:end][:0xFF]            # strncpy(local_100, line, 0x100): at most 255 bytes + NUL
        p += n


def tokenize(line: bytes):
    """FUN_004b6340 up to the handler call: (buffer start .. keyword NUL .. args) or None."""
    b = line
    for ch in b";#/":
        i = b.find(bytes([ch]))
        if i >= 0:
            b = b[:i]
    b = b.split(b"\0", 1)[0]
    i = 0
    while i < len(b) and b[i] in DELIMS:
        i += 1
    if i >= len(b):
        return None
    j = i
    while j < len(b) and b[j] not in DELIMS:
        j += 1
    kw = b[i:j]
    buf = b[:j] + b"\0" + b[j + 1:] + b"\0\0\0\0"
    return kw, buf


def run_file(fx: Fixture, text: bytes, stack_fill: int = 0, table_va: int = TABLE_VA):
    """Run every line of `text` through its handler, in order, on one persistent state.

    `table_va` selects the exe keyword table the dispatch resolves against (car or part).
    `stack_fill`: byte the stack is refilled with before EVERY handler call, so reads of
    uninitialised locals are deterministic (0 = the port's model; another value probes them).
    Returns the list of per-line records (only lines whose keyword is in the table)."""
    fx.restore(stack_fill)
    o, hk = fx.o, fx.hk
    table = {k.lower(): (k, h) for k, h in read_table(o, table_va)}
    base = o.read(HEAP_BASE, o.heap_ptr - HEAP_BASE)
    rows = []
    for no, line in enumerate(split_lines(text), 1):
        tk = tokenize(line)
        if tk is None:
            continue
        kw, buf = tk
        k = kw.decode("latin-1").lower()
        if k == "eof":
            rows.append({"line": no, "kw": "eof"})
            break
        if k not in table:
            continue
        name, h = table[k]
        o.write(STACK_TOP - 0x8000, bytes([stack_fill]) * 0x7F00)
        o.scratch_ptr = SCRATCH_BASE + 0x100
        a = o.scratch(len(buf) + 16)
        o.write(a, buf)
        hk.log.clear()
        r = o.call(h, [a, fx.k], "cdecl", fpu_cw=CW, limit=2_000_000)
        diff, hp = heap_words(o, base, fx.hp)
        base = o.read(HEAP_BASE, o.heap_ptr - HEAP_BASE)
        other = [w for w in r.mem_writes if not _benign(w[0])]
        if other:
            raise RuntimeError(f"line {no} {name}: writes outside heap/stack/scratch: {other[:4]}")
        rows.append({"line": no, "kw": name, "exe_addr": f"0x{h:x}", "args": buf.hex(),
                     "diff": diff, "hp": hp, "calls": [dict(c) for c in hk.log]})
    return rows


def _benign(a: int) -> bool:
    return (HEAP_BASE <= a < HEAP_BASE + 0x04000000 or SCRATCH_BASE <= a < SCRATCH_BASE + 0x100000
            or STACK_TOP - 0x100000 <= a < STACK_TOP + 0x1000)


def corpus_files(root: str):
    out = []
    for dp, dn, fn in os.walk(root):
        dn.sort()
        if os.sep + "particles" in dp or dp.endswith("particles"):
            continue
        for f in sorted(fn):
            if f.lower().endswith(".cfg"):
                out.append(os.path.join(dp, f))
    return out


def gen_handlers(which: list) -> dict:
    fx = Fixture(_exe())
    os.makedirs(OUT_DIR, exist_ok=True)
    init = {"k": fx.k, "p": fx.p, "hp": fx.hp, "words": fx.words(),
            "table": [[k, f"0x{h:x}"] for k, h in fx.table]}
    with open(os.path.join(OUT_DIR, "init.json"), "w") as f:
        json.dump(init, f, separators=(",", ":"))
    stats = {}
    roots = {"stock": os.environ.get("SLR_STOCK", os.path.join(MAIN_CHECKOUT, "stock")),
             "modded": os.environ.get("SLR_REFERENCE", os.path.join(MAIN_CHECKOUT, "reference", "raw"))}
    for src in which:
        if src == "synthetic":
            continue
        root = roots[src]
        rows = []
        nondet = 0
        for path in corpus_files(root):
            text = open(path, "rb").read()
            a = run_file(fx, text, 0)
            b = run_file(fx, text, 0xCC)
            for ra, rb in zip(a, b):
                if ra != rb:
                    ra["nondet"] = True
                    nondet += 1
            rows.append({"src": src, "file": os.path.relpath(path, root), "lines": a})
        _write_jsonl(f"handlers_{src}.jsonl", rows)
        stats[src] = {"files": len(rows), "lines": sum(len(r["lines"]) for r in rows), "nondet": nondet}
        print(src, stats[src])
    return stats


# ====================================================================== leaves
# The cfg keywords the stock+modded corpus never exercises (ledger rows `ported (no test
# yet)` in slr-physics): synthetic files run through the real dispatch over random and edge
# vectors.  Car-table leaves parse through the table at TABLE_VA, part-table leaves through
# the part table at 0x611368 (both dispatch into the same K via FUN_004b6480).
PART_TABLE_VA = 0x611368

_FLOATEDGE = ["nan", "inf", "-inf", "0", "-0.0", "0.1", "1e-45", "3.5e38", "16777217",
              "16777219", "-1", "1234.5678", "6.02e23", "-273.15"]
_INTEDGE = ["0", "1", "-1", "2147483647", "-2147483648", "4294967295", "65536", "0x1f",
            "007", "abc", ""]


def _float_battery(n: int, cap: int = None) -> list:
    """Edge arg vectors for a keyword that scans `n` floats.  `cap` bounds the numeric
    tokens per line (for formats whose exe conversion count exceeds its destination count,
    where the surplus would store through uninitialised stack)."""
    out = ["", " ".join(["1"] * n), " ".join(["-1"] * n), " ".join(["nan"] * n),
           " ".join(_FLOATEDGE[:n]),
           " ".join(_FLOATEDGE[n:2 * n] if 2 * n <= len(_FLOATEDGE) else _FLOATEDGE[-n:]),
           " ".join(["3.4e38", "1e-45", "-0.0", "0.3", "7", "1e6", "2.5", "-9", "0", "4"][:n]),
           "1.5.5 " + " ".join(["2"] * max(n - 1, 0)),
           " ".join(["abc"] * n)]
    if cap is None or n + 2 <= cap:
        out.insert(-1, " ".join(["1"] * (n + 2)))
    return out


def _int_battery(n: int) -> list:
    out = ["", " ".join(["1"] * n), " ".join(["0"] * n), " ".join(["-1"] * n),
           " ".join(_INTEDGE[:n]),
           " ".join(_INTEDGE[n:2 * n] if 2 * n <= len(_INTEDGE) else _INTEDGE[-n:]),
           " ".join(["1"] * (n + 2)), " ".join(["abc"] * n)]
    return out


def _fuzz_line(rng: random.Random, nnum: int, strings: int = 0, allow_nan: bool = True) -> str:
    """A fuzzed argument list: `nnum` numbers (the format's conversion count) + `strings`
    free words, glued with the delimiters the tokenizer splits on."""
    parts = []
    for _ in range(nnum):
        t = rng.choice(_NUM) if not allow_nan or rng.random() < 0.85 else rng.choice(["nan", "inf", "-inf"])
        parts.append(t)
        parts.append(rng.choice(_SEP) if rng.random() < 0.85 else " ")
    for i in range(strings):
        n = rng.choice([0, 1, 4, 8, 10])
        parts.append("".join(rng.choice("abcdefghABCDEF019._-") for _ in range(n)))
        parts.append(" ")
    return "".join(parts).rstrip()


# keyword -> (table, needs preamble, vector builder).  `preamble` lines run first in the
# same file (wheel records need a `type`, part-slot keywords need a `slot`, `trigger` a
# live `body` record, `flapsfx` a flap node on the current slot).
LEAVES = {
    # ---- car table: wheel records
    "friction": ("car", 2, ["type 1000 sphere 1 1 1", "type 200 box 1 1 1"], lambda r: _float_battery(2)),
    "skid": ("car", 3, [], lambda r: _float_battery(3)),
    "wheeldeform": ("car", 1, ["type 1000 sphere 1 1 1", "type 200 box 1 1 1"], lambda r: _float_battery(1)),
    "arm": ("car", 7, ["type 1000 sphere 1 1 1", "type 200 box 1 1 1"], lambda r: _float_battery(7)),
    "hub": ("car", 10, ["type 1000 sphere 1 1 1", "type 200 box 1 1 1"], lambda r: _float_battery(10)),
    "instant_center": ("car", 6, ["type 1000 sphere 1 1 1"],
                       # the exe's format has ten %f but only six destinations: more than
                       # six numbers would store through uninitialised stack (not modelled).
                       lambda r: _float_battery(6, cap=6)),
    # ---- car table: vehicle scalars
    "sw_radius": ("car", 2, [], lambda r: _float_battery(2)),
    "mass": ("car", 1, [], lambda r: _float_battery(1)),
    "torque": ("car", 0, [], lambda r: [
        "", "0", "1 1000:200", "3 0:1 1e5:2.5 -3:0.5", "2 abc", "-1",
        "4 0:0 1:1 2:2 3:3", "2 1000:200 nan:inf", "1 5:6 7:8"]
        # the pair count leads the line; a huge count (e.g. `99999999999`, which sscanf
        # clamps to INT_MAX) makes the exe walk billions of pairs - an exe-side DoS shape
        # that is not a testable vector, so the count stays small and only the pairs fuzz.
        + [f"{r.randint(0, 5)} " + _fuzz_line(r, 2 * r.randint(0, 4), allow_nan=False).strip()
           for _ in range(6)]),
    "clutch": ("car", 1, [], lambda r: _float_battery(1)),
    "rpm_idle": ("car", 1, [], lambda r: _float_battery(1)),
    "drag": ("car", 4, [], lambda r: _float_battery(4)),
    "center": ("car", 3, [], lambda r: _float_battery(3)),
    "nodriver": ("car", 0, [], lambda r: ["", "1", "abc"]),
    "key": ("car", 0, [], lambda r: ["", "1234", "-1 abc defgh", "0 0", "4294967295 nan"]),
    "handbrake": ("car", 0, [], lambda r: ["", "1", "-0.5 xyz"]),
    "turn": ("car", 3, [], lambda r: _float_battery(3)),
    "velfactor": ("car", 2, [], lambda r: _float_battery(2)),
    "roll": ("car", 3, [], lambda r: _float_battery(3)),
    "pitch": ("car", 3, [], lambda r: _float_battery(3)),
    "cheat_control": ("car", 5, [], lambda r: _float_battery(5)),
    # ---- car table: HUD gauges and sound records
    "osd_gauge": ("car", 4, [], lambda r: _int_battery(1) + _float_battery(4)
                  + [_fuzz_line(r, 4) for _ in range(4)]),
    "osd_rpmpin": ("car", 8, [], lambda r: _float_battery(8)),
    "osd_spdpin": ("car", 8, [], lambda r: _float_battery(8)),
    "osd_nospin": ("car", 8, [], lambda r: _float_battery(8)),
    "osd_damage": ("car", 4, [], lambda r: _float_battery(4)),
    "osd_damage_slot": ("car", 1, [], lambda r: ["", "x", "ab", "1", " "]),
    "osd_gear": ("car", 6, [], lambda r: [
        "", "1", "1 2 3 4 5 abcdefgh", "1 2 3 4 5 abcdefghij", "1 2 3 4 -5 x",
        "-2147483648 nan inf -inf 6 z", "1 2 3"]
        + [_fuzz_line(r, 5, strings=1) for _ in range(4)]),
    "osd_speed": ("car", 6, [], lambda r: _float_battery(6) +
                  ["1 2 3 4 5 abcdefghij", "0 0 0 0 0 a"]),
    "osd_rpm": ("car", 6, [], lambda r: _float_battery(6) +
                ["1 2 3 4 5 abcdefghij", "0 0 0 0 0 a"]),
    "osd_nos": ("car", 10, [], lambda r: _float_battery(10) +
                ["1 2 3 4 5 abcdefgh 6 7 8 9", "1 2 3 4"]),
    "osd_turbo": ("car", 10, [], lambda r: _float_battery(10) +
                  ["1 2 3 4 5 abcdefgh 6 7 8 9", "1 2 3 4"]),
    "osd_mileage": ("car", 10, [], lambda r: _float_battery(10) +
                    ["1 2 3 4 5 abcdefgh 6 7 8 9", "1 2 3 4"]),
    "osd_abs": ("car", 5, [], lambda r: _float_battery(5) + ["1 2 3 4 5 abcdefghij"]),
    "osd_asr": ("car", 5, [], lambda r: _float_battery(5) + ["1 2 3 4 5 abcdefghij"]),
    "osd_hbrake": ("car", 5, [], lambda r: _float_battery(5) + ["1 2 3 4 5 abcdefghij"]),
    "osd_emergency": ("car", 5, [], lambda r: _float_battery(5) + ["1 2 3 4 5 abcdefghij"]),
    "sfx_engine_up": ("car", 6, [], lambda r: _float_battery(6)),
    "sfx_engine_down": ("car", 6, [], lambda r: _float_battery(6)),
    "sfx_exhaust": ("car", 6, [], lambda r: _float_battery(6)),
    "sfx_gearbox_fwd": ("car", 2, [], lambda r: _float_battery(2)),
    "sfx_gearbox_rev": ("car", 2, [], lambda r: _float_battery(2)),
    "sfx_wreck": ("car", 1, [], lambda r: _int_battery(1)),
    "sfx_gearchange": ("car", 2, [], lambda r: _int_battery(2)),
    # ---- car table: part-record keywords that are car-table rows here
    "shadow": ("car", 7, ["body 1 2 3 4 5 6 7 sphere 1 1 1"], lambda r: _float_battery(7)),
    "bolt": ("car", 7, ["slot 1 2 3 4 5 6 7", "slot 8 9 10 11 12 13 0"],
             lambda r: _float_battery(7)),
    "flapsfx": ("car", 2, ["slot 1 2 3 4 5 6 7", "flap 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15"],
                lambda r: _int_battery(2)),
    "slotmove": ("car", 5, ["slot 1 2 3 4 5 6 7"], lambda r: _float_battery(5)),
    "deform": ("car", 1, [], lambda r: _float_battery(1)),
    "logo": ("car", 1, [], lambda r: _int_battery(1)),
    # ---- part table (0x611368): parsed by FUN_0046e570 into the same K
    "trigger": ("part", 0, ["body 1 2 3 4 5 6 7 sphere 1 1 1"], lambda r: ["", "1", "abc"]),
    "slotturn": ("part", 6, ["slot 1 2 3 4 5 6 7"], lambda r: _float_battery(6)),
    "intdamage": ("part", 1, [], lambda r: _float_battery(1)),
    "material": ("part", 4, [], lambda r: _int_battery(3) + _float_battery(4)),
    "wing": ("part", 10, ["body 1 2 3 4 5 6 7 sphere 1 1 1"], lambda r: _float_battery(10)),
    "rollbar": ("part", 4, [], lambda r: _int_battery(2) + _float_battery(4)),
    "sfx_loop": ("part", 2, [], lambda r: _float_battery(2)),
    "skinned": ("part", 3, [], lambda r: _float_battery(3)),
}
# Duplicate-key / count-boundary stress: keywords whose handlers append bounded records
# (at most 16/8/2) or rebind slots: hammer them past the boundary.
_OVERFLOW = {"osd_rpmpin": 17, "osd_spdpin": 17, "osd_nospin": 17, "sfx_engine_up": 17,
             "sfx_engine_down": 17, "sfx_exhaust": 17, "rollbar": 4, "slot": 3, "type": 17}


def _leaf_vectors(kw: str, rng: random.Random) -> list:
    """Hand-written edge vectors plus a seeded fuzz tail shaped by LEAVES_NNUM/LEAVES_SSTR."""
    vecs = LEAVES[kw][3](rng)
    vecs += [_fuzz_line(rng, LEAVES_NNUM.get(kw, 4), strings=LEAVES_SSTR.get(kw, 0))
             for _ in range(6)]
    return vecs


LEAVES_NNUM = {"friction": 2, "skid": 3, "wheeldeform": 1, "arm": 7, "hub": 10,
               "instant_center": 6, "sw_radius": 2, "mass": 1, "clutch": 1, "rpm_idle": 1,
               "drag": 4, "center": 3, "turn": 3, "velfactor": 2, "roll": 3, "pitch": 3,
               "cheat_control": 5, "osd_gauge": 4, "osd_rpmpin": 8, "osd_spdpin": 8,
               "osd_nospin": 8, "osd_damage": 4, "osd_gear": 5, "osd_speed": 5,
               "osd_rpm": 5, "osd_nos": 9, "osd_turbo": 9, "osd_mileage": 9, "osd_abs": 5,
               "osd_asr": 5, "osd_hbrake": 5, "osd_emergency": 5, "sfx_engine_up": 6,
               "sfx_engine_down": 6, "sfx_exhaust": 6, "sfx_gearbox_fwd": 2,
               "sfx_gearbox_rev": 2, "sfx_wreck": 1, "sfx_gearchange": 2, "shadow": 7,
               "bolt": 7, "flapsfx": 2, "slotmove": 5, "slotturn": 6, "deform": 1,
               "intdamage": 1, "material": 4, "wing": 10, "rollbar": 4, "sfx_loop": 2, "skinned": 3,
               "key": 1, "handbrake": 1, "osd_damage_slot": 1}
LEAVES_SSTR = {"osd_gear": 1, "osd_speed": 1, "osd_rpm": 1, "osd_nos": 1, "osd_turbo": 1,
               "osd_mileage": 1, "osd_abs": 1, "osd_asr": 1, "osd_hbrake": 1,
               "osd_emergency": 1}


def _leaf_file(kw: str, vecs: list, overflow: int) -> list:
    """(keyword, args) lines of one leaf scenario: preamble, the battery, the keyword again
    after the preamble state evolved (duplicate-key re-entry), the overflow block."""
    preamble = LEAVES[kw][2]
    lines = list(preamble)
    for v in vecs:
        lines.append((kw + " " + v).rstrip())
    if overflow:
        for i in range(overflow):
            lines.append(f"{kw} {i % 7} {0.5 * (i % 5 + 1)} extra{i % 3}")
        lines.append((kw + " 8 9 tail").rstrip())
    else:
        lines.append((kw + " 42 0.25 reentry").rstrip())
    return lines


def gen_leaves() -> dict:
    fx = Fixture(_exe())
    rng = random.Random(0x13A7)
    os.makedirs(OUT_DIR, exist_ok=True)
    stats = {}
    files = {"car": "leaves_car.jsonl", "part": "leaves_part.jsonl"}
    texts = {"car": [], "part": []}
    for kw, (table, _n, _pre, _build) in LEAVES.items():
        vecs = _leaf_vectors(kw, rng)
        texts[table].append((kw, _leaf_file(kw, vecs, _OVERFLOW.get(kw, 0))))
    seen = {t: set() for t in texts}
    for table, name in files.items():
        rows = []
        for kw, lines in texts[table]:
            text = ("\n".join(lines) + "\n").encode("latin-1")
            a = run_file(fx, text, 0, PART_TABLE_VA if table == "part" else TABLE_VA)
            b = run_file(fx, text, 0xCC, PART_TABLE_VA if table == "part" else TABLE_VA)
            for ra, rb in zip(a, b):
                if ra != rb:
                    ra["nondet"] = True
            rows.append({"table": table, "leaf": kw, "lines": a})
            seen[table].update(r["kw"] for r in a if r.get("kw"))
        _write_jsonl(name, rows)
        n_lines = sum(len(r["lines"]) for r in rows)
        stats[table] = {"keywords": len(rows), "lines": n_lines}
        print(table, stats[table])
    missing = {kw for kw in LEAVES if kw not in seen[LEAVES[kw][0]]}
    if missing:
        raise RuntimeError(f"leaf keywords with no dispatch row (not in their table?): {sorted(missing)}")
    return stats


# ====================================================================== direct helpers
# The six slr-physics cfg helpers that are not keyword handlers, called directly on
# scratch cells ( Unicorn captures the raw words and the return registers).
DIRECT_LIST_HEAD = 0x428fd0     # __fastcall, ECX = a
DIRECT_LIST_NODE = 0x429130     # __fastcall, ECX = a
DIRECT_VEC3_ZERO = 0x551c60     # __fastcall, ECX = a
DIRECT_VEC3_SET = 0x551c70      # __thiscall, ECX = a, (x, y, z) on the stack
DIRECT_REF_RELEASE = 0x4776f0   # __fastcall, ECX = slot
DIRECT_STRICMP = 0x5d7190       # cdecl, (a, b)


def gen_direct() -> dict:
    rng = random.Random(0x13A7)
    o = Oracle(_exe(), fpu_cw=CW)
    rows = []

    def cell(n_words: int, words: list) -> int:
        p = o.scratch(4 * n_words + 16)
        o.write_u32s(p, words + [0] * (n_words - len(words)))
        return p

    def record(name: str, va: int, conv: str, this, args: list, p: int, words: list, r):
        n = len(words)
        out = o.read_u32s(p, n)
        row = {"id": f"direct_{len(rows):05d}", "name": name, "exe_addr": f"0x{va:x}",
               "conv": conv, "addr": p, "n_words": n,
               "in_words": words[:n], "out_words": out,
               "eax": r.eax, "edx": r.edx, "esp_delta": r.esp_delta,
               "oracle": "unicorn", "unicorn": UC_VER, "cw": hex(CW), "date": _today()}
        if this is not None:
            row["this"] = this
        if args:
            row["args"] = list(args)
        rows.append(row)

    def garbage(n: int) -> list:
        # deterministic mixed garbage: a ctor must overwrite every word it owns, and the
        # capture must show which words the exe actually writes
        return [rng.choice([0, 0xCC, rng.randrange(0x100000000), 0xFFFFFFF0 + i])
                for i in range(n)]

    # ---- list_head_ctor / list_node_ctor / vec3_zero: one 28/12-byte cell of garbage
    for name, va, n in (("list_head_ctor", DIRECT_LIST_HEAD, 7),
                        ("list_node_ctor", DIRECT_LIST_NODE, 3),
                        ("vec3_zero", DIRECT_VEC3_ZERO, 3)):
        for _ in range(6):
            words = garbage(n)
            p = cell(n, words)
            r = o.call(va, [], "fastcall", regs={"ecx": p}, fpu_cw=CW)
            record(name, va, "fastcall", p, [], p, words, r)

    # ---- vec3_set: thiscall with three raw words (float bit patterns included)
    set_args = [0x7FC00000, 0x7F800000, 0xFF800000, 0x00000000, 0x80000000, 0x3F800000,
                0x3E99999A, 0x7F7FFFFF, 0x00000001, 0xCC]
    for _ in range(8):
        words = garbage(3)
        p = cell(3, words)
        a = [rng.choice(set_args + [rng.randrange(0x100000000)]) for _ in range(3)]
        r = o.call(DIRECT_VEC3_SET, a, "thiscall", this=p, fpu_cw=CW)
        record("vec3_set", DIRECT_VEC3_SET, "thiscall", p, a, p, words, r)

    # ---- ref_release: owner {+0xc chain head, +0x10 refcount}, slots {owner, next}
    def ref_case(head: int, nexts: list, release: int, refcount: int):
        """owner + len(nexts) slots; slot i has next nexts[i]; release slot #`release`
        (or a foreign slot when release == len(nexts))."""
        n = len(nexts)
        words = garbage(2 + 2 * n + 8)     # [owner(2), slots(2n), spare(8)]
        p = cell(2 + 2 * n + 8, words)
        owner = p
        slots = [p + 8 + 8 * i for i in range(n)]
        foreign = p + 8 + 8 * n
        o.write_u32s(owner, [words[0], words[1]])
        o.write_u32(owner + 0xC, slots[head] if head is not None and n else 0)
        o.write_u32(owner + 0x10, refcount & 0xFFFFFFFF)
        for i, s in enumerate(slots):
            nxt = nexts[i]
            o.write_u32s(s, [owner, slots[nxt] if nxt is not None else 0])
        o.write_u32s(foreign, [owner, 0])
        target = foreign if release >= n else slots[release]
        # `in_words` must be the memory the call actually sees. The layout above
        # writes owner and slot words that the garbage list does not carry.
        words = o.read_u32s(p, len(words))
        r = o.call(DIRECT_REF_RELEASE, [], "fastcall", regs={"ecx": target}, fpu_cw=CW)
        record("ref_release", DIRECT_REF_RELEASE, "fastcall", target, [], p, words, r)

    ref_case(None, [], 0, 1)                    # empty chain: early return, no writes
    ref_case(None, [], 0, 0)                    # owner == chain head NULL
    ref_case(0, [None], 0, 1)                   # single slot at the head
    ref_case(0, [1, None], 0, 2)                # head of two
    ref_case(0, [1, None], 1, 2)                # tail of two
    ref_case(0, [1, 2, None], 1, 3)             # middle of three
    ref_case(0, [1, None], 1, 0)                # refcount 0 -> 0xffffffff
    ref_case(0, [None], 1, 5)                   # foreign slot: no writes
    ref_case(0, [1, 2, None], 3, 3)             # foreign slot behind a chain: no writes
    ref_case(0, [None, None], 0, 2)             # two disjoint singletons, release head
    ref_case(0, [None, 0], 1, 2)                # slot 1 -> slot 0 cycle tail
    ref_case(0, [0], 0, 1)                      # self-cycle
    for _ in range(8):                          # random valid chains
        n = rng.randint(1, 6)
        order = list(range(n))
        rng.shuffle(order)
        # build a 0-terminated chain walking the shuffled order: nexts[order[k]] = order[k+1]
        nexts = [None] * n
        for k in range(n - 1):
            nexts[order[k]] = order[k + 1]
        ref_case(order[0], nexts, rng.randint(0, n), rng.choice([0, 1, 2, 5, 0xFFFFFFFF]))

    # ---- stricmp: (a, b) C strings; the port asserts the equality bit, the oracle the
    # full int3 return.  The exe's locale branch (DAT_0079d6b0) is 0 in the file image and
    # at cfg-parse time (setlocale is never called), so the pure byte loop runs.
    assert o.read_u32(0x79D6B0) == 0, "exe stricmp locale flag must be the default"
    str_pairs = [
        (b"driver", b"driver"), (b"DRIVER", b"driver"), (b"driver", b"DRIVER"),
        (b"Driver", b"dRiVeR"), (b"driverx", b"driver"), (b"driver", b"driverx"),
        (b"drive", b"driver"), (b"driver", b"drive"), (b"", b""), (b"", b"a"),
        (b"a", b""), (b"driver\0x", b"driver"), (b"driver", b"driver\0x"),
        (b"sphere", b"sphere"), (b"ball", b"sphere"), (b"BOX", b"box"),
        (b"rodx", b"rody"), (b"a", b"z"), (b"z", b"a"), (b"1", b"2"),
        (b"\x80", b"\x80"), (b"\x80", b"\xa0"), (b"\xc1", b"\xe1"), (b"\xe1", b"\xc1"),
        (b"a.b", b"a b"), (b"osd_gear", b"osd_gear"), (b"osd_gea", b"osd_gear"),
        (b" ", b" "), (b" ", b""), (b"~", b"\x01"),
    ]
    alpha = b"abcXYZ019_. -/\\:>~"
    for _ in range(40):
        na, nb = rng.randint(0, 12), rng.randint(0, 12)
        a = bytes(rng.choice(alpha) for _ in range(na))
        b = bytes(rng.choice(alpha) for _ in range(nb))
        if rng.random() < 0.4 and a:
            b = a[:nb]  # force prefixes / equal shapes
        str_pairs.append((a, b))
    for a, b in str_pairs:
        pa = o.scratch(len(a) + 8)
        o.write(pa, a + b"\0" * 8)
        pb = o.scratch(len(b) + 8)
        o.write(pb, b + b"\0" * 8)
        r = o.call(DIRECT_STRICMP, [pa, pb], "cdecl", fpu_cw=CW)
        rows.append({"id": f"direct_{len(rows):05d}", "name": "stricmp_eq",
                     "exe_addr": f"0x{DIRECT_STRICMP:x}", "conv": "cdecl",
                     "a": a.hex(), "b": b.hex(), "eax": r.eax,
                     "esp_delta": r.esp_delta,
                     "oracle": "unicorn", "unicorn": UC_VER, "cw": hex(CW),
                     "date": _today()})

    os.makedirs(OUT_DIR, exist_ok=True)
    with open(os.path.join(OUT_DIR, "leaves_direct.jsonl"), "w") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":")) + "\n")
    stats = {}
    for row in rows:
        stats[row["name"]] = stats.get(row["name"], 0) + 1
    print("direct", stats)
    return stats


# ====================================================================== main
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("what", choices=["sscanf", "handlers", "leaves", "direct", "all"])
    sub.add_parser("verify")
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        if a.what in ("sscanf", "all"):
            rows = gen_sscanf()
            _write_jsonl("sscanf.jsonl", rows)
            print(f"sscanf: {len(rows)} rows")
        if a.what in ("handlers", "all"):
            gen_handlers(["stock", "modded"])
        if a.what in ("leaves", "all"):
            gen_leaves()
        if a.what in ("direct", "all"):
            gen_direct()
    return 0


if __name__ == "__main__":
    sys.exit(main())
