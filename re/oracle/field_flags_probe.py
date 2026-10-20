#!/usr/bin/env python3
"""Original field-slot selection by flags with supplied decoded metadata.

403e80 initialization is an explicit success boundary. Original404730 chooses
between distinct instance/static field arrays. Derived rows only in re/out/.
"""

import json
from vmexec_vectors import arr, CW
from oracle import Oracle

for flags in [0, 1, 8, 9, 0x20, 0x28, 0x29]:
    o = Oracle(fpu_cw=CW)
    cls = o.alloc(0x200)
    module = o.alloc(0x80)
    field = o.alloc(24)
    other = o.alloc(24)
    o.write_u32(cls + 4, module)
    o.write_u32(field + 4, flags)
    o.write_u32(module + 0x4C, arr(o, [field, other], True))
    o.write_u32(module + 0x54, arr(o, [other, field], True))
    o.hook(0x403E80, lambda oc, a: 1, "thiscall", 0)
    result = o.call(0x404730, [field], convention="thiscall", this=cls, fpu_cw=CW)
    assert result.eax == (1 if flags & 8 else 0)
    print(
        json.dumps(
            {
                "flags": flags,
                "slot": result.eax,
                "static": bool(flags & 8),
                "oracle": "original404730; supplied field arrays and successful initialization",
            },
            sort_keys=True,
        )
    )
