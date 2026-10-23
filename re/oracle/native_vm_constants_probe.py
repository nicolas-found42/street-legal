#!/usr/bin/env python3
"""Original407660 raw constants, no descriptor/name interning replacement.

Only Scenario allocator/log/clock/fatal-position collaborators. Complete heaps
include raw source bytes, realloc retention, per-kind record/padding and string
buffers. This is memory evidence; FPU flags and machine scratch are not claimed.
"""
from pathlib import Path
import hashlib,json,random,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE

def capture(label,records,seed):
    s=Scenario();o=s.o
    data=struct.pack('<I',len(records))
    for kind,value in records:
        data+=struct.pack('<I',kind)
        if kind==0:data+=struct.pack('<I',len(value))+value+b'\0'
        elif isinstance(value,tuple):data+=struct.pack('<II',*value)
        else:data+=struct.pack('<I',value)
    source=o.alloc(len(data));o.write(source,data)
    pool=o.alloc(20);rng=random.Random(seed);o.write(pool,bytes(rng.randrange(256)for _ in range(20)))
    o.write_u32s(pool+8,[0,0,0])
    def heap():return{'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();o.call(0x407660,[source],convention='thiscall',this=pool,fpu_cw=CW)
    return {'label':label,'seed':seed,'pool':pool,'source':source,'source_bytes':len(data),'records':len(records),'before':before,'after':heap()}

def main():
    cases=[('empty',[]),('all_kinds',[(0,b'Fixture'),(1,0xffffffff),(2,0x3fc00000),(3,(4,0x12345678)),(4,0),(5,(3,0x87654321)),(7,(6,0xffffffff)),(0,b'')]),
           ('float_bits',[(2,n)for n in [0,0x80000000,1,0x80000001,0x7f7fffff,0x7f800000,0xff800000,0x7fc12345,0xffc12345]]),
           ('signaling_nan_boundary',[(2,n)for n in [0x7f812345,0xff812345]]),
           ('string_raw_bytes',[(0,b'first\0retained'),(0,b'\xff\x80raw'),(0,b'unchanged'),(0,b'unchanged')]),
           ('growth',[(1,n)for n in range(70)])]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':1,'scope':__doc__,'fpu_cw':CW,'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},
                      'rows':[capture(label,records,seed)for label,records in cases for seed in [606,20261022]]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
