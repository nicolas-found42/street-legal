#!/usr/bin/env python3
"""Original54ecd0 complete buffers and instruction-local x87 sin/cos results.

Executes original code with no substituted math helpers. The six observation
sites are immediately after the real FSIN/FCOS instructions; hooks only read.
Ordinary finite angles remain diagnostic until an exact provider is available.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import random
import struct

from oracle import Oracle, CW_D3D9, HEAP_BASE
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_FPSW

ROOT=Path(__file__).resolve().parents[2]
DEFAULT=ROOT/'re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json'


def word(value):return struct.unpack('<I',struct.pack('<f',value))[0]


def domain(bits):
    absolute=bits&0x7fffffff
    if absolute<=0x2f000000:return 'tiny-exact'
    if 0x5f000000<absolute<0x7f800000:return 'range-c2'
    if absolute==0x5f000000:return 'range-boundary-unresolved'
    if absolute>=0x7f800000:return 'nonfinite-unresolved'
    return 'regular-unresolved'


def capture():
    cases=[]
    for signs in itertools.product([0,0x80000000],repeat=3):cases.append(('signed-zero',list(signs)))
    for value in [1,0x007fffff,0x00800000,0x2e800000,0x2f000000,0x2f000001,
                  0x5effffff,0x5f000000,0x5f000001,0x7f7fffff,
                  0x7f800000,0x7fc01234,0x7f800001]:
        for sign in [0,0x80000000]:
            for axis in range(3):
                angles=[0,0,0];angles[axis]=value|sign
                cases.append((domain(value),angles))
    cases.extend([('nan-payload-order',[0x7fc11111,0xffc22222,0x7fc33333]),
                  ('nan-signaling-order',[0xff800123,0x7f800456,0xff800789])])
    for angle in [0.1,0.25,0.5,1.0,-1.0,1.5707963267948966,3.141592653589793,6.283185307179586]:
        for axis in range(3):
            angles=[0,0,0];angles[axis]=word(angle)
            cases.append(('ordinary-angle',angles))
    rng=random.Random(0x54ecd0)
    cases.extend(('finite-mixed',[word(rng.uniform(-6.3,6.3)) for _ in range(3)]) for _ in range(48))
    rows=[]
    sites={0x54ecdb:(0,'sin'),0x54ecdf:(0,'cos'),0x54ece8:(1,'sin'),
           0x54eced:(1,'cos'),0x54ecf6:(2,'sin'),0x54ecff:(2,'cos')}
    for index,(label,angles) in enumerate(cases):
        o=Oracle()
        buffer=o.alloc(0x90);out=buffer+0x10
        source=buffer+0x60 if index%3==0 else out+(0 if index%3==1 else 8)
        o.write_u32s(buffer,[0x5a010000+i*137 for i in range(0x90//4)])
        o.write_u32s(source,angles)
        before=o.read_u32s(HEAP_BASE,0x90//4)
        observations=[]
        def observe(uc,address,size,user):
            if address in sites:
                axis,op=sites[address]
                observations.append(dict(site=address,axis=axis,op=op,input=angles[axis],
                                         st0=o._read_st0_raw().hex(),fpsw=uc.reg_read(UC_X86_REG_FPSW)))
        o.uc.hook_add(UC_HOOK_CODE,observe)
        result=o.call(0x54ecd0,[source],convention='thiscall',this=out,fpu_cw=CW_D3D9)
        assert len(observations)==6
        rows.append(dict(index=index,label=label,domains=[domain(w) for w in angles],angles=angles,
                         out=out,source=source,before=before,after=o.read_u32s(HEAP_BASE,0x90//4),
                         trig=observations,fpsw=result.fpsw,fpcw=result.fpcw,fpu_depth=result.fpu_top_depth))
    path=ROOT/'reference/raw/StreetLegal_Redline.bak.exe'
    return dict(schema='slr-original-euler-v2',original_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                entry=0x54ecd0,fpu_cw=CW_D3D9,hooks='read-only instruction observations; no substituted code',
                function_bytes_sha256=hashlib.sha256(o.read(0x54ecd0,0xbf)).hexdigest(),
                comparison='entire caller heap including matrix padding and aliased source; raw x87 sin/cos intermediates',rows=rows)


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('action',choices=['gen','verify']);ap.add_argument('--out',type=Path,default=DEFAULT)
    args=ap.parse_args()
    if not args.out.resolve().is_relative_to(ROOT/'re/out'):ap.error('output must stay under ignored re/out')
    result=capture()
    if args.action=='verify':
        if json.loads(args.out.read_text())!=result:raise SystemExit('original Euler replay differs')
    else:
        if args.out.exists():raise SystemExit('preserve capture; choose fresh --out or verify')
        args.out.parent.mkdir(parents=True,exist_ok=True);args.out.write_text(json.dumps(result)+'\n')
    print(f'{len(result["rows"])} original54ecd0 rows: {args.action}')


if __name__=='__main__':main()
