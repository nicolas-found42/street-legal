#!/usr/bin/env python3
"""Original54d290/5553b0 request handle, publication fields and semaphore signal."""
import json,itertools,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE
BASE=0x76aa30;SIZE=0x4700

def run():
 rows=[]
 for active,fill,free in itertools.product([0,1],[0,0xa5],[-1,0,3,63]):
  o=Oracle();o.hook_alloc();path=o.alloc(512);text='cars/example/mesh.scx';o.write(path,text.encode()+b'\0');o.write(BASE,bytes([fill])*SIZE)
  for i in range(64):o.write_u32(BASE+i*0x11c,1)
  if free>=0:o.write_u32(BASE+free*0x11c,0)
  o.write_u32(0x76f980,active);o.write_u32(0x61ac3c,123);o.write_u32(0x76f998,7);calls=[];o.hook(0x559880,lambda oc,a:calls.append(a[0]),'cdecl',1)
  before=o.read_u32s(BASE,SIZE//4);result=o.call(0x54d290,[path,0x30001000,0x505c00,0x20001234,8]).eax
  rows.append(dict(active=active,fill=fill,free=free,text=text,input=before,output=o.read_u32s(BASE,SIZE//4),result=result,calls=calls,fullcount=o.read_u32(0x76f998)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
