#!/usr/bin/env python3
"""Original427980 orchestration with explicit supplied subsystem boundaries.

Executes original ordering/context link stores; resolves declared package IDs,
constructs supplied JVM/Config/resource containers, and records native batches,
loader/settings/refresh boundaries. No original script body or startup runs.
Redirect derived output into ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, CW_D3D9


def run():
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    o.hook_alloc()
    context, jvm, config = [o.alloc(0x1000) for _ in range(3)]
    o.write_u32(config + 0x50, 0xffff0080)
    events = []
    containers = {}
    handles = {0x62f260: 'gametype-library', 0x636460: 'game-root', 0x636328: 'root-type14', 0x62f250: 'root-type12', 0x636318: 'root-type13', 0x636450: 'world-root'}

    def resource(oc, args):
        name = oc.read_cstr(args[0]).decode('latin1')
        assert name == 'system.rpk'
        events.append({'call': 'resource-id', 'package': name, 'local_id': args[1]})
        return 0x10000 | args[1]

    def resolve(oc, args):
        resource_id, kind, flags = args
        assert flags == 0
        if resource_id not in containers:
            containers[resource_id] = o.alloc(256)
        node = containers[resource_id]
        o.write_u32(node + 0x4c, kind)
        o.write_u32(node + 0x50, resource_id)
        o.write_u32s(oc.reg('ecx'), [0, 0, resource_id, node])
        events.append({'call': 'resource-link', 'handle': handles.get(oc.reg('ecx'), 'config-template'), 'local_id': resource_id & 0xffff, 'kind': kind, 'flags': flags})
        return 0

    def simple(address, name, conv='cdecl', count=0, result=0):
        def record(oc, args):
            events.append({'call': name})
            return result
        o.hook(address, record, conv, count)

    def create(oc, args):
        assert args[0] == 0x636460
        assert oc.read_u32(args[1] + 8) == 0x10047
        assert oc.read_cstr(args[3]) == b'Config'
        events.append({'call': 'create-config', 'parent': 'game-root', 'local_id': 0x47, 'name': 'Config'})
        return config

    o.hook(0x538440, resource, 'thiscall', 2)
    o.hook(0x546070, resolve, 'thiscall', 3)
    simple(0x40fb50, 'jvm-constructor', 'thiscall', result=jvm)
    simple(0x40fe70, 'loader-boot', 'thiscall')
    simple(0x487f20, 'native-batch-487f20')
    simple(0x46b530, 'native-batch-46b530')
    simple(0x4427e0, 'native-batch-4427e0')
    simple(0x4a5580, 'subsystem-4a5580', 'thiscall')
    simple(0x544f40, 'refresh-game-root', 'thiscall', 1)
    o.hook(0x53a5e0, create, 'cdecl', 4)
    simple(0x427bf0, 'apply-settings')
    result = o.call(0x427980, [], convention='thiscall', this=context, fpu_cw=CW_D3D9)
    assert result.eax == 1
    assert o.read_u32(context + 0xe4) == jvm
    assert o.read_u32s(context + 0xf8, 4) == [0, 0, 0xffff0080, config]
    assert o.read_u32(config + 0x48) == context + 0xf8
    assert events[-1] == {'call': 'apply-settings'}
    return {'oracle': 'Unicorn original427980 orchestration/context stores; supplied resource/JVM/loader/native/create/settings boundaries', 'events': events, 'context_jvm_matches': True, 'context_config_id': o.read_u32(context + 0x100), 'context_config_node_matches': True, 'instructions': result.icount}


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
