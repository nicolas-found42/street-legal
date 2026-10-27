#!/usr/bin/env python3
"""Retained original411b20 caller through first407660 constant-pool entry.

The original module constructor and every allocation/C++ header execute after
source filesystem/cache loading. No metadata body or VM result is hooked.
Captured states are strict test evidence, never production state templates.
"""
from pathlib import Path
import argparse,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_class_file_probe as file_input
cache=file_input.cache
base=cache.base
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=base.ROOT/'re/out/boot/native-vm-class-module-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(file_input.__file__),Path(cache.__file__),Path(base.__file__),Path(__file__).with_name('oracle.py')]
 identity=dict(schema=1,exe_sha256=base.digest(base.ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=base.version,cw=cache.CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in(base.inventory()[:1]if a.first else base.inventory()):rows.append(base.capture(spec,0x407660,cache.install))
 except base.CaptureFailure as failure:
  a.output.with_suffix('.failure.json').write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity['producer_sources']=={p.name:base.digest(p)for p in sources}
 a.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(rows))))
if __name__=='__main__':main()
