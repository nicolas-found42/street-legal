#!/usr/bin/env python3
"""Original46b950 CFG click consumer with original54ecd0 left unhooked.

Only primitive-body creation is substituted (declared null-body collaborator).
Compare every word of the complete 0x2e98-byte caller object, captured Euler
arguments, and the primitive-body call. Ordinary-angle rows are diagnostics.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path

from oracle import Oracle, CW_D3D9
from unicorn import UC_HOOK_CODE
from native_car_euler_vectors import domain

ROOT = Path(__file__).resolve().parents[2]
DEFAULT = ROOT / 're/out/checkpoints/20261021-campaign/car-command/cfg-euler-v2.json'


def capture():
    cases = [('signed-zero', ' '.join(v), 0)
             for v in itertools.product(['0', '-0'], repeat=3)]
    for axis in range(3):
        for value in ['0.0000000001', '-0.0000000001',
                      '100000000000000000000.0', '-100000000000000000000.0']:
            angles = ['0'] * 3
            angles[axis] = value
            cases.append(('single-axis', ' '.join(angles), 0))
    cases += [('mixed-tiny', '-0.0000000001 0.00000000001 -0.000000000001', 0),
              ('ordinary-unresolved', '0.1 0.25 0.5', 0),
              ('ordinary-unresolved', '1 -1 3.14159', 0),
              ('configured-guard', '1 -1 3.14159', 1)]
    rows = []
    for index, (label, angles, guard) in enumerate(cases):
        o = Oracle()
        k = o.alloc(0x2e98)
        o.write(k, bytes(0x2e98))
        o.write_u32(k + 0x120, guard)
        for i in range(16):
            o.write_u32(k + 0x1000 + i * 4, 0x5a000000 + i * 73)
        before = o.read_u32s(k, 0x2e98 // 4)
        body_calls = []
        def primitive(oc, a):
            body_calls.append(dict(this=oc.reg('ecx'), a=list(a[:5]) + [oc.read_cstr(a[5]).decode()]))
            return 0
        o.hook(0x49ab60, primitive, 'thiscall', 6)
        euler = []
        def observe(uc, address, size, user):
            if address == 0x54ecd0:
                pointer = o.read_u32(o.reg('esp') + 4)
                euler.append(dict(this=o.reg('ecx'), a=o.read_u32s(pointer, 3)))
        o.uc.hook_add(UC_HOOK_CODE, observe)
        line = 'click\0shape 1.25 -2.5 3.75 ' + angles + '\0'
        pointer = o.scratch(len(line))
        o.write(pointer, line.encode())
        o.call(0x46b950, [pointer, k], fpu_cw=CW_D3D9, limit=2000000)
        rows.append(dict(index=index, label=label, k=k, line=line, guard=guard,
                         before=before, after=o.read_u32s(k, 0x2e98 // 4),
                         euler=euler, body_calls=body_calls,
                         domains=[domain(w) for call in euler for w in call['a']]))
    return dict(schema='slr-original-cfg-euler-v1', entry=0x46b950, fpu_cw=CW_D3D9,
                original_sha256=hashlib.sha256((ROOT / 'reference/raw/StreetLegal_Redline.bak.exe').read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                collaborators={'0x49ab60': 'primitive body creation returns null; arguments recorded'},
                math='original54ecd0 executes without substitution; entry hook observes captured arguments only',
                comparison='every word of complete caller object; Euler and body arguments', rows=rows)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('action', choices=['gen', 'verify'])
    ap.add_argument('--out', type=Path, default=DEFAULT)
    args = ap.parse_args()
    if not args.out.resolve().is_relative_to(ROOT / 're/out'):
        ap.error('output must stay under ignored re/out')
    result = capture()
    if args.action == 'verify':
        if json.loads(args.out.read_text()) != result:
            raise SystemExit('original CFG Euler replay differs')
    else:
        if args.out.exists():
            raise SystemExit('preserve capture; choose fresh --out or verify')
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(result) + '\n')
    print(f'{len(result["rows"])} original CFG Euler rows: {args.action}')


if __name__ == '__main__':
    main()
