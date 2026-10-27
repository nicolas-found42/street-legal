#!/usr/bin/env python3
"""Execute original416b00 registration,41dc30 signature and canonical names.

Only Scenario allocator/log/clock/error collaborators are controlled. Native
implementation addresses are input data and never executed. Complete heaps,
ordered CPU heap stores and full EAX results retained; caller stack/GPR parity
is explicitly outside this memory projection.
"""
from pathlib import Path
import hashlib,itertools,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE,STACK_TOP,STACK_SIZE
from unicorn import UC_HOOK_MEM_WRITE

def row(label,records,seed):
    s=Scenario();o=s.o;holder=o.alloc(8)
    o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW)
    o.write_u32(s.jvm+0x68,holder)
    registry=o.alloc(0x628);o.write_u32(s.jvm+12,registry)
    o.alloc(seed%41+1);inputs=[]
    for i,(cls,method,signature) in enumerate(records):
        pointers=[]
        for raw in [cls,method,signature]:
            p=o.alloc(len(raw)+1);o.write(p,raw+b'\0');pointers.append(p)
        inputs.append(dict(class_bytes=list(cls),method_bytes=list(method),signature_bytes=list(signature),pointers=pointers,implementation=0x470000+i*16))
    def heap():return dict(heap_ptr=o.heap_ptr,bytes=list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE)))
    before=heap();stores=[]
    def write(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            assert address+size<=o.heap_ptr
            stores.append([address,size,value&((1<<(8*size))-1)])
        elif not(STACK_TOP-STACK_SIZE<=address and address+size<=STACK_TOP):
            raise RuntimeError(f'undeclared write {address:x}/{size}')
    o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
    for item in inputs:
        r=o.call(0x416b00,item['pointers']+[item['implementation']],convention='thiscall',this=s.jvm,fpu_cw=CW)
        assert r.fpcw==CW and r.fpu_top_depth==0
        item['result']=r.eax
    assert not o.import_log
    return dict(label=label,seed=seed,jvm=s.jvm,inputs=inputs,before=before,after=heap(),stores=stores)
def main():
    collision=[b'Probe.'+bytes(p) for p in itertools.islice(dict.fromkeys(itertools.permutations(b'aabbccdd')),70)]
    cases=[('basic_duplicate',[(b'java.io.Input',b'lastKey',b'()I'),(b'java.render.Osd',b'animate',b'()V'),(b'java.io.Input',b'lastKey',b'()I')]),('collision_growth',[(n,b'm',b'(IFLjava.lang.String;)[I') for n in collision]),('empty_signed',[(b'',b'',b'()V'),(b'\x80\xff',b'\xfe',b'()I'),(b'x',b'\x80',b'()F')])]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps(dict(schema=1,scope=__doc__,fpu_cw=CW,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},rows=[row(k,v,seed)for k,v in cases for seed in [606,20261023]]),sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
