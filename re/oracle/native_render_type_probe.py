#!/usr/bin/env python3
"""Original549720 full heap with declared package-manager/weak-resource bindings.
Original descriptor scanning, scalar stores and texture-array construction run.
"""
import json
from oracle import Oracle,CW_D3D9

def run():
 rows=[]
 for text in ['shd_texture 5\nshd_vbase 1.2345 -2.1 0.13\nshd_vup 1.2345 -2.1 0.13\nshd_radius 1.2345\nshd_uv 0.1 0.2 0.3 0.4\nshd_center 1.1 2.2 -3.3\nshd_diru 2.1 -1.5 0.1\nshd_dirv 4.3 -0.33 2.14\n','shd_vup 0.0 0.0 0.0\n','', 'mesh 1\r\nnumbones 7\r\nflags 0xabc\r\nlod_bias 1.2345\r\nlod_amp -0.43\r\n', 'mesh 1\ntexture 2\ntexture 3\ntexture 4\nflags 17\n', 'unknown 7\nFlags 200\nnumbones -1\nflags -5\nlod_amp 0.1\n']:
  o=Oracle();o.hook_alloc();body=o.alloc(0xc0);resources=[o.alloc(0x90)for _ in range(5)]
  o.call(0x549580,convention='thiscall',this=body,fpu_cw=CW_D3D9)
  for i,p in enumerate(resources):o.write_u32(p+0x50,0x50001+i)
  before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4);calls=[]
  o.hook(0x5383f0,lambda oc,a:0x620000,'thiscall',1)
  o.hook(0x544590,lambda oc,a:a[0]|0x50000,'thiscall',1)
  def bind(oc,a):
   slot=oc.reg('ecx');calls.append([slot]+a);raw=resources[(a[0]&0xffff)-1];old=oc.read_u32(raw+0x48)
   if old:oc.write_u32(old,slot)
   oc.write_u32s(slot,[0,old,a[0],raw]);oc.write_u32(raw+0x48,slot);return 0
  o.hook(0x546070,bind,'thiscall',3)
  source=o.scratch(128);o.write_u32(source+0x50,0x50001);buf=o.scratch(len(text)+1);o.write(buf,text.encode()+b'\0')
  o.call(0x549720,[source,buf,len(text)],convention='thiscall',this=body,fpu_cw=CW_D3D9)
  rows.append(dict(text=text,before=before,calls=calls,after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 clones=[]
 for count in [0,1,3]:
  for shadow in [False,True]:
   o=Oracle();o.hook_alloc();source=o.alloc(0xc0);target=o.alloc(0xc0);resources=[o.alloc(0x90)for _ in range(5)]
   for body in [source,target]:o.call(0x549580,convention='thiscall',this=body,fpu_cw=CW_D3D9)
   for i,raw in enumerate(resources):o.write_u32(raw+0x50,0x50001+i)
   o.call(0x429060,[resources[0]],convention='thiscall',this=source+0xc)
   o.write_u32(source+0x1c,7);o.write_u32(source+0x34,0x88776655);o.write_u32(source+0x38,0x11223344)
   o.write_u32s(source+0x44,[0x3fa12345,0xbf345678])
   o.write_u32s(source+0x5c,[0x3f012345+i for i in range(21)])
   if count:
    array=o.alloc(count*16+4);o.write_u32(array,count);o.write_u32(source+0x24,array+4);o.write_u32(source+0x20,count)
    for i in range(count):o.call(0x429060,[resources[i+1]],convention='thiscall',this=array+4+i*16)
   if shadow:o.call(0x429060,[resources[4]],convention='thiscall',this=source+0x4c)
   before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
   result=o.call(0x54a190,[target],convention='thiscall',this=source,fpu_cw=CW_D3D9);assert result.eax==1
   clones.append(dict(count=count,shadow=shadow,before=before,after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(rows=rows,clones=clones)
if __name__=='__main__':print(json.dumps(run()))
