#!/usr/bin/env python3
"""Original47a920 boot command branches; controlled419860,425a90,477e00,545fc0.
Scanner551240 andstrcmp5d7190 execute actual original instructions. Complete
caller heaps/callback heaps, actual ECX/args/ESP/returnPC, callee cleanup and
nonvolatile/FPU state are compared. No camera/target/collision acceptance.
"""
import argparse,hashlib,itertools,json
from pathlib import Path
from oracle import Oracle,CW_D3D9,HEAP_BASE
from native_player_probe import bind
ROOT=Path(__file__).resolve().parents[2];EXE=ROOT/'stock/StreetLegal_Redline.bak.exe'
TEXTS=['device 0','device -7','device 2147483648','device 4294967297','device +3','device junk','  device\t9','setcontrol','setcontrol 55','renderinstance -57343','renderinstance 0','renderinstance junk','team -19','team 2147483648','team +1','team 7tail','uncontrol 3','TEAM 2','unrecognized 123']
SCENARIOS=['null-raw','no-envelope','native','script','script-mutation']

def case(fill,scenario,command,event):
 o=Oracle(str(EXE),fpu_cw=CW_D3D9)
 k=o.alloc(0x50);body=o.alloc(0x2510);raw=o.alloc(0x180);env=o.alloc(0x80);other=o.alloc(0x180)
 o.write(k,bytes([fill])*0x50);o.write(body,bytes([fill])*0x2510)
 o.write_u32(raw+0x50,0xffff2001);o.write_u32(other+0x50,0xffff2002)
 for ref in [body,body+0x2194]:o.write_u32s(ref,[0]*4)
 if scenario!='null-raw':bind(o,body,raw)
 bind(o,body+0x2194,raw)
 o.write_u32(env+0x50,0x30001234 if scenario.startswith('script') else 0)
 words=(o.heap_ptr-HEAP_BASE)//4;events=[]
 ptr=o.scratch(len(command)+1);o.write(ptr,command.encode()+b'\0')
 def heap():return o.read_u32s(HEAP_BASE,words)
 def record(name,oc,args):
  sp=oc.reg('esp');events.append(dict(name=name,this=oc.reg('ecx'),args=list(args),esp=sp,return_pc=oc.read_u32(sp),heap=heap()))
 def access(oc,args):record('access',oc,args);return 0 if scenario=='no-envelope' else env
 def script(oc,args):
  record('script',oc,args)
  if scenario=='script-mutation':oc.write_u32(body+0x2414,oc.read_u32(body+0x2414)^0x99887766)
  return 0
 def device(oc,args):record('device',oc,args);oc.write_u32(body+0x1c+4,args[1]);return 0
 def binding(oc,args):
  record('bind',oc,args);bind(oc,oc.reg('ecx'),other if args[0]==0xffff2002 else (raw if args[0]==0xffff2001 else 0));return 0
 o.hook(0x419860,access,'thiscall',4)
 #425a90 is variadic. Its actual command path determines argument count.
 o.hook(0x425a90,script,'cdecl',6 if command.lstrip().split()[0]=='device' else 4)
 o.hook(0x477e00,device,'cdecl',2);o.hook(0x545fc0,binding,'thiscall',1)
 before=heap();result=o.call(0x47a920,[body,0x13572468,event,ptr],convention='thiscall',this=k)
 return dict(fill=fill,scenario=scenario,command=command,event=event,layout=dict(k=k,body=body,raw=raw,envelope=env,other=other,words=words,command=ptr),before=before,after=heap(),events=events,result=result.eax,esp_delta=result.esp_delta,fpu_cw=result.fpcw,fpu_depth=result.fpu_top_depth,nonvolatile={r:o.reg(r)for r in ['ebx','esi','edi','ebp']})

def capture():
 rows=[case(fill,scenario,text,0x10)for fill,scenario,text in itertools.product([0,0xa5],SCENARIOS,TEXTS)]
 rows += [case(fill,'native','unused',event)for fill,event in itertools.product([0,0xa5],[0,2,8,0x1c,0xffffffff])]
 return dict(schema=1,scope=__doc__,exe_sha256=hashlib.sha256(EXE.read_bytes()).hexdigest(),producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),oracle_sha256=hashlib.sha256(Path(__file__).with_name('oracle.py').read_bytes()).hexdigest(),binding_collaborator_sha256=hashlib.sha256(Path(__file__).with_name('native_player_probe.py').read_bytes()).hexdigest(),texts=TEXTS,scenarios=SCENARIOS,rows=rows)
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,required=True);args=p.parse_args()
 with args.out.open('x')as stream:json.dump(capture(),stream,sort_keys=True);stream.write('\n')
