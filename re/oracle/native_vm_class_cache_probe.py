#!/usr/bin/env python3
"""Original retained bootstrap caller through cache-header validation4165a0.

Only allocator/logger/Win32 directory, read-only file and synchronization leaves
are collaborators. Cache construction, filesystem adapters, header byte-order
reads and VM/class callers execute original instructions. No VM or native result
is hooked. Captured pages are test inputs, never production state templates.
"""
from pathlib import Path
import argparse,hashlib,json,sys,struct
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_class_load_probe as base
from oracle import CW_D3D9
ROOT=base.ROOT

def install(o,event):
 handles={};next_handle=0x4000
 def path_for(text):
  p=ROOT/'stock'
  for part in text.replace('\\','/').split('/'):
   if not part:continue
   assert part not in ['.','..']
   if not p.is_dir():return None
   children=[c for c in p.iterdir()if c.name.lower()==part.lower()]
   assert len(children)<=1
   if not children:return None
   p=children[0]
  assert p.resolve().is_relative_to((ROOT/'stock').resolve())
  return p if p.is_file()else None
 def leaf(name,args):
  nonlocal next_handle
  event(name,args)
  if name=='CreateSemaphoreA':return 0x3000
  if name=='WaitForSingleObject':assert args[0]==0x3000;return 0
  if name=='ReleaseSemaphore':
   assert args[0]==0x3000
   if args[2]:o.write_u32(args[2],0)
   return 1
  if name=='CreateFileA':
   assert args[1]==0x80000000 and args[4]==3, 'capture only permits read-only existing files'
   p=path_for(o.read_cstr(args[0]).decode('cp1252'))
   if p is None:return 0xffffffff
   next_handle+=1;handles[next_handle]=[p.read_bytes(),0];return next_handle
  if name=='CloseHandle':assert handles.pop(args[0],None)is not None or args[0]==0x3000;return 1
  if name=='GetFileSize':
   if args[1]:o.write_u32(args[1],0)
   return len(handles[args[0]][0])
  if name=='SetFilePointer':
   assert args[2]==0
   data,cursor=handles[args[0]];offset=struct.unpack('<i',struct.pack('<I',args[1]))[0]
   cursor=([0,cursor,len(data)][args[3]])+offset;assert 0<=cursor<=len(data)
   handles[args[0]][1]=cursor;return cursor
  if name=='ReadFile':
   assert args[4]==0
   data,cursor=handles[args[0]];chunk=data[cursor:cursor+args[2]]
   o.write(args[1],chunk);o.write_u32(args[3],len(chunk));handles[args[0]][1]+=len(chunk);return 1
  raise AssertionError(name)
 counts={'CreateSemaphoreA':4,'WaitForSingleObject':2,'ReleaseSemaphore':3,'CreateFileA':7,'CloseHandle':1,'GetFileSize':2,'SetFilePointer':4,'ReadFile':5}
 for address,qualified in o.import_names.items():
  name=qualified.split('!')[-1]
  if name in counts:o.hook(address,lambda oc,args,name=name:leaf(name,args),'stdcall',counts[name])
 # Source platform initialization precedes the measured VM/bootstrap caller.
 o.call(0x54c0d0,[],fpu_cw=CW_D3D9)
 o.call(0x558640,[],fpu_cw=CW_D3D9)

def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-vm-class-cache-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(base.__file__),Path(__file__).with_name('oracle.py')]
 identity=dict(schema=1,exe_sha256=base.digest(ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=base.version,cw=CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in(base.inventory()[:1]if a.first else base.inventory()):rows.append(base.capture(spec,0x410451,install))
 except base.CaptureFailure as failure:
  target=a.output.with_suffix('.failure.json');target.write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity['producer_sources']=={p.name:base.digest(p)for p in sources}
 a.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(rows))))
if __name__=='__main__':main()
