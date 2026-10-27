#!/usr/bin/env python3
"""Original renderer constructor and viewport registration/activation lifecycle.

Original4fb8c0/4fce00/515f60/4fd020/4fd280/4fd050 execute unhooked.
Only the normal allocator and display-device dimensions are supplied. The
registration uses explicit original viewport-resource payloads, constructed by
original53d990. Every caller heap word and renderer word is compared after
every operation, including dirty allocation storage and inactive lists. This
is not a stock-scene, complete renderer or cursor-animation acceptance claim.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE, HEAP_BASE
ROOT = Path(__file__).resolve().parents[2]


def run(fill, dimensions, flags, rectangle=None):
    rectangle = rectangle or [0x3e800000, 0xbf000000, 0x3f400000, 0x3fc00000]
    o = Oracle(fpu_cw=CW_D3D9)
    o.hook_alloc()
    renderer = 0x64a4d8
    o.write(renderer, bytes([fill])*0x240)
    o.write_u32(0x6187b0, renderer)
    device = o.alloc(4)
    vt = o.alloc(104)
    o.write_u32(device, vt)
    o.write_u32(vt+100, 0x300f0000)
    o.write_u32(renderer+0x204, device)
    events = []

    def dimensions_call(oc, args):
        assert oc.reg('ecx') == device
        oc.write_u32(args[0], dimensions[0])
        oc.write_u32(args[1], dimensions[1])
        events.append(['dimensions', *dimensions])
        return 0

    def allocate(oc, args):
        pointer = oc.alloc(args[0])
        oc.write(pointer, bytes([fill])*args[0])
        events.append(['allocate', args[0], pointer])
        return pointer

    o.hook(0x300f0000, dimensions_call, 'thiscall', 2)
    refs, payloads = [], []
    for index, value in enumerate(flags):
        raw, desc, ref = o.alloc(0xcc), o.alloc(24), o.alloc(16)
        o.write_u32s(desc, [18, 0xffff0060+index, 0, 0, 0])
        o.call(0x53d990, [desc], convention='thiscall', this=raw, fpu_cw=CW_D3D9)
        o.write_u32s(raw+0x8c, [value, *rectangle])
        o.call(0x429060, [raw], convention='thiscall', this=ref, fpu_cw=CW_D3D9)
        refs.append(ref)
        payloads.append(raw+0x80)
    o.hook(0x54f560, allocate, 'cdecl', 1)

    def state():
        return dict(heap_ptr=o.heap_ptr,
                    heap=o.read_u32s(HEAP_BASE, (o.heap_ptr-HEAP_BASE+3)//4),
                    renderer=o.read_u32s(renderer, 0x240//4))

    before = state()
    steps = []
    actions = [('construct', -1, 0)]
    actions += [('register', i, 0) for i in range(3)]
    actions += [('activate', i, flag) for i, flag in [(0,0xffffffff),(1,0xffffffff),(2,0xffffffff),(1,5)]]
    actions += [('deactivate', i, 0) for i in [1,1,0,2]]
    actions += [('activate', i, flag) for i, flag in [(2,0x80000000),(0,0),(1,0xffffffff)]]
    for action, index, flag in actions:
        events.clear()
        function, args = {
            'construct': (0x4fb8c0, []),
            'register': (0x4fce00, [refs[index], payloads[index]]),
            'activate': (0x4fd020, [refs[index], flag, 0xffffffff]),
            'deactivate': (0x4fd050, [refs[index]]),
        }[action]
        result = o.call(function, args, convention='thiscall', this=renderer, fpu_cw=CW_D3D9)
        steps.append(dict(action=action, index=index, flag=flag, result=result.eax,
                          events=list(events), state=state()))
    return dict(fill=fill, dimensions=dimensions, flags=flags, rectangle=rectangle, reference=refs,
                payload=payloads, renderer=renderer, before=before, steps=steps)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, default=ROOT/'re/out/boot/native-renderer-original.json')
    a = p.parse_args()
    sources = [Path(__file__), Path(__file__).with_name('oracle.py')]
    result = dict(schema=1, scope=__doc__, exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                  sources={str(f.relative_to(ROOT)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sources},
                  rows=[run(fill, dimensions, flags) for fill,dimensions,flags in itertools.product(
                      [0,0xa5], [(800,600),(960,540),(0xffffffff,0x80000001)],
                      [(127,20,127),(0xffffffff,0,0x80000000)])]+[run(fill,(800,0x01000001),(127,20,127),[0,0,0x3f800000,0x3f800001]) for fill in [0,0xa5]])
    a.output.parent.mkdir(parents=True, exist_ok=True)
    assert not a.output.is_symlink(), 'do not overwrite a shared fixture link'
    a.output.write_text(json.dumps(result, separators=(',',':'))+'\n')
    print('original renderer cases',len(result['rows']),'operations',sum(len(r['steps']) for r in result['rows']))

if __name__ == '__main__': main()
