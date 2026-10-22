#!/usr/bin/env python3
"""Original448430 through its updatevariables callback boundary.

Resource get-data/flags, integer field lookup and parameter clone are controlled
collaborators. The callback is a stop, not a supplied return. Original copies,
stores, guards and wheel resets execute. This is not a complete update oracle.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9


class CallbackBoundary(Exception):
    pass


def run(wheels, shared, suspended=False, script=73, scale=1.0):
    o = Oracle(fpu_cw=CW_D3D9)
    sizes = dict(template=0x2e98, body=0x212c, parameters=0xc1c,
                 clone=0xc1c, resource=0x15c, wheel=0x2b4 * 3, vt=0x40)
    a = {key: o.alloc(size) for key, size in sizes.items()}
    for key in ('body', 'wheel'):
        o.write(a[key], bytes([0xa5]) * sizes[key])
    o.write_u32(a['template'] + 0x2e48, a['parameters'])
    o.write_u32(a['template'] + 0x1f40, wheels)
    o.write(a['template'] + 0x2e50, bytes(range(0x44)))
    o.write_u32(a['body'] + 0x1fbc, a['parameters' if shared else 'clone'])
    o.write_u32(a['body'] + 0xc, a['resource'])
    o.write_u32(a['body'] + 0x13e4, a['wheel'])
    o.write_u32(a['resource'], a['vt'])
    o.write_u32(a['resource'] + 0x4c, 1)
    o.write_u32(a['resource'] + 0x64, 1)
    o.write_u32(a['resource'] + 0x128, script)
    if suspended or script == 0:
        o.write_f32(a['body'] + 0x1f24, scale)
        o.write_f32(a['body'] + 0x1f28, 2.0)
        o.write_f32(a['body'] + 0x1f2c, 3.0)
        for i in range(3):
            o.write_f32(a['body'] + 0x1f14 + 4*i, 0.5)
            wheel = a['wheel'] + i*0x2b4
            for off, value in [(0x64, 8.0), (0x68, 16.0), (0x15c, 0.5)]:
                o.write_f32(wheel + off, value)
        for key in ['parameters', 'clone']:
            o.write_f32(a[key]+0xd0, 4.0)
            o.write_f32(a[key]+0xd4, 8.0)
    o.write_u32(a['vt'] + 0xc, 0x300f0000)
    o.hook(0x300f0000, lambda oc, args: a['resource'] + 0xd8, 'thiscall', 1)
    o.hook(0x5447d0, lambda oc, args: 0, 'thiscall', 3)
    o.hook(0x42a430, lambda oc, args: int(suspended), 'cdecl', 2)
    calls = []
    o.hook(0x43e3e0, lambda oc, args: calls.append('clone') or a['clone'],
           'thiscall', 0)
    def callback(oc, args):
        assert args == [a['resource'] + 0xd8, 0x6100b8, 79, 79]
        calls.append('updatevariables')
        raise CallbackBoundary
    o.hook(0x425a90, callback, 'cdecl', 4)
    before = {key: o.read_u32s(a[key], size // 4) for key, size in sizes.items()}
    try:
        o.call(0x448430, [a['body']], convention='thiscall', this=a['template'],
               fpu_cw=CW_D3D9)
        assert suspended or script == 0, 'callback not reached'
    except CallbackBoundary:
        pass
    return dict(wheels=wheels, shared=shared, suspended=suspended, script=script,
                scale=scale, addresses=a, before=before, calls=calls,
                after={key: o.read_u32s(a[key], size // 4) for key, size in sizes.items()})


if __name__ == '__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         scope=__doc__, rows=[run(w, s) for w in [0, 2] for s in [False, True]]),
                     sort_keys=True))
