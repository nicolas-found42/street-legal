#!/usr/bin/env python3
"""Original4036b0 variable owner constructor and4038c0 raw-name insertion.

Entries are actual4050b0 pool allocations initialized by408d60; source caller
sets their variable vtable/name before the measured owner operations. No class
initializer/object/native-result call is stubbed. Allocator/log/clock/error
collaborators use Scenario's declared profile. Complete heaps, all CPU heap
stores and fullEAX results retained for owner/vector operations. Separate
403e80 already-initialized rows compare full heap, retained stack and eightGPRs
after running complete originalfirstinitialization. The firstinitialization
port and other owner/vector caller stack/registers remain unclaimed.
"""
from pathlib import Path
import hashlib,json,sys,struct,random
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW,TC
from oracle import DEFAULT_EXE,HEAP_BASE,STACK_TOP,STACK_SIZE
from unicorn import UC_HOOK_MEM_WRITE,UC_HOOK_CODE

def row(label,names,seed):
    s=Scenario();o=s.o;holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
    o.alloc(seed%41+1);owner=o.alloc(20);o.write(owner,bytes([seed&255])*20);inputs=[]
    for name in names:
        raw=name.encode();pointer=o.alloc(len(raw)+1);o.write(pointer,raw+b'\0')
        variable=o.call(0x4050b0,[o.read_u32(0x62f134)],convention='fastcall',fpu_cw=CW).eax
        o.call(0x408d60,[s.jvm,TC['I']],convention='thiscall',this=variable,fpu_cw=CW)
        o.write_u32(variable,0x5e7344);o.write_u32(variable+12,pointer)
        inputs.append({'variable':variable,'name':name,'name_pointer':pointer})
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();stores=[]
    def write(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            assert address+size<=o.heap_ptr
            stores.append([address,size,value&((1<<(8*size))-1)])
        elif not (STACK_TOP-STACK_SIZE<=address and address+size<=STACK_TOP):
            raise RuntimeError(f'undeclared original write {address:x}/{size}')
    o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
    ctor=o.call(0x4036b0,[s.jvm],convention='thiscall',this=owner,fpu_cw=CW).eax
    for item in inputs:item['result']=o.call(0x4038c0,[item['variable']],convention='thiscall',this=owner,fpu_cw=CW).eax
    return {'label':label,'seed':seed,'owner':owner,'jvm':s.jvm,'inputs':inputs,'constructor_result':ctor,'before':before,'after':heap(),'stores':stores}
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
def reentry_row(seed,fill):
    s=Scenario();o=s.o
    for addr in [0x406f60,0x406140,0x551140]:o.hooks.pop(addr,None)
    holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
    o.write_u32(s.jvm+8,o.alloc(0x1828))
    u=lambda n:struct.pack('<I',n)
    ch=lambda t,b:t+u(len(b))+b
    entries=[u(0)+u(5)+b'Probe'+b'\0',u(4)+u(0)]
    raw=ch(b'TUFA',u(0x11ae1))+ch(b'CONS',u(2)+b''.join(entries))+ch(b'CLSS',u(0)+u(0)+u(1)+u(0xffffffff)+u(0))+ch(b'TREE',u(0))
    source=o.alloc(len(raw));o.write(source,raw)
    cls=o.call(0x411b20,[source,len(raw)],convention='thiscall',this=s.jvm,fpu_cw=CW).eax
    # Establish the sentinel through unmodified complete original403e80.
    o.call(0x403e80,[],convention='thiscall',this=cls,fpu_cw=CW)
    owner=o.read_u32(cls+0x1d0);assert owner and o.read_u32(owner)==0x5e7338
    stack_base=STACK_TOP-0x400;o.write(stack_base,bytes([fill])*0x400)
    rng=random.Random(seed);registers={r:rng.getrandbits(32)for r in REGS[:-1]};registers['ecx']=cls;entry=None
    def state():return {'heap_ptr':o.heap_ptr,'heap':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE)),'stack':list(o.read(stack_base,0x400)),'registers':{r:o.reg(r)for r in REGS}}
    def code(uc,pc,size,_):
        nonlocal entry
        if pc==0x403e80:entry=o.reg('esp');o.set_reg('ebp',registers['ebp'])
    def write(uc,access,address,size,value,_):
        assert stack_base<=address and address+size<=STACK_TOP,(hex(address),size)
    before=state();o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
    # First original initialization translated this entry before passive hooks.
    o.uc.ctl_remove_cache(0x403e80,0x404391)
    o.call(0x403e80,[],convention='thiscall',this=cls,regs=registers,fpu_cw=CW)
    assert entry is not None
    return {'seed':seed,'fill':fill,'class':cls,'owner':owner,'entry_esp':entry,'register_inputs':registers,'stack_base':stack_base,'before':before,'after':state()}

def main():
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    cases=[('empty',[]),('content_duplicates',['same','other','same','', '','other','prefix','prefix.x','prefix']),('case_and_odd_even',['a','A','ab','abc','abcd','ab','abc','A']),('growth_full_eax',[f'field{i:03}' for i in range(258)]+['field000','field257'])]
    print(json.dumps({'schema':2,'scope':__doc__,'fpu_cw':CW,'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},'rows':[row(k,v,seed)for k,v in cases for seed in [606,20261023]],'reentries':[reentry_row(seed,fill)for seed in [606,20261023]for fill in [0,165]]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
