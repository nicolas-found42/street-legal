#!/usr/bin/env python3
"""Run stock426c90/426fd0 complete control/animation registration passes.

Original traversal, access419860, weak references, flag scopes and cleanup run
unchanged. Only minimum/prepare/payload/native/script callbacks and allocator
free are controlled collaborators. Complete allocated caller heaps are retained
before, after and at every callback, without exclusions. Every callback retains
its actual ESP, return PC, ECX and ABI arguments. These bounded cases establish
dispatch semantics, not callback bodies, device behavior or whole-game parity.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, HEAP_BASE

ROOT = Path(__file__).resolve().parents[2]
EXE = ROOT / 'stock/StreetLegal_Redline.bak.exe'
MODES = ['empty-list', 'retired', 'unbound', 'pending', 'no-payload', 'empty',
         'script', 'native-no-template', 'template-pending', 'template-no-payload',
         'template-no-class', 'native', 'template-mutates-body', 'native-flags',
         'native-destroy-current', 'native-retire-next', 'native-rebind-local',
         'native-retire-self']


def bind(o, link, target):
    """Declared collaborator mutation, not a replacement for original callers."""
    old = o.read_u32(link + 12)
    if old == target:
        return
    if old:
        previous, following = o.read_u32s(link, 2)
        o.write_u32(previous + 4 if previous else old + 0x48, following)
        if following:
            o.write_u32(following, previous)
    o.write_u32s(link, [0] * 4)
    if target:
        following = o.read_u32(target + 0x48)
        if following:
            o.write_u32(following, link)
        o.write_u32(link + 4, following)
        o.write_u32(target + 0x48, link)
        o.write_u32(link + 8, o.read_u32(target + 0x50))
        o.write_u32(link + 12, target)


def capture_row(kind, flags, resource_kind, template_kind, mode, dt):
    o = Oracle(str(EXE), fpu_cw=CW_D3D9)
    o.hook_alloc()
    context = o.alloc(0x200)
    resources = [o.alloc(0x100) for _ in range(3)]
    template = o.alloc(0x100)
    envelope = o.alloc(0x100)
    template_data = o.alloc(0x40)
    klass = o.alloc(0x40)
    references = [o.alloc(16) for _ in range(3)]
    for offset, count in [(0, 0xc4), (0x1c, 0xc8), (0x38, 0xcc)]:
        o.call(0x428fd0, convention='thiscall', this=context + offset)
        o.write_u32(context + count, 0)
    vtable = o.scratch(0x100)
    code = o.scratch(0x100)
    o.write_u32s(vtable, [0] * 64)
    for at, target in [(0xc, code), (0x14, code + 16), (0x2c, code + 32), (0x34, code + 32)]:
        o.write_u32(vtable + at, target)
    o.write_u32(klass, vtable)
    for i, raw in enumerate(resources + [template]):
        o.write_u32(raw, vtable)
        o.write_u32(raw + 0x4c, resource_kind if i < 3 else template_kind)
        o.write_u32(raw + 0x50, 0xffff1000 + i)
        o.write_u32(raw + 0x54, flags)
    o.write_u32(envelope + 0x4c, 0 if mode in ['script', 'empty'] else 0x12345678)
    o.write_u32(envelope + 0x50, 0x1004 if mode == 'script' else 0)
    o.write_u32(envelope + 0x44, 0 if mode == 'native-no-template' else template)
    o.write_u32(template_data + 12, 0 if mode == 'template-no-class' else klass)
    for reference, raw in zip(references, resources):
        o.call(0x429060, [raw], convention='thiscall', this=reference)
    if mode != 'empty-list':
        # Three nodes expose cross-iteration weak-reference retention, retired
        # next-node handling, and snapshot-next semantics after callbacks.
        for reference in references:
            o.call(0x427370, [reference, kind], convention='thiscall', this=context)
    list_offset = 0 if kind == 8 else 0x38
    nodes = []
    node = o.read_u32(context + list_offset + 8)
    while o.read_u32(node + 4):
        nodes.append(node)
        node = o.read_u32(node + 4)
    if nodes and mode == 'retired':
        for node in nodes:
            o.call(0x4274e0, [references[nodes.index(node)], kind], convention='thiscall', this=context)
    if nodes and mode == 'unbound':
        # A stale nonzero ID with a null resource is a separate branch from
        # retired ID0. Both are possible weak-reference states.
        bind(o, nodes[0] + 12, 0)
        o.write_u32(nodes[0] + 20, 0xffff1000)
    words = (o.heap_ptr - HEAP_BASE + 3) // 4
    before = o.read_u32s(HEAP_BASE, words)
    events, frees = [], []
    mutated = False

    def record(name, oc, args):
        esp = oc.reg('esp')
        events.append(dict(name=name, this=oc.reg('ecx'), args=list(args),
                           esp=esp, return_pc=oc.read_u32(esp),
                           registers={r: oc.reg(r) for r in ['eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp']},
                           heap=oc.read_u32s(HEAP_BASE, words)))

    def minimum(oc, args):
        record('minimum', oc, args)
        return 0

    def prepare(oc, args):
        record('prepare', oc, args)
        if mode == 'template-mutates-body' and oc.reg('ecx') == template:
            oc.write_u32(envelope + 0x4c, 0x87654321)
        return 0x80000001 if mode == 'pending' or (mode == 'template-pending' and oc.reg('ecx') == template) else 0

    def payload(oc, args):
        record('payload', oc, args)
        if oc.reg('ecx') in resources:
            return 0 if mode == 'no-payload' else envelope
        return 0 if mode == 'template-no-payload' else template_data

    def native(oc, args):
        nonlocal mutated
        record('native', oc, args)
        if mode == 'native-flags':
            for raw in resources:
                oc.write_u32(raw + 0x54, 0x40)
        if mutated:
            return 0
        mutated = True
        if mode == 'native-destroy-current':
            link = oc.read_u32(resources[0] + 0x48)
            while link:
                following = oc.read_u32(link + 4)
                oc.write_u32s(link, [0] * 4)
                link = following
            oc.write_u32(resources[0] + 0x48, 0)
        elif mode == 'native-retire-next':
            bind(oc, nodes[1] + 12, 0)
        elif mode == 'native-retire-self':
            bind(oc, nodes[0] + 12, 0)
        elif mode == 'native-rebind-local':
            bind(oc, oc.read_u32(resources[0] + 0x48), resources[2])
        return 0

    def script(oc, args):
        record('script', oc, args)
        return 0

    def free(oc, args):
        frees.append(args[0])
        return 0

    o.hook(code, payload, 'thiscall', 1)
    o.hook(code + 16, minimum, 'thiscall', 1)
    o.hook(code + 32, native, 'thiscall', 2)
    o.hook(0x5447d0, prepare, 'thiscall', 3)
    o.hook(0x425a90, script, 'cdecl', 7 if kind == 8 else 4)
    o.hook(0x54f5b0, free, 'cdecl', 1)
    result = o.call(0x426c90 if kind == 8 else 0x426fd0, [dt], convention='thiscall', this=context)
    return dict(kind=kind, flags=flags, resource_kind=resource_kind, template_kind=template_kind,
                mode=mode, dt=dt, context=context, resources=resources, nodes=nodes,
                template=template, envelope=envelope, template_data=template_data, klass=klass,
                references=references, vtable=vtable, before=before, events=events, frees=frees,
                after=o.read_u32s(HEAP_BASE, words), fpu_cw=result.fpcw,
                fpu_depth=result.fpu_top_depth, esp_delta=result.esp_delta,
                nonvolatile={r: o.reg(r) for r in ['ebx', 'esi', 'edi', 'ebp']})


def capture():
    rows = []
    for index, params in enumerate(itertools.product([8, 0x1c], [0, 0x100, 0x08000110], [1, 2], [1, 2], MODES)):
        dt = [0, 0x3c888889, 0xbf800000, 0x80000000][index % 4]
        rows.append(capture_row(*params, dt))
    return dict(schema=1, entries=[0x426c90, 0x426fd0], fpu_cw=CW_D3D9, scope=__doc__,
                exe_sha256=hashlib.sha256(EXE.read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                rows=rows)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    # Exclusive creation preserves old and failed attempts.
    with args.out.open('x') as stream:
        json.dump(capture(), stream, sort_keys=True)
        stream.write('\n')
