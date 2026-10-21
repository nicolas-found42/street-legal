#!/usr/bin/env python3
"""Original camera render-state/bone constructors (5404d0,540270)."""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
WORLD=0x6409c4

def constructor_rows():
 rows=[]
 for fill in [0,0xa5]:
  for kind in ['render','bone']:
   o=Oracle();o.hook_alloc();o.write_u32(0x617650,WORLD);o.write_u32(WORLD,0x3f123456);o.write_u32(0x6200a4,123)
   size=0x190 if kind=='render' else 0xf4;p=o.alloc(size);o.write(p,bytes([fill])*size)
   args=[]
   if kind=='bone':name=o.alloc(32);o.write(name,b'bone00\0');args=[0x8000,name]
   o.call(0x5404d0 if kind=='render' else 0x540270,args,convention='thiscall',this=p,fpu_cw=CW_D3D9)
   rows.append(dict(kind=kind,fill=fill,p=p,words=o.read_u32s(p,size//4)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)


def retarget_rows():
 from unicorn import x86_const as X
 rows=[]
 for position,angles in [([0.,0.,0.],[0.,0.,0.]),([1.25,-2.5,10.],[.1,-.157,.2])]:
  o=Oracle();o.hook_alloc();o.write_u32(0x617650,WORLD);o.write_u32(0x6200a4,123)
  reference=o.alloc(16);resource=o.alloc(0x100);body=o.alloc(0x190);physref=o.alloc(16);physres=o.alloc(0x100);physbody=o.alloc(0x90);pos=o.alloc(12);ang=o.alloc(12);name=o.alloc(16);o.write(name,b'bone00\0')
  o.call(0x5404d0,[],convention='thiscall',this=body,fpu_cw=CW_D3D9)
  vtable=0x30000000;o.write_u32(vtable+12,0x300f0000)
  for r,id in [(resource,0xffff0100),(physres,0xffff0080)]:o.write_u32(r,vtable);o.write_u32(r+0x4c,1);o.write_u32(r+0x50,id);o.write_u32(r+0x54,0x201)
  o.hook(0x300f0000,lambda oc,a:body if oc.uc.reg_read(X.UC_X86_REG_ECX)==resource else physbody,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  o.call(0x429060,[resource],convention='thiscall',this=reference);o.call(0x429060,[physres],convention='thiscall',this=physref)
  o.write_u32(physbody+0x80,physbody+0x6c);o.write_u32(physbody+0x70,physbody+0x78)
  o.write_f32s(pos,position);o.write_f32s(ang,angles)
  id=o.call(0x5411b0,[name],convention='thiscall',this=body).eax
  bone=o.read_u32(body+0x12c)
  o.call(0x48bf50,[id,physref,pos,ang],convention='thiscall',this=reference,fpu_cw=CW_D3D9)
  rows.append(dict(position=[int.from_bytes(__import__('struct').pack('<f',v),'little')for v in position],angles=[int.from_bytes(__import__('struct').pack('<f',v),'little')for v in angles],reference=reference,resource=resource,body=body,physref=physref,physres=physres,physbody=physbody,bone=bone,id=id,heap=o.heap_ptr,words=[[p,o.read_u32(p)]for p in range(0x20000000,o.heap_ptr,4)if o.read_u32(p)]))
 return rows

def replacement_rows():
 from unicorn import x86_const as X
 rows=[]
 for mode in ["same","replace","null"]:
  position=[1.25,-2.5,10.];angles=[.1,-.157,.2]
  o=Oracle();o.hook_alloc();o.write_u32(0x617650,WORLD);o.write_u32(0x6200a4,123)
  reference=o.alloc(16);resource=o.alloc(0x100);body=o.alloc(0x190);physref=o.alloc(16);physres=o.alloc(0x100);physbody=o.alloc(0x90);pos=o.alloc(12);ang=o.alloc(12);name=o.alloc(16);o.write(name,b'bone00\0')
  o.call(0x5404d0,[],convention='thiscall',this=body,fpu_cw=CW_D3D9)
  vtable=0x30000000;o.write_u32(vtable+12,0x300f0000)
  for r,id in [(resource,0xffff0100),(physres,0xffff0080)]:o.write_u32(r,vtable);o.write_u32(r+0x4c,1);o.write_u32(r+0x50,id);o.write_u32(r+0x54,0x201)
  o.hook(0x300f0000,lambda oc,a:body if oc.uc.reg_read(X.UC_X86_REG_ECX)==resource else physbody,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  o.call(0x429060,[resource],convention='thiscall',this=reference);o.call(0x429060,[physres],convention='thiscall',this=physref)
  o.write_u32(physbody+0x80,physbody+0x6c);o.write_u32(physbody+0x70,physbody+0x78)
  o.write_f32s(pos,position);o.write_f32s(ang,angles)
  id=o.call(0x5411b0,[name],convention='thiscall',this=body).eax
  bone=o.read_u32(body+0x12c)
  o.call(0x48bf50,[id,physref,pos,ang],convention='thiscall',this=reference,fpu_cw=CW_D3D9)
  target_ref=physref;target_body=physbody
  if mode=='replace':
   target_ref=o.alloc(16);target_res=o.alloc(0x100);target_body=o.alloc(0x90)
   o.write_u32(target_res,vtable);o.write_u32(target_res+0x4c,1);o.write_u32(target_res+0x50,0xffff0090);o.write_u32(target_res+0x54,0x201)
   o.call(0x429060,[target_res],convention='thiscall',this=target_ref)
   o.write_u32(target_body+0x80,target_body+0x6c);o.write_u32(target_body+0x70,target_body+0x78)
   o.hook(0x300f0000,lambda oc,a:body if oc.uc.reg_read(X.UC_X86_REG_ECX)==resource else (physbody if oc.uc.reg_read(X.UC_X86_REG_ECX)==physres else target_body),'thiscall',1)
  if mode=='null':target_ref=0;target_body=0
  before=[[p,o.read_u32(p)]for p in range(0x20000000,o.heap_ptr,4)if o.read_u32(p)];heap_before=o.heap_ptr
  o.call(0x48bf50,[id,target_ref,pos,ang],convention='thiscall',this=reference,fpu_cw=CW_D3D9)
  rows.append(dict(mode=mode,input_words=before,heap_before=heap_before,target_ref=target_ref,target_body=target_body,position=[int.from_bytes(__import__('struct').pack('<f',v),'little')for v in position],angles=[int.from_bytes(__import__('struct').pack('<f',v),'little')for v in angles],reference=reference,resource=resource,body=body,physref=physref,physres=physres,physbody=physbody,bone=bone,id=id,heap=o.heap_ptr,words=[[p,o.read_u32(p)]for p in range(0x20000000,o.heap_ptr,4)if o.read_u32(p)]))
 return rows

def run():
 result=constructor_rows();result['retarget']=retarget_rows();result['replacement']=replacement_rows();return result
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
