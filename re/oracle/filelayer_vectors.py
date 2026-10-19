#!/usr/bin/env python3
"""Unicorn vectors for the ported file-layer helpers (M1-09).

The `java.io.File` natives call a CRT-less file layer below them; `io_vectors.py`
hooks that layer to isolate the natives. This harness does the opposite: it
drives the layer itself, hooking only the Win32/CRT boundary beneath it, so the
real slot-table code runs.

  0x54c880 open_read(path)            0x54caf0 open_write(path)
  0x54c8d0 the slot-table open        0x54cca0 close(slot)
  0x54cdd0 size(slot)                 0x54ce40 seek(slot, off, whence)
  0x54cec0 read(slot, buf, n)         0x54cf00 write(slot, buf, n)
  0x54cf50 find-open(pattern, flags)  0x54d1c0 find-advance()
  0x54d240 find-free()                0x554a10 cmp_ci(a, b)

Hook boundaries:

- `0x554860` (the path resolver) is answered with the path the probe wrote, so
  `open_read`'s own `/` -> `\\` normalisation still runs on it.
- the Win32 open/read/write/seek/close/size calls the slot table makes are
  served from a probe-owned byte buffer, and every call is recorded.
- the find layer's three `FindFirstFileA`/`FindNextFileA`/`FindClose` IAT
  slots (`0x5e7098`/`0x5e709c`/`0x5e70a0`) are served from a synthetic
  directory, exactly as `io_vectors.py`'s I8 section does.

Row: `filelayer.jsonl` under `re/out/io/`, git-ignored.
"""

import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from oracle import CW_D3D9, Oracle  # noqa: E402

CW = CW_D3D9
UC_VER = "2.1.4"

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
OUT_DIR = os.path.join(REPO, "re", "out", "io")

OPEN_READ = 0x54C880
OPEN_WRITE = 0x54CAF0
SLOT_OPEN = 0x54C8D0
CLOSE = 0x54CCA0
SIZE = 0x54CDD0
SEEK = 0x54CE40
READ = 0x54CEC0
WRITE = 0x54CF00
FIND_OPEN = 0x54CF50
FIND_NEXT = 0x54D1C0
FIND_FREE = 0x54D240
CMP_CI = 0x554A10

ROWS = {
    OPEN_READ: ("fileio", "open_read", "(P)I"),
    OPEN_WRITE: ("fileio", "open_write", "(P)I"),
    SLOT_OPEN: ("fileio", "slot_open", "(PI)I"),
    # 0x54c8d0 takes (name, mode): the name is a 0x100-byte buffer the caller
    # fills, and mode 0 reads, 1 writes
    CLOSE: ("fileio", "close", "(I)I"),
    SIZE: ("fileio", "size", "(I)I"),
    SEEK: ("fileio", "seek", "(III)I"),
    READ: ("fileio", "read", "(IPI)I"),
    WRITE: ("fileio", "write", "(IPI)I"),
    FIND_OPEN: ("findfile", "open_search", "(PI)I"),
    FIND_NEXT: ("findfile", "advance", "()I"),
    FIND_FREE: ("findfile", "free", "()I"),
    CMP_CI: ("findfile", "cmp_ci", "(PPB)I"),
}

PATH_RESOLVER = 0x554860
# `0x54cf50` opens a search through the find slot table `0x559340`, which
# calls the Win32 IAT slot `0x5e7098` (FindFirstFileA) with a WIN32_FIND_DATAA
# buffer. Hooking the IAT entry runs the exe's real slot table and name copy.
FIND_FIRST = 0x5E7098
FIND_NEXT = 0x5E709C
# the find-record allocator `0x54f580`, hooked so open_search can build its
# own table (the real one goes through the CRT heap `0x559510`)
FIND_ALLOC = 0x54F580
# CreateFileA's IAT slot, used by the write-mode create helper `0x558890`
CREATE_FILE = 0x5E7194
# WriteFile's IAT slot and the unmapped address its stub points at
WRITE_FILE = 0x5E7060
WRITE_STUB = 0x76656C
# The Win32 imports the slot table calls through its own thunks. Hooking the
# thunks is what makes the exe's real slot-allocation loop run.
CRT_CALLS = {
    0x559090: "create_or_open",
    0x558E40: "close_handle",
    0x559150: "size",
    0x558D80: "read_write",
}


def _default_exe():
    for cand in (
        os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe"),
        os.path.join(MAIN_CHECKOUT, "reference", "raw", "StreetLegal_Redline.bak.exe"),
    ):
        if os.path.exists(cand):
            return cand
    raise SystemExit("stock exe not found; pass --exe")


def _alloc_cstr(o, text):
    buf = o.alloc(len(text) + 1)
    o.write(buf, text + b"\x00")
    return buf


class World:
    """One fresh Oracle with the CRT boundary hooked and a byte buffer behind
    it."""

    def __init__(self, exe=None, contents=b"", path="/stock/system/x.dat"):
        self.o = Oracle(exe or _default_exe())
        self.events = []
        self.buf = bytearray(contents)
        self.pos = 0
        self.open = False
        self.find_done = False
        self.path = _alloc_cstr(self.o, path.encode())
        o = self.o
        o.hook_alloc()
        # the resolver hands back the probe's own path text
        o.hook(PATH_RESOLVER, self._h_resolve, "cdecl", 2)
        for addr in CRT_CALLS:
            o.hook(addr, self._h_crt, "cdecl", 3)
        # the IAT slot holds a pointer to a stub (`31c0c3`), so hook the stub
        # itself; hooking the slot address would never run
        o.hook(o.read_u32(FIND_FIRST), self._h_find, "stdcall", 2)
        o.hook(o.read_u32(FIND_NEXT), self._h_find_next, "stdcall", 2)
        # `0x558890` (the write mode's create helper) calls CreateFileA through
        # IAT `0x5e7194`; its stub returns 0, which the helper reads as failure
        o.hook(o.read_u32(CREATE_FILE), self._h_create, "stdcall", 7)
        # the helper's WriteFile IAT slot holds an unmapped target (0x76656c)
        o.hook(WRITE_STUB, self._h_file_write, "stdcall", 5)
        o.hook(FIND_ALLOC, lambda oc, a: (oc.alloc(a[0]) if a[0] else 0), "cdecl", 2)
        # `[0x7686e0]` gates the live-data path cache `0x554630`; the probe has
        # no mounted data, so the search takes its direct arm.
        o.write_u32(0x7686E0, 0)

    def _h_resolve(self, oc, a):
        # (src, dst) -> copy the source text into the destination
        src, dst = a[0], a[1]
        text = oc.read_cstr(src) if src else b""
        oc.write(dst, text + b"\x00")
        self.events.append(["resolve", text.decode(errors="replace")])
        return dst

    def _h_file_write(self, oc, a):
        # (handle, buf, n, written_ptr, ovl) -> nonzero on success
        handle, buf, n, written = a[0], a[1], a[2], a[3]
        self.events.append(["write_file", handle & 0xFFFFFFFF, n & 0xFFFFFFFF])
        if written:
            oc.write_u32(written, n)
        return 1

    def _h_create(self, oc, a):
        # (name, access, share, sa, disposition, flags, template) -> handle
        name = oc.read_cstr(a[0]).decode(errors="replace") if a[0] else ""
        self.events.append(["create", name, a[4] & 0xFFFFFFFF])
        return 1

    def _h_find_next(self, oc, a):
        # (handle, WIN32_FIND_DATAA*) -> 0 ends the enumeration
        self.events.append(["find_next", a[0] & 0xFFFFFFFF])
        return 0

    def _h_find(self, oc, a):
        # (pattern, WIN32_FIND_DATAA*): one entry, then an invalid handle
        pat, data = a
        text = oc.read_cstr(pat).decode(errors="replace") if pat else ""
        self.events.append(["find_first", text])
        if self.find_done:
            return 0xFFFFFFFF
        self.find_done = True
        if data:
            # WIN32_FIND_DATAA: [+0] attrs, [+0x2c] name
            oc.write_u32(data, 0x20)
            name = b"gti.car" + b"\x00"
            oc.write(data + 0x2C, name)
        return 1

    def _h_crt(self, oc, a):
        name = CRT_CALLS.get(oc.reg("eip"))
        self.events.append(
            [
                "crt",
                name or "?",
                a[0] & 0xFFFFFFFF,
                a[1] & 0xFFFFFFFF,
                a[2] & 0xFFFFFFFF,
            ]
        )
        # a handle the table can store; every other call answers "fine"
        if name == "create_or_open":
            return 1
        if name == "size":
            return len(self.buf)
        return 0


def run_layer(case, exe=None):
    _id, spec = case
    world = {k: v for k, v in spec.items() if k not in ("addr", "params", "contents")}
    w = World(exe, contents=spec.get("contents", b""), **world)
    o = w.o
    addr = spec["addr"]
    params = []
    for ty, value in spec.get("params", []):
        if ty == "P":
            params.append((ty, _alloc_cstr(o, value.encode())))
        elif ty in ("B", "I"):
            params.append((ty, int(value) & 0xFFFFFFFF))
        else:
            params.append((ty, int(value) & 0xFFFFFFFF))
    err, eax = None, None
    try:
        words = [value for _ty, value in params]
        res = o.call(addr, words, "cdecl", limit=200_000)
        eax = res.eax & 0xFFFFFFFF
    except Exception as exc:  # noqa: BLE001 - the row records the failure
        err = type(exc).__name__
    result = {"eax": eax, "events": list(w.events), "error": err}
    return result


def layer_cases():
    return [
        ("open_read_file", {"addr": OPEN_READ, "params": [("P", "/stock/system/x.dat")]}),
        ("open_read_slashes", {"addr": OPEN_READ, "params": [("P", "stock/system/x.dat")]}),
        (
            "open_write_file",
            {"addr": OPEN_WRITE, "params": [("P", "/stock/system/y.dat")]},
        ),

        ("close_slot", {"addr": CLOSE, "params": [("I", 1)]}),
        ("size_slot", {"addr": SIZE, "params": [("I", 1)]}),
        ("seek_slot", {"addr": SEEK, "params": [("I", 1), ("I", 4), ("I", 0)]}),
        ("read_slot", {"addr": READ, "params": [("I", 1), ("P", "buf"), ("I", 4)]}),
        ("write_slot", {"addr": WRITE, "params": [("I", 1), ("P", "data"), ("I", 4)]}),
        (
            "cmp_ci_equal",
            {
                "addr": CMP_CI,
                "params": [("P", "Texture.dds"), ("P", "texture.DDS"), ("B", 1)],
            },
        ),
        (
            "cmp_ci_different",
            {"addr": CMP_CI, "params": [("P", "alpha"), ("P", "beta"), ("B", 1)]},
        ),
        (
            "cmp_ci_case_sensitive",
            {"addr": CMP_CI, "params": [("P", "alpha"), ("P", "Alpha"), ("B", 0)]},
        ),
    ]


def layer_rows_list(exe=None, only=None):
    rows = []
    for case_id, spec in layer_cases():
        if only and only not in case_id:
            continue
        result = run_layer((case_id, spec), exe)
        addr = spec["addr"]
        module, name, sig = ROWS[addr]
        rows.append(
            {
                "id": case_id,
                "exe_addr": addr,
                "class": module,
                "name": name,
                "sig": sig,
                "args": {"params": spec.get("params", [])},
                "result": result,
                "oracle": "unicorn",
                "unicorn": UC_VER,
                "cw": hex(CW),
                "date": _today(),
            }
        )
    return rows


def _today():
    import datetime

    return datetime.date.today().isoformat()


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = layer_rows_list(exe, only)
    if not rows:
        return
    path = os.path.join(OUT_DIR, "filelayer.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def probe(exe=None, only=None):
    for r in layer_rows_list(exe, only):
        print(f"== {r['id']} {r['name']} {r['sig']}")
        print("   ", r["result"])


def main(argv=None):
    import argparse

    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("cmd", choices=["gen", "probe"], nargs="?", default="probe")
    p.add_argument("--exe", default=None)
    p.add_argument("--only", default=None)
    a = p.parse_args(argv)
    exe = a.exe or _default_exe()
    if a.cmd == "gen":
        gen(exe, a.only)
    else:
        probe(exe, a.only)


if __name__ == "__main__":
    main()
