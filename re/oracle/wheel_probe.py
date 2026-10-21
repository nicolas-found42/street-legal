#!/usr/bin/env python3
"""Execute original WheelRef setters with real argument marshaling.

Only Native.ptr and scalar field lookups are controlled. Spring491f80 and
matrix54ecd0 run original machine code; capture all wheel and spring words.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from io_vectors import _cell,_call_frame

ROWS=[('setDrive',0x440e00,'F'),('setSteer',0x440e90,'F'),('setRadius',0x440ed0,'F'),('setFriction',0x440f90,'F'),('setFrictn_x',0x441050,'F'),('setSliction',0x441090,'F'),('setStiffness',0x4410d0,'F'),('setRollRes',0x441110,'F'),('setBearing',0x441150,'F'),('setMaxLoad',0x441190,'F'),('setLoadSmooth',0x4411d0,'F'),('setPacejka',0x441210,'IF'),('setForce',0x441270,'F'),('setDamping',0x4412e0,'F'),('setDamping',0x441360,'FF'),('setRestLen',0x4413e0,'F'),('setMinLen',0x441460,'F'),('setMaxLen',0x4414d0,'F'),('setBrake',0x441580,'F'),('setHBrake',0x441640,'F'),('setWidth',0x4416c0,'F'),('setCPatch',0x441700,'FFF'),('setArm',0x441770,'FFFFFFF'),('setHub',0x441880,'FFFFFFFFFF'),('setInstantCenter',0x441a80,'FFFFFF'),('setYpr',0x442090,'Ljava.lang.Ypr;')]
def bits(f):return struct.unpack('<I',struct.pack('<f',f))[0]
def run_row(name,address,params,values,seed=.25,null=False,ypr=None):
 o=Oracle();o.hook_alloc();wheel=o.alloc(0x2b4);spring=o.alloc(0x100)
 initial=[bits(seed)]*(0x2b4//4);initial[0x30//4]=0
 o.write(wheel,struct.pack('<173I',*initial));o.write_u32(wheel+0x30,spring)
 o.hook(0x42ab50,lambda oc,a:0 if null else wheel,'cdecl',2)
 if ypr is not None:
  o.hook(0x42a560,lambda oc,a:float(ypr['ypr'.index(oc.read_cstr(a[1]).decode())]),'cdecl',2)
 signature='('+params+')V';types=list(params) if ypr is None else ['L']
 cells=[_cell(o,t,v) for t,v in reversed(list(zip(types,values)))]+[_cell(o,'L',123)]
 frame=_call_frame(o,'java.game.parts.WheelRef',name,signature,cells)
 o.call(address,[frame],fpu_cw=CW_D3D9)
 result=o.read_u32s(wheel,173);result[0x30//4]=0
 return dict(name=name,address=address,signature=signature,args=values,seed=bits(seed),null=null,ypr=ypr,words=result,spring=o.read_u32s(spring,64))
def getwheel_rows():
 rows=[]
 for index in [-1,0,1,2]:
  o=Oracle();o.hook_alloc();handle,resource,nativeres,vtable,data,nativedata=[o.alloc(0x100) for _ in range(6)];body=o.alloc(0x1500);template=o.alloc(0x2200);wheels=o.alloc(0x568)
  o.write_u32(handle+12,resource)
  for r in [resource,nativeres]:o.write_u32(r,vtable);o.write_u32(r+0x4c,1)
  o.write_u32(vtable+12,0x300f0000);o.write_u32(data+0x44,nativeres);o.write_u32(data+0x4c,body);o.write_u32(nativedata+0xc,template);o.write_u32(template+0x1f40,2);o.write_u32(body+0x13e4,wheels)
  o.hook(0x300f0000,lambda oc,a:data if oc.reg('ecx')==resource else nativedata,'thiscall',1)
  o.hook(0x42ab50,lambda oc,a:handle,'cdecl',2);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  o.hook(0x410890,lambda oc,a:42,'thiscall',1)
  allocations=[];writes=[]
  def alloc(oc,a):
   word=oc.alloc(0x40);allocations.append(word);return word
  o.hook(0x404e20,alloc,'thiscall',0)
  o.hook(0x42a9e0,lambda oc,a:writes.append(a),'cdecl',3)
  cells=[_cell(o,'I',index&0xffffffff),_cell(o,'L',123)]
  frame=_call_frame(o,'java.game.parts.bodypart.Chassis','getWheel','(I)Ljava.game.parts.WheelRef;',cells)
  results=[o.call(0x440c80,[frame],fpu_cw=CW_D3D9).eax for _ in range(2)]
  expected=(wheels+index*0x2b4) if 0<=index<2 else None
  assert all(a[2]==expected for a in writes)
  assert (len(allocations)==2 and results[0]!=results[1]) if expected is not None else (results==[0,0] and not allocations)
  rows.append(dict(index=index,count=2,allocations=len(allocations),distinct_wrappers=results[0]!=results[1],bound_offsets=[a[2]-wheels for a in writes]))
 return rows
def constructor_rows():
 rows=[]
 for fill in [0,0xa5]:
  o=Oracle();o.hook_alloc();wheel=o.alloc(0x2b4);o.write(wheel,bytes([fill])*0x2b4)
  result=o.call(0x45e840,[],convention='thiscall',this=wheel,fpu_cw=CW_D3D9).eax
  assert result==wheel
  rows.append(dict(fill=fill,address=wheel,words=o.read_u32s(wheel,173)))
 return rows
def template_copy_rows():
 from unicorn import UC_HOOK_CODE
 from oracle import SENTINEL
 rows=[]
 for index in [0,1]:
  o=Oracle();o.hook_alloc();template=o.alloc(0x3000);body=o.alloc(0x1500);wheels=o.alloc(0x568);wheel=wheels+index*0x2b4
  for i in range(0x3000//4):o.write_u32(template+i*4,bits((i%13+1)*.125))
  o.write_u32(body+0x13e4,wheels)
  o.call(0x45e840,[],convention='thiscall',this=wheel,fpu_cw=CW_D3D9)
  def boundary(uc,address,size,user):
   if address==0x44a9ab:
    stack=o.reg('esp');o.set_reg('ebx',template+0x1ffc+index*0xc8)
    for off,value in [(0x10,template),(0x14,index*0x2b4),(0x20,index),(0x28,body)]:o.write_u32(stack+off,value)
   if address==0x44ab58:o.set_reg('eip',SENTINEL)
  o.uc.hook_add(UC_HOOK_CODE,boundary)
  o.call(0x44a9ab,[],fpu_cw=CW_D3D9)
  rows.append(dict(index=index,template=template,body=body,wheels=wheels,wheel=wheel,template_words=o.read_u32s(template,0x3000//4),words=o.read_u32s(wheel,173),scope='Original44a9ab..44ab58 bounded template copy; register/stack inputs declared, stop before physicsworld reads'))
 return rows
def run():
 rows=[]
 for name,address,params in ROWS:
  if name=='setYpr':
   for angles in ([0.,0.,0.],[.1,-.2,.3]):rows.append(run_row(name,address,params,[1234],ypr=angles))
   continue
  values=[bits((i+1)*.2) if t=='F' else 3 for i,t in enumerate(params)]
  rows.append(run_row(name,address,params,values));rows.append(run_row(name,address,params,values,null=True))
  if name in ('setFriction','setBrake','setHBrake'):
   rows.append(run_row(name,address,params,[bits(-2.5)],seed=1.5))
  if name=='setPacejka':
   for index in [-1,0,16,17]:rows.append(run_row(name,address,params,[index&0xffffffff,bits(2.5)]))
  if name=='setHub':rows.append(run_row(name,address,params,[0]+values[1:]))
 return {'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'scope':__doc__,'rows':rows,'getwheel':getwheel_rows(),'constructors':constructor_rows(),'template_copy':template_copy_rows()}
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
