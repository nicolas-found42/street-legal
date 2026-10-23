#!/usr/bin/env python3
"""Execute the resource-eligibility prefix of original4255e0.

The original runs until its first VM-thread setup instruction425643, or its
ordinary zero return. Resource virtual minimum/payload and5447d0 are explicit
controlled collaborators. All caller heap words and ordered collaborator-time
heaps are compared. No VM body, retained stack, CPU-register parity or complete
4255e0/stock callback acceptance is claimed by this bounded observation.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path

import unicorn
from oracle import Oracle, DEFAULT_EXE, CW_D3D9, HEAP_BASE

PROFILE = {
    'name': '4255e0-resource-prefix-v1',
    'endpoint': '425643 before VM-thread setup, or ordinary zero return',
    'comparison': 'complete caller heap, ordered resource calls and callback eligibility/live script',
    'controlled': ['resource minimum virtual', '5447d0', 'resource payload virtual'],
    'unclaimed': ['VM execution', 'retained stack', 'CPU registers', 'physical devices'],
}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def capture():
    rows = []
    cases = itertools.product([0, 0x13572468], [False, True], [1, 8],
                              ['ready', 'pending', 'no-payload', 'minimum-replaces-script',
                               'prepare-replaces-script', 'payload-clears-script'])
    for script, secondary_present, kind, mode in cases:
        o = Oracle(fpu_cw=CW_D3D9)
        envelope = o.alloc(0x100)
        secondary = o.alloc(0x100)
        payload = o.alloc(0x100)
        o.write_u32s(envelope, [0x51000000 + i * 17 for i in range(0xc0)])
        vtable, code = o.scratch(0x100), o.scratch(0x100)
        o.write_u32(vtable + 0x14, code)
        o.write_u32(vtable + 0xc, code + 16)
        o.write_u32(secondary, vtable)
        o.write_u32(secondary + 0x4c, kind)
        o.write_u32(envelope + 0x44, secondary if secondary_present else 0)
        o.write_u32(envelope + 0x50, script)
        before = o.read_u32s(HEAP_BASE, 0xc0)
        events, ready = [], False

        def record(oc, name, args):
            events.append(dict(name=name, this=oc.reg('ecx'), args=list(args),
                               heap=oc.read_u32s(HEAP_BASE, 0xc0)))

        def minimum(oc, args):
            record(oc, 'minimum', args)
            if mode == 'minimum-replaces-script':
                oc.write_u32(envelope + 0x50, 0x24681357)
            return 0

        def prepare(oc, args):
            record(oc, 'prepare', args)
            if mode == 'prepare-replaces-script':
                oc.write_u32(envelope + 0x50, 0x35792468)
            return 0x80000001 if mode == 'pending' else 0x12345678

        def data(oc, args):
            record(oc, 'payload', args)
            if mode == 'payload-clears-script':
                oc.write_u32(envelope + 0x50, 0)
            return 0 if mode == 'no-payload' else payload

        def endpoint(uc, pc, size, _):
            nonlocal ready
            if pc == 0x425643:
                ready = True
                uc.emu_stop()

        o.hook(code, minimum, 'thiscall', 1)
        o.hook(code + 16, data, 'thiscall', 1)
        o.hook(0x5447d0, prepare, 'thiscall', 3)
        o.uc.hook_add(unicorn.UC_HOOK_CODE, endpoint)
        stop = None
        try:
            result = o.call(0x4255e0, [envelope, 0, 0x60cadc, 0x4f, 0])
            if result.eax != 0 or result.esp_delta != 0 or result.fpcw != CW_D3D9:
                raise RuntimeError('unexpected original no-callback ABI/result')
        except Exception as error:
            stop = str(error)
            if not ready or stop != 'emulation stopped early at eip=0x425643':
                raise
        rows.append(dict(script=script, secondary_present=secondary_present,
                         kind=kind, mode=mode, envelope=envelope, secondary=secondary,
                         payload=payload, before=before, events=events,
                         after=o.read_u32s(HEAP_BASE, 0xc0), ready=ready,
                         target_script=o.read_u32(envelope + 0x50) if ready else None,
                         instructions=o.icount, stop=stop))
    return dict(schema=1, entry=0x4255e0, fpu_cw=CW_D3D9,
                exe_sha256=digest(DEFAULT_EXE), producer_sha256=digest(__file__),
                oracle_sha256=digest(Path(__file__).with_name('oracle.py')),
                unicorn=unicorn.__version__, profile=PROFILE,
                profile_sha256=hashlib.sha256(json.dumps(PROFILE, sort_keys=True,
                    separators=(',', ':')).encode()).hexdigest(), rows=rows)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Preserve previous attempts; choose a fresh output')
    result = capture()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, sort_keys=True) + '\n')
    print(len(result['rows']), 'original resource-prefix cases')
