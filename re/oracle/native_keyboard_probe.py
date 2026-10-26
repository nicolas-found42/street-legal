#!/usr/bin/env python3
"""Original47cc10 and556e00 with explicit DirectInput/Win32 leaf inputs.

All wrapper/poll instructions run. Only Acquire/GetDeviceData and Win32 key
translation leaves are controlled. Complete heap, retained stack, eight GPRs,
ring bytes and global words are compared at every leaf and return; chronological
ring/global writes retained. This does not observe a physical keyboard or prove
the VM/native caller ABI. No input/native-result fixtures enter runtime code.
"""
from pathlib import Path
import argparse,hashlib,itertools,json,random,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,__version__ as version
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
GLOBALS=[0x76f9b0,0x777430,0x612c68,0x5e72b0,0x5e72b4,0x5e72b8]
RING=0x640924
STACK_LOW=STACK_TOP-0x500
CASES=[dict(label='acquire-failed',acquire=0x8007001e),dict(label='data-failed',data_result=0x8007001e),dict(label='empty',count=0),dict(label='two-events',count=2),dict(label='scan-too-high',scan=256),dict(label='key-up',data=0),dict(label='state-failed',state_result=0),dict(label='ascii-zero',ascii=0),dict(label='ascii-one',ascii=65),dict(label='ascii-two',ascii=0x4241),dict(label='dead-key',ascii=0xb4,ascii_result=0xffffffff),dict(label='scan-zero',scan=0,ascii=81),dict(label='scan-last',scan=255,ascii=127),dict(label='ascii-signed',ascii=0xff80),dict(label='ascii-high-only',ascii=0x100),dict(label='buffer-overflow',data_result=1),dict(label='quiescent-count-only',count=0,data_writes=False),dict(label='data-failed-untouched',data_result=0x8007001e,data_writes=False,data_count_write=False),dict(label='state-failed-untouched',state_result=0,state_writes=False)]
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def inventory():return [dict(case=i,fill=f,ring_offset=r) for i,f,r in itertools.product(range(len(CASES)),[0,0xa5],[0,15])]
def capture(spec):
 case=dict(acquire=0,data_result=0,count=1,scan=30,data=128,state_result=1,ascii=65,ascii_result=1,vk=0x1234,data_writes=True,data_count_write=True,state_writes=True);case.update(CASES[spec['case']])
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*0x100);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 device=o.alloc(16);table=o.alloc(64);info=o.alloc(16);o.write_u32(device,table);o.write_u32(table+0x1c,0x41b860);o.write_u32(table+0x28,0x41b870)
 o.write_u32(0x76f9b0,device);o.write_u32(0x777430,0x4090409);o.write_u32(0x612c68,RING+spec['ring_offset']);o.write(RING,bytes(range(16)))
 rng=random.Random(606+spec['case']);regs={r:rng.getrandbits(32) for r in REGS[:-1]};events=[];stores=[];started=False;entry_esp=None
 keyboard=bytes((i*17+3)&255 for i in range(256))
 def state():return dict(heap=list(o.read(HEAP_BASE,0x100)),heap_ptr=o.heap_ptr,stack=list(o.read(STACK_LOW,0x500)),registers={r:o.reg(r) for r in REGS},globals={str(p):o.read_u32(p) for p in GLOBALS},ring=list(o.read(RING,16)))
 def leaf(kind):
  def call(oc,args):
   events.append(dict(kind=kind,args=args,state=state()))
   if kind=='acquire':return case['acquire']
   if kind=='data':
    assert args[0]==device and args[1]==20 and args[4]==0 and oc.read_u32(args[3])==1
    if case['data_count_write']:oc.write_u32(args[3],case['count'])
    if case['data_writes']:oc.write_u32s(args[2],[case['scan'],case['data'],0x12345678,0x23456789,0x3456789a])
    return case['data_result']
   if kind=='map':assert args==[case['scan'],1,0x4090409];return case['vk']
   if kind=='state':
    if case['state_writes']:oc.write(args[0],keyboard)
    return case['state_result']
   if kind=='ascii':
    assert args[0:2]==[case['vk'],case['scan']] and args[4:]==[0,0x4090409]
    assert oc.read(args[2],256)==keyboard;oc.write(args[3],case['ascii'].to_bytes(2,'little'));return case['ascii_result']
   raise RuntimeError(kind)
  return call
 o.hook(0x41b860,leaf('acquire'),'stdcall',1);o.hook(0x41b870,leaf('data'),'stdcall',5)
 for iat,kind,n in [(0x5e72b0,'map',3),(0x5e72b4,'state',1),(0x5e72b8,'ascii',6)]:o.hook(o.read_u32(iat),leaf(kind),'stdcall',n)
 def code(uc,pc,size,unused):
  nonlocal started,entry_esp
  if pc==0x47cc10:started=True;entry_esp=o.reg('esp');o.set_reg('ebp',regs['ebp'])
 def write(uc,access,address,size,value,unused):
  if not started:return
  if STACK_LOW<=address and address+size<=STACK_TOP:return
  if RING<=address and address+size<=RING+16 or address==0x612c68 and size==4:stores.append([address,size,value]);return
  raise RuntimeError(f'undeclared write {address:x}/{size} at {o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write);before=state()
 result=o.call(0x47cc10,[info],convention='cdecl',regs=regs,fpu_cw=CW_D3D9)
 assert not o.import_log and result.fpcw==CW_D3D9 and result.fpu_top_depth==0
 return dict(input=spec,case=case,info=info,register_inputs=regs,entry_esp=entry_esp,before=before,events=events,after=state(),stores=stores,result=result.eax)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);a=p.parse_args();assert not a.output.exists(),'preserve prior output'
 sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 result=dict(schema=1,entry=0x47cc10,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:digest(p) for p in sources},cw=CW_D3D9,unicorn=version,profile=__doc__,rows=[capture(s) for s in inventory()])
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(result,separators=(',',':'))+'\n');print(json.dumps(dict(rows=len(result['rows']),sha256=digest(a.output))))
if __name__=='__main__':main()
