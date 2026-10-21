#!/usr/bin/env python3
"""Original spring45e5f0/490dc0 with real body/entity/link collaborators.

Declared fresh world body/spring list sentinels and bump allocator; all spring,
physics shape, child entity, weak-reference, resource-link and list code runs
original x86. The car handle points to a supplied resource container.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
WORLD=0x6409c4
def bits(f):
 import struct
 return struct.unpack('<I',struct.pack('<f',f))[0]
def run(lengths,position):
 o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
 for a,v in [(0x188,WORLD+0x190),(0x198,WORLD+0x184),(0x9c,WORLD+0xa4),(0xac,WORLD+0x98)]:o.write_u32(WORLD+a,v)
 zero=0x30001000;point=zero+16;direction=point+16;o.write(zero,bytes(12));o.write_u32s(point,[bits(v) for v in position]);o.write_u32s(direction,[0,bits(-1.),0])
 parent_body=o.call(0x49ab60,[0,0,0,0,0,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
 parent=o.call(0x49b300,[parent_body,zero,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
 handle=o.alloc(0x10);resource=o.alloc(0x60);o.write_u32(handle+12,resource);o.write_u32(resource+0x50,0x12345678)
 springs=[]
 for length in lengths:
  spring=o.alloc(0xec);o.call(0x45e5f0,[spring],convention='fastcall',fpu_cw=CW_D3D9)
  for offset in [0x7c,0x80,0x94,0x98,0xb8,0xbc]:o.write_u32(spring+offset,0)
  o.write_u32(spring,0x5f0fcc)
  result=o.call(0x490dc0,[handle,parent,point,direction,bits(length),bits(0.17)],convention='thiscall',this=spring,fpu_cw=CW_D3D9).eax
  assert result==1
  springs.append(spring)
 return dict(lengths=[bits(v) for v in lengths],position=[bits(v) for v in position],parent=parent,parent_body=parent_body,car_handle=handle,resource=resource,springs=springs,heap=o.heap_ptr,world=o.read_u32s(WORLD,0x210//4),words=[[p,o.read_u32(p)] for p in range(HEAP_BASE,o.heap_ptr,4) if o.read_u32(p)])
def configuration_rows():
 rows=[]
 for values in [[1250.,220.,1.05,1.2,.55,170.,.32],[17.,29.,-.5,0.,-1.,-2.,0.],[5.,7.,.4,-.2,0.,0.,-.1]]:
  o=Oracle(fpu_cw=CW_D3D9);spring=o.alloc(0xec);o.write_u32s(spring,[0x12340000+i for i in range(0xec//4)]);args=[bits(v) for v in values]
  o.call(0x491f80,args,convention='thiscall',this=spring,fpu_cw=CW_D3D9)
  rows.append(dict(args=args,words=o.read_u32s(spring,0xec//4)))
 return rows
def main():
 digest=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest();assert digest=='7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
 print(json.dumps(dict(exe_sha256=digest,scope=__doc__,configure=configuration_rows(),rows=[run([length],[-0.75,-0.12,1.4]) for length in [0.,0.9,1.2,-0.5]]+[run([0.9,1.2],[0.3,0.2,-0.7])]),sort_keys=True))
if __name__=='__main__':main()
