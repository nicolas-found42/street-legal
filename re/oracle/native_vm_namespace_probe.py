#!/usr/bin/env python3
"""Original41cc00(path0),54d1c0,54d240 with complete bounded machine state.

Constructor/name-copy and raw CRT record consumption/close execute real code.
Only54f560 malloc and54f5b0 free are controlled. Raw slot tables and records are
declared inputs, independent of semantic FindHost. This is not full namespace,
find-first, bootstrap, caller-above-helper, EFLAGS or full native ABI acceptance.
"""
from pathlib import Path
import argparse, hashlib, itertools, json, random, sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle, HEAP_BASE, STACK_TOP, STACK_SIZE, CW_D3D9
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, __version__ as version
ROOT = Path(__file__).resolve().parents[2]
REGS = ['eax','ebx','ecx','edx','esi','edi','ebp','esp']
HEAP_BYTES = 0x2000
STACK_LOW = STACK_TOP - 0x500
TABLES = [0x7663d8, 0x7685e0, 0x768660]

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def inventory():
    return ([dict(kind='construct', fill=fill, parent=parent, name=name)
             for fill,parent,name in itertools.product([0,0xa5],[False,True],['','java','n'*255])]
        + [dict(kind='next', fill=fill, handle=handle, position=position)
           for fill,handle,position in itertools.product([0,0xa5],[0,31],['missing','first','last','exhausted'])]
        + [dict(kind='close', fill=fill, handle=handle, occupied=occupied)
           for fill in [0,0xa5] for handle,occupied in [(0,False),(0,True),(31,False),(31,True),(0xffffffff,False),(32,False)]])

def capture(spec):
    o = Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'))
    o.write(HEAP_BASE, bytes([spec['fill']])*HEAP_BYTES)
    o.write(STACK_TOP-STACK_SIZE, bytes([spec['fill']])*STACK_SIZE)
    o.heap_ptr = HEAP_BASE + (48 if spec.get('parent') or spec.get('handle') == 31 else 0)
    for table in TABLES: o.write(table, bytes([spec['fill']])*128)
    args = []
    this = None
    if spec['kind'] == 'construct':
        parent = o.alloc(16) if spec['parent'] else 0
        this = o.alloc(16)
        raw = spec['name'].encode(); name = o.alloc(len(raw)+1); o.write(name, raw+b'\0')
        args = [parent,0,name]; target=0x41cc00; convention='thiscall'
    elif spec['kind'] == 'next':
        handle = spec['handle']; records = o.alloc(520)
        for ordinal,text in enumerate([b'',b'n'*255]):
            o.write(records+ordinal*260,text+b'\0');o.write_u32(records+ordinal*260+256,0x12345678+ordinal)
        missing = spec['position'] == 'missing'
        index = {'missing':0,'first':0,'last':1,'exhausted':2}[spec['position']]
        for table,value in zip(TABLES,[0 if missing else records,index,2]): o.write_u32(table+handle*4,value)
        destination=o.alloc(256);directory=o.alloc(4);args=[handle,destination,directory]
        target=0x54d1c0;convention='cdecl'
    else:
        handle = spec['handle']
        if handle < 32:
            records = o.alloc(260) if spec['occupied'] else 0
            for table,value in zip(TABLES,[records,3,7]): o.write_u32(table+handle*4,value)
        args=[handle];target=0x54d240;convention='cdecl'
    seed=606 if spec['fill']==0 else 20261023
    rng=random.Random(seed);registers={r:rng.getrandbits(32) for r in REGS[:-1]}
    if this is not None:registers['ecx']=this
    events=[];stores=[];started=False
    def state():return dict(heap=list(o.read(HEAP_BASE,HEAP_BYTES)),heap_ptr=o.heap_ptr,
        stack=list(o.read(STACK_LOW,0x500)),registers={r:o.reg(r) for r in REGS},
        tables={str(table):o.read_u32s(table,32) for table in TABLES})
    def allocate(oc,values):
        events.append(dict(kind='allocate',args=values,state=state()))
        result=oc.alloc(values[0]);assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES;return result
    def free(oc,values):events.append(dict(kind='free',args=values,state=state()));return 0
    o.hook(0x54f560,allocate,'cdecl',1);o.hook(0x54f5b0,free,'cdecl',1)
    def code(uc,pc,size,unused):
        nonlocal started
        if pc==target and not started:started=True;o.set_reg('ebp',registers['ebp'])
    def write(uc,access,address,size,value,unused):
        if not started:return
        if HEAP_BASE<=address and address+size<=HEAP_BASE+HEAP_BYTES or any(t<=address and address+size<=t+128 for t in TABLES):
            stores.append([address,size,value]);return
        if STACK_LOW<=address and address+size<=STACK_TOP:return
        raise RuntimeError(f'out-of-profile write {address:x}/{size} at{o.reg("eip"):x}')
    o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
    before=state();result=o.call(target,args,convention=convention,this=this,regs=registers,fpu_cw=CW_D3D9)
    assert not o.import_log and result.fpcw==CW_D3D9 and result.fpu_top_depth==0
    return dict(input=spec,target=target,args=args,this=this,register_inputs=registers,
        before=before,events=events,after=state(),stores=stores,result=result.eax,esp_delta=result.esp_delta)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-namespace-original.json');args=parser.parse_args()
    sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
    value=dict(schema=1,profile=__doc__,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),
        producer_sources={p.name:digest(p) for p in sources},unicorn=version,cw=CW_D3D9,rows=[capture(spec) for spec in inventory()])
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(value,separators=(',',':'))+'\n')
    print(json.dumps(dict(output=str(args.output),rows=len(value['rows']),sha256=digest(args.output))))
if __name__=='__main__':main()
