#!/usr/bin/env python3
"""Unicorn vectors for the TUFA value helpers (M1-02, issue #29).

Calls each helper directly with hand-built cells in emulator memory (no JVM
boot needed). Layouts (all machine-code verified, see docs/tufa-executor.md):

- value cell: [vtable=0x5e734c][cstr I/F/L][payload] (12 bytes).
- varslot:    [vtable=0x5e7344][cstr][payload][name] (16 bytes).
- T_Container (operand stack): [+0] cap, [+4] data, [+8] count.
- value pool DAT_0062f130: [0] blockptr table, [1] nblocks, [2] handed,
  [3] capacity, [4] freelist head. Cells 12 bytes, 512 per 0x1800 block.
  Growth: capacity += 0x200 cells, table grows by one pointer (realloc
  0x54f580), fresh block malloced (0x54f560). The pool struct itself is
  heap-built here (the game builds it in FUN_004230d0 at boot); the helper
  only touches the struct words, so a scratch struct is faithful.
- live counter DAT_0062f138: i32, +1 on copy (00423e30), -1 on release.

Row (JSONL under re/out/vmval/, git-ignored):
  {"id","exe_addr","name","convention","args","result","writes","oracle",
   "unicorn","cw","date"}
`writes` is the mem-write capture restricted to the scenario arenas
(scratch/stack/pool/counter), sorted, as [addr,size,value] with addresses
rebased to arena labels (arena-dependent addresses are never recorded).
The Rust test replays the same scenario against the port and compares
(return + writes) bit-for-bit.

Like vm_vectors.py: `this` is a zeroed scratch whose [0] points at a fake
thread carrying +0x70/+0x74 cstrings; the log sinks
(0x551140/0x551220/0x5513b0) are stubbed; FUN_004190d0 fatal is NOT stubbed.
Type cstrings point directly at the exe's read-only chars ('I' @0x60ab50,
'F' @0x60ab58, 'L;' @0x60ab5c); DAT_0062e00c points at a heap cell holding
the 'I' cstring (production shape from vm_vectors.py).
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402
import unicorn  # noqa: F401

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "vmval")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
VAR_VT = 0x5E7344
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C, "[": 0x60AB5C,
      "V": 0x60AB60}
CW = CW_D3D9
UC_VER = "2.1.4"


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
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
    o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
    o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
    o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
    d = o.alloc(4)
    o.write_u32(d, TC["I"])
    o.write_u32(0x62E00C, d)
    return o


class Arena:
    """Scratch arenas with stable labels for write capture.

    Every `take` mints a unique region name (`cell0`, `poolblock1`, ...)
    so labels never collide: the oracle bump allocator aligns to 16 bytes
    while cells are 12, so same-kind regions are never contiguous."""

    def __init__(self, o: Oracle):
        self.o = o
        self.regions: list[tuple[int, int, str]] = []
        self.counts: dict[str, int] = {}

    def take(self, n: int, label: str) -> int:
        p = self.o.alloc(n)
        name = f"{label}{self.counts.get(label, 0)}"
        self.counts[label] = self.counts.get(label, 0) + 1
        self.regions.append((p, n, name))
        return p

    def label(self, addr: int, size: int):
        for base, n, name in self.regions:
            if base <= addr and addr + size <= base + n:
                return f"{name}+{addr - base:#x}"
        return None

    def norm_val(self, size: int, val: int):
        """Normalize a written value: 4-byte values pointing into a known
        arena become labels (arena-dependent addresses are never
        recorded); anything else stays a raw word."""
        val &= 0xFFFFFFFF
        if size == 4:
            for base, n, name in self.regions:
                if base <= val < base + n:
                    return f"{name}+{val - base:#x}"
        return val

    def writes(self, mem_writes):
        out = []
        for addr, size, val in mem_writes:
            lab = self.label(addr, size)
            if lab is not None:
                out.append([lab, size, self.norm_val(size, val)])
        return sorted(out, key=lambda r: (r[0], r[1], str(r[2])))


def _fake_this(o: Oracle) -> int:
    fake, thread = o.alloc(0x100), o.alloc(0x100)
    o.write_u32(fake, thread)
    o.write_u32(thread + 0x70, 0x60AB50)
    o.write_u32(thread + 0x74, 0x60AB58)
    return fake


def _cell(o: Oracle, arena: Arena, ty: str, w: int) -> int:
    p = arena.take(12, "cell")
    o.write_u32(p, VAL_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    return p


def _read_cell(o: Oracle, p: int):
    return chr(o.read_u8(o.read_u32(p + 4))), o.read_u32(p + 8)


def _varslot(o: Oracle, arena: Arena, ty: str, w: int, name: int) -> int:
    p = arena.take(16, "varslot")
    o.write_u32(p, VAR_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    o.write_u32(p + 12, name & 0xFFFFFFFF)
    return p


def _container(o: Oracle, arena: Arena, words: list[int],
               cap_extra: int = 0) -> int:
    c = arena.take(12, "container")
    cap = len(words) + cap_extra
    data = arena.take(max(4 * cap, 4), "stackdata")
    for i, w in enumerate(words):
        o.write_u32(data + 4 * i, w & 0xFFFFFFFF)
    o.write_u32(c, cap & 0xFFFFFFFF)
    o.write_u32(c + 4, data)
    o.write_u32(c + 8, len(words) & 0xFFFFFFFF)
    return c


def _pool(o: Oracle, arena: Arena, nblocks: int, handed: int,
          free_cells: list[int]) -> int:
    """Build a DAT_0062f130-shaped pool struct. Free cells are arena cells
    chained head-first (arena list order = chain order). Prebuilt blocks
    are registered as one arena region each so returned pointers label."""
    ps = arena.take(20, "pool")
    table = arena.take(max(4 * max(nblocks, 1), 4), "pooltable")
    for i in range(nblocks):
        blk = o.alloc(0x1800)
        arena.regions.append((blk, 0x1800, "poolblock"))
        o.write_u32(table + 4 * i, blk)
    head = 0
    for c in reversed(free_cells):
        o.write_u32(c, head)
        head = c
    o.write_u32(ps, table)
    o.write_u32(ps + 4, nblocks & 0xFFFFFFFF)
    o.write_u32(ps + 8, handed & 0xFFFFFFFF)
    o.write_u32(ps + 12, (512 * nblocks) & 0xFFFFFFFF)
    o.write_u32(ps + 16, head)
    return ps


def _fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def I(v):
    return ("I", v & 0xFFFFFFFF)


def F(v):
    return ("F", _fbits(v))


def L(v):
    return ("L", v & 0xFFFFFFFF)
def _sc_alloc(o, arena, case):
    nblocks, handed, nfree = case
    free = [_cell(o, arena, "I", 0xDEAD0000 + i) for i in range(nfree)]
    ps = _pool(o, arena, nblocks, handed, free)
    return {"ps": ps}


def _ck_alloc(o, arena, ctx, case, res):
    ps = ctx["ps"]
    got = res.eax
    # Growth reallocs the block table: register the LIVE table and its
    # blocks so pointers label (the arena snapshot predates realloc).
    table = o.read_u32(ps)
    nblocks = o.read_u32(ps + 4)
    if arena.label(table, 4) is None:
        arena.regions.append((table, 4 * max(nblocks, 1), "pooltable-live"))
    for i in range(nblocks):
        blk = o.read_u32(table + 4 * i)
        if blk and arena.label(blk, 12) is None:
            arena.regions.append((blk, 0x1800, "poolblock-live"))
    lab = arena.label(got, 12) if got else None
    assert lab is not None, f"alloc returned unmapped {got:#x}"
    result = {"ptr": lab, "nblocks": nblocks,
              "handed": o.read_u32(ps + 8),
              "capacity": o.read_u32(ps + 12),
              "freelist": "empty" if o.read_u32(ps + 16) == 0
              else arena.label(o.read_u32(ps + 16), 4)}
    return result, arena.writes(res.mem_writes)


def _sc_copy(o, arena, case):
    ty, w, live0 = case
    src = _cell(o, arena, ty, w)
    dst = arena.take(12, "cell")
    o.write_u32(0x62F138, live0 & 0xFFFFFFFF)
    return {"src": src, "dst": dst}


def _ck_copy(o, arena, ctx, case, res):
    ty, w, live0 = case
    assert _read_cell(o, ctx["dst"]) == (ty, w & 0xFFFFFFFF), \
        f"copy dst={_read_cell(o, ctx['dst'])} want {(ty, w)}"
    result = {"dst": list(_read_cell(o, ctx["dst"])),
              "live": o.read_u32(0x62F138)}
    return result, arena.writes(res.mem_writes)


def _thread_with_stack(o: Oracle, arena: Arena, words: list[int],
                       cap_extra: int = 0) -> tuple[int, int]:
    """Build thread -> frame -> T_Container chain (push/deref take the
    thread in ECX and walk `*(thread + 0x20) + 0x28`; pop takes the
    container itself)."""
    thread = arena.take(0x100, "thread")
    frame = arena.take(0x100, "frame")
    c = _container(o, arena, words, cap_extra)
    o.write_u32(thread + 0x20, frame)
    o.write_u32(frame + 0x28, c)
    return thread, c


def _sc_push(o, arena, case):
    words, extra = case
    thread, c = _thread_with_stack(o, arena, words, extra)
    val = arena.take(12, "cell")
    o.write_u32(val, VAL_VT)
    o.write_u32(val + 4, TC["I"])
    o.write_u32(val + 8, 0x12345678)
    return {"thread": thread, "c": c, "val": val, "n0": len(words)}


def _ck_push(o, arena, ctx, case, res):
    c = ctx["c"]
    cap, data, count = o.read_u32(c), o.read_u32(c + 4), o.read_u32(c + 8)
    expect_top = ctx["val"]
    top = o.read_u32(data + 4 * (count - 1)) if count else None
    assert top == expect_top, f"push top={top:#x} want {expect_top:#x}"
    # Growth reallocs the data array: register the LIVE array so the
    # container+0x4 write value labels.
    if arena.label(data, 4) is None:
        arena.regions.append((data, 4 * max(cap, 1), "stackdata-live"))
    result = {"cap": cap, "count": count, "top_is_val": top == expect_top}
    return result, arena.writes(res.mem_writes)


def _sc_pop(o, arena, case):
    words, extra = case
    c = _container(o, arena, words, extra)
    return {"c": c, "n0": len(words)}


def _ck_pop(o, arena, ctx, case, res):
    c = ctx["c"]
    count = o.read_u32(c + 8)
    result = {"eax": res.eax, "count": count}
    return result, arena.writes(res.mem_writes)


def _sc_deref(o, arena, case):
    kind, ty, w = case
    thread, c = _thread_with_stack(o, arena, [], 4)
    data = o.read_u32(c + 4)
    if kind == "val":
        cell = _cell(o, arena, ty, w)
    else:
        cell = _varslot(o, arena, ty, w, 0x60AB50)
        ps = _pool(o, arena, 1, 0, [])
        o.write_u32(0x62F130, ps)
    o.write_u32(data, cell)
    o.write_u32(c + 8, 1)
    return {"thread": thread, "c": c, "cell": cell}


def _ck_deref(o, arena, ctx, case, res):
    kind, ty, w = case
    got = res.eax
    assert got != 0, "deref returned null"
    assert _read_cell(o, got) == (ty, w & 0xFFFFFFFF), \
        f"deref={_read_cell(o, got)} want {(ty, w)}"
    result = {"cell": list(_read_cell(o, got)),
              "same_as_input": got == ctx["cell"]}
    return result, arena.writes(res.mem_writes)


def _sc_promote(o, arena, case):
    a, b = case
    pa = _cell(o, arena, a[0], a[1])
    pb = _cell(o, arena, b[0], b[1])
    return {"a": pa, "b": pb}


def _ck_promote(o, arena, ctx, case, res):
    result = {"a": list(_read_cell(o, ctx["a"])),
              "b": list(_read_cell(o, ctx["b"]))}
    return result, arena.writes(res.mem_writes)


def _sc_boolconv(o, arena, case):
    ty, w = case
    p = _cell(o, arena, ty, w)
    return {"p": p}


def _ck_boolconv(o, arena, ctx, case, res):
    result = {"cell": list(_read_cell(o, ctx["p"]))}
    return result, arena.writes(res.mem_writes)


def _sc_conv(o, arena, case):
    # case: (target_ty, value_ty, word). Call shape mirrors promote's
    # `FUN_00417da0(value_cell, &other_cell+4)`: arg1 = value cell,
    # arg2 = pointer at a word holding the target type cstring.
    target_ty, value_ty, w = case
    tcell = _cell(o, arena, target_ty, 0)
    v = _cell(o, arena, value_ty, w)
    return {"tcell": tcell, "v": v}


def _ck_conv(o, arena, ctx, case, res):
    target_ty, value_ty, w = case
    result = {"eax": res.eax, "cell": list(_read_cell(o, ctx["v"]))}
    return result, arena.writes(res.mem_writes)


def _sc_slotkind(o, arena, case):
    kind, ty, w = case
    if kind == "val":
        c = _cell(o, arena, ty, w)
    else:
        c = _varslot(o, arena, ty, w, 0x60AB50)
    return {"c": c}


def _ck_slotkind(o, arena, ctx, case, res):
    # Slot 0 is `mov eax, 1; ret` (value) or `mov eax, 2; ret` (varslot):
    # call the vtable slot directly with the cell as `this`.
    result = {"eax": res.eax}
    return result, arena.writes(res.mem_writes)


SCENARIOS = [
    # alloc: (nblocks, handed, nfree). Covers freelist hit, pooled next,
    # growth (handed == capacity), and multi-block pooled index.
    ("alloc", 0x423C00, "fastcall",
     _sc_alloc, _ck_alloc,
     [(0, 0, 2), (1, 0, 0), (1, 5, 0), (1, 512, 0), (2, 600, 0),
      (1, 511, 0), (2, 1024, 0), (1, 3, 1)]),
    ("copy", 0x423E30, "thiscall",
     _sc_copy, _ck_copy,
     [(I(0x12345678)[0], I(0x12345678)[1], 0),
      ("I", (-5) & 0xFFFFFFFF, 41),
      ("F", _fbits(1.5), 100),
      ("F", _fbits(-0.0), 7),
      ("L", 0x20001000, 3),
      ("L", 0, 9)]),
    # push: (initial words, capacity extra). Covers no-grow and growth
    # (incl. the 0/0 -> free+null path via cap=count=0 with extra=0... note
    # _container with [] and extra 0 gives cap 0; growth new = 0+0+1 = 1).
    ("push", 0x423BA0, "thiscall",
     _sc_push, _ck_push,
     [([], 0), ([1], 0), ([1, 2], 3), ([9], 0), ([1, 2, 3], 0),
      ([5, 6], 5)]),
    # pop: (initial words, capacity extra). Last row is the underflow log
    # path (eax 0, count stays 0).
    ("pop", 0x423D40, "thiscall",
     _sc_pop, _ck_pop,
     [([0xAAAAAAAA], 0), ([1, 2, 3], 2), ([0xDEADBEEF], 0), ([], 2)]),
    # deref: (kind, type, word). Var rows exercise alloc+copy of the slot.
    ("deref", 0x41F7D0, "fastcall",
     _sc_deref, _ck_deref,
     [("val", "I", 123), ("val", "F", _fbits(2.5)),
      ("val", "L", 0x20001000), ("var", "I", 77),
      ("var", "F", _fbits(-1.5)), ("var", "L", 0x20002000),
      ("var", "I", (-1) & 0xFFFFFFFF), ("var", "F", _fbits(0.0))]),
    # promote: pairs incl. no-op same types, I<->F conversions, L+L pass,
    # and the cannot-promote log path (I+L) which must leave words alone.
    ("promote", 0x417570, "thiscall",
     _sc_promote, _ck_promote,
     [(I(6), I(7)), (F(1.5), F(2.25)), (I(6), F(2.5)), (F(2.5), I(6)),
      (I(-3), F(0.5)), (L(0x20001000), L(0x20002000)),
      (I(6), L(0x20001000)), (F(1.5), L(0)), (L(0), I(4)),
      (I(0), I(0))]),
    # boolconv: F +/-0.0, nonzero, L null/non-null, I passthrough.
    ("boolconv", 0x417840, "stdcall",
     _sc_boolconv, _ck_boolconv,
     [("F", _fbits(1.5)), ("F", _fbits(0.0)), ("F", _fbits(-0.0)),
      ("F", _fbits(-2.0)), ("L", 0), ("L", 0x20001000),
      ("I", 0), ("I", 5), ("I", (-3) & 0xFFFFFFFF),
      ("F", _fbits(float("inf")))]),
    # conv: (target, value_ty, word). Boot-free paths only: I<->F, same
    # type, null L. String/ref paths need class loading (open, static).
    ("conv", 0x417DA0, "thiscall",
     _sc_conv, _ck_conv,
     [("F", "I", 6), ("F", "I", (-5) & 0xFFFFFFFF), ("F", "I", 0),
      ("I", "F", _fbits(2.75)), ("I", "F", _fbits(-2.75)),
      ("I", "F", _fbits(5e9)), ("I", "F", _fbits(float("nan"))),
      ("I", "F", _fbits(1e20)), ("I", "F", _fbits(-0.0)),
      ("I", "I", 9), ("F", "F", _fbits(1.5)), ("L", "L", 0)]),
    # slotkind: slot-0 probe on value cells (expect 1) and varslots (2).
    ("slotkind", 0x515610, "thiscall",
     _sc_slotkind, _ck_slotkind,
     [("val", "I", 1), ("val", "F", _fbits(1.5)), ("val", "L", 0),
      ("var", "I", 7), ("var", "F", _fbits(-1.0)),
      ("var", "L", 0x20001000)]),
]


def _call_for(name, o, ctx, case):
    # Conventions from machine code, not Ghidra headers: 23c00/23d40 take
    # their struct in ECX with `ret` (fastcall, one reg param); 1f7d0 takes
    # the thread in ECX the same way. Oracle `fastcall` puts the first int
    # args in ECX, EDX — so every ECX-pointer call passes args=[ptr].
    fake = _fake_this(o)
    if name == "alloc":
        return o.call(0x423C00, args=[ctx["ps"]], convention="fastcall",
                      fpu_cw=CW)
    if name == "copy":
        return o.call(0x423E30, args=[ctx["src"]], convention="thiscall",
                      this=ctx["dst"], fpu_cw=CW)
    if name == "push":
        return o.call(0x423BA0, args=[ctx["val"]], convention="thiscall",
                      this=ctx["thread"], fpu_cw=CW)
    if name == "pop":
        return o.call(0x423D40, args=[ctx["c"]], convention="fastcall",
                      fpu_cw=CW)
    if name == "deref":
        return o.call(0x41F7D0, args=[ctx["thread"]], convention="fastcall",
                      fpu_cw=CW)
    if name == "promote":
        return o.call(0x417570, args=[ctx["a"], ctx["b"]],
                      convention="thiscall", this=fake, fpu_cw=CW)
    if name == "boolconv":
        return o.call(0x417840, args=[ctx["p"]], convention="stdcall",
                      fpu_cw=CW)
    if name == "conv":
        return o.call(0x417DA0, args=[ctx["v"], ctx["tcell"] + 4],
                      convention="thiscall", this=fake, fpu_cw=CW)
    if name == "slotkind":
        # Slot 0 is `mov eax, imm; ret`: read the code pointer from the
        # cell's vtable and call it with the cell as `this`.
        vt = o.read_u32(ctx["c"])
        slot0 = o.read_u32(vt)
        return o.call(slot0, args=[], convention="thiscall",
                      this=ctx["c"], fpu_cw=CW)
    raise ValueError(name)


def _arg_json(name, ctx, case):
    if name == "alloc":
        nblocks, handed, nfree = case
        return {"pool": {"nblocks": nblocks, "handed": handed,
                         "nfree": nfree}}
    if name == "copy":
        ty, w, live0 = case
        return {"src": {"type": ty, "values": [w & 0xFFFFFFFF]},
                "live0": live0}
    if name == "push":
        words, extra = case
        return {"stack": [w & 0xFFFFFFFF for w in words], "extra": extra}
    if name == "pop":
        words, extra = case
        return {"stack": [w & 0xFFFFFFFF for w in words], "extra": extra}
    if name == "deref":
        kind, ty, w = case
        return {"kind": kind, "cell": {"type": ty,
                                       "values": [w & 0xFFFFFFFF]}}
    if name == "promote":
        a, b = case
        return {"a": {"type": a[0], "values": [a[1] & 0xFFFFFFFF]},
                "b": {"type": b[0], "values": [b[1] & 0xFFFFFFFF]}}
    if name == "boolconv":
        ty, w = case
        return {"cell": {"type": ty, "values": [w & 0xFFFFFFFF]}}
    if name == "conv":
        target_ty, value_ty, w = case
        return {"target": target_ty,
                "cell": {"type": value_ty, "values": [w & 0xFFFFFFFF]}}
    if name == "slotkind":
        kind, ty, w = case
        return {"kind": kind, "cell": {"type": ty,
                                       "values": [w & 0xFFFFFFFF]}}
    raise ValueError(name)


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for name, addr, conv, sc, ck, cases in SCENARIOS:
        rows = []
        for i, case in enumerate(cases):
            o = _new_oracle(exe)
            arena = Arena(o)
            ctx = sc(o, arena, case)
            res = _call_for(name, o, ctx, case)
            result, writes = ck(o, arena, ctx, case, res)
            rows.append({
                "id": i, "exe_addr": f"0x{addr:08x}", "name": name,
                "convention": conv, "this": "ctx",
                "args": _arg_json(name, ctx, case),
                "result": result, "writes": writes,
                "oracle": "unicorn", "unicorn": UC_VER,
                "cw": f"0x{CW:04x}", "date": _today()})
        path = os.path.join(OUT_DIR, f"vmval_{name}.jsonl")
        with open(path, "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        total += len(rows)
        print(f"{name}: {len(rows)} rows -> {path}")
    print(f"total {total} rows, {len(SCENARIOS)} helpers")


def verify(files, exe=None):
    bad = n = 0
    for path in files:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        scen = next(s for s in SCENARIOS
                    if os.path.basename(path) == f"vmval_{s[0]}.jsonl")
        _, addr, _, sc, ck, _ = scen
        name = scen[0]
        for row in rows:
            n += 1
            o = _new_oracle(exe)
            arena = Arena(o)
            case = _case_from_args(name, row["args"])
            ctx = sc(o, arena, case)
            res = _call_for(name, o, ctx, case)
            result, writes = ck(o, arena, ctx, case, res)
            if result != row["result"] or writes != row["writes"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {name}:\n"
                      f"  want {(row['result'], row['writes'])}\n"
                      f"  got  {(result, writes)}")
        print(f"{path}: ok")
    print(f"verify: {n - bad}/{n} byte-identical")
    return 1 if bad else 0


def _case_from_args(name, args):
    if name == "alloc":
        p = args["pool"]
        return (p["nblocks"], p["handed"], p["nfree"])
    if name == "copy":
        return (args["src"]["type"], args["src"]["values"][0], args["live0"])
    if name in ("push", "pop"):
        return (args["stack"], args["extra"])
    if name == "deref":
        return (args["kind"], args["cell"]["type"], args["cell"]["values"][0])
    if name == "promote":
        return ((args["a"]["type"], args["a"]["values"][0]),
                (args["b"]["type"], args["b"]["values"][0]))
    if name == "boolconv":
        return (args["cell"]["type"], args["cell"]["values"][0])
    if name == "conv":
        return (args["target"], args["cell"]["type"],
                args["cell"]["values"][0])
    if name == "slotkind":
        return (args["kind"], args["cell"]["type"],
                args["cell"]["values"][0])
    raise ValueError(name)


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
