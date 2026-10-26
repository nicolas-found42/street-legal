#!/usr/bin/env python3
"""Passive genuine original-script callback producer observations.

The validated original Osd replay executes callback creation, unchanged scripts,
lookup and native instructions. Captured incoming pages are comparison inputs
only. Production never reads these files. No VM or native-result hook is added.
Unicorn omits arithmetic PE in5516c0. Each observed clock is independently
executed by native x87 with its complete incoming environment and numeric
platform samples. Only a measured missing PE flag is propagated into Unicorn;
all GPRs, memory, script code/results and retained physical FP bits stay intact.
--unqualified-unicorn preserves the raw emulator observation. Rosetta's empty
slot canonicalization and native reserved CW bit6 normalization are explicitly
outside this flag qualification; the live result and full tag word must match.
"""
from pathlib import Path
import argparse,hashlib,importlib,json,sys,struct,subprocess,platform

def main():
 root=Path(__file__).resolve().parents[2];p=argparse.ArgumentParser();p.add_argument('--unqualified-unicorn',action='store_true');p.add_argument('--evidence-root',type=Path,default=root);p.add_argument('--dispatches',type=int,default=1);p.add_argument('--native-entry',action='store_true');p.add_argument('--native-return',action='store_true');p.add_argument('--callback-return',action='store_true');p.add_argument('--output',type=Path,default=root/'re/out/boot/native-vm-callback-genuine-original.json');a=p.parse_args()
 referee=root/'re/out/native-abi/callback-clock-fpu-referee';referee_source=Path(__file__).with_name('callback_clock_fpu_referee.c')
 referee.parent.mkdir(parents=True,exist_ok=True)
 subprocess.run(['clang','-O0']+(['-arch','x86_64']if platform.system()=='Darwin'else[])+['-o',str(referee),str(referee_source)],check=True)
 evidence=a.evidence_root.resolve();sys.path.insert(0,str(evidence/'re/oracle'))
 probe=importlib.import_module('native_osd_first_animate_probe');from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE
 from unicorn import x86_const as X
 from capstone import Cs,CS_ARCH_X86,CS_MODE_32
 base=probe.Replay;observations=[]
 class Observe(base):
  def __init__(self,name):
   super().__init__(name);self._run=None;self._clock=None;self._clock_errata=[];decoder=Cs(CS_ARCH_X86,CS_MODE_32)
   def state():
    d=self._run
    return dict(registers={r:self.reg(r)for r in('eax','ebx','ecx','edx','esi','edi','ebp','esp')},fpu=dict(control=self.uc.reg_read(X.UC_X86_REG_FPCW),status=self.uc.reg_read(X.UC_X86_REG_FPSW),tags=self.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(self.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)]),heap_ptr=self.heap_ptr,changed={str(p):self.read(p,4096).hex()for p in sorted(d['dirty'])})
   counters={self.read_u32(0x5e7050):'frequency',self.read_u32(0x5e7048):'counter'}
   def code(uc,pc,size,_):
    if self._run is None and pc==0x425a90:
     initial={}
     for first,last in[(0x400000,0x7c0000),(0x20000000,(self.heap_ptr+0x1ffff)&~4095),(0x7fe00000,0x7ff00000)]:
      for address in range(first,last,4096):initial[address]=self.read(address,4096)
     self._run=dict(initial=initial,pages=set(),dirty=set(),events=[],opcodes=[],native_entries=[],instructions=[],entry_sp=self.reg('esp'),thread=self.reg('ecx'),return_pc=self.read_u32(self.reg('esp')),seen_native=False,budget=self.read_u32(self.reg('esp')+4),arguments=self.read_u32s(self.reg('esp')+4,4));self._run['before']=state()
    d=self._run
    if d is None:return
    if pc==0x5516c0:
     self._clock=dict(control=self.uc.reg_read(X.UC_X86_REG_FPCW),status=self.uc.reg_read(X.UC_X86_REG_FPSW),tags=self.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(self.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)])
    if pc==0x5516f2:
     before=self._clock
     assert before is not None
     samples=struct.unpack('<qqq',self.read(0x76a280,24));scale=self.read_u32(0x5f0910)
     inputs=[before['control'],before['status'],before['tags'],*[v for pair in before['registers']for v in pair],*samples,scale]
     native=json.loads(subprocess.check_output([str(referee),*map(str,inputs)]))
     observed=self.uc.reg_read(X.UC_X86_REG_FPSW);top=(observed>>11)&7
     # Real x87 normalizes reserved FPCW bit6. This probe preserves the
     # Unicorn FPCW representation; bit6 has no arithmetic effect. Rosetta
     # canonicalizes popped empty slots, so only the live result is qualified.
     assert native['control']==before['control']|0x40
     assert native['tags']==self.uc.reg_read(X.UC_X86_REG_FPTAG)
     assert native['registers'][top]==list(self.uc.reg_read(X.UC_X86_REG_FP0+top))
     assert native['status']^observed in (0,0x20)
     if native['status']!=observed:
      assert native['status']&0x20
      self._clock_errata.append(dict(pc=pc,inputs=before,samples=list(samples),scale=scale,unicorn_status=observed,native=native))
      if not a.unqualified_unicorn:self.uc.reg_write(X.UC_X86_REG_FPSW,native['status'])
     self._clock=None
    instruction=next(decoder.disasm(self.read(pc,size),pc));d['instructions'].append(dict(pc=pc,assembly=instruction.mnemonic+' '+instruction.op_str))
    if pc==0x421061:d['before_clock']=state()
    if pc in counters:
     address=self.read_u32(self.reg('esp')+4);d['events'].append(dict(kind=counters[pc],args=[address],state=state(),value=self.read_u32s(address,2)))
    if pc==0x4210c5:
     d['opcodes'].append(dict(pc=self.read_u32(self.reg('ebp')),opcode=self.reg('eax'),state=state()))
     if a.native_return and not a.callback_return and d['seen_native']:
      d['after']=state();d['clock_errata']=list(self._clock_errata);observations.append(d);uc.emu_stop();return
     if not a.native_entry and not a.native_return and not a.callback_return and len(d['opcodes'])==a.dispatches:
      d['after']=state();d['clock_errata']=list(self._clock_errata);observations.append(d);uc.emu_stop()
    if pc in (0x47cc10,0x47c920):d['native_entries'].append(dict(target=pc,state=state()))
    if pc==0x47cc10:d['seen_native']=True
    if (a.callback_return and pc==0x425aae) or (not a.callback_return and ((not a.native_entry and not a.native_return and pc==0x420ff0) or (a.native_entry and not a.native_return and pc==0x47cc10))):
     d['after']=state();d['clock_errata']=list(self._clock_errata);observations.append(d);uc.emu_stop()
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
 out=dict(schema=2,profile=__doc__,native_entry=a.native_entry,native_return=a.native_return,callback_return=a.callback_return,dispatches=len(d['opcodes']),root_identity=result['root_identity'],snapshot_provenance=result['snapshot_provenance'],snapshot_metadata_sha256=result['snapshot_metadata_sha256'],snapshot_pages_sha256=result['snapshot_pages_sha256'],execution_profile=result['execution_profile'],comparison_policy=result['comparison_policy'],pages=[dict(address=p,bytes=initial[p].hex())for p in sorted(pages)],**d)
 out['qualified_clock_flags']=not a.unqualified_unicorn
 sources=[Path(__file__),referee_source]+[evidence/'re/oracle'/f for f in['native_osd_first_animate_probe.py','snapshot_replay.py','headless_snapshot.py','oracle.py']];out['producer_sources']={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources}
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),pages=len(pages),instructions=len(d['instructions']),opcodes=len(d['opcodes']))))
if __name__=='__main__':main()
