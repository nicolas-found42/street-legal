#!/usr/bin/env python3
"""Original427370/4274e0 registrations and426f60 mutable drive-list traversal.

Only malloc/free and the per-resource426e10 frame callback are controlled.
All list and weak-reference instructions execute original code. The callback
records its live reference. Registration/removal sequences include duplicates,
unbound inputs, missing removals and deferred destruction. These observations
cover list semantics, not per-resource frame dispatch or stock reachability.
"""
import hashlib
import itertools
import json
import struct
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE, HEAP_BASE
from unicorn import UC_HOOK_CODE


def initialize(o, ctx):
    for offset, count in [(0, 0xc4), (0x1c, 0xc8), (0x38, 0xcc)]:
        head=ctx+offset
        o.call(0x428fd0,convention='thiscall',this=head)
        o.write_u32(ctx+count,0)


def capture():
    rows=[]
    operations=[[(1,0),(1,0),(1,1),(2,0)],[(1,0),(1,1),(2,0),(2,0)],
                [(1,0),(1,1),(2,1),(1,1)],[(2,0),(1,0),(1,2)],
                [(1,3),(2,3),(1,0)],[(1,0),(1,1),(1,2),(2,1)]]
    for kind, sequence in itertools.product([0,7,8,9,0x1c,0x1d],operations):
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc()
        context=o.alloc(0x200)
        resources=[o.alloc(0x100) for _ in range(3)]
        references=[o.alloc(16) for _ in range(4)]
        initialize(o,context)
        for i,r in enumerate(resources):
            o.write_u32(r+0x4c,1 if i!=2 else 8)
            o.write_u32(r+0x50,0xffff1000+i)
            o.call(0x429060,[r],convention='thiscall',this=references[i])
        before=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)
        actions=[]
        for op,index in sequence:
            o.call(0x427370 if op==1 else 0x4274e0,[references[index],kind],
                   convention='thiscall',this=context)
            actions.append(dict(op=op,index=index,after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)))
        # For drive, preserve stale nodes/counts until the real426f60 pass.
        calls=[];frees=[]
        def callback(oc,args):
            ref=args[1]
            calls.append(dict(dt=args[0],reference=ref,words=oc.read_u32s(ref,4)))
            return 0
        def free(oc,args):frees.append(args[0]);return 0
        o.hook(0x426e10,callback,'thiscall',2)
        o.hook(0x54f5b0,free,'cdecl',1)
        if kind==9:o.call(0x426f60,[0x3c888889],convention='thiscall',this=context)
        rows.append(dict(kind=kind,sequence=sequence,context=context,resources=resources,references=references,
                         before=before,actions=actions,calls=calls,frees=frees,
                         after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)))
    return dict(schema=1,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                fpu_cw=CW_D3D9,scope=__doc__,rows=rows,mutations=mutable_cases())


def mutable_cases():
    """A declared frame collaborator calls the real nested register/unregister.

   426e10's entire per-resource behavior is replaced here. The stub executes
    normal x86 CALLs to427370/4274e0 once, retaining the outer traversal's CPU
    context. The enclosing426f60 and all list/weak-reference code stay original.
    """
    scenarios=[([0],[(1,1)]),([0,1],[(1,2)]),([0,1],[(2,1)]),
               ([0,1],[(2,0)]),([0,1,2],[(2,1),(1,1)]),
               ([0,0,1],[(2,0)]),([0,1],[(2,1),(2,0)]),
               ([0,1],[(1,0)])]
    rows=[]
    for initial,mutation in scenarios:
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc()
        context=o.alloc(0x200);initialize(o,context)
        resources=[o.alloc(0x100) for _ in range(3)]
        references=[o.alloc(16) for _ in range(3)]
        for i,(resource,ref) in enumerate(zip(resources,references)):
            o.write_u32(resource+0x4c,1);o.write_u32(resource+0x50,0xffff2000+i)
            o.call(0x429060,[resource],convention='thiscall',this=ref)
        for index in initial:o.call(0x427370,[references[index],9],convention='thiscall',this=context)
        before=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)
        stub=o.scratch(256);counter=stub+240
        p=lambda x:struct.pack('<I',x&0xffffffff)
        code=bytearray(b'\x83\x3d'+p(counter)+b'\0\x75\0')
        code+=b'\xc7\x05'+p(counter)+p(1)+b'\x60'
        for op,index in mutation:
            code+=b'\xb9'+p(context)+b'\x68'+p(9)+b'\x68'+p(references[index])
            target=0x427370 if op==1 else 0x4274e0
            code+=b'\xe8'+p(target-(stub+len(code)+5))
        code+=b'\x61';code[8]=len(code)-9;code+=b'\xc2\x08\x00'
        o.write(stub,code);o.write_u32(counter,0)
        calls=[];frees=[]
        def observe(uc,address,size,user):
            if address!=0x426e10:return
            esp=o.reg('esp');ref=o.read_u32(esp+8)
            calls.append(dict(dt=o.read_u32(esp+4),reference=ref,words=o.read_u32s(ref,4)))
            uc.reg_write(__import__('unicorn').x86_const.UC_X86_REG_EIP,stub)
        o.uc.hook_add(UC_HOOK_CODE,observe)
        def free(oc,args):frees.append(args[0]);return 0
        o.hook(0x54f5b0,free,'cdecl',1)
        passes=[]
        for _ in range(2):
            calls.clear();frees.clear()
            o.call(0x426f60,[0x3c888889],convention='thiscall',this=context)
            passes.append(dict(calls=list(calls),frees=list(frees),
                               after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)))
        rows.append(dict(initial=initial,mutation=mutation,context=context,
                         resources=resources,references=references,before=before,passes=passes,
                         collaborator_code=code.hex()))
    return rows


if __name__=='__main__':print(json.dumps(capture(),sort_keys=True))
