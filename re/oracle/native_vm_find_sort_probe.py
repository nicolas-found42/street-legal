#!/usr/bin/env python3
"""Real5d765d/5d77b1/5d77ff with54d1a0/554a10 comparator, no hooks.

Authored raw260-byte find records include untouched fill and ordinal flags.
Entire0x8000heap and0x1000stack plus8GPRs and raw ordered CPU heap writes are
retained. EFLAGS/x87 raw status/caller-above-qsort remain unclaimed.
"""
from pathlib import Path
import argparse,hashlib,json,random,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,__version__ as version
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def sequences():return [[],['z'],['b','A'],list('hgfedcba'),['a','B','a','b','A','C','c','b','A'],
    [f'n{i:02}' for i in range(17)],[f'n{i:02}' for i in reversed(range(51))]]
def inventory():return [dict(fill=f,names=names) for f in [0,0xa5] for names in sequences()]
def capture(spec):
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*0x8000);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 o.heap_ptr=HEAP_BASE+(48 if spec['fill'] else 0);base=o.alloc(max(1,len(spec['names'])*260))
 for i,name in enumerate(spec['names']):o.write(base+i*260,name.encode()+b'\0');o.write_u32(base+i*260+256,0x10000000+i)
 rng=random.Random(20261023 if spec['fill'] else 606);regs={r:rng.getrandbits(32) for r in REGS[:-1]};stores=[];started=False
 def state():return dict(heap=list(o.read(HEAP_BASE,0x8000)),heap_ptr=o.heap_ptr,stack=list(o.read(STACK_TOP-0x1000,0x1000)),registers={r:o.reg(r) for r in REGS})
 def code(uc,pc,size,_):
  nonlocal started
  if pc==0x5d765d and not started:started=True;o.set_reg('ebp',regs['ebp'])
 def write(uc,access,address,size,value,_):
  if not started:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+0x8000:stores.append([address,size,value]);return
  if STACK_TOP-0x1000<=address and address+size<=STACK_TOP:return
  raise RuntimeError(f'out-of-profile store {address:x}/{size} at{o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
 before=state();result=o.call(0x5d765d,[base,len(spec['names']),260,0x54d1a0],regs=regs,fpu_cw=CW_D3D9)
 assert not o.import_log and result.fpcw==CW_D3D9 and result.fpu_top_depth==0
 return dict(input=spec,base=base,register_inputs=regs,before=before,after=state(),stores=stores,result=result.eax,esp_delta=result.esp_delta)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-find-sort-original.json');args=p.parse_args()
 sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 value=dict(schema=1,profile=__doc__,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:digest(p) for p in sources},unicorn=version,cw=CW_D3D9,rows=[capture(spec) for spec in inventory()])
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(value,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(args.output),rows=len(value['rows']),sha256=digest(args.output))))
if __name__=='__main__':main()
