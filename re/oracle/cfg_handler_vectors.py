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
OUT_DIR = os.path.join(REPO, "re", "out", "cfg_handlers")
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


def read_table(o: Oracle):
    """(keyword, handler) pairs of the keyword table, in exe order."""
    rows = []
    a = TABLE_VA
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


def run_file(fx: Fixture, text: bytes, stack_fill: int = 0):
    """Run every line of `text` through its handler, in order, on one persistent state.

    `stack_fill`: byte the stack is refilled with before EVERY handler call, so reads of
    uninitialised locals are deterministic (0 = the port's model; another value probes them).
    Returns the list of per-line records (only lines whose keyword is in the table)."""
    fx.restore(stack_fill)
    o, hk = fx.o, fx.hk
    table = {k.lower(): (k, h) for k, h in fx.table}
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


# ====================================================================== main
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("what", choices=["sscanf", "handlers", "all"])
    sub.add_parser("verify")
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        if a.what in ("sscanf", "all"):
            rows = gen_sscanf()
            _write_jsonl("sscanf.jsonl", rows)
            print(f"sscanf: {len(rows)} rows")
        if a.what in ("handlers", "all"):
            gen_handlers(["stock", "modded"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
