#!/usr/bin/env python3
"""Unicorn vectors for the render/animation/particle/painter natives left out
of `res_vectors.py` (M1-11 follow-up: the RenderRef/Animation/ParticleSystem
rows the headless driver captured only, plus the three Painter rows).

Groups (JSONL under re/out/res/, git-ignored, `SLR_RES_VECTORS` override):

  rset   RenderRef: changeResource setColor setLight setType setFlare
         getDetail getTypeID lineCreate lineAdd getBoneId setMatrix(I,V3,Ypr)
         setMatrix(V3,Ypr) plotRoute
  anim2  Animation: <init> finalize getPos
  psys2  ParticleSystem: init stop setDirectSource setSource
  paint  Painter: paintPart doPaint xPaint

The harness is `res_vectors.World` (same argument-frame popper, same
0x42ab50 field-read hook). This file adds the render-envelope object the
RenderRef natives walk (`env: +8 obj word, +0xc ref`), hooks the engine
tails (0x48c8c0/0x48c9d0/0x48c970/0x48cb40/0x48bf50/0x48a670/0x48a860/
0x48bc40/0x48a490/0x48a610/0x53fe70/0x5414xx/0x48d450/0x426800) and zeroes
the route-engine global 0x6408d0 so plotRoute exercises the exe's own
no-engine arm.

Row shape: {"id","exe_addr","name","convention","sig","args","result",
"events","oracle","unicorn","cw","date"} — the compared facts are the
native's return word and the ordered engine-call events, exactly the words
the port's EngineSink records.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402
import unicorn  # noqa: E402

import res_vectors as rv  # noqa: E402

REPO = rv.REPO
OUT_DIR = os.path.join(REPO, "re", "out", "res")
CW = CW_D3D9
UC_VER = unicorn.__version__

FIELD_IDX = rv.FIELD_IDX

VT_ENVL = 0x5F37F4      # the render-instance vtable the exe uses
VT_PART_NODE = 0x5F13A0  # counter-node vtable (res_vectors reuses it)
VT_SRC_NODE = 0x5F1128   # source-node vtable written by the source setters


def _today() -> str:
    return datetime.date.today().isoformat()


class RenderWorld(rv.World):
    """Adds the RenderRef envelope object, the painter collaborators and the
    engine-tail recorder to the shared res_vectors harness."""

    def __init__(self, exe=None, exists: int = 1, mode: int = 1,
                 obj8: int = 1, fetch_null: int = 0):
        super().__init__(exe, exists=exists, mode=mode)
        self.events = []
        self.ret = 0
        self.fetch_null = fetch_null
        self.store_calls = []
        o = self.o
        # render envelope: [env+8] obj word, [env+0xc] ref (mode/detail).
        env = o.alloc(0x80)
        o.write_u32(env, VT_ENVL)
        o.write_u32(env + 8, obj8)
        o.write_u32(env + 0xC, self.ref)
        self.renv = env
        # vtable slots the Animation/Painter paths call through.
        vt = o.alloc(0x20)
        o.write_u32(vt + 0x00, self.stub)   # finalize: vtable[0](1)
        o.write_u32(vt + 0x0C, self.stub)   # resolve
        o.write_u32(vt + 0x14, self.stub)   # materialise
        o.write_u32(env, vt)
        o.write_u32(self.ref, vt)
        # engine tails: record and answer the no-device words.
        o.hook(0x48C8C0, self._ev("0x48c8c0"), "thiscall", 1)
        o.hook(0x48C9D0, self._ev("0x48c9d0"), "thiscall", 4)
        o.hook(0x48C970, self._ev("0x48c970"), "thiscall", 1)
        o.hook(0x48CB40, self._ev("0x48cb40"), "thiscall", 1)
        o.hook(0x48BF50, self._ev("0x48bf50"), "cdecl", 16)
        o.hook(0x48A670, self._h_line_create, "thiscall", 5)
        o.hook(0x48A860, self._ev("0x48a860"), "thiscall", 4)
        o.hook(0x48BC40, self._h_bone, "thiscall", 1)
        o.hook(0x48A490, self._ev("0x48a490"), "thiscall", 5)
        o.hook(0x48A610, self._ev("0x48a610"), "thiscall", 0)
        o.hook(0x53FE70, self._h_anim_sample, "thiscall", 0)
        o.hook(0x541400, self._h_541400, "thiscall", 0)
        o.hook(0x540440, self._ev("0x540440"), "thiscall", 0)
        o.hook(0x42A9E0, self._h_store, "cdecl", 3)
        # plotRoute: the route engine global stays 0 (fresh heap), pinning
        # the exe's own no-engine arm.
        o.write_u32(0x6408D0, 0)
        # getTypeID's fetch honours fetch_null through the shared envelope.
        self.fetch_envelope = 0 if fetch_null else self.envelope
        # painter collaborators.
        o.hook(0x48D450, self._h_paint_query, "cdecl", 10)
        o.hook(0x426800, self._h_make_object, "thiscall", 4)
        o.hook(0x45FB80, self._ev("0x45fb80"), "thiscall", 0)

    # ------------------------------------------------------------ helpers
    def _ev(self, fn):
        def hook(oc: Oracle, args: list):
            self.events.append({"fn": fn, "this": oc.reg("ecx"), "args": list(args)})
            return 0
        return hook

    def _h_line_create(self, oc: Oracle, args: list) -> int:
        self.events.append({"fn": "0x48a670", "this": oc.reg("ecx"), "args": list(args)})
        return 0  # the engine miss: lineCreate/plotRoute answer 0

    def _h_bone(self, oc: Oracle, args: list) -> int:
        self.events.append({"fn": "0x48bc40", "this": oc.reg("ecx"), "args": list(args)})
        return 0x2A  # a live bone id word

    def _h_anim_sample(self, oc: Oracle, args: list) -> float:
        self.events.append({"fn": "0x53fe70", "this": oc.reg("ecx"), "args": []})
        return 0.25

    def _h_541400(self, oc: Oracle, args: list) -> int:
        self.events.append({"fn": "0x541400", "this": oc.reg("ecx"), "args": []})
        return self.envelope

    def _h_store(self, oc: Oracle, args: list) -> None:
        """0x42a9e0(obj, field, value): the reference-field store."""
        self.store_calls.append({"obj": args[0], "field": args[1], "value": args[2]})

    def _h_envelope_fetch(self, oc: Oracle, args: list) -> int:
        self.fetch_calls += 1
        return self.fetch_envelope

    def _h_paint_query(self, oc: Oracle, args: list) -> int:
        self.events.append({"fn": "0x48d450", "this": 0, "args": list(args)})
        return self.paint_query

    def _h_make_object(self, oc: Oracle, args: list) -> int:
        self.events.append({"fn": "0x426800", "this": oc.reg("ecx"), "args": list(args)})
        return self.o.alloc(0x40)


def _cell(w: int, ty: str, payload: int) -> int:
    return rv._cell(w.o, ty, payload)


def _frame(w, sig: bytes, cells: list, cls: bytes) -> int:
    return rv._frame(w.o, sig, cells, cls)


def _run(w: RenderWorld, addr: int, frame: int) -> None:
    """Call the native at `addr` with the popped argument frame."""
    w.events = []
    w.store_calls = []
    w.ret = 0
    w.o.hook(0x419860, w._h_envelope_fetch, "stdcall", 4)
    # cdecl: the frame pointer is the only stack argument.
    w.o.call(addr, "cdecl", [frame])


def _row(w: RenderWorld, name: str, addr: int, sig: str, args: dict,
         cls: bytes) -> dict:
    return {
        "id": 0,  # assigned by the caller
        "exe_addr": f"0x{addr:08x}",
        "name": name,
        "convention": "cdecl",
        "sig": sig,
        "args": args,
        "result": {"ret": w.ret},
        "events": list(w.events),
        "stores": list(w.store_calls),
        "oracle": "unicorn",
        "unicorn": UC_VER,
        "cw": f"0x{CW:04x}",
        "date": _today(),
    }


def _write(rows: list, stem: str) -> str:
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, stem)
    with open(path, "x", encoding="utf-8") as out:
        for row in rows:
            out.write(json.dumps(row) + "\n")
    return path


# ------------------------------------------------------------------ rset

def _render_obj(w: RenderWorld, ptr: int) -> int:
    """A fresh `this` whose 0x42ab50 read answers the render envelope."""
    o = w.o
    obj = o.alloc(0x10)
    o.write_u32(obj + 8, ptr)
    return obj


RSET_PLAN = [
    # (name, addr, sig, scenarios)
    ("changeResource", 0x480220, b"(Ljava.util.resource.ResourceRef;Ljava.util.resource.ResourceRef;)V", [
        {"env": 0},
        {"args": [0x1111, 0x2222]},
        {"args": [0x1111, 0x2222], "mode": 1},
        {"args": [0x1111, 0]},          # null new
        {"args": [0, 0x2222]},          # null old
    ]),
    ("setColor", 0x480310, b"(I)V", [
        {"env": 0},
        {"args": [0]},
        {"args": [0x00ff00ff]},
        {"args": [0xffffffff]},
    ]),
    ("setLight", 0x486ab0, b"(III)V", [
        {"env": 0},
        {"args": [0, 0, 0]},
        {"args": [1, 2, 3]},
        {"args": [255, 128, 64]},
    ]),
    ("setType", 0x486ba0, b"(Ljava.util.resource.ResourceRef;)V", [
        {"env": 0},
        {"args": [0]},
        {"args": [0x3333]},
    ]),
    ("setFlare", 0x486b20, b"(Ljava.util.resource.ResourceRef;IFFII)V", [
        {"args": [0x4444, 1, 0x3f800000, 2, 3]},
        {"args": [0, 0, 0, 0, 0]},
    ]),
    ("getDetail", 0x480440, b"()F", [
        {"env": 0},
        {"mode": 0},
        {"mode": 1},
        {"mode": 2},
        {"mode": 3},
        {"mode": 1, "obj8": 0},
        {"mode": 3, "obj8": 0},
        {"mode": 1, "detail": 0x3f000000},
        {"mode": 3, "detail": 0x40000000},
    ]),
    ("getTypeID", 0x486be0, b"()I", [
        {"env": 0},
        {"mode": 2},
        {"mode": 3},
        {"mode": 3, "obj8": 0},
        {"mode": 3, "fetch_null": 1},
        {"mode": 3, "typeid": 0x1234},
    ]),
    ("lineCreate", 0x47fe70, b"(Ljava.util.resource.ResourceRef;Ljava.util.resource.ResourceRef;)I", [
        {"env": 0},
        {"args": [0, 0]},
        {"args": [0x5555, 0x6666]},
        {"args": [0x5555, 0]},
    ]),
    ("lineAdd", 0x47fee0, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;IF)V", [
        {"env": 0},
        {"args": [0, 0, 1, 0x3f800000]},
        {"args": [0x7777, 0x8888, 2, 0x40000000],
         "v0": {"x": 1.0, "y": -2.0, "z": 0.5}, "v1": {"x": 4.0, "y": 0.0, "z": -0.25}},
    ]),
    ("getBoneId", 0x481020, b"(Ljava.lang.String;)I", [
        {"env": 0},
        {"args": [0]},
        {"args": [0x9999]},
    ]),
    ("setMatrix", 0x4810b0, b"(ILjava.util.resource.ResourceRef;Ljava.lang.Vector3;Ljava.lang.Ypr;)V", [
        {"env": 0},
        {"args": [2, 0xaaaa, 0xbbbb, 0xcccc],
         "pos": {"x": 1.5, "y": 2.5, "z": 3.5}, "ypr": {"y": 0.1, "p": 0.2, "r": 0.3}},
        {"args": [0, 0, 0, 0]},
    ]),
    ("setMatrix", 0x481240, b"(Ljava.lang.Vector3;Ljava.lang.Ypr;)V", [
        {"env": 0},
        {"args": [0xdddd, 0xeeee],
         "pos": {"x": -1.0, "y": 0.0, "z": 1.0}, "ypr": {"y": 3.0, "p": 0.0, "r": -3.0}},
        {"args": [0, 0]},
    ]),
    ("plotRoute", 0x483960, b"(Ljava.util.resource.ResourceRef;Ljava.util.resource.ResourceRef;IFLjava.lang.Vector3;)I", [
        {"args": [0x1111, 0x2222, 3, 0x3f800000, 0xffff]},
    ]),
]


def gen_rset(exe=None) -> str:
    rows = []
    rid = 0
    for name, addr, sig, scenarios in RSET_PLAN:
        for scene in scenarios:
            w = RenderWorld(
                exe,
                mode=scene.get("mode", 1),
                obj8=scene.get("obj8", 1),
                fetch_null=scene.get("fetch_null", 0),
            )
            w.components.update(scene.get("v0", {}))
            w.obj_components[w.renv] = dict(scene.get("v1", {}))
            w.obj_components[w.ref] = {}
            if "detail" in scene:
                w.o.write_u32(w.ref + 0x6C, scene["detail"])
            if "typeid" in scene:
                w.o.write_u32(w.envelope + 0x80, scene["typeid"])
            cells = []
            this = _render_obj(w, 0x2000)
            cells.append(_cell(w, "L", this))
            for i, value in enumerate(scene.get("args", [])):
                ty = "I"
                if name == "lineAdd" and i == 3:
                    ty = "F"
                cells.append(_cell(w, ty, value))
            frame = _frame(w, sig, cells, b"java.util.resource.RenderRef")
            w.obj = w.renv
            try:
                _run(w, addr, frame)
            except Exception as error:  # noqa: BLE001 - recorded, not raised
                row = _row(w, name, addr, sig, scene, b"java.util.resource.RenderRef")
                row["error"] = repr(error)
                row["id"] = rid
                rows.append(row)
                rid += 1
                continue
            row = _row(w, name, addr, sig, scene, b"java.util.resource.RenderRef")
            row["id"] = rid
            rows.append(row)
            rid += 1
    return _write(rows, "res_rset_render.jsonl")


# ------------------------------------------------------------------ anim2

def gen_anim2(exe=None) -> str:
    rows = []
    rid = 0
    sig = b"(Ljava.util.resource.RenderRef;Ljava.util.resource.ResourceRef;)"
    for scene in [
        {"exists": 0},                       # no live handle: setters return
        {"args": [0, 0]},                    # null render/res
        {"args": [0x1111, 0x2222]},          # the create path
    ]:
        w = RenderWorld(exe, exists=scene.get("exists", 1))
        cells = [_cell(w, "L", 0x4000), _cell(w, "I", 0)]  # placeholder, reset below
        cells = []
        cells.append(_cell(w, "L", scene["args"][0] if "args" in scene else 0))
        cells.append(_cell(w, "L", scene["args"][1] if "args" in scene else 0))
        this = _render_obj(w, 0)
        frame = _frame(w, sig, cells, b"java.util.resource.Animation")
        w.obj = this
        try:
            _run(w, 0x47EA10, frame)
        except Exception as error:  # noqa: BLE001
            row = _row(w, "init", 0x47EA10, sig.decode(), scene, b"java.util.resource.Animation")
            row["error"] = repr(error)
            row["id"] = rid
            rows.append(row)
            rid += 1
            continue
        row = _row(w, "init", 0x47EA10, sig.decode(), scene, b"java.util.resource.Animation")
        row["id"] = rid
        rows.append(row)
        rid += 1
    # finalize / getPos over a planted envelope value in this.obj.
    for name, addr, sig_s, scene in [
        ("finalize", 0x47EBA0, "()V", {"exists": 0}),
        ("finalize", 0x47EBA0, "()V", {}),
        ("getPos", 0x47F050, "()F", {"exists": 0}),
        ("getPos", 0x47F050, "()F", {}),
        ("getPos", 0x47F050, "()F", {"obj8": 0}),
    ]:
        w = RenderWorld(exe, exists=scene.get("exists", 1), obj8=scene.get("obj8", 1))
        cells = []
        this = _render_obj(w, 0)
        # this.obj (field 3 of the Animation) -> the render envelope.
        w.o.write_u32(this + 0xC, w.renv)
        frame = _frame(w, sig_s.encode(), cells, b"java.util.resource.Animation")
        w.obj = this
        try:
            _run(w, addr, frame)
        except Exception as error:  # noqa: BLE001
            row = _row(w, name, addr, sig_s, scene, b"java.util.resource.Animation")
            row["error"] = repr(error)
            row["id"] = rid
            rows.append(row)
            rid += 1
            continue
        row = _row(w, name, addr, sig_s, scene, b"java.util.resource.Animation")
        row["id"] = rid
        rows.append(row)
        rid += 1
    return _write(rows, "res_anim2_lifecycle.jsonl")


# ------------------------------------------------------------------ psys2

def gen_psys2(exe=None) -> str:
    rows = []
    rid = 0
    init_sig = b"(Ljava.util.resource.ResourceRef;Ljava.util.resource.RenderRef;Ljava.lang.String;)V"
    for scene in [
        {"exists": 0},
        {"args": [0, 0x1111, 0x3333]},
        {"args": [0x1111, 0, 0x3333]},
        {"args": [0x1111, 0x2222, 0x3333]},
    ]:
        w = RenderWorld(exe, exists=scene.get("exists", 1))
        cells = []
        cells.append(_cell(w, "L", 0x2000))
        for value in scene.get("args", []):
            cells.append(_cell(w, "L", value))
        this = _render_obj(w, 0x2000)
        frame = _frame(w, init_sig, cells, b"java.util.resource.ParticleSystem")
        w.obj = this
        try:
            _run(w, 0x47F0B0, frame)
        except Exception as error:  # noqa: BLE001
            row = _row(w, "init", 0x47F0B0, init_sig.decode(), scene, b"java.util.resource.ParticleSystem")
            row["error"] = repr(error)
            row["id"] = rid
            rows.append(row)
            rid += 1
            continue
        row = _row(w, "init", 0x47F0B0, init_sig.decode(), scene, b"java.util.resource.ParticleSystem")
        row["id"] = rid
        rows.append(row)
        rid += 1
    # stop
    for scene in [{"exists": 0}, {}]:
        w = RenderWorld(exe, exists=scene.get("exists", 1))
        cells = [_cell(w, "L", 0x2000)]
        this = _render_obj(w, 0x2000)
        frame = _frame(w, b"()V", cells, b"java.util.resource.ParticleSystem")
        w.obj = this
        _run(w, 0x47F120, frame)
        row = _row(w, "stop", 0x47F120, "()V", scene, b"java.util.resource.ParticleSystem")
        row["id"] = rid
        rows.append(row)
        rid += 1
    # the source setters: full real walk over planted nodes.
    src_sig = b"(Ljava.lang.String;Ljava.lang.Vector3;FFLjava.lang.Vector3;FFFLjava.lang.String;)V"
    rate = 0x3F800000
    for name, addr, type_id, scene in [
        ("setDirectSource", 0x47F270, 0xB, {"pre": []}),
        ("setDirectSource", 0x47F270, 0xB, {"pre": [(0xA, "fx")]},),
        ("setDirectSource", 0x47F270, 0xB, {"pre": [(0xB, "fx")], "reuse": True}),
        ("setSource", 0x47F660, 0xA, {"pre": []}),
        ("setSource", 0x47F660, 0xA, {"pre": [(0xB, "fx")]}),
        ("setSource", 0x47F660, 0xA, {"pre": [(0xA, "fx")], "reuse": True}),
        ("setSource", 0x47F660, 0xA, {"label": (0x39,)}),
    ]:
        w = RenderWorld(exe)
        label = "FX" if not scene.get("label") else "missing"
        pre = []
        for node_type, node_label in scene.get("pre", []):
            pre.append(w.node(node_type, node_label))
        w.particle_obj(flags=0x1234, freq_bits=rate)
        w.link(pre)
        cells = [_cell(w, "L", 0x2000)]
        # label, v0, f, f, v1, f, f, f, bone(null)
        label_obj = w.o.alloc(8)
        w.o.write(label_obj, label.encode() + b"\0")
        for value in [label_obj, 0x1111, 1.5, 2.0, 0x2222, 0.5, 4.0, 8.0, 0]:
            cells.append(_cell(w, "L", value))
        this = _render_obj(w, 0x2000)
        frame = _frame(w, src_sig, cells, b"java.util.resource.ParticleSystem")
        w.obj = this
        w.components.update({"x": 1.0, "y": 2.0, "z": 3.0})
        w.obj_components[w.renv] = {"x": 4.0, "y": 5.0, "z": 6.0}
        try:
            _run(w, addr, frame)
        except Exception as error:  # noqa: BLE001
            row = _row(w, name, addr, src_sig.decode(), {**scene, "label": label},
                       b"java.util.resource.ParticleSystem")
            row["error"] = repr(error)
            row["id"] = rid
            rows.append(row)
            rid += 1
            continue
        row = _row(w, name, addr, src_sig.decode(), {**scene, "label": label},
                   b"java.util.resource.ParticleSystem")
        row["actions"] = w.actions_view()
        row["id"] = rid
        rows.append(row)
        rid += 1
    return _write(rows, "res_psys2_sources.jsonl")


# ------------------------------------------------------------------ paint

def gen_paint(exe=None) -> str:
    rows = []
    rid = 0
    for name, addr, sig, scenes in [
        ("xPaint", 0x482C00, "(Ljava.util.resource.GameRef;)V", [{}]),
        ("paintPart", 0x4827D0, "(Ljava.util.resource.GameRef;I)V", [
            {"query": 0},
            {"query": 1, "type": 9},
            {"query": 1, "type": 5},
            {"query": 1, "type": 3},
        ]),
        ("doPaint", 0x482960, "(Ljava.util.resource.GameRef;IIIFFI)V", [
            {"query": 0},
            {"query": 1, "type": 9},
            {"query": 1, "type": 3},
        ]),
    ]:
        for scene in scenes:
            w = RenderWorld(exe)
            w.paint_query = scene.get("query", 0)
            w.obj_components[w.renv] = {}
            cells = [_cell(w, "L", 0x2000)]
            for value in scene.get("args", []):
                cells.append(_cell(w, "I", value))
            this = _render_obj(w, 0)
            frame = _frame(w, sig.encode(), cells, b"java.game.Painter")
            w.obj = this
            try:
                _run(w, addr, frame)
            except Exception as error:  # noqa: BLE001
                row = _row(w, name, addr, sig, scene, b"java.game.Painter")
                row["error"] = repr(error)
                row["id"] = rid
                rows.append(row)
                rid += 1
                continue
            row = _row(w, name, addr, sig, scene, b"java.game.Painter")
            row["id"] = rid
            rows.append(row)
            rid += 1
    return _write(rows, "res_paint_painter.jsonl")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", default=rv._default_exe())
    parser.add_argument("groups", nargs="*", default=["rset", "anim2", "psys2", "paint"])
    args = parser.parse_args()
    paths = []
    for group in args.groups:
        if group == "rset":
            paths.append(gen_rset(args.exe))
        elif group == "anim2":
            paths.append(gen_anim2(args.exe))
        elif group == "psys2":
            paths.append(gen_psys2(args.exe))
        elif group == "paint":
            paths.append(gen_paint(args.exe))
        else:
            raise SystemExit(f"unknown group {group}")
    for path in paths:
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
