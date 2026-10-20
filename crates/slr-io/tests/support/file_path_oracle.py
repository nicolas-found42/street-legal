#!/usr/bin/env python3
"""Native File.exists/delete + real path wrappers; controlled OS helper seam.

Real 486150/486170 and45d910 marshal String words; real54c7c0/54c170
normalize path and choose helpers. File cache starts empty. OS probes/deletion,
CRT string copy and locks are explicit boundaries; no Windows filesystem claim.
Generated rows belong only in ignored re/out/io/path_native.jsonl.
"""
import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 're/oracle'))
from oracle import Oracle, CW_D3D9
from io_vectors import _alloc_cstr, _call_frame, _cell


def run(name, path, present):
    exe = ROOT / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    events = []
    for slot in range(32):
        o.write_u32(0x766460 + slot * 0x10c, 0xffffffff)
    o.write_u32(0x7686e0, 0)

    def copy(oc, args):
        oc.write(args[0], oc.read_cstr(args[1]) + b'\0')
        return args[0]

    def helper(oc, args):
        events.append({'path': oc.read_cstr(args[0]).decode(), 'helper': 'exists' if name == 'exists' else 'delete'})
        return int(present) if name == 'exists' else 1

    o.hook(0x554860, copy, 'cdecl', 2)
    o.hook(0x555550, lambda oc, a: len(oc.read_cstr(a[0])), 'cdecl', 1)
    o.hook(0x5504c0, lambda oc, a: None, 'thiscall', 1)
    o.hook(0x5504f0, lambda oc, a: None, 'thiscall', 0)
    o.hook(0x5586b0 if name == 'exists' else 0x558cf0, helper, 'cdecl', 1)
    word = _alloc_cstr(o, path.encode())
    frame = _call_frame(o, 'java.io.File', name, '(Ljava.lang.String;)I', [_cell(o, 'L', word), _cell(o, 'L', 0xdeadbeef)])
    address = 0x486170 if name == 'exists' else 0x486150
    result = o.call(address, [frame], fpu_cw=CW_D3D9)
    assert events == [{'path': path.split('\0')[0].replace('/', '\\'), 'helper': name}]
    assert result.eax == (int(present) if name == 'exists' else 1)
    return {'name': name, 'path': path, 'present': present, 'eax': result.eax, 'events': events,
            'oracle': 'Unicorn original native + argument marshaler + path wrapper; controlled OS helper', 'instructions': result.icount}


if __name__ == '__main__':
    for name, path, present in [
        ('exists', 'data/cars/gti.car', True),
        ('exists', 'data/cars/missing.car', False),
        ('exists', 'data\\cars\\gti.car', True),
        ('exists', 'data/cars/gti.car\0ignored', True),
        ('delete', 'data/cars/gti.car', True),
        ('delete', 'data/cars/missing.car', False),
        ('delete', 'data\\cars\\gti.car', True),
    ]:
        print(json.dumps(run(name, path, present), sort_keys=True))
