#!/usr/bin/env python3
"""Original field initializer new+constructor dispatch, controlled metadata.

Executes 403e80/407da0, 420ff0, 408a30, 4207c0 and null-body 41fbc0.
Allocation, decoded pool/type/class identity, method lookup and cached argument
signature are supplied. Real argument collection and null-body dispatch execute.
No constructor script body, stock class loading, whole-world allocation or boot.
Redirect rows into ignored re/out/ only.
"""
import json
from vminit_vectors import World, n, STOP, pool, CW
from unicorn import UC_HOOK_CODE
from vmexec_vectors import arr


def run(static, arguments):
    w = World()
    o = w.o
    interner = o.alloc(0x30)
    o.call(0x41e4d0, [], convention='thiscall', this=interner, fpu_cw=CW)
    o.write_u32(w.s.jvm + 0x68, interner)
    nodes = [n(0x1007, 0x1e), n(0x1006, 0)]
    if arguments:
        nodes += [n(0x1007, 0x1c), n(0x100b, 17)]
    nodes += [n(0x1027, int(arguments)), n(0x21), STOP]
    fields = [('value', 'LTarget;', 0)]
    holder = w.klass('Holder', statics=fields if static else [], instance=[] if static else fields, bodies=[nodes])
    target = w.klass('Target')
    p, records = pool(o, [(0, 'Target', 6)])
    o.write_u32(holder + 0x14, p)
    o.write_u32(o.read_u32(holder + 4) + 0x24, p)
    o.hook(0x406140, lambda oc, args: o.read_u32(records[args[0]] + 8) if args[0] < len(records) else args[0], 'thiscall', 1)
    o.hook(0x411610, lambda oc, args: target, 'thiscall', 1)
    o.hook(0x412670, lambda oc, args: o.read_u32(target + 4), 'thiscall', 1)
    o.hook(0x410890, lambda oc, args: target, 'thiscall', 1)
    instance = o.alloc(32)
    o.write_u32(instance + 12, target)
    o.hook(0x404e20, lambda oc, args: instance, 'thiscall', 0)
    argument_signature = '(I)' if arguments else '()'
    method = o.alloc(28)
    o.write_u32s(method, [0, 1, w.text('<init>'), w.text(argument_signature), 0, 0, 0])
    parameter_type = o.alloc(4)
    o.write_u32(parameter_type, w.text('I'))
    signature = o.alloc(12)
    o.write_u32s(signature, [w.s.jvm, 0, arr(o, [parameter_type, 0] if arguments else [0])])
    o.write_u32(method + 24, signature)
    o.write_u32(target + 24, arr(o, [0]))
    record = o.alloc(16)
    o.write_u32s(record, [target, 0, method, 0])
    requests = []
    def resolve(oc, args):
        requests.append({'name': oc.read_cstr(args[0]).decode(), 'target_matches': oc.reg('ecx') == target, 'signature': oc.read_cstr(args[1]).decode()})
        return record
    o.hook(0x404910, resolve, 'thiscall', 2)
    o.hook(0x41def0, lambda oc, args: w.text(argument_signature), 'thiscall', 0)
    entries = []
    collected_arguments = []
    collector_counts = []
    dispatch_returns = []
    constructor_threads = []
    def observe(uc, address, size, user):
        if address in {0x408a30, 0x4207c0, 0x41fbc0}:
            entries.append(hex(address))
        if address == 0x408a30:
            thread, name = o.read_u32s(o.reg('esp') + 4, 2)
            assert o.reg('ecx') == instance
            assert o.read_u32(thread + 0x10) == w.s.jvm
            constructor_threads.append(thread)
            assert o.read_cstr(name) == b'<init>'
        elif address == 0x4207c0:
            clazz, actual_receiver, name = o.read_u32s(o.reg('esp') + 4, 3)
            assert o.reg('ecx') == constructor_threads[0]
            assert clazz == target and actual_receiver == instance
            assert o.read_cstr(name) == b'<init>'
        elif address == 0x41dcb0:
            assert o.read_u32(o.reg('esp') + 4) == constructor_threads[0]
            frame = o.read_u32(constructor_threads[0] + 0x20)
            stack = w.s._container(o.read_u32(frame + 0x28))
            marker = stack[-1]
            assert o.read_cstr(o.read_u32(marker + 4)) == b'I'
            collector_counts.append(o.read_u32(marker + 8))
        elif address == 0x41fbc0:
            assert o.reg('ecx') == constructor_threads[0]
            assert o.read_u32s(o.reg('esp') + 4, 3) == [record, target, instance]
            frame = o.read_u32(constructor_threads[0] + 0x20)
            stack = w.s._container(o.read_u32(frame + 0x28))
            # Initializer destination and new-object result remain below args.
            assert len(stack) == 2 + int(arguments)
            assert o.read_u32(stack[0] + 8) == 0
            assert o.read_u32(stack[1] + 8) == instance
            collected_arguments.append([
                {'type': o.read_cstr(o.read_u32(cell + 4)).decode(),
                 'value': o.read_u32(cell + 8)} for cell in stack[2:]
            ])
        elif address == 0x420826:
            dispatch_returns.append(o.reg('eax'))
    o.uc.hook_add(UC_HOOK_CODE, observe)
    receiver = 0 if static else o.alloc(32)
    outcome = w.call(0x403e80 if static else 0x407da0, holder if static else receiver, [] if static else [holder, 0])
    assert outcome == 'ok', outcome
    cells = w.owner(o.read_u32(holder + 0x1d0) if static else o.read_u32(receiver + 16))
    assert len(cells) == 1 and cells[0]['value'] == instance
    assert requests == [{'name': '<init>', 'target_matches': True, 'signature': argument_signature}]
    assert entries == ['0x408a30', '0x4207c0', '0x41fbc0']
    assert collector_counts == [int(arguments)]
    assert collected_arguments == [[{'type': 'I', 'value': 17}] if arguments else []]
    assert dispatch_returns == [1]
    body_entries = [e['initializer_frame'] for e in w.events if 'initializer_frame' in e]
    assert body_entries == ['Holder']
    return {'oracle': 'Unicorn original initializer/interpreter/408a30/4207c0/null-body; supplied allocation/type/pool/method lookup/cached argument signature', 'static': static, 'arguments': int(arguments), 'outcome': outcome, 'entries': entries, 'requests': requests, 'value_matches_allocated_instance': True, 'value_type': cells[0]['type'], 'body_entries': body_entries, 'collector_counts': collector_counts, 'collected_arguments': collected_arguments, 'receiver_matches_allocated_instance': True, 'dispatch_returns': dispatch_returns}


if __name__ == '__main__':
    for static in [True, False]:
        for arguments in [False, True]:
            print(json.dumps(run(static, arguments), sort_keys=True))
