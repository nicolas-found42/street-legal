#!/usr/bin/env python3
"""Original openLib, package lookup and header loading over a declared empty RPK.

Only file existence/open/read/close and allocation are supplied. Package name
registration, FUN_005383f0, FUN_00544170, state access and native marshalling
execute original code. The synthetic file contains no local resource 1.
"""
import hashlib
import json
import struct
from pathlib import Path

from langobj_vectors import Probe, default_exe
from unicorn import UC_HOOK_CODE


def run():
    exe = default_exe()
    assert hashlib.sha256(Path(exe).read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    p = Probe(exe)
    o = p.o
    manager = o.alloc(0x110000)
    o.write_u32(0x618D48, manager)
    system = p.plant(b'system.rpk', 122)
    assert o.call(0x538380, [system], convention='thiscall', this=manager).eax == 0
    data = b'RPAK' + struct.pack('<7I', 0x200, 0, 0, 0, 0, 0, 0)
    offset = [0]
    existence_checks = [0]
    loads, fallbacks = [], []

    def read(oc, args):
        handle, pointer, count = args
        assert handle == 1 and offset[0] + count <= len(data)
        oc.write(pointer, data[offset[0]:offset[0] + count])
        offset[0] += count
        return count

    def load(uc, address, size, user):
        package = o.reg('ecx')
        loads.append({'state': o.read_u32(package + 0x50),
                      'caller': o.read_u32(o.reg('esp'))})

    def fallback(uc, address, size, user):
        fallbacks.append(o.read_u32(o.reg('esp') + 4))

    def exists(oc, args):
        existence_checks[0] += 1
        return 1 if existence_checks[0] == 1 else 0

    o.hook(0x54C170, exists, 'cdecl', 1)
    o.hook(0x54C880, lambda oc, args: 1, 'cdecl', 1)
    o.hook(0x54CEC0, read, 'cdecl', 3)
    o.hook(0x54CCA0, lambda oc, args: 0, 'cdecl', 1)
    o.uc.hook_add(UC_HOOK_CODE, load, None, 0x544170, 0x544170)
    o.uc.hook_add(UC_HOOK_CODE, fallback, None, 0x545FC0, 0x545FC0)
    name = p.plant(b'declared-empty.rpk', 123)
    results = []
    for _ in range(2):
        info = p.call_info(0, b'(Ljava.lang.String;)I', params=[name])
        results.append(p.call(0x487BE0, info).eax)
    assert results == [1, 1] and offset[0] == len(data) and not fallbacks
    assert [entry['state'] for entry in loads] == [0, 2, 2, 2]
    return {'oracle': 'original487be0/538380/5383f0/544170/544000; supplied file IO and allocation',
            'input': 'declared valid empty RPK, no links or entries',
            'results': results, 'header_loads': loads, 'fallbacks': fallbacks,
            'bytes_read': offset[0], 'existence_checks': existence_checks[0],
            'final_header_state': o.read_u32(manager + 0xFFE94 + 0x70 + 0x50)}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
