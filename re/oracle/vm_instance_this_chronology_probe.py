#!/usr/bin/env python3
"""Passive instance-this stores in original41fbc0, not full ABI acceptance.

Real frame/variable construction and numeric parameter assignment run with
vmexec_vectors.setup's declared allocator/class/clock collaborators. Hooks only
observe instructions and stores. No measured helper is replaced by this probe.
"""
from pathlib import Path
import argparse
import hashlib
import json
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from vmexec_vectors import setup, arr, TC, CW, EXE_SHA256
from vm_cell_identity_probe import cell_identity
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE, __version__ as unicorn_version

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def inventory():
    return [dict(receiver_variant=variant, parameters=parameters)
            for variant in [0, 1] for parameters in [[], ['I'], ['I', 'F']]]


def capture(spec):
    s, pool, tref, base, leaf, obj, methods, seams = setup()
    o = s.o
    if spec['receiver_variant']:
        o.alloc(113)
        obj = o.alloc(32)
        o.write_u32(obj + 12, leaf)
    method = methods[0]
    o.write_u32(method + 4, 1)
    types = []
    for ty in spec['parameters'] + ['I']:
        cell = o.alloc(4)
        o.write_u32(cell, TC[ty])
        types.append(cell)
    sig = o.alloc(12)
    o.write_u32s(sig, [s.jvm, 0, arr(o, types)])
    o.write_u32(method + 24, sig)
    s._container_set(s.frames[0] + 28, [s._cell(dict(k='v', t=ty,
        w=11 if ty == 'I' else 0x40200000)) for ty in spec['parameters']])
    body = o.alloc(24)
    o.write_u32s(body, [0x1007, 0x1c, 0x100b, 77, 0x2b, 0])
    o.write_u32(base + 24, arr(o, [body]))
    record = o.alloc(16)
    o.write_u32s(record, [base, 0, method, 1])
    events = []
    state = {}

    def record_event(kind, **extra):
        events.append(dict(kind=kind, pc=o.reg('eip'),
            current_frame=o.read_u32(s.thread + 0x20),
            registers={r:o.reg(r) for r in ['eax', 'ebx', 'ecx', 'edx',
                                           'esi', 'edi', 'ebp', 'esp']}, **extra))

    def observe(uc, pc, size, unused):
        if pc == 0x423ec0 and not state.get('constructor_seen'):
            state['constructor_seen'] = True
            state['variable'] = o.reg('ecx')
            state['callee'] = o.read_u32(s.thread + 0x20)
            state['locals'] = o.read_u32(state['callee'] + 0x18)
            record_event('constructor_enter', variable=state['variable'],
                name_address=o.read_u32(o.reg('esp') + 4))
        if pc == 0x4202c7:
            variable = state['variable']
            record_event('typed_zero_constructed', variable=variable,
                identity=cell_identity(s, variable), words=o.read_u32s(variable, 4),
                descriptor=o.read_cstr(o.read_u32(variable + 4)).decode('ascii'),
                name=o.read_cstr(o.read_u32(variable + 12)).decode('ascii'))

    def write(uc, access, address, size, value, unused):
        if size != 4:
            return
        pc = o.reg('eip')
        if address == s.thread + 0x20 and pc == 0x4202a1:
            record_event('select_callee', previous=o.read_u32(address), selected=value)
        if 'variable' not in state:
            return
        variable = state['variable']
        if address == state['locals'] + 8 and pc == 0x4202f8:
            old_count = o.read_u32(address)
            data = o.read_u32(state['locals'] + 4)
            record_event('append_this', previous_count=old_count, count=value,
                attached=o.read_u32(data + 4 * old_count),
                payload=o.read_u32(variable + 8))
        if address == variable + 8 and pc == 0x4202fb:
            locals_ = s._container(state['callee'] + 12)
            record_event('store_receiver', before=o.read_u32(address), after=value,
                locals=locals_, attached=locals_[-1] == variable)

    o.uc.hook_add(UC_HOOK_CODE, observe)
    o.uc.hook_add(UC_HOOK_MEM_WRITE, write)
    result = o.call(0x41fbc0, [record, base, obj], convention='thiscall',
                    this=s.thread, fpu_cw=CW)
    assert [e['kind'] for e in events] == ['select_callee', 'constructor_enter',
        'typed_zero_constructed', 'append_this', 'store_receiver']
    assert result.fpcw == CW and result.fpu_top_depth == 0
    variable = state['variable']
    assert o.read_u32(variable + 8) == obj
    return dict(input=spec, receiver=obj, events=events,
        final_words=o.read_u32s(variable, 4), return_code=result.eax,
        instructions=result.icount, seams=seams)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path,
        default=ROOT/'re/out/vmexec/vm_instance_this_chronology.json')
    args = parser.parse_args()
    executable = ROOT/'reference/raw/StreetLegal_Redline.bak.exe'
    assert digest(executable) == EXE_SHA256
    producers = [Path(__file__), *[Path(__file__).with_name(name) for name in [
        'vmexec_vectors.py', 'thread_vectors.py', 'oracle.py',
        'vm_cell_identity_probe.py', 'vmclass_vectors.py']]]
    document = dict(schema=1, executable_sha256=EXE_SHA256,
        unicorn=unicorn_version, cw=CW,
        producers={str(p.relative_to(ROOT)):digest(p) for p in producers},
        scope='bounded instance-this41fbc0/423ec0 chronology; declared setup collaborators; passive stores',
        cases=[capture(spec) for spec in inventory()])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2)+'\n')
    print(json.dumps(dict(output=str(args.output), cases=len(document['cases']),
                         sha256=digest(args.output))))


if __name__ == '__main__':
    main()
