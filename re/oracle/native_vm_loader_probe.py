#!/usr/bin/env python3
"""Original411b20 complete file-to-physical-module memory comparison.

Exact stock TUFA bytes (or declared synthetic serialization) are source inputs.
Original CONS/member/node readers, class constructor, descriptor interning and
class cache code execute unchanged. Scenario supplies malloc/free/realloc,
clock/log/fatal-position only; no VM, class lookup or native-result hooks.
Complete heap and all chronological CPU heap stores are retained. Source caller
stack/register history is explicitly outside this memory projection.
"""
from pathlib import Path
import hashlib,json,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE

def u(n):return struct.pack('<I',n)
def chunk(tag,b):return tag+u(len(b))+b
def string(b):return u(0)+u(len(b))+b+b'\0'
def synthetic(opwords):
    entries=[string(b'Probe'),u(4)+u(0),string(b'I'),string(b'field'),string(b'method'),string(b'()I')]
    fields=u(70)+b''.join(u(0)+u(3)+u(2)+u(0xffffffff) for _ in range(70))+u(1)+u(0)+u(3)+u(2)+u(0xffffffff)
    methods=u(70)+b''.join(u(0x200)+u(4)+u(5)+u(0xffffffff)+u(0) for _ in range(70))+u(1)+u(0x200)+u(4)+u(5)+u(0xffffffff)+u(0)
    nodes=[]
    for i,op in enumerate(opwords):
        b=bytes([i])+struct.pack('<H',(i*2003)&65535)
        if op&0xf000 in [0x1000,0x2000,0x4000]:b+=u(0xa5a50000|i)
        nodes.append(b)
    return b''.join([chunk(b'TUFA',u(0x11ae1)),chunk(b'CONS',u(len(entries))+b''.join(entries)),chunk(b'FILD',fields),chunk(b'MTHD',methods),chunk(b'CLSS',u(0)+u(0)+u(1)+u(0xffffffff)+u(1)+u(1)),chunk(b'TREE',u(2)+u(0)+u(len(nodes))+b''.join(nodes))])

def row(label,data,seed,stock):
    s=Scenario();o=s.o
    for addr in [0x406f60,0x406140,0x551140]:o.hooks.pop(addr,None)
    holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
    cache=o.alloc(0x1828);o.write_u32(s.jvm+8,cache)
    o.alloc(seed%41+1)
    source=o.alloc(len(data));o.write(source,data)
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();stores=[];modules=[]
    def observe(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            assert address+size<=o.heap_ptr
            stores.append({'address':address,'size':size,'value':value&((1<<(size*8))-1)})
    def code(uc,address,size,_):
        if address==0x403a90:
            esp=o.reg('esp');assert o.read_u32(esp+4)==s.jvm
            modules.append({'class':o.reg('ecx'),'module':o.read_u32(esp+8)})
        if address==0x423e10:modules[-1]['descriptor_scratch']=o.read_u32(o.reg('esp')+8)
    o.uc.hook_add(UC_HOOK_MEM_WRITE,observe);o.uc.hook_add(UC_HOOK_CODE,code)
    r=o.call(0x411b20,[source,len(data)],convention='thiscall',this=s.jvm,fpu_cw=CW)
    assert r.eax==modules[0]['class'];assert len({x['descriptor_scratch'] for x in modules})==1
    return {'label':label,'seed':seed,'jvm':s.jvm,'source':source,'source_bytes':len(data),'stock':stock,'modules':modules,'result':r.eax,'before':before,'after':heap(),'stores':stores}

def main():
    root=Path(__file__).resolve().parents[2];stock=[]
    for relative in ['system/scripts/io/Input.class','system/scripts/render/Osd.class','system/scripts/lang/String.class']:
        data=(root/'stock'/relative).read_bytes();stock.append((Path(relative).stem,data,[{'path':relative,'sha256':hashlib.sha256(data).hexdigest(),'offset':0,'bytes':len(data)}]))
    s=Scenario();opwords=list(struct.unpack('<47H',s.o.read(0x5f0914,94)))
    stock.append(('concatenated_Input_Osd',stock[0][1]+stock[1][1],[dict(x,offset=offset) for case,offset in [(stock[0],0),(stock[1],len(stock[0][1]))] for x in case[2]]))
    stock.append(('synthetic_member_growth_all_nodes',synthetic(opwords),[]))
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':1,'scope':__doc__,'fpu_cw':CW,'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'opwords':opwords,
      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'rows':[row(label,data,seed,files) for label,data,files in stock for seed in [606,20261023]]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
