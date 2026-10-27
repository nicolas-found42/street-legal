#!/usr/bin/env python3
"""Original423ba0 operand push: actual pool-owned values, real frame/container.

Scenario supplies its documented allocator/clock/error-position collaborators.
No native results, VM dispatch or Thread.run instructions are replaced here.
Complete heap and retained caller stack are compared without exclusions.
"""
from pathlib import Path
import hashlib
import json
import random
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from thread_vectors import Scenario, CW, TC
from oracle import DEFAULT_EXE, HEAP_BASE, STACK_TOP
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE


def case(capacity, count, indirect, seed, nested):
    s = Scenario()
    o = s.o
    frame = o.read_u32(s.thread + 0x20)
    container = o.alloc(12) if indirect else frame + 0x1c
    data = o.alloc(capacity * 4) if capacity else 0
    o.write_u32s(container, [capacity, data, count])
    o.write_u32(frame + 0x28, container)
    source = o.alloc(12)
    cells = []
    for index in range(4):
        pointer = o.call(0x423c00, [o.read_u32(0x62f130)], convention='fastcall', fpu_cw=CW).eax
        # Equal payloads deliberately have distinct live cell addresses.
        o.write_u32s(source, [0x5e734c, TC['I'], 7 if index < 2 else index])
        o.call(0x423e30, [source], convention='thiscall', this=pointer, fpu_cw=CW)
        cells.append(pointer)
    for index in range(count):
        o.write_u32(data + index * 4, cells[index % 4])
    parent, holder = 0, 0
    incoming = {'ebx':seed ^ 0x11335577, 'esi':seed ^ 0x22446688, 'edi':seed ^ 0x99aabbcc}
    if nested:
        # Declared synthetic caller drives the actual pending CALL. Its bytes
        # are evidence inputs, never copied into the production implementation.
        parent, holder = o.alloc(32), o.alloc(4)
        code = b'\xb9' + struct.pack('<I',s.thread) + b'\xff\x35' + struct.pack('<I',holder)
        code += b'\xe8' + struct.pack('<I',(0x423ba0 - (parent+16)) & 0xffffffff) + b'\xc3'
        o.write(parent,code)
    stack_base, stack_bytes = STACK_TOP - 0x200, 0x180
    rng = random.Random(seed)
    scratch = bytes(rng.randrange(256) for _ in range(stack_bytes))
    o.write(stack_base, scratch)
    def heap():
        return list(o.read(HEAP_BASE, o.heap_ptr - HEAP_BASE))
    before = {'heap': heap(), 'heap_ptr': o.heap_ptr, 'stack': list(scratch)}
    stores = []
    returned = {}
    def returned_observation(uc, pc, size, _):
        if nested and pc == parent+16:
            returned.clear()
            returned.update({r:o.reg(r) for r in ['eax','ebx','ecx','edx','esi','edi','ebp','esp']})
    def observe(uc, access, address, size, value, _):
        if HEAP_BASE <= address < o.heap_ptr:
            stores.append({'address': address, 'size': size, 'value': value})
    o.uc.hook_add(UC_HOOK_MEM_WRITE, observe)
    o.uc.hook_add(UC_HOOK_CODE,returned_observation)
    steps = []
    for index in range(9):
        stores.clear()
        cell = 0 if index in (1,6) else cells[index % 4]
        if nested:
            o.write_u32(holder,cell)
            o.call(parent,regs=incoming,fpu_cw=CW)
        else:
            o.call(0x423ba0, [cell], convention='thiscall', this=s.thread, fpu_cw=CW)
        steps.append({'cell': cell, 'heap': heap(), 'heap_ptr': o.heap_ptr,
                      'stack': list(o.read(stack_base, stack_bytes)), 'stores': list(stores),
                      'registers': dict(returned) if nested else {r: o.reg(r) for r in ['eax','ebx','ecx','edx','esi','edi','ebp','esp']}})
    return {'capacity': capacity, 'count': count, 'indirect': indirect, 'seed': seed,
            'thread': s.thread, 'frame': frame, 'container': container,
            'nested':nested,'parent':parent,'holder':holder,'incoming':incoming,
            'stack_base': stack_base, 'before': before, 'steps': steps}


def main():
    sources = [Path(__file__), Path(__file__).with_name('oracle.py'), Path(__file__).with_name('thread_vectors.py')]
    rows = [case(capacity,count,indirect,seed,nested)
            for capacity,count in [(0,0),(1,0),(1,1),(3,2),(3,3),(7,0),(7,6),(7,7)]
            for indirect in [False,True] for seed in [606,20261022] for nested in [False,True]]
    print(json.dumps({'schema':1, 'scope':__doc__, 'fpu_cw':CW,
                      'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},
                      'allocator':'Scenario malloc/realloc zero bump16 allocate/copy old allocation size',
                      'rows':rows},separators=(',',':'),sort_keys=True))


if __name__ == '__main__': main()
