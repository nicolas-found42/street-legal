#!/usr/bin/env python3
"""Original54d300/555470 range allocation, queue publication and return value."""
import hashlib,itertools,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE
BASE=0x76aa30

def run():
 rows=[]
 for active,free,length in itertools.product([0,1],[-1,0,63],[0,24]):
  o=Oracle();o.hook_alloc();o.hook(0x54f6e0,lambda oc,a:oc.alloc(a[0]) if a[0]>0 else 0,'cdecl',1)
  text='cars/example.rpk';path=o.alloc(256);o.write(path,text.encode()+b'\0')
  for i in range(64):o.write_u32(BASE+i*0x11c,1)
  if free>=0:o.write_u32(BASE+free*0x11c,0)
  o.write_u32(0x76f980,active);o.write_u32(0x61ac3c,123);calls=[]
  o.hook(0x559880,lambda oc,a:calls.append(a[0]) or 0,'cdecl',1)
  before=o.read_u32s(BASE,64*0x11c//4);heap_before=o.heap_ptr
  result=o.call(0x54d300,[path,0,1234,length,0x544540,0x20010000,7]).eax
  rows.append(dict(active=active,free=free,length=length,text=text,input=before,heap_before=heap_before,output=o.read_u32s(BASE,64*0x11c//4),result=result,calls=calls,fullcount=o.read_u32(0x76f998),heap=o.heap_ptr))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
