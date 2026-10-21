#!/usr/bin/env python3
"""Original46eae0 physical graph with declared resource-factory collaborator.
All physics constructors/transforms/compound aggregation execute original x86.
539fe0 is the boundary: declared raw resource instances retain real weak/strong
record layout, payload and names; this probe does not claim factory allocation.
"""
import json,hashlib,struct
from pathlib import Path
from unicorn import x86_const as X
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
WORLD=0x6409c4
VT=0x30000000;OWNER=0x30010000

def bits(x):return struct.unpack('<I',struct.pack('<f',x))[0]
def words(o,a,n):return [[p,o.read_u32(p)]for p in range(a,a+n,4)if o.read_u32(p)]
def run():
 rows=[]
 for count in [0,1,2]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD);o.write_u32(0x640bd0,1)
  o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
  for off,e,b in [(0x84,0x30001000,0x30003000),(0x88,0x30002000,0x30004000)]:o.write_u32(WORLD+off,e);o.write_u32(e+0x5c,b);o.write_u32(b+0x9c,0x105)
  k=o.alloc(0x2e98);parent=o.alloc(0x120);arg=o.alloc(64);obj=o.alloc(0x212c)
  o.write(arg,b'0,0,0,0,0,0,7\0');o.write_u32(k+0x120,1);o.write_u32(k+0xff0,0);o.write_u32(k+0xff4,1);o.write_u32(k+0x11dc,bits(2.));o.write_u32(k+0x11e0,bits(.05));o.write_u32(k+0xfa8,count)
  o.write_u32(parent+8,0xffff0080);o.write_u32(parent+12,OWNER);o.write_u32(OWNER,VT);o.write_u32(OWNER+0x4c,1);o.write_u32(OWNER+0x50,0xffff0080)
  o.write_u32(VT+12,0x300f0000);resources={OWNER:obj};calls=[]
  def get(oc,args):return resources[oc.uc.reg_read(X.UC_X86_REG_ECX)]
  o.hook(0x300f0000,get,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  def create(oc,args):
   owner,unused,entity,name=args;name=bytes(oc.read(name,32)).split(b'\0')[0].decode();index=len(calls);r=0x30020000+index*0x1000;payload=r+0x100
   oc.write_u32(r,VT);oc.write_u32(r+0x4c,1);oc.write_u32(r+0x50,0xffff0100+index);oc.write_u32(r+0x54,0x200);oc.write_u32(payload+0x84,entity);oc.write_u32(payload+0x88,oc.read_u32(entity));oc.write_u32(entity,payload+0x84);oc.write_u32(entity+4,oc.read_u32(entity+4)+1)
   resources[r]=payload;calls.append(dict(owner=owner,entity=entity,name=name,resource=r));return r
  o.hook(0x539fe0,create,'cdecl',4)
  for i in range(count):
   b=o.call(0x49ab60,[bits(1.+i),5,bits(2.),bits(3.),bits(4.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   rec=k+0x128+i*0x74;o.write_f32s(rec,[float(i),2.,3.]);o.write_u32(rec+0x3c,1);o.write_u32(rec+0x40,b)
   angles=0x30008000;o.write_f32s(angles,[0.,-.157 if i else 0.,0.]);o.call(0x54ecd0,[angles],convention='thiscall',this=rec+12,fpu_cw=CW_D3D9)
  before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,OWNER,0x100)+words(o,0x30001000,0x4000);heap_before=o.heap_ptr
  o.call(0x46eae0,[parent,arg,obj],convention='thiscall',this=k,fpu_cw=CW_D3D9)
  after=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,OWNER,0x100)+words(o,0x30001000,0x4000)
  for r in list(resources)[1:]:after+=words(o,r,0x200)
  rows.append(dict(count=count,k=k,parent=parent,object=obj,input_words=before,heap_before=heap_before,output_words=after,heap=o.heap_ptr,calls=calls))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
