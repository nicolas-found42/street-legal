#!/usr/bin/env python3
"""Unicorn vectors for the GroundRef / PhysicsRef / Part native slice
(worker NativesGroundPhysics; ticket: 38+3 missing rows, missing_rows.rs).

Groups (natives run as real exe code; every engine tail, message-port query,
envelope fetch, gate and JVM field/class helper is a hooked, recorded
boundary — the accepted `res_vectors.py` methodology):

  phys    0x480b00 getPos      0x480c10 getOri     0x480920 setMatrix
          0x4807f0 create     0x4805b0 createBox  0x4806f0 createSphere
  part    0x468ab0 getSlotDamage       0x468c50 setSlotDamage
          0x468df0 setSlotPos  0x469cc0 getMesh    0x469d90 setMesh
          0x46a0e0 getLogo    0x469290 getMass    0x46a1b0 setSfxLoopParams
          0x46a5a0 flap       0x46a680 install_OK
  ground  0x4866c0 setWater(FFF)          0x4867b0 setWater(V3V3FF)
          0x486920 addWaterLimit          0x486a20 setFog
          0x484730 addTrafficCar          0x484a90 remTrafficCar
          0x484af0 notifyTrafficCar       0x484050 addTrafficN
          0x484420 addTrafficP            0x484b50 delTraffic
          0x487ec0 setTrafficCarBehaviour 0x484b90 haltTrafficCross
          0x4835e0 haltTrafficPath        0x483d00 getNearestCross
          0x483400 getStartDirection      0x483750 getRouteLength(V3V3)/findRoute
          0x483e60 alignToRoad            0x483c00 getRouteLength()
          0x483b30 getRoutePos            0x483c20 getRouteDist
          0x484d30 setPedestrianDensityN  0x484d90 addPedestrianType
          0x484e20 removePedestrianType   0x484c60 pedestrianDistance

Part fake graph (mirrors the exe chain the natives walk):
  obj(+8 handle flag, +0xc ref) -> ref vtable{+0xc fetch -> E1, +0x14 materialise}
  E1(+0x4c mode, +0x44 -> E2, +0x4c -> runtime body B)
  B(+0x78 runtime slot base, +0x94 mesh, +0xa8, +0xb4 tear, +0xb8 wear...,
    +0x114/+0x118 sfx loop, +0x1120?? no: template K)
  second fetch 0x419860(E1, 1.0f, tag, 0, 0) -> X / 0x419860(E2, ...,0x20000000) -> Y
  D = E1.vtable[0xc](1.0f); K = E2.vtable[0xc](1.0f) or [Y+0xc]
  template T = K+0xc: +0x1150 slot count, +0x1154/+0x115c descriptor list
  descriptors: +4 next, +0x48 slot id, +0xc pos(3f), +0x18 matrix words(12)

Rows (JSONL under re/out/boot/ground-physics-part/, git-ignored) carry the
full recorded boundary events so the Rust replay asserts each native's own
branching, pushed words and returned word exactly.
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, DEFAULT_EXE, CW_D3D9  # noqa: E402
from res_vectors import (World, _alloc_cstr, _cell, _fbits, _f32_of,  # noqa: E402
                         _frame)

OUT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))), "re", "out", "boot", "ground-physics-part")
CW = CW_D3D9

# ---- exe data words the port must mirror (pinned once in the manifest row)
CONSTANTS = {
    "fmt_slot_damage": 0x6114A8, "fmt_slot_damage_scan": 0x6114C8,
    "zero": 0x5E73CC, "unit": 0x5F08F0, "fog_scale": 0x5E7334,
    "box_scale": 0x5F09D0, "rand_scale": 0x5F0DBC, "water_default_a": 0x5F0B0C,
    "water_default_b": 0x5F0D50, "trafficn_scale": 0x5F13C4,
    "trafficcar_scale": 0x5F13C8, "trafficn_speed": 0x5F0C4C,
    "ped_density_scale": 0x5F0C6C, "ped_distance_default": 0x5F0C70,
}

PART_CLASS = b"java.game.parts.Part"
GROUND_CLASS = b"java.util.resource.GroundRef"
PHYSICS_CLASS = b"java.util.resource.PhysicsRef"

NATIVES = {
    # phys
    "phys_getpos": (0x480B00, b"()Ljava.lang.Vector3;", PHYSICS_CLASS),
    "phys_getori": (0x480C10, b"()Ljava.lang.Ypr;", PHYSICS_CLASS),
    "phys_setmatrix": (0x480920, b"(Ljava.lang.Vector3;Ljava.lang.Ypr;)V", PHYSICS_CLASS),
    "phys_create": (0x4807F0, b"(Ljava.util.resource.ResourceRef;ILjava.lang.String;)V", PHYSICS_CLASS),
    "phys_createbox": (0x4805B0, b"(Ljava.util.resource.ResourceRef;FFFLjava.lang.String;)V", PHYSICS_CLASS),
    "phys_createsphere": (0x4806F0, b"(Ljava.util.resource.ResourceRef;FLjava.lang.String;)V", PHYSICS_CLASS),
    # part
    "part_getslotdamage": (0x468AB0, b"(I)Ljava.lang.String;", PART_CLASS),
    "part_setslotdamage": (0x468C50, b"(ILjava.lang.String;)V", PART_CLASS),
    "part_setslotpos": (0x468DF0, b"(ILjava.lang.Vector3;Ljava.lang.Ypr;)V", PART_CLASS),
    "part_getmesh": (0x469CC0, b"()I", PART_CLASS),
    "part_setmesh": (0x469D90, b"(I)I", PART_CLASS),
    "part_getlogo": (0x46A0E0, b"()I", PART_CLASS),
    "part_getmass": (0x469290, b"()F", PART_CLASS),
    "part_setsfxloop": (0x46A1B0, b"(FF)I", PART_CLASS),
    "part_flap": (0x46A5A0, b"(I)I", PART_CLASS),
    "part_installok": (0x46A680, b"(Ljava.util.resource.GameRef;ILjava.util.resource.GameRef;ILjava.lang.Vector3;)[I", PART_CLASS),
    # ground water/fog
    "ground_setwater_fff": (0x4866C0, b"(FFF)V", GROUND_CLASS),
    "ground_setwater_vv": (0x4867B0, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;FF)V", GROUND_CLASS),
    "ground_addwaterlimit": (0x486920, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;)V", GROUND_CLASS),
    "ground_setfog": (0x486A20, b"(IFF)V", GROUND_CLASS),
    # ground traffic
    "ground_addtrafficcar": (0x484730, b"(Ljava.util.resource.GameRef;Ljava.lang.Vector3;)I", GROUND_CLASS),
    "ground_remtrafficcar": (0x484A90, b"(I)V", GROUND_CLASS),
    "ground_notifytrafficcar": (0x484AF0, b"(II)I", GROUND_CLASS),
    "ground_addtrafficn": (0x484050, b"(Ljava.util.resource.GameRef;IFFF)I", GROUND_CLASS),
    "ground_addtrafficp": (0x484420, b"(Ljava.util.resource.GameRef;Ljava.lang.Vector3;IFFF)I", GROUND_CLASS),
    "ground_deltraffic": (0x484B50, b"()V", GROUND_CLASS),
    "ground_settrafficbehaviour": (0x487EC0, b"(II)V", GROUND_CLASS),
    "ground_halttrafficcross": (0x484B90, b"(Ljava.lang.Vector3;F)V", GROUND_CLASS),
    "ground_halttrafficpath": (0x4835E0, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;)V", GROUND_CLASS),
    # ground route
    "ground_getnearestcross": (0x483D00, b"(Ljava.lang.Vector3;F)Ljava.lang.Vector3;", GROUND_CLASS),
    "ground_getstartdirection": (0x483400, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;)Ljava.lang.Vector3;", GROUND_CLASS),
    "ground_getroutelength": (0x483750, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;)F", GROUND_CLASS),
    "ground_alignroad": (0x483E60, b"(Ljava.lang.Vector3;)[Ljava.lang.Vector3;", GROUND_CLASS),
    "ground_routelength0": (0x483C00, b"()F", GROUND_CLASS),
    "ground_routepos": (0x483B30, b"(F)Ljava.lang.Vector3;", GROUND_CLASS),
    "ground_routedist": (0x483C20, b"(Ljava.lang.Vector3;)F", GROUND_CLASS),
    # ground pedestrians
    "ground_peddensity": (0x484D30, b"(F)V", GROUND_CLASS),
    "ground_addpedtype": (0x484D90, b"(Ljava.util.resource.GameRef;)V", GROUND_CLASS),
    "ground_rempedtype": (0x484E20, b"(Ljava.util.resource.GameRef;)V", GROUND_CLASS),
    "ground_peddistance": (0x484C60, b"(Ljava.lang.Vector3;I)F", GROUND_CLASS),
}


def _messaging(w: dict, o: Oracle) -> None:
    """Record the 0x636338 message-port sends (0x426470) and answer them."""
    def send(oc, args):
        w["messages"].append({"target": args[0], "msg": args[1],
                              "arg": args[2], "ecx": oc.reg("ecx")})
        return w["controller"]
    o.hook(0x426470, send, "thiscall", 3)


def _fields(w: dict, o: Oracle) -> None:
    def get(oc, a):
        name = oc.read_cstr(a[1], 8).decode("latin1")
        w["field_gets"].append([a[0], name])
        return w["components"].get(a[0], {}).get(name, 0.0)
    def put(oc, a):
        name = oc.read_cstr(a[1], 8).decode("latin1")
        w["field_sets"].append([a[0], name, a[2]])
        return None
    o.hook(0x42A560, get, "cdecl", 2)
    o.hook(0x42A040, put, "cdecl", 3)


def _classes(w: dict, o: Oracle) -> None:
    def load(oc, a):
        name = oc.read_cstr(a[0], 64).decode("latin1")
        w["class_loads"].append(name)
        return 0x5100 + len(w["class_loads"])
    def alloc_obj(oc, a):
        w["object_allocs"].append({"class": a[0], "count": a[1], "ecx": oc.reg("ecx")})
        obj = oc.alloc(0x40)
        w["allocated"].append(obj)
        return obj
    o.hook(0x410890, load, "thiscall", 1)
    o.hook(0x407DA0, alloc_obj, "thiscall", 2)
    o.hook(0x4174A0, lambda oc, a: (w["allocated"].append("intern"),
                                    0x7100 + len(w["allocated"]))[1],
           "thiscall", 1)


def _arrays(w: dict, o: Oracle) -> None:
    o.hook(0x42B060, lambda oc, a: w["array_sets"].append(["i", a[0], a[1], a[2]]) or 1,
           "cdecl", 3)
    o.hook(0x42B0A0, lambda oc, a: w["array_sets"].append(["a", a[0], a[1], a[2]]) or 1,
           "cdecl", 3)


def _engine(w: dict, o: Oracle) -> None:
    """Every engine tail the group can reach: recorded, configurable result."""
    calls = []
    def record(name, nargs, result=None, conv_default=True):
        def call(oc, a):
            calls.append({"engine": name, "ecx": oc.reg("ecx"),
                          "args": list(a[:nargs])})
            live = w["engine_result"].get(name, 0)
            if callable(live):
                return live(oc, a)
            return live
        return call
    o.hook(0x48AEA0, record("body_ptr", 0, lambda oc, a: w["body_ptr"]), "thiscall", 0)
    for spec in [(0x48A1E0, "attach", 5), (0x49A990, "bodydef", 2),
                          (0x49AB60, "body", 5), (0x49B440, "set_water", 5),
                          (0x49B530, "water_limit", 2), (0x4986F0, "swept", 1, "cdecl"),
                          (0x48AA70, "controller", 1), (0x48ABA0, "detach", 1),
                          (0x581E00, "mk_traffic_car", 8), (0x57B420, "pick_cross", 4, "cdecl"),
                          (0x581A00, "nearest_cross", 1), (0x583840, "start_dir", 1),
                          (0x583FD0, "halt_path", 2), (0x57C170, "halt_mode", 2),
                          (0x581480, "del_traffic", 0), (0x5791C0, "activate", 0),
                          (0x57EF20, "project", 2), (0x57EA00, "route_pos", 1, "cdecl"),
                          (0x57DA40, "route_free", 0), (0x5826F0, "find_route", 8),
                          (0x589B80, "ped_density", 2), (0x589940, "ped_add", 1),
                          (0x589A20, "ped_remove", 1), (0x589BE0, "ped_distance", 1),
                          (0x467D30, "mesh_release", 3, "cdecl"), (0x546070, "mesh_find", 3),
                          (0x473B10, "install_slot", 3), (0x474B00, "flap", 2),
                          (0x473230, "rebuild", 1), (0x470E20, "rebuild2", 1),
                          (0x5447D0, "gate", 3), (0x419860, "fetch", 4),
                          (0x4261F0, "config_float", 1), (0x545FC0, "string_ctor", 1),
                          (0x426800, "queue", 5),
                          (0x429060, "list_insert", 1, "cdecl")]:
        addr, name, n = spec[0], spec[1], spec[2]
        conv = spec[3] if len(spec) > 3 else "thiscall"
        o.hook(addr, record(name, n, w["engine_result"].get(name, 0)), conv, n)
    w["engine_calls"] = calls


def _deterministic(o: Oracle) -> None:
    """rand(): the exe's LCG with a planted seed so draws are reproducible."""
    state = {"value": 0x12345678}

    def rand(oc, a):
        state["value"] = (0x343FD * state["value"] + 0x269EC3) & 0xFFFFFFFF
        return (state["value"] >> 16) & 0x7FFF
    o.hook(0x5D7408, rand, "cdecl", 0)


def fresh(case: dict) -> dict:
    """One World plus the group-agnostic hooks and recorded-boundary state."""
    w = World(case.get("exe"), exists=1, mode=1)
    o = w.o
    state = {
        "o": o, "world": w, "controller": case.get("controller", 0),
        "body_ptr": case.get("body_ptr", 0),
        "components": {}, "allocated": [], "engine_result": case.get("engine_result", {}),
        "fetch": case.get("fetch", {}), "envelopes": [],
        "messages": [], "field_gets": [], "field_sets": [], "class_loads": [],
        "object_allocs": [], "array_sets": [], "engine_calls": [],
    }
    o.uc.mem_write(0x7FE00000, b"\x00" * 0x100000)
    try:
        o.uc.mem_map(0, 0x1000)  # uninitialised-memory branch reads land on 0
    except Exception:
        pass
    _deterministic(o)
    _messaging(state, o)
    _fields(state, o)
    _classes(state, o)
    _arrays(state, o)
    _engine(state, o)

    def fetch(oc, a):
        state.setdefault("fetch_calls", []).append(
            {"ecx": oc.reg("ecx"), "args": list(a)})
        return state["fetch"].get(oc.reg("ecx"), state["fetch"].get("*", 0))
    o.hook(0x419860, fetch, "thiscall", 4)

    def gate(oc, a):
        state.setdefault("gate_calls", []).append(
            {"ecx": oc.reg("ecx"), "tag": a[0]})
        return 0x80000000 if case.get("gate_refused") else 0
    o.hook(0x5447D0, gate, "thiscall", 3)
    o.hooks.pop(0x5447D0, None)
    o.hooks[0x5447D0] = (gate, "thiscall", 3)
    return state


# ------------------------------------------------------------------ phys
def phys_view(w, name, case, res):
    o = w["o"]
    view = {"ret": res.eax,
            "st0": None if res.st0 is None else res.st0_raw.hex(),
            "writes": [[a, sz, v] for (a, sz, v) in res.mem_writes]}
    return view


def run_phys(name, case, exe=None):
    w = fresh(case)
    o = w["o"]
    world = w["world"]
    addr, sig, cls = NATIVES[name]
    if case.get("handle_null"):
        world.obj = 0
    engine = 0
    if case.get("engine_live"):
        engine = o.alloc(0x400)
        body = o.alloc(0x8000)
        o.write_u32(engine + 0x84, body)
        o.write_u32(body + 0x20C, case["slot"])
        for index, words in enumerate(case.get("slots", [])):
            o.write_u32s(body + index * 0xD4 + 0x64, words[0])
            o.write_u32s(body + index * 0xD4 + 0xA4, words[1])
            for r in range(3):
                o.write_u32s(body + index * 0xD4 + 0xA4 + r * 16,
                             words[1][r * 3:r * 3 + 3])
        w["body_ptr"] = engine
        w["engine_base"] = body
    if name.startswith("phys_create"):
        w["engine_result"]["bodydef"] = o.alloc(0x200)
        w["engine_result"]["body"] = o.alloc(0x200)
    if name in ("phys_setmatrix",):
        pos, ori = o.alloc(0x20), o.alloc(0x20)
        w["components"][pos] = dict(zip("xyz", [_f32_of(b) for b in case["pos"]]))
        w["components"][ori] = {"y": _f32_of(case["ori"][0]),
                                "p": _f32_of(case["ori"][1]),
                                "r": _f32_of(case["ori"][2])}
        args = [_cell(o, "L", 0 if case.get("ori_null") else ori),
                _cell(o, "L", 0 if case.get("pos_null") else pos)]
    elif name in ("phys_create",):
        resw = o.alloc(0x40)
        name_arg = _alloc_cstr(o, case.get("name", b"box"))
        args = [_cell(o, "L", name_arg), _cell(o, "I", case.get("type", 0)),
                _cell(o, "L", 0 if case.get("res_null") else resw)]
    elif name in ("phys_createbox",):
        resw = o.alloc(0x80)
        name_arg = _alloc_cstr(o, case.get("name", b"box"))
        args = [_cell(o, "L", name_arg),
                _cell(o, "F", _fbits(case["x"])), _cell(o, "F", _fbits(case["y"])),
                _cell(o, "F", _fbits(case["z"])),
                _cell(o, "L", 0 if case.get("res_null") else resw)]
    elif name in ("phys_createsphere",):
        resw = o.alloc(0x80)
        name_arg = _alloc_cstr(o, case.get("name", b"sphere"))
        args = [_cell(o, "L", name_arg), _cell(o, "F", _fbits(case["r"])),
                _cell(o, "L", 0 if case.get("res_null") else resw)]
    else:
        args = []
    if case.get("res_null"):
        # 0x42ab50 answers the receiver word: null receiver -> exe error path.
        world.obj = 0
    frame = _frame(w["world"], sig, args + [_cell(o, "L", 1)], cls=cls)
    res = o.call(addr, [frame], "cdecl", limit=400_000)
    view = phys_view(w, name, case, res)
    view["events"] = {k: w[k] for k in ("messages", "field_gets", "field_sets",
                                        "class_loads", "object_allocs",
                                        "engine_calls", "array_sets")}
    view["engine_base"] = w.get("engine_base")
    if w.get("engine_base") is not None:
        lo = w["engine_base"]
        view["body_words"] = o.read_u32s(lo, 0x400 // 4)
    return view


def phys_cases(name):
    live = {"engine_live": True, "slot": 0,
            "slots": [[[0x41200000, 0x41400000, 0x41600000],
                       [0x3F800000, 0, 0, 0, 0x3F800000, 0, 0, 0, 0x3F800000]]]}
    if name in ("phys_getpos", "phys_getori"):
        out = [{"handle_null": True}, {}, dict(live, slot=1, slots=[
            [[0x41A00000, 0x41C00000, 0x41E00000],
             [0, 0x3FC00000, 0, 0xBF800000, 0, 0, 0, 0, 0x3F800000]]]),
            dict(live, body_ptr=0)]
        return out
    if name == "phys_setmatrix":
        base = {"engine_live": True, "slot": 0, "slots": [[[0] * 3, [0] * 9]]}
        out = [dict(base, pos=[_fbits(10.0), _fbits(20.0), _fbits(30.0)],
                    ori=[_fbits(1.0), _fbits(2.0), _fbits(3.0)])]
        out.append(dict(base, body_ptr=0, pos=[_fbits(1.0), _fbits(2.0), _fbits(3.0)],
                        ori=[_fbits(4.0), _fbits(5.0), _fbits(6.0)]))
        out.append(dict(base, engine_live=False,
                        pos=[_fbits(1.0), _fbits(2.0), _fbits(3.0)],
                        ori=[_fbits(4.0), _fbits(5.0), _fbits(6.0)]))
        out.append(dict(base, pos_null=True,
                        pos=[0, 0, 0], ori=[_fbits(0.5), _fbits(0.25), _fbits(0.125)]))
        out.append(dict(base, ori_null=True,
                        pos=[_fbits(7.0), _fbits(8.0), _fbits(9.0)], ori=[0, 0, 0]))
        return out
    if name == "phys_create":
        return [{"res_null": True, "type": 0}, {"type": 0},
                {"type": 1, "name": b"wheel"},
                {"type": 0x7FFFFFFF, "name": b""}]
    if name == "phys_createbox":
        return [{"res_null": True, "x": 1.0, "y": 2.0, "z": 3.0},
                {"x": 1.0, "y": 2.0, "z": 3.0},
                {"x": 0.0, "y": -2.5, "z": 1e10},
                {"x": 3.4e38, "y": 1e-38, "z": -0.0}]
    if name == "phys_createsphere":
        return [{"res_null": True, "r": 1.0}, {"r": 1.0},
                {"r": 0.0}, {"r": -3.5}, {"r": 3.4e38}]
    return []


# ------------------------------------------------------------------ part
def part_chain(w, o, case):
    """obj -> ref -> E1(+0x44 E2, +0x4c body); vtable fetches; template K."""
    body = o.alloc(0x8000)
    template = o.alloc(0x2000)
    k = o.alloc(0x40)
    o.write_u32(k + 0xC, template)
    o.write_u32(template + 0x1150, case.get("slot_count", 2))
    o.write_u32(template + 0x1120, case.get("logo", 0x5150))
    descs = []
    head = o.alloc(0x80)
    descs.append(head)
    for i in range(1, case.get("slot_count", 2)):
        node = o.alloc(0x80)
        descs.append(node)
    for i, node in enumerate(descs):
        o.write_u32(node + 4, descs[i + 1] if i + 1 < len(descs) else 0)
        o.write_u32(node + 0x48, 100 + i)
        o.write_u32s(node + 0xC, case.get("desc_pos", [0x3F800000, 0x40000000, 0x40400000]))
        o.write_u32s(node + 0x18, case.get("desc_matrix", [0x3F800000, 0, 0, 0,
                                                           0, 0x3F800000, 0, 0,
                                                           0, 0, 0x3F800000, 0]))
    if descs:
        o.write_u32(template + 0x115C, descs[0])
    base = o.alloc(0x68 * max(case.get("slot_count", 2), 1) + 0x100)
    o.write_u32(body + 0x78, base)
    for i in range(case.get("slot_count", 2)):
        slot = base + i * 0x68
        o.write_u32(slot + 0x48, 100 + i)
        o.write_u32(slot + 0x14, case.get("slot_flags", 0))
        o.write_u32s(slot + 0x2C, case.get("slot_damage", [0, 0, 0]))
        for r in range(3):
            o.write_u32s(slot + 0x38 + r * 16, [0x3F800000, 0, 0])
    o.write_u32(body + 0x94, case.get("mesh", 0x77))
    o.write_u32(body + 0xA8, case.get("a8", 0))
    o.write_u32(body + 0xB4, 0)
    o.write_u32(body + 0xB8, 0)
    o.write_u32(body + 0xCC, case.get("cc", 0x31))
    o.write_u32(body + 0xC8, case.get("c8", 0x32))
    o.write_u32(body + 0xD0, case.get("d0", 0x3F800000))
    o.write_u32(body + 0x114, case.get("loop_a", 0))
    o.write_u32(body + 0x118, case.get("loop_b", 0))
    o.write_u32(body + 0xDD, 0)
    o.write_u32(body + 0x138, case.get("traffic", 0x99))
    o.write_u32(body + 0x160, 0)
    peer = case.get("peer", 0x4444)

    e2 = o.alloc(0x80)
    e1 = o.alloc(0x80)
    d = o.alloc(0x80)
    o.write_u32(e1 + 0x4C, 1)
    o.write_u32(e1 + 0x44, e2)
    o.write_u32(e1 + 0x4C, 1)
    o.write_u32(d + 0x44, e2)
    o.write_u32(d + 0x4C, body)
    o.write_u32(e2 + 0x4C, 1)
    o.write_u32(e2 + 0xC, k)
    vt = o.read_u32(w["world"].ref)
    stub = o.alloc(16)
    chain_vt = o.alloc(0x20)

    def resolve(oc, a):
        w["engine_calls"].append({"engine": "resolve", "ecx": oc.reg("ecx"), "args": list(a)})
        return d if oc.reg("ecx") == e1 else k
    o.hook(stub, resolve, "thiscall", 1)
    o.write_u32(vt + 0xC, stub)
    o.write_u32(vt + 0x14, stub)
    o.write_u32(chain_vt + 0xC, stub)
    for env in (e1, e2, d, k):
        o.write_u32(env, chain_vt)
    w["fetch"] = {e1: d, e2: k}
    w["engine_result"]["install_slot"] = case.get("install_node", 0x333)
    w["engine_result"]["flap"] = case.get("flap_result", 0x5A5A)
    w["engine_result"]["mesh_find"] = case.get("mesh_find", 0x0BAD)
    w["engine_result"]["bodydef"] = case.get("bodydef", 0x0BEEF)
    w["engine_result"]["attach"] = case.get("attach", 0)
    obj = w["world"].obj
    if not case.get("no_envelope"):
        o.write_u32(obj + 0xC, e1)
    if case.get("bound_null"):
        o.write_u32(obj + 0xC, 0)
    if case.get("handle_null"):
        w["world"].obj = 0
    w["part"] = {"body": body, "template": template, "base": base,
                 "descs": descs, "e1": e1, "e2": e2, "d": d, "k": k}
    return obj


def run_part(name, case, exe=None):
    w = fresh(case)
    o = w["o"]
    addr, sig, cls = NATIVES[name]
    part_chain(w, o, case)
    p = w["part"]
    args = []
    if name == "part_getslotdamage":
        args = [_cell(o, "I", case["index"])]
    elif name == "part_setslotdamage":
        text = case.get("text", b"0.5 1.25 -2 0.1 0.2 0.3")
        args = [_cell(o, "I", case["index"]), _cell(o, "L", _alloc_cstr(o, text))]
    elif name == "part_setslotpos":
        pos = ori = 0
        if not case.get("pos_null"):
            pos = o.alloc(0x20)
            w["components"][pos] = dict(zip("xyz", [_f32_of(b) for b in case["pos"]]))
        if not case.get("ori_null"):
            ori = o.alloc(0x20)
            w["components"][ori] = {"y": _f32_of(case["ori"][0]),
                                    "p": _f32_of(case["ori"][1]),
                                    "r": _f32_of(case["ori"][2])}
        args = [_cell(o, "I", case["index"]), _cell(o, "L", pos), _cell(o, "L", ori)]
    elif name == "part_setmesh":
        args = [_cell(o, "I", case.get("mesh_arg", 0x88))]
    elif name == "part_setsfxloop":
        args = [_cell(o, "F", _fbits(case.get("a", 1.5))),
                _cell(o, "F", _fbits(case.get("b", -0.75)))]
    elif name == "part_flap":
        args = [_cell(o, "I", case.get("flap_arg", 2))]
    elif name == "part_installok":
        g1 = 0 if case.get("g1_null") else o.alloc(0x40)
        g2 = 0 if case.get("g2_null") else o.alloc(0x40)
        vec = 0 if case.get("vec_null") else o.alloc(0x20)
        if vec:
            w["components"][vec] = dict(zip("xyz", [_f32_of(b) for b in case.get("vec", [0x3E800000, 0xBF000000, 0x40000000])]))
        args = [_cell(o, "L", g1), _cell(o, "I", case.get("slot", 100)),
                _cell(o, "L", g2), _cell(o, "I", case.get("slot2", 101)),
                _cell(o, "L", vec)]
        w["components"][g2] = {"y": 0.0, "p": 0.0, "r": 0.0} if g2 else {}
    frame = _frame(w["world"], sig, args + [_cell(o, "L", 1)], cls=cls)
    res = o.call(addr, [frame], "cdecl", limit=400_000)
    view = {"ret": res.eax,
            "st0": None if res.st0 is None else res.st0_raw.hex(),
            "writes": [[a, sz, v] for (a, sz, v) in res.mem_writes]}
    view["events"] = {k: w[k] for k in ("messages", "field_gets", "field_sets",
                                        "class_loads", "object_allocs",
                                        "engine_calls", "array_sets")}
    view["slot_words"] = {}
    for i in range(case.get("slot_count", 2)):
        slot = p["base"] + i * 0x68
        view["slot_words"][i] = {"flags": o.read_u32(slot + 0x14),
                                 "damage": o.read_u32s(slot + 0x2C, 3),
                                 "matrix": o.read_u32s(slot + 0x38, 9)}
    view["mesh_word"] = o.read_u32(p["body"] + 0x94)
    view["sfx_loop"] = [o.read_u32(p["body"] + 0x114), o.read_u32(p["body"] + 0x118)]
    view["logo_word"] = o.read_u32(p["template"] + 0x1120)
    return view


def part_cases(name):
    base = {"slot_count": 2, "slot_flags": 0x20,
            "slot_damage": [_fbits(0.5), _fbits(1.25), _fbits(-2.0)]}
    if name == "part_getslotdamage":
        fmt = b"y=%f p=%f r=%f d=%f,%f,%f"  # placeholder; exe string pinned in manifest
        return [dict(base, index=0), dict(base, index=1), dict(base, index=5),
                dict(base, index=-1), dict(base, index=0, slot_flags=0),
                dict(base, index=0, slot_count=0)]
    if name == "part_setslotdamage":
        return [dict(base, index=0, text=b"0.5 1.25 -2 0.1 0.2 0.3"),
                dict(base, index=0, text=b""),
                dict(base, index=0, text=b"x"),
                dict(base, index=3, text=b"0.5 1.25 -2 0.1 0.2 0.3"),
                dict(base, index=0, slot_flags=0, text=b"0.5 1.25 -2 0.1 0.2 0.3")]
    if name == "part_setslotpos":
        pos = [_fbits(0.25), _fbits(-0.5), _fbits(2.0)]
        ori = [_fbits(10.0), _fbits(20.0), _fbits(30.0)]
        return [dict(base, index=0, pos=pos, ori=ori),
                dict(base, index=0, pos=pos, ori=ori, pos_null=True),
                dict(base, index=0, pos=pos, ori=ori, ori_null=True),
                dict(base, index=4, pos=pos, ori=ori)]
    if name == "part_getmesh":
        return [dict(base), dict(base, mesh=0)]
    if name == "part_setmesh":
        return [dict(base, mesh_arg=0x88), dict(base, mesh=0, mesh_arg=0x88)]
    if name == "part_getlogo":
        return [dict(base, logo=0x5150), dict(base, logo=0)]
    if name == "part_getmass":
        return [dict(base)]
    if name == "part_setsfxloop":
        return [dict(base, a=1.5, b=-0.75), dict(base, bound_null=True, a=1.5, b=-0.75)]
    if name == "part_flap":
        return [dict(base, flap_arg=2), dict(base, flap_arg=2, flap_result=-1),
                dict(base, handle_null=True, flap_arg=2)]
    if name == "part_installok":
        return [dict(base), dict(base, g1_null=True), dict(base, vec_null=True),
                dict(base, handle_null=True)]
    return []


# ------------------------------------------------------------------ ground
def run_ground(name, case, exe=None):
    w = fresh(case)
    o = w["o"]
    addr, sig, cls = NATIVES[name]
    # the world object the water natives write (file image holds 0x6409c4)
    o.write_u32(0x617650, 0x6409C4)
    o.write_u32s(0x6408D0, case.get("route_cache", [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]))
    vecs = []
    for spec in case.get("vectors", []):
        v = o.alloc(0x20)
        w["components"][v] = dict(zip("xyz", [_f32_of(b) for b in spec]))
        vecs.append(v)
    args = []
    floats = case.get("floats", [])
    if name == "ground_setwater_fff":
        args = [_cell(o, "F", _fbits(f)) for f in floats]
    elif name == "ground_setwater_vv":
        args = [_cell(o, "L", 0 if case.get("v1_null") else vecs[0]),
                _cell(o, "L", 0 if case.get("v2_null") else vecs[1]),
                _cell(o, "F", _fbits(floats[0])), _cell(o, "F", _fbits(floats[1]))]
    elif name == "ground_addwaterlimit":
        args = [_cell(o, "L", 0 if case.get("v1_null") else vecs[0]),
                _cell(o, "L", 0 if case.get("v2_null") else vecs[1])]
    elif name == "ground_setfog":
        args = [_cell(o, "I", case.get("mode", 2)),
                _cell(o, "F", _fbits(floats[0])), _cell(o, "F", _fbits(floats[1]))]
    elif name == "ground_addtrafficcar":
        args = [_cell(o, "L", 0 if case.get("arg_null") else o.alloc(0x40)),
                _cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0])]
    elif name in ("ground_addtrafficn",):
        args = [_cell(o, "L", 0 if case.get("arg_null") else o.alloc(0x40)),
                _cell(o, "I", case.get("count", 2)),
                _cell(o, "F", _fbits(floats[0])), _cell(o, "F", _fbits(floats[1])),
                _cell(o, "F", _fbits(floats[2]))]
    elif name in ("ground_addtrafficp",):
        args = [_cell(o, "L", 0 if case.get("arg_null") else o.alloc(0x40)),
                _cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0]),
                _cell(o, "I", case.get("count", 1)),
                _cell(o, "F", _fbits(floats[0])), _cell(o, "F", _fbits(floats[1])),
                _cell(o, "F", _fbits(floats[2]))]
    elif name == "ground_remtrafficcar":
        args = [_cell(o, "I", case.get("channel", 3))]
    elif name == "ground_notifytrafficcar":
        args = [_cell(o, "I", case.get("a", 1)), _cell(o, "I", case.get("b", 0))]
    elif name == "ground_settrafficbehaviour":
        args = [_cell(o, "I", case.get("a", 5)), _cell(o, "I", case.get("b", 0))]
    elif name == "ground_halttrafficcross":
        args = [_cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0]),
                _cell(o, "F", _fbits(floats[0]))]
    elif name in ("ground_halttrafficpath", "ground_getstartdirection"):
        args = [_cell(o, "L", 0 if case.get("v1_null") or len(vecs) < 1 else vecs[0]),
                _cell(o, "L", 0 if case.get("v2_null") or len(vecs) < 2 else vecs[1])]
    elif name == "ground_getnearestcross":
        args = [_cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0]),
                _cell(o, "F", _fbits(floats[0]))]
    elif name == "ground_getroutelength":
        args = [_cell(o, "L", 0 if case.get("v1_null") or len(vecs) < 1 else vecs[0]),
                _cell(o, "L", 0 if case.get("v2_null") or len(vecs) < 2 else vecs[1])]
    elif name == "ground_alignroad":
        args = [_cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0])]
    elif name == "ground_routepos":
        args = [_cell(o, "F", _fbits(floats[0]))]
    elif name == "ground_routedist":
        args = [_cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0])]
    elif name == "ground_peddensity":
        args = [_cell(o, "F", _fbits(floats[0]))]
    elif name == "ground_addpedtype":
        args = [_cell(o, "L", 0 if case.get("arg_null") else o.alloc(0x40))]
    elif name == "ground_rempedtype":
        args = [_cell(o, "L", 0 if case.get("arg_null") else o.alloc(0x40))]
    elif name == "ground_peddistance":
        args = [_cell(o, "L", 0 if case.get("vec_null") or not vecs else vecs[0]),
                _cell(o, "I", case.get("kind", 1))]
    if case.get("handle_null"):
        w["world"].obj = 0
    frame = _frame(w["world"], sig, args + [_cell(o, "L", 1)], cls=cls)
    res = o.call(addr, [frame], "cdecl", limit=400_000)
    view = {"ret": res.eax,
            "st0": None if res.st0 is None else res.st0_raw.hex(),
            "writes": [[a, sz, v] for (a, sz, v) in res.mem_writes
                       if 0x6400000 <= a <= 0x6500000 or 0x6408D0 <= a <= 0x640900]}
    view["events"] = {k: w[k] for k in ("messages", "field_gets", "field_sets",
                                        "class_loads", "object_allocs",
                                        "engine_calls", "array_sets")}
    view["water_flag"] = o.read_u32(0x6409C4 + 0x80)
    view["route_cache"] = o.read_u32s(0x6408D0, 12)
    return view


def ground_cases(name):
    if name == "ground_setwater_fff":
        return [{"floats": [12.5, 3.0, 1.0]}, {"floats": [0.0, 1.0, 1.0]},
                {"floats": [-1.0, 1.0, 1.0]}, {"floats": [12.5, -0.0, 1.0]},
                {"floats": [12.5, 3.0, 1.0], "handle_null": True}]
    if name == "ground_setwater_vv":
        v = [_fbits(1.0), _fbits(2.0), _fbits(3.0)]
        return [{"vectors": [v, v], "floats": [10.0, 0.5]},
                {"vectors": [v, v], "floats": [0.0, 0.5]},
                {"vectors": [v, v], "floats": [10.0, 0.5], "v1_null": True},
                {"vectors": [v, v], "floats": [10.0, 0.5], "handle_null": True}]
    if name == "ground_addwaterlimit":
        v = [_fbits(4.0), _fbits(-5.0), _fbits(6.0)]
        return [{"vectors": [v, v]}, {"vectors": [v, v], "v1_null": True},
                {"vectors": [v, v], "handle_null": True}]
    if name == "ground_setfog":
        return [{"mode": 2, "floats": [100.0, 0.02]},
                {"mode": 0, "floats": [-3.0, 1.5]},
                {"mode": 2, "floats": [100.0, 0.02], "handle_null": True}]
    if name == "ground_addtrafficcar":
        return [{"controller": 0}, {"controller": 0x9000},
                {"controller": 0, "arg_null": True},
                {"controller": 0, "vec_null": True}]
    if name == "ground_remtrafficcar":
        return [{"channel": 3}, {"channel": 0}]
    if name == "ground_notifytrafficcar":
        return [{"a": 1, "b": 0}, {"a": -1, "b": 0}, {"a": 7, "b": 0}]
    if name == "ground_addtrafficn":
        return [{"count": 2, "floats": [0.1, 0.2, 0.3]},
                {"count": 0, "floats": [0.1, 0.2, 0.3]},
                {"count": 2, "floats": [0.1, 0.2, 0.3], "handle_null": True},
                {"count": -3, "floats": [0.1, 0.2, 0.3]}]
    if name == "ground_addtrafficp":
        v = [_fbits(8.0), _fbits(9.0), _fbits(10.0)]
        return [{"vectors": [v], "count": 1, "floats": [0.4, 0.5, 0.6]},
                {"vectors": [v], "count": 0, "floats": [0.4, 0.5, 0.6]},
                {"vectors": [v], "count": 1, "floats": [0.4, 0.5, 0.6], "vec_null": True},
                {"vectors": [v], "count": 1, "floats": [0.4, 0.5, 0.6], "handle_null": True}]
    if name == "ground_deltraffic":
        return [{}]
    if name == "ground_settrafficbehaviour":
        return [{"a": 5, "b": 0}]
    if name == "ground_halttrafficcross":
        v = [_fbits(1.5), _fbits(2.5), _fbits(3.5)]
        return [{"vectors": [v], "floats": [2.0]}, {"vectors": [v], "floats": [2.0], "vec_null": True}]
    if name == "ground_halttrafficpath":
        v = [_fbits(1.0), _fbits(2.0), _fbits(3.0)]
        return [{"vectors": [v, v]}, {"vectors": [v, v], "v1_null": True},
                {"vectors": [v, v], "v2_null": True}]
    if name == "ground_getnearestcross":
        v = [_fbits(5.0), _fbits(6.0), _fbits(7.0)]
        return [{"vectors": [v], "floats": [10.0]},
                {"vectors": [v], "floats": [10.0], "vec_null": True},
                {"vectors": [v], "floats": [10.0], "handle_null": True}]
    if name == "ground_getstartdirection":
        v = [_fbits(1.0), _fbits(0.0), _fbits(0.0)]
        return [{"vectors": [v, v]}, {"vectors": [v, v], "v1_null": True},
                {"vectors": [v, v], "handle_null": True}]
    if name == "ground_getroutelength":
        v = [_fbits(1.0), _fbits(2.0), _fbits(3.0)]
        w2 = [_fbits(4.0), _fbits(5.0), _fbits(6.0)]
        return [{"vectors": [v, w2]}, {"vectors": [v, w2], "v1_null": True},
                {"vectors": [v, w2], "handle_null": True}]
    if name == "ground_alignroad":
        v = [_fbits(2.0), _fbits(4.0), _fbits(6.0)]
        return [{"vectors": [v]}, {"vectors": [v], "vec_null": True},
                {"vectors": [v], "handle_null": True}]
    if name == "ground_routelength0":
        return [{"route_cache": [0] * 12},
                {"route_cache": [0x2400, _fbits(120.0), 0, 0, _fbits(1.0), _fbits(2.0),
                                 _fbits(3.0), _fbits(9.0), _fbits(8.0), _fbits(7.0), 0, 0]}]
    if name == "ground_routepos":
        return [{"route_cache": [0] * 12},
                {"route_cache": [0x2400, _fbits(120.0), _fbits(0.5), 0, _fbits(10.0),
                                 _fbits(0.0), _fbits(5.0), _fbits(1.0), 0, 0, 0, 0],
                 "floats": [0.25]}]
    if name == "ground_routedist":
        v = [_fbits(10.0), _fbits(0.0), _fbits(5.0)]
        return [{"route_cache": [0] * 12, "vectors": [v]},
                {"route_cache": [0x2400, _fbits(120.0), _fbits(0.5), 0, _fbits(10.0),
                                 _fbits(0.0), _fbits(5.0), _fbits(2.0), 0, 0, 0, 0],
                 "vectors": [v]},
                {"route_cache": [0x2400, 0, _fbits(0.5), 0, _fbits(10.0), _fbits(0.0),
                                 _fbits(5.0), 0, 0, 0, 0, 0], "vectors": [v]}]
    if name == "ground_peddensity":
        return [{"floats": [2.0]}, {"floats": [-1.0]}, {"floats": [2.0], "handle_null": True}]
    if name == "ground_addpedtype":
        return [{}, {"arg_null": True}]
    if name == "ground_rempedtype":
        return [{}, {"arg_null": True}]
    if name == "ground_peddistance":
        v = [_fbits(3.0), _fbits(4.0), _fbits(5.0)]
        return [{"vectors": [v], "kind": 1}, {"vectors": [v], "kind": 1, "vec_null": True},
                {"vectors": [v], "kind": 1, "handle_null": True}]
    return []


RUNNERS = {"phys": run_phys, "part": run_part, "ground": run_ground}
CASES = {"phys": phys_cases, "part": part_cases, "ground": ground_cases}


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    manifest = {"constants": {}, "exe_sha256": __import__("hashlib").sha256(
        open(DEFAULT_EXE, "rb").read()).hexdigest(),
        "unicorn": __import__("unicorn").__version__, "cw": CW,
        "date": datetime.datetime.utcnow().isoformat() + "Z"}
    probe = Oracle()
    for label, addr in list(CONSTANTS.items()):
        raw = probe.read(addr, 96).split(b"\0")[0]
        manifest["constants"][label] = {"addr": addr,
                                        "text": raw.decode("latin1"),
                                        "hex": raw.hex()}
    for f in ("zero", "unit", "fog_scale", "box_scale", "rand_scale",
              "water_default_a", "water_default_b", "trafficn_scale",
              "trafficcar_scale", "trafficn_speed", "ped_density_scale",
              "ped_distance_default"):
        manifest["constants"][f]["bits"] = f"{probe.read_u32(manifest['constants'][f]['addr']):08x}"
    with open(os.path.join(OUT_DIR, "manifest.json"), "w") as fh:
        json.dump(manifest, fh, indent=1)
    for group, runner in RUNNERS.items():
        path = os.path.join(OUT_DIR, f"{group}.jsonl")
        with open(path, "w") as fh:
            for name, (addr, sig, _cls) in NATIVES.items():
                if not name.startswith(group):
                    continue
                for id_, case in enumerate(CASES[group](name)):
                    view = runner(name, case, None)
                    row = {"id": id_, "name": name, "exe_addr": f"0x{addr:X}",
                           "sig": sig.decode(), "args": case, "result": view,
                           "oracle": "unicorn", "cw": CW,
                           "date": manifest["date"]}
                    fh.write(json.dumps(row, sort_keys=True) + "\n")
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
