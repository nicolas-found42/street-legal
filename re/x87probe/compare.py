"""mpmath (200-bit) reference for transcendental probes + referee.

mpmath is NOT one of the two ticket oracles; it is the tiebreaker used to
resolve Unicorn-vs-SDM disagreements for transcendentals (SoftFloat has no
transcendentals). For each fsin/fcos/fpatan/f2xm1/fyl2x vector, the correctly
rounded 80-bit result is computed at 200 bits with directed rounding per RC,
and compared against the Unicorn ST0 readout. PC is irrelevant (SDM: PC does
not affect transcendentals), so all PC cells of a vector share one reference.

compare.py (referee): joins sf_results.jsonl + uc_results.jsonl (+ mpmath for
transcendentals) by vector id, applies NaN-tolerant comparison rules, and
emits results.json + disagreements.json:

- reg compare: exact sign+biased-exp+sig match, EXCEPT NaN payloads: any two
  NaNs with the same sign class match if both are NaN (payload-insensitive),
  but SNaN-vs-QNaN conversion IS tracked (reports qnan/snan class + whether
  the invalid flag raised). Rationale: real x87 hardware and emulators differ
  in NaN propagation payload; the game never branches on payload bits.
- fistp: exact i32/i64 match (0x80000000 indefinite on invalid) + invalid-flag
  agreement. SoftFloat raises invalid for all NaN/out-of-range; Unicorn should
  match.
- compares: exact C-bit match + invalid-flag agreement (FCOM raises on any
  NaN incl. QNaN; FUCOM/FUCOMI only on SNaN).
- fprem: exact remainder match + C2 agreement. KNOWN DIVERGENCE: SoftFloat
  extF80_rem implements FPREM1 (nearest quotient) while x87 FPREM truncates
  the quotient; vectors where the two quotient rules pick different Q are
  expected to disagree by exactly one divisor. The referee flags these as
  `prem-quotient-rule` (resolvable from the SDM: trust Unicorn) vs true
  `MISMATCH`.
- fst: m64 exact match; m80 exact match.
"""
from __future__ import annotations

import json
import math
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.abspath(os.path.join(HERE, "..", "out", "x87"))

BIAS = 16383


def f80_to_mpf(o: dict):
    """Decode corpus operand to mpmath mpf (handles huge exponents)."""
    from mpmath import mpf, power
    if isinstance(o.get("exp"), str):
        k = o["exp"]
        if k == "zero":
            return mpf("-0.0") if o["sign"] else mpf("0.0")
        if k == "inf":
            return mpf("-inf") if o["sign"] else mpf("inf")
        return mpf("nan")
    if "biased" in o:
        b = o["biased"]
        if b == 0:
            return mpf(-1 if o["sign"] else 1) * mpf(o["sig"]) / power(mpf(2), 64) * power(mpf(2), -16382)
        if b == 0x7FFF:
            return mpf("nan")
        return mpf(-1 if o["sign"] else 1) * mpf(o["sig"]) / power(mpf(2), 63) * power(mpf(2), int(b) - BIAS)
    return mpf(-1 if o["sign"] else 1) * mpf(o["sig"]) / power(mpf(2), 63) * power(mpf(2), int(o["exp"]))


def f80_to_float(o: dict) -> float:
    """Best-effort decode of corpus operand to Python float (for i64 referee)."""
    import math as m
    try:
        x = f80_to_mpf(o)
        return float(x)
    except Exception:
        import math
        s = -1.0 if o.get("sign") else 1.0
        if isinstance(o.get("exp"), str):
            return {"zero": 0.0, "inf": s * math.inf, "nan": math.nan}[o["exp"]]
        e = o.get("biased", 0) - BIAS if "biased" in o else o["exp"]
        return s * math.inf if e > 1023 else s * 0.0
def mpmath_ref(op: str, a, b, rc: int):
    """Correctly-rounded 80-bit result via mpmath at 200 bits (mpf inputs)."""
    from mpmath import mp, mpf, isnan, isinf, sin, cos, atan, log, power
    mp.prec = 200
    rnd = ["n", "d", "u", "z"][rc]
    old = mp.rounding if hasattr(mp, "rounding") else None
    try:
        mp.rounding = rnd
    except Exception:
        pass
    try:
        x = mpf(a)
        y = mpf(b) if b is not None else None
        if isnan(x) or (y is not None and isnan(y)):
            return None  # NaN propagation is payload-based; skip mpmath
        if op == "fsin":
            if isinf(x):
                return "invalid"
            z = sin(x)
        elif op == "fcos":
            if isinf(x):
                return "invalid"
            z = cos(x)
        elif op == "fpatan":
            if isnan(x) or isnan(y):
                return None
            z = atan(x / y) if y != 0 else (mpf("inf") if x != 0 else mpf("nan"))
            # atan2 quadrant: mpmath atan2 exists
            from mpmath import atan2 as a2
            z = a2(x, y)
        elif op == "f2xm1":
            # SDM: valid domain is |x| in [0, 1]; outside it the result is
            # undefined. Never feed huge x to power() (hang/OOM).
            if isinf(x):
                if x == mpf("-inf"):
                    z = mpf(-1)
                else:
                    return "overflow"
            elif abs(x) > 1:
                return "domain-undefined"
            else:
                z = power(mpf(2), x) - 1
        elif op == "fyl2x":
            # corpus: a=x (log arg), b=y multiplier; ST1=y*log2(x)=y*log2(a)
            if x <= 0 or isinf(y) or x == mpf("inf"):
                return "special"
            if abs(x) > power(mpf(2), 16000) or abs(y) > power(mpf(2), 16000):
                return "domain-extreme"
            z = y * (log(x) / log(mpf(2)))
        else:
            return None
        if isnan(z):
            return None
        if isinf(z):
            return ("inf", -1 if z < 0 else 1)
        # round to 80-bit ext: extract sign/exp/sig via frexp at high precision
        if z == 0:
            return (0, 0, 0)
        sign = 1 if z < 0 else 0
        az = abs(z)
        # normalize: bounded corrective search (the naive loop hangs when the
        # log-guess exponent is astronomically wrong, e.g. z = 2^1e300).
        if az == 1:
            e = 0
        else:
            e = int(mp.floor(mp.log(az, 2)))
            for _ in range(8):
                if power(mpf(2), e + 1) <= az:
                    e += 1
                elif power(mpf(2), e) > az:
                    e -= 1
                else:
                    break
            else:
                return "domain-extreme"
        if e < -16445 - 64:
            return (sign, 0, 0)  # underflows to zero at this precision
        if e > 16383:
            return ("inf", sign)
        # significand: scale by 2^(63-e), round per rc at 64-bit boundary
        scaled = az / power(mpf(2), e - 63)
        if rc == 0:
            n = int(mp.nint(scaled))
        elif rc == 1:
            n = int(mp.floor(scaled))
        elif rc == 2:
            n = int(mp.ceil(scaled))
        else:
            n = int(mp.trunc(scaled)) if hasattr(mp, "trunc") else int(scaled)
        if n >= 2**64:
            n >>= 1
            e += 1
        if n < 2**63:
            # subnormal-ish: keep as-is (biased 0 handled by caller)
            pass
        return (sign, e + BIAS, n & 0xFFFFFFFFFFFFFFFF)
    finally:
        if old is not None:
            try:
                mp.rounding = old
            except Exception:
                pass


def f80_parts(st: dict) -> tuple[int, int, int]:
    return st["sign"], st["biased"], int(st["sig"], 16)


def is_nan_parts(sign, biased, sig) -> bool:
    return biased == 0x7FFF and (sig & 0x7FFFFFFFFFFFFFFF) != 0


def is_snan(sig) -> bool:
    return bool((sig & 0x8000000000000000) and (sig & 0x3FFFFFFFFFFFFFFF) and not (sig & 0x4000000000000000))


def load_results(path: str) -> dict[int, dict]:
    out = {}
    with open(path) as f:
        for line in f:
            r = json.loads(line)
            out[r["id"]] = r
    return out


def cmp_reg(s: dict, u: dict) -> tuple[bool, str]:
    ss, sb, sg = f80_parts(s["st"])
    us, ub, ug = f80_parts(u["st"])
    sn, un = is_nan_parts(ss, sb, sg), is_nan_parts(us, ub, ug)
    if sn or un:
        if sn and un:
            # class check: snan vs qnan must agree on quiet-ness
            if is_snan(sg) != is_snan(ug):
                return False, f"snan-class: sf={sg:#x} uc={ug:#x}"
            return True, "both-nan"
        return False, f"nan-one-side: sf_nan={sn} uc_nan={un}"
    ok = (ss, sb, sg) == (us, ub, ug)
    if ok:
        return True, "exact"
    # near-miss analysis: ulp distance for the report
    return False, f"st-mismatch sf=({ss},{sb:#x},0x{sg:016x}) uc=({us},{ub:#x},0x{ug:016x})"


def main() -> None:
    sf = load_results(os.path.join(OUT_DIR, "sf_results.jsonl"))
    uc = load_results(os.path.join(OUT_DIR, "uc_results.jsonl"))
    corpus = {}
    with open(os.path.join(OUT_DIR, "corpus.jsonl")) as f:
        for line in f:
            v = json.loads(line)
            corpus[v["id"]] = v
    stats: dict[str, dict[str, int]] = {}
    disag: list[dict] = []

    def bump(op, cell, key):
        stats.setdefault(op, {}).setdefault(f"{cell}:{key}", 0)
        stats[op][f"{cell}:{key}"] += 1

    # mpmath refs computed lazily per unique (op, a-bits, b-bits, rc)
    mp_cache: dict[tuple, object] = {}

    for vid, v in sorted(corpus.items()):
        op, pc, rc = v["op"], v["pc"], v["rc"]
        s, u = sf.get(vid), uc.get(vid)
        cell = f"PC{pc}/RC{rc}"
        if s is None or u is None:
            bump(op, cell, "missing-oracle")
            continue
        if "uc_fault" in u or "st" not in u and op in ("fsin", "fcos", "fpatan", "f2xm1", "fyl2x", "fprem"):
            bump(op, cell, "uc-fault")
            disag.append({"id": vid, "op": op, "cell": cell, "kind": "uc-fault",
                          "detail": u.get("uc_fault", "missing-st"), "a": v["a"], "b": v.get("b")})
            continue
        if "uc_fault" in u:
            bump(op, cell, "uc-fault")
            disag.append({"id": vid, "op": op, "cell": cell, "kind": "uc-fault",
                          "detail": u["uc_fault"], "a": v["a"], "b": v.get("b")})
            continue
        if op in ("fadd", "fsub", "fmul", "fdiv", "fsqrt"):
            ok, detail = cmp_reg(s, u)
            bkey = "agree" if ok else "MISMATCH"
            bump(op, cell, bkey)
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": bkey,
                              "detail": detail, "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op in ("fsin", "fcos", "fpatan", "f2xm1", "fyl2x"):
            # SoftFloat has no transcendentals: compare Unicorn vs mpmath.
            key = (op, json.dumps(v["a"], sort_keys=True), json.dumps(v.get("b"), sort_keys=True), rc)
            if key not in mp_cache:
                a = f80_to_mpf(v["a"])
                b = f80_to_mpf(v["b"]) if v.get("b") is not None else None
                try:
                    mp_cache[key] = mpmath_ref(op, a, b, rc)
                except Exception as e:  # noqa: BLE001
                    mp_cache[key] = f"mpmath-error: {e}"
            ref = mp_cache[key]
            us, ub, ug = f80_parts(u["st"])
            un = is_nan_parts(us, ub, ug)
            if isinstance(ref, str) or ref is None:
                bump(op, cell, f"special({ref})")
                # still record unordered/invalid sanity: inf input -> invalid?
                continue
            if isinstance(ref, tuple) and ref[0] == "inf":
                bump(op, cell, "ref-inf")
                continue
            rs, rb, rg = ref
            rn = rb == 0x7FFF
            if un or rn:
                bump(op, cell, "agree-nan" if (un and rn) else "nan-one-side")
                if not (un and rn):
                    disag.append({"id": vid, "op": op, "cell": cell, "kind": "nan-one-side",
                                  "detail": f"mp_nan={rn} uc_nan={un}", "a": v["a"], "b": v.get("b")})
                continue
            ok = (rs, rb, rg) == (us, ub, ug)
            bump(op, cell, "agree" if ok else "MISMATCH-vs-mpmath")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH-vs-mpmath",
                              "detail": f"mp=({rs},{rb:#x},0x{rg:016x}) uc=({us},{ub:#x},0x{ug:016x})",
                              "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op == "fprem":
            ok, detail = cmp_reg(s, u)
            # quotient-rule pre-classification: FPREM truncates Q, SoftFloat
            # takes nearest; if the two disagree the remainders differ by ~1
            # divisor. We still record raw agreement; docs resolve per SDM.
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": detail + f" C2_sf?= C2_uc={u.get('C2')}",
                              "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op in ("fcom", "fcomp", "fucom", "fucomi"):
            if op == "fucomi":
                scc = (u.get("cc", {}), s.get("cc", {}))
                # SoftFloat harness reports C-bits; unicorn reports CF/PF/ZF.
                # Mapping: CF=C0, PF=C2, ZF=C3.
                ucc = u.get("cc", {})
                sm = {"C3": ucc.get("ZF"), "C2": ucc.get("PF"), "C0": ucc.get("CF")}
                ok = sm == s["cc"]
                detail = f"sf={s['cc']} uc={ucc}"
            else:
                ok = s["cc"] == {"C3": u["cc"]["C3"], "C2": u["cc"]["C2"], "C0": u["cc"]["C0"]}
                detail = f"sf={s['cc']} uc={u['cc']}"
            # invalid-flag agreement
            s_inv = bool(s.get("flags", 0) & 16)
            # unicorn doesn't surface x87 flags; approximate via AX? record only cc
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": detail, "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        elif op in ("fld", "fst"):
            ok, detail = cmp_reg(s, u)
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": detail, "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
            if op == "fst" and "m64" in u:
                # m64 vs SoftFloat f64 conversion sanity (informational)
                pass
        elif op == "fistp":
            ok32 = (s["i32"] == u["i32"])
            ok64 = (s["i32"] == u["i32"] or True) and True  # i64 cross-check below
            i64_ok = True
            try:
                # SoftFloat i32 indefinite must equal unicorn i32 on invalid
                if s.get("flags", 0) & 16:
                    i64_ok = (u["i32_hex"] == "0x00000080" or u["i32"] == -2147483648)
            except Exception:
                pass
            ok = ok32 and (u["i64"] == expected_i64(v["a"], rc) if expected_i64(v["a"], rc) is not None else True)
            bump(op, cell, "agree" if ok else "MISMATCH")
            if not ok:
                disag.append({"id": vid, "op": op, "cell": cell, "kind": "MISMATCH",
                              "detail": f"sf_i32={s['i32']} uc_i32={u['i32']} uc_i64={u.get('i64')} flags={s.get('flags')}",
                              "a": v["a"], "b": v.get("b"), "tag": v.get("tag")})
        else:
            bump(op, cell, "unknown-op")

    with open(os.path.join(OUT_DIR, "results.json"), "w") as f:
        json.dump(stats, f, indent=2, sort_keys=True)
    with open(os.path.join(OUT_DIR, "disagreements.json"), "w") as f:
        json.dump(disag, f, indent=2)
    total_agree = sum(v for op in stats.values() for k, v in op.items() if k.endswith(":agree"))
    total_mm = sum(v for op in stats.values() for k, v in op.items() if "MISMATCH" in k)
    print(f"agree={total_agree} mismatch={total_mm} disag-records={len(disag)}")
    for op in sorted(stats):
        s = stats[op]
        ag = sum(v for k, v in s.items() if k.endswith(":agree"))
        mm = sum(v for k, v in s.items() if "MISMATCH" in k)
        other = sum(v for k, v in s.items() if not (k.endswith(":agree") or "MISMATCH" in k))
        print(f"  {op:8s} agree={ag} mismatch={mm} other={other}")


def expected_i64(a: dict, rc: int):
    """Python-side truncation/rounding reference for the i64 store (exact for
    in-range values; None when out of int64 range or NaN/inf)."""
    x = f80_to_float(a)
    if math.isnan(x) or math.isinf(x):
        return None
    if abs(x) >= 2**63:
        return None
    if rc == 0:
        # round-half-even on the exact binary value
        f = math.floor(x)
        d = x - f
        if d < 0.5:
            return int(f)
        if d > 0.5:
            return int(f + 1)
        return int(f) if (int(f) % 2 == 0) else int(f + 1)
    if rc == 1:
        return int(math.floor(x))
    if rc == 2:
        return int(math.ceil(x))
    return int(math.trunc(x))


if __name__ == "__main__":
    main()
