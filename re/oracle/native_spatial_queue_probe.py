#!/usr/bin/env python3
"""Original498810 enqueue,498dd0 index refresh,4a2540 queue consumption.
Constructors, bounds aggregation, relocation, physical candidate eligibility,
resource access and queue destruction execute original x86. These scenes have
no native type35 collision engine; collision resolution is outside this fixture.
Allocator/free and process atexit are the only execution callbacks.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
from native_spatial_index_probe import resource,append
WORLD=0x6409c4

def image(o):
 ranges=[(HEAP_BASE,o.heap_ptr),(0x643730,0x643750),(WORLD,WORLD+0x60),(0x643530,0x643534)]
 return [[p,o.read_u32(p)]for a,b in ranges for p in range(a,b,4)if o.read_u32(p)]
def run():
 rows=[]
 for mode in ['valid','empty','deleted','invalid','duplicate','sleeping','huge','nested-move']:
  o=Oracle();o.hook_alloc();o.write_u32(0x6200a4,0);o.hook(0x5d707d,lambda oc,a:0,'cdecl',1);o.call(0x498080)
  root=resource(o,0,[0.,0.,0.],[50.,50.,50.]);car=resource(o,1,[1.,0.,0.],[1.,1.,1.]);append(o,root,car)
  o.write_u32(car+0xcc,root)
  for i in range(0 if mode in ['empty','nested-move'] else 2):
   r=resource(o,i+2,[float(i),2.,-3.],[100.,100.,100.]if mode=='huge' else [.5,.6,.7],kind=2);append(o,car,r)
   entity=o.alloc(0x218);o.write_u32(entity+0x58,0x80 if mode=='sleeping' else 1);o.write_u32(r+0x15c,entity)
  if mode=='nested-move':
   for i,x in enumerate([20.,-20.]):
    group=resource(o,i+10,[0.,0.,0.],[1.,1.,1.]);append(o,car,group)
    r=resource(o,i+20,[x,0.,0.],[1.,1.,1.],kind=2);append(o,group,r)
  handle=o.alloc(16);o.call(0x429060,[car],convention='thiscall',this=handle)
  before=image(o);heap_before=o.heap_ptr
  o.call(0x498810,[handle]);
  if mode=='duplicate':o.call(0x498810,[handle])
  queued=image(o);heap_queued=o.heap_ptr
  if mode=='deleted':o.write_u32(car+0x54,o.read_u32(car+0x54)|0x200000)
  if mode=='invalid':o.call(0x429060,[0],convention='thiscall',this=o.read_u32(0x643738)+12)
  refresh_input=image(o);o.call(0x498dd0,fpu_cw=CW_D3D9);refreshed=image(o)
  r=o.call(0x4a2540,[WORLD],convention='fastcall',fpu_cw=CW_D3D9)
  rows.append(dict(mode=mode,resource=car,heap_before=heap_before,heap=heap_queued,input_words=before,queued_words=queued,refresh_input=refresh_input,refreshed_words=refreshed,output_words=image(o),result=r.eax))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
