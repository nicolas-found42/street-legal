#!/usr/bin/env python3
"""Unicorn vectors for TUFA string/new/call-adjacent helpers (M2 follow-up).

Runs the REAL exe functions in emulator memory (no JVM boot, no game data).
Complements `vm_vectors.py` (I/F numerics), `vmval_vectors.py` (cells) and
`vmclass_vectors.py` (interner M1-04: FUN_0041e4d0/0041e610 registrar
FUN_00416b00, boot binds FUN_00423e10 — reused here verbatim).

Suites with vectors (all byte-identical round-trip via `verify`):

- `stradd` (FUN_0041b4d0, sub 0x10 `+`, int/float paths only): mirrors the
  `vm_vectors.py` cell pattern (`[vtable 0x5e734c][cstr][payload]`, type
  cstrings point at the exe's static chars). 7 rows.
- `strcheck` (FUN_00417a70, the `+` string-concatenation gate): both sides
  non-String (I/I, I/F, F/I, F/F) returns early with operands untouched;
  both sides `Ljava.lang.String;` (type cells bound through a booted
  interner + FUN_00423e10, production shape) also returns early. The
  observable result is the two operand cells, byte-identical. 5 rows.
  (eax is a heap address on these paths — arena-dependent, not recorded.)
- `strconcat` (FUN_0041b4d0 L-String path, null payloads only): with both
  sides bound-`String`/null the exe takes the `FUN_004174a0(null) = 0`
  path and stores payload 0, keeping the `String` type. 2 rows.
- `conv` (FUN_00417da0, the conversion all string/cast paths funnel
  through): I->F, F->I (truncating `__ftol`), same-pointer identity
  (`pcVar3 == pcVar1` pointer equality → 1 without touching the cell),
  same-spelling/different-pointer (→ 0 — identity is pointer equality,
  not spelling equality), null-`L` retarget across descriptors (→ 1, cell
  re-pointed, no class state touched). Result = eax + cell (type recorded
  as bytes, never as an arena address). 9 rows.

Row (JSONL under re/out/vmstr/, git-ignored):
  {"id","exe_addr","name","convention":"thiscall"/"cdecl","this",
   "args":[{"vmval":{"type","values":[w]}}...],"dst",
   "result":{"cells":[{"type","values":[w]}],"eax":int|null},
   "oracle":"unicorn","unicorn":"2.1.4","cw":"0x007f","date":...}
`verify` re-runs every row on a fresh `Oracle` and requires byte-identical
eax (when recorded) and cell (type bytes + payload) words.

HONEST BLOCKERS (no vectors; probed, not faked):

- Non-null string concatenation (`local_24`/`pcVar7` both non-null →
  `malloc` + copy + `FUN_004174a0` intern): needs live `String` objects
  with char storage (`FUN_00408800` field access over a loaded
  `java.lang.String`, whose instance layout comes from the class loader)
  plus class state inside `FUN_004174a0` (`FUN_0041eb70`/`FUN_00404e20`/
  `FUN_0041e800`). Missing: a booted JVM ctx (class table, package roots,
  loaded `java.lang.String`) — M1-13 booted state.
- Mixed String/int|float `+` (exactly one side `String` → `FUN_00417da0`
  conversion of the other side): the I/F→String leg calls
  `FUN_00411610` (class-table lookup over the boot-built table) and
  crashes under the oracle at `0x411682` (unmapped class-table deref;
  probed L-null/I). Missing: same booted class table with
  `java.lang.String` loaded.
- `new` (0x1e: `FUN_00410890` class init → `FUN_00404e20` sizing →
  `FUN_00423c00` + `FUN_00423e60`): `FUN_00410890` strips `L...;` then
  needs `FUN_00411610` (class table) or `FUN_00410630` (load from file —
  needs the game data files + package roots). Missing: booted loader.
- `new T[]` (0x1f), `instanceof` (0x20), casts (0x21 `FUN_00417da0`/
  `FUN_00417d80` on non-null refs): 1-D array build needs element-type
  lookup; `instanceof`/cast on live refs need the `FUN_00405400`
  hierarchy walk over loaded classes. The null-ref legs work without
  state (covered by `conv` rows 6–7) but a full op-level vector needs the
  dispatch-time class context (`frame+0x34` pool, `0x1006`/`0x1019`
  operand nodes) — i.e. a live `Thread::run`, M1-13.
- Call protocols (`0x4025/0x4026` → `FUN_00408a30`/`FUN_004207c0`,
  `0x0021–0x0024` → `FUN_0041fbc0`, incl. native targets via the
  registry): `FUN_004207c0` resolves through `FUN_00404910` over the
  thread-local method table (`FUN_0041dcb0`/`FUN_0041def0`) and
  `FUN_0041fbc0` needs live frames, the operand stack and a loaded
  method record (script path) or a registry hit + native impl (native
  path). Probed: zeroed-thread calls crash in prologue derefs
  (`0x4207d3`, `0x41f7d3`); the null-receiver leg reaches the
  (deliberately unstubbed) `FUN_004190d0` fatal. Missing: booted thread +
  loaded classes/methods — M1-13.

Usage: vmstr_vectors.py gen [--exe ...] | verify [files] [--exe ...]
  (output re/out/vmstr/)
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
OUT_DIR = os.path.join(REPO, "re", "out", "vmstr")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58}
ADD = 0x41B4D0
STRCHECK = 0x417A70
CONV = 0x417DA0
E4D0 = 0x41E4D0
E610 = 0x41E610
BIND = 0x423E10
F210 = 0x41F210
CW = CW_D3D9  # game device-thread word
UC_VER = "2.1.4"


def _today() -> str:
    return datetime.date.today().isoformat()


def _fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        if not root:
            continue
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _new_oracle(exe=None) -> Oracle:
    o = Oracle(exe or _default_exe())
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
    # Stub the diagnostic log sinks (never on happy paths). 4190d0 (fatal
    # script error) is deliberately NOT stubbed: reaching it is a loud
    # vector failure.
    o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
    o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
    o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
    o.hook(F210, lambda oc, a: 0, "fastcall", 0)
    # Production shape (as in vm_vectors.py): DAT_* globals point at a
    # heap cell holding the type cstring.
    for cell, ch in ((0x62E00C, "I"), (0x62DEF0, "I"), (0x62DEEC, "F"),
                     (0x62E000, "L"), (0x62E010, "V")):
        d = o.alloc(4)
        o.write_u32(d, TC.get(ch, 0x60AB5C))
        o.write_u32(cell, d)
    return o


class Ctx:
    """Per-oracle builder: interner holder + bound-descriptor cache.

    `bound(spelling)` returns a heap pointer at the interned NUL bytes,
    exactly what `FUN_00423e10` leaves in the boot cell. Comparisons are
    always by bytes, never by address.
    """

    def __init__(self, o: Oracle):
        self.o = o
        self.holder = o.alloc(8)
        o.call(E4D0, args=[], convention="thiscall", this=self.holder)
        self.cache: dict[str, int] = {}

    def bound(self, spelling: str) -> int:
        if spelling not in self.cache:
            o = self.o
            s = spelling.encode("latin1")
            cell = o.alloc(4)
            cs = o.alloc(len(s) + 1)
            o.write(cs, s + b"\0")
            jvm = o.alloc(0x80)
            o.write_u32(jvm + 0x68, self.holder)
            o.call(BIND, args=[jvm, cs], convention="thiscall", this=cell)
            self.cache[spelling] = o.read_u32(cell)
        return self.cache[spelling]

    def static_cstr(self, spelling: str) -> int:
        o = self.o
        p = o.alloc(len(spelling) + 1)
        o.write(p, spelling.encode("latin1") + b"\0")
        return p

    def typeptr(self, spelling: str) -> int:
        # 'I'/'F' use the exe's static chars (vm_vectors.py shape);
        # reference spellings go through the booted interner bind.
        if spelling in TC:
            return TC[spelling]
        return self.bound(spelling)

    def cell(self, spelling: str, w: int, fresh_type: bool = False) -> int:
        o = self.o
        p = o.alloc(12)
        o.write_u32(p, VAL_VT)
        tp = (self.static_cstr(spelling) if fresh_type
              else self.typeptr(spelling))
        o.write_u32(p + 4, tp)
        o.write_u32(p + 8, w & 0xFFFFFFFF)
        return p

    def fake_thread(self) -> int:
        o = self.o
        fake, thread = o.alloc(0x100), o.alloc(0x100)
        o.write_u32(fake, thread)  # error paths read [this] as the thread
        o.write_u32(thread + 0x70, 0x60AB50)
        o.write_u32(thread + 0x74, 0x60AB58)
        return fake


def _read_cell(o: Oracle, addr: int, spelling: str):
    """(type bytes, payload word) for a cell whose type has this spelling."""
    tp = o.read_u32(addr + 4)
    n = len(spelling.encode("latin1"))
    return bytes(o.read(tp, n)).decode("latin1"), o.read_u32(addr + 8)


# ---------------------------------------------------------------- suites
STR = "Ljava.lang.String;"


def suite_rows():
    """(file, addr, convention, [row-spec]) — row-spec is opaque per suite."""
    add_cases = [
        ("I", 6, "I", 7), ("I", 0, "I", 0),
        ("I", 0xFFFFFFFD, "I", 0xFFFFFFFC),
        ("I", 0x7FFFFFFF, "I", 1),
        ("F", _fbits(1.5), "F", _fbits(2.25)),
        ("F", _fbits(-4.0), "F", _fbits(0.5)),
        ("F", _fbits(0.0), "F", _fbits(-0.0)),
    ]
    check_cases = [
        ("I", 6, "I", 7), ("I", 6, "F", _fbits(1.5)),
        ("F", _fbits(1.5), "I", 6),
        ("F", _fbits(1.5), "F", _fbits(2.0)),
        (STR, 0, STR, 0),
    ]
    concat_cases = [(STR, 0, STR, 0, "bound"), (STR, 0, STR, 0, "heap")]
    conv_cases = [
        # (src spelling, src word, target: spelling | "same", note)
        ("I", 6, "F"),          # int -> float, exact
        ("I", 0xFFFFFFFD, "F"),  # -3 -> -3.0f, exact
        ("F", _fbits(1.5), "I"),  # float -> int, truncates to 1
        ("F", _fbits(2.75), "I"),  # truncates to 2
        ("I", 6, "same"),        # same pointer identity -> 1
        ("I", 6, "I"),           # same spelling, distinct pointers -> 0
        ("F", _fbits(1.5), "same"),
        ("F", _fbits(1.5), "F"),  # distinct -> 0
        (STR, 0, "Ljava.lang.Object;"),  # null ref retarget -> 1
    ]
    return [
        ("vmstr_add", ADD, "thiscall", add_cases),
        ("vmstr_strcheck", STRCHECK, "thiscall", check_cases),
        ("vmstr_concat", ADD, "thiscall", concat_cases),
        ("vmstr_conv", CONV, "cdecl", conv_cases),
    ]


def _run_add(o: Oracle, ctx: Ctx, spec):
    lt, lw, rt, rw = spec
    fake = ctx.fake_thread()
    a = ctx.cell(lt, lw)
    b = ctx.cell(rt, rw)
    r = o.call(ADD, args=[a, b], convention="thiscall", this=fake,
               fpu_cw=CW)
    got = [_read_cell(o, a, lt), _read_cell(o, b, rt)]
    return got, None, [a, b], [lt, rt]


def _run_check(o: Oracle, ctx: Ctx, spec):
    lt, lw, rt, rw = spec
    fake = ctx.fake_thread()
    a = ctx.cell(lt, lw)
    b = ctx.cell(rt, rw)
    o.call(STRCHECK, args=[a, b], convention="thiscall", this=fake,
           fpu_cw=CW)
    got = [_read_cell(o, a, lt), _read_cell(o, b, rt)]
    return got, None, [a, b], [lt, rt]


def _run_concat(o: Oracle, ctx: Ctx, spec):
    lt, lw, rt, rw, kind = spec
    fake = ctx.fake_thread()
    # "bound": the descriptor pointer is the interned one `FUN_00423e10`
    # leaves in a boot cell. "heap": a fresh heap copy of the same bytes.
    # The exe compares 0x13 bytes of the type name, so both are accepted;
    # the two rows prove the comparison is content-, not address-based.
    fresh = kind == "heap"
    a = ctx.cell(lt, lw, fresh_type=fresh)
    b = ctx.cell(rt, rw, fresh_type=fresh)
    r = o.call(ADD, args=[a, b], convention="thiscall", this=fake,
               fpu_cw=CW)
    got = [_read_cell(o, a, lt)]
    return got, r.eax, [a], [lt]


def _run_conv(o: Oracle, ctx: Ctx, spec):
    st, sw, target = spec
    src = ctx.static_cstr(st)
    cell = ctx.cell(st, sw) if st in TC else None
    if st not in TC:
        # reference source: fresh bytes (distinct pointer from target)
        cell = ctx.o.alloc(12)
        ctx.o.write_u32(cell, VAL_VT)
        ctx.o.write_u32(cell + 4, src)
        ctx.o.write_u32(cell + 8, sw & 0xFFFFFFFF)
    if target == "same":
        # Identity means the target pointer IS the cell's own type pointer
        # (`FUN_00417da0` compares `*pp` against `[cell+4]` at 0x417dbc).
        tgt = o.read_u32(cell + 4)
    else:
        tgt = ctx.static_cstr(target)
    pp = ctx.o.alloc(4)
    ctx.o.write_u32(pp, tgt)
    r = o.call(CONV, args=[cell, pp], convention="cdecl", fpu_cw=CW)
    cur = _read_cell(ctx.o, cell, st)[1]
    # result type: same-pointer/same-spelling keep src; successful
    # conversions take the target spelling.
    rspell = target if (target != "same" and r.eax == 1) else st
    if target == "same":
        rspell = st
    if rspell != st and r.eax == 1:
        got_type = bytes(ctx.o.read(ctx.o.read_u32(cell + 4),
                                    len(rspell.encode("latin1"))))
        got_type = got_type.decode("latin1")
    else:
        got_type = st if rspell == st else rspell
        if rspell == st:
            got_type = bytes(ctx.o.read(ctx.o.read_u32(cell + 4),
                                        len(st.encode("latin1")))).decode("latin1")
    return [(got_type, cur)], r.eax, [cell], [rspell]


_RUNNERS = {"vmstr_add": _run_add, "vmstr_strcheck": _run_check,
            "vmstr_concat": _run_concat, "vmstr_conv": _run_conv}


def _spec_to_args(spec, name: str):
    if name == "vmstr_conv":
        st, sw, target = spec
        return [{"vmval": {"type": st, "values": [sw & 0xFFFFFFFF]}},
                {"target": target}]
    if name == "vmstr_concat":
        lt, lw, rt, rw, kind = spec
        return [{"vmval": {"type": lt, "values": [lw & 0xFFFFFFFF]}},
                {"vmval": {"type": rt, "values": [rw & 0xFFFFFFFF]}},
                {"kind": kind}]
    lt, lw, rt, rw = spec
    return [{"vmval": {"type": lt, "values": [lw & 0xFFFFFFFF]}},
            {"vmval": {"type": rt, "values": [rw & 0xFFFFFFFF]}}]


def _args_to_spec(args, name: str):
    if name == "vmstr_conv":
        v = args[0]["vmval"]
        return (v["type"], v["values"][0], args[1]["target"])
    if name == "vmstr_concat":
        l, r = args[0]["vmval"], args[1]["vmval"]
        return (l["type"], l["values"][0], r["type"], r["values"][0],
                args[2]["kind"])
    l, r = args[0]["vmval"], args[1]["vmval"]
    return (l["type"], l["values"][0], r["type"], r["values"][0])


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for name, addr, _conv, specs in suite_rows():
        runner = _RUNNERS[name]
        rows = []
        for i, spec in enumerate(specs):
            o = _new_oracle(exe)
            ctx = Ctx(o)
            got, eax, _addrs, _sp = runner(o, ctx, spec)
            rows.append({
                "id": i, "exe_addr": f"0x{addr:08x}", "name": name,
                "convention": _conv, "this": "zero" if _conv == "thiscall" else "none",
                "args": _spec_to_args(spec, name),
                "dst": 0,
                "result": {
                    "cells": [{"type": t, "values": [w]} for (t, w) in got],
                    "eax": eax},
                "oracle": "unicorn", "unicorn": UC_VER,
                "cw": f"0x{CW:04x}", "date": _today()})
        path = os.path.join(OUT_DIR, f"{name}.jsonl")
        with open(path, "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        total += len(rows)
        print(f"{name}: {len(rows)} rows -> {path}")
    print(f"total {total} rows in {OUT_DIR}")


def _decode_and_run(o: Oracle, row: dict):
    ctx = Ctx(o)
    name = row["name"]
    addr = int(row["exe_addr"], 16)
    spec = _args_to_spec(row["args"], name)
    _ = addr  # runner uses the suite address; recorded addr is provenance
    got, eax, _addrs, _sp = _RUNNERS[name](o, ctx, spec)
    return got, eax


def verify(paths, exe=None):
    bad = 0
    n = 0
    for path in paths:
        o = _new_oracle(exe)
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        ok = 0
        for row in rows:
            n += 1
            got, eax = _decode_and_run(o, row)
            want_cells = [(c["type"], c["values"][0])
                          for c in row["result"]["cells"]]
            if got != want_cells or eax != row["result"]["eax"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['name']}: "
                      f"want {(want_cells, row['result']['eax'])} "
                      f"got {(got, eax)}")
            else:
                ok += 1
        print(f"{path}: {ok}/{len(rows)} ok")
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
