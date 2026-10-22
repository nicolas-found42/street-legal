#!/usr/bin/env python3
"""Generate input vectors, run them through the Unicorn oracle, round-trip bit for bit.

Two acceptance drivers (ticket M0-08, issue #9):

  leaf-float:  FUN_00436950 (all_pseudo.c:34297-34303), thiscall 2-vector dot::
                 return (float10)*param_2 * (float10)*param_1
                      + (float10)param_2[1] * (float10)param_1[1];
               this=float[2], one stack arg=float[2]*, ST0 f80 result.
               fmul/fadd only -> PC-insensitive (verified identical ST0 bytes
               at CW 0x007F and 0x027F), so the Unicorn leg is authoritative.
  int-smoke:   FUN_00570330 (all_pseudo.c:176046-176052), cdecl ceil_div::
                 return (param_1 + -1 + param_2) / param_2;
               pure integer divide, no FPU involvement at all.

Vector file format (one JSON object per line, under re/out/oracle/ which is
git-ignored via /re/out; field names follow re/x87probe conventions where
they overlap -- id, results, cw -- and add exe_addr/oracle for the consumer):

  {"id": 0, "exe_addr": "0x436950", "name": "dot2_F32x2",
   "convention": "thiscall", "args": [{"ptr": "f32", "values": [1.0, 2.0]},
                                      {"ptr": "f32", "values": [3.0, 4.0]}],
   "cw": "0x7f", "result": {"st0": "0x4002b000000000000000", "eax": "0x0"},
   "writes": [], "oracle": "unicorn", "unicorn": "2.1.4",
   "date": "2026-10-17"}

Bit-exactness contract: floats are encoded with struct.pack('<f') and results
compared as raw bytes (f32 bits for memory outputs, 10-byte ST0 for register
outputs, u32 for integer outputs). `verify` re-runs every row from the
encoded inputs on a fresh Oracle and requires byte-identical outputs.

Usage (from the repo root, venv at ~/slr-oracle-venv):
  ~/slr-oracle-venv/bin/python re/oracle/run_vectors.py gen --suite leaf-float
  ~/slr-oracle-venv/bin/python re/oracle/run_vectors.py verify re/out/oracle/dot2_f32x2.jsonl
  ~/slr-oracle-venv/bin/python re/oracle/run_vectors.py gen --suite int-smoke && \
    ~/slr-oracle-venv/bin/python re/oracle/run_vectors.py verify re/out/oracle/ceil_div_i32.jsonl

Never writes to reference/ or stock/; the exe is only read.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import CW_D3D9, Oracle
from unicorn import __version__ as _uc_ver  # noqa: F401  (recorded per row)

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT):
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    return os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe")


DEFAULT_EXE = _default_exe()
OUT_DIR = os.path.join(REPO, "re", "out", "oracle")

DOT2_ADDR = 0x436950
CEILDIV_ADDR = 0x570330


def _st0hex(raw: bytes) -> str:
    return "0x" + raw.hex()


def _today() -> str:
    return datetime.date.today().isoformat()


# ---------------------------------------------------------------- generators
def gen_dot2(n: int = 64) -> list[dict]:
    """Deterministic 2-vector dot inputs: edges, small ints, f32-inexact, big/small."""
    edge = [
        ([0.0, 0.0], [0.0, 0.0]),
        ([1.0, 0.0], [0.0, 1.0]),
        ([1.0, 2.0], [3.0, 4.0]),
        ([-1.0, 2.0], [3.0, -4.0]),
        ([0.1, 0.2], [0.3, 0.4]),
        ([1e10, 1e-10], [1e10, 1e-10]),
        ([3.4028235e38, 1.0], [1.0, 1.0]),     # f32 max (finite; no overflow in f80)
        ([1.1754944e-38, 1.4e-45], [2.0, 3.0]),  # f32 min-normal, min-subnormal
        ([-0.0, 0.0], [0.0, -0.0]),
    ]
    rows: list[dict] = []
    for a, b in edge:
        rows.append({"a": a, "b": b})
    # deterministic pseudo-random fill (LCG, no dependency on random module state)
    s = 0x12345678
    while len(rows) < n:
        vals = []
        for _ in range(4):
            s = (1103515245 * s + 12345) & 0xFFFFFFFF
            mag = (s % 2001) - 1000  # -1000..1000
            s = (1103515245 * s + 12345) & 0xFFFFFFFF
            frac = (s % 1000) / 1000.0
            vals.append(mag + (frac if mag >= 0 else -frac))
        rows.append({"a": vals[:2], "b": vals[2:]})
    return rows[:n]


def gen_ceildiv(n: int = 64) -> list[dict]:
    edge = [(0, 1), (1, 1), (7, 3), (8, 4), (9, 4), (100, 7), (2**31 - 1, 2),
            (2**31 - 1, 1), (1000000, 3), (5, 5), (6, 5), (1, 1000)]
    rows = [{"a": a, "b": b} for a, b in edge]
    s = 0x87654321
    while len(rows) < n:
        s = (1103515245 * s + 12345) & 0xFFFFFFFF
        a = s % 100000
        s = (1103515245 * s + 12345) & 0xFFFFFFFF
        b = 1 + (s % 997)
        rows.append({"a": a, "b": b})
    return rows[:n]


# ---------------------------------------------------------------- run + encode
def run_dot2(oracle: Oracle, a: list[float], b: list[float]) -> dict:
    # Only ST0 is a defined output here (EAX is a scratch leftover: the
    # function returns float10 in ST0 and never writes EAX).
    pa = oracle.scratch(8)
    oracle.write_f32s(pa, a)
    pb = oracle.scratch(8)
    oracle.write_f32s(pb, b)
    r = oracle.call(DOT2_ADDR, args=[pb], convention="thiscall", this=pa)
    if r.mem_writes:
        raise AssertionError(f"dot2 wrote memory: {r.mem_writes}")
    return {"st0": _st0hex(r.st0_raw)}


def run_ceildiv(oracle: Oracle, a: int, b: int) -> dict:
    # Only EAX is a defined output (EDX is the idiv remainder leftover).
    r = oracle.call(CEILDIV_ADDR, args=[a, b], convention="cdecl")
    return {"eax": f"0x{r.eax:08x}"}


def cmd_gen(ns) -> int:
    import unicorn

    os.makedirs(OUT_DIR, exist_ok=True)
    oracle = Oracle(ns.exe, fpu_cw=ns.cw)
    date = _today()
    ucver = unicorn.__version__
    if ns.suite == "instance-query":
        from instance_query_vectors import run, inputs
        path = os.path.join(OUT_DIR, "instance_query.jsonl")
        count = max(ns.n, len(inputs()))
        with open(path, "w") as f:
            for i in range(count):
                result = run(i, exe=ns.exe, cw=ns.cw)
                f.write(json.dumps(dict(id=i, exe_addr="0x426470/0x427210", name="instance_query",
                    args=dict(case=i), cw=hex(ns.cw), oracle="unicorn", unicorn=ucver,
                    date=date, result=result), sort_keys=True) + "\n")
        print(f"wrote {count} rows -> {path}")
    elif ns.suite == "car-update-active":
        from car_update_active_probe import run
        path = os.path.join(OUT_DIR, "car_update_active.jsonl")
        with open(path, "w") as f:
            for i in range(ns.n):
                result = run(i, exe=ns.exe, cw=ns.cw)
                f.write(json.dumps(dict(id=i, exe_addr="0x448430", name="car_update_active",
                    args=dict(case=i), cw=hex(ns.cw), oracle="unicorn", unicorn=ucver,
                    date=date, result=result), sort_keys=True) + "\n")
        print(f"wrote {ns.n} rows -> {path}")
    elif ns.suite == "leaf-float":
        path = os.path.join(OUT_DIR, "dot2_f32x2.jsonl")
        rows = gen_dot2(ns.n)
        with open(path, "w") as f:
            for i, v in enumerate(rows):
                res = run_dot2(oracle, v["a"], v["b"])
                f.write(json.dumps({
                    "id": i, "exe_addr": hex(DOT2_ADDR), "name": "dot2_F32x2",
                    "convention": "thiscall",
                    "args": [{"ptr": "f32", "values": v["a"]},
                             {"ptr": "f32", "values": v["b"]}],
                    "cw": hex(ns.cw), "result": res, "writes": [],
                    "oracle": "unicorn", "unicorn": ucver, "date": date,
                }) + "\n")
        print(f"wrote {len(rows)} rows -> {path}")
    else:
        path = os.path.join(OUT_DIR, "ceil_div_i32.jsonl")
        rows = gen_ceildiv(ns.n)
        with open(path, "w") as f:
            for i, v in enumerate(rows):
                res = run_ceildiv(oracle, v["a"], v["b"])
                f.write(json.dumps({
                    "id": i, "exe_addr": hex(CEILDIV_ADDR), "name": "ceil_div_I32",
                    "convention": "cdecl",
                    "args": [{"int": v["a"]}, {"int": v["b"]}],
                    "cw": hex(ns.cw), "result": res, "writes": [],
                    "oracle": "unicorn", "unicorn": ucver, "date": date,
                }) + "\n")
        print(f"wrote {len(rows)} rows -> {path}")
    return 0


def _decode_and_run(oracle: Oracle, row: dict) -> dict:
    """Re-run one vector row from its encoded inputs (no access to stored result)."""
    if row["name"] == "instance_query":
        from instance_query_vectors import run
        if oracle.fpu_cw != int(row["cw"], 0):
            raise ValueError("control word differs from recorded instance query vector")
        return run(row["args"]["case"], exe=oracle.exe, cw=oracle.fpu_cw)
    if row["name"] == "car_update_active":
        from car_update_active_probe import run
        if oracle.fpu_cw != int(row["cw"], 0):
            raise ValueError("control word differs from recorded car update vector")
        return run(row["args"]["case"], exe=oracle.exe, cw=oracle.fpu_cw)
    if row["name"] == "dot2_F32x2":
        # Bit-exact re-encode: JSON doubles re-packed to f32 exactly as gen did.
        a = [struct.unpack("<f", struct.pack("<f", v))[0] for v in row["args"][0]["values"]]
        b = [struct.unpack("<f", struct.pack("<f", v))[0] for v in row["args"][1]["values"]]
        return run_dot2(oracle, a, b)
    if row["name"] == "ceil_div_I32":
        return run_ceildiv(oracle, row["args"][0]["int"], row["args"][1]["int"])
    raise ValueError(f"unknown vector {row['name']}")


def cmd_verify(ns) -> int:
    oracle = Oracle(ns.exe, fpu_cw=ns.cw)
    n = bad = 0
    with open(ns.vectors) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            got = _decode_and_run(oracle, row)
            n += 1
            if got != row["result"]:
                bad += 1
                print(f"MISMATCH id={row['id']}: stored={row['result']} rerun={got}")
    print(f"verify: {n - bad}/{n} byte-identical ({ns.vectors})")
    return 1 if bad else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen", help="generate + run vectors")
    g.add_argument("--suite", required=True, choices=["leaf-float", "int-smoke", "car-update-active", "instance-query"])
    g.add_argument("-n", type=int, default=64)
    g.add_argument("--cw", type=lambda s: int(s, 0), default=CW_D3D9)
    g.add_argument("--exe", default=DEFAULT_EXE)
    v = sub.add_parser("verify", help="re-run vectors, require byte-identical results")
    v.add_argument("vectors")
    v.add_argument("--cw", type=lambda s: int(s, 0), default=CW_D3D9)
    v.add_argument("--exe", default=DEFAULT_EXE)
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        return cmd_gen(a)
    return cmd_verify(a)


if __name__ == "__main__":
    sys.exit(main())
