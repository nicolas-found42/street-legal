#!/usr/bin/env python3
"""Unicorn vectors for the SDAT reader/writer functions (M2-05, issue #20).

Calls the exe's `File` natives and the SDAT store/close code directly with
hand-built objects in emulator memory (no JVM boot needed):

  0x485b20 File.write(int)     0x485bc0 File.write(float)
  0x485b70 File.readInt()      0x485c10 File.readFloat()
  0x484fa0 SDAT slot-run writer (JPEG-slot triples + companion hooks)
  0x485870 File.close(): SDAT footer writer (+ the slot run via 0x484fa0)

Boundaries that are replaced by Python hooks (everything else is the real
exe code, including `0x45d910` argument popping, `0x42ab50` field lookup,
`0x5d7190` strcmp and the C runtime `sprintf`/`sscanf`):

- the file layer `0x54cf00` (write), `0x54cec0` (read), `0x54cca0` (close):
  they are logged as events (slot, bytes); the CRT `fwrite` slot table
  behind them is NOT emulated;
- JPEG-companion savers `0x48daa0` / `0x48da30` and the screenshot prepare
  call `0x419860`: logged as events (companion file name, kind);
- list/object plumbing used by the close cleanup (`0x429390`, `0x4293a0`,
  `0x45fa70`, `0x42a9e0`, free `0x54f5b0`): logged / constant.

Layouts (machine-code verified from the disassembly, see docs/formats.md):
- call frame F: [+8] -> operand-stack T_Container {cap,data,count};
  [+0xc] -> method info M; [M+0x14] -> call info C; [C+4] -> signature
  cstr "(I)V" (cached, so `0x41def0` returns it directly).
  The `this` cell is the TOP of the stack, the single argument below it.
- File object O: [O+0x10] -> class struct K; [K+0xc] -> field container
  {count,data}; field `DAT_0062e008` is an 'I' cell whose payload word is a
  POINTER to the native file struct S (a NULL payload or a field of another
  type makes `0x42ab50` log and return 0).
- native file struct S: [0] = file slot (0 = closed), [+8] resource count,
  [+0xc] / [+0x2c] list objects whose head word [+8] is the resource list
  head [+0x14] / the JPEG-slot list head [+0x34], [+0x28] slot count word,
  [+0x48] mode (0 read, 1 write),
  [+0x4c] inline NUL-terminated file name.
- JPEG-slot node: [+4] next, [+0xc] id, [+0x10] kind (5 or 7), [+0x14]
  embedded companion object, [+0x20] object -> {[+0x58] blob ptr, [+0x5c]
  blob len}; the list ends in a sentinel node whose next is 0.
- resource node: [+4] next, [+0xc] id, [+0x14] inline NUL-terminated path;
  same sentinel-terminated list.
- file-format major version byte: `0x612c76` (byte 2 of `0x00020100`).

Row (JSONL under re/out/sdat/, git-ignored):
  {"id","exe_addr","name","convention","args","result","events",
   "oracle","unicorn","cw","date"}
`events` is the ordered file-layer / hook log (never arena addresses). The
Rust test rebuilds the scenario in the port and compares result + events.
Results the exe leaves undefined (uninitialised stack slot returned by
`readInt` / `readFloat` on a closed handle or a short read) are recorded as
{"unspecified": true}, never as the stack garbage.
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
OUT_DIR = os.path.join(REPO, "re", "out", "sdat")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}
FIELD_IDX = 3          # value planted in DAT_0062e008 (the `handle` field id)
FIELD_CELL_COUNT = 5   # fields in the fake class struct
CW = CW_D3D9
UC_VER = "2.1.4"
VERSION_BYTE = 0x612C76


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


class World:
    """One fresh Oracle plus the hook state for a scenario."""

    def __init__(self, exe=None, wr_rv=None, read_data=b""):
        self.o = Oracle(exe or _default_exe())
        self.events: list = []
        self.wr_rv = wr_rv
        self.read_data = bytearray(read_data)
        o = self.o
        o.hook_alloc()
        o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
        o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
        # error-log helper reached only on a bad field lookup: returns a
        # method-name string for the (stubbed) log sink
        self._logname = _alloc_cstr(o, b"log")
        o.hook(0x407D90, lambda oc, a: self._logname, "thiscall", 0)
        o.hook(0x54CF00, self._h_write, "cdecl", 3)
        o.hook(0x54CEC0, self._h_read, "cdecl", 3)
        o.hook(0x54CCA0, self._h_close, "cdecl", 1)
        o.hook(0x54F5B0, self._h_free, "cdecl", 1)
        o.hook(0x54F560, self._h_allocbuf, "cdecl", 1)
        o.hook(0x4174A0, self._h_intern, "thiscall", 1)
        o.hook(0x419860, self._h_prepare, "thiscall", 4)
        o.hook(0x48DAA0, lambda oc, a: self._h_comp(5, a), "thiscall", 1)
        o.hook(0x48DA30, lambda oc, a: self._h_comp(7, a), "thiscall", 1)
        o.hook(0x429390, lambda oc, a: 1, "thiscall", 0)
        o.hook(0x4293A0, lambda oc, a: 0, "thiscall", 0)
        o.hook(0x45FA70, self._h_listfree, "thiscall", 0)
        o.hook(0x42A9E0, self._h_setfield, "cdecl", 3)
        self.base = 0  # native file struct address (for listfree offsets)
        d = o.alloc(4)
        o.write_u32(d, TC["I"])
        o.write_u32(0x62E00C, d)
        o.write_u32(0x62E008, FIELD_IDX)

    # -- hooks
    def _h_allocbuf(self, oc, a):
        n = a[0] & 0xFFFFFFFF
        if n == 0 or n & 0x80000000:
            return 0
        p = oc.alloc(n)
        self.events.append(["alloc", n, p])
        return p

    def _h_intern(self, oc, a):
        try:
            text = oc.read_cstr(a[0]).decode() if a[0] else "<null>"
        except Exception:
            text = "<bad>"
        self.events.append(["intern", text])
        return 0x00ABCDEF

    def _h_write(self, oc, a):
        slot, buf, n = a
        self.events.append(["w", slot, oc.read(buf, n).hex()])
        # a real `fwrite` answers the item count, so the byte count here; the
        # write_int/float cases pin the discarded-return arms explicitly
        return self.wr_rv if self.wr_rv is not None else n

    def _h_read(self, oc, a):
        slot, buf, n = a
        got = bytes(self.read_data[:n])
        del self.read_data[:n]
        oc.write(buf, got)
        self.events.append(["r", slot, n])
        return len(got)

    def _h_close(self, oc, a):
        self.events.append(["close", a[0]])
        return 0

    def _h_free(self, oc, a):
        self.events.append(["free"])

    def _h_prepare(self, oc, a):
        self.events.append(["prepare", a[0], a[1], a[2], a[3]])

    def _h_comp(self, kind, a):
        self.events.append(["companion", kind, self.o.read_cstr(a[0]).decode()])

    def _h_listfree(self, oc, a):
        self.events.append(["listfree", oc.reg("ecx") - self.base])

    def _h_setfield(self, oc, a):
        self.events.append(["setfield", a[1], a[2]])


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


def _file_obj(w: World, state: str, slot: int):
    """Return (object, native struct S or 0). state: open|closed|missing|
    wrongtype (see module docstring)."""
    o = w.o
    s = 0
    cells = []
    for i in range(FIELD_CELL_COUNT):
        cells.append(_cell(o, "I", 0))
    if state in ("open", "closed"):
        s = o.alloc(0x80)
        o.write_u32(s, slot if state == "open" else 0)
        w.base = s
        cells[FIELD_IDX] = _cell(o, "I", s)
    elif state == "wrongtype":
        cells[FIELD_IDX] = _cell(o, "F", 0)
    elif state == "missing":
        cells = cells[:FIELD_IDX]  # index out of range
    fields = _container(o, cells)
    k = o.alloc(0x20)
    o.write_u32(k + 0xC, fields)
    obj = o.alloc(0x30)
    o.write_u32(obj + 0x10, k)
    return obj, s


def _frame(w: World, sig: bytes, stack_cells: list) -> int:
    o = w.o
    stack = _container(o, stack_cells)
    sigp = _alloc_cstr(o, sig)
    callinfo = o.alloc(0x20)
    o.write_u32(callinfo + 4, sigp)
    method = o.alloc(0x40)
    o.write_u32(method + 0xC, _alloc_cstr(o, b"java.io.File"))
    o.write_u32(method + 0x10, _alloc_cstr(o, b"native"))
    o.write_u32(method + 0x14, callinfo)
    frame = o.alloc(0x40)
    o.write_u32(frame + 8, stack)
    o.write_u32(frame + 0xC, method)
    return frame


# ---------------------------------------------------------------- natives
NATIVES = {
    "write_int": (0x485B20, b"(I)V", "I"),
    "write_float": (0x485BC0, b"(F)V", "F"),
    "read_int": (0x485B70, b"()I", None),
    "read_float": (0x485C10, b"()F", None),
    "write_string": (0x485FD0, b"(Ljava.lang.String;)V", "L"),
    "read_string": (0x486060, b"()Ljava.lang.String;", None),
}


def run_native(name, case, exe=None):
    """case: {"state", "slot", "value"(bits, writes), "rv"(write layer
    return), "input"(hex, reads)}. Returns (result, events)."""
    addr, sig, ty = NATIVES[name]
    w = World(exe, wr_rv=case.get("rv", None),
              read_data=bytes.fromhex(case.get("input", "")))
    o = w.o
    obj, _ = _file_obj(w, case["state"], case.get("slot", 1))
    cells = []
    if ty == "L" and "string_hex" in case:
        # `0x485fd0` reads the string word as a buffer pointer directly
        raw = bytes.fromhex(case["string_hex"])
        buf = o.alloc(len(raw) + 1)
        o.write(buf, raw + b"\x00")
        cells.append(_cell(o, "L", buf))
    elif ty:
        cells.append(_cell(o, ty, case.get("value", 0)))
    cells.append(_cell(o, "L", obj))
    frame = _frame(w, sig, cells)
    res = o.call(addr, [frame], "cdecl", limit=2_000_000)
    if name.startswith("write"):
        result = {"eax": res.eax}
    elif name == "read_string":
        result = {"eax": res.eax}
    elif name == "read_int":
        defined = _read_defined(case)
        result = {"eax": res.eax} if defined else {"unspecified": True}
    else:
        defined = _read_defined(case)
        if case["state"] in ("missing", "wrongtype"):
            result = {"st0_f64": _f64bits(res.st0)}
        elif defined:
            result = {"st0_f64": _f64bits(res.st0)}
        else:
            result = {"unspecified": True}
    return result, w.events


def _read_defined(case):
    """readInt/readFloat return the (stale-stack) local when the handle is
    closed or the read is short; only a full read is defined. Missing /
    wrong-type field returns the constant 0 (int) / DAT_005e73cc (float)."""
    if case["state"] in ("missing", "wrongtype"):
        return True
    if case["state"] == "closed":
        return False
    return len(bytes.fromhex(case.get("input", ""))) >= 4


def _f64bits(x: float) -> str:
    return struct.pack("<d", x).hex()


# ---------------------------------------------------------------- slot run
def _build_nodes(o: Oracle, nodes: list) -> int:
    """Sentinel-terminated slot-node list; returns the head pointer."""
    addrs = [o.alloc(0x80) for _ in range(len(nodes) + 1)]
    for i, n in enumerate(nodes):
        a = addrs[i]
        o.write_u32(a + 4, addrs[i + 1])
        o.write_u32(a + 0xC, n["id"])
        o.write_u32(a + 0x10, n["kind"])
        obj = n.get("obj")
        if obj is not None:
            ob = o.alloc(0x80)
            o.write_u32(a + 0x20, ob)
            blob = obj.get("blob")
            if blob is not None:
                raw = bytes.fromhex(blob)
                bp = o.alloc(len(raw) + 16)
                o.write(bp, raw)
                o.write_u32(ob + 0x58, bp)
                o.write_u32(ob + 0x5C, len(raw))
    return addrs[0]


def _file_struct(w: World, c: dict) -> int:
    o = w.o
    s = o.alloc(0x200)
    w.base = s
    o.write_u32(s, c["slot"])
    o.write_u32(s + 8, c.get("res_count", 0))
    o.write_u32(s + 0x28, c.get("slot_count", 0))
    o.write_u32(s + 0x34, _build_nodes(o, c["nodes"]))
    o.write_u32(s + 0x48, c.get("mode", 1))
    o.write(s + 0x4C, c.get("name", "save\\car").encode() + b"\0")
    # [s+0xc] / [s+0x2c] are list objects whose head word ([+8]) is the
    # resource head ([s+0x14]) / slot head ([s+0x34]) written here; the
    # cleanup loops over them are hooked (0x429390 / 0x4293a0).
    # resource list
    res = c.get("res", [])
    addrs = [o.alloc(0x120) for _ in range(len(res) + 1)]
    for i, r in enumerate(res):
        a = addrs[i]
        o.write_u32(a + 4, addrs[i + 1])
        o.write_u32(a + 0xC, r["id"])
        o.write(a + 0x14, r["path"].encode() + b"\0")
    o.write_u32(s + 0x14, addrs[0])
    return s


def run_store(case, exe=None):
    """0x484fa0: returns (result, events)."""
    w = World(exe)
    s = _file_struct(w, case)
    res = w.o.call(0x484FA0, [s], "cdecl", limit=2_000_000)
    return {"eax": res.eax}, w.events


def run_close(case, exe=None):
    """0x485870 File.close(): returns (result, events)."""
    w = World(exe)
    o = w.o
    obj, _ = _file_obj(w, "open", case["slot"])
    # replace the plain struct by the full SDAT struct, same field cell
    s = _file_struct(w, case)
    fields = o.read_u32(o.read_u32(obj + 0x10) + 0xC)
    data = o.read_u32(fields + 4)
    cell = o.read_u32(data + 4 * FIELD_IDX)
    o.write_u32(cell + 8, s)
    o.write_u8(VERSION_BYTE, case["version"])
    frame = _frame(w, b"()V", [_cell(o, "L", obj)])
    o.call(0x485870, [frame], "cdecl", limit=2_000_000)
    # `File.close` is a void native: EAX is whatever the last call left.
    return {"unspecified": True}, w.events


# ---------------------------------------------------------------- cases
def _nodes_a():
    return [
        {"id": 1, "kind": 7, "obj": {"blob": b"sourcefile save\\ screenshot".hex()}},
        {"id": 2, "kind": 7, "obj": {"blob": b"sourcefile data/x.jpg \xff\xd8".hex()}},
        {"id": 3, "kind": 7, "obj": {"blob": None}},
        {"id": 4, "kind": 7},
        {"id": 5, "kind": 5},
        {"id": 6, "kind": 5, "obj": {"blob": b"sourcefile save/ x".hex()}},
        {"id": 7, "kind": 9},
        {"id": 8, "kind": 7, "obj": {"blob": b"sourcefile   save/zzz".hex()}},
        {"id": 9, "kind": 7, "obj": {"blob": b"sourcefile ab".hex()}},
        {"id": 10, "kind": 7, "obj": {"blob": b"sourcefile saves".hex()}},
        {"id": 11, "kind": 7, "obj": {"blob": b"sourcefile save\\extra long".hex()}},
        {"id": 12, "kind": 7, "obj": {"blob": b"sourcefile\t\nsave/".hex()}},
        {"id": 13, "kind": 7, "obj": {"blob": b"sourcefile  ab cd".hex()}},
        {"id": 0xFFFFFFFF, "kind": 7},
    ]


FLOATS = [0.0, -0.0, 1.0, -1.5, 3.14159274, 1e-45, 3.4028235e38,
          float("inf"), float("-inf"), 0.1]


def _fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def native_cases(name):
    out = []
    ints = [0, 1, 0xFFFFFFFF, 0x7FFFFFFF, 0x80000000, 0x12345678, 0xDEADBEEF]
    if name == "write_int":
        for v in ints:
            for rv in (4, 0xFFFFFFFF):
                out.append({"state": "open", "slot": 3, "value": v, "rv": rv})
        out.append({"state": "open", "slot": 31, "value": 7})
        out += [{"state": s, "value": 5} for s in
                ("closed", "missing", "wrongtype")]
    elif name == "write_float":
        for f in FLOATS:
            out.append({"state": "open", "slot": 2, "value": _fbits(f)})
        out.append({"state": "open", "slot": 2, "value": 0x7FC00000})
        out.append({"state": "open", "slot": 2, "value": 0x7FA00000,
                    "rv": 0})
        out += [{"state": s, "value": _fbits(1.5)} for s in
                ("closed", "missing", "wrongtype")]
    elif name in ("write_string", "read_string"):
        # the string word is a pointer to a NUL-terminated buffer
        for text in (b"", b"a", b"hello", b"x" * 32):
            out.append(
                {
                    "state": "open",
                    "slot": 3,
                    "string_hex": text.hex(),
                    "input": (len(text) + 1).to_bytes(4, "little").hex() + text.hex() + "00",
                }
            )
        out += [{"state": s, "string_hex": b"hi".hex()} for s in
                ("closed", "missing", "wrongtype")]
        out.append({"state": "open", "slot": 3, "string_hex": b"hi".hex()})
        # a zero length word answers 0 without reading bytes
        out.append({"state": "open", "slot": 3, "input": "00000000"})
    elif name == "read_int":
        for v in ints:
            out.append({"state": "open", "slot": 4,
                        "input": struct.pack("<I", v).hex()})
        out.append({"state": "open", "slot": 4,
                    "input": struct.pack("<II", 1, 2).hex()})
        out += [{"state": s, "input": "01000000"} for s in
                ("closed", "missing", "wrongtype")]
        out.append({"state": "open", "slot": 4, "input": "aabb"})
        out.append({"state": "open", "slot": 4, "input": ""})
    elif name == "read_float":
        for f in FLOATS:
            out.append({"state": "open", "slot": 6,
                        "input": struct.pack("<f", f).hex()})
        out.append({"state": "open", "slot": 6, "input": "0000c07f"})
        out += [{"state": s, "input": "0000803f"} for s in
                ("closed", "missing", "wrongtype")]
        out.append({"state": "open", "slot": 6, "input": ""})
    return out


def store_cases():
    nodes = _nodes_a()
    cases = [
        {"slot": 2, "name": "save\\ROC_X_1", "nodes": []},
        {"slot": 2, "name": "save\\ROC_X_1", "nodes": nodes[:1]},
        {"slot": 7, "name": "cars\\a b", "nodes": nodes},
    ]
    # the stock shape: 13 screenshot slots, all written as companions
    cases.append({"slot": 9, "name": "save\\ROC_GT2_1",
                  "nodes": [{"id": i, "kind": 7,
                             "obj": {"blob": b"sourcefile save\\".hex()}}
                            for i in range(1, 14)]})
    return cases


def close_cases():
    base_nodes = [
        {"id": 1, "kind": 7, "obj": {"blob": b"sourcefile save\\".hex()}},
        {"id": 2, "kind": 7, "obj": {"blob": b"sourcefile x.jpg".hex()}},
        {"id": 3, "kind": 5},
    ]
    res = [{"id": 1, "path": "cars\\racers\\Baiern.rpk"},
           {"id": 2, "path": "parts\\wheels.rpk"},
           {"id": 3, "path": "p"}]
    full = {"slot": 5, "name": "save\\ROC_X_1", "slot_count": 3,
            "res_count": 3, "nodes": base_nodes, "res": res}
    cases = []
    for version in (1, 2, 3):
        for mode in (0, 1, 2):
            cases.append({**full, "version": version, "mode": mode})
    cases.append({**full, "version": 2, "mode": 1, "slot": 0})
    cases.append({**full, "version": 2, "mode": 1, "nodes": [], "res": [],
                  "slot_count": 0, "res_count": 0})
    cases.append({**full, "version": 1, "mode": 1, "nodes": [], "res": [],
                  "slot_count": 0, "res_count": 0})
    cases.append({**full, "version": 2, "mode": 1, "res": res[:1],
                  "res_count": 1, "nodes": nodes_a_small()})
    # counts that disagree with the lists: the exe writes the stored words
    cases.append({**full, "version": 2, "mode": 1, "slot_count": 77,
                  "res_count": 99})
    return cases


def nodes_a_small():
    return _nodes_a()[:6]


SCENARIOS = (
    [(n, NATIVES[n][0], "cdecl", None, native_cases(n)) for n in NATIVES]
    + [("store_slots", 0x484FA0, "cdecl", run_store, store_cases()),
       ("close", 0x485870, "cdecl", run_close, close_cases())]
)


def _run(name, case, exe):
    if name in NATIVES:
        return run_native(name, case, exe)
    return {"store_slots": run_store, "close": run_close}[name](case, exe)


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for name, addr, conv, _fn, cases in SCENARIOS:
        rows = []
        for i, case in enumerate(cases):
            result, events = _run(name, case, exe)
            rows.append({
                "id": i, "exe_addr": f"0x{addr:08x}", "name": name,
                "convention": conv, "args": case, "result": result,
                "events": events, "oracle": "unicorn", "unicorn": UC_VER,
                "cw": f"0x{CW:04x}", "date": _today()})
        path = os.path.join(OUT_DIR, f"sdat_{name}.jsonl")
        with open(path, "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        total += len(rows)
        print(f"{name}: {len(rows)} rows -> {path}")
    print(f"total {total} rows, {len(SCENARIOS)} functions")


def verify(files, exe=None):
    bad = n = 0
    for path in files:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        for row in rows:
            n += 1
            result, events = _run(row["name"], row["args"], exe)
            if result != row["result"] or events != row["events"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['name']}:\n"
                      f"  want {(row['result'], row['events'])}\n"
                      f"  got  {(result, events)}")
        print(f"{path}: ok")
    print(f"verify: {n - bad}/{n} byte-identical")
    return 1 if bad else 0


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(
        description=__doc__,
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
