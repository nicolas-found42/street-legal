#!/usr/bin/env python3
"""Execute original48d450 and513840 with only49e3d0 as a controlled boundary.

Viewport/camera intrusive walks, viewport normalization, camera unprojection,
ray construction, reference/count-link lifetime and output publication execute
original x86. The supplied collision query records all ray words, selection,
limit and complete live caller heap, then applies declared hit outputs. No
renderer result is supplied to the Rust production runtime.
"""
import argparse
import hashlib
import itertools
import json
import random
import struct
from pathlib import Path
import unicorn
from unicorn import x86_const as X
from oracle import Oracle,CW_D3D9,DEFAULT_EXE,HEAP_BASE

ROOT=Path(__file__).resolve().parents[2]
GLOBALS=[0x6187b0,0x617650,*range(0x643580,0x6435bc,4)]
PROFILE=dict(name='original-cursor-pick-ray-boundary-v1',hooks=[0x49e3d0],
             comparison='all caller heap words, all17 picking/context globals, every ordered ray call and its complete live heap',
             ray_collaborator='declared result/global hit output; ray geometry and traversal execute original code',
             exclusions='collision body49e3d0, device pixels, CPU stack/register identities are unclaimed')
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def heap(o):return o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4)
def globals_(o):return {str(p):o.read_u32(p) for p in GLOBALS}
def bind(o,p,r):o.call(0x429060,[r],convention='thiscall',this=p,fpu_cw=CW_D3D9)
def counted(o,p,r):
    o.write_u32(p,r);o.write_u32(p+4,o.read_u32(r) if r else 0)
    if r:o.write_u32(r,p);o.write_u32(r+4,o.read_u32(r+4)+1)
def reverse_list(o,owner,first,tail,nodes):
    start=o.alloc(12);end=o.alloc(12)
    o.write_u32(start+4,nodes[0] if nodes else end);o.write_u32(end+8,nodes[-1] if nodes else start)
    for i,p in enumerate(nodes):o.write_u32(p+4,nodes[i+1] if i+1<len(nodes) else end);o.write_u32(p+8,nodes[i-1] if i else start)
    o.write_u32(owner+first,nodes[0] if nodes else end);o.write_u32(owner+tail,nodes[-1] if nodes else start)
def matrix(o,camera,seed):
    rng=random.Random(seed)
    for base in [0x294,0x354]:
        for i in range(16):o.write_u32(camera+base+i*4,bits((1. if i%5==0 else 0.)+rng.uniform(-.15,.15)))

def unprojections():
    rows=[]
    for index,alias in itertools.product(range(24),[0,1]):
        o=Oracle(fpu_cw=CW_D3D9);camera=o.alloc(0x670);point=o.alloc(12);out=point if alias else o.alloc(12)
        matrix(o,camera,1000+index);rng=random.Random(index)
        o.write_u32s(point,[bits(rng.uniform(-2,2)) for _ in range(3)])
        before=heap(o);result=o.call(0x513840,[out,point],convention='thiscall',this=camera,fpu_cw=CW_D3D9)
        rows.append(dict(camera=camera,point=point,out=out,alias=alias,before=before,after=heap(o),fpu_depth=result.fpu_top_depth,esp_delta=result.esp_delta,fpu_cw=o.uc.reg_read(X.UC_X86_REG_FPCW)))
    return rows

def pick_case(label,rects,camera_counts,position,responses,mask=0x1ff,mutate=False):
    o=Oracle(fpu_cw=CW_D3D9);renderer=o.alloc(0x100);point=o.alloc(12);ref=o.alloc(16);link=o.alloc(8)
    outputs=[o.alloc(n) for n in [12,12,4,4,12,12,4]]
    resources=[o.alloc(0x100) for _ in range(3)];entities=[o.alloc(0x60) for _ in range(2)]
    for i,p in enumerate(resources):o.write_u32(p+0x50,0xffff2001+i)
    bind(o,ref,resources[0]);counted(o,link,entities[0])
    # Neighboring aliases make clearing/rebinding observable beyond the output.
    extra_ref=o.alloc(16);bind(o,extra_ref,resources[0]);extra_link=o.alloc(8);counted(o,extra_link,entities[0])
    viewports=[];cameras=[]
    for i,(rect,count) in enumerate(zip(rects,camera_counts)):
        vp=o.alloc(0x80);viewports.append(vp)
        for off,value in zip([0x5c,0x60,0x64,0x68],rect):o.write_u32(vp+off,bits(value))
        nodes=[]
        for j in range(count):
            camera=o.alloc(0x670);nodes.append(camera);cameras.append(camera);matrix(o,camera,i*20+j)
            o.write_u32(camera+0x658,0xaabb0000+len(cameras))
        reverse_list(o,vp,0x2c,0x3c,nodes)
    reverse_list(o,renderer,0xac,0xbc,viewports)
    o.write_u32(0x6187b0,renderer);o.write_u32(0x617650,0x30000f00)
    o.write_u32s(point,[bits(position[0]),bits(position[1]),0])
    for i,p in enumerate(outputs):o.write(p,bytes([0xa0+i])*[12,12,4,4,12,12,4][i])
    args=[point,*[p if mask&(1<<i) else 0 for i,p in enumerate([ref,link,*outputs])]]
    events=[]
    def ray(oc,a):
        index=len(events);limit=oc.read_u32(a[2]);words=oc.read_u32s(a[0],6)
        result=responses[index] if index<len(responses) else 0
        events.append(dict(ray=words,selection=a[1],limit=limit,context=oc.reg('ecx'),heap=heap(oc),result=result))
        if result:
            target=resources[1+(index%2)];entity=entities[1]
            oc.write_u32(0x6435b8,target);oc.write_u32(0x6435a4,entity)
            oc.write_u32s(0x643584,[bits(.125),bits(-.25),bits(.5)])
            oc.write_u32s(0x643590,[bits(-1),0,bits(1)])
            oc.write_u32(0x64359c,0xabcd1234+index);oc.write_u32(0x643580,bits(.375));oc.write_u32(0x6435a0,0xf00d0000+index)
        if mutate and index==0 and cameras:
            # Mutate a later camera's live inverse matrix, not a captured ray.
            oc.write_u32(cameras[0]+0x2c4,bits(17.))
        oc.write_u32(a[2],bits(.5));return result
    o.hook(0x49e3d0,ray,'thiscall',3)
    before=heap(o);before_globals=globals_(o)
    result=o.call(0x48d450,args,fpu_cw=CW_D3D9)
    return dict(label=label,rects=rects,camera_counts=camera_counts,position_bits=[bits(position[0]),bits(position[1])],responses=responses,mask=mask,renderer=renderer,args=args,viewports=viewports,cameras=cameras,resources=resources,entities=entities,mutate=mutate,before=before,before_globals=before_globals,
                events=events,after=heap(o),globals=globals_(o),result=result.eax,fpu_depth=result.fpu_top_depth,esp_delta=result.esp_delta,fpu_cw=o.uc.reg_read(X.UC_X86_REG_FPCW))

def picks():
    rows=[pick_case('empty-renderer',[],[],[0,0],[]),pick_case('empty-camera-list',[[0,0,1,1]],[0],[0,0],[])]
    masks=[0,0x1ff,*[1<<i for i in range(9)],*[0x1ff^(1<<i) for i in range(9)]]
    for i,mask in enumerate(masks):rows.append(pick_case(f'nullable-{i}',[[0,0,1,1]],[1],[.25,-.5],[1],mask))
    for i,position in enumerate([[-1,-1],[1,1],[-1.0001,0],[1.0001,0],[0,-1.0001],[0,1.0001],[.75,0],[-.75,0],[0,0]]):
        rows.append(pick_case(f'viewport-order-{i}',[[0,0,.5,1],[.5,0,.5,1]],[2,2],position,[0,0x1234]))
    rows.append(pick_case('all-rays-miss',[[0,0,1,1],[0,0,1,1]],[2,2],[0,0],[0,0,0,0],mutate=True))
    rows.append(pick_case('third-ray-hit',[[0,0,1,1],[0,0,1,1]],[2,2],[0,0],[0,0,3],mutate=True))
    rows.append(pick_case('zero-extent',[[0,0,0,0]],[1],[0,0],[1]))
    return rows
def capture():
    return dict(schema=1,exe_sha256=digest(DEFAULT_EXE),stock_exe_sha256=digest(ROOT/'stock/StreetLegal_Redline.exe'),producer_sha256=digest(__file__),oracle_sha256=digest(Path(__file__).with_name('oracle.py')),fpu_cw=CW_D3D9,
                unicorn=unicorn.__version__,profile=PROFILE,profile_sha256=hashlib.sha256(json.dumps(PROFILE,sort_keys=True,separators=(',',':')).encode()).hexdigest(),unprojections=unprojections(),picks=picks())
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    if args.output.exists():raise SystemExit('Preserve old output; choose another path')
    result=capture();args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,sort_keys=True)+'\n');print(len(result['unprojections']),'unprojections',len(result['picks']),'picking cases')
