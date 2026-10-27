"""Original49e710 tree with declared419860 data and optional49fa60 boundary.

All recursive traversal, spatial classification, segment-box tests and41-style
sphere/box body selection execute unhooked. Synthetic caller layouts only.
"""
import argparse,hashlib,sys,json,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'re/oracle'))
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE
def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]

def run(label,kind=1,children=(),triangles=(),closest=1,flags=0,payload_missing=False,body_missing=False,bounds_kind=1,old=False,original_triangles=False,ray_offset=(0,0),normal_sign=1):
    o=Oracle(fpu_cw=CW_D3D9,stub_imports=False);mapping={};responses={};events=[]
    def resource(kind):
        raw=o.alloc(0x200);payload=o.alloc(0x100)
        o.write_u32(raw+0x4c,kind);o.write_u32(raw+0x50,0xffff1000+(raw-HEAP_BASE)//16)
        tail=raw+0x28;o.write_u32(raw+0x20,tail)
        mapping[raw]=payload
        return raw,payload
    def entity(raw,center=(2,0,0),shape=5):
        p=o.alloc(0x218);body=o.alloc(0xa0);definition=o.alloc(0xac)
        o.write_u32(p+0x44,raw);o.write_u32(p+0x5c,body);o.write_u32(body+0x78,definition)
        o.write_u32(definition+8,shape);o.write_f32(definition+12,1);o.write_f32s(definition+0x9c,[.5,.5,.5])
        o.write_f32s(p+0x64,center)
        for i in range(3):o.write_f32(p+0xa4+i*20,1)
        return p
    root,payload=resource(kind);o.write_u32(root+0x54,flags)
    if payload_missing:mapping[root]=0
    if kind==2 and not body_missing:o.write_u32(payload+0x84,entity(root))
    if triangles:
        records=o.alloc(len(triangles)*32);vertices=o.alloc(36*len(triangles))
        o.write_u32s(payload+0x14,[vertices,len(triangles),records])
        for i,result in enumerate(triangles):
            pointer=records+i*32
            plane=2+i*.5;o.write_f32s(vertices+i*36,[plane,-1,-1,plane,1,-1,plane,-1,1])
            o.write_u32s(pointer,[0x12340000+i,3*i,3*i+1,3*i+2,*map(bits,[normal_sign,0.,0.]),0])
            responses[pointer]=bits(result)
    nodes=[]
    for item in children:
        child,child_data=resource(item['kind']);nodes.append(child)
        mode=item.get('mode',2);o.write_u32(child+0x80,mode)
        center=item.get('center',[2,0,0]);o.write_f32s(child+0x84,center);o.write_f32s(child+0x90,[.5,.5,.5])
        if item['kind']==2:o.write_u32(child_data+0x84,entity(child,center))
    if nodes:o.write_u32(root+0x20,nodes[0])
    for i,node in enumerate(nodes):o.write_u32(node+4,nodes[i+1] if i+1<len(nodes) else root+0x28)
    o.write_f32s(0x643538,[0,*ray_offset,4,*ray_offset,1,0,0,4])
    o.write_u32s(0x643560,[bounds_kind,*map(bits,[2,0,0,2,0,0])])
    o.write_u32(0x64357c,closest);o.write_f32(0x643580,1)
    o.write_u32s(0x643590,[bits(7),bits(8),bits(9),0xfeed,0xcafe])
    if old:
        prior,prior_payload=resource(2);body=entity(prior)
        o.write_u32s(0x6435a4,[body,0]);o.write_u32s(body,[0x6435a4,1])
        o.call(0x429060,[prior],convention='thiscall',this=0x6435ac,fpu_cw=CW_D3D9)
    def state():return dict(heap=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4),globals=o.read_u32s(0x643538,33))
    def data(oc,args):
        raw=oc.reg('ecx');assert args==[0xa0000000,bits(1),0,bits(10)]
        events.append(dict(name='data',resource=raw,args=args,state=state()))
        return mapping[raw]
    def triangle(oc,args):
        assert oc.reg('ecx')==0x610000 and args[0]==args[3]==0 and args[4]==0x643538
        events.append(dict(name='triangle',world=oc.reg('ecx'),args=args,state=state()))
        return struct.unpack('<f',struct.pack('<I',responses[args[2]]))[0]
    o.hook(0x419860,data,'thiscall',4)
    if original_triangles:
        from unicorn import UC_HOOK_CODE
        def observe(uc,address,size,user):
            if address==0x49fa60:events.append(dict(name='triangle',world=o.reg('ecx'),args=o.read_u32s(o.reg('esp')+4,5),state=state()))
        o.uc.hook_add(UC_HOOK_CODE,observe)
    else:o.hook(0x49fa60,triangle,'thiscall',5)
    before=state();r=o.call(0x49e710,[root],convention='thiscall',this=0x610000,fpu_cw=CW_D3D9)
    return dict(label=label,world=0x610000,original_triangles=original_triangles,root=root,mapping=mapping,responses=responses,before=before,after=state(),events=events,result=r.eax)

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-world-tree-original.json');args=parser.parse_args()
    rows=[]
    for kind in [0,1,2,3,0xffffffff]:rows.append(run('kind-'+str(kind),kind=kind))
    rows.append(run('missing-data',payload_missing=True))
    rows.append(run('missing-body',kind=2,body_missing=True))
    for closest in [0,1]:
        for triangles in [[0.],[-1.],[.25],[.25,.75],[.75,.25],[1.],[1.25]]:
            rows.append(run('triangles-'+str(closest)+'-'+str(triangles),triangles=triangles,closest=closest,old=True))
    for mode in [0,1,2,3]:
        for center in [[2,0,0],[2,2,0],[-2,0,0],[6,0,0]]:
            rows.append(run('child-'+str(mode)+'-'+str(center),children=[{'kind':2,'mode':mode,'center':center}],old=True))
    rows.append(run('mixed-children',children=[{'kind':0},{'kind':3},{'kind':1},{'kind':2}],old=True))
    rows.append(run('suppressed-children',children=[{'kind':2}],flags=0x40,old=True))
    rows.append(run('unbounded-ray',children=[{'kind':2,'mode':1}],bounds_kind=2,old=True))
    rows.append(run('original-triangle-hit',triangles=[.25],original_triangles=True,old=True))
    rows.append(run('original-two-triangles-closest',triangles=[.25,.75],original_triangles=True,old=True))
    rows.append(run('original-two-triangles-any',triangles=[.25,.75],closest=0,original_triangles=True,old=True))
    rows.append(run('original-triangle-outside',triangles=[.25],ray_offset=(2,2),original_triangles=True,old=True))
    rows.append(run('original-triangle-backface',triangles=[.25],normal_sign=-1,original_triangles=True,old=True))
    rows.append(run('original-triangle-and-body',triangles=[.25],children=[{'kind':2}],original_triangles=True,old=True))
    for index,row in enumerate(rows):row['id']=index
    packed=b''
    for row in rows:
        groups=[[row['world'],row['root'],int(row['original_triangles'])],[len(row['before']['heap']),*row['before']['heap']],[len(row['before']['globals']),*row['before']['globals']]]
        for key in ['mapping','responses']:
            pairs=sorted((int(k),v)for k,v in row[key].items());groups.append([len(pairs),*[word for pair in pairs for word in pair]])
        for words in groups:packed+=struct.pack('<'+str(len(words))+'I',*words)
    sources=[Path(__file__).resolve(),ROOT/'re/oracle/oracle.py']
    out=dict(schema=1,kernel='world-tree',control_word=CW_D3D9,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),sources={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(sources)},input_sha256=hashlib.sha256(packed).hexdigest(),rows=rows)
    args.output.parent.mkdir(parents=True,exist_ok=True);assert not args.output.is_symlink();args.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
    print('original tree cases',len(rows),'data/triangle calls',sum(len(r['events']) for r in rows),'inputSHA',out['input_sha256'])
if __name__=='__main__':main()
