#!/usr/bin/env python3
"""Original419860/5447d0 access protocol with declared manager/virtual callbacks.

The executable decides callback order, load gating, return values and state
changes. Manager loading, spatial notification, resource quality virtuals and
payload are explicit scenario collaborators. Full resource words are retained.
"""
import hashlib
import json
import random
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9


def run():
    rng = random.Random(0x419860)
    rows = []
    for index in range(128):
        o = Oracle()
        raw = o.alloc(0xd0)
        vt = o.alloc(0x40)
        o.write_u32(raw, vt)
        kind = [1, 3, 5, 7, 14, 21][index % 6]
        state = rng.choice([0, 1, 0x10000000, 0x10000001])
        flags = rng.choice([0, 0x100, 0x02000000, 0x02000100])
        quality = rng.choice([0., .25, .75, 1., 2.])
        requested = rng.choice([0., .25, .75, 1., 2.])
        minimum = rng.choice([0., .25, 1.])
        access = [rng.choice([0, 0x20000000, 0x40000000, 0x80000000,
                              0xa0000000, 0xc0000000]), requested,
                  rng.choice([0., 1.]), rng.choice([0., 10.])]
        loaded_by_request = index % 3 == 0
        sensitive = o.read_u32(0x619dc0 + kind * 4)
        o.write_u32(0x6200a4, 123)
        o.write_u32(raw + 0x4c, kind)
        o.write_u32(raw + 0x50, 0x100053)
        o.write_u32(raw + 0x54, flags)
        o.write_u32(raw + 0x64, state)
        o.write_f32s(raw + 0x68, [quality, .5])
        o.write_u32(raw + 0x74, 123 if index % 4 == 0 else 122)
        calls = []

        def record(name, args):
            calls.append(dict(name=name, args=args, state=o.read_u32s(raw, 0xd0 // 4)))

        def request(oc, args):
            record('request', args[1:])
            assert args[0] == raw
            if loaded_by_request:
                oc.write_u32(raw + 0x64, oc.read_u32(raw + 0x64) | 1)
                oc.write_u32(raw + 0x68, args[1])
            return 0

        def spatial(oc, args):
            record('spatial', args[1:])
            assert args[0] == raw
            return 0

        def touch(oc, args):
            record('touch', [])
            assert args == [raw]
            oc.write_u32(raw + 0x74, oc.read_u32(0x6200a4))
            return 0

        def set_minimum(oc, args):
            record('minimum', args)
            oc.write_u32(raw + 0x6c, args[0])
            return 0

        def min_quality(oc, args):
            record('min_quality', [])
            return float(minimum)

        def refresh(oc, args):
            record('refresh', [])
            return 0

        def payload(oc, args):
            record('payload', args)
            return raw + 0x80

        o.hook(0x537240, request, 'thiscall', 4)
        o.hook(0x537790, spatial, 'thiscall', 4)
        o.hook(0x537d10, touch, 'thiscall', 1)
        for offset, callback, nargs in [(4, min_quality, 0), (8, refresh, 0),
                                         (12, payload, 1), (20, set_minimum, 1)]:
            target = 0x300f0000 + offset * 4
            o.write_u32(vt + offset, target)
            o.hook(target, callback, 'thiscall', nargs)
        before = o.read_u32s(raw, 0xd0 // 4)
        result = o.call(0x419860, access, convention='thiscall', this=raw, fpu_cw=CW_D3D9)
        import struct
        bits = lambda f: struct.unpack('<I', struct.pack('<f', f))[0]
        rows.append(dict(raw=raw, before=before, after=o.read_u32s(raw, 0xd0 // 4),
                         args=[access[0], *map(bits, access[1:])], sensitive=sensitive,
                         minimum=bits(minimum), loaded_by_request=loaded_by_request,
                         result=result.eax, calls=calls))
    return dict(scope=__doc__, exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(), rows=rows)


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
