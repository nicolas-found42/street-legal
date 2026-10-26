#!/usr/bin/env python3
"""Real54cf50/Win32CRT/qsort over readonly stock directory leaves.

Only allocator/free and Win32 FindFirst/Next/Close are controlled. Preexisting
raw slots are constructed by actual54cf50 before measurement. Complete heap,
stack, CRT image state and eightGPRs are retained at every leaf and return.
All raw CPU heap/image writes compare unmasked. Alternate archive state,
caller above54cf50, EFLAGS/x87 raw state and full bootstrap remain unclaimed.
"""
from pathlib import Path
import argparse,base64,hashlib,itertools,json,random,sys,zlib
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,HEAP_BASE,STACK_TOP,STACK_SIZE,CW_D3D9
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE,__version__ as version
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
IMAGE=[(0x766000,0x3000),(0x777000,0x6000)]
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def blob(b):return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest(),zlib_base64=base64.b64encode(zlib.compress(b)).decode())
def inventory():return [dict(fill=fill,label=label,pattern=pattern,existing=existing) for fill in [0,0xa5] for label,pattern,existing in [
 ('ordinary','system/scripts/lang/*',0),('growth','parts/wheels/rims/scripts/*',2),('absent','system/scripts/no_such_namespace/*',0),('slots_full','system/scripts/lang/*',32)]]
def directory(folder):
 p=ROOT/'stock'
 for part in folder.replace('\\','/').split('/'):
  if part in ('','.'):continue
  if part=='..':raise RuntimeError('unsafe stock path')
  choices=[q for q in p.iterdir() if q.name.lower()==part.lower()]
  if not choices:return None
  assert len(choices)==1;p=choices[0];assert p.resolve().is_relative_to((ROOT/'stock').resolve())
 return p
def capture(spec):
 o=Oracle(str(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'));o.write(HEAP_BASE,bytes([spec['fill']])*0x80000);o.write(STACK_TOP-STACK_SIZE,bytes([spec['fill']])*STACK_SIZE)
 o.heap_ptr=HEAP_BASE+(48 if spec['fill'] else 0);o.write_u32(0x7686e0,0)
 measured=False;events=[];stores=[];enumerations=[];finds={};identity=0
 def state():return dict(heap=blob(o.read(HEAP_BASE,0x80000)),heap_ptr=o.heap_ptr,stack=blob(o.read(STACK_TOP-0x1000,0x1000)),registers={r:o.reg(r) for r in REGS},image=[dict(base=p,data=blob(o.read(p,n))) for p,n in IMAGE])
 def event(kind,args):
  if measured:events.append(dict(kind=kind,args=args,state=state()))
 def alloc(oc,args):event('allocate',args);p=oc.alloc(args[0]);assert oc.heap_ptr<=HEAP_BASE+0x80000;return p
 def realloc(oc,args):
  event('reallocate',args);old,n=args;p=oc.alloc(n);assert oc.heap_ptr<=HEAP_BASE+0x80000
  if old:oc.write(p,oc.read(old,min(n,oc.heap_sizes[old])))
  return p
 def free(oc,args):event('free',args);return 0
 def find_data(destination,entry):
  name,dir_,size=entry;data=bytearray(320);data[:4]=(16 if dir_ else 32).to_bytes(4,'little');data[32:36]=size.to_bytes(4,'little');raw=name.encode('cp1252');assert len(raw)<260;data[44:44+len(raw)]=raw;o.write(destination,data)
 def first(oc,args):
  nonlocal identity
  event('FindFirstFileA',args);pattern=oc.read_cstr(args[0]).decode('cp1252').replace('\\','/');folder,mask=pattern.rsplit('/',1);assert mask=='*';p=directory(folder)
  if p is None:
   if measured:enumerations.append(dict(pattern=pattern,entries=[],handle=0xffffffff))
   return 0xffffffff
  entries=[('.',True,0),('..',True,0)]+[(q.name,q.is_dir(),0 if q.is_dir() else q.stat().st_size) for q in p.iterdir()]
  assert len({name.lower() for name,_,_ in entries})==len(entries);entries.sort(key=lambda row:row[0].lower());identity+=1;finds[identity]=entries[1:];find_data(args[1],entries[0])
  if measured:enumerations.append(dict(pattern=pattern,entries=entries,handle=identity))
  return identity
 def next_(oc,args):
  event('FindNextFileA',args);entries=finds[args[0]]
  if not entries:return 0
  find_data(args[1],entries.pop(0));return 1
 def close(oc,args):event('FindClose',args);del finds[args[0]];return 1
 for address,fn,count in [(0x54f560,alloc,1),(0x54f580,realloc,2),(0x54f5b0,free,1)]:o.hook(address,fn,'cdecl',count)
 for slot,fn,count in [(0x5e7098,first,2),(0x5e709c,next_,2),(0x5e70a0,close,1)]:o.hook(o.read_u32(slot),fn,'stdcall',count)
 def text(value):raw=value.encode();p=o.alloc(len(raw)+1);o.write(p,raw+b'\0');return p
 seedpattern=text('system/scripts/lang/*');destination=o.alloc(256);flag=o.alloc(4)
 for i in range(spec['existing']):
  result=o.call(0x54cf50,[seedpattern,destination,flag],fpu_cw=CW_D3D9);assert result.eax==i and not finds
 pattern=text(spec['pattern']);rng=random.Random(20261023 if spec['fill'] else 606);regs={r:rng.getrandbits(32) for r in REGS[:-1]}
 def code(uc,pc,size,_):
  nonlocal measured
  if pc==0x54cf50 and not measured:measured=True;o.set_reg('ebp',regs['ebp'])
 def write(uc,access,address,size,value,_):
  if not measured:return
  if HEAP_BASE<=address and address+size<=HEAP_BASE+0x80000 or any(p<=address and address+size<=p+n for p,n in IMAGE):stores.append([address,size,value]);return
  if STACK_TOP-0x1000<=address and address+size<=STACK_TOP:return
  raise RuntimeError(f'out-of-profile store {address:x}/{size} at{o.reg("eip"):x}')
 o.uc.hook_add(UC_HOOK_CODE,code);o.uc.hook_add(UC_HOOK_MEM_WRITE,write)
 # Preexisting slots executed these blocks before measurement hooks existed.
 # Invalidate translated blocks so both code and write hooks observe their
 # measured reexecution, including the existing-slot and slots-full profiles.
 o.uc.ctl_remove_cache(0x400000,0x600000)
 before=state();result=o.call(0x54cf50,[pattern,destination,flag],regs=regs,fpu_cw=CW_D3D9,limit=50000000)
 assert not o.import_log and result.fpcw==CW_D3D9 and result.fpu_top_depth==0
 assert len(finds)==(1 if spec['label']=='slots_full' else 0)
 return dict(input=spec,args=[pattern,destination,flag],register_inputs=regs,before=before,events=events,after=state(),stores=stores,enumerations=enumerations,result=result.eax,remaining_handles=sorted(finds))
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-find-original.json');p.add_argument('--first',action='store_true');args=p.parse_args();sources=[Path(__file__),Path(__file__).with_name('oracle.py')]
 value=dict(schema=1,profile=__doc__,exe_sha256=digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:digest(p) for p in sources},unicorn=version,cw=CW_D3D9,rows=[capture(spec) for spec in (inventory()[:1] if args.first else inventory())])
 args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(value,separators=(',',':'))+'\n');print(json.dumps(dict(output=str(args.output),rows=len(value['rows']),sha256=digest(args.output))))
if __name__=='__main__':main()
