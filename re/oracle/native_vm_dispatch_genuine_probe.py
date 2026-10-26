#!/usr/bin/env python3
"""Observe complete41fbc0 Input.lastKey dispatch from genuine stock Osd.animate.

The existing provenance-validated Osd snapshot replay executes unchanged stock
scripts, callback creation, Thread.run, native lookup, actual47cc10/556e00 and
41fbc0 result ownership/return. This probe adds passive observations only. Its
endpoint is the selected dispatcher return, not the callback/frame return.
Captured pages are test inputs/evidence; never production caller templates.
"""
from pathlib import Path
import argparse, hashlib, importlib, json, sys

def main():
 root=Path(__file__).resolve().parents[2]
 parser=argparse.ArgumentParser();parser.add_argument('--method',default='lastKey');parser.add_argument('--endpoint',choices=['prefix','script-frame','return'],default='return');parser.add_argument('--evidence-root',type=Path,default=root);parser.add_argument('--output',type=Path,default=root/'re/out/boot/native-vm-dispatch-genuine-original.json');args=parser.parse_args()
 evidence=args.evidence_root.resolve();sys.path.insert(0,str(evidence/'re/oracle'))
 probe=importlib.import_module('native_osd_first_animate_probe')
 from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE
 original=probe.Replay;observations=[]
 class Observe(original):
  def __init__(self,name):
   super().__init__(name);self._dispatch=None
   def state():
    d=self._dispatch
    return dict(registers={r:self.reg(r) for r in ('eax','ebx','ecx','edx','esi','edi','ebp','esp')},heap_ptr=self.heap_ptr,changed={str(p):self.read(p,4096).hex() for p in sorted(d['dirty'])})
   def code(uc,pc,size,unused):
    if self._dispatch is None and pc==0x41fbc0:
     sp=self.reg('esp');record=self.read_u32(sp+4);cl=self.read_u32(record);method=self.read_u32(record+8);pool=self.read_u32(cl+0x14);index=self.read_u32(method+8)
     method_name=self.read_cstr(self.read_u32(self.read_u32(self.read_u32(pool+12)+index*4)+8))
     if method_name!=args.method.encode('ascii'):return
     assert bool(self.read_u32(method+4)&0x40)==(args.method=='lastKey')
     initial={}
     for first,last in [(0x400000,0x7c0000),(0x20000000,(self.heap_ptr+0x1ffff)&~4095),(0x7fe00000,0x7ff00000)]:
      for p in range(first,last,4096):initial[p]=self.read(p,4096)
     self._dispatch=dict(initial=initial,pages=set(),dirty=set(),events=[],entry_sp=sp,return_pc=self.read_u32(sp),args=self.read_u32s(sp+4,3),thread=self.reg('ecx'))
     self._dispatch['before']=state()
    d=self._dispatch
    if d is not None and ((args.endpoint in ('prefix','script-frame') and pc=={'prefix':0x41fc31,'script-frame':0x4202fe}[args.endpoint]) or (args.endpoint=='return' and pc==d['return_pc'] and self.reg('esp')>d['entry_sp'])):
     d['after']=state();observations.append(d);uc.emu_stop()
   def access(uc,access,address,size,value,unused):
    d=self._dispatch
    if d is None:return
    for p in range(address&~4095,(address+size+4095)&~4095,4096):
     if p not in d['initial']:raise RuntimeError(f'outside declared initial page profile: {p:x}')
     d['pages'].add(p)
   def write(uc,access_,address,size,value,unused):
    access(uc,access_,address,size,value,unused)
    if self._dispatch is not None:
     self._dispatch['dirty'].update(range(address&~4095,(address+size+4095)&~4095,4096))
   self.uc.hook_add(UC_HOOK_CODE,code);self.uc.hook_add(UC_HOOK_MEM_READ,access);self.uc.hook_add(UC_HOOK_MEM_WRITE,write)
   # The outer probe installs its declared environmental leaves after this
   # constructor. Wrap them without changing results, memory or registers.
   inherited_hook=self.hook
   def hook(address,fn,conv='cdecl',nargs=0):
    def observed(oc,words):
     d=self._dispatch
     if d is not None:
      d['events'].append(dict(target=address,args=words,state=state()))
     return fn(oc,words)
    return inherited_hook(address,observed,conv,nargs)
   self.hook=hook
   # Replay installed malloc/free before wrappers could be observed.
   for address in [0x54f560,0x54f5b0]:
    fn,conv,nargs=self.hooks[address];hook(address,fn,conv,nargs)
 probe.Replay=Observe
 result=probe.capture(endpoint='callback-return')
 assert len(observations)==1,'dispatcher return not reached'
 d=observations[0];pages=d['pages']|d['dirty']
 if args.endpoint=='return':assert d['after']['registers']['eax']==(1 if args.method=='lastKey' else 0)
 out=dict(schema=1,profile=__doc__,method=args.method,endpoint=args.endpoint,root_identity=result['root_identity'],snapshot_provenance=result['snapshot_provenance'],snapshot_metadata_sha256=result['snapshot_metadata_sha256'],snapshot_pages_sha256=result['snapshot_pages_sha256'],fpu_cw=result['fpu_cw'],comparison_policy=result['comparison_policy'],execution_profile=result['execution_profile'],before=d['before'],after=d['after'],args=d['args'],thread=d['thread'],entry_sp=d['entry_sp'],return_pc=d['return_pc'],events=d['events'],pages=[dict(address=p,bytes=d['initial'][p].hex())for p in sorted(pages)])
 sources=[Path(__file__)]+[evidence/'re/oracle'/f for f in ['native_osd_first_animate_probe.py','snapshot_replay.py','headless_snapshot.py','oracle.py']]
 out['producer_sources']={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(args.output),pages=len(pages),leaves=len(d['events']),sha256=hashlib.sha256(args.output.read_bytes()).hexdigest())))
if __name__=='__main__':main()
