#!/usr/bin/env python3
"""Original startup through all353 native registrations, with a declared1MiB heap.

Uses the same actual427980 caller as native_vm_root_probe. The earlier512KiB
attempt exhausted its declared allocator bound and remains a failed receipt.
Only allocator/logger/filesystem and explicit resource-domain leaves are hooked;
no script/native implementation, method selection or VM body is replaced.
"""
from pathlib import Path
import argparse
import json
import native_vm_root_probe as root

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists() or args.output.with_suffix('.failure.json').exists():
        parser.error('preserve earlier attempts; choose a fresh output path')
    root.HEAP_BYTES=0x100000
    sources=[Path(__file__),Path(root.__file__),Path(root.cache.__file__),Path(root.base.__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('native_vm_registry_catalog.py')]
    identity=dict(schema=1,endpoint='427a0c',world='636338',heap_bytes=root.HEAP_BYTES,
                  resource_profile='fill0 missing packages; fillA5 two existing resource owners',
                  exe_sha256=root.base.digest(root.ROOT/'reference/raw/StreetLegal_Redline.bak.exe'),
                  producer_sources={p.name:root.base.digest(p) for p in sources},
                  unicorn=root.base.version,cw=root.CW_D3D9,profile=__doc__)
    rows=[]
    try:
        for spec in root.base.inventory():
            rows.append(root.capture(spec,registry=True,catalogs=True))
    except root.CaptureFailure as failure:
        args.output.with_suffix('.failure.json').write_text(json.dumps(dict(**identity,completed_rows=rows,failure=failure.details),separators=(',',':'))+'\n')
        raise
    assert identity['producer_sources']=={p.name:root.base.digest(p) for p in sources}
    args.output.write_text(json.dumps(dict(**identity,rows=rows),separators=(',',':'))+'\n')
    print(json.dumps(dict(output=str(args.output),rows=len(rows),heap_bytes=root.HEAP_BYTES)))

if __name__=='__main__':
    main()
