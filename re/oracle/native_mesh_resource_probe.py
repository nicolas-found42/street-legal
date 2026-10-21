#!/usr/bin/env python3
"""Original53af40 descriptor parsing with declared505960 shape collaborator."""
import json,hashlib,itertools
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9

def run():
 rows=[]
 for text in ['sourcefile example.scx\n','sourcefile example.scy\n','flags 9\nslot 3\nradius 2.25\nsourcefile example.scx\n','sourcefile example.scx\noc_rect 1 2 3 4\n','sourcefile example.scx\noc_rect 1 2\n']:
  o=Oracle();o.hook_alloc();r=o.alloc(0xfc);d=o.alloc(24);arg=o.alloc(len(text)+1);o.write_u32s(d,[5,0xffff1234,0,0,0]);o.call(0x53b520,[d],convention='thiscall',this=r);o.write(arg,text.encode()+b'\0');o.write_u32(0x619994,1);calls=[]
  def shape(oc,a):calls.append(dict(args=a[:2]+a[3:],source=oc.read(a[2],256).split(b'\0')[0].decode()));return 0x30004000
  o.hook(0x505960,shape,'cdecl',6);before=o.read_u32s(r,(o.heap_ptr-r+3)//4);heap_before=o.heap_ptr
  o.call(0x53af40,[arg,len(text),0x3f800000],convention='thiscall',this=r+0x80,fpu_cw=CW_D3D9)
  rows.append(dict(text=text,raw=r,body=r+0x80,input=before,heap_before=heap_before,output=o.read_u32s(r,(o.heap_ptr-r+3)//4),heap=o.heap_ptr,calls=calls))
 progress=[]
 for rawflags,shapeflags,detail,null in itertools.product([0,0x100],[0,8,0x14,0x80000008],[0,0x447a0000,0x7fc00000],[False,True]):
  o=Oracle();o.hook_alloc();r=o.alloc(0xfc);d=o.alloc(24);shape=o.alloc(0x110);o.write_u32s(d,[5,0xffff1234,0,0,0]);o.call(0x53b520,[d],convention='thiscall',this=r);o.write_u32(r+0x54,rawflags);o.write_u32(r+0x64,1);o.write_u32(r+0x68,0x3f000000);o.write_u32(r+0x6c,detail);o.write_u32(r+0x98,0 if null else shape);o.write_u32(shape+0x20,shapeflags);o.write_u32(r+0xa8,1);o.write_u32(r+0xb8,0x40000000);calls=[]
  o.hook(0x537790,lambda oc,a:calls.append(a),'thiscall',4);before=o.read_u32s(r,0xfc//4);o.call(0x53b710,[0,0x3f800000],convention='thiscall',this=r,fpu_cw=CW_D3D9)
  progress.append(dict(raw=r,shape=shape,input=before,output=o.read_u32s(r,0xfc//4),rawflags=rawflags,shapeflags=shapeflags,detail=detail,null=null,calls=calls,mode=o.read_u32(0x619994)))
 return dict(progress=progress,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
