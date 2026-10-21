#!/usr/bin/env python3
"""Original538520 buckets and544b90→537d80/537de0 fullraw lists/state words."""
import json
from oracle import Oracle,CW_D3D9

def run():
 rows=[]
 for kind in [0,1,3,5,14,21]:
  for fill in [0,0xa5]:
   o=Oracle();o.hook_alloc();manager=o.alloc(0xda0);resources=[o.alloc(0x90)for _ in range(2)];o.write_u32(0x618d48,manager)
   o.write(manager,bytes([fill])*0xda0)
   for raw in resources:o.write_u32(raw+0x4c,kind);o.write_u32(raw+0x68,0x3f123456)
   before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
   for i in range(22):o.call(0x538520,convention='thiscall',this=manager+0x140+i*0x90)
   operations=[[0,1],[1,1],[0,0],[0,1],[1,0],[1,0],[0,0],[1,1]]
   for index,load in operations:o.call(0x544b90,[load],convention='thiscall',this=resources[index],fpu_cw=CW_D3D9)
   rows.append(dict(kind=kind,fill=fill,before=before,operations=operations,after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(rows=rows)
if __name__=='__main__':print(json.dumps(run()))
