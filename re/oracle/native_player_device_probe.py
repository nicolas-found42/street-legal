#!/usr/bin/env python3
"""Original477e00/54d650/54d7b0 with controlled physical/config collaborators.
Original static profiles, weak slot selection, axis configuration, scanner-free
native mapping and actual54de20 wrapper execute from stock build606. Only
558100 name,557430 sample,557990 axis kind and4261f0 deadzone are controlled.
This establishes raw mapping storage behavior, NOT macOS physical device parity.
"""
import argparse,hashlib,itertools,json,struct
from pathlib import Path
from oracle import Oracle,CW_D3D9,HEAP_BASE
ROOT=Path(__file__).resolve().parents[2];EXE=ROOT/'stock/StreetLegal_Redline.bak.exe'
NAMES=['SysKeyboard','4 axis 16 button joystick','InterAct SV-262 HammerHead Fx','Microsoft SideWinder game pad','DUALSHOCK','unrecognized']
WORDS=0x2300//4

def configurations():
 rows=list(itertools.product([0,0xa5],NAMES,['empty','last-two'],[0],[0x3e000000]))
 rows += [(fill,name,'empty',0,0x3e000000)for fill,name in itertools.product([0,0xa5],[None,'sYSkeYBoARD'])]
 rows += [(0,name,'empty',0,dead)for name,dead in itertools.product([NAMES[i]for i in [1,2,4,5]],[0x80000000,1,0x3f666666])]
 rows += [(0,name,'empty',device,0x3e000000)for name,device in itertools.product(NAMES,[0xffffffff,0x7fffffff])]
 rows += [(0,name,slots,2,0x3e000000)for name,slots in itertools.product(NAMES,['full','sparse'])]
 return rows

def sample_bits(device,control):return [0x80000000,0x3eaaaaab,0xbf400000,1][(device+control)%4]
def kind(device,control):return (device^control)&1

def case(fill,name,slots,device,dead):
 o=Oracle(str(EXE),fpu_cw=CW_D3D9);input=o.alloc(0x2300);o.write(input,bytes([fill])*0x2300)
 o.write_u32(input,7)
 for i in range(152):
  occupied=slots=='full'or(slots=='last-two'and i<150)or(slots=='sparse'and i%3!=1)
  o.write_u32(input+0xe44+i*0x20,0x40+i if occupied else 0)
 nameptr=o.scratch(128)
 if name is not None:o.write(nameptr,name.encode()+b'\0')
 events=[]
 def heap():return o.read_u32s(HEAP_BASE,WORDS)
 def record(label,oc,args):
  sp=oc.reg('esp');events.append(dict(name=label,this=oc.reg('ecx'),args=list(args),esp=sp,return_pc=oc.read_u32(sp),heap=heap(),registers={r:oc.reg(r)for r in ['eax','ebx','ecx','edx','esi','edi','ebp']}))
 def naming(oc,args):record('name',oc,args);return nameptr if name is not None else 0
 def config(oc,args):record('deadzone',oc,args);return struct.unpack('<f',struct.pack('<I',dead))[0]
 def sample(oc,args):record('sample',oc,args);return struct.unpack('<f',struct.pack('<I',sample_bits(*args)))[0]
 def axis_kind(oc,args):record('axis_kind',oc,args);return kind(*args)
 o.hook(0x558100,naming,'cdecl',1);o.hook(0x4261f0,config,'cdecl',1)
 o.hook(0x557430,sample,'cdecl',2);o.hook(0x557990,axis_kind,'cdecl',2)
 before=heap();result=o.call(0x477e00,[input,device])
 return dict(fill=fill,name=name,slots=slots,device=device,deadzone=dead,layout=dict(input=input,name=nameptr,words=WORDS),before=before,after=heap(),events=events,args_after=o.read_u32s(o.reg('esp'),2),esp_delta=result.esp_delta,fpu_cw=result.fpcw,fpu_depth=result.fpu_top_depth,nonvolatile={r:o.reg(r)for r in ['ebx','esi','edi','ebp']})

def helper_case(fill,slots,action,device,control):
 o=Oracle(str(EXE),fpu_cw=CW_D3D9);input=o.alloc(0x2300);o.write(input,bytes([fill])*0x2300)
 o.write_u32(input,0xffffffff)
 for i in range(152):
  occupied=slots=='full'or(slots=='last-two'and i<150)or(slots=='sparse'and i%3!=1)
  o.write_u32(input+0xe44+i*0x20,0x40+i if occupied else 0)
 events=[]
 def heap():return o.read_u32s(HEAP_BASE,WORDS)
 def record(label,oc,args):
  sp=oc.reg('esp');events.append(dict(name=label,this=oc.reg('ecx'),args=list(args),esp=sp,return_pc=oc.read_u32(sp),heap=heap(),registers={r:oc.reg(r)for r in ['eax','ebx','ecx','edx','esi','edi','ebp']}))
 def sample(oc,args):record('sample',oc,args);return struct.unpack('<f',struct.pack('<I',sample_bits(*args)))[0]
 def axis_kind(oc,args):record('axis_kind',oc,args);return kind(*args)
 o.hook(0x557430,sample,'cdecl',2);o.hook(0x557990,axis_kind,'cdecl',2)
 args=[action,device,control,0x80000000,0x3f200000,0xbf800000,0x3f800000]
 before=heap();result=o.call(0x54d650,args,convention='thiscall',this=input)
 return dict(fill=fill,slots=slots,action=action,device=device,control=control,args=args,layout=dict(input=input,words=WORDS),before=before,after=heap(),events=events,result=result.eax,esp_delta=result.esp_delta,fpu_cw=result.fpcw,fpu_depth=result.fpu_top_depth,nonvolatile={r:o.reg(r)for r in ['ebx','esi','edi','ebp']})

def capture():
 return dict(schema=1,scope=__doc__,exe_sha256=hashlib.sha256(EXE.read_bytes()).hexdigest(),producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),recovery_sha256=hashlib.sha256(Path(__file__).with_name('native_player_device_recover.py').read_bytes()).hexdigest(),rows=[case(*c)for c in configurations()],helpers=[helper_case(*c)for c in itertools.product([0,0xa5],['empty','last-two','full','sparse'],[0,75],[0,0xffffffff],[0,0xffffffff])])
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 with a.out.open('x')as stream:json.dump(capture(),stream,sort_keys=True);stream.write('\n')
