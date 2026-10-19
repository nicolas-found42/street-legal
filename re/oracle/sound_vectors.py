#!/usr/bin/env python3
"""Unicorn vectors for the ten `java.sound.Sound` rows ported by M1-12.

Calls the exe's natives directly with a hand-built `CallInfo` frame (no JVM
boot, no game data). The music set, the three volume words and the two
DirectSound capability words are planted, and the real code runs. Every row
is a small switch on one of those globals, so the probe covers both arms:

  0x487460 `changeMusicSet(I)V`   0x487500 `nextTrack()V`
  0x487510 `prevTrack()V`         0x487520 `setMode(I)I`
  0x487560 `getMode()I`           0x487580 `setVolume(IF)V`
  0x4875f0 `getVolume(I)F`        0x487640 `has3DHardware()I`
  0x487650 `hasMixHardware()I`    0x487f00 `enableDebugDump(ResourceRef)I`

Hook boundaries (everything else is real exe code, including `0x45d910`
marshalling and the switch dispatch):

- `0x550f30` (release the current set), `0x550e50` (install/reload a set) and
  `0x550fe0` (push channel 1's volume into the set) are *recorded* and answered
  with a stub, so a row carries the library calls the port models as
  [`SoundEvent`]s. The set objects themselves are the image's four static
  descriptors (`0x63c878`/`0x640908`/`0x6408a8`/`0x640890`), which the probe
  plants as small real allocations so the release/install calls carry a live
  pointer.
- `0x42a690`/`0x42ab50`/`0x42a9e0` (field plumbing) answer as they do for a
  missing object in `io_vectors.py`.
- `0x4174a0` the intern call: recorded, answered with an arena word.

Row: `sound.jsonl` under `re/out/sound/`, git-ignored.
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
OUT_DIR = os.path.join(REPO, "re", "out", "sound")

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}

# the globals the rows read and write
CURRENT_SET = 0x64091C
VOLUME_0 = 0x612C58
VOLUME_1 = 0x612C5C
VOLUME_2 = 0x612C60
DS_3D = 0x77C9B8
DS_MIX = 0x77C9A0
ZERO_FLOAT = 0x5E73CC

# the four music-set descriptors the image holds as static objects
MUSIC_SETS = (0x63C878, 0x640908, 0x6408A8, 0x640890)
# the library calls behind the rows, recorded as the port's SoundEvents
RELEASE = 0x550F30
INSTALL = 0x550E50
PUSH_VOLUME = 0x550FE0

CHANGE_MUSIC_SET = 0x487460
NEXT_TRACK = 0x487500
PREV_TRACK = 0x487510
SET_MODE = 0x487520
GET_MODE = 0x487560
SET_VOLUME = 0x487580
GET_VOLUME = 0x4875F0
HAS_3D_HARDWARE = 0x487640
HAS_MIX_HARDWARE = 0x487650
ENABLE_DEBUG_DUMP = 0x487F00

ROWS = {
    CHANGE_MUSIC_SET: ("java.sound.Sound", "changeMusicSet", "(I)V"),
    NEXT_TRACK: ("java.sound.Sound", "nextTrack", "()V"),
    PREV_TRACK: ("java.sound.Sound", "prevTrack", "()V"),
    SET_MODE: ("java.sound.Sound", "setMode", "(I)I"),
    GET_MODE: ("java.sound.Sound", "getMode", "()I"),
    SET_VOLUME: ("java.sound.Sound", "setVolume", "(IF)V"),
    GET_VOLUME: ("java.sound.Sound", "getVolume", "(I)F"),
    HAS_3D_HARDWARE: ("java.sound.Sound", "has3DHardware", "()I"),
    HAS_MIX_HARDWARE: ("java.sound.Sound", "hasMixHardware", "()I"),
    ENABLE_DEBUG_DUMP: (
        "java.sound.Sound",
        "enableDebugDump",
        "(Ljava.util.resource.ResourceRef;)I",
    ),
}


def _default_exe():
    for cand in (
        os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe"),
        os.path.join(MAIN_CHECKOUT, "reference", "raw", "StreetLegal_Redline.bak.exe"),
    ):
        if os.path.exists(cand):
            return cand
    raise SystemExit("stock exe not found; pass --exe")


def _container(o, cells):
    """The operand container `{cap@+0, data@+4, count@+8}`."""
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
    """One value cell: vtable `0x5e734c`, `+4` type cstr, `+8` payload."""
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
    """One fresh Oracle with the sound globals planted and the library calls
    hooked."""

    def __init__(
        self,
        exe=None,
        current=None,
        tracks=None,
        modes=None,
        volumes=(0x3F800000, 0x3F800000, 0x3F800000),
        ds_3d=0,
        ds_mix=0,
    ):
        self.o = Oracle(exe or _default_exe())
        self.events = []
        self.interns = []
        o = self.o
        o.hook_alloc()
        # the four static descriptors, as live objects the calls can carry
        # the row does `mov esi, 0x63c878`: the image address IS the object,
        # so the probe plants the track/mode words at `addr+4`/`addr+0x10` and
        # leaves the descriptor address itself alone.
        self.objects = {addr: addr for addr in MUSIC_SETS}
        o.write_u32(CURRENT_SET, current or 0)
        for addr, value in zip((VOLUME_0, VOLUME_1, VOLUME_2), volumes):
            o.write_u32(addr, value & 0xFFFFFFFF)
        o.write_u32(DS_3D, ds_3d & 0xFFFFFFFF)
        o.write_u32(DS_MIX, ds_mix & 0xFFFFFFFF)
        # the set's own track (+4) and mode (+0x10) words
        for addr in MUSIC_SETS:
            o.write_u32(addr + 4, (tracks or {}).get(addr, 0) & 0xFFFFFFFF)
            o.write_u32(addr + 0x10, (modes or {}).get(addr, 0) & 0xFFFFFFFF)
        o.hook(RELEASE, self._h_release, "thiscall", 0)
        o.hook(INSTALL, self._h_install, "thiscall", 0)
        o.hook(PUSH_VOLUME, self._h_push, "thiscall", 1)
        o.hook(0x4174A0, self._h_intern, "thiscall", 1)
        o.hook(0x42A690, lambda oc, a: 0, "cdecl", 3)
        o.hook(0x42AB50, lambda oc, a: 0, "cdecl", 2)
        o.hook(0x42A9E0, lambda oc, a: None, "cdecl", 3)

    def _h_release(self, oc, a):
        self.events.append(["release", oc.reg("ecx") & 0xFFFFFFFF])
        return None

    def _h_install(self, oc, a):
        self.events.append(["install", oc.reg("ecx") & 0xFFFFFFFF])
        return None

    def _h_push(self, oc, a):
        self.events.append(["push_volume", a[0] & 0xFFFFFFFF])
        return 0.0

    def _h_intern(self, oc, a):
        self.interns.append(oc.read_cstr(a[0]).decode(errors="replace"))
        return 0x1000 + len(self.interns)


def run_sound(case, exe=None):
    _id, spec = case
    world = {k: v for k, v in spec.items() if k not in ("addr", "params")}
    w = World(exe, **world)
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
            "current": o.read_u32(CURRENT_SET),
            "current_is": _which(w, o.read_u32(CURRENT_SET)),
            "volume": [o.read_u32(VOLUME_0), o.read_u32(VOLUME_1), o.read_u32(VOLUME_2)],
            "track": _word(w, o.read_u32(CURRENT_SET), 4),
            "mode": _word(w, o.read_u32(CURRENT_SET), 0x10),
        },
        "error": err,
    }
    return result


def _which(world, object_word):
    for addr, live in world.objects.items():
        if live == object_word:
            return addr
    return 0


def _word(world, object_word, off):
    if not object_word:
        return 0
    return world.o.read_u32(object_word + off)


def sound_cases():
    return [
        ("change_music_set_index_0", {"addr": CHANGE_MUSIC_SET, "params": [("I", 0)]}),
        ("change_music_set_index_3", {"addr": CHANGE_MUSIC_SET, "params": [("I", 3)]}),
        (
            "change_music_set_replaces_current",
            {"addr": CHANGE_MUSIC_SET, "params": [("I", 2)], "current": MUSIC_SETS[0]},
        ),
        (
            "change_music_set_same_object",
            {"addr": CHANGE_MUSIC_SET, "params": [("I", 0)], "current": MUSIC_SETS[0]},
        ),
        (
            "change_music_set_out_of_range",
            {"addr": CHANGE_MUSIC_SET, "params": [("I", 4)], "current": MUSIC_SETS[0]},
        ),
        ("next_track_without_set", {"addr": NEXT_TRACK, "params": []}),
        (
            "next_track_with_set",
            {"addr": NEXT_TRACK, "params": [], "current": MUSIC_SETS[1]},
        ),
        ("prev_track_without_set", {"addr": PREV_TRACK, "params": []}),
        (
            "prev_track_with_set",
            {"addr": PREV_TRACK, "params": [], "current": MUSIC_SETS[1]},
        ),
        ("get_mode_without_set", {"addr": GET_MODE, "params": []}),
        (
            "get_mode_with_set",
            {"addr": GET_MODE, "params": [], "current": MUSIC_SETS[2], "modes": {MUSIC_SETS[2]: 2}},
        ),
        ("set_mode_without_set", {"addr": SET_MODE, "params": [("I", 1)]}),
        (
            "set_mode_in_range",
            {"addr": SET_MODE, "params": [("I", 3)], "current": MUSIC_SETS[0]},
        ),
        (
            "set_mode_out_of_range",
            {"addr": SET_MODE, "params": [("I", 7)], "current": MUSIC_SETS[0]},
        ),
        ("set_volume_channel_0", {"addr": SET_VOLUME, "params": [("I", 0), ("F", 0x3F000000)]}),
        ("set_volume_channel_1_no_set", {"addr": SET_VOLUME, "params": [("I", 1), ("F", 0x3F000000)]}),
        (
            "set_volume_channel_1_with_set",
            {"addr": SET_VOLUME, "params": [("I", 1), ("F", 0x3F000000)], "current": MUSIC_SETS[0]},
        ),
        ("set_volume_channel_2", {"addr": SET_VOLUME, "params": [("I", 2), ("F", 0x3F000000)]}),
        ("set_volume_channel_other", {"addr": SET_VOLUME, "params": [("I", 9), ("F", 0x3F000000)]}),
        ("get_volume_channel_0", {"addr": GET_VOLUME, "params": [("I", 0)]}),
        ("get_volume_channel_1", {"addr": GET_VOLUME, "params": [("I", 1)]}),
        ("get_volume_channel_2", {"addr": GET_VOLUME, "params": [("I", 2)]}),
        ("get_volume_channel_other", {"addr": GET_VOLUME, "params": [("I", 5)]}),
        ("has_3d_below_threshold", {"addr": HAS_3D_HARDWARE, "params": [], "ds_3d": 3}),
        ("has_3d_at_threshold", {"addr": HAS_3D_HARDWARE, "params": [], "ds_3d": 4}),
        ("has_3d_above_threshold", {"addr": HAS_3D_HARDWARE, "params": [], "ds_3d": 9}),
        ("has_mix_below_threshold", {"addr": HAS_MIX_HARDWARE, "params": [], "ds_mix": 3}),
        ("has_mix_at_threshold", {"addr": HAS_MIX_HARDWARE, "params": [], "ds_mix": 4}),
        ("enable_debug_dump_null", {"addr": ENABLE_DEBUG_DUMP, "params": [("L", 0)]}),
        (
            "enable_debug_dump_object",
            {"addr": ENABLE_DEBUG_DUMP, "params": [("L", 0x500000)]},
        ),
    ]


def sound_rows_list(exe=None, only=None):
    rows = []
    for case_id, spec in sound_cases():
        if only and only not in case_id:
            continue
        result = run_sound((case_id, spec), exe)
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
    rows = sound_rows_list(exe, only)
    if not rows:
        return
    path = os.path.join(OUT_DIR, "sound.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def probe(exe=None, only=None):
    for r in sound_rows_list(exe, only):
        print(f"== {r['id']} {r['name']} {r['sig']}")
        print("   ", r["result"])


def verify(exe=None, only=None):
    path = os.path.join(OUT_DIR, "sound.jsonl")
    checked = 0
    with open(path) as source:
        for line in source:
            row = json.loads(line)
            if only and only not in row["id"]:
                continue
            spec = dict(row["world"], addr=row["exe_addr"], params=row["args"]["params"])
            for field in ("tracks", "modes"):
                if field in spec:
                    spec[field] = {int(address): word for address, word in spec[field].items()}
            actual = run_sound((row["id"], spec), exe)
            if actual["error"] is not None or row["result"]["error"] is not None:
                raise AssertionError(f"{row['id']}: faulting rows cannot be verified")
            if actual != row["result"]:
                raise AssertionError(f"{row['id']}: fresh exe result differs: {actual!r} != {row['result']!r}")
            checked += 1
    if not checked:
        raise AssertionError("no oracle rows verified")
    print(f"verified {checked} sound rows by fresh Unicorn execution")


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
