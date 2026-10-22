#!/usr/bin/env python3
"""Execute build-606 426470 over controlled instance envelopes.

Only resource virtual get-data/flags, template materialisation and script/native
callback boundaries are supplied. The original chooses every branch and callback
argument. Rows contain relative identities, never executable replacement results.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9


def run(kind, body, script, template=True, flags=0, bound=True):
    o = Oracle(fpu_cw=CW_D3D9)
    reference, resource, data, vtable, template_ref, template_data, payload, payload_vt = [
        o.alloc(512) for _ in range(8)]
    o.write_u32s(reference, [0, 0, 123 if bound else 0, resource])
    o.write_u32(resource, vtable)
    o.write_u32(resource + 0x4c, kind)
    o.write_u32(vtable + 0xc, 0x300f0000)
    o.write_u32(vtable + 0x14, 0x300f0010)
    o.write_u32(data + 0xc, payload)
    o.write_u32(data + 0x44, template_ref if template else 0)
    o.write_u32(data + 0x4c, body)
    o.write_u32(data + 0x50, script)
    o.write_u32(template_data + 0xc, payload)
    o.write_u32(payload, payload_vt)
    o.write_u32(payload_vt + 0x3c, 0x300f0020)
    events = []
    o.hook(0x300f0000, lambda oc, args: data, 'thiscall', 1)
    o.hook(0x300f0010, lambda oc, args: events.append(['load', args[0]]) or 0,
           'thiscall', 1)
    o.hook(0x5447d0, lambda oc, args: flags, 'thiscall', 3)
    o.hook(0x419860, lambda oc, args: events.append(['template']) or template_data,
           'thiscall', 4)
    o.hook(0x300f0020,
           lambda oc, args: events.append(['native', *args]) or 17,
           'thiscall', 3)
    def callback(oc, args):
        assert args[0] == data
        assert oc.read_cstr(args[1]) == b'getInfo'
        events.append(['script', args[2], args[3], args[4], args[5]])
        return 23
    o.hook(0x425a90, callback, 'cdecl', 6)
    result = o.call(0x426470, [reference, 8, 37], convention='thiscall',
                    this=0x636338, fpu_cw=CW_D3D9)
    return dict(kind=kind, body=body, script=script, template=template,
                flags=flags, bound=bound, result=result.eax, events=events)


if __name__ == '__main__':
    rows = [run(*case) for case in [
        (1, 91, 73), (1, 0, 73), (1, 91, 0), (1, 0, 0),
        (1, 91, 73, False), (8, 91, 73), (2, 91, 73),
        (1, 91, 73, True, 0x80000000),
        (1, 91, 73, True, 0, False),
    ]]
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         scope=__doc__, rows=rows), sort_keys=True))
