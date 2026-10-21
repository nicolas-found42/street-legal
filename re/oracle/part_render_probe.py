#!/usr/bin/env python3
"""Original Part render wrappers and no-payload helper branches.

Only wrapper resource unwrap/load/virtual-data collaborators are supplied. The
original wrapper and467ff0/4682a0 guard/link instructions execute unchanged.
Nonempty render payload retargets are outside this bounded probe.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from render_vectors import _cell,_frame

ROWS=[('getTexture',0x469ab0,'()I'),('setTexture',0x469b80,'(I)I'),('getRenderType',0x469ed0,'()I'),('setRenderType',0x469fa0,'(I)I')]
def wrapper(name,address,sig,mode):
    o=Oracle(fpu_cw=CW_D3D9)
    node,res,template,data,td,body,vt,k=[o.alloc(n) for n in (16,0x60,0x60,0x80,0x20,0x180,0x20,0x2e98)]
    for c in [res,template]:o.write_u32(c,vt);o.write_u32(c+0x4c,1)
    o.write_u32(vt+0xc,0x300f0010);o.write_u32(node+0xc,res)
    for p,v in [(data+0x44,template),(data+0x4c,body),(td+0xc,k),(body+0x84,17),(body+0xa4,29)]:o.write_u32(p,v)
    if name.startswith('set'):o.write_u32(body+(0x84 if name=='setTexture' else 0xa4),0)
    o.hook(0x42ab50,lambda oc,a:0 if mode=='null' else node,'cdecl',2)
    o.hook(0x5447d0,lambda oc,a:0x80000000 if mode=='load_failed' else 0,'thiscall',3)
    o.hook(0x300f0010,lambda oc,a:data if oc.reg('ecx')==res else td,'thiscall',1)
    binds=[]
    def bind(oc,a):binds.append(a);return 0
    o.hook(0x546070,bind,'thiscall',3)
    cells=([_cell(o,'I',77)] if sig.startswith('(I)') else [])+[_cell(o,'L',123)]
    result=o.call(address,[_frame(o,'java.game.parts.Part',name,sig,cells)],fpu_cw=CW_D3D9)
    return {'name':name,'mode':mode,'result':result.eax,'binds':binds}

def helper(name):
    o=Oracle();k=o.alloc(0x2e98);body=o.alloc(0x180);target=o.alloc(16);old=o.alloc(0x80)
    if name=='texture_default_zero':
        o.write_u32(body+0x84,17);o.write_u32(target+8,29);o.write_u32(k+0x10f0,0)
        address=0x467ff0
    else:
        o.write_u32(body+0xa4,17);o.write_u32(body+0xa8,old);o.write_u32(old+0x48,body+0x9c)
        o.write_u32(body+0x94,29);address=0x4682a0
    o.call(address,[k,body,target])
    return {'name':name,'texture':o.read_u32(body+0x84),'render':o.read_u32(body+0xa4),'mesh':o.read_u32(body+0x94),'old_head':o.read_u32(old+0x48)}

def run():
    rows=[wrapper(*row,mode) for row in ROWS for mode in ['valid','null','load_failed']]
    for row in rows:
        expected=1 if row['name'].startswith('set') else (17 if row['name']=='getTexture' else 29) if row['mode']=='valid' else 0
        assert row['result']==expected,row
    helpers=[helper('texture_default_zero'),helper('null_render')]
    assert helpers[0]['texture']==17
    assert helpers[1]['render']==0 and helpers[1]['mesh']==29 and helpers[1]['old_head']==0
    return {'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'scope':__doc__,'rows':rows,'helpers':helpers}
if __name__=='__main__':print(json.dumps(run(),indent=2))
