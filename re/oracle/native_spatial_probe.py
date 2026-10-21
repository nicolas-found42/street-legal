#!/usr/bin/env python3
"""Original54b850/54ba20 bounds classification,4986f0 swept and4988a0 aggregate bounds.
Only537790 resource request is a declared callback; classification and all raw
bounds stores execute original x86. These vectors do not substitute index trees.
"""
import json,random,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
from native_spring_probe import bits

def words(o,a,n):return [[p,o.read_u32(p)]for p in range(a,a+n,4)if o.read_u32(p)]
def run():
 kernels=[];rng=random.Random(0x4a5190)
 for kind in [0,1,2]:
  for index in range(100):
   o=Oracle();bound=o.alloc(28);point=o.alloc(12);half=o.alloc(12)
   centers=[[rng.choice([-3.,-1.,-.1,0.,.1,1.,3.])for _ in range(3)]for _ in range(2)]
   extents=[[rng.choice([.01,.1,1.,2.])for _ in range(3)]for _ in range(2)]
   data=[kind]+list(map(bits,centers[0]+extents[0]));p=list(map(bits,centers[1]));h=list(map(bits,extents[1]));o.write_u32s(bound,data);o.write_u32s(point,p);o.write_u32s(half,h)
   sphere=o.call(0x54b850,[point,h[0]],convention='thiscall',this=bound,fpu_cw=CW_D3D9).eax
   box=o.call(0x54ba20,[point,half],convention='thiscall',this=bound,fpu_cw=CW_D3D9).eax
   kernels.append(dict(bounds=data,center=p,half=h,sphere=sphere,box=box))
 aggregate=[]
 for count in [0,1,3]:
  o=Oracle();parent=o.alloc(0x200);tail=parent+0x30;o.write_u32(parent+0x20,tail);o.write_u32(parent+0x80,2)
  nodes=[o.alloc(0x200)for _ in range(count)]
  if nodes:o.write_u32(parent+0x20,nodes[0])
  for i,node in enumerate(nodes):
   o.write_u32(node+4,nodes[i+1]if i+1<count else tail);o.write_u32(node+0x4c,2);o.write_u32(node+0x80,1)
   o.write_f32s(node+0x84,[i*1.7-2.,i*-.3+1.,i*.4-2.]);o.write_f32s(node+0x90,[.1+i*.2,.2+i*.1,.3+i*.3])
  before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE);o.call(0x4988a0,[parent],fpu_cw=CW_D3D9)
  aggregate.append(dict(resource=parent,input_words=before,output_words=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE),heap=o.heap_ptr))
 swept=[]
 for active in [0,1]:
  for flags in [0,0x100]:
   o=Oracle();entity=o.alloc(0x218);body=o.alloc(0xa0);definition=o.alloc(0xac);resource=o.alloc(0x200)
   o.write_u32(entity+0x54,resource);o.write_u32(entity+0x5c,body);o.write_u32(body+0x78,definition);o.write_u32(entity+0x20c,active);o.write_u32(definition+0xc,bits(2.3));o.write_u32(resource+0x54,flags)
   o.write_f32s(entity+0x64,[1.,-2.,3.]);o.write_f32s(entity+0x138,[-.3,1.7,-2.1])
   calls=[];o.hook(0x537790,lambda oc,a:calls.append(a),'thiscall',4)
   before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE);o.call(0x4986f0,[entity],fpu_cw=CW_D3D9)
   swept.append(dict(entity=entity,input_words=before,output_words=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE),heap=o.heap_ptr,calls=calls))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),kernels=kernels,aggregate=aggregate,swept=swept)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
