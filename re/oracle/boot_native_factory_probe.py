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


def run(request_id, allocated):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    root, request, template, node, data, gtype, vt, tvt, script, klass = [o.alloc(512) for _ in range(10)]
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
        assert args[:2] == [allocated, 77] and args[3:] == [0, 0]
        assert oc.read_u32s(args[2], 5) == [1, allocated, 0x21, 0, 0]
        return node

    hook(0x536ed0, 0, allocated, 'thiscall')
    hook(0x551120, 3)
    hook(0x536890, 5, construct, 'thiscall')
    hook(0x544b90, 1, 0, 'thiscall')
    hook(0x300f0000, 1, data, 'thiscall')
    hook(0x300f0010, 1, gtype, 'thiscall')
    hook(0x300f0020, 1, 0, 'thiscall')
    hook(0x5447d0, 3, 0, 'thiscall')
    hook(0x427210, 1, 0, 'thiscall')
    hook(0x4988a0, 1)
    hook(0x498b10, 1)
    hook(0x48ae40, 0, 0, 'thiscall')
    script_entries = []
    def observe_script(uc, address, size, user):
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
        assert o.read_u32(node + 0x54) == 0x200
        assert o.read_u32(node + 0x68) == 0x3f800000
        assert o.read_u32(node + 0xbc) == 0x1234
        assert o.read_u32(node + 0xb8) == 0x5678
        assert o.read_u32(0x765f60) == (before_count + 1) & 0xffffffff
    else:
        assert events == (['0x536ed0'] if request_id else [])
    assert script_entries == []
    return {'oracle': 'Unicorn original53a2a0 with real429060/4290f0 links; controlled resource/virtual/event callbacks', 'request_id': request_id, 'allocated': allocated, 'result': 'node' if success else 'null', 'template_link_retained': success, 'script_backref_matches_argument3': success, 'temporary_instance_link_released': success, 'flags': o.read_u32(node + 0x54), 'quality_bits': o.read_u32(node + 0x68), 'events': events, 'script_entries': script_entries, 'instructions': result.icount}


if __name__ == '__main__':
    for request_id, allocated in [(0xffff007e, 0xffff007d), (0xffff007e, 0), (0, 0xffff007d)]:
        print(json.dumps(run(request_id, allocated), sort_keys=True))
