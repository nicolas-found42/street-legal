#!/usr/bin/env python3
"""Unicorn vectors for the `java.render.*` rows ported by M1-12.

Calls the exe's natives directly with a hand-built `CallInfo` frame (no JVM
boot, no game data). The rows here are the ones whose whole body is global-word
traffic, so the probe plants the globals and lets the real code run:

  0x47c440 `GfxEngine.numDisplayModes()I`         0x47c450 `currDisplayMode()I`
  0x47c460 `displayModeName(I)String`             0x47c3f0 `changeVideoMode(III)V`
  0x47c1d0 `forceRendering()V`                    0x47c2d0 `flush()V`
  0x487020 `Text.setDefaultColor(I)V`             0x487040 `Text.getDefaultColor()I`

Hook boundaries (everything else is real exe code, including `0x45d910`
marshalling and the display-mode helpers `0x4b9ef0`/`0x4b9ea0`/`0x4b9f00`):

- `0x4174a0` the intern call: recorded, answered with an arena word;
- `0x5378d0`/`0x537b40`/`0x4fca30` (the render flush sequence behind
  `forceRendering`/`flush`) and the `[device+0x204]` vtable behind
  `changeVideoMode`: recorded, so a row carries the calls the port models as
  events. Both flush rows loop their sequence **seven** times
  (`mov esi,7` / `dec esi; jne`), so one call bumps the frame counter by
  seven. `flush` additionally records `0x537ed0(1)` and `0x4b7b90()`.
  Recording hooks use the instructions' actual stack arity and return
  normally; faulting rows are rejected by the replay tests.
- the mode table is laid out in the probe's own memory exactly as
  `0x4b9ea0` reads it (`0x14`-byte entries, the three words at `+0`/`+4`/`+8`),
  and the current descriptor is a real allocation whose `+0x1bc..+0x1c0`
  `WORD`s the probe writes.

Row: `render.jsonl` under `re/out/render/`, git-ignored.
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
OUT_DIR = os.path.join(REPO, "re", "out", "render")

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}

MODE_COUNT = 0x6495F4
MODE_DESC = 0x6495DC
MODE_TABLE = 0x6466A4
MODE_INDEX = 0x64A394
TEXT_COLOR = 0x612C64
DIRTY = 0x65AECC
FRAME = 0x6200A4

NUM_DISPLAY_MODES = 0x47C440
CURR_DISPLAY_MODE = 0x47C450
DISPLAY_MODE_NAME = 0x47C460
CHANGE_VIDEO_MODE = 0x47C3F0
FORCE_RENDERING = 0x47C1D0
FLUSH = 0x47C2D0
TEXT_SET_DEFAULT_COLOR = 0x487020
TEXT_GET_DEFAULT_COLOR = 0x487040
IS_PLAYING_VIDEO = 0x47C390
VIEWPORT_ACTIVATE = 0x481680
VIEWPORT_DEACTIVATE = 0x4816C0
VIEWPORT_GET_ASPECT = 0x4817B0
VIEWPORT_GET_WIDTH = 0x4817F0
VIEWPORT_GET_HEIGHT = 0x481870
VIEWPORT_GET_TOP = 0x4818F0
VIEWPORT_GET_LEFT = 0x481970
VIEWPORT_RESIZE = 0x481700
VIDEO_WORD = 0x64A394

ROWS = {
    NUM_DISPLAY_MODES: ("java.render.GfxEngine", "numDisplayModes", "()I"),
    CURR_DISPLAY_MODE: ("java.render.GfxEngine", "currDisplayMode", "()I"),
    DISPLAY_MODE_NAME: (
        "java.render.GfxEngine",
        "displayModeName",
        "(I)Ljava.lang.String;",
    ),
    CHANGE_VIDEO_MODE: ("java.render.GfxEngine", "changeVideoMode", "(III)V"),
    FORCE_RENDERING: ("java.render.GfxEngine", "forceRendering", "()V"),
    FLUSH: ("java.render.GfxEngine", "flush", "()V"),
    TEXT_SET_DEFAULT_COLOR: ("java.render.Text", "setDefaultColor", "(I)V"),
    TEXT_GET_DEFAULT_COLOR: ("java.render.Text", "getDefaultColor", "()I"),
    IS_PLAYING_VIDEO: ("java.render.GfxEngine", "isPlayingVideo", "()I"),
    VIEWPORT_ACTIVATE: ("java.render.Viewport", "activate", "(I)V"),
    VIEWPORT_DEACTIVATE: ("java.render.Viewport", "deactivate", "()V"),
    VIEWPORT_GET_ASPECT: ("java.render.Viewport", "getAspect", "()F"),
    VIEWPORT_GET_WIDTH: ("java.render.Viewport", "getWidth", "()F"),
    VIEWPORT_GET_HEIGHT: ("java.render.Viewport", "getHeight", "()F"),
    VIEWPORT_GET_TOP: ("java.render.Viewport", "getTop", "()F"),
    VIEWPORT_GET_LEFT: ("java.render.Viewport", "getLeft", "()F"),
    VIEWPORT_RESIZE: ("java.render.Viewport", "resize", "(FFFF)V"),
}


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _alloc_cstr(o, s: bytes) -> int:
    p = o.alloc(len(s) + 16)
    o.write(p, s + b"\0")
    return p


def _cell(o, ty: str, w: int) -> int:
    p = o.alloc(12)
    o.write_u32(p, VAL_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    return p


def _container(o, words: list) -> int:
    c = o.alloc(12)
    data = o.alloc(max(4 * len(words), 4))
    for i, w in enumerate(words):
        o.write_u32(data + 4 * i, w)
    o.write_u32(c, len(words))
    o.write_u32(c + 4, data)
    o.write_u32(c + 8, len(words))
    return c


def _frame(o, cls: str, name: str, sig: str, cells: list) -> int:
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
    """One fresh Oracle with the render globals planted and the boundaries
    hooked."""

    def __init__(
        self,
        exe=None,
        modes=(),
        current=None,
        text_color=-1,
        video_playing=0,
        dirty=0,
        frame=0,
    ):
        self.o = Oracle(exe or _default_exe())
        self.events: list = []
        self.interns: list = []
        o = self.o
        o.hook_alloc()
        # the mode table, laid out as 0x4b9ea0 reads it
        table = o.alloc(max(0x14 * len(modes), 0x14))
        for i, entry in enumerate(modes):
            for j, word in enumerate(entry):
                o.write_u32(table + i * 0x14 + 4 * j, word & 0xFFFFFFFF)
        o.write_u32(MODE_COUNT, len(modes))
        o.write_u32(MODE_TABLE, table)
        desc = o.alloc(0x1C4)
        o.write_u32(MODE_DESC, desc)
        for off, word in zip((0x1BC, 0x1BE, 0x1C0), current or (0, 0, 0)):
            o.write_u32(desc + off, word & 0xFFFFFFFF)
        o.write_u32(TEXT_COLOR, text_color & 0xFFFFFFFF)
        o.write_u32(DIRTY, dirty & 0xFFFFFFFF)
        o.write_u32(FRAME, frame & 0xFFFFFFFF)
        o.write_u32(MODE_INDEX, 0)
        o.write_u32(VIDEO_WORD, video_playing & 0xFFFFFFFF)
        # the receiver-field accessor: these rows take their "no engine
        # envelope" arm when `0x42ab50` misses, which is every port run
        o.hook(0x42AB50, lambda oc, a: 0, "cdecl", 2)
        o.hook(0x42A9E0, lambda oc, a: None, "cdecl", 3)
        o.hook(0x42A690, lambda oc, a: 0, "cdecl", 3)
        # boundaries
        o.hook(0x4174A0, self._h_intern, "thiscall", 1)
        o.hook(0x5378D0, self._h_select, "thiscall", 0)
        o.hook(0x537B40, self._h_flush, "thiscall", 0)
        o.hook(0x4FCA30, self._h_device_flush, "thiscall", 0)
        o.hook(0x537ED0, lambda oc, a: self.events.append(["manager_finish", a[0]]), "thiscall", 1)
        o.hook(0x4B7B90, lambda oc, a: self.events.append(["device_sync"]), "cdecl", 0)
        # `[device+0x204]`'s vtable: the probe plants a device whose vtable
        # slot the rows can call without a D3D9 device behind it.
        device = o.alloc(0x300)
        vtable = o.alloc(0x40)
        stub = o.alloc(0x10)
        o.write(stub, b"\x31\xc0\xc2\x0c\x00")  # xor eax,eax; ret 0xc
        mode_device = o.alloc(0x20)
        o.write_u32(mode_device, vtable)
        o.write_u32(device + 0x204, mode_device)
        o.write_u32(vtable + 0x14, stub)
        o.hook(stub, lambda oc, a: self.events.append(["change_video_mode", *a]), "thiscall", 3)
        o.write_u32(0x6187B0, device)

    def _h_intern(self, oc, a):
        (buf,) = a
        text = oc.read_cstr(buf).decode()
        self.interns.append(text)
        return 0x1000 + len(self.interns)

    def _h_select(self, oc, a):
        self.events.append(["manager_select"])
        return None

    def _h_flush(self, oc, a):
        self.events.append(["manager_flush"])
        return None

    def _h_device_flush(self, oc, a):
        self.events.append(["device_flush"])
        return None


def run_render(case, exe=None):
    """One row: drive the native and report its answer plus the globals."""
    _id, spec = case
    w = World(exe, **spec.get("world", {}))
    o = w.o
    addr = spec["addr"]
    cls, name, sig = ROWS[addr]
    params = spec.get("params", [])
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
    result = {
        "eax": eax,
        "st0": st0,
        "interns": list(w.interns),
        "events": list(w.events),
        "globals": {
            "mode_count": o.read_u32(MODE_COUNT),
            "text_color": o.read_u32(TEXT_COLOR),
            "dirty": o.read_u32(DIRTY),
            "frame": o.read_u32(FRAME),
            "mode_index": o.read_u32(MODE_INDEX),
        },
        "error": err,
    }
    return result, w.events, {}


def render_cases():
    return [
        (
            "num_display_modes_empty",
            {"addr": NUM_DISPLAY_MODES, "params": []},
        ),
        (
            "num_display_modes_two",
            {
                "addr": NUM_DISPLAY_MODES,
                "params": [],
                "world": {"modes": [[640, 480, 16], [800, 600, 32]]},
            },
        ),
        (
            "curr_display_mode_empty",
            {"addr": CURR_DISPLAY_MODE, "params": []},
        ),
        (
            "curr_display_mode_match",
            {
                "addr": CURR_DISPLAY_MODE,
                "params": [],
                "world": {
                    "modes": [[640, 480, 16], [800, 600, 32], [1024, 768, 32]],
                    "current": [800, 600, 32],
                },
            },
        ),
        (
            "curr_display_mode_no_match",
            {
                "addr": CURR_DISPLAY_MODE,
                "params": [],
                "world": {
                    "modes": [[640, 480, 16], [800, 600, 32]],
                    "current": [123, 456, 78],
                },
            },
        ),
        (
            "display_mode_name_first",
            {
                "addr": DISPLAY_MODE_NAME,
                "params": [("I", 0)],
                "world": {"modes": [[800, 600, 32]]},
            },
        ),
        (
            "display_mode_name_out_of_range",
            {
                "addr": DISPLAY_MODE_NAME,
                "params": [("I", 9)],
                "world": {"modes": [[800, 600, 32]]},
            },
        ),
        (
            "change_video_mode_three_words",
            {"addr": CHANGE_VIDEO_MODE, "params": [("I", 1), ("I", 2), ("I", 3)]},
        ),
        (
            "force_rendering_clears_and_bumps",
            {"addr": FORCE_RENDERING, "params": [], "world": {"dirty": 0xDEAD, "frame": 7}},
        ),
        (
            "flush_clears_and_bumps",
            {"addr": FLUSH, "params": [], "world": {"dirty": 0xDEAD, "frame": 7}},
        ),
        (
            "text_set_default_color",
            {"addr": TEXT_SET_DEFAULT_COLOR, "params": [("I", 0x11223344)]},
        ),
        (
            "text_get_default_color",
            {"addr": TEXT_GET_DEFAULT_COLOR, "params": []},
        ),
        (
            "is_playing_video_idle",
            {"addr": IS_PLAYING_VIDEO, "params": []},
        ),
        (
            "is_playing_video_running",
            {"addr": IS_PLAYING_VIDEO, "params": [], "world": {"video_playing": 1}},
        ),
        (
            "viewport_activate_no_envelope",
            {"addr": VIEWPORT_ACTIVATE, "params": [("I", 0)]},
        ),
        (
            "viewport_deactivate_no_envelope",
            {"addr": VIEWPORT_DEACTIVATE, "params": []},
        ),
        (
            "viewport_get_aspect_no_envelope",
            {"addr": VIEWPORT_GET_ASPECT, "params": []},
        ),
        (
            "viewport_get_width_no_envelope",
            {"addr": VIEWPORT_GET_WIDTH, "params": []},
        ),
        (
            "viewport_get_height_no_envelope",
            {"addr": VIEWPORT_GET_HEIGHT, "params": []},
        ),
        (
            "viewport_get_top_no_envelope",
            {"addr": VIEWPORT_GET_TOP, "params": []},
        ),
        (
            "viewport_get_left_no_envelope",
            {"addr": VIEWPORT_GET_LEFT, "params": []},
        ),
        (
            "viewport_resize_no_envelope",
            {
                "addr": VIEWPORT_RESIZE,
                "params": [("F", 0x3F800000), ("F", 0x40000000), ("F", 0x40400000), ("F", 0x40800000)],
            },
        ),
    ]


def render_rows_list(exe=None, only=None):
    rows = []
    for case_id, spec in render_cases():
        if only and only not in case_id:
            continue
        result, events, details = run_render((case_id, spec), exe)
        addr = spec["addr"]
        cls, name, sig = ROWS[addr]
        rows.append(
            {
                "id": case_id,
                "exe_addr": addr,
                "class": cls,
                "name": name,
                "sig": sig,
                "args": {
                    "params": [[ty, word] for ty, word in spec.get("params", [])],
                    "world": spec.get("world", {}),
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


def _today() -> str:
    import datetime

    return datetime.date.today().isoformat()


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = render_rows_list(exe, only)
    if not rows:
        return
    path = os.path.join(OUT_DIR, "render.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def probe(exe=None, only=None):
    for r in render_rows_list(exe, only):
        print(f"== {r['id']} {r['name']} {r['sig']}")
        print("   ", r["result"])


def verify(exe=None, only=None):
    path = os.path.join(OUT_DIR, "render.jsonl")
    checked = 0
    with open(path) as source:
        for line in source:
            row = json.loads(line)
            if only and only not in row["id"]:
                continue
            spec = {"addr": row["exe_addr"], "params": row["args"]["params"], "world": row["args"]["world"]}
            actual, _, _ = run_render((row["id"], spec), exe)
            if actual["error"] is not None or row["result"]["error"] is not None:
                raise AssertionError(f"{row['id']}: faulting rows cannot be verified")
            if actual != row["result"]:
                raise AssertionError(f"{row['id']}: fresh exe result differs: {actual!r} != {row['result']!r}")
            checked += 1
    if not checked:
        raise AssertionError("no oracle rows verified")
    print(f"verified {checked} render rows by fresh Unicorn execution")


def main(argv=None):
    import argparse

    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("cmd", choices=["gen", "probe", "verify"], nargs="?", default="probe")
    p.add_argument("--exe", default=None)
    p.add_argument("--only", default=None)
    a = p.parse_args(argv)
    exe = a.exe or _default_exe()
    if a.cmd == "gen":
        gen(exe, a.only)
    elif a.cmd == "verify":
        verify(exe, a.only)
    else:
        probe(exe, a.only)


if __name__ == "__main__":
    main()
