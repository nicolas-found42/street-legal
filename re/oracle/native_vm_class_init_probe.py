#!/usr/bin/env python3
"""Source403e80 through403fd9, before recursive dependency initialization.

Cached base/interface records are built by original403a90 and CONS loaders.
The measured prefix executes all original lookup, allocation and vector code.
Heap allocator/free leaves and, in RID profiles, an explicit resource-table
lookup collaborator are supplied. No VM/native/script result is replaced.
Compare every declared heap and
stack byte, heap cursor and eight GPRs at entry, allocator leaves and endpoint.
Captures are test inputs, never runtime state templates. The initializer
profile qualifies Unicorn's missing I->F precision/C1 transition against native
x87 FILD/FSTP. The default applies only that independently measured FPSW
transition at417e35; --unqualified-unicorn retains the raw emulator observation.
GPRs, memory, opcodes and native/script results are never patched or masked.
"""
from pathlib import Path
import hashlib,json,random,struct,sys,platform,subprocess
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE,STACK_TOP
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn import x86_const as X
ROOT=Path(__file__).resolve().parents[2]
REFEREE=ROOT/'re/out/native-abi/class-init-fpu-referee'
REGS=('eax','ebx','ecx','edx','esi','edi','ebp','esp')
STACK=STACK_TOP-0x4000;CALLER=0x30000000

def u(x):return struct.pack('<I',x&0xffffffff)
def capture(seed,base,interfaces,static_owner=False,complete=False,cold=False,fields=0,initializers=False,stock=False,constructor=False,constructor_name=None,array_prefix=False,array_complete=False,array_object=False):
 instance='--instance'in sys.argv
 stock_osd='--stock-osd'in sys.argv
 parent_callback='--parent-callback'in sys.argv
 sample_counter=1000000 if parent_callback else 1000512
 call_constructor='--call-constructor'in sys.argv
 stock_resource='--stock-resource'in sys.argv or stock_osd
 constructor_argument='--constructor-argument'in sys.argv or stock_resource
 stock_vector='--stock-vector'in sys.argv
 stock_class=stock_vector or stock_resource
 stock_name='java.util.resource.ResourceRef'if stock_resource else'java.util.Vector'
 s=Scenario();o=s.o
 for address in [0x406f60,0x406140,0x551140]:o.hooks.pop(address,None)
 if stock:
  o.hook(0x54be80,lambda oc,args:0,'thiscall',1)
  o.call(0x40fb50,[],convention='thiscall',this=s.jvm,fpu_cw=CW);o.hooks.pop(0x54be80)
  stopped=False
  def boot_prefix(uc,address,size,_):
   nonlocal stopped
   if address==0x40ff66:stopped=True;uc.emu_stop()
  hook=o.uc.hook_add(UC_HOOK_CODE,boot_prefix)
  try:o.call(0x40fe70,[],convention='thiscall',this=s.jvm,fpu_cw=CW)
  except Exception:
   if not stopped:raise
  assert stopped;o.uc.hook_del(hook)

 else:
  if instance:
   o.hook(0x54be80,lambda oc,args:0,'thiscall',1)
   o.call(0x40fb50,[],convention='thiscall',this=s.jvm,fpu_cw=CW);o.hooks.pop(0x54be80)
  holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
  cache=o.alloc(0x1828);o.write_u32(s.jvm+8,cache)
 stock_files=[];stock_modules=[]
 if stock:
  for relative in (['system/Scripts/lang/Object.class','system/Scripts/lang/String.class','system/Scripts/lang/Native.class','system/Scripts/lang/Runnable.class','system/Scripts/util/resource/ResourceRef.class','system/Scripts/util/resource/GameRef.class','system/Scripts/lang/GameType.class','system/Scripts/util/Vector.class','system/Scripts/render/Osd.class']if stock_osd else['system/scripts/lang/Object.class','system/scripts/io/Controller.class','system/scripts/io/Input.class']):
   data=(ROOT/'stock'/relative).read_bytes();source=o.alloc(len(data));o.write(source,data)
   cl=o.call(0x411b20,[source,len(data)],convention='thiscall',this=s.jvm,fpu_cw=CW).eax
   stock_files.append(dict(path=relative,sha256=hashlib.sha256(data).hexdigest()));stock_modules.append(o.read_u32(cl+4))
  if parent_callback:
   for relative in ['system/Scripts/io/Controller.class','system/Scripts/io/Input.class','system/Scripts/lang/System.class','system/Scripts/render/Frontend.class']:
    data=(ROOT/'stock'/relative).read_bytes();source=o.alloc(len(data));o.write(source,data)
    loaded=o.call(0x411b20,[source,len(data)],convention='thiscall',this=s.jvm,fpu_cw=CW).eax
    stock_files.append(dict(path=relative,sha256=hashlib.sha256(data).hexdigest()));stock_modules.append(o.read_u32(loaded+4))
  module=o.read_u32(cl+4);fields=o.read_u32(o.read_u32(module+0x54)+12)
  for global_,source in [(0x62def0,0x60ab50),(0x62e00c,0x60ab54),(0x62deec,0x60ab58),(0x62e000,0x60ab5c),(0x62e010,0x60ab60)]:o.call(0x423e10,[s.jvm,source],convention='thiscall',this=o.read_u32(global_),fpu_cw=CW)
 else:
  names=['Root','Base','IfaceA','IfaceB'];records=[]
  data=u(len(names)*2+((9 if stock_class else 8 if call_constructor else 7 if '--resource-literal'in sys.argv else 6 if '--new-instance'in sys.argv else 5) if fields else 0))
  for i,name in enumerate(names):
   name=name.encode();data+=u(0)+u(len(name))+name+b'\0'+u(4)+u(i*2)
  if fields:
   for name in [b'I',b'F',b'fieldA',b'fieldB',b'fieldC']+([('L'+stock_name+';').encode()if stock_class else b'LBase;',b'<init>',b'(I)'if constructor_argument else b'()']+([stock_name.encode()]if stock_class else[])if call_constructor else[b'LBase;']if '--new-instance'in sys.argv else[b'example.rpk']if '--resource-literal'in sys.argv else[]):data+=u(0)+u(len(name))+name+b'\0'
  if '--resource-literal'in sys.argv:data+=u(3)+u(0xffffffff if '--resource-null-package'in sys.argv else 13)+u(0x1234)
  for i,name in enumerate(names):
   source=o.alloc(len(data));o.write(source,data);pool=o.alloc(20)
   o.call(0x405d20,[],convention='thiscall',this=pool,fpu_cw=CW)
   o.call(0x407660,[source],convention='thiscall',this=pool,fpu_cw=CW)
   module=o.alloc(0x64);o.write_u32(module+0xc,s.jvm);o.write_u32(module+0x24,pool);o.write_u32(module+0x3c,i*2+1);o.write_u32(module+0x38,int(i>=2));o.write_u32(module+0x40,0xffffffff);o.write_u32(module+0x60,o.alloc(24))
   cl=o.alloc(0x1d8);o.call(0x403a90,[s.jvm,module],convention='thiscall',this=cl,fpu_cw=CW);records.append((cl,module))
  cl,module=records[0];o.write_u32(module+0x40,3 if base else 0xffffffff)
  o.write_u32(module+0x44,interfaces);indices=o.alloc(max(4,interfaces*4));o.write_u32s(indices,[5,7][:interfaces]);o.write_u32(module+0x48,indices)
  if static_owner and not cold:
   for dep,_ in records[1:]:o.call(0x403e80,[],convention='thiscall',this=dep,fpu_cw=CW)
  if fields:
   for global_,source in [(0x62def0,0x60ab50),(0x62e00c,0x60ab54),(0x62deec,0x60ab58),(0x62e000,0x60ab5c),(0x62e010,0x60ab60)]:o.call(0x423e10,[s.jvm,source],convention='thiscall',this=o.read_u32(global_),fpu_cw=CW)
   members=o.alloc(16);table=o.alloc(fields*4);o.write_u32s(members,[0x5f08b8,fields,table,fields]);o.write_u32(module+(0x4c if instance else 0x54),members)
   for i in range(fields):
    record=o.alloc(20);o.write_u32s(record,[0x5f0890,0,10+i,13 if i==2 and ('--new-instance'in sys.argv or call_constructor) else 9 if i==2 and '--float-literal'in sys.argv else 8+i%2,i if initializers else 0xffffffff]);o.write_u32(table+i*4,record)
  if initializers:
   tree=o.read_u32(module+0x60);table=o.alloc(fields*4);o.write_u32s(tree,[fields,table,fields])
   for i in range(fields):
    pc=o.alloc((56 if constructor_argument else 40) if i==2 and call_constructor else 24);o.write_u32s(pc,([0x1007,0x1e,0x401c,16 if stock_class else 2,0x1007,0x1c,0x100b,(42 if '--bound-resource'in sys.argv else 0) if stock_resource else 42,0x1027,1,0x21,0,0x10,0]if constructor_argument else[0x1007,0x1e,0x401c,16 if stock_class else 2,0x1027,0,0x21,0,0x10,0])if i==2 and call_constructor else[0x1007,0x1e,0x401c,2,0x10,0]if i==2 and '--new-instance'in sys.argv else[0x1007,0x1c,0x200a,0x40af5c29 if seed==606 else 0x3fbc0830,0x10,0]if i==2 and '--float-literal'in sys.argv else[0x1007,0x1c,0x100e,13 if '--resource-wrong-kind'in sys.argv else 0xffffffff if '--resource-bad-index'in sys.argv else 14,0x10,0]if i==2 and '--resource-literal'in sys.argv else[0x1007,0x1c,0x100b,[seed,-seed,0x112233][i],0x10,0]);o.write_u32(table+i*4,pc)
 if call_constructor and not stock:
  for _,owner in records:
   for off in [0x50,0x58]:
    members=o.alloc(16);o.write_u32s(members,[0x5f08a8,0,0,0]);o.write_u32(owner+off,members)
  base_module=records[1][1];members=o.alloc(16);table=o.alloc(4);method=o.alloc(32)
  o.write_u32s(method,[0x5f08d0,1,14,15,0,2 if constructor_argument else 1,0,0]);o.write_u32(table,method);o.write_u32s(members,[0x5f08a8,1,table,1]);o.write_u32(base_module+0x50,members)
  tree=o.read_u32(base_module+0x60);table=o.alloc(4);pc=o.alloc(24 if constructor_argument else 16);o.write_u32s(pc,[0x1003,0,0x1003,0,0x2a,0]if constructor_argument else[0x1003,0,0x2a,0]);o.write_u32(table,pc);o.write_u32s(tree,[1,table,1])
 if stock_class and not stock:
  for relative in (['system/Scripts/lang/Object.class','system/Scripts/lang/String.class','system/Scripts/lang/Native.class','system/Scripts/util/resource/ResourceRef.class']if stock_resource else['system/Scripts/lang/Object.class','system/Scripts/util/Vector.class']):
   raw=(ROOT/'stock'/relative).read_bytes();source=o.alloc(len(raw));o.write(source,raw)
   o.call(0x411b20,[source,len(raw)],convention='thiscall',this=s.jvm,fpu_cw=CW)
   stock_files.append(dict(path=relative,sha256=hashlib.sha256(raw).hexdigest()))
 if stock_resource:
  o.write_u32(0x62e008,0)
  for method,sig,target in [('newNative','()V',0x47cea0),('set','(I)V',0x47cf80)]:
   pointers=[]
   for value in [stock_name,method,sig]:
    text=value.encode()+b'\0';pointer=o.alloc(len(text));o.write(pointer,text);pointers.append(pointer)
   o.call(0x416b00,[*pointers,target],convention='thiscall',this=s.jvm,fpu_cw=CW)
 if stock_resource and '--bound-resource'in sys.argv:
  manager=o.alloc(16);resource=o.alloc(0x180);o.write_u32(resource+0x50,42);o.write_u32(0x618d48,manager)
 if stock_osd:
  manager=o.alloc(16);o.write_u32(0x618d48,manager);resources={};containers={}
  for owner_module in stock_modules:
   pool=o.read_u32(owner_module+0x24)
   for i in range(o.read_u32(pool+16)):
    member=o.read_u32(o.read_u32(pool+12)+i*4)
    if o.read_u32(member+4)!=3:continue
    package_index=o.read_u32(member+12);package=None if package_index==0xffffffff else o.read_cstr(o.read_u32(o.read_u32(o.read_u32(pool+12)+package_index*4)+8)).decode()
    key=(package,o.read_u32(member+8))
    if key in resources:continue
    rid=0x71000000+len(resources)+1;resources[key]=rid
    container=o.alloc(0x180);o.write_u32(container+0x50,rid);containers[rid]=container
 if parent_callback:
  o.write_u32(0x63641c,s.jvm)
  for cls,method,sig,implementation in [('java.io.Input','lastKey','()I',0x47cc10),('java.lang.System','currentTime','()F',0x47bfe0)]:
   pointers=[]
   for value in [cls,method,sig]:
    text=value.encode()+b'\0';pointer=o.alloc(len(text));o.write(pointer,text);pointers.append(pointer)
   o.call(0x416b00,[*pointers,implementation],convention='thiscall',this=s.jvm,fpu_cw=CW)
  context=o.alloc(0x200);resource_parent=o.alloc(0x180);envelope=resource_parent+0x80;parent_ref=o.alloc(16);name=o.alloc(16);o.write(name,b'caller-osd\0')
  for off in [0,0x1c,0x38]:o.call(0x428fd0,[],convention='thiscall',this=context+off,fpu_cw=CW)
  o.write_u32s(resource_parent,[0x5f38a8]);o.write_u32(resource_parent+0x4c,1);o.write_u32(resource_parent+0x50,0x7100ff00);o.write_u32(resource_parent+0x60,name);o.write_u32(resource_parent+0x64,1);o.write_u32(resource_parent+0x68,0x3f800000);o.write_u32(resource_parent+0x6c,0x3f800000);o.write_u32(resource_parent+0x74,o.read_u32(0x6200a4))
  o.call(0x429060,[resource_parent],convention='thiscall',this=envelope+0x38,fpu_cw=CW);o.call(0x429060,[resource_parent],convention='thiscall',this=parent_ref,fpu_cw=CW);o.call(0x427370,[parent_ref,0x1c],convention='thiscall',this=context,fpu_cw=CW)
  device=o.alloc(4);vtable=o.alloc(0x40);o.write_u32(device,vtable);o.write_u32(vtable+0x1c,0x41b860);o.write_u32(vtable+0x28,0x41b870);o.write_u32(0x76f9b0,device)
 if constructor:
  if constructor_name is not None:
   name=constructor_name.encode();data=u(2)+u(0)+u(len(name))+name+b'\0'+u(4)+u(0);source=o.alloc(len(data));o.write(source,data);pool=o.alloc(20)
   o.call(0x405d20,[],convention='thiscall',this=pool,fpu_cw=CW);o.call(0x407660,[source],convention='thiscall',this=pool,fpu_cw=CW)
   module=o.alloc(0x64);o.write_u32(module+0xc,s.jvm);o.write_u32(module+0x24,pool);o.write_u32(module+0x3c,1);o.write_u32(module+0x40,0xffffffff);o.write_u32(module+0x60,o.alloc(24))
  cl=o.alloc(0x1d8)
 if initializers:
  o.hooks.pop(0x5516c0,None);o.write(0x76a280,struct.pack('<qqq',1024,1000000,1000000))
 cached='--array-cached'in sys.argv
 cached_descriptor=0
 if cached:
  def numeric(value):
   def sample(oc,args):oc.write(args[0],struct.pack('<q',value));return 1
   return sample
  o.hook(o.read_u32(0x5e7050),numeric(1024),'stdcall',1);o.hook(o.read_u32(0x5e7048),numeric(1000512),'stdcall',1)
  o.call(0x403e80,[],convention='thiscall',this=cl,fpu_cw=CW)
  owner=o.read_u32(cl+0x1d0);variables=o.read_u32(owner+12);table=o.read_u32(variables+4);field=o.read_u32(table+(152 if '--primitive-array'in sys.argv else 151)*4);obj=o.read_u32(field+8);array_class=o.read_u32(obj+12);cached_descriptor=o.read_u32(o.read_u32(array_class+8));cl=s.jvm
 rng=random.Random(seed);o.write(STACK,rng.randbytes(0x4000))
 incoming={r:rng.getrandbits(32)for r in REGS[:-1]};incoming['ecx']=cl
 code=bytearray()
 for r,opcode in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)]:code+=bytes([opcode])+u(incoming[r])
 target=0x404e20 if '--instance-default'in sys.argv else 0x404e60 if instance else 0x4109e0 if cached else 0x403a90 if constructor else 0x403e80
 if instance and '--instance-default'not in sys.argv:code+=b'\x68'+u(0)
 elif constructor:code+=b'\x68'+u(module)+b'\x68'+u(s.jvm)
 elif cached:code+=b'\x68'+u(cached_descriptor)
 at=CALLER+len(code);code+=b'\xe8'+u(target-at-5);ret=CALLER+len(code);constructor_return=ret
 if parent_callback:
  code+=b'\xa3'+u(envelope+0x50)+b'\xb9'+u(context)+b'\x68'+u(0x3c888889)
  at=CALLER+len(code);code+=b'\xe8'+u(0x426fd0-at-5);ret=CALLER+len(code)
 o.write(CALLER,code)
 errata=[];conversion=None;started=False;entry_name=0;entry_return=ret;array_seen=0;entry_this=0
 limit=(o.heap_ptr+(0x400000 if stock_resource else 0x150000 if fields or instance else 0x2000)+0xfff)&~0xfff;events=[];states={}
 areas=[('heap',HEAP_BASE,limit-HEAP_BASE),('stack',STACK,0x4000),('globals',0x62ddf0,0x1350),('constants',0x607540,0x100),('timer',0x76a280,24),('format',0x607500,0x40),('instance_constants',0x607900,0x200),('signature_constants',0x60c060,0x30),('constructor_constants',0x60c650,0x30),('resource_globals',0x618d48,4),('callback_constants',0x60c960,0x80),('callback_names',0x60cae0,0x20),('keyboard_globals',0x76f9b0,4),('clock_scale',0x5f10a4,4),('resource_epoch',0x6200a4,4),('quality_kinds',0x619dc0,0x80),('callback_globals',0x636418,0x10)]
 baseline=None
 def state():
  nonlocal baseline
  memory={key:o.read(address,n)for key,address,n in areas}
  out=dict(registers={r:o.reg(r)for r in REGS},heap_ptr=o.heap_ptr,fpu=dict(control=o.uc.reg_read(X.UC_X86_REG_FPCW),status=o.uc.reg_read(X.UC_X86_REG_FPSW),tags=o.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(o.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)]))
  if baseline is None:
   baseline=memory;out.update({k:v.hex()for k,v in memory.items()})
  else:
   out['patches']={k:[[i,data[i:i+4096].hex()]for i in range(0,len(data),4096)if data[i:i+4096]!=baseline[k][i:i+4096]]for k,data in memory.items()}
  return out
 def leaf(kind):
  def run(oc,args):
   event=dict(kind=kind,args=args,state=state())if started else None
   if event is not None:events.append(event)
   if kind=='allocate':p=oc.alloc(args[0])
   elif kind=='reallocate':
    old,n=args;p=oc.alloc(max(1,n))
    if old:oc.write(p,oc.read(old,min(oc.heap_sizes[old],n)))
   else:p=0
   if event is not None:event['result']=p
   assert o.heap_ptr<=limit, (hex(o.heap_ptr),hex(limit),kind,args,hex(oc.reg("eip")))
   return p
  return run
 if initializers:
  def platform(kind,value):
   def run(oc,args):
    if started:events.append(dict(kind=kind,args=args,state=state(),result=1))
    oc.write(args[0],struct.pack('<q',value));return 1
   return run
  o.hook(o.read_u32(0x5e7050),platform('frequency',1024),'stdcall',1);o.hook(o.read_u32(0x5e7048),platform('counter',sample_counter),'stdcall',1)
 o.hook(0x54f560,leaf('allocate'),'cdecl',1);o.hook(0x54f580,leaf('reallocate'),'cdecl',2);o.hook(0x54f5b0,leaf('free'),'cdecl',1)
 if '--resource-literal'in sys.argv:
  def resource(oc,args):
   assert args[1]==0x1234
   assert args[0]==0 or oc.read_cstr(args[0])==b'example.rpk'
   result=args[1]^(0x5a5a0000 if args[0]==0 else 0xa5a50000)
   if started:events.append(dict(kind='resource',args=args,state=state(),result=result))
   return result
  o.hook(0x4122b0,resource,'cdecl',2)
 if stock_resource and '--bound-resource'in sys.argv:
  def resolve(oc,args):
   assert args==[42,0,0]
   if started:events.append(dict(kind='resolve-resource',args=args,state=state(),result=resource))
   return resource
  o.hook(0x536820,resolve,'thiscall',3)
 if stock_osd:
  def resource_lookup(oc,args):
   package=None if args[0]==0 else oc.read_cstr(args[0]).decode();result=resources[(package,args[1])]
   if started:events.append(dict(kind='resource',args=args,state=state(),result=result))
   return result
  def resolve_osd(oc,args):
   assert args[1:]==[0,0];result=containers[args[0]]
   if started:events.append(dict(kind='resolve-resource',args=args,state=state(),result=result))
   return result
  o.hook(0x4122b0,resource_lookup,'cdecl',2);o.hook(0x536820,resolve_osd,'thiscall',3)
 if parent_callback:
  def acquire(oc,args):
   if started:events.append(dict(kind='keyboard-acquire',args=args,state=state(),result=0))
   return 0
  def data(oc,args):
   if started:events.append(dict(kind='keyboard-data',args=args,state=state(),result=0))
   oc.write_u32(args[3],0);return 0
  o.hook(0x41b860,acquire,'stdcall',1);o.hook(0x41b870,data,'stdcall',5)
 def observe(uc,address,size,_):
  nonlocal conversion,started,entry_name,entry_return,array_seen,entry_this
  if parent_callback and started and address in [constructor_return,0x426fd0]:events.append(dict(kind='constructed-osd'if address==constructor_return else'parent-entry',args=[],state=state()))
  if parent_callback and started and address in [0x425aae,0x427111]:events.append(dict(kind='callback-setup-return'if address==0x425aae else'callback-wrapper-return',args=[],state=state()))
  if stock_resource and started and address in [0x47cea0,0x47cf80,0x47cc10,0x47bfe0]:events.append(dict(kind='native-entry',target=address,args=[o.read_u32(o.reg('esp')+4)],state=state()))
  if initializers and address==0x417e32:
   value=struct.unpack('<i',o.read(o.reg('esi')+8,4))[0]
   conversion=json.loads(subprocess.check_output([str(REFEREE),hex(CW),str(value)]));conversion['value']=value
  if conversion is not None and address==0x417e35:
   assert o.read_u32(o.reg('esi')+8)==conversion['bits']
   observed=uc.reg_read(X.UC_X86_REG_FPSW);assert conversion['status']&~0x220==0
   corrected=(observed&~0x220)|(conversion['status']&0x220)|(observed&0x20)
   if corrected!=observed:
    errata.append(dict(pc=address,unicorn_status=observed,native_status=corrected,conversion=conversion))
    if '--unqualified-unicorn'not in sys.argv:uc.reg_write(X.UC_X86_REG_FPSW,corrected)
   conversion=None
  if array_prefix and address==(0x404e60 if array_object else 0x4109e0):array_seen+=1
  if 'before'not in states and ((array_prefix and address==(0x404e60 if array_object else 0x4109e0) and array_seen==(2 if '--primitive-array'in sys.argv else 1))or(not array_prefix and address==target and o.reg('ecx')==cl)):
   started=True;entry_this=o.reg('ecx');entry_name=o.read_u32(o.reg('esp')+4)if array_prefix else 0;entry_return=o.read_u32(o.reg('esp'));states['before']=state()
  if started and ((array_prefix and address==(entry_return if array_complete else 0x410bdd))or(not array_prefix and ((complete and address==ret)or(not complete and address==(0x404377 if static_owner else 0x403fd9)and o.reg('edi')==cl)))):states['after']=state();uc.emu_stop()
 def writes(uc,access,address,n,value,_):
  if not started:return
  if HEAP_BASE<=address<0x30000000:assert address+n<=limit;return
  if STACK<=address and address+n<=STACK_TOP:return
  if 0x62ddf0<=address and address+n<=0x62f140:return
  if 0x76a280<=address and address+n<=0x76a298:return
  raise AssertionError(f'out-of-profile CPU write {address:x}/{n} at {o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_MEM_WRITE,writes)
 o.uc.hook_add(UC_HOOK_CODE,observe);o.uc.ctl_remove_cache(0x400000,0x600000)
 try:o.call(CALLER,[],fpu_cw=CW)
 except Exception:
  if 'after' not in states:raise
 assert len(states)==2 and not o.import_log
 return dict(instance=instance,seed=seed,base=base,interfaces=interfaces,static_owner=static_owner,complete=complete,cold=cold,fields=fields,initializers=initializers,errata=errata,qualified='--unqualified-unicorn' not in sys.argv,stock=stock_files,constructor=constructor,constructor_name=constructor_name,array_prefix=array_prefix,array_complete=array_complete,array_object=array_object,entry_this=entry_this,entry_name=entry_name,module=module,parent_callback=parent_callback,parent_context=context if parent_callback else 0,parent_envelope=envelope if parent_callback else 0,parent_return=ret,platform=dict(frequency=1024,counter=sample_counter),jvm=s.jvm,cl=cl,return_pc=entry_return,events=events,**states)
def main():
 output=Path(sys.argv[1])
 c=Path(__file__).with_name('class_init_fpu_referee.c')
 if '--stock-osd'in sys.argv or '--initializers'in sys.argv or '--stock-input'in sys.argv or '--array-prefix'in sys.argv or '--array-class'in sys.argv or '--array-object'in sys.argv or '--array-cached'in sys.argv:
  subprocess.run(['clang','-O0']+(['-arch','x86_64']if platform.system()=='Darwin'else[])+['-o',str(REFEREE),str(c)],check=True)
 sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py'),c]
 if '--stock-osd'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True)for seed in [606,20261025]]
 elif '--stock-resource'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,3,True)for seed in [606,20261025]]
 elif '--instance'in sys.argv:
  rows=[capture(seed,base,n,True,True,True,3 if '--fields'in sys.argv else 0,'--initializers'in sys.argv)for seed in [606,20261025]for base in [False,True]for n in [0,1,2]]
 elif '--array-cached'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True,False,None,True,True)for seed in [606,20261025]]
 elif '--array-object'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True,False,None,True,True,True)for seed in [606,20261025]]
 elif '--array-class'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True,False,None,True,True)for seed in [606,20261025]]
 elif '--array-prefix'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True,False,None,True)for seed in [606,20261025]]
 elif '--class-constructor'in sys.argv:
  rows=[capture(seed,False,0,False,True,False,0,False,False,True,name)for seed in [606,20261025]for name in [None,"FreshClass","[F","[java.io.Controller"]]
 elif '--stock-input'in sys.argv:
  rows=[capture(seed,False,0,True,True,True,0,True,True)for seed in [606,20261025]]
 else:
  rows=[capture(seed,base,n,'--static-owner'in sys.argv or '--complete'in sys.argv,'--complete'in sys.argv,'--cold-dependencies'in sys.argv,3 if '--fields'in sys.argv else 0,'--initializers'in sys.argv)for seed in [606,20261025]for base in ([False]if '--fields'in sys.argv else[False,True])for n in ([0]if '--fields'in sys.argv else[0,1,2])]
 output.write_text(json.dumps(dict(schema=3,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},rows=rows),separators=(',',':'))+'\n')
 print(json.dumps(dict(output=str(output),rows=len(rows))))
if __name__=='__main__':main()
