#!/usr/bin/env python3
"""Original502de0 SCX4 CPU graph. Graphics material-description and final GPU
object compilation are declared collaborators; original503400,5038e0,4fe980,
4ff710, object constructors/arrays, and bounds execute unmodified.
"""
import json,struct,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE

def f(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def words(v):return struct.pack('<'+'I'*len(v),*[x&0xffffffff for x in v])
def scx(chunks):
 cur=12+len(chunks)*8;table=[];body=b''
 for tag,data in chunks:table.extend([tag,cur]);chunk=words([tag,len(data)+8])+data;body+=chunk;cur+=len(chunk)
 return b'INVO'+words([4,len(chunks)])+words(table)+body

def run():
 rows=[]
 profiles=[('empty',0,0,0),('unknown',0,0,0),('material',0,0,0),('metadata',0,0,0),('position',1,0,0),('normal-uv',0x241,1,0),('weighted',0x3e5,2,4),('two-weight',0x249,1,0),('three-weight',0x251,1,4)]
 for profile,flags,channels,slot in profiles:
  o=Oracle();o.hook_alloc()
  def realloc(oc,a):
   old,n=a;p=oc.alloc(n)if n else 0
   if old and n:oc.write(p,oc.read(old,min(n,oc.heap_sizes[old])))
   return p
  o.hook(0x54f580,realloc,'cdecl',2);o.write_u32(0x6200a4,123)
  chunks=[]
  if profile=='unknown':chunks=[(99,b'')]
  elif profile not in ['empty']:
   chunks=[(0,words([0,1,2,3]))]
   if profile!='material':
    tag=3 if profile in ['weighted','three-weight']else(2 if profile=='two-weight'else 1)
    payload=words([0,3,1,2,3])if tag==3 else(words([0,1,2])if tag==2 else words([0]))
    chunks.append((tag,payload))
    if flags:
     vertex=[]
     for index,pos in enumerate([[1.2345,-2.1,.13],[-3.15,.27,4.33],[.33,1.14,-.59]]):
      if flags&1:vertex.extend(map(f,pos))
      n=1 if flags&4 else(2 if flags&8 else(3 if flags&16 else 0));vertex.extend([f(.25+i*.13)for i in range(n)])
      if flags&0x20:vertex.append(0x03020100)
      if flags&0x40:vertex.extend(map(f,[.1,.2,.3]))
      if flags&0x80:vertex.append(0x00123456+index)
      if flags&0x100:vertex.append(0xaabbccdd-index)
      for k in range(8):
       if flags&(0x200<<k):vertex.extend(map(f,[.23+index*.11,.87-index*.13]))
     chunks += [(4,words([3,flags]+vertex)),(5,words([3])+struct.pack('<3H',0,1,2))]
  data=scx(chunks);shape=o.alloc(0x110);source=o.alloc(len(data));o.write(source,data);o.call(0x4fe210,convention='thiscall',this=shape);o.write_u32(shape+0x28,source);o.write_u32(shape+0x2c,len(data));o.write_u32(shape+0x44,slot)
  context=o.alloc(0x208);device=o.alloc(4);material=o.alloc(4);devicevt=o.scratch(0xd0);materialvt=o.scratch(12)
  o.write_u32(0x6187b0,context);o.write_u32(context+0x204,device);o.write_u32(device,devicevt);o.write_u32(devicevt+0xcc,0x300f0000);o.write_u32(material,materialvt);o.write_u32(materialvt+8,0x300f0010)
  descriptions=[]
  for i in range(channels):descriptions.append([0 if i==0 else 0xffffffff,f(1.3+i*.2),f(.7+i*.13),f(.12+i*.02),f(-.21+i*.02)])
  calls=[]
  o.hook(0x300f0000,lambda oc,a:calls.append(['material',a[0],a[1]])or material,'thiscall',2)
  def description(oc,a):
   oc.write(a[0],bytes(168));oc.write_u32(a[0]+4,channels)
   for i,row in enumerate(descriptions):oc.write_u32s(a[0]+8+i*20,row)
   return 0
  o.hook(0x300f0010,description,'thiscall',1)
  o.hook(0x4fffe0,lambda oc,a:calls.append(['finalize',oc.reg('ecx')])or 0,'thiscall',0)
  global_addresses=list(range(0x65bc68,0x65bd24,4))+[0x65bd40,0x65bd48]
  globals_before={str(p):o.read_u32(p)for p in global_addresses}
  before=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4);start=o.heap_ptr
  o.call(0x502de0,convention='thiscall',this=shape,fpu_cw=CW_D3D9)
  rows.append(dict(globals_before=globals_before,profile=profile,shape=shape,material=material,descriptions=descriptions,before=before,start=start,end=o.heap_ptr,after=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4),globals={str(p):o.read_u32(p)for p in list(range(0x65bc68,0x65bd24,4))+[0x65bd40,0x65bd48]},calls=calls))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
