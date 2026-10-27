#!/usr/bin/env python3
"""Write the deterministic adjudication corpus (21-byte records, see bochs_stream.cc) plus manifest.

The corpus is synthetic (seeded generators and named edge points); it contains no original-game
data. Every leg that executes it must produce the same number of 22-byte records in order.
"""
import argparse, hashlib, json, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import xcorpus as C
SPECS = C.SPECS


def edge_points():
    import math
    f64 = [0.25, 11 / 16, 0.5, 1.0, 1.5, 2.0, 3.0, math.pi, math.pi / 2, math.pi / 4, 2 * math.pi,
           3 * math.pi / 2, 1e-5, 1e-10, 12345.678, 1e8, 1e15, 2.0 ** 63, 0.1, 0.2, 0.3, 0.7, 0.9]
    words = []
    for v in f64:
        for sign in (1, -1):
            words.append(C.from_f64_bits(struct.unpack("<Q", struct.pack("<d", sign * v))[0]))
    specials = [
        C.f80(0, 0, 0), C.f80(1, 0, 0), C.f80(0, 0x7FFF, 1 << 63), C.f80(1, 0x7FFF, 1 << 63),
        C.f80(0, 0x7FFF, 0xC000000000000000), C.f80(1, 0x7FFF, 0xC000000000000001),
        C.f80(0, 0x7FFF, 0xA000000000000000), C.f80(1, 0x7FFF, 0x8000000000000001),  # signalling NaNs
        C.f80(0, 0, 1), C.f80(1, 0, 0x7FFFFFFFFFFFFFFF), C.f80(0, 1, 1 << 63), C.f80(0, 0x7FFE, ~0 & (2**64 - 1)),
        C.f80(0, 0x3FFF, 1 << 63), C.f80(1, 0x3FFF, 1 << 63), C.f80(0, 0x4000, 1 << 63), C.f80(0, 0x400E, 0x8000000000000000),
        C.f80(0, 0x400F, 0x8000000000000000), C.f80(1, 0x400E, 0xC000000000000000), C.f80(0, 0x4013, 0x8000000000000000),
    ]
    return words + specials


def build(n):
    out = bytearray()
    index = []
    for i, (op, cls, lo, hi, lo2, hi2) in enumerate(SPECS):
        data = C.corpus(op, cls, n, 5000 + i, lo, hi, lo2, hi2)
        index.append(dict(spec=f"{op}-{cls}-{lo}_{hi}", op=op, first=len(out) // 21, rows=n))
        out += data
    pts = edge_points()
    one = C.f80(0, 0x3FFF, 1 << 63)
    for name, opc in C.OPS.items():
        first = len(out) // 21
        for a in pts:
            if name in C.BINARY:
                for b in pts:
                    out += bytes([opc]) + a + b
            else:
                out += bytes([opc]) + a + bytes(10)
        index.append(dict(spec=f"edge-{name}", op=name, first=first, rows=len(out) // 21 - first))
    return bytes(out), index


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--n", type=int, default=20000)
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    data, index = build(a.n)
    (a.out / "corpus.in").write_bytes(data)
    (a.out / "corpus.json").write_text(json.dumps(dict(
        rows=len(data) // 21, sha256=hashlib.sha256(data).hexdigest(), record_in=21, record_out=22,
        n_per_spec=a.n, index=index), indent=1))
    print(len(data) // 21, "rows", hashlib.sha256(data).hexdigest())


if __name__ == "__main__":
    main()
