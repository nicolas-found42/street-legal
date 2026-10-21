#!/usr/bin/env python3
"""Original49a760/49a810 reparenting of fresh roots, including49a840.

Allocator and empty world are declared boundaries. Constructors and relative
basis conversion execute original code. Existing relative-record destruction is
outside this test, and remains an explicit port boundary.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
WORLD=0x6409c4
def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def run():
 rows=[]
 for old_parent in [False,True]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD)
  o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
  parent_list=o.alloc(0x218)
  o.write_u32(WORLD+0x84,parent_list if old_parent else 0)
  entities=[]
  position=o.alloc(16)
  for mass,point in [(100.,[2.,3.,4.]),(1.,[.1,.2,.3])]:
   body=o.call(0x49ab60,[bits(mass),5,bits(2.),bits(3.),bits(4.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   o.write_f32s(position,point)
   entities.append(o.call(0x49b300,[body,position,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax)
  entity,parent=entities
  before=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4);start=o.heap_ptr
  o.call(0x49a760,[entity],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
  o.call(0x49a810,[entity,parent],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
  rows.append(dict(old_parent=old_parent,entity=entity,parent=parent,start=start,end=o.heap_ptr,before=before,after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4)))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
