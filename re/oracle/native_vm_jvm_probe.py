#!/usr/bin/env python3
"""Real40fb50 plus419200/41ab50/41a370/41a450/41e4d0/408f20/40f860.

Only54f560 allocator and54be80 logger are controlled collaborators. Entire heap
arena and retained stack, sevenGPRs/ESP and chronological heap/global writes are
recorded. All other reached imports and out-of-profile writes are rejected.
Ordinary successful allocations only; this is not boot or native ABI acceptance.
"""
from pathlib import Path
import argparse,hashlib,json,sys,itertools,random
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,__version__ as version
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
HEAP_BYTES=0xc000
STACK_LOW=STACK_TOP-0x500
GLOBAL_WORDS=[0x62ddf0,0x62f130,0x62f134,0x62de04,0x62de08]
GLOBAL_BYTES=[0x62dea2,0x62deb4,0x62deae,0x62dea6,0x62deb2,0x62de70,0x62de62,0x62de67,0x62de9c]

def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def inventory():return [dict(fill=f,seed=s,gap=g) for f,s,g in itertools.product([0,0xa5],[606,20261023],[0,48])]
def capture(spec):
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*HEAP_BYTES);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 o.heap_ptr=HEAP_BASE+spec['gap'];this=o.alloc(0x80);base=o.heap_ptr;events=[];stores=[]
 for p in GLOBAL_WORDS:o.write_u32(p,0xa5a5a5a5^spec['seed'])
 for p in GLOBAL_BYTES:o.write(p,bytes([spec['fill']]))
 rng=random.Random(spec['seed']);regs={r:rng.getrandbits(32) for r in REGS[:-1]};regs['ecx']=this
 def state():return dict(heap=list(o.read(HEAP_BASE,HEAP_BYTES)),heap_ptr=o.heap_ptr,stack=list(o.read(STACK_LOW,0x500)),registers={r:o.reg(r) for r in REGS},globals={str(p):o.read_u32(p) for p in GLOBAL_WORDS},global_bytes={str(p):o.read_u8(p) for p in GLOBAL_BYTES})
 def alloc(oc,args):
  events.append(dict(kind='allocate',bytes=args[0],state=state()));p=oc.alloc(args[0]);assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES;return p
 logs=[]
 def logger(oc,args):
  events.append(dict(kind='logger',this=oc.reg('ecx'),message=args[0],state=state()));logs.append(args[0]);return 0xaabb0000+len(logs)
 o.hook(0x54f560,alloc,'cdecl',1);o.hook(0x54be80,logger,'thiscall',1)
 started=False
 def code(uc,pc,size,unused):
  nonlocal started
  if pc==0x40fb50:
   started=True;o.set_reg('ebp',regs['ebp'])
 def write(uc,access,address,size,value,unused):
  if not started:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+HEAP_BYTES:stores.append([address,size,value]);return
  if STACK_LOW<=address and address+size<=STACK_TOP:return
  if address in GLOBAL_WORDS and size==4 or address in GLOBAL_BYTES and size==1:stores.append([address,size,value]);return
  raise RuntimeError(f'out-of-profile store {address:x}/{size} at {o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
 before=state();r=o.call(0x40fb50,[],convention='thiscall',this=this,regs=regs,fpu_cw=CW_D3D9)
 assert not o.import_log and r.fpcw==CW_D3D9 and r.fpu_top_depth==0
 return dict(input=spec,this=this,register_inputs=regs,before=before,events=events,after=state(),stores=stores,return_value=r.eax,esp_delta=r.esp_delta)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-jvm-original.json');a=p.parse_args()
 sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 out=dict(schema=1,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:digest(p) for p in sources},unicorn=version,cw=CW_D3D9,profile=__doc__,rows=[capture(s) for s in inventory()])
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(out['rows']),sha256=digest(a.output))))
if __name__=='__main__':main()
