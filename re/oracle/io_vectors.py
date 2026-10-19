#!/usr/bin/env python3
"""Unicorn vectors for the `File.open` / `File.close` (M1-09 I2) and
`File.write(ResourceRef)` / `File.readResID` (M1-09 I5) natives.

Calls the exe's natives directly with a hand-built `CallInfo` frame (no JVM
boot, no game data) and replaces the boundaries with Python hooks:

  0x485440 `java.io.File.open(I)I`          0x485870 `java.io.File.close()V`
  0x485c60 `write(ResourceRef)I`            0x485ea0 `readResID()I`

Hook boundaries (everything else is the real exe code, including `0x45d910`
argument marshalling, `0x54f560`/`0x54f5b0` allocation and the CRT-less file
layer call sequences):

- object field plumbing `0x42a690` (field by name) / `0x42ab50` (int field
  read) / `0x42a9e0` (int field write): canned values, logged. The probe feeds
  `0x42a690` a canned `java.lang.String` object and `0x42ab50` the name byte
  pointer for that object, the handle word for the receiver. The setter is the
  handle-field store the port must reproduce (logged with its arguments).
- the file layer `0x54c880` (open-read), `0x54caf0` (open-write),
  `0x54cec0` (read), `0x54cf00` (write), `0x54ce40` (seek), `0x54cca0`
  (close): logged as events and served from a Python-side byte buffer; the CRT
  table behind them is NOT emulated.
- `0x551120` (`strncpy` into `S+0x4c` and into a resource-table node) is
  emulated; `0x551330` / `0x551370` (the `printf`-style log calls) are recorded
  as their format + argument.
- the script-side SDAT slot-list builder `0x485190` and the list plumbing
  (`0x429350`, `0x429390`, `0x4293a0`, `0x45fa70`): constant / logged.
- the `java.util.resource` plumbing `0x5383f0` (the resource-manager kind
  descriptor lookup: `+8` = the kind, `+0xc` = the archive path; `[0x618d48]`
  is the manager), `0x42cec0` (a node's sub-object constructor) and `0x429060`
  (the node's `0xc`-byte assignment from the ref's type object): the descriptor
  is a canned Python object, the other two are emulated/no-ops.

Row (JSONL under `re/out/io/`, git-ignored): `open_close.jsonl` for the I2
natives, `resid.jsonl` for the I5 pair:
  {"id","exe_addr","name","convention","args","result","events","struct",
   "oracle","unicorn","cw","date"}
`events` is the ordered hook log; `struct` is the native file struct `S` as it
stands after the call (slot `+0`, open word `+4`, mode `+0x48`, name `+0x4c`)
and, for the I5 rows, the two resource tables (the counters `S+8` / `S+0x28`
and the rows: resource table `(id, kind, path)`, slot run `(id, kind, resID)`).

File.move (#107) adds `move.jsonl`, exercising native `0x486120`, layer
`0x54c650`, and helper `0x558ba0` against controlled Win32 boundaries. Its rows
retain the original missing-parent bug and compare the ADR 0007 correction
separately. Run `gen --only move_` and `verify-move` for this suite.

File.copy (#108) adds `copy.jsonl`, executing native `0x4860f0`, layer
`0x54c550`, and helper `0x558a50` with the same controlled platform boundary.
It checks overwrite, missing-parent retry, failure, and retained read/write
handles. Run `gen --only copy_` and `verify-copy`. Win32 contract:
https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfilea

I7 adds `input.jsonl` for the `Input` / `MouseCursor` / `Controller` logging
stubs (`input_rows_list` below): the exe's own natives called with the device
layer absent, i.e. the constant each stub must return. Its rows carry
`{"id","exe_addr","class","name","sig","args":{"params"},"result":{"word",
"eax","st0"},"events","logs",...}` (see the section comment).
"""
from __future__ import annotations
import datetime
import json
import os
import struct as _struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402
import unicorn  # noqa: F401

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "io")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}
# The build's SDAT header (`0x612c70`); the version byte is `0x612c76`.
SDAT_HEADER = bytes.fromhex("5344415400010200")
VERSION_BYTE = 0x612C76
HANDLE_FIELD_IDX = 0
NAME_CSTR = b"data/cars/gti.car"
# `java.util.resource.ResourceRef` natives (I5): the id word the ref object
# carries at `+8`, the type object at `+0xc` whose `+0x4c` is the kind, and the
# resource-manager descriptor lookup `0x5383f0` (`[0x618d48]` = the manager,
# descriptor `+8` = the kind, `+0xc` = the archive path).
RES_REF_OBJ_ID_OFF = 8
RES_REF_TYPE_OFF = 0xC
RES_TYPE_KIND_OFF = 0x4C
RES_DESC_KIND_OFF = 8
RES_DESC_PATH_OFF = 0xC
SCRIPT_KIND = 0xFFFF
WRITE_RES_REF = 0x485C60
READ_RES_ID = 0x485EA0
WRITE_RES_REF_SIG = b"(Ljava.util.resource.ResourceRef;)I"
READ_RES_ID_SIG = b"()I"
# The exe's two `write(ResourceRef)` / `readResID` diagnostics (`0x6138cc`,
# `0x6138ec`), appended after the `"!"` prefix (`0x6138c8` / `0x6138e8`).
UNSAVEABLE_CSTR = b"ERROR! Unsaveable resource!"
UNSAVED_CSTR = b"ERROR! Unsaved resource!"
# A path the port's root-confined resolver refuses (`..`): the file layer
# answers 0 for it in both implementations, which is the exe's "no slot" case.
REFUSED_CSTR = b"..\\escape.dat"
NAME_OFF = 0x4C
CW = CW_D3D9
UC_VER = "2.1.4"


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


class World:
    """One fresh Oracle plus the hook state for one scenario."""

    def __init__(self, exe=None, file: bytes = b"", handle: int = 0, open_slot: int = 1,
                 name: bytes = NAME_CSTR):
        self.o = Oracle(exe or _default_exe())
        self.events: list = []
        self.file = bytearray(file)
        self.pos = 0
        self.handle = handle
        self.open_slot = open_slot
        self.name_cstr = name
        self.base = 0  # the native struct, once allocated
        # the resource manager's kind descriptors (`0x5383f0`), built on demand
        self.res_descs: dict = {}   # kind -> descriptor address
        self.res_paths: dict = {}   # kind -> archive path bytes
        self.res_desc_max = 0x10000  # the manager's kind count (`[mgr+0xffe20]`)
        o = self.o
        o.hook_alloc()
        o.hook(0x54F560, self._h_alloc, "cdecl", 1)
        o.hook(0x54F5B0, self._h_free, "cdecl", 1)
        # field plumbing
        self.name_obj = _alloc_cstr(o, self.name_cstr)
        o.hook(0x42A690, self._h_getfield, "cdecl", 3)
        o.hook(0x42AB50, self._h_getint, "cdecl", 2)
        o.hook(0x42A9E0, self._h_setfield, "cdecl", 3)
        # the log sink (`0x62e018`): append + flush are no-ops, the
        # printf-style calls record the format and its argument
        o.hook(0x551140, self._h_append, "cdecl", 3)
        o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
        o.hook(0x551330, self._h_log2, "cdecl", 2)
        o.hook(0x551370, self._h_log2, "cdecl", 2)
        # `0x551120` strncpy(dst, src, n)
        o.hook(0x551120, self._h_strncpy, "cdecl", 3)
        # `java.util.resource` plumbing (I5): the manager's descriptor lookup
        # (`0x5383f0`, `0x618d48` = the manager), the list node's sub-object
        # constructor (`0x42cec0`) and its assignment from the ref's type
        # object (`0x429060`, `0xc` copied bytes).
        o.hook(0x5383F0, self._h_resdesc, "cdecl", 1)
        o.hook(0x42CEC0, self._h_subctor, "thiscall", 0)
        o.hook(0x429060, self._h_resassign, "thiscall", 1)
        # file layer
        o.hook(0x54C880, lambda oc, a: self._h_open("read", a), "cdecl", 1)
        o.hook(0x54CAF0, lambda oc, a: self._h_open("write", a), "cdecl", 1)
        o.hook(0x54CEC0, self._h_read, "cdecl", 3)
        o.hook(0x54CF00, self._h_write, "cdecl", 3)
        o.hook(0x54CE40, self._h_seek, "cdecl", 3)
        o.hook(0x54CCA0, self._h_closeslot, "cdecl", 1)
        # script-side SDAT model (slr-fmt-sdat's domain)
        o.hook(0x485190, lambda oc, a: 0, "cdecl", 2)
        o.hook(0x429350, lambda oc, a: 0, "thiscall", 0)
        o.hook(0x429390, lambda oc, a: 1, "thiscall", 0)
        o.hook(0x4293A0, lambda oc, a: 0, "thiscall", 0)
        o.hook(0x45FA70, lambda oc, a: self.events.append(["listfree", oc.reg("ecx") - self.base]), "thiscall", 0)
        # the field-id global the natives read (`DAT_0062e008`)
        d = o.alloc(4)
        o.write_u32(d, TC["I"])
        o.write_u32(0x62E00C, d)
        o.write_u32(0x62E008, HANDLE_FIELD_IDX)

    # -- hooks
    def _h_getfield(self, oc, a):
        obj, name, sig = a
        self.events.append(["getfield", self._label(obj), oc.read_cstr(name).decode(), oc.read_cstr(sig).decode()])
        return self.name_obj

    def _h_getint(self, oc, a):
        obj, idx = a
        self.events.append(["getint", self._label(obj), idx])
        return self.name_obj if obj == self.name_obj else self.handle

    def _h_setfield(self, oc, a):
        obj, idx, value = a
        self.events.append(["setfield", self._label(obj), idx, value])
        return None

    def _h_append(self, oc, a):
        _buf, text, _n = a
        self.events.append(["log_append", oc.read_cstr(text).decode()])
        return None

    def _h_log2(self, oc, a):
        fmt, arg = a
        self.events.append(["log", oc.read_cstr(fmt).decode(), self._cstr(arg)])
        return None

    def _h_strncpy(self, oc, a):
        dst, src, n = a
        data = oc.read_cstr(src)
        oc.write(dst, data + b"\0")
        off = dst - self.base if self.base and self.base <= dst < self.base + 0x200 else None
        self.events.append(["strncpy", off, data.decode(), n])
        return dst

    def _h_resdesc(self, oc, a):
        """`0x5383f0(manager, id)`: the kind descriptor for the id's high half."""
        (word,) = a
        kind = word >> 16
        if kind == SCRIPT_KIND or kind >= self.res_desc_max:
            self.events.append(["resdesc", word, kind, None])
            return 0
        desc = self.res_descs.get(kind)
        if desc is None:
            desc = oc.alloc(0x70)
            # the manager entry holds the archive path inline at `+0xc`
            # (`0x543eb0`'s `strncpy(entry+0xc, path, 0x40)`)
            path = self.res_paths.get(kind, b"resource.rpk")
            oc.write_u32(desc + RES_DESC_KIND_OFF, kind)
            oc.write(desc + RES_DESC_PATH_OFF, path + b"\0")
            self.res_descs[kind] = desc
        self.events.append(["resdesc", word, kind, oc.read_cstr(desc + RES_DESC_PATH_OFF).decode()])
        return desc

    def _h_subctor(self, oc, a):
        """`0x42cec0(node + 0x14)`: the node's resource sub-object."""
        self.events.append(["subctor", oc.reg("ecx") - self.base if self.base else None])
        return None

    def _h_resassign(self, oc, a):
        """`0x429060(node + 0x14, type)`: `0xc` bytes copied (the third word is
        the type's id, `[type+8]`, which is the node's `+0x1c`)."""
        (src,) = a
        dst = oc.reg("ecx")
        data = oc.read(src, 0xC)
        oc.write(dst, data)
        def rel(w):
            return w - self.base if self.base and self.base <= w < self.base + 0x200 else w
        self.events.append(["resassign", rel(dst), rel(src), oc.read_u32(src + 8)])
        return None

    def _h_open(self, kind, a):
        name = self.o.read_cstr(a[0]).decode()
        self.events.append([f"open_{kind}", name])
        self.pos = 0
        return self.open_slot

    def _h_read(self, oc, a):
        slot, buf, n = a
        got = bytes(self.file[self.pos : self.pos + n])
        oc.write(buf, got)
        self.pos += len(got)
        self.events.append(["read", slot, n, len(got)])
        return len(got)

    def _h_write(self, oc, a):
        slot, buf, n = a
        data = oc.read(buf, n)
        self.events.append(["write", slot, n, data.hex()])
        return n

    def _h_seek(self, oc, a):
        slot, whence, offset = a
        off = offset if offset < 0x8000_0000 else offset - 0x1_0000_0000
        base = {0: 0, 1: len(self.file), 2: self.pos}.get(whence)
        prev = self.pos
        self.events.append(["seek", slot, whence, off])
        if base is None:
            return 0xFFFFFFFF
        self.pos = max(0, base + off)
        return prev

    def _h_closeslot(self, oc, a):
        self.events.append(["close_slot", a[0]])
        return 0

    def _h_free(self, oc, a):
        self.events.append(["free", self._label(a[0])])
        if a[0] == self.base:
            self.base = 0
        return None

    def _h_alloc(self, oc, a):
        (size,) = a
        p = oc.alloc(size)
        self.events.append(["alloc", size, self._label(p)])
        if size == 0x14C:
            self.base = p
        return p

    # -- helpers
    def _label(self, word: int) -> str:
        if word == self.name_obj:
            return "name"
        if self.base and word == self.base:
            return "S"
        if word == 0:
            return "null"
        return f"obj@{word:#x}"

    def _cstr(self, addr: int) -> str:
        try:
            return self.o.read_cstr(addr).decode()
        except Exception:
            return f"!{addr:#x}"

    def struct_state(self, addr: int) -> dict:
        o = self.o
        return {
            "slot": o.read_u32(addr),
            "open_word": o.read_u32(addr + 4),
            "resource_count": o.read_u32(addr + 8),
            "slot_count": o.read_u32(addr + 0x28),
            "mode": o.read_u32(addr + 0x48),
            "name": o.read_cstr(addr + NAME_OFF).decode(),
        }

    # -- resource tables (`S+0x14` the resource table, `S+0x34` the slot run)

    def _chain(self, head: int, end: int) -> list:
        """The real nodes of one list. The head field holds the first node, or
        the list object's end marker when it is empty; every node's `+4` points
        at the next node and the last one at the end marker, whose own `+4` is
        the walk's terminator (`0x485e0b` / `0x485d53`)."""
        out, seen, node = [], set(), head
        while node and node not in seen and len(out) < 0x40:
            seen.add(node)
            if node == end:
                break
            out.append(node)
            node = self.o.read_u32(node + 4)
        return out

    def _append_node(self, s: int, node: int, tail_off: int, end: int):
        """The exe's list insert (`0x485e37` for the resource table, `0x485d8d`
        for the slot run): link the node after the tail field's node, whose `+4`
        is the head field itself while the list is empty (`A+4` = `S+0x14`)."""
        o = self.o
        tail = o.read_u32(s + tail_off)
        o.write_u32(tail + 4, node)
        o.write_u32(node + 4, end)
        o.write_u32(node + 8, tail)
        o.write_u32(s + tail_off, node)

    def tables_state(self) -> dict:
        """`S`'s two resource tables after the call: the counters and, per row,
        the node's `+0xc` / `+0x10` / path (`+0x14`) / `+0x1c`."""
        o = self.o
        s = self.base
        resources = []
        for node in self._chain(o.read_u32(s + 0x14), s + 0x1C):
            path = o.read_cstr(node + 0x14).decode()
            resources.append([o.read_u32(node + 0xC), o.read_u32(node + 0x10), path])
        slots = []
        for node in self._chain(o.read_u32(s + 0x34), s + 0x3C):
            slots.append([o.read_u32(node + 0xC), o.read_u32(node + 0x10), o.read_u32(node + 0x1C)])
        return {
            "slot": o.read_u32(s),
            "resource_count": o.read_u32(s + 8),
            "slot_count": o.read_u32(s + 0x28),
            "resources": resources,
            "slots": slots,
        }


def _alloc_cstr(o: Oracle, s: bytes) -> int:
    p = o.alloc(len(s) + 16)
    o.write(p, s + b"\0")
    return p


def _cell(o: Oracle, ty: str, w: int) -> int:
    p = o.alloc(12)
    o.write_u32(p, VAL_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    return p


def _container(o: Oracle, words: list) -> int:
    c = o.alloc(12)
    data = o.alloc(max(4 * len(words), 4))
    for i, w in enumerate(words):
        o.write_u32(data + 4 * i, w)
    o.write_u32(c, len(words))
    o.write_u32(c + 4, data)
    o.write_u32(c + 8, len(words))
    return c


def _frame(w: World, sig: bytes, stack_cells: list) -> int:
    o = w.o
    stack = _container(o, stack_cells)
    sigp = _alloc_cstr(o, sig)
    callinfo = o.alloc(0x20)
    o.write_u32(callinfo + 4, sigp)
    method = o.alloc(0x40)
    o.write_u32(method + 0xC, _alloc_cstr(o, b"java.io.File"))
    o.write_u32(method + 0x10, _alloc_cstr(o, b"open"))
    o.write_u32(method + 0x14, callinfo)
    frame = o.alloc(0x40)
    o.write_u32(frame + 8, stack)
    o.write_u32(frame + 0xC, method)
    return frame


def run_open(case, exe=None):
    """0x485440 File.open(I)I: returns (result, events, struct)."""
    w = World(exe, file=bytes.fromhex(case.get("file", "")), handle=case.get("handle", 0),
              open_slot=case.get("open_slot", 1),
              name=(case["name"].encode() if "name" in case else NAME_CSTR))
    o = w.o
    if "reuse_slot" in case:
        # a live struct the handle field already points at (`0x485496`)
        s = o.alloc(0x200)
        w.base = s
        o.write_u32(s, case["reuse_slot"])
        o.write_u32(s + 0x48, 0)
        o.write(s + NAME_OFF, w.name_cstr + b"\0")
        w.handle = s
    obj = o.alloc(0x30)
    cells = [_cell(o, "I", case.get("mode", 0)), _cell(o, "L", obj)]
    frame = _frame(w, b"(I)I", cells)
    truncated = None
    try:
        res = o.call(0x485440, [frame], "cdecl", limit=2_000_000)
        eax = res.eax
    except (Exception,) as e:  # the exe diverges into the script-side table loop
        truncated = f"{type(e).__name__}: {e}"
        eax = None
    struct = None
    if w.base:
        try:
            struct = w.struct_state(w.base)
        except Exception:
            struct = None
    result = {"eax": eax}
    if truncated:
        result["truncated"] = truncated
    return result, w.events, struct


def run_close(case, exe=None):
    """0x485870 File.close()V: returns (result, events)."""
    w = World(exe, file=bytes.fromhex(case.get("file", "")))
    o = w.o
    # the native file struct the handle field points at
    s = o.alloc(0x200)
    w.base = s
    o.write_u32(s, case.get("slot", 0))
    o.write_u32(s + 0x28, case.get("slot_count", 0))
    o.write_u32(s + 8, case.get("resource_count", 0))
    o.write_u32(s + 0x48, case.get("mode", 0))
    # the two script-side lists end at their sentinel (`[head+4] == 0`): the
    # JPEG-slot list (`S+0x34`) and the resource list (`S+0x14`) are empty.
    sent_slots = o.alloc(0x40)
    o.write_u32(sent_slots + 4, 0)
    sent_res = o.alloc(0x40)
    o.write_u32(sent_res + 4, 0)
    o.write_u32(s + 0x34, sent_slots)
    o.write_u32(s + 0x14, sent_res)
    o.write(s + NAME_OFF, NAME_CSTR + b"\0")
    w.handle = 0 if case.get("missing") else s
    obj = o.alloc(0x30)
    frame = _frame(w, b"()V", [_cell(o, "L", obj)])
    truncated = None
    try:
        res = o.call(0x485870, [frame], "cdecl", limit=2_000_000)
        eax = res.eax
    except (Exception,) as e:
        truncated = f"{type(e).__name__}: {e}"
        eax = None
    result = {"eax": eax}
    if truncated:
        result["truncated"] = truncated
    return result, w.events


def _res_ref(o: Oracle, word, kind: int) -> int:
    """A `java.util.resource.ResourceRef` object: `+8` = its packed id,
    `+0xc` = its type object whose `+0x4c` is the resource kind."""
    ref = o.alloc(0x20)
    type_ = o.alloc(0x60)
    o.write_u32(ref + RES_REF_OBJ_ID_OFF, word & 0xFFFFFFFF)
    o.write_u32(ref + RES_REF_TYPE_OFF, type_)
    o.write_u32(type_ + RES_TYPE_KIND_OFF, kind)
    o.write_u32(type_ + 8, word & 0xFFFFFFFF)
    return ref


def _resid_world(case, exe=None):
    """A world with a hand-built `S` and the two SDAT tables `case` describes."""
    w = World(exe, file=bytes.fromhex(case.get("file", "")), handle=0)
    o = w.o
    if "desc_max" in case:
        w.res_desc_max = case["desc_max"]
    for kind, path in case.get("paths", {}).items():
        w.res_paths[int(kind)] = path.encode()
    s = o.alloc(0x200)
    w.base = s
    o.write_u32(s, case.get("slot", 1))
    o.write_u32(s + 8, case.get("resource_count", 0))
    o.write_u32(s + 0x28, case.get("slot_count", 0))
    o.write_u32(s + 0x48, case.get("mode", 1))
    o.write(s + NAME_OFF, NAME_CSTR + b"\0")
    # The two list objects as the real `File.open` leaves them (`0x4854b0`:
    # `0x429350` then the `0x5f09a8` vtable): per list an `A` sub-object whose
    # `+4` IS the head field and whose address the tail field holds, and an
    # end marker `E` whose `+4` is the walk's terminator (0) and whose `+8` is
    # the tail field. Empty: head = &E, tail = &A.
    o.write_u32(s + 0x0C, 0x5F09A8)
    o.write_u32(s + 0x10, 0x5F09AC)
    o.write_u32(s + 0x14, s + 0x1C)
    o.write_u32(s + 0x24, s + 0x10)
    o.write_u32(s + 0x1C, 0x5F09AC)
    o.write_u32(s + 0x2C, 0x5F09A8)
    o.write_u32(s + 0x30, 0x5F09AC)
    o.write_u32(s + 0x34, s + 0x3C)
    o.write_u32(s + 0x44, s + 0x30)
    o.write_u32(s + 0x3C, 0x5F09AC)
    for row in case.get("resources", []):
        node = o.alloc(0x114)
        o.write_u32(node + 0xC, row[0])
        o.write_u32(node + 0x10, row[1])
        o.write(node + 0x14, row[2].encode() + b"\0")
        w._append_node(s, node, 0x24, s + 0x1C)
    for row in case.get("slots", []):
        node = o.alloc(0x24)
        o.write_u32(node + 0xC, row[0])
        o.write_u32(node + 0x10, row[1])
        o.write_u32(node + 0x1C, row[2])
        w._append_node(s, node, 0x44, s + 0x3C)
    w.handle = 0 if case.get("missing") else s
    return w


def _resid_result(w):
    tables = None
    if w.base:
        try:
            tables = w.tables_state()
        except Exception:
            tables = None
    return tables


def run_write_res_ref(case, exe=None):
    """0x485c60 `File.write(ResourceRef)I`: (result, events, tables)."""
    w = _resid_world(case, exe)
    o = w.o
    ref = 0 if case.get("null_ref") else _res_ref(o, case.get("id", 0), case.get("kind", 5))
    obj = o.alloc(0x30)
    cells = [_cell(o, "L", ref), _cell(o, "L", obj)]
    frame = _frame(w, WRITE_RES_REF_SIG, cells)
    truncated = None
    try:
        res = o.call(WRITE_RES_REF, [frame], "cdecl", limit=200_000)
        eax = res.eax
    except Exception as e:
        truncated = f"{type(e).__name__}: {e}"
        eax = None
    result = {"eax": eax}
    if truncated:
        result["truncated"] = truncated
    return result, w.events, _resid_result(w)


def run_read_res_id(case, exe=None):
    """0x485ea0 `File.readResID()I`: (result, events, tables)."""
    w = _resid_world(case, exe)
    o = w.o
    obj = o.alloc(0x30)
    frame = _frame(w, READ_RES_ID_SIG, [_cell(o, "L", obj)])
    truncated = None
    try:
        res = o.call(READ_RES_ID, [frame], "cdecl", limit=200_000)
        eax = res.eax
    except Exception as e:
        truncated = f"{type(e).__name__}: {e}"
        eax = None
    result = {"eax": eax}
    if truncated:
        result["truncated"] = truncated
    return result, w.events, _resid_result(w)


def open_cases():
    """The files the exe's `File.open` footer scan reads."""
    hdr = SDAT_HEADER
    # version 2, empty body: header + [slot_count 0][resource_count 0][total 8]
    v2_empty = hdr + bytes(4) + bytes(4) + _struct.pack("<I", 8)
    # version 1, empty body: header + 4 body bytes + [table size 4][count 0]
    v1_empty = hdr[:6] + bytes([1, 0]) + bytes(4) + _struct.pack("<I", 4) + bytes(4)
    return [
        ("open_read_v2_empty", {"mode": 0, "file": v2_empty.hex()}),
        ("open_read_v2_short_total", {"mode": 0, "file": (hdr + bytes(4) + bytes(4) + _struct.pack("<I", 4)).hex()}),
        ("open_read_v1_empty", {"mode": 0, "file": v1_empty.hex()}),
        ("open_read_short_header", {"mode": 0, "file": bytes(4).hex()}),
        ("open_read_empty", {"mode": 0, "file": ""}),
        ("open_write", {"mode": 1, "file": ""}),
        ("open_write_no_slot", {"mode": 1, "file": "", "open_slot": 0, "name": REFUSED_CSTR.decode()}),
        ("open_invalid_mode", {"mode": 2, "file": ""}),
        ("open_read_no_slot", {"mode": 0, "file": "", "open_slot": 0, "name": REFUSED_CSTR.decode()}),
        ("open_read_reuse", {"mode": 0, "file": v2_empty.hex(), "reuse_slot": 1}),
    ]


def close_cases():
    return [
        ("close_read_slot", {"slot": 1, "mode": 0}),
        ("close_write_slot", {"slot": 1, "mode": 1}),
        ("close_write_slot_counts", {"slot": 3, "mode": 1, "slot_count": 0, "resource_count": 0}),
        ("close_other_mode", {"slot": 1, "mode": 5}),
        ("close_slot_zero", {"slot": 0, "mode": 1}),
        ("close_missing_handle", {"slot": 1, "mode": 1, "missing": True}),
    ]


def write_res_ref_cases():
    """`write(ResourceRef)`: saved ids (packed through the resource table),
    script-side `0xffff`-high ids (packed through the slot run), the
    unsaveable-kind and zero/null arms."""
    return [
        ("write_res_saved_new", {"id": 3 << 16 | 7, "kind": 5}),
        ("write_res_saved_reuse", {"id": 3 << 16 | 9, "kind": 5, "resources": [[1, 3, "system.rpk"]]}),
        ("write_res_saved_other_kind", {"id": 4 << 16 | 1, "kind": 5, "resources": [[2, 3, "system.rpk"]]}),
        ("write_res_saved_path", {"id": 3 << 16 | 7, "kind": 5, "paths": {"3": "data/cars.rpk"}}),
        ("write_res_script_new", {"id": 0xFFFF << 16 | 7, "kind": 5}),
        ("write_res_script_kind7", {"id": 0xFFFF << 16 | 3, "kind": 7}),
        ("write_res_script_reuse", {"id": 0xFFFF << 16 | 7, "kind": 5, "slots": [[1, 5, 0xFFFF0007]]}),
        ("write_res_unsaveable_kind3", {"id": 0xFFFF << 16 | 4, "kind": 3}),
        ("write_res_unsaveable_kind0", {"id": 0xFFFF << 16 | 4, "kind": 0}),
        ("write_res_saveable_kind5", {"id": 0xFFFF << 16 | 4, "kind": 5}),
        # a small id: high half 0 is a *manager* kind on the write side
        ("write_res_zero_high_id", {"id": 5, "kind": 5}),
        ("write_res_zero_id", {"id": 0, "kind": 5}),
        ("write_res_null_ref", {"null_ref": True}),
        ("write_res_no_handle", {"id": 3 << 16 | 7, "kind": 5, "missing": True}),
        ("write_res_closed_slot", {"id": 3 << 16 | 7, "kind": 5, "slot": 0}),
        # `0x5383f0` has no descriptor for this kind (`[mgr+0xffe20]` bounds):
        # the exe reads `[0+8]`, the port uses the id's high half
        ("write_res_no_descriptor", {"id": 0x2345 << 16 | 1, "kind": 5, "desc_max": 0x100}),
    ]


def read_res_id_cases():
    """`readResID`: the saved word (the table row's kind comes back in the high
    half), the script-side word (the slot row's stored id comes back), the
    misses and the two non-open arms."""

    def q(word: int) -> str:
        return _struct.pack("<I", word).hex()

    return [
        ("read_res_saved", {"file": q(1 << 16 | 7), "resources": [[1, 3, "system.rpk"]]}),
        ("read_res_saved_second_row", {"file": q(2 << 16 | 5), "resources": [[1, 3, "a.rpk"], [2, 7, "b.rpk"]]}),
        ("read_res_script", {"file": q(1), "slots": [[1, 5, 0xFFFF0007]]}),
        ("read_res_script_ffff_high", {"file": q(0xFFFF0000 | 3), "slots": [[3, 7, 0xFFFF0003]]}),
        ("read_res_script_second_row", {"file": q(2), "slots": [[1, 5, 0xFFFF0007], [2, 7, 0xFFFF0009]]}),
        ("read_res_zero", {"file": q(0)}),
        ("read_res_unsaved_slot", {"file": q(9)}),
        ("read_res_unsaved_saved_high", {"file": q(4 << 16 | 1)}),
        ("read_res_no_handle", {"file": q(1), "slots": [[1, 5, 0xFFFF0007]], "missing": True}),
        ("read_res_closed_slot", {"file": q(1), "slot": 0}),
        ("read_res_short_read", {"file": "0102"}),
    ]


# --------------------------------------------------------------------------
# I7: the Input / MouseCursor / Controller logging stubs
# --------------------------------------------------------------------------
# These rows call the exe's own input natives with a hand-built `CallInfo`
# frame while the *device* layer is absent: the device table `0x76f9b0` /
# `0x777434` / `0x770104` is left zero-initialised (no DirectInput
# enumeration) and the object plumbing is faked, so every arm that needs a
# device, a controller or an object takes its "no handle" path. That is
# exactly the state a port run is in, and the row is the constant the real
# exe code returns there.
#
# Boundary hooks (everything else is the real exe code, including `0x45d910`
# argument marshalling and the `0x4xxxxx` native bodies):
#
# - the object plumbing `0x42a690` (field by name) answers a canned
#   `java.lang.String`; `0x42ab50` (int field read) answers 0 (the missing
#   field arm), `0x42a010` (field lookup by name, `MouseCursor`'s `cursor`)
#   answers 0, and the two setters (`0x42a9e0`, `0x42a170`) are logged;
# - `0x4174a0` (the interner) answers a canned word and logs the C string it
#   was handed (a *null* pointer is logged as `"<null>"`, which is the
#   `getDeviceName` arm);
# - the log sink `0x62e018` (`0x551140` append, `0x5513b0` flush, the two
#   `printf`-style `0x551330`/`0x551370`) is logged;
# - the device lookups the natives sit on (`0x54de20` = `0x558100`, the
#   `lastKey` poll `0x556e00`) answer 0.
#
# Row: `input.jsonl` under `re/out/io/`, git-ignored, keyed by the native's
# exe address: {"id","exe_addr","class","name","sig","args":{"params"},
# "result":{"word","eax","st0","fault"},"events","logs","oracle","unicorn",
# "cw","date"}. `result.word` is what the VM would push: the `F` result's
# `f32` bits, the `I`/`L` result word, or null for a `V` row. A row whose call
# faults (the unmodelled controller chain) carries "fault" instead of a word.

# addr -> (class, name, sig), in the exe's registration order
INPUT_ROWS = {
    0x47C990: ("java.io.Input", "getAxis", "(II)F"),
    0x47C9C0: ("java.io.Input", "getDeviceName", "(I)Ljava.lang.String;"),
    0x47C9F0: ("java.io.Input", "mapAxis", "(Ljava.util.resource.GameRef;IIIFFFF)V"),
    0x47CA70: ("java.io.Input", "setAxisSmooth", "(Ljava.util.resource.GameRef;IFFFF)V"),
    0x47CAE0: ("java.io.Input", "activeAxis", "(I)I"),
    0x47CB00: ("java.io.Input", "axisName", "(II)Ljava.lang.String;"),
    0x47CC10: ("java.io.Input", "lastKey", "()I"),
    0x47CC50: (
        "java.io.Input",
        "createHotkey",
        "(IILjava.io.Hotkey;Ljava.util.resource.GameRef;Ljava.render.Osd;I)V",
    ),
    0x47CD00: ("java.io.Input", "deleteHotkey", "(Ljava.io.Hotkey;)V"),
    0x47CD40: (
        "java.io.Input",
        "checkHotkeys",
        "(Ljava.util.resource.GameRef;Ljava.render.Osd;)V",
    ),
    0x47CE80: ("java.io.Input", "flushHotkeys", "()V"),
    0x487870: ("java.io.MouseCursor", "getPickedPos", "()Ljava.lang.Vector3;"),
    0x487970: ("java.io.MouseCursor", "getPos", "()Ljava.lang.Vector3;"),
    0x477740: ("java.io.Controller", "user_Add", "(IIIFFFF)I"),
    0x477810: ("java.io.Controller", "user_Del", "(III)I"),
    0x4778B0: ("java.io.Controller", "user_SetAxisSmooth", "(IFFFFF)I"),
    0x477970: ("java.io.Controller", "user_SetAxisSpeed", "(IF)V"),
    0x477A00: ("java.io.Controller", "user_Reset", "()V"),
    0x477A80: ("java.io.Controller", "user_GetAxisVal", "(I)F"),
    0x477B20: ("java.io.Controller", "user_SetAxisForce", "(IFF)V"),
}

# The canned intern word; the port's own word is its string table's (`the
# two string rows intern the empty string` in `crates/slr-io/src/input.rs`).
INPUT_INTERN_WORD = 0x00007000


def input_param_types(sig: bytes) -> list:
    """The descriptor's parameter bytes, `L...;`/`[...;` collapsed."""
    out, i = [], 0
    while i < len(sig):
        b = sig[i : i + 1]
        if b == b"(":
            i += 1
        elif b == b")":
            break
        elif b == b"L":
            out.append("L")
            while i < len(sig) and sig[i : i + 1] != b";":
                i += 1
            i += 1
        elif b == b"[":
            while i < len(sig) and sig[i : i + 1] == b"[":
                i += 1
            if i < len(sig) and sig[i : i + 1] == b"L":
                while i < len(sig) and sig[i : i + 1] != b";":
                    i += 1
            i += 1
            out.append("[")
        else:
            out.append(b.decode())
            i += 1
    return out


def input_params(addr: int, *values) -> list:
    """One case's parameters, in declaration order, typed by the row's
    descriptor: an `F` parameter takes the float the case spells, everything
    else the integer word."""
    types = input_param_types(INPUT_ROWS[addr][2].encode())
    assert len(types) == len(values), (addr, types, values)
    return [
        [ty, _struct.unpack("<I", _struct.pack("<f", value))[0] if ty == "F" else value]
        for ty, value in zip(types, values)
    ]


def input_cases():
    """One case per row (plus parameter variants for the scalar arms), all
    with the device layer absent."""
    return [
        ("input_get_axis_0", 0x47C990, input_params(0x47C990, 0, 0)),
        ("input_get_axis_3_7", 0x47C990, input_params(0x47C990, 3, 7)),
        ("input_get_device_name_0", 0x47C9C0, input_params(0x47C9C0, 0)),
        ("input_get_device_name_5", 0x47C9C0, input_params(0x47C9C0, 5)),
        (
            "input_map_axis",
            0x47C9F0,
            input_params(0x47C9F0, 0, 1, 2, 3, 0.5, 1.5, -2.0, 4.0),
        ),
        (
            "input_set_axis_smooth",
            0x47CA70,
            input_params(0x47CA70, 0, 1, 0.5, 1.5, -2.0, 4.0),
        ),
        ("input_active_axis_2", 0x47CAE0, input_params(0x47CAE0, 2)),
        ("input_active_axis_neg", 0x47CAE0, input_params(0x47CAE0, -1)),
        ("input_axis_name_0_1", 0x47CB00, input_params(0x47CB00, 0, 1)),
        ("input_last_key", 0x47CC10, []),
        (
            "input_create_hotkey",
            0x47CC50,
            input_params(0x47CC50, 1, 2, 0, 0, 0, 3),
        ),
        ("input_delete_hotkey", 0x47CD00, input_params(0x47CD00, 0)),
        ("input_check_hotkeys", 0x47CD40, input_params(0x47CD40, 0, 0)),
        ("input_flush_hotkeys", 0x47CE80, []),
        ("cursor_get_picked_pos", 0x487870, []),
        ("cursor_get_pos", 0x487970, []),
        ("controller_user_add", 0x477740, input_params(0x477740, 1, 2, 3, 0.5, 1.5, -2.0, 4.0)),
        ("controller_user_del", 0x477810, input_params(0x477810, 1, 2, 3)),
        (
            "controller_user_set_axis_smooth",
            0x4778B0,
            input_params(0x4778B0, 1, 0.5, 1.5, -2.0, 4.0, 0.25),
        ),
        ("controller_user_set_axis_speed", 0x477970, input_params(0x477970, 1, 0.5)),
        ("controller_user_reset", 0x477A00, []),
        ("controller_user_get_axis_val", 0x477A80, input_params(0x477A80, 3)),
        ("controller_user_get_axis_val_0", 0x477A80, input_params(0x477A80, 0)),
        ("controller_user_set_axis_force", 0x477B20, input_params(0x477B20, 1, 0.5, 1.5)),
    ]


class InputWorld:
    """One fresh Oracle with the input natives' boundaries hooked."""

    def __init__(self, exe=None):
        self.o = Oracle(exe or _default_exe())
        self.events = []
        self.logs = []
        o = self.o
        o.hook_alloc()
        o.hook(0x54F560, lambda oc, a: (self.events.append(["alloc", a[0]]), oc.alloc(a[0]))[1], "cdecl", 1)
        self.name_obj = _alloc_cstr(o, b"data/cars/gti.car")
        o.hook(0x42A690, self._h_getfield, "cdecl", 3)
        o.hook(0x42AB50, self._h_getint, "cdecl", 2)
        o.hook(0x42A9E0, self._h_setint, "cdecl", 3)
        o.hook(0x42A010, self._h_getfield2, "cdecl", 2)
        o.hook(0x42A430, self._h_getbyname, "cdecl", 2)
        o.hook(0x42A170, self._h_setbyname, "cdecl", 3)
        o.hook(0x4174A0, self._h_intern, "thiscall", 1)
        o.hook(0x551140, self._h_append, "cdecl", 3)
        o.hook(0x5513B0, self._h_flush, "cdecl", 0)
        o.hook(0x551330, self._h_log2, "cdecl", 2)
        o.hook(0x551370, self._h_log2, "cdecl", 2)
        o.hook(0x54DE20, lambda oc, a: 0, "cdecl", 1)
        o.hook(0x556E00, lambda oc, a: 0, "cdecl", 0)
        d = o.alloc(4)
        o.write_u32(d, TC["I"])
        o.write_u32(0x62E00C, d)
        o.write_u32(0x62E008, 0)

    # -- hooks
    def _h_getfield(self, oc, a):
        self.events.append(["getfield", oc.read_cstr(a[1]).decode()])
        return self.name_obj

    def _h_getint(self, oc, a):
        self.events.append(["getint", a[1]])
        return 0

    def _h_setint(self, oc, a):
        self.events.append(["setint", a[1], a[2]])
        return None

    def _h_getfield2(self, oc, a):
        self.events.append(["getfield2", oc.read_cstr(a[1]).decode()])
        return 0

    def _h_getbyname(self, oc, a):
        self.events.append(["getbyname", oc.read_cstr(a[1]).decode()])
        return -1

    def _h_setbyname(self, oc, a):
        self.events.append(["setbyname", oc.read_cstr(a[1]).decode(), a[2]])
        return None

    def _h_intern(self, oc, a):
        try:
            text = oc.read_cstr(a[0]).decode() if a[0] else "<null>"
        except Exception:
            text = "<bad>"
        self.events.append(["intern", text])
        return INPUT_INTERN_WORD

    def _h_append(self, oc, a):
        text = oc.read_cstr(a[1]).decode()
        self.logs.append(text)
        self.events.append(["append", text])
        return None

    def _h_flush(self, oc, a):
        self.events.append(["flush"])
        return None

    def _h_log2(self, oc, a):
        self.events.append(["logfmt", oc.read_cstr(a[0]).decode()])
        return None


def run_input(case, exe=None):
    """One input row: the native's `result.word`, its hook events and the
    log-sink appends."""
    _case_id, addr, params = case
    cls, name, sig = INPUT_ROWS[addr]
    w = InputWorld(exe)
    o = w.o
    obj = o.alloc(0x40)
    cells = [_cell(o, ty, value) for ty, value in reversed(params)]
    cells.append(_cell(o, "L", obj))
    frame = _call_frame(o, cls, name, sig, cells)
    err, eax, st0 = None, None, None
    try:
        res = o.call(addr, [frame], "cdecl", limit=3_000_000)
        eax, st0 = res.eax & 0xFFFFFFFF, res.st0
    except Exception as e:  # the unmodelled controller chain
        err = f"{type(e).__name__}: {e}"
    ret = sig[-1]
    if err:
        word = None
    elif ret == "V":
        word = None
    elif ret == "F":
        word = _struct.unpack("<I", _struct.pack("<f", st0 if st0 is not None else 0.0))[0]
    else:
        word = eax
    result = {"word": word, "eax": eax, "st0": st0}
    if err:
        result = {"word": None, "fault": err}
    return result, w.events, w.logs


def _call_frame(o, cls, name, sig, cells):
    """The hand-built `CallInfo` the natives read (`0x45d910`'s frame):
    the argument container, the signature text, the native record and the
    frame itself."""
    stack = _container(o, cells)
    sigp = _alloc_cstr(o, sig if isinstance(sig, bytes) else sig.encode())
    callinfo = o.alloc(0x20)
    o.write_u32(callinfo + 4, sigp)
    method = o.alloc(0x40)
    o.write_u32(method + 0xC, _alloc_cstr(o, cls.encode()))
    o.write_u32(method + 0x10, _alloc_cstr(o, name.encode()))
    o.write_u32(method + 0x14, callinfo)
    frame = o.alloc(0x40)
    o.write_u32(frame + 8, stack)
    o.write_u32(frame + 0xC, method)
    return frame


def input_rows_list(exe=None, only=None):
    rows = []
    for case_id, addr, params in input_cases():
        if only and only not in case_id:
            continue
        cls, name, sig = INPUT_ROWS[addr]
        result, events, logs = run_input((case_id, addr, params), exe)
        rows.append(
            {
                "id": case_id,
                "exe_addr": addr,
                "class": cls,
                "name": f"{cls.rsplit('.', 1)[-1]}.{name}",
                "sig": sig,
                "args": {"params": params},
                "result": result,
                "events": events,
                "logs": logs,
                "oracle": "unicorn",
                "unicorn": UC_VER,
                "cw": hex(CW),
                "date": _today(),
            }
        )
    return rows


PATH_RUNNERS = []


def run_path(addr, sig, cases, exe=None):
    """Legacy wrapper-level delete/exists probes (I8).

    This older suite calls wrappers, with their platform helpers replaced by
    canned responses. It does not exercise native argument marshalling or
    establish platform return semantics. File.copy and File.move use native
    probes below, executing the complete chain to Win32 boundaries.
    """
    w = World(exe)
    o = w.o
    # The probes see the *converted* name (the wrappers rewrite `'/'` to
    # `'\\'` before calling out), so the expectation is converted too.
    existing = {
        c.get("existing", "data/cars/gti.car").replace("/", "\\").encode()
        for _cid, c in cases
    }

    def h_copy(oc, a):
        dst, src = a[0], a[1]
        data = oc.read_cstr(src)
        oc.write(dst, data + b"\0")
        return dst

    def h_probe(oc, a):
        target = a[0] if len(a) == 1 else a[1]
        return 1 if oc.read_cstr(target) in existing else 0

    o.hook(0x554860, h_copy, "cdecl", 2)
    o.hook(0x5542D0, lambda oc, a: h_probe(oc, a), "cdecl", 2)
    o.hook(0x5586B0, lambda oc, a: h_probe(oc, a), "cdecl", 1)
    o.hook(0x558CF0, lambda oc, a: 1, "cdecl", 1)
    o.hook(0x554A10, lambda oc, a: 1, "cdecl", 3)
    o.hook(0x559090, lambda oc, a: None, "cdecl", 1)
    o.hook(0x5504C0, lambda oc, a: None, "thiscall", 1)
    o.hook(0x5504F0, lambda oc, a: None, "thiscall", 0)
    o.hook(0x555550, lambda oc, a: None, "cdecl", 1)
    o.hook(0x559830, lambda oc, a: 0, "cdecl", 2)
    o.hook(0x559880, lambda oc, a: 0, "cdecl", 1)
    # `0x7686e0` gates the `_access` probe; 0 leaves the open probe in charge.
    o.write_u32(0x7686E0, 0)
    rows = []
    for case_id, case in cases:
        args = [_alloc_cstr(o, label.encode()) for label in case.get("args", [])]
        try:
            eax = o.call(addr, args, "cdecl", limit=500_000).eax
            rows.append((case_id, {"eax": eax & 0xFFFFFFFF}, list(w.events), None))
        except Exception as e:  # noqa: BLE001 - the divergence is the datum
            rows.append((case_id, {"error": f"{type(e).__name__}: {e}"}, list(w.events), None))
            print(f"# {addr:#x} {case_id}: {type(e).__name__}: {e}", file=sys.stderr)
    return rows


# File.move: real native + path/CRT helpers; only platform/cache boundaries stubbed.
FILE_MOVE = 0x486120
MOVE_SIG = "(Ljava.lang.String;Ljava.lang.String;)I"


def move_cases():
    src, dst = "data/cars/source.bin", "data/cars/dest.bin"
    world = {"src": src, "dst": dst, "files": {src: "00534c52ff"}, "dirs": ["data", "data/cars"]}
    return [
        ("move_success", dict(world)),
        ("move_destination_exists", dict(world, files={src: "00534c52ff", dst: "aabbcc"})),
        ("move_source_missing", dict(world, files={})),
        ("move_same_name", dict(world, dst=src)),
        ("move_mixed_slashes", dict(world, src="data\\cars/source.bin", dst="data/cars\\dest.bin")),
        ("move_destination_directory", dict(world, dirs=["data", "data/cars", dst])),
        ("move_missing_parent", dict(world, dst="save/missing/new.bin")),
        ("move_parent_is_file", dict(world, dst="blocked/new.bin", files={src: "00534c52ff", "blocked": "01"})),
        ("move_closed_source_handle", dict(world, cache=[{"path": src, "in_use": False}])),
        ("move_closed_destination_handle", dict(world, files={src: "00534c52ff", dst: "aabbcc"}, cache=[{"path": dst, "in_use": False}])),
        ("move_active_source_handle", dict(world, cache=[{"path": src, "in_use": True}])),
        ("move_two_closed_source_handles", dict(world, cache=[{"path": src, "in_use": False}, {"path": src, "in_use": False}])),
        ("move_unrelated_closed_handle", dict(world, files={src: "00534c52ff", "data/cars/other.bin": "1234"}, cache=[{"path": "data/cars/other.bin", "in_use": False}])),
    ]


def run_move(case, exe=None):
    return _run_file_operation(case, exe, copy=False)


def run_copy(case, exe=None):
    return _run_file_operation(case, exe, copy=True)


def _run_file_operation(case, exe=None, *, copy=False):
    """Execute the real native/layer/helper; replace only external boundaries.

    Move: 0x486120 -> 0x54c650 -> 0x558ba0, absent destination required.
    Copy: 0x4860f0 -> 0x54c550 -> 0x558a50, bFailIfExists is FALSE.
    Both helpers convert Win32 BOOL to 0/UINT_MAX and retry after error 3.
    The boundary models plain files and controlled read/write handle sharing;
    it does not run Windows or establish ACL/hidden/encryption/link behavior.
    Exact failure codes here are controlled inputs, not observed Windows codes.
    No game files are modified. Cache flush, locking and diagnostics are hooks.
    """
    _case_id, spec = case
    o = Oracle(exe or _default_exe())
    files = {name.replace("/", "\\"): bytes.fromhex(data) for name, data in spec["files"].items()}
    dirs = {name.replace("/", "\\") for name in spec.get("dirs", [])} | {""}
    events, last_error = [], [0]
    cache = spec.get("cache", [])
    def text(pointer): return o.read_cstr(pointer).decode("ascii")
    def parent(name): return name.rsplit("\\", 1)[0] if "\\" in name else ""
    def sharing_conflict(oc, src, dst):
        for index, item in enumerate(cache, 1):
            if oc.read_u32(0x766460 + index * 0x10c) == 0xffffffff:
                continue
            name = item["path"].replace("/", "\\")
            if name == dst or (name == src and (not copy or item.get("mode", 0) == 1)):
                return True
        return False
    def operate(oc, args):
        src, dst = map(text, args[:2])
        if copy:
            assert args[2] == 0, "real helper must permit destination overwrite"
        if src not in files:
            error = 2
        elif parent(dst) not in dirs:
            error = 3
        elif dst in dirs or dst == src or (not copy and dst in files):
            error = 183
        elif sharing_conflict(oc, src, dst):
            # Read: desired READ/share READ (0x558d80). Write: desired
            # WRITE/share WRITE (0x558e40). Reads permit copying the source;
            # source writers and any retained destination refuse copy opens.
            # Neither open permits FILE_SHARE_DELETE, so both block moves.
            error = 32
        else:
            files[dst] = files[src] if copy else files.pop(src)
            error = 0
        last_error[0] = error
        event = (["CopyFileA", src, dst, args[2], error] if copy
                 else ["MoveFileA", src, dst, error])
        events.append(event)
        return int(error == 0)
    def mkdir(oc, args):
        name = text(args[0])
        error = 183 if name in dirs or name in files else 3 if parent(name) not in dirs else 0
        if not error: dirs.add(name)
        last_error[0] = error
        events.append(["CreateDirectoryA", name, error])
        return int(not error)
    def create(oc, args):
        name = text(args[0])
        error = 3 if parent(name) not in dirs else 5 if name in dirs else 0
        if not error: files[name] = b""
        last_error[0] = error
        events.append(["CreateFileA", name, args[4], error])
        return 0xffffffff if error else 0xabc
    def format_message(oc, args):
        # Formatting/diagnostics are a recorded external boundary, not a
        # fabricated success/failure branch inside the original helper.
        oc.write(args[4], b"synthetic Win32 error\0")
        return len(b"synthetic Win32 error")
    hooks = {("CopyFileA" if copy else "MoveFileA"): (operate, 3 if copy else 2), "GetLastError": (lambda oc, a: last_error[0], 0),
             "CreateDirectoryA": (mkdir, 2), "CreateFileA": (create, 7),
             "CloseHandle": (lambda oc, a: 1, 1), "FormatMessageA": (format_message, 7)}
    for address, name in o.import_names.items():
        symbol = name.split("!")[-1]
        if symbol in hooks:
            fn, argc = hooks[symbol]
            o.hook(address, fn, "stdcall", argc)
    o.hook(0x5504c0, lambda oc, a: 0, "thiscall", 1)
    o.hook(0x5504f0, lambda oc, a: 0, "thiscall", 0)
    o.hook(0x555550, lambda oc, a: events.append(["flush", text(a[0])]), "cdecl", 1)
    o.hook(0x554da0, lambda oc, a: 0, "cdecl", 5)
    o.hook(0x558510, lambda oc, a: events.append(["report_error", last_error[0]]), "cdecl", 2)
    # Free slot table, then controlled retained read/write handles. The native and
    # layer perform the real first-match scan, close and handle=-1 stores.
    for index in range(1, 32): o.write_u32(0x766460 + index * 0x10c, 0xffffffff)
    for index, item in enumerate(cache, 1):
        base = 0x766460 + index * 0x10c
        o.write_u32(base, 0x100 + index)
        o.write(base + 4, item["path"].replace("/", "\\").encode("ascii") + b"\0")
        o.write(base + 0x108, bytes([int(item["in_use"]), item.get("mode", 0)]))
    o.hook(0x554a10, lambda oc, a: int(text(a[0]) != text(a[1])), "cdecl", 3)
    o.hook(0x559090, lambda oc, a: events.append(["close_cached", a[0] - 0x100]), "cdecl", 1)
    src, dst = (_alloc_cstr(o, spec[key].encode("ascii")) for key in ("src", "dst"))
    cells = [_cell(o, "L", dst), _cell(o, "L", src), _cell(o, "L", 0)]
    frame = _call_frame(o, "java.io.File", "copy" if copy else "move", MOVE_SIG, cells)
    result = o.call(FILE_COPY if copy else FILE_MOVE, [frame], "cdecl", limit=500_000)
    if result.esp_delta != 0:
        raise AssertionError(f"{_case_id}: unbalanced native cdecl stack: {result.esp_delta}")
    return {"eax": result.eax & 0xffffffff,
            "files": {name.replace("\\", "/"): data.hex() for name, data in sorted(files.items())},
            "dirs": sorted(name.replace("\\", "/") for name in dirs if name), "events": events,
            "cache": [{"handle_live": o.read_u32(0x766460 + index * 0x10c) != 0xffffffff,
                       "in_use": bool(o.read(0x766460 + index * 0x10c + 0x108, 1)[0]),
                       **({"mode": o.read(0x766460 + index * 0x10c + 0x109, 1)[0]} if copy else {})}
                      for index in range(1, len(cache) + 1)]}


def move_rows_list(exe=None, only=None):
    return [{"id": case_id, "exe_addr": FILE_MOVE, "name": "File.move", "sig": MOVE_SIG,
             "args": spec, "result": run_move((case_id, spec), exe), "oracle": "unicorn",
             "unicorn": UC_VER, "cw": hex(CW), "date": _today()}
            for case_id, spec in move_cases() if not only or only in case_id]

# File.copy native 0x4860f0 -> layer 0x54c550 -> helper 0x558a50.
FILE_COPY = 0x4860f0


def copy_cases():
    cases = [(name.replace("move_", "copy_"), spec) for name, spec in move_cases()]
    src, dst = "data/cars/source.bin", "data/cars/dest.bin"
    base = {"src": src, "dst": dst, "files": {src: "00534c52ff", dst: "aabbcc"}, "dirs": ["data", "data/cars"]}
    cases += [
        ("copy_active_destination_handle", dict(base, cache=[{"path": dst, "in_use": True}])),
        ("copy_two_closed_destination_handles", dict(base, cache=[{"path": dst, "in_use": False}, {"path": dst, "in_use": False}])),
        ("copy_active_source_writer", dict(base, cache=[{"path": src, "in_use": True, "mode": 1}])),
        ("copy_empty_source", dict(base, files={src: "", dst: "aabbcc"})),
    ]
    return cases


def copy_rows_list(exe=None, only=None):
    return [{"id": case_id, "exe_addr": FILE_COPY, "name": "File.copy", "sig": MOVE_SIG,
             "args": spec, "result": run_copy((case_id, spec), exe), "oracle": "unicorn",
             "unicorn": UC_VER, "cw": hex(CW), "date": _today()}
            for case_id, spec in copy_cases() if not only or only in case_id]


def path_cases():
    a = "data/cars/gti.car"
    return {
        0x486170: [
            ("exists_present", {"existing": a, "args": [a]}),
            ("exists_absent", {"existing": a, "args": ["data/cars/none.car"]}),
            ("exists_slash_converted", {"existing": a, "args": ["data\\cars\\gti.car"]}),
        ],
        0x486150: [
            ("delete_relative", {"args": [a]}),
            ("delete_refused_dotdot", {"args": ["..\\escape.dat"]}),
        ],
    }


def _path_runner(addr, sig):
    wrapper, _argc = PATH_ROWS[addr]

    def run(case, exe=None):
        return run_path(wrapper, sig, [("row", case)], exe)[0][1:]

    return run


# Each `(exe address, wrapper address, cdecl argument count)`.
PATH_ROWS = {
    0x486150: (0x54C7C0, 1),  # delete -> table scan + DeleteFileA
    0x486170: (0x54C170, 1),  # exists -> access/open probe
}

RUNNERS = [
    ("open", 0x485440, "cdecl", run_open, open_cases()),
    ("close", 0x485870, "cdecl", run_close, close_cases()),
    ("write_res_ref", WRITE_RES_REF, "cdecl", run_write_res_ref, write_res_ref_cases()),
    ("read_res_id", READ_RES_ID, "cdecl", run_read_res_id, read_res_id_cases()),
] + [
    (label, addr, "cdecl", _path_runner(addr, b""), cases)
    for label, addr, cases in [
        ("delete", 0x486150, path_cases()[0x486150]),
        ("exists", 0x486170, path_cases()[0x486170]),
    ]
]


def _run(name, case_id, case, exe):
    for label, addr, conv, runner, _cases in RUNNERS:
        if label != name:
            continue
        out = runner(case, exe)
        return {
            "id": case_id,
            "exe_addr": addr,
            "name": f"File.{name}",
            "convention": conv,
            "args": case,
            "result": out[0],
            "events": out[1],
            "struct": out[2] if len(out) > 2 else None,
        }
    raise SystemExit(f"unknown native {name}")


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = []
    for name, _addr, _conv, _runner, cases in RUNNERS:
        for case_id, case in cases:
            if only and only not in case_id:
                continue
            row = _run(name, case_id, case, exe)
            row.update({"oracle": "unicorn", "unicorn": UC_VER, "cw": hex(CW), "date": _today()})
            rows.append(row)
    # five files: the I2 rows, the I5 `ResourceRef` pair, the path rows and
    # the FindFile rows (I8)
    path_addrs = (0x4860F0, 0x486150, 0x486170)
    groups = {
        "open_close.jsonl": [
            r for r in rows
            if r["exe_addr"] not in (WRITE_RES_REF, READ_RES_ID, *path_addrs)
        ],
        "resid.jsonl": [r for r in rows if r["exe_addr"] in (WRITE_RES_REF, READ_RES_ID)],
        "path.jsonl": [r for r in rows if r["exe_addr"] in path_addrs],
    }
    groups["findfile.jsonl"] = findfile_rows_list(exe, only)
    for name, group in groups.items():
        if not group:
            continue
        path = os.path.join(OUT_DIR, name)
        with open(path, "w") as f:
            for r in group:
                f.write(json.dumps(r, sort_keys=True) + "\n")
        print(f"wrote {len(group)} rows to {path}")
    copy_rows = copy_rows_list(exe, only)
    if copy_rows:
        path = os.path.join(OUT_DIR, "copy.jsonl")
        with open(path, "w") as output:
            for row in copy_rows:
                output.write(json.dumps(row, sort_keys=True) + "\n")
        print(f"wrote {len(copy_rows)} native copy rows to {path}")
    move_rows = move_rows_list(exe, only)
    if move_rows:
        path = os.path.join(OUT_DIR, "move.jsonl")
        with open(path, "w") as output:
            for row in move_rows:
                output.write(json.dumps(row, sort_keys=True) + "\n")
        print(f"wrote {len(move_rows)} native move rows to {path}")
    # the I7 input stubs (their own file: their row shape carries `logs`)
    input_rs = input_rows_list(exe, only)
    if input_rs:
        path = os.path.join(OUT_DIR, "input.jsonl")
        with open(path, "w") as f:
            for r in input_rs:
                f.write(json.dumps(r, sort_keys=True) + "\n")
        print(f"wrote {len(input_rs)} rows to {path}")
    return rows


def probe(exe=None, only=None):
    rows = []
    for name, _addr, _conv, _runner, cases in RUNNERS:
        for case_id, case in cases:
            if only and only not in case_id:
                continue
            rows.append(_run(name, case_id, case, exe))
    for r in rows:
        print(f"== {r['id']} {r['name']} result={r['result']}")
        for ev in r["events"]:
            print("   ", ev)
        if r.get("struct"):
            print("    struct", r["struct"])
    for r in findfile_rows_list(exe, only):
        print(f"== {r['id']} {r['name']} {r['sig']} result={r['result']}")
        for ev in r["events"]:
            print("   ", ev)
    for r in input_rows_list(exe, only):
        print(f"== {r['id']} {r['name']} {r['sig']} result={r['result']}")
        print("    logs", r["logs"])
        for ev in r["events"]:
            print("   ", ev)
    return rows


# --------------------------------------------------------------------------
# I8: the FindFile rows (the Win32 find imports hooked)
# --------------------------------------------------------------------------
# `java.io.FindFile`'s three natives sit on the CRT find layer `0x54cf50`
# (open + collect + sort + deliver record 0), `0x54d1c0` (deliver the next
# record) and `0x54d240` (free the slot), which delegate the OS search to the
# imported `FindFirstFileA`/`FindNextFileA`/`FindClose` through the IAT slots
# `0x5e7098`/`0x5e709c`/`0x5e70a0` (`oracle.py` points them at `xor eax,eax;
# ret` thunks, which is why the natives could not run before this slice).
#
# This section hooks those three thunks (stdcall, so the exe's wrappers get
# their 8/8/4 stack bytes back) against a synthetic directory, leaving the
# exe's own find layer, its `0x5d765d` `qsort` and its `0x54d1a0`/`0x554a10`
# compare real. The rows then carry the slot's records as the exe left them in
# memory (`0x7663d8` block pointers, `0x768660` counts, `0x7685e0` cursors), so
# a replay pins the enumeration, the sort and the natives' filter against the
# exe rather than against a re-derivation.
#
# Row: `findfile.jsonl` under `re/out/io/`, git-ignored:
# {"id","exe_addr","class","name","args":{"pattern","flags","entries",
# "files_dir"},"result":{"steps","handle_field","flags_field","after_close",
# "slot"},"events","oracle","unicorn","cw","date"}.

FIND_FIRST = 0x487CB0
FIND_NEXT = 0x487DA0
FIND_CLOSE_NATIVE = 0x487E70
FIND_FIRST_SIG = b"(Ljava.lang.String;I)Ljava.lang.String;"
FIND_NEXT_SIG = b"()Ljava.lang.String;"
FIND_CLOSE_SIG = b"()V"
FIND_NATIVES = {
    FIND_FIRST: ("java.io.FindFile", "first", FIND_FIRST_SIG),
    FIND_NEXT: ("java.io.FindFile", "next", FIND_NEXT_SIG),
    FIND_CLOSE_NATIVE: ("java.io.FindFile", "close", FIND_CLOSE_SIG),
}
# the IAT slots the find layer calls through, and the thunks `oracle.py` put
# there (its own import table, so read them rather than hard-coding)
FIND_IMPORT_SLOTS = (0x5E7098, 0x5E709C, 0x5E70A0)
# the find layer's globals: the 0x40-entry OS-handle table, its
# WIN32_FIND_DATAA array (`0x140` bytes each: attributes at +0, cFileName at
# +0x2c) and the per-slot record blocks / counts / cursors
FIND_HANDLES = 0x777458
FIND_DATA = 0x777758
FIND_DATA_STRIDE = 0x140
WIN32_ATTR_OFF = 0x00
WIN32_NAME_OFF = 0x2C
INVALID_HANDLE_VALUE = 0xFFFFFFFF
SLOT_BLOCKS = 0x7663D8
SLOT_COUNTS = 0x768660
SLOT_CURSORS = 0x7685E0
SLOT_RECORD = 0x104
SLOT_NAME_LEN = 0x100
FIND_SLOTS = 0x20
# `FILE_ATTRIBUTE_DIRECTORY` / `FILE_ATTRIBUTE_ARCHIVE`
ATTR_DIR = 0x10
ATTR_ARCHIVE = 0x20
# the canned intern word (the port's own table mints its own)
FIND_INTERN_WORD = 0x00007001


def _dos_mask(pattern: bytes) -> bytes:
    """`FindFirstFileA`'s mask: the pattern's last component."""
    return pattern.replace(b"/", b"\\").rsplit(b"\\", 1)[-1]


def pattern_match(mask: bytes, name: bytes) -> bool:
    """The documented `FindFirstFileA` mask semantics the port reproduces:
    `*` any run, `?` one character, ASCII case-insensitive, `*.*` = `*`."""
    if mask in (b"*", b"*.*"):
        return True
    fold = lambda b: b + 0x20 if 0x41 <= b <= 0x5A else b  # noqa: E731
    m = [fold(c) for c in mask]
    n = [fold(c) for c in name]
    # a mask component that ends in `.` matches nothing per the docs (the
    # callers here never use one)
    dp = [[False] * (len(n) + 1) for _ in range(len(m) + 1)]
    dp[0][0] = True
    for i, mc in enumerate(m, 1):
        if mc == ord("*"):
            dp[i][0] = dp[i - 1][0]
        for j, nc in enumerate(n, 1):
            if mc == ord("*"):
                dp[i][j] = dp[i - 1][j] or dp[i][j - 1]
            elif mc == ord("?") or mc == nc:
                dp[i][j] = dp[i - 1][j - 1]
    return dp[len(m)][len(n)]


class FindWorld:
    """One fresh Oracle with the three find imports served from a synthetic
    directory and the `handle`/`flags` fields kept in Python."""

    def __init__(self, exe=None, entries=()):
        self.o = Oracle(exe or _default_exe())
        self.entries = list(entries)  # (name, is_dir)
        self.fields = {}              # (object word, field name) -> word
        self.searches = {}            # OS handle -> index into self.entries
        self.events = []
        self.interns = []
        o = self.o
        o.hook_alloc()
        # the CRT realloc the record block grows with (`0x54f580`)
        o.hook(0x54F580, self._h_realloc, "cdecl", 2)
        # the three imports, by the thunk `oracle.py` installed
        first = o.read_u32(FIND_IMPORT_SLOTS[0])
        nxt = o.read_u32(FIND_IMPORT_SLOTS[1])
        close = o.read_u32(FIND_IMPORT_SLOTS[2])
        assert o.import_names[first].endswith("FindFirstFileA"), o.import_names[first]
        assert o.import_names[nxt].endswith("FindNextFileA"), o.import_names[nxt]
        assert o.import_names[close].endswith("FindClose"), o.import_names[close]
        o.hook(first, self._h_find_first, "stdcall", 2)
        o.hook(nxt, self._h_find_next, "stdcall", 2)
        o.hook(close, self._h_find_close, "stdcall", 1)
        # the two field accessors the natives use by name
        o.hook(0x42A170, self._h_set_field, "cdecl", 3)
        o.hook(0x42A430, self._h_get_field, "cdecl", 2)
        o.hook(0x4174A0, self._h_intern, "thiscall", 1)

    # -- the imports
    def _write_data(self, data: int, index: int) -> None:
        name, is_dir = self.entries[index]
        attr = ATTR_DIR if is_dir else ATTR_ARCHIVE
        self.o.write_u32(data + WIN32_ATTR_OFF, attr)
        self.o.write(data + WIN32_NAME_OFF, name.encode() + b"\0")

    def _match(self, mask: bytes):
        return [i for i, (name, _d) in enumerate(self.entries) if pattern_match(mask, name.encode())]

    def _h_find_first(self, oc, a):
        pattern, data = a
        text = oc.read_cstr(pattern)
        mask = _dos_mask(text)
        self.events.append(["find_first", text.decode()])
        hits = self._match(mask)
        if not hits:
            return INVALID_HANDLE_VALUE
        index = hits[0]
        handle = 0x1000 + index
        self.searches[handle] = (mask, index)
        self._write_data(data, index)
        self.events.append(["entry", self.entries[index][0], self.entries[index][1]])
        return handle

    def _h_find_next(self, oc, a):
        handle, data = a
        mask, last = self.searches.get(handle, (b"*", -1))
        hits = [i for i in self._match(mask) if i > last]
        if not hits:
            self.events.append(["find_next_end"])
            return 0
        index = hits[0]
        self.searches[handle] = (mask, index)
        self._write_data(data, index)
        self.events.append(["entry", self.entries[index][0], self.entries[index][1]])
        return 1

    def _h_find_close(self, oc, a):
        (handle,) = a
        self.events.append(["find_close", handle])
        self.searches.pop(handle, None)
        return 1

    # -- the CRT and the natives' field plumbing
    def _h_realloc(self, oc, a):
        old, size = a
        new = oc.alloc(size)
        if old:
            oc.write(new, oc.read(old, min(size, oc.heap_sizes.get(old, size))))
        return new

    def _h_set_field(self, oc, a):
        obj, name, value = a
        key = (obj, oc.read_cstr(name).decode())
        self.fields[key] = value
        self.events.append(["set_field", key[1], value])
        return None

    def _h_get_field(self, oc, a):
        obj, name = a
        key = (obj, oc.read_cstr(name).decode())
        # the class's `FILD` default is -1 for both fields
        value = self.fields.get(key, 0xFFFFFFFF)
        self.events.append(["get_field", key[1], value])
        return value

    def _h_intern(self, oc, a):
        text = oc.read_cstr(a[0]).decode() if a[0] else "<null>"
        self.interns.append(text)
        self.events.append(["intern", text])
        return FIND_INTERN_WORD

    # -- state the rows carry
    def slot_state(self, index: int) -> dict:
        o = self.o
        block = o.read_u32(SLOT_BLOCKS + 4 * index)
        count = o.read_u32(SLOT_COUNTS + 4 * index)
        cursor = o.read_u32(SLOT_CURSORS + 4 * index)
        records = []
        if block:
            for i in range(count):
                base = block + i * SLOT_RECORD
                name = o.read(base, SLOT_NAME_LEN).split(b"\0")[0].decode()
                records.append([name, o.read_u32(base + SLOT_NAME_LEN)])
        return {
            "block": bool(block),
            "count": count,
            "cursor": cursor,
            "records": records,
        }


def findfile_cases():
    """One case per row: the synthetic directory, the pattern and the flags,
    plus which natives to drive. `first` then `next` to exhaustion then
    `close` is the native sequence the port's own tests use."""
    cars = [
        ("gti.car", False),
        ("mustang.car", False),
        ("skyline.car", False),
        ("sub", True),
        ("notes.txt", False),
    ]
    all_kinds = [
        ("alpha.car", False),
        ("bravo.car", False),
        ("delta.car", False),
        ("subdir", True),
        ("zeta.dat", False),
    ]
    return [
        ("find_first_files_dirs", {"entries": cars, "pattern": "data/cars/*.car", "flags": 0, "steps": ["first", "next", "close"]}),
        ("find_first_files_only", {"entries": all_kinds, "pattern": "data/cars/*.car", "flags": 1, "steps": ["first", "next", "close"]}),
        ("find_first_dirs_only", {"entries": all_kinds, "pattern": "data/cars/*", "flags": 2, "steps": ["first", "next", "close"]}),
        ("find_first_exact_name", {"entries": all_kinds, "pattern": "data/cars/bravo.car", "flags": 0, "steps": ["first", "next", "close"]}),
        ("find_first_no_match", {"entries": all_kinds, "pattern": "data/cars/*.zzz", "flags": 0, "steps": ["first", "next", "close"]}),
        ("find_first_dirs_all", {"entries": all_kinds, "pattern": "data/cars/*", "flags": 0, "steps": ["first", "next", "close"]}),
        ("find_next_without_first", {"entries": all_kinds, "pattern": "data/cars/*.car", "flags": 0, "steps": ["next", "close"]}),
    ]


def run_findfile(case, exe=None):
    """One `findfile.jsonl` row: drive the natives over a synthetic directory
    and report the delivered names, the fields and the slot's records."""
    _id, spec = case
    w = FindWorld(exe, spec["entries"])
    o = w.o
    obj = o.alloc(0x40)
    pattern = spec["pattern"]
    flags = spec["flags"]
    steps = []
    slot_index = None
    last_intern = None

    def call(addr, cells):
        cls, name, sig = FIND_NATIVES[addr]
        frame = _call_frame(o, cls, name, sig, cells)
        before = len(w.interns)
        res = o.call(addr, [frame], "cdecl", limit=3_000_000)
        interned = w.interns[before:]
        return res.eax & 0xFFFFFFFF, (interned[-1] if interned else None)

    if "first" in spec["steps"]:
        # The container holds *cell* pointers (`0x45d910` reads `[cell+8]`),
        # so the pattern must be wrapped like every other argument.
        cells = [
            _cell(o, "I", flags),
            _cell(o, "L", _alloc_cstr(o, pattern.encode())),
            _cell(o, "L", obj),
        ]
        word, name = call(FIND_FIRST, cells)
        steps.append({"step": "first", "word": word, "name": name})
        handle = w.fields.get((obj, "handle"), None)
        if handle is not None and handle < 0x8000_0000:
            slot_index = handle
    for _ in range(0x40):
        if "next" not in spec["steps"]:
            break
        word, name = call(FIND_NEXT, [_cell(o, "L", obj)])
        if word == 0:
            steps.append({"step": "next", "word": word, "name": None})
            break
        steps.append({"step": "next", "word": word, "name": name})
        last_intern = name
    if "close" in spec["steps"]:
        call(FIND_CLOSE_NATIVE, [_cell(o, "L", obj)])
        steps.append({"step": "close", "word": None, "name": None})

    slot = w.slot_state(slot_index) if slot_index is not None else None
    result = {
        "steps": steps,
        "handle_field": w.fields.get((obj, "handle")),
        "flags_field": w.fields.get((obj, "flags")),
        "slot": slot,
        "after_close": w.slot_state(slot_index) if slot_index is not None else None,
    }
    events = list(w.events)
    details = {"slot_index": slot_index, "last_intern": last_intern}
    return result, events, details


def findfile_rows_list(exe=None, only=None):
    rows = []
    for case_id, spec in findfile_cases():
        if only and only not in case_id:
            continue
        result, events, details = run_findfile((case_id, spec), exe)
        rows.append(
            {
                "id": case_id,
                "exe_addr": FIND_FIRST,
                "class": "java.io.FindFile",
                "name": "FindFile.first",
                "sig": FIND_FIRST_SIG.decode(),
                "args": {
                    "pattern": spec["pattern"],
                    "flags": spec["flags"],
                    "entries": [[n, d] for n, d in spec["entries"]],
                    "steps": spec["steps"],
                },
                "result": result,
                "details": details,
                "events": events,
                "oracle": "unicorn",
                "unicorn": UC_VER,
                "cw": hex(CW),
                "date": _today(),
            }
        )
    return rows


def verify_move(exe=None, only=None, *, copy=False):
    checked = 0
    operation = "copy" if copy else "move"
    with open(os.path.join(OUT_DIR, operation + ".jsonl")) as source:
        for line in source:
            row = json.loads(line)
            if only and only not in row["id"]:
                continue
            actual = (run_copy if copy else run_move)((row["id"], row["args"]), exe)
            if actual != row["result"]:
                raise AssertionError(f"{row['id']}: fresh native output differs: {actual!r} != {row['result']!r}")
            checked += 1
    if not checked:
        raise AssertionError(f"no {operation} rows verified")
    print(f"verified {checked} File.{operation} native rows by fresh Unicorn execution")


def main(argv=None):
    import argparse

    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("cmd", choices=["gen", "probe", "verify-move", "verify-copy"], nargs="?", default="probe")
    p.add_argument("--exe", default=None)
    p.add_argument("--only", default=None)
    a = p.parse_args(argv)
    exe = a.exe or _default_exe()
    if a.cmd == "gen":
        gen(exe, a.only)
    elif a.cmd in ("verify-move", "verify-copy"):
        verify_move(exe, a.only, copy=a.cmd == "verify-copy")
    else:
        probe(exe, a.only)


if __name__ == "__main__":
    main()
