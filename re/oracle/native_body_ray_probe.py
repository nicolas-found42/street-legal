#!/usr/bin/env python3
"""Original49deb0 unhooked sphere/box/mesh body selection, complete caller heap/globals.
Synthetic layouts, no production or complete collision-engine claim.
"""
import argparse,hashlib,json,struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'re/oracle'))
from oracle import Oracle,DEFAULT_EXE,CW_D3D9,HEAP_BASE

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def run(shape,active,center,half,radius,limit=1.,flags=0,closest=1,rotation=None,child=False,old=False):
 o=Oracle(fpu_cw=CW_D3D9,stub_imports=False)
 def entity(center,shape,flags):
  entity=o.alloc(0x218);body=o.alloc(0xa0);definition=o.alloc(0xac);raw=o.alloc(0x200)
  o.write_u32(raw+0x4c,2);o.write_u32(raw+0x50,0xffff0040+(entity-HEAP_BASE)//16)
  o.write_u32(entity+0x44,raw);o.write_u32(entity+0x58,flags);o.write_u32(entity+0x5c,body);o.write_u32(entity+0x20c,active);o.write_u32(body+0x78,definition)
  o.write_u32(definition+8,shape);o.write_f32(definition+12,radius);o.write_f32s(definition+0x9c,half)
  if shape in [6,7]:
   vertices=o.alloc(72);triangles=o.alloc(64)
   o.write_f32s(vertices,[0,-1,-1,0,1,-1,0,-1,1,.5,-1,-1,.5,1,-1,.5,-1,1])
   for i in range(2):o.write_u32s(triangles+i*32,[0,3*i,3*i+1,3*i+2,0,bits(1),0,0])
   o.write_u32s(definition+0xa0,[vertices,2,triangles])
  for buf in [0,1]:
   o.write_f32s(entity+0x64+buf*0xd4,center)
   for row,values in enumerate(rotation or [[1,0,0],[0,1,0],[0,0,1]]):o.write_f32s(entity+0xa4+buf*0xd4+row*16,values)
  return entity,body,definition,raw
 root,*_=entity(center,shape,flags)
 if child:
  nested,*_=entity([2,0,0],5,0);o.write_u32(root+0x24,nested)
 o.write_f32s(0x643538,[0,0,0,4,0,0,1,0,0,4])
 o.write_u32(0x64357c,closest);o.write_f32(0x643580,limit)
 for address in range(0x643584,0x64359c,4):o.write_u32(address,0xa5a5a5a5)
 o.write_u32(0x64359c,0xfeed);o.write_u32(0x6435a0,0xcafe)
 if old:
  prior,*rest=entity([0,0,0],1,0);raw=rest[-1]
  o.write_u32s(0x6435a4,[prior,0]);o.write_u32s(prior,[0x6435a4,1])
  o.call(0x429060,[raw],convention='thiscall',this=0x6435ac,fpu_cw=CW_D3D9)
 def state():return dict(heap=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE+3)//4),globals=o.read_u32s(0x643538,33))
 before=state();r=o.call(0x49deb0,[root],convention='thiscall',this=0x610000,fpu_cw=CW_D3D9)
 return dict(shape=shape,active=active,center=center,half=half,radius=radius,limit=limit,flags=flags,closest=closest,child=child,old=old,rotation=rotation,entity=root,before=before,after=state(),result=r.eax)

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-body-ray-original.json');args=parser.parse_args()
 rows=[]
 for shape in [0,1,5,6,7]:
  for active in [0,1]:
   for center,half,radius in [([2,0,0],[.5,.5,.5],1),([2,2,0],[.5,.5,.5],1),([0,0,0],[1,1,1],2),([-2,0,0],[1,1,1],1),([6,0,0],[1,1,1],1)]:
    rows.append(run(shape,active,center,half,radius))
 for flags in [0x20,0x4000,0x4020]:rows.append(run(5,0,[2,0,0],[.5,.5,.5],1,flags=flags))
 for limit in [0.,.25,.375,.5,1.]:rows.append(run(5,1,[2,0,0],[.5,.5,.5],1,limit=limit,old=True))
 rows.append(run(5,0,[2,0,0],[.5,1,.25],1,rotation=[[0,1,0],[-1,0,0],[0,0,1]],old=True))
 rows.append(run(5,1,[2,0,0],[1,1,1],2,child=True))
 rows.append(run(5,1,[2,0,0],[.5,.5,.5],1,closest=0))
 for shape in [6,7]:
  for closest in [0,1]:rows.append(run(shape,1,[2,0,0],[.5,.5,.5],2,closest=closest,old=True))
 out=args.output;out.parent.mkdir(parents=True,exist_ok=True);assert not out.is_symlink()
 for index,row in enumerate(rows):row.update(id=index,world=0x610000)
 packed=b''
 for row in rows:
  for words in [[row['world'],row['entity']], [len(row['before']['heap']),*row['before']['heap']],[len(row['before']['globals']),*row['before']['globals']]]:packed+=struct.pack('<'+str(len(words))+'I',*words)
 sources=[Path(__file__).resolve(),ROOT/'re/oracle/oracle.py']
 meta=dict(schema=1,kernel='body',control_word=CW_D3D9,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),sources={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(sources)},input_sha256=hashlib.sha256(packed).hexdigest(),rows=rows)
 out.write_text(json.dumps(meta,separators=(',',':'))+'\n')
 print('unhooked original body cases',len(rows),'inputSHA',meta['input_sha256'])
if __name__=='__main__':main()
