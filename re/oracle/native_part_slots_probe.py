#!/usr/bin/env python3
"""Original46eae0 slot query/command orchestration with declared virtual handlers.
The original constructor allocates slots and chooses dispatch; only classquery
and command bodies are collaborators, with their actual arguments recorded.
"""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from unicorn import x86_const as X

def run():
 rows=[]
 for count in [0,1,3]:
  for result in [0,5,9]:
   o=Oracle();o.hook_alloc();k=o.alloc(0x2e98);parent=o.alloc(0x120);arg=o.alloc(64);obj=o.alloc(0x212c);owner=o.alloc(0x80);vt=0x30000000
   o.write(arg,b'0,0,0,0,0,0,0\0');o.write_u32(k,vt);o.write_u32(k+0x120,1);o.write_u32(k+0xff0,0xffffffff);o.write_u32(k+0x11dc,0x3f800000)
   o.write_u32(owner,vt);o.write_u32(owner+0x50,5);o.write_u32(parent+12,owner);o.write_u32(parent+8,5)
   o.write_u32(vt+12,0x300f0000);o.write_u32(vt+0x3c,0x300f0010);o.write_u32(vt+0x38,0x300f0020)
   events=[]
   o.hook(0x300f0000,lambda oc,a:obj,'thiscall',1)
   def query(oc,a):events.append(dict(kind='query',this=oc.uc.reg_read(X.UC_X86_REG_ECX),args=a));return result
   def command(oc,a):events.append(dict(kind='command',this=oc.uc.reg_read(X.UC_X86_REG_ECX),args=a[:3],text=oc.read(a[3],128).split(b'\0')[0].decode()));return 0
   o.hook(0x300f0010,query,'thiscall',3);o.hook(0x300f0020,command,'thiscall',4)
   descriptors=[o.alloc(0xd0) for _ in range(count+1)]
   for index,p in enumerate(descriptors[:-1]):o.write_u32(p+4,descriptors[index+1]);o.write_u32(p+0xc4,index%2+1);o.write_u32(p+0xc,10+index)
   o.write_u32(k+0x1150,count);o.write_u32(k+0x115c,descriptors[0]);o.write_u32(0x76639c,0)
   o.call(0x46eae0,[parent,arg,obj],convention='thiscall',this=k,fpu_cw=CW_D3D9)
   rows.append(dict(count=count,result=result,k=k,object=obj,descriptors=descriptors,slots=o.read_u32(obj+0x78),events=events,flags=o.read_u32(obj+0x70)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
