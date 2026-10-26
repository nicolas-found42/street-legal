#!/usr/bin/env python3
"""Original build606 cell identities, ownership transfers and shared pools.

Executes original value/variable allocators, copy/deref/push/release helpers,
Thread.run and frame destruction over declared synthetic nodes/cells. Scenario
supplies allocator, clock, logging and class-position collaborators; no native
gameplay or stock callback completion is claimed. Raw addresses remain local
evidence. Equal payloads never stand in for cell identity.
"""
from pathlib import Path
import hashlib
import json
import sys
import zlib
import base64

sys.path.insert(0, str(Path(__file__).resolve().parent))
from thread_vectors import Scenario, ScriptFatal, CW, TC, RUN, THREAD_CTOR
from oracle import DEFAULT_EXE, HEAP_BASE
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_WRITE


def pool_state(o, global_address):
    pool = o.read_u32(global_address)
    words = o.read_u32s(pool, 5)
    free = []
    pointer = words[4]
    while pointer:
        if pointer in free or len(free) >= 4096:
            raise RuntimeError('invalid observed freelist')
        free.append(pointer)
        pointer = o.read_u32(pointer)
    blocks = o.read_u32s(words[0], words[1]) if words[1] else []
    stride, cells = (12, 512) if global_address == 0x62f130 else (16, 84000)
    def slot(pointer):
        for index, block in enumerate(blocks):
            if block <= pointer < block + cells * stride and (pointer - block) % stride == 0:
                return index * cells + (pointer - block) // stride
        raise RuntimeError('observed cell is outside its real pool blocks')
    return {'address': pool, 'words': words, 'blocks': blocks, 'free': free,
            'free_slots': [slot(pointer) for pointer in free]}


def cell_identity(s, pointer):
    o = s.o
    kind = 'variable' if o.read_u32(pointer) == 0x5e7344 else 'value'
    pool = pool_state(o, 0x62f134 if kind == 'variable' else 0x62f130)
    stride, cells = (16, 84000) if kind == 'variable' else (12, 512)
    for index, block in enumerate(pool['blocks']):
        if block <= pointer < block + cells * stride:
            return {'kind': kind, 'slot': index * cells + (pointer - block) // stride}
    raise RuntimeError('live cell identity is outside its real pool blocks')


def state(s):
    o = s.o
    frame = o.read_u32(s.thread + 0x20)
    stack = s._container(frame + 0x1c) if frame else []
    values = pool_state(o, 0x62f130)
    value_cells = [{'slot': slot,
                    'words': o.read_u32s(values['blocks'][slot // 512] + (slot % 512) * 12, 3)}
                   for slot in range(values['words'][2])]
    return {'thread': s.thread, 'frame': frame, 'value_cells': value_cells,
            'stack': [{'address': cell, 'words': o.read_u32s(cell, 3),
                       'identity': cell_identity(s, cell)} for cell in stack],
            'values': pool_state(o, 0x62f130), 'variables': pool_state(o, 0x62f134),
            'live': o.read_i32(0x62f138)}


def program(s, nodes):
    if not s.node_span:
        s.add_frame(0, {'nodes': nodes, 'entry': 0, 'locals': [], 'stack': []})
    else:
        o = s.o
        code = o.alloc(len(nodes) * 8)
        for i, (op, line, imm) in enumerate(nodes):
            o.write_u16(code + 8*i, op)
            o.write_u16(code + 8*i + 2, line)
            o.write_u32(code + 8*i + 4, imm)
        s.node_span[0] = (code, len(nodes))
        o.write_u32(s.frames[0] + 0x2c, code)
    s.o.write_u32(s.thread + 0x2c, 0)
    s.finish_setup()


def attach_observation(s):
    o = s.o
    events = []
    pending = []
    targets = {0x423c00: 'allocate_value', 0x4050b0: 'allocate_variable',
               0x423e30: 'copy', 0x405000: 'release_value',
               0x405040: 'release_variable', 0x41f7d0: 'deref_pop',
               0x423ba0: 'push', 0x407ba0: 'destroy_frame'}

    def observe(uc, pc, size, _):
        while pending and pc == pending[-1]['return_pc'] and o.reg('esp') > pending[-1]['esp']:
            entry = pending.pop()
            events.append({'return_from': entry['name'], 'eax': o.reg('eax'),
                           'values': pool_state(o, 0x62f130),
                           'variables': pool_state(o, 0x62f134),
                           'live': o.read_i32(0x62f138)})
        if pc in targets:
            esp = o.reg('esp')
            event = {'enter': targets[pc], 'pc': pc, 'this': o.reg('ecx'),
                     'args': o.read_u32s(esp + 4, 2),
                     'values': pool_state(o, 0x62f130),
                     'variables': pool_state(o, 0x62f134), 'live': o.read_i32(0x62f138)}
            events.append(event)
            pending.append({'name': targets[pc], 'esp': esp, 'return_pc': o.read_u32(esp)})
        if pc == 0x42108b:
            # The running thread comes from the actual interpreter EBX.
            thread = o.reg('ebx')
            frame = o.read_u32(thread + 0x20)
            nodes = o.read_u32(thread + 0x24)
            events.append({'opcode': o.read_u16(nodes), 'immediate': o.read_u32(nodes + 4),
                           'thread': thread, 'stack': s._container(frame + 0x1c),
                           'values': pool_state(o, 0x62f130),
                           'variables': pool_state(o, 0x62f134), 'live': o.read_i32(0x62f138)})

    def writes(uc, access, address, size, value, _):
        for global_address, kind, release_pc in [(0x62f130, 'value', 0x405019),
                                                 (0x62f134, 'variable', 0x40505f)]:
            pool = o.read_u32(global_address)
            if size != 4 or address not in (pool + 8, pool + 16):
                continue
            if address == pool + 8:
                old = o.read_u32(address)
                if value != old + 1:
                    raise RuntimeError('unexpected handed-out counter mutation')
                event = {'pool_event': 'allocate', 'kind': kind, 'slot': old, 'origin': 'pooled'}
            else:
                old = o.read_u32(address)
                if o.reg('eip') == release_pc:
                    blocks = pool_state(o, global_address)['blocks']
                    stride, cells = (12,512) if kind == 'value' else (16,84000)
                    slot = next(i*cells+(value-block)//stride for i,block in enumerate(blocks)
                                if block <= value < block + cells*stride)
                    event = {'pool_event': 'release', 'kind': kind, 'slot': slot}
                else:
                    if old == 0 or value != o.read_u32(old):
                        raise RuntimeError('unexpected freelist pop mutation')
                    blocks = pool_state(o, global_address)['blocks']
                    stride, cells = (12,512) if kind == 'value' else (16,84000)
                    slot = next(i*cells+(old-block)//stride for i,block in enumerate(blocks)
                                if block <= old < block + cells*stride)
                    event = {'pool_event': 'allocate', 'kind': kind, 'slot': slot, 'origin': 'freelist'}
            events.append({**event, 'pc': o.reg('eip')})

    o.uc.hook_add(UC_HOOK_CODE, observe)
    o.uc.hook_add(UC_HOOK_MEM_WRITE, writes)
    return events


def new_value(s, payload=7):
    o = s.o
    pointer = o.call(0x423c00, [o.read_u32(0x62f130)], convention='fastcall', fpu_cw=CW).eax
    source = o.alloc(12)
    o.write_u32s(source, [0x5e734c, TC['I'], payload])
    o.call(0x423e30, [source], convention='thiscall', this=pointer, fpu_cw=CW)
    return pointer


def new_variable(s, payload=7, name=0):
    o = s.o
    pointer = o.call(0x4050b0, [o.read_u32(0x62f134)], convention='fastcall', fpu_cw=CW).eax
    # Declared synthetic variable initialization. Allocation, dereferencing,
    # use, destruction and freelist links still execute original code.
    o.write_u32s(pointer, [0x5e7344, TC['I'], payload, name])
    o.write_u32(0x62f138, o.read_u32(0x62f138) + 1)
    return pointer


def push(s, pointer):
    s.o.call(0x423ba0, [pointer], convention='thiscall', this=s.thread, fpu_cw=CW)


def run(s):
    s.o.call(RUN, [0.0], convention='thiscall', this=s.thread, fpu_cw=CW)


def heap(s):
    data = s.o.read(HEAP_BASE, s.o.heap_ptr - HEAP_BASE)
    return {'address': HEAP_BASE, 'length': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
            'zlib_base64': base64.b64encode(zlib.compress(data)).decode()}


def release_choice(release_first):
    s = Scenario()
    program(s, [[0x10, 1, 0]])
    events = attach_observation(s)
    first, second = new_value(s), new_value(s)
    push(s, first)
    push(s, second)
    before = heap(s)
    popped = [s.o.call(0x41f7d0, [s.thread], convention='fastcall', fpu_cw=CW).eax for _ in range(2)]
    released, survivor = (first, second) if release_first else (second, first)
    s.o.call(0x405000, [1], convention='thiscall', this=released, fpu_cw=CW)
    free_before_reuse = state(s)
    push(s, survivor)
    replacement = new_value(s)
    push(s, replacement)
    assert first != second and replacement == released
    return {'name': 'release_first' if release_first else 'release_second',
            'first': first, 'second': second, 'popped': popped, 'released': released,
            'survivor': survivor, 'replacement': replacement,
            'before': before, 'after': heap(s), 'free_before_reuse': free_before_reuse,
            'final': state(s), 'events': events}


def opcode_case(name, nodes, variable=False, destroy=False, fatal=False):
    s = Scenario()
    program(s, nodes)
    events = attach_observation(s)
    if variable:
        variables = [new_variable(s), new_variable(s)]
        s._container_set(s.frames[0] + 0xc, variables)
    before = heap(s)
    outcome = 'returned'
    try:
        run(s)
    except ScriptFatal:
        if not fatal:
            raise
        outcome = 'fatal'
    if fatal and outcome != 'fatal':
        raise RuntimeError('expected original fatal boundary was not reached')
    after_run = state(s)
    if destroy:
        s.o.call(0x407d10, [1], convention='thiscall', this=s.frames[0], fpu_cw=CW)
    return {'name': name, 'nodes': nodes, 'outcome': outcome, 'synthetic_variable': variable,
            'synthetic_variable_count': 2 if variable else 0,
            'destroyed': destroy, 'before': before, 'after': heap(s),
            'after_run': after_run, 'final': state(s), 'events': events}


def no_frame_push():
    s = Scenario()
    events = attach_observation(s)
    cell = new_value(s, 19)
    s.o.write_u32(s.thread + 0x20, 0)
    before = heap(s)
    error = None
    try:
        push(s, cell)
    except Exception as exception:
        error = str(exception)
    if error is None or s.o.reg('eip') != 0x423ba4:
        raise RuntimeError('missing original null-frame fault')
    return {'name': 'push_without_frame', 'outcome': 'fault', 'stop': error,
            'fault_pc': s.o.reg('eip'), 'before': before, 'after': heap(s),
            'final': state(s), 'events': events}


def cross_thread():
    s = Scenario()
    program(s, [[0x1027, 1, 7], [0x10, 1, 0]])
    o = s.o
    other = object.__new__(Scenario)
    other.o, other.jvm = o, s.jvm
    other.thread = o.alloc(0x100)
    name = o.alloc(16)
    o.write(name, b'other\0')
    o.call(THREAD_CTOR, [s.jvm, 0, 0, name], convention='thiscall', this=other.thread, fpu_cw=CW)
    other.frames = [o.read_u32(other.thread + 0x20)]
    other.ctx, other.ctx_q = s.ctx, s.ctx_q
    other.node_span, other.local_ptrs = [], []
    program(other, [[0x1027, 1, 7], [0x10, 1, 0]])
    events = attach_observation(s)
    first_var = new_variable(s)
    s._container_set(s.frames[0] + 0xc, [first_var])
    before = heap(s)
    run(s)
    first = state(s)
    pointer = first['stack'][0]['address']
    program(s, [[0x18, 1, 0], [0x10, 1, 0]])
    run(s)
    free = state(s)
    o.call(0x407d10, [1], convention='thiscall', this=s.frames[0], fpu_cw=CW)
    second_var = new_variable(other)
    other._container_set(other.frames[0] + 0xc, [second_var])
    run(other)
    second = state(other)
    assert second['stack'][0]['address'] == pointer
    assert first_var == second_var
    return {'name': 'cross_thread_shared_pool', 'before': before, 'after': heap(s),
            'first': first, 'released': free, 'second': second,
            'first_variable': first_var, 'second_variable': second_var, 'events': events}


def capture():
    stop = [0x10, 1, 0]
    rows = [release_choice(True), release_choice(False)]
    cases = [
        ('equal_distinct_literals', [[0x1027,1,7],[0x1027,1,7],stop], False, False),
        ('deref_repush_same_cell', [[0x1001,1,0],[0x28,1,0],[0x28,1,0],stop], True, False),
        ('equal_distinct_deref_copies', [[0x1001,1,0],[0x28,1,0],[0x1001,1,0],[0x28,1,0],stop], True, False),
        ('binary_release_then_reuse', [[0x1027,1,7],[0x1027,1,7],[0x1007,1,0x10],[0x1027,1,7],stop], False, False),
        ('frame_destroy_values_and_borrowed_variable', [[0x1027,1,7],[0x1001,1,0],[0x1027,1,7],stop], True, True),
        ('ternary_false_distinct_equal_cells', [[0x1027,1,0],[0x1027,1,7],[0x1027,1,7],[0x1007,1,0],stop], False, False),
        ('ternary_true_distinct_equal_cells', [[0x1027,1,1],[0x1027,1,7],[0x1027,1,7],[0x1007,1,0],stop], False, False),
        ('switch_miss_preserves_switch_owner', [[0x1027,1,7],[0x1027,1,8],[0x1017,1,1],stop], False, False),
        ('assignment_value_fatal_retains_cell', [[0x1027,1,1],[0x1027,1,2],[0x1007,1,0x23],stop], False, False, True),
    ]
    rows += [opcode_case(*case) for case in cases]
    rows.append(cross_thread())
    rows.append(no_frame_push())
    sources = [Path(__file__), Path(__file__).with_name('thread_vectors.py'), Path(__file__).with_name('oracle.py')]
    return {'schema': 1, 'scope': __doc__, 'fpu_cw': CW,
            'exe_sha256': hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
            'producer_sources': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},
            'rows': rows}


if __name__ == '__main__':
    print(json.dumps(capture(), sort_keys=True))
