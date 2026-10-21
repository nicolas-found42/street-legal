#!/usr/bin/env python3
"""Original43e3e0 complete parameter clone with original weak-link operations.

Only allocation and lazy resource package lookup/bind use controlled Hooks.
Curve arrays, all copied scalars, sound tables, every weak link, collision records
and global cache guards execute original machine code. No captured bytes supply
runtime behavior.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
from cfg_handler_vectors import Hooks
REFS=[0x7d4,0x7e4,0x7f4,0x814,0x824,0x834,0x844,0x854,0x864,0x894,0x8a4,0x884,0x8b4,0x8c4,0x8dc,0x8ec]
CACHES=[0x63c680,0x63c6a0,0x63c788]
def run():
 rows=[]
 for count in [0,1,3]:
  for cached in [False,True]:
   o=Oracle();hooks=Hooks(o)
   source=o.alloc(0xc1c);curves=[o.alloc(40),o.alloc(40)];targets=[o.alloc(0x100),o.alloc(0x100)]
   for i in range(0xc1c//4):o.write_u32(source+i*4,(0x3e800000+i*0x1001)&0xffffffff)
   for i,target in enumerate(targets):o.write_u32(target+0x50,0xabcdef01+i)
   for table,co in [(0xf8,0x338),(0x340,0x580),(0x588,0x7c8)]:
    o.write_u32(source+co,count)
    for index in range(16):o.write(source+table+index*0x24,bytes(16))
    for index in range(count):o.call(0x429060,[targets[index%2]],convention='thiscall',this=source+table+index*0x24)
   for i,offset in enumerate(REFS):
    o.write(source+offset,bytes(16))
    o.call(0x429060,[targets[i%2]],convention='thiscall',this=source+offset)
   for index,offset in enumerate([0,8]):
    o.write_u32(source+offset,count);o.write_u32(source+offset+4,curves[index])
    for i in range(10):o.write_u32(curves[index]+i*4,0x3f000000+i*0x12345+index)
   o.write_u32(source+0xa54,count)
   for slot in CACHES:o.write(slot,bytes(16));o.write_u32(slot+8,int(cached))
   start=o.heap_ptr
   before=o.read_u32s(HEAP_BASE,(start-HEAP_BASE)//4)
   hooks.log.clear()
   result=o.call(0x43e3e0,[],convention='thiscall',this=source,fpu_cw=CW_D3D9).eax
   rows.append(dict(count=count,cached=cached,source=source,start=start,end=o.heap_ptr,result=result,before=before,after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4),calls=hooks.log,cache_after=[[slot,o.read_u32s(slot,4)] for slot in CACHES]))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
