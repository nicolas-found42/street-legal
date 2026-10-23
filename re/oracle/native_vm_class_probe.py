#!/usr/bin/env python3
"""Original403a90 physical class identity/descriptor/cache memory.

Supplied module/CONS graphs are declared inputs; actual406f60/406140 and string
append/interner/collision instructions execute. Only Scenario allocator, clock,
logging and fatal-position collaborators remain. All initial/final heap bytes
are retained; machine scratch/register parity is not claimed by this producer.
"""
from pathlib import Path
import hashlib,itertools,json,random,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE

def capture(label,names,seed):
    s=Scenario();o=s.o
    for target in [0x406f60,0x406140,0x551140]:o.hooks.pop(target,None)
    holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW)
    cache=o.alloc(0x182c);o.call(0x41a450,[],convention='thiscall',this=cache,fpu_cw=CW)
    o.write_u32(s.jvm+8,cache);o.write_u32(s.jvm+0x68,holder)
    inputs=[];rng=random.Random(seed)
    for index,name in enumerate(names):
        raw=name.encode();text=o.alloc(len(raw)+1);o.write(text,raw+b'\0')
        string=o.alloc(16);o.write_u32s(string,[0,0,text,len(raw)])
        classref=o.alloc(12);o.write_u32s(classref,[0,4,0])
        data=o.alloc(8);o.write_u32s(data,[string,classref])
        pool=o.alloc(20);o.write_u32s(pool,[0,0,2,data,2])
        module=o.alloc(0x80);o.write(module,bytes(rng.randrange(256)for _ in range(0x80)))
        bodies=o.alloc(12);nodes=o.alloc(8);o.write_u32s(nodes,[0x10,0])
        bodydata=o.alloc(4);o.write_u32(bodydata,nodes);o.write_u32s(bodies,[1,bodydata,1])
        for off,value in [(0x24,pool),(0x38,index%2),(0x3c,1),(0x60,bodies)]:o.write_u32(module+off,value)
        this=o.alloc(0x1d8);o.write(this,bytes(rng.randrange(256)for _ in range(0x1d8)))
        inputs.append({'this':this,'module':module,'name':name})
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap()
    for item in inputs:
        result=o.call(0x403a90,[s.jvm,item['module']],convention='thiscall',this=item['this'],fpu_cw=CW)
        if result.eax!=item['this']:raise RuntimeError('original class constructor returned different instance')
    return {'label':label,'seed':seed,'jvm':s.jvm,'inputs':inputs,'before':before,'after':heap()}

def main():
    collide=['Probe.'+''.join(p)for p in sorted(set(itertools.permutations('aabbccdd')))[:70]]
    cases=[('simple_and_duplicate',['Probe','Other','Probe','']),('cache_growth',collide),('duplicate_records',['Probe']*12)]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':1,'scope':__doc__,'fpu_cw':CW,
                      'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},
                      'rows':[capture(label,names,seed)for label,names in cases for seed in [606,20261022]]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
