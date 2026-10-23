#!/usr/bin/env python3
"""Complete original54ecd0 buffers for a reduced-angle output-certification experiment.

Original code executes without substituted functions. The input cube is bounded
by9/8 radians, inside both Intel SDM8.3.10 safe FSIN/FCOS argument domains.
Every word of the caller allocation is retained, including aliasing and padding.
These observations do not certify raw extF80 instruction values or status flags.
"""
import hashlib
import itertools
import json
from pathlib import Path
import random
import struct
from oracle import Oracle, CW_D3D9, DEFAULT_EXE, HEAP_BASE


def word(x):return struct.unpack('<I',struct.pack('<f',x))[0]


def cases():
    rows=[list(x) for x in itertools.product([0,0x80000000],repeat=3)]
    # General edge families, not captured output values or trace positions.
    for bits in [1,0x7fffff,0x800000,0x2f000000,0x2f000001,
                 word(0.1),word(0.157),word(0.25),word(0.5),word(1),word(1.125)]:
        for sign,axis in itertools.product([0,0x80000000],range(3)):
            a=[0,0,0];a[axis]=bits|sign;rows.append(a)
    rng=random.Random(0x54ecd0)
    rows.extend([[word(rng.uniform(-1.125,1.125)) for _ in range(3)] for _ in range(256)])
    return rows


def capture():
    rows=[]
    for index,angles in enumerate(cases()):
        o=Oracle(fpu_cw=CW_D3D9);buffer=o.alloc(0x90);out=buffer+0x10
        source=buffer+0x60 if index%3==0 else out+(0 if index%3==1 else 8)
        o.write_u32s(buffer,[0x5a010000+i*137 for i in range(0x90//4)])
        o.write_u32s(source,angles)
        before=o.read_u32s(HEAP_BASE,0x90//4)
        result=o.call(0x54ecd0,[source],convention='thiscall',this=out)
        rows.append(dict(index=index,angles=angles,source=source,out=out,before=before,
                         after=o.read_u32s(HEAP_BASE,0x90//4),fpcw=result.fpcw,
                         fpsw=result.fpsw,depth=result.fpu_top_depth))
    return dict(schema=1,entry=0x54ecd0,fpu_cw=CW_D3D9,
                exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                scope=__doc__,rows=rows)


if __name__=='__main__':print(json.dumps(capture(),sort_keys=True))
