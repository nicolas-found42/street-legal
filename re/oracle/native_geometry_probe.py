#!/usr/bin/env python3
"""Original49ab60 primitive constructors and4a6010 inertia, without callee stubs.
Only allocator/declared fresh world-list scenario is controlled. Geometry and
inertia execute original x86; all body/definition fields are retained.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from unicorn import UC_HOOK_CODE
WORLD=0x6409c4
def bits(f):return struct.unpack('<I',struct.pack('<f',f))[0]
def mesh_bytes():
 data=bytearray(b'INVO'+struct.pack('<I',3)+struct.pack('<I',0x38)+bytes(0x34))
 points=[(0.,0.,0.),(100.,0.,0.),(0.,100.,0.),(0.,0.,1000.)]
 data+=struct.pack('<I',len(points))
 for point in points:data+=struct.pack('<3f',*point)+bytes(0x2c-12)
 faces=[(0,2,1),(0,1,3),(0,3,2),(1,2,3)]
 data+=struct.pack('<I',len(faces))
 for face in faces:data+=struct.pack('<3I',*face)
 data+=bytes(4)
 return bytes(data)
def mesh_rows():
 rows=[]
 for label,data in [('declared-v3-tetra',mesh_bytes())]:
  o=Oracle();o.hook_alloc();definition=o.alloc(0xac);path=o.alloc(16);o.write(path,b'probe.scx\0')
  o.hook(0x54c300,lambda oc,a:load(oc,a),'cdecl',2)
  def load(oc,args):
   p=oc.alloc(len(data));oc.write(p,data);oc.write_u32(args[1],p);return len(data)
  o.call(0x4b5c30,[path,definition],fpu_cw=CW_D3D9)
  vertices=o.read_u32(definition+0x9c);faces=o.read_u32(definition+0xa4)
  rows.append(dict(label=label,data_hex=data.hex(),definition=definition,words=o.read_u32s(definition,43),vertices=o.read_u32s(o.read_u32(definition+0xa0),vertices*3),faces=o.read_u32s(o.read_u32(definition+0xa8),faces*8)))
 return rows
def definition_body_rows():
 rows=[]
 for mass in [0.,1.,100.,-2.]:
  o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
  definition=o.alloc(0xac);o.write_u32(definition+0x4c,bits(123.))
  handle,resource,vtable,data=[o.alloc(0x100) for _ in range(4)]
  o.write_u32(handle+12,resource);o.write_u32(resource,vtable);o.write_u32(resource+0x4c,1);o.write_u32(vtable+12,0x300f0000);o.write_u32(data+12,definition)
  o.hook(0x300f0000,lambda oc,a:data,'thiscall',1);o.hook(0x5447d0,lambda oc,a:0,'thiscall',3)
  body=o.call(0x49a990,[bits(mass),handle],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
  rows.append(dict(mass=bits(mass),definition=definition,body=body,body_words=o.read_u32s(body,40),definition_words=o.read_u32s(definition,43),world_words=o.read_u32s(WORLD+0x170,16)))
 return rows
def mesh_body_rows():
 rows=[]
 for label,data,masses in [('declared-v3-tetra',mesh_bytes(),[0.,1.,100.,-2.]),('stock-baiern-water',(Path(__file__).resolve().parents[2]/'stock/cars/racers/Baiern_data/meshes/_phys_chassis_water.scx').read_bytes(),[0.001]),('stock-prime-cockpit',(Path(__file__).resolve().parents[2]/'stock/cars/racers/Prime_data/meshes/_phys_chassis_cockpit.scx').read_bytes(),[120.])]:
  for kind in [6,7]:
   for mass in masses:
    o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
    # Path lives in scratch, outside observable physics allocations.
    path=0x30001000;o.write(path,b'probe.scx\0')
    def load(oc,args):
     p=oc.alloc(len(data));oc.write(p,data);oc.write_u32(args[1],p);return len(data)
    o.hook(0x54c300,load,'cdecl',2)
    stages={}
    def stage(uc,address,size,_):
     if address==0x4aa364:stages['integrals']=o.read_u32s(0x644208,20)
     if address==0x4aa50b:stages['tensor']=o.read_u32s(o.read_u32(o.reg('esp')),12)
    o.uc.hook_add(UC_HOOK_CODE,stage)
    body=o.call(0x49ab60,[bits(mass),kind,0,0,0,path],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
    definition=o.read_u32(body+0x78)
    rows.append(dict(stages=stages,label=label,data_hex=data.hex(),data_sha256=hashlib.sha256(data).hexdigest(),kind=kind,mass=bits(mass),body=body,definition=definition,body_words=o.read_u32s(body,40),definition_words=o.read_u32s(definition,43),vertices=o.read_u32s(o.read_u32(definition+0xa0),o.read_u32(definition+0x9c)*3),faces=o.read_u32s(o.read_u32(definition+0xa8),o.read_u32(definition+0xa4)*8),world_words=o.read_u32s(WORLD+0x170,16)))
 return rows
def run():
 rows=[]
 for kind in range(8):
  for mass in [0.,1.,100.,-2.]:
   o=Oracle();o.hook_alloc();o.write(WORLD,bytes(0x400));o.write_u32(WORLD+0x188,WORLD+0x190);o.write_u32(WORLD+0x198,WORLD+0x184)
   dims=[bits(2.3),bits(3.7),bits(4.1)]
   body=o.call(0x49ab60,[bits(mass),kind,*dims,0],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9).eax
   definition=o.read_u32(body+0x78)
   rows.append(dict(kind=kind,mass=bits(mass),dimensions=dims,body=body,definition=definition,body_words=o.read_u32s(body,40),definition_words=o.read_u32s(definition,43),world_words=o.read_u32s(WORLD+0x170,16)))
 return {'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'scope':__doc__,'rows':rows,'mesh':mesh_rows(),'definition_bodies':definition_body_rows(),'mesh_bodies':mesh_body_rows()}
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
