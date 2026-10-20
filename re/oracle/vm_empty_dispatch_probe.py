#!/usr/bin/env python3
"""Original 41fbc0 null-body-pointer dispatch with controlled metadata.

Uses vmexec_vectors' explicit heap/class-init seams. No script body executes.
Redirect derived JSONL into ignored re/out/boot/empty-dispatch.jsonl.
"""

import sys, json
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from vmexec_vectors import setup, arr, CW, TC


def run(arguments, return_type):
    s, p, tr, base, leaf, obj, methods, events = setup()
    o = s.o
    m = methods[0]
    tc = []
    for ty in (["I", "F"] if arguments else []) + [return_type]:
        if ty is None:
            tc.append(0)
            continue
        cell = o.alloc(4)
        o.write_u32(cell, TC[ty])
        tc.append(cell)
    sig = o.alloc(12)
    o.write_u32s(sig, [s.jvm, 0, arr(o, tc)])
    o.write_u32(m + 24, sig)
    if arguments:
        s._container_set(
            s.frames[0] + 28,
            [
                s._cell({"k": "v", "t": "I", "w": 11}),
                s._cell({"k": "v", "t": "F", "w": 0x40200000}),
            ],
        )
    o.write_u32(base + 24, arr(o, [0]))
    rec = o.alloc(16)
    o.write_u32s(rec, [base, 0, m, 1])
    caller = s.frames[0]
    pc = o.read_u32(s.thread + 0x24)
    r = o.call(
        0x41FBC0,
        [rec, base, obj],
        convention="thiscall",
        this=s.thread,
        fpu_cw=CW,
        limit=500000,
    )
    vals = s._container(caller + 28)
    result = {
        "arguments": arguments,
        "return_type": return_type,
        "return_code": r.eax,
        "same_frame": caller == o.read_u32(s.thread + 0x20),
        "same_pc": pc == o.read_u32(s.thread + 0x24),
        "stack": [
            {
                "type": o.read_cstr(o.read_u32(v + 4)).decode(),
                "payload": o.read_u32(v + 8),
            }
            for v in vals
        ],
    }
    assert (
        r.eax == 1
        and result["same_frame"]
        and result["same_pc"]
        and result["stack"]
        == ([] if return_type is None else [{"type": "I", "payload": 0}])
    )
    return result


if __name__ == "__main__":
    for arguments in [False, True]:
        for return_type in [None, "I", "V"]:
            print(json.dumps(run(arguments, return_type), sort_keys=True))
