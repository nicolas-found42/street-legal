#!/usr/bin/env python3
"""Original48a8d0/48a8f0 deferred release routing and4277f0 dispatch.
The queue allocation, weak binding, traversal and destruction execute original
x86. Resource unload/destructor virtuals are declared recording callbacks; this
probe determines dispatch selection, not resource destructor fidelity.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE

def run():
 rows=[]
 for mode in ['destroy','unload']:
  o=Oracle();o.hook_alloc();game=0x636338
  for offset in [0x64,0x80,0xb8]:
   o.write_u32(game+offset-8,game+offset)
   o.write_u32(game+offset+8,game+offset-12)
  vt=o.alloc(0x20);resource=o.alloc(0xd8);handle=o.alloc(16);events=[]
  o.write_u32(vt,0x400100);o.write_u32(vt+0x1c,0x400110)
  o.write_u32(resource,vt);o.write_u32(resource+0x4c,3);o.write_u32(resource+0x50,0xffff1234)
  for address,name in [(0x400100,'destruct'),(0x400110,'unload')]:
   def record(oc,args,name=name):events.append([name,oc.reg('ecx'),args]);return 0
   o.hook(address,record,'thiscall',1)
  o.call(0x429060,[resource],convention='thiscall',this=handle)
  o.call(0x48a8d0 if mode=='destroy' else 0x48a8f0,convention='thiscall',this=handle)
  queued=[o.read_u32(game+0xd0),o.read_u32(game+0xd4)]
  o.call(0x4277f0,convention='thiscall',this=game)
  rows.append(dict(mode=mode,resource=resource,queued=queued,events=events,remaining=[o.read_u32(game+0xd0),o.read_u32(game+0xd4)]))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
