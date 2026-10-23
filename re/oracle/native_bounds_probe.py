#!/usr/bin/env python3
"""Complete original498f50 bounds over full retained buffers, including sphere4ac370.

Only malloc/free are controlled zeroed bump boundaries. Parent dispatch0..8,
child dispatch0..8, active buffers, orientations, zero/two children and owned
prior poly storage exercise all original switch destinations. Outputs retain
all heap bytes, globals and the center argument; no field is excluded.
Run: ~/slr-oracle-venv/bin/python re/oracle/native_bounds_probe.py > re/out/boot/native-bounds-original.json
"""
import hashlib,itertools,json,random,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE,STACK_TOP

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def image(o):return o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4)
def make_entity(o,kind,index,oriented,point):
 e=o.alloc(0x218);body=o.alloc(0xa0);d=o.alloc(0xac);rel=o.alloc(0x50)
 o.write_u32(e+0x5c,body);o.write_u32(body+0x78,d);o.write_u32(body+0x14,bits(.5))
 o.write_u32(e+0x1c,rel);o.write_f32s(rel+4,point);o.write_u32(e+0x20c,index)
 o.write_u32(d+8,kind);o.write_u32(d+0xc,bits(2));o.write_u32(d+0x10,bits(.125))
 for n in [0,1]:
  o.write_f32s(e+0x64+n*0xd4,[v+(10 if n!=index else 0) for v in point])
  rotation=[[.9877,0,-.15635],[0,1,0],[.15635,0,.9877]] if oriented else [[1,0,0],[0,1,0],[0,0,1]]
  for r,values in enumerate(rotation):o.write_f32s(e+0xa4+n*0xd4+r*16,values)
 if kind in [6,7]:
  p=o.alloc(24);o.write_f32s(p,[-2,.5,3,4,-1,2]);o.write_u32(d+0x9c,2);o.write_u32(d+0xa0,p)
 else:o.write_f32s(d+0x9c,[1.25,2.5,3.75])
 return e,body,d

def run():
 rows=[];rng=random.Random(0x498f50)
 cases=list(itertools.product(range(9),range(9),[0,1]))
 cases += [(k,5,0) for k in range(9)]
 for i,(kind,child_kind,index) in enumerate(cases):
  o=Oracle();o.hook_alloc();e,b,d=make_entity(o,kind,index,bool(index),[1,-2,3])
  static=i%3==0;o.write_u32(b+0x14,0 if static else bits(.5))
  # Existing owned poly contents must be freed/reset before either rebuild.
  old=o.alloc(0x20);verts=o.alloc(12);edges=o.alloc(0x78)
  o.write_u32s(d+0x20,[1,verts,1,1,old,7]);o.write_u32(old+0x1c,edges)
  o.write_u32(d+0xa4,0x12345678);o.write_u32(d+0xa8,0x87654321)
  count=0 if i>=162 else (1 if index==0 else 2)
  for n in range(count):
   child,_,cd=make_entity(o,child_kind,index^1,n==0,[rng.uniform(-5,5) for _ in range(3)])
   o.write_u32(child+0x28,o.read_u32(e+0x24));o.write_u32(e+0x24,child)
   if child_kind==6 and index==0:
    o.write_u32(cd+0x20,o.read_u32(cd+0x9c));o.write_u32(cd+0x24,o.read_u32(cd+0xa0));o.write_u32(cd+0x9c,0)
   if n==1 and i%5==0:o.write_u32(child+0x58,0x20)
  center=o.alloc(12);o.write_f32s(center,[.25,-.5,.75])
  before=image(o);stack=o.read_u32s(STACK_TOP-0x1000,0x1000//4)
  result=o.call(0x498f50,[e,kind,center],fpu_cw=CW_D3D9)
  rows.append(dict(kind=kind,child_kind=child_kind,index=index,static=static,entity=e,center=center,
   before=before,stack=stack,before_heap=HEAP_BASE+len(before)*4,after=image(o),heap=o.heap_ptr,fpu_cw=result.fpcw))
 return dict(schema_version=1,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
  producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),fpu_cw=CW_D3D9,scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
