#!/usr/bin/env python3
"""Scripted keyboard / mouse / gamepad scenarios through the original device layer.

`gen` runs every scenario against the exe's own input functions (see
`input_device_rig.py`) and writes one JSON row per scenario: the declared
scenario, plus per step the DirectInput call stream (`calls`), return values,
and the device-table/global word diff. `verify` re-runs every scenario on a
fresh oracle and requires byte-identical rows. All devices, states and timings
are declared inputs here, not observations from hardware; the rows are
original-derived local evidence and are never committed (`re/out/` is ignored).

  ~/slr-oracle-venv/bin/python re/oracle/input_device_vectors.py gen [--out PATH]
  ~/slr-oracle-venv/bin/python re/oracle/input_device_vectors.py verify PATH

The default PATH is `re/out/boot/input-device.jsonl` (worktree-local).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from input_device_rig import Rig  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
DEFAULT_OUT = REPO / "re/out/boot/input-device.jsonl"
SCHEMA = 1

GUID_X = "e0026da3f3c9cf11bfc7444553540000"
GUID_Y = "e1026da3f3c9cf11bfc7444553540000"
GUID_Z = "e2026da3f3c9cf11bfc7444553540000"
GUID_RZ = "e3026da3f3c9cf11bfc7444553540000"
GUID_SLIDER = "e4026da3f3c9cf11bfc7444553540000"
GUID_RX = "f4026da3f3c9cf11bfc7444553540000"
GUID_RY = "f5026da3f3c9cf11bfc7444553540000"
GUID_BUTTON = "f0026da3f3c9cf11bfc7444553540000"
GUID_POV = "f2026da3f3c9cf11bfc7444553540000"
GUID_KEY = "f1026da3f3c9cf11bfc7444553540000"


def lcg(seed):
    state = seed

    def next_(n):
        nonlocal state
        state = (1103515245 * state + 12345) & 0xFFFFFFFF
        return (state >> 8) % n

    return next_


def pad_spec(index, sliders=2, buttons=12, povs=1, axes=("x", "y", "z", "rx", "ry", "rz"), extra_objects=()):
    table = {"x": GUID_X, "y": GUID_Y, "z": GUID_Z, "rx": GUID_RX, "ry": GUID_RY, "rz": GUID_RZ}
    objects = []
    for n, name in enumerate(axes):
        objects.append({"guid": table[name], "type": 0x02 | (n << 8), "name": f"{name.upper()} Axis", "flags": 0})
    for n in range(sliders):
        objects.append({"guid": GUID_SLIDER, "type": 0x02 | ((len(axes) + n) << 8), "name": f"Slider {n}", "flags": 0})
    for n in range(buttons):
        objects.append({"guid": GUID_BUTTON, "type": 0x04 | (n << 8), "name": f"Button {n}", "flags": 0})
    for n in range(povs):
        objects.append({"guid": GUID_POV, "type": 0x10 | (n << 8), "name": f"Hat {n}", "flags": 0})
    objects.extend(extra_objects)
    return {
        "instance_guid": f"{0xA0 + index:02x}" + "11223344556677889900aabbccddee"[0:30],
        "product_guid": f"{0xB0 + index:02x}" + "11223344556677889900aabbccddee"[0:30],
        "devtype": 0x214,
        "name": f"Declared Pad {index}",
        "caps": {"flags": 1, "devtype": 0x214, "axes": len(axes) + sliders, "buttons": buttons, "povs": povs},
        "objects": objects,
    }


def kb_report(keys):
    report = bytearray(256)
    for key in keys:
        report[key] = 0x80
    return report.hex()


def mouse_report(dx, dy, dz, buttons):
    return (struct.pack("<3i", dx, dy, dz) + bytes(0x80 if b else 0 for b in buttons)).hex()


def joy_report(axes, sliders=(0, 0), povs=(0xFFFFFFFF,) * 4, buttons=()):
    data = bytearray(0x110)
    struct.pack_into("<6i", data, 0, *axes)
    struct.pack_into("<2i", data, 0x18, *sliders)
    struct.pack_into("<4I", data, 0x20, *povs)
    for index in buttons:
        data[0x30 + index] = 0x80
    return data.hex()


def base_scenario(sid, gamepads=(), **env):
    return {
        "id": sid,
        "env": {"hwnd": 0x10042, "layout": 0x04090409, "ff": 0, "qpf": 1_000_000, **env},
        "devices": {
            "caps": {
                "kbd": {"flags": 1, "devtype": 1027, "axes": 0, "buttons": 128, "povs": 0},
                "mouse": {"flags": 1, "devtype": 258, "axes": 3, "buttons": 4, "povs": 0},
            },
            "gamepads": list(gamepads),
        },
        "steps": [],
    }


def poll(qpc, time_ms, feed=None, **kw):
    f = {"qpc": [qpc], "time": [time_ms]}
    f.update(feed or {})
    return {"op": "poll", "feed": f, **kw}


def scenarios():
    out = []

    # -- keyboard and mouse only (the stock oracle's declared machine)
    sc = base_scenario("keyboard-mouse-poll")
    sc["steps"].append({"op": "init"})
    rnd = lcg(1)
    qpc = 5_000_000
    for frame in range(24):
        qpc += 16_000 + rnd(2000)
        keys = sorted({rnd(256) for _ in range(rnd(4))})
        feed = {
            "kbd.state": [kb_report(keys)],
            "mouse.state": [mouse_report(rnd(41) - 20, rnd(41) - 20, rnd(5) - 2, [rnd(2), rnd(2), 0, 0])],
        }
        if frame == 5:
            feed["kbd.GetDeviceState.hr"] = [0x8007000C]  # NOTACQUIRED, Acquire succeeds, retry
            feed["kbd.state"] = [kb_report([0x1E]), kb_report([0x1E])]
        if frame == 9:
            feed["mouse.GetDeviceState.hr"] = [0x8007000C, 0x8007000C]  # still lost after Acquire
            feed["mouse.Acquire.hr"] = [0x80070005]
        if frame == 12:
            feed["kbd.GetDeviceState.hr"] = [0x8007000C, 0x8007000C]
            feed["kbd.Acquire.hr"] = [0x8007001E]  # DIERR_... acquire fails
        sc["steps"].append(poll(qpc, 100 + frame * 16, feed))
    for key in (0, 1, 0x1E, 0x39, 0xFF, 0x100, -1, 0x7FFFFFFF):
        sc["steps"].append({"op": "isdown", "args": [0, 0, key & 0xFFFFFFFF]})
    for axis in range(0, 14):
        sc["steps"].append({"op": "axis", "args": [0, axis]})
        sc["steps"].append({"op": "isaxis", "args": [0, axis]})
        sc["steps"].append({"op": "axisname", "args": [0, axis]})
    for dev in (-1, 0, 1, 2):
        sc["steps"].append({"op": "devname", "args": [dev & 0xFFFFFFFF]})
        sc["steps"].append({"op": "devkind", "args": [dev & 0xFFFFFFFF]})
        sc["steps"].append({"op": "activeaxis", "args": [dev & 0xFFFFFFFF]})
    sc["steps"].append({"op": "shutdown"})
    out.append(sc)

    # -- mouse state transitions through the axis table
    sc = base_scenario("mouse-axis-integration")
    sc["steps"].append({"op": "init"})
    qpc = 2_000_000
    rnd = lcg(7)
    for frame in range(16):
        qpc += 10_000 + rnd(20_000)
        sc["steps"].append(poll(qpc, frame * 10, {
            "kbd.state": [kb_report([])],
            "mouse.state": [mouse_report(rnd(401) - 200, rnd(401) - 200, rnd(121) - 60, [frame & 1, (frame >> 1) & 1, 0, 1])],
        }))
        for axis in range(0, 7):
            sc["steps"].append({"op": "axis", "args": [1, axis]})
        sc["steps"].append({"op": "activeaxis", "args": [1]})
    for code in (0xFFFFFFFF, 0xFFFFFFFE, 0xFFFFFF9C, 0xFFFFFF9B, 0x80000000):
        sc["steps"].append({"op": "axis", "args": [1, code]})
        sc["steps"].append({"op": "poll", "feed": {"qpc": [qpc + 5000], "time": [999], "mouse.state": [mouse_report(30, -30, 0, [0, 0, 0, 0])]}})
        sc["steps"].append({"op": "axis", "args": [1, 0]})
    out.append(sc)

    # -- last key (buffered keyboard read + virtual-key translation)
    sc = base_scenario("keyboard-lastkey")
    sc["steps"].append({"op": "init"})
    cases = [
        ({"kbd.data": [[[0x1E, 0x80]]], "mapvk": [0x41], "kbstate": [kb_report([0x1E])], "toascii": [[1, 0x61]]}),
        ({"kbd.data": [[[0x2A, 0x80]]], "mapvk": [0x10], "kbstate": [kb_report([0x2A])], "toascii": [[0, 0]]}),
        ({"kbd.data": [[[0x1E, 0x00]]]}),
        ({"kbd.data": [[]]}),
        ({"kbd.data": [[[0x1E, 0x80], [0x30, 0x80]]], "mapvk": [0x41], "kbstate": [kb_report([])], "toascii": [[2, 0x00620061]]}),
        ({"kbd.data": [[[0x100, 0x80]]]}),
        ({"kbd.data": [[[0x1C, 0x81]]], "mapvk": [0x0D], "kbstate": [kb_report([0x1C])], "kbstate.ok": [0]}),
        ({"kbd.Acquire.hr": [0x8007001E], "kbd.GetDeviceData.hr": [0x8007000C]}),
        ({"kbd.GetDeviceData.hr": [0x80070057]}),
    ]
    for feed in cases:
        sc["steps"].append({"op": "lastkey", "feed": feed})
    out.append(sc)

    # -- init failures
    for name, feed, pads in (
        ("init-keyboard-format-fails", {"kbd.SetDataFormat.hr": [0x80070057]}, ()),
        ("init-keyboard-coop-fails", {"kbd.SetCooperativeLevel.hr": [0x80070057]}, ()),
        ("init-keyboard-caps-fails", {"kbd.GetCapabilities.hr": [0x80070057]}, ()),
        ("init-keyboard-create-fails", {"di.CreateDevice.hr": [0x80040154]}, ()),
        ("init-mouse-create-fails", {"di.CreateDevice.hr": [0, 0x80040154]}, ()),
        ("init-mouse-format-fails", {"mouse.SetDataFormat.hr": [0x80070057]}, ()),
        ("init-mouse-coop-fails", {"mouse.SetCooperativeLevel.hr": [0x80070057]}, ()),
        ("init-mouse-caps-fails", {"mouse.GetCapabilities.hr": [0x80070057]}, ()),
        ("init-keyboard-property-fails-still-acquires", {"kbd.SetProperty.hr": [0x80070057]}, ()),
    ):
        sc = base_scenario(name, pads)
        sc["steps"].append({"op": "init", "feed": feed})
        sc["steps"].append({"op": "poll", "feed": {"qpc": [1_000_000], "time": [5]}})
        sc["steps"].append({"op": "shutdown"})
        out.append(sc)

    # -- gamepads
    sc = base_scenario("gamepad-enumeration-poll", [pad_spec(0)])
    sc["steps"].append({"op": "init"})
    rnd = lcg(11)
    qpc = 1_000_000
    for frame in range(24):
        qpc += 12_000 + rnd(8000)
        axes = [rnd(65536) for _ in range(6)]
        povs = [rnd(36000) if rnd(3) else 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]
        buttons = [b for b in range(12) if rnd(2)]
        feed = {
            "kbd.state": [kb_report([])],
            "mouse.state": [mouse_report(0, 0, 0, [0, 0, 0, 0])],
            "joy0.state": [joy_report(axes, (rnd(65536), rnd(65536)), povs, buttons)],
        }
        if frame == 4:
            feed["joy0.Poll.hr"] = [0x8007000C]
            feed["joy0.Acquire.hr"] = [0]
        if frame == 8:
            feed["joy0.Poll.hr"] = [0x8007000C, 0x8007000C]
            feed["joy0.Acquire.hr"] = [0x8007001E]
        if frame == 11:
            feed["joy0.Poll.hr"] = [0x8007000C] * 12
            feed["joy0.Acquire.hr"] = [0x8007000C]  # DIERR_INPUTLOST every time -> bounded retry
        sc["steps"].append(poll(qpc, 20 * frame, feed))
        for axis in range(0, 26):
            sc["steps"].append({"op": "axis", "args": [2, axis]})
        sc["steps"].append({"op": "activeaxis", "args": [2]})
    for axis in range(0, 26):
        sc["steps"].append({"op": "isaxis", "args": [2, axis]})
        sc["steps"].append({"op": "axisname", "args": [2, axis]})
    sc["steps"].append({"op": "devname", "args": [2]})
    sc["steps"].append({"op": "devkind", "args": [2]})
    sc["steps"].append({"op": "shutdown"})
    out.append(sc)

    sc = base_scenario("gamepad-pov-cardinals", [pad_spec(0, povs=4)])
    sc["steps"].append({"op": "init"})
    qpc = 3_000_000
    for angle in (0, 4500, 9000, 13500, 18000, 22500, 27000, 31500, 35999, 0xFFFFFFFF, 1, 17999):
        qpc += 16_000
        sc["steps"].append(poll(qpc, 10, {
            "kbd.state": [kb_report([])], "mouse.state": [mouse_report(0, 0, 0, [0, 0, 0, 0])],
            "joy0.state": [joy_report((32767,) * 6, (0, 0), (angle, angle, 9000, 0xFFFFFFFF), [])],
        }))
        for axis in range(0, 40):
            sc["steps"].append({"op": "axis", "args": [2, axis]})
    out.append(sc)

    sc = base_scenario("two-gamepads-and-limits", [
        pad_spec(0, sliders=3, buttons=40, povs=5),
        pad_spec(1, sliders=0, buttons=0, povs=0, axes=("x", "y")),
    ])
    sc["steps"].append({"op": "init"})
    sc["steps"].append(poll(1_000_000, 1, {
        "kbd.state": [kb_report([])], "mouse.state": [mouse_report(0, 0, 0, [0, 0, 0, 0])],
        "joy0.state": [joy_report((0, 65535, 100, 200, 300, 400), (1, 2), (0, 9000, 18000, 27000), [0, 5, 39])],
        "joy1.state": [joy_report((65535, 0, 0, 0, 0, 0))],
    }))
    for dev in (2, 3):
        for axis in range(0, 70):
            sc["steps"].append({"op": "axis", "args": [dev, axis]})
        sc["steps"].append({"op": "activeaxis", "args": [dev]})
        sc["steps"].append({"op": "devname", "args": [dev]})
    sc["steps"].append({"op": "shutdown"})
    out.append(sc)

    sc = base_scenario("gamepad-enumeration-stops-and-fails", [pad_spec(0), pad_spec(1), pad_spec(2)])
    sc["steps"].append({"op": "init", "feed": {
        "di.CreateDevice.hr": [0, 0, 0x80040154, 0, 0],
    }})
    sc["steps"].append({"op": "shutdown"})
    out.append(sc)
    sc = base_scenario("gamepad-setup-failures", [pad_spec(0), pad_spec(1), pad_spec(2), pad_spec(3)])
    sc["steps"].append({"op": "init", "feed": {
        "joy0.SetDataFormat.hr": [0x80070057],
        "joy1.SetCooperativeLevel.hr": [0x80070057],
        "joy2.GetCapabilities.hr": [0x80070057],
        "joy3.SetProperty.hr": [0x80070057],
    }})
    sc["steps"].append({"op": "shutdown"})
    out.append(sc)
    return out


def run_scenario(sc):
    rig = Rig(sc)
    rows = []
    for step in sc["steps"]:
        rows.append(rig.run(step))
    return {"schema": SCHEMA, "scenario": sc, "rows": rows}


def encode(row):
    return json.dumps(row, sort_keys=True, separators=(",", ":"))


def gen(out):
    out.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    with out.open("w") as handle:
        for sc in scenarios():
            handle.write(encode(run_scenario(sc)) + "\n")
            count += 1
    print(f"wrote {count} scenarios to {out}")
    print("sha256", hashlib.sha256(out.read_bytes()).hexdigest())


def verify(path):
    ok = True
    for line in path.read_text().splitlines():
        row = json.loads(line)
        again = encode(run_scenario(row["scenario"]))
        if again != line:
            ok = False
            print("MISMATCH", row["scenario"]["id"])
    print("verified" if ok else "FAILED")
    return 0 if ok else 1


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--out", type=Path, default=DEFAULT_OUT)
    v = sub.add_parser("verify")
    v.add_argument("path", type=Path)
    args = parser.parse_args(argv)
    if args.cmd == "gen":
        gen(args.out)
        return 0
    return verify(args.path)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
