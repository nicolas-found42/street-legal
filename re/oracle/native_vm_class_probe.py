#!/usr/bin/env python3
"""Original403a90 class metadata and cache, composed with original CONS and names.

Module fields, cache/name holders and raw source bytes are explicit inputs.
406f60/406140, descriptor formatting and name interning execute real code.
Only Scenario allocator/log/clock/fatal-position collaborators remain. Complete
heaps and all ordered CPU heap stores are retained. Native scratch/register
parity remains outside this memory seam; the observed stack local is explicit.
"""
from pathlib import Path
import hashlib,itertools,json,random,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE

def u(n):return struct.pack('<I',n)
def row(label,names,seed):
    s=Scenario();o=s.o
    for a in [0x406f60,0x406140,0x551140]:o.hooks.pop(a,None)
    holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
    cache=o.alloc(0x1828);o.write_u32(s.jvm+8,cache)
    inputs=[];rng=random.Random(seed)
    for name in names:
        name=name.encode();data=u(2)+u(0)+u(len(name))+name+b'\0'+u(4)+u(0)
        source=o.alloc(len(data));o.write(source,data);pool=o.alloc(20)
        o.call(0x405d20,[],convention='thiscall',this=pool,fpu_cw=CW)
        o.call(0x407660,[source],convention='thiscall',this=pool,fpu_cw=CW)
        module=o.alloc(0x80);o.write_u32(module+0x24,pool);o.write_u32(module+0x3c,1)
        o.write_u32(module+0x38,rng.randrange(0x100000000));o.write_u32(module+0x60,o.alloc(24))
        this=o.alloc(0x1d8);o.write(this,bytes(rng.randrange(256) for _ in range(0x1d8)))
        inputs.append({'module':module,'this':this,'name':name.decode()})
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();stores=[];current=None
    def observe(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            assert address+size<=o.heap_ptr
            stores.append({'address':address,'size':size,'value':value&((1<<(size*8))-1)})
    def code(uc,address,size,_):
        if address==0x423e10:
            current['descriptor_scratch']=o.read_u32(o.reg('esp')+8)
    o.uc.hook_add(UC_HOOK_MEM_WRITE,observe);o.uc.hook_add(UC_HOOK_CODE,code)
    for current in inputs:o.call(0x403a90,[s.jvm,current['module']],convention='thiscall',this=current['this'],fpu_cw=CW)
    return {'label':label,'seed':seed,'jvm':s.jvm,'inputs':inputs,'before':before,'after':heap(),'stores':stores}
def main():
    collision=['Probe.'+''.join(p) for p in itertools.islice(dict.fromkeys(itertools.permutations('aabbccdd')),70)]
    cases=[('simple_duplicate',['Probe','Other','Probe','']),('cache_growth',collision),('duplicate_records',['Probe']*12),('array_descriptors',['[I','[[F','[java.game.Player','[[java.game.Car','java.render.Osd'])]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':2,'scope':__doc__,'fpu_cw':CW,'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'rows':[row(k,v,seed) for k,v in cases for seed in [606,20261023]]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
