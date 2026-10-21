#!/usr/bin/env python3
"""Actual54a450 mesh/texture replacement and52d2a0 empty-listener notification."""
import json
from oracle import Oracle,CW_D3D9

def run():
 rows=[]
 for profile,index,clear,listener_mode in [(p,i,c,l)for p,i,c in [('mesh',0,False),('texture',2,False),('missing',4,False),('clear',1,True)]for l in ['empty','unmatched','matched-null']]:
  o=Oracle();o.hook_alloc();body=o.alloc(0xc0);resources=[o.alloc(0x90)for _ in range(6)];array=o.alloc(3*16);old=o.alloc(16);new=o.alloc(16)
  o.call(0x549580,convention='thiscall',this=body,fpu_cw=CW_D3D9)
  for i,raw in enumerate(resources):o.write_u32(raw+0x50,0x50001+i)
  o.call(0x429060,[resources[0]],convention='thiscall',this=body+0xc);o.call(0x429060,[resources[5]],convention='thiscall',this=body+0xb0)
  o.write_u32(body+0x20,3);o.write_u32(body+0x24,array)
  for i in range(3):o.call(0x429060,[resources[i+1]],convention='thiscall',this=array+i*16)
  o.call(0x429060,[resources[index]],convention='thiscall',this=old)
  if not clear:o.call(0x429060,[resources[4]],convention='thiscall',this=new)
  listener=o.alloc(4);owner_id=o.read_u32(body+0xb8);entry=0x65c690+(owner_id&31)*12
  if listener_mode!='empty':o.write_u32s(entry,[owner_id if listener_mode=='matched-null' else owner_id+32,listener,0])
  globals_before={str(a):o.read_u32(a)for a in list(range(0x65c690,0x65c810,4))+[0x65c838,0x65c870]}
  before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
  result=o.call(0x54a450,[old,new],convention='thiscall',this=body,fpu_cw=CW_D3D9)
  rows.append(dict(profile=profile+listener_mode,globals_before=globals_before,globals_after={a:o.read_u32(int(a))for a in globals_before},body=body,old=old,new=new,before=before,result=bool(result.eax),after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(rows=rows)
if __name__=='__main__':print(json.dumps(run()))
