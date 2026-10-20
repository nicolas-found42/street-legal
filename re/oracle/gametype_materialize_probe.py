#!/usr/bin/env python3
"""Original type8 text materialization over read-only stock RPK descriptors.

Runs546ae0 defaults and5461a0 parser; records410890 class requests with supplied
class identity. No original class loading/script execution, package manager or
whole startup. Redirect derived rows into ignored re/out/ only.
"""
import hashlib
import json
import struct
from pathlib import Path
from oracle import Oracle, CW_D3D9


def descriptors(path):
    data = path.read_bytes()
    assert data[:8] == b'RPAK\x00\x02\x00\x00'
    p = 16 + 64 * struct.unpack_from('<I', data, 8)[0]
    headers, count, type1, other = struct.unpack_from('<IIII', data, p)
    assert count == type1 + other
    p += 16
    start = p
    rows = {}
    for _ in range(count):
        parent, ident, kind, flags, amp, offset, size = struct.unpack_from('<IIBBfII', data, p)
        p += 22
        length = data[p]
        p += 1
        alias = data[p:p + length].split(b'\0')[0].decode('latin1')
        p += length + (48 if kind == 1 else 0)
        assert offset + size <= len(data)
        rows[ident] = {'parent': parent, 'kind': kind, 'flags': flags, 'alias': alias, 'text': data[offset:offset + size]}
    assert p - start == headers
    return rows


def run():
    root = Path(__file__).resolve().parents[2]
    exe = root / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    stock = descriptors(root / 'stock/system.rpk')
    assert stock[27]['parent'] == 1 and stock[27]['kind'] == 8
    assert stock[27]['text'] == b'script java.util.resource.Dummy\r\n'
    assert stock[1]['text'] == b'lod_amp 1.000\r\n'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    node, descriptor, klass = [o.alloc(512) for _ in range(3)]
    o.write_u32s(descriptor, [8, 27, 0, 0, 0, 0])
    o.call(0x546ae0, [descriptor], convention='thiscall', this=node, fpu_cw=CW_D3D9)
    data = node + 0x80
    requests = []
    def lookup(oc, args):
        requests.append(oc.read_cstr(args[0]).decode('latin1'))
        return klass
    o.hook(0x410890, lookup, 'thiscall', 1)
    phases = []
    for ident in [1, 27]:
        text = stock[ident]['text']
        buffer = o.alloc(len(text) + 1)
        o.write(buffer, text + b'\0')
        r = o.call(0x5461a0, [node, buffer, len(text), ('f32', 1.0)], convention='thiscall', this=data, fpu_cw=CW_D3D9)
        phases.append({'local_id': ident, 'descriptor': text.decode('latin1'), 'instructions': r.icount})
    assert requests == ['java.util.resource.Dummy']
    assert o.read_u32(data + 12) == 0
    assert o.read_u32(data + 16) == klass
    assert o.read_u32(data + 24) == 0x3f800000
    assert o.read_u32(data + 28) == 0
    return {'oracle': 'Unicorn original546ae0+5461a0; actual stock system.rpk text; supplied410890 class identity', 'phases': phases, 'requests': requests, 'template_flags': o.read_u32(node + 0x54), 'compiled_constructor': 0, 'class_matches_request': True, 'word18_bits': o.read_u32(data + 24), 'word1c_bits': o.read_u32(data + 28), 'scope': 'two supplied parser phases; package inheritance/loading not executed'}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
