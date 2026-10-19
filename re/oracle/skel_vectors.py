#!/usr/bin/env python3
"""Unicorn vectors for the skeleton / animation file readers (M2-09, issue #24).

Two exe readers are run on file bytes held in emulator memory:

- 0x00531e90  skeleton text reader, thiscall(ECX = 0x18-byte skeleton,
  stack: buf, size), `ret 8`. Reads the `.bon`/`.skl` text format: per bone 9
  lines (name, parent, three matrix rows, a translation row scaled by
  DAT_005f08e8 = 0.1f, three `min, max` limit lines). Initialiser 0x005310b0
  (fastcall ECX). Output: skeleton[0] = bone array (stride 0xe4), [1] =
  count, [2] = traversal head. Per bone: +0 parent i32, +4 next i32 (preorder
  thread built by 0x00530cd0), +8 local 4x4 f32 matrix, +0x48 name char*,
  +0x4c three mins, +0x58 three maxs. The rest of the 0xe4-byte record (world
  matrices, inverses) is not compared here.
- 0x00532e70  animation (`.matrixdata`) reader, thiscall(ECX = 0x14-byte
  object, stack: buf, size), `ret 8`. Object: [0] bones, [1] keys per bone,
  [2] times f32*, [3] keys (7 f32 each, key-major `(key*bones+bone)*7`),
  [4] name pointers. Initialiser 0x00532b80 only zeroes five words.

Unicorn honours the control word's precision bits for FADD/FMUL (probed
here: PC24 and PC53 runs differ, see tests/oracle.rs); `--cw` regenerates the
vectors at another control word (suffix `_cw027f`).

Inputs are the real corpus files (stock + modded) read from disk at gen time
plus synthetic edge cases. Vectors are git-ignored (`re/out/skel/`): they
embed the file bytes (hex) and nothing derived from the original is
committed. Floats are recorded as u32 bit patterns, names as latin-1 text.

Row: {"id","exe_addr","name","source","input_hex","result","oracle",
      "unicorn","cw","date"}

Allocator hooks as in the other generators: malloc/free (0x54f560/0x54f5b0)
bump, realloc 0x54f580 copy-grow. Memory is zeroed so the buffer end reads
as NUL (the exe's line reader relies on a terminator).
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "skel")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
UC_VER = "2.1.4"
CW = CW_D3D9
SKEL = 0x531E90
SKEL_INIT = 0x5310B0
MATRIXDATA = 0x532E70


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _new_oracle(exe=None) -> Oracle:
    o = Oracle(exe or _default_exe())
    o.hook_alloc()

    def _realloc(oc, args):
        old, size = args[0], args[1]
        new = oc.alloc(max(size, 1))
        if old:
            n = oc.heap_sizes.get(old, 0)
            if n:
                oc.write(new, oc.read(old, min(n, size)))
        return new

    o.hook(0x54F580, _realloc, "cdecl", 2)
    return o


def _latin1(b: bytes) -> str:
    return b.decode("latin-1")


def run_skeleton(data: bytes, exe=None, cw=CW) -> dict:
    o = _new_oracle(exe)
    buf = o.alloc(len(data) + 16)
    o.write(buf, data)
    obj = o.alloc(0x18)
    o.call(SKEL_INIT, args=[obj], convention="fastcall", fpu_cw=cw)
    o.call(SKEL, args=[buf, len(data)], convention="thiscall", this=obj,
           fpu_cw=cw)
    bones_ptr, n, head = o.read_u32s(obj, 3)
    head = struct.unpack("<i", struct.pack("<I", head))[0]
    bones = []
    for i in range(n):
        b = bones_ptr + i * 0xE4
        parent = o.read_i32(b)
        nxt = o.read_i32(b + 4)
        matrix = o.read_u32s(b + 8, 16)
        name = _latin1(o.read_cstr(o.read_u32(b + 0x48)))
        lim = o.read_u32s(b + 0x4C, 6)
        bones.append({"parent": parent, "next": nxt, "name": name,
                      "matrix": matrix, "min": lim[:3], "max": lim[3:]})
    return {"bones": n, "head": head, "list": bones}


def run_matrixdata(data: bytes, exe=None, cw=CW) -> dict:
    o = _new_oracle(exe)
    buf = o.alloc(len(data) + 16)
    o.write(buf, data)
    obj = o.alloc(0x14)
    o.call(0x532B80, args=[obj], convention="fastcall", fpu_cw=cw)
    o.call(MATRIXDATA, args=[buf, len(data)], convention="thiscall",
           this=obj, fpu_cw=cw)
    nb, nk, tp, kp, np_ = o.read_u32s(obj, 5)
    times = o.read_u32s(tp, nk) if nk and tp else []
    keys = [o.read_u32s(kp + 28 * i, 7) for i in range(nb * nk)] \
        if nb and nk else []
    names = [_latin1(o.read_cstr(o.read_u32(np_ + 4 * i)))
             for i in range(nb)]
    return {"bones": nb, "keys": nk, "times": times, "key_list": keys,
            "names": names}


# --------------------------------------------------------------- inputs

def _f(v: float) -> str:
    return f"{v:.6f}"


def _bone_txt(name, parent, rows, tr, limits, nl="\r\n") -> bytes:
    lines = [name, str(parent)]
    lines += [", ".join(_f(x) for x in r) for r in rows]
    lines.append(", ".join(_f(x) for x in tr))
    for lo, hi in limits:
        lines.append(f"{_f(lo)}, {_f(hi)}")
    return (nl.join(lines) + nl).encode("latin-1")


IDENT = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
PI = 3.141593
FULL = [(-PI, PI)] * 3


def synthetic_skeletons():
    out = []
    out.append(("one_bone", _bone_txt("root", -1, IDENT, (1, 2, 3), FULL)))
    out.append(("chain_backref", b"".join([
        _bone_txt("a", -1, IDENT, (0, 0, 0), FULL),
        _bone_txt("b", 0, IDENT, (0, 24.5, 0), FULL),
        _bone_txt("c", 1, IDENT, (-3.25, 7, 0.125), [(-1, 1)] * 3)])))
    out.append(("forward_parent", b"".join([
        _bone_txt("leaf", 2, IDENT, (1, 0, 0), FULL),
        _bone_txt("mid", 2, IDENT, (0, 1, 0), FULL),
        _bone_txt("root", -1, IDENT, (0, 0, 1), FULL)])))
    out.append(("two_roots", b"".join([
        _bone_txt("bone00", -1, IDENT, (0, 0, 0), FULL),
        _bone_txt("bone01", -1, IDENT, (0, 24.5, 0), FULL)])))
    out.append(("fan_out", b"".join(
        [_bone_txt("root", -1, IDENT, (0, 0, 0), FULL)] +
        [_bone_txt(f"k{i}", 0, IDENT, (i, 0, 0), FULL)
         for i in range(1, 40)])))
    out.append(("name_with_spaces", _bone_txt(
        "Bip01 R Thigh", -1, IDENT, (9.936615, -11.649181, -0.026481),
        FULL)))
    # Limit line with a single number: sscanf count != 6 -> defaults.
    t = _bone_txt("short_limit", -1, IDENT, (0, 0, 0), FULL)
    lines = t.split(b"\r\n")
    lines[6] = b"-0.5"
    out.append(("limit_short_line", b"\r\n".join(lines)))
    # Missing separator space after the comma is still accepted by scanf.
    t = _bone_txt("nospace", -1, IDENT, (0, 0, 0), FULL)
    out.append(("limit_no_space", t.replace(b", ", b",")))
    out.append(("many_digits", _bone_txt(
        "rounding", -1, [[0.100000, 0.200000, 0.300000],
                         [0.7, 0.8, 0.9], [0.123457, 0.5, 0.25]],
        (0.1, 0.3, 123456.789012), FULL)))
    # LF-only line endings: the exe strips the last two characters of the
    # name line (CRLF assumed), so the name loses a real character.
    out.append(("lf_only", _bone_txt("abcdef", -1, IDENT, (1, 2, 3), FULL,
                                     nl="\n")))
    out.append(("long_name", _bone_txt("n" * 300, -1, IDENT, (0, 0, 0),
                                       FULL)))
    return out


def _bits(f):
    return struct.unpack("<I", struct.pack("<f", f))[0]


def _md(nbones, nkeys, names, vals=None, counts=None, times=None):
    out = struct.pack("<I", nbones)
    for b in range(nbones):
        c = counts[b] if counts else nkeys
        out += struct.pack("<I", c)
        for k in range(nkeys):
            t = times[k] if times else 0.1 * k
            v = vals(b, k) if vals else [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0]
            out += struct.pack("<8f", t, *v)
    out += b"".join(n.encode("latin-1") + b"\n" for n in names)
    return out


def synthetic_matrixdata():
    out = []
    out.append(("one_bone_one_key", _md(1, 1, ["Bip01"])))
    out.append(("three_bones_four_keys", _md(
        3, 4, ["a", "b b", "c"],
        vals=lambda b, k: [b * 0.1, k * 0.25, -1.5, 0.5, 0.0, 1.0, b + k])))
    out.append(("zero_bones", struct.pack("<I", 0)))
    # Later bones' key counts are never read by the exe: only bone 0's
    # count sizes the arrays and later counts are skipped.
    out.append(("ignored_later_counts", _md(
        2, 3, ["a", "b"], counts=[3, 9])))
    out.append(("long_name", _md(1, 1, ["n" * 300])))
    out.append(("crlf_names", _md(2, 1, ["a\r", "b\r"])))
    return out


def corpus_files():
    roots = []
    for env, default in (("SLR_STOCK", "stock"),
                         ("SLR_REFERENCE", os.path.join("reference", "raw"))):
        cands = [os.environ.get(env), os.path.join(REPO, default),
                 os.path.join(MAIN_CHECKOUT, default)]
        for c in cands:
            if c and os.path.isdir(c):
                roots.append((default.split(os.sep)[0], c))
                break
    seen = {}
    for label, root in roots:
        for dp, _, fns in os.walk(root):
            for fn in sorted(fns):
                ext = fn.rsplit(".", 1)[-1].lower() if "." in fn else ""
                if ext in ("bon", "skl", "matrixdata"):
                    p = os.path.join(dp, fn)
                    data = open(p, "rb").read()
                    key = (ext, data)
                    rel = f"{label}/{os.path.relpath(p, root)}"
                    if key not in seen:
                        seen[key] = rel
    return [(ext, rel, data) for (ext, data), rel in sorted(
        seen.items(), key=lambda kv: kv[1])]


def build_cases():
    cases = []
    for ext, rel, data in corpus_files():
        kind = "matrixdata" if ext == "matrixdata" else "skeleton"
        cases.append((kind, rel, data))
    for label, data in synthetic_skeletons():
        cases.append(("skeleton", f"synthetic:{label}", data))
    for label, data in synthetic_matrixdata():
        cases.append(("matrixdata", f"synthetic:{label}", data))
    return cases


RUN = {"skeleton": (SKEL, run_skeleton), "matrixdata": (MATRIXDATA,
                                                         run_matrixdata)}


def gen(exe=None, cw=CW):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = {"skeleton": [], "matrixdata": []}
    suffix = "" if cw == CW else f"_cw{cw:04x}"
    for kind, source, data in build_cases():
        addr, fn = RUN[kind]
        res = fn(data, exe, cw)
        rows[kind].append({
            "id": len(rows[kind]), "exe_addr": f"0x{addr:08x}",
            "name": kind, "source": source, "input_hex": data.hex(),
            "result": res, "oracle": "unicorn", "unicorn": UC_VER,
            "cw": f"0x{cw:04x}", "date": _today()})
    for kind, rs in rows.items():
        path = os.path.join(OUT_DIR, f"skel_{kind}{suffix}.jsonl")
        with open(path, "w") as f:
            for r in rs:
                f.write(json.dumps(r) + "\n")
        print(f"{kind}: {len(rs)} rows -> {path}")


def verify(files, exe=None):
    bad = n = 0
    for path in files:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        for row in rows:
            n += 1
            _, fn = RUN[row["name"]]
            got = fn(bytes.fromhex(row["input_hex"]), exe,
                     int(row["cw"], 16))
            if got != row["result"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['source']}")
    print(f"verify: {n - bad}/{n} byte-identical")
    return 1 if bad else 0


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    g.add_argument("--cw", type=lambda x: int(x, 0), default=CW,
                   help="x87 control word (0x7f = PC24 device thread, "
                        "0x27f = PC53 worker thread)")
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe, a.cw)
        return 0
    files = a.files or sorted(os.path.join(OUT_DIR, f) for f in
                              os.listdir(OUT_DIR) if f.endswith(".jsonl"))
    return verify(files, a.exe)


if __name__ == "__main__":
    sys.exit(main())
