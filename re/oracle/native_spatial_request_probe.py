#!/usr/bin/env python3
"""Original537790 request scheduling and physical request53fce0.

Pool baseline executes original535a70 constructor and original535e70's
535f2b..535f57 link block (ESI=manager), stopping before unrelated startup.
Requests execute all original code, including physics virtual callbacks.
No resource output is mocked; record+c/+10 retain the final two request args.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE

def words(o,start,end):
 return [[p,o.read_u32(p)] for p in range(start,end,4) if o.read_u32(p)]

def setup():
 o=Oracle();o.hook_alloc();manager=o.alloc(0x106ee4)
 o.call(0x535a70,[manager],convention='fastcall',fpu_cw=CW_D3D9,limit=10000000)
 o.hook(0x535f57,lambda oc,a:None)
 o.call(0x535f2b,regs={'esi':manager},limit=1000000)
 o.write_u32(0x618d48,manager)
 return o,manager

def run():
 o,manager=setup()
 baseline=words(o,manager+0xde8,manager+0xf0e20)
 rows=[]
 for mode in ['empty','nonempty','already-queued','repeat','two-resources','exhausted','retained-old-reference']:
  o,manager=setup();resources=[]
  for i in range(2):
   r=o.alloc(0x164);d=o.alloc(24);o.write_u32s(d,[2,0xffff1000+i,0x201,0,0,0])
   o.call(0x53f8b0,[d,0,0],convention='thiscall',this=r,fpu_cw=CW_D3D9)
   resources.append(r)
  if mode!='empty':
   for r in resources:
    n=o.alloc(12);o.write_u32(n+4,r+0x150);o.write_u32(r+0x148,n)
  if mode=='already-queued':o.write_u32(resources[0]+0x54,o.read_u32(resources[0]+0x54)|0x4000000)
  if mode=='exhausted':o.write_u32(manager+0xe0c,manager+0xe14);o.write_u32(manager+0xe1c,manager+0xe08)
  if mode=='retained-old-reference':o.call(0x429060,[resources[1]],convention='thiscall',this=manager+0xe20+0x18,fpu_cw=CW_D3D9)
  ranges=[(manager+0xde8,manager+0xe20+0x78),(manager+0xf0de4,manager+0xf0e20),(manager+0x106ee4,o.heap_ptr)]
  before=sum((words(o,a,b)for a,b in ranges),[])
  sequence=[resources[0]]
  if mode=='repeat':sequence.append(resources[0])
  if mode=='two-resources':sequence.append(resources[1])
  for r in sequence:o.call(0x537790,[r,0x3dcccccd,0,0x3f800000],convention='thiscall',this=manager,fpu_cw=CW_D3D9)
  rows.append(dict(mode=mode,manager=manager,heap=o.heap_ptr,resources=resources,sequence=sequence,ranges=ranges,input_words=before,output_words=sum((words(o,a,b)for a,b in ranges),[])))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),manager=manager,pool_words=baseline,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
