#!/usr/bin/env python3
"""Compare an independent source-derived native-entry kernel with build606.

The original41fbc0 resolves controlled metadata, converts actual caller cells,
builds its native argument vector, then executes420025 through native entry.
The independent projection covers precisely that final kernel, including the
cached41e2f0 helper and its stack stores. Earlier lookup/conversion/allocator
effects are supplied live preconditions, not claimed as projected. This is an
executable seam experiment, not a production bridge or complete callback test.
"""
from pathlib import Path
import hashlib
import json
import random
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import CW_D3D9, DEFAULT_EXE
from vmexec_vectors import setup, cstr, _holder, native_cases
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE

REGS = ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp')


def project(o):
    """420025..4200b7, after argument conversion, before native execution."""
    r = {name: o.reg(name) for name in REGS}
    b = r['esp']
    low, high = b - 64, b + 0x60
    memory = bytearray(o.read(low, high - low))
    read_pages = {low & ~4095, (high - 1) & ~4095}
    writes = []
    overlay = {}

    def read(address):
        read_pages.add(address & ~4095)
        read_pages.add((address + 3) & ~4095)
        return overlay.get(address, o.read_u32(address))

    def write(address, value, pc):
        overlay[address] = value
        writes.append({'pc': pc, 'address': address, 'size': 4, 'value': value})
        if low <= address <= high - 4:
            memory[address - low:address - low + 4] = value.to_bytes(4, 'little')

    # Carry the live registry record needed to validate the production seam's
    # supplied implementation identity, even though this kernel already has
    # that address in EDI from the preceding original lookup.
    if read(read(b + 0x14) + 8) != r['edi']:
        raise RuntimeError('native record and prepared implementation differ')

    # Original420025..420050 creates the receiver slot and CallInfo.
    r['ebx'] = read(b + 0x10)
    write(r['eax'] + r['ecx'] * 4, r['esi'], 0x420029)
    r['edx'] = read(b + 0x2c)
    r['ecx'] = read(b + 0x14)
    r['edx'] += 1
    write(b + 0x2c, r['edx'], 0x420035)
    r['edx'] = read(r['ebx'] + 0x10)
    r['eax'] = b + 0x24
    write(b + 0x48, r['ecx'], 0x420040)
    r['ecx'] = read(b + 0x50)
    write(b + 0x40, r['ebx'], 0x420048)
    write(b + 0x3c, r['edx'], 0x42004c)
    write(b + 0x44, r['eax'], 0x420050)
    # Cached41e2f0: its CALL and PUSH ESI are retained scratch too.
    write(b - 4, 0x420059, 0x420054)
    write(b - 8, r['esi'], 0x41e2f0)
    types = read(r['ecx'] + 8)
    if types == 0:
        raise RuntimeError('projection requires cached signature types')
    r['ecx'] = types
    r['eax'] = read(types + 8) - 1
    r['edx'] = read(types)
    if r['edx'] > r['eax']:
        r['ecx'] = read(types + 4)
        r['eax'] = read(r['ecx'] + r['eax'] * 4)
    else:
        r['eax'] = 0
    info = b + 0x3c
    if r['eax'] == 0:
        r['eax'] = info
        ret, push_pc, call_pc = 0x4200b9, 0x4200b6, 0x4200b7
    else:
        r['edx'] = read(r['eax'])
        byte = o.read(r['edx'], 1)[0]
        read_pages.add(r['edx'] & ~4095)
        r['eax'] = ((byte if byte < 128 else byte - 256) - 0x46) & 0xffffffff
        if r['eax'] > 0x15:
            raise RuntimeError('unsupported original return-type branch')
        r['ecx'] = o.read(0x42079c + r['eax'], 1)[0]
        read_pages.add(0x420000)
        branch = o.read_u32(0x420788 + r['ecx'] * 4)
        if branch == 0x420079:
            r['edx'] = info
            ret, push_pc, call_pc = 0x420080, 0x42007d, 0x42007e
        elif branch == 0x420087:
            r['eax'] = info
            ret, push_pc, call_pc = 0x420090, 0x42008b, 0x42008c
        elif branch == 0x42009d:
            r['ecx'] = info
            ret, push_pc, call_pc = 0x4200a4, 0x4200a1, 0x4200a2
        elif branch == 0x4200ab:
            r['edx'] = info
            ret, push_pc, call_pc = 0x4200b9, 0x4200af, 0x4200b7
        else:
            raise RuntimeError(f'unsupported return jump {branch:#x}')
    write(b - 4, info, push_pc)
    write(b - 8, ret, call_pc)
    r['esp'] = b - 8
    return {'registers': r, 'stack_address': low, 'stack_hex': memory.hex(),
            'writes': writes, 'return_pc': ret, 'call_info': info,
            'initial': {'registers': {name: o.reg(name) for name in REGS},
                        'pages': [{'address': address, 'bytes': o.read(address, 4096).hex()}
                                  for address in sorted(read_pages)]}}


def run(ret, static, seed):
    s, pool, _, base, _, obj, methods, events = setup()
    o = s.o
    record = o.read_u32(o.read_u32(pool + 12) + 16)
    o.write_u32(record + 8, cstr(o, 'Base'))
    o.write_u32(s.jvm + 0x68, _holder(o))
    o.hook(0x41f210, lambda oc, args: 0, 'fastcall', 0)
    o.hooks.pop(0x406f60, None)
    registry = o.alloc(0x62c)
    o.call(0x41a370, [registry], convention='fastcall')
    o.write_u32(s.jvm + 12, registry)
    signature = '(IF)' + ret
    descriptor = o.read_u32(o.read_u32(pool + 12) + 8)
    o.write_u32(descriptor + 8, cstr(o, signature))
    o.write_u32(methods[0] + 4, 0x49 if static else 0x41)
    implementation = 0x41b860
    o.call(0x416b00, [cstr(o, 'Base'), cstr(o, 'method'), cstr(o, signature), implementation],
           convention='thiscall', this=s.jvm)
    cells = [s._cell({'k': 'v', 't': 'I', 'w': 11}),
             s._cell({'k': 'v', 't': 'F', 'w': 0x40200000})]
    s._container_set(s.frames[0] + 28, cells)
    call_record = o.alloc(16)
    o.write_u32s(call_record, [base, 0, methods[0], 1])
    random_source = random.Random(seed)
    o.write(0x7fef0000, random_source.randbytes(0x10000))
    predicted = actual = None
    kernel_writes = []
    dispatch_esp = None

    def observe(uc, pc, size, _):
        nonlocal predicted, dispatch_esp
        if pc == 0x41fbc0:
            dispatch_esp = o.reg('esp')
        if pc == 0x420025:
            if predicted is not None:
                raise RuntimeError('kernel entered twice')
            predicted = project(o)

    def store(uc, access, address, size, value, _):
        if predicted is not None and actual is None:
            kernel_writes.append({'pc': o.reg('eip'), 'address': address,
                                  'size': size, 'value': value})

    def native(oc, args):
        nonlocal actual
        actual = {'registers': {name: oc.reg(name) for name in REGS},
                  'stack_address': predicted['stack_address'],
                  'stack_hex': oc.read(predicted['stack_address'], 0xa0).hex(),
                  'writes': list(kernel_writes), 'return_pc': oc.read_u32(oc.reg('esp')),
                  'call_info': args[0]}
        return 2.5 if ret == 'F' else 0x11223344

    o.hook(implementation, native, 'cdecl', 1)
    o.uc.hook_add(UC_HOOK_CODE, observe)
    o.uc.hook_add(UC_HOOK_MEM_WRITE, store)
    result = o.call(0x41fbc0, [call_record, base, 0 if static else obj],
                    convention='thiscall', this=s.thread, fpu_cw=CW_D3D9)
    expected = {key: value for key, value in predicted.items() if key != 'initial'}
    if actual != expected:
        mismatches = [key for key in expected if expected[key] != actual[key]]
        raise AssertionError({'signature': signature, 'static': static,
                              'mismatches': mismatches, 'expected': predicted, 'actual': actual})
    assert actual['registers']['esp'] == dispatch_esp - 0x54
    return {'signature': signature, 'static': static, 'scratch_seed': seed,
            'dispatch_esp': dispatch_esp, 'matched': True, 'projection': predicted,
            'original_return_eax': result.eax, 'original_instructions': o.icount,
            'collaborators': events}


def capture():
    rows = [run(case['ret'], case['static'], seed)
            for case in native_cases() for seed in (606, 20261022)]
    sources = [Path(__file__), Path(__file__).with_name('oracle.py'),
               Path(__file__).with_name('vmexec_vectors.py'),
               Path(__file__).with_name('thread_vectors.py'),
               Path(__file__).with_name('vmclass_vectors.py')]
    return {'schema': 1, 'scope': __doc__, 'rows': rows,
            'genuine_stock_kernel': genuine_stock_kernel(),
            'exe_sha256': hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
            'producer_sources': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}}


def genuine_stock_kernel():
    """Apply the same projection to the actual nested stock callback path."""
    import native_vm_entry_probe as probe
    original_replay = probe.Replay
    compared = []

    class ObservedReplay(original_replay):
        def __init__(self, name):
            super().__init__(name)
            predicted = None
            writes = []

            def observe(uc, pc, size, _):
                nonlocal predicted
                if pc == 0x420025:
                    if predicted is not None:
                        raise RuntimeError('unexpected second live native kernel')
                    predicted = project(self)
                if pc == 0x486f30:
                    actual = {'registers': {name: self.reg(name) for name in REGS},
                              'stack_address': predicted['stack_address'],
                              'stack_hex': self.read(predicted['stack_address'], 0xa0).hex(),
                              'writes': list(writes), 'return_pc': self.read_u32(self.reg('esp')),
                              'call_info': self.read_u32(self.reg('esp') + 4)}
                    if actual != {key: value for key, value in predicted.items() if key != 'initial'}:
                        raise AssertionError({'expected': predicted, 'actual': actual})
                    compared.append(predicted)
                    predicted = None

            def store(uc, access, address, size, value, _):
                if predicted is not None:
                    writes.append({'pc': self.reg('eip'), 'address': address,
                                   'size': size, 'value': value})

            self.uc.hook_add(UC_HOOK_CODE, observe)
            self.uc.hook_add(UC_HOOK_MEM_WRITE, store)

    try:
        probe.Replay = ObservedReplay
        observation = probe.capture()
    finally:
        probe.Replay = original_replay
    if not observation['reached_target_return'] or len(compared) != 1:
        raise AssertionError({'stop': observation['stop'], 'comparisons': len(compared)})
    return {'matched': True, 'projection': compared[0],
            'snapshot_metadata_sha256': observation['snapshot_metadata_sha256'],
            'snapshot_pages_sha256': observation['snapshot_pages_sha256'],
            'snapshot_provenance': observation['snapshot_provenance'],
            'execution_profile': observation['execution_profile'],
            'producer_sources': observation['producer_sources'],
            'original_instructions': observation['instructions']}


if __name__ == '__main__':
    print(json.dumps(capture(), sort_keys=True))
