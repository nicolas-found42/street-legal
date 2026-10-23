#!/usr/bin/env python3
"""Original4265c0 routing with controlled resource, native and script collaborators.

Executes the complete routing function and its real429060 weak-reference logic.
Callbacks supply runtime collaborators and explicit mutations, not port answers.
The destructor collaborator unlinks the notification and releases its weak ref;
its storage is retained so the complete memory image remains comparable.
"""
import hashlib
import itertools
import json
from pathlib import Path

from oracle import CW_D3D9, DEFAULT_EXE, HEAP_BASE, Oracle
from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X


def run(case):
    o = Oracle(fpu_cw=CW_D3D9)
    calls = []
    names = {}
    def alloc(name, size=0x100):
        value = o.alloc(size)
        names[name] = value
        return value
    root = alloc('root', 16)
    source = alloc('source', 16)
    vtable = alloc('resource_vtable')
    payload_vtable = alloc('payload_vtable')
    o.write_u32(vtable + 0xc, 0x300f0000)
    o.write_u32(vtable + 0x14, 0x300f0010)
    o.write_u32(payload_vtable + 0x38, 0x300f0020)
    objects = {}
    for i in range(4):
        resource, envelope, secondary, data, payload, sentinel = [
            alloc(f'{name}{i}') for name in
            ['resource', 'envelope', 'secondary', 'data', 'payload', 'sentinel']]
        o.write_u32(resource, vtable)
        o.write_u32(resource + 0x4c, case.get('kind', 1) if i == 0 else 1)
        o.write_u32(resource + 0x50, i + 1)
        o.write_u32(secondary, vtable)
        o.write_u32(secondary + 0x4c, case.get('secondary_kind', 1) if i == 0 else 1)
        o.write_u32(envelope + 0x44, secondary if case.get('secondary', True) or i else 0)
        o.write_u32(envelope + 0x4c, 0x100 + i if case.get('body', True) or i else 0)
        o.write_u32(envelope + 0x50, 0x200 + i if case.get('script', True) or i else 0)
        o.write_u32(envelope + 0x70, case.get('mask', 0x10) if i == 0 else 0xffffffff)
        o.write_u32(envelope + 0x5c, sentinel)
        o.write_u32(data + 0xc, payload)
        o.write_u32(payload, payload_vtable)
        objects[resource] = (i, envelope)
        objects[secondary] = (i, data)
    def bind(reference, resource):
        o.call(0x429060, [resource], convention='thiscall', this=reference)
    if case.get('bound', True):
        bind(root, names['resource0'])
    bind(source, names['resource3'])
    node_vtable = alloc('node_vtable')
    o.write_u32(node_vtable, 0x300f0040)
    nodes = []
    for i, spec in enumerate(case.get('notifications', [])):
        node = alloc(f'node{i}', 0x2c)
        nodes.append(node)
        o.write_u32(node, node_vtable)
        bind(node + 0xc, names[f'resource{spec.get("listener", i + 1)}'])
        o.write_u32(node + 0x1c, spec.get('mask', 0x10))
        o.write_u32(node + 0x20, spec.get('kind', 0))
        o.write_u32(node + 0x24, spec.get('argument', 0))
        o.write_u32(node + 0x28, spec.get('name', 0))
    # A real two-sentinel list: the first sentinel is not itself in traversal.
    head = names['envelope0'] + 0x58
    names['head'] = head
    tail = names['sentinel0']
    chain = [head] + nodes + [tail]
    for a, b in zip(chain, chain[1:]):
        o.write_u32(a + 4, b)
        o.write_u32(b + 8, a)
    o.write_u32(names['envelope0'] + 0x5c, nodes[0] if nodes else tail)
    # Empty sentinel +4 is zero, marking end. Other envelopes are empty.
    def mutation(kind, index):
        if case.get('mutate_on') != [kind, index]:
            return
        change = case['mutation']
        if change == 'clear_script':
            o.write_u32(names[f'envelope{index}'] + 0x50, 0)
        elif change == 'set_script':
            o.write_u32(names[f'envelope{index}'] + 0x50, 0xabc)
        elif change == 'clear_body':
            o.write_u32(names[f'envelope{index}'] + 0x4c, 0)
        elif change == 'clear_next_listener':
            o.write_u32(nodes[1] + 0x14, 0)
        elif change == 'rewrite_next_kind':
            o.write_u32(nodes[1] + 0x20, 0x10000080)
            o.write_u32(nodes[1] + 0x24, 0x123456)
        elif change == 'replace_list':
            o.write_u32(names['envelope0'] + 0x5c, nodes[1])
        elif change == 'sever_current_next':
            o.write_u32(nodes[0] + 4, tail)
        elif change == 'clear_one_shot':
            o.write_u32(nodes[0] + 0x1c, 0x10)
        elif change == 'set_one_shot':
            o.write_u32(nodes[0] + 0x1c, 0x80000010)
        else:
            raise ValueError(change)
    def prepare(oc, args):
        resource = oc.uc.reg_read(X.UC_X86_REG_ECX)
        index = objects[resource][0]
        calls.append(dict(kind='prepare', this=resource, args=args))
        mutation('prepare', index)
        return case.get('flags', 0) if resource == names['resource0'] else case.get('secondary_flags', 0)
    def data(oc, args):
        resource = oc.uc.reg_read(X.UC_X86_REG_ECX)
        index, value = objects[resource]
        calls.append(dict(kind='data', this=resource, args=args))
        mutation('data', index)
        if resource == names[f'secondary{index}']:
            mutation('secondary_data', index)
        return 0 if case.get('null_data') == names_reverse[resource] else value
    def minimum(oc, args):
        resource = oc.uc.reg_read(X.UC_X86_REG_ECX)
        calls.append(dict(kind='minimum', this=resource, args=args))
        return 0
    def native(oc, args):
        payload = oc.uc.reg_read(X.UC_X86_REG_ECX)
        index = next(i for i in range(4) if names[f'payload{i}'] == payload)
        calls.append(dict(kind='native', this=payload, args=args))
        mutation('native', index)
        return case.get('consumed', 0) if index == 0 else 0
    def script(oc, args):
        envelope = args[4]
        index = next(i for i in range(4) if names[f'envelope{i}'] == envelope)
        calls.append(dict(kind='script', this=0, args=args))
        mutation('script', index)
        return 0
    def destructor(oc, args):
        node = oc.uc.reg_read(X.UC_X86_REG_ECX)
        calls.append(dict(kind='destroy', this=node, args=args))
        nxt, prev = oc.read_u32(node + 4), oc.read_u32(node + 8)
        if nxt and prev:
            oc.write_u32(nxt + 8, prev)
            oc.write_u32(prev + 4, nxt)
        oc.write_u32(node + 4, 0)
        oc.write_u32(node + 8, 0)
        # Inline429060(reference,0), without nesting an Oracle.call in a hook.
        ref = node + 0xc
        old = oc.read_u32(ref + 12)
        if old:
            first, second = oc.read_u32(ref), oc.read_u32(ref + 4)
            oc.write_u32(first + 4 if first else old + 0x48, second)
            if second:
                oc.write_u32(second, first)
            oc.write(ref, bytes(16))
        return node
    names_reverse = {value: name for name, value in names.items()}
    for address, fn, convention, nargs in [
        (0x5447d0, prepare, 'thiscall', 3),
        (0x300f0000, data, 'thiscall', 1),
        (0x300f0010, minimum, 'thiscall', 1),
        (0x300f0020, native, 'thiscall', 4),
        (0x425c60, script, 'cdecl', 6),
        (0x300f0040, destructor, 'thiscall', 1),
    ]:
        o.hook(address, fn, convention, nargs)
    scratch = []
    def observe_binding(uc, address, size, user_data):
        if address == 0x4265f0:
            reference = uc.reg_read(X.UC_X86_REG_ECX)
            if reference >= 0x7fe00000 and reference not in scratch:
                scratch.append(reference)
    o.uc.hook_add(UC_HOOK_CODE, observe_binding, begin=0x4265f0, end=0x4265f0)
    size = (o.heap_ptr - HEAP_BASE) // 4
    before = o.read_u32s(HEAP_BASE, size)
    result = o.call(0x4265c0,
                    [root, source if case.get('source') else 0,
                     case.get('event', 0x10), case.get('argument', 0x87654321),
                     case.get('name', 0)], convention='thiscall', this=0x636338)
    normalize = {address: 0x300e0000 + i * 16 for i, address in enumerate(scratch)}
    for call in calls:
        call['this'] = normalize.get(call['this'], call['this'])
        call['args'] = [normalize.get(value, value) for value in call['args']]
    return dict(case=case, memory_base=HEAP_BASE, names=names, before=before,
                after=o.read_u32s(HEAP_BASE, size), calls=calls, eax=result.eax,
                scratch_references=list(normalize.values()))


def cases():
    for kind, body, script, consumed in itertools.product([1, 2, 8], [False, True], [False, True], [0, 1]):
        yield dict(kind=kind, body=body, script=script, consumed=consumed)
    for override in [dict(bound=False), dict(mask=0), dict(flags=0x80000000),
                     dict(secondary=False), dict(secondary_kind=8),
                     dict(secondary_flags=0x80000000), dict(null_data='resource0'),
                     dict(null_data='secondary0'), dict(event=0x90000010, consumed=0),
                     dict(event=0x90000010, consumed=1)]:
        yield override
    for source, mask, kind in itertools.product([False, True],
            [0x10, 0x20, 0x80000010, 0x40000010, 0xc0000010],
            [0, 0x80, 0x10000000, 0x10000080]):
        yield dict(source=source, notifications=[dict(mask=mask, kind=kind,
                   argument=0x11223344, name=0x55667788)])
    for on, mutation in [
        (['native', 0], 'clear_script'), (['native', 0], 'set_script'),
        (['secondary_data', 0], 'clear_body'), (['native', 0], 'replace_list'),
        (['script', 0], 'replace_list'), (['native', 1], 'sever_current_next'),
        (['native', 1], 'clear_next_listener'), (['native', 1], 'rewrite_next_kind'),
        (['native', 1], 'clear_one_shot'), (['native', 1], 'set_one_shot')]:
        yield dict(mutate_on=on, mutation=mutation,
                   script=mutation != 'set_script',
                   notifications=[dict(mask=0x80000010), dict(mask=0x10)])
    for guard in [dict(mask=0), dict(kind=2), dict(flags=0x80000000)]:
        yield dict(**guard, notifications=[dict(mask=0x10)])


if __name__ == '__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         scope=__doc__, rows=[run(case) for case in cases()]), sort_keys=True))
