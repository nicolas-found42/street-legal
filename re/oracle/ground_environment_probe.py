#!/usr/bin/env python3
"""Build-606 GroundRef environment natives and complete water setter callees.

Run: python re/oracle/ground_environment_probe.py > re/out/ground_environment.json
Only VM field lookup, allocation and fog message enqueue are collaborators.
The native argument marshaller, water comparisons, normalization, replacement
and limit append execute as original instructions. No derived data is committed.
"""
import hashlib
import json
import random
import struct
from pathlib import Path

from oracle import CW_D3D9, DEFAULT_EXE, Oracle
from res_vectors import World, _cell, _frame
from unicorn import x86_const as X


def bits(value):
    return struct.unpack('<I', struct.pack('<f', value))[0]


def projection(o, world):
    count = o.read_u32(world + 0x1d4)
    return dict(enabled=o.read_u32(world + 0x80),
                position=o.read_u32s(world + 0x1b8, 3),
                normal=o.read_u32s(world + 0x1c4, 3),
                extra=o.read_u32s(world + 0x1d8, 3),
                sizes=o.read_u32s(world + 0x1e4, 2),
                limits=[o.read_u32s(o.read_u32(world + 0x1d0) + n * 24, 6)
                        for n in range(count)])


def run_case(kind, bound, values):
    w = World()
    o = w.o
    world = o.alloc(0x200)
    o.write_u32(0x617650, world)
    w.obj = o.alloc(16)
    o.write_u32(w.obj + 12, 0x12345678 if bound else 0)
    o.write_u32(world + 0x80, 7)
    o.write_u32s(world + 0x1b8, [bits(13), bits(-12), bits(15)])
    o.write_u32s(world + 0x1c4, [bits(3), bits(4), bits(5)])
    o.write_u32s(world + 0x1d8, [bits(7), bits(8), bits(9)])
    o.write_u32s(world + 0x1e4, [bits(23), bits(24)])
    initial_limits = [list(range(1, 7)), list(range(10, 16))]
    buf = o.alloc(48)
    o.write_u32s(buf, sum(initial_limits, []))
    o.write_u32(world + 0x1d0, buf)
    o.write_u32(world + 0x1d4, 2)
    before = projection(o, world)
    vector_words = [o.alloc(12), o.alloc(12)]
    fields = []
    for pointer, words in zip(vector_words, values.get('vectors', [])):
        o.write_u32s(pointer, words)

    def component(oc, args):
        name = oc.read_cstr(args[1], 16).decode('ascii')
        fields.append([vector_words.index(args[0]), name])
        word = oc.read_u32(args[0] + ['x', 'y', 'z'].index(name) * 4)
        if (word & 0x7f800000) != 0x7f800000:
            return struct.unpack('<f', struct.pack('<I', word))[0]
        # Oracle._fpu_push accepts finite Python floats only. Preserve the
        # original FLD m32 infinity/NaN image, including payload and sign.
        top = (oc._fpu_top() - 1) & 7
        sw = (oc.uc.reg_read(X.UC_X86_REG_FPSW) & ~0x3800) | (top << 11)
        oc.uc.reg_write(X.UC_X86_REG_FPSW, sw)
        tag = oc.uc.reg_read(X.UC_X86_REG_FPTAG) & 0xffff & ~(3 << (2 * top))
        oc.uc.reg_write(X.UC_X86_REG_FPTAG, tag)
        mantissa = (1 << 63) | ((word & 0x7fffff) << 40)
        if word & 0x7fffff:
            mantissa |= 1 << 62
        oc.uc.reg_write(X.UC_X86_REG_FP0 + top,
                        (mantissa, 0x7fff | ((word >> 16) & 0x8000)))

    o.hook(0x42a560, component, 'cdecl', 2)
    messages = []

    def enqueue(oc, args):
        messages.append(dict(receiver=args[0], code=args[1],
                             payload=oc.read_u32s(args[2], 4)))
        return 0

    o.hook(0x426470, enqueue, 'thiscall', 3)
    if kind == 'simple':
        address, sig = 0x4866c0, b'(FFF)V'
        args = [('F', word) for word in values['scalars']]
    elif kind == 'plane':
        address, sig = 0x4867b0, b'(Ljava.lang.Vector3;Ljava.lang.Vector3;FF)V'
        args = [('L', pointer) for pointer in vector_words]
        args += [('F', word) for word in values['scalars']]
    elif kind == 'limit':
        address, sig = 0x486920, b'(Ljava.lang.Vector3;Ljava.lang.Vector3;)V'
        args = [('L', pointer) for pointer in vector_words]
    else:
        address, sig = 0x486a20, b'(IFF)V'
        args = [('I', values['color'])] + [('F', word) for word in values['scalars']]
    cells = [_cell(o, ty, word) for ty, word in reversed(args)]
    cells.append(_cell(o, 'L', 0x2468))
    frame = _frame(w, sig, cells, b'java.util.resource.GroundRef')
    o.call(address, [frame], fpu_cw=CW_D3D9)
    return dict(kind=kind, bound=bound, values=values, before=before,
                after=projection(o, world), fields=fields,
                messages=[dict(code=m['code'], payload=m['payload']) for m in messages])


def run():
    rng = random.Random(0x4866c0)
    rows = []
    edge = [0, 0x80000000, bits(-1), bits(1), 1, 0x7f7fffff,
            0x7f800000, 0xff800000, 0x7fc12345, 0x7f812345,
            0xff812345, 0x007fffff, 0x00800000, 0x000ccccc,
            0x000ccccd, 0x000cccce]
    for kind in ['simple', 'plane', 'limit', 'fog']:
        cases = []
        for value in edge:
            cases.append(dict(scalars=[bits(-12), value, value] if kind == 'simple'
                              else [value, value],
                              vectors=[[value, bits(1), bits(2)], [bits(3), value, bits(4)]],
                              color=0xa1b2c3d4))
        for _ in range(24):
            cases.append(dict(scalars=[bits(rng.uniform(-20, 1000))
                                       for _ in range(3 if kind == 'simple' else 2)],
                              vectors=[[bits(rng.uniform(-100, 100)) for _ in range(3)]
                                       for _ in range(2)], color=rng.getrandbits(32)))
        # Exercise the complete exponent range and stored subnormal rounding,
        # rather than sampling only ordinary f32 values via Python doubles.
        for _ in range(32):
            cases.append(dict(scalars=[rng.getrandbits(32)
                                       for _ in range(3 if kind == 'simple' else 2)],
                              vectors=[[rng.getrandbits(32) for _ in range(3)]
                                       for _ in range(2)], color=rng.getrandbits(32)))
        for bound in [False, True]:
            for case in cases:
                rows.append(run_case(kind, bound, case))
    helpers = []
    normals = [[0, 0, 0], [bits(3), bits(4), 0], [1, 2, 3],
               [0x7f7fffff] * 3, [0x7f800000, 0, 0],
               [0x7fc12345, bits(1), bits(2)], [0x7f812345, bits(1), bits(2)],
               [0x80000000, 0x80000001, bits(-2)]]
    for mask in range(8):
        for normal in normals:
            o = Oracle()
            o.hook_alloc()
            world = o.alloc(0x200)
            o.write_u32(world + 0x80, 7)
            for offset in [0x1b8, 0x1c4, 0x1d8]:
                o.write_u32s(world + offset, [bits(13), bits(14), bits(15)])
            buf = o.alloc(24)
            o.write_u32s(buf, list(range(6)))
            o.write_u32(world + 0x1d0, buf)
            o.write_u32(world + 0x1d4, 1)
            inputs = [normal, [bits(-7), bits(8), bits(-9)], [bits(3), bits(2), bits(1)]]
            pointers = []
            for i, words in enumerate(inputs):
                pointer = o.alloc(12)
                o.write_u32s(pointer, words)
                pointers.append(pointer if mask & (1 << i) else 0)
            before = projection(o, world)
            sizes = [bits(34), bits(56)]
            o.call(0x49b440, sizes + pointers, convention='thiscall', this=world, fpu_cw=CW_D3D9)
            helpers.append(dict(mask=mask, inputs=inputs, sizes=sizes, before=before, after=projection(o, world)))
    return dict(schema_version=2, scope=__doc__,
                exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                fpu_cw=CW_D3D9,
                addresses=[0x4866c0, 0x4867b0, 0x486920, 0x486a20, 0x49b440, 0x49b530],
                collaborators=["VM field lookup", "zeroed bump allocation/free", "fog enqueue"],
                row_count=len(rows), helper_count=len(helpers), rows=rows, helpers=helpers)


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
