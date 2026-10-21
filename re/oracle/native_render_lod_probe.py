#!/usr/bin/env python3
"""Original46f9e0/44c1f0 with declared precreated render-factory outputs.
Factory539880, resource data/type/load and destroy48a8d0 are collaborators.
Original5411b0 bone creation and48c0f0/48be10 retarget/controller graphs execute.
Compare complete arenas for selection/removal, normal COM/basis retargeting,
second bone, wheel indices0/1, clipping planes, colors and resource flags.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def snapshot(o):return o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4)
def run():
 rows=[]
 profiles=[(*p,None)for p in [(0,0,False,False,False),(1,1,False,False,False),(0,0,True,False,False),(2,0,False,False,False),(2,0,False,True,False),(0,0,False,False,True),(1,1,False,False,True)]]
 profiles += [(0,0,False,False,True,0),(1,0,False,False,True,1)]
 for lod,variant,lighting,remove,retarget,wheel_index in profiles:
  o=Oracle();o.hook_alloc();o.write_u32(0x6200a4,123)
  k=o.alloc(0x2e98);body=o.alloc(0x212c);refs=o.alloc(32);indices=o.alloc(8);source=o.alloc(0x200);resource=o.alloc(0x300);table=o.alloc(0x100);payload=resource+0xd8;colors=o.alloc(8)
  o.call(0x5404d0,[],convention='thiscall',this=payload,fpu_cw=CW_D3D9)
  o.write_u32(k+0xfa8,1);o.write_u32(k+0xff8,2);o.write_u32(k+0xff0,0xffffffff)
  descriptor=k+0x128;o.write_u32(descriptor+0x3c,3)
  flags=(0 if retarget else 0x8000)|(1<<20)|(1 if lighting else 0);o.write_u32(descriptor+0x58,flags)
  o.write_u32(source+0x50,77);o.call(0x429060,[source],convention='thiscall',this=descriptor+0x48)
  o.write_u32(descriptor+0x5c,2);o.write_u32(descriptor+0x60,colors);o.write_u32(colors,0x11223344);o.write_u32(colors+4,0xaabbccdd)
  o.write_u32(body+8,0xffff1234);o.write_u32(body+0x4c,refs);o.write_u32(body+0x50,indices);o.write_u32(body+0x58,variant);o.write_u32(body+0xac,0xabcd1234)
  o.write_f32s(body+0xfc,[1.25,-2.5,10.]);o.write_f32s(body+0x108,[.1,-.3,.7])
  o.write_u32(resource,table);o.write_u32(resource+0x4c,3);o.write_u32(resource+0x50,0xffff0200);o.write_u32(resource+0x54,0x201);o.write_u32(table+0xc,0x300f0000);o.write_u32(table+0x14,0x300f0010)
  o.hook(0x300f0000,lambda oc,a:payload if oc.reg('ecx')==resource else physbody,'thiscall',1);o.hook(0x300f0010,lambda oc,a:None,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  physbody=0
  if retarget:
   o.write_u32(0x617650,0x6409c4)
   physres=o.alloc(0x100);physbody=o.alloc(0x90);physical=o.alloc(0x90);definition=o.alloc(0x80)
   o.write_u32(physres,table);o.write_u32(physres+0x4c,1);o.write_u32(physres+0x50,0xffff0080);o.write_u32(physres+0x54,0x201)
   o.call(0x429060,[physres],convention='thiscall',this=body+0x2c)
   o.write_u32(physbody+0x80,physbody+0x6c);o.write_u32(physbody+0x70,physbody+0x78)
   frame=k+0x1000;o.write_u32(frame+0x40,physical);o.write_f32(physical+0x14,1.);o.write_u32(physical+0x78,definition);o.write_f32s(definition+0x40,[.13,-.21,.33])
   o.write_f32s(frame,[.31,.7,-.87]);o.write_f32s(descriptor,[1.33,-.17,.54])
   for base,values in [(frame+12,[.9877,.13,-.15635,-.2,1.1,.21,.15635,.23,.9877]),(descriptor+12,[.87,.21,-.33,-.11,1.,.13,.44,.17,.92])]:
    for i,value in enumerate(values):o.write_f32(base+(i//3)*16+(i%3)*4,value)
   if lod==1:o.write_u32(k+0x11b8,1);o.write_f32s(k+0x11bc,[.1,-.2,.3])
  if wheel_index is not None:
   wheels=o.alloc(2*0x2b4);o.write_u32(body+0x13e4,wheels);o.write_u32(k+0x1f40,2);o.write_u32(descriptor+0x58,flags|0x1000|(wheel_index<<24))
   o.call(0x429060,[physres],convention='thiscall',this=wheels+wheel_index*0x2b4)
  calls=[]
  o.hook(0x539880,lambda oc,a:(calls.append(['create',a[0],a[1],oc.read_cstr(a[2]).decode()]) or resource),'cdecl',4)
  o.hook(0x48a8d0,lambda oc,a:calls.append(['destroy',oc.reg('ecx')]),'thiscall',0)
  if remove:
   o.call(0x46f9e0,[body,0],convention='thiscall',this=k,fpu_cw=CW_D3D9);calls.clear()
  start=o.heap_ptr;before=snapshot(o)
  o.call(0x46f9e0 if wheel_index is None else 0x44c1f0,[body,lod],convention='thiscall',this=k,fpu_cw=CW_D3D9)
  rows.append(dict(wheel_index=wheel_index,retarget=retarget,physbody=physbody,k=k,body=body,source=source,resource=resource,payload=payload,start=start,end=o.heap_ptr,before=before,after=snapshot(o),lod=lod,calls=calls))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
