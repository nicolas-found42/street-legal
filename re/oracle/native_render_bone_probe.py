#!/usr/bin/env python3
"""Original48bc40/48c0f0 named-bone lookup and matrix/owner changes.

Only resource minimum, prepare and payload are declared collaborators. Bone
allocation, intrusive lists, matrix construction and controller replacement run
as original instructions. Every retained heap word and callback order is kept.
"""
import hashlib,itertools,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE


def run():
 rows=[]
 for kind,mode,pose,flags in itertools.product([1,3],['new','same','replace','null','missing','blocked','unavailable','blocked-physics','unavailable-physics'],['matrix','null'],[0,0x201,0x4201]):
  o=Oracle();o.hook_alloc();o.write_u32(0x617650,0x6409c4);o.write_u32(0x6200a4,123)
  reference=o.alloc(16);resource=o.alloc(0x100);body=o.alloc(0x190)
  owners=[];bodies=[]
  for i in range(2):
   ref=o.alloc(16);res=o.alloc(0x100);payload=o.alloc(0x90)
   owners.append(ref);bodies.append(payload)
   o.write_u32(res,0x30000000);o.write_u32(res+0x4c,1);o.write_u32(res+0x50,0xffff0100+i)
   o.call(0x429060,[res],convention='thiscall',this=ref)
   o.write_u32(payload+0x80,payload+0x6c);o.write_u32(payload+0x70,payload+0x78)
  pos=o.alloc(12);rot=o.alloc(48);name=o.alloc(16);o.write(name,b'bone00\0')
  o.write_f32s(pos,[1.25,-2.5,10.]);o.write_f32s(rot,[.87,.21,-.33,0,-.11,1.,.13,0,.44,.17,.92,0])
  o.call(0x5404d0,[],convention='thiscall',this=body,fpu_cw=CW_D3D9)
  o.write_u32(resource,0x30000000);o.write_u32(resource+0x4c,kind);o.write_u32(resource+0x50,0xffff0200);o.write_u32(resource+0x54,flags)
  o.call(0x429060,[resource],convention='thiscall',this=reference)
  o.write_u32(0x3000000c,0x300f0000);o.write_u32(0x30000014,0x300f0010)
  events=[];active=False
  resources={resource:body,**{o.read_u32(ref+12):payload for ref,payload in zip(owners,bodies)}}
  def state():
   bone=o.read_u32(body+0x12c)
   return [o.read_u32(bone+off) for off in [0x4c,0xac,0x54,0x84]] if o.read_u32(bone+4) else []
  def minimum(oc,args):events.append(['minimum',oc.reg('ecx'),args[0],state()])
  def prepare(oc,args):
   events.append(['prepare',oc.reg('ecx'),args[0],state()])
   return 0x80000000 if active and (mode=='blocked' or (mode=='blocked-physics' and oc.reg('ecx')!=resource)) else 0
  def payload(oc,args):
   events.append(['payload',oc.reg('ecx'),state()]);return 0 if active and (mode=='unavailable' or (mode=='unavailable-physics' and oc.reg('ecx')!=resource)) else resources[oc.reg('ecx')]
  o.hook(0x300f0000,payload,'thiscall',1);o.hook(0x300f0010,minimum,'thiscall',1);o.hook(0x5447d0,prepare,'thiscall',3)
  if mode in ['same','replace','null']:
   id=o.call(0x48bc40,[name],convention='thiscall',this=reference,fpu_cw=CW_D3D9).eax
   o.call(0x48c0f0,[id,owners[0],pos,rot],convention='thiscall',this=reference,fpu_cw=CW_D3D9)
  events.clear();active=True;start=o.heap_ptr;before=o.read_u32s(HEAP_BASE,(start-HEAP_BASE)//4)
  id=o.call(0x48bc40,[name],convention='thiscall',this=reference,fpu_cw=CW_D3D9).eax
  lookup_id=id
  if mode=='missing':id=123
  owner=0 if mode=='null' else owners[1 if mode=='replace' else 0]
  result=o.call(0x48c0f0,[id,owner,pos if pose=='matrix' else 0,rot if pose=='matrix' else 0],convention='thiscall',this=reference,fpu_cw=CW_D3D9).eax
  rows.append(dict(kind=kind,mode=mode,pose=pose,flags=flags,reference=reference,resource=resource,body=body,resources=resources,
   owner=owner,pos=pos,rot=rot,start=start,end=o.heap_ptr,before=before,after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4),
   id=id,lookup_id=lookup_id,result=result,events=events))
 return dict(schema=1,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
  producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),fpu_cw=CW_D3D9,scope=__doc__,rows=rows)
if __name__=='__main__':
 out=Path('re/out/boot/native-render-bone-original.json');out.write_text(json.dumps(run(),sort_keys=True));print(out)
