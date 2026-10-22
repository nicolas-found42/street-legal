#!/usr/bin/env python3
"""Original43e320 guard/dispatch vectors, controlled marshalling/resource seams."""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE


def run(case):
    o=Oracle()
    a={name:o.alloc(0x200) for name in ['reference','resource','template','data','tdata','vt','tvt']}
    o.write_u32(a['reference']+12,0 if case==1 else a['resource'])
    o.write_u32(a['resource'],a['vt'])
    o.write_u32(a['template'],a['tvt'])
    o.write_u32(a['resource']+0x4c,1 if case%2==0 else 8)
    o.write_u32(a['template']+0x4c,8)
    o.write_u32(a['vt']+12,0x300f0000)
    o.write_u32(a['tvt']+12,0x300f0010)
    o.write_u32(a['vt']+20,0x300f0020)
    o.write_u32(a['tvt']+20,0x300f0020)
    o.write_u32(a['data']+0x44,0 if case==4 else a['template'])
    o.write_u32(a['data']+0x4c,91)
    o.write_u32(a['tdata']+12,73)
    calls=[]
    accesses=[]
    def marshal(oc,args): oc.write_u32(args[1],17)
    o.hook(0x45d910,marshal,'cdecl',2)
    o.hook(0x42ab50,lambda oc,args:0 if case==0 else a['reference'],'cdecl',2)
    o.hook(0x300f0020,lambda oc,args:calls.append(['minimum',oc.reg('ecx'),args[0]]),'thiscall',1)
    def prepare(oc,args):
        raw=oc.reg('ecx')
        accesses.append([raw,args[0]])
        return 0x80000000 if ((case==2 and raw==a['resource']) or (case==5 and raw==a['template'])) else 0
    o.hook(0x5447d0,prepare,'thiscall',3)
    o.hook(0x300f0000,lambda oc,args:0 if case==3 else a['data'],'thiscall',1)
    o.hook(0x300f0010,lambda oc,args:0 if case==6 else a['tdata'],'thiscall',1)
    result=[]
    o.hook(0x448430,lambda oc,args:result.append([oc.reg('ecx'),args[0]]),'thiscall',1)
    before={key:o.read_u32s(value,0x200//4) for key,value in a.items()}
    o.call(0x43e320,[0],convention='cdecl')
    return dict(case=case,addresses=a,before=before,accesses=accesses,result=result,
                reference=0 if case==0 else a['reference'])

if __name__=='__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=[run(i) for i in range(9)]),sort_keys=True))
