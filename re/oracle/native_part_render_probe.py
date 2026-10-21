#!/usr/bin/env python3
"""Original467ff0/4682a0 complete reference graph with declared payload/clone callbacks.

Real48d860 reference cloning wrapper,5439e0 render binding and intrusive links
execute.545230 cloning,5447d0 readiness, payload replacement and deferred release
are explicit collaborators; no raw output is normalized.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def run():
 rows=[]
 for operation in ['texture','render']:
  for profile in ['shared','private','clear','no_active','null_payload','clone_failed','same','no_default']:
   o=Oracle();o.hook_alloc()
   template=o.alloc(0x2e98);body=o.alloc(0x180);target=o.alloc(16)
   resources=[o.alloc(0x300)for _ in range(7)]
   default_render,private_render,active,old_texture,new_texture,mesh,incoming_render=resources
   vt=o.alloc(0x40)
   for i,raw in enumerate(resources):
    o.write_u32(raw,vt);o.write_u32(raw+0x50,0x50001+i)
    o.write_u32(raw+0x4c,3 if raw==active else 7 if raw in [old_texture,new_texture] else 14)
   o.write_u32(old_texture+0x50,0xffff0010 if profile=='private' else 0x50004)
   o.write_u32s(vt+8,[0x300f0000,0x300f0010,0,0x300f0020])
   def bind(slot,raw):o.call(0x429060,[raw],convention='thiscall',this=slot)
   bind(template+0x1108,default_render)
   if profile!='no_default':bind(template+0x10e8,new_texture)
   bind(template+0x170,default_render)
   bind(body+0x9c,private_render if profile=='private' else default_render)
   bind(body+0x7c,old_texture)
   bind(body+0x8c,mesh)
   bind(private_render+0x8c,mesh);bind(incoming_render+0x8c,mesh)
   bind(active+0xd8+0x78,default_render)
   refs=o.alloc(16);indices=o.alloc(4)
   o.write_u32(body+0x4c,refs);o.write_u32(body+0x50,indices)
   o.write_u32(body+0x54,0 if profile=='no_active' else 1);bind(refs,active)
   replacement=new_texture if operation=='texture' else incoming_render
   if profile=='clear':replacement=0
   if profile=='same':replacement=old_texture if operation=='texture' else default_render
   bind(target,replacement)
   calls=[]
   def record(name,args):calls.append(dict(name=name,args=args))
   def clone(oc,a):record('clone',[oc.reg('ecx')]);return 0 if profile=='clone_failed' else private_render
   def payload(oc,a):
    raw=oc.reg('ecx');record('payload',[raw]);return 0 if profile=='null_payload' else raw+(0xd8 if raw==active else 0x80)
   def replace(oc,a):record('replace',[oc.reg('ecx'),*a]);return 1
   def defer(oc,a):record('defer',[oc.read_u32(oc.reg('ecx')+12)]);return 0
   o.hook(0x545230,clone,'thiscall',0)
   o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
   o.hook(0x300f0010,payload,'thiscall',1)
   o.hook(0x300f0020,lambda oc,a:0,'thiscall',1)
   o.hook(0x300f0000,replace,'thiscall',2)
   # Payloads have their own virtual table; replacements execute through+8.
   for raw in resources:o.write_u32(raw+(0xd8 if raw==active else 0x80),vt)
   o.hook(0x48a8d0,defer,'thiscall',0)
   before=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)
   o.call(0x467ff0 if operation=='texture' else 0x4682a0,[template,body,target],fpu_cw=CW_D3D9)
   rows.append(dict(operation=operation,profile=profile,template=template,body=body,target=target,active=active,
    clone=0 if profile=='clone_failed' else private_render,before=before,calls=calls,
    after=o.read_u32s(0x20000000,(o.heap_ptr-0x20000000)//4)))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
