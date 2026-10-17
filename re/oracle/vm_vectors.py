#!/usr/bin/env python3
"""Unicorn vectors for the TUFA operator/assign helpers (M1-01, issue #28).

Calls each helper directly with hand-built value cells (no JVM boot needed):
  value = [vtable=0x5e734c, typecstr, payload]  (12 bytes, game-heap alloc)
  typecstr points directly at the exe's read-only type chars ('I' @0x60ab50,
  'F' @0x60ab58); helpers only read the first char.
  this  = zeroed scratch (only touched on error paths, which vectors avoid).
  DAT_0062e00c/0062deec/0062def0/0062e000/0062e010 point at the same static
  chars (helpers load result-type pointers from them).

Pop-order truth (verified against machine code + corpus, see docs):
  Thread::run pops T (stack top, RIGHT operand) then N (deeper, LEFT), and
  calls arithmetic/compare helpers as (p2=N=LEFT, p3=T=RIGHT); the result
  overwrites the LEFT cell, the RIGHT cell is released. Shift helpers
  (shr/shl) are the exception: run calls them as (p2=T=count, p3=N=value)
  and the result overwrites the VALUE cell (args[1]).

Row (JSONL under re/out/vm/, git-ignored):
  {"id","exe_addr","name","convention":"thiscall","this":"zero",
   "args":[{"vmval":{"type","values":[w]}}...], "dst":0|1,
   "result":{"type","values":[w]},
   "oracle":"unicorn","unicorn":"2.1.4","cw":"0x7f","date":...}
"dst" names the result cell. Addresses are never recorded (arena-dependent).
`gen` writes one file per helper; `verify` re-runs every row on a fresh
Oracle and requires byte-identical result words.
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402
import unicorn  # noqa: F401  (version recorded per row)

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "vm")

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C, "V": 0x60AB60}
CW = CW_D3D9  # game device-thread word
UC_VER = "2.1.4"


def _today() -> str:
    return datetime.date.today().isoformat()


def _new_oracle(exe=None) -> Oracle:
    o = Oracle(exe) if exe else Oracle()
    o.hook_alloc()

    def _realloc(oc, args):
        old, size = args[0], args[1]
        new = oc.alloc(max(size, 1))
        if old:
            n = oc.heap_sizes.get(old, 0)
            if n:
                oc.write(new, oc.read(old, min(n, size)))
        return new

    o.hook(0x54F580, _realloc, "cdecl", 2)
    # Stub the diagnostic log sinks (never on happy paths): the div-by-zero
    # paths log first and then continue, so float /0.0 vectors need these
    # to reach the fdiv. 4190d0 (fatal script error) is deliberately NOT
    # stubbed: reaching it is a loud vector failure.
    o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
    o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
    o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
    # Production shape: DAT_* globals point at a heap cell whose content is
    # the type cstring (literals/compare results resolve through it), while
    # live value cells hold the cstring directly. All helper reads observe
    # cstrings either way (single byte [[v+4]] or word [v+4] equality).
    for cell, ch in ((0x62E00C, "I"), (0x62DEF0, "I"), (0x62DEEC, "F"),
                     (0x62E000, "L"), (0x62E010, "V")):
        d = o.alloc(4)
        o.write_u32(d, TC[ch])
        o.write_u32(cell, d)
    return o


def _cell(o: Oracle, ty: str, w: int) -> int:
    p = o.alloc(12)
    o.write_u32(p, VAL_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    return p


def _call(addr: int, cells, extra=(), exe=None):
    """Call helper with cell addrs (+extra words); return (cells, oracle)."""
    o = _new_oracle(exe)
    fake, thread = o.alloc(0x100), o.alloc(0x100)
    o.write_u32(fake, thread)  # error paths read [this] as the thread
    o.write_u32(thread + 0x70, 0x60AB50)
    o.write_u32(thread + 0x74, 0x60AB58)
    addrs = [_cell(o, t, w) for (t, w) in cells]
    o.call(addr, args=addrs + list(extra), convention="thiscall",
           this=fake, fpu_cw=CW)
    out = []
    for a in addrs:
        out.append((chr(o.read_u8(o.read_u32(a + 4))), o.read_u32(a + 8)))
    return out, o


def _fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def I(v):
    return ("I", v & 0xFFFFFFFF)


def F(v):
    return ("F", _fbits(v))


def L(v):
    return ("L", v & 0xFFFFFFFF)


def suite_for(name: str):
    """name -> (dst_index, [cell-pair or cell-triple operand lists])."""
    if name == "mul":
        return 0, [(I(6), I(7)), (I(0), I(7)), (I(-3), I(7)),
                   (I(2**31 - 1), I(2)), (I(-2**31), I(-1)),
                   (I(100), I(-7)), (I(2**31 - 1), I(2**31 - 1)),
                   (F(1.5), F(2.25)), (F(-4.0), F(0.5)), (F(0.0), F(5.0)),
                   (F(-0.0), F(5.0))]
    if name == "add":
        return 0, [(I(6), I(7)), (I(0), I(0)), (I(-3), I(-4)),
                   (I(2**31 - 1), I(1)),
                   (F(1.5), F(2.25)), (F(-4.0), F(0.5)), (F(0.0), F(-0.0))]
    if name == "sub":  # dst = dst + (-src)
        return 0, [(I(6), I(7)), (I(7), I(6)), (I(0), I(0)),
                   (I(-2**31), I(1)),
                   (F(1.5), F(2.25)), (F(2.25), F(1.5)), (F(0.0), F(-0.0))]
    if name == "div":
        # int div-by-zero and INT_MIN/-1 trap (#DE) after logging: excluded
        # by construction (documented); float /0.0 -> +-inf, no trap.
        return 0, [(I(7), I(3)), (I(-7), I(3)), (I(7), I(-3)),
                   (I(0), I(5)), (I(2**31 - 1), I(2)),
                   (F(1.5), F(0.5)), (F(-4.0), F(2.0)),
                   (F(1.5), F(0.0)), (F(-2.0), F(0.0))]
    if name == "mod":  # dst %= src; src==0 logs then traps: excluded
        return 0, [(I(7), I(3)), (I(-7), I(3)), (I(7), I(-3)),
                   (I(0), I(5)), (I(2**31 - 1), I(2))]
    if name in ("bor", "band"):  # ||, && : int-only (booleanConvert first)
        return 0, [(I(0), I(0)), (I(0), I(1)), (I(1), I(0)), (I(1), I(2)),
                   (I(-1), I(0)), (I(5), I(-3))]
    if name in ("bitor", "bitxor", "bitand"):
        return 0, [(I(12), I(10)), (I(0), I(0)), (I(-1), I(1)),
                   (I(0x7FFFFFFF), I(1)), (I(0xFFFFFFFF), I(0xFFFFFFFF))]
    if name in ("shr", "shl"):  # args = (count=RIGHT, value=LEFT); dst = 1
        return 1, [(I(1), I(0x80000000)), (I(0), I(-1)), (I(31), I(-1)),
                   (I(32), I(1)), (I(33), I(1)), (I(-1), I(16)),
                   (I(2), I(-8)), (I(4), I(0xFFFFFFFF))]
    if name in ("eq", "ne"):
        return 0, [(I(6), I(7)), (I(6), I(6)), (I(-1), I(1)), (I(0), I(0)),
                   (F(1.5), F(2.25)), (F(2.0), F(2.0)), (F(0.0), F(-0.0)),
                   (F(float("nan")), F(1.0)),
                   (F(float("nan")), F(float("nan"))),
                   (F(float("inf")), F(float("inf"))),
                   (L(0x20001000), L(0x20001000)),
                   (L(0x20001000), L(0x20002000)), (L(0), L(0))]
    if name in ("gt", "ge", "lt", "le"):
        return 0, [(I(5), I(3)), (I(3), I(5)), (I(4), I(4)), (I(-1), I(1)),
                   (I(0), I(0)), (I(2**31 - 1), I(-2**31)),
                   (F(2.5), F(1.5)), (F(1.5), F(2.5)), (F(3.0), F(3.0)),
                   (F(0.0), F(-0.0)),
                   (F(float("nan")), F(1.0)),
                   (F(float("nan")), F(float("nan"))),
                   (F(1.0), F(float("nan")))]
    if name in ("neg", "lnot", "uminus", "inc", "dec"):  # unary
        return 0, [[I(0)], [I(1)], [I(-5)], [I(2**31 - 1)], [I(-2**31)],
                   [F(1.5)], [F(-0.0)], [F(0.0)]]
    if name.startswith("assign"):
        kind = {"assign23": 0x23, "assign24": 0x24, "assign25": 0x25,
                "assign26": 0x26, "assign27": 0x27, "assign28": 0x28,
                "assign2c": 0x2C, "assign2d": 0x2D, "assign2e": 0x2E}[name]
        # (dst, src); kinds 0x24/0x25/0x26 trap on zero src: excluded there.
        pairs = [(I(0), I(12345)), (I(-7), I(99)), (F(0.0), F(2.5)),
                 (F(1.0), F(-3.75)), (L(0), L(0x20001000)),
                 (L(0x20001000), L(0x20002000)), (I(0), F(2.0)),
                 (F(0.0), I(-8))]
        if name in ("assign24", "assign25", "assign26"):
            pairs = [p for p in pairs if not (p[0][0] == "I" and p[1] == I(0))]
            pairs = [(I(7), I(3)), (I(-7), I(3)), (F(6.0), F(2.5))] + pairs
        return (0, kind), pairs
    raise ValueError(name)


HELPERS = [
    # (exe addr, vector name, sub-op); names mirror docs/tufa-executor.md
    (0x41B860, "mul", 0x13), (0x41B4D0, "add", 0x10), (0x41B760, "sub", 0x0F),
    (0x41B970, "div", 0x12), (0x41BBF0, "mod", 0x11),
    (0x41BFB0, "bor", 0x01), (0x41BEA0, "band", 0x02),
    (0x41C290, "bitor", 0x03), (0x41C380, "bitxor", 0x04),
    (0x41C1A0, "bitand", 0x05),
    (0x41AFF0, "shr", 0x0D), (0x41B0E0, "shl", 0x0E),
    (0x41C5C0, "eq", 0x07), (0x41C6C0, "ne", 0x06),
    (0x41C860, "ge", 0x08), (0x41CB60, "le", 0x09),
    (0x41C790, "gt", 0x0A), (0x41C9E0, "lt", 0x0B),
    (0x41C0C0, "neg", 0x14), (0x41BDB0, "lnot", 0x15),
    (0x41B1D0, "uminus", 0x16), (0x41B2D0, "inc", None),
    (0x41B3D0, "dec", None),
    (0x41ACB0, "assign23", 0x23), (0x41ACB0, "assign24", 0x24),
    (0x41ACB0, "assign25", 0x25), (0x41ACB0, "assign26", 0x26),
    (0x41ACB0, "assign27", 0x27), (0x41ACB0, "assign28", 0x28),
    (0x41ACB0, "assign2c", 0x2C), (0x41ACB0, "assign2d", 0x2D),
    (0x41ACB0, "assign2e", 0x2E),
]
UNARY = {"neg", "lnot", "uminus", "inc", "dec"}


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for addr, name, _sub in HELPERS:
        dstinfo, pairs = suite_for(name)
        if isinstance(dstinfo, tuple):
            dst, kind = dstinfo
            extra = (kind,)
        else:
            dst, extra = dstinfo, ()
        rows = []
        for i, ops in enumerate(pairs):
            cells = [(t, w) for (t, w) in ops]
            got, _ = _call(addr, cells, extra, exe)
            rtype, rword = got[dst]
            rows.append({
                "id": i, "exe_addr": f"0x{addr:08x}", "name": name,
                "convention": "thiscall", "this": "zero",
                "args": ([{"vmval": {"type": t, "values": [w]}}
                          for (t, w) in cells] +
                         ([{"int": kind}] if extra else [])),
                "dst": dst,
                "result": {"type": rtype, "values": [rword]},
                "oracle": "unicorn", "unicorn": UC_VER,
                "cw": f"0x{CW:04x}", "date": _today()})
        path = os.path.join(OUT_DIR, f"vm_{name}.jsonl")
        with open(path, "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        total += len(rows)
        print(f"{name}: {len(rows)} rows -> {path}")
    print(f"total {total} rows, {len(HELPERS)} helpers")


def _decode_and_run(o: Oracle, row: dict):
    addr = int(row["exe_addr"], 16)
    cells = []
    extra = []
    for a in row["args"]:
        if "vmval" in a:
            v = a["vmval"]
            cells.append((v["type"], v["values"][0]))
        else:
            extra.append(a["int"])
    fake, thread = o.alloc(0x100), o.alloc(0x100)
    o.write_u32(fake, thread)
    o.write_u32(thread + 0x70, 0x60AB50)
    o.write_u32(thread + 0x74, 0x60AB58)
    addrs = [_cell(o, t, w) for (t, w) in cells]
    o.call(addr, args=addrs + extra, convention="thiscall", this=fake,
           fpu_cw=CW)
    a = addrs[row["dst"]]
    return chr(o.read_u8(o.read_u32(a + 4))), o.read_u32(a + 8)


def verify(paths, exe=None):
    bad = 0
    n = 0
    for path in paths:
        o = _new_oracle(exe)
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        for row in rows:
            n += 1
            got = _decode_and_run(o, row)
            want = (row["result"]["type"], row["result"]["values"][0])
            if got != want:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['name']}: "
                      f"want {want} got {got}")
        print(f"{path}: {len(rows) - sum(1 for _ in [])}/{len(rows)} ok")
    print(f"verify: {n - bad}/{n} byte-identical")
    return 1 if bad else 0


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe)
        return 0
    files = a.files or sorted(os.path.join(OUT_DIR, f) for f in
                              os.listdir(OUT_DIR) if f.endswith(".jsonl"))
    return verify(files, a.exe)


if __name__ == "__main__":
    sys.exit(main())
