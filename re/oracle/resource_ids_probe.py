#!/usr/bin/env python3
"""Controlled original runtime resource-ID allocation/recycling.

Runs original 536ed0/536f90 with explicit initial free-stack state and a bump
reallocator boundary. This does not establish pre-Config whole-boot allocation
counts. Derived JSONL must be redirected to ignored re/out/ only.
"""

import sys, json, hashlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle, CW_D3D9, DEFAULT_EXE

assert hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest() == (
    "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
)

for seed in [1, 65530]:
    o = Oracle(fpu_cw=CW_D3D9)
    o.hook_alloc()
    manager = o.alloc(0xDB0)
    o.write_u32(manager + 0xDAC, seed)

    def realloc(oc, a):
        p = oc.alloc(a[1])
        old = a[0]
        if old:
            oc.write(p, oc.read(old, oc.heap_sizes.get(old, 0)))
        return p

    o.hook(0x54F580, realloc, "cdecl", 2)
    got = []
    for _ in range(130):
        got.append(
            o.call(
                0x536ED0,
                [],
                convention="fastcall",
                regs={"ecx": manager},
                fpu_cw=CW_D3D9,
            ).eax
        )
    expected = [0xFFFF0000 | ((seed + 127 - i) & 65535) for i in range(128)] + [
        0xFFFF0000 | ((seed + 255 - i) & 65535) for i in range(2)
    ]
    assert got == expected
    o.call(0x536F90, [0xFFFF1234], convention="thiscall", this=manager, fpu_cw=CW_D3D9)
    recycled = o.call(
        0x536ED0, [], convention="fastcall", regs={"ecx": manager}, fpu_cw=CW_D3D9
    ).eax
    assert recycled == 0xFFFF1234
    print(
        json.dumps(
            {
                "seed": seed,
                "ids": got,
                "recycled": recycled,
                "remaining": o.read_u32(manager + 0xDA4),
                "next": o.read_u32(manager + 0xDAC),
            },
            sort_keys=True,
        )
    )
