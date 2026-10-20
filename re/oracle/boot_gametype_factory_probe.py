#!/usr/bin/env python3
"""Original default GameType resource factory, controlled allocation/node seams.

Executes53a1a0 in Unicorn; supplies allocator ID, resource node constructors,
name copy and data/refcount boundaries. It does not execute scripts or startup.
Redirect derived rows into ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9


def run(parent, allocated):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    root, klass, node, data, vt, owner = [o.alloc(512) for _ in range(6)]
    name = o.alloc(32)
    o.write(name, b'java.util.resource.Dummy\0')
    o.write_u32s(root, [0, 0, 77, owner if parent else 0])
    o.write_u32(node, vt)
    o.write_u32(vt + 12, 0x300f0000)
    events = []

    def allocate(oc, args):
        events.append({'call': 'allocate-id', 'id': allocated})
        return allocated

    def copy(oc, args):
        events.append({'call': 'copy-name', 'name': oc.read_cstr(args[1]).decode('latin1'), 'limit': args[2]})
        return args[0]

    def construct(oc, args, child):
        descriptor = oc.read_u32s(args[2], 6)
        events.append({'call': 'create-child' if child else 'create-root', 'id': args[0], 'parent_id': args[1], 'descriptor': descriptor[:5], 'parent_node_matches': args[3] == owner if child else None, 'trailing': args[4:] if child else args[3:]})
        assert descriptor[:5] == [8, allocated, 2, 0, 0]
        assert child == parent and args[:2] == [allocated, 77]
        assert (args[4:] if child else args[3:]) == [0, 0]
        if child:
            assert args[3] == owner
        return node

    def data_get(oc, args):
        assert oc.reg('ecx') == node and args == [0x3f800000]
        events.append({'call': 'data', 'quality_bits': args[0]})
        return data

    def ref(oc, args):
        assert oc.reg('ecx') == node and args == [1]
        events.append({'call': 'retain', 'count': 1})
        return 0

    o.hook(0x536ed0, allocate, 'thiscall', 0)
    o.hook(0x551120, copy, 'cdecl', 3)
    o.hook(0x536890, lambda oc, args: construct(oc, args, False), 'thiscall', 5)
    o.hook(0x536900, lambda oc, args: construct(oc, args, True), 'thiscall', 6)
    o.hook(0x544b90, ref, 'thiscall', 1)
    o.hook(0x300f0000, data_get, 'thiscall', 1)
    result = o.call(0x53a1a0, [root, klass, name], fpu_cw=CW_D3D9)
    assert result.eax == (node if allocated else 0)
    if allocated:
        assert o.read_u32(data + 0x10) == klass
        assert o.read_u32(node + 0x68) == 0x3f800000
    else:
        assert events == [{'call': 'allocate-id', 'id': 0}]
    return {'oracle': 'Unicorn original53a1a0; controlled ID/node/name/data/refcount boundaries', 'parent': parent, 'allocated': allocated, 'events': events, 'result': 'node' if allocated else 'null', 'data_class_matches_argument2': bool(allocated and o.read_u32(data + 0x10) == klass), 'node_quality_bits': o.read_u32(node + 0x68), 'instructions': result.icount}


if __name__ == '__main__':
    for parent, allocated in [(False, 0xffff007e), (True, 0xffff007e), (False, 0)]:
        print(json.dumps(run(parent, allocated), sort_keys=True))
