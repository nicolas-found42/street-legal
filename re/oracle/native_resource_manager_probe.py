#!/usr/bin/env python3
"""Original537240 asynchronous descriptor ownership/list/file publication.

Package availability, resource virtuals and5444a0 file dispatch are declared
collaborators. Queue mutations, reference ownership, priority and returns run
unhooked; memory snapshots include resource, sentinels and request records.
"""
import hashlib,itertools,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE

def sync_rows():
 rows=[]
 for pending,complete,failed in [(False,False,False),(True,False,False),(True,True,False),(True,True,True)]:
  o=Oracle();o.hook_alloc();manager=o.alloc(0x106ee8);raw=o.alloc(0x160);record=manager+0xf0e20
  for off in [0xdb0,0xdcc]:o.call(0x429350,convention='thiscall',this=manager+off)
  o.call(0x538640,convention='thiscall',this=record)
  head=manager+(0xdb4 if pending else 0xdd0);tail=head+12
  o.write_u32(head+4,record);o.write_u32(record+4,tail);o.write_u32(record+8,head);o.write_u32(tail+8,record)
  o.write_u32(manager+0xffe20,2);o.write_u32(0x618d48,manager)
  for off,v in [(0,0x30000000),(0x4c,14),(0x50,0x10053),(0x54,0x2000000 if pending else 0),(0x5c,24),(0x64,64),(0x6c,0x3f800000)]:o.write_u32(raw+off,v)
  for off,v in [(0x18,0),(0x1c,0),(0x20,0x10053 if pending else 0),(0x24,raw if pending else 0),(0x28,0x80000000|(1 if complete else 0)|(4 if failed else 0)),(0x2c,0 if complete else 1),(0x30,0x30003000 if complete else 0),(0x34,24),(0x38,manager+0xffe94+0x70)]:o.write_u32(record+off,v)
  if pending:o.write_u32(raw+0x48,record+0x18)
  calls=[]
  o.write_u32(0x30000028,0x30000120)
  o.hook(0x30000120,lambda oc,a:calls.append(['load',oc.reg('ecx'),*a,oc.read_u32(raw+0x6c)]) or 0,'thiscall',2)
  o.hook(0x544170,lambda oc,a:calls.append(['package',oc.reg('ecx')]) or 1,'thiscall',0)
  o.hook(0x5443d0,lambda oc,a:calls.append(['read',oc.reg('ecx'),*a]) or 0x30002000,'thiscall',1)
  o.hook(0x544490,lambda oc,a:calls.append(['release',*a]),'cdecl',1)
  o.hook(0x544570,lambda oc,a:(calls.append(['release',oc.read_u32(a[0])]),oc.write_u32(a[0],0),0)[2],'cdecl',1)
  initial=dict(resource=o.read_u32s(raw,0x160//4),queues=o.read_u32s(manager+0xdb0,14),record=o.read_u32s(record,15))
  o.call(0x537240,[raw,1.,0.,0.],convention='thiscall',this=manager)
  rows.append(dict(pending=pending,complete=complete,failed=failed,manager=manager,raw=raw,record_address=record,input=initial,resource=o.read_u32s(raw,0x160//4),queues=o.read_u32s(manager+0xdb0,14),record=o.read_u32s(record,15),calls=calls,sync_reads=o.read_u32(0x765f1c)))
 return rows

def run():
 rows=[]
 for kind,accepted,loaded,repeat in itertools.product([5,14],[0,-5],[0,1],[False,True]):
  o=Oracle();o.hook_alloc();manager=o.alloc(0x106ee8);raw=o.alloc(0x160);record=manager+0xf0e20
  for offset in [0xdb0,0xdcc]:o.call(0x429350,convention='thiscall',this=manager+offset)
  o.call(0x538640,convention='thiscall',this=record)
  prev=manager+0xdd0;tail=manager+0xddc;o.write_u32(prev+4,record);o.write_u32(record+4,tail);o.write_u32(record+8,prev);o.write_u32(tail+8,record)
  o.write_u32(manager+0xffe20,2);o.write_u32(raw,0x30000000);o.write_u32(raw+0x4c,kind);o.write_u32(raw+0x50,0x10053);o.write_u32(raw+0x5c,24);o.write_u32(raw+0x64,loaded);o.write_u32(raw+0x68,0x3f800000 if loaded else 0)
  o.write_u32(0x3000000c,0x30000100);o.write_u32(0x30000014,0x30000110)
  calls=[]
  o.hook(0x30000100,lambda oc,a:calls.append(['payload',oc.reg('ecx'),*a]) or raw+0x80,'thiscall',1)
  o.hook(0x30000110,lambda oc,a:calls.append(['minimum',oc.reg('ecx'),*a]) or 0,'thiscall',1)
  o.hook(0x544170,lambda oc,a:calls.append(['package',oc.reg('ecx')]) or 1,'thiscall',0)
  o.hook(0x5444a0,lambda oc,a:calls.append(['file',oc.reg('ecx'),*a]) or accepted,'thiscall',2)
  initial=dict(resource=o.read_u32s(raw,0x160//4),queues=o.read_u32s(manager+0xdb0,0x38//4),record=o.read_u32s(record,15))
  o.call(0x537240,[raw,1.0,0.0,10.0],convention='thiscall',this=manager)
  if repeat:o.call(0x537240,[raw,1.0,0.0,10.0],convention='thiscall',this=manager)
  rows.append(dict(kind=kind,accepted=accepted,loaded=loaded,repeat=repeat,manager=manager,raw=raw,record_address=record,input=initial,resource=o.read_u32s(raw,0x160//4),queues=o.read_u32s(manager+0xdb0,0x38//4),record=o.read_u32s(record,15),calls=calls,weights=o.read_u32s(0x618d70,22),scale=o.read_u32(0x5f3560)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows,synchronous=sync_rows())
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
