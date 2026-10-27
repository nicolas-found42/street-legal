#!/usr/bin/env python3
"""Original498580 ray/sphere broadphase: returned80bits and3pointed input arrays.
No imported function stubs or code hooks. Scratch stack/exception flags excluded.
"""
import argparse,hashlib,json,struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'re/oracle'))

def bits(v):return struct.unpack('<I',struct.pack('<f',v))[0]
def inputs():
 cases=[]
 for start,direction,length,center,radius in [
  ([0,0,0],[1,0,0],4,[2,0,0],1),
  ([0,0,0],[1,0,0],4,[-2,0,0],1),
  ([0,0,0],[1,0,0],4,[-.5,0,0],1),
  ([0,0,0],[1,0,0],4,[6,0,0],1),
  ([0,0,0],[1,0,0],4,[4.5,0,0],1),
  ([0,0,0],[1,0,0],4,[2,1,0],1),
  ([0,0,0],[1,0,0],4,[2,1.000001,0],1),
  ([0,0,0],[1,0,0],0,[0,0,0],1),
  ([0,0,0],[1,0,0],4,[0,0,0],0),
  ([0,0,0],[1,0,0],4,[2,0,0],-1),
  ([0,0,0],[0,0,1],4,[0,0,2],1),
  ([0,0,0],[0,1,0],4,[0,2,0],1)]:
  cases.append(list(map(bits,start+direction+[length]+center+[radius])))
 state=0x498580
 def nxt():
  nonlocal state
  state=(state*1664525+1013904223)&0xffffffff
  return state
 for _ in range(512):
  cases.append([bits(((nxt()%65)-32)/8) for _ in range(6)]+[bits((nxt()%65)/8)]+[bits(((nxt()%65)-32)/8) for _ in range(3)]+[bits((nxt()%33)/8)])
 pool=[0,0x80000000,0x3f800000,0xbf800000,0x40000000,0xc0000000,1,0x80000001,0x007fffff,0x00800000,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7fc12345,0x7fa12345]
 for _ in range(128):cases.append([pool[(nxt()>>16)%len(pool)] for _ in range(11)])
 cases.append([0,0,0xff7fffff,0x3f800000,0,0,0x40800000,0,0,0x7f7fffff,0x7f7fffff])
 return cases

def main():
 from oracle import Oracle,DEFAULT_EXE,CW_D3D9
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,default=ROOT/"re/out/boot/native-ray-sphere-original.json");args=parser.parse_args()
 o=Oracle(fpu_cw=CW_D3D9,stub_imports=False)
 start,direction,center=[o.alloc(12) for _ in range(3)]
 rows=[];cases=inputs()
 for index,words in enumerate(cases):
  for p,ws in [(start,words[:3]),(direction,words[3:6]),(center,words[7:10])]:o.write_u32s(p,ws)
  def read():return b''.join(o.read(p,12) for p in [start,direction,center])
  before=read();r=o.call(0x498580,[start,direction,words[6],center,words[10]],fpu_cw=CW_D3D9)
  assert read()==before and r.fpu_top_depth==1
  rows.append(dict(id=index,words=words,st0=r.st0_raw.hex(),input_unchanged=True))
 sources=[Path(__file__).resolve(),ROOT/"re/oracle/oracle.py"]
 result=dict(schema=1,kernel="sphere",scope=__doc__,control_word=CW_D3D9,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),sources={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(sources)},input_sha256=hashlib.sha256(b''.join(struct.pack('<11I',*w) for w in cases)).hexdigest(),rows=rows)
 out=args.output
 out.parent.mkdir(parents=True,exist_ok=True);assert not out.is_symlink()
 out.write_text(json.dumps(result,separators=(',',':'))+'\n')
 print('original sphere rows',len(rows),'inputSHA',result['input_sha256'])
if __name__=='__main__':main()
