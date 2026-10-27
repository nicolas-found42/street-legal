"""Original49fa60 nullentity kernel: all80 returned bits and all3 pointed inputs.
No function hooks/import stubs. Nonnull entity collision publication excluded.
"""
import argparse,sys,struct,json,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'re/oracle'))

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def inputs():
 cases=[]
 for axis in range(3):
  others=[i for i in range(3) if i!=axis]
  vertices=[[0.,0.,0.]for _ in range(3)];vertices[1][others[0]]=1.;vertices[2][others[1]]=1.
  for sign in [-1,1]:
   normal=[0.,0.,0.];normal[axis]=sign
   for distance in [1.,.001,.01,.02,.03]:
    for u,v in [(.25,.25),(.75,.75),(0,0),(1,0),(0,1),(-.1,.5),(1.1,0),(.5,-.1)]:
     a=[0.,0.,0.];b=list(a);a[axis]=-sign*distance;b[axis]=sign*distance
     for p in [a,b]:p[others[0]]=u;p[others[1]]=v
     cases.append(list(map(bits,sum(vertices,[])+normal+a+b)))
   for start,end in [([-1,.25],[.5,.25]),([.5,.25],[-1,.25]),([1.5,1.5],[.2,.2]),([.2,.2],[1.5,1.5])]:
    a=[0.,0.,0.];b=list(a);a[axis]=-sign*.001;b[axis]=sign*.001
    for p,xy in [(a,start),(b,end)]:p[others[0]],p[others[1]]=xy
    cases.append(list(map(bits,sum(vertices,[])+normal+a+b)))
 state=0x49fa60
 def nxt():
  nonlocal state
  state=(state*1664525+1013904223)&0xffffffff
  return state
 for _ in range(512):cases.append([bits(((nxt()%65)-32)/8)for _ in range(18)])
 pool=[0,0x80000000,0x3f800000,0xbf800000,0x40000000,0xc0000000,1,0x80000001,0x007fffff,0x00800000,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7fc12345,0x7fa12345]
 for _ in range(128):cases.append([pool[(nxt()>>16)%len(pool)]for _ in range(18)])
 return cases

def main():
 from oracle import Oracle,DEFAULT_EXE,CW_D3D9
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,default=ROOT/"re/out/boot/native-ray-game-triangle-original.json");args=parser.parse_args()
 o=Oracle(fpu_cw=CW_D3D9,stub_imports=False);vertices=o.alloc(36);triangle=o.alloc(32);ray=o.alloc(24)
 assert o.read_u32(0x5f1070)==bits(.02) and o.read_u32(0x5f0c24)==bits(1e-5)
 rows=[];cases=inputs()
 for index,w in enumerate(cases):
  o.write_u32s(vertices,w[:9]);o.write_u32s(triangle,[0,0,1,2,*w[9:12],0]);o.write_u32s(ray,w[12:])
  def read():return o.read(vertices,36)+o.read(triangle,32)+o.read(ray,24)
  before=read();result=o.call(0x49fa60,[0,vertices,triangle,0,ray],convention='thiscall',this=0x610000,fpu_cw=CW_D3D9)
  assert read()==before and result.fpu_top_depth==1
  rows.append(dict(id=index,words=w,st0=result.st0_raw.hex(),input_unchanged=True))
 out=dict(schema=1,kernel="game-triangle",scope=__doc__,control_word=CW_D3D9,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),input_sha256=hashlib.sha256(b''.join(struct.pack('<18I',*w)for w in cases)).hexdigest(),rows=rows)
 sources=[Path(__file__).resolve(),ROOT/"re/oracle/oracle.py"]
 out["sources"]={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(sources)}
 args.output.parent.mkdir(parents=True,exist_ok=True);assert not args.output.is_symlink()
 args.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
 print('original nullentity triangles',len(rows),'inputSHA',out['input_sha256'])
if __name__=='__main__':main()
