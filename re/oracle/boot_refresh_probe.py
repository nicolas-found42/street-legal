#!/usr/bin/env python3
"""Original4fc590 conditional renderer refresh, controlled device callbacks.

Absent-device cases execute without helper replacement. Present-device cases
supply virtual slot0x134 results and destructor/free recording boundaries.
No D3D device, GPU work or whole startup is executed. Derived rows re/out/ only.
"""

import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE

assert (
    hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()
    == "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
)


def run(device, retained):
    o = Oracle(fpu_cw=CW_D3D9)
    context = o.alloc(0x208)
    events = []
    old = [0x11223344, 0x55667788] if retained else [0, 0]
    for address, value in zip([0x64ADAC, 0x64ADB0], old):
        o.write_u32(address, value)
    o.write_u32(0x6187B8, 1024)
    if device:
        backend = o.alloc(4)
        vtable = o.alloc(0x138)
        o.write_u32(context + 0x204, backend)
        o.write_u32(backend, vtable)
        o.write_u32(vtable + 0x134, 0x300F0000)

        def create(oc, a):
            events.append({"create": a})
            return 0x90000000 + len([e for e in events if "create" in e])

        o.hook(0x300F0000, create, "thiscall", 2)
        o.hook(
            0x4F5F50,
            lambda oc, a: events.append({"release": oc.reg("ecx")}) or 0,
            "thiscall",
            0,
        )
        o.hook(0x54F5B0, lambda oc, a: events.append({"free": a[0]}) or 0, "cdecl", 1)
    r = o.call(
        0x4FC590, [], convention="fastcall", regs={"ecx": context}, fpu_cw=CW_D3D9
    )
    result = [o.read_u32(a) for a in [0x64ADAC, 0x64ADB0]]
    if device:
        assert result == [0x90000001, 0x90000002]
        assert [e for e in events if "create" in e] == [
            {"create": [1024, 0]},
            {"create": [1024, 0]},
        ]
        assert [e["release"] for e in events if "release" in e] == (
            old if retained else []
        )
        assert [e["free"] for e in events if "free" in e] == (old if retained else [])
    else:
        assert not events and result == old
    return {
        "device": device,
        "retained": retained,
        "before": old,
        "after": result,
        "events": events,
        "instructions": r.icount,
        "oracle": "original4fc590; device and callback boundary as declared",
    }


if __name__ == "__main__":
    for device in [False, True]:
        for retained in [False, True]:
            print(json.dumps(run(device, retained), sort_keys=True))
