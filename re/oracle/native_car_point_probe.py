#!/usr/bin/env python3
"""Original4a6f30 local-to-world point projection; no callee stubs."""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def run():
 rows=[]
 for active in [0,1]:
  for matrix in [[1.,0.,0.,0.,1.,0.,0.,0.,1.],[.9,.2,-.3,.4,1.1,.6,-.7,.8,1.3]]:
   for local in [[0.,0.,0.],[.11,-.13,.17],[2.3,-4.1,6.7]]:
    o=Oracle();entity=o.alloc(0x218);point=o.alloc(16);o.write_u32(entity+0x20c,active);state=entity+0x64+active*0xd4
    o.write_f32s(state,[1.25,-2.5,10.]);o.write_f32s(point,local)
    for i,v in enumerate(matrix):o.write_u32(state+0x40+(i//3)*16+(i%3)*4,bits(v))
    o.call(0x4a6f30,[point,point],convention='thiscall',this=entity,fpu_cw=CW_D3D9)
    rows.append(dict(active=active,matrix=list(map(bits,matrix)),local=list(map(bits,local)),position=list(map(bits,[1.25,-2.5,10.])),result=o.read_u32s(point,3)))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
