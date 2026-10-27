"""Unhooked original49f7b0 mesh ray: returned80bits and four complete input arrays."""
import argparse,sys,struct,json,hashlib,importlib.util
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT/'re/oracle'))
import native_ray_game_triangle_probe as game
def main():
 from oracle import Oracle,DEFAULT_EXE,CW_D3D9
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,default=ROOT/"re/out/boot/native-ray-mesh-triangle-original.json");args=parser.parse_args()
 o=Oracle(fpu_cw=CW_D3D9,stub_imports=False);vertices=o.alloc(36);normal=o.alloc(12);indices=o.alloc(12);ray=o.alloc(24)
 o.write_u32s(indices,[0,1,2]);rows=[];cases=[w+[limit]for w in game.inputs() for limit in [0,0x3e800000,0x3f000000,0x3f800000]]
 for index,w in enumerate(cases):
  o.write_u32s(vertices,w[:9]);o.write_u32s(normal,w[9:12]);o.write_u32s(ray,w[12:18])
  def read():return o.read(vertices,36)+o.read(normal,12)+o.read(indices,12)+o.read(ray,24)
  before=read();result=o.call(0x49f7b0,[vertices,normal,indices,ray,w[18]],convention='stdcall',fpu_cw=CW_D3D9)
  assert read()==before and result.fpu_top_depth==1
  rows.append(dict(id=index,words=w,st0=result.st0_raw.hex(),input_unchanged=True))
 out=dict(schema=1,kernel="mesh-triangle",scope=__doc__,control_word=CW_D3D9,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),input_sha256=hashlib.sha256(b''.join(struct.pack('<19I',*w)for w in cases)).hexdigest(),rows=rows)
 sources=[Path(__file__).resolve(),ROOT/"re/oracle/oracle.py",ROOT/"re/oracle/native_ray_game_triangle_probe.py"]
 out["sources"]={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(sources)}
 args.output.parent.mkdir(parents=True,exist_ok=True);assert not args.output.is_symlink()
 args.output.write_text(json.dumps(out,separators=(',',':'))+'\n')
 print('original mesh triangles',len(rows),'inputSHA',out['input_sha256'])
if __name__=='__main__':main()
