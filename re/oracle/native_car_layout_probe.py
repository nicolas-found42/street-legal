#!/usr/bin/env python3
"""Original44b486..44b512 auxiliary camera nodes and44b512..44b79d layout.

All weak-link operations execute original code. Inputs are declared actual raw
resource containers and configured slot descriptors; no external callee hooks
except allocation are needed. These are constructor slices, not car acceptance.
"""
import hashlib,json
from pathlib import Path
from unicorn import UC_HOOK_CODE
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,SENTINEL,HEAP_BASE

def run_slice(o,start,stop,registers,stack=None):
 def boundary(uc,address,size,user):
  if address==start:
   for name,value in registers.items():o.set_reg(name,value)
   for offset,value in (stack or {}).items():o.write_u32(o.reg('esp')+offset,value)
  elif address==stop:o.set_reg('eip',SENTINEL)
 h=o.uc.hook_add(UC_HOOK_CODE,boundary)
 o.call(start,[],fpu_cw=CW_D3D9)
 o.uc.hook_del(h)
def words(o):return o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4)
def run():
 cameras=[]
 for count in [0,1,16]:
  o=Oracle();o.hook_alloc();template=o.alloc(0x2e98);body=o.alloc(0x212c);entity=o.alloc(0x218);resource=o.alloc(0x100)
  run_slice(o,0x44a288,0x44a3fd,dict(esi=body,ebx=0))
  o.write_u32(template+0x2bc4,count);o.write_u32(resource+0x84,entity)
  o.write_u32(body+0x13bc,entity);o.write_u32(entity,body+0x13bc);o.write_u32(entity+4,1)
  start=o.heap_ptr;before=words(o)
  run_slice(o,0x44b486,0x44b512,dict(eax=resource,edi=template,ebp=body,ebx=0),{0x10:template})
  cameras.append(dict(count=count,template=template,body=body,entity=entity,start=start,end=o.heap_ptr,before=before,after=words(o)))
 layouts=[]
 for count in [0,5]:
  o=Oracle();o.hook_alloc();template=o.alloc(0x2e98);body=o.alloc(0x212c);entity=o.alloc(0x218);wheels=o.alloc(0x2b4*4);resources=[o.alloc(0x200) for _ in range(5)]
  runtime=o.alloc(max(1,count)*0x68);descriptors=[o.alloc(0x60) for _ in range(count+1)]
  o.write_u32(body+0xc,resources[0]);o.write_u32(body+0x13bc,entity);o.write_u32(body+0x13e4,wheels);o.write_u32(body+0x78,runtime)
  o.write_f32s(entity+0x64,[1.25,-2.5,10.]);o.write_u32(template+0x1f40,4);o.write_u32(template+0x1150,count);o.write_u32(template+0x115c,descriptors[0])
  for i,r in enumerate(resources):o.write_u32(r+0x50,0xffff0050+i)
  o.call(0x429060,[resources[1]],convention='thiscall',this=body+0x2c)
  for offset,r in [(0,resources[1]),(0x10,resources[2]),(0x20,resources[3])]:o.call(0x429060,[r],convention='thiscall',this=wheels+offset)
  for i,kind in enumerate([0,11,101,201,301][:count]):
   d=descriptors[i];o.write_u32(d+4,descriptors[i+1]);o.write_u32(d+0x4c,kind)
   for offset in range(0xc,0x48,4):o.write_u32(d+offset,0x3e000000+i*0x10000+offset*15)
   o.call(0x429060,[resources[4]],convention='thiscall',this=runtime+i*0x68+0x18)
  start=o.heap_ptr;before=words(o)
  run_slice(o,0x44b512,0x44b79d,dict(ebp=body,edi=template,ebx=0),{0x10:template})
  layouts.append(dict(count=count,template=template,body=body,start=start,end=o.heap_ptr,before=before,after=words(o)))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,cameras=cameras,layouts=layouts)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
