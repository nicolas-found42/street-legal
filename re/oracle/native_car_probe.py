#!/usr/bin/env python3
"""Original44a472..44a776 scalar car runtime initialization.

Entry is after base46eae0 and initial resource binding; body/template/world
registers are declared inputs. No callee is substituted. Stop before parameter
attachment48b940. Clone-parameter43e3e0 is outside this bounded slice.
"""
import hashlib
import json
import struct
from pathlib import Path
from unicorn import UC_HOOK_CODE
from oracle import Oracle, DEFAULT_EXE, CW_D3D9, SENTINEL


def shell_rows():
    rows = []
    for fill in [0, 0xa5]:
        o = Oracle()
        body = o.alloc(0x212c)
        o.write(body, bytes([fill]) * 0x212c)
        def boundary(uc, address, size, user):
            if address == 0x44a288:
                o.set_reg('esi', body)
                o.set_reg('ebx', 0)
            elif address == 0x44a3fd:
                o.set_reg('eip', SENTINEL)
        o.uc.hook_add(UC_HOOK_CODE, boundary)
        o.call(0x44a288, [], fpu_cw=CW_D3D9)
        rows.append(dict(fill=fill, body=body, words=o.read_u32s(body,0x212c//4)))
    return rows


def wheel_storage_rows():
    rows=[]
    for count in [0,1,4,16]:
        o=Oracle();o.hook_alloc();template=o.alloc(0x2e98);body=o.alloc(0x212c)
        o.write_u32(template+0x1f40,count)
        def boundary(uc,address,size,user):
            if address==0x44a7de:
                o.set_reg('edi',template);o.set_reg('ebp',body);o.set_reg('ebx',0)
            elif address in [0x44a845,0x44b41f]:o.set_reg('eip',SENTINEL)
        o.uc.hook_add(UC_HOOK_CODE,boundary)
        o.call(0x44a7de,[],fpu_cw=CW_D3D9)
        pointer=o.read_u32(body+0x13e4)
        rows.append(dict(count=count,template=template,body=body,heap=o.heap_ptr,pointer=pointer,
                         words=o.read_u32s(pointer-4,1+count*(0x2b4//4)) if pointer else []))
    return rows

def wheel_tail_rows():
    rows=[]
    for index in [0,1]:
      for other in [-1,0,1]:
        o=Oracle();template=o.alloc(0x2e98);body=o.alloc(0x212c);wheels=o.alloc(0x568)
        o.write_u32(template+0x1f40,2);o.write_u32(body+0x13e4,wheels)
        end=template+0x1ffc+index*0xc8;wheel=wheels+index*0x2b4
        for offset in range(0,0x2e98,4):
            if offset not in [0x1f40]:o.write_u32(template+offset,0x3e000000+offset*15)
        o.write_u32(end-0x34,other&0xffffffff)
        o.write(wheel,bytes([0xa5])*0x2b4)
        def boundary(uc,address,size,user):
            if address==0x44b2a8:
                o.set_reg('ebx',end);o.set_reg('ebp',wheel);o.set_reg('edi',0)
                o.write_u32(o.reg('esp')+0x28,body)
            elif address==0x44b3e2:o.set_reg('eip',SENTINEL)
        o.uc.hook_add(UC_HOOK_CODE,boundary)
        o.call(0x44b2a8,[],fpu_cw=CW_D3D9)
        rows.append(dict(index=index,other=other,template=template,body=body,wheels=wheels,words=o.read_u32s(wheel,0x2b4//4)))
    return rows

def wheel_frame_rows():
    rows=[]
    for index in [0,1]:
      for angles in [[0.,0.,0.],[.1,-.2,.3]]:
        o=Oracle();template=o.alloc(0x2e98);body=o.alloc(0x212c);wheels=o.alloc(0x568)
        entity=o.alloc(0x218);physical=o.alloc(0xa0);definition=o.alloc(0xac);relative=o.alloc(0x50)
        o.write_u32(template+0x1f40,2);o.write_u32(body+0x13e4,wheels);o.write_u32(body+0x13bc,entity)
        o.write_u32(entity+0x5c,physical);o.write_u32(physical+0x78,definition);o.write_u32(entity+0x1c,relative)
        o.write_f32s(definition+0x40,[.23,-.41,.67]);o.write_f32s(relative+4,[.11,-.13,.17])
        end=template+0x1ffc+index*0xc8;wheel=wheels+index*0x2b4
        for offset in range(0xc8//4):o.write_u32(end-0xb8+offset*4,0x3e000000+offset*0x10001)
        o.write_f32s(end-0xac,angles);o.write(wheel,bytes([0xa5])*0x2b4)
        o.write_f32s(wheel+0x5c,[.93,.31]);anchor=[]
        def boundary(uc,address,size,user):
            if address==0x44ab58:
                o.set_reg('ebx',end);o.set_reg('ebp',wheel);o.set_reg('esi',body)
                o.write_u32(o.reg('esp')+0x38,entity)
            elif address==0x44ac71:
                anchor.extend(o.read_u32s(o.reg('esp')+0x4c,3));o.set_reg('eip',SENTINEL)
        o.uc.hook_add(UC_HOOK_CODE,boundary)
        o.call(0x44ab58,[],fpu_cw=CW_D3D9)
        rows.append(dict(index=index,angles=angles,template=template,body=body,wheels=wheels,entity=entity,physical=physical,definition=definition,relative=relative,words=o.read_u32s(wheel,0x2b4//4),anchor=anchor))
    return rows

def wheel_force_rows():
    rows=[]
    for gravity in [[0.,-9.81,0.],[.123,-.456,.789]]:
      for mass in [0.,100.,-1.]:
        o=Oracle();template=o.alloc(0x2e98);wheel=o.alloc(0x2b4);spring=o.alloc(0xec);world=o.alloc(0x400)
        o.write_u32(template+0x1f40,4);o.write_f32s(template+0x1ec4,[mass]);o.write_f32s(world+0x104,gravity);o.write_u32(0x617650,world)
        o.write_f32s(wheel+0x3c,[.17,2.]);o.write_u32(wheel+0x30,spring)
        def boundary(uc,address,size,user):
            if address==0x44ad54:
                o.set_reg('esi',wheel+0x30);o.set_reg('ebp',wheel);o.set_reg('edi',0);o.write_u32(o.reg('esp')+0x10,template)
            elif address==0x44adb0:o.set_reg('eip',SENTINEL)
        o.uc.hook_add(UC_HOOK_CODE,boundary);o.call(0x44ad54,[],fpu_cw=CW_D3D9)
        rows.append(dict(gravity=gravity,mass=mass,words=o.read_u32s(wheel+0x44,3)))
    return rows

def run():
    rows = []
    for fill in [0, 0xa5]:
        for mass in [0x3f800000, 0x42c80000, 0xbf800000, 0x00000000,
                     0x80000000, 0x7f800000, 0x00000001, 0x7fc12345]:
            for count in [0, 1, 4]:
                o = Oracle()
                template = o.alloc(0x2e98)
                body = o.alloc(0x212c)
                parameters = o.alloc(0xb00)
                world = o.alloc(0x400)
                o.write(body, bytes([fill]) * 0x212c)
                o.write_u32(template + 0x11dc, mass)
                o.write_u32(template + 0x11e0, 0x3ec12345)
                o.write_u32(template + 0x2e48, parameters)
                o.write_u32(template + 0x177c, count)
                o.write_u32(parameters + 0xa34, 0x41c80000)
                o.write_u32(parameters + 0xa3c, 0x3e800000)
                o.write_u32(world, 0x12345678)
                o.write_u32(0x617650, world)
                def boundary(uc, address, size, user):
                    if address == 0x44a472:
                        o.set_reg('ebp', body)
                        o.set_reg('edi', template)
                        o.set_reg('ebx', 0)
                    elif address == 0x44a776:
                        o.set_reg('eip', SENTINEL)
                o.uc.hook_add(UC_HOOK_CODE, boundary)
                o.call(0x44a472, [], fpu_cw=CW_D3D9)
                rows.append(dict(fill=fill, mass=mass, count=count,
                                 template=template, body=body,
                                 parameters=parameters, world=world,
                                 words=o.read_u32s(body, 0x212c//4)))
    return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                scope=__doc__, rows=rows, shell_rows=shell_rows(), wheel_storage=wheel_storage_rows(), wheel_tail=wheel_tail_rows(), wheel_frame=wheel_frame_rows(), wheel_force=wheel_force_rows())


if __name__ == '__main__':
    print(json.dumps(run(), sort_keys=True))
