#!/usr/bin/env python3
"""Unhooked original48d450 on a genuine nonempty GUI selection.

The capture must retain19 tree entries (18game resources and one physical
resource), one box body, two ray queries and zero platform/game hook calls.
Incoming pages are test inputs only; production never reads this fixture.
CPU scratch/register/exception parity is not claimed by this retained-state slice.

Prepare the genuine input before running this probe:
  ~/slr-oracle-venv/bin/python re/oracle/headless_snapshot.py gui-cursor-pick-nonempty --function 0x48d450 --after-native 7001 --args 10 --run-root re/out/headless/gui-ray-capture
Then run this script with the same Python environment. Original snapshot
provenance is validated; changed source/profile inputs require a real recapture.
"""
import argparse,hashlib,json,struct
from pathlib import Path
from snapshot_replay import Replay,SNAPSHOTS
from unicorn import UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE,UC_HOOK_CODE
ROOT=Path(__file__).resolve().parents[2]
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-cursor-pick-nonempty-original.json');a=p.parse_args()
 name='gui-cursor-pick-nonempty';o=Replay(name);pages=set();queries=[];trees=[];bodies=[];triangles=[];reached_hooks=[]
 assert o.meta['function']==0x48d450 and o.meta['native_calls']==7001 and len(o.meta['stack_args'])==10
 def access(uc,kind,address,size,value,user):pages.update(range(address&~4095,(address+size+4095)&~4095,4096))
 def code(uc,address,size,user):
  if address in o.hooks:reached_hooks.append(address)
  if address==0x49e3d0:
   args=o.read_u32s(o.reg('esp')+4,3);queries.append(dict(world=o.reg('ecx'),ray=o.read_u32s(args[0],6),selection=args[1],limit_before=o.read_u32(args[2]) if args[2] else None))
  if address==0x49e710:
   raw=o.read_u32(o.reg('esp')+4);trees.append(dict(raw=raw,kind=o.read_u32(raw+0x4c),id=o.read_u32(raw+0x50)))
  if address==0x49deb0:
   entity=o.read_u32(o.reg('esp')+4);body=o.read_u32(entity+0x5c);definition=o.read_u32(body+0x78) if body else 0;bodies.append(dict(entity=entity,shape=o.read_u32(definition+8) if definition else 0))
  if address in [0x49fa60,0x49f7b0]:triangles.append(address)
 o.uc.hook_add(UC_HOOK_MEM_READ,access);o.uc.hook_add(UC_HOOK_MEM_WRITE,access);o.uc.hook_add(UC_HOOK_CODE,code)
 result=o.resume(limit=2000000)
 assert result==1 and len(queries)==2 and len(trees)==19 and [t['kind'] for t in trees].count(1)==18 and [t['kind'] for t in trees].count(2)==1
 assert len(bodies)==1 and bodies[0]['shape']==5 and not triangles and not reached_hooks
 inputs=[dict(address=page,words=list(struct.unpack('<1024I',o.pages.get(page,bytes(4096)))))for page in sorted(pages)]
 sources=[Path(__file__).resolve(),ROOT/'re/oracle/snapshot_replay.py',ROOT/'re/oracle/headless_snapshot.py',ROOT/'re/oracle/oracle.py']
 packed=b''.join(struct.pack('<1025I',row['address'],*row['words'])for row in inputs)
 diff=o.diff();changed=b''.join(struct.pack('<3I',*row)for row in diff)
 out=dict(schema=1,profile=__doc__,snapshot=name,snapshot_metadata_sha256=hashlib.sha256((SNAPSHOTS/f'{name}.json').read_bytes()).hexdigest(),snapshot_pages_sha256=o.meta['pages_sha256'],provenance=o.meta['provenance'],sources={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()for p in sorted(sources)},pages=inputs,input_sha256=hashlib.sha256(packed).hexdigest(),diff_sha256=hashlib.sha256(changed).hexdigest(),args=o.meta['stack_args'],entry_sp=o.meta['registers']['esp'],return_pc=o.meta['return_address'],registers=o.meta['registers'],comparison_policy=o.meta['comparison_policy'],heap_ptr=o.heap_ptr,queries=queries,trees=trees,bodies=bodies,triangle_calls=triangles,reached_hooks=reached_hooks,diff=diff,eax=result,instructions=o.icount)
 a.output.parent.mkdir(parents=True,exist_ok=True);assert not a.output.is_symlink();a.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
 print('genuine nonempty pick',len(inputs),'pages',len(queries),'queries',len(diff),'retained changes',o.icount,'instructions','inputSHA',out['input_sha256'],'diffSHA',out['diff_sha256'])
if __name__=='__main__':main()
