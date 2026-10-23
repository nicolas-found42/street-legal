#!/usr/bin/env python3
"""Actual477bc0/477cb0/4797d0 player constructors and boot-empty virtuals.

No436370 constructor is used. The original class/runtime instructions, mapping
defaults/chain, scanner and formatter execute unchanged. Allocator destinations,
resource manager/binding, config settings, registrar and event subscription are
declared controlled collaborators. Complete caller heaps, relevant globals,
callback-time heaps/order/ESP/return PC/arguments and original ABI are retained.
Nonempty control/animation and live getInfo mapping branches remain unaccepted.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, HEAP_BASE

ROOT=Path(__file__).resolve().parents[2]
EXE=ROOT/'stock/StreetLegal_Redline.bak.exe'
GLOBALS=[0x63c86c,0x7686f0,0x7686f8,0x63c870,0x63c874,0x6439c0]


def bind(o,link,raw):
    old=o.read_u32(link+12)
    if old==raw:return
    if old:
        previous,following=o.read_u32s(link,2)
        o.write_u32(previous+4 if previous else old+0x48,following)
        if following:o.write_u32(following,previous)
    o.write_u32s(link,[0]*4)
    if raw:
        following=o.read_u32(raw+0x48)
        if following:o.write_u32(following,link)
        o.write_u32(link+4,following);o.write_u32(raw+0x48,link)
        o.write_u32(link+8,o.read_u32(raw+0x50));o.write_u32(link+12,raw)


def setup(fill):
    o=Oracle(str(EXE),fpu_cw=CW_D3D9)
    owner=o.alloc(0x180);reference=o.alloc(16);k=o.alloc(0x50);body=o.alloc(0x2510)
    resources=[o.alloc(0x100) for _ in range(4)];output=o.alloc(16)
    o.write(k,bytes([fill])*0x50);o.write(body,bytes([fill])*0x2510)
    for i,raw in enumerate([owner]+resources):o.write_u32(raw+0x50,0xffff2000+i)
    bind(o,reference,owner)
    o.write_u32(0x618d48,0x30080000);o.write_u32(0x63641c,0x30090000)
    for i,a in enumerate(GLOBALS):o.write_u32(a,0x11223300+i)
    o.write_u32(0x63c86c,0);o.write_u32(0x7686f8,resources[3]);o.write_u32(0x7686f0,7)
    return o,dict(owner=owner,reference=reference,k=k,body=body,resources=resources,output=output,
                  words=(o.heap_ptr-HEAP_BASE)//4)


def capture_case(fill,loaded,registered,parameters):
    o,layout=setup(fill);k=layout['k'];body=layout['body'];events=[]
    def snapshot():return o.read_u32s(HEAP_BASE,layout['words'])
    def global_words():return {str(a):o.read_u32(a) for a in GLOBALS}
    def record(name,oc,args):
        esp=oc.reg('esp')
        events.append(dict(name=name,this=oc.reg('ecx'),args=list(args),esp=esp,
                           return_pc=oc.read_u32(esp),heap=snapshot(),globals=global_words()))
    def allocate(oc,args):record('allocate',oc,args);return k if args[0]==0x50 else body
    def name(oc,args):record('name',oc,args);return 0x30070000
    def lookup(oc,args):record('lookup',oc,args);return {0x612624:0xffff2001,0x612630:0xffff2002,0x6128ac:0xffff2003}[args[0]]
    def bind_typed(oc,args):
        record('bind',oc,args)
        raw=next((r for r in [layout['owner']]+layout['resources'] if oc.read_u32(r+0x50)==args[0]),0)
        bind(oc,oc.reg('ecx'),raw);return 0
    def bind_any(oc,args):record('bind',oc,args);return bind_typed_no_record(oc,args)
    def bind_typed_no_record(oc,args):
        raw=next((r for r in [layout['owner']]+layout['resources'] if oc.read_u32(r+0x50)==args[0]),0)
        bind(oc,oc.reg('ecx'),raw);return 0
    def register(oc,args):record('register',oc,args);return 0
    def integer(oc,args):record('integer',oc,args);return 0x12345678
    def floating(oc,args):record('float',oc,args);return {0x612860:1.5,0x61287c:0.25,0x612894:-2.0}[args[0]]
    def subscribe(oc,args):record('subscribe',oc,args);return 0
    o.hook(0x54f560,allocate,'cdecl',1);o.hook(0x5383f0,name,'thiscall',1)
    o.hook(0x538440,lookup,'thiscall',2);o.hook(0x546070,bind_typed,'thiscall',3)
    o.hook(0x545fc0,bind_any,'thiscall',1);o.hook(0x416b00,register,'thiscall',4)
    o.hook(0x426170,integer,'cdecl',1);o.hook(0x4261f0,floating,'cdecl',1)
    o.hook(0x48d3d0,subscribe,'cdecl',3)
    before=snapshot();globals_before=global_words()
    result=o.call(0x477bc0,[0x1a000006,layout['owner']])
    template_result=dict(result=result.eax,esp_delta=result.esp_delta,fpu_cw=result.fpcw,
                         before=before,after=snapshot(),events=list(events),globals_before=globals_before,globals_after=global_words())
    o.write_u32(k+0x18,loaded);o.write_u32(0x63c86c,registered)
    parameters_ptr=o.scratch(len(parameters)+1);o.write(parameters_ptr,parameters.encode()+b'\0')
    before=snapshot();globals_before=global_words();events.clear()
    result=o.call(0x4797d0,[layout['reference'],parameters_ptr],convention='thiscall',this=k)
    constructor=dict(result=result.eax,esp_delta=result.esp_delta,fpu_cw=result.fpcw,fpu_depth=result.fpu_top_depth,
                     nonvolatile={r:o.reg(r) for r in ['ebx','esi','edi','ebp']},before=before,after=snapshot(),
                     events=list(events),globals_before=globals_before,globals_after=global_words())
    frames=[]
    for entry,dt in itertools.product([0x47a5f0,0x47a780],[0,0x3c888889,0x80000000,0x7fc01234]):
        before=snapshot();globals_before=global_words();events.clear()
        result=o.call(entry,[body,dt],convention='thiscall',this=k)
        frames.append(dict(entry=entry,dt=dt,before=before,after=snapshot(),globals_before=globals_before,
                           globals_after=global_words(),events=list(events),esp_delta=result.esp_delta,fpu_cw=result.fpcw))
    queries=[]
    for message,arg in [(i,0)for i in range(0,70)]+[(25,76),(25,0xffffffff),(67,76),(67,0xffffffff),(26,layout['output'])]:
        before=snapshot();globals_before=global_words();events.clear()
        result=o.call(0x47b220,[body,message,arg],convention='thiscall',this=k)
        queries.append(dict(message=message,arg=arg,result=result.eax,before=before,after=snapshot(),
                            globals_before=globals_before,globals_after=global_words(),events=list(events),esp_delta=result.esp_delta,fpu_cw=result.fpcw))
    return dict(fill=fill,loaded=loaded,registered=registered,parameters=parameters,layout=layout,
                template=template_result,constructor=constructor,frames=frames,queries=queries)


def capture():
    rows=[capture_case(*case)for case in itertools.product([0,0xa5],[0,3],[0,1],['0 0 0','7 -3 -57341','-12 19 0','4 junk'])]
    return dict(schema=1,scope=__doc__,exe_sha256=hashlib.sha256(EXE.read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                global_addresses=GLOBALS,rows=rows)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    with args.out.open('x')as stream:json.dump(capture(),stream,sort_keys=True);stream.write('\n')
