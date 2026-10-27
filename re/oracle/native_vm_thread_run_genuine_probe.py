#!/usr/bin/env python3
"""Passive genuine original-script Thread.run observations.

The validated original Osd replay executes callback creation, unchanged scripts,
lookup and native instructions. Captured incoming pages are comparison inputs
only. Production never reads these files. No VM or native-result hook is added.
"""
from pathlib import Path
import argparse,hashlib,importlib,json,sys

def main():
 root=Path(__file__).resolve().parents[2];p=argparse.ArgumentParser();p.add_argument('--evidence-root',type=Path,default=root);p.add_argument('--dispatches',type=int,default=1);p.add_argument('--native-entry',action='store_true');p.add_argument('--output',type=Path,default=root/'re/out/boot/native-vm-run-genuine-original.json');a=p.parse_args()
 evidence=a.evidence_root.resolve();sys.path.insert(0,str(evidence/'re/oracle'))
 probe=importlib.import_module('native_osd_first_animate_probe');from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE
 from unicorn import x86_const as X
 from capstone import Cs,CS_ARCH_X86,CS_MODE_32
 base=probe.Replay;observations=[]
 class Observe(base):
  def __init__(self,name):
   super().__init__(name);self._run=None;decoder=Cs(CS_ARCH_X86,CS_MODE_32)
   def state():
    d=self._run
    return dict(registers={r:self.reg(r)for r in('eax','ebx','ecx','edx','esi','edi','ebp','esp')},fpu=dict(control=self.uc.reg_read(X.UC_X86_REG_FPCW),status=self.uc.reg_read(X.UC_X86_REG_FPSW),tags=self.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(self.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)]),heap_ptr=self.heap_ptr,changed={str(p):self.read(p,4096).hex()for p in sorted(d['dirty'])})
   counters={self.read_u32(0x5e7050):'frequency',self.read_u32(0x5e7048):'counter'}
   def code(uc,pc,size,_):
    if self._run is None and pc==0x420ff0:
     initial={}
     for first,last in[(0x400000,0x7c0000),(0x20000000,(self.heap_ptr+0x1ffff)&~4095),(0x7fe00000,0x7ff00000)]:
      for address in range(first,last,4096):initial[address]=self.read(address,4096)
     self._run=dict(initial=initial,pages=set(),dirty=set(),events=[],opcodes=[],instructions=[],entry_sp=self.reg('esp'),thread=self.reg('ecx'),return_pc=self.read_u32(self.reg('esp')),budget=self.read_u32(self.reg('esp')+4));self._run['before']=state()
    d=self._run
    if d is None:return
    instruction=next(decoder.disasm(self.read(pc,size),pc));d['instructions'].append(dict(pc=pc,assembly=instruction.mnemonic+' '+instruction.op_str))
    if pc==0x421061:d['before_clock']=state()
    if pc in counters:
     address=self.read_u32(self.reg('esp')+4);d['events'].append(dict(kind=counters[pc],args=[address],state=state(),value=self.read_u32s(address,2)))
    if pc==0x4210c5:
     d['opcodes'].append(dict(pc=self.read_u32(self.reg('ebp')),opcode=self.reg('eax'),state=state()))
     if not a.native_entry and len(d['opcodes'])==a.dispatches:
      d['after']=state();observations.append(d);uc.emu_stop()
    if a.native_entry and pc==0x47cc10:
     d['after']=state();observations.append(d);uc.emu_stop()
   def access(uc,access,address,size,value,_):
    d=self._run
    if d is None:return
    for page in range(address&~4095,(address+size+4095)&~4095,4096):
     if page not in d['initial']:raise RuntimeError(f'outside captured source profile {page:x}')
     d['pages'].add(page)
   def write(uc,access_,address,size,value,_):
    access(uc,access_,address,size,value,_)
    if self._run is not None:self._run['dirty'].update(range(address&~4095,(address+size+4095)&~4095,4096))
   self.uc.hook_add(UC_HOOK_CODE,code);self.uc.hook_add(UC_HOOK_MEM_READ,access);self.uc.hook_add(UC_HOOK_MEM_WRITE,write)
   inherited=self.hook
   def hook(address,fn,conv='cdecl',nargs=0):
    def observed(oc,words):
     if self._run is not None:self._run['events'].append(dict(kind='external',target=address,args=words,state=state()))
     return fn(oc,words)
    return inherited(address,observed,conv,nargs)
   self.hook=hook
   for address in[0x54f560,0x54f5b0]:
    fn,conv,nargs=self.hooks[address];hook(address,fn,conv,nargs)
 probe.Replay=Observe;result=probe.capture(endpoint='callback-return');assert len(observations)==1
 d=observations[0];pages=d.pop('pages')|d.pop('dirty');initial=d.pop('initial')
 out=dict(schema=1,profile=__doc__,native_entry=a.native_entry,dispatches=len(d['opcodes']),root_identity=result['root_identity'],snapshot_provenance=result['snapshot_provenance'],snapshot_metadata_sha256=result['snapshot_metadata_sha256'],snapshot_pages_sha256=result['snapshot_pages_sha256'],execution_profile=result['execution_profile'],comparison_policy=result['comparison_policy'],pages=[dict(address=p,bytes=initial[p].hex())for p in sorted(pages)],**d)
 sources=[Path(__file__)]+[evidence/'re/oracle'/f for f in['native_osd_first_animate_probe.py','snapshot_replay.py','headless_snapshot.py','oracle.py']];out['producer_sources']={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources}
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),pages=len(pages),instructions=len(d['instructions']),opcodes=len(d['opcodes']))))
if __name__=='__main__':main()
