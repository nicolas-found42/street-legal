#!/usr/bin/env python3
"""Original48d3d0 subscription flags through the real53efa0 virtual getter.

Run: ~/slr-oracle-venv/bin/python re/oracle/native_resource_flags_probe.py > re/out/boot/native-resource-flags-original.json
Inputs are a part's embedded Ref bound to its own loaded game resource, a
distinct owning car resource, and independent native body state at body+70.
The unbound branch is included. No executable call or virtual dispatch is
hooked. Every heap word is compared, including the owning car and native body.
"""
import hashlib
import json
from pathlib import Path

from oracle import Oracle, DEFAULT_EXE, HEAP_BASE


def image(o):
    return o.read_u32s(HEAP_BASE, (o.heap_ptr - HEAP_BASE) // 4)


def run():
    rows = []
    for bound in [False, True]:
        for initial in [0, 0x12345678, 0xffffffff]:
            for flags in [0, 0x60, 0x10295, 0xffffffff]:
                o = Oracle()
                own = o.alloc(0x200)
                car = o.alloc(0x200)
                body = o.alloc(0x11c)
                for raw in [own, car]:
                    o.write_u32(raw, 0x5f375c)
                    o.write_u32(raw + 0x4c, 1)
                    o.write_u32(raw + 0x64, 1)
                o.write_u32(own + 0x14, car)
                o.write_u32(body + 0xc, own if bound else 0)
                o.write_u32(body + 0x70, 0x66778899)
                o.write_u32(own + 0xd8 + 0x70, initial)
                o.write_u32(car + 0xd8 + 0x70, 0x55aa55aa)
                before = image(o)
                result = o.call(0x48d3d0, [body, flags])
                rows.append(dict(bound=bound, flags=flags, initial=initial,
                                 own=own, car=car, body=body,
                                 before=before, after=image(o), result=result.eax))
    return dict(schema_version=1, scope=__doc__,
                exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                addresses=[0x48d3d0, 0x53efa0], rows=rows)


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
