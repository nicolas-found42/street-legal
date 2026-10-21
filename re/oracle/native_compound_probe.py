#!/usr/bin/env python3
"""Original49a050 mass/inertia phase through49a3f4, before bounds regeneration.
Inputs explicitly supply identity-oriented child records and diagonal inertia.
"""
import json,struct,hashlib,itertools
from pathlib import Path
from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,SENTINEL

def bits(x):return struct.unpack('<I',struct.pack('<f',x))[0]
def mass_rows():
 rows=[]
 for count in [0,1,2,3]:
  o=Oracle();o.hook_alloc();parent=o.alloc(0x218);body=o.alloc(0xa0);o.write_u32(parent+0x5c,body);o.write_u32(body+0x9c,0x405)
  children=[]
  for i in range(count):
   entity=o.alloc(0x218);b=o.alloc(0xa0);rel=o.alloc(0x50)
   inverse_mass=[0.,0.5,0.25][i]
   position=[[1.,2.,3.],[-2.,1.,4.],[3.,-4.,2.]][i]
   o.write_u32(entity+0x5c,b);o.write_u32(entity+0x1c,rel)
   o.write_u32(entity+0x28,o.read_u32(parent+0x24));o.write_u32(parent+0x24,entity)
   o.write_u32(b+0x14,bits(inverse_mass));o.write_f32s(rel+4,position)
   for off in [0x20,0x34,0x48]:o.write_u32(rel+off,bits(1.))
   for j,off in enumerate([0x48,0x5c,0x70]):o.write_u32(b+off,bits((i+1)*(j+1)))
   children.append(dict(entity=entity,body=b,relative=rel,inverse_mass=bits(inverse_mass),position=list(map(bits,position)),inertia=list(map(bits,[(i+1)*(j+1) for j in range(3)]))))
  center=[]
  def stop(uc,address,size,user):
   if address==0x49a3f4:
    center.extend(o.read_u32s(uc.reg_read(X.UC_X86_REG_ESP)+0x14,3));uc.reg_write(X.UC_X86_REG_EIP,SENTINEL)
  o.uc.hook_add(UC_HOOK_CODE,stop,begin=0x49a3f4,end=0x49a3f4)
  o.call(0x49a050,[parent],fpu_cw=CW_D3D9)
  rows.append(dict(count=count,parent=parent,body=body,children=children,body_words=o.read_u32s(body,40),center=center))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)


def full_rows():
 rows=[]
 for count,kind,nested,matrix in itertools.product([0,1,2,3],[1,2,3,4,5],[False,True],[[1.,0.,0.,0.,1.,0.,0.,0.,1.],[.9877,0.,-.15635,0.,1.,0.,.15635,0.,.9877]]):
  o=Oracle();o.hook_alloc();o.write_u32(0x640bd0,1);parent=o.alloc(0x218);body=o.alloc(0xa0);definition=o.alloc(0xac)
  o.write_u32(parent+0x5c,body);o.write_u32(body+0x9c,0x405);o.write_u32(body+0x78,definition);o.write_u32(definition+8,6)
  for base in [0,0xd4]:
   for off in [0xa4,0xb8,0xcc,0x104]:o.write_u32(parent+base+off,bits(1.))
  children=[]
  for i in range(count):
   entity=o.alloc(0x218);b=o.alloc(0xa0);rel=o.alloc(0x50);d=o.alloc(0xac)
   inverse_mass=[0.,0.5,0.25][i];position=[[1.,2.,3.],[-2.,1.,4.],[3.,-4.,2.]][i]
   o.write_u32(entity+0x20,parent);o.write_u32(entity+0x5c,b);o.write_u32(entity+0x1c,rel);o.write_u32(b+0x78,d);o.write_u32(d+8,kind)
   o.write_u32(entity+0x28,o.read_u32(parent+0x24));o.write_u32(parent+0x24,entity)
   o.write_u32(b+0x14,bits(inverse_mass));o.write_f32s(rel+4,position);o.write_f32s(entity+0x64,position)
   for base in [0,0xd4]:
    for off in [0xa4,0xb8,0xcc,0x104]:o.write_u32(entity+base+off,bits(1.))
   for off in [0x20,0x34,0x48]:o.write_u32(rel+off,bits(1.))
   for j,off in enumerate([0x48,0x5c,0x70]):o.write_u32(b+off,bits((i+1)*(j+1)))
   for index,v in enumerate(matrix):
    o.write_u32(rel+0x20+(index//3)*16+(index%3)*4,bits(v))
    for base in [0,0xd4]:o.write_u32(entity+base+0xa4+(index//3)*16+(index%3)*4,bits(v))
   o.write_f32s(d+0x9c,[float(i+1),2.,3.]);o.write_u32(d+0x10,bits(2.) if kind==1 else bits(1.) if kind in [2,3,4] else 0)
   children.append(dict(entity=entity,body=b,relative=rel,definition=d,inverse_mass=bits(inverse_mass),position=list(map(bits,position)),inertia=list(map(bits,[(i+1)*(j+1) for j in range(3)])),dimensions=list(map(bits,[float(i+1),2.,3.]))))
  ancestor=None
  if nested:
   ancestor=o.alloc(0x218);abody=o.alloc(0xa0);adefinition=o.alloc(0xac);arelative=o.alloc(0x50)
   o.write_u32(ancestor+0x5c,abody);o.write_u32(abody+0x78,adefinition);o.write_u32(abody+0x9c,0x405);o.write_u32(adefinition+8,5)
   for base in [0,0xd4]:
    for off in [0xa4,0xb8,0xcc,0x104]:o.write_u32(ancestor+base+off,bits(1.))
   o.write_u32(parent+0x20,ancestor);o.write_u32(parent+0x1c,arelative);o.write_u32(parent+0x58,4);o.write_u32(ancestor+0x24,parent);o.write_u32(arelative,parent)
   for off in [0x1c,0x20,0x34,0x48]:o.write_u32(arelative+off,bits(1.))
  o.call(0x49a050,[parent],fpu_cw=CW_D3D9)
  rows.append(dict(nested=nested,ancestor=ancestor,ancestor_words=o.read_u32s(ancestor,0x218//4) if ancestor else [],ancestor_body_words=o.read_u32s(abody,40) if ancestor else [],ancestor_definition_words=o.read_u32s(adefinition,43) if ancestor else [],ancestor_vertices=o.read_u32s(o.read_u32(adefinition+0x24),24) if ancestor else [],ancestor_relative_words=o.read_u32s(arelative,20) if ancestor else [],kind=kind,matrix=list(map(bits,matrix)),count=count,parent=parent,body=body,definition=definition,children=children,parent_words=o.read_u32s(parent,0x218//4),body_words=o.read_u32s(body,40),definition_words=o.read_u32s(definition,43),vertices=o.read_u32s(o.read_u32(definition+0x24),24),relative_words=[o.read_u32s(c['relative'],20) for c in children],heap=o.heap_ptr))
 return rows

def run():
 result=mass_rows();result['full']=full_rows();return result
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
