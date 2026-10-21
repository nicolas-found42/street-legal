#!/usr/bin/env python3
"""Original543610 minimum-quality arithmetic, clamping and generation updates.

No collaborators are replaced. Full resource words are retained, including
unchanged fields. Inputs include finite, signed-zero, infinities and NaNs.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9

def run():
    rows = []
    for value in [0, 0x80000000, 0x3e800000, 0x40000000, 0xbf800000,
                  0x7f800000, 0xff800000, 0x7fc12345]:
        for stamp in [122, 123, 124]:
            for bias, scale in [(0, 0x3f800000), (0x3e800000, 0x3f000000)]:
                o = Oracle()
                raw = o.alloc(0x26c)
                o.write_u32(0x6200a4, 123)
                o.write_u32(raw + 0x6c, 0x3f000000)
                o.write_u32(raw + 0x78, stamp)
                o.write_u32(raw + 0xb8, bias)
                o.write_u32(raw + 0xbc, scale)
                before = o.read_u32s(raw, 0x26c // 4)
                o.call(0x543610, [value], convention='thiscall', this=raw, fpu_cw=CW_D3D9)
                rows.append(dict(resource=raw, value=value, before=before,
                                 after=o.read_u32s(raw, 0x26c // 4)))
    return dict(scope=__doc__, exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(), rows=rows)

if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
