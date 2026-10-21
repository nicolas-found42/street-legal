#!/usr/bin/env python3
"""Run original546e10 collision resource materialization, including its parser.

Only file bytes and allocation are supplied. No geometry/inertia callees are
stubbed. Output is derived data; keep it ignored under re/out/boot.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9
from native_geometry_probe import mesh_bytes

def run():
    rows=[]
    for extra in ['', 'friction 0.4\ncoll_coeff 0.7\nskin 0.01\nmass 3.5\n', 'type 7\nmass -2\n', 'inertia 1 2 3\nunknown 4\n']:
        data=mesh_bytes()
        text=f'shape probe.scx\n{extra}'
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc()
        envelope=o.alloc(0x14);source=o.scratch(len(text)+1);o.write(source,text.encode()+b'\0')
        def load(oc,args):
            p=oc.alloc(len(data));oc.write(p,data);oc.write_u32(args[1],p);return len(data)
        o.hook(0x54c300,load,'cdecl',2)
        result=o.call(0x546e10,[source,len(text)],convention='thiscall',this=envelope,fpu_cw=CW_D3D9,limit=20_000_000)
        definition=o.read_u32(envelope+0xc);body=o.read_u32(envelope+0x10)
        rows.append(dict(text=text,mesh=data.hex(),definition=definition,body=body,definition_words=o.read_u32s(definition,43),body_words=o.read_u32s(body,40),heap=o.heap_ptr))
    return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)

if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
