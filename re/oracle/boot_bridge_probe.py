#!/usr/bin/env python3
"""Controlled GameInit creation bridge probe, build 606 (no whole-game claim).

Runs original FUN_0053a5e0 and FUN_00408a70 in Unicorn. Resource resolution,
node data, allocation, script dispatch and interpreter are explicit recording
boundaries. No class file, script body or renderer is executed by this probe.
Output is derived evidence: redirect into ignored re/out/ only.
"""

import sys, json, hashlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle, CW_D3D9


def run(script, dispatch_result=0, real_link=False):
    exe = (
        Path(__file__).resolve().parents[2]
        / "reference/raw/StreetLegal_Redline.bak.exe"
    )
    assert (
        hashlib.sha256(exe.read_bytes()).hexdigest()
        == "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    )
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    events = []

    def mem(n):
        return o.alloc(n)

    root = mem(32)
    request = mem(32)
    node = mem(256)
    data = mem(256)
    gtype = mem(64)
    vtable = mem(32)
    instance = mem(32)
    klass = mem(512)
    thread = mem(64)
    o.write_u32(root + 8, 1)
    o.write_u32(request + 8, 456)
    reqnode = mem(128)
    o.write_u32(request + 12, reqnode)
    o.write_u32(reqnode + 0x4C, 8)
    o.write_u32(node, vtable)
    o.write_u32(vtable + 12, 0x300F0000)
    o.write_u32(gtype + 0x10, klass if script else 0)
    o.write_u32(data + 0x44, reqnode)
    o.write_u32(instance + 12, klass)
    o.write_u32(0x63641C, 0x1234)
    holder = mem(4)
    o.write_u32(holder, 0x55)
    o.write_u32(0x62E00C, holder)
    if real_link:
        owner = mem(32)
        container = mem(12)
        elements = mem(4)
        ptr_cell = mem(12)
        int_type = mem(2)
        o.write(int_type, b"I\0")
        o.write_u32(ptr_cell + 4, int_type)
        o.write_u32(elements, ptr_cell)
        o.write_u32s(container, [1, elements, 1])
        o.write_u32(owner + 12, container)
        o.write_u32(instance + 16, owner)
        o.write_u32(0x62E008, 0)
        o.write_u32(data + 0x44, 0)
        o.write_u32(node + 0x50, 123)
        o.write_u32(reqnode + 0x50, 456)

    def hook(addr, n, ret=0, conv="cdecl", name=None):
        def f(oc, a):
            events.append({"call": name or hex(addr), "this": oc.reg("ecx"), "args": a})
            return ret(oc, a) if callable(ret) else ret

        o.hook(addr, f, conv, n)

    hook(0x551330, 2, 0)
    hook(0x536ED0, 0, 123)
    hook(0x551120, 3, 0)
    hook(0x536890, 5, node, "thiscall")
    hook(0x544B90, 1, 0, "thiscall")
    hook(0x300F0000, 1, data, "thiscall", "node.data")
    if not real_link:
        hook(0x429060, 1, 0, "thiscall")
    hook(0x419860, 4, gtype, "thiscall")
    hook(0x41F340, 4, thread, "thiscall")
    hook(0x404E20, 0, instance, "thiscall")
    hook(0x423C00, 0, lambda oc, a: mem(12))

    def resize(oc, a):
        p = oc.reg("ecx")
        oc.write_u32(p, a[0])
        oc.write_u32(p + 4, mem(max(4, a[0] * 4)))
        return 0

    hook(0x405180, 1, resize, "thiscall")

    def arguments(oc, a):
        container = a[0]
        assert oc.read_u32(container + 8) == 1
        cells = oc.read_u32(container + 4)
        cell = oc.read_u32(cells)
        assert oc.read_u32(cell + 8) == 123
        return 0

    hook(0x41F9F0, 1, arguments, "thiscall")
    if not real_link:
        hook(0x42A9E0, 3)

    constructor_link_state = None

    def dispatch(oc, args):
        nonlocal constructor_link_state
        native_ptr = oc.read_u32(ptr_cell + 8)
        prev, next_link, created, owner_node = oc.read_u32s(native_ptr, 4)
        assert prev == 0 and created == 123 and owner_node == node
        assert next_link != 0 and oc.read_u32(next_link) == native_ptr
        assert oc.read_u32(node + 0x48) == native_ptr
        constructor_link_state = {
            "id": 123,
            "owner": "native-node",
            "prev": 0,
            "temporary_successor": True,
            "owner_head_matches": True,
        }
        assert oc.read_u32(data + 0x50) == instance
        return dispatch_result

    hook(0x4207C0, 3, dispatch if real_link else dispatch_result, "thiscall")
    hook(0x427210, 1, 0, "thiscall")
    hook(0x4988A0, 0, 0, "thiscall")
    hook(0x498B10, 0, 0, "thiscall")
    hook(0x48AE40, 0)
    hook(0x420FF0, 1, 0.0, "thiscall")
    args = [root, request, 0, 0]
    try:
        r = o.call(0x53A5E0, args, limit=100000, fpu_cw=CW_D3D9)
    except Exception:
        print(events, file=sys.stderr)
        raise
    native_observations = []
    if script and real_link:
        from io_vectors import _call_frame, _cell

        native_ptr = o.read_u32(ptr_cell + 8)
        expected_link = o.read_u32s(native_ptr, 4)
        for address, name, signature, parameters in [
            (0x47CEA0, "newNative", "()V", []),
            (0x47CF80, "set", "(I)V", [_cell(o, "I", 123)]),
            (0x47D290, "id", "()I", []),
        ]:
            frame = _call_frame(
                o,
                "java.util.resource.ResourceRef",
                name,
                signature,
                parameters + [_cell(o, "L", instance)],
            )
            before_heap = o.heap_ptr
            result = o.call(address, [frame], fpu_cw=CW_D3D9)
            assert o.read_u32(ptr_cell + 8) == native_ptr
            assert o.read_u32s(native_ptr, 4) == expected_link
            assert o.heap_ptr == before_heap
            if name == "id":
                assert result.eax == 123
            native_observations.append(
                {
                    "name": name,
                    "signature": signature,
                    "no_allocation": True,
                    "same_link": True,
                    "id": result.eax if name == "id" else None,
                }
            )
    labels = {
        root: "root",
        request: "request",
        node: "native-node",
        data: "native-data",
        gtype: "gametype-data",
        klass: "script-class",
        instance: "script-instance",
        thread: "thread",
        reqnode: "gametype-node",
    }

    def label(x):
        return labels.get(x, x)

    for e in events:
        e["this"] = label(e["this"])
        e["args"] = [label(x) for x in e["args"]]
        if e["call"] == "0x41f340":
            e["args"][3] = o.read_cstr(e["args"][3]).decode()
        if e["call"] == "0x4207c0":
            e["args"][2] = o.read_cstr(e["args"][2]).decode()
    link_state = None
    if script and real_link:
        native_ptr = o.read_u32(ptr_cell + 8)
        assert native_ptr != 0
        assert o.read_u32s(native_ptr, 4) == [0, 0, 123, node]
        assert o.read_u32(node + 0x48) == native_ptr
        assert o.read_u32(data + 0x50) == instance
        link_state = {
            "id": 123,
            "owner": "native-node",
            "prev": 0,
            "next": 0,
            "owner_head_matches": True,
            "script_backref_matches": True,
        }
    calls = [e["call"] for e in events]
    if script:
        expected = ["0x41f340", "0x404e20", "0x41f9f0", "0x4207c0"]
        if not real_link:
            expected.insert(3, "0x42a9e0")
        if dispatch_result == 0:
            expected.append("0x420ff0")
        assert [x for x in calls if x in expected or x == "0x420ff0"] == expected
        assert next(e for e in events if e["call"] == "0x41f340")["args"] == [
            0x1234,
            10,
            0,
            "THRD-CREATE",
        ]
        assert next(e for e in events if e["call"] == "0x4207c0")["args"] == [
            "script-class",
            "script-instance",
            "<init>",
        ]
        if dispatch_result == 0:
            assert next(e for e in events if e["call"] == "0x420ff0")["args"] == [0]
    else:
        assert not any(
            x in calls for x in ["0x41f340", "0x404e20", "0x4207c0", "0x420ff0"]
        )
    assert o.reg("eip") == 0x7FFF0000
    return {
        "script": script,
        "dispatch_result": dispatch_result,
        **(
            {"constructor_link_state": constructor_link_state, "link_state": link_state, "native_observations": native_observations}
            if real_link
            else {}
        ),
        "events": events,
        "stop": hex(o.reg("eip")),
        "oracle": "Unicorn original FUN_0053a5e0; synthetic resource/node/heap boundaries; original FUN_00408a70; Thread::run recording boundary",
        "instructions": r.icount,
    }


if __name__ == "__main__":
    for script, result in [(False, 0), (True, 0), (True, 1)]:
        print(json.dumps(run(script, result), sort_keys=True))
