#!/usr/bin/env python3
"""All frontend headers through original registration, with declared lookups.

Real 544010/536900/536a10 constructors and inactive-list insertion execute over
unchanged frontend.rpk headers in file order. Supplied allocation, RID mapping,
parent lookup, empty foreign system-root constructors and inactive-list anchors.
No original package-file I/O, source descriptors/materialization, devices, scripts
or startup. Derived rows belong in ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9
from resource_header_registration_probe import headers

CTORS = {3: 0x543000, 5: 0x53b520, 6: 0x53bf40, 7: 0x53d060,
         8: 0x546ae0, 9: 0x5473d0, 13: 0x549010, 14: 0x54a5b0}


def run():
    root = Path(__file__).resolve().parents[2]
    exe = root / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    data = (root / 'stock/frontend.rpk').read_bytes()
    rows = headers(data)
    system = headers((root / 'stock/system.rpk').read_bytes())
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    manager, package = o.alloc(0x110000), o.alloc(512)
    o.write_u32(0x618d48, manager)
    known, kinds, children = {}, {}, {}
    def construct(kind, descriptor):
        node = o.alloc(2048)
        o.call(CTORS[kind], [descriptor, 0, 0] if kind == 3 else [descriptor], convention='thiscall', this=node, fpu_cw=CW_D3D9)
        return node
    foreign = sorted({parent & 0xffff for _, parent, *_ in rows.values() if parent >> 16})
    for ident in foreign:
        _, _, kind, flags, _, _ = system[ident]
        descriptor = o.alloc(128)
        o.write_u32s(descriptor, [kind, ident, flags, 0, 0, 0])
        known[ident], kinds[ident], children[ident] = construct(kind, descriptor), kind, []
    mappings, lookups = [], []
    def map_id(oc, args):
        assert oc.reg('ecx') == package
        mappings.append(args[0])
        return 0x10000 | args[0]
    def lookup(oc, args):
        assert oc.reg('ecx') == manager
        parent, kind, options = args
        assert parent in known and kind == kinds[parent] and options == 0
        lookups.append([parent, kind, options])
        return known[parent]
    o.hook(0x544590, map_id, 'thiscall', 1)
    o.hook(0x537000, lookup, 'thiscall', 3)
    for kind in CTORS:
        o.write_u32(manager + 0x198 + kind * 0x90 + 0x28, o.alloc(64))
    output = []
    for ident, (raw, original_parent, kind, flags, offset, size) in rows.items():
        parent = original_parent & 0xffff if original_parent >> 16 else 0x10000 | original_parent
        pointer, out = o.alloc(len(raw)), o.alloc(4)
        o.write(pointer, raw)
        result = o.call(0x544010, [pointer, parent, out, 0], convention='thiscall', this=package, fpu_cw=CW_D3D9)
        node, global_id = o.read_u32(out), 0x10000 | ident
        assert node and result.eax == len(raw)
        expected_flags = flags | (0x100 if kind in [3, 5] else 0)
        assert o.read_u32s(node + 0x4c, 5) == [kind, global_id, expected_flags, offset, size]
        assert o.read_u32s(node + 0x64, 2) == [0x40, 0]
        alias = o.read_cstr(o.read_u32(node + 0x60)).decode('latin1') if o.read_u32(node + 0x60) else None
        expected_alias = raw[23:23 + raw[22]].split(b'\0')[0].decode('latin1') if kind == 8 and raw[22] else None
        assert alias == expected_alias
        known[global_id], kinds[global_id], children[global_id] = node, kind, []
        children[parent].append(global_id)
        for index, child_id in enumerate(children[parent]):
            child = known[child_id]
            assert o.read_u32(child + 0x14) == known[parent]
            assert o.read_u32(child + 8) == (known[parent] + 0x1c if index == 0 else known[children[parent][index - 1]])
            assert o.read_u32(child + 4) == (known[parent] + 0x30 if index + 1 == len(children[parent]) else known[children[parent][index + 1]])
        assert o.read_u32(known[parent] + 0x20) == known[children[parent][0]]
        assert o.read_u32(known[parent] + 0x38) == node
        output.append({'local_id': ident, 'parent_id': parent, 'kind': kind, 'flags': expected_flags, 'descriptor_offset': offset, 'descriptor_size': size, 'alias': alias, 'load_state': 0x40, 'range_bits': 0, 'parent_children': list(children[parent]), 'header_bytes': result.eax})
    assert len(output) == 279 and mappings == list(rows)
    assert len(lookups) == len(output)
    return {'oracle': 'Unicorn original frontend header registration and constructors; supplied RID mapping/parent lookup/foreign roots/inactive anchors/allocation', 'package_sha256': hashlib.sha256(data).hexdigest(), 'entries': output, 'scope': 'all279 actual frontend headers in file order; no original package I/O, materialization or startup'}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
