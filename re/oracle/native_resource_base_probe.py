#!/usr/bin/env python3
"""Original551750/544620/544d40 raw resource graph and descriptor constructor."""
import json,itertools,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE

def run():
 rows=[]
 for spatial,kind,frame,fill,name in itertools.product([False,True],[1,2,3,5,6,7,8,9,10,12,13,14,18,19,20,21],[0,123],[0,0xa5],[None,'resource-name-longer-than-thirty-two-bytes-to-test-bound']):
  o=Oracle();o.hook_alloc();p=o.alloc(0x26c);d=o.alloc(24);n=o.alloc(48);b=o.alloc(28);o.write(p,bytes([fill])*0x26c)
  desc=[kind,0xffff1234,0x201,0x12345678,0xabcdef01];o.write_u32s(d,desc)
  if name is not None:o.write(n,name.encode()+b'\0');o.write_u32(d+20,n)
  bounds=[1,2,3,4,5,6,7] if spatial and fill else None
  if bounds:o.write_u32s(b,bounds)
  o.write_u32(0x6200a4,frame);o.write_u32(0x76639c,123)
  args=[d,b if bounds else 0,0] if spatial else [d]
  o.call(({1:0x53e6e0,2:0x53f8b0,3:0x543000,12:0x547f60,13:0x549010,14:0x54a5b0,5:0x53b520,8:0x546ae0,6:0x53bf40,7:0x53d060,9:0x5473d0,10:0x547650,18:0x53d990,19:0x5389e0,20:0x549430,21:0x53c3d0}.get(kind,0x544d40)) if spatial else 0x544620,args,convention='thiscall',this=p)
  rows.append(dict(spatial=spatial,descriptor=desc,frame=frame,fill=fill,name=name,bounds=bounds,p=p,heap=o.heap_ptr,words=o.read_u32s(p,0x26c//4),name_words=o.read_u32s(o.read_u32(p+0x60),8) if name else None))
 roots=[]
 for parentkind,childflags,parentroot,previous in itertools.product([1,2,3,8],[0,16],[False,True],[False,True]):
  o=Oracle();o.hook_alloc();p=o.alloc(0x164);c=o.alloc(0x164);d=o.alloc(24);old=o.alloc(0x164)
  o.write_u32s(d,[parentkind,101,0,0,0]);o.call(0x544d40,[d,0,0],convention='thiscall',this=p)
  o.write_u32s(d,[2,102,childflags,0,0]);o.call(0x544d40,[d,0,0],convention='thiscall',this=c)
  if parentroot:o.write_u32(p+0xcc,p)
  if previous:
   o.write_u32(old+0x50,103);o.write_u32s(c+0xc0,[0,0,103,old]);o.write_u32(old+0x48,c+0xc0)
  o.call(0x544fe0,[p],convention='thiscall',this=c)
  roots.append(dict(parentkind=parentkind,childflags=childflags,parentroot=parentroot,previous=previous,parent=p,child=c,old=old,words=o.read_u32s(p,(o.heap_ptr-p)//4)))
 return dict(roots=roots,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
