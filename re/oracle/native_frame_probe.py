#!/usr/bin/env python3
"""Original426e10 per-resource drive dispatch with explicit collaborators.

The original resource reference, flag scope, branch selection and cleanup run
unchanged. Resource minimum/prepare/payload virtuals and native/script callbacks
are controlled, recorded collaborators. This does not certify their bodies or
an end-to-end physics session. Complete caller heaps and callback-time heaps
are retained, with no masks. Original-derived observations stay ignored.
"""
import hashlib
import itertools
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9, DEFAULT_EXE, HEAP_BASE


def capture():
    rows = []
    modes = ['null', 'pending', 'no-payload', 'empty', 'script',
             'native-no-template', 'template-pending', 'template-no-payload',
             'template-no-class', 'native', 'native-mutates', 'template-mutates-body']
    for kind, flags, mode in itertools.product([1, 2], [0, 0x100, 0x08000110], modes):
        o = Oracle(fpu_cw=CW_D3D9)
        resource = o.alloc(0x100)
        template = o.alloc(0x100)
        envelope = o.alloc(0x100)
        template_data = o.alloc(0x40)
        klass = o.alloc(0x40)
        reference = o.alloc(16)
        vtable = o.scratch(0x100)
        code = o.scratch(0x100)
        o.write_u32s(vtable, [0] * 64)
        for offset, address in [(0xc, code), (0x14, code + 16), (0x30, code + 32)]:
            o.write_u32(vtable + offset, address)
        for p in [resource, template, klass]:
            o.write_u32(p, vtable)
        o.write_u32(resource + 0x4c, kind)
        o.write_u32(resource + 0x50, 0xffff1234)
        o.write_u32(resource + 0x54, flags)
        o.write_u32(template + 0x4c, kind)
        o.write_u32(template + 0x50, 0xffff1235)
        o.write_u32(envelope + 0x4c, 0 if mode in ['script', 'empty'] else 0x12345678)
        o.write_u32(envelope + 0x50, 0x1004 if mode == 'script' else 0)
        o.write_u32(envelope + 0x44, 0 if mode == 'native-no-template' else template)
        o.write_u32(template_data + 0xc, 0 if mode == 'template-no-class' else klass)
        o.call(0x429060, [0 if mode == 'null' else resource], convention='thiscall', this=reference)
        before = o.read_u32s(HEAP_BASE, (o.heap_ptr - HEAP_BASE) // 4)
        events = []

        def record(name, oc, args):
            events.append(dict(name=name, this=oc.reg('ecx'), args=list(args),
                               heap=oc.read_u32s(HEAP_BASE, len(before))))

        def minimum(oc, args):
            record('minimum', oc, args)
            return 0

        def prepare(oc, args):
            record('prepare', oc, args)
            if mode == 'template-mutates-body' and oc.reg('ecx') == template:
                oc.write_u32(envelope + 0x4c, 0x87654321)
            pending = mode == 'pending' or (mode == 'template-pending' and oc.reg('ecx') == template)
            return 0x80000001 if pending else 0

        def payload(oc, args):
            record('payload', oc, args)
            if oc.reg('ecx') == resource:
                return 0 if mode == 'no-payload' else envelope
            return 0 if mode == 'template-no-payload' else template_data

        def native(oc, args):
            record('native', oc, args)
            if mode == 'native-mutates':
                oc.write_u32(resource + 0x54, 0x40)
            return 0

        def script(oc, args):
            record('script', oc, args)
            return 0

        o.hook(code, payload, 'thiscall', 1)
        o.hook(code + 16, minimum, 'thiscall', 1)
        o.hook(code + 32, native, 'thiscall', 2)
        o.hook(0x5447d0, prepare, 'thiscall', 3)
        o.hook(0x425a90, script, 'cdecl', 4)
        result = o.call(0x426e10, [0x3c888889, reference], convention='thiscall', this=0)
        rows.append(dict(kind=kind, flags=flags, mode=mode, resource=resource,
                         template=template, envelope=envelope, template_data=template_data,
                         klass=klass, reference=reference, vtable=vtable, before=before,
                         events=events, after=o.read_u32s(HEAP_BASE, len(before)),
                         fpu_cw=result.fpcw, esp_delta=result.esp_delta))
    return dict(schema=1, entry=0x426e10, fpu_cw=CW_D3D9, scope=__doc__,
                exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                rows=rows)


if __name__ == '__main__':
    print(json.dumps(capture(), sort_keys=True))
