#!/usr/bin/env python3
"""Execute original554ea0 worker with declared file/semaphore/allocator seams.

The worker itself selects requests, manages its cache, reads and calls completion.
Stop at the first empty-queue sleep; all recorded state precedes shutdown.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,OracleError,DEFAULT_EXE
from unicorn import UC_HOOK_CODE
BASE=0x76aa30
CACHE=0x76f138

def run_case(name,requests,requeue=False):
 o=Oracle();o.hook_alloc();o.write(BASE,bytes(64*0x11c));o.write(CACHE,bytes(8*0x108))
 for i in range(8):o.write_u32(CACHE+i*0x108+0x100,0xffffffff)
 o.write_u32(0x61ac38,101);o.write_u32(0x61ac3c,102)
 files={};next_handle=[7];calls=[];completions=[];snapshot={};retry={};requeued=[False]
 def text(p):return o.read(p,256).split(b'\0')[0].decode('ascii')
 for r in requests:
  p=BASE+r['slot']*0x11c
  for off,val in [(0,1),(4,r.get('destination',0)),(0x108,r.get('offset',0)),(0x10c,r.get('length',0)),(0x110,r['slot']),(0x114,r.get('priority',0)),(0x118,0x30001000)]:o.write_u32(p+off,val)
  o.write(p+8,r['path'].encode()+b'\0')
 def opened(oc,a):
  path=text(a[0]);calls.append(['open',path])
  if path=='missing':return -1
  if path=='retry' and not retry.get(path):retry[path]=True;return -2
  h=next_handle[0];next_handle[0]+=1;files[h]=[path,0];return h
 def seek(oc,a):files[a[0]][1]=a[1];calls.append(['seek',*a]);return a[1]
 def length(oc,a):calls.append(['length',*a]);return 0 if files[a[0]][0]=='empty' else 12
 def read(oc,a):
  h,dest,n=a;path,off=files[h];count=min(n,12-off)
  if path=='short':count=max(0,count-1)
  payload=bytes(range(12))[off:off+count];oc.write(dest,payload);calls.append(['read',h,dest,n,count]);return count
 def complete(oc,a):
  slot,result,dest,n=a;completions.append(dict(args=a,state=oc.read_u32(BASE+slot*0x11c),bytes=list(oc.read(dest,n)) if dest and n<100 else []))
  if requeue and not requeued[0]:
   # Publish a new equal-priority request behind and ahead of the last cursor.
   requeued[0]=True
   for slot2 in [0,2]:
    p=BASE+slot2*0x11c;oc.write_u32(p,1);oc.write_u32(p+0x110,slot2);oc.write_u32(p+0x118,0x30001000);oc.write(p+8,b'same\0')
 def wait(oc,a):
  if a[0]==102:
   snapshot.update(requests=oc.read_u32s(BASE,64*0x11c//4),cache=oc.read_u32s(CACHE,8*0x108//4),serial=oc.read_u32(0x76f134),processed=oc.read_u32(0x76f98c),read_bytes=oc.read_u32(0x76f994),heap=oc.heap_ptr)
   oc.uc.emu_stop()
  return 0
 o.hook(0x559830,wait,'cdecl',2);o.hook(0x559880,lambda oc,a:0,'cdecl',1)
 o.hook(0x554480,lambda oc,a:-1,'cdecl',1) # declared no packed file
 o.hook(0x558d80,opened,'cdecl',1);o.hook(0x559090,lambda oc,a:calls.append(['close',*a]),'cdecl',1)
 o.hook(0x559150,seek,'cdecl',2);o.hook(0x559100,length,'cdecl',1);o.hook(0x5591d0,read,'cdecl',3)
 o.hook(0x54f6e0,lambda oc,a:oc.alloc(a[0]) if 0<a[0]<100 else 0,'cdecl',1)
 o.hook(0x30001000,complete,'cdecl',4)
 initial=o.read_u32s(BASE,64*0x11c//4)
 try:o.call(0x554ea0,limit=2000000)
 except OracleError:
  if not snapshot or o.reg("eip") != 0x5552ae:raise
 return dict(name=name,input=initial,submitted=requests,requeue=requeue,calls=calls,completions=completions,**snapshot)

def run():
 cases=[('priority',[dict(slot=i,path='same' if i%2==0 else 'other',priority=p) for i,p in [(0,0),(3,2),(8,2),(63,-1)]]),('errors',[dict(slot=i,path=p) for i,p in enumerate(['missing','empty','short','retry'])]),('ranges',[dict(slot=0,path='range',offset=3,length=4),dict(slot=1,path='range',offset=7,destination=0x30002000)]),('eviction',[dict(slot=i,path=f'file{i}') for i in range(10)]),('allocation',[dict(slot=0,path='range',length=128)])]
 rows=[run_case(n,r) for n,r in cases]
 rows.append(run_case('requeue',[dict(slot=1,path='same')],True))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
