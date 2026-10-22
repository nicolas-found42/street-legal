#!/usr/bin/env python3
"""Original creation wrappers' complete weak-link transitions and return lookup.

45d910,47d7b0/47d900 and429010 execute unchanged. Supplied boundaries:
receiver field lookup;53a5e0/53a2a0 constructors;419860 resource access.
Both constructor success and failure are exercised with unbound, already-bound
and differently-bound receiver handles, including neighboring weak references.
No whole factory, script body or loader acceptance is claimed.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9
from render_vectors import _cell, _frame

SIG = '(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)'


def run(native, initial, success, default_owner):
    o = Oracle(fpu_cw=CW_D3D9)
    receiver, handle, neighbor, owner, old, created, envelope, owner_resource = [o.alloc(0x100) for _ in range(8)]
    o.write_u32s(owner, [0, 0, 77, owner_resource])
    o.write_u32s(0x636460, [0, 0, 77, owner_resource])
    o.write_u32(old + 0x50, 7)
    o.write_u32(created + 0x50, 11)
    o.write_u32(envelope + 0x50, 0x1234)
    target = {'unbound': 0, 'old': old, 'created': created}[initial]
    # Make the receiver the second link, so unlink must update both neighbors
    # and the resource's head correctly; same-target creation must be a no-op.
    if target:
        o.call(0x429060, [target], convention='thiscall', this=handle)
        o.call(0x429060, [target], convention='thiscall', this=neighbor)
    o.hook(0x42ab50, lambda oc, args: handle if args[0] == receiver else 0, 'cdecl', 2)
    calls = []
    def construct(oc, args):
        calls.append(['construct', args])
        return created if success else 0
    def access(oc, args):
        calls.append(['access', oc.reg('ecx'), args])
        return envelope
    o.hook(0x53a2a0 if native else 0x53a5e0, construct, 'cdecl', 5 if native else 4)
    o.hook(0x419860, access, 'thiscall', 4)
    words = [0 if default_owner else owner, 0x456, 0x789, 0xabc]
    cells = [_cell(o, 'L', word) for word in reversed(words)] + [_cell(o, 'L', receiver)]
    signature = SIG + ('V' if native else 'Ljava.lang.GameType;')
    result = o.call(0x47d900 if native else 0x47d7b0,
                    [_frame(o, 'java.util.resource.GameRef', 'create_native' if native else 'create', signature, cells)],
                    fpu_cw=CW_D3D9).eax
    def identity(pointer):
        return {0:'null',handle:'handle',neighbor:'neighbor',old:'old',created:'created'}.get(pointer, hex(pointer))
    def link(pointer):
        prev, following, resource_id, container = o.read_u32s(pointer, 4)
        return dict(prev=identity(prev), next=identity(following), id=resource_id, container=identity(container))
    assert len(calls) == (2 if success and not native else 1)
    if success and not native:
        assert calls[1] == ['access', created, [0x80000000, 0x3f800000, 0, 0]]
    return dict(native=native, initial=initial, success=success, default_owner=default_owner,
                result=None if native else result, handle=link(handle), neighbor=link(neighbor),
                old_head=identity(o.read_u32(old+0x48)), created_head=identity(o.read_u32(created+0x48)),
                construction_owner_default=calls[0][1][0] == 0x636460,
                access_created=success and not native)


if __name__ == '__main__':
    rows = [run(native, initial, success, default_owner)
            for native in [False,True] for initial in ['unbound','old','created']
            for success in [False,True] for default_owner in [False,True]]
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         scope=__doc__, rows=rows), indent=2))
