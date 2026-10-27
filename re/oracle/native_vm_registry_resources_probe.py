#!/usr/bin/env python3
"""Original487f20 entry through all256 registrations,546070 binds and RET.

The initial JVM is constructed by original40fb50.538440 package lookup and
536820 physical-resource resolution are explicit resource-domain collaborators;
this does not prove their internals, live production root or ABI admission.
No VM method, script, native implementation or native-result body is hooked.
Complete declared heap/stack/image bytes, ordered heap/image stores, allGPRs
and retained physical x87 state are observed at every leaf and endpoint.
"""
from pathlib import Path
import argparse,json,random,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_jvm_probe as base
from native_vm_bootstrap_probe import blob
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,x86_const as X,__version__
ROOT=base.ROOT;HEAP_BYTES=0x40000;STACK_LOW=STACK_TOP-0x2000

def capture(seed,mode):
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([0xa5 if seed==606 else 0])*HEAP_BYTES);o.write(STACK_TOP-STACK_SIZE,bytes([0xa5 if seed==606 else 0])*STACK_SIZE)
 o.hook_alloc();o.hook(0x54be80,lambda oc,args:0xaabb0001,'thiscall',1)
 jvm=o.alloc(0x80);o.call(0x40fb50,[],convention='thiscall',this=jvm,fpu_cw=CW_D3D9);o.write_u32(0x63641c,jvm)
 manager=o.alloc(16);o.write_u32(0x618d48,manager)
 resources=[o.alloc(0x60),o.alloc(0x60),o.alloc(0x60)]
 for resource in resources:o.write(resource,bytes(0x60))
 links=[0x6408f8,0x6408c0];ids=[0x10008,0x10007]
 for link in links:o.write(link,bytes(16))
 # Both head removal and interior removal, followed by insertion before a head.
 if mode=='replace':
  for i,link in enumerate(links):
   prev=o.alloc(16);next_=o.alloc(16);o.write(prev,bytes(16));o.write(next_,bytes(16))
   words=[prev if i else 0,next_,0x20000+i,resources[2]]
   o.write(link,struct.pack('<4I',*words));o.write_u32(next_,link)
   if i:o.write_u32(prev+4,link)
   else:o.write_u32(resources[2]+0x48,link)
   head=o.alloc(16);o.write(head,bytes(16));o.write_u32(resources[i]+0x48,head)
 elif mode=='same':
  for link,id_ in zip(links,ids):o.write_u32(link+8,id_)
 measured=False;before=None;events=[];stores=[]
 image_ranges=[(0x618000,0x1000),(0x636000,0x1000),(0x640000,0x1000)]
 def state():return dict(heap=blob(o.read(HEAP_BASE,HEAP_BYTES)),heap_ptr=o.heap_ptr,stack=blob(o.read(STACK_LOW,0x2000)),registers={r:o.reg(r)for r in base.REGS},image=[dict(base=p,data=blob(o.read(p,n)))for p,n in image_ranges],fpu=dict(control=o.uc.reg_read(X.UC_X86_REG_FPCW),status=o.uc.reg_read(X.UC_X86_REG_FPSW),tags=o.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(o.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)]))
 def event(kind,args):
  if measured:events.append(dict(kind=kind,args=args,state=state()))
 def alloc(oc,args):event('allocate',args);p=oc.alloc(args[0]);assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES;return p
 def realloc(oc,args):
  event('reallocate',args);old,n=args;p=oc.alloc(n)
  if old:oc.write(p,oc.read(old,min(oc.heap_sizes[old],n)))
  assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES;return p
 def package(oc,args):
  event('package',args);assert oc.reg('ecx')==manager and oc.read_cstr(args[0])==b'system.rpk';assert args[1]in(7,8)
  return 0 if mode=='missing' else ids[0 if args[1]==8 else 1]
 def resolve(oc,args):
  event('resolve',args);assert oc.reg('ecx')==manager;assert args in([ids[0],7,0],[ids[1],5,0])
  return resources[0 if args[0]==ids[0]else 1]
 o.hook(0x54f560,alloc,'cdecl',1);o.hook(0x54f580,realloc,'cdecl',2);o.hook(0x538440,package,'thiscall',2);o.hook(0x536820,resolve,'thiscall',3)
 image_inputs=[dict(base=p,data=blob(o.read(p,4096)))for p in range(0x400000,0x7c0000,4096)]
 def code(uc,pc,size,_):
  nonlocal measured,before
  if pc==0x487f20:
   for i in range(8):uc.reg_write(X.UC_X86_REG_FP0+i,((1<<63)+seed+i,0x3fff+i))
   uc.reg_write(X.UC_X86_REG_FPSW,0x1021);uc.reg_write(X.UC_X86_REG_FPTAG,0xfff0)
   measured=True;before=state()
  if pc==0x30000028:uc.emu_stop()
  if measured and pc in o.import_names and pc not in o.hooks:raise RuntimeError(f'unresolved import {o.import_names[pc]}')
 def write(uc,access,address,size,value,_):
  if not measured:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+HEAP_BYTES or any(p<=address and address+size<=p+n for p,n in image_ranges):stores.append([address,size,value]);return
  if STACK_LOW<=address and address+size<=STACK_TOP:return
  raise RuntimeError(f'out-of-profile store {address:x}/{size}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
 rng=random.Random(seed);regs={r:rng.getrandbits(32)for r in base.REGS[:-1]}
 driver=b''.join(bytes([op])+struct.pack('<I',regs[r])for r,op in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)])
 driver+=b'\xe8'+struct.pack('<I',(0x487f20-0x30000028)&0xffffffff)+b'\xc3';o.write(0x30000000,driver)
 try:o.call(0x30000000,[],fpu_cw=CW_D3D9,limit=20000000)
 except Exception:
  if o.reg('eip')!=0x30000028:raise
 assert o.reg('eip')==0x30000028 and before is not None and not o.import_log
 return dict(seed=seed,mode=mode,jvm=jvm,manager=manager,resources=resources,ids=ids,before=before,after=state(),events=events,stores=stores,image_inputs=image_inputs)

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-registry-resources-original.json');a=p.parse_args()
 sources=[Path(__file__),Path(base.__file__),Path(__file__).with_name('native_vm_bootstrap_probe.py'),Path(__file__).with_name('oracle.py')]
 v=dict(schema=1,exe_sha256=base.digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=__version__,profile=__doc__,rows=[capture(seed,mode)for seed in(606,20261023)for mode in('empty','replace','same','missing')])
 a.output.write_text(json.dumps(v,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(v['rows']))))
if __name__=='__main__':main()
