#!/usr/bin/env python3
"""Original Viewport.create -> 538d60 -> ID allocator and native handle binding.

Resource registration/virtual materialisation and renderer registration are
controlled seams. The marshaller, allocator, state stores and intrusive handle
operations execute original x86. No D3D device or whole-startup claim.
Redirect derived rows to ignored re/out/boot/viewport-create.jsonl.
"""
import hashlib
import json
from pathlib import Path

from oracle import CW_D3D9, DEFAULT_EXE, Oracle
from render_vectors import _cell, _frame


def run(initially_bound=False, missing_handle=False):
    assert hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest() == (
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    )
    o = Oracle(fpu_cw=CW_D3D9)
    o.hook_alloc()
    manager = o.alloc(0xDB0)
    o.write_u32(manager + 0xDAC, 1)
    o.write_u32(0x618D48, manager)

    def realloc(oc, args):
        p = oc.alloc(args[1])
        if args[0]:
            oc.write(p, oc.read(args[0], oc.heap_sizes[args[0]]))
        return p

    o.hook(0x54F580, realloc, "cdecl", 2)
    # Declared earlier allocations, matching the driver's supplied Config/Init
    # reservation. This does not reconstruct original startup history.
    reserved = [o.call(0x536ED0, [], convention="thiscall", this=manager).eax for _ in range(2)]
    receiver, handle, old, other, created, data, vt = [o.alloc(0x100) for _ in range(7)]
    if initially_bound:
        o.write_u32s(handle, [0, other, 123, old])
        o.write_u32s(other, [handle, 0, 123, old])
        o.write_u32(old + 0x48, handle)
    o.write_u32(created, vt)
    o.write_u32(vt + 0xC, 0x300F0000)
    o.hook(0x300F0000, lambda oc, a: data, "thiscall", 1)
    o.hook(0x42AB50, lambda oc, a: 0 if missing_handle else handle, "cdecl", 2)
    o.hook(0x544B90, lambda oc, a: None, "thiscall", 1)
    events = []

    def construct(oc, args):
        descriptor = oc.read_u32s(args[2], 5)
        assert descriptor == [0x12, args[0], 0, 0, 0]
        assert args[1] == 0 and args[3:] == [0, 0]
        oc.write_u32(created + 0x50, args[0])
        events.append({"construct": descriptor, "owner": args[1]})
        return created

    o.hook(0x536890, construct, "thiscall", 5)

    def register(oc, args):
        assert oc.read_u32s(args[0] + 8, 2) == [oc.read_u32(created + 0x50), created]
        assert args[1] == data
        events.append({"renderer_register": True})

    o.hook(0x4FCE00, register, "thiscall", 2)
    o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
    o.hook(0x5513B0, lambda oc, a: None, "cdecl", 1)
    state = [127, 0x3E800000, 0x3F000000, 0x3F400000, 0x3F800000]
    params = list(zip("IFFFF", state))
    cells = [_cell(o, ty, value) for ty, value in reversed(params)]
    cells.append(_cell(o, "L", receiver))
    frame = _frame(o, "java.render.Viewport", "create", "(IFFFF)V", cells)
    o.call(0x4814E0, [frame], fpu_cw=CW_D3D9)
    id_frame = _frame(o, "java.util.resource.ResourceRef", "id", "()I", [_cell(o, "L", receiver)])
    result_id = o.call(0x47D290, [id_frame], fpu_cw=CW_D3D9).eax
    expected = 0 if missing_handle else 0xFFFF007E
    assert result_id == expected
    if not missing_handle:
        assert o.read_u32s(data + 0xC, 5) == state
        assert o.read_u32s(handle, 4) == [0, 0, expected, created]
        assert o.read_u32(created + 0x48) == handle
        if initially_bound:
            assert o.read_u32(old + 0x48) == other
            assert o.read_u32(other) == 0
    else:
        assert events == []
    remaining = o.read_u32(manager + 0xDA4)
    next_id = o.call(0x536ED0, [], convention="thiscall", this=manager).eax
    return {
        "oracle": "original4814e0/538d60/536ed0/47d290; controlled registration/materialisation/renderer and ptr lookup",
        "initially_bound": initially_bound, "missing_handle": missing_handle,
        "reserved": reserved, "state": state, "id": result_id,
        "events": events, "remaining_ids": remaining, "next_id": next_id,
        "old_other_retained": initially_bound and not missing_handle,
    }


if __name__ == "__main__":
    for bound, missing in [(False, False), (True, False), (False, True)]:
        print(json.dumps(run(bound, missing), sort_keys=True))
