#!/usr/bin/env python3
"""Original44b79d..44b903 LOD/render/notification/final payload sequencing.

Resource type check/get-data, both render refresh functions, message delivery
and snprintf are declared boundaries. Branching, level selection and payload
stores execute original code. This does not certify any of those collaborators.
"""
import hashlib,json,struct
from pathlib import Path
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,SENTINEL

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def run():
 rows=[]
 for has_resource in [False,True]:
  for distance in [-1.,1.,5.,100.]:
   o=Oracle();k=o.alloc(0x2e98);body=o.alloc(0x212c);resource=o.alloc(0x200);table=o.alloc(0x100);payload=o.alloc(0x100)
   o.write_u32(k+0xfac,3);o.write_f32s(k+0xfb0,[0.,2.,10.,1000.]);o.write_u32(k+0x1f40,4);o.write_u32(k+0x2e4c,2)
   o.write_u32(body+8,0x12345678);o.write_u32(body+0xc,resource if has_resource else 0)
   o.write_u32(resource,table);o.write_u32(resource+0x4c,1);o.write_f32s(resource+0x70,[distance]);o.write_u32(table+0xc,0x300f0000)
   o.write_u32(payload+0xc,4);o.write_u32(payload+0x74,0xaaaaaaaa);o.write_u32(payload+0x78,0xbbbbbbbb)
   events=[]
   o.hook(0x5447d0,lambda oc,a:0,'thiscall',3);o.hook(0x300f0000,lambda oc,a:payload,'thiscall',1)
   o.hook(0x46f9e0,lambda oc,a:events.append(['render',False,a[1]]),'thiscall',2)
   o.hook(0x44c1f0,lambda oc,a:events.append(['render',True,a[1]]),'thiscall',2)
   def snprintf(oc,a):oc.write(a[0],f'car_add {a[2]} {a[3]}\0'.encode());return 1
   o.hook(0x551220,snprintf,'cdecl',4)
   o.hook(0x426800,lambda oc,a:events.append(['notify',oc.read_cstr(a[3]).decode()]),'thiscall',5)
   def write(uc,access,address,size,value,user):
    if address==payload+0x78:events.append(['window',o.read_u32(payload+0x74),value])
    elif address==payload+0xc:events.append(['flags',0x4000])
   o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
   def boundary(uc,address,size,user):
    if address==0x44b79d:o.set_reg('ebp',body);o.set_reg('ebx',0);o.write_u32(o.reg('esp')+0x10,k)
    elif address==0x44b903:o.set_reg('eip',SENTINEL)
   o.uc.hook_add(UC_HOOK_CODE,boundary);o.call(0x44b79d,[],fpu_cw=CW_D3D9)
   rows.append(dict(has_resource=has_resource,distance=distance,k=k,body=body,resource=resource,payload=payload,events=events,payload_words=o.read_u32s(payload,64)))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
