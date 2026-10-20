#!/usr/bin/env python3
"""Original type8/type1 resource constructor defaults and empty lifecycle.

Original546ae0/53e6e0/base constructors execute over supplied descriptors and
poisoned object storage. Type1 empty-child bounds/reparent/physics lookup also
execute. No resource manager registration, script execution or startup runs.
Output belongs in ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9


def run(kind):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    node = o.alloc(0x15c)
    o.write(node, b'\xa5' * 0x15c)
    descriptor = o.alloc(24)
    o.write_u32s(descriptor, [kind, 0xffff007e, 2 if kind == 8 else 0x21, 0, 0, 0])
    address = 0x546ae0 if kind == 8 else 0x53e6e0
    result = o.call(address, [descriptor] if kind == 8 else [descriptor, 0, 0], convention='thiscall', this=node, fpu_cw=CW_D3D9)
    assert result.eax == node
    if kind == 8:
        data = node + 0x80
        assert o.read_u32(data + 12) == 0  # compiled constructor
        assert o.read_u32(data + 16) == 0  # script class before53a1a0 installs it
        assert o.read_u32(data + 24) == 0x3f800000
        assert o.read_u32(data + 28) == 0
        defaults = {'compiled_constructor': 0, 'script_class': 0, 'word18_bits': o.read_u32(data + 24), 'word1c_bits': o.read_u32(data + 28)}
        lifecycle = []
    else:
        data = node + 0xd8
        assert o.read_u32(data + 0x50) == 0
        assert o.read_u32(node + 0x80) == 2
        child = o.read_u32(node + 0x20)
        assert child != 0 and o.read_u32(child + 4) == 0
        lifecycle = []
        for address, name, args, this in [(0x4988a0, 'bounds', [node], None), (0x498b10, 'reparent', [node], None), (0x48ae40, 'physics-lookup', [], node + 0xc0)]:
            r = o.call(address, args, convention='thiscall' if this is not None else 'cdecl', this=this, fpu_cw=CW_D3D9)
            if name != 'bounds':
                assert r.eax == 0
            object_writes = [w for w in r.mem_writes if node <= w[0] < node + 0x15c]
            assert not object_writes
            lifecycle.append({'name': name, 'return': None if name == 'bounds' else r.eax, 'object_writes': 0, 'instructions': r.icount})
        defaults = {'script_object': 0, 'bounds_kind': 2, 'empty_child_sentinel': True, 'physics_owner': o.read_u32(node + 0xcc)}
        assert defaults['physics_owner'] == 0
    return {'oracle': 'Unicorn original546ae0/53e6e0 and base constructors; supplied descriptor/poisoned storage; original empty type1 lifecycle', 'kind': kind, 'resource_id': o.read_u32(node + 0x50), 'flags': o.read_u32(node + 0x54), 'defaults': defaults, 'lifecycle': lifecycle, 'constructor_instructions': result.icount}


if __name__ == '__main__':
    for kind in [8, 1]:
        print(json.dumps(run(kind), sort_keys=True))
