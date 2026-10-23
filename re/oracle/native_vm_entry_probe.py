#!/usr/bin/env python3
"""Observe genuine original installation -> stock addpart -> getScriptInstance.

Validates and resumes install-part-queue using Replay's original registers,
stack/pages and FPU state. The original installation, callback425a90/4255e0,
unchanged stock script, Thread.run, native41fbc0 and486f30 execute. The declared
endpoint is the first486f30 return or first425a90 return; neither completes the
outer installation.
Replay's malloc/free bump allocator is supplied; realloc(null,n) allocates and
non-null realloc is admitted only at source-inspected 4-byte vector callsites,
copying min(old-capacity,new-size) bytes from live state. Other realloc callers
fail closed. No VM, script, class lookup or native-result hooks are installed.
Passive stack/register observations are verification artifacts, never runtime
answers. This does not implement a production VM/native CPU-context bridge.
"""
from pathlib import Path
import hashlib
import json
import sys
import platform
import importlib.metadata

sys.path.insert(0, str(Path(__file__).resolve().parent))
from snapshot_replay import Replay, SNAPSHOTS
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from capstone import Cs, CS_ARCH_X86, CS_MODE_32


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def capture(name='install-part-queue', endpoint='native-return'):
    if endpoint not in ('native-return', 'callback-return'):
        raise ValueError(f'unsupported endpoint {endpoint}')
    o = Replay(name)
    contexts, stores, allocations, wrapper_instructions = [], [], [], []
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    target = None
    callback_target = None
    dirty_pages = set()
    reached = False
    entry_esp = o.meta['registers']['esp']
    ranges = o.meta['comparison_policy']['thread_stack_ranges']

    def words_at(pointer, count):
        return {'address': pointer, 'words': o.read_u32s(pointer, count)}

    def cstring(pointer):
        value = bytearray()
        for offset in range(1024):
            byte = o.read(pointer + offset, 1)[0]
            if byte == 0:
                return value.decode('ascii')
            value.append(byte)
        raise RuntimeError('unterminated observed native record string')

    def native_objects(info):
        """Read the live CallInfo graph; do not synthesize or write VM state."""
        fields = o.read_u32s(info, 4)
        vector = o.read_u32s(fields[2], 4)
        if vector[2] > 1024:
            raise RuntimeError('observed argument vector exceeds probe bound')
        slots = o.read_u32s(vector[1], vector[2])
        record = o.read_u32s(fields[3], 6)
        thread = o.read_u32s(fields[1], 14)
        return {'call_info': words_at(info, 4),
                'arguments': words_at(fields[2], 4),
                'argument_pointers': slots,
                'argument_cells': [words_at(slot, 3) for slot in slots],
                'native_record': words_at(fields[3], 6),
                'class': cstring(record[3]), 'method': cstring(record[4]),
                'thread': words_at(fields[1], 14),
                'frame': words_at(thread[8], 16)}

    def realloc(oc, args):
        old, size = args
        caller = oc.read_u32(oc.reg('esp'))
        count = 0
        if old:
            # Capacity/data/count vector layout; registers proved by original
            #403930/405180/41a570/41fa30/41fb40/41e170 instructions at these CALLs.
            register = {0x403965: 'edi', 0x4051ae: 'esi', 0x41fa73: 'esi',
                        0x41fb81: 'esi', 0x41e259: 'esi', 0x41a5a7: 'esi'}.get(caller)
            if register is None:
                raise RuntimeError(f'unsupported non-null realloc caller {caller:#x}')
            container = oc.reg(register)
            if old != oc.read_u32(container + 4):
                raise RuntimeError('realloc data pointer differs from declared vector')
            count = min(oc.read_u32(container) * 4, size)
        pointer = oc.alloc(size) if size else 0
        if count:
            oc.write(pointer, oc.read(old, count))
            dirty_pages.update(range(pointer & ~4095, (pointer + count + 4095) & ~4095, 4096))
        allocations.append({'caller': caller, 'old': old, 'size': size,
                            'copied_bytes': count, 'new': pointer})
        return pointer

    o.hook(0x54f580, realloc, 'cdecl', 2)
    watched = {o.meta['function'], 0x425a90, 0x4255e0, 0x420ff0,
               0x41fbc0, 0x486f30, 0x42ab50}

    def observe(uc, pc, size, _):
        nonlocal target, callback_target, reached
        if 0x486f30 <= pc <= 0x487011:
            instruction = next(decoder.disasm(o.read(pc, size), pc))
            wrapper_instructions.append({'pc': pc, 'bytes': instruction.bytes.hex(),
                                         'instruction': f'{instruction.mnemonic} {instruction.op_str}',
                                         'esp': o.reg('esp'), 'eax': o.reg('eax'),
                                         'ecx': o.reg('ecx'), 'edx': o.reg('edx')})
        if pc in watched:
            esp = o.reg('esp')
            contexts.append({'pc': pc, 'esp': esp, 'instruction_index': o.icount,
                             'return_pc': o.read_u32(esp),
                             'registers': {r: o.reg(r) for r in
                                           ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp')},
                             'stack_address': esp - 64,
                             'stack_words': o.read_u32s(esp - 64, 320)})
            if pc == 0x425a90 and callback_target is None:
                callback_target = (o.read_u32(esp), esp)
            if pc == 0x486f30:
                if target is None:
                    target = (o.read_u32(esp), esp)
                contexts[-1]['objects'] = native_objects(o.read_u32(esp + 4))
                # Every byte remains local evidence. The final-writer ledger
                # distinguishes current-frame stores from inherited scratch.
                first, last = esp - 64, entry_esp + 256
                contexts[-1]['retained_stack'] = {
                    'address': first, 'bytes': o.read(first, last - first).hex(),
                    'final_writers': final_writers(stores, first, last)}
        requested = target if endpoint == 'native-return' else callback_target
        if requested and pc == requested[0] and o.reg('esp') > requested[1]:
            reached = True
            contexts.append({'pc': pc, 'esp': o.reg('esp'), 'return_eax': o.reg('eax'),
                             'registers': {r: o.reg(r) for r in
                                           ('eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp')}})
            uc.emu_stop()

    def writes(uc, access, address, size, value, _):
        dirty_pages.add(address & ~4095)
        dirty_pages.add((address + size - 1) & ~4095)
        if any(low <= address and address + size <= high for low, high in ranges):
            stores.append({'pc': o.reg('eip'), 'address': address,
                           'size': size, 'value': value, 'instruction_index': o.icount})

    o.uc.hook_add(UC_HOOK_CODE, observe)
    o.uc.hook_add(UC_HOOK_MEM_WRITE, writes)
    stop = None
    try:
        o.resume(limit=2_000_000)
    except Exception as error:
        stop = str(error)
    requested = target if endpoint == 'native-return' else callback_target
    expected_endpoint = reached and stop == f'stopped early at eip={requested[0]:#x}'
    # Preserve complete changed pages, including all retained native scratch.
    # No stack, heap-word or padding exclusion is applied here. These bytes
    # are evidence for a future production comparison, never runtime inputs.
    changed_pages = []
    for address in sorted(dirty_pages | set(o.pages)):
        before = o.pages.get(address, bytes(4096))
        after = o.read(address, 4096)
        if before != after:
            changed_pages.append({'address': address, 'bytes': after.hex(),
                                  'before_sha256': hashlib.sha256(before).hexdigest()})
    sources = [Path(__file__), Path(__file__).with_name('snapshot_replay.py'),
               Path(__file__).with_name('oracle.py'), Path(__file__).with_name('headless_snapshot.py')]
    return {'schema': 2, 'scope': __doc__, 'endpoint': endpoint,
            'reached_target_return': expected_endpoint,
            'outer_install_complete': False, 'stop': stop, 'instructions': o.icount,
            'snapshot': name, 'snapshot_metadata_sha256': digest(SNAPSHOTS / f'{name}.json'),
            'snapshot_pages_sha256': digest(SNAPSHOTS / f'{name}.pages'),
            'snapshot_provenance': o.meta['provenance'], 'fpu_cw': o.meta['fpu_cw'],
            'comparison_policy': o.meta['comparison_policy'],
            'execution_profile': {
                'oracle': 'Unicorn original executable from validated headless snapshot',
                'host': platform.platform(), 'python': platform.python_version(),
                'packages': {name: importlib.metadata.version(name)
                             for name in ('unicorn', 'capstone', 'pefile')},
                'vm_script_native_result_hooks': [],
                'boundary': 'Replay malloc/free; declared vector-only realloc; declared endpoint'},
            'producer_sources': {p.name: digest(p) for p in sources},
            'entry_esp': entry_esp, 'contexts': contexts, 'stack_stores': stores,
            'final_heap_ptr': o.heap_ptr, 'changed_pages': changed_pages,
            'wrapper_instructions': wrapper_instructions,
            'reallocations': allocations, 'import_calls': o.import_log}


def final_writers(stores, first, last):
    """Byte-granular last observed writer, coalesced by original store index.

    A missing byte is inherited snapshot memory, not an assumed zero. Retain
    store indices so callers can inspect overwritten byte/word writes exactly.
    """
    writers = {}
    for index, store in enumerate(stores):
        for byte in range(max(first, store['address']),
                          min(last, store['address'] + store['size'])):
            writers[byte] = index
    runs = []
    for address in range(first, last):
        index = writers.get(address)
        if runs and runs[-1]['store_index'] == index:
            runs[-1]['length'] += 1
        else:
            runs.append({'address': address, 'length': 1, 'store_index': index})
    return runs


if __name__ == '__main__':
    observation = capture(sys.argv[1] if len(sys.argv) > 1 else 'install-part-queue',
                          sys.argv[2] if len(sys.argv) > 2 else 'native-return')
    print(json.dumps(observation, sort_keys=True))
    sys.exit(0 if observation['reached_target_return'] else 1)
