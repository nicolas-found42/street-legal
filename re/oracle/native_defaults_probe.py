#!/usr/bin/env python3
"""Original442500 constructor then446180 preparse defaults.

External resource lookup/bind/name use cfg_handler_vectors.Hooks' declared
controlled behavior. Runtime code must derive defaults from original source,
never load this oracle memory image.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,HEAP_BASE,CW_D3D9
from cfg_handler_vectors import Hooks,FACTORY,CONFIGURE,DISPATCH_MEM

def run():
 o=Oracle(fpu_cw=CW_D3D9);hk=Hooks(o)
 def resname(oc,a):
  result=hk.h_resname(oc,a);hk.log[-1]['r']=result;return result
 o.hook(0x5383f0,resname,'thiscall',1)
 o.hook(0x4261f0,lambda oc,a:1.0,'cdecl',1);o.hook(0x426100,lambda oc,a:0,'cdecl',2);o.hook(0x4427e0,lambda oc,a:None,'cdecl',0)
 text=o.alloc(16);o.write(text,b'eof\n\0');arg=o.scratch(64);o.write(arg,f'x {text} 4\0'.encode())
 k=o.call(FACTORY,[arg,0],fpu_cw=CW_D3D9,limit=20_000_000).eax
 def words(end):return [[a,o.read_u32(a)] for a in range(HEAP_BASE,end,4) if o.read_u32(a)]
 before=dict(heap=o.heap_ptr,words=words(o.heap_ptr));hk.log.clear();snapshot={}
 def grab(oc,a):
  snapshot.update(heap=oc.heap_ptr,words=words(oc.heap_ptr),calls=list(hk.log));return 1
 o.hook(DISPATCH_MEM,grab,'cdecl',4)
 o.call(CONFIGURE,[k],'fastcall',fpu_cw=CW_D3D9,limit=20_000_000)
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,k=k,before=before,after=snapshot,configured=dict(heap=o.heap_ptr,words=words(o.heap_ptr),calls=list(hk.log)))
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
