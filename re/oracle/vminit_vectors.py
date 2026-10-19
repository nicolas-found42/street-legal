#!/usr/bin/env python3
"""Bounded original403e80/407da0 initializer observations.

Supplied canonical metadata, class/type lookup and common Scenario boundaries
remain explicit. Payload RID/String/new/call boundaries are declared in their
callbacks. Fatal cases stop at4190d0; no original handler/unwind, class poisoning,
constructor, active GC, or whole-world boot claim. Outputs never touch reference
or stock. The instance self-new case retains its supplied allocation boundary.
"""
from pathlib import Path
import datetime,hashlib,json,os,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW,n,STOP,ScriptFatal
from unicorn import UC_HOOK_CODE
from vmexec_vectors import pool,EXE_SHA256
REPO=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('SLR_VMINIT_VECTORS',REPO/'re/out/vminit'))
class World:
 def __init__(self):
  self.s=Scenario();self.o=o=self.s.o;self.names={};self.events=[];self.allclasses=[];self.texts={'I':0x60ab50,'F':0x60ab58,'V':0x60ab60}
  # Descriptor binding and decoded pool string lookup are explicit common metadata seams.
  o.hook(0x406140,lambda oc,a:a[0],'thiscall',1)
  o.hook(0x423e10,lambda oc,a:oc.write_u32(oc.reg('ecx'),a[1]),'thiscall',2)
  o.hook(0x4117a0,lambda oc,a:self.classrefs[(a[0],a[1])],'thiscall',2);self.classrefs={}
  head=o.alloc(0x40);owner=o.alloc(0x40);o.write_u32(owner+0x18,head);o.write_u32(self.s.jvm+0x10,owner)
  self.event_hook=o.uc.hook_add(UC_HOOK_CODE,self.observe)
  o.uc.ctl_remove_cache(0x400000,0x600000)
 def text(self,s):
  if s in self.texts:return self.texts[s]
  data=s.encode('latin1')+b'\0';p=self.o.alloc(len(data));self.o.write(p,data);self.texts[s]=p;return p
 def table(self,items):
  o=self.o;holder=o.alloc(16);data=o.alloc(max(4,len(items)*4))
  if items:o.write_u32s(data,items)
  o.write_u32s(holder,[0,len(items),data,len(items)]);return holder
 def fields(self,items):
  o=self.o;result=[]
  for name,ty,body in items:
   f=o.alloc(32);o.write_u32s(f+8,[self.text(name),self.text(ty),body if body is not None else 0xffffffff]);result.append(f)
  return self.table(result)
 def klass(self,name,base=0,statics=(),instance=(),bodies=(),interfaces=()):
  o=self.o;cl=o.alloc(0x200);mod=o.alloc(0x80);self.names[cl]=name;self.allclasses.append(cl)
  desc=o.alloc(4);o.write_u32(desc,self.text('L'+name+';'));o.write_u32s(cl+4,[mod,desc,self.s.jvm,0,mod])
  o.write_u32(cl+0x10,1 if name.startswith('Interface') else 0);o.write_u32(cl+0x1c8,base)
  o.write_u32(mod+0x40,0 if base else 0xffffffff);self.classrefs[(mod,0)]=base
  o.write_u32(mod+0x44,len(interfaces));ids=o.alloc(max(4,len(interfaces)*4))
  for i,iface in enumerate(interfaces):o.write_u32(ids+4*i,i+1);self.classrefs[(mod,i+1)]=iface
  o.write_u32(mod+0x48,ids);o.write_u32(mod+0x4c,self.fields(instance));o.write_u32(mod+0x54,self.fields(statics))
  arr=[]
  for nodes in bodies:
   nodes=([n(0x1007,0x1c)]+nodes) if nodes[0][0] in (0x100b,0x100d,0x100e,0x4009) else nodes
   p=o.alloc(8*(len(nodes)+1))
   for i,(op,line,imm) in enumerate(nodes):o.write_u32s(p+8*i,[(line<<16)|op,imm])
   arr.append(p)
  tb=o.alloc(12);data=o.alloc(max(4,len(arr)*4));
  if arr:o.write_u32s(data,arr)
  o.write_u32s(tb,[len(arr),data,len(arr)]);o.write_u32(cl+0x18,tb)
  return cl
 def owner(self,p):
  o=self.o
  if not p:return []
  con=o.read_u32(p+12)
  if not con:return []
  return [{'name':o.read_cstr(o.read_u32(c+12)).decode('latin1'),'type':o.read_cstr(o.read_u32(c+4)).decode('latin1'),'value':o.read_u32(c+8)} for c in self.s._container(con)]
 def state(self):return {self.names[c]:{'installed':bool(self.o.read_u32(c+0x1d0)),'static':self.owner(self.o.read_u32(c+0x1d0))} for c in self.allclasses}
 def observe(self,uc,address,size,ud):
  o=self.o
  if address==0x4038c0:
   cell=o.read_u32(o.reg('esp')+4);self.events.append({'add_field':o.read_cstr(o.read_u32(cell+12)).decode('latin1'),'state':self.state()})
  elif address==0x407b10:
   args=o.read_u32s(o.reg('esp')+4,5)
   if args[1] in self.names:self.events.append({'initializer_frame':self.names[args[1]],'receiver':bool(args[3]),'state':self.state()})
 def call(self,target,this,args=[]):
  try:self.o.call(target,args=args,convention='thiscall',this=this,fpu_cw=CW,limit=1000000);return 'ok'
  except Exception as e:return type(e).__name__+': '+str(e)

def order_probe(mode):
 w=World();o=w.o
 base=w.klass('Base',statics=[('bs','I',0)],instance=[('bi','I',1)],bodies=[[n(0x100b,11),STOP],[n(0x100b,12),STOP]])
 iface=w.klass('InterfaceOne',statics=[('is','I',0)],bodies=[[n(0x100b,21),STOP]])
 leaf=w.klass('Leaf',base=base,statics=[('ls','I',0),('ls2','F',None)],instance=[('li','I',1),('li2','F',None)],bodies=[[n(0x100b,31),STOP],[n(0x100b,32),STOP]],interfaces=[iface])
 if mode=='static':result=w.call(0x403e80,leaf)
 else:
  obj=o.alloc(32);result=w.call(0x407da0,obj,[leaf,0])
 out={'mode':mode,'outcome':result,'state':w.state(),'events':w.events}
 if mode!='static':out['instance']=w.owner(o.read_u32(obj+16))
 return out
def interface_order():
 w=World();a=w.klass('InterfaceA',statics=[('A','I',0)],bodies=[[n(0x100b,1),STOP]])
 b=w.klass('InterfaceB',statics=[('B','I',0)],bodies=[[n(0x100b,2),STOP]])
 c=w.klass('Declaring',statics=[('C','I',0)],bodies=[[n(0x100b,3),STOP]],interfaces=[b,a])
 result=w.call(0x403e80,c)
 return {'mode':'two_interfaces','outcome':result,'declared':['InterfaceB','InterfaceA'],'add_order':[x['add_field'] for x in w.events if 'add_field' in x],'state':w.state()}

def visibility_probe(static,index):
 w=World();o=w.o;key='static' if static else 'instance'
 # a=11; b=read a/b/c (one field later); c=33.
 spec=[('a','I',0),('b','I',1),('c','I',2)]
 cl=w.klass('ReadClass',statics=spec if static else [],instance=[] if static else spec,bodies=[[n(0x100b,11),STOP],[n(0x101b,77),STOP],[n(0x100b,33),STOP]])
 w.classrefs[(o.read_u32(cl+0x14),99)]=cl
 o.hook(0x406c20,lambda oc,a:99,'thiscall',1)
 o.hook(0x406bb0,lambda oc,a:index|(0x40000000 if static else 0),'thiscall',1)
 # Record real errors but keep the Scenario fatal-stop boundary unchanged.
 fat=o.hooks[0x4190d0]
 def fatal(oc,a):
  w.events.append({'fatal_site':hex(oc.read_u32(oc.reg('esp'))),'fatal_args':[hex(x) for x in a]});return fat[0](oc,a)
 o.hook(0x4190d0,fatal,fat[1],fat[2])
 obj=0 if static else o.alloc(32)
 target=0x403e80 if static else 0x407da0
 result=w.call(target,cl if static else obj,[] if static else [cl,0])
 before=w.state();inst=None if static else w.owner(o.read_u32(obj+16))
 events=list(w.events);w.events=[]
 # Reentry into the partially initialized class, without an artificial error flag.
 retry=w.call(0x403e80,cl);retryevents=list(w.events)
 return {'kind':key,'read_index':index,'outcome':result,'state':before,'instance':inst,'events':events,'class_reentry':{'outcome':retry,'events':retryevents,'state':w.state()}}

def payload_probe(kind,static):
 w=World();o=w.o;calls=[]
 ty='I';nodes=[]
 if kind=='resource':nodes=[n(0x100e,1),STOP]
 elif kind=='string':ty='Ljava.lang.String;';nodes=[n(0x4009,2),STOP]
 elif kind=='cast':ty='F';nodes=[n(0x1007,0x1c),n(0x100b,11),n(0x1007,0x21),n(0x1006,3),STOP]
 elif kind=='new':ty='LTarget;';nodes=[n(0x1007,0x1e),n(0x1006,4),STOP]
 elif kind=='self_new':ty='LHolder;';nodes=[n(0x1007,0x1e),n(0x1006,5),STOP]
 elif kind=='script_call':nodes=[n(0x24),n(0x1011,1),n(0x101a,6),STOP]
 else:raise ValueError(kind)
 spec=[('value',ty,0),('later','I',None)]
 cl=w.klass('Holder',statics=spec if static else [],instance=[] if static else spec,bodies=[nodes])
 p,records=pool(o,[(0,'system.rpk',10),(3,0,0),(0,'initializer text',16),(0,'F',1),(0,'Target',6),(0,'Holder',6),(5,0,0)])
 # Use actual pool shape/member constructor; decoded metadata already supplies pointers.
 o.call(0x407210,[0,27],convention='thiscall',this=records[1],fpu_cw=CW)
 o.write_u32(cl+0x14,p);o.write_u32(o.read_u32(cl+4)+0x24,p)
 o.hook(0x406140,lambda oc,a:o.read_u32(records[a[0]]+8) if a[0]<len(records) else a[0],'thiscall',1)
 def rid(oc,a):calls.append({'resource':oc.read_cstr(a[0]).decode(),'local_id':a[1]});return 0x12340000|a[1]
 o.hook(0x4122b0,rid,'cdecl',2)
 def string(oc,a):calls.append({'string':oc.read_cstr(a[0]).decode()});return supplied
 o.hook(0x4174a0,string,'thiscall',1)
 target=w.klass('Target')
 stringcl=w.klass('java.lang.String')
 supplied=o.alloc(32);o.write_u32(supplied+12,stringcl if kind=='string' else cl if kind=='self_new' else target)
 o.hook(0x411610,lambda oc,a:stringcl if 'String' in oc.read_cstr(a[0]).decode() else target if 'Target' in oc.read_cstr(a[0]).decode() else cl,'thiscall',1)
 o.hook(0x412670,lambda oc,a:o.read_u32((stringcl if 'String' in oc.read_cstr(a[0]).decode() else target if 'Target' in oc.read_cstr(a[0]).decode() else cl)+4),'thiscall',1)
 fatal=o.hooks[0x4190d0]
 def bad(oc,a):
  calls.append({'fatal_site':hex(oc.read_u32(oc.reg('esp'))),'args':[oc.read_cstr(x).decode('latin1') if x and x>0x400000 and x<0x800000 else hex(x) for x in a]});return fatal[0](oc,a)
 o.hook(0x4190d0,bad,fatal[1],fatal[2])
 def load(oc,a):
  name=oc.read_cstr(a[0]).decode();calls.append({'load':name});return cl if name=='Holder' else target
 o.hook(0x410890,load,'thiscall',1)
 def alloc(oc,a):calls.append({'new':w.names[oc.reg('ecx')]});return supplied
 o.hook(0x404e20,alloc,'thiscall',0)
 if kind=='script_call':
  # Controlled resolver returns a real static method record; call and frame execution remain real.
  method=o.alloc(28);o.write_u32s(method,[0,9,0,0,0,0,0])
  desc=o.alloc(4);o.write_u32(desc,w.text('I'));types=o.alloc(12);data=o.alloc(4);o.write_u32(data,desc);o.write_u32s(types,[1,data,1]);sig=o.alloc(12);o.write_u32s(sig,[w.s.jvm,0,types]);o.write_u32(method+24,sig)
  body=o.alloc(24);o.write_u32s(body,[0x1007,0x1c,0x100b,77,0x2b,0]);table=o.read_u32(cl+24);data2=o.alloc(8);o.write_u32s(data2,[o.read_u32(o.read_u32(table+4)),body]);o.write_u32s(table,[2,data2,2]);o.write_u32(method+16,1)
  record=o.alloc(16);o.write_u32s(record,[cl,0,method,0])
  def resolve(oc,a):
   pc=oc.read_u32(a[0]);oc.write_u32(a[0],pc+8);calls.append({'resolve_static_script':True});return record
  o.hook(0x4208e0,resolve,'thiscall',1)
 obj=0 if static else o.alloc(32)
 result=w.call(0x403e80 if static else 0x407da0,cl if static else obj,[] if static else [cl,0])
 return {'body':kind,'kind':'static' if static else 'instance','outcome':result,'state':w.state(),'instance':None if static else w.owner(o.read_u32(obj+16)),'calls':calls,'object_count':o.read_u32(w.s.jvm+0x34),'scope_new':'404e20 allocation boundary when requested'}

def failure_probe(static):
 w=World();o=w.o
 failbody=[n(0x1007,0x1c),n(0x100b,99),n(0x101b,77),n(0x1008,0x23),n(0x0)]
 fields=[('a','I',0),('b','I',1),('c','I',2)]
 cl=w.klass('FailClass',statics=fields if static else [],instance=[] if static else fields,bodies=[[n(0x100b,11),STOP],failbody,[n(0x100b,33),STOP]])
 o.hook(0x406bb0,lambda oc,a:0x40000000 if static else 0,'thiscall',1);o.hook(0x406c20,lambda oc,a:99,'thiscall',1);w.classrefs[(o.read_u32(cl+0x14),99)]=cl
 obj=0 if static else o.alloc(32)
 result=w.call(0x403e80 if static else 0x407da0,cl if static else obj,[] if static else [cl,0])
 before={'class':w.state(),'instance':None if static else w.owner(o.read_u32(obj+16)),'live':o.read_i32(0x62f138),'object_count':o.read_u32(w.s.jvm+0x34)}
 w.events=[];retry=w.call(0x403e80,cl)
 # Use actual public getters after the stopped failure; preserve only bounded observation.
 get=o.call(0x404800 if static else 0x4087e0,[0],convention='thiscall',this=cl if static else obj,fpu_cw=0x27f)
 return {'kind':'static' if static else 'instance','outcome':result,'state_at_fatal_stop':before,'class_reentry':retry,'reentry_events':w.events,'first_field_after_reentry':o.read_u32(get.eax+8),'failure_boundary':'Scenario ScriptFatal stops at4190d0; no original handler/unwind executed'}

def assignment_probe(kind,static):
 w=World();w.texts.update({'I':0x60ab50,'F':0x60ab58,'V':0x60ab60});o=w.o;o.hooks.pop(0x551220,None);calls=[];log_fragments=[];fatal=[];assign=[];after_copy=[];counts={};objects={}
 source_string=w.klass('java.lang.String');other=w.klass('Other');target=w.klass('Target')
 for name,cl in [('java.lang.String',source_string),('Other',other),('Target',target)]:
  word=o.alloc(32);o.write_u32(word+12,cl);objects[name]=word
 if kind in ('numeric_string','float_string'):ty='Ljava.lang.String;'
 elif kind in ('numeric_other','reference_other','mutate_reference_then_reference'):ty='LTarget;'
 else:ty='I'
 if kind=='float_string':expr=[n(0x1007,0x1c),n(0x200a,0x40200000)]
 elif kind in ('numeric_string','numeric_other'):expr=[n(0x1007,0x1c),n(0x100b,11)]
 else:expr=[n(0x1007,0x1c),n(0x4009,w.text('supplied source'))]
 prefix=[]
 if kind in ('mutate_integer_then_reference','mutate_integer_then_fatal'):
  prefix=[n(0x1007,0x1c),n(0x100b,99),n(0x101b,77),n(0x1008,0x23)]
 elif kind=='mutate_reference_then_reference':
  prefix=[n(0x1007,0x1e),n(0x1006,w.text('Target')),n(0x101b,77),n(0x1008,0x23)]
 if kind=='mutate_integer_then_fatal':expr=[n(0x0)]
 cl=w.klass('Holder',statics=[('value',ty,0),('later','I',None)]if static else [],instance=[]if static else[('value',ty,0),('later','I',None)],bodies=[prefix+expr+[STOP]])
 holder_module=o.read_u32(cl+0x14);w.classrefs[(holder_module,99)]=cl
 o.hook(0x406bb0,lambda oc,a:0x40000000 if static else 0,'thiscall',1)
 o.hook(0x406c20,lambda oc,a:99,'thiscall',1)
 classes={'java.lang.String':source_string,'Other':other,'Target':target,'Holder':cl}
 def find(oc,a):
  name=oc.read_cstr(a[0]).decode('latin1');calls.append({'find':name});return classes.get(name.removeprefix('L').removesuffix(';'),0)
 o.hook(0x411610,find,'thiscall',1)
 def ccd(oc,a):
  name=oc.read_cstr(a[0]).decode('latin1');calls.append({'descriptor_to_ccd':name});return o.read_u32(classes[name.removeprefix('L').removesuffix(';')]+4)
 o.hook(0x412670,ccd,'thiscall',1)
 def string(oc,a):calls.append({'intern_text':oc.read_cstr(a[0]).decode('latin1')if a[0]else None});return objects['java.lang.String']
 o.hook(0x4174a0,string,'thiscall',1)
 def load(oc,a):
  name=oc.read_cstr(a[0]).decode('latin1');calls.append({'new_target':name});return classes[name.removeprefix('L').removesuffix(';')]
 o.hook(0x410890,load,'thiscall',1)
 def alloc(oc,a):
  name=w.names[oc.reg('ecx')];calls.append({'allocation':name});return objects[name]
 o.hook(0x404e20,alloc,'thiscall',0)
 o.hook(0x551140,lambda oc,a:log_fragments.append(oc.read_cstr(a[1]).decode('latin1')if a[1]else '<null>')or None,'cdecl',3)
 o.hook(0x5513b0,lambda oc,a:log_fragments.append({'sink':oc.read_cstr(a[0]).decode('latin1')})or None,'cdecl',1)
 old=o.hooks[0x4190d0]
 def stop(oc,a):fatal.append({'site':hex(oc.read_u32(oc.reg('esp'))),'args':a});return old[0](oc,a)
 o.hook(0x4190d0,stop,old[1],old[2])
 def observe(uc,address,size,ud):
  if address in (0x403e80,0x407da0,0x41acb0,0x41ab60,0x4178f0,0x417da0,0x4044e0,0x405400,0x420ff0,0x551220):counts[hex(address)]=counts.get(hex(address),0)+1
  if address in (0x41ab9c,0x41abab,0x41abba):
   dst=o.reg('edi');src=o.reg('ebx');after_copy.append({'site':hex(address),'dst_type':o.read_cstr(o.read_u32(dst+4)).decode('latin1'),'dst_value':o.read_u32(dst+8),'src_type':o.read_cstr(o.read_u32(src+4)).decode('latin1'),'src_value':o.read_u32(src+8)})
  if address in (0x41acb0,0x41ab60):
   args=o.read_u32s(o.reg('esp')+4,3 if address==0x41acb0 else 2);dst=ocell=o.reg('ecx')
   # Both are thiscall(thread); explicit args are dst,src[,kind].
   dst,src=args[:2]
   def cell(p):return {'type':o.read_cstr(o.read_u32(p+4)).decode('latin1'),'value':o.read_u32(p+8)}
   assign.append({'helper':hex(address),'dst':cell(dst),'src':cell(src),'state':w.state()})
 o.uc.hook_add(UC_HOOK_CODE,observe);o.uc.ctl_remove_cache(0x400000,0x600000)
 obj=0 if static else o.alloc(32)
 outcome=w.call(0x403e80 if static else 0x407da0,cl if static else obj,[]if static else[cl,0])
 fields=w.owner(o.read_u32(cl+0x1d0))if static else w.owner(o.read_u32(obj+16))
 def normalized(v):return next((k for k,p in objects.items()if p==v),v)
 for f in fields:f['normalized_value']=normalized(f['value'])
 return {'kind':kind,'static':static,'outcome':outcome,'fields':fields,'calls':calls,'diagnostic_fragments':log_fragments,'fatal':fatal,'assignment_entry':assign,'assignment_after_copy':after_copy,'original_helper_entries':counts,'supplied_objects':objects,'boundaries':['commonheap/time/log/fatal','decodedpool/typebinding','classlookup/CCD','Stringintern/allocation'],'predicate_helpers':'real4044e0/405400 over controlled valid already-loaded class/CCD metadata'}

def dependency_kind_probe(case, instance):
    w = World(); o = w.o; logs = []
    w.texts.update({'I': 0x60ab50, 'F': 0x60ab58, 'V': 0x60ab60})
    base = w.klass('Base', statics=[('base', 'I', 0)], bodies=[[n(0x100b, 11), STOP]])
    iface = w.klass('InterfaceOne', statics=[('iface', 'I', 0)], bodies=[[n(0x100b, 21), STOP]])
    leaf = w.klass('Leaf', base=base, interfaces=[iface], statics=[('own', 'I', 0)],
                   instance=[('item', 'I', 1)], bodies=[[n(0x100b, 31), STOP], [n(0x100b, 32), STOP]])
    if case == 'interface_class': o.write_u32(iface + 0x10, 0)
    if case == 'interface_unknown': o.write_u32(iface + 0x10, 2)
    if case == 'base_interface': o.write_u32(base + 0x10, 1)
    def concat(oc, args):
        logs.append({'fragment': oc.read_cstr(args[1]).decode('latin1') if args[1] else None})
        return None
    o.hook(0x551140, concat, 'cdecl', 3)
    o.hook(0x5513b0, lambda oc, args: logs.append({'sink': oc.read_cstr(args[0]).decode('latin1')}) or None, 'cdecl', 1)
    obj = o.alloc(32) if instance else 0
    outcome = w.call(0x407da0 if instance else 0x403e80, obj if instance else leaf, [leaf, 0] if instance else [])
    return {'case': case, 'instance': instance, 'outcome': outcome,
            'base_kind': o.read_u32(base + 0x10), 'interface_kind': o.read_u32(iface + 0x10),
            'state': w.state(), 'add_order': [e['add_field'] for e in w.events if 'add_field' in e],
            'instance_fields': w.owner(o.read_u32(obj + 16)) if instance else [], 'logs': logs,
            'boundaries': ['common heap/time/fatal', 'decoded pool/class references/canonical metadata', 'diagnostic concatenation and sink']}


def self_descriptor_probe(static,ancestor):
 w=World();o=w.o;base=w.klass('Base') if ancestor else 0
 ty='LBase;' if ancestor else 'LHolder;'
 fields=[('value',ty,0),('later','I',None)]
 cl=w.klass('Holder',base=base,statics=fields if static else [],instance=[] if static else fields,bodies=[[n(0x0),STOP]])
 obj=0 if static else o.alloc(32)
 outcome=w.call(0x403e80 if static else 0x407da0,cl if static else obj,[] if static else [cl,0])
 return {'static':static,'ancestor':ancestor,'outcome':outcome,'state':w.state(),'fields':w.owner(o.read_u32(cl+0x1d0)) if static else w.owner(o.read_u32(obj+16)),'body_entries':[e['initializer_frame'] for e in w.events if 'initializer_frame' in e],'boundary':'explicit fatal0 stops at4190d0; no unwind'}

GROUPS={
 'self_descriptor':(self_descriptor_probe,[dict(static=True,ancestor=False),dict(static=False,ancestor=False),dict(static=True,ancestor=True)]),
 'dependency_kind':(dependency_kind_probe,[dict(case=case,instance=instance) for instance in [False,True] for case in ['valid','interface_class','interface_unknown','base_interface']]),
 'assignment':(assignment_probe,[dict(kind=kind,static=static) for static in [True,False] for kind in ['numeric_string','float_string','numeric_other','reference_other','reference_integer','mutate_integer_then_reference','mutate_reference_then_reference','mutate_integer_then_fatal']]),
 'order':(order_probe,[dict(mode='static'),dict(mode='instance')]),
 'visibility':(visibility_probe,[dict(static=static,index=index) for static in [True,False] for index in range(3)]),
 'payloads':(payload_probe,[dict(kind=kind,static=static) for static in [True,False] for kind in ['resource','string','cast','new','self_new','script_call']]),
 'failure':(failure_probe,[dict(static=True),dict(static=False)]),
 'interfaces':(lambda:interface_order(),[{}]),
}
def main():
 command=sys.argv[1] if len(sys.argv)>1 else 'gen'
 if command=='verify':
  selected=[Path(p) for p in sys.argv[2:]];paths=selected or [OUT/f'vminit_{name}.jsonl' for name in GROUPS];count=0
  for path in paths:
   name=path.stem.removeprefix('vminit_')
   if name not in GROUPS:raise SystemExit(f'unknown suite: {path}')
   rows=[json.loads(line) for line in path.read_text().splitlines()]
   if not rows:raise SystemExit(f'empty vectors: {path}')
   run,cases=GROUPS[name];seen=set()
   if not selected and len(rows)!=len(cases):raise SystemExit(f'incomplete suite: {name}')
   for row in rows:
    index=row['id'];assert type(index) is int and 0<=index<len(cases) and index not in seen,(path,index);seen.add(index)
    assert row['name']==name and row['input']==cases[index] and row['oracle']=='unicorn' and row['exe_sha256']==EXE_SHA256 and row['cw']==hex(CW) and row['exe_addr']=='0x00403e80/0x00407da0/0x00420ff0',(path,index,'manifest')
    actual=json.loads(json.dumps(run(**row['input'])));assert actual==row['result'],(path,index,actual,row['result']);count+=1
  if not count:raise SystemExit('no comparisons')
  print(f'verify: {count}/{count} identical')
 else:
  OUT.mkdir(parents=True,exist_ok=True)
  for name in (GROUPS if command=='gen' else [command]):
   run,cases=GROUPS[name];rows=[dict(id=index,name=name,input=case,result=run(**case),oracle='unicorn',exe_sha256=EXE_SHA256,exe_addr='0x00403e80/0x00407da0/0x00420ff0',cw=hex(CW),date=datetime.date.today().isoformat()) for index,case in enumerate(cases)]
   path=OUT/f'vminit_{name}.jsonl';path.write_text(''.join(json.dumps(row,sort_keys=True)+'\n' for row in rows));print(f'{path}: {len(rows)} rows')
if __name__=='__main__':main()
