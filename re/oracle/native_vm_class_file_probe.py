#!/usr/bin/env python3
"""Retained original bootstrap/class/cache callers through411b20 file input.

Read-only Win32 input leaves are the same contract as class_cache_probe.
The measured source executes its complete cached file lookup, seek, size,
allocation, read and release before reaching the real module loader entry.
No VM/script/native answer is supplied. Captures never seed production state.
"""
from pathlib import Path
import argparse,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_class_cache_probe as cache
base=cache.base
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=base.ROOT/'re/out/boot/native-vm-class-file-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(cache.__file__),Path(base.__file__),Path(__file__).with_name('oracle.py')]
 identity=dict(schema=1,exe_sha256=base.digest(base.ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=base.version,cw=cache.CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in(base.inventory()[:1]if a.first else base.inventory()):rows.append(base.capture(spec,0x411b20,cache.install))
 except base.CaptureFailure as failure:
  a.output.with_suffix('.failure.json').write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity['producer_sources']=={p.name:base.digest(p)for p in sources}
 a.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(rows))))
if __name__=='__main__':main()
