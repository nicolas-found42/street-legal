#!/usr/bin/env python3
"""Run complete original425c60; observe its425a90 argument list and conversion order.

Only VM collaborators are controlled:4174a0 string construction,411600 class
lookup,407da0 object construction,408800 field access,42a430 asyncMode lookup,
and425a90 callback execution. The original decides callback name, arity, types,
argument order, and which collaborators execute. This is bounded callback
conversion evidence, not proof that stock gameplay reaches every event kind.

Regenerate from the repository root:
    ~/slr-oracle-venv/bin/python re/oracle/script_event_arguments_probe.py > re/out/boot/script-event-arguments-original.json
    cargo test -p slr-boot --bin slr-boot event_argument -- --nocapture
"""
import hashlib
import itertools
import json
from pathlib import Path

from oracle import CW_D3D9, DEFAULT_EXE, Oracle


def run(case):
    o = Oracle(fpu_cw=CW_D3D9)
    calls = []
    reference = o.alloc(16)
    source = o.alloc(16)
    resource = o.alloc(0x100)
    envelope = o.alloc(0x100)
    field = o.alloc(16)
    argument = o.alloc(32)
    override = o.alloc(32)
    o.write(argument, b'event payload\0')
    o.write(override, b'overrideHandler\0')
    o.write_u32(reference + 12, resource)
    o.write_u32(source + 12, resource)
    o.write_u32(resource + 0x50, 0x1234)
    o.write_u32(envelope + 0x50, 0x9876)
    argument = argument if case['argument'] else 0
    allocations = []

    def allocate(oc, args):
        value = oc.alloc(args[0])
        allocations.append(value)
        calls.append(dict(kind='allocate', size=args[0]))
        return value

    def string(oc, args):
        calls.append(dict(kind='string', text=oc.read_cstr(args[0]).decode('ascii')))
        return 0x12345678

    def class_lookup(oc, args):
        calls.append(dict(kind='class'))
        return 0x12340000

    def construct(oc, args):
        calls.append(dict(kind='construct'))
        return 0

    def field_access(oc, args):
        calls.append(dict(kind='field'))
        return field

    def asynchronous(oc, args):
        calls.append(dict(kind='asyncMode'))
        return int(case['async'])

    def callback(oc, args):
        stack = oc.reg('esp') + 4
        words = []
        # First three words are envelope, method name, request_return.
        # Remaining words are (tag,value)* followed by the0x4f terminator.
        for i in range(3, 16):
            word = oc.read_u32(stack + 4 * i)
            words.append(word)
            if i % 2 == 1 and word == 0x4f:
                break
        else:
            raise AssertionError('callback has no variadic terminator')
        pairs = []
        for tag, value in zip(words[0:-1:2], words[1:-1:2]):
            if value == argument and case['event'] == 0x100000:
                value = 'argument' if argument else 0
            elif value in allocations:
                value = 'GameRef'
            pairs.append([tag, value])
        calls.append(dict(kind='callback', method=oc.read_cstr(oc.read_u32(stack + 4)).decode('ascii'),
                          request_return=oc.read_u32(stack + 8), arguments=pairs,
                          terminator=words[-1]))
        return 0

    for address, fn, convention, nargs in [
        (0x54f560, allocate, 'cdecl', 1),
        (0x4174a0, string, 'thiscall', 1),
        (0x411600, class_lookup, 'thiscall', 1),
        (0x407da0, construct, 'thiscall', 2),
        (0x408800, field_access, 'thiscall', 1),
        (0x42a430, asynchronous, 'cdecl', 2),
        (0x425a90, callback, 'cdecl', 0),
    ]:
        o.hook(address, fn, convention, nargs)
    o.call(0x425c60, [reference, source if case['source'] else 0,
                    case['event'], argument, envelope, override if case['name'] else 0])
    return dict(case=case, argument_word=argument, calls=calls)


def cases():
    for event, argument, asynchronous, source, name in itertools.product(
            [0x100000, 0x10100000, 0x80, 0x10000080, 0x10, 0x10000010, 0],
            [False, True], [False, True], [False, True], [False, True]):
        yield dict(event=event, argument=argument, source=source, name=name) | {'async': asynchronous}


if __name__ == '__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                         producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                         oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
                         configuration=dict(entry=0x425c60, fpu_cw=CW_D3D9),
                         scope=__doc__, rows=[run(case) for case in cases()]), sort_keys=True))
