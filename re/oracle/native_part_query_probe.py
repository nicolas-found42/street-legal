#!/usr/bin/env python3
"""4769e0 closed scalar/reference/default branches over controlled part layouts.

The template is already postparsed. Only the oracle allocator is hooked; all
query and resource functions execute original code.
The caller's output starts unbound or belongs to a real intrusive reference
chain. 47 uses real template/runtime slot records; 49 uses sentinel-terminated
part lists. Loaded, pending, and entity-null physics references and populated
render-color selection are exercised. Installation/slot compatibility branches
40/41/58/71/72 and template lazy initialization remain outside this scope.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9, HEAP_BASE


def words(o):
    return o.read_u32s(HEAP_BASE, (o.heap_ptr - HEAP_BASE + 3) // 4)


def run():
    rows = []
    cases = [(f"message{n}", n, 0) for n in range(81)
             if n not in [11, 40, 41, 47, 49, 58, 60, 71, 72]]
    cases += [("mass-unbound", 11, 0)]
    for msg in [1, 6, 23, 43]:
        cases += [(f"reference{msg}-{mode}", msg, mode) for mode in [1, 2, 3, 4, 5, 6]]
    cases += [(f"slot-{i}", 47, i + 1) for i in range(3)]
    cases += [(f"parts-{i}", 49, i) for i in range(8)]
    cases += [(f"variant-{i}", 60, i) for i in [0, 1, 0xffffffff]]
    cases += [(f"truncate-{bits:08x}", 15, bits) for bits in
              [0x3c75c28f, 0xbc75c28f, 0x4b800001, 0xcb800001,
               0x7f800000, 0xff800000, 0x7fc01234, 0x00000001]]
    for mass in [0, 0x3f000000, 0x3f800000, 0xbf800000, 0x3c75c28f,
                 0x4b800001, 0x7f800000, 0x7fc01234]:
        for output in [0, 1]:
            cases.append((f"mass-loaded-{mass:08x}-{output}", 11, mass))
    cases += [("mass-missing", 11, 0), ("mass-pending", 11, 0)]
    cases += [(f"variant-populated-{n}", 60, n) for n in [0, 1, 4, 0xffffffff]]
    cases += [("variant-spatial", 60, 3), ("variant-pending", 60, 2)]
    for label, message, mode in cases:
        o = Oracle(fpu_cw=CW_D3D9)
        o.hook_alloc()
        o.write_u32(0x6200a4, 0)
        k = o.alloc(0x2e98)
        body = o.alloc(0x300)
        output = o.alloc(0x30)
        resources = [o.alloc(0x240) for _ in range(4)]
        renders = o.alloc(48)
        indices = o.alloc(12)
        slots = o.alloc(3 * 0x68)
        nodes = [o.alloc(0x100) for _ in range(3)]
        sentinel = o.alloc(16)
        neighbor = o.alloc(16)
        for i, p in enumerate(resources):
            o.write_u32(p + 0x50, 0x1a000010 + i)
        o.write_u32(k, 0x5f0f60)
        o.write_u32(k + 0x120, 1)
        o.write_u32(k + 0xc, 0x1a000006)
        o.write_u32(k + 0x10, resources[0])
        o.write_u32(k + 0x114c, 0xc0ffee)
        o.write_u32(k + 0x1150, 3)
        o.write_u32(k + 0x115c, nodes[0])
        o.write_u32(body + 0x24, 0x1a000011)
        o.write_u32(body + 0x28, resources[1])
        o.write_u32(body + 0x4c, renders)
        o.write_u32(body + 0x50, indices)
        o.write_u32(body + 0x58, 12)
        o.write_u32(body + 0x5c, 0xaabbccdd)
        o.write_u32(body + 0x74, 7)
        o.write_u32(body + 0x78, slots)
        o.write_u32(body + 0xac, 0x01020304)
        o.write_f32(body + 0xb4, -1.1275)
        o.write_u32(body + 0xc8, 0x1a000013)
        o.write_u32(body + 0xcc, resources[3])
        o.write_u32(renders + 8, 0x1a000012)
        o.write_u32(renders + 12, resources[2])
        for i, p in enumerate(nodes):
            o.write_u32(p + 4, nodes[i + 1] if i < 2 else sentinel)
            o.write_u32(p + 0x48, 100 + i)
            o.write_u32(slots + i * 0x68 + 0x24, resources[i])
        argument = 0
        if message in [1, 6, 23, 43] and mode:
            argument = output
            if mode in [2, 3, 4, 5]:
                target = resources[0] if mode in [2, 4, 5] else resources[{1: 1, 6: 0, 23: 2, 43: 3}[message]]
                o.write_u32(output + 8, o.read_u32(target + 0x50))
                o.write_u32(output + 12, target)
                o.write_u32(target + 0x48, output)
                if mode == 4:
                    o.write_u32(target + 0x48, neighbor)
                    o.write_u32(neighbor + 4, output)
                    o.write_u32(output, neighbor)
                if mode == 5:
                    o.write_u32(output + 4, neighbor)
                    o.write_u32(neighbor, output)
            if mode == 6:
                source = {1: body + 0x24, 6: k + 0xc, 23: renders + 8, 43: body + 0xc8}[message]
                o.write_u32(source, 0)
                o.write_u32(source + 4, 0)
                o.write_u32(output + 8, 0x1a000010)
                o.write_u32(output + 12, resources[0])
                o.write_u32(resources[0] + 0x48, output)
        if message == 47:
            argument = output
            o.write_u32(output, 100 + mode - 1)
        if message == 49:
            argument = output if mode != 4 else 0
            o.write_u32(body + 0xdc, nodes[0])
            for i, p in enumerate(nodes):
                o.write_u32(p + 0x14, o.read_u32(resources[i] + 0x50))
                o.write_u32(p + 0x18, resources[i])
            if mode in [1, 2, 3]:
                target = resources[mode - 1]
                o.write_u32(output + 8, o.read_u32(target + 0x50))
                o.write_u32(output + 12, target)
                o.write_u32(target + 0x48, output)
            if mode == 5:
                o.write_u32(output + 8, 0x7fffffff)
            if mode == 6:
                o.write_u32(body + 0xdc, sentinel)
            if mode == 7:
                o.write_u32(output + 8, o.read_u32(resources[0] + 0x50))
        if label == "mass-unbound":
            o.write_u32(body + 0x24, 0)
            o.write_u32(body + 0x28, 0)
        if message == 60:
            argument = mode
        if label.startswith("truncate"):
            o.write_u32(body + 0xb4, mode)
        if label.startswith("mass-") and label != "mass-unbound":
            resource = resources[1]
            entity = o.alloc(0x214)
            definition = o.alloc(0x100)
            shape = o.alloc(0x100)
            o.write_u32(resource, 0x5f37a4)
            o.write_u32(resource + 0x4c, 2)
            o.write_u32(resource + 0x64, 1)
            o.write_u32(resource + 0x68, 0x3f800000)
            o.write_u32(resource + 0x15c, entity)
            o.write_u32(entity + 0x5c, definition)
            o.write_u32(definition + 0x78, shape)
            o.write_u32(shape + 12, mode)
            argument = output if label.endswith("-1") else 0
            if label == "mass-missing":
                o.write_u32(resource + 0x15c, 0)
                o.call(0x429350, convention='thiscall', this=0x636338 + 0x70)
            if label == "mass-pending":
                o.write_u32(resource + 0x64, 0x10000000)
        if message == 60 and not label.startswith("variant-0") and not label.startswith("variant-1") and label != "variant-4294967295":
            o.write_u32(body + 0x54, 3)
            for i, index in enumerate([2, 0, 1]):
                resource = resources[i]
                o.write_u32(resource, 0x5f37f4)
                o.write_u32(resource + 0x4c, 3)
                o.write_u32(resource + 0x54, 0x100)
                o.write_u32(resource + 0x64, 1)
                o.write_u32(resource + 0x68, 0x3f800000)
                o.write_u32(resource + 0xbc, 0x3f800000)
                o.write_u32(renders + i * 16 + 12, resource)
                o.write_u32(indices + i * 4, index)
                descriptor = k + 0x128 + index * 0x74
                colors = o.alloc(32) + 16
                o.write_u32(descriptor + 0x5c, [2, 0, 3][i])
                o.write_u32(descriptor + 0x60, colors)
                for j in range(-3, 3):
                    o.write_u32(colors + j * 4, 0x20200000 + i * 0x100 + j)
                if label == "variant-pending":
                    o.write_u32(resource + 0x64, 0x10000000)
            if label == "variant-spatial":
                # A valid pool with a single remaining free record. Further
                # requests execute the original pool-exhaustion branch.
                manager = o.alloc(0x1100)
                free = o.alloc(0x3c)
                o.write_u32(0x618d48, manager)
                for offset in [0xde8, 0xe04]:
                    o.call(0x429350, convention='thiscall', this=manager + offset)
                    o.write_u32(manager + offset, 0x5f09a8)
                o.write_u32(free, 0x5f3558)
                o.write_u32(free + 4, manager + 0xe14)
                o.write_u32(free + 8, manager + 0xe08)
                o.write_u32(manager + 0xe0c, free)
                o.write_u32(manager + 0xe1c, free)
                o.write_u32(resources[0] + 0xd8 + 0xbc, 0x4800)
                o.write_u32(resources[2] + 0xd8 + 0xbc, 0x800)
        global_addresses = [0x618d48, 0x6200a4] + list(range(0x636338 + 0x70, 0x636338 + 0xf0, 4))
        global_before = {a: o.read_u32(a) for a in global_addresses}
        before = words(o)
        heap_end = o.heap_ptr
        try:
            result = o.call(0x4769e0, [body, message, argument],
                            convention="thiscall", this=k, fpu_cw=CW_D3D9)
        except Exception as error:
            raise RuntimeError(label) from error
        after = words(o)
        rows.append(dict(name=label, template=k, body=body, message=message,
                         argument=argument, eax=result.eax, heap_end=heap_end, heap_after=o.heap_ptr,
                         initial=[[HEAP_BASE + i * 4, x] for i, x in enumerate(before) if x] + [[a, x] for a, x in global_before.items() if x],
                         changes=sorted([[HEAP_BASE + i * 4, x] for i, x in enumerate(after) if x != (before[i] if i < len(before) else 0)] + [[a, o.read_u32(a)] for a, x in global_before.items() if o.read_u32(a) != x])))
    return dict(scope=__doc__, exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                constants=[[a, o.read_u32(a)] for a in [0x5f09c8, 0x5f11d8]], rows=rows)


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
