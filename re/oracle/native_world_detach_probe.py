#!/usr/bin/env python3
"""Original world attachment, relative teardown and angular momentum graphs.

Run: ~/slr-oracle-venv/bin/python re/oracle/native_world_detach_probe.py > re/out/boot/native-world-detach-original.json
Only the zeroed bump allocator/free is hooked. Constructors, attachment,
relative transforms, compound recomputation, inverse inertia and matrix inverse
execute original instructions. Complete heap/world words are compared; stack
scratch is captured as input and is not compared as observable output.
"""
import hashlib
import itertools
import json
import random
import struct
from pathlib import Path

from oracle import Oracle, DEFAULT_EXE, CW_D3D9, HEAP_BASE, STACK_TOP

WORLD = 0x6409c4
STACK_BASE = STACK_TOP - 0x1000


def bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def image(o):
    return dict(heap=o.heap_ptr,
                words=o.read_u32s(HEAP_BASE, (o.heap_ptr - HEAP_BASE) // 4),
                world=o.read_u32s(WORLD, 0x210 // 4))


def capture(o, address, this, args, **labels):
    before = image(o)
    stack = o.read_u32s(STACK_BASE, 0x1000 // 4)
    result = o.call(address, args, convention='thiscall', this=this, fpu_cw=CW_D3D9)
    return dict(address=address, this=this, args=args, before=before,
                stack=stack, after=image(o), fpu_depth=result.fpu_top_depth,
                fpu_cw=result.fpcw, **labels)


def world():
    o = Oracle()
    o.hook_alloc()
    o.write(WORLD, bytes(0x400))
    o.write_u32(0x617650, WORLD)
    o.write_u32(WORLD + 0x188, WORLD + 0x190)
    o.write_u32(WORLD + 0x198, WORLD + 0x184)
    # The process has already run the compound helper's static identity
    # initializer. Supply that input state, without entering CRT atexit.
    o.write_u32(0x640bd0, 1)
    o.call(0x54e1d0, convention='thiscall', this=0x643638, fpu_cw=CW_D3D9)
    return o


def entity(o, mass, position, oriented=False):
    body = o.call(0x49ab60, [bits(mass), 5, bits(2), bits(3), bits(4), 0],
                  convention='thiscall', this=WORLD, fpu_cw=CW_D3D9).eax
    point = o.alloc(12)
    o.write_f32s(point, position)
    rotation = 0
    if oriented:
        rotation = o.alloc(0x30)
        for r, values in enumerate([[.9877, 0, -.15635], [0, 1, 0], [.15635, 0, .9877]]):
            o.write_f32s(rotation + r * 16, values)
    return o.call(0x49b300, [body, point, rotation], convention='thiscall', this=WORLD,
                  fpu_cw=CW_D3D9).eax


def run():
    rows = []
    rng = random.Random(0x4a6470)
    matrices = [[1, 0, 0, 0, 1, 0, 0, 0, 1], [0] * 9,
                [1, 2, 3, 2, 4, 6, 0, 0, 0]]
    matrices += [[rng.uniform(-100, 100) for _ in range(9)] for _ in range(32)]
    for matrix in matrices:
        o = world()
        state = o.alloc(0xd4)
        velocity = o.alloc(12)
        o.write_f32s(velocity, [rng.uniform(-10, 10) for _ in range(3)])
        for r in range(3):
            o.write_f32s(state + 0xa4 + r * 16, matrix[r * 3:r * 3 + 3])
        rows.append(capture(o, 0x4a6470, state, [velocity], kind='angular'))
    for static_parent, siblings, oriented in itertools.product([False, True], [0, 2], [False, True]):
        o = world()
        parent = entity(o, 100, [2, 3, 4], oriented)
        member = entity(o, 1, [.1, .2, .3], oriented)
        if static_parent:
            body = o.read_u32(parent + 0x5c)
            o.write_u32(body + 0x9c, o.read_u32(body + 0x9c) | 0x100)
        for n in range(siblings):
            sibling = entity(o, n + 2, [n - .1, n + .2, n + .3])
            o.call(0x49a810, [sibling, parent], convention='thiscall', this=WORLD,
                   fpu_cw=CW_D3D9)
        rows.append(capture(o, 0x49a810, WORLD, [member, parent],
                            kind='attach', static_parent=static_parent, siblings=siblings, oriented=oriented))
        if not static_parent:
            o.call(0x49a050, [parent], convention='thiscall', this=WORLD,
                   fpu_cw=CW_D3D9)
        for index in [0, 1]:
            # Separate integration buffers, with nonzero angular velocity.
            o.write_u32(member + 0x20c, index)
            o.write_f32s(member + 0x64 + index * 0xd4 + 0x70, [.4, -.2, .7])
            rows.append(capture(o, 0x49a760, WORLD, [member], kind='detach',
                                static_parent=static_parent, siblings=siblings, oriented=oriented, index=index))
            if index == 0:
                o.call(0x49a810, [member, parent], convention='thiscall', this=WORLD,
                       fpu_cw=CW_D3D9)
        o.call(0x49a810, [member, parent], convention='thiscall', this=WORLD,
               fpu_cw=CW_D3D9)
        sleeping = entity(o, 0, [0, 0, 0])
        body = o.read_u32(sleeping + 0x5c)
        o.write_u32(body + 0x9c, o.read_u32(body + 0x9c) | 0x100)
        o.write_u32(WORLD + 0x84, parent)
        o.write_u32(WORLD + 0x88, sleeping)
        rows.append(capture(o, 0x4a7a80, member, [], kind='sleep',
                            static_parent=static_parent, siblings=siblings, oriented=oriented))
    return dict(schema_version=1, exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                fpu_cw=CW_D3D9, scope=__doc__, rows=rows)


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
