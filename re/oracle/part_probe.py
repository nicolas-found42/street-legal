#!/usr/bin/env python3
"""Original Part scalar wrappers, with real marshal/load guards and raw body cells.

Native.ptr and resource virtual data/load are declared external boundaries.
No script return values are supplied; x87 arithmetic runs original instructions.
"""
import hashlib,json,struct
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from render_vectors import _cell,_frame
ROWS=[('getWear',0x469340,'()F'),('setWear',0x469420,'(F)F'),('setMaxWear',0x469500,'(F)V'),('getTear',0x4695d0,'()F'),('setTear',0x4696a0,'(F)F'),('getCar',0x4690a0,'()I'),('getWheelID',0x4691f0,'()I')]
def bits(f):return struct.unpack('<I',struct.pack('<f',f))[0]
def run(name,address,sig,arg,mode,slot):
 o=Oracle(fpu_cw=CW_D3D9);node=o.alloc(16);res=o.alloc(0x60);template=o.alloc(0x60);data=o.alloc(0x80);td=o.alloc(0x20);body=o.alloc(0x180);vt=o.alloc(0x20)
 for c,d in [(res,data),(template,td)]:o.write_u32(c,vt);o.write_u32(c+0x4c,1)
 o.write_u32(vt+0xc,0x300f0010);o.write_u32(node+0xc,0 if mode=='unbound' else res);o.write_u32(data+0x44,template);o.write_u32(data+0x4c,body)
 o.write_u32(body+0xb4,bits(.25));o.write_u32(body+0xb8,bits(.125));o.write_u32(body+0xbc,bits(2));o.write_u32(body+0xc8,0xffff1234);o.write_u32(body+0xd0,slot&0xffffffff)
 loads=[]
 o.hook(0x42ab50,lambda oc,a:0 if mode=='null' else node,'cdecl',2)
 def load(oc,a):
  loads.append([oc.reg('ecx'),a]);return 0x80000000 if mode=='load_failed' else 0
 o.hook(0x5447d0,load,'thiscall',3)
 o.hook(0x300f0010,lambda oc,a:data if oc.reg('ecx')==res else td,'thiscall',1)
 cells=([_cell(o,'F',arg)] if '(F)' in sig else [])+[_cell(o,'L',123)]
 r=o.call(address,[_frame(o,'java.game.parts.Part',name,sig,cells)],fpu_cw=CW_D3D9)
 result=bits(r.st0) if sig.endswith('F') else (r.eax if sig.endswith('I') else None)
 return dict(name=name,address=address,signature=sig,arg=arg,mode=mode,slot=slot,result=result,body=[o.read_u32(body+i) for i in range(0xb4,0xd4,4)],load_flags=[a[1][0] for a in loads])
def run_all():
 rows=[]
 for name,address,sig in ROWS:
  for mode in ['valid','null','unbound','load_failed']:
   rows.append(run(name,address,sig,bits(.7),mode,111))
  if name=='getWheelID':
   for slot in [0,100,101,110,399,400,401,-1]:rows.append(run(name,address,sig,0,'valid',slot))
  if name=='setMaxWear':
   for value in [0.,-2.,float('inf')]:rows.append(run(name,address,sig,bits(value),'valid',0))
 return dict(exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),scope=__doc__,rows=rows)

def slot_case(name, address, signature, args, initial_flags):
    o = Oracle(fpu_cw=CW_D3D9)
    node, res, template = o.alloc(16), o.alloc(0x60), o.alloc(0x60)
    data, td, body, vt = o.alloc(0x80), o.alloc(0x20), o.alloc(0x180), o.alloc(0x20)
    k, runtime = o.alloc(0x2e98), o.alloc(3 * 0x68)
    for c in [res, template]:
        o.write_u32(c, vt)
        o.write_u32(c + 0x4c, 1)
    o.write_u32(vt + 0xc, 0x300f0010)
    for address_, value in [(node + 0xc, res), (data + 0x44, template),
                            (data + 0x4c, body), (td + 0xc, k),
                            (body + 0x78, runtime), (data + 0x50, 0xabcde),
                            (body + 0xcc, res)]:
        o.write_u32(address_, value)
    descriptors = [o.alloc(0xc0) for _ in range(3)]
    tail = o.alloc(12)
    o.write_u32(k + 0x1150, 3)
    o.write_u32(k + 0x115c, descriptors[0])
    o.write_u32(k + 0x116c, descriptors[-1])
    for i, descriptor in enumerate(descriptors):
        o.write_u32(descriptor + 4, descriptors[i + 1] if i < 2 else tail)
        o.write_u32(descriptor + 0x48, [101, 201, 301][i])
        o.write_u32(runtime + i * 0x68 + 0x14, initial_flags[i])
    # Slot201 is attached to this same resource through its slot101. This
    # exercises the original reciprocal peer flag reset without fake callbacks.
    o.write_u32(runtime + 0x68 + 8, 0xffff1111)
    o.write_u32(runtime + 0x68 + 0xc, res)
    o.write_u32(runtime + 0x68 + 0x10, 101)
    o.hook(0x42ab50, lambda oc, a: node, 'cdecl', 2)
    o.hook(0x5447d0, lambda oc, a: 0, 'thiscall', 3)
    o.hook(0x300f0010, lambda oc, a: data if oc.reg('ecx') == res else td, 'thiscall', 1)
    cells = [_cell(o, 'I', arg & 0xffffffff) for arg in reversed(args)] + [_cell(o, 'L', 123)]
    result = o.call(address, [_frame(o, 'java.game.parts.Part', name, signature, cells)], fpu_cw=CW_D3D9).eax
    return dict(name=name, args=args, result=result,
                flags=[o.read_u32(runtime + i * 0x68 + 0x14) for i in range(3)])


def slot_rows():
    rows = []
    for name, address, signature in [
        ('getSlots', 0x4684a0, '()I'), ('getSlotID', 0x468570, '(I)I'),
        ('getSlotIndex', 0x4686e0, '(I)I'), ('isSlotDisabled', 0x469970, '(I)I'),
        ('partOnSlot', 0x4687f0, '(I)Ljava.game.parts.Part;'),
        ('getCarRef', 0x469120, '()Ljava.game.parts.Part;'),
        ('slotIDOnSlot', 0x468990, '(I)I')]:
        for arg in [-2, -1, 0, 1, 2, 3, 101, 201, 301, 999]:
            rows.append(slot_case(name, address, signature,
                                  [arg] if '(I)' in signature else [], [0, 0x11, 0x40]))
    for slot in [101, 201, 301, 999]:
        for mode in [0, 1, 2, 3, -1]:
            rows.append(slot_case('disableSlot', 0x469770, '(II)V',
                                  [slot, mode], [0x10, 0x11, 0x40]))
    return rows


if __name__ == '__main__':
    result = run_all()
    result['slots'] = slot_rows()
    print(json.dumps(result, sort_keys=True))
