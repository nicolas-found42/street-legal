#!/usr/bin/env python3
"""Original53a2a0 existing-script native instance branch, controlled resources.

Real intrusive links execute. ID/node constructors, virtual data/flags/refcount,
compiled-object and event/physics callbacks are supplied boundaries. No script
body/startup/device executes. Redirect rows into ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9
from unicorn import UC_HOOK_CODE


def run(request_id, allocated, fresh_defaults=False, fresh_child=False):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    root, request, template, node, data, gtype, vt, tvt, script, klass = [o.alloc(512) for _ in range(10)]
    constructor_instructions = None
    if fresh_defaults:
        descriptor = o.alloc(24)
        o.write_u32s(descriptor, [1, allocated, 0x21, 0, 0, 0])
        constructed = o.call(0x53e6e0, [descriptor, 0, 0], convention='thiscall', this=node, fpu_cw=CW_D3D9)
        assert constructed.eax == node
        constructor_instructions = constructed.icount
        data = node + 0xd8
    parent = None
    if fresh_child:
        assert fresh_defaults
        parent = o.alloc(0x15c)
        parent_descriptor = o.alloc(24)
        o.write_u32s(parent_descriptor, [1, 4, 1, 0, 0, 0])
        o.call(0x53e6e0, [parent_descriptor, 0, 0], convention='thiscall', this=parent, fpu_cw=CW_D3D9)
        # Explicitly declared already materialized empty parent, not a load claim.
        o.write_u32(parent + 0x64, 1)
        o.write_u32(root + 12, parent)
        o.write_u32(node + 0x14, parent)
        o.write(vt, o.read(o.read_u32(node), 0x30))
    o.write_u32(root + 8, 77)
    o.write_u32s(request, [0, 0, request_id, template])
    o.write_u32(template, tvt)
    o.write_u32(template + 0x4c, 8)
    o.write_u32(template + 0x50, request_id)
    o.write_u32(tvt + 12, 0x300f0010)
    o.write_u32(tvt + 20, 0x300f0020)
    o.write_u32(node, vt)
    o.write_u32(node + 0x50, allocated)
    o.write_u32(vt + 12, 0x300f0000)
    o.write_u32(gtype + 16, klass)
    o.write_u32(gtype + 24, 0x1234)
    o.write_u32(gtype + 28, 0x5678)
    events = []

    def hook(address, count, callback=0, conv='cdecl'):
        def record(oc, args):
            events.append(hex(address))
            return callback(oc, args) if callable(callback) else callback
        o.hook(address, record, conv, count)

    def construct(oc, args):
        assert args[:2] == [allocated, 77]
        assert args[3:] == ([parent, 0, 0] if fresh_child else [0, 0])
        assert oc.read_u32s(args[2], 5) == [1, allocated, 0x21, 0, 0]
        return node

    hook(0x536ed0, 0, allocated, 'thiscall')
    hook(0x551120, 3)
    hook(0x536900 if fresh_child else 0x536890, 6 if fresh_child else 5, construct, 'thiscall')
    hook(0x544b90, 1, 0, 'thiscall')
    hook(0x300f0000, 1, data, 'thiscall')
    hook(0x300f0010, 1, gtype, 'thiscall')
    hook(0x300f0020, 1, 0, 'thiscall')
    hook(0x5447d0, 3, 0, 'thiscall')
    hook(0x427210, 1, 0, 'thiscall')
    if not fresh_defaults:
        hook(0x4988a0, 1)
        hook(0x498b10, 1)
        hook(0x48ae40, 0, 0, 'thiscall')
    script_entries = []
    def observe_script(uc, address, size, user):
        if fresh_defaults and address in {0x4988a0, 0x498b10, 0x48ae40}:
            events.append(hex(address))
        if address in {0x404e20, 0x41f340, 0x4207c0, 0x420ff0}:
            script_entries.append(hex(address))
    o.uc.hook_add(UC_HOOK_CODE, observe_script)
    before_count = o.read_u32(0x765f60)
    result = o.call(0x53a2a0, [root, request, script, 0, 0], fpu_cw=CW_D3D9)
    success = bool(request_id and allocated)
    assert result.eax == (node if success else 0)
    if success:
        assert o.read_u32s(data + 0x38, 4) == [0, 0, request_id, template]
        assert o.read_u32(template + 0x48) == data + 0x38
        assert o.read_u32(data + 0x50) == script
        assert o.read_u32(node + 0x48) == 0
        assert o.read_u32(node + 0x54) == (0x221 if fresh_defaults else 0x200)
        assert o.read_u32(node + 0x68) == 0x3f800000
        assert o.read_u32(node + 0xbc) == 0x1234
        assert o.read_u32(node + 0xb8) == 0x5678
        assert o.read_u32(0x765f60) == (before_count + 1) & 0xffffffff
    else:
        assert events == (['0x536ed0'] if request_id else [])
    assert script_entries == []
    if fresh_child:
        assert o.read_u32s(node + 0x6c, 2) == o.read_u32s(parent + 0x6c, 2)
    return {**({'fresh_default_constructor': True, 'constructor_instructions': constructor_instructions, **({'parent_profile': 'real fresh type1; declared materialized empty parent', 'parent_range_bits': o.read_u32s(parent + 0x6c, 2), 'child_range_bits': o.read_u32s(node + 0x6c, 2)} if fresh_child else {})} if fresh_defaults else {}), 'oracle': 'Unicorn original53a2a0 with real429060/4290f0 links; controlled resource/virtual/event callbacks', 'request_id': request_id, 'allocated': allocated, 'result': 'node' if success else 'null', 'template_link_retained': success, 'script_backref_matches_argument3': success, 'temporary_instance_link_released': success, 'flags': o.read_u32(node + 0x54), 'quality_bits': o.read_u32(node + 0x68), 'events': events, 'script_entries': script_entries, 'instructions': result.icount}


if __name__ == '__main__':
    for request_id, allocated in [(0xffff007e, 0xffff007d), (0xffff007e, 0), (0, 0xffff007d)]:
        print(json.dumps(run(request_id, allocated), sort_keys=True))
