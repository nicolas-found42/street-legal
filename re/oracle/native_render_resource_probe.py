#!/usr/bin/env python3
"""539880 zero-bone6/c/d/14 instance stages; real543000 base/payload, links,5439e0.
Resource ID allocation, tree insertion, load marking and payload vcall are
explicit collaborators. Compare complete resource/parent/template buffers.
"""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def run():
 rows=[]
 profiles=[(kind,parented,0,0,False)for kind in [6,0xc,0xd,0x14]for parented in [False,True]]
 profiles += [(0xe,parented,shape,present,null)for parented in [False,True]for shape,present,null in [(0,1,False),(1,1,False),(1,0,False),(1,1,True)]]
 for kind,parented,shape,present,null in profiles:
  o=Oracle();o.hook_alloc();o.write_u32(0x617650,0x6409c4);o.write_u32(0x6200a4,123)
  owner=o.alloc(16);ref=o.alloc(16);parent=o.alloc(0xd8);template=o.alloc(0xbc);raw=o.alloc(0x26c);desc=o.alloc(24)
  o.write_u32s(desc,[3,0xffff0101,1,0,0,0]);o.call(0x543000,[desc,0,0],convention='thiscall',this=raw,fpu_cw=CW_D3D9)
  o.write_u32(template+0x4c,kind);o.write_u32(template+0x50,0xffff0100)
  o.call(0x429060,[template],convention='thiscall',this=ref)
  o.write_u32(owner+8,77)
  if parented:
   o.write_u32(owner+12,parent);o.write_u32(raw+0x14,parent)
   o.write_u32s(parent+0x6c,[0x41200000,0x447a0000]);o.write_u32s(parent+0x80,[1,0x3f800000,0x40000000,0x40400000,0x40800000,0x40a00000,0x40c00000])
  typedata=o.alloc(0xc0);mesh=o.alloc(0x50)
  o.write_u32(typedata+0x18,0x300f8000 if present else 0);o.write_u32(mesh+0x28,shape);o.write_u32s(mesh+0x38,[0x3f9e0652,0x402df854,0x40a4bbcf])
  # Resource load/request and template virtual get-data are explicit collaborators.
  # Original539880's branch and complete resulting bounds execute unmodified.
  callbacks=[]
  def payload(oc,args):
   callbacks.append(dict(resource=oc.read_u32s(raw,0x26c//4),count=oc.read_u32(0x765f94)))
   return typedata if oc.reg('ecx')==template else (0 if null else mesh)
  if kind==0xe:o.hook(0x419860,payload,'thiscall',4)
  before={str(p):o.read_u32s(p,n//4)for p,n in [(owner,16),(ref,16),(parent,0xd8),(template,0xbc),(raw,0x26c),(typedata,0xc0),(mesh,0x50)]}
  o.hook(0x536ed0,lambda oc,a:0xffff0101,'thiscall',0)
  o.hook(0x536900 if parented else 0x536890,lambda oc,a:raw,'thiscall',6 if parented else 5)
  o.hook(0x544b90,lambda oc,a:oc.write_u32(raw+0x64,oc.read_u32(raw+0x64)|1) or 0,'thiscall',1)
  # Original range virtuals execute, including clamping/generation stamps.
  # Concrete vtable functions are retained except lazy load/payload access.
  vt=o.alloc(0x40);o.write(vt,o.read(o.read_u32(raw),0x40));o.write_u32(raw,vt);o.write_u32(vt+12,0x300f0000)
  before[str(raw)][0]=vt
  o.hook(0x300f0000,lambda oc,a:raw+0xd8,'thiscall',1)
  initial_count=o.read_u32(0x765f94)
  result=o.call(0x539880,[owner,ref,0,0],fpu_cw=CW_D3D9)
  assert result.eax==raw
  assert o.read_u32(0x765f94)==(initial_count+1)&0xffffffff
  rows.append(dict(kind=kind,parented=parented,mesh=0 if null else mesh,shape=shape,present=present,resource=raw,template=template,before=before,callbacks=callbacks,after={str(p):o.read_u32s(p,len(words))for p,words in [(int(k),v)for k,v in before.items()]}))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
