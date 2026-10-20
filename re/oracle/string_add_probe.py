#!/usr/bin/env python3
"""Mixed String/number addition through original promotion and concatenation.

Original41b4d0/417a70/417da0/4174a0 and the real byte interner/CRT execute.
Loaded String class lookup, hierarchy predicate, object allocation and ptr-field
lookup are supplied. No full class initialization, script dispatch or GC.
"""
import json
import struct

from vmclass_vectors import _new_oracle, E4D0
from vmexec_vectors import REPO, cstr


def run_case(left, right):
    o = _new_oracle(str(REPO / 'reference/raw/StreetLegal_Redline.bak.exe'))
    holder = o.alloc(0x30)
    o.call(E4D0, [], convention='thiscall', this=holder)
    jvm, owner, cls = o.alloc(0x100), o.alloc(4), o.alloc(0x80)
    o.write_u32(owner, jvm)
    o.write_u32(jvm + 0x68, holder)
    objects, text_by_word = {}, {}
    def allocate(oc, args):
        word = oc.alloc(0x30)
        objects[word] = oc.alloc(12)
        return word
    def ptr_field(oc, args):
        assert oc.read_cstr(args[0]) == b'ptr'
        return objects[oc.reg('ecx')]
    o.hook(0x411610, lambda oc, args: cls, 'thiscall', 1)
    o.hook(0x4044E0, lambda oc, args: 1, 'thiscall', 1)
    o.hook(0x404E20, allocate, 'thiscall', 0)
    o.hook(0x408800, ptr_field, 'thiscall', 1)
    o.hooks.pop(0x41F210, None)
    o.hooks.pop(0x551220, None)  # Numeric promotion must execute the real CRT formatter.
    o.hooks.pop(0x551140, None)  # Concatenation must execute the real CRT strncat.

    def string(text):
        return o.call(0x4174A0, [cstr(o, text)], convention='thiscall', this=jvm).eax
    types = {'S': cstr(o, 'Ljava.lang.String;'), 'I': 0x60AB50, 'F': 0x60AB58}
    cells = []
    for kind, value in (left, right):
        word = string(value) if kind == 'S' and value is not None else (
            0 if kind == 'S' else int.from_bytes(struct.pack('<f', value), 'little') if kind == 'F' else value & 0xFFFFFFFF)
        cell = o.alloc(12)
        o.write_u32s(cell, [0x5E734C, types[kind], word])
        cells.append(cell)
    before = len(objects)
    o.call(0x41B4D0, cells, convention='thiscall', this=owner)
    result = o.read_u32(cells[0] + 8)
    for word, field in objects.items():
        text_by_word[word] = o.read_cstr(o.read_u32(field + 8)).decode('latin1')
    return {'left': left, 'right': right, 'result': text_by_word.get(result),
            'allocated_during_add': len(objects) - before,
            'allocated_texts': list(text_by_word.values())}


if __name__ == '__main__':
    cases = [(('S', 'value='), ('I', -7)), (('I', 7), ('S', '!')),
             (('S', 'float='), ('F', 1.25)), (('S', None), ('I', 7))]
    results = [run_case(a, b) for a, b in cases]
    assert [row['result'] for row in results] == ['value=-7', '7!', 'float=1.250', '7']
    assert [row['allocated_during_add'] for row in results] == [2, 2, 2, 1]
    print(json.dumps({'oracle': __doc__, 'cases': results}, sort_keys=True))
