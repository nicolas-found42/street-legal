#!/usr/bin/env python3
"""Original cold JVM constructor/bootstrap and487f20 native catalog prefix.

The explicit machine driver publishes its constructed JVM and calls487f20.
All256 native registrations execute their original bodies, with no VM metadata
preloads or registry/name/signature result hooks. The source caller remains
entered at489e20, before the two resource-binding suffix operations.
"""
from pathlib import Path
import argparse,json,sys,itertools,random,struct,fnmatch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_class_cache_probe as cache
base=cache.base
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,UC_HOOK_MEM_READ,x86_const as X
ROOT=base.ROOT;REGS=base.REGS;HEAP_BYTES=base.HEAP_BYTES;STACK_LOW=base.STACK_LOW;IMAGE_RANGES=base.IMAGE_RANGES
CaptureFailure=base.CaptureFailure;blob=base.blob
def capture(spec, requests=()):
 heap_bytes=0x100000 if requests else HEAP_BYTES
 endpoint_pc=0x489e20
 install=cache.install
 image_ranges=IMAGE_RANGES if install is None else [(0x62d000,0x3000),(0x766000,0x3000),(0x777000,0x8000),(0x61b000,0x1000),(0x636000,0x1000)]
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*heap_bytes);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 o.heap_ptr=HEAP_BASE+spec['gap'];this=o.alloc(0x80);events=[];stores=[];files=[];phases=[];phase='descriptors';finds={};find_id=0;logging=0;measured=False;endpoint=False;pages={};before=None
 rng=random.Random(spec['seed']);regs={r:rng.getrandbits(32) for r in REGS[:-1]};regs['ecx']=this
 def state():return dict(heap=blob(o.read(HEAP_BASE,heap_bytes)),heap_ptr=o.heap_ptr,stack=blob(o.read(STACK_LOW,0x3000)),registers={r:o.reg(r) for r in REGS},fpu_cw=o.uc.reg_read(X.UC_X86_REG_FPCW),fpu_depth=o._fpu_depth(),fpu=dict(control=o.uc.reg_read(X.UC_X86_REG_FPCW),status=o.uc.reg_read(X.UC_X86_REG_FPSW),tags=o.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(o.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)]),image=[dict(base=p,data=blob(o.read(p,n))) for p,n in image_ranges])
 def event(kind,args):
  if measured:events.append(dict(kind=kind,args=args,phase=phase,state=state()))
 def alloc(oc,args):
  event('allocate',args);p=oc.alloc(args[0]);assert oc.heap_ptr<=HEAP_BASE+heap_bytes;return p
 def realloc(oc,args):
  event('reallocate',args);old,n=args;p=oc.alloc(n);assert oc.heap_ptr<=HEAP_BASE+heap_bytes
  if old:oc.write(p,oc.read(old,min(n,oc.heap_sizes[old])))
  return p
 def free(oc,args):event('free',args);return 0
 def logger(oc,args):
  nonlocal logging
  event('logger',args);logging+=1;return 0xaabb0000+logging
 def resolve_stock(text):
  parts=text.replace('\\','/').split('/');p=ROOT/'stock'
  for part in parts:
   if not part:continue
   if part in ('.','..'):raise RuntimeError('unsafe stock path')
   children=[c for c in p.iterdir() if c.name.lower()==part.lower()]
   if len(children)!=1:raise RuntimeError(f'ambiguous/missing stock component {text}')
   p=children[0];assert p.resolve().is_relative_to((ROOT/'stock').resolve())
  return p
 def write_find(dest,entry):
  name,dir_,size=entry;oc=o;data=bytearray(320);data[:4]=(16 if dir_ else 32).to_bytes(4,'little');data[32:36]=size.to_bytes(4,'little');b=name.encode('cp1252');assert len(b)<260;data[44:44+len(b)]=b;oc.write(dest,data)
 def first(oc,args):
  nonlocal find_id
  event('FindFirstFileA',args);pattern=oc.read_cstr(args[0]).decode('cp1252').replace('\\','/');folder,mask=pattern.rsplit('/',1);assert mask in ('*','*.*','*.java','*.class')
  try:directory=resolve_stock(folder)
  except RuntimeError:
   files.append(dict(pattern=pattern,entries=[],missing=True));return 0xffffffff
  entries=[('.',True,0),('..',True,0)]+[(p.name,p.is_dir(),0 if p.is_dir() else p.stat().st_size) for p in directory.iterdir()]
  assert len({n.lower() for n,d,s in entries})==len(entries);entries=[e for e in entries if mask in ('*','*.*') or fnmatch.fnmatchcase(e[0].lower(),mask.lower())]
  if not entries:
   files.append(dict(pattern=pattern,entries=[]));return 0xffffffff
  entries.sort(key=lambda e:e[0].lower());files.append(dict(pattern=pattern,entries=entries));find_id+=1;finds[find_id]=entries[1:];write_find(args[1],entries[0]);return find_id
 def next_(oc,args):
  event('FindNextFileA',args);rows=finds[args[0]]
  if not rows:return 0
  write_find(args[1],rows.pop(0));return 1
 def close(oc,args):event('FindClose',args);del finds[args[0]];return 1
 for address,fn,n,conv in [(0x54f560,alloc,1,'cdecl'),(0x54f580,realloc,2,'cdecl'),(0x54f5b0,free,1,'cdecl'),(0x54be80,logger,1,'thiscall')]:o.hook(address,fn,conv,n)
 for slot,fn,n in [(0x5e7098,first,2),(0x5e709c,next_,2),(0x5e70a0,close,1)]:o.hook(o.read_u32(slot),fn,'stdcall',n)
 if install is not None:install(o,event)
 o.write_u32(0x7686e0,0)
 image_initial={p:bytes(o.read(p,4096)) for p in range(0x400000,0x7c0000,4096)}
 def code(uc,pc,size,unused):
  nonlocal measured,endpoint,phase,before
  if pc==0x30000023:before=state();measured=True;phases.append(dict(pc=pc,event_count=len(events),store_count=len(stores),state=state()))
  if pc in [0x30000028,0x30000032]:event('constructor-return'if pc==0x30000028 else'bootstrap-return',[])
  if pc in request_returns:event('class-return',[request_returns[pc]])
  if pc in o.import_names and pc not in o.hooks:raise RuntimeError(f'unresolved import {o.import_names[pc]}')
  if pc in [0x40ff4e,0x40ff84,0x41001d]:
   phases.append(dict(pc=pc,event_count=len(events),store_count=len(stores),state=state()));phase='namespace'
  if pc==endpoint_pc:
   phases.append(dict(pc=pc,event_count=len(events),store_count=len(stores),state=state()));endpoint=True;uc.emu_stop()
 def read(uc,access,address,size,value,unused):
  if measured and 0x400000<=address<0x7c0000:
   p=address&~4095;pages[p]=image_initial[p]
 def write(uc,access,address,size,value,unused):
  if not measured:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+heap_bytes or any(p<=address and address+size<=p+n for p,n in image_ranges):stores.append([address,size,value]);return
  if STACK_LOW<=address and address+size<=STACK_TOP:return
  raise RuntimeError(f'out-of-profile store {address:x}/{size} at {o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write);o.uc.hook_add(UC_HOOK_MEM_READ,read)
 code=b''.join(bytes([opcode])+struct.pack('<I',regs[reg])for reg,opcode in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)])
 code+=b'\xe8'+struct.pack('<I',(0x40fb50-0x30000028)&0xffffffff)+b'\xb9'+struct.pack('<I',this)+b'\xe8'+struct.pack('<I',(0x40fe70-0x30000032)&0xffffffff)
 request_returns={};request_inputs=[];registration_inputs=[]
 code+=b'\xb8'+struct.pack('<I',this)+b'\xa3'+struct.pack('<I',0x63641c)
 code+=b'\xe8'+struct.pack('<I',(0x487f20-0x30000041)&0xffffffff)
 code+=b'\xc3';o.write(0x30000000,code)
 try:o.call(0x30000000,[],fpu_cw=CW_D3D9,limit=50000000)
 except Exception as e:
  if not endpoint:raise CaptureFailure(e,dict(input=spec,this=this,before=before,events=events,stores=stores,phases=phases,at_failure=state(),error=str(e))) from e
 if not endpoint and endpoint_pc==0x7fff0000:
  assert o.reg('eip')==endpoint_pc, 'complete bootstrap must execute its real RET'
  endpoint=True
 assert endpoint and not o.import_log and not finds
 assert o.uc.reg_read(X.UC_X86_REG_FPCW)==CW_D3D9 and o._fpu_depth()==0
 return dict(input=spec,this=this,register_inputs=regs,before=before,events=events,after=state(),stores=stores,endpoint=endpoint_pc,phases=phases,input_pages=[dict(base=p,data=blob(data)) for p,data in sorted(pages.items())],stock_enumerations=files,requests=request_inputs,registrations=registration_inputs)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-runtime-catalog-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(cache.__file__),Path(base.__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('native_vm_registry_catalog.py')]
 identity=dict(schema=1,exe_sha256=base.digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=base.version,cw=CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in(base.inventory()[:1]if a.first else base.inventory()):rows.append(capture(spec))
 except CaptureFailure as failure:
  a.output.with_suffix('.failure.json').write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity['producer_sources']=={p.name:base.digest(p)for p in sources}
 a.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(rows))))
if __name__=='__main__':main()
