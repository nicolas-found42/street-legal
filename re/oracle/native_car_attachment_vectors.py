#!/usr/bin/env python3
"""Original458c00 attachment/scalar commands and complete raw-memory results.

Allocator, resource access/resolution and VM callbacks are declared collaborators;
all command parsing, constraints, list/reference mutations, skeleton allocation,
destructors, x87 geometry and CRT rand execute original code. Scratch contains
caller text, VM cells, resource vtable thunks and the modeled CRT thread block.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

from oracle import Oracle, CW_D3D9, HEAP_BASE

ROOT = Path(__file__).resolve().parents[2]
DEFAULT = ROOT / 're/out/boot/native-car-command-attachments-v1.json'


def word(f):
    return struct.unpack('<I', struct.pack('<f', f))[0]


class Fixture:
    def __init__(self, label):
        self.label = label
        self.o = o = Oracle()
        o.hook_alloc()
        self.events, self.payloads, self.ids, self.cells = [], {}, {}, []
        self.dyno, self.query_resource = None, 0
        self.k, self.body = o.alloc(0x2e98), o.alloc(0x212c)
        self.part = o.alloc(0x300)
        self.slots, self.nodes = o.alloc(0x68 * 3), [o.alloc(0xb0) for _ in range(3)]
        self.wheels, self.bars = o.alloc(0x2b4 * 3), o.alloc(0xc20)
        o.write_u32(self.body + 8, 0xcafe)
        o.write_u32(self.body + 0x78, self.slots)
        o.write_u32(self.body + 0x13e4, self.wheels)
        o.write_u32(self.body + 0x1fbc, self.bars)
        o.write_u32(self.body + 0x70, 0x731295e0)
        o.write_u32(self.body + 0x1dbc, word(1.25))
        for p in [self.k + 0x1154, self.body + 0xd4, self.body + 0x1ddc,
                  self.body + 0x205c, self.body + 0x2078, self.body + 0x2094,
                  self.body + 0x20b0] + [self.wheels + i * 0x2b4 + 0x294 for i in range(3)]:
            self.list(p)
        for i in range(3):
            o.write_u32(self.nodes[i] + 4, self.nodes[i + 1] if i < 2 else self.k + 0x1164)
            o.write_u32(self.nodes[i] + 8, self.nodes[i - 1] if i else self.k + 0x1158)
            o.write_u32(self.nodes[i] + 0x48, 100 + i)
            o.write_u32s(self.slots + i * 0x68 + 0x2c, [word((i + j + 1) / 8) for j in range(3)])
            o.write_u32s(self.slots + i * 0x68 + 0x38, [word(1 if j in [0, 5, 10] else 0) for j in range(12)])
        o.write_u32(0x3000100c, 0x30002000)
        o.write_u32(0x30001014, 0x30002010)
        self.external_words = [[0x3000100c, 0x30002000], [0x30001014, 0x30002010]]
        def prepare(oc, args):
            self.events.append(['prepare', oc.reg('ecx'), args[0]])
            return 0
        def payload(oc, args):
            self.events.append(['payload', oc.reg('ecx')])
            return self.payloads.get(oc.reg('ecx'), 0)
        def minimum(oc, args):
            self.events.append(['minimum', oc.reg('ecx'), args[0]])
            return 0
        def resolve(oc, args):
            self.events.append(['resolve', *args])
            return self.ids.get(args[0], 0)
        def vm_class(oc, args):
            self.events.append(['vm_class', oc.read_cstr(args[0]).decode()])
            return 0x30000100
        def vm_object(oc, args):
            self.events.append(['vm_object', oc.reg('ecx'), *args])
            return 0
        def vm_field(oc, args):
            cell = 0x30000800 + 16 * len(self.cells)
            self.cells.append(cell)
            self.events.append(['vm_field', oc.reg('ecx'), oc.read_cstr(args[0]).decode(), cell])
            return cell
        def script(oc, args):
            sp = oc.reg('esp')
            words = [oc.read_u32(sp + 12)]
            for i in range(3, 100):
                w = oc.read_u32(sp + 4 + i * 4)
                words.append(w)
                if w == 0x4f: break
            else: raise RuntimeError('unterminated script arguments')
            self.events.append(['script', oc.read_u32(sp + 4), oc.read_cstr(oc.read_u32(sp + 8)).decode(), words])
            return 0
        def native_field(oc, args):
            self.events.append(['vm_reference_native', args[0], oc.read_cstr(args[1]).decode(), oc.read_cstr(args[2]).decode()])
            return 0 if self.dyno is None else 0x30000400
        def query(oc, args):
            self.events.append(['query', args[0], args[1]])
            if self.query_resource:
                self.bind(args[2], self.query_resource)
            return int(bool(self.query_resource))
        o.hook(0x5447d0, prepare, 'thiscall', 3)
        o.hook(0x30002000, payload, 'thiscall', 1)
        o.hook(0x30002010, minimum, 'thiscall', 1)
        o.hook(0x536820, resolve, 'thiscall', 3)
        o.hook(0x411600, vm_class, 'thiscall', 1)
        o.hook(0x407da0, vm_object, 'thiscall', 2)
        o.hook(0x408800, vm_field, 'thiscall', 1)
        o.hook(0x425a90, script, 'cdecl', 0)
        o.hook(0x42a690, native_field, 'cdecl', 3)
        o.hook(0x4044e0, lambda oc,a: 1, 'thiscall', 1)
        o.hook(0x42ab50, lambda oc,a: self.dyno, 'cdecl', 2)
        o.hook(0x426470, query, 'thiscall', 3)
        # Execute the real CRT rand/srand; only _getptd is a platform seam.
        o.hook(0x5dd8d3, lambda oc,a: 0x3000f000, 'cdecl', 0)
        o.call(0x5d73fb, [0x12345678])

    def list(self, p):
        self.o.write_u32s(p, [0x5f09a8, 0x5f09ac, p + 16, 0, 0x5f09ac, 0, p + 4])

    def bind(self, link, resource):
        o = self.o
        head = o.read_u32(resource + 0x48)
        if head: o.write_u32(head, link)
        o.write_u32s(link, [0, head, o.read_u32(resource + 0x50), resource])
        o.write_u32(resource + 0x48, link)

    def resource(self, id, data):
        resource = self.o.alloc(0xd8)
        self.o.write_u32(resource, 0x30001000)
        self.o.write_u32(resource + 0x4c, 1)
        self.o.write_u32(resource + 0x50, id)
        self.o.write_u32(resource + 0x54, 0xabc12340)
        self.payloads[resource], self.ids[id] = data, resource
        return resource

    def physics(self, id):
        o = self.o
        data, entity, relative = o.alloc(0x100), o.alloc(0x240), o.alloc(0x60)
        o.write_u32(data + 0x84, entity)
        o.write_u32(entity + 0x40, id)
        o.write_u32(entity + 0x1c, relative)
        for b in [0, 0xd4]:
            o.write_u32s(entity + b + 0x64, [word(1/8), word(1/4), word(-1/8)])
            o.write_u32s(entity + b + 0xa4, [word(1 if i in [0,5,10] else 0) for i in range(12)])
        o.write_u32s(relative + 4, [word(1/8), word(-1/4), word(1/2)])
        o.write_u32s(relative + 0x10, [0, 0, 0, word(1)])
        o.write_u32s(relative + 0x20, [word(1 if i in [0,5,10] else 0) for i in range(12)])
        return self.resource(id, data), entity

    def capture(self, commands, native_events=()):
        o = self.o
        before_ptr = o.heap_ptr
        before = o.read_u32s(HEAP_BASE, (before_ptr - HEAP_BASE + 3) // 4)
        self.events.clear()
        outputs = []
        for command in commands:
            p = 0x30010000
            o.write(p, command.encode() + b'\0')
            result = o.call(0x458c00, [self.body, 0, 0x10, p], convention='thiscall', this=self.k, fpu_cw=CW_D3D9)
            outputs.append(result.eax)
        event_results=[]
        for event,argument in native_events:
            result=o.call(0x458c00,[self.body,0,event,argument],convention='thiscall',this=self.k,fpu_cw=CW_D3D9)
            event_results.append(result.eax)
        return dict(label=self.label, k=self.k, body=self.body, heap_before=before_ptr, heap_after=o.heap_ptr,
                    before=before, after=o.read_u32s(HEAP_BASE, (o.heap_ptr - HEAP_BASE + 3)//4),
                    external_words=self.external_words, payloads=self.payloads, ids=self.ids,
                    dyno=self.dyno, query_resource=self.query_resource, commands=commands, results=outputs,
                    events=self.events, cells=[[c,o.read_u32(c+8)] for c in self.cells],
                    native_events=list(native_events),event_results=event_results,
                    rng_before=0x12345678, rng_after=o.read_u32(0x3000f014))


def capture():
    rows = []
    f = Fixture('scalar-controls')
    rows.append(f.capture(['unknown_command', 'brake', 'idle', 'team -17', 'sethorn -7', 'sethorn 11'] +
                         [f'transmission {i}' for i in range(8)] +
                         [f'{verb} {value}' for verb in ['abs','asr','esp','difflock','steerhelp','damage_multiplier'] for value in ['0','-2','0.125','3.25']]))
    f = Fixture('mount-slot-lifecycle')
    rows.append(f.capture([f'add_mslot {f.slots+i*0x68} {500+i}' for i in range(3)] +
                         [f'rem_mslot {f.slots+0x68} 501', f'rem_mslot {f.slots} 999',f'rem_mslot {f.slots} 500',f'rem_mslot {f.slots+2*0x68} 502']))
    f = Fixture('wing-lifecycle')
    entity = f.o.alloc(0x100)
    rows.append(f.capture([f'add_wing {i} {entity}' for i in [17,23,17]] +
                         [f'rem_wing {i} {entity}' for i in [17,99,23,17]]))
    f = Fixture('rollbar-lifecycle')
    rows.append(f.capture(['add_rollbar 1 2 10000 500']*9 + ['rem_rollbar 1 2 10000 500']*9 + ['add_rollbar','rem_rollbar']))
    f=Fixture('rollbar-unordered-float')
    f.o.write_u32(f.bars+0x44,1);f.o.write_u32s(f.bars+0x48,[1,2,0x7fc01234,word(500)])
    rows.append(f.capture(['rem_rollbar 1 2 10000 500']))
    f=Fixture('rollbar-integer-sign-bit')
    f.o.write_u32(f.bars+0x44,1);f.o.write_u32s(f.bars+0x48,[0x80000000,0,word(10000),word(500)])
    rows.append(f.capture(['rem_rollbar 0 0 10000 500']))
    f=Fixture('light-invalid-reference')
    rows.append(f.capture(['add_light 0 8',f'add_light {f.part} 8','rem_light 0 8']))
    f=Fixture('event-fallthrough')
    rows.append(f.capture(['unknown_command'],[(0x80,0),(0x80,4),(0x80,0xffffffff),(0x200,0),(0x20,0),(0x10000,0),(0,0),(0x4000000,0)]))
    for flags in [0,8,16,32,40,64,72,127]:
        f = Fixture(f'light-{flags}')
        resource = f.resource(1001, 0)
        f.bind(f.part, resource)
        rows.append(f.capture([f'add_light {f.part} {flags}', f'add_light {f.part} {flags}',
                               f'rem_light 99999 {flags}', f'rem_light 1001 {flags & 8}',
                               f'rem_light 1001 {flags}', f'rem_light 1001 {flags}']))
    for wheel in [-1,2]:
        f = Fixture(f'linked-{wheel}')
        first, second = f.o.alloc(0x30), f.o.alloc(0x30)
        a, ae = f.physics(1001); b, be = f.physics(1002)
        f.bind(first+0x18,a);f.bind(second+0x18,b)
        render = f.o.alloc(0x190)
        f.o.call(0x5404d0, convention='thiscall',this=render)
        f.resource(2001,render)
        rows.append(f.capture([f'add_linked 2001 {wheel} 0 {second}', f'add_linked 2001 {wheel} {first} {second}',
                               f'add_linked 2001 {wheel} {first} {second}',f'rem_linked 999 {wheel}',
                               f'rem_linked 2001 {wheel}',f'rem_linked 2001 {wheel}']))
    f=Fixture('linked-unresolved-id')
    first,second=f.o.alloc(0x30),f.o.alloc(0x30)
    a,ae=f.physics(1001);b,be=f.physics(1002)
    f.bind(first+0x18,a);f.bind(second+0x18,b)
    rows.append(f.capture([f'add_linked 999 -1 {first} {second}','rem_linked 999 -1','rem_linked 0 -1']))
    for mode in ['no-dynamics','record-missing-physics','record-live-physics','engine-none','engine-zero','engine-native']:
        f = Fixture(f'part-{mode}')
        o = f.o
        data, part_body = o.alloc(0x100), o.alloc(0x300)
        o.write_u32(data+0x4c,part_body)
        resource = f.resource(3001,data)
        f.bind(f.part,resource)
        f.bind(f.slots,resource)
        o.write_u32(f.k+0x1150,1);o.write_u32(f.k+0x115c,f.nodes[0]);o.write_u32(f.nodes[0]+4,f.k+0x1164)
        if mode.startswith('record'):
            definition=o.alloc(0x40)
            o.write_u32s(definition,[word(1/8),word(1/4),word(1/2),word(1),0,0,0,word(1),0,0,0,word(1/2),0,word(2),word(3),0])
            o.write_u32(f.nodes[0]+0xac,definition)
        if mode=='record-live-physics':
            ar,ae=f.physics(4001);br,be=f.physics(4002);pr,pe=f.physics(4003)
            f.bind(f.body+0x2c,ar);f.bind(part_body+0x1c,br)
            o.write_u32(be+0x20,pe)
        if mode.startswith('engine'):
            o.write_u32(f.nodes[0]+0x48,0x191)
            o.write_u32(data+0x50,0x30000300)
            f.dyno={'engine-none':None,'engine-zero':0,'engine-native':0x30000500}[mode]
        rows.append(f.capture([f'add_part {f.part}',f'rem_part {f.part}']))
    for selected in range(3):
        f = Fixture(f'dynamics-compact-{selected}')
        o = f.o
        o.write_u32(f.part+8,3001)
        o.write_u32(f.body+0x14e8,3)
        o.write_u32(f.k+0x1150,3)
        shared=o.alloc(0x100)
        for i in range(3):
            record=f.body+0x14ec+i*0x8c
            entity=o.alloc(0x100)
            o.write_u32(entity+0x40,3001 if i==selected else 7000+i)
            o.write_u32s(record,[0x3e000000+i*0x100+j for j in range(0x8c//4)])
            o.write_u32s(record+0x30,[0,0,0,0])
            o.call(0x45fb10,[shared],convention='thiscall',this=record+0x30)
            o.call(0x45fb10,[entity],convention='thiscall',this=record+0x38)
            o.write_u32(f.slots+i*0x68+0x28,record)
        rows.append(f.capture([f'rem_part {f.part}']))
    for mounted in [False,True]:
        for dyno in [None,0,0x30000500]:
            f=Fixture(f'callback-dyno-{int(mounted)}-{dyno}')
            o=f.o
            own_data=o.alloc(0x100);o.write_u32(own_data+0x50,0x30000300)
            f.bind(f.body,f.resource(5001,own_data))
            part_data=o.alloc(0x100);part_body=o.alloc(0x300)
            o.write_u32(part_data+0x4c,part_body);o.write_u32(part_data+0x50,0x30000300)
            resource=f.resource(3001,part_data);f.bind(f.part,resource);f.bind(f.slots,resource)
            f.dyno=dyno
            if mounted:
                o.write_u32(f.k+0x1150,1);o.write_u32(f.k+0x115c,f.nodes[0])
                o.write_u32(f.nodes[0]+4,f.k+0x1164);o.write_u32(f.nodes[0]+0x48,0x191)
                query,entity=f.physics(6001);f.query_resource=query
            rows.append(f.capture([f'add_part {f.part}',f'rem_part {f.part}']))
    for signs in range(8):
        f=Fixture(f'wheel-signed-zero-{signs}')
        o=f.o
        part_data,part_body,template_data,part_k = o.alloc(0x100),o.alloc(0x300),o.alloc(0x100),o.alloc(0x1200)
        runtime_slots,template_nodes=o.alloc(0x68*2),[o.alloc(0xb0),o.alloc(0xb0)]
        f.list(part_k+0x1154)
        o.write_u32(part_k+0x1150,2);o.write_u32(part_k+0x115c,template_nodes[0])
        o.write_u32(template_nodes[0]+4,template_nodes[1]);o.write_u32(template_nodes[1]+4,part_k+0x1164)
        o.write_u32(template_nodes[1]+0x4c,0x100000)
        o.write_u32(template_data+0xc,part_k)
        o.write_u32(part_data+0x44,f.resource(3002,template_data));o.write_u32(part_data+0x4c,part_body)
        o.write_u32(part_body+0x78,runtime_slots);o.write_u32(part_body+0xd0,103)
        o.write_u32s(runtime_slots+0x68+0x2c,[word(1/8),word(-1/4),word(3/8)])
        resource=f.resource(3001,part_data);f.bind(f.part,resource)
        wheel=f.wheels+2*0x2b4
        ar,ae=f.physics(4001);pr,pe=f.physics(4002);wr,we=f.physics(4003)
        f.bind(f.body+0x2c,ar);o.write_u32(ae+0x20,pe)
        physics=o.alloc(0x100);o.write_u32(physics+0x7c,we);o.write_u32(physics+0x30,word(1/4))
        o.write_u32(wheel+0x30,physics)
        o.write_u32s(wheel+0x78,[word(3/4),word(-1/2),word(1/8)])
        o.write_u32s(wheel+0x90,[word(1 if i in [0,5,10] else 0) for i in range(12)])
        config=f.k+0x1f44+400
        o.write_u32s(config,[word(1),word(-1),word(2)])
        o.write_u32s(config+0xc,[0x80000000 if signs&(1<<i) else 0 for i in range(3)])
        o.write_u32(config+0x38,word(1/4));o.write_u32(config+0x3c,word(1/2))
        rows.append(f.capture([f'add_part {f.part}',f'rem_part {f.part}']))
    original = Oracle()
    constants = [0x5e73cc,0x5f08f0,0x5f09d0,0x5f0c20,0x5f0d50,0x5f0dbc,0x5f0cd4,0x5f0cf8,
                 0x5f09b4,0x5f09ac,0x5f0fc8,0x5f1184,0x5f1188,0x5f1190,0x5f37dc]
    return dict(schema='slr-car-attachments-v1', original_sha256=hashlib.sha256((ROOT/'reference/raw/StreetLegal_Redline.bak.exe').read_bytes()).hexdigest(),
                fpu_cw=CW_D3D9, image_words=[[a,original.read_u32(a)] for a in constants],
                collaborators=['allocator','resource minimum/prepare/payload/resolve','VM class/object/field/script/native-reference','query','CRT _getptd'],
                comparison='entire constructor heap + VM field stores + RNG state + ordered collaborator calls + results', rows=rows)


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('action',choices=['gen','verify'])
    ap.add_argument('--out',type=Path,default=DEFAULT)
    args=ap.parse_args()
    if not args.out.resolve().is_relative_to(ROOT/'re/out'):ap.error('output must stay under ignored re/out')
    result=capture()
    if args.action=='verify':
        if json.loads(args.out.read_text())!=json.loads(json.dumps(result)):raise SystemExit('original replay differs')
    else:
        if args.out.exists():raise SystemExit('preserve existing capture; choose a fresh --out or verify')
        args.out.parent.mkdir(parents=True,exist_ok=True);args.out.write_text(json.dumps(result)+'\n')
    print(f'{len(result["rows"])} original command sequences: {args.action}')


if __name__=='__main__':main()
