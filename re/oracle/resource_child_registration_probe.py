#!/usr/bin/env python3
"""Original536a10 explicit-parent registration and child-list append.

Real parent/child constructors and list registration execute; malloc and type1
pool allocation are supplied, parent identity is explicit. No manager lookup,
package hierarchy loading, script or startup. Output in re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9


def run(kind):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    manager, parent, descriptor = [o.alloc(0x1000) for _ in range(3)]
    o.write_u32s(descriptor, [kind, 4, 1 if kind == 1 else 0, 0, 0, 0])
    o.call(0x53e6e0 if kind == 1 else 0x546ae0, [descriptor, 0, 0] if kind == 1 else [descriptor], convention='thiscall', this=parent, fpu_cw=CW_D3D9)
    o.hook(0x538720, lambda oc, args: oc.alloc(0x15c), 'thiscall', 0)
    ids = [0xffff007e, 0xffff007d, 0xffff007c]
    children = []
    snapshots = []
    for ident in ids:
        o.write_u32s(descriptor, [kind, ident, 0x21 if kind == 1 else 2, 0, 0, 0])
        r = o.call(0x536a10, [4, descriptor, 0, 0, parent], convention='thiscall', this=manager, fpu_cw=CW_D3D9)
        assert r.eax != 0
        children.append(r.eax)
        got = []
        cursor = o.read_u32(parent + 0x20)
        while cursor != parent + 0x30:
            assert cursor in children and len(got) < len(children)
            got.append(o.read_u32(cursor + 0x50))
            assert o.read_u32(cursor + 0x14) == parent
            cursor = o.read_u32(cursor + 4)
        assert got == ids[:len(children)]
        for index, child in enumerate(children):
            assert o.read_u32(child + 8) == (parent + 0x1c if index == 0 else children[index - 1])
            assert o.read_u32(child + 4) == (parent + 0x30 if index + 1 == len(children) else children[index + 1])
        assert o.read_u32(parent + 0x38) == children[-1]
        snapshots.append({'ids': got, 'parent_matches': True, 'prev_next_consistent': True, 'tail_matches_last': True, 'instructions': r.icount})
    return {'oracle': 'Unicorn original536a10 +parent/child constructors and append; supplied malloc/type1pool and explicit parent identity', 'kind': kind, 'snapshots': snapshots, 'scope': 'explicit-parent only; no manager lookup or package hierarchy load'}


if __name__ == '__main__':
    for kind in [8, 1]:
        print(json.dumps(run(kind), sort_keys=True))
