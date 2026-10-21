#!/usr/bin/env python3
"""Original505960 synchronous mesh lifecycle and505cd0 retry pass.
Read and501b90 decoder are declared collaborators;501a70 queue transition executes.
"""
import itertools,json
from oracle import Oracle
from unicorn.x86_const import UC_X86_REG_ECX

def snapshot(o):
 return dict(heap=o.heap_ptr,words=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000+3)//4),queues={hex(p):o.read_u32s(p,7)for p in [0x65bd58,0x65bd78,0x65c198,0x65c1b8,0x65c1e0]},globals={hex(p):o.read_u32(p)for p in [0x65c1d8,0x65c200,0x65c204,0x65c208,0x65c20c,0x618838]})
def run():
 rows=[]
 for kind,skinned,slot in itertools.product([3,4],[0,1],[0,3]):
  o=Oracle();o.hook_alloc();source=o.alloc(64);o.write(source,b'cars/example/mesh.scx\0');data=o.alloc(16);o.write(data,b'INV\x4f'+bytes(12));calls=[]
  for p in [0x505460,0x505360,0x505560,0x505660,0x505760]:o.call(p)
  def read(oc,a):calls.append(['read',a[1]]);oc.write_u32(a[1],data);return 16
  def decode(oc,a):
   raw=oc.uc.reg_read(UC_X86_REG_ECX);calls.append(['decode',raw]);oc.write_u32(raw+0x20,(oc.read_u32(raw+0x20)&~2)|0x14);return 1
  def free(oc,a):calls.append(['free',a[0]]);return 0
  o.hook(0x54c300,read,'cdecl',2);o.hook(0x501b90,decode,'thiscall',0);o.hook(0x503b00,decode,'thiscall',0);o.hook(0x54f5b0,free,'cdecl',1)
  before=snapshot(o);shape=o.call(0x505960,[kind,0,source,skinned,0xffff1234,slot]).eax
  rows.append(dict(kind=kind,skinned=skinned,slot=slot,shape=shape,data=data,before=before,after=snapshot(o),calls=calls))
 retries=[]
 for handle in [42,-5,-1]:
  o=Oracle();o.hook_alloc();source=o.alloc(64);o.write(source,b'cars/example/mesh.scx\0');calls=[]
  for p in [0x505460,0x505360,0x505560]:o.call(p)
  o.hook(0x5504c0,lambda oc,a:0,'thiscall',1);o.hook(0x5504f0,lambda oc,a:0,'thiscall',0);o.hook(0x54d290,lambda oc,a:-5,'cdecl',5)
  shapes=[o.call(0x505960,[3,1,source,0,0xffff1234,0]).eax for _ in range(2)]
  def request(oc,a):calls.append(a[1:]);return handle
  o.hook(0x54d290,request,'cdecl',5);before=snapshot(o);result=o.call(0x505cd0).eax
  retries.append(dict(handle=handle,before=before,after=snapshot(o),calls=calls,result=result))
 return dict(rows=rows,retries=retries)
if __name__=='__main__':print(json.dumps(run()))
