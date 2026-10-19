#!/usr/bin/env python3
"""Unicorn vectors for the `java.util.resource.ResourceRef` rows (M1-10).

Calls the exe's natives directly with a hand-built `CallInfo` frame (no JVM
boot, no game data), the same shape `res_vectors.py` uses for the M1-11 handle
classes. The receiver's `obj` field (`0x42ab50`) is hooked to answer a node
word the probe allocated, so the real entry points run and the probe observes
what each one does to the node/container words:

  0x47cea0 newNative          0x47cf00 deleteNative     0x47cf80 set(I)
  0x47cfc0 set(ResourceRef)   0x47d0a0 load             0x47d170 unload
  0x47d1e0 destroy            0x47d210 type             0x47d290 id
  0x47d3e0 getParentID        0x47d4b0 getParent        0x47d570 getWTRoot
  0x47d630 getFirstChild      0x47d6f0 getNextChild     0x47ffe0 makeTexture
  0x480100 makeSound          0x47e8d0 cache            0x47e950 precache
  0x4802c0 duplicate          0x480390 scaleMesh

Hook boundaries (everything else is real exe code, including `0x45d910`
marshalling and the inlined insert/unlink sequences):

- `0x536820` (the manager lookup), `0x539170`/`0x539230` (the texture/sound
  factories), `0x48a920` (the cache helpers), `0x48d860` (duplicate),
  `0x48e7f0` (the mesh scaler), `0x48a8f0`/`0x48a8d0` (unload/destroy),
  `0x47d300` (the wrapper factory) and `0x54f560`/`0x54f5b0` (the node
  allocator) are recorded and served from probe-owned memory.
- the `R` node and `X` container are laid out exactly as
  `slr-notes/handoff-M1-10.md` §3.1 describes: `R = {prev, next, id,
  container}` (0x10 bytes), `X` with `+0x14 parent`, `+0x20 first child`,
  `+0x48` handle-list head, `+0x4c` type, `+0x50` id, `+0x54` flags,
  `+0xcc` WT root.

Row: `resnode.jsonl` under `re/out/res/`, git-ignored.
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
OUT_DIR = os.path.join(REPO, "re", "out", "res")

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}

NEW_NATIVE = 0x47CEA0
DELETE_NATIVE = 0x47CF00
SET_ID = 0x47CF80
SET_REF = 0x47CFC0
LOAD = 0x47D0A0
UNLOAD = 0x47D170
DESTROY = 0x47D1E0
TYPE_ID = 0x47D210
ID = 0x47D290
GET_PARENT_ID = 0x47D3E0
GET_PARENT = 0x47D4B0
GET_WT_ROOT = 0x47D570
GET_FIRST_CHILD = 0x47D630
GET_NEXT_CHILD = 0x47D6F0
MAKE_TEXTURE = 0x47FFE0
MAKE_SOUND = 0x480100
CACHE = 0x47E8D0
PRECACHE = 0x47E950
DUPLICATE = 0x4802C0
SCALE_MESH = 0x480390
GET_FLAGS = 0x47DF40
SET_FLAGS = 0x486C80
CLEAR_FLAGS = 0x486CD0
SET_ACTIVE_COLLISION = 0x47DF80
GET_SCRIPT_INSTANCE = 0x486F30
GET_DETAIL = 0x480440

ROWS = {
    NEW_NATIVE: ("java.util.resource.ResourceRef", "newNative", "()V"),
    DELETE_NATIVE: ("java.util.resource.ResourceRef", "deleteNative", "()V"),
    SET_ID: ("java.util.resource.ResourceRef", "set", "(I)V"),
    SET_REF: (
        "java.util.resource.ResourceRef",
        "set",
        "(Ljava.util.resource.ResourceRef;)V",
    ),
    LOAD: ("java.util.resource.ResourceRef", "load", "()V"),
    UNLOAD: ("java.util.resource.ResourceRef", "unload", "()V"),
    DESTROY: ("java.util.resource.ResourceRef", "destroy", "()V"),
    TYPE_ID: ("java.util.resource.ResourceRef", "type", "()I"),
    ID: ("java.util.resource.ResourceRef", "id", "()I"),
    GET_PARENT_ID: ("java.util.resource.ResourceRef", "getParentID", "()I"),
    GET_PARENT: (
        "java.util.resource.ResourceRef",
        "getParent",
        "()Ljava.util.resource.ResourceRef;",
    ),
    GET_WT_ROOT: (
        "java.util.resource.ResourceRef",
        "getWTRoot",
        "()Ljava.util.resource.ResourceRef;",
    ),
    GET_FIRST_CHILD: (
        "java.util.resource.ResourceRef",
        "getFirstChild",
        "()Ljava.util.resource.ResourceRef;",
    ),
    GET_NEXT_CHILD: (
        "java.util.resource.ResourceRef",
        "getNextChild",
        "()Ljava.util.resource.ResourceRef;",
    ),
    MAKE_TEXTURE: (
        "java.util.resource.ResourceRef",
        "makeTexture",
        "(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V",
    ),
    MAKE_SOUND: (
        "java.util.resource.ResourceRef",
        "makeSound",
        "(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V",
    ),
    CACHE: ("java.util.resource.ResourceRef", "cache", "()V"),
    PRECACHE: ("java.util.resource.ResourceRef", "precache", "()V"),
    DUPLICATE: (
        "java.util.resource.ResourceRef",
        "duplicate",
        "(Ljava.util.resource.ResourceRef;)V",
    ),
    SCALE_MESH: ("java.util.resource.ResourceRef", "scaleMesh", "(FFF)V"),
    GET_FLAGS: ("java.util.resource.GameRef", "getFlags", "()I"),
    SET_FLAGS: ("java.util.resource.GameRef", "setFlags", "(I)V"),
    CLEAR_FLAGS: ("java.util.resource.GameRef", "clearFlags", "(I)V"),
    SET_ACTIVE_COLLISION: (
        "java.util.resource.GameRef",
        "setActiveCollision",
        "()V",
    ),
    GET_SCRIPT_INSTANCE: (
        "java.util.resource.GameRef",
        "getScriptInstance",
        "()Ljava.lang.Object;",
    ),
    GET_DETAIL: ("java.util.resource.GameRef", "getDetail", "()F"),
}

# the engine tails, recorded instead of executed
LOOKUP = 0x536820
WRAP = 0x47D300
ALLOC = 0x54F560
FREE = 0x54F5B0
TEXTURE_FACTORY = 0x539170
SOUND_FACTORY = 0x539230
CACHE_CALL = 0x48A920
DUPLICATE_CALL = 0x48D860
SCALE_CALL = 0x48E7F0
UNLOAD_CALL = 0x48A8F0
DESTROY_CALL = 0x48A8D0
PARENT_CALL = 0x48AAF0
WT_CALL = 0x48AA70
CHILD_CALL = 0x48D230
NEXT_CALL = 0x48D300


def _default_exe():
    for cand in (
        os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe"),
        os.path.join(MAIN_CHECKOUT, "reference", "raw", "StreetLegal_Redline.bak.exe"),
    ):
        if os.path.exists(cand):
            return cand
    raise SystemExit("stock exe not found; pass --exe")


def _container(o, cells):
    data = o.alloc(max(4 * len(cells), 4))
    for i, cell in enumerate(cells):
        o.write_u32(data + 4 * i, cell)
    box = o.alloc(0xC)
    o.write_u32(box, len(cells))
    o.write_u32(box + 4, data)
    o.write_u32(box + 8, len(cells))
    return box


def _alloc_cstr(o, text):
    buf = o.alloc(len(text) + 1)
    o.write(buf, text + b"\x00")
    return buf


def _cell(o, ty, value):
    cell = o.alloc(0x10)
    o.write_u32(cell, VAL_VT)
    o.write_u32(cell + 4, TC.get(ty, TC["I"]))
    o.write_u32(cell + 8, value & 0xFFFFFFFF)
    return cell


def _frame(o, cls, name, sig, cells):
    stack = _container(o, cells)
    sigp = _alloc_cstr(o, sig.encode())
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


class World:
    """One fresh Oracle with the node/container words planted and the engine
    tails hooked."""

    def __init__(
        self,
        exe=None,
        node=True,
        node_id=0,
        container=None,
        parent=None,
        wt_root=None,
        first_child=None,
        ground_precache=0,
    ):
        self.o = Oracle(exe or _default_exe())
        self.events = []
        o = self.o
        o.hook_alloc(ALLOC, FREE)
        self.node = 0
        self.container = 0
        if node:
            self.node = o.alloc(0x10)
            o.write_u32(self.node + 8, node_id & 0xFFFFFFFF)
        if container is not None:
            self.container = o.alloc(0x120)
            o.write_u32(self.container + 0x14, parent or 0)
            o.write_u32(self.container + 0x20, first_child or 0)
            o.write_u32(self.container + 0x4C, container & 0xFFFFFFFF)
            o.write_u32(self.container + 0x50, node_id & 0xFFFFFFFF)
            o.write_u32(self.container + 0xCC, wt_root or 0)
            if self.node:
                o.write_u32(self.node + 0xC, self.container)
        # the field accessor answers the probe's node
        o.hook(0x42AB50, lambda oc, a: self.node, "cdecl", 2)
        o.hook(0x42A9E0, self._h_set_field, "cdecl", 3)
        o.hook(0x42A690, lambda oc, a: 0, "cdecl", 3)
        o.hook(LOOKUP, self._h_lookup, "thiscall", 3)
        o.hook(WRAP, self._h_wrap, "cdecl", 2)
        o.hook(TEXTURE_FACTORY, self._h_factory, "thiscall", 2)
        o.hook(SOUND_FACTORY, self._h_factory, "thiscall", 2)
        o.hook(CACHE_CALL, self._h_generic, "thiscall", 3)
        o.hook(DUPLICATE_CALL, self._h_generic, "thiscall", 2)
        o.hook(SCALE_CALL, self._h_generic, "thiscall", 2)
        o.hook(UNLOAD_CALL, self._h_generic, "thiscall", 1)
        o.hook(DESTROY_CALL, self._h_generic, "thiscall", 1)
        o.hook(PARENT_CALL, self._h_parent, "thiscall", 1)
        o.hook(WT_CALL, self._h_wt, "thiscall", 1)
        o.hook(CHILD_CALL, self._h_child, "cdecl", 2)
        o.hook(NEXT_CALL, self._h_next, "cdecl", 2)
        # the bitset gate `0x5447d0(X, a, b, mask)` and the error-report calls:
        # answered "not refused" / "nothing", which is the port's arm too
        o.hook(0x5447D0, lambda oc, a: 0, "cdecl", 4)
        o.hook(0x551140, lambda oc, a: None, "cdecl", 1)
        o.hook(0x551330, lambda oc, a: None, "cdecl", 2)
        o.hook(0x5513B0, lambda oc, a: None, "cdecl", 1)
        o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
        o.hook(0x426170, lambda oc, a: self.ground_precache, "cdecl", 1)
        self.ground_precache = ground_precache
        # a real allocation for any caller-supplied object word (the probe's
        # arena is 0x20000000; a bare 0x60000000 is unmapped)
        self.dummy = o.alloc(0x40)

    def _record(self, name, oc, args):
        self.events.append([name] + [a & 0xFFFFFFFF for a in args])
        return None

    def _h_lookup(self, oc, a):
        return self._record("lookup", oc, a)

    def _h_wrap(self, oc, a):
        self.events.append(["wrap", a[1] & 0xFFFFFFFF])
        return 0x7000

    def _h_factory(self, oc, a):
        return self._record("make", oc, a)

    def _h_generic(self, oc, a):
        return self._record("call", oc, a)

    def _h_parent(self, oc, a):
        # 0x48aaf0: thiscall (ecx = node), the link is the stack word
        node, link = oc.reg("ecx"), a[0]
        # `0x48aaf0` gates on `[node+8]` and `[node+0xc]`: a null node or a
        # null container answers 0 and writes nothing
        if not node or not self.o.read_u32(node + 8):
            self.events.append(["parent", 0])
            return 0
        cont = self.o.read_u32(node + 0xC)
        if not cont:
            self.events.append(["parent", 0])
            return 0
        parent = self.o.read_u32(cont + 0x14)
        self.o.write_u32(link, parent)
        self.events.append(["parent", parent])
        return 1

    def _h_wt(self, oc, a):
        node, link = oc.reg("ecx"), a[0]
        if not node:
            self.events.append(["wt", 0])
            return 0
        cont = self.o.read_u32(node + 0xC)
        if not cont:
            self.events.append(["wt", 0])
            return 0
        wt = self.o.read_u32(cont + 0xCC)
        self.o.write_u32(link, wt)
        self.events.append(["wt", wt])
        return 1

    def _h_child(self, oc, a):
        node, link = a  # cdecl (node, link)
        if not node:
            self.events.append(["child", 0])
            return 0
        cont = self.o.read_u32(node + 0xC)
        if not cont:
            self.events.append(["child", 0])
            return 0
        child = self.o.read_u32(cont + 0x20)
        self.o.write_u32(link, child)
        self.events.append(["child", child])
        return 1

    def _h_next(self, oc, a):
        node, link = a  # cdecl (node, link)
        if not node:
            self.events.append(["next", 0])
            return 0
        cont = self.o.read_u32(node + 0xC)
        if not cont:
            self.events.append(["next", 0])
            return 0
        nxt = self.o.read_u32(cont + 4)
        self.o.write_u32(link, nxt)
        self.events.append(["next", nxt])
        return 1

    def _h_set_field(self, oc, a):
        self.events.append(["set_field", a[2] & 0xFFFFFFFF])
        return None


def run_resnode(case, exe=None):
    _id, spec = case
    world = {k: v for k, v in spec.items() if k not in ("addr", "params")}
    w = World(exe, **world)
    o = w.o
    addr = spec["addr"]
    cls, name, sig = ROWS[addr]
    params = spec.get("params", [])
    # caller-supplied object words become real allocations
    remap = {0x900: w.dummy, 0xA00: w.dummy, 0xB00: w.dummy}
    params = [(ty, remap.get(word, word)) for ty, word in params]
    cells = [_cell(o, ty, word) for ty, word in reversed(params)]
    cells.append(_cell(o, "L", 0))
    frame = _frame(o, cls, name, sig, cells)
    err, eax, st0 = None, None, None
    try:
        res = o.call(addr, [frame], "cdecl", limit=500_000)
        eax = res.eax & 0xFFFFFFFF
        st0 = res.st0 if isinstance(res.st0, float) else None
    except Exception as exc:  # noqa: BLE001 - the row records the failure
        err = type(exc).__name__
    node = w.node
    result = {
        "eax": eax,
        "st0": st0,
        "events": list(w.events),
        "node": {
            "exists": node != 0 and o.read_u32(node) != 0xFFFFFFFF,
            "id": o.read_u32(node + 8) if node else 0,
            "container": o.read_u32(node + 0xC) if node else 0,
            "prev": o.read_u32(node) if node else 0,
            "next": o.read_u32(node + 4) if node else 0,
        },
        "container": {
            "head": o.read_u32(w.container + 0x48) if w.container else 0,
            "flags": o.read_u32(w.container + 0x54) if w.container else 0,
        },
        "error": err,
    }
    return result


def resnode_cases():
    return [
        ("new_native_no_field", {"addr": NEW_NATIVE, "params": [], "node": False}),
        ("new_native_has_field", {"addr": NEW_NATIVE, "params": []}),
        ("delete_native_no_field", {"addr": DELETE_NATIVE, "params": [], "node": False}),
        ("delete_native_unbound", {"addr": DELETE_NATIVE, "params": []}),
        (
            "delete_native_bound",
            {"addr": DELETE_NATIVE, "params": [], "node_id": 0x1234, "container": 1},
        ),
        ("set_id_no_field", {"addr": SET_ID, "params": [("I", 7)], "node": False}),
        ("set_id_same", {"addr": SET_ID, "params": [("I", 0)], "node_id": 0}),
        ("set_id_miss", {"addr": SET_ID, "params": [("I", 0xBEEF)]}),
        ("set_ref_zero", {"addr": SET_REF, "params": [("L", 0)]}),
        ("set_ref_no_field", {"addr": SET_REF, "params": [("L", 0)], "node": False}),
        ("load_no_field", {"addr": LOAD, "params": [], "node": False}),
        (
            "load_unbound",
            {"addr": LOAD, "params": []},
        ),
        (
            "load_typed",
            {"addr": LOAD, "params": [], "node_id": 5, "container": 3},
        ),
        (
            "load_precache_flag",
            {"addr": LOAD, "params": [], "node_id": 5, "container": 3, "ground_precache": 2},
        ),
        (
            "load_type_one_skips_materialise",
            {"addr": LOAD, "params": [], "node_id": 5, "container": 1},
        ),
        ("unload_no_field", {"addr": UNLOAD, "params": [], "node": False}),
        ("unload_unbound", {"addr": UNLOAD, "params": []}),
        (
            "unload_bound",
            {"addr": UNLOAD, "params": [], "node_id": 9, "container": 1},
        ),
        ("destroy_no_field", {"addr": DESTROY, "params": [], "node": False}),
        (
            "destroy_bound",
            {"addr": DESTROY, "params": [], "node_id": 9, "container": 1},
        ),
        ("type_no_field", {"addr": TYPE_ID, "params": [], "node": False}),
        ("type_unbound", {"addr": TYPE_ID, "params": []}),
        (
            "type_typed",
            {"addr": TYPE_ID, "params": [], "node_id": 5, "container": 0x0E},
        ),
        ("id_no_field", {"addr": ID, "params": [], "node": False}),
        ("id_bound", {"addr": ID, "params": [], "node_id": 0x4242}),
        ("parent_id_no_field", {"addr": GET_PARENT_ID, "params": [], "node": False}),
        ("parent_id_unbound", {"addr": GET_PARENT_ID, "params": []}),
        (
            "parent_id_bound",
            {"addr": GET_PARENT_ID, "params": [], "node_id": 5, "container": 3},
        ),
        ("parent_no_field", {"addr": GET_PARENT, "params": [], "node": False}),
        ("parent_unbound", {"addr": GET_PARENT, "params": []}),
        (
            "parent_bound",
            {"addr": GET_PARENT, "params": [], "node_id": 5, "container": 3, "parent": 0x900},
        ),
        ("wt_root_unbound", {"addr": GET_WT_ROOT, "params": []}),
        (
            "wt_root_bound",
            {"addr": GET_WT_ROOT, "params": [], "node_id": 5, "container": 3, "wt_root": 0xA00},
        ),
        ("first_child_unbound", {"addr": GET_FIRST_CHILD, "params": []}),
        (
            "first_child_bound",
            {
                "addr": GET_FIRST_CHILD,
                "params": [],
                "node_id": 5,
                "container": 3,
                "first_child": 0xB00,
            },
        ),
        ("next_child_unbound", {"addr": GET_NEXT_CHILD, "params": []}),
        (
            "make_texture_no_field",
            {"addr": MAKE_TEXTURE, "params": [("L", 0x900), ("L", 0)], "node": False},
        ),
        ("make_texture_null_name", {"addr": MAKE_TEXTURE, "params": [("L", 0x900), ("L", 0)]}),
        ("make_sound_null_name", {"addr": MAKE_SOUND, "params": [("L", 0x900), ("L", 0)]}),
        ("cache_no_field", {"addr": CACHE, "params": [], "node": False}),
        ("cache_unbound", {"addr": CACHE, "params": []}),
        ("precache_no_field", {"addr": PRECACHE, "params": [], "node": False}),
        ("precache_unbound", {"addr": PRECACHE, "params": []}),
        ("duplicate_no_field", {"addr": DUPLICATE, "params": [("L", 0x900)], "node": False}),
        ("duplicate_zero", {"addr": DUPLICATE, "params": [("L", 0)]}),
        (
            "duplicate_other",
            {"addr": DUPLICATE, "params": [("L", 0x900)]},
        ),
        ("scale_mesh_no_field", {"addr": SCALE_MESH, "params": [], "node": False}),
        ("get_flags_no_field", {"addr": GET_FLAGS, "params": [], "node": False}),
        ("get_flags_unbound", {"addr": GET_FLAGS, "params": []}),
        (
            "get_flags_bound",
            {"addr": GET_FLAGS, "params": [], "node_id": 5, "container": 1},
        ),
        ("set_flags_no_container", {"addr": SET_FLAGS, "params": [("I", 0x10)]}),
        (
            "set_flags_recolour",
            {"addr": SET_FLAGS, "params": [("I", 0x10)], "node_id": 5, "container": 1},
        ),
        (
            "set_flags_plain",
            {"addr": SET_FLAGS, "params": [("I", 0x400)], "node_id": 5, "container": 1},
        ),
        ("clear_flags_no_container", {"addr": CLEAR_FLAGS, "params": [("I", 0xF0)]}),
        (
            "clear_flags_bound",
            {"addr": CLEAR_FLAGS, "params": [("I", 0xF0)], "node_id": 5, "container": 1},
        ),
        (
            "set_active_collision_no_field",
            {"addr": SET_ACTIVE_COLLISION, "params": [], "node": False},
        ),
        (
            "set_active_collision_bound",
            {"addr": SET_ACTIVE_COLLISION, "params": [], "node_id": 5, "container": 1},
        ),
        (
            "get_script_instance_unbound",
            {"addr": GET_SCRIPT_INSTANCE, "params": []},
        ),
        (
            "get_script_instance_game",
            {"addr": GET_SCRIPT_INSTANCE, "params": [], "node_id": 5, "container": 1},
        ),
        (
            "get_script_instance_render",
            {"addr": GET_SCRIPT_INSTANCE, "params": [], "node_id": 5, "container": 3},
        ),
        ("get_detail_unbound", {"addr": GET_DETAIL, "params": []}),
        (
            "get_detail_game",
            {"addr": GET_DETAIL, "params": [], "node_id": 5, "container": 1},
        ),
        (
            "get_detail_render",
            {"addr": GET_DETAIL, "params": [], "node_id": 5, "container": 3},
        ),
        (
            "get_detail_other",
            {"addr": GET_DETAIL, "params": [], "node_id": 5, "container": 9},
        ),
        (
            "scale_mesh_unbound",
            {
                "addr": SCALE_MESH,
                "params": [("F", 0x40000000), ("F", 0x40000000), ("F", 0x40000000)],
            },
        ),
    ]


def resnode_rows_list(exe=None, only=None):
    rows = []
    for case_id, spec in resnode_cases():
        if only and only not in case_id:
            continue
        result = run_resnode((case_id, spec), exe)
        addr = spec["addr"]
        cls, name, sig = ROWS[addr]
        rows.append(
            {
                "id": case_id,
                "exe_addr": addr,
                "class": cls,
                "name": name,
                "sig": sig,
                "args": {"params": [[ty, word] for ty, word in spec.get("params", [])]},
                "world": {k: v for k, v in spec.items() if k not in ("addr", "params")},
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
    rows = resnode_rows_list(exe, only)
    if not rows:
        return
    path = os.path.join(OUT_DIR, "resnode.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def probe(exe=None, only=None):
    for r in resnode_rows_list(exe, only):
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
