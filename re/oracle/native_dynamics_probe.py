#!/usr/bin/env python3
"""Original4a5190 with real CRT/world startup and all native solver calls.
Declared boundaries: allocator/free, process atexit, profiling clocks and script
426f60 drive dispatch and537790 resource requests. Active profiles execute
original integration and swept-bounds calls for independent and compound bodies
across repeated steps. Spatial-index queue and contacts are empty in these
profiles; they cannot establish configured-car or collision acceptance.
"""
import json,hashlib,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
WORLD=0x6409c4
bits=lambda x:struct.unpack('<I',struct.pack('<f',x))[0]
def words(o,a,n):return [[p,o.read_u32(p)]for p in range(a,a+n,4)if o.read_u32(p)]
def run():
 rows=[]
 for sequence in [[0.00005,0.00005],[.02],[.016,.016],[0.,-.001,.02]]:
  o=Oracle();o.hook_alloc();o.hook(0x5d707d,lambda oc,a:0,'cdecl',1)
  o.hook(0x552310,lambda oc,a:None,'cdecl',1);o.hook(0x552330,lambda oc,a:0.,'cdecl',1)
  callbacks=[];o.hook(0x426f60,lambda oc,a:callbacks.append(a[0]),'thiscall',1)
  for address,this in [(0x498080,None),(0x497b60,WORLD),(0x4a5580,WORLD)]:o.call(address,[],convention='thiscall' if this else 'cdecl',this=this,fpu_cw=CW_D3D9)
  before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,0x643730,0x30)+words(o,0x643980,0x60)
  start=o.heap_ptr;results=[]
  for dt in sequence:
   r=o.call(0x4a5190,[bits(dt)],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
   results.append(dict(result=r.eax,world=words(o,WORLD,0x210),heap=o.heap_ptr,callbacks=list(callbacks)))
  after=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)+words(o,0x643730,0x30)+words(o,0x643980,0x60)
  rows.append(dict(dt=list(map(bits,sequence)),input_words=before,heap_before=start,results=results,output_words=after,heap=o.heap_ptr))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),empty=rows,active=active_rows(),integrators=integrator_rows())

def active_rows():
 rows=[]
 for mass,velocity,omega,nested in [(1000.,[1.,0.,0.],[0.,0.,0.],False),(2.,[3.,2.,1.],[.1,-.2,.3],False),(2.,[3.,2.,1.],[.1,-.2,.3],True),(2.,[3.,2.,1.],[.1,-.2,.3],'spring'),(2.,[0.,0.,0.],[0.,0.,0.],'sleep'),(2.,[3.,2.,1.],[.1,-.2,.3],'euler'),(2.,[3.,2.,1.],[.1,-.2,.3],'plain'),(2.,[3.,2.,1.],[.1,-.2,.3],'spatial'),(2.,[3.,2.,1.],[.1,-.2,.3],'spatial-request'),(2.,[3.,2.,1.],[.1,-.2,.3],'compound-spatial'),(2.,[3.,2.,1.],[.1,-.2,.3],'indexed')]:
  o=Oracle();o.hook_alloc();o.hook(0x5d707d,lambda oc,a:0,'cdecl',1)
  o.hook(0x552310,lambda oc,a:None,'cdecl',1);o.hook(0x552330,lambda oc,a:0.,'cdecl',1)
  callbacks=[];o.hook(0x426f60,lambda oc,a:callbacks.append(a[0]),'thiscall',1)
  for address,this in [(0x498080,None),(0x497b60,WORLD),(0x4a5580,WORLD)]:o.call(address,[],convention='thiscall' if this else 'cdecl',this=this,fpu_cw=CW_D3D9)
  body=o.call(0x49ab60,[bits(mass),5,bits(1.),bits(1.),bits(1.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  position=0x30001000;o.write_f32s(position,[0.,2.,0.])
  entity=o.call(0x49b300,[body,position,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  if nested is True or nested=='compound-spatial':
   child_body=o.call(0x49ab60,[bits(1.),5,bits(.2),bits(.3),bits(.4),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   o.write_f32s(position,[.2,1.7,.4]);child=o.call(0x49b230,[entity,child_body,position,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   o.call(0x49a050,[entity],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
  if nested=='spring':
   handle=o.alloc(16);spring=o.alloc(0xec);o.call(0x45e5f0,[spring],convention='fastcall',fpu_cw=CW_D3D9)
   for offset in [0x7c,0x80,0x94,0x98,0xb8,0xbc]:o.write_u32(spring+offset,0)
   o.write_u32(spring,0x5f0fcc);o.write_f32s(position,[.3,.2,-.7])
   o.call(0x490dc0,[handle,entity,position,0,bits(.9),bits(.17)],convention='thiscall',this=spring,fpu_cw=CW_D3D9)
  if nested=='sleep':o.write_u32(WORLD+0x78,0)
  if nested in ['euler','plain']:o.write_u32(entity+0x58,o.read_u32(entity+0x58)|0x100)
  if nested=='plain':o.write_u32(body+0x9c,o.read_u32(body+0x9c)|0x200)
  requests=[]
  if nested!='indexed':o.hook(0x537790,lambda oc,a:requests.append(a),'thiscall',4)
  if nested in ['spatial','spatial-request']:
   resource=o.alloc(0x100);o.write_u32(entity+0x50,1);o.write_u32(entity+0x54,resource)
   o.write_u32(resource+0x54,0x100 if nested=='spatial-request' else 0)
  if nested=='compound-spatial':
   for target in [entity,child]:
    resource=o.alloc(0x100);o.write_u32(target+0x50,1);o.write_u32(target+0x54,resource);o.write_u32(resource+0x54,0x100)
  if nested=='indexed':
   from native_spatial_index_probe import resource as make_resource,append
   o.write_u32(0x6200a4,0)
   root_resource=make_resource(o,0,[0.,0.,0.],[50.,50.,50.])
   owner=make_resource(o,1,[0.,2.,0.],[2.,2.,2.]);append(o,root_resource,owner);o.write_u32(owner+0xcc,root_resource)
   physical=make_resource(o,2,[0.,2.,0.],[1.,1.,1.],kind=2);append(o,owner,physical)
   o.write_u32(physical+0x15c,entity)
   o.call(0x429060,[owner],convention='thiscall',this=entity+0x38)
   o.call(0x429060,[physical],convention='thiscall',this=entity+0x48)
  o.write_f32s(entity+0x70,velocity);o.write_f32s(entity+0xd4,omega)
  for step in range(3):
   callbacks.clear();requests.clear()
   heap_before=o.heap_ptr
   before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)
   r=o.call(0x4a5190,[bits(.02)],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
   rows.append(dict(profile=str(nested),real_requests=nested=='indexed',step=step,heap_before=heap_before,heap=o.heap_ptr,entity=entity,body=body,input_words=before,result=r.eax,output_words=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210),callbacks=list(callbacks),requests=list(requests)))
 return rows



def integrator_rows():
 rows=[]
 for mass,angular,mode in [(1000.,False,'rk4'),(2.,True,'rk4'),(2.,True,'euler'),(2.,True,'plain')]:
  o=Oracle();o.hook_alloc();o.hook(0x5d707d,lambda oc,a:0,'cdecl',1)
  for address in [0x497b60,0x4a5580]:o.call(address,[],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9)
  body=o.call(0x49ab60,[bits(mass),5,bits(1.),bits(1.),bits(1.),0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  position=0x30001000;o.write_f32s(position,[1.,2.,3.])
  entity=o.call(0x49b300,[body,position,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  if mode!='rk4':o.write_u32(entity+0x58,o.read_u32(entity+0x58)|0x100)
  if mode=='plain':o.write_u32(body+0x9c,o.read_u32(body+0x9c)|0x200)
  src=entity+0x64;dst=src+0xd4
  o.write_f32s(src+0xc,[1.,2.,3.]);o.write_f32s(src+0x28,[-9810.,4.,2.])
  if angular:
   o.write_f32s(src+0x70,[.1,-.2,.3]);o.write_f32s(src+0x7c,[.4,.2,-.1]);o.write_f32s(src+0x88,[.2,-.3,.4]);o.write_f32s(src+0x94,[.1,-.2,.3,.9])
  ranges=[(HEAP_BASE,o.heap_ptr-HEAP_BASE),(0x6439e0,0x7a0),(0x7a0560,0x30)]
  before=sum((words(o,a,n)for a,n in ranges),[])
  o.call(0x4a7c10,[src,dst,bits(.02)],convention='thiscall',this=entity,fpu_cw=CW_D3D9)
  after=sum((words(o,a,n)for a,n in ranges),[])
  rows.append(dict(entity=entity,body=body,source=src,destination=dst,dt=bits(.02),ranges=ranges,input_words=before,output_words=after,heap=o.heap_ptr))
 return rows
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
