"""Referee for the Rosetta leg: SoftFloat vs Rosetta-probe (Wine) results.

Joins re/out/x87/sf_results.jsonl (M0-07 SoftFloat oracle) with the Rosetta
probe output (re/out/wine/logs/probe-stdout.log, copied to
re/out/x87/rosetta_results.jsonl by run_rosetta.sh) by vector id, using the
same NaN-tolerant rules as compare.py (imported). Writes
re/out/x87/rosetta_results.json + rosetta_disagreements.json and prints the
per-op agree/mismatch table that docs/x87.md's Rosetta leg reports.

Transcendentals compare Rosetta vs mpmath (same mpmath_ref as compare.py),
since SoftFloat has no transcendentals.
"""
from __future__ import annotations

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import compare as C

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.abspath(os.path.join(HERE, "..", "out", "x87"))
ROSETTA_IN = os.path.join(os.path.dirname(OUT_DIR), "wine", "logs", "probe-stdout.log")
ROSETTA_COPY = os.path.join(OUT_DIR, "rosetta_results.jsonl")


def main() -> None:
    import shutil
    if os.path.exists(ROSETTA_IN):
        shutil.copyfile(ROSETTA_IN, ROSETTA_COPY)
    sf = C.load_results(os.path.join(OUT_DIR, "sf_results.jsonl"))
    rt = C.load_results(ROSETTA_COPY)
    corpus = {}
    with open(os.path.join(OUT_DIR, "corpus.jsonl")) as f:
        for line in f:
            v = json.loads(line)
            corpus[v["id"]] = v
    print(f"sf={len(sf)} rosetta={len(rt)} corpus={len(corpus)}")
    stats: dict[str, dict[str, int]] = {}
    disag: list[dict] = []

    def bump(op, cell, key):
        stats.setdefault(op, {}).setdefault(f"{cell}:{key}", 0)
        stats[op][f"{cell}:{key}"] += 1

    mp_cache: dict[tuple, object] = {}
    for vid, v in sorted(corpus.items()):
        op, pc, rc = v["op"], v["pc"], v["rc"]
        s, u = sf.get(vid), rt.get(vid)
        cell = f"PC{pc}/RC{rc}"
        if s is None or u is None:
            bump(op, cell, "missing-oracle")
            continue
        if "error" in u:
            bump(op, cell, "rosetta-error")
            disag.append({"id": vid, "op": op, "cell": cell, "kind": "rosetta-error",
                          "detail": u["error"]})
            continue
        if op in ("fadd", "fsub", "fmul", "fdiv", "fsqrt", "fld", "fst"):
            ok, detail = C.cmp_reg(s, u)
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": "rosetta-vs-sf: " + detail,
                              "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
            if op == "fst" and ok and "m64" in u and "m64" in s:
                if s["m64"] != u["m64"]:
                    disag.append({"id": vid, "op": op, "cell": cell, "kind": "m64-MISMATCH",
                                  "detail": f"sf={s['m64']} rt={u['m64']}",
                                  "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op in ("fsin", "fcos", "fpatan", "f2xm1", "fyl2x"):
            key = (op, json.dumps(v["a"], sort_keys=True), json.dumps(v.get("b"), sort_keys=True), rc)
            if key not in mp_cache:
                a = C.f80_to_mpf(v["a"])
                b = C.f80_to_mpf(v["b"]) if v.get("b") is not None else None
                try:
                    mp_cache[key] = C.mpmath_ref(op, a, b, rc)
                except Exception as e:  # noqa: BLE001
                    mp_cache[key] = f"mpmath-error: {e}"
            ref = mp_cache[key]
            us, ub, ug = C.f80_parts(u["st"])
            un = C.is_nan_parts(us, ub, ug)
            if isinstance(ref, str) or ref is None or (isinstance(ref, tuple) and ref[0] == "inf"):
                bump(op, cell, f"special({ref})")
                continue
            rs, rb, rg = ref
            rn = rb == 0x7FFF
            if un or rn:
                bump(op, cell, "agree-nan" if (un and rn) else "nan-one-side")
                if not (un and rn):
                    disag.append({"id": vid, "op": op, "cell": cell, "kind": "nan-one-side",
                                  "detail": f"mp_nan={rn} rt_nan={un}"})
                continue
            ok = (rs, rb, rg) == (us, ub, ug)
            bump(op, cell, "agree" if ok else "MISMATCH-vs-mpmath")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH-vs-mpmath",
                              "detail": f"mp=({rs},{rb:#x},0x{rg:016x}) rt=({us},{ub:#x},0x{ug:016x})"})
        elif op == "fprem":
            ok, detail = C.cmp_reg(s, u)
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": "rosetta-vs-sf: " + detail,
                              "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op in ("fcom", "fcomp", "fucom"):
            # sf harness reports cc C-bits; rosetta matches unicorn layout
            ok = s["cc"] == {"C3": u["cc"]["C3"], "C2": u["cc"]["C2"], "C0": u["cc"]["C0"]}
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": f"sf={s['cc']} rt={u['cc']}"})
        elif op == "fucomi":
            ucc = u.get("cc", {})
            sm = {"C3": ucc.get("ZF"), "C2": ucc.get("PF"), "C0": ucc.get("CF")}
            ok = sm == s["cc"]
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": f"sf={s['cc']} rt={ucc}"})
        elif op == "fistp":
            ok32 = (s["i32"] == u["i32"])
            exp64 = C.expected_i64(v["a"], rc)
            ok64 = (exp64 is None) or (u["i64"] == exp64)
            ok = ok32 and ok64
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": f"sf_i32={s['i32']} rt_i32={u['i32']} rt_i64={u.get('i64')} exp64={exp64}"})
        else:
            bump(op, cell, "unknown-op")

    with open(os.path.join(OUT_DIR, "rosetta_results.json"), "w") as f:
        json.dump(stats, f, indent=2, sort_keys=True)
    with open(os.path.join(OUT_DIR, "rosetta_disagreements.json"), "w") as f:
        json.dump(disag, f, indent=2)
    total_agree = sum(v for op in stats.values() for k, v in op.items() if k.endswith(":agree"))
    total_mm = sum(v for k in ("MISMATCH", "MISMATCH-vs-mpmath", "m64-MISMATCH") for op in stats.values() for kk, v in op.items() if kk.endswith(":" + k))
    print(f"agree={total_agree} mismatch={total_mm} disag-records={len(disag)}")
    for op in sorted(stats):
        s = stats[op]
        ag = sum(v for k, v in s.items() if k.endswith(":agree"))
        mm = sum(v for k, v in s.items() if "MISMATCH" in k)
        other = sum(v for k, v in s.items() if not (k.endswith(":agree") or "MISMATCH" in k))
        print(f"  {op:8s} agree={ag} mismatch={mm} other={other}")


if __name__ == "__main__":
    main()
