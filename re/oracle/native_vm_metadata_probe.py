#!/usr/bin/env python3
"""Original Thread41f340 and frame407b10 construction over live raw metadata.

Scenario's existing original JVM/thread/list/frame-pool graph is the explicit
source input. New constructors execute unmodified instructions. Only Scenario
malloc/realloc/free, clock, logging and error-position collaborators are hooked.
This is bounded constructor evidence, not a boot/callback implementation.
"""
from pathlib import Path
import hashlib, json, random, sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import HEAP_BASE,STACK_TOP,DEFAULT_EXE

REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']

def snapshot(o,base):
    return {'heap_ptr':o.heap_ptr,'heap':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE)),
            'stack':list(o.read(base,0x400)), 'registers':{r:o.reg(r) for r in REGS}}

def row(kind,shape,name,seed):
    s=Scenario();o=s.o
    pool=o.read_u32(0x62ddf0)
    if kind=='thread':
        if shape=='empty':
            pool=o.alloc(20);o.write_u32(0x62ddf0,pool)
        elif shape=='grow':
            for _ in range(255):o.call(0x408d90,[pool],convention='fastcall',fpu_cw=CW)
        elif shape=='freelist':
            f=o.call(0x408d90,[pool],convention='fastcall',fpu_cw=CW).eax
            o.call(0x407b10,[s.thread,0,0,0,0],convention='thiscall',this=f,fpu_cw=CW)
            o.call(0x407d10,[1],convention='thiscall',this=f,fpu_cw=CW)
        else:assert shape=='existing'
        source_name=o.alloc(len(name)+1);o.write(source_name,name.encode()+b'\0')
        this=o.alloc(0x100)
        args=[s.jvm,seed%9,seed^0xa5a55a5a,source_name]
        address=0x41f340
    else:
        this=o.alloc(64)
        args=[s.thread,o.alloc(64),o.alloc(24),seed^0x12345678,shape]
        address=0x407b10
    rng=random.Random(seed)
    o.write(this,bytes(rng.randrange(256) for _ in range(0x100 if kind=='thread' else 64)))
    base=STACK_TOP-0x500
    o.write(base,bytes(rng.randrange(256) for _ in range(0x400)))
    before=snapshot(o,base)
    o.call(address,args,convention='thiscall',this=this,fpu_cw=CW)
    return {'kind':kind,'shape':shape,'name':name,'seed':seed,'this':this,'args':args,
            'frame_pool':pool,'stack_base':base,'before':before,'after':snapshot(o,base)}

def main():
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    rows=[row('thread',shape,name,seed) for shape in ['empty','existing','grow','freelist']
          for name in ['', 'thread','worker.addpart'] for seed in [606,20261022]]
    rows +=[row('frame',locals,'',seed) for locals in [0,1,3,9] for seed in [606,20261022]]
    print(json.dumps({'schema':1,'scope':__doc__,'fpu_cw':CW,
                      'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},
                      'rows':rows},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
