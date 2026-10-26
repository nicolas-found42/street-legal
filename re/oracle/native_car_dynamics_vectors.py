#!/usr/bin/env python3
"""Original43ee70 constraint refresh over complete caller-owned buffers.

No simulation helper is hooked. Only43ee70's code and its original vector/matrix
callees execute. Both buffer indices, missing links, zero normalization, signed
zeros and finite arithmetic cases are retained. Outputs are raw words.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import struct

from oracle import Oracle, CW_D3D9, HEAP_BASE

ROOT = Path(__file__).resolve().parents[2]
DEFAULT = ROOT / "re/out/boot/native-car-dynamics-original.json"


def bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def capture():
    rows = []
    rng = random.Random(0x43ee70)
    for case in range(42):
        o = Oracle()
        record = o.alloc(0x8c)
        definition = o.alloc(0x40)
        a, b, parent = [o.alloc(0x240) for _ in range(3)]
        # Poison incoming unused cells, including matrix padding, so a
        # zero-initialized replacement cannot accidentally compare equal.
        for p, size in [(record, 0x8c), (definition, 0x40), (a, 0x240), (b, 0x240), (parent, 0x240)]:
            o.write_u32s(p, [bits(rng.randint(-100, 100) / 16) for _ in range(size // 4)])
        o.write_u32(record, definition)
        o.write_u32(record + 0x30, 0 if case == 0 else a)
        o.write_u32(record + 0x38, 0 if case == 1 else b)
        o.write_u32(a + 0x20c, case & 1)
        o.write_u32(b + 0x20c, (case >> 1) & 1)
        o.write_u32(parent + 0x20c, (case >> 2) & 1)
        o.write_u32(b + 0x20, parent)
        if case in [2, 3, 4]:
            o.write_u32s(definition + 0x18, [0, 0x80000000 if case == 3 else 0, 0])
        if case == 4:
            for entity in [a, b, parent]:
                state = entity + o.read_u32(entity + 0x20c) * 0xd4
                o.write_u32s(state + 0x64, [0] * 3)
                o.write_u32s(state + 0xa4, [bits(1 if i in [0, 5, 10] else 0) for i in range(12)])
            o.write_u32s(record + 4, [0] * 3)
            o.write_u32s(definition + 0xc, [0] * 3)
        count = (o.heap_ptr - HEAP_BASE + 3) // 4
        before = o.read_u32s(HEAP_BASE, count)
        o.call(0x43ee70, [0], convention='thiscall', this=record, fpu_cw=CW_D3D9)
        rows.append(dict(case=case, record=record, before=before, after=o.read_u32s(HEAP_BASE, count)))
    binary = ROOT / 'reference/raw/StreetLegal_Redline.bak.exe'
    return dict(schema='slr-car-dynamics-v1', original_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                entry=0x43ee70, fpu_cw=CW_D3D9, hooks=[],
                comparison='entire caller heap; original stack is private ABI scratch', rows=rows)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('action', choices=['gen', 'verify'])
    ap.add_argument('--out', type=Path, default=DEFAULT)
    args = ap.parse_args()
    if not args.out.resolve().is_relative_to(ROOT / 're/out'):
        ap.error('derived output must stay under ignored re/out')
    fresh = capture()
    if args.action == 'verify':
        if json.loads(args.out.read_text()) != fresh:
            raise SystemExit('original replay differs')
    else:
        if args.out.exists():
            raise SystemExit('preserve previous capture; use verify or a fresh --out')
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(fresh) + '\n')
    print(f'{len(fresh["rows"])} original43ee70 full-buffer rows: {args.action}')


if __name__ == '__main__':
    main()
