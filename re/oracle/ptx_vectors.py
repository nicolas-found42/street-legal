#!/usr/bin/env python3
"""Unicorn vectors for the PTX texture-container loader (M2 follow-up, #105).

Target: 0x00507110  thiscall(ECX = res-obj) resource loader for the type-1
("ptx") texture path. What the disassembly shows:

- Init path (taken when `[res+0x68] == 0`): reads the file buffer at
  `[res+0x28]`: `ecx = buf[1]` (mip count), `edx = buf[2]` (width),
  `edi = buf[3]` (height); first entry pointer `buf+0x10` goes to
  `[res+0x70]`, count to `[res+0x68]`, index 0 to `[res+0x6c]`.
- Clamp: `drop = max(1, count - [DAT_006188b4])`; unless `drop >= count`
  (i.e. keep-all), width/height are `>>= [DAT_006188b4]`... more precisely
  `>>= (count - drop)`, and the kept count `drop` lands in `[res+0x68]`.
  Since entries are stored smallest-first, the clamp keeps the `drop`
  SMALLEST mips and drops the largest `[DAT_006188b4]` mips (the walk below
  finishes at `index == [res+0x68]` and never decodes above it).
- The init path then issues one device allocation through the vtable call
  `[ebx+0xec]` (`ebx = [[DAT_006187b0]+0x204]`), stack args
  `(width, height, kept, [res+0x54])`; a zero return aborts the load (zeroes
  `[res+0x68/0x6c/0x70/0x80]`, sets flag bits in `[res+0x20]`), a nonzero
  handle is kept in `[res+0x4c]` and checksummed twice via 0x004f6230 (sums
  land in `DAT_0065c234` / `[res+0x38]`).
- The same first call falls through to the walk: with `[res+0x80] == 0`
  (nothing decoded yet) 0x00506ad0 is skipped and 0x00506c70 returns 1
  trivially (`[res+0x74] == 0`), so index goes 0 -> 1 and `[res+0x80]`
  points at entry 0.
- Walk path (later calls): if `[res+0x80]` is set and no decode is in
  flight (`[res+0x74] == 0`), 0x00506ad0 starts the RGB JPEG decode;
  0x00506c70 pumps it and reports done (nonzero EAX). On done: if the
  completed index was 1, flag `0x10` is set in `[res+0x20]` and
  `DAT_006200a4` is snapshotted to `[res+0x34]`; if index == `[res+0x68]`
  the load is finished (`[res+0x70] = 0`); else `[res+0x80] = cur`,
  `next = cur + *cur + 0x14`, `[res+0x70] = next`, `[res+0x6c]++`.

Booted-state problem (honest blocker, cf. M1-13): in a fresh Unicorn
mapping the device chain behind `DAT_006187b0` is null
(`[[0x64a4d8]+0x204] == 0`), and the JPEG decoders below 0x00506ad0 /
0x00506c70 (0x00564b50 et al, the exe's static libjpeg) need a booted
heap/device and run tens of millions of instructions per mip. A fully
faithful end-to-end run is therefore impossible here; faking the whole
device would be testing our own stub. What this harness does instead:

- `init` rows run 100% real exe code (header parse + clamp + vtable slot
  call). Only the device itself is faked: a scratch vtable whose `+0xec`
  slot records `(width, height, kept, [res+0x54])` and returns a nonzero
  handle. The two 0x004f6230 checksum calls are hooked to deterministic
  stand-ins (return 0) and logged. Recorded: the surviving `[res+0x68]`
  word, index/pointer words as buffer offsets (position-independent),
  `[res+0x98/0x4c/0x38/0x34/0x20]` and the device/sum event log.
- `walk` rows drive the per-mip advance with 0x00506ad0/0x00506c70 hooked
  as event-log stubs (record entry offset + index, report done): every
  recorded word transition (`[res+0x80/0x70/0x6c]`, flag `0x10`, finish) is
  real exe code (`next = cur + *cur + 0x14`).

Row: {"id","exe_addr","name","source","clamp_b4","res54","input_hex",
      "result","oracle","unicorn","cw","date"}.
`result` holds res words as buffer offsets plus the event log. `gen`
writes JSONL under re/out/ptx/ (git-ignored); `verify` re-runs every row
byte-identically (fresh Oracle per row, no cross-row heap reuse).
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "ptx")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
UC_VER = "2.1.4"
CW = CW_D3D9

FUN = 0x507110
CTOR = 0x507530
SUMFN = 0x4F6230
DECODE0 = 0x506AD0
DECODE1 = 0x506C70
DAT_B4 = 0x6188B4
DAT_SNAP = 0x6200A4
DAT_CHAIN = 0x6187B0
CHAIN_OFF = 0x204
VTBL_SLOT = 0xEC
DEV_HANDLE = 0x0EC00001


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


class PtxWorld:
    """Fresh Oracle + fake device chain + event log for one scenario."""

    def __init__(self, exe=None):
        self.o = Oracle(exe or _default_exe())
        self.o.hook_alloc()
        self.events = []

        def _realloc(oc, args):
            old, size = args[0], args[1]
            new = oc.alloc(max(size, 1))
            if old:
                n = oc.heap_sizes.get(old, 0)
                if n:
                    oc.write(new, oc.read(old, min(n, size)))
            return new

        self.o.hook(0x54F580, _realloc, "cdecl", 2)

        # Fake device: [[DAT_CHAIN]+CHAIN_OFF] -> obj -> vtbl; the +0xec
        # slot is a scratch sentry recording (w, h, kept, [res+0x54]).
        chain = self.o.read_u32(DAT_CHAIN)
        self.obj = self.o.alloc(0x100)
        self.vtbl = self.o.alloc(0x200)
        self.sentry = self.o.scratch(16)
        self.o.write_u32(chain + CHAIN_OFF, self.obj)
        self.o.write_u32(self.obj, self.vtbl)
        self.o.write_u32(self.vtbl + VTBL_SLOT, self.sentry)

        def _device(oc, args):
            self.events.append(["device", args[0], args[1], args[2], args[3]])
            return DEV_HANDLE

        self.o.hook(self.sentry, _device, "stdcall", 4)

        def _sum(oc, args):
            self.events.append(["sum", oc.reg("ecx")])
            return 0

        self.o.hook(SUMFN, _sum, "thiscall", 0)


def _new_res(w: PtxWorld, data: bytes, res54=None) -> tuple[int, int]:
    """Ctor a res object, attach `data` as its file buffer. Returns (res, buf)."""
    o = w.o
    res = o.alloc(0xA0)
    o.call(CTOR, args=[], convention="thiscall", this=res)
    buf = o.alloc(len(data) + 16)
    o.write(buf, data)
    o.write_u32(res + 0x28, buf)
    if res54 is not None:
        o.write_u32(res + 0x54, res54)
    return res, buf


def _off(base: int, ptr: int) -> int:
    return (ptr - base) if ptr else 0


def _snapshot(w: PtxWorld, res: int, buf: int, totals: list) -> dict:
    o = w.o
    return {
        "r68_kept": o.read_u32(res + 0x68),
        "r6c_index": o.read_u32(res + 0x6C),
        "r70_cur_off": _off(buf, o.read_u32(res + 0x70)),
        "r80_entry_off": _off(buf, o.read_u32(res + 0x80)),
        "r98": o.read_u32(res + 0x98),
        "r4c_handle": o.read_u32(res + 0x4C),
        "r38_sum": o.read_u32(res + 0x38),
        "r34_snap": o.read_u32(res + 0x34),
        "r20_flags": o.read_u32(res + 0x20),
        "entry_totals": list(totals),
    }


def _entry_totals(data: bytes, n: int) -> list:
    out = []
    off = 16
    for _ in range(n):
        (total,) = struct.unpack_from("<I", data, off)
        out.append(total)
        off += total + 0x14
    return out


def run_case(data: bytes, clamp_b4=None, res54=None, exe=None,
             cw=CW, walk=True) -> dict:
    """One fresh world: ctor + init call, then walk steps to completion.

    Returns the complete row result: the init snapshot, the event log of the
    init call, and one snapshot per walk call. The last walk has
    `r70_cur_off == 0` (finish path) for well-formed inputs.
    """
    w = PtxWorld(exe)
    o = w.o
    if clamp_b4 is not None:
        o.write_u32(DAT_B4, clamp_b4)
    res, buf = _new_res(w, data, res54)
    n = struct.unpack("<I", bytes(data[4:8]))[0]
    totals = _entry_totals(bytes(data), n)
    r = o.call(FUN, args=[], convention="thiscall", this=res, fpu_cw=cw)
    init = {"eax": r.eax & 0xFFFFFFFF, **_snapshot(w, res, buf, totals)}
    init_events = list(w.events)

    walks = []
    if walk and n >= 1:
        def _d0(oc, args):
            oc_this = oc.reg("ecx")
            idx = oc.read_u32(oc_this + 0x6C)
            ent = oc.read_u32(oc_this + 0x80)
            w.events.append(["decode0", _off(buf, ent), idx])
            oc.write_u32(oc_this + 0x74, 0x0DEC0000 + idx)
            return None

        def _d1(oc, args):
            oc_this = oc.reg("ecx")
            idx = oc.read_u32(oc_this + 0x6C)
            ent = oc.read_u32(oc_this + 0x80)
            w.events.append(["decode1", _off(buf, ent), idx])
            oc.write_u32(oc_this + 0x74, 0)
            return 1

        o.hook(DECODE0, _d0, "thiscall", 0)
        o.hook(DECODE1, _d1, "thiscall", 0)
        for _ in range(n):
            r2 = o.call(FUN, args=[], convention="thiscall", this=res,
                        fpu_cw=cw)
            walks.append({"eax": r2.eax & 0xFFFFFFFF,
                          **_snapshot(w, res, buf, totals)})
            if walks[-1]["r70_cur_off"] == 0:
                break
    return {"init": init, "init_events": init_events, "walks": walks}


def run_row(row: dict, exe=None) -> dict:
    data = bytes.fromhex(row["input_hex"])
    return run_case(data, row.get("clamp_b4"), row.get("res54"), exe,
                    int(row["cw"], 16))


# ---------------------------------------------------------------- inputs

def corpus_files() -> list:
    roots = []
    for env, default in (("SLR_STOCK", "stock"),
                         ("SLR_REFERENCE", os.path.join("reference", "raw"))):
        cands = [os.environ.get(env), os.path.join(REPO, default),
                 os.path.join(MAIN_CHECKOUT, default)]
        for c in cands:
            if c and os.path.isdir(c):
                roots.append((default.split(os.sep)[0], c))
                break
    seen = {}
    for label, root in roots:
        for dp, _, fns in os.walk(root):
            for fn in sorted(fns):
                if fn.rsplit(".", 1)[-1].lower() != "ptx":
                    continue
                p = os.path.join(dp, fn)
                data = open(p, "rb").read()
                rel = f"{label}/{os.path.relpath(p, root)}"
                if data not in seen:
                    seen[data] = rel
    return [(rel, data) for data, rel in sorted(
        seen.items(), key=lambda kv: kv[1])]


def _synthetic(count: int, w: int = 64, h: int = 64) -> bytes:
    """Minimal well-formed container: total == rgb_size, alpha 0, SOI heads."""
    out = struct.pack("<IIII", 0, count, w, h)
    for i in range(count):
        rgb = bytes([0xFF, 0xD8, i, 0x11]) * 8
        total = len(rgb)
        out += struct.pack("<IIIIf", total, total, 0, 0,
                           (i + 1) / count) + rgb
    return out


def synthetic_inputs() -> list:
    out = []
    for n in range(1, 9):
        out.append((f"synthetic:mips{n}", _synthetic(n)))
    out.append(("synthetic:zero_count", struct.pack("<IIII", 0, 0, 8, 8)))
    return out


def build_cases() -> list:
    """(kind, source, data, clamp_b4, res54). clamp cases vary DAT_006188b8."""
    cases = []
    for rel, data in corpus_files():
        n = struct.unpack("<I", data[4:8])[0]
        cases.append(("init+walk", rel, data, None, None))
        if n >= 5:
            # clamp drops the top `clamp_b4` mips: kept = n - clamp_b4
            for b4 in (1, 3):
                if b4 < n:
                    cases.append(("init+walk", rel, data, b4, None))
    for label, data in synthetic_inputs():
        n = struct.unpack("<I", data[4:8])[0]
        cases.append(("init+walk", label, data, None, None))
        if 1 <= n:
            for b4 in (0, 1, 3, 7):
                if b4 < n or (b4 == 0 and n >= 1):
                    cases.append(("init+walk", label, data, b4, None))
    cases.append(("init+walk", "synthetic:mips3_p54zero",
                  _synthetic(3), None, 0))
    return cases


# ---------------------------------------------------------------- gen/verify

def gen(exe=None, cw=CW):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = []
    for kind, source, data, clamp_b4, res54 in build_cases():
        b4live = (Oracle(exe or _default_exe()).read_u32(DAT_B4)
                  if clamp_b4 is None else clamp_b4)
        result = run_case(data, clamp_b4, res54, exe, cw)
        rows.append({
            "id": len(rows), "exe_addr": f"0x{FUN:08x}", "name": "init",
            "source": source, "clamp_b4": b4live, "res54": res54,
            "input_hex": bytes(data).hex(), "result": result,
            "oracle": "unicorn", "unicorn": UC_VER,
            "cw": f"0x{cw:04x}", "date": _today()})
    path = os.path.join(OUT_DIR, "ptx_vectors.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(f"init+walk: {len(rows)} rows -> {path}")


def verify(files, exe=None):
    bad = n = 0
    for path in files:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        ok = 0
        for row in rows:
            n += 1
            got = run_row(row, exe)
            if got != row["result"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['source']} "
                      f"b4={row['clamp_b4']}")
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