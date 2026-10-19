#!/usr/bin/env python3
"""Unicorn checks for the claims in docs/physics.md (M3-01, issue #42).

Every check calls the REAL exe machine code (reference exe, read only) on a hand-built scenario
and compares against the value the document states.  Callees that are not part of the claim are
replaced by Python hooks; the hooks are listed per check.  Nothing is written to disk and nothing
derived from the exe is committed; this file contains only the scenario builders and expected
values the document quotes.

    ~/slr-oracle-venv/bin/python re/oracle/physics_trace.py all          # run + print + assert
    ~/slr-oracle-venv/bin/python re/oracle/physics_trace.py substeps     # one check

Checks (exe address -> claim):
    ctor      0x004a5580  physics-world constructor: step 0.02 s, gravity (0,-9.81,0), air density
    substeps  0x00428450  per-frame substep schedule (clamp 0.1 s, chunks of <= step, half split)
    worldstep 0x004a5190  accumulate/min-step 1e-4, carried remainder, <= 20 solver iterations
    rk4       0x004a7c10  rigid-body integrator is RK4 (exact for constant acceleration)
    pacejka   0x0043bb30  tyre force = Fz*(p1*Fz+p2)*sin(C*atan(Bx-E*(Bx-atan Bx)))
    cfg       0x00443f90  cfg handlers write the documented vehicle-parameter offsets
    xhelp     0x00454500  (static) sanity: car vtable slots 11/12/13 -> control/drive/animate

Oracle limits (see re/oracle/README.md): Unicorn ignores x87 precision control and its
transcendentals are approximate, so `pacejka` compares with a tolerance; everything else is an
exact float32 comparison.
"""
from __future__ import annotations

import argparse
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

WORLD_PTR = 0x617650          # PTR_DAT_00617650 -> physics world (static object at 0x6409c4)
F32 = lambda x: struct.unpack("<f", struct.pack("<f", x))[0]  # noqa: E731


def _exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    sys.exit("exe not found (reference/raw/StreetLegal_Redline.bak.exe)")


def _new() -> Oracle:
    o = Oracle(_exe(), fpu_cw=CW_D3D9)     # game float code runs at PC24 (ADR 0007)
    o.hook_alloc()
    o.hook(0x5D707D, lambda oc, a: 0, "cdecl", 1)   # atexit(): static-init guards register dtors
    return o


class Fail(Exception):
    pass


def expect(cond, msg):
    if not cond:
        raise Fail(msg)


# ------------------------------------------------------------------ ctor
def check_ctor():
    o = _new()

    def mk(oc, a):                       # body/anchor factories are not part of the claim
        b, s = oc.alloc(0x400), oc.alloc(0x400)
        oc.write_u32(b + 0x5C, s)
        return b

    o.hook(0x49AB60, mk, "thiscall", 6)
    o.hook(0x49B300, mk, "thiscall", 3)
    o.hook(0x54E1D0, lambda oc, a: None, "thiscall", 0)
    w = o.alloc(0x800)
    o.call(0x4A5580, [w], "fastcall")
    f = lambda off: o.read_f32(w + off)  # noqa: E731
    out = {
        "time(+0)": f(0), "accum(+4)": f(4), "step(+8)": f(8), "lastdt(+0xc)": f(0xC),
        "gravity(+0x104)": (f(0x104), f(0x108), f(0x10C)), "air_density(+0x1b4)": f(0x1B4),
    }
    for k, v in out.items():
        print(f"  {k}: {v}")
    expect(f(8) == F32(0.02) and f(0xC) == F32(0.02), "step != 0.02")
    expect((f(0x104), f(0x108), f(0x10C)) == (0.0, F32(-9.81), 0.0), "gravity")
    expect(f(0x1B4) == F32(1.2928), "air density")
    expect(f(0) == 0.0 and f(4) == 0.0, "time/accum init")
    return out


# ------------------------------------------------------------------ substeps
def substeps(frame_dt, scale=1.0, step=0.02):
    o = _new()
    w = o.read_u32(WORLD_PTR)
    o.write_f32(w + 8, step)
    g = o.alloc(0x400)
    o.write_u32(g + 8, o.alloc(0x40))          # empty object list: sentinel whose +4 == 0
    o.write_f32(g + 0x110, scale)              # time scale (System.timeWarp)
    o.write_f32(0x63C534, frame_dt)            # previous frame wall time
    for a in (0x63C538, 0x63C520, 0x63C550):
        o.write_u32(a, 0)
    calls = []

    def stepper(oc, a):
        calls.append(struct.unpack("<f", struct.pack("<I", a[0]))[0])
        return 1

    o.hook(0x4A5190, stepper, "thiscall", 1)
    for va, conv, n in ((0x4A5180, "cdecl", 0), (0x427160, "thiscall", 0), (0x4277F0, "thiscall", 0),
                        (0x552310, "cdecl", 1)):
        o.hook(va, lambda oc, a: None, conv, n)
    o.hook(0x552330, lambda oc, a: 1.0, "cdecl", 1)
    o.call(0x428450, [g], "fastcall")
    return calls


def check_substeps():
    rows = []
    for d in (0.0, 5e-5, 0.01, 0.02, 0.021, 0.04, 0.05, 0.06, 0.1, 0.25):
        c = substeps(F32(d))
        rows.append((d, c))
        print(f"  frame {d!r:>7}: {len(c)} step(s) {[round(x, 6) for x in c]}")
    got = {d: [round(x, 6) for x in c] for d, c in rows}
    expect(got[0.0] == [], "zero frame -> no step")
    expect(got[0.01] == [0.01], "below step: single chunk")
    expect(got[0.02] == [0.01, 0.01], "== step: two halves")
    expect(got[0.04] == [0.02, 0.01, 0.01], "2*step")
    expect(got[0.05] == [0.02, 0.015, 0.015], "0.05")
    expect(got[0.1] == [0.02] * 4 + [0.01] * 2, "0.1 s")
    expect(got[0.25] == got[0.1], "frame dt clamped to 0.1 s (DAT_0060c8cc)")
    c = substeps(F32(0.05), scale=2.0)                 # timescale multiplies before the clamp
    print(f"  frame 0.05 x timescale 2.0 -> {[round(x, 6) for x in c]}")
    expect([round(x, 6) for x in c] == [0.02] * 4 + [0.01] * 2, "timescale applied before 0.1 clamp")
    c = substeps(F32(0.05), step=0.01)                 # step size comes from world+8
    print(f"  frame 0.05 with world step 0.01 -> {[round(x, 6) for x in c]}")
    return rows


# ------------------------------------------------------------------ world step
def worldstep(dt, accum0=0.0, collide=False):
    o = _new()
    w = o.alloc(0x800)
    o.write_f32(w + 4, accum0)
    owner = o.alloc(0x100)                       # body-list owner, +0x24 = no bodies
    o.write_u32(w + 0x21 * 4, owner)
    o.write_u32(w + 0x22 * 4, owner)
    for idx in (0x27, 0x2E, 0x46):               # empty intrusive lists
        o.write_u32(w + idx * 4, o.alloc(0x40))
    log = []

    def mk(name):
        def h(oc, a):
            log.append(name)
            if name == "narrow" and collide:
                oc.write_u32(oc.reg("ecx") + 0x7B * 4, 0)      # flag: contacts still unresolved
            if name in ("resolve2540", "resolve35d0"):
                return 1 if collide else 0
            return 0
        return h

    for va, name, conv, n in ((0x4A3F90, "pre", "cdecl", 0), (0x4A3900, "solve", "cdecl", 0),
                              (0x4A2D10, "narrow", "cdecl", 0), (0x498DD0, "p498dd0", "cdecl", 0),
                              (0x4A2540, "resolve2540", "cdecl", 0), (0x4A35D0, "resolve35d0", "cdecl", 0),
                              (0x4A2B90, "cleanup", "cdecl", 0), (0x426F60, "drive", "thiscall", 1)):
        o.hook(va, mk(name), conv, n)
    o.hook(0x552310, lambda oc, a: None, "cdecl", 1)
    o.hook(0x552330, lambda oc, a: 0.0, "cdecl", 1)
    r = o.call(0x4A5190, [("f32", dt)], "thiscall", this=w, limit=3_000_000)
    return dict(ret=r.eax, log=log, accum=o.read_f32(w + 4), simtime=o.read_f32(w), last=o.read_f32(w + 0xC),
                iters=o.read_u32(w + 0x10))


def check_worldstep():
    a = worldstep(F32(5e-5))
    print(f"  dt=5e-5     -> ret {a['ret']} accum {a['accum']} calls {a['log']}")
    expect(a["ret"] == 0 and a["accum"] == F32(5e-5) and a["log"] == [], "min-step 1e-4: no step, accumulate")
    b = worldstep(F32(0.02))
    print(f"  dt=0.02     -> ret {b['ret']} simtime {b['simtime']} iters {b['iters']} calls {b['log']}")
    expect(b["ret"] == 1 and b["simtime"] == F32(0.02) and b["iters"] == 1 and b["accum"] == 0.0, "plain step")
    expect(b["log"][0] == "drive", "drive callbacks run first")
    c = worldstep(F32(0.02), accum0=F32(6e-5))
    print(f"  dt=0.02 + carried 6e-5 -> simtime {c['simtime']}")
    expect(abs(c["simtime"] - (F32(0.02) + F32(6e-5))) < 1e-9 and c["accum"] == 0.0, "carried remainder merges")
    d = worldstep(F32(0.02), collide=True)
    n = d["log"].count("solve")
    print(f"  dt=0.02 with persistent contacts -> {n} trial iterations (counter {d['iters']})")
    expect(n == 20 and d["iters"] == 20 and d["ret"] == 1, "iteration cap 20")
    return a, b, c, d


# ------------------------------------------------------------------ rk4
def integrate(vel, force, dt, inv_mass=1 / 1000.0):
    o = _new()
    body, P, S = o.alloc(0x600), o.alloc(0x200), o.alloc(0x100)
    o.write_u32(body + 0x58, 1)                 # active, not kinematic (0x100), not child (4)
    o.write_u32(body + 0x5C, P)
    o.write_u32(body + 0x20C, 0)                # state buffer index
    o.write_f32(P + 0x14, inv_mass)
    o.write_u32(P + 0x78, S)
    o.write_f32(S + 0xC, 1.0)
    src, dst = body + 0x64, body + 0x64 + 0xD4  # two 0xd4-byte states, double buffered
    o.write_f32s(src + 12, vel)                 # state+0x0c velocity
    o.write_f32s(src + 40, force)               # state+0x28 force accumulator
    o.write_f32s(src + 0x25 * 4, [0, 0, 0, 1.0])
    o.call(0x4A7C10, [src, dst, ("f32", dt)], "thiscall", this=body, limit=3_000_000)
    return o.read_f32s(dst, 3), o.read_f32s(dst + 12, 3)


def check_rk4():
    dt = 0.02
    pos, vel = integrate([1, 0, 0], [0, -9810.0, 0], dt)
    print(f"  v0=(1,0,0) a=(0,-9.81,0) dt=0.02 -> pos {pos} vel {vel}")
    expect(abs(pos[1] - (-0.5 * 9.81 * dt * dt)) < 1e-8, "pos.y is 0.5*a*dt^2 (explicit Euler would give 0)")
    expect(abs(pos[0] - 0.02) < 1e-8 and abs(vel[1] - (-9.81 * dt)) < 1e-7, "pos.x / vel.y")
    pos, vel = integrate([3, 2, 1], [10000, 0, 0], 0.01)
    print(f"  v0=(3,2,1) a=(10,0,0) dt=0.01 -> pos {pos} vel {vel}")
    expect(abs(pos[0] - (0.03 + 0.5 * 10 * 1e-4)) < 1e-8, "pos.x = v dt + a dt^2/2")
    return pos, vel


# ------------------------------------------------------------------ pacejka
def pacejka_ref(p, slip, fz):
    d = fz * p[1] + p[2]
    if d * p[0] == 0:
        return 0.0
    b = (fz * p[3] + p[4]) * math.exp(-fz * p[5]) / (d * p[0])
    x = (fz * p[9] + p[10] + slip) * b
    e = fz * fz * p[6] + fz * p[7] + p[8]
    return d * fz * math.sin(p[0] * math.atan(e * (math.atan(x) - x) + x))


def check_pacejka():
    rng = random.Random(42)
    worst = 0.0
    for _ in range(40):
        p = [rng.uniform(1.2, 2.0), rng.uniform(-0.2, 0.0), rng.uniform(1.0, 1.8), rng.uniform(5, 15),
             rng.uniform(1, 5), rng.uniform(0, 0.01), rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5),
             rng.uniform(-1, 1), rng.uniform(-0.01, 0.01), rng.uniform(-0.05, 0.05), 0, 0, 0, 0, 0, 0]
        p = [F32(x) for x in p]
        slip, fz = F32(rng.uniform(-0.3, 0.3)), F32(rng.uniform(1.0, 6.0))
        o = _new()
        a = o.alloc(0x100)
        o.write_f32s(a, p)
        r = o.call(0x43BB30, [("f32", slip), ("f32", fz)], "thiscall", this=a)
        ref = pacejka_ref(p, slip, fz)
        worst = max(worst, abs(r.st0 - ref) / max(1e-9, abs(ref), 1e-3))
        expect(abs(r.st0 - ref) <= 2e-4 * max(1.0, abs(ref)), f"pacejka mismatch {r.st0} vs {ref} for {p} {slip} {fz}")
    print(f"  40 random coefficient sets: max relative error vs closed form = {worst:.2e} (Unicorn x87 transcendentals)")
    return worst


# ------------------------------------------------------------------ cfg handlers
CFG = (  # (keyword, handler, line, expected {offset: float})
    ("enginepower", 0x443F90, b"135.0 6.5 0.00006 0.003",
     {0xD0: 135.0, 0xD4: 135.0, 0xE0: 6.5, 0xF0: F32(0.00006), 0xF4: F32(0.003), 0xE4: -1.0, 0xE8: -1.0}),
    ("enginepower(6)", 0x443F90, b"1 2 3 4 5 6",
     {0xD0: 1.0, 0xD4: 1.0, 0xE0: 2.0, 0xF0: 3.0, 0xF4: 4.0, 0xE4: 5.0, 0xE8: 6.0}),
    ("brakepower", 0x444190, b"1300.0 1800.0", {0xD8: 1300.0, 0xDC: 1800.0}),
    ("rearendratio", 0x444150, b"3.33", {0x40: F32(3.33)}),
    ("clutch", 0x444200, b"0.5", {0xC8: 0.5}),
)


def run_cfg(handler, line):
    o = _new()
    car, params = o.alloc(0x3000), o.alloc(0xD00)
    o.write_u32(car + 0x2E48, params)            # car object -> default vehicle parameter block
    o.write_u32(car + 0x120, 0)                  # "already configured" guard must be 0
    for off in range(0, 0x100, 4):
        o.write_f32(params + off, -1.0)
    buf = o.alloc(256)
    o.write(buf, b"kw\x00" + line + b"\x00")     # tokenizer leaves args right after the keyword NUL
    r = o.call(handler, [buf, car], "cdecl", limit=5_000_000)
    touched = sorted({a - params for a, s, v in r.mem_writes if params <= a < params + 0xD00})
    return {off: o.read_f32(params + off) for off in range(0, 0x100, 4)}, touched


def check_cfg():
    for kw, h, line, want in CFG:
        got, touched = run_cfg(h, line)
        print(f"  {kw:15s} 0x{h:08x} '{line.decode()}' wrote offsets {[hex(t) for t in touched]}")
        for off, v in want.items():
            expect(abs(got[off] - v) <= 1e-6 * max(1.0, abs(v)), f"{kw}: +{off:#x} = {got[off]} != {v}")
        if "(6)" not in kw:
            expect(set(touched) <= set(want), f"{kw}: unexpected writes {touched}")


# ------------------------------------------------------------------ static
def check_xhelp():
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import exe_xrefs as x
    img = x.Image(_exe())
    vt = 0x5F0F18
    want = {11: 0x44E690, 12: 0x454500, 13: 0x4518C0, 3: 0x44A250, 4: 0x44B920, 14: 0x458C00, 17: 0x457850}
    for slot, tgt in want.items():
        got = img.u32(vt + 4 * slot)
        print(f"  car vtable {vt:#x} slot {slot:2d} (+{4 * slot:#04x}) -> {got:#010x}")
        expect(got == tgt, f"slot {slot}")
    # class registry: 'car' -> factory 0x442500
    expect(img.u32(0x60C864) == 0x442500, "class table 'car'")
    expect(img.u32(0x617650) == 0x6409C4, "world pointer")


CHECKS = {"ctor": check_ctor, "substeps": check_substeps, "worldstep": check_worldstep, "rk4": check_rk4,
          "pacejka": check_pacejka, "cfg": check_cfg, "xhelp": check_xhelp}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("check", choices=["all", *CHECKS])
    a = ap.parse_args()
    names = list(CHECKS) if a.check == "all" else [a.check]
    bad = 0
    for n in names:
        print(f"[{n}]")
        try:
            CHECKS[n]()
            print(f"  PASS {n}")
        except Fail as e:
            bad += 1
            print(f"  FAIL {n}: {e}")
    print(f"{len(names) - bad}/{len(names)} checks passed (oracle: Unicorn function oracle, no whole-game run)")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
