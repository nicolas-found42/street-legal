#!/usr/bin/env python3
"""Bounded original Thread.start dispatch and pending-frame continuation.

Executes real47c570/42a690/41f9f0/408a30/4207c0/41fbc0 and an optional
later420ff0 pump over a controlled void-return body. Uses the real Scenario
Thread constructor with explicitly planted CREATED/script receiver state.
Native handle mapping, target field lookup, declared-type class/predicate,
method lookup and cached argument-signature lookup are supplied seams.
No actual inheritance-compatibility, stock script, scheduler-wide lifecycle,
Thread.init, or boot acceptance. Output must stay in ignored re/out/ only.
"""
import hashlib
import json
from pathlib import Path
from vminit_vectors import World,n,CW
from vmexec_vectors import arr
from langobj_vectors import Probe
from unicorn import UC_HOOK_CODE

EXE = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
EXE_SHA256 = '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
assert hashlib.sha256(EXE.read_bytes()).hexdigest() == EXE_SHA256

def run(body):
 w=World();o=w.o;s=w.s;thread=s.thread
 interner=o.alloc(0x30);o.call(0x41e4d0,[],convention='thiscall',this=interner,fpu_cw=CW);o.write_u32(s.jvm+0x68,interner)
 holder=w.klass('ThreadFixture');target=w.klass('Target',bodies=[[n(0x2a)]] if body else [])
 if not body:o.write_u32(target+24,arr(o,[0]))
 target_object=o.alloc(32);o.write_u32(target_object+12,target)
 receiver=o.alloc(32);o.write_u32(receiver+12,holder);o.write_u32(thread+0x18,receiver);o.write_u32(thread+0x2c,2)
 field=o.alloc(16);o.write_u32s(field,[0,w.text('Ljava.lang.Runnable;'),target_object,w.text('target')])
 calls=[];handle=[thread]
 def get_handle(oc,a):assert a[0]==receiver;return handle[0]
 def set_handle(oc,a):assert a[0]==receiver and a[2]==0;handle[0]=a[2]
 o.hook(0x42ab50,get_handle,'cdecl',2);o.hook(0x42a9e0,set_handle,'cdecl',3)
 def lookup(oc,a):assert oc.reg('ecx')==receiver;calls.append({'field':oc.read_cstr(a[0]).decode()});return field
 o.hook(0x408800,lookup,'thiscall',1)
 runnable=w.klass('RunnableDeclaredType')
 def loaded(oc,a):calls.append({'declared_type':oc.read_cstr(a[0]).decode()});return runnable
 o.hook(0x410890,loaded,'thiscall',1)
 def typed(oc,a):assert oc.reg('ecx')==runnable;calls.append({'required_type':oc.read_cstr(a[0]).decode()});return 1
 o.hook(0x4044e0,typed,'thiscall',1)
 method=o.alloc(28);o.write_u32s(method,[0,1,w.text('run'),w.text('()V'),0,1,0]);signature=o.alloc(12);o.write_u32s(signature,[s.jvm,0,arr(o,[0])]);o.write_u32(method+24,signature)
 record=o.alloc(16);o.write_u32s(record,[target,0,method,0])
 def resolve(oc,a):assert oc.reg('ecx')==target;assert oc.read_cstr(a[0])==b'run';assert oc.read_cstr(a[1])==b'()';return record
 o.hook(0x404910,resolve,'thiscall',2);o.hook(0x41def0,lambda oc,a:w.text('()'),'thiscall',0)
 returns=[];entries=[];run_phases=[];phase=['start']
 def observe(uc,address,size,data):
  if address in {0x408a30,0x4207c0,0x41fbc0,0x41f780}:entries.append(hex(address))
  if address==0x408a30:assert o.reg('ecx')==target_object;assert o.read_u32(o.reg('esp')+4)==thread
  if address==0x47c5d7:returns.append(o.reg('eax'))
  if address==0x420ff0:run_phases.append(phase[0])
 o.uc.hook_add(UC_HOOK_CODE,observe)
 p=Probe.__new__(Probe);p.o=o;info=p.call_info(receiver,b'()V');o.write_u32(info,s.jvm)
 base=o.read_u32(thread+0x20);p.call(0x47c570,info)
 frame=o.read_u32(thread+0x20);state={'flags':o.read_u32(thread+0x2c),'script_retained':o.read_u32(thread+0x18)==receiver,'handle_retained':handle[0]==thread,'new_frame_pending':frame!=base,'dispatch_returns':returns,'entries':list(entries),'field_calls':calls}
 assert calls==[{'field':'target'},{'declared_type':'Ljava.lang.Runnable;'},{'required_type':'java.lang.Runnable'}]
 assert run_phases==[]
 assert returns==[0 if body else 1]
 assert state['flags']==(0 if body else 128)
 assert state['script_retained']==body and state['handle_retained']==body and state['new_frame_pending']==body
 assert entries==['0x408a30','0x4207c0','0x41fbc0']+([] if body else ['0x41f780'])
 if body:
  state['pending_receiver']=o.read_u32(s._container(o.read_u32(frame+0x18))[0]+8)==target_object
  state['pending_pc_nonzero']=o.read_u32(frame+0x2c)!=0
  assert state['pending_receiver'] and state['pending_pc_nonzero']
  phase[0]='pump'
  o.call(0x420ff0,[10.0],convention='thiscall',this=thread,fpu_cw=CW,limit=1000000)
  state['after_pump']={'flags':o.read_u32(thread+0x2c),'base_frame_restored':o.read_u32(thread+0x20)==base,'handle_retained':handle[0]==thread,'script_retained':o.read_u32(thread+0x18)==receiver}
  assert state['after_pump']=={'flags':64,'base_frame_restored':True,'handle_retained':True,'script_retained':True}
  assert run_phases==['pump']
 state['interpreter_entry_phases']=run_phases
 return {'body_present':body,'scope':'real Thread.start/42a690/408a30/4207c0/41fbc0; field lookup, declared-type lookup/predicate, method lookup, cached argument signature, Native handle map supplied; controlled body optional void-return; no startup','state':state}
if __name__ == '__main__':
 for body in (False, True):
  print(json.dumps(run(body), sort_keys=True))
