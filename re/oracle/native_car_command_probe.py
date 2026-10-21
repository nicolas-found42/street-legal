#!/usr/bin/env python3
"""Actual458c00 message16 add_mslot, complete allocator/transform/list buffers."""
import json
from oracle import Oracle,CW_D3D9

def run():
 rows=[]
 for count in [1,3]:
  o=Oracle();o.hook_alloc()
  k=o.alloc(0x2e98);obj=o.alloc(0x212c);slots=o.alloc(0x68*3);descriptors=o.alloc(0x100*3)
  for i in range(3):
   o.write_u32s(slots+i*0x68+0x2c,[0x3f112233+i,0x40123456+i,0xbf135790+i]+[0x3e123456+j*157+i for j in range(12)])
  o.write_u32s(obj+0x2098,[0x5f09a8,obj+0x20a4,0,0x5f09ac,0,obj+0x2098])
  before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
  for i in range(count):
   text=o.scratch(96);o.write(text,(f'add_mslot {slots+i*0x68} {descriptors+i*0x100}').encode()+b'\0')
   # Text is caller-owned ABI storage, kept outside constructor heap comparison.
   result=o.call(0x458c00,[obj,0,16,text],convention='thiscall',this=k,fpu_cw=CW_D3D9)
   assert result.eax==1
  rows.append(dict(count=count,before=before,after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(rows=rows)
if __name__=='__main__':print(json.dumps(run()))
