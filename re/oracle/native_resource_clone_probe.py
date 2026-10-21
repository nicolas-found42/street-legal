#!/usr/bin/env python3
"""545230 typee cloning. ID/factory/5447d0 are declared live collaborators;
original54a830 range,54a190 virtual copy,537d80 registration all execute.
"""
import json
from oracle import Oracle,CW_D3D9

def run():
 rows=[]
 for count in [0,1,3]:
  for named,flags in [(named,flags)for named in [False,True]for flags in [0,0x100]]:
   o=Oracle();o.hook_alloc();manager=o.alloc(0xda0);source=o.alloc(0x140);target=o.alloc(0x140);parent=o.alloc(0x80);resources=[o.alloc(0x90)for _ in range(4)]
   o.write_u32(0x618d48,manager);o.write_u32(0x6200a4,123)
   for kind in range(22):o.call(0x538520,convention='thiscall',this=manager+0x140+kind*0x90)
   for raw,id in [(source,0x50001),(target,0xffff0001)]:
    descriptor=o.scratch(24);o.write_u32s(descriptor,[14,id,0,0,0,0]);o.call(0x54a5b0,[descriptor],convention='thiscall',this=raw,fpu_cw=CW_D3D9);o.write_u32(raw+0x14,parent)
   o.write_u32(parent+0x50,0x50000)
   for i,raw in enumerate(resources):o.write_u32(raw+0x50,0x50002+i)
   o.call(0x429060,[resources[0]],convention='thiscall',this=source+0x8c)
   if count:
    array=o.alloc(count*16+4);o.write_u32(array,count);o.write_u32(source+0xa4,array+4);o.write_u32(source+0xa0,count)
    for i in range(count):o.call(0x429060,[resources[i+1]],convention='thiscall',this=array+4+i*16)
   if named:
    name=o.alloc(32);o.write(name,b'typee real clone name\0');o.write_u32(source+0x60,name)
   o.write_u32(source+0xc4,0x3e123456);o.write_u32(source+0xc8,0x3f234567)
   o.call(0x544b90,[1],convention='thiscall',this=source);o.call(0x544b90,[0],convention='thiscall',this=target);o.write_u32(source+0x68,0x3f712345)
   o.write_u32(target+0x54,o.read_u32(target+0x54)|flags)
   spatial=[]
   def request(oc,a):
    raw=a[0]
    spatial.append(dict(raw=raw,args=list(a[1:]),state=oc.read_u32(raw+0x64),quality=oc.read_u32(raw+0x68)));return 0
   o.hook(0x537790,request,'thiscall',4)
   before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
   o.hook(0x536ed0,lambda oc,a:0xffff0001,'thiscall',0)
   o.hook(0x536900,lambda oc,a:target,'thiscall',6)
   # The actual factory collaborators supplied alreadyconstructed live resources.
   o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
   result=o.call(0x545230,convention='thiscall',this=source,fpu_cw=CW_D3D9);assert result.eax==target
   rows.append(dict(count=count,named=named,flags=flags,spatial=spatial,source=source,target=target,before=before,after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(rows=rows)
if __name__=='__main__':print(json.dumps(run()))
