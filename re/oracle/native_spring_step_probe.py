#!/usr/bin/env python3
"""Original491100 spring phase: detached, sleeping/static and no-contact branches.
All spring instructions execute original x86 on declared graph inputs. Contact
response requires additional spatial/contact authority and is not certified here.
"""
import json,hashlib
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
from native_spring_probe import bits
WORLD=0x6409c4
def words(o,a,n):return [[p,o.read_u32(p)]for p in range(a,a+n,4)if o.read_u32(p)]
def run():
 rows=[]
 for mode in ['detached','awake','sleeping','static']:
  for length in [.8,1.2,-.5]:
   o=Oracle();o.hook_alloc();o.call(0x497b60,[],convention='thiscall',this=WORLD,fpu_cw=CW_D3D9);o.write_u32(0x617650,WORLD)
   spring=o.alloc(0xec);entity=o.alloc(0x218);body=o.alloc(0xa0)
   o.write_u32s(spring,[0x12340000+i for i in range(0xec//4)])
   o.write_u32(spring,0x5f0fcc);o.write_u32(spring+0x7c,0 if mode=='detached' else entity);o.write_u32(entity+0x5c,body);o.write_u32(entity+0x58,0x81 if mode=='sleeping' else 1);o.write_u32(body+0x9c,4 if mode=='static' else 0)
   o.write_u32(spring+0xb8,0);o.write_u32(spring+0x38,bits(length))
   previous=o.read_u32(WORLD+0xac);o.write_u32(previous+4,spring);o.write_u32(spring+8,previous);o.write_u32(spring+4,WORLD+0xa4);o.write_u32(WORLD+0xac,spring)
   before=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)
   result=o.call(0x491100,[],convention='thiscall',this=spring,fpu_cw=CW_D3D9).eax
   after=words(o,HEAP_BASE,o.heap_ptr-HEAP_BASE)+words(o,WORLD,0x210)
   rows.append(dict(mode=mode,length=bits(length),spring=spring,input_words=before,output_words=after,result=result,heap=o.heap_ptr))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
