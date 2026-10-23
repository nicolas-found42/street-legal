#!/usr/bin/env python3
"""Original46e930 LOD selection slice46ea10..46ea90.

Runs the original branch slice with EBX=template, EBP=constructed body,
EDI=0. Only419860 is a declared resource-access callback;46ea90 terminates
before render construction. Compares every retained input/payload word.
"""
import hashlib,itertools,json
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EBP
from oracle import Oracle,DEFAULT_EXE,CW_D3D9


def run():
 rows=[]
 for count,own,loaded,distance in itertools.product([-1,0,1,3],[False,True],[False,True],
        [0,0x3f000000,0x3f800000,0x40000000,0x40c00000,0x7fc01234,0x7f801234,0x80000000]):
  o=Oracle();k=o.alloc(0x126c);body=o.alloc(0x11c);resource=o.alloc(0x160);payload=o.alloc(0x80)
  # Poisoned Ref-shaped caller storage is deliberately a different distance.
  o.write_u32(k+0xfac,count&0xffffffff);o.write_u32s(k+0xfb0,[0x3f000000,0x3f800000,0x40000000,0x40800000])
  o.write_u32(body+12,resource if own else 0);o.write_u32(resource+0x70,distance)
  o.write_u32s(payload+0x74,[0xaabbccdd,0x11223344])
  before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4);events=[]
  def begin(uc,a,size,user):uc.reg_write(UC_X86_REG_EBP,body)
  o.uc.hook_add(UC_HOOK_CODE,begin,begin=0x46ea10,end=0x46ea10)
  def access(oc,args):
   events.append(dict(resource=oc.reg('ecx'),args=list(args)));return payload if loaded else 0
  o.hook(0x419860,access,'thiscall',4)
  o.hook(0x46ea90,lambda oc,args:oc.reg('esi'),'cdecl',0)
  r=o.call(0x46ea10,regs={'ebx':k,'edi':0},fpu_cw=CW_D3D9)
  rows.append(dict(count=count,own=own,loaded=loaded,distance=distance,template=k,body=body,
       resource=resource,payload=payload,before=before,after=o.read_u32s(0x20000000,len(before)),events=events,result=r.eax))
 return dict(schema=1,oracle='unicorn-original-instruction-slice',fpu_cw=CW_D3D9,
   exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
   producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':
 out=Path('re/out/boot/native-part-lod-original.json');out.parent.mkdir(parents=True,exist_ok=True)
 out.write_text(json.dumps(run(),sort_keys=True));print(out)
