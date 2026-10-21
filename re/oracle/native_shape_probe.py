#!/usr/bin/env python3
"""Original505960 async mesh shape graph, declared asynchronous file manager."""
import json,itertools,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE

def run():
 rows=[]
 for kind,skinned,slot,fill,handle in itertools.product([3,4],[0,1],[0,3],[0,0xa5],[42,-5]):
  o=Oracle();o.hook_alloc();source=o.alloc(64);o.write(source,b'cars/example/mesh.scx\0');o.call(0x505460);o.call(0x505360);calls=[]
  def allocate(oc,a):
   p=oc.alloc(a[0]);
   if a[0] in [0x110,0x114]:oc.write(p,bytes([fill])*a[0])
   return p
  o.hook(0x54f560,allocate,'cdecl',1)
  o.hook(0x5504c0,lambda oc,a:0,'thiscall',1);o.hook(0x5504f0,lambda oc,a:0,'thiscall',0)
  def request(oc,a):calls.append(dict(source=oc.read(a[0],256).split(b'\0')[0].decode(),args=a[1:]));return handle
  o.hook(0x54d290,request,'cdecl',5)
  result=o.call(0x505960,[kind,1,source,skinned,0xffff1234,slot]).eax
  rows.append(dict(handle=handle,retry_queue=o.read_u32s(0x65c1b8,7),retry_count=o.read_u32(0x65c200),kind=kind,skinned=skinned,slot=slot,fill=fill,shape=result,heap=o.heap_ptr,words=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000+3)//4),queue=o.read_u32s(0x65bd78,7),count=o.read_u32(0x65c204),calls=calls))
 completions=[]
 for index,condition in itertools.product([0,1],['success','negative','null','empty']):
  o=Oracle();o.hook_alloc();o.call(0x505460);o.call(0x505560)
  o.hook(0x5504c0,lambda oc,a:0,'thiscall',1);o.hook(0x5504f0,lambda oc,a:0,'thiscall',0);o.hook(0x54d290,lambda oc,a:42,'cdecl',5)
  source=o.alloc(64);o.write(source,b'cars/example/mesh.scx\0');shapes=[o.call(0x505960,[kind,1,source,0,0xffff1234,0]).eax for kind in [3,4]];data=o.alloc(32);o.write(data,bytes(range(32)));shape=shapes[index];result=-1 if condition=='negative' else 42;ptr=0 if condition=='null'else data;length=0 if condition=='empty' else 32
  before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000+3)//4);queues=o.read_u32s(0x65bd78,7)+o.read_u32s(0x65c198,7);counts=[o.read_u32(0x65c204),o.read_u32(0x65c208)]
  o.call(0x505c00,[shape,result,ptr,length])
  completions.append(dict(index=index,condition=condition,shape=shape,result=result,data=ptr,length=length,heap=o.heap_ptr,input=before,queues_before=queues,counts_before=counts,output=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000+3)//4),queues_after=o.read_u32s(0x65bd78,7)+o.read_u32s(0x65c198,7),counts_after=[o.read_u32(0x65c204),o.read_u32(0x65c208)]))
 return dict(completions=completions,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
