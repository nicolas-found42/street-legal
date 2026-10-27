#!/usr/bin/env python3
"""Controlled original cursor constructors and frame transitions, build606.

45fd00/45fdf0/461c20/461c40 execute original instructions. Resource name,
resource lookup/resolution, settings, input, placement, picking and rendering
are declared collaborators. Weak links, counted entity links, math, formatting,
event selection/order and allocation writes execute original code. The entire
caller heap is compared after every step, with no ignored words. These bounded
observations do not prove stock boot,48d450 picking, real devices, or pixels.
"""
import argparse
import hashlib
import itertools
import json
import struct
from pathlib import Path
import unicorn
import capstone
from oracle import Oracle, CW_D3D9, DEFAULT_EXE, HEAP_BASE

ROOT = Path(__file__).resolve().parents[2]
PROFILE = dict(name="cursor-controlled-collaborators-v1", oracle="Unicorn original x86 functions",
               constructor_hooks=[0x54f560,0x5383f0,0x538440,0x536820,0x4261f0,0x426100,0x426170],
               command_hooks=[0x536820,0x48a8d0,0x48c8c0,0x4b7bd0,0x4b7cb0,0x4b7cc0,0x4b7c50,0x4b7c90,0x4261f0,0x48bc40,0x48c0f0],
               frame_hooks=[0x426470,0x48bc40,0x48c0f0,0x48d450,0x426800,0x48c4b0,0x48c670,0x48a8d0],
               comparison="all allocated heap words and ordered collaborator calls; no retained-word exclusions",
               exclusions="CPU stack/registers and collaborators' internals are not claimed")

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def bits(v): return struct.unpack("<I",struct.pack("<f",v))[0]
def heap(o): return o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)
def globals_(o): return {str(a):o.read_u32(a) for a in [0x610a54,0x610a58,0x610a5c,0x649598,0x64959c,0x6495a4,0x63c83c]}
def bind(o,p,r): o.call(0x429060,[r],convention="thiscall",this=p,fpu_cw=CW_D3D9)
def counted(o,p,r):
    if o.read_u32(p): o.call(0x45fb80,convention="thiscall",this=p,fpu_cw=CW_D3D9)
    o.write_u32(p,r);o.write_u32(p+4,o.read_u32(r) if r else 0)
    if r:o.write_u32(r,p);o.write_u32(r+4,o.read_u32(r+4)+1)

def constructors():
    rows=[]
    for fill,setting,exists,system,owner_bound in itertools.product([0,0xa5],[.5,1.0],[0,1],[0,1],[0,1]):
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();events=[]
        owner=o.alloc(0x200);visual=o.alloc(0x200);reference=o.alloc(16)
        for raw,ident,kind in [(owner,0xffff0066,1),(visual,0x10023,0x23)]:
            o.write_u32(raw,0x5f375c);o.write_u32(raw+0x4c,kind);o.write_u32(raw+0x64,1);o.write_u32(raw+0x50,ident)
        if owner_bound: bind(o,reference,owner)
        o.write_u32(0x6495a4,0x33445566)
        def malloc(oc,a):
            p=oc.alloc(a[0]);oc.write(p,bytes([fill])*a[0]);events.append(["alloc",a[0],p]);return p
        o.hook(0x54f560,malloc,"cdecl",1)
        def name(oc,a):events.append(["name",a[0]]);return 0x610a64
        o.hook(0x5383f0,name,"thiscall",1)
        def lookup(oc,a):events.append(["lookup",oc.read_cstr(a[0]).decode(),a[1]]);return 0x10023
        o.hook(0x538440,lookup,"thiscall",2)
        def resolve(oc,a):events.append(["resolve",*a]);return visual
        o.hook(0x536820,resolve,"thiscall",3)
        def sensitivity(oc,a):events.append(["sensitivity",oc.read_cstr(a[0]).decode()]);return setting
        o.hook(0x4261f0,sensitivity,"cdecl",1)
        def option(oc,a):events.append(["option",oc.read_cstr(a[0]).decode(),a[1]]);return exists
        o.hook(0x426100,option,"cdecl",2)
        def integer(oc,a):events.append(["integer",oc.read_cstr(a[0]).decode()]);return system
        o.hook(0x426170,integer,"cdecl",1)
        before=heap(o);before_globals=globals_(o)
        k=o.call(0x45fd00,[0,owner if owner_bound else 0],fpu_cw=CW_D3D9).eax
        template_after=heap(o);template_events=list(events);events.clear()
        result=o.call(0x45fdf0,[reference,0],convention="thiscall",this=k,fpu_cw=CW_D3D9)
        rows.append(dict(fill=fill,setting=bits(setting),exists=exists,system=system,owner_bound=owner_bound,
                         owner=owner,visual=visual,reference=reference,before=before,before_globals=before_globals,
                         template=k,template_after=template_after,template_events=template_events,
                         body=result.eax,after=heap(o),events=events,globals=globals_(o)))
    return rows

def frame_case(label,frames,initial=None):
    initial=initial or {}
    o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();events=[]
    body=o.alloc(0x1bc);cam=o.alloc(0x90);owner=o.alloc(0x200);hit=o.alloc(0x200);entity=o.alloc(0x60);k=o.alloc(0x28)
    for r,ident in [(owner,0xffff0066),(hit,0xffff0436)]:
        o.write_u32(r,0x5f375c);o.write_u32(r+0x4c,1);o.write_u32(r+0x64,1);o.write_u32(r+0x50,ident)
    o.write_u32(entity+0x50,0xfff62a0d)
    bind(o,body,owner)
    o.write_u32(body+0x10,initial.get("flags",1));o.write_u32(body+0x24,initial.get("control",0xffff0069))
    o.write_u32(body+0x50,initial.get("status",3));o.write_u32(body+0x7c,initial.get("viewport",0))
    o.write_u32(k+0x20,bits(.02));o.write_u32(k+0x24,bits(.01))
    o.write_u32(0x610a54,cam);o.write_u32(0x6495a4,1);o.write_u32(0x63c83c,initial.get("debug",0))
    o.write_u32(0x610a58,bits(500));o.write_u32(0x610a5c,bits(500))
    o.write_u32s(cam,[bits(v) for v in [1.25,-2.5,3.75,10.,20.,30.]])
    o.write_u32(cam+0x48,bits(.375));o.write_u32s(cam+0x4c,[bits(.5),bits(.25),bits(-.125)])
    o.write_u32(cam+0x64,0x45)
    # Keep identity rotation's nine written words in the placement observation;
    # padding words are not inputs to the declared placement collaborator.
    def place(oc,a):
        events.append(["place",oc.reg("ecx"),a[0],a[1],oc.read_u32s(a[2],3),[oc.read_u32(a[3]+i*4) for i in [0,1,2,4,5,6,8,9,10]]]);return 0x30000c00
    o.hook(0x48bc40,place,"thiscall",4)
    def placed(oc,a):events.append(["placed",oc.reg("ecx"),a[0]]);return 0
    o.hook(0x48c0f0,placed,"thiscall",1)
    def query(oc,a):
        events.append(["query",*a])
        if a[1]==0x19:return current["buttons"][a[2]-0x2d]
        if a[1]==0x43:
            p=0x30000800+(a[2]-0x2a)*4;oc.write_u32(p,bits(current.get("delta",[0,0])[a[2]-0x2a]));return p
        if a[1]==7:return 0xb
        if a[1]==1:return 0
        raise RuntimeError(a)
    o.hook(0x426470,query,"thiscall",3)
    def command(oc,a):
        events.append(["command",a[0],a[1],a[2],oc.read_cstr(a[3]).decode(),a[4]])
        if current.get("mutate") and sum(e[0]=="command" for e in events)==1:
            oc.write_u32(body+0x160,0x23456789)
        return 0
    o.hook(0x426800,command,"thiscall",5)
    # A declared native picking collaborator applies its real output layout.
    # Inline weak/count-link mutations avoid resetting the enclosing CPU state.
    def weak(oc,p,r):
        old=oc.read_u32(p+12)
        if old==r:return
        if old:
            prev,nxt=oc.read_u32s(p,2);oc.write_u32(prev+4 if prev else old+0x48,nxt)
            if nxt:oc.write_u32(nxt,prev)
        oc.write_u32s(p,[0]*4)
        if r:
            nxt=oc.read_u32(r+0x48)
            if nxt:oc.write_u32(nxt,p)
            oc.write_u32s(p,[0,nxt,oc.read_u32(r+0x50),r]);oc.write_u32(r+0x48,p)
    def link(oc,p,r):
        old=oc.read_u32(p)
        if old:
            cur=oc.read_u32(old);prev=0
            while cur and cur!=p:prev=cur;cur=oc.read_u32(cur+4)
            if cur:
                oc.write_u32(prev+4 if prev else old,oc.read_u32(cur+4));oc.write_u32(old+4,oc.read_u32(old+4)-1);oc.write_u32s(p,[0,0])
        oc.write_u32(p,r);oc.write_u32(p+4,oc.read_u32(r) if r else 0)
        if r:oc.write_u32(r,p);oc.write_u32(r+4,oc.read_u32(r+4)+1)
    def pick(oc,a):
        events.append(["pick",*a]);weak(oc,a[1],hit if current["hit"] else 0);link(oc,a[2],entity if current.get("entity") else 0)
        for dst in [a[3],a[4]]:oc.write_u32s(dst,[bits(v) for v in [.5,.25,-.125]])
        oc.write_u32(a[5],0x45);oc.write_u32(a[6],bits(.375));oc.write_u32(a[9],0)
        return current["hit"]
    o.hook(0x48d450,pick,"cdecl",10)
    def render(oc,a):events.append(["render",oc.reg("ecx"),*a]);return 0
    o.hook(0x48c4b0,render,"thiscall",7)
    before=heap(o);before_globals=globals_(o);steps=[]
    for current in frames:
        events.clear();xy=current.get("xy",[0.,0.])
        if "xy_bits" in current:xy_words=current["xy_bits"]
        else:xy_words=list(map(bits,xy))
        o.write_u32(0x64959c,xy_words[0]);o.write_u32(0x649598,xy_words[1]);o.write_u32(0x6495a4,current.get("absolute",1))
        entry=0x461c40 if current.get("animate") else 0x461c20
        result=o.call(entry,[body,bits(current.get("dt",1/60))],convention="thiscall",this=k,fpu_cw=CW_D3D9)
        steps.append(dict(input=current,events=list(events),after=heap(o),globals=globals_(o),fpu_depth=result.fpu_top_depth))
    return dict(label=label,initial=initial,body=body,camera=cam,owner=owner,hit=hit,entity=entity,template=k,before=before,before_globals=before_globals,steps=steps)

def frames():
    cases=[]
    def step(hit=1,entity=0,buttons=None,**kw):return dict(hit=hit,entity=entity,buttons=buttons or [0,0],**kw)
    cases.append(frame_case("hover-cycle",[step(v,1,mutate=True) for v in [1,1,0,1,0]]))
    for right,entity,hit in itertools.product([0,1],[0,1],[0,1]):
        button=lambda v:[0,v] if right else [v,0]
        cases.append(frame_case(f"click-{right}-{entity}-{hit}",[step(hit,entity,button(v)) for v in [0,65535,65535,0,0]]))
        cases.append(frame_case(f"drag-{right}-{entity}-{hit}",[step(hit,entity,button(v),xy=[x,0]) for v,x in [(0,0),(65535,0),(65535,.1),(65535,.2),(0,.3)]]))
    cases.append(frame_case("relative",[step(0,absolute=0,delta=[.000001,-.000001]),step(1,absolute=0,delta=[.1,-.1]),step(1,absolute=0,delta=[-.1,.1])]))
    for xy in [[2.,-2.],[-0.,0.]]:
        cases.append(frame_case("absolute-edges-"+str(xy),[step(0,xy=xy)],dict(status=4)))
    for xy in [[0x7fc12345,0xffc12345],[1,0x80000001],[0x7f7fffff,0xff7fffff],[0x7f800000,0xff800000]]:
        cases.append(frame_case("absolute-raw-"+str(xy),[step(0,xy_bits=xy)],dict(status=4)))
    for initial in [dict(flags=0),dict(control=0),dict(status=0),dict(status=4)]:
        cases.append(frame_case("guards-"+str(initial),[step()],initial))
    cases.append(frame_case("animation-empty",[step(0,animate=True)],dict(flags=0,control=0)))
    cases.append(frame_case("animation-visible-relative",[step(0,animate=True,absolute=0)],dict(viewport=0xffff0063)))
    return cases

def commands():
    rows=[]
    sequences=[['activate 4353 4354','deactivate'],['activate','deactivate'],
               ['cursor 4355 4354 4353','cursor 0 0 0'],['cursor 4355','cursor'],
               ['enable','disable','enable','standby','enable'],['lock','unlock','sens'],
               ['color -1234','color'],['move 0.25,-0.5','relmove 0.25,0.5','relmove 2,-2','move -2,2'],
               ['mode 3 A 4356','mode 4 D 0','mode 1 G 0','mode 2 M 0','mode 3 W 0','mode 5 + 0'],
               ['unknown','ENABLE',''],['ignored']]
    for system,sequence in itertools.product([0,1],sequences):
        o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();events=[]
        body=o.alloc(0x1bc);k=o.alloc(0x28)
        resources={ident:o.alloc(0x200) for ident in [4353,4354,4355,4356]}
        for ident,raw in resources.items():
            o.write_u32(raw+0x4c,{4353:1,4354:1,4355:0x12,4356:0xe}[ident]);o.write_u32(raw+0x50,ident)
        o.write_u32(body+0x4c,0xaabbccdd);o.write_u32(body+0x1a8,1);o.write_u32(body+0x50,3);o.write_u32(body+0x10,0x20)
        o.write(body+0x1ac,b'+\0');o.write_u32(0x6495a4,system)
        # Live hover aliases must be retired by disable/standby, not forgotten.
        bind(o,body+0x13c,resources[4353]);o.write_u32s(body+0x154,[1,2,3]);o.write_u32(body+0x160,123)
        entity=o.alloc(0x60);counted(o,body+0x14c,entity)
        def resolve(oc,a):
            events.append(['resolve_kind',*a]);raw=resources.get(a[0],0)
            return raw if raw and (not a[1] or oc.read_u32(raw+0x4c)==a[1]) else 0
        o.hook(0x536820,resolve,'thiscall',3)
        def clear(oc,a):events.append(['release_render',oc.reg('ecx')]);return 0
        o.hook(0x48a8d0,clear,'thiscall',0)
        def color(oc,a):events.append(['color',oc.reg('ecx'),a[0]]);return 0
        o.hook(0x48c8c0,color,'thiscall',1)
        for address,operation,count in [(0x4b7bd0,'mode',1),(0x4b7cb0,'show',0),(0x4b7cc0,'hide',0),(0x4b7c50,'lock',0),(0x4b7c90,'unlock',0)]:
            def backend(oc,a,op=operation):events.append(['backend',op,a[0] if a else 0]);return 0
            o.hook(address,backend,'cdecl',count)
        def sensitivity(oc,a):events.append(['sensitivity','mouseSensitivity']);return .625
        o.hook(0x4261f0,sensitivity,'cdecl',1)
        def place(oc,a):
            events.append(['place',oc.reg('ecx'),a[0],a[1],oc.read_u32s(a[2],3),[oc.read_u32(a[3]+i*4) for i in [0,1,2,4,5,6,8,9,10]]]);return 0x30000c00
        o.hook(0x48bc40,place,'thiscall',4)
        def placed(oc,a):events.append(['placed',oc.reg('ecx'),a[0]]);return 0
        o.hook(0x48c0f0,placed,'thiscall',1)
        before=heap(o);before_globals=globals_(o);steps=[]
        for text in sequence:
            events.clear();pointer=0x30001000;o.write(pointer,text.encode()+b'\0')
            event=0x20 if text=='ignored' else 0x10
            result=o.call(0x461d20,[body,0,event,pointer],convention='thiscall',this=k,fpu_cw=CW_D3D9)
            steps.append(dict(text=text,event=event,events=list(events),after=heap(o),globals=globals_(o),result=result.eax))
        rows.append(dict(system=system,body=body,template=k,resources=resources,before=before,before_globals=before_globals,steps=steps))
    return rows

def capture():
    return dict(schema=1,exe_sha256=digest(DEFAULT_EXE),producer_sha256=digest(__file__),oracle_sha256=digest(Path(__file__).with_name("oracle.py")),
                stock_exe_sha256=digest(ROOT/"stock/StreetLegal_Redline.exe"),fpu_cw=CW_D3D9,
                tools=dict(unicorn=unicorn.__version__,capstone=capstone.__version__),profile=PROFILE,
                profile_sha256=hashlib.sha256(json.dumps(PROFILE,sort_keys=True,separators=(",",":")).encode()).hexdigest(),
                constructors=constructors(),frames=frames(),commands=commands())

if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("--output",type=Path,required=True);args=parser.parse_args()
    if args.output.exists():raise SystemExit("Preserve existing vectors; choose a new output path")
    result=capture();args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,sort_keys=True,allow_nan=False)+"\n")
    print(f"captured {len(result['constructors'])} constructors and {len(result['frames'])} frame sequences")
