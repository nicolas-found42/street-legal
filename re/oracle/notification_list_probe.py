#!/usr/bin/env python3
"""Build606 notification48b500/48b670/48f290 lists, real weak links/destructors.
Only419860 resource access and malloc/free are supplied. Full heap, retired
sentinels and free order are compared; names include the 255-byte copy limit.
"""
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE
import hashlib

BASE = 0x20000000
RETIRED = 0x6363e0

def run(case):
    o = Oracle(fpu_cw=CW_D3D9)
    o.hook_alloc()
    freed = []
    o.hook(0x54f5b0, lambda oc,a: freed.append(a[0]) or 0, 'cdecl', 1)
    source=o.alloc(16); listener=o.alloc(16); resource=o.alloc(0x200)
    data=o.alloc(0x90); targets=[o.alloc(0x80) for _ in range(3)]
    o.write_u32s(source,[0,0,123 if case['bound'] else 0, resource if case['resource'] else 0])
    o.write_u32(resource+0x50,123)
    for i,target in enumerate(targets): o.write_u32(target+0x50,900+i)
    o.call(0x53dd80,convention='thiscall',this=data)
    o.call(0x428fd0,convention='thiscall',this=RETIRED)
    calls=[]
    def access(oc,args):
        calls.append(dict(this=oc.reg('ecx'),args=args))
        return data if case['loaded'] else 0
    o.hook(0x419860,access,'thiscall',4)
    before=o.read_u32s(BASE,(o.heap_ptr-BASE)//4)
    results=[]
    for op in case['ops']:
        target=targets[op.get('target',0)] if op.get('linked',True) else 0
        o.call(0x429060,[target],convention='thiscall',this=listener)
        if target==0: o.write_u32(listener+8,op.get('unbound_id',0))
        if op['op']=='add':
            name=op.get('name')
            text=o.scratch(len(name.encode())+1) if name is not None else 0
            if text:o.write(text,name.encode()+b'\0')
            args=[listener,op['mask'],op.get('kind',0),op.get('arg',0),text]
            r=o.call(0x48b500,args,convention='thiscall',this=source)
        elif op['op']=='remove':
            r=o.call(0x48b670,[listener,op['mask']],convention='thiscall',this=source)
        else:
            node=o.read_u32(data+0x5c)
            r=o.call(0x48f290,[op['flags']],convention='thiscall',this=node)
        results.append(r.eax)
    return dict(case=case,source=source,listener=listener,resource=resource,data=data,targets=targets,
                before=before,after=o.read_u32s(BASE,(o.heap_ptr-BASE+3)//4),heap=o.heap_ptr,
                retired=o.read_u32s(RETIRED,7),results=results,calls=calls,freed=freed)

def cases():
    for bound,resource,loaded in [(True,True,True),(False,True,True),(True,False,True),(True,True,False)]:
        for name in [None,'','event','x'*254,'x'*255,'x'*256,'x'*300]:
            ops=[dict(op='add',mask=16,target=0,name=name,kind=0x30000010,arg=0x300ff000),
                 dict(op='add',mask=32,target=1),dict(op='add',mask=16,target=0,name='replacement'),
                 dict(op='remove',mask=16,target=2),dict(op='remove',mask=16,target=0),
                 dict(op='remove',mask=16,target=0),dict(op='remove',mask=32,target=1)]
            yield dict(bound=bound,resource=resource,loaded=loaded,ops=ops)
    for flags in [0,1,2,3,255]:
        for linked in [True,False]:
            for kind in [0,0x20000000,0x30000000]:
                yield dict(bound=True,resource=True,loaded=True,ops=[
                    dict(op='add',mask=0x80000010,linked=linked,kind=kind,arg=0x300ff000,name='owned'),
                    dict(op='destroy',flags=flags)])

if __name__=='__main__':
    out=Path('re/out/oracle/notification-lists.json')
    out.parent.mkdir(parents=True,exist_ok=True)
    rows=[run(c) for c in cases()]
    out.write_text(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                                   oracle='unicorn',scope=__doc__,rows=rows)))
    print(f'{len(rows)} notification rows -> {out}')
