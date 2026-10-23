#!/usr/bin/env python3
"""Original pool memory behavior. Synthetic typed source cells, real allocators,
copy/value and variable constructors/destructors. No Thread.run/ABI parity claim.
Allocator uses Scenario's declared malloc/realloc profile. All heap bytes and
ordered original pool/cell/counter stores are retained, no comparison exclusions.
"""
from pathlib import Path
import base64, hashlib, json, sys, zlib
sys.path.insert(0, str(Path(__file__).resolve().parent))
from thread_vectors import Scenario, CW, TC
from oracle import DEFAULT_EXE, HEAP_BASE
from unicorn import UC_HOOK_MEM_WRITE


def main():
    s = Scenario()
    o = s.o
    source = o.alloc(12)
    o.write_u32s(source, [0x5e734c, TC['I'], 7])
    #423ec0 dereferences type+8, then reads the full type-name pointer.
    descriptor = o.alloc(12)
    name_pointer = o.alloc(4)
    o.write_u32(descriptor+8, name_pointer)
    o.write_u32(name_pointer, TC['I'])
    ops, stores, cells = [], [], {}
    recording = False
    def observe(uc, access, address, size, value, _):
        if recording and (HEAP_BASE <= address < o.heap_ptr or address == 0x62f138):
            stores.append({'address': address, 'size': size, 'value': value})
    o.uc.hook_add(UC_HOOK_MEM_WRITE, observe)
    def snapshot():
        data = o.read(HEAP_BASE, o.heap_ptr-HEAP_BASE)
        return {'heap_ptr':o.heap_ptr,'length':len(data),'sha256':hashlib.sha256(data).hexdigest(),
                'zlib_base64':base64.b64encode(zlib.compress(data)).decode(),
                'live':o.read_u32(0x62f138)}
    initial = snapshot()
    recording = True
    def allocate(kind, slot, payload=7, name=0):
        pool = o.read_u32(0x62f130 if kind=='value' else 0x62f134)
        pointer = o.call(0x423c00 if kind=='value' else 0x4050b0, [pool], convention='fastcall', fpu_cw=CW).eax
        if kind=='value':
            o.write_u32(source+8,payload)
            o.call(0x423e30,[source],convention='thiscall',this=pointer,fpu_cw=CW)
        else:
            o.call(0x423ec0,[name,descriptor],convention='thiscall',this=pointer,fpu_cw=CW)
        ops.append({'action':'allocate','kind':kind,'slot':slot,'payload':payload if kind=='value' else 0,'name':name,'address':pointer})
        cells[kind,slot] = pointer
    def release(kind,slot):
        pointer = cells[kind,slot]
        o.call(0x405000 if kind=='value' else 0x405040,[1],convention='thiscall',this=pointer,fpu_cw=CW)
        ops.append({'action':'release','kind':kind,'slot':slot,'address':pointer})
    allocate('value',0);allocate('value',1)
    allocate('variable',0,name=0x12345678);allocate('variable',1,name=0x87654321)
    release('value',0);release('variable',1)
    allocate('value',0,9);allocate('variable',1,name=0x11223344)
    release('value',1);release('value',0)
    allocate('value',0,11);allocate('value',1,11)
    # Cross both the512-cell value block boundary and table realloc branch.
    for slot in range(2,514): allocate('value',slot,slot)
    release('variable',0);release('variable',1)
    recording=False
    output={'schema':1,'scope':'pool memory only; supplied synthetic constructor inputs',
            'profile':{'fpu_cw':CW,'allocator':'Scenario malloc/realloc (zero bump16, realloc allocate/copy old size)',
                       'original_exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()},
            'producer_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'oracle_sha256':hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),
            'scenario_sha256':hashlib.sha256(Path(__file__).with_name('thread_vectors.py').read_bytes()).hexdigest(),
            'initial':initial,'final':snapshot(),'type_pointer':TC['I'],'source':source,
            'pools':{'value':o.read_u32(0x62f130),'variable':o.read_u32(0x62f134)},
            'ops':ops,'stores':stores}
    print(json.dumps(output,separators=(',',':'),sort_keys=True))
if __name__=='__main__':main()
