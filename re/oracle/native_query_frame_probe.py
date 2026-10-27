#!/usr/bin/env python3
"""Original426470/427210 caller frames with declared controlled resource/VM
callees and allocator. Complete fixed caller arena, retained stack and eight
registers are observed at every collaborator entry and final return. Bodies
of resource access, script execution and class getInfo are not claimed.
"""
import hashlib,json,argparse
from pathlib import Path
from unicorn import x86_const as X
import instance_query_vectors as source
from oracle import Oracle,STACK_TOP,CW_D3D9
ROOT=Path(__file__).resolve().parents[2]
REGS=['eax','ebx','ecx','edx','esi','edi','ebp','esp']
LOW=STACK_TOP-0x1000
class Capture(Oracle):
    def __init__(self,*a,**kw):
        super().__init__(*a,**kw);self.measured=False;self.observations=[];self.fill=FILL
    def snapshot(self):
        return {'heap':self.read_u32s(0x20000000,1024),'stack':bytes(self.uc.mem_read(LOW,0x1000)).hex(),'regs':{r:self.reg(r) for r in REGS}}
    def hook(self,address,callback,convention='cdecl',nargs=0,**kw):
        def wrapped(o,args):
            if self.measured:self.observations.append(dict(address=address,args=args,this=o.reg('ecx'),state=self.snapshot()))
            return callback(o,args)
        return super().hook(address,wrapped,convention,nargs,**kw)
    def call(self,address,*a,**kw):
        if address not in [0x426470,0x427210]:return super().call(address,*a,**kw)
        self.uc.mem_write(LOW,bytes([self.fill])*0x1000)
        self.before=self.snapshot();self.measured=True
        result=super().call(address,*a,**kw);self.measured=False
        if result.fpcw != CW_D3D9 or result.fpu_top_depth != 0:
            raise ValueError('unexpected original FPU control/depth')
        for location,size,value in result.mem_writes:
            if not (0x20000000 <= location and location+size <= 0x20001000 or LOW <= location and location+size <= STACK_TOP):
                raise ValueError(f'original store outside complete declared windows: {location:#x}/{size}')
        self.after=self.snapshot();ACTIVE.append(self)
        return result
FILL=0;ACTIVE=[]
def capture():
    source.Oracle=Capture
    specs=source.inputs()
    # Preserve the full original input inventory, repeated with nonzero
    # retained stack bytes; no branch or failure case is selected away.
    indices=list(range(len(specs)))
    rows=[]
    global FILL
    for FILL in [0,0xa5]:
        for index in indices:
            row=source.run(index,exe=ROOT/'stock/StreetLegal_Redline.bak.exe',cw=CW_D3D9);o=ACTIVE.pop()
            rows.append(dict(index=index,fill=FILL,input=row,initial=o.before,events=o.observations,final=o.after))
    return dict(schema=1,scope=__doc__,fpu_cw=CW_D3D9,stack_low=LOW,stack_bytes=0x1000,heap_bytes=0x1000,exe_sha256=hashlib.sha256((ROOT/'stock/StreetLegal_Redline.bak.exe').read_bytes()).hexdigest(),producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),source_sha256=hashlib.sha256(Path(source.__file__).read_bytes()).hexdigest(),oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),rows=rows)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);a=p.parse_args();a.out.parent.mkdir(parents=True,exist_ok=True)
    result=capture();a.out.write_text(json.dumps(result,separators=(',',':'))+'\n');print(len(result['rows']),'original caller cases',a.out)
