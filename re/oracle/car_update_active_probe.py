#!/usr/bin/env python3
"""Complete original448430 active-script runs with controlled VM/resource calls.

Original control flow, part fold43c520, slot lookup4707d0, weak-link edits,
curves, projection and4479d0 execute. Field lookup, synchronous callback,
resource prepare/get-data, sound bind and parameter clone are declared seams.
Finite dyadic cases avoid claiming arbitrary PC-sensitive float coverage.
"""
import hashlib
import json
import random
import struct
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9


def word(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def run(case, exe=DEFAULT_EXE, cw=CW_D3D9):
    o = Oracle(exe, fpu_cw=cw)
    o.hook_alloc()
    sizes = dict(template=0x2e98, body=0x212c, parameters=0xc1c,
                 clone=0xc1c, resource=0x15c, wheels=0x2b4*3, vt=0x40,
                 torque=0x200, forward=16, reverse=16, old_a=32, old_b=32,
                 nodes=0x30*11, parts=0x100*11, resources=0x15c*11,
                 descriptors=0x60*4, slots=0x68*3, weak=16*4)
    a = {k: o.alloc(n) for k,n in sizes.items()}
    end = o.heap_ptr
    for key in ['body','parameters','clone','wheels']:
        o.write(a[key], bytes([0xa5])*sizes[key])
    wheels = [0,1,3][case%3]
    o.write_u32(a['template']+0x1f40, wheels)
    o.write_u32(a['template']+0x2e48, a['parameters'])
    o.write(a['template']+0x2e50, bytes(range(0x44)))
    o.write_u32(a['body']+0x1fbc, a['parameters' if case%2 else 'clone'])
    o.write_u32(a['body']+0xc, a['resource'])
    o.write_u32(a['body']+0x13e4, a['wheels'])
    o.write_u32(a['body']+0xdc, a['nodes'])
    o.write_u32(a['body']+0x78, a['slots'])
    for raw in [a['resource']]+[a['resources']+i*0x15c for i in range(11)]:
        o.write_u32(raw, a['vt'])
        o.write_u32(raw+0x4c, 1)
        o.write_u32(raw+0x64, 1)
        o.write_u32(raw+0x50, 0x1234+raw-a['resource'])
    o.write_u32(a['resource']+0x128, 73)
    kinds=[5,6,7,8,9,10,12,11,4, (3<<16)|5]
    if case%4 == 0: kinds=[]
    for i,kind in enumerate(kinds):
        node=a['nodes']+i*0x30
        part=a['parts']+i*0x100
        res=a['resources']+i*0x15c
        o.write_u32(node+4,node+0x30)
        o.write_u32(node+0x18, res if not (case%5==0 and i==1) else 0)
        o.write_u32(res+0x124,part)
        o.write_u32(part+0xb0,kind)
        o.write_u32(part+0xc,res)
        o.write_f32(part+0xb8,0.5)
        o.write_f32(part+0xbc,0.25)
    # Install prior weak links with a predecessor/successor in half the cases.
    for i,off in enumerate([0x202c,0x203c,0x204c]):
        for j in range(4): o.write_u32(a['body']+off+j*4,0)
        if case%2:
            link=a['body']+off
            target=a['resources']+10*0x15c
            prev=a['weak']+i*16
            nxt=prev+4
            o.write_u32(link,prev)
            o.write_u32(link+4,nxt)
            o.write_u32(link+8,0x777)
            o.write_u32(link+12,target)
            o.write_u32(prev+4,link)
            o.write_u32(nxt,link)
    for i in range(3):
        desc=a['descriptors']+i*0x60
        o.write_u32(desc+4,desc+0x60)
        o.write_u32(desc+0x48,0x65+i)
        slot=a['slots']+i*0x68
        if (case+i)%3:
            o.write_u32(slot+8,0x1234)
            o.write_u32(slot+12,a['resources']+i*0x15c)
        wheel=a['wheels']+i*0x2b4
        for off in range(0x1d8,0x1e8,4): o.write_u32(wheel+off,0)
        if case%2:
            o.write_u32(wheel+0x1e4,a['resources']+10*0x15c)
            o.write_u32(wheel+0x1e0,0x555)
        for off,v in [(0x64,8.),(0x68,16.)]: o.write_f32(wheel+off,v)
        o.write_f32(a['body']+0x1f14+i*4,0.5)
    o.write_u32(a['template']+0x115c,a['descriptors'] if case%7 else a['descriptors']+3*0x60)
    for off,v in [(0x1f24,[-1.,0.,1.5][case%3]),(0x1f28,2.),(0x1f2c,3.),
                  (0x1e58,8000.),(0x1e64,0.5),(0x1e38,1.),(0x1ef8,[0.,4.,32.][case%3])]:
        o.write_f32(a['body']+off,v)
    curve=case%4
    count=4 if case%6 else 0
    for key in ['parameters','clone']:
        p=a[key]
        o.write_u32(p, count if case%2 else 2)
        o.write_u32(p+4,a['old_a'])
        o.write_u32(p+8,2)
        o.write_u32(p+12,a['old_b'])
        o.write_u32(p+0xc18,0)  # callback below supplies the live value
    o.write_u32(a['torque']+0x1d0,500)
    o.write_u32(a['torque']+0x1d4,count)
    o.write_u32(a['torque']+0x1c0,0x76)
    o.write_u32(a['torque']+0x1dc,a['forward'] if curve>1 else 0)
    o.write_u32(a['torque']+0x1e0,a['reverse'] if curve>2 else 0)
    o.write_f32s(a['forward'],[0.,4.,8.,12.])
    o.write_f32s(a['reverse'],[-1.,-2.,-3.,-4.])
    rng=random.Random(606+case)
    floats={name:word(rng.choice([0.5,1.,2.,4.,8.,16.])) for name in [
        'engine_inertia','starter_torque','engine_friction_fwd','engine_friction_rev',
        'engine_rpm_idle','engine_mass','rearend_ratio','maxRPM','RPM_limit','ClutchF',
        'rpm_trans_fwd','rpm_trans_rev','sfx_starter_rpm','tank_nitro','consumption_nitro','C_drag','x','y','z']}
    ints=dict(suspend_update=0,gears=5,SFX_trans_fwd=10,SFX_trans_rev=11,SFX_ignition=12)
    refs=dict(ratio=74,drag_center=75)
    ratios=[word(i*0.5) for i in range([0,3,8,12][case%4])]
    calls=[]
    o.write_u32(a['vt']+0xc,0x300f0000)
    o.hook(0x300f0000,lambda oc,args: oc.reg('ecx')+0xd8,'thiscall',1)
    o.hook(0x5447d0,lambda oc,args: 0,'thiscall',3)
    o.hook(0x43e3e0,lambda oc,args: calls.append(['clone',oc.reg('ecx')]) or a['clone'],'thiscall',0)
    def field(oc,args,values,ty):
        name=oc.read(args[1],80).split(b'\0')[0].decode()
        calls.append(['field',args[0],name,ty])
        return values[name]
    o.hook(0x42a430,lambda oc,args:field(oc,args,ints,'I'),'cdecl',2)
    o.hook(0x42a560,lambda oc,args:struct.unpack('<f',struct.pack('<I',field(oc,args,floats,'F')))[0],'cdecl',2)
    o.hook(0x42a690,lambda oc,args:field(oc,args,refs,'R'),'cdecl',3)
    o.hook(0x42ab50,lambda oc,args:len(ratios),'cdecl',2)
    o.hook(0x42b040,lambda oc,args:struct.unpack('<f',struct.pack('<I',ratios[args[1]]))[0],'cdecl',2)
    o.hook(0x546070,lambda oc,args:calls.append(['sound',oc.reg('ecx'),args[0]]) or 0,'thiscall',3)
    callback_torque=a['torque'] if curve else 0
    def callback(oc,args):
        calls.append(['updatevariables'])
        target=oc.read_u32(a['body']+0x1fbc)
        oc.write_u32(target+0xc18,callback_torque)
        return 0
    o.hook(0x425a90,callback,'cdecl',4)
    before=o.read_u32s(a['template'],(end-a['template']+3)//4)
    r=o.call(0x448430,[a['body']],convention='thiscall',this=a['template'],fpu_cw=cw)
    assert r.fpu_top_depth==0
    return dict(case=case,addresses=a,heap_start=end,heap_end=o.heap_ptr,before=before,
                after=o.read_u32s(a['template'],(o.heap_ptr-a['template']+3)//4),
                callback_torque=callback_torque,floats=floats,ints=ints,refs=refs,ratios=ratios,calls=calls)


if __name__=='__main__':
    print(json.dumps(dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=[run(i) for i in range(48)]),sort_keys=True))
