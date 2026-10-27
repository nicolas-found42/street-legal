#!/usr/bin/env python3
"""Method-level vectors for the declared DirectSound device (S6, device layer).

Drives `re/headless/dsound_platform.py` (the headless original's declared
device) through its real COM thunks with seeded operation sequences, recording
the canonical events of `re/headless/dsound_trace.py`. The Rust port's
`slr_audio::device::DirectSound` replays the same operations
(`crates/slr-audio/tests/device_vectors.rs`) and must produce identical events,
and stop at the same operation where the Python model raises a `Boundary`.

    ~/slr-oracle-venv/bin/python re/oracle/dsound_device_vectors.py gen
    ~/slr-oracle-venv/bin/python re/oracle/dsound_device_vectors.py verify re/out/audio/vectors/device.jsonl

Output stays under `re/out/audio/vectors/` (git-ignored, never committed). The
vectors say nothing about the exe: they judge only that the Rust device keeps
the contract the oracle's device declares. Cases never touch the primary
buffer's cursors or status (the Python model divides by zero there; the Rust
device reports them as unmodelled instead of inventing an answer).
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import random
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "re/headless"))

import startup  # noqa: E402
from com_tables import INTERFACES  # noqa: E402
from desktop import DesktopProfile  # noqa: E402
from dsound_trace import DirectSoundTrace  # noqa: E402
from graphics import GraphicsProfile  # noqa: E402
from media import MediaProfile  # noqa: E402
from platform_profile import PlatformProfile  # noqa: E402

OUT = REPO / "re/out/audio/vectors"
HEADLESS = REPO / "re/out/headless"
WINDOW = 0x00010040
STACK = 0x7FEF0000
RETURN_SITE = 0x00559944

IID = {
    "listener": bytes.fromhex("84fa9a278149ce11a5210020af0be560"),
    "3d": bytes.fromhex("86fa9a278149ce11a5210020af0be560"),
    "other": bytes(range(16)),
}
FLAGS = [0x0, 0x2, 0x10, 0x20, 0x40, 0x80, 0xE0, 0xE2, 0xF2, 0xA0, 0x80 | 0x40 | 0x20 | 0x10, 0x4, 0x8, 0x100, 0x4000]
BAD_FLAGS = [0x40000, 0x400000]


class Rig:
    """A Startup host with the declared platform profile, never run."""

    def __init__(self, profile_path: Path):
        self.host = startup.Startup(
            REPO / "reference/raw/StreetLegal_Redline.bak.exe",
            media_profile=MediaProfile.load(HEADLESS / "hypothetical-cd-toc.json"),
            graphics_profile=GraphicsProfile.load(HEADLESS / "boot/graphics-profile.json"),
            desktop_profile=DesktopProfile.load(
                HEADLESS / "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json"
            ),
            platform_profile=PlatformProfile.load(profile_path),
            fast_count=True,
        )
        self.host.windows[WINDOW] = {}
        self.trace = DirectSoundTrace(self.host, None, startup.Boundary)
        self.trace.attach_create(self.host)
        self.scratch = self.host.alloc(0x1000)
        self.pointers = {}  # canonical name -> COM object pointer
        self.last_lock = None

    def stack_call(self, thunk, words):
        h = self.host
        h.write_u32s(STACK, [RETURN_SITE, *words])
        h.set_reg("esp", STACK)
        return h.com.invoke(thunk)

    def method(self, pointer, interface, name):
        slot = [m for m, _ in INTERFACES[interface]].index(name)
        table = self.host.read_u32(pointer)
        return self.host.read_u32(table + 4 * slot)

    def com(self, name, interface, method, *args):
        pointer = self.pointers[name]
        return self.stack_call(self.method(pointer, interface, method), [pointer, *args])

    def put_format(self, address, fmt):
        self.host.write(address, bytes(16) if fmt is None else
                        __import__("struct").pack("<HHIIHH", fmt["tag"], fmt["channels"], fmt["rate"],
                                                  fmt["avg_bytes_per_sec"], fmt["block_align"], fmt["bits"]))

    def put_desc(self, desc):
        h = self.host
        base = self.scratch + 0x100
        fmt_ptr = 0
        if desc["format"] is not None:
            fmt_ptr = self.scratch + 0x200
            self.put_format(fmt_ptr, desc["format"])
        h.write_u32s(base, [desc["size"], desc["flags"], desc["bytes"], desc["reserved"], fmt_ptr])
        h.write(base + 20, bytes.fromhex(desc["guid"]))
        return base


def wfx(rate=22050, channels=1, bits=16, tag=1, **override):
    align = channels * bits // 8
    f = {"tag": tag, "channels": channels, "rate": rate, "avg_bytes_per_sec": rate * align,
         "block_align": align, "bits": bits}
    f.update(override)
    return f


def execute(rig: Rig, op):
    """Run one op against the rig; a `Boundary` from the model propagates."""
    h = rig.host
    kind = op["op"]
    if kind == "clock":
        h.elapsed_ms = op["ms"] - h.icount // 1000
    elif kind == "create":
        out = rig.scratch
        h.write_u32(out, 0)
        handler = h.apis["ord1"][1]
        handler([0, out, 0])
        rig.pointers["ds"] = h.read_u32(out)
    elif kind == "caps":
        h.write_u32(rig.scratch, op["size"])
        rig.com("ds", "IDirectSound", "GetCaps", rig.scratch)
    elif kind == "coop":
        rig.com("ds", "IDirectSound", "SetCooperativeLevel", op["window"], op["level"])
    elif kind == "create_buffer":
        out = rig.scratch + 0x40
        h.write_u32(out, 0)
        rig.com("ds", "IDirectSound", "CreateSoundBuffer", rig.put_desc(op["desc"]), out, 0)
        if h.read_u32(out):
            name = f"buf#{rig.trace.buffer_count - 1}"
            rig.pointers[name] = h.read_u32(out)
    elif kind == "duplicate":
        out = rig.scratch + 0x40
        h.write_u32(out, 0)
        rig.com("ds", "IDirectSound", "DuplicateSoundBuffer", rig.pointers[op["src"]], out)
        if h.read_u32(out):
            rig.pointers[f"buf#{rig.trace.buffer_count - 1}"] = h.read_u32(out)
    elif kind == "set_format":
        address = 0
        if op["format"] is not None:
            address = rig.scratch + 0x300
            rig.put_format(address, op["format"])
        rig.com(op["buf"], "IDirectSoundBuffer", "SetFormat", address)
    elif kind == "scalar":
        rig.com(op["buf"], "IDirectSoundBuffer", op["method"], *[a & 0xFFFFFFFF for a in op["args"]])
    elif kind == "get":
        rig.com(op["buf"], "IDirectSoundBuffer", op["method"], rig.scratch + 0x60, rig.scratch + 0x64)
    elif kind == "get_format":
        rig.com(op["buf"], "IDirectSoundBuffer", "GetFormat", rig.scratch + 0x300, op["size"], rig.scratch + 0x60)
    elif kind == "get_caps":
        h.write_u32(rig.scratch + 0x300, op["size"])
        rig.com(op["buf"], "IDirectSoundBuffer", "GetCaps", rig.scratch + 0x300)
    elif kind == "lock":
        s = rig.scratch
        slots = op["slots"]
        for off in (0x80, 0x84, 0x88, 0x8C):
            h.write_u32(s + off, 0xDEAD0000 + off)
        rig.com(op["buf"], "IDirectSoundBuffer", "Lock", op["offset"], op["bytes"],
                s + 0x80 if slots[0] else 0, s + 0x84 if slots[1] else 0,
                s + 0x88 if slots[2] else 0, s + 0x8C if slots[3] else 0, op["flags"])
        if h.reg("eax") == 0:
            rig.last_lock = [h.read_u32(s + off) if given else 0 for off, given in zip((0x80, 0x84, 0x88, 0x8C), slots)]
    elif kind == "unlock":
        back = rig.last_lock or [0, 0, 0, 0]
        if op["mode"] == "wrong":
            back = [back[0], back[1] ^ 4, back[2], back[3]]
        rig.com(op["buf"], "IDirectSoundBuffer", "Unlock", *back)
    elif kind == "qi":
        h.write(rig.scratch + 0x90, IID[op["iid"]])
        h.write_u32(rig.scratch + 0xB0, 0)
        rig.com(op["buf"], "IDirectSoundBuffer", "QueryInterface", rig.scratch + 0x90, rig.scratch + 0xB0)
        pointer = h.read_u32(rig.scratch + 0xB0)
        if pointer:
            obj = h.com.objects[pointer]
            rig.pointers[rig.trace.names[pointer]] = pointer
    elif kind in ("addref", "release"):
        rig.com(op["buf"], "IDirectSoundBuffer", "AddRef" if kind == "addref" else "Release")
    elif kind == "release_ds":
        rig.com("ds", "IDirectSound", "Release")
    else:
        raise ValueError(kind)


def compared(event):
    return {k: event[k] for k in ("i", "call", "this", "args", "ret", "out")}


# ----------------------------------------------------------------------------- generator

def random_format(rng, secondary=True):
    roll = rng.random()
    rate = rng.choice([8000, 11025, 22050, 44100, 48000, 100, 200000])
    channels = rng.choice([1, 1, 1, 2])
    bits = rng.choice([8, 16, 16])
    if roll < 0.78:
        return wfx(rate, channels, bits)
    if roll < 0.84:
        return wfx(rate, channels, bits, tag=rng.choice([0, 2, 85]))
    if roll < 0.90:
        return wfx(rate, 3)
    if roll < 0.94:
        return wfx(rate, channels, bits, avg_bytes_per_sec=rate)
    if roll < 0.97:
        return wfx(rate, channels, bits, block_align=3)
    return wfx(rate, channels, 12)


def random_desc(rng):
    flags = rng.choice(FLAGS + FLAGS + BAD_FLAGS) | rng.choice([0, 0, 0x80, 0xE0])
    primary = rng.random() < 0.08
    if primary:
        flags |= 1
    fmt = None if (primary and rng.random() < 0.85) else random_format(rng)
    if not primary and rng.random() < 0.04:
        fmt = None
    nbytes = 0 if (primary and rng.random() < 0.85) else rng.choice([0, 3, 4, 100, 4096, 22050, 65536, 88200, 0x0FFFFFFF, 0x10000000, 1001])
    if not primary and rng.random() < 0.7 and fmt is not None:
        nbytes = fmt["block_align"] * rng.choice([1, 2, 10, 100, 1000, 8820, 20000])
    return {
        "size": rng.choice([36, 36, 36, 20, 20, 24, 0]),
        "flags": flags,
        "bytes": nbytes,
        "reserved": rng.choice([0, 0, 0, 1]),
        "format": fmt,
        "guid": rng.choice(["00" * 16, "00" * 16, bytes(range(16)).hex()]),
    }


def gen_case(rng, case_id):
    ops = []
    clock = rng.randrange(0, 5000)
    ops.append({"op": "clock", "ms": clock})
    ops.append({"op": "create"})
    ops.append({"op": "caps", "size": rng.choice([96, 96, 96, 95, 0])})
    if rng.random() < 0.9:
        ops.append({"op": "coop", "window": rng.choice([WINDOW, WINDOW, WINDOW, 0, 0x1234]), "level": rng.choice([2, 2, 1, 3, 4, 0, 5])})
    else:
        ops.append({"op": "create_buffer", "desc": random_desc(rng)})
    # Start an ordinary secondary buffer so the stateful operation pool is real.
    ops.append({"op": "coop", "window": WINDOW, "level": 2})
    ops.append({"op": "create_buffer", "desc": {"size": 36, "flags": 0xE0, "bytes": 8820, "reserved": 0, "format": wfx(), "guid": "00" * 16}})
    primary_done = False
    for _ in range(rng.randrange(8, 40)):
        roll = rng.random()
        clock += rng.choice([0, 1, 7, 20, 100, 333, 1000])
        if roll < 0.05:
            ops.append({"op": "clock", "ms": clock})
            continue
        if roll < 0.20:
            desc = random_desc(rng)
            if desc["flags"] & 1 and primary_done:
                desc["flags"] &= ~1
            ops.append({"op": "create_buffer", "desc": desc})
            if desc["flags"] & 1:
                primary_done = True
            continue
        if roll < 0.28:
            ops.append({"op": "duplicate", "src": "BUF"})
            continue
        ops.append({"op": "pick", "roll": roll})
    return {"id": case_id, "seed_ops": ops}


def materialise(rng, rig: Rig, seed_ops):
    """Run a symbolic case against the rig step by step, choosing buffer targets
    from the buffers that actually exist, and return (ops, events, stop)."""
    concrete = []
    pool = []  # created, non-primary, non-released names
    primaries = []
    for symbolic in seed_ops:
        for op in expand(rng, symbolic, pool, primaries):
            concrete.append(op)
            before = len(rig.trace.captured)
            try:
                execute(rig, op)
            except startup.Boundary as error:
                return concrete, list(rig.trace.captured), {"op": len(concrete) - 1, "kind": error.facts["kind"]}
            new = rig.trace.captured[before:]
            if op["op"] == "create_buffer" and new and new[-1]["ret"] == 0:
                (primaries if op["desc"]["flags"] & 1 else pool).append(new[-1]["out"]["object"])
            if op["op"] == "duplicate" and new and new[-1]["ret"] == 0:
                pool.append(new[-1]["out"]["object"])
            if op["op"] == "release" and new and new[-1]["ret"] == 0:
                for names in (pool, primaries):
                    if op["buf"] in names:
                        names.remove(op["buf"])
    return concrete, list(rig.trace.captured), None


def expand(rng, symbolic, pool, primaries):
    kind = symbolic["op"]
    if kind == "duplicate":
        if pool and rng.random() < 0.9:
            return [{"op": "duplicate", "src": rng.choice(pool)}]
        if primaries:
            return [{"op": "duplicate", "src": primaries[0]}]
        return []
    if kind == "create_buffer":
        return [symbolic]
    if kind != "pick":
        return [symbolic]
    target = rng.choice(pool) if pool else None
    prim = primaries[0] if primaries else None
    r = rng.random()
    if target is None and prim is None:
        return []
    if target is None or (prim is not None and r < 0.12):
        # primary: only operations the Python model implements without crashing
        choice = rng.random()
        if choice < 0.3:
            return [{"op": "set_format", "buf": prim, "format": rng.choice([wfx(22050, 2, 16), wfx(44100, 2, 16), wfx(11025, 1, 8), None, wfx(22050, 3)])}]
        if choice < 0.5:
            return [{"op": "scalar", "buf": prim, "method": "Play", "args": [rng.choice([0, 0, 1]), 0, rng.choice([1, 1, 0, 3])]}]
        if choice < 0.65:
            return [{"op": "qi", "buf": prim, "iid": rng.choice(["listener", "listener", "3d", "other"])}]
        if choice < 0.8:
            return [{"op": "get_format", "buf": prim, "size": rng.choice([16, 18, 15, 0])}]
        return [{"op": "get_caps", "buf": prim, "size": rng.choice([20, 20, 19])}]
    t = target
    r = rng.random()
    if r < 0.14:
        return [{"op": "scalar", "buf": t, "method": "SetVolume", "args": [rng.choice([0, -1, -450, -10000, -10001, 1, -5000, 2**31, -(2**31)])]}]
    if r < 0.24:
        return [{"op": "scalar", "buf": t, "method": "SetPan", "args": [rng.choice([0, -10000, 10000, 10001, -10001, 500, -3000])]}]
    if r < 0.36:
        return [{"op": "scalar", "buf": t, "method": "SetFrequency", "args": [rng.choice([0, 99, 100, 11025, 22050, 44100, 200000, 200001, 300000])]}]
    if r < 0.52:
        return [{"op": "scalar", "buf": t, "method": "Play", "args": [rng.choice([0, 0, 0, 1]), rng.choice([0, 0, 5]), rng.choice([0, 1, 1, 1, 2, 3, 0x20])]}]
    if r < 0.60:
        return [{"op": "scalar", "buf": t, "method": "Stop", "args": []}]
    if r < 0.67:
        return [{"op": "scalar", "buf": t, "method": "SetCurrentPosition", "args": [rng.choice([0, 0, 2, 4, 3, 100, 1000, 8820, 88200, 4096, 2**31])]}]
    if r < 0.74:
        return [{"op": "get", "buf": t, "method": "GetCurrentPosition"}]
    if r < 0.78:
        return [{"op": "get", "buf": t, "method": "GetStatus"}]
    if r < 0.82:
        return [{"op": "get", "buf": t, "method": rng.choice(["GetVolume", "GetPan", "GetFrequency"])}]
    if r < 0.86:
        return [{"op": "get_format", "buf": t, "size": rng.choice([16, 18, 15])}]
    if r < 0.88:
        return [{"op": "get_caps", "buf": t, "size": rng.choice([20, 20, 21])}]
    if r < 0.93:
        slots = rng.choice([[True, True, True, True]] * 6 + [[True, True, False, False], [False, True, True, True], [True, False, True, True]])
        flags = rng.choice([0, 0, 0, 2, 2, 3, 4, 1])
        offset = rng.choice([0, 0, 2, 4, 100, 1000, 100000])
        nbytes = rng.choice([0, 4, 100, 4000, 20000, 100000, 2**31])
        lock = {"op": "lock", "buf": t, "offset": offset, "bytes": nbytes, "flags": flags, "slots": slots}
        ops = [lock]
        if rng.random() < 0.85:
            ops.append({"op": "unlock", "buf": t, "mode": rng.choice(["matching"] * 9 + ["wrong"])})
        return ops
    if r < 0.96:
        return [{"op": "qi", "buf": t, "iid": rng.choice(["listener", "3d", "3d", "other"])}]
    if r < 0.98:
        return [{"op": "addref", "buf": t}]
    return [{"op": rng.choice(["release", "release", "addref"]), "buf": t}]


def profile_for(path: Path, write_lead_ms: int):
    out = path / "platform-profile.json"
    import subprocess
    subprocess.run([sys.executable, str(REPO / "re/headless/make_boot_profile.py"), str(out)], check=True, capture_output=True)
    data = json.loads(out.read_text())
    data["sound"]["write_lead_ms"] = write_lead_ms
    out.write_text(json.dumps(data, indent=1))
    return out, data["sound"]


def generate(cases: int, seed: int):
    OUT.mkdir(parents=True, exist_ok=True)
    rows = []
    for lead in (10, 0, 37):
        sub = OUT / f"profile-lead{lead}"
        sub.mkdir(exist_ok=True)
        profile_path, sound = profile_for(sub, lead)
        for n in range(cases):
            rng = random.Random(f"{seed}/{lead}/{n}")
            rig = Rig(profile_path)
            case = gen_case(rng, f"lead{lead}-{n:04d}")
            if n < 4:
                desc = {"size": 36, "flags": 0xE0, "bytes": 8820, "reserved": 0, "format": wfx(), "guid": "00" * 16}
                operations = [{"op": "create"}, {"op": "coop", "window": WINDOW, "level": 2}, {"op": "create_buffer", "desc": desc}, {"op": "release_ds"}]
                operations.append([{"op": "caps", "size": 96}, {"op": "coop", "window": WINDOW, "level": 2}, {"op": "create_buffer", "desc": desc}, {"op": "duplicate", "src": "buf#0"}][n])
                case["seed_ops"] = operations
            ops, events, stop = materialise(rng, rig, case["seed_ops"])
            rows.append({
                "id": case["id"], "sound": sound, "window": WINDOW,
                "ops": ops, "events": [compared(e) for e in events], "stop": stop,
            })
            del rig
            gc.collect()
    return rows


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--cases", type=int, default=60)
    g.add_argument("--seed", type=int, default=20261026)
    v = sub.add_parser("verify")
    v.add_argument("path", type=Path)
    args = ap.parse_args()
    if args.cmd == "gen":
        rows = generate(args.cases, args.seed)
        path = OUT / "device.jsonl"
        path.write_text("".join(json.dumps(r, sort_keys=True) + "\n" for r in rows))
        sources = ["re/oracle/dsound_device_vectors.py", "re/headless/dsound_trace.py", "re/headless/dsound_platform.py", "re/headless/d3d9_device.py", "re/headless/com.py", "re/headless/com_tables.py", "re/headless/startup.py", "re/headless/make_boot_profile.py"]
        manifest = {"schema": 1, "oracle_kind": "declared-device-model", "exe_code_executed": False, "seed": args.seed, "cases_per_profile": args.cases, "resource_arena_bytes": 0x10000000, "vector_sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "sources": {name: hashlib.sha256((REPO / name).read_bytes()).hexdigest() for name in sources}, "profiles": {f"profile-lead{lead}/platform-profile.json": hashlib.sha256((OUT / f"profile-lead{lead}/platform-profile.json").read_bytes()).hexdigest() for lead in (10, 0, 37)}}
        path.with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        total = sum(len(r["events"]) for r in rows)
        stops = sum(1 for r in rows if r["stop"])
        print(json.dumps({"path": str(path), "cases": len(rows), "events": total, "boundary_stops": stops,
                          "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}))
        return 0
    expected = args.path.read_bytes()
    rows = generate(args.cases if hasattr(args, "cases") else 60, 20261026)
    again = "".join(json.dumps(r, sort_keys=True) + "\n" for r in rows).encode()
    print("identical" if again == expected else "DIFFERENT")
    return 0 if again == expected else 1


if __name__ == "__main__":
    raise SystemExit(main())
