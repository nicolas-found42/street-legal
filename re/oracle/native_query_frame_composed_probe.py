#!/usr/bin/env python3
"""Original53a2a0 ->427210 ->426470 -> real player/cursor getInfo.

The source caller and native constructors run with the existing instance
probe's declared resource/config/manager/allocator collaborators.427210,
426470,47b220 and4625f0 are not hooked.419860 resource access and bounded
callback-node allocation are explicit additional collaborators. Original
template vtables remain intact; only their actual postcreate entry is hooked.
This is bounded composed original execution, not stock boot, VM or device
acceptance. Full caller arena, retained stack, declared globals and all GPRs
are compared at every collaborator/native entry and ordinary caller return.
"""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
from pathlib import Path

from unicorn import UC_HOOK_CODE

import native_instance_probe as source
from oracle import Oracle, HEAP_BASE, CW_D3D9

ROOT = Path(__file__).resolve().parents[2]
CONTEXT = 0x636338
GLOBALS = list(dict.fromkeys(source.GLOBALS + list(range(CONTEXT, CONTEXT + 0xd0, 4))))
REGISTERS = ['eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp']
PROFILE = dict(
    entry=0x53a2a0,
    real_composition=[0x53a2a0, 0x427210, 0x426470, 0x4797d0, 0x45fdf0,
                      0x47b220, 0x4625f0],
    additional_controlled=['419860 returns declared template payload',
                           '54f560 callback nodes at arena+4800,4820,4840',
                           'actual native template postcreate entry'],
    inherited=source.PROFILE,
    stack='nonzero caller GPRs and original53a2a0 pending CALL; whole thread '
          'stack seeded, complete observed4096-byte window, strict store bounds',
    unclaimed=['resource/VM/config collaborator bodies', 'upper VM caller ABI',
               'chronological store sequence parity', 'EFLAGS/raw x87 parity',
               'whole boot/features/devices'],
)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def inventory():
    return [dict(mode='ready', composition=kind, fill=fill, parent=parent)
            for kind, fill, parent in itertools.product(
                ['player', 'cursor'], [0, 0xa5], [False, True])]


class ComposedOracle(Oracle):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.composed_measured = False
        self.composed_events = []
        self.callbacks = {}
        self.class_table = 0
        self.registration_nodes = 0
        self.postcreate = None
        # Original list constructors execute as declared setup, before the
        # instance probe records its initial pages, bytes and globals.
        for offset in [0, 0x1c, 0x38]:
            super().call(0x428fd0, [], convention='thiscall', this=CONTEXT + offset)
        for offset in [0xc4, 0xc8, 0xcc]:
            super().write_u32(CONTEXT + offset, 0)
        self.uc.hook_add(UC_HOOK_CODE, self.observe_entry)

    def snapshot(self):
        return dict(heap=self.read_u32s(HEAP_BASE, source.HEAP_BYTES // 4),
                    stack=self.read_u32s(source.STACK_LOW, 1024),
                    registers={r: self.reg(r) for r in REGISTERS},
                    globals={str(a): self.read_u32(a) for a in GLOBALS})

    def record(self, address, args, name='collaborator'):
        if self.composed_measured:
            self.composed_events.append(dict(name=name, address=address,
                args=list(args), return_pc=self.read_u32(self.reg('esp')),
                **self.snapshot()))

    def observe_entry(self, _uc, pc, _size, _data):
        if not self.composed_measured:
            return
        if pc == 0x427210:
            self.record(pc, self.read_u32s(self.reg('esp') + 4, 1), 'real-registration-entry')
        elif pc in [0x47b220, 0x4625f0]:
            self.record(pc, self.read_u32s(self.reg('esp') + 4, 3), 'real-native-getInfo-entry')

    def hook(self, address, callback, convention='cdecl', nargs=0):
        self.callbacks[address] = (callback, convention, nargs)
        if address == 0x427210:
            return  # Its complete original body and nested queries execute.

        def wrapped(oc, args):
            self.record(address, args)
            if address == 0x54f560 and args == [0x1c]:
                if self.registration_nodes >= 3:
                    raise RuntimeError('undeclared fourth callback-node allocation')
                pointer = HEAP_BASE + 0x4800 + self.registration_nodes * 0x20
                self.registration_nodes += 1
                return pointer
            return callback(oc, args)

        return super().hook(address, wrapped, convention, nargs)

    def write_u32(self, address, value):
        if self.class_table and address == HEAP_BASE + 0x800 and value == HEAP_BASE + 0x900:
            # The base probe copies a native vtable solely to replace+40.
            # Keep original class identity and declare the actual+40 boundary.
            value = self.class_table
            self.postcreate = self.read_u32(value + 0x40)
            callback, convention, nargs = self.callbacks[0x300f0060]
            self.hook(self.postcreate, callback, convention, nargs)
        return super().write_u32(address, value)

    def call(self, address, *args, **kwargs):
        if address == 0x53a2a0:
            self.hook(0x419860, lambda *_: HEAP_BASE + 0x700, 'thiscall', 4)
            if 0x427210 in self.hooks or 0x426470 in self.hooks:
                raise RuntimeError('composed query/registration body unexpectedly hooked')
            if self.read_u32(HEAP_BASE + 0x800) not in [0x5f12f0, 0x5f11e4]:
                raise RuntimeError('actual native template identity was not retained')
            self.composed_measured = True
            try:
                result = super().call(address, *args, **kwargs)
            finally:
                self.composed_measured = False
            ACTIVE.append(self)
            return result
        result = super().call(address, *args, **kwargs)
        if address in [0x477bc0, 0x45fd00]:
            self.class_table = self.read_u32(result.eax)
        return result


ACTIVE = []


def capture():
    previous_oracle, previous_globals = source.Oracle, source.GLOBALS
    source.Oracle, source.GLOBALS = ComposedOracle, GLOBALS
    rows, pages = [], {}
    try:
        for case in inventory():
            try:
                row = source.capture_case(case)
                oracle = ACTIVE.pop()
                row['events'] = oracle.composed_events
                row['postcreate'] = oracle.postcreate
                row['registration_nodes'] = oracle.registration_nodes
                row['native_table'] = oracle.class_table
                if sum(e['address'] == 0x427210 for e in row['events']) != 1:
                    raise RuntimeError('complete original registration entry was not observed exactly once')
                native = 0x47b220 if case['composition'] == 'player' else 0x4625f0
                if sum(e['address'] == native for e in row['events']) != 3:
                    raise RuntimeError('actual native getInfo did not run all three capability queries')
                refs = []
                for page in row.pop('input_pages'):
                    identity = f"{page['address']:08x}-{page['sha256']}"
                    pages.setdefault(identity, page)
                    refs.append(identity)
                row['input_page_ids'] = refs
                rows.append(row)
            except Exception as error:
                raise source.CaptureFailure(error, dict(completed_rows=rows,
                    failed_case=case, details=getattr(error, 'details', None))) from error
    finally:
        source.Oracle, source.GLOBALS = previous_oracle, previous_globals
    return dict(schema=1, scope=__doc__, profile=PROFILE,
        profile_sha256=hashlib.sha256(json.dumps(PROFILE, sort_keys=True,
            separators=(',', ':')).encode()).hexdigest(),
        producer_sha256=digest(__file__),
        source_sha256=digest(Path(source.__file__)),
        oracle_sha256=digest(Path(source.__file__).with_name('oracle.py')),
        exe_sha256=digest(source.EXE), fpu_cw=CW_D3D9,
        execution=dict(oracle='Unicorn original build606 executable',
            unicorn=source.unicorn.__version__,
            python=source.platform.python_version(),
            packages={name: source.importlib.metadata.version(name)
                      for name in ['unicorn', 'capstone', 'pefile']},
            host=source.platform.platform()),
        inventory=inventory(), global_addresses=GLOBALS,
        input_pages=pages, rows=rows)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists():
        raise SystemExit('Preserve previous attempts; choose a fresh output path')
    args.out.parent.mkdir(parents=True, exist_ok=True)
    try:
        document = capture()
    except Exception as error:
        with args.out.with_suffix(args.out.suffix + '.failure.json').open('x') as stream:
            json.dump(dict(error=str(error), details=getattr(error, 'details', None),
                producer_sha256=digest(__file__), exe_sha256=digest(source.EXE)), stream)
            stream.write('\n')
        raise
    with args.out.open('x') as stream:
        json.dump(document, stream, sort_keys=True)
        stream.write('\n')
    print(len(document['rows']), 'actual composed53a2a0/427210/426470/native-getInfo callers', args.out)
