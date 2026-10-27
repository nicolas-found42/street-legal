#!/usr/bin/env python3
"""Original407660 CONS deserialization, complete heaps and ordered heap stores.

The exact serialized CONS bytes are explicit inputs. Scenario supplies only its
allocator, logging, clock and fatal-position collaborators. No VM/native-result
hook is installed. Stack/register/FPU status parity is outside this memory seam.
"""
from pathlib import Path
import hashlib,json,random,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE
from unicorn import UC_HOOK_MEM_WRITE

def u(n):return struct.pack('<I',n)
def scalar(k,*v):return u(k)+b''.join(u(x) for x in v)
def string(b):return u(0)+u(len(b))+b+b'\0'
def row(label,entries,seed,stock=None,raw=None,unresolved=False):
    s=Scenario();o=s.o;data=raw if raw is not None else u(len(entries))+b''.join(entries)
    source=o.alloc(len(data));o.write(source,data)
    pool=o.alloc(20);o.write(pool,bytes(random.Random(seed).randrange(256) for _ in range(20)))
    # Original407660 expects an already constructed empty ConstantPool.
    o.call(0x405d20,[],convention='thiscall',this=pool,fpu_cw=CW)
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();stores=[]
    def observe(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            assert address+size<=o.heap_ptr
            stores.append({'address':address,'size':size,'value':value&((1<<(size*8))-1)})
    o.uc.hook_add(UC_HOOK_MEM_WRITE,observe)
    result=o.call(0x407660,[source],convention='thiscall',this=pool,fpu_cw=CW)
    return {'label':label,'seed':seed,'pool':pool,'source':source,'source_bytes':len(data),'record_count':struct.unpack_from('<I',data)[0],'unresolved_snan':unresolved,
            'stock':stock,'before':before,'after':heap(),'stores':stores,'fpu_depth':result.fpu_top_depth}
def stock_rows():
    rows=[]
    root=Path(__file__).resolve().parents[2]
    for relative in ['system/scripts/render/Osd.class','system/scripts/io/Input.class']:
        path=root/'stock'/relative;data=path.read_bytes();pos=0;module=-1
        while pos<len(data):
            tag=data[pos:pos+4];n=struct.unpack_from('<I',data,pos+4)[0];off=pos+8
            assert off+n<=len(data)
            if tag==b'TUFA':module+=1
            if tag==b'CONS':
                rows.append(row('stock_'+path.stem,[],606,stock={'path':relative,'sha256':hashlib.sha256(data).hexdigest(),'offset':off,'bytes':n,'module':module},raw=data[off:off+n]))
            pos=off+n
    return rows

def main():
    cases=[('empty',[]),('all_kinds',[string(b''),scalar(1,0x80000001),scalar(2,0x3fa00000),scalar(3,2,0xaabbccdd),scalar(4,0),scalar(5,4,5),scalar(7,8,9)]),
      ('float_bits',[scalar(2,n) for n in [0,0x80000000,1,0x007fffff,0x00800000,0x7f7fffff,0x7f800000,0xff800000,0x7fc12345]]),
      ('signaling_nan_boundary',[scalar(2,0x7f812345),scalar(2,0xff812345)]),
      ('raw_strings',[string(s) for s in [b'',b'raw\xff\xfe',b'a\0tail',b'Ljava.render.Osd;',b'[I']]+[u(0)+u(1)+b'AX']),
      ('growth',[scalar(1,n) for n in range(70)])]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':3,'scope':__doc__,'fpu_cw':CW,'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'rows':[row(k,v,seed,unresolved=k=='signaling_nan_boundary') for k,v in cases for seed in [606,20261023]]+stock_rows()},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
