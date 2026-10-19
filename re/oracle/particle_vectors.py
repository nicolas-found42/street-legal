#!/usr/bin/env python3
"""Unicorn vectors for the particle cfg dialect parsers (M2-10, issue #25).

Runs the exe's two script parsers on cfg bytes and dumps the objects they
build, so the Rust port (crates/slr-fmt-cfg, tests/particle_oracle.rs) can be
compared field by field.

- FUN_0051ac20 (0x51ac20): `[STATE]` / `[ACTION]` emitter script -> ParticleSystem
  object (0xbc bytes). Line handlers: state FUN_00519110, action FUN_0051a140,
  domain FUN_005195e0, defaults FUN_0051bbc0.
- FUN_00521e10 (0x521e10): skidmark script -> 0xb0-byte object (maxcount,
  texture, width, height, uv_normal, uv_end, fade).

Hooks (everything else, including the CRT strtok 0x554c70 / sscanf 0x553890
/ strstr 0x554c20 / stricmp 0x5d7190, runs as exe code):
  0x54c880 open -> handle 1, 0x54cdd0 size, 0x54cec0 read (copies the cfg
  bytes), 0x54cca0 close, 0x54f560/0x54f5b0 malloc/free (bump, zeroed),
  0x545fc0 thiscall texture-slot select on obj+0xa0 (skid; the id is recorded).

Object layout derived from the disassembly (byte offsets):

ParticleSystem (0xbc):
  +0x00 vtable 0x5f314c        +0x04 u32 1000   +0x08 u32 0   +0x0c f32 1.0
  +0x1c vtable 0x5f09a8        +0x20 list head node {vt 0x5f09ac, next, prev}
  +0x2c list tail node {vt 0x5f09ac, next, prev}; action nodes are linked
        head.next ... tail.prev (node: +0 vtable, +4 next, +8 prev).
  +0x38 state (FUN_0051bbc0, ECX = obj+0x38):
        +0x38 f32 0.1 (state[0], never touched by the parser)
        +0x3c..+0x64 eleven domain pointers: source_size, dest_size,
              source_color, dest_color, source_alpha, dest_alpha,
              source_angle, dest_angle, texture_offset, angular_velocity,
              lifetime
        +0x68 f32 velscale (1.0)  +0x6c u32 type (0|0x10|0x20)
        +0x70 u32 flags           +0x74/+0x78 u32 dimension (1,1)
        +0x7c/+0x80 f32 (0,0)     +0x84/+0x88 f32 (1.0,1.0) = 1/dim_y, 1/dim_x
  +0x90 = +0x7c, +0x94 = +0x80, +0x98 = +0x84, +0x9c = +0x88, +0xac = +0x70
        (copied after the script loop), +0xb0 = 0.
  Words +0x8c, +0xa0, +0xa4, +0xa8, +0xb4, +0xb8 are never written
  (heap garbage in the exe) and are not part of the comparison.

Domain objects (vtable -> kind, f32 words after the vtable):
  0x5f30d4 point(3) 0x5f30c0 line(p3,d3) 0x5f30ac triangle(15)
  0x5f3098 plane(7) 0x5f3084 rectangle(15) 0x5f3070 box(6) 0x5f113c sphere(7)

Action nodes (+0x10 type id, +0x14 char[64] label, params from +0x54):
  0x5f3138 bounce(2) 0x5f3124 damping(3) 0x5f3110 explosion(4)
  0x5f30fc gravity(5) 0x5f30e8 move(7) 0x5f138c source(10)
  0x5f1128 directsource(11) 0x5f13a0 counter(13)

Row (JSONL under re/out/particle/, git-ignored):
  {"id","exe_addr","name","src","cfg_hex","result","oracle","unicorn","cw","date"}
Floats in `result` are f32 bit patterns (u32). Unicorn ignores x87 precision
control (docs/x87.md): derived domain fields (triangle/plane/rectangle) are
therefore computed at PC=64 by the oracle; the Rust differential test runs the
port at Precision::Pc64 for the same reason.
"""
from __future__ import annotations
import argparse
import datetime
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "particle")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
UC_VER = "2.1.4"

DOMAIN_VT = {
    0x5F30D4: ("point", 3),
    0x5F30C0: ("line", 6),
    0x5F30AC: ("triangle", 15),
    0x5F3098: ("plane", 7),
    0x5F3084: ("rectangle", 15),
    0x5F3070: ("box", 6),
    0x5F113C: ("sphere", 7),
}
STATE_DOMAINS = ["source_size", "dest_size", "source_color", "dest_color",
                 "source_alpha", "dest_alpha", "source_angle", "dest_angle",
                 "texture_offset", "angular_velocity", "lifetime"]
# vtable -> (name, type id, word offsets of plain params, [domain offsets])
ACTION_VT = {
    0x5F3138: ("bounce", 2, [0x54, 0x58, 0x5C], [0x60]),
    0x5F3124: ("damping", 3, [0x54, 0x58, 0x5C, 0x60, 0x64], []),
    0x5F3110: ("explosion", 4, [0x54, 0x58, 0x5C, 0x60, 0x64, 0x68, 0x6C, 0x70], []),
    0x5F30FC: ("gravity", 5, [0x54, 0x58, 0x5C], []),
    0x5F30E8: ("move", 7, [0x54], []),
    0x5F138C: ("source", 10, [0x54], [0x58, 0x5C]),
    0x5F1128: ("directsource", 11, [0x54], [0x58, 0x5C]),
    0x5F13A0: ("counter", 13, [0x54, 0x58, 0x5C], [0x60]),
}


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _new_oracle(data: bytes, exe=None):
    o = Oracle(exe or _default_exe())
    o.hook_alloc()
    textures: list[int] = []
    o.hook(0x54C880, lambda oc, a: 1, "cdecl", 1)
    o.hook(0x54CDD0, lambda oc, a: len(data), "cdecl", 1)

    def _read(oc, a):
        oc.write(a[1], data[: a[2]])
        return 0

    o.hook(0x54CEC0, _read, "cdecl", 3)
    o.hook(0x54CCA0, lambda oc, a: None, "cdecl", 1)
    o.hook(0x545FC0, lambda oc, a: textures.append(a[0]) or 0, "thiscall", 1)
    return o, textures


def _dump_domain(o, p):
    vt = o.read_u32(p)
    kind, n = DOMAIN_VT[vt]
    v = [o.read_u32(p + 4 + 4 * i) for i in range(n)]
    if kind == "sphere":
        # A sphere with fewer than two radius fields reads the callee's leftover argument
        # slots: radius 1 = param_1 (0.0), radius 2 = param_2 = a *stack address* (FUN_005195e0's
        # delimiter-table pointer) reinterpreted as a float. On the exe's real stack that is a
        # tiny denormal (~1e-39), here (stack top 0x7ff00000) a NaN. Both stand for "radius ~ 0":
        # the NaN words are normalised to 0.0, like the port's missing-field default.
        for i in range(3, 7):
            if (v[i] & 0x7F800000) == 0x7F800000 and (v[i] & 0x007FFFFF):
                v[i] = 0
    return {"k": kind, "v": v}


def dump_particle(o, p):
    d = {
        "limit": o.read_u32(p + 4), "count": o.read_u32(p + 8),
        "scale": o.read_u32(p + 0xC), "tail": o.read_u32(p + 0xB0),
    }
    st = {
        "p0": o.read_u32(p + 0x38),
        "velscale": o.read_u32(p + 0x68), "kind": o.read_u32(p + 0x6C),
        "flags": o.read_u32(p + 0x70),
        "dim": [o.read_u32(p + 0x74), o.read_u32(p + 0x78)],
        "tex_origin": [o.read_u32(p + 0x7C), o.read_u32(p + 0x80)],
        "tex_step": [o.read_u32(p + 0x84), o.read_u32(p + 0x88)],
        "init_tex_origin": [o.read_u32(p + 0x90), o.read_u32(p + 0x94)],
        "init_tex_step": [o.read_u32(p + 0x98), o.read_u32(p + 0x9C)],
        "init_flags": o.read_u32(p + 0xAC),
        "domains": {n: _dump_domain(o, o.read_u32(p + 0x3C + 4 * i))
                    for i, n in enumerate(STATE_DOMAINS)},
    }
    d["state"] = st
    acts = []
    tail = p + 0x2C
    node = o.read_u32(p + 0x24)
    guard = 0
    while node != tail:
        guard += 1
        assert guard < 10000, "action list does not terminate"
        vt = o.read_u32(node)
        name, tid, words, doms = ACTION_VT[vt]
        assert o.read_u32(node + 0x10) == tid, (hex(vt), o.read_u32(node + 0x10))
        label = o.read(node + 0x14, 64)
        label = label.split(b"\0", 1)[0].decode("latin-1")
        acts.append({
            "t": tid, "name": label,
            "v": [o.read_u32(node + w) for w in words],
            "d": [_dump_domain(o, o.read_u32(node + w)) for w in doms],
        })
        node = o.read_u32(node + 4)
    d["actions"] = acts
    return d


def dump_skid(o, p, textures):
    w = lambda i: o.read_u32(p + 4 * i)  # noqa: E731
    return {
        "maxcount": w(0), "width": [w(2), w(3)], "height": w(4),
        "uv_normal": [w(5), w(6), w(7), w(8)], "uv_end": [w(9), w(10), w(11), w(12)],
        "fade": w(13), "one": w(14), "tail": [w(i) for i in range(0x28, 0x2C)],
        "textures": list(textures),
    }


def run(kind: str, data: bytes, exe=None):
    o, textures = _new_oracle(data, exe)
    addr = {"particle": 0x51AC20, "skid": 0x521E10}[kind]
    r = o.call(addr, [0x1234], "cdecl", fpu_cw=CW_D3D9, limit=200_000_000)
    assert r.eax, "parser returned NULL"
    if kind == "particle":
        return dump_particle(o, r.eax)
    return dump_skid(o, r.eax, textures)


# ----------------------------------------------------------------- inputs
def _f(rng, lo=-500.0, hi=500.0):
    """A decimal token with a random number of significant digits."""
    digits = rng.choice([1, 2, 3, 4, 5, 6, 7, 8, 9])
    v = rng.uniform(lo, hi)
    return f"{v:.{digits}g}"


def _vec(rng, scale=500.0):
    return " ".join(_f(rng, -scale, scale) for _ in range(3))


def _domain(rng, kind):
    if kind == "point":
        return f"point {_vec(rng)}"
    if kind == "line":
        return f"line {_vec(rng)} {_vec(rng)}"
    if kind == "triangle":
        return f"triangle {_vec(rng)} {_vec(rng)} {_vec(rng)}"
    if kind == "plane":
        return f"plane {_vec(rng)} {_vec(rng)}"
    if kind == "rectangle":
        return f"rectangle {_vec(rng)} {_vec(rng)} {_vec(rng)}"
    if kind == "box":
        return f"box {_vec(rng)} {_vec(rng)}"
    if kind == "sphere":
        return f"sphere {_vec(rng)} {_f(rng, 0, 50)} {_f(rng, 0, 50)}"
    raise ValueError(kind)


DOMAIN_KINDS = ["point", "line", "triangle", "plane", "rectangle", "box", "sphere"]


def synthetic_particle():
    """Hand-written and seeded-random particle scripts (ours, not asset data)."""
    cases = []

    def add(name, text, nl="\r\n"):
        cases.append((name, text.replace("\n", nl).encode("latin-1")))

    add("minimal", "[STATE]\n[ACTION]\n")
    add("empty", "\n")
    add("state_only_defaults", "[STATE]\ntype 1\n")
    add("crlf_all_actions",
        "[STATE]\ntype\t\t2\nvelscale\t0.5\n"
        "source_size\tpoint\t1 2 3\ndest_size\tline\t0 0 0\t1 2 3\n"
        "source_color point 0.25 0.5 0.75\ndest_color point 1 1 1\n"
        "source_alpha point 1 0 0\ndest_alpha point 0 0 0\n"
        "lifetime line 1 0 0 2 0 0\n\n"
        "[ACTION]\n"
        "source s0 300 line -0.1 0 0 0.1 0 0 sphere 0 0 0 0.1 2.5\n"
        "gravity g0 0 -9.81 0\n"
        "bounce b0 0.01 0.35 0 plane 0 5 0 0 1 0\n"
        "move m0 1 0\n"
        "damping d0 0.95 0.95 0.95\n"
        "explosion e0 1 2 3 4 5 6 7 8\n"
        "counter c0 7 point 1 2 3\n"
        "directsource ds0 12 point 0 0 0 sphere 0 0 0 1 0.5\n")
    add("lf_only", "[STATE]\nsource_size point 1 2 3\n[ACTION]\ngravity g 0 -1 0\n", "\n")
    add("no_trailing_newline", "[STATE]\ntype 1\n[ACTION]\ngravity g0 0 -9.81 0", "\n")
    add("commas_and_spaces",
        "[ACTION]\ngravity,g0,0,-9.81,0\ngravity   g1   1  2  3  \n"
        "move m0 ,, 1 ,, -1\n")
    add("keyword_case", "[STATE]\nTYPE 2\nVelScale 3\nSOURCE_SIZE POINT 1 2 3\n"
        "[ACTION]\nGRAVITY g0 1 2 3\nMove m0 -1 -1\n")
    add("comments_and_unknown",
        "; leading comment\n[STATE]\n;type 1\nbogus 1 2 3\nvelscale 2\n"
        "[ACTION]\n;source s0 1 point 0 0 0 point 0 0 0\nnonsense x y\n"
        "gravity g0 0 -1 0\n")
    add("section_marker_substring",
        ";[ACTION] mentioned in a comment\ngravity g0 0 -1 0\n"
        "x[STATE]x\nvelscale 4\n[ACTION]\ngravity g1 0 -2 0\n")
    add("section_order_swapped",
        "[ACTION]\ngravity g0 0 -1 0\n[STATE]\nvelscale 2\n[ACTION]\nmove m0 1 1\n")
    add("lines_before_section", "gravity g0 0 -1 0\nvelscale 9\n[STATE]\nvelscale 2\n")
    add("move_variants",
        "[ACTION]\nmove a 0 0\nmove b 1 0\nmove c -1 0\nmove d 0 1\nmove e 0 -1\n"
        "move f 1 1\nmove g -1 -1\nmove h 1 -1\nmove i -1 1\nmove j 5 7\n")
    add("state_misc", "[STATE]\ndimension 4 2\nsource_angle point 1 2 3\n"
        "dest_angle line 1 2 3 4 5 6\ntexture_offset point 0.5 0.25 0\n"
        "angular_velocity point 3 0 0\ntype 0\n")
    add("state_type_values", "[STATE]\ntype 1\ntype 2\ntype 3\n")
    add("state_type_one_then_zero", "[STATE]\ntype 1\ntype 0\n")
    add("long_label", "[ACTION]\ngravity " + "L" * 40 + " 1 2 3\n")
    add("ints_given_floats", "[ACTION]\nmove m 1.9 -1.9\ncounter c 3.7 point 0 0 0\n")
    add("float_formats", "[STATE]\nvelscale 1e2\n[ACTION]\ngravity g 5. -2.25 3\n"
        "gravity h 1.5e-3 2E+2 0x10\n")
    add("domain_each",
        "[STATE]\n"
        "source_size triangle 0 0 0 1 0 0 0 1 0\n"
        "dest_size rectangle 1 2 3 4 0 0 0 5 0\n"
        "source_color plane 0 1 0 0 3 4\n"
        "dest_color box 3 2 1 -1 -2 -3\n"
        "source_alpha sphere 1 2 3 5 2\n"
        "dest_alpha sphere 1 2 3 2 5\n"
        "lifetime box -1 4 -3 1 2 3\n")
    add("domain_box_mixed", "[STATE]\nsource_size box 1 -5 2 -1 5 2\n"
        "dest_size box -1 5 -2 1 -5 2\n")
    add("plane_zero_normal", "[STATE]\nsource_size plane 1 2 3 0 0 0\n")
    add("triangle_degenerate", "[STATE]\nsource_size triangle 1 1 1 1 1 1 2 2 2\n")

    rng = random.Random(0x25_10)
    for i in range(60):
        kind = DOMAIN_KINDS[i % len(DOMAIN_KINDS)]
        text = f"[STATE]\nsource_size {_domain(rng, kind)}\n" \
               f"dest_color {_domain(rng, DOMAIN_KINDS[(i * 3 + 1) % 7])}\n" \
               f"[ACTION]\nbounce b0 {_f(rng, 0, 2)} {_f(rng, 0, 2)} {_f(rng, 0, 2)} " \
               f"{_domain(rng, DOMAIN_KINDS[(i * 5 + 2) % 7])}\n" \
               f"gravity g0 {_vec(rng, 20)}\n" \
               f"damping d0 {_vec(rng, 2)} {_f(rng, 0, 5)} {_f(rng, 100, 2000)}\n" \
               f"explosion e0 {_vec(rng, 50)} " + " ".join(_f(rng, 0, 100) for _ in range(5)) + "\n" \
               f"source s0 {_f(rng, 1, 500)} {_domain(rng, DOMAIN_KINDS[(i * 2) % 7])} " \
               f"{_domain(rng, DOMAIN_KINDS[(i * 3) % 7])}\n"
        add(f"random_{i:02d}", text)
    # sscanf("%f") corner cases: one token per gravity component
    weird = [".5", "5.", "-.5", "+.5e1", "1e", "1e+", "1e-", "+3", "inf", "nan",
             "1.2.3", "--1", "1e5", "1E-3", "0x1p3", "00012", "-0", "1e39", "1e-46",
             "3.4028235e38", "3.4028236e38", "1.17549435e-38", "1e-39", "1.4e-45",
             "16777217", "16777219", "0.1", "0.3", "123456789012345678901234567890",
             "0.000000000000000000000000000001", "8388609.5", "-16777217.5", "1e", "e5", "-", "+", ".", "abc"]
    for i in range(0, len(weird) - 2, 3):
        # failing conversions leave uninitialised stack in the exe, so they are only
        # exercised on the first parsed action line (fresh stack => zero)
        add(f"weird_{i:02d}", f"[ACTION]\ngravity g {weird[i]} {weird[i+1]} {weird[i+2]}\n")
    # many-float conversion stress: random decimals over many magnitudes
    for i in range(40):
        lines = []
        for _ in range(60):
            exp = rng.randint(-12, 12)
            toks = " ".join(f"{rng.uniform(-1, 1) * 10 ** exp:.{rng.randint(1, 12)}g}" for _ in range(3))
            lines.append(f"gravity g {toks}")
        add(f"floats_{i:02d}", "[ACTION]\n" + "\n".join(lines) + "\n")
    return cases


def synthetic_skid():
    cases = []

    def add(name, text, nl="\r\n"):
        cases.append((name, text.replace("\n", nl).encode("latin-1")))

    add("empty", "\n")
    add("stock_shape",
        "maxcount\t100\ntexture\t0\nwidth\t0.3\t0.4\nheight\t0.3\n"
        "uv_end\t0.125 0.0 0.375 1.0\nuv_normal 0.625 0.0 0.875 1.0\nfade 0.1\n")
    add("keyword_case", "MAXCOUNT 7\nTexture 1f\nWIDTH 1 2\nHeight 3\nFADE 4\n")
    add("hex_texture", "texture 0x1F\ntexture ff\ntexture 10\n")
    add("no_maxcount", "width 2 4\nheight 1\n")
    add("maxcount_twice", "maxcount 5\nmaxcount 6\n")
    add("comments", ";c\nmaxcount 3\n#x\nbogus 1\nfade 0.5\n")
    add("no_trailing_newline", "maxcount 3\nfade 0.25", "\n")
    add("lf", "maxcount 12\nwidth 0.5 0.75\n", "\n")
    add("fade_default_zero", "fade\n")
    rng = random.Random(0x25_11)
    for i in range(30):
        add(f"random_{i:02d}",
            f"maxcount {rng.randint(0, 500)}\nwidth {_f(rng, 0, 3)} {_f(rng, 0, 3)}\n"
            f"height {_f(rng, 0, 3)}\n"
            f"uv_normal {_f(rng, 0, 1)} {_f(rng, 0, 1)} {_f(rng, 0, 1)} {_f(rng, 0, 1)}\n"
            f"uv_end {_f(rng, 0, 1)} {_f(rng, 0, 1)} {_f(rng, 0, 1)} {_f(rng, 0, 1)}\n"
            f"fade {_f(rng, 0, 1)}\ntexture {rng.randint(0, 255):x}\n")
    return cases


def corpus_inputs():
    """Stock install + modded overlay cfgs (asset data; rows stay git-ignored)."""
    out = []
    roots = [("stock", os.environ.get("SLR_STOCK") or os.path.join(MAIN_CHECKOUT, "stock")),
             ("modded", os.environ.get("SLR_REFERENCE") or os.path.join(MAIN_CHECKOUT, "reference", "raw"))]
    for label, root in roots:
        base = os.path.join(root, "particles", "scripts")
        if not os.path.isdir(base):
            continue
        for dirpath, _, files in sorted(os.walk(base)):
            for fn in sorted(files):
                if not fn.lower().endswith(".cfg"):
                    continue
                full = os.path.join(dirpath, fn)
                rel = os.path.relpath(full, base).replace(os.sep, "/")
                kind = "skid" if rel.startswith("skids/") else "particle"
                with open(full, "rb") as f:
                    out.append((kind, f"{label}:{rel}", f.read()))
    return out


def all_inputs():
    rows = []
    for k, n, d in corpus_inputs():
        rows.append((k, n, d))
    for n, d in synthetic_particle():
        rows.append(("particle", f"syn:{n}", d))
    for n, d in synthetic_skid():
        rows.append(("skid", f"syn:{n}", d))
    return rows


def _row(i, kind, src, data, result):
    return {
        "id": i, "exe_addr": "0x0051ac20" if kind == "particle" else "0x00521e10",
        "name": f"{kind}_parse", "kind": kind, "src": src, "cfg_hex": data.hex(),
        "result": result, "oracle": "unicorn", "unicorn": UC_VER,
        "cw": f"0x{CW_D3D9:04x}", "date": datetime.date.today().isoformat(),
    }


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = {"particle": [], "skid": []}
    for i, (kind, src, data) in enumerate(all_inputs()):
        rows[kind].append(_row(len(rows[kind]), kind, src, data, run(kind, data, exe)))
    for kind, rs in rows.items():
        path = os.path.join(OUT_DIR, f"{kind}_parse.jsonl")
        with open(path, "w") as f:
            for r in rs:
                f.write(json.dumps(r) + "\n")
        print(f"{kind}: {len(rs)} rows -> {path}")


def verify(exe=None):
    bad = n = 0
    for kind in ("particle", "skid"):
        path = os.path.join(OUT_DIR, f"{kind}_parse.jsonl")
        with open(path) as f:
            for line in f:
                row = json.loads(line)
                n += 1
                again = run(kind, bytes.fromhex(row["cfg_hex"]), exe)
                if again != row["result"]:
                    bad += 1
                    print("MISMATCH", row["src"])
    print(f"verify: {n - bad}/{n} rows reproduce")
    return 1 if bad else 0


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("cmd", choices=["gen", "verify"])
    ap.add_argument("--exe")
    a = ap.parse_args()
    if a.cmd == "gen":
        gen(a.exe)
    else:
        sys.exit(verify(a.exe))
