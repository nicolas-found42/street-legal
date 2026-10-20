#!/usr/bin/env python3
"""Original427bf0 settings application over controlled Config field getters.

Resources/field lookup and final graphics refresh are supplied boundaries.
Original arithmetic and global writes execute at PC24. No graphics engine,
whole startup, Config constructor or original script bodies execute here.
Redirect derived rows to ignored re/out/boot/settings.jsonl.
"""

import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE

assert (
    hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()
    == "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
)


def run(optional):
    o = Oracle(fpu_cw=CW_D3D9)
    data = o.alloc(0x60)
    o.write_u32(data + 0x50, 0x12345678)
    calls = []

    def field(oc, args, kind, index):
        name = oc.read_cstr(args[index]).decode("latin1")
        if kind == "has":
            value = int(optional)
        elif kind == "int":
            value = 17
        else:
            value = 2.25
        calls.append({"kind": kind, "name": name, "value": value})
        return value

    o.hook(0x429190, lambda oc, a: data, "thiscall", 4)
    o.hook(0x42A430, lambda oc, a: field(oc, a, "int", 1), "cdecl", 2)
    o.hook(0x42A560, lambda oc, a: field(oc, a, "float", 1), "cdecl", 2)
    o.hook(0x42A800, lambda oc, a: field(oc, a, "has", 1), "cdecl", 3)
    o.hook(0x426100, lambda oc, a: field(oc, a, "has", 0), "cdecl", 2)
    o.hook(0x426170, lambda oc, a: field(oc, a, "int", 0), "cdecl", 1)
    o.hook(0x4261F0, lambda oc, a: field(oc, a, "float", 0), "cdecl", 1)
    o.hook(
        0x4FC590, lambda oc, a: calls.append({"refresh": "0x4fc590"}) or 0, "cdecl", 0
    )
    result = o.call(0x427BF0, [], fpu_cw=CW_D3D9, limit=100000)
    writes = [
        {"address": hex(a), "size": n, "bits": v}
        for a, n, v in result.mem_writes
        if 0x600000 <= a < 0x800000
    ]
    assert calls[-1] == {"refresh": "0x4fc590"}
    assert all(w["size"] == 4 for w in writes)
    assert writes
    return {
        "optional": optional,
        "calls": calls,
        "writes": writes,
        "instructions": result.icount,
        "oracle": "original427bf0 arithmetic/globalwrites; controlled field getters and graphics refresh",
    }


if __name__ == "__main__":
    for optional in [False, True]:
        print(json.dumps(run(optional), sort_keys=True))
