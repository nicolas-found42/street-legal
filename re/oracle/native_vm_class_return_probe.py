#!/usr/bin/env python3
"""Original bootstrap through the first complete class-load return410022.

Module/TREE/class/cache construction, file-buffer release and namespace
publication execute their original bodies. The first410890 caller returns to
its retained40fe70 bootstrap frame. Only environmental leaves are supplied.
"""
from pathlib import Path
import argparse,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
import native_vm_class_tree_probe as tree_input
members_input=tree_input.members_input
constants_input=members_input.constants_input
module_input=constants_input.module_input
file_input=module_input.file_input
cache=file_input.cache
base=cache.base
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=Path,default=base.ROOT/'re/out/boot/native-vm-class-return-original.json');p.add_argument('--first',action='store_true');a=p.parse_args()
 sources=[Path(__file__),Path(tree_input.__file__),Path(members_input.__file__),Path(constants_input.__file__),Path(module_input.__file__),Path(file_input.__file__),Path(cache.__file__),Path(base.__file__),Path(__file__).with_name('oracle.py')]
 identity=dict(schema=1,exe_sha256=base.digest(base.ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),producer_sources={p.name:base.digest(p)for p in sources},unicorn=base.version,cw=cache.CW_D3D9,profile=__doc__)
 rows=[]
 try:
  for spec in(base.inventory()[:1]if a.first else base.inventory()):rows.append(base.capture(spec,0x410022,cache.install))
 except base.CaptureFailure as failure:
  a.output.with_suffix('.failure.json').write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n');raise
 assert identity['producer_sources']=={p.name:base.digest(p)for p in sources}
 a.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(a.output),rows=len(rows))))
if __name__=='__main__':main()
