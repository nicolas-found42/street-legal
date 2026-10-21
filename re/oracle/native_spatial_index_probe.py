#!/usr/bin/env python3
"""Original498b10 spatial tree relocation with actual resource constructors.
Every tested resource is already loaded and has a runtime ID. The native
collision-root lookup48ae40 is executed; these game payloads have no type35
collision root. No index or relocation callback is hooked.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
from native_spring_probe import bits

def words(o):return [[p,o.read_u32(p)]for p in range(HEAP_BASE,o.heap_ptr,4)if o.read_u32(p)]
def resource(o,ident,center,half,kind=1):
 r=o.alloc(0x164);d=o.alloc(24);o.write_u32s(d,[kind,0xffff1000+ident,0,0,0,0])
 o.call(0x53e6e0 if kind==1 else 0x53f8b0,[d,0,0],convention='thiscall',this=r,fpu_cw=CW_D3D9)
 o.write_u32(r+0x64,0x21);o.write_u32(r+0x68,bits(1.));o.write_u32(r+0x6c,bits(1.));o.write_u32(r+0x80,1)
 o.write_f32s(r+0x84,center);o.write_f32s(r+0x90,half)
 return r

def append(o,parent,child):
 tail=o.read_u32(parent+0x38);o.write_u32(tail+4,child);o.write_u32(child+4,parent+0x30);o.write_u32(child+8,tail);o.write_u32(parent+0x38,child);o.write_u32(child+0x14,parent)

def run():
 rows=[]
 for mode in ['stay','ascend','descend','sphere','unbounded','disabled','payload-blocks']:
  o=Oracle();o.hook_alloc()
  root=resource(o,0,[0.,0.,0.],[20.,20.,20.]);room=resource(o,1,[-3.,0.,0.],[2.,2.,2.]);sibling=resource(o,2,[3.,0.,0.],[2.,2.,2.])
  pos=[-3.,0.,0.]if mode=='stay' else [3.,0.,0.]if mode in ['descend','sphere','disabled','payload-blocks']else [9.,0.,0.]
  car=resource(o,3,pos,[.5,.5,.5]);append(o,root,room);append(o,root,sibling);append(o,room,car)
  if mode=='sphere':o.write_u32(car+0x80,0)
  if mode=='unbounded':o.write_u32(car+0x80,2)
  if mode=='disabled':o.write_u32(sibling+0x54,o.read_u32(sibling+0x54)|0x20)
  if mode=='payload-blocks':o.write_u32(sibling+0xd8+0x30,1)
  before=words(o);r=o.call(0x498b10,[car],fpu_cw=CW_D3D9)
  rows.append(dict(mode=mode,resource=car,heap=o.heap_ptr,result=r.eax,input_words=before,output_words=words(o)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
