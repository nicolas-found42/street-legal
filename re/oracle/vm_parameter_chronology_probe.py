#!/usr/bin/env python3
"""Original 41fbc0 parameter variable allocation/attachment before 41ab60.

Calls real frame entry and numeric conversion helpers with declared loaded class,
method/signature and I/F arguments. Passive Unicorn code hooks never replace a
measured helper. Allocator, class-init, class lookup, diagnostics and clock
collaborators are inherited from vmexec_vectors.setup. This establishes the
bounded chronology and final parameter values, not raw VM/native ABI acceptance.
"""
from pathlib import Path
import argparse
import hashlib
import json
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from vmexec_vectors import setup, arr, TC, CW, EXE_SHA256
from vm_cell_identity_probe import pool_state, cell_identity
from unicorn import UC_HOOK_CODE, __version__ as unicorn_version

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def inventory():
    # Arguments are declaration order; original consumes last first.
    cases = [
        [('I', 11, 'I'), ('F', 0x40200000, 'F')],
        [('I', 0xfffffff9, 'F'), ('F', 0xc0600000, 'I')],
        [('F', 0x3fc00000, 'I'), ('I', 16777217, 'F'), ('F', 0x80000000, 'F')],
    ]
    return [dict(static=static, arguments=[dict(source=t, payload=w, target=d)
            for t, w, d in arguments]) for static in [True, False] for arguments in cases]


def capture(spec):
    s, pool, tref, base, leaf, obj, methods, seams = setup()
    o = s.o
    method = methods[0]
    o.write_u32(method + 4, 9 if spec['static'] else 1)
    type_cells = []
    for ty in [a['target'] for a in spec['arguments']] + ['I']:
        cell = o.alloc(4)
        o.write_u32(cell, TC[ty])
        type_cells.append(cell)
    sig = o.alloc(12)
    o.write_u32s(sig, [s.jvm, 0, arr(o, type_cells)])
    o.write_u32(method + 24, sig)
    args = [s._cell(dict(k='v', t=a['source'], w=a['payload'])) for a in spec['arguments']]
    s._container_set(s.frames[0] + 28, args)
    body = o.alloc(24)
    o.write_u32s(body, [0x1007, 0x1c, 0x100b, 77, 0x2b, 0])
    o.write_u32(base + 24, arr(o, [body]))
    record = o.alloc(16)
    o.write_u32s(record, [base, 0, method, 1])
    observations = []

    def value(pointer):
        return dict(type=o.read_cstr(o.read_u32(pointer + 4)).decode('ascii'),
                    payload=o.read_u32(pointer + 8))

    def observe(uc, pc, size, unused):
        if pc != 0x41ab60:
            return
        destination, source = o.read_u32s(o.reg('esp') + 4, 2)
        frame = o.read_u32(s.thread + 0x20)
        locals_ = s._container(frame + 12)
        identity = cell_identity(s, destination)
        assert identity['kind'] == 'variable'
        assert locals_[-1] == destination
        assert o.read_u32(destination + 8) == 0
        observations.append(dict(destination=value(destination), source=value(source),
            destination_slot=identity['slot'], local_count=len(locals_),
            destination_is_last_local=locals_[-1] == destination,
            variable_handed=pool_state(o, 0x62f134)['words'][2],
            local_addresses=locals_, destination_address=destination,
            source_address=source, frame_address=frame,
            registers={r:o.reg(r) for r in ['eax','ebx','ecx','edx','esi','edi','ebp','esp']}))

    o.uc.hook_add(UC_HOOK_CODE, observe)
    result = o.call(0x41fbc0, [record, base, 0 if spec['static'] else obj],
                    convention='thiscall', this=s.thread, fpu_cw=CW)
    assert len(observations) == len(spec['arguments'])
    assert result.fpcw == CW and result.fpu_top_depth == 0
    locals_ = s._container(o.read_u32(s.thread + 0x20) + 12)
    final = [value(pointer) for pointer in locals_]
    if not spec['static']:
        assert final[0]['payload'] == obj
        final[0] = dict(type='LLeaf;', payload='receiver')
    return dict(input=spec, stores=observations, final_locals=final,
                return_code=result.eax, instructions=result.icount, seams=seams)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, default=ROOT/'re/out/vmexec/vm_parameter_chronology.json')
    args = parser.parse_args()
    executable = ROOT/'reference/raw/StreetLegal_Redline.bak.exe'
    assert digest(executable) == EXE_SHA256
    producers = [Path(__file__), Path(__file__).with_name('vmexec_vectors.py'),
        Path(__file__).with_name('thread_vectors.py'), Path(__file__).with_name('oracle.py'),
        Path(__file__).with_name('vm_cell_identity_probe.py'), Path(__file__).with_name('vmclass_vectors.py')]
    document = dict(schema=1, executable_sha256=EXE_SHA256, unicorn=unicorn_version,
        cw=CW, producers={str(p.relative_to(ROOT)):digest(p) for p in producers},
        scope='real41fbc0/41ab60; declared loaded metadata and inherited collaborators; bounded parameter chronology',
        cases=[capture(spec) for spec in inventory()])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2)+'\n')
    print(json.dumps(dict(output=str(args.output), cases=len(document['cases']), sha256=digest(args.output))))


if __name__ == '__main__':
    main()
