#!/usr/bin/env python3
"""Unicorn vectors for the TUFA class-loader primitives (M1-04, issue #31).

Runs the REAL exe functions in emulator memory (no JVM boot, no game data):

- string interner: `FUN_0041e4d0` (table ctor) + `FUN_0041e610` (intern) +
  `FUN_0041ef10` (lookup). Setup mirrors the exe's `FUN_0040fb50` string
  table: an 8-byte holder whose `+4` is the `0x7f40` table block. Rows
  record the interned bytes, length, refcount and dedup identity.
- native registrar: `FUN_00416b00` with a stub JVM ctx (`+0xc` = a
  `FUN_0041a370`-initialized registry, `+0x68` = the string-table holder).
  Rows record `eax` (1), the bucket index (`hash & 0x7f`) and the record
  words `[jvm][impl][name][sig]` resolved back to strings.
- boot descriptors: `FUN_0040fe70` needs the full JVM ctor; instead each
  `FUN_00423e10` bind is called exactly as the boot calls it
  (fresh cell + `ecx` = the string-table holder) and the bound type names
  are recorded. This pins the 5 primitive spellings `I/I/F/L;/V` and the
  `java` root package expectation without a booted JVM.

Conventions follow `re/oracle/vmval_vectors.py`: `hook_alloc` +
`realloc` hook, log sinks stubbed, `CreateSemaphoreA -> 0x1234`,
`FindFirstFileA -> -1` (absent), string-table cursor `FUN_0041f210`
stubbed to 0 (empty-table miss path). Hashes use signed bytes
(`movsx`), matching the exe.

Usage: vmclass_vectors.py gen | verify [files]   (output re/out/vmclass/)
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle  # noqa: E402
import unicorn  # noqa: F401

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "vmclass")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

E4D0 = 0x41E4D0
E610 = 0x41E610
EF10 = 0x41EF10
F210 = 0x41F210
REG = 0x416B00
REG_INIT = 0x41A370
BIND = 0x423E10
UC_VER = "2.1.4"


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        if not root:
            continue
        for rel in ("reference/raw/StreetLegal_Redline.bak.exe",
                    "reference/raw/StreetLegal_Redline.exe"):
            p = os.path.join(root, rel)
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
    o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
    o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
    o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
    o.hook(0x7F0001C0, lambda oc, a: 0x1234, "stdcall", 4)
    o.hook(0x7F000190, lambda oc, a: 0xFFFFFFFF, "stdcall", 2)
    o.hook(F210, lambda oc, a: 0, "fastcall", 0)
    return o


def _holder(o: Oracle):
    """Fresh string-table holder (8 bytes, +4 = table block)."""
    h = o.alloc(8)
    o.call(E4D0, args=[], convention="thiscall", this=h)
    return h


def _hash(s: bytes) -> int:
    if not s:
        return 0
    b = lambda i: struct.unpack("b", s[i:i + 1])[0]
    if len(s) < 4:
        return (b(0) + len(s)) & 0xFFFFFFFF
    return ((b(3) + b(len(s) - 4)) * 0x49 + b(len(s) - 2) + len(s)) & 0xFFFFFFFF


def _reg_hash(cls: bytes, name: bytes) -> int:
    return (sum(name) * 0x49 + sum(cls)) & 0xFFFFFFFF


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    # --- interner rows
    rows = []
    for i, s in enumerate([b"I", b"F", b"V", b"L;", b"java.lang",
                  b"java.lang.String", b"Ljava.lang.String;",
                  b"[I", b"()V", b"(F)V", b"getPos"]):
        o = _new_oracle(exe)
        h = _holder(o)
        inner = o.read_u32(h + 4)
        p = o.alloc(len(s) + 1)
        o.write(p, s + b"\0")
        o.call(E610, args=[p, len(s)], convention="thiscall", this=h)
        r = o.call(EF10, args=[_hash(s)], convention="thiscall", this=inner)
        rec = r.eax
        # entry record: [+0] vtable 0x5f0980, [+4] bytes ptr, [+8] len,
        # [+0xc] refcount (oracle-verified layout dump).
        bp = o.read_u32(rec + 4)
        got = bytes(o.read(bp, len(s)))
        assert got == s, f"entry bytes {got!r} != {s!r}: rec={rec:#x}" 
        length = o.read_u32(rec + 8)
        refs = o.read_u32(rec + 12)
        # dedup: intern again, same record, refcount 2
        o.call(E610, args=[p, len(s)], convention="thiscall", this=h)
        refs2 = o.read_u32(rec + 12)
        rows.append({"id": i, "exe_addr": hex(E610), "name": "intern",
                     "args": {"str": s.decode("latin1")}, "result": {
                         "bytes": got.decode("latin1"), "len": length,
                         "refs": refs, "refs_after_second": refs2,
                         "hash": _hash(s)},
                     "oracle": "unicorn", "unicorn": UC_VER, "date": _today()})
    with open(os.path.join(OUT_DIR, "vmclass_intern.jsonl"), "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    total += len(rows)
    # --- registrar rows
    rows = []
    cases = [("java.game.parts.WheelRef", "getPos", "()Ljava.lang.Vector3;", 0x41B860),
             ("java.lang.System", "currentTimeMillis", "()I", 0x41B860),
             ("java.game.GameType", "addCall", "(I)V", 0x41B860)]
    for i, (cls, name, sig, impl) in enumerate(cases):
        o = _new_oracle(exe)
        h = _holder(o)
        mreg = o.alloc(0x62C)
        o.call(REG_INIT, args=[mreg], convention="fastcall")
        jvm = o.alloc(0x80)
        o.write_u32(jvm + 0xC, mreg)
        o.write_u32(jvm + 0x68, h)
        pc = o.alloc(len(cls) + 1)
        o.write(pc, cls.encode() + b"\0")
        pn = o.alloc(len(name) + 1)
        o.write(pn, name.encode() + b"\0")
        ps = o.alloc(len(sig) + 1)
        o.write(ps, sig.encode() + b"\0")
        r = o.call(REG, args=[pc, pn, ps, impl], convention="thiscall", this=jvm)
        hh = _reg_hash(cls.encode(), name.encode())
        bkt = mreg + (hh & 0x7F) * 0xC
        got_hash = o.read_u32(bkt)
        cell = o.read_u32(bkt + 4)
        rec = o.read_u32(cell)
        got_impl = o.read_u32(rec + 8)
        ne = o.read_u32(rec + 12)
        se = o.read_u32(rec + 16)
        got_class = bytes(o.read(ne, len(cls)))
        got_name = bytes(o.read(se, len(name)))
        sigrec = o.read_u32(rec + 20)
        got_sig = o.read_cstr(o.read_u32(sigrec + 4)).decode("latin1")
        rows.append({"id": i, "exe_addr": hex(REG), "name": "register",
                     "args": {"class": cls, "name": name, "sig": sig,
                              "impl": hex(impl)},
                     "result": {"eax": r.eax, "hash": hh, "bucket": hh & 0x7F,
                                "bucket_hash": got_hash, "impl": hex(got_impl),
                                "rec_name": got_name.decode("latin1"),
                                "rec_sig": got_sig,
                                "rec_class": got_class.decode("latin1")},
                     "oracle": "unicorn", "unicorn": UC_VER, "date": _today()})
    with open(os.path.join(OUT_DIR, "vmclass_register.jsonl"), "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    total += len(rows)
    # --- boot bind rows (FUN_00423e10 per descriptor)
    rows = []
    for i, s in enumerate([b"I", b"I", b"F", b"L;", b"V"]):
        o = _new_oracle(exe)
        h = _holder(o)
        cell = o.alloc(4)
        cs = o.alloc(len(s) + 1)
        o.write(cs, s + b"\0")
        # boot shape: this=cell, args=[jvm-ish, cstr]; the helper reads
        # the string-table holder at [jvm+0x68].
        jvm = o.alloc(0x80)
        o.write_u32(jvm + 0x68, h)
        o.call(BIND, args=[jvm, cs], convention="thiscall", this=cell)
        entry = o.read_u32(cell)
        got = bytes(o.read(entry, len(s)))
        rows.append({"id": i, "exe_addr": hex(BIND), "name": "bind",
                     "args": {"str": s.decode("latin1")},
                     "result": {"bytes": got.decode("latin1")},
                     "oracle": "unicorn", "unicorn": UC_VER, "date": _today()})
    with open(os.path.join(OUT_DIR, "vmclass_bind.jsonl"), "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    total += len(rows)
    print(f"total {total} rows in {OUT_DIR}")


if __name__ == "__main__":
    gen()
