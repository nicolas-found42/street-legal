#!/usr/bin/env python3
"""Original 46eae0 base Part construction, declared no-physics/no-render template path."""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def run():
 rows=[]
 for count in [0,1,3]:
  for parameters in ['0 0 0 0 0 0 0','1.25 -2.5 10 0 0 0 7','1.25,-2.5,10,0,0,0,7','1.25,-2.5,10,0.1,-0.157,0.2,7']:
   o=Oracle();o.hook_alloc()
   k=o.alloc(0x2e98);parent=o.alloc(0x120);arg=o.alloc(64);obj=o.alloc(0x212c)
   o.write(arg,parameters.encode()+b'\0');o.write_u32(k+0x120,1);o.write_u32(k+0xff0,0xffffffff)
   o.write_u32(k+0xff8,count);o.write_u32(k+0x11dc,0x40000000);o.write_u32(k+0x11e0,0x12345678)
   o.write_u32(parent+8,0xffff1234)
   o.call(0x46eae0,[parent,arg,obj],convention='thiscall',this=k,fpu_cw=CW_D3D9)
   rows.append(dict(parameters=parameters,count=count,k=k,parent=parent,arg=arg,object=obj,words=o.read_u32s(obj,0x120//4),render_words=o.read_u32s(o.read_u32(obj+0x4c)-4,count*4+1),heap=o.heap_ptr))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
