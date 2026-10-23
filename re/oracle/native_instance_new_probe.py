#!/usr/bin/env python3
"""Execute original53a5e0 native-only cases, with declared controlled collaborators.

Template script class at data+10 is zero; nonzero script allocation/constructor
branches are explicitly unclaimed. These are native-only full caller cases.

The type1 instance and optional parent are constructed by original53e6e0
before the measured call. The manager's ID/node allocation, resource virtuals,
class constructor/postcreate, callback registration, spatial operations and
allocator are explicit boundaries. Real53a5e0 instructions, name copy, and
intrusive weak-reference operations execute. Composed cases execute actual
4797d0 (player) or45fdf0 (cursor), with their resource/config/registry/platform
collaborators declared below. This is neither stock startup nor full factory,
VM, device or retained upper-caller ABI acceptance.

Every callback records the complete fixed caller arena and observed stack
window, all general registers, EFLAGS and FPU status. The full original thread
stack is seeded; every measured stack store must fit the captured window.
Uncaptured stack bytes therefore remain the declared initial fill. Failed
attempts are preserved separately; no fixture result enters production code.
"""
import argparse
import hashlib
import itertools
import json
import platform
import importlib.metadata
from pathlib import Path

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, x86_const as X
import unicorn
from oracle import Oracle, CW_D3D9, HEAP_BASE, STACK_TOP, STACK_SIZE

ROOT = Path(__file__).resolve().parents[2]
EXE = ROOT / 'stock/StreetLegal_Redline.bak.exe'
HEAP_BYTES = 0x5000
STACK_LOW = STACK_TOP - 0x1000
GLOBALS = [0x765f60, 0x63c86c, 0x7686f0, 0x7686f8,
           0x63c870, 0x63c874, 0x6439c0, 0x610a58, 0x610a5c, 0x6495a4]
MODES = ['ready', 'request-zero', 'allocation-zero', 'template-pending',
         'template-no-payload', 'no-class', 'constructor-null',
         'registration-replaces-body', 'registration-clears-class',
         'registration-rebinds-reference', 'registration-retires-reference',
         'relocate-once', 'collision-present', 'preexisting-references',
         'parent-unloaded', 'null-name', 'null-parameters']
PROFILE = {
    'name': '53a5e0-native-only-complete-caller-v1',
    'entry': 0x53a5e0,
    'endpoint': 'ordinary return to oracle driver sentinel',
    'comparison': ['complete caller arena', 'complete observed stack window',
                   'ordered collaborator snapshots', 'all GPR snapshots',
                   'declared globals', 'return value', 'callee-saved registers',
                   'ESP delta', 'FPU control word and depth'],
    'stack': {'seed_entire_thread_stack': True, 'low': STACK_LOW,
              'high': STACK_TOP, 'reject_measured_stores_outside_window': True,
              'uncaptured_bytes': 'unchanged initial fill; enforced by write hook'},
    'input_pages': 'initial original-image/scratch pages actually read by measured instructions; content-addressed bytes, with declared mutable globals overlaid',
    'driver': 'declared original entry context with seven nonzero GPR inputs; EBP applied before first measured instruction',
    'imports': 'Oracle Win32 zero-return thunks configured; this profile rejects any reached import',
    'controlled': ['536ed0 manager ID allocation', '536890/536900 node allocation',
                   '544b90 resource load transition', 'resource minimum/maximum/payload', '419860 template access',
                   '5447d0 template access status', 'class constructor and postcreate',
                   '427210 callback registration', '4988a0/498b10/48ae40 spatial operations',
                   '54f560/54f5b0 allocator'],
    'composition': {'player': ['original477bc0 setup', 'original4797d0 measured constructor'],
                    'cursor': ['original45fd00 setup', 'original45fdf0 measured constructor'],
                    'controlled': ['5383f0 name', '538440 resource lookup',
                                   '546070/545fc0 resource binding', '536820 resolution',
                                   '416b00 registrar', '426170/4261f0/426100 Config reads',
                                   '48d3d0 subscription', 'postcreate virtual']},
    'unclaimed': ['upper caller/interpreter frames', 'whole resource manager',
                  'whole factory or stock boot', 'nonzero script class allocation/constructor branch', 'VM execution', 'physical devices',
                  'EFLAGS parity', 'raw x87 register parity'],
}


class CaptureFailure(RuntimeError):
    def __init__(self, error, details):
        super().__init__(str(error))
        self.details = details


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def bind(o, link, raw):
    old = o.read_u32(link + 12)
    if old == raw:
        return
    if old:
        previous, following = o.read_u32s(link, 2)
        o.write_u32(previous + 4 if previous else old + 0x48, following)
        if following:
            o.write_u32(following, previous)
    o.write_u32s(link, [0] * 4)
    if raw:
        following = o.read_u32(raw + 0x48)
        if following:
            o.write_u32(following, link)
        o.write_u32s(link, [0, following, o.read_u32(raw + 0x50), raw])
        o.write_u32(raw + 0x48, link)


def inventory():
    rows = [dict(mode=mode, fill=fill, parent=parent, composition='controlled')
            for mode, fill, parent in itertools.product(MODES, [0, 0xa5], [False, True])
            if mode != 'no-class' and (mode != 'parent-unloaded' or parent)]
    rows.extend(dict(mode='ready', fill=fill, parent=parent, composition=composition)
                for composition, fill, parent in
                itertools.product(['player', 'cursor'], [0, 0xa5], [False, True]))
    return rows


def capture_case(case):
    o = Oracle(str(EXE), fpu_cw=CW_D3D9)
    arena = o.alloc(HEAP_BYTES)
    o.write(arena, bytes([case['fill']]) * HEAP_BYTES)
    p = lambda offset: arena + offset
    layout = dict(root=p(0), request=p(0x20), template=p(0x100), node=p(0x300),
                  parent=p(0x500), template_payload=p(0x700), klass=p(0x800),
                  class_vtable=p(0x900), name_allocation=p(0xa00), body=p(0x1000),
                  resources=[p(0x4000 + i * 0x100) for i in range(4)],
                  aliases=[p(0x4500), p(0x4510)],
                  heap_address=arena, heap_words=HEAP_BYTES // 4,
                  stack_address=STACK_LOW, stack_words=(STACK_TOP - STACK_LOW) // 4)
    root, request, template, node, parent, data, k, vt = (
        layout[key] for key in ['root', 'request', 'template', 'node', 'parent',
                               'template_payload', 'klass', 'class_vtable'])
    descriptor = o.scratch(24)
    o.write_u32s(descriptor, [1, 0xffff007d, 0x21, 0, 0, 0])
    setup_receipts = []
    result = o.call(0x53e6e0, [descriptor, 0, 0], convention='thiscall', this=node)
    setup_receipts.append(dict(entry=0x53e6e0, this=node, args=[descriptor, 0, 0],
                               result=result.eax, instructions=result.icount,
                               fpu_cw=result.fpcw, esp_delta=result.esp_delta))
    o.write_u32s(descriptor, [1, 4, 1, 0, 0, 0])
    result = o.call(0x53e6e0, [descriptor, 0, 0], convention='thiscall', this=parent)
    setup_receipts.append(dict(entry=0x53e6e0, this=parent, args=[descriptor, 0, 0],
                               result=result.eax, instructions=result.icount,
                               fpu_cw=result.fpcw, esp_delta=result.esp_delta))
    o.write_u32(parent + 0x64, 0 if case['mode'] == 'parent-unloaded' else 1)
    o.write_u32s(parent + 0x6c, [0x3e800000, 0x3f400000])
    o.write_u32(parent + 0x68, 0x3f000000)
    o.write_u32(node + 0x14, parent if case['parent'] else 0)
    o.write_u32s(root, [0, 0, 4, parent if case['parent'] else 0])
    requested = 0 if case['mode'] == 'request-zero' else 0xffff007e
    o.write_u32s(request, [0, 0, requested, template])
    o.write_u32(template + 0x48, 0)
    o.write_u32(template + 0x4c, 8)
    o.write_u32(template + 0x50, 0xffff007e)
    o.write_u32s(data, [0x5f389c, 0, 0, k, 0, 0, 0x1234, 0x5678])
    o.write_u32(k, vt)
    o.write_u32(vt + 0xc, 0x300f0050)
    o.write_u32(vt + 0x40, 0x300f0060)
    for i, raw in enumerate(layout['resources']):
        o.write_u32(raw + 0x48, 0)
        o.write_u32(raw + 0x4c, 6 if i < 2 else 0xe)
        o.write_u32(raw + 0x50, 0xffff2001 + i)
    for alias in layout['aliases']:
        o.write_u32s(alias, [0] * 4)
    if case['mode'] == 'preexisting-references':
        bind(o, layout['aliases'][0], node)
        bind(o, layout['aliases'][1], template)
    o.write_u32(0x618d48, 0x30080000)
    o.write_u32(0x63641c, 0x30090000)
    o.write_u32(0x63c86c, 0)
    o.write_u32(0x7686f0, 7)
    o.write_u32(0x7686f8, layout['resources'][3])
    o.write_u32(0x6495a4, 0x33445566)
    events, contexts, stores, external_writes = [], [], [], []
    measured = False

    def registers():
        return {r: o.reg(r) for r in ['eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp', 'esp']}

    def state():
        return dict(heap=o.read_u32s(arena, HEAP_BYTES // 4),
                    stack=o.read_u32s(STACK_LOW, (STACK_TOP - STACK_LOW) // 4),
                    registers=registers(), globals={str(a): o.read_u32(a) for a in GLOBALS},
                    eflags=o.uc.reg_read(X.UC_X86_REG_EFLAGS),
                    fpu_cw=o.uc.reg_read(X.UC_X86_REG_FPCW),
                    fpu_sw=o.uc.reg_read(X.UC_X86_REG_FPSW), fpu_depth=o._fpu_depth())

    def hook(address, name, convention, nargs, callback):
        def invoke(oc, args):
            if measured:
                events.append(dict(name=name, address=address, args=list(args),
                                   return_pc=oc.read_u32(oc.reg('esp')), **state()))
            return callback(oc, args)
        o.hook(address, invoke, convention, nargs)

    def allocate(oc, args):
        if args[0] == 0x20:
            return layout['name_allocation']
        expected = 0x2510 if case['composition'] == 'player' else 0x1bc
        if args[0] == expected:
            return layout['body']
        raise RuntimeError(f'undeclared allocator request {args[0]:#x}')

    hook(0x54f560, 'allocate', 'cdecl', 1, allocate)
    hook(0x54f5b0, 'free', 'cdecl', 1, lambda *_: 0)
    hook(0x536ed0, 'allocate-id', 'thiscall', 0,
         lambda *_: 0 if case['mode'] == 'allocation-zero' else 0xffff007d)

    def create(oc, args):
        expected = 6 if case['parent'] else 5
        if len(args) != expected or args[0] != 0xffff007d:
            raise RuntimeError('unexpected original node factory inputs')
        return node

    hook(0x536900 if case['parent'] else 0x536890, 'create-node',
         'thiscall', 6 if case['parent'] else 5, create)
    hook(0x419860, 'template-access', 'thiscall', 4,
         lambda *_: 0 if case['mode'] in ['template-pending', 'template-no-payload'] else data)
    hook(0x544b90, 'mark-loaded', 'thiscall', 1, lambda *_: 0)
    hook(0x5447d0, 'prepare', 'thiscall', 3,
         lambda oc, _: 0x80000001 if oc.reg('ecx') == template
         and case['mode'] == 'template-pending' else 0)
    node_vt, template_vt, parent_vt = [o.scratch(0x80) for _ in range(3)]
    for raw, table, get in [(node, node_vt, 0x300f0000),
                            (template, template_vt, 0x300f0010),
                            (parent, parent_vt, 0x300f0020)]:
        o.write_u32(raw, table)
        o.write_u32(table + 0xc, get)
        o.write_u32(table + 0x14, 0x300f0030)
        o.write_u32(table + 0x18, 0x300f0040)
    hook(0x300f0000, 'instance-payload', 'thiscall', 1, lambda *_: node + 0xd8)
    hook(0x300f0010, 'template-payload', 'thiscall', 1,
         lambda *_: 0 if case['mode'] == 'template-no-payload' else data)
    hook(0x300f0020, 'parent-payload', 'thiscall', 1, lambda *_: parent + 0xd8)
    hook(0x300f0030, 'minimum', 'thiscall', 1,
         lambda oc, args: oc.write_u32(oc.reg('ecx') + 0x6c, args[0]))
    hook(0x300f0040, 'maximum', 'thiscall', 1,
         lambda oc, args: oc.write_u32(oc.reg('ecx') + 0x70, args[0]))
    hook(0x300f0050, 'constructor', 'thiscall', 2,
         lambda *_: 0 if case['mode'] == 'constructor-null' else layout['body'])
    hook(0x300f0060, 'postcreate', 'thiscall', 1, lambda *_: 0)

    def register(oc, args):
        if case['mode'] == 'registration-replaces-body':
            oc.write_u32(node + 0xd8 + 0x4c, layout['body'] + 4)
        if case['mode'] == 'registration-clears-class':
            oc.write_u32(data + 12, 0)
        if case['mode'] == 'registration-rebinds-reference':
            bind(oc, args[0], layout['resources'][0])
        if case['mode'] == 'registration-retires-reference':
            bind(oc, args[0], 0)
        return 0

    hook(0x427210, 'register-callbacks', 'thiscall', 1, register)
    hook(0x4988a0, 'aggregate-bounds', 'cdecl', 1, lambda *_: 0)
    relocated = False
    def relocate(oc, args):
        nonlocal relocated
        result = case['mode'] == 'relocate-once' and not relocated
        relocated = True
        return int(result)
    hook(0x498b10, 'relocate', 'cdecl', 1, relocate)
    hook(0x48ae40, 'collision-owner', 'thiscall', 0,
         lambda *_: layout['resources'][3] if case['mode'] == 'collision-present' else 0)
    hook(0x4b3ee0, 'collision-relocate', 'thiscall', 2, lambda *_: 0)
    if case['mode'] == 'no-class':
        o.write_u32(data + 12, 0)

    if case['composition'] != 'controlled':
        # Actual native template construction is setup, outside the measured
        # caller. Allocate its K at its declared destination, then run the real
        # native constructor only through53a8c3's original virtual call.
        template_bytes = 0x50 if case['composition'] == 'player' else 0x28
        def allocation_composed(oc, args):
            return k if args[0] == template_bytes else allocate(oc, args)
        hook(0x54f560, 'allocate', 'cdecl', 1, allocation_composed)
        hook(0x5383f0, 'resource-name', 'thiscall', 1, lambda *_: 0x610a64)
        hook(0x538440, 'lookup', 'thiscall', 2,
             lambda oc, args: {0x612624: 0xffff2001, 0x612630: 0xffff2002,
                              0x6128ac: 0xffff2003}.get(args[0], 0xffff2003))
        def bind_resource(oc, args):
            raw = next((raw for raw in [node] + layout['resources']
                        if oc.read_u32(raw + 0x50) == args[0]), 0)
            bind(oc, oc.reg('ecx'), raw)
            return 0
        hook(0x546070, 'bind-typed', 'thiscall', 3, bind_resource)
        hook(0x545fc0, 'bind', 'thiscall', 1, bind_resource)
        hook(0x536820, 'resolve', 'thiscall', 3, lambda *_: layout['resources'][2])
        hook(0x416b00, 'register-native', 'thiscall', 4, lambda *_: 0)
        hook(0x426170, 'integer-setting', 'cdecl', 1, lambda *_: 0)
        hook(0x4261f0, 'float-setting', 'cdecl', 1, lambda *_: 0.5)
        hook(0x426100, 'option-setting', 'cdecl', 2, lambda *_: 1)
        def subscribe(oc, args):
            raw = oc.read_u32(args[0] + 12)
            if raw != node:
                raise RuntimeError(f'composed constructor owner differs: {raw:#x}')
            oc.write_u32(raw + 0x148, oc.read_u32(raw + 0x148) | args[1])
            return 0
        hook(0x48d3d0, 'subscribe', 'cdecl', 3, subscribe)
        entry = 0x477bc0 if case['composition'] == 'player' else 0x45fd00
        result = o.call(entry, [0, template])
        setup_receipts.append(dict(entry=entry, args=[0, template], result=result.eax,
                                   instructions=result.icount, fpu_cw=result.fpcw,
                                   esp_delta=result.esp_delta))
        if result.eax != k:
            raise RuntimeError('actual native template returned unexpected allocation')
        # Postcreate behavior is separately declared; original class constructor
        # is untouched. Clone its table only to replace the+40 endpoint.
        image_vtable = o.read_u32(k)
        o.write(vt, o.read(image_vtable, 0x80))
        o.write_u32(vt + 0x40, 0x300f0060)
        o.write_u32(k, vt)

    text = o.scratch(64)
    o.write(text, b'7 -3 0\0')
    name = o.scratch(64)
    o.write(name, b'composed-source-caller\0')
    args = [root, request,
            0 if case['mode'] == 'null-parameters' else text,
            0 if case['mode'] == 'null-name' else name]
    o.write(STACK_TOP - STACK_SIZE, bytes([case['fill']]) * STACK_SIZE)
    before = state()
    entry_registers = {r: 0x12345000 + i * 0x111 for i, r in
                       enumerate(['eax', 'ebx', 'ecx', 'edx', 'esi', 'edi', 'ebp'])}
    # Capture initial bytes before execution. Emulated read hooks select the
    # pages actually needed, including writable-image constants and mixed
    # pages whose mutable words are independently declared in GLOBALS.
    initial_pages = {}
    for low, high in [(o.base, o.base + o.image_size),
                      (0x30000000, (o.scratch_ptr + 0xfff) & ~0xfff)]:
        for address in range(low, high, 0x1000):
            initial_pages[address] = o.read(address, 0x1000)
    read_pages = set()

    def observe(uc, pc, size, _):
        if measured and pc == 0x53a5e0:
            o.set_reg('ebp', entry_registers['ebp'])
        if measured and pc in {0x53a5e0, 0x53a85e, 0x53a8c3, 0x53a8c6,
                               0x53aa0c, 0x53aa11, 0x53aa74}:
            contexts.append(dict(pc=pc, **state()))

    def stack_write(uc, access, address, size, value, _):
        if not measured:
            return
        if STACK_TOP - STACK_SIZE <= address < STACK_TOP:
            if address < STACK_LOW or address + size > STACK_TOP:
                raise RuntimeError(f'measured stack store outside captured window {address:#x}')
            stores.append(dict(pc=o.reg('eip'), address=address, size=size, value=value))
        elif not arena <= address < arena + HEAP_BYTES:
            if not any(a <= address and address + size <= a + 4 for a in GLOBALS):
                raise RuntimeError(f'undeclared measured write outside caller heap/stack/globals {address:#x}+{size}')
            external_writes.append(dict(pc=o.reg('eip'), address=address, size=size, value=value))

    def memory_read(uc, access, address, size, value, _):
        if not measured or arena <= address < arena + HEAP_BYTES:
            return
        if STACK_TOP - STACK_SIZE <= address < STACK_TOP:
            if address < STACK_LOW or address + size > STACK_TOP:
                raise RuntimeError(f'measured stack read outside captured window {address:#x}+{size}')
            return
        for page in range(address & ~0xfff, (address + size + 0xfff) & ~0xfff, 0x1000):
            if page not in initial_pages:
                raise RuntimeError(f'undeclared measured input page {page:#x}')
            read_pages.add(page)

    o.uc.hook_add(UC_HOOK_CODE, observe)
    o.uc.hook_add(UC_HOOK_MEM_WRITE, stack_write)
    o.uc.hook_add(UC_HOOK_MEM_READ, memory_read)
    measured = True
    try:
        result = o.call(0x53a5e0, args, regs=entry_registers)
    except Exception as error:
        raise CaptureFailure(error, dict(case=case, layout=layout, args=args,
            before=before, stopped=state(), events=events, contexts=contexts,
            stack_stores=stores, external_writes=external_writes)) from error
    measured = False
    if result.esp_delta != 0 or result.fpcw != CW_D3D9 or result.fpu_top_depth != 0:
        raise RuntimeError('unexpected complete original caller ABI/FPU endpoint')
    if o.import_log:
        raise RuntimeError(f'undeclared reached import: {o.import_log}')
    inputs = [dict(address=address, bytes=initial_pages[address].hex(),
                   sha256=hashlib.sha256(initial_pages[address]).hexdigest(),
                   mutable_addresses=[a for a in GLOBALS if address <= a < address + 0x1000])
              for address in sorted(read_pages)]
    return dict(case=case, layout=layout, args=args, entry_registers=entry_registers,
                setup_executions=setup_receipts,
                input_pages=inputs, before=before, after=state(),
                events=events, contexts=contexts, stack_stores=stores,
                external_writes=external_writes, import_calls=o.import_log,
                result=result.eax, instructions=result.icount, esp_delta=result.esp_delta,
                fpu_cw=result.fpcw, fpu_depth=result.fpu_top_depth)


def capture():
    rows = []
    for case in inventory():
        try:
            rows.append(capture_case(case))
        except Exception as error:
            raise CaptureFailure(error, dict(completed_rows=rows, failed_case=case,
                details=getattr(error, 'details', None))) from error
    # Deduplicate identical local page payloads without discarding page VAs or
    # per-case inventories. Consumers verify both ID and payload hashes.
    pages = {}
    for row in rows:
        refs = []
        for page in row.pop('input_pages'):
            ident = f"{page['address']:08x}-{page['sha256']}"
            pages.setdefault(ident, page)
            refs.append(ident)
        row['input_page_ids'] = refs
    return dict(schema=1, scope=__doc__, profile=PROFILE,
                profile_sha256=hashlib.sha256(json.dumps(PROFILE, sort_keys=True,
                    separators=(',', ':')).encode()).hexdigest(),
                exe_sha256=digest(EXE), producer_sha256=digest(__file__),
                oracle_sha256=digest(Path(__file__).with_name('oracle.py')),
                execution=dict(oracle='Unicorn original build606 executable',
                               unicorn=unicorn.__version__, python=platform.python_version(),
                               packages={name: importlib.metadata.version(name)
                                         for name in ['unicorn', 'capstone', 'pefile']},
                               host=platform.platform()),
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
        result = capture()
    except Exception as error:
        failure = args.out.with_suffix(args.out.suffix + '.failure.json')
        with failure.open('x') as stream:
            json.dump(dict(error=str(error), details=getattr(error, 'details', None),
                           producer_sha256=digest(__file__),
                           exe_sha256=digest(EXE)), stream, sort_keys=True)
            stream.write('\n')
        raise
    with args.out.open('x') as stream:
        json.dump(result, stream, sort_keys=True)
        stream.write('\n')
    print(len(result['rows']), 'complete original53a5e0 cases; controlled/composed scope as declared')
