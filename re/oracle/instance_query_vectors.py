"""Build-606 instance routing and registration over controlled resource graphs.

Runs original426470 and427210; resource virtual calls,419860,425a90 and
native vtable+3c are declared callbacks. Registration allocation, constructors,
intrusive lists and resource weak links execute in the original. No script
body, loader, native class handler or physical device is claimed by these rows.
"""
import hashlib
import itertools
from pathlib import Path

from oracle import Oracle, DEFAULT_EXE, CW_D3D9


def inputs():
    rows = [dict(kind=kind, body=body, script=script, secondary=secondary,
                 bound=bound, resource=True, refused=refused, data=True,
                 secondary_data=True, mutation=0, registration=False)
            for kind, body, script, secondary, bound, refused in itertools.product(
                [0, 1, 2, 8, 21], [False, True], [False, True],
                [False, True], [False, True], [0, 1, 0x80000000])]
    base = dict(kind=1, body=False, script=True, secondary=True, bound=True,
                resource=True, refused=0, data=True, secondary_data=True,
                mutation=0, registration=False)
    for overrides in [dict(resource=False), dict(data=False),
                      dict(body=True, secondary_data=False),
                      dict(kind=8, data=False)]:
        rows.append(dict(base, **overrides))
    # A script can clear its script slot/reference or install a native body.
    # A secondary load can change the body after the router's first test.
    for mutation in range(6):
        for body in [False, True]:
            for mask in range(8):
                rows.append(dict(base, mutation=mutation, body=body,
                                 registration=True, answers=mask))
    return rows


def run(case, exe=DEFAULT_EXE, cw=CW_D3D9):
    spec = inputs()[case % len(inputs())]
    o = Oracle(exe=exe, fpu_cw=cw)
    o.hook_alloc()
    ref, resource, envelope, vt, secondary, secondary_data, payload, pvt = [
        o.alloc(0x100) for _ in range(8)]
    context = o.alloc(0xd0)
    for offset in [0, 0x1c, 0x38]:
        o.call(0x428fd0, [], convention='thiscall', this=context + offset)
    o.write_u32s(ref, [0, 0, 123 if spec['bound'] else 0,
                       resource if spec['resource'] else 0])
    o.write_u32(resource, vt)
    o.write_u32(resource + 0x4c, spec['kind'])
    o.write_u32(resource + 0x50, 123)
    o.write_u32(vt + 12, 0x300f0000)
    o.write_u32(vt + 20, 0x300f0010)
    o.write_u32(envelope + 12, payload)
    o.write_u32(envelope + 0x44, secondary if spec['secondary'] else 0)
    o.write_u32(envelope + 0x4c, 0x200000fc if spec['body'] else 0)
    o.write_u32(envelope + 0x50, 73 if spec['script'] else 0)
    o.write_u32(secondary_data + 12, payload)
    o.write_u32(payload, pvt)
    o.write_u32(pvt + 0x3c, 0x300f0020)
    events, answers = [], []

    def event(name, oc, args):
        events.append(dict(name=name, this=oc.reg('ecx'), args=args))

    def data(oc, args):
        event('data', oc, args)
        return envelope if spec['data'] else 0

    def minimum(oc, args):
        event('minimum', oc, args)
        return 0

    def prepare(oc, args):
        event('prepare', oc, args)
        return spec['refused']

    def access(oc, args):
        event('access', oc, args)
        if spec['mutation'] == 4:
            oc.write_u32(envelope + 0x4c, 0x200000f8)
        elif spec['mutation'] == 5:
            oc.write_u32(envelope + 0x4c, 0)
        return secondary_data if spec['secondary_data'] else 0

    def answer(message):
        if spec['registration']:
            index = [8, 9, 28].index(message)
            value = (spec['answers'] >> index) & 1
        else:
            value = message ^ 0x12345678
        answers.append(value)
        return value

    def native(oc, args):
        event('native', oc, args)
        return answer(args[1])

    def script(oc, args):
        #426470's actual varargs are envelope,getInfo,1,2,message,0x4f.
        assert oc.read_cstr(args[1]) == b'getInfo'
        # ECX is a scratch register on this cdecl call; don't claim it as input.
        events.append(dict(name='script', args=args))
        if spec['mutation'] == 1:
            oc.write_u32(envelope + 0x4c, 0x200000fc)
        elif spec['mutation'] == 2:
            oc.write_u32(envelope + 0x50, 0)
        elif spec['mutation'] == 3:
            oc.write_u32(ref + 8, 0)
        return answer(args[4])

    for address, callback, convention, count in [
        (0x300f0000, data, 'thiscall', 1),
        (0x300f0010, minimum, 'thiscall', 1),
        (0x5447d0, prepare, 'thiscall', 3),
        (0x419860, access, 'thiscall', 4),
        (0x300f0020, native, 'thiscall', 3),
        (0x425a90, script, 'cdecl', 6),
    ]:
        o.hook(address, callback, convention, count)
    heap_start, heap_before = ref, o.heap_ptr
    before = o.read_u32s(heap_start, (heap_before - heap_start + 3) // 4)
    if spec['registration']:
        o.call(0x427210, [ref], convention='thiscall', this=context, fpu_cw=cw)
        result = None
    else:
        result = o.call(0x426470, [ref, 8, 37], convention='thiscall',
                        this=0x636338, fpu_cw=cw).eax
    return dict(spec=spec, exe_sha256=hashlib.sha256(Path(exe).read_bytes()).hexdigest(),
                reference=ref, resource=resource, envelope=envelope,
                secondary=secondary, secondary_data=secondary_data,
                payload=payload, context=context, heap_start=heap_start,
                heap_before=heap_before, heap_after=o.heap_ptr,
                before=before, after=o.read_u32s(heap_start, (o.heap_ptr - heap_start + 3)//4),
                events=events, answers=answers, result=result)
