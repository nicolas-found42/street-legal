"""Passive original48d450/49e3d0 graph on a fresh genuine cursor-pick entry.

Every game function executes unhooked. Replay supplies only its normal heap
allocator; this first genuine selection has no nonempty world-ray trees.
Incoming pages are test inputs and never used by production.
"""
import argparse,hashlib,json,struct
from pathlib import Path
from snapshot_replay import Replay,SNAPSHOTS
from unicorn import UC_HOOK_MEM_READ,UC_HOOK_MEM_WRITE,UC_HOOK_CODE
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--snapshot',default='first-cursor-pick');p.add_argument('--output',type=Path,default=Path('re/out/boot/native-cursor-pick-genuine-original.json'));a=p.parse_args()
o=Replay(a.snapshot);pages=set();queries=[];trees=[]
def access(uc,kind,address,size,value,user):
 pages.update(range(address&~4095,(address+size+4095)&~4095,4096))
def code(uc,address,size,user):
 if address==0x49e3d0:
  args=o.read_u32s(o.reg('esp')+4,3)
  queries.append({'world':o.reg('ecx'),'ray':o.read_u32s(args[0],6),'selection':args[1],'limit_before':o.read_u32(args[2]) if args[2] else None})
 if address==0x49e710:trees.append(address)
o.uc.hook_add(UC_HOOK_MEM_READ,access);o.uc.hook_add(UC_HOOK_MEM_WRITE,access);o.uc.hook_add(UC_HOOK_CODE,code)
eax=o.resume(limit=2000000)
assert not trees,'this observation must not claim unimplemented nonempty tree traversal'
inputs=[]
for page in sorted(pages):inputs.append({'address':page,'words':list(struct.unpack('<1024I',o.pages.get(page,bytes(4096))))})
root=Path(__file__).resolve().parents[2]
sources=[Path('re/oracle/native_cursor_pick_genuine_probe.py'),Path('re/oracle/snapshot_replay.py'),Path('re/oracle/headless_snapshot.py'),Path('re/oracle/oracle.py')]
out={'schema':1,'profile':__doc__,'snapshot':a.snapshot,'snapshot_metadata_sha256':hashlib.sha256((SNAPSHOTS/f'{a.snapshot}.json').read_bytes()).hexdigest(),'snapshot_pages_sha256':o.meta['pages_sha256'],'provenance':o.meta['provenance'],'sources':{str(s):hashlib.sha256((root/s).read_bytes()).hexdigest() for s in sources},'pages':inputs,'args':o.meta['stack_args'],'entry_sp':o.meta['registers']['esp'],'return_pc':o.meta['return_address'],'registers':o.meta['registers'],'comparison_policy':o.meta['comparison_policy'],'heap_ptr':o.heap_ptr,'queries':queries,'diff':o.diff(),'eax':eax,'instructions':o.icount}
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(out,indent=1)+'\n');print('genuine pick',len(inputs),'pages',len(queries),'ray queries',len(out['diff']),'retained changes',o.icount,'instructions')
