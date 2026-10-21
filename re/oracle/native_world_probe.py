#!/usr/bin/env python3
"""Original4a5580 initialization and4a7a80 dynamic-root sleeping, unhooked callees.

Scenario: zeroed world storage with constructor body-list sentinels; allocation
uses the same declared bump allocator as the existing cfg/entity probes.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
WORLD=0x6409c4
def image(o):
    return dict(heap=o.heap_ptr,world=o.read_u32s(WORLD,0x210//4),words=[[p,o.read_u32(p)] for p in range(HEAP_BASE,o.heap_ptr,4) if o.read_u32(p)])
def run():
    rows=[]
    for sleeping,storage,activating in [(False,False,False),(True,False,False),(False,True,False),(True,True,True)]:
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
        if storage:
            o.hook(0x5d707d,lambda oc,args:0,'cdecl',1)
            o.call(0x497b60,fpu_cw=CW_D3D9)
        o.call(0x4a5580,[WORLD],convention='fastcall',fpu_cw=CW_D3D9,limit=20_000_000)
        initial=image(o)
        if sleeping:
            position=o.scratch(12);o.write(position,bytes(12))
            body=o.call(0x49ab60,[0x42c80000,5,0x40000000,0x40400000,0x40800000,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
            entity=o.call(0x49b300,[body,position,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
            o.call(0x4a7a80,[entity],convention='fastcall',fpu_cw=CW_D3D9)
            if activating:o.call(0x4a7990,[entity],convention='fastcall',fpu_cw=CW_D3D9)
        rows.append(dict(sleeping=sleeping,storage=storage,activating=activating,initial=initial,after=image(o)))
    return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
