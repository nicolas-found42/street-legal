#!/usr/bin/env python3
"""Execute original Chassis and WheelRef natives with constructed native-car state.

The resource/data/nativeres/body/template chain is the one FUN_0042ab50,
FUN_00419860 and FUN_00426470 walk: data+0x44 = nativeres, data+0x4c = body,
nativedata+0xc = K. 0x42ab50/0x5447d0/0x410890/0x404e20/0x407da0/0x42a040 are
controlled exactly like re/oracle/wheel_probe.py; field sets, allocations and
calls into unported collaborators are recorded, everything else runs original
machine code. Every capture records raw return words and full body, template,
parameters and wheel words so the Rust port replays bit for bit.
"""
import hashlib, json, struct
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9
from io_vectors import _cell, _call_frame

VT = 0x300F0000   # shared thunk: ecx==resource -> data, ecx==nativeres -> nativedata
VT2 = 0x300F0010  # entity-resource thunk: -> entity


def bits(f):
    return struct.unpack('<I', struct.pack('<f', f))[0]


def build_chassis(o, wheels=2):
    a = {name: o.alloc(0x100) for name in
         ('handle', 'resource', 'vtable', 'data', 'nativeres', 'nativedata')}
    a.update((name, o.alloc(size)) for name, size in
             (('vtable2', 0x40), ('body', 0x2200), ('K', 0x3000), ('params', 0x1000),
              ('ewrap', 0x100), ('eres', 0x100), ('entity', 0x400), ('node', 0x100),
              ('tobj', 0x100), ('tobj2', 0x100), ('spr', 0x100), ('arr', 0x200),
              ('spring0', 0x100)))
    a['wheels'] = o.alloc(0x2b4 * 3)
    o.write_u32(a['handle'] + 12, a['resource'])
    for r in (a['resource'], a['nativeres']):
        o.write_u32(r, a['vtable'])
        o.write_u32(r + 0x4c, 1)
    o.write_u32(a['eres'], a['vtable2'])
    o.write_u32(a['eres'] + 0x4c, 1)
    o.write_u32(a['vtable'] + 12, VT)
    o.write_u32(a['vtable2'] + 12, VT2)
    o.write_u32(a['data'] + 0x44, a['nativeres'])
    o.write_u32(a['data'] + 0x4c, a['body'])
    o.write_u32(a['nativedata'] + 0xc, a['K'])
    o.write_u32(a['body'] + 0x1fbc, a['params'])
    o.write_u32(a['body'] + 0x13e4, a['wheels'])
    o.write_u32(a['body'] + 0x13bc, 0)
    o.write_u32(a['body'] + 0x2c, 0)
    o.write_u32(a['ewrap'] + 8, 1)
    o.write_u32(a['ewrap'] + 0xc, a['eres'])
    o.write_u32(a['entity'] + 0x84, a['node'])
    o.write_u32(a['node'] + 0x1c, a['tobj'])
    o.write_u32(a['node'] + 0x20, a['node'])
    o.write_u32(a['node'] + 0x58, 0)
    o.write_u32(a['spr'] + 0x78, a['arr'])
    o.write_u32(a['node'] + 0x5c, a['spr'])
    o.write_u32(a['spring0'] + 0x30, 0)
    o.write_u32(a['spring0'] + 0x7c, a['node'])
    o.write_u32(a['K'] + 0x1f40, 2)
    return a


def vt_hooks(o, a):
    o.hook(VT, lambda oc, arg: a['data'] if oc.reg('ecx') == a['resource'] else a['nativedata'],
           'thiscall', 1)
    o.hook(VT2, lambda oc, arg: a['entity'], 'thiscall', 1)


def install_common(o, log):
    """The controlled collaborators every chassis row shares."""
    o.hook(0x5447d0, lambda oc, arg: 0, 'thiscall', 3)
    o.hook(0x404e20, lambda oc, arg: log['alloc'].append(('vm', oc.reg('eax'))) or oc.alloc(0x40),
           'thiscall', 0)
    o.hook(0x407da0, lambda oc, arg: log['alloc'].append(('init', oc.reg('eax'), oc.reg('edx')))
           or None, 'cdecl', 3)
    o.hook(0x42a040, lambda oc, arg: log['set'].append(
        (oc.read_cstr(arg[1]).decode(), struct.unpack('<f', struct.pack('<I', arg[2]))[0]))
        or None, 'cdecl', 3)
    o.hook(0x410890, lambda oc, arg: 42, 'thiscall', 1)


def identity_transform(o, tobj, tx=0.0, ty=0.0, tz=0.0):
    for r in range(3):
        for c in range(3):
            o.write_f32(tobj + 0x20 + r * 0x10 + c * 4, 1.0 if r == c else 0.0)
    o.write_f32(tobj + 4, tx)
    o.write_f32(tobj + 8, ty)
    o.write_f32(tobj + 0xc, tz)


def dump(o, a):
    return {
        'body': o.read_u32s(a['body'], 0x2200 // 4),
        'K': o.read_u32s(a['K'], 0x3000 // 4),
        'params': o.read_u32s(a['params'], 0x1000 // 4),
        'wheels': o.read_u32s(a['wheels'], 0x2b4 * 3 // 4),
        'spr': o.read_u32s(a['spr'], 0x40 // 4),
        'arr': o.read_u32s(a['arr'], 0x200 // 4),
        'entity': o.read_u32s(a['entity'], 0x100 // 4),
        'node': o.read_u32s(a['node'], 0x80 // 4),
        'tobj': o.read_u32s(a['tobj'], 0x50 // 4),
        'eres': o.read_u32s(a['eres'], 0x40 // 4),
    }


def seed(o, a, seed=0.25):
    for region, size in (('body', 0x2200), ('K', 0x3000), ('params', 0x1000)):
        for i in range(size // 4):
            o.write_u32(a[region] + i * 4, bits(seed + i * 0.013))
    for i in range(0x2b4 * 3 // 4):
        o.write_u32(a['wheels'] + i * 4, bits(seed * 2 + i * 0.031))
    for name, n in (('ewrap', 0x40), ('eres', 0x40), ('entity', 0x100), ('node', 0x20),
                    ('tobj', 0x40), ('tobj2', 0x40), ('spr', 0x40), ('arr', 0x80),
                    ('spring0', 0x40)):
        for i in range(n):
            o.write_u32(a[name] + i * 4, bits(seed * 3 + i * 0.017))
    # structural words seeding must not destroy
    o.write_u32(a['body'] + 0x1fbc, a['params'])
    o.write_u32(a['body'] + 0x13e4, a['wheels'])
    o.write_u32(a['body'] + 0x13bc, 0)
    o.write_u32(a['body'] + 0x2c, 0)
    o.write_u32(a['body'] + 0xc, a['nativeres'])
    o.write_u32(a['body'] + 8, 1)
    o.write_u32(a['K'] + 0x1f40, 2)
    o.write_u32(a['node'] + 0x1c, a['tobj'])
    o.write_u32(a['node'] + 0x20, a['node'])
    o.write_u32(a['node'] + 0x58, 0)
    o.write_u32(a['spr'] + 0x78, a['arr'])
    o.write_u32(a['node'] + 0x5c, a['spr'])
    o.write_u32(a['ewrap'] + 8, 1)
    o.write_u32(a['ewrap'] + 0xc, a['eres'])
    o.write_u32(a['eres'], a['vtable2'])
    o.write_u32(a['eres'] + 0x4c, 1)
    o.write_u32(a['entity'] + 0x84, a['node'])
    o.write_u32(a['spring0'] + 0x30, 0)
    o.write_u32(a['spring0'] + 0x7c, a['node'])


def fresh(log, wheels=2):
    o = Oracle()
    o.hook_alloc()
    a = build_chassis(o, wheels)
    vt_hooks(o, a)
    install_common(o, log)
    seed(o, a)
    return o, a


ADDR = {
    'getTorque': 0x43c7f0, 'setTorque': 0x43c930, 'getMass': 0x43caa0, 'getCM': 0x43cb80,
    'getMin': 0x43d600, 'getMax': 0x43d8b0, 'setHornSFX': 0x43dc00, 'setNitroSFX': 0x43dd70,
    'setCooling': 0x43deb0, 'forceUpdate': 0x43e320, 'getMileage': 0x43dff0,
    'setMileage': 0x43e080, 'setBuck': 0x43e110, 'setSfxExhaustMinVol': 0x4409d0,
    'setSteerWheelRadius': 0x440af0, 'setSteerWheel': 0x440a50, 'setAckermann': 0x440b70,
    'getSfxTable': 0x442210, 'getWheels': 0x43cd70, 'getWheelPos': 0x43ce30,
    'getWheelDamage': 0x43d080, 'setWheelDamage': 0x43d280,
    'WheelRef.getPos': 0x441b60, 'WheelRef.getYpr': 0x441fd0,
    'WheelRef.setPos': 0x441d10, 'WheelRef.setOppWheel': 0x441a10,
}
CHASSIS = 'java.game.parts.bodypart.Chassis'
WHEELREF = 'java.game.parts.WheelRef'


def chassis_call(o, a, name, sig, args, this=None):
    this = this or a['handle']
    o.hook(0x42ab50, lambda oc, arg: this, 'cdecl', 2)
    cells = [_cell(o, t, v & 0xFFFFFFFF) for t, v in args]
    cells.append(_cell(o, 'L', this))
    frame = _call_frame(o, CHASSIS, name, sig, cells)
    return o.call(ADDR[name], [frame], fpu_cw=CW_D3D9)


def main():
    rows = []

    def record(row):
        rows.append(row)
        state = 'error' if 'error' in row else 'ok'
        print(f"{row.get('name')} {row.get('case')}: {state}", file=sys.stderr)

    import sys
    # --- broken chains: unbound handle ---------------------------------------
    for name, sig, args in [
            ('getWheels', '()I', []), ('getMass', '()F', []),
            ('getMileage', '()F', []), ('getTorque', '(FF)F', [('F', bits(0.5)), ('F', bits(0.3))]),
            ('getCM', '()Ljava.lang.Vector3;', []), ('getMin', '()Ljava.lang.Vector3;', []),
            ('getMax', '()Ljava.lang.Vector3;', []), ('getSfxTable', '(I)Ljava.game.parts.SfxTable;', [('I', 1)]),
            ('getWheelPos', '(I)Ljava.lang.Vector3;', [('I', 0)]),
            ('getWheelDamage', '(I)Ljava.lang.String;', [('I', 0)])]:
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        o.write_u32(a['handle'] + 12, 0)
        try:
            r = chassis_call(o, a, name, sig, args)
            record({'name': name, 'case': 'unbound', 'ret': {'eax': r.eax, 'st0': r.st0},
                    'alloc': log['alloc'], 'set': log['set']})
        except Exception as error:  # noqa: BLE001
            record({'name': name, 'case': 'unbound', 'error': str(error)})

    # stage-2 break: nativeres missing from data
    for name, sig, args in [('getWheels', '()I', []), ('getMass', '()F', []),
                            ('getWheelDamage', '(I)Ljava.lang.String;', [('I', 0)])]:
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        o.write_u32(a['data'] + 0x44, 0)
        try:
            r = chassis_call(o, a, name, sig, args)
            record({'name': name, 'case': 'no-nativeres', 'ret': {'eax': r.eax, 'st0': r.st0}})
        except Exception as error:  # noqa: BLE001
            record({'name': name, 'case': 'no-nativeres', 'error': str(error)})

    # --- getWheels / getMass / mileage ----------------------------------------
    log = {'alloc': [], 'set': []}
    o, a = fresh(log)
    o.write_u32(a['K'] + 0x1f40, 3)
    r = chassis_call(o, a, 'getWheels', '()I', [])
    record({'name': 'getWheels', 'case': 'count3', 'ret': {'eax': r.eax}})
    massobj, inner = o.alloc(0x100), o.alloc(0x100)
    o.write_u32(a['body'] + 0x13bc, massobj)
    o.write_u32(massobj + 0x5c, inner)
    for f in (2.0, 0.0, -4.0):
        o.write_f32(inner + 0x14, f)
        r = chassis_call(o, a, 'getMass', '()F', [])
        record({'name': 'getMass', 'case': f'f={f}', 'ret': {'eax': r.eax, 'st0': r.st0}})
    o.write_f32(a['body'] + 0x20fc, 1234.5)
    r = chassis_call(o, a, 'getMileage', '()F', [])
    record({'name': 'getMileage', 'case': 'read', 'ret': {'eax': r.eax, 'st0': r.st0}})
    before = dump(o, a)
    chassis_call(o, a, 'setMileage', '(F)V', [('F', bits(77.25))])
    record({'name': 'setMileage', 'case': 'write', 'word': o.read_u32(a['body'] + 0x20fc),
            'before': before, 'after': dump(o, a)})

    # --- torque ----------------------------------------------------------------
    for boost in (0.25, 1.0, 2.0):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        o.write_f32(a['body'] + 0x1e38, boost)
        o.write_f32(a['body'] + 0x1dd0, bits(11.0))
        o.write_f32(a['body'] + 0x1dd4, bits(13.0))
        table = o.alloc(0x80)
        for i, (xx, yy) in enumerate(((0.0, 5.0), (0.5, 7.0), (1.0, 3.0))):
            o.write_f32(table + i * 8, xx)
            o.write_f32(table + i * 8 + 4, yy)
        o.write_u32(a['params'] + 0, 3)
        o.write_u32(a['params'] + 4, table)
        o.write_u32(a['params'] + 8, 3)
        o.write_u32(a['params'] + 0xc, table)
        for rpm in (0.1, 0.75, 1.5):
            r = chassis_call(o, a, 'getTorque', '(FF)F',
                             [('F', bits(rpm)), ('F', bits(0.5))])
            record({'name': 'getTorque', 'case': f'boost={boost} rpm={rpm}',
                    'ret': {'eax': r.eax, 'st0': r.st0}})
        before = dump(o, a)
        chassis_call(o, a, 'setTorque', '(F)V', [('F', bits(42.5))])
        record({'name': 'setTorque', 'case': f'boost={boost}', 'before': before,
                'after': dump(o, a)})

    # --- simple parameter setters -----------------------------------------------
    for name, offset, value in [('setSfxExhaustMinVol', 0x7d0, bits(0.125)),
                                ('setSteerWheelRadius', 0xa14, bits(1.5)),
                                ('setAckermann', 0xa1c, bits(-0.5))]:
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        before = dump(o, a)
        chassis_call(o, a, name, '(F)V', [('F', value)])
        record({'name': name, 'case': f'params+{offset:#x}',
                'word': o.read_u32(a['params'] + offset), 'before': before, 'after': dump(o, a)})
    log = {'alloc': [], 'set': []}
    o, a = fresh(log)
    before = dump(o, a)
    chassis_call(o, a, 'setSteerWheel', '(FF)V', [('F', bits(0.75)), ('F', bits(2.25))])
    record({'name': 'setSteerWheel', 'case': 'a14,a18',
            'words': [o.read_u32(a['params'] + 0xa14), o.read_u32(a['params'] + 0xa18)],
            'before': before, 'after': dump(o, a)})
    for args in ((1.0, 2.0, 3.0), (-1.0, -2.0, -3.0), (0.0, 60.0, -0.25)):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        before = dump(o, a)
        chassis_call(o, a, 'setCooling', '(FFF)V', [('F', bits(f)) for f in args])
        record({'name': 'setCooling', 'case': repr(args),
                'words': [o.read_u32(a['params'] + 0xa48), o.read_u32(a['params'] + 0xa4c),
                          o.read_u32(a['params'] + 0xa50)],
                'before': before, 'after': dump(o, a)})

    # --- horn / nitro sfx ---------------------------------------------------------
    def horn(name, sig, addr, index, live, volume):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        if live:
            target = o.alloc(0x100)
            o.write_u32(target + 0x50, 0x515)
            a['target'] = target
        slot = a['params'] + (0x814 if name == 'setNitroSFX' else 0x834 + index * 16)
        if name == 'setHornSFX':
            o.write_u32(slot + 0xc, a['eres'])
            o.write_u32(a['eres'] + 0x48, slot)
        rr = o.alloc(0x100)
        o.write_u32(rr + 0xc, a.get('target', 0))
        args = [('L', rr if live else 0), ('F', bits(volume))]
        if name == 'setHornSFX':
            args.append(('I', index & 0xFFFFFFFF))
        before = dump(o, a)
        chassis_call(o, a, name, sig, args)
        record({'name': name, 'case': f'idx={index} res={"live" if live else "null"} vol={volume}',
                'before': before, 'after': dump(o, a),
                'slot_words': o.read_u32s(slot, 4),
                'volume_word': o.read_u32(a['params'] + 0x874 + index * 4)
                if name == 'setHornSFX' else None,
                'target_words': o.read_u32s(a.get('target', a['eres']), 0x60 // 4)
                if live else None})

    for index in (0, 2, 4, -1):
        horn('setHornSFX', '(Ljava.util.resource.ResourceRef;FI)V', 0x43dc00, index, True, 0.8)
    horn('setHornSFX', '(Ljava.util.resource.ResourceRef;FI)V', 0x43dc00, 1, False, 0.4)
    horn('setNitroSFX', '(Ljava.util.resource.ResourceRef;F)V', 0x43dd70, 0, True, 0.6)
    horn('setNitroSFX', '(Ljava.util.resource.ResourceRef;F)V', 0x43dd70, 0, False, 0.2)

    # --- getSfxTable ----------------------------------------------------------------
    for index in (0, 1, 2, 3, -1):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        try:
            r = chassis_call(o, a, 'getSfxTable', '(I)Ljava.game.parts.SfxTable;',
                             [('I', index & 0xFFFFFFFF)])
            record({'name': 'getSfxTable', 'case': f'idx={index}', 'ret': {'eax': r.eax},
                    'alloc': log['alloc']})
        except Exception as error:  # noqa: BLE001
            record({'name': 'getSfxTable', 'case': f'idx={index}', 'error': str(error)})

    # --- setBuck ----------------------------------------------------------------------
    def buck(id1, floats, existing):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        world = o.alloc(0x40)
        o.write_u32(0x617650, world)
        o.write_u32(world, bits(12.5))
        head = a['body'] + 0x1dec
        if existing:
            rec = o.alloc(0x40)
            for i in range(0x10):
                o.write_u32(rec + i * 4, bits(0.5 + i * 0.01))
            o.write_u32(rec, 0x5f0f04)
            o.write_u32(rec + 4, head)
            o.write_u32(rec + 8, head)
            o.write_u32(a['body'] + 0x1df4, rec)
            o.write_u32(head, 0)
        before = dump(o, a)
        cells = [('I', id1 & 0xFFFFFFFF)] + [('F', bits(f)) for f in floats]
        try:
            r = chassis_call(o, a, 'setBuck', '(IIFFFF)I', cells)
            record({'name': 'setBuck', 'case': f'id={id1} existing={existing} f={floats}',
                    'ret': {'eax': r.eax}, 'before': before, 'after': dump(o, a)})
        except Exception as error:  # noqa: BLE001
            record({'name': 'setBuck', 'case': f'id={id1} existing={existing}',
                    'error': str(error), 'before': before})

    buck(0, (1.0, 2.0, 3.0, 4.0), False)
    buck(-5, (1.0, 2.0, 3.0, 4.0), False)
    buck(7, (1.5, -2.0, 3.0, 4.0), True)
    buck(7, (0.0, 1.0, 1.0, 1.0), True)
    buck(9, (1.0, 1.0, 1.0, 1.0), True)

    # --- forceUpdate: capture the 0x448430 tail call -------------------------------------
    log = {'alloc': [], 'set': []}
    o, a = fresh(log)
    calls = []
    o.hook(0x448430, lambda oc, arg: calls.append(oc.reg('ecx')), 'thiscall', 1)
    try:
        chassis_call(o, a, 'forceUpdate', '()V', [])
        record({'name': 'forceUpdate', 'case': 'tail-call',
                'callee_this': calls[0] if calls else None,
                'callee_is_K': bool(calls) and calls[0] == a['K']})
    except Exception as error:  # noqa: BLE001
        record({'name': 'forceUpdate', 'case': 'tail-call', 'error': str(error)})

    # --- entity-dependent Vector3 natives --------------------------------------------------
    def entity_case(name, sig, entity_present, walk, extra=None):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        if not entity_present:
            o.write_u32(a['ewrap'] + 8, 0)
        for i, f in enumerate((1.0, 2.0, 3.0)):
            o.write_f32(a['K'] + 0x1eac + i * 4, f)
        if walk:
            o.write_u32(a['node'] + 0x58, 4)
            identity_transform(o, a['tobj'], 100.0, 200.0, 300.0)
        if name == 'getCM' and entity_present:
            o.write_u32(a['entity'] + 0x20c, 1)
            rec = a['entity'] + 0x64 + 1 * 0xd4
            for r in range(3):
                for c in range(3):
                    o.write_f32(rec + 0x40 + r * 0x10 + c * 4, 2.0 if r == c else 0.0)
            o.write_f32(rec + 0, 1000.0)
            o.write_f32(rec + 4, 2000.0)
            o.write_f32(rec + 8, 3000.0)
            o.write_f32(rec + 0x40, 5.0)
            o.write_f32(rec + 0x44, 6.0)
            o.write_f32(rec + 0x48, 7.0)
            o.write_u32(a['arr'] + 0x20, 2)
            for i in range(2):
                o.write_f32(a['arr'] + 0x24 + i * 0xc, 3.0 + i)
                o.write_f32(a['arr'] + 0x24 + i * 0xc + 4, 4.0 + i)
                o.write_f32(a['arr'] + 0x24 + i * 0xc + 8, 5.0 + i)
            o.write_f32(a['arr'] + 0x40, 8.0)
            o.write_f32(a['arr'] + 4, 9.0)
            o.write_f32(a['arr'] + 8, 10.0)
        if name in ('getMin', 'getMax') and entity_present:
            o.write_u32(a['arr'] + 0x20, 2)
            for i in range(2):
                o.write_f32(a['arr'] + 0x24 + i * 0xc, 1.5 + i)
                o.write_f32(a['arr'] + 0x24 + i * 0xc + 4, 2.5 + i)
                o.write_f32(a['arr'] + 0x24 + i * 0xc + 8, 3.5 + i)
        if name == 'getWheelPos':
            for i, f in enumerate((1.0, 2.0, 3.0)):
                o.write_f32(a['K'] + 0x1f44 + i * 4, f)
            o.write_f32(a['wheels'] + 0x78, 7.0)
            o.write_f32(a['wheels'] + 0x7c, 8.0)
            o.write_f32(a['wheels'] + 0x80, 9.0)
        before = dump(o, a)
        cells = []
        if sig == '(I)Ljava.lang.Vector3;':
            cells.append(('I', 0))
        try:
            r = chassis_call(o, a, name, sig, cells)
            row = {'name': name, 'case': f'entity={entity_present} walk={walk}',
                   'ret': {'eax': r.eax}, 'before': before, 'after': dump(o, a),
                   'alloc': log['alloc'], 'set': log['set']}
            record(row)
        except Exception as error:  # noqa: BLE001
            record({'name': name, 'case': f'entity={entity_present} walk={walk}',
                    'error': str(error), 'before': before})

    for name, sig in [('getCM', '()Ljava.lang.Vector3;'), ('getMin', '()Ljava.lang.Vector3;'),
                      ('getMax', '()Ljava.lang.Vector3;'), ('getWheelPos', '(I)Ljava.lang.Vector3;')]:
        entity_case(name, sig, False, False)
        entity_case(name, sig, True, False)
        entity_case(name, sig, True, True)

    # --- wheel damage strings ---------------------------------------------------
    GET = object()

    def wheel_damage(index, text, damaged):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        if damaged:
            o.write_u32(a['wheels'] + 0x100, 0x20)
        o.write_f32(a['wheels'] + 0x78, 1.25)
        o.write_f32(a['wheels'] + 0x7c, 2.5)
        o.write_f32(a['wheels'] + 0x80, -3.75)
        o.write_f32(a['wheels'] + 0xf4, 4.5)
        o.write_f32(a['wheels'] + 0xf8, 5.5)
        o.write_f32(a['K'] + 0x1f44 + 0x2c, 6.5)
        for i, f in enumerate((0.1, 0.2, 0.3)):
            o.write_f32(a['K'] + 0x1f44 + 0xc + i * 4, f)
        # damaged path formats the wheel matrix; seed a non-identity rotation
        o.write_f32(a['wheels'] + 0x90, bits(0.70710677))
        o.write_f32(a['wheels'] + 0x94, bits(-0.70710677))
        o.write_f32(a['wheels'] + 0xa4, bits(0.70710677))
        o.write_f32(a['wheels'] + 0xa8, bits(0.70710677))
        o.write_u32(a['wheels'] + 0x30, a['spring0'])
        o.write_f32(a['spring0'] + 0x30, 8.0)
        before = dump(o, a)
        if text is GET:
            name, sig, addr, cells = 'getWheelDamage', '(I)Ljava.lang.String;', 0x43d080, []
        else:
            sptr = o.alloc(0x40)
            o.write(sptr, text.encode() + b'\0')
            name, sig, addr = 'setWheelDamage', '(ILjava.lang.String;)V', 0x43d280
            cells = [('L', sptr)]
        interned = []
        o.hook(0x4174a0, lambda oc, arg: interned.append(oc.reg('ecx')) or 0xBEEF000,
               'cdecl', 1)
        try:
            r = chassis_call(o, a, name, sig, [('I', index & 0xFFFFFFFF)] + cells)
            record({'name': name, 'case': f'idx={index} text={text!r} damaged={damaged}',
                    'ret': {'eax': r.eax}, 'before': before, 'after': dump(o, a),
                    'interned_ecx': interned[0] if interned else None})
        except Exception as error:  # noqa: BLE001
            record({'name': name, 'case': f'idx={index} text={text!r} damaged={damaged}',
                    'error': str(error), 'before': before})

    wheel_damage(0, GET, False)
    wheel_damage(0, GET, True)
    wheel_damage(0, '1,2,3,4,5,6,7,8,9,10', False)
    wheel_damage(0, '0,0,0,0,0,0,0,0,0,0', True)
    wheel_damage(0, '', False)
    wheel_damage(2, '1,2,3,4,5,6,7,8,9,10', False)
    wheel_damage(-1, '1,2,3,4,5,6,7,8,9,10', False)

    # --- WheelRef natives ---------------------------------------------------------
    def wheel_native(name, sig, args, with_template=True, entity_present=False, index0=0,
                     vec_values=(5.0, 6.0, 7.0)):
        log = {'alloc': [], 'set': []}
        o, a = fresh(log)
        wheel = a['wheels'] + index0 * 0x2b4
        o.write_u32(wheel + 0x22c, a['body'])
        o.write_u32(wheel + 0x30, a['spring0'])
        o.write_f32(a['spring0'] + 0x30, 8.0)
        if with_template:
            for i, f in enumerate((11.0, 22.0, 33.0)):
                o.write_f32(a['K'] + 0x1f44 + index0 * 0xc8 + i * 4, f)
            o.write_f32(a['K'] + 0x1f44 + index0 * 0xc8 + 0x38, 0.5)
            o.write_f32(a['K'] + 0x1f44 + index0 * 0xc8 + 0x3c, 0.25)
        else:
            o.write_u32(a['body'] + 0xc, 0)
            o.write_u32(a['body'] + 8, 0)
        if entity_present:
            o.write_u32(a['node'] + 0x58, 4)
            identity_transform(o, a['tobj'], 1000.0, 2000.0, 3000.0)
            o.write_u32(a['body'] + 0x2c, a['ewrap'])
        for r in range(3):
            for c in range(3):
                o.write_f32(wheel + 0x90 + r * 0x10 + c * 4, 1.0 if r == c else 0.0)
        o.write_f32(wheel + 0x78, 1.0)
        o.write_f32(wheel + 0x7c, 2.0)
        o.write_f32(wheel + 0x80, 3.0)
        o.write_u32(wheel + 0x250, 0x7654321)
        before = dump(o, a)
        vec = 0
        if any(t == 'V' for t, _ in args):
            vec = o.alloc(0x40)
            for i, f in enumerate(vec_values):
                o.write_f32(vec + 0x10 + i * 4, f)
        o.hook(0x42ab50, lambda oc, arg: wheel, 'cdecl', 2)
        o.hook(0x42a560, lambda oc, arg: struct.unpack(
            '<f', o.read(vec + 0x10 + {'x': 0, 'y': 4, 'z': 8}[
                oc.read_cstr(arg[1]).decode()])[0])[0], 'cdecl', 2)
        cells = []
        for t, _ in args:
            cells.append(_cell(o, 'L' if t == 'V' else t, vec if t == 'V' else 0))
        cells.append(_cell(o, 'L', wheel))
        frame = _call_frame(o, WHEELREF, name, sig, cells)
        try:
            r = o.call(ADDR[f'WheelRef.{name}'], [frame], fpu_cw=CW_D3D9)
            record({'name': f'WheelRef.{name}',
                    'case': f'template={with_template} entity={entity_present} idx0={index0}',
                    'ret': {'eax': r.eax}, 'before': before, 'after': dump(o, a),
                    'alloc': log['alloc'], 'set': log['set']})
        except Exception as error:  # noqa: BLE001
            record({'name': f'WheelRef.{name}',
                    'case': f'template={with_template} entity={entity_present} idx0={index0}',
                    'error': str(error), 'before': before})

    wheel_native('getPos', '()Ljava.lang.Vector3;', [], True, False)
    wheel_native('getPos', '()Ljava.lang.Vector3;', [], False, False)
    wheel_native('getPos', '()Ljava.lang.Vector3;', [], True, True)
    wheel_native('getYpr', '()Ljava.lang.Ypr;', [], True, False)
    wheel_native('setPos', '(Ljava.lang.Vector3;)V', [('V', 1)], True, False)
    wheel_native('setPos', '(Ljava.lang.Vector3;)V', [('V', 1)], False, False)
    wheel_native('setPos', '(Ljava.lang.Vector3;)V', [('V', 1)], True, True)
    for index in (1, -1, 0x100):
        wheel_native('setOppWheel', '(I)V', [('I', index & 0xFFFFFFFF)], True, False)

    payload = {
        'exe_sha256': hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
        'scope': __doc__,
        'rows': rows,
    }
    print(json.dumps(payload, sort_keys=True))


if __name__ == '__main__':
    main()
