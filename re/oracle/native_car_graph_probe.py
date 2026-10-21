#!/usr/bin/env python3
"""Original44a250 through44b486: composed Part, root, wheel and spring graph.
539fe0 resource construction is the declared collaborator on both sides; all
physics functions and caller frames execute original x86. The caller supplies
an existing zeroed object to the allocator's first0x212c request, matching the
port's placement API. Camera/layout/render finalization follows this boundary.
"""
import json,hashlib,struct
from pathlib import Path
from unicorn import x86_const as X,UC_HOOK_CODE
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE,SENTINEL
WORLD=0x6409c4
VT=0x30000000;OWNER=0x30010000

def bits(x):return struct.unpack('<I',struct.pack('<f',x))[0]
def words(o,a,n):return [[p,o.read_u32(p)]for p in range(a,a+n,4)if o.read_u32(p)]
def run(final=False,attachments=0):
 rows=[]
 for count in [0,1,2]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(0x617650,WORLD);o.write_u32(0x640bd0,1);o.call(0x497b60,[],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
  o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
  for off,e,b in [(0x84,0x30001000,0x30003000),(0x88,0x30002000,0x30004000)]:o.write_u32(WORLD+off,e);o.write_u32(e+0x5c,b);o.write_u32(b+0x9c,0x105)
  k=o.alloc(0x2e98);parent=o.alloc(0x120);arg=o.alloc(64);obj=o.alloc(0x212c)
  parameter_block=o.alloc(0xb00);o.write_u32(k+0x2e48,parameter_block);o.write_u32(k+0x1f40,count);o.write_f32s(WORLD+0x104,[0.,-9.81,0.]);
  o.write(arg,b'0,0,0,0,0,0,7\0');o.write_u32(k+0x120,1);o.write_u32(k+0xff0,0);o.write_u32(k+0xff4,1);o.write_u32(k+0x11dc,bits(2.));o.write_u32(k+0x11e0,bits(.05));o.write_u32(k+0xfa8,count)
  o.write_u32(parent+8,0xffff0080);o.write_u32(parent+12,OWNER);o.write_u32(OWNER,VT);o.write_u32(OWNER+0x4c,1);o.write_u32(OWNER+0x50,0xffff0080)
  o.write_u32(VT+12,0x300f0000);resources={OWNER:obj};calls=[];render_calls=[];attachment_calls=[]
  def get(oc,args):return resources[oc.uc.reg_read(X.UC_X86_REG_ECX)]
  o.hook(0x300f0000,get,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  def create(oc,args):
   owner,unused,entity,name=args;name=bytes(oc.read(name,32)).split(b'\0')[0].decode();index=len(calls);r=0x30020000+index*0x1000;payload=r+0x100
   oc.write_u32(r,VT);oc.write_u32(r+0x4c,1);oc.write_u32(r+0x50,0xffff0100+index);oc.write_u32(r+0x54,0x200);oc.write_u32(payload+0x84,entity);oc.write_u32(payload+0x88,oc.read_u32(entity));oc.write_u32(entity,payload+0x84);oc.write_u32(entity+4,oc.read_u32(entity+4)+1)
   resources[r]=payload;calls.append(dict(owner=owner,entity=entity,name=name,resource=r));return r
  o.hook(0x539fe0,create,'cdecl',4)
  if final:
   o.hook(0x46f9e0,lambda oc,a:render_calls.append([False,a[1]]),'thiscall',2)
   o.hook(0x44c1f0,lambda oc,a:render_calls.append([True,a[1]]),'thiscall',2)
  for i in range(count):
   b=o.call(0x49ab60,[bits(1.+i),5,bits(2.),bits(3.),bits(4.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   rec=k+0x128+i*0x74;o.write_f32s(rec,[float(i),2.,3.]);o.write_u32(rec+0x3c,1);o.write_u32(rec+0x40,b)
   angles=0x30008000;o.write_f32s(angles,[0.,-.157 if i else 0.,0.]);o.call(0x54ecd0,[angles],convention='thiscall',this=rec+12,fpu_cw=CW_D3D9)
  if attachments:
   o.write_u32(parameter_block+0xa54,attachments)
   for index in range(attachments):o.write_u32(parameter_block+0xa58+index*0x1c+24,bits(.75))
   def attach(oc,args):
    attachment_calls.append(dict(destination=oc.reg('ecx'),source=args[0],position=args[1],rotation=args[2],fov=args[3],internal=args[4]));return 0
   o.hook(0x48b940,attach,'thiscall',5)
  allocated=[False]
  def allocate(oc,args):
   if args[0]==0x212c and not allocated[0]:allocated[0]=True;return obj
   return oc.alloc(args[0]) if args[0] else 0
  o.hook(0x54f560,allocate,'cdecl',1)
  wheel_frames=[]
  def stop(uc,pc,size,user):
   if pc==0x44ab58:
    e=oc_entity=o.read_u32(obj+0x13bc);d=o.read_u32(o.read_u32(e+0x5c)+0x78);other=o.read_u32(o.reg('esp')+0x38);rel=o.read_u32(other+0x1c);wheel_frames.append(dict(entity=e,other=other,center=o.read_u32s(d+0x40,3),relative=o.read_u32s(rel+4,3)))
   if pc==(0x44b903 if final else 0x44b486):uc.reg_write(X.UC_X86_REG_EIP,SENTINEL)
  o.uc.hook_add(UC_HOOK_CODE,stop)
  for i in range(count):
   end=k+0x1ffc+i*0xc8
   o.write_f32s(end-0xb8,[float(i),0.,0.])
   for off,val in [(-0x58,.9),(-0x54,.3),(-0x50,1.),(-0x4c,.3),(-0x48,.5),(-0x44,.2),(-0x40,.1),(-0x3c,.1)]:o.write_u32(end+off,bits(val))
  before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,OWNER,0x100)+words(o,0x30001000,0x4000);heap_before=o.heap_ptr
  o.call(0x44a250,[parent,arg],convention='thiscall',this=k,fpu_cw=CW_D3D9)
  after=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,OWNER,0x100)+words(o,0x30001000,0x4000)
  for r in list(resources)[1:]:after+=words(o,r,0x200)
  rows.append(dict(attachment_calls=attachment_calls,render_calls=render_calls,wheel_frames=wheel_frames,count=count,k=k,parent=parent,object=obj,input_words=before,heap_before=heap_before,output_words=after,heap=o.heap_ptr,calls=calls))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
