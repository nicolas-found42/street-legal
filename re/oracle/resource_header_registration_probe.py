#!/usr/bin/env python3
"""Original header registration and unmaterialized ResourceRef binding.

Reads three unchanged frontend.rpk headers. Executes 544010 -> 536900 ->
536a10 -> 53d060, real inactive-list insertion, then 545fc0. Allocation, package
ID mapping and parent/container lookup are supplied; the foreign root and
inactive-list anchor are declared. No package file loading, sourcefile parsing,
texture/device creation, script execution or whole startup. Output re/out/ only.
"""
import hashlib
import json
import struct
from pathlib import Path
from oracle import Oracle, CW_D3D9


def headers(data):
    assert data[:8] == b'RPAK\x00\x02\x00\x00'
    p = 16 + 64 * struct.unpack_from('<I', data, 8)[0]
    size, count, type1, other = struct.unpack_from('<IIII', data, p)
    assert count == type1 + other
    p += 16
    start = p
    rows = {}
    for _ in range(count):
        begin = p
        parent, ident, kind, flags, amp, offset, length = struct.unpack_from('<IIBBfII', data, p)
        p += 23 + data[p + 22] + (48 if kind == 1 else 0)
        assert offset + length <= len(data)
        rows[ident] = (data[begin:p], parent, kind, flags, offset, length)
    assert p - start == size
    return rows


def run():
    root = Path(__file__).resolve().parents[2]
    exe = root / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    data = (root / 'stock/frontend.rpk').read_bytes()
    rows = headers(data)
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    manager, package = o.alloc(0x110000), o.alloc(512)
    o.write_u32(0x618d48, manager)
    descriptor, foreign_root = o.alloc(128), o.alloc(512)
    o.write_u32s(descriptor, [7, 8, 4, 0, 0, 0])
    o.call(0x53d060, [descriptor], convention='thiscall', this=foreign_root, fpu_cw=CW_D3D9)
    known = {8: foreign_root}
    mappings, lookups, binds = [], [], []
    def map_id(oc, args):
        assert oc.reg('ecx') == package
        mappings.append(args[0])
        return 0x10000 | args[0]
    def parent_lookup(oc, args):
        assert oc.reg('ecx') == manager
        lookups.append(args)
        assert args[1:] == [7, 0]
        return known[args[0]]
    def container_lookup(oc, args):
        assert oc.reg('ecx') == manager
        binds.append(args)
        assert args[1:] == [0, 0]
        return known[args[0]]
    o.hook(0x544590, map_id, 'thiscall', 1)
    o.hook(0x537000, parent_lookup, 'thiscall', 3)
    o.hook(0x536820, container_lookup, 'thiscall', 3)
    # Declared type7 inactive-list tail, not a replay of manager construction.
    anchor = o.alloc(64)
    o.write_u32(manager + 0x198 + 7 * 0x90 + 0x28, anchor)
    snapshots = []
    for ident, parent in [(3, 8), (210, 0x10003), (168, 0x100d2)]:
        raw, raw_parent, kind, flags, offset, length = rows[ident]
        assert kind == 7 and flags == 4
        assert raw_parent == (0x10008 if ident == 3 else parent & 0xffff)
        pointer, out = o.alloc(len(raw)), o.alloc(4)
        o.write(pointer, raw)
        result = o.call(0x544010, [pointer, parent, out, 0], convention='thiscall', this=package, fpu_cw=CW_D3D9)
        node = o.read_u32(out)
        assert node and result.eax == len(raw)
        assert o.read_u32s(node + 0x4c, 8) == [7, 0x10000 | ident, 4, offset, length, 0, 0x40, 0]
        assert o.read_u32(node + 0x14) == known[parent]
        assert o.read_u32(known[parent] + 0x20) == node
        assert o.read_u32(known[parent] + 0x38) == node
        assert o.read_u32(node + 8) == known[parent] + 0x1c
        assert o.read_u32(node + 4) == known[parent] + 0x30
        known[0x10000 | ident] = node
        snapshots.append({'local_id': ident, 'parent_id': parent, 'kind': kind, 'flags': flags, 'descriptor_offset': offset, 'descriptor_size': length, 'consumed_header': result.eax, 'load_state': o.read_u32(node + 0x64), 'parent_and_child_links_match': True})
    ref = o.alloc(16)
    target = known[0x100a8]
    before = o.read(target, 0xac)
    o.call(0x545fc0, [0x100a8], convention='thiscall', this=ref, fpu_cw=CW_D3D9)
    assert o.read_u32s(ref, 4) == [0, 0, 0x100a8, target]
    assert o.read_u32(target + 0x48) == ref
    # Only retained-reference head changes, no sourcefile/materialization state.
    after = bytearray(o.read(target, 0xac))
    after[0x48:0x4c] = before[0x48:0x4c]
    assert bytes(after) == before
    o.call(0x545fc0, [0x100a8], convention='thiscall', this=ref, fpu_cw=CW_D3D9)
    assert mappings == [3, 210, 168]
    assert lookups == [[8, 7, 0], [0x10003, 7, 0], [0x100d2, 7, 0]]
    assert binds == [[0x100a8, 0, 0]]
    return {'oracle': 'Unicorn original544010/536900/536a10/53d060/537de0/545fc0; supplied allocation, RID mapping, parent/container lookup, foreign root and inactive-list anchor', 'package_sha256': hashlib.sha256(data).hexdigest(), 'snapshots': snapshots, 'bind_lookups': binds, 'bound_id': 0x100a8, 'same_id_zero_lookup': True, 'binding_does_not_materialize': True, 'scope': 'three actual headers in supplied parent order; no original package file loading, descriptor parsing or device'}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
