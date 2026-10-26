#!/usr/bin/env python3
"""Real40fe70 retained caller through pre-CALL4165a0 at41044c, after real40fb50 setup.

No descriptor, name, package, find-slot or VM constructor body is hooked.
Declared collaborators are malloc/realloc/free, logger, and Win32 find leaves backed by the read-only stock directory tree. Original
54cf50 sorting/allocation and full namespace construction execute. Class resolution410890/411610/410630/41d290 and lazy41d690 inventory execute. Cache/compiler preparation4165a0 is not entered. Caller frame remains active at the precise pre-CALL
boundary. Full bootstrap, caller above40fe70, flags and x87 raw state unclaimed.
"""
from pathlib import Path
import argparse,hashlib,json,sys,itertools,random,zlib,base64,fnmatch
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,UC_HOOK_MEM_READ,x86_const as X,__version__ as version
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
HEAP_BYTES=0x80000
STACK_LOW=STACK_TOP-0x3000
class CaptureFailure(RuntimeError):
 def __init__(self,error,details):super().__init__(str(error));self.details=details

IMAGE_RANGES=[(0x62d000,0x3000),(0x766000,0x3000),(0x777000,0x1000)]

def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def blob(data):return dict(bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),zlib_base64=base64.b64encode(zlib.compress(data)).decode())
def inventory():return [dict(fill=f,seed=s,gap=g) for f,s,g in itertools.product([0,0xa5],[606,20261023],[0,48])]
def capture(spec):
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*HEAP_BYTES);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 o.heap_ptr=HEAP_BASE+spec['gap'];this=o.alloc(0x80);events=[];stores=[];files=[];phases=[];phase='descriptors';finds={};find_id=0;logging=0;measured=False;endpoint=False;pages={}
 rng=random.Random(spec['seed']);regs={r:rng.getrandbits(32) for r in REGS[:-1]};regs['ecx']=this
 def state():return dict(heap=blob(o.read(HEAP_BASE,HEAP_BYTES)),heap_ptr=o.heap_ptr,stack=blob(o.read(STACK_LOW,0x3000)),registers={r:o.reg(r) for r in REGS},fpu_cw=o.uc.reg_read(X.UC_X86_REG_FPCW),fpu_depth=o._fpu_depth(),image=[dict(base=p,data=blob(o.read(p,n))) for p,n in IMAGE_RANGES])
 def event(kind,args):
  if measured:events.append(dict(kind=kind,args=args,phase=phase,state=state()))
 def alloc(oc,args):
  event('allocate',args);p=oc.alloc(args[0]);assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES;return p
 def realloc(oc,args):
  event('reallocate',args);old,n=args;p=oc.alloc(n);assert oc.heap_ptr<=HEAP_BASE+HEAP_BYTES
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
 o.write_u32(0x7686e0,0)
 o.call(0x40fb50,[],convention='thiscall',this=this,regs=regs,fpu_cw=CW_D3D9)
 before=state();image_initial={p:bytes(o.read(p,4096)) for p in range(0x400000,0x7c0000,4096)}
 def code(uc,pc,size,unused):
  nonlocal measured,endpoint,phase
  if pc==0x40fe70:measured=True;o.set_reg('ebp',regs['ebp'])
  if pc in o.import_names and pc not in o.hooks:raise RuntimeError(f'unresolved import {o.import_names[pc]}')
  if pc in [0x40ff4e,0x40ff84,0x41001d]:
   phases.append(dict(pc=pc,event_count=len(events),store_count=len(stores),state=state()));phase='namespace'
  if pc==0x41044c:
   phases.append(dict(pc=pc,event_count=len(events),store_count=len(stores),state=state()));endpoint=True;uc.emu_stop()
 def read(uc,access,address,size,value,unused):
  if measured and 0x400000<=address<0x7c0000:
   p=address&~4095;pages[p]=image_initial[p]
 def write(uc,access,address,size,value,unused):
  if not measured:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+HEAP_BYTES or any(p<=address and address+size<=p+n for p,n in IMAGE_RANGES):stores.append([address,size,value]);return
  if STACK_LOW<=address and address+size<=STACK_TOP:return
  raise RuntimeError(f'out-of-profile store {address:x}/{size} at {o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write);o.uc.hook_add(UC_HOOK_MEM_READ,read)
 try:o.call(0x40fe70,[],convention='thiscall',this=this,regs=regs,fpu_cw=CW_D3D9,limit=50000000)
 except Exception as e:
  if not endpoint:raise CaptureFailure(e,dict(input=spec,this=this,before=before,events=events,stores=stores,phases=phases,at_failure=state(),error=str(e))) from e
 assert endpoint and not o.import_log and not finds
 assert o.uc.reg_read(X.UC_X86_REG_FPCW)==CW_D3D9 and o._fpu_depth()==0
 return dict(input=spec,this=this,register_inputs=regs,before=before,events=events,after=state(),stores=stores,endpoint=0x41044c,phases=phases,input_pages=[dict(base=p,data=blob(data)) for p,data in sorted(pages.items())],stock_enumerations=files)
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-class-load-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 identity=dict(schema=1,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:digest(p) for p in sources},unicorn=version,cw=CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in (inventory()[:1] if a.first else inventory()):rows.append(capture(spec))
 except CaptureFailure as failure:
  target=a.output.with_suffix('.failure.json');target.parent.mkdir(parents=True,exist_ok=True);target.write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity["producer_sources"]=={p.name:digest(p) for p in sources}, "producer source changed during capture"
 out=dict(**identity,rows=rows)
 a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(out['rows']),sha256=digest(a.output))))
if __name__=='__main__':main()
