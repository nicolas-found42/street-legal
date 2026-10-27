#!/usr/bin/env python3
"""Original498240 segment/AABB interval result, with no function hooks.

Compare the complete returned80-bit value and both pointed-to3-word input arrays
at CW007f. The deterministic authored corpus covers directed slabs, degenerate
segments, boundary/tie cases, normal random geometry and special operands.
CPU register/stack contents and exception flags are not claimed by this slice.
"""
import argparse, hashlib, json, struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]

def bits(v): return struct.unpack('<I',struct.pack('<f',v))[0]
def inputs():
    cases=[]
    for axis in range(3):
        for sign in [-1,1]:
            a=[0.,0.,0.];b=list(a);a[axis]=-2*sign;b[axis]=2*sign
            cases.append([*map(bits,a+b),*[bits(1.)]*3])
    for a,b,e in [([0,0,0],[0,0,0],[1,1,1]),([2,2,2],[2,2,2],[1,1,1]),
                  ([-1,-1,-1],[1,1,1],[1,1,1]),([-2,1,1],[2,1,1],[1,1,1]),
                  ([-2,1.000001,0],[2,1.000001,0],[1,1,1]),
                  ([-2,0,0],[2,0,0],[0,0,0]),([-2,0,0],[2,0,0],[-1,1,1]),
                  ([1,0,0],[2,0,0],[1,1,1]),([2,0,0],[1,0,0],[1,1,1]),
                  ([0,0,0],[2,0,0],[1,1,1])]:cases.append(list(map(bits,a+b+e)))
    state=0x498240
    def next_word():
        nonlocal state
        state=(state*1664525+1013904223)&0xffffffff
        return state
    for _ in range(512):
        a=[((next_word()%65)-32)/8 for _ in range(6)]
        e=[(next_word()%33)/8 for _ in range(3)]
        cases.append(list(map(bits,a+e)))
    pool=[0,0x80000000,0x3f800000,0xbf800000,0x40000000,0xc0000000,1,0x80000001,
          0x007fffff,0x00800000,0x7f7fffff,0xff7fffff,0x7f800000,0xff800000,0x7fc12345,0x7fa12345]
    for _ in range(128):cases.append([pool[(next_word()>>16)%len(pool)] for _ in range(9)])
    # X delta stays live in x87 even when it exceeds the m32 exponent range.
    cases.append([0x7f7fffff,0,0,0xff7fffff,0,0,0x7effffff,0x3f800000,0x3f800000])
    # Z far remains live below the smallest m32 value until interval selection.
    cases.append([0x3f800000,0,0x3faaaaab,0x80000001,0,0,0,0x3f800000,1])
    return cases

def read_inputs(reader,start,end):
    return reader.read(start,12)+reader.read(end,12)

def main():
    from oracle import Oracle, DEFAULT_EXE, CW_D3D9
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,default=ROOT/'re/out/boot/native-ray-box-original.json');a=p.parse_args()
    cases=inputs();rows=[]
    for index,words in enumerate(cases):
        o=Oracle(fpu_cw=CW_D3D9,stub_imports=False);start=o.alloc(12);end=o.alloc(12);o.write_u32s(start,words[:3]);o.write_u32s(end,words[3:6])
        before=read_inputs(o,start,end);result=o.call(0x498240,[start,end,*words[6:]],fpu_cw=CW_D3D9)
        assert read_inputs(o,start,end)==before and result.fpu_top_depth==1
        rows.append(dict(id=index,words=words,st0=result.st0_raw.hex(),input_unchanged=True))
    source=[Path(__file__),Path(__file__).with_name('oracle.py')]
    out=dict(schema=1,scope=__doc__,control_word=CW_D3D9,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),sources={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in source},input_sha256=hashlib.sha256(b''.join(struct.pack('<9I',*words) for words in cases)).hexdigest(),rows=rows)
    a.output.parent.mkdir(parents=True,exist_ok=True);assert not a.output.is_symlink();a.output.write_text(json.dumps(out,separators=(',',':'))+'\n');print('original box rows',len(rows),'input SHA',out['input_sha256'])
if __name__=='__main__':main()
