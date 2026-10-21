#!/usr/bin/env python3
"""Original4472ef..44751d car postparse including original46e400.

Declared entry registers follow successful cfg dispatch. Only resource package
lookup and reference bind use controlled cfg Hooks; geometry, transforms, wheel
scales and loadcount run original instructions. Three unspecified stack padding
words per slot matrix are excluded; all meaningful coefficients are compared.
"""
import hashlib,json,struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,SENTINEL,HEAP_BASE
from cfg_handler_vectors import Hooks

def bits(value):return struct.unpack('<I',struct.pack('<f',value))[0]
def run():
 rows=[]
 for defaults in [False,True]:
  for mode in ['none','indexed','fallback']:
   o=Oracle();hooks=Hooks(o)
   k=o.alloc(0x2e98);p=o.alloc(0xc1c);body=o.alloc(0xa0);definition=o.alloc(0xac);target=o.alloc(0x100)
   slots=[o.alloc(0x60),o.alloc(0x60),o.alloc(0x60)]
   o.write_u32(k+0x2e48,p);o.write_u32(k+0x120,9);o.write_u32(k+0xff0,0 if mode=='indexed' else 0xffffffff)
   o.write_u32(k+0x1f40,4);o.write_u32(k+0x1150,2);o.write_u32(k+0x115c,slots[0])
   for index,slot in enumerate(slots[:-1]):
    o.write_u32(slot+4,slots[index+1]);o.write_f32s(slot+0xc,[2.3+index,-4.1,6.7])
    for i,val in zip([0,1,2,4,5,6,8,9,10],[.3,.7,-.9,1.1,-1.3,1.7,1.9,-2.3,2.7]):o.write_u32(slot+0x18+i*4,bits(val+index))
   if mode!='none':
    o.write_u32(k+0xfa8,1)
    frame=k+(0x128 if mode=='indexed' else 0x1000)
    o.write_f32s(frame,[.2,-.4,.8]);o.write_u32(frame+0x40,body)
    for i,val in zip([0,1,2,4,5,6,8,9,10],[.9,.2,-.3,.4,1.1,.6,-.7,.8,1.3]):o.write_u32(frame+0xc+i*4,bits(val))
    o.write_f32s(body+0x14,[2.0]);o.write_u32(body+0x78,definition);o.write_f32s(definition+0x40,[.1,-.2,.3])
   o.write_u32(p+0x850,target);o.write_u32(target+0x50,0x12345678)
   o.write_u32(p+0x878,0x3e99999a)
   if not defaults:o.write_u32(p+0x338,2);o.write_u32(p+0x580,3)
   o.write_f32s(p+0xd8,[.7,1.3])
   for index in range(4):o.write_f32s(k+0x1f64+index*0xc8,[.11*(index+1),.13*(index+1)])
   end=o.heap_ptr
   before=o.read_u32s(HEAP_BASE,(end-HEAP_BASE)//4)
   def boundary(uc,address,size,user):
    if address==0x4472ef:o.set_reg('esi',k);o.set_reg('edi',0);o.set_reg('ebp',bits(1.0))
    elif address==0x44751d:o.set_reg('eip',SENTINEL)
   o.uc.hook_add(UC_HOOK_CODE,boundary)
   o.call(0x4472ef,[],fpu_cw=CW_D3D9)
   ignored=[slot+0x18+i*4 for slot in slots[:-1] for i in [3,7,11]] if mode!='none' else []
   rows.append(dict(defaults=defaults,mode=mode,k=k,p=p,end=end,before=before,after=o.read_u32s(HEAP_BASE,(end-HEAP_BASE)//4),ignored=ignored,calls=hooks.log))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
