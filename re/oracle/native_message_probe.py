#!/usr/bin/env python3
"""Original car45c510 query7 category and unchanged optional argument, no hooks."""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def run():
 rows=[]
 for id in [5,0x1a000006,0xffff0080]:
  for output in [False,True]:
   o=Oracle();o.hook_alloc();k=o.alloc(0x2e98);obj=o.alloc(0x212c);resource=o.alloc(0x100);ref=o.alloc(16)
   o.write_u32(k,0x5f0f18);o.write_u32(k+0xc,id);o.write_u32(k+0x10,resource);o.write_u32(resource+0x50,0xffff0080)
   result=o.call(0x45c510,[obj,7,ref if output else 0],convention='thiscall',this=k,fpu_cw=CW_D3D9)
   rows.append(dict(id=id,output=output,result=result.eax,words=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
