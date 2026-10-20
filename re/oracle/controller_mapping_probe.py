#!/usr/bin/env python3
"""Original user_Add/Del/Reset -> mapping table, with declared loaded player data.

Marshalling and mapping operations execute original x86. Native.ptr lookup,
resource acquisition and its virtual data accessor are controlled seams. Device
state is absent, so original axis/sample lookups take their zero branch. This
validates mapping identities and slots, not DirectInput or player commands.
"""
import hashlib
import json
from pathlib import Path

from oracle import DEFAULT_EXE, Oracle
from io_vectors import INPUT_ROWS, _cell, _call_frame, input_params


def run():
    assert hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle()
    o.hook_alloc()
    handle, resource, vtable, data = [o.alloc(0x100) for _ in range(4)]
    controller = o.alloc(0x2200)
    o.write_u32(handle + 12, resource)
    o.write_u32(resource, vtable)
    o.write_u32(resource + 0x4c, 1)
    o.write_u32(vtable + 12, 0x300f0000)
    o.write_u32(data + 0x4c, controller - 0x1c)
    o.hook(0x300f0000, lambda oc, a: data, 'thiscall', 1)
    o.hook(0x42ab50, lambda oc, a: handle, 'cdecl', 2)
    o.hook(0x5447d0, lambda oc, a: 0, 'thiscall', 3)
    operations = []

    def call(name, values):
        address = {'add': 0x477740, 'del': 0x477810, 'reset': 0x477a00}[name]
        params = input_params(address, *values)
        cells = [_cell(o, ty, value) for ty, value in reversed(params)] + [_cell(o, 'L', 123)]
        frame = _call_frame(o, *INPUT_ROWS[address], cells)
        result = o.call(address, [frame]).eax
        operations.append({'name': name, 'params': [value & 0xffffffff for _, value in params],
                           'result': result if name != 'reset' else None,
                           'count': o.read_u32(controller),
                           'slots': [o.read_u32s(controller + 0xe44 + i * 0x20, 8) for i in range(3)]})

    call('add', [1, 0, 1, .25, 1.5, -2., 4.])
    call('add', [1, 0, 1, 0., 1., 0., 1.])  # duplicates occupy another slot
    call('add', [2, -1, 0, 0., 1., 0., 1.])
    call('add', [2, 0, -1, 0., 1., 0., 1.])
    call('del', [1, 0, 1])
    call('add', [3, 5, 999, 0., 1., 0., 1.])  # nonnegative unknown devices still allocate
    call('del', [9, 0, 1])
    call('reset', [])
    call('add', [0, 0, 1, .5, 1., 0., 1.])  # zero action stays a free sentinel
    call('add', [1, 0, 1, 0., 1., 0., 1.])
    call('reset', [])
    for i in range(153):
        call('add', [1, 0, i, 0., 1., 0., 1.])
    call('del', [1, 0, 0])
    call('add', [7, 0, 1, 0., 1., 0., 1.])
    return {'oracle': 'original477740/477810/477a00 + 54d650/54d700/54d750; controlled Native.ptr and loaded data; absent devices', 'operations': operations}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
