#!/usr/bin/env python3
"""Scripted save/config sessions through the original `File` natives (G9 / S8).

`sdat_vectors.py` / `io_vectors.py` call one native on a fresh, hand-built
struct. This driver instead runs a whole *session* in one persistent Unicorn
world: the exe's real `File.open` allocates the native file struct and writes
the SDAT header, the real `write(int/float/String/ResourceRef)` natives append
the body, the real `File.close` writes the version-2 footer, and the real
`readInt` / `readFloat` / `readString` / `readResID` natives load the bytes
back (the footer scan in `File.open` rebuilds the resource table and the slot
run, `0x485190`). The sessions are the native-level equivalent of what
`Config.saveConfig`, `ControlSet.save` and `Vehicle.save` do (docs/formats.md
SDAT): the scripts issue exactly these native calls in this order. Driving the
scripts themselves needs the VM boot and is blocked (see docs/saves-e2e.md).

Boundaries replaced by Python (everything else is the real exe code):

- the file layer `0x54c880` / `0x54caf0` / `0x54cec0` / `0x54cf00` /
  `0x54ce40` / `0x54cca0`: served from an in-memory file system, so the bytes
  the natives hand the layer *are* the file (the layer's own path conversion
  and CRT slot table are covered by `filelayer_vectors.py`);
- object field plumbing `0x42a690` / `0x42ab50` / `0x42a9e0`: a per-object
  `name` string and a stateful handle field;
- initial resource-manager package names are declared by the session's dense
  `kinds` table and marked already loaded. Registration `0x538380`, descriptor
  lookup `0x5383f0` and the loaded-header fast path execute the real code;
- the companion/texture loaders `0x539080` (JPEG companion by name) and
  `0x539170` (inline blob), which return a fresh zeroed object and log the
  request; `0x419860` (never reached without a slot object);
- the allocator, the interner `0x4174a0` and the log sink.

CLI:

  save_session.py run --spec save_sessions.json --out DIR [--seed-from DIR]
                      [--stock DIR] [--exe EXE] [--only ID]

Writes `DIR/results.json` (schema below) and `DIR/files/<session>/<name>` for
every file a session wrote. A session may seed files: `{"from": "session",
"session": S, "name": N}` reads `<seed-from>/files/S/N` (the cross-load inputs;
default seed dir is `DIR` itself), `{"from": "stock", "path": P}` reads
`<stock>/P`, `{"from": "hex", "hex": H}` is inline.

results.json: {"schema": 1, "implementation": "original", "exe_sha256",
"sessions": {id: {"ops": [...], "files": {name: sha256}}}}.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import io_vectors as iov  # noqa: E402
from io_vectors import _alloc_cstr, _cell, _container, _frame  # noqa: E402

REPO = iov.REPO
SCHEMA = 1

NATIVES = {
    "open": (0x485440, b"(I)I"),
    "close": (0x485870, b"()V"),
    "write_int": (0x485B20, b"(I)I"),
    "write_float": (0x485BC0, b"(F)I"),
    "write_string": (0x485FD0, b"(Ljava.lang.String;)I"),
    "write_res_ref": (0x485C60, iov.WRITE_RES_REF_SIG),
    "read_int": (0x485B70, b"()I"),
    "read_float": (0x485C10, b"()F"),
    "read_string": (0x486060, b"()Ljava.lang.String;"),
    "read_res_id": (0x485EA0, iov.READ_RES_ID_SIG),
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ext80(raw: bytes) -> str:
    """ST0's 10 raw bytes (mantissa, then sign+exponent) as `sexp:mantissa` hex,
    so NaN payloads and signs are compared exactly."""
    mant = int.from_bytes(raw[:8], "little")
    se = int.from_bytes(raw[8:10], "little")
    return f"{se:04x}{mant:016x}"


class Vfs:
    """The file layer's file system: named byte buffers behind slot numbers."""

    def __init__(self, files=None):
        self.files = {k: bytearray(v) for k, v in (files or {}).items()}
        self.slots = {}
        self.next_slot = 0

    def open(self, name, mode):
        if mode == "read":
            if name not in self.files:
                return 0
        else:
            self.files[name] = bytearray()
        self.next_slot += 1
        self.slots[self.next_slot] = {"name": name, "pos": 0}
        return self.next_slot

    def buf(self, slot):
        return self.files[self.slots[slot]["name"]]


class SessionWorld(iov.World):
    """`io_vectors.World` plus: a real file system, per-object fields, the
    resource-manager path lookup and the slot-run loaders."""

    def __init__(self, exe, vfs: Vfs, kinds: dict):
        super().__init__(exe)
        o = self.o
        self.vfs = vfs
        self.objects = {}        # object address -> {"name_obj", "handle"}
        self.name_objs = {}      # name string address -> object address
        self.interned = []       # interned byte strings; word = index + 0x1000
        self.loads = []          # companion / blob load requests, in order
        self.logs = []
        # Allocate the manager's inline descriptor table for all 16-bit package
        # indices. The profile supplies already-loaded initial packages; paths
        # encountered in SDAT footers are registered by the original itself.
        self.manager = o.alloc(0xFFE94 + 0x10000 * 0x70)
        o.write_u32(0x618D48, self.manager)
        del o.hooks[0x5383F0]
        for expected, path in sorted((int(kind), path) for kind, path in kinds.items()):
            result = o.call(0x538380, [_alloc_cstr(o, path.encode())],
                            convention="thiscall", this=self.manager)
            if result.eax != expected:
                raise ValueError("initial kinds must declare a dense, case-insensitively unique manager table")
            o.write_u32(self.manager + 0xFFE94 + expected * 0x70 + 0x50, 2)
        # the lists are built by the real `0x429350` and the slot run by the
        # real `0x485190`
        del o.hooks[0x429350]
        del o.hooks[0x485190]
        o.hook(0x54C880, lambda oc, a: self._open("read", a), "cdecl", 1)
        o.hook(0x54CAF0, lambda oc, a: self._open("write", a), "cdecl", 1)
        o.hook(0x54CEC0, self._read, "cdecl", 3)
        o.hook(0x54CF00, self._write, "cdecl", 3)
        o.hook(0x54CE40, self._seek, "cdecl", 3)
        o.hook(0x54CCA0, lambda oc, a: 0, "cdecl", 1)
        o.hook(0x4174A0, self._intern, "thiscall", 1)
        o.hook(0x539080, self._load_companion, "cdecl", 2)
        o.hook(0x539170, self._load_blob, "cdecl", 3)
        o.hook(0x419860, lambda oc, a: None, "thiscall", 4)
        # the companion savers `0x48daa0` / `0x48da30` run for real: a node
        # `write(ResourceRef)` appended has no object, so they return at once
        o.hook(0x42A690, self._getfield, "cdecl", 3)
        o.hook(0x42AB50, self._getint, "cdecl", 2)
        o.hook(0x42A9E0, self._setfield2, "cdecl", 3)
        o.hook(0x551140, self._append, "cdecl", 3)

    # -- object plumbing
    def new_file_object(self, name: str) -> int:
        o = self.o
        obj = o.alloc(0x30)
        name_obj = _alloc_cstr(o, name.encode())
        self.objects[obj] = {"name_obj": name_obj, "handle": 0}
        self.name_objs[name_obj] = obj
        return obj

    def _getfield(self, oc, a):
        obj = a[0]
        return self.objects[obj]["name_obj"]

    def _getint(self, oc, a):
        obj, _idx = a
        if obj in self.name_objs:
            return obj  # the `name` String's char buffer is the cstring itself
        return self.objects[obj]["handle"]

    def _setfield2(self, oc, a):
        obj, _idx, value = a
        self.objects[obj]["handle"] = value

    def _append(self, oc, a):
        self.logs.append(oc.read_cstr(a[1]).decode())

    # -- file layer
    def _open(self, kind, a):
        return self.vfs.open(self.o.read_cstr(a[0]).decode(), kind)

    def _read(self, oc, a):
        slot, buf, n = a
        ent = self.vfs.slots.get(slot)
        if ent is None:
            return 0xFFFFFFFF
        data = self.vfs.buf(slot)
        got = bytes(data[ent["pos"]:ent["pos"] + n])
        oc.write(buf, got)
        ent["pos"] += len(got)
        return len(got)

    def _write(self, oc, a):
        slot, buf, n = a
        ent = self.vfs.slots.get(slot)
        if ent is None:
            return 0xFFFFFFFF
        data = self.vfs.buf(slot)
        chunk = oc.read(buf, n)
        pos = ent["pos"]
        data[pos:pos + n] = chunk
        ent["pos"] = pos + n
        return n

    def _seek(self, oc, a):
        slot, whence, offset = a
        ent = self.vfs.slots.get(slot)
        if ent is None:
            return 0xFFFFFFFF
        off = offset if offset < 0x8000_0000 else offset - 0x1_0000_0000
        size = len(self.vfs.buf(slot))
        base = {0: 0, 1: size, 2: ent["pos"]}.get(whence)
        if base is None:
            return 0xFFFFFFFF
        prev = ent["pos"]
        ent["pos"] = (base + off) & 0xFFFFFFFF
        return prev

    # -- manager / loaders
    def _intern(self, oc, a):
        self.interned.append(oc.read_cstr(a[0]))
        return 0x1000 + len(self.interned) - 1

    def _fake_object(self):
        return self.o.alloc(0x100)

    def _load_companion(self, oc, a):
        self.loads.append(["load_companion", oc.read_cstr(a[1]).decode()])
        return self._fake_object()

    def _load_blob(self, oc, a):
        _desc, buf, n = a
        # the exe's buffer past the sprintf'd text is uninitialised heap (the
        # size-0 arm), so only the text up to the first NUL is observable
        self.loads.append(["load_blob", n, oc.read(buf, n).split(b"\0")[0].hex()])
        return self._fake_object()


class Session:
    def __init__(self, exe, seeds: dict, kinds: dict):
        self.vfs = Vfs(seeds)
        self.w = SessionWorld(exe, self.vfs, kinds)
        self.files = {}   # session file label -> object address

    def obj(self, label, name=None):
        if label not in self.files:
            self.files[label] = self.w.new_file_object(name or label)
        return self.files[label]

    def tables(self, obj):
        w = self.w
        handle = w.objects[obj]["handle"]
        if not handle:
            return None
        w.base = handle
        state = w.tables_state()
        return {
            "resource_count": state["resource_count"],
            "slot_count": state["slot_count"],
            "resources": state["resources"],
            "slots": state["slots"],
        }

    def run_op(self, op):
        w, o = self.w, self.w.o
        kind = op["op"]
        if kind == "open":
            # a new File object per open; the `name` field is what open re-reads
            self.files.pop(op["file"], None)
            obj = self.obj(op["file"], op["name"])
        else:
            obj = self.files.get(op["file"])
            if obj is None:
                obj = self.obj(op["file"], op.get("name", op["file"]))
        w.loads.clear()
        w.logs.clear()
        addr, sig = NATIVES[kind]
        cells = []
        if kind == "open":
            cells.append(_cell(o, "I", op["mode"]))
        elif kind == "write_int":
            cells.append(_cell(o, "I", op["value"]))
        elif kind == "write_float":
            cells.append(_cell(o, "F", op["bits"]))
        elif kind == "write_string":
            raw = bytes.fromhex(op["hex"])
            buf = o.alloc(len(raw) + 1)
            o.write(buf, raw + b"\0")
            cells.append(_cell(o, "L", buf))
        elif kind == "write_res_ref":
            ref = op["ref"]
            if ref is None:
                cells.append(_cell(o, "L", 0))
            else:
                cells.append(_cell(o, "L", iov._res_ref(o, ref["id"], ref["kind"])))
        cells.append(_cell(o, "L", obj))
        frame = _frame(w, sig, cells)
        base_events = len(w.events)
        res = o.call(addr, [frame], "cdecl", limit=5_000_000)
        out = {"op": kind, "file": op["file"]}
        if kind in ("open", "write_int", "write_float", "write_string", "write_res_ref",
                    "read_int", "read_res_id"):
            out["result"] = res.eax
        elif kind == "read_float":
            out["result"] = ext80(res.st0_raw)
        elif kind == "read_string":
            word = res.eax
            out["result"] = (
                None if word == 0 else w.interned[word - 0x1000].hex()
            )
        if kind == "open":
            handle = w.objects[obj]["handle"]
            out["handle"] = bool(handle)
            if handle:
                st = w.struct_state(handle)
                out["struct"] = {k: st[k] for k in ("mode", "name", "open_word")}
                out["tables"] = self.tables(obj)
        out["loads"] = list(w.loads)
        out["log"] = list(w.logs)
        del w.events[base_events:]
        return out


def load_seed(seed: dict, args, results_dir):
    src = seed["from"]
    if src == "hex":
        return bytes.fromhex(seed["hex"])
    if src == "stock":
        with open(os.path.join(args.stock, seed["path"]), "rb") as f:
            return f.read()
    if src == "session":
        base = args.seed_from or results_dir
        with open(os.path.join(base, "files", seed["session"], seed["name"]), "rb") as f:
            return f.read()
    raise SystemExit(f"unknown seed source {src!r}")


def run_session(spec, sess_spec, args, results_dir, exe):
    seeds = {
        name: load_seed(seed, args, results_dir)
        for name, seed in sess_spec.get("seed", {}).items()
    }
    sess = Session(exe, seeds, spec["kinds"])
    ops = []
    for i, op in enumerate(sess_spec["ops"]):
        try:
            ops.append(sess.run_op(op))
        except Exception as error:
            raise SystemExit(f"session {sess_spec['id']} op {i} {op}: {type(error).__name__}: {error}")
    files = {name: bytes(data) for name, data in sess.vfs.files.items()}
    return ops, files


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--spec", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--seed-from")
    r.add_argument("--stock", default=os.path.join(REPO, "stock"))
    r.add_argument("--exe")
    r.add_argument("--only")
    args = ap.parse_args(argv)
    spec = json.load(open(args.spec))
    exe = args.exe or iov._default_exe()
    with open(exe, "rb") as f:
        exe_hash = sha256(f.read())
    results = {"schema": SCHEMA, "implementation": "original", "exe_sha256": exe_hash,
               "spec_sha256": sha256(open(args.spec, "rb").read()), "sessions": {}}
    os.makedirs(args.out, exist_ok=True)
    for sess in spec["sessions"]:
        if args.only and sess["id"] != args.only:
            continue
        ops, files = run_session(spec, sess, args, args.out, exe)
        results["sessions"][sess["id"]] = {
            "ops": ops,
            "files": {name: sha256(data) for name, data in sorted(files.items())},
        }
        for name, data in files.items():
            if name in sess.get("seed", {}):
                continue
            dest = os.path.join(args.out, "files", sess["id"], name)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "wb") as f:
                f.write(data)
    with open(os.path.join(args.out, "results.json"), "w") as f:
        json.dump(results, f, indent=1, sort_keys=True)
        f.write("\n")
    print(f"{len(results['sessions'])} sessions -> {args.out}/results.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
