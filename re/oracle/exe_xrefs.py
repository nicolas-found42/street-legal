#!/usr/bin/env python3
"""Absolute-address cross references in the exe via its base-relocation table (M3-01).

Ghidra's all_pseudo.c shows direct CALLs but hides vtable slots and `push imm`/`mov reg,imm`
address uses.  The exe carries a .reloc table, so every absolute 32-bit address operand/data
word is a relocation site; this script indexes them (read-only on the exe) and answers:

    exe_xrefs.py to 0x4a5190            # sites whose dword == VA (+ vtable slot context)
    exe_xrefs.py vtable 0x5f0000        # list the code-pointer slots of the vtable at VA
    exe_xrefs.py str gravity            # strings containing TEXT (exact: str =gravity), with code xrefs

Output is evidence for docs/physics.md; nothing here is committed data.
Usage from repo root with the oracle venv: ~/slr-oracle-venv/bin/python re/oracle/exe_xrefs.py ...
"""
from __future__ import annotations

import argparse
import bisect
import os
import struct
import sys

import pefile

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAIN = "/Users/nicolas/Documents/github/games/street-legal"


def _default_exe() -> str:
    for base in (REPO, MAIN):
        p = os.path.join(base, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.isfile(p):
            return p
    raise SystemExit("exe not found; pass --exe")


class Image:
    def __init__(self, exe: str):
        self.pe = pefile.PE(exe, fast_load=False)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.data = self.pe.get_memory_mapped_image()
        self.sections = [
            (s.Name.rstrip(b"\0").decode(), self.base + s.VirtualAddress, s.Misc_VirtualSize)
            for s in self.pe.sections
        ]
        self.relocs: dict[int, list[int]] = {}
        for d in self.pe.DIRECTORY_ENTRY_BASERELOC:
            for r in d.entries:
                if r.type != 3 or r.rva is None:
                    continue
                va = self.base + r.rva
                tgt = struct.unpack_from("<I", self.data, r.rva)[0]
                self.relocs.setdefault(tgt, []).append(va)

    def section(self, va: int) -> str:
        for n, s, sz in self.sections:
            if s <= va < s + sz:
                return n
        return "?"

    def u32(self, va: int) -> int:
        return struct.unpack_from("<I", self.data, va - self.base)[0]

    def is_code(self, va: int) -> bool:
        return self.section(va) == ".text"


def load_index(path: str | None):
    if not path or not os.path.isfile(path):
        return [], []
    starts, names = [], []
    for line in open(path):
        p = line.split()
        if len(p) >= 2:
            starts.append(int(p[0], 16))
            names.append(p[1])
    order = sorted(range(len(starts)), key=lambda i: starts[i])
    return [starts[i] for i in order], [names[i] for i in order]


def func_of(va, starts, names):
    i = bisect.bisect_right(starts, va) - 1
    return f"{starts[i]:#010x}({names[i]})" if i >= 0 else "?"


def vtable_of(img: Image, slot: int):
    first = slot
    while img.is_code(img.u32(first - 4)) and img.section(first - 4) in (".rdata", ".data"):
        first -= 4
    return first, (slot - first) // 4


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["to", "vtable", "str"])
    ap.add_argument("va")
    ap.add_argument("--exe", default=None)
    ap.add_argument("--func-index", default=os.path.join(REPO, "re", "out", "index.txt"))
    ap.add_argument("-n", type=int, default=64)
    a = ap.parse_args()
    img = Image(a.exe or _default_exe())
    starts, names = load_index(a.func_index)
    if a.cmd == "str":
        exact = a.va.startswith("=")
        needle = a.va[1:].encode() if exact else a.va.encode()
        pos = 0
        while True:
            i = img.data.find(needle, pos)
            if i < 0:
                break
            pos = i + 1
            st = i
            while st > 0 and 32 <= img.data[st - 1] < 127:
                st -= 1
            en = img.data.find(b"\0", i)
            text = img.data[st:en]
            if exact and text != needle:
                continue
            va = img.base + st
            xs = sorted(img.relocs.get(va, []))
            where = ", ".join(func_of(x_, starts, names) if img.section(x_) == ".text" else f"{x_:#x}({img.section(x_)})" for x_ in xs)
            print(f"{va:#010x} {text.decode('latin1')!r} <- {where}")
        return
    va = int(a.va, 16)
    if a.cmd == "to":
        for site in sorted(img.relocs.get(va, [])):
            sec = img.section(site)
            if sec in (".rdata", ".data"):
                v, i = vtable_of(img, site)
                print(f"{site:#010x} {sec} vtable@{v:#010x} slot {i} (+{i*4:#x})")
            else:
                print(f"{site:#010x} {sec} in {func_of(site, starts, names)}")
    else:
        for i in range(a.n):
            t = img.u32(va + 4 * i)
            if not img.is_code(t):
                break
            print(f"slot {i:2d} (+{i*4:#04x}) -> {t:#010x} {func_of(t, starts, names)}")


if __name__ == "__main__":
    main()
