#!/usr/bin/env python3
"""Unicorn vectors for the file layer's `open_write` and the raw find layer.

Closes the four ledger rows the original `filelayer_vectors.py` harness left
open: `0x54caf0` (open_write — its create helper `0x558890` used to fault on
an unmodelled `CloseHandle` thunk cleanup), and the find layer `0x54cf50`
(open_search), `0x54d1c0` (advance), `0x54d240` (free), whose Win32 boundary
(`FindFirstFileA`/`FindNextFileA`/`FindClose` through IAT
`0x5e7098`/`0x5e709c`/`0x5e70a0`) is served from a probe-owned synthetic
directory.

Everything between the hooks is the exe's real code: the `'/'`->`'\\'`
normalisation, the same-name closed-slot release scan, the slot table
(`0x76656c`, 0x10c bytes per entry: +0 handle, +4 name[0x100], +0x104 id,
+0x108 in-use byte) and the find slot arrays `0x7663d8`/`0x768660`/`0x7685e0`
with `qsort` comparator `0x54d1a0` (= `0x554a10` case-folded strcmp).

The resolver `0x554860` is answered with the probe's own path text (its real
signature is `(dst, src)`). One row's create helper config makes
`CreateFileA` fail once so the helper's `GetLastError`/`CreateDirectoryA`
directory-creation loop runs (`GetLastError` must answer 3,
ERROR_PATH_NOT_FOUND, for the loop to start).

Row: `re/out/boot/filelayer2.jsonl` (git-ignored; the original
`re/out/io/filelayer.jsonl` capture is preserved untouched).
{"id","exe_addr","class","name","sig","args","result","oracle","unicorn","cw","date"}
`result` = {"eax", "events", "slots"/"find", "error"}: the returned word, the
recorded boundary events, and the observed table state read back afterwards.
"""
from __future__ import annotations

import datetime
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from filelayer_vectors import (  # noqa: E402
    CLOSE,
    CREATE_FILE,
    CW,
    FIND_ALLOC,
    FIND_FIRST,
    FIND_NEXT,
    FIND_OPEN,
    OPEN_READ,
    OPEN_WRITE,
    ROWS,
    UC_VER,
    World,
    _alloc_cstr,
    _default_exe,
)
from oracle import Oracle  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "boot")

FIND_CLOSE_IAT = 0x5E70A0
GET_LAST_ERROR_IAT = 0x5E7040
CREATE_DIR_IAT = 0x5E7080
SLOT_BASE = 0x76656C
SLOT_STRIDE = 0x10C
FIND_SLOTS = 0x20
INVALID_HANDLE = 0xFFFFFFFF


def _today() -> str:
    return datetime.date.today().isoformat()


class World2(World):
    """`filelayer_vectors.World` with a stateful synthetic find directory and
    an optional failing `CreateFileA` arm for the directory-creation loop."""

    def __init__(self, exe=None, contents=b"", path="/stock/system/x.dat",
                 entries=(), fail_first_create=False):
        super().__init__(exe, contents=contents, path=path)
        self.o = self._redecorate(self.o, entries, fail_first_create)

    def _redecorate(self, o, entries, fail_first_create):
        # The parent class hooked the find stubs with one-entry handlers;
        # replace them with the queue-backed versions below.
        self.find_entries = list(entries)
        self.search_seq = 0
        self.find_pos = {}
        self.finds = []          # recorded (pattern) starts
        self.dirs_created = []
        self.creates = []
        self.create_calls = 0

        def find_first(oc, a):
            # (pattern, WIN32_FIND_DATAA*) -> search handle or INVALID_HANDLE
            pat = oc.read_cstr(a[0]).decode(errors="replace") if a[0] else ""
            self.finds.append(pat)
            if not self.find_entries:
                return INVALID_HANDLE
            self.search_seq += 1
            self.find_pos[self.search_seq] = 1
            if a[1]:
                oc.write_u32(a[1], self.find_entries[0][1])
                oc.write(a[1] + 0x2C, self.find_entries[0][0].encode() + b"\0")
            return self.search_seq

        def find_next(oc, a):
            # (handle, WIN32_FIND_DATAA*) -> 1 with the next entry, else 0
            handle = a[0] & 0xFFFFFFFF
            pos = self.find_pos.get(handle)
            if pos is None or pos >= len(self.find_entries):
                return 0
            if a[1]:
                oc.write_u32(a[1], self.find_entries[pos][1])
                oc.write(a[1] + 0x2C, self.find_entries[pos][0].encode() + b"\0")
            self.find_pos[handle] = pos + 1
            return 1

        def find_close(oc, a):
            return 1

        def create_file(oc, a):
            # (name, access, share, sa, disposition, flags, template)
            name = oc.read_cstr(a[0]).decode(errors="replace") if a[0] else ""
            self.create_calls += 1
            self.creates.append(["create", name, a[4] & 0xFFFFFFFF])
            if fail_first_create and self.create_calls == 1:
                return INVALID_HANDLE
            return 1

        def get_last_error(oc, a):
            return 3  # ERROR_PATH_NOT_FOUND

        def create_dir(oc, a):
            # (name, security-attributes): record the prefix the helper
            # materialised (the helper NUL-terminates in place).
            name = oc.read_cstr(a[0]).decode(errors="replace") if a[0] else ""
            self.dirs_created.append(name)
            return 1

        o.hook(o.read_u32(FIND_FIRST), find_first, "stdcall", 2)
        o.hook(o.read_u32(FIND_NEXT), find_next, "stdcall", 2)
        o.hook(o.read_u32(FIND_CLOSE_IAT), find_close, "stdcall", 1)
        o.hook(o.read_u32(CREATE_FILE), create_file, "stdcall", 7)
        o.hook(o.read_u32(GET_LAST_ERROR_IAT), get_last_error, "stdcall", 0)
        o.hook(o.read_u32(CREATE_DIR_IAT), create_dir, "stdcall", 2)
        # The parent's `_h_create` answer is superseded by `create_file` above
        # (same stub address), and `hook_alloc`/CRT hooks carry over.
        return o


def _call(w, addr, params):
    """One cdecl call with ('P'|'I'|'B') params; returns eax or the error."""
    words = []
    for ty, value in params:
        if ty == "P":
            words.append(_alloc_cstr(w.o, value.encode()))
        else:
            words.append(int(value) & 0xFFFFFFFF)
    try:
        res = w.o.call(addr, words, "cdecl", limit=400_000)
        return res.eax & 0xFFFFFFFF, None
    except Exception as exc:  # noqa: BLE001 - the row records the failure
        return None, type(exc).__name__


def _slots_view(w, n=4):
    o = w.o
    out = []
    for i in range(1, n + 1):
        base = SLOT_BASE + (i - 1) * SLOT_STRIDE
        out.append({
            "slot": i,
            "handle": o.read_i32(base) & 0xFFFFFFFF,
            "name": o.read_cstr(base + 4, 0x100).decode("latin1"),
            "id": o.read_u32(base + 0x104),
            "in_use": o.read_u8(base + 0x108),
            "mode": o.read_u8(base + 0x109),
        })
    return out


def _find_view(w):
    o = w.o
    slots = []
    for i in range(FIND_SLOTS):
        block = o.read_u32(0x7663D8 + 4 * i)
        count = o.read_u32(0x768660 + 4 * i)
        cursor = o.read_u32(0x7685E0 + 4 * i)
        record = None
        if block and count:
            record = {
                "name": o.read_cstr(block, 0x100).decode("latin1"),
                "attr": o.read_u32(block + 0x100),
            }
        slots.append({"slot": i, "block": block, "count": count,
                      "cursor": cursor, "record0": record})
    return {"slots": slots, "finds": list(w.finds),
            "dirs_created": list(w.dirs_created), "creates": list(w.creates)}


def _run(name, scenario):
    w = World2(**scenario.get("world", {}))
    try:
        facts = scenario["run"](w)
        err = None
    except Exception as exc:  # noqa: BLE001 - the row records the failure
        facts, err = {}, type(exc).__name__
    return {"id": name, **facts, "events": list(w.events), "error": err}


# --------------------------------------------------------------- scenarios

def sc_open_write_zero(w):
    eax, err = _call(w, OPEN_WRITE, [("P", "/stock/system/y.dat")])
    return {"eax": eax, "slots": _slots_view(w)}


def sc_open_write_closed_same_name(w):
    first, _ = _call(w, OPEN_READ, [("P", "/stock/system/y.dat")])
    _call(w, CLOSE, [("I", first)])
    eax, err = _call(w, OPEN_WRITE, [("P", "/stock/system/y.dat")])
    return {"eax": eax, "first_open": first, "slots": _slots_view(w)}


def sc_open_write_in_use_same_name(w):
    first, _ = _call(w, OPEN_READ, [("P", "/stock/system/y.dat")])
    eax, err = _call(w, OPEN_WRITE, [("P", "/stock/system/y.dat")])
    return {"eax": eax, "first_open": first, "slots": _slots_view(w)}


def sc_open_write_full_table(w):
    opens = []
    for i in range(31):
        s, _ = _call(w, OPEN_READ, [("P", f"/stock/system/f{i:02d}.dat")])
        opens.append(s)
    eax, err = _call(w, OPEN_WRITE, [("P", "/stock/new/y.dat")])
    return {"eax": eax, "opens": opens, "creates": list(w.creates),
            "slots": _slots_view(w, 3)}


def sc_open_write_mkdir(w):
    eax, err = _call(w, OPEN_WRITE, [("P", "/stock/miss/y.dat")])
    return {"eax": eax, "creates": list(w.creates),
            "dirs_created": list(w.dirs_created), "slots": _slots_view(w, 3)}


def _find_call(w, pattern):
    # `0x54cf50(pattern, &name, &attr)`: the delivered record 0 lands in the
    # caller's buffers, so allocate real ones and hand their addresses.
    name = w.o.alloc(0x104)
    attr = w.o.alloc(4)
    w.o.write(name, b"\xAA" * 0x104)
    w.o.write_u32(attr, 0xDEADBEEF)
    return _find_open_raw(w, pattern, name, attr)


def _find_open_raw(w, pattern, name, attr):
    eax, err = _call(w, FIND_OPEN,
                     [("P", pattern), ("I", name), ("I", attr)])
    record = None
    if eax is not None and (eax & 0x80000000) == 0:
        record = {"name": w.o.read_cstr(name, 0x104).decode("latin1").rstrip("\xAA"),
                  "attr": w.o.read_u32(attr)}
    return eax, record, err


def _advance(w, handle):
    # `0x54d1c0(handle, &name, &attr)`: record 1.. lands in the caller's
    # buffers, exactly like open.
    name = w.o.alloc(0x104)
    attr = w.o.alloc(4)
    w.o.write(name, b"\xAA" * 0x104)
    w.o.write_u32(attr, 0xDEADBEEF)
    eax, err = _call(w, 0x54D1C0, [("I", handle), ("I", name), ("I", attr)])
    record = None
    if eax == 0:
        record = {"name": w.o.read_cstr(name, 0x104).decode("latin1").rstrip("\xAA"),
                  "attr": w.o.read_u32(attr)}
    return eax, record, err


def sc_find_open_one(w):
    eax, record, err = _find_call(w, "/stock/cars/*.car")
    return {"eax": eax, "record0": record, "find": _find_view(w)}


def sc_find_open_sorted(w):
    eax, record, err = _find_call(w, "/stock/cars/*")
    return {"eax": eax, "record0": record, "find": _find_view(w)}


def sc_find_open_nomatch(w):
    eax, record, err = _find_call(w, "/stock/none/*.car")
    return {"eax": eax, "record0": record, "find": _find_view(w)}


def sc_find_open_empty_pattern(w):
    eax, record, err = _find_call(w, "")
    return {"eax": eax, "record0": record, "find": _find_view(w)}


def sc_find_two_slots_and_reuse(w):
    a, ra, _ = _find_call(w, "/stock/cars/*.car")
    b, rb, _ = _find_call(w, "/stock/cars/*")
    freed, _ = _call(w, 0x54D240, [("I", a)])
    c, rc, _ = _find_call(w, "/stock/cars/*.car")
    return {"eax": [a, b, c], "record0": [ra, rb, rc], "freed": freed,
            "find": _find_view(w)}


def sc_find_open_full_table(w):
    handles = []
    for i in range(FIND_SLOTS + 1):
        h, _, _ = _find_call(w, f"/stock/cars{i}/*.car")
        handles.append(h)
    return {"eax": handles, "find": _find_view(w)}


def sc_find_advance(w):
    h, record, _ = _find_call(w, "/stock/cars/*")
    seq = [record]
    rets = []
    for _ in range(4):
        eax, rec, _ = _advance(w, h)
        rets.append(eax)
        seq.append(rec)
    return {"eax": h, "record0": seq, "advance_rets": rets,
            "find": _find_view(w)}


def sc_find_advance_bad(w):
    free_ret, _ = _call(w, 0x54D1C0, [("I", 3)])
    oob_ret, _ = _call(w, 0x54D1C0, [("I", FIND_SLOTS)])
    neg_ret, _ = _call(w, 0x54D1C0, [("I", -1)])
    return {"eax": [free_ret, oob_ret, neg_ret]}


def sc_find_free(w):
    h, _, _ = _find_call(w, "/stock/cars/*.car")
    ok, _ = _call(w, 0x54D240, [("I", h)])
    again, _ = _call(w, 0x54D240, [("I", h)])
    oob, _ = _call(w, 0x54D240, [("I", FIND_SLOTS)])
    neg, _ = _call(w, 0x54D240, [("I", -1)])
    return {"eax": h, "rets": [ok, again, oob, neg], "find": _find_view(w)}


def sc_find_advance_after_free(w):
    h, record, _ = _find_call(w, "/stock/cars/*.car")
    _call(w, 0x54D240, [("I", h)])
    eax, rec, _ = _advance(w, h)
    return {"eax": h, "record0": record, "advance": [eax, rec],
            "find": _find_view(w)}


GTI_DIR_ATTR = 0x10  # FILE_ATTRIBUTE_DIRECTORY -> attr bit 1
GTI_FILE_ATTR = 0x20  # archive -> attr bit 0

SCENARIOS = [
    ("open_write_zero", {"run": sc_open_write_zero}),
    ("open_write_closed_same_name",
     {"run": sc_open_write_closed_same_name}),
    ("open_write_in_use_same_name", {"run": sc_open_write_in_use_same_name}),
    ("open_write_full_table", {"run": sc_open_write_full_table}),
    ("open_write_mkdir", {"run": sc_open_write_mkdir,
                          "world": {"fail_first_create": True}}),
    ("find_open_one", {"run": sc_find_open_one,
                       "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
    ("find_open_sorted", {"run": sc_find_open_sorted,
                          "world": {"entries": [("z.car", GTI_FILE_ATTR),
                                                ("a.car", GTI_FILE_ATTR),
                                                ("m.dir", GTI_DIR_ATTR)]}}),
    ("find_open_nomatch", {"run": sc_find_open_nomatch}),
    ("find_open_empty_pattern", {"run": sc_find_open_empty_pattern}),
    ("find_two_slots_and_reuse",
     {"run": sc_find_two_slots_and_reuse,
      "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
    ("find_open_full_table", {"run": sc_find_open_full_table,
                              "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
    ("find_advance", {"run": sc_find_advance,
                      "world": {"entries": [("z.car", GTI_FILE_ATTR),
                                            ("a.car", GTI_FILE_ATTR),
                                            ("m.dir", GTI_DIR_ATTR)]}}),
    ("find_advance_bad", {"run": sc_find_advance_bad,
                          "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
    ("find_free", {"run": sc_find_free,
                   "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
    ("find_advance_after_free",
     {"run": sc_find_advance_after_free,
      "world": {"entries": [("gti.car", GTI_FILE_ATTR)]}}),
]

ADDR_OF = {
    "open_write": OPEN_WRITE,
    "find": FIND_OPEN,
}


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = []
    for name, spec in SCENARIOS:
        if only and only not in name:
            continue
        w_spec = dict(spec.get("world", {}))
        row = _run(name, spec)
        # The exe_addr/class/name/sig columns follow the first native the
        # scenario exercises; mixed scenarios keep the leading one.
        addr = OPEN_WRITE if name.startswith("open_write") else FIND_OPEN
        module, native, sig = ROWS[addr]
        rows.append({
            "id": name,
            "exe_addr": addr,
            "class": module,
            "name": native,
            "sig": sig,
            "args": {"world": w_spec},
            "result": row,
            "oracle": "unicorn",
            "unicorn": UC_VER,
            "cw": hex(CW),
            "date": _today(),
        })
    if not rows:
        return
    path = os.path.join(OUT_DIR, "filelayer2.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def probe(exe=None, only=None):
    for name, spec in SCENARIOS:
        if only and only not in name:
            continue
        row = _run(name, spec)
        print(f"== {name}\n   {row}")


def main(argv=None):
    import argparse

    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("cmd", choices=["gen", "probe"], nargs="?", default="probe")
    p.add_argument("--exe", default=None)
    p.add_argument("--only", default=None)
    a = p.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe, a.only)
    else:
        probe(a.exe, a.only)
    return 0


if __name__ == "__main__":
    sys.exit(main())
