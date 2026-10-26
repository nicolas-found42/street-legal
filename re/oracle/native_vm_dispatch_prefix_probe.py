#!/usr/bin/env python3
"""Original41fbc0 cached zero-argument dispatch and actual Input.lastKey.

Controlled callers supply seeded GPRs and retained stack bytes. The original
method/string/signature/registry, pool, receiver, argument, result and release
helpers execute. The complete cases execute47cc10/556e00 with declared quiescent
DirectInput Acquire/GetDeviceData leaves. Allocator leaves use a retained bump
profile. Capture complete heap/stack, eight GPRs and globals at every declared
leaf and endpoint. No VM or native-result hooks. This does not prove callback
creation, Thread.run, uncached signatures, nonzero arguments or whole boot.
"""
from pathlib import Path
import hashlib, itertools, json, random, sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle, DEFAULT_EXE, CW_D3D9, STACK_TOP, STACK_SIZE
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
REGS=('eax','ebx','ecx','edx','esi','edi','ebp','esp')
ROOT=Path(__file__).resolve().parents[2]
CALLER=0x30000000
STACK=STACK_TOP-0x800
HEAP=0x8000

def capture(seed, fill, static, native, lookup=False, collision=False, prepared=False, entry=False, complete=False):
 o=Oracle();o.write(0x20000000,bytes([fill])*HEAP)
 o.write(STACK,random.Random(seed).randbytes(0x800))
 thread=o.alloc(0x38);jvm=o.alloc(0x80);cl=o.alloc(0x64);pool=o.alloc(20)
 table=o.alloc(4);constant=o.alloc(16);method=o.alloc(28);sig=o.alloc(12)
 record=o.alloc(16);name=o.alloc(12);o.write(name,b'lastKey\0');receiver=o.alloc(16)
 o.write_u32s(record,[cl,0,method,1]);o.write_u32(cl+0x14,pool)
 o.write_u32(thread+0x10,jvm);o.write_u32s(pool,[0,0,1,table,1]);o.write_u32(table,constant)
 o.write_u32s(constant,[0,0,name,7]);o.write_u32s(method,[0, (8 if static else 0)|(0x40 if native else 0),0,0,0,0,sig])
 o.write_u32s(sig,[jvm,name,0])
 if lookup:
  def cstr(text):
   p=o.alloc(len(text)+1);o.write(p,text+b'\0');return p
  module=o.alloc(0x80);pool2=o.alloc(20);table2=o.alloc(12)
  class_constant=o.alloc(16);class_name_constant=o.alloc(16);method_constant=o.alloc(16)
  class_name=cstr(b'java.io.Input');signature=cstr(b'()I')
  o.write_u32(cl+4,module);o.write_u32(module+0x24,pool2);o.write_u32(module+0x3c,1)
  o.write_u32s(pool2,[0,0,3,table2,3]);o.write_u32s(table2,[method_constant,class_constant,class_name_constant])
  o.write_u32s(class_constant,[0,4,2,0]);o.write_u32s(class_name_constant,[0,0,class_name,13]);o.write_u32s(method_constant,[0,0,name,7]);o.write_u32(sig+4,signature)
  registry=o.alloc(0x62c);o.write(registry,bytes(0x62c));o.write_u32(jvm+12,registry)
  h=(sum(b'lastKey')*73+sum(b'java.io.Input'))&0xffffffff
  candidates=[]
  if collision:
   candidates += [(cstr(b'java.io.Inupt'),name,signature),(class_name,cstr(b'lastKye'),signature),(class_name,name,cstr(b'()I'))]
  candidates += [(class_name,name,signature)]
  bucket=registry+(h&127)*12
  for i,(cn,mn,sn) in enumerate(candidates):
   node=bucket if i==0 else o.alloc(12)
   slot=o.alloc(4);record2=o.alloc(24);sig2=o.alloc(12)
   o.write_u32s(node,[h,slot,0]);o.write_u32(slot,record2)
   o.write_u32s(record2,[0,0,0x47cc10,cn,mn,sig2]);o.write_u32s(sig2,[jvm,sn,0])
   if i:o.write_u32(previous+8,node)
   previous=node
 if prepared:
  descriptor=o.alloc(4);o.write_u32(descriptor,0x60ab5c);o.write_u32(cl+8,descriptor)
  types=o.alloc(12);type_data=o.alloc(4);type_cell=o.alloc(4);o.write_u32(type_cell,0x60ab50)
  o.write_u32(type_data,type_cell);o.write_u32s(types,[1,type_data,1]);o.write_u32(sig+8,types)
  value_pool=o.alloc(20);o.write(value_pool,bytes(20));o.write_u32(0x62f130,value_pool);o.write_u32(0x62f138,0)
 if complete:
  frame=o.alloc(0x40);operands=o.alloc(12);o.write(operands,bytes(12));o.write_u32(frame+0x28,operands)
  o.write_u32(thread+0x20,frame);o.write_u32(thread+0x24,0 if static else 1);o.write_u32(thread+0x2c,0)
  device=o.alloc(4);vtable=o.alloc(0x40);o.write_u32(device,vtable);o.write_u32(vtable+0x1c,0x41b860);o.write_u32(vtable+0x28,0x41b870);o.write_u32(0x76f9b0,device)
 args=[record,cl,0 if static else receiver]
 rng=random.Random(seed);incoming={r:rng.getrandbits(32) for r in REGS[:-1]};incoming['ecx']=thread
 # Real controlled caller: seeded registers, ordinary PUSH/CALL, endpoint in callee.
 # No post-entry stack or register patch is used in the original.
 code=bytearray()
 for reg,op in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)]:
  code+=bytes([op])+incoming[reg].to_bytes(4,'little')
 for value in reversed(args):code+=b'\x68'+value.to_bytes(4,'little')
 pc=CALLER+len(code);code+=b'\xe8'+((0x41fbc0-pc-5)&0xffffffff).to_bytes(4,'little')
 ret=CALLER+len(code);o.write(CALLER,bytes(code))
 states={};stores=[];events=[];measured=False
 def state():return dict(registers={r:o.reg(r) for r in REGS},stack=list(o.read(STACK,0x800)),heap=list(o.read(0x20000000,HEAP)),heap_ptr=o.heap_ptr,globals={str(a):o.read_u32(a) for a in [0x62f130,0x62f138,0x76f9b0]})
 if prepared:
  def allocate(oc,args):
   events.append(dict(kind='allocate',args=args,state=state()));return oc.alloc(args[0])
  def realloc(oc,args):
   events.append(dict(kind='reallocate',args=args,state=state()));old,n=args;pointer=oc.alloc(n)
   if old:oc.write(pointer,oc.read(old,min(n,oc.heap_sizes[old])))
   return pointer
  def free(oc,args):events.append(dict(kind='free',args=args,state=state()))
  o.hook(0x54f560,allocate,'cdecl',1);o.hook(0x54f580,realloc,'cdecl',2);o.hook(0x54f5b0,free,'cdecl',1)
 if complete:
  def acquire(oc,args):
   events.append(dict(kind='acquire',args=args,state=state()));return 0
  def data(oc,args):
   events.append(dict(kind='data',args=args,state=state()));assert args[0]==device;oc.write_u32(args[3],0);return 0
  o.hook(0x41b860,acquire,'stdcall',1);o.hook(0x41b870,data,'stdcall',5)
 def observe(uc,pc,size,unused):
  nonlocal measured
  if pc==0x41fbc0:states['before']=state();measured=True
  if pc==(ret if complete else 0x47cc10 if entry else 0x420025 if prepared else 0x41fe04 if lookup else 0x41fc31):states['after']=state();uc.emu_stop()
 def store(uc,access,address,size,value,unused):
  if measured:stores.append([o.reg('eip'),address,size,value])
 o.uc.hook_add(UC_HOOK_CODE,observe);o.uc.hook_add(UC_HOOK_MEM_WRITE,store)
 try:o.call(CALLER,[],fpu_cw=CW_D3D9)
 except Exception:
  if 'after' not in states:raise
 assert set(states)=={'before','after'} and not o.import_log
 return dict(seed=seed,fill=fill,static=static,native=native,lookup=lookup,collision=collision,prepared=prepared,entry=entry,complete=complete,events=events,thread=thread,args=args,return_pc=ret,stores=stores,**states)

def main():
 out=Path(sys.argv[1]) if len(sys.argv)>1 else ROOT/'re/out/boot/native-vm-dispatch-prefix-original.json'
 sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
 rows=[capture(*spec) for spec in itertools.product([606,20261024],[0,0xa5],[False,True],[False,True])]
 rows += [capture(seed,fill,static,True,True,collision,prepared,entry,complete) for seed,fill,static,collision,(prepared,entry,complete) in itertools.product([606,20261024],[0,0xa5],[False,True],[False,True],[(False,False,False),(True,False,False),(True,True,False),(True,True,True)])]
 assert hashes=={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
 out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(dict(schema=1,profile=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources=hashes,cw=CW_D3D9,rows=rows),separators=(',',':'))+'\n')
 print(json.dumps(dict(output=str(out),rows=len(rows),sha256=hashlib.sha256(out.read_bytes()).hexdigest())))
if __name__=='__main__':main()
