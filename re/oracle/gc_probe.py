#!/usr/bin/env python3
"""Original incremental mark/sweep over a declared502-object chain and2 garbage.

Real417f80/418970/4198b0/408500 execute. Allocation/free and empty native roots
are supplied; declared thread/static root lists are empty. No scriptfinalizers.
"""
import json
from langobj_vectors import Probe, default_exe


def run(wide=False):
    p = Probe(default_exe())
    o = p.o
    def realloc(oc, args):
        pointer = oc.alloc(args[1])
        if args[0]:
            oc.write(pointer, oc.read(args[0], min(oc.heap_sizes[args[0]], args[1])))
        return pointer
    o.hook(0x54F580, realloc, 'cdecl', 2)
    jvm = o.alloc(0x100)
    o.write_u32(jvm + 8, o.alloc(512 * 12))
    cls = o.alloc(0x200)
    o.write_u32(cls + 0xC, jvm)
    type_name = o.alloc(16)
    o.write(type_name, b'Ltest.Graph;\0')
    type_record = o.alloc(4)
    o.write_u32(type_record, type_name)
    o.write_u32(cls + 8, type_record)
    objects = [o.alloc(0x20) for _ in range(504)]
    for index, word in enumerate(objects):
        o.write_u32(word + 0xC, cls)
        table, vector = o.alloc(0x20), o.alloc(12)
        o.write_u32(word + 0x10, table)
        o.write_u32(table + 0xC, vector)
        if index < 501:
            cell, data = o.alloc(16), o.alloc(4)
            o.write_u32s(cell, [0x5E734C, type_name, objects[index + 1], 0])
            o.write_u32(data, cell)
            o.write_u32s(vector, [1, data, 1])

    def chain(words):
        root = o.alloc(0x20)
        entries = [root + 4, *words, root + 0x10]
        for previous, following in zip(entries, entries[1:]):
            o.write_u32(previous + 4, following)
            o.write_u32(following + 8, previous)
        return root

    o.write_u32(jvm + 0x10, chain(objects))
    o.write_u32(jvm + 0x18, chain([]))
    o.write_u32(jvm + 0x1C, chain([]))
    o.write_u32(jvm + 0x44, chain([]))
    o.write_u32(jvm + 0x34, 504)
    o.write_u32(jvm + 0x38, 1000)
    o.hook(0x418110, lambda oc, args: 0, 'thiscall', 0)
    rows = []

    def snapshot(result):
        context = o.read_u32(jvm + 0x20)
        rows.append({'result': result, 'active': bool(context),
                     'pending': o.read_u32(context + 8) if context else 0,
                     'marked': sum(bool(o.read_u32(word + 0x18) & 1) for word in objects),
                     'queued': [index + 1 for index, word in enumerate(objects)
                                if o.read_u32(word + 0x18) & 8],
                     'allocation_counter': o.read_u32(jvm + 0x34),
                     'mark_bytes': o.read_u32(jvm + 0x28)})

    result = o.call(0x417F80, [], convention='thiscall', this=jvm).eax
    root = o.alloc(12)
    for word in objects[:502] if wide else objects[:1]:
        o.write_u32s(root, [0x5E734C, type_name, word])
        o.call(0x4198B0, [root], convention='thiscall', this=jvm)
    snapshot(result)
    for _ in range(3):
        snapshot(o.call(0x417F80, [], convention='thiscall', this=jvm).eax)
    assert [row['marked'] for row in rows] == ([502, 502, 502, 0] if wide else [1, 251, 501, 0])
    assert rows[-1]['queued'] == [503, 504]
    return {'oracle': 'original417f80/418970/4198b0/408500; allocation and empty native roots supplied',
            'chain_length': 502, 'garbage': 2, 'rows': rows}


if __name__ == '__main__':
    result = run()
    result['wide'] = run(True)
    print(json.dumps(result, sort_keys=True))
