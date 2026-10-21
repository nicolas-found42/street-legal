#!/usr/bin/env python3
"""Passive original-x86 provenance for54e3b0's three unwritten matrix slots.

Observe memory stores and frame pointers without changing control flow or values.
Run existing world, spring and Part scenarios; stack traces are verification
artifacts only, never runtime inputs. The retained last writer explains every
copied padding word, including call return PCs and callee-saved registers.
"""
import hashlib,json
from pathlib import Path
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn import x86_const as X
from oracle import Oracle,DEFAULT_EXE
import native_world_probe,native_spring_probe,native_part_physics_probe
original_init=Oracle.__init__
traces=[]
current_case=''
def traced_init(self,*args,**kwargs):
 original_init(self,*args,**kwargs)
 stores={};events=[];traces.append(dict(case=current_case,events=events))
 def write(uc,access,address,size,value,_):
  pc=uc.reg_read(X.UC_X86_REG_EIP)
  # Keep byte stores too: a word can retain bytes from distinct instructions.
  for i in range(size):stores[address+i]=dict(pc=pc,address=address,size=size,value=value)
 def instruction(uc,pc,size,_):
  if pc!=0x54e3b0:return
  sp=uc.reg_read(X.UC_X86_REG_ESP)
  slots=[]
  for delta in [-0x24,-0x14,-4]:
   p=sp+delta;origins=[]
   for i in range(4):
    origin=stores.get(p+i)
    if origin not in origins:origins.append(origin)
   slots.append(dict(address=p,value=self.read_u32(p),last_writes=origins))
  events.append(dict(pc=pc,esp=sp,ecx=uc.reg_read(X.UC_X86_REG_ECX),esi=uc.reg_read(X.UC_X86_REG_ESI),edi=uc.reg_read(X.UC_X86_REG_EDI),slots=slots,region_address=sp-0x40,region_words=self.read_u32s(sp-0x40,0x80//4)))
 self.uc.hook_add(UC_HOOK_MEM_WRITE,write)
 self.uc.hook_add(UC_HOOK_CODE,instruction)
Oracle.__init__=traced_init
if __name__=='__main__':
 current_case='world-startup-and-sleep';native_world_probe.run()
 current_case='two-consecutive-springs';native_spring_probe.run([.9,1.2],[.3,.2,-.7])
 current_case='part-physical-graph';native_part_physics_probe.run()
 digest=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()
 print(json.dumps(dict(exe_sha256=digest,scope=__doc__,traces=traces),indent=2))
