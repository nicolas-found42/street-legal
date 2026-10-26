#!/usr/bin/env python3
"""Bochs-vs-Rosetta-vs-mpmath comparison of the five game-relevant x87 transcendentals.

Diagnostic evidence for the G11 decision, not an acceptance result. All inputs are synthetic
(deterministic seeds) and carry no original data. Requires the harness binaries built by
build_harnesses.sh and ~/slr-oracle-venv (mpmath).
"""
import argparse, collections, hashlib, json, subprocess, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import xcorpus, xcompare, xanalyze as A

SPECS = xcorpus.SPECS


def digest(b):
    return hashlib.sha256(b).hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--build", type=Path, required=True, help="dir with bochs-stream and rosetta-stream")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--n", type=int, default=20000)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    table = []
    for i, (op, cls, lo, hi, lo2, hi2) in enumerate(SPECS):
        data = xcorpus.corpus(op, cls, args.n, 1000 + i, lo, hi, lo2, hi2)
        b = xcompare.run([args.build / "bochs-stream"], data)
        r = xcompare.run([args.build / "rosetta-stream"], data)
        name = f"{op}-{cls}-{lo}_{hi}"
        (args.out / f"{name}.in").write_bytes(data)
        (args.out / f"{name}.bochs").write_bytes(b)
        (args.out / f"{name}.rosetta").write_bytes(r)
        stat = collections.Counter()
        dist = collections.Counter()
        for ins, x, y in zip(xcompare.records(data, 21), xcompare.records(b, 22), xcompare.records(r, 22)):
            ref = A.ref_round(A.reference(op, ins[1:11], ins[11:21]))
            same = x[:10] == y[:10]
            stat["rows"] += 1
            stat["bochs_eq_rosetta"] += same
            if ref is not None:
                stat["ref_rows"] += 1
                stat["bochs_exact"] += x[:10] == ref
                stat["rosetta_exact"] += y[:10] == ref
                stat["both_exact"] += (x[:10] == ref) and (y[:10] == ref)
                stat["neither_exact"] += (x[:10] != ref) and (y[:10] != ref)
            if not same:
                sx, ex, mx = A.dec(x[:10]); sy, ey, my = A.dec(y[:10])
                d = abs(mx - my) if (sx, ex) == (sy, ey) else -1
                dist["1" if d == 1 else "2-3" if 2 <= d <= 3 else "4-15" if 4 <= d <= 15 else "16-4095" if 4 <= d < 4096 or 16 <= d < 4096 else ">=4096" if d >= 4096 else "binade/sign"] += 1
            stat["sw_diff"] += x[20:22] != y[20:22]
        table.append(dict(spec=name, op=op, cls=cls, **stat, diff_distance_ulp64=dict(dist),
                          input_sha256=digest(data), bochs_sha256=digest(b), rosetta_sha256=digest(r)))
        print(name, dict(stat), file=sys.stderr)
    (args.out / "table.json").write_text(json.dumps(table, indent=1))


if __name__ == "__main__":
    main()
