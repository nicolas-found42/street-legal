#!/usr/bin/env python3
"""Original 49b300 root entity construction; only allocator and fresh world scenario controlled."""
import json, hashlib, struct
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9
WORLD=0x6409c4
def bits(x): return struct.unpack('<I',struct.pack('<f',x))[0]
def run():
 rows=[]
 for mass in [0.,1.,100.,-2.]:
  for position in [[0.,0.,0.],[1.25,-2.5,10.]]:
   o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
   o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
   body=o.call(0x49ab60,[bits(mass),5,bits(2.),bits(3.),bits(4.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   p=o.alloc(16);o.write_f32s(p,position)
   entity=o.call(0x49b300,[body,p,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   rows.append(dict(mass=bits(mass),position=list(map(bits,position)),body=body,entity=entity,entity_words=o.read_u32s(entity,0x218//4),body_words=o.read_u32s(body,40),world_words=o.read_u32s(WORLD+0x50,12)))
 children=[]
 for mass in [0.,1.,100.]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
  o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
  parent=o.alloc(0x218);b2=o.alloc(0xa0);p=o.alloc(16)
  o.write_f32s(parent+0x64,[1.,2.,3.])
  for off in [0xa4,0xb8,0xcc]:o.write_u32(parent+off,bits(1.))
  o.write_u32(parent+0x104,bits(1.))
  o.write_u32(b2+0x14,bits(mass));o.write_u32(b2+0x9c,1 if mass else 5)
  for off in [0x18,0x2c,0x40]:o.write_u32(b2+off,bits(mass))
  o.write_f32s(p,[3.,6.,9.])
  child=o.call(0x49b230,[parent,b2,p,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  children.append(dict(mass=bits(mass),parent=parent,child=child,body=b2,parent_words=o.read_u32s(parent,0x218//4),entity_words=o.read_u32s(child,0x218//4),relative_words=o.read_u32s(o.read_u32(child+0x1c),20),body_words=o.read_u32s(b2,40),world_words=o.read_u32s(WORLD+0x50,12)))
 rotations=[]
 for matrix in [[1.,0.,0.,0.,1.,0.,0.,0.,1.],[.9877,0.,-.15635,0.,1.,0.,.15635,0.,.9877],[-1.,0.,0.,0.,-1.,0.,0.,0.,1.],[1.,.2,.3,.4,1.5,.6,.7,.8,1.2]]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
  b=o.alloc(0xa0);p=o.alloc(16);r=o.alloc(48);o.write_u32(b+0x14,bits(.5));o.write_u32(b+0x9c,1);o.write_f32s(p,[1.25,-2.5,10.])
  inertia=[1.,.1,.2,.1,2.,.3,.2,.3,3.]
  for i,v in enumerate(matrix):o.write_u32(r+(i//3)*16+(i%3)*4,bits(v))
  for i,v in enumerate(inertia):o.write_u32(b+0x18+(i//3)*16+(i%3)*4,bits(v))
  entity=o.call(0x49b300,[b,p,r],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  rotations.append(dict(matrix=list(map(bits,matrix)),inertia=list(map(bits,inertia)),body=b,entity=entity,entity_words=o.read_u32s(entity,0x218//4),body_words=o.read_u32s(b,40),world_words=o.read_u32s(WORLD+0x50,12)))
 rotated_children=[]
 for matrix in [[.9877,0.,-.15635,0.,1.,0.,.15635,0.,.9877],[1.,.2,.3,.4,1.5,.6,.7,.8,1.2]]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
  b=o.alloc(0xa0);p=o.alloc(16);r=o.alloc(48)
  o.write_u32(b+0x14,bits(.5));o.write_u32(b+0x9c,1);o.write_f32s(p,[1.,2.,3.])
  inertia=[1.,.1,.2,.1,2.,.3,.2,.3,3.]
  for i,v in enumerate(matrix):o.write_u32(r+(i//3)*16+(i%3)*4,bits(v))
  for i,v in enumerate(inertia):o.write_u32(b+0x18+(i//3)*16+(i%3)*4,bits(v))
  parent=o.call(0x49b300,[b,p,r],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  b2=o.alloc(0xa0);o.write_u32(b2+0x14,bits(.25));o.write_u32(b2+0x9c,1)
  for i,v in enumerate(inertia):o.write_u32(b2+0x18+(i//3)*16+(i%3)*4,bits(v))
  o.write_f32s(p,[4.,6.,9.]);o.write_u32(WORLD+0x84,0x640000)
  child=o.call(0x49b230,[parent,b2,p,r],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  rotated_children.append(dict(matrix=list(map(bits,matrix)),inertia=list(map(bits,inertia)),parent=parent,entity=child,entity_words=o.read_u32s(child,0x218//4),parent_words=o.read_u32s(parent,0x218//4),relative_words=o.read_u32s(o.read_u32(child+0x1c),20),body_words=o.read_u32s(b2,40),world_words=o.read_u32s(WORLD+0x50,12)))
 return dict(rotated_children=rotated_children,rotations=rotations,children=children,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
