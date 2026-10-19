#!/usr/bin/env python3
"""Original build606 VM execution vectors for #110.

Runs bounded original dispatch, frame/return, literal, constructor/array,
cast and no-GC String identity paths. Each scenario records its supplied
metadata and callback boundaries; this does not establish whole helpers,
class initialization, machine CallInfo identity, active GC, or whole boot.
No reference/stock writes or Wine.
"""
from pathlib import Path
import datetime,hashlib,json,os,sys
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from thread_vectors import Scenario,RUN,CW,TC,ScriptFatal,VAR_VT
from vmclass_vectors import _holder,_new_oracle,E4D0
REPO=Path(__file__).resolve().parents[2]
OUT=Path(os.environ.get('SLR_VMEXEC_VECTORS',REPO/'re/out/vmexec'))
EXE_SHA256='7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
assert hashlib.sha256((REPO/'reference/raw/StreetLegal_Redline.bak.exe').read_bytes()).hexdigest()==EXE_SHA256
def cstr(o,text):
 data=text.encode()+b'\0';p=o.alloc(len(data));o.write(p,data);return p
def pool(o,records):
 pointers=[]
 for kind,a,b in records:
  p=o.alloc(16);o.write_u32s(p,[0,kind,cstr(o,a) if kind==0 else a,b]);pointers.append(p)
 data=o.alloc(4*len(pointers));o.write_u32s(data,pointers)
 p=o.alloc(20);o.write_u32s(p,[0,0,len(pointers),data,len(pointers)]);return p,pointers
def arr(o,words,metadata=False):
    h=o.alloc(20);d=o.alloc(max(4,4*len(words)))
    if words:o.write_u32s(d,words)
    o.write_u32s(h,[0,len(words),d,len(words)] if metadata else [len(words),d,len(words)])
    return h

def setup():
    s=Scenario();o=s.o;o.hooks.pop(0x406140,None)
    p,ps=pool(o,[(0,'method',6),(5,0,0),(0,'()I',3),(4,4,0),(0,'ProbeNative',11)])
    # Type5 cache slot is extra word at +10, class reference in +8.
    tref=o.alloc(20);o.write_u32s(tref,[0,5,0,0,0]);o.write_u32(o.read_u32(p+12)+4,tref)
    base=o.alloc(0x200);leaf=o.alloc(0x200)
    bm=o.alloc(0x80);lm=o.alloc(0x80)
    for cl,mod,name in [(base,bm,'LBase;'),(leaf,lm,'LLeaf;')]:
        o.write_u32(cl+4,mod);o.write_u32(cl+12,s.jvm);o.write_u32(cl+0x14,p)
        desc=o.alloc(4);o.write_u32(desc,cstr(o,name));o.write_u32(cl+8,desc)
        o.write_u32(mod+0x24,p);o.write_u32(cl+0x18,o.alloc(16))
        o.write_u32(mod+0x3c,3)
    o.write_u32(leaf+0x1c8,base)
    methods=[]
    for cl,mod in [(base,bm),(leaf,lm)]:
        method=o.alloc(28);o.write_u32s(method,[0,1,0,2,0,4,0]);methods.append(method)
        o.write_u32(mod+0x50,arr(o,[method],True));o.write_u32(mod+0x58,arr(o,[method],True))
    obj=o.alloc(32);o.write_u32(obj+12,leaf)
    s.ctx=leaf;s.ctx_q=lm
    s.add_frame(0,{'nodes':[[0x24,1,0],[0x10,1,0]],'entry':0,'locals':[],'stack':[]});s.finish_setup()
    events=[]
    oldfatal=o.hooks[0x4190d0]
    def fatal(oc,a):
        events.append({'fatal_site':hex(oc.read_u32(oc.reg('esp'))),'fatal_message':oc.read_cstr(a[1]).decode('latin1')})
        return oldfatal[0](oc,a)
    o.hook(0x4190d0,fatal,oldfatal[1],oldfatal[2])
    o.hook(0x403e80,lambda oc,a:events.append({'seam':'class_init','this':hex(oc.reg('ecx'))}) or 1,'thiscall',0)
    def load(oc,a):
        events.append({'seam':'class_from_pool','pool_matches_defining':a[0]==p,'class_idx':a[1]})
        return leaf
    o.hook(0x4117a0,load,'thiscall',2)
    return s,p,tref,base,leaf,obj,methods,events

def frame_call(static=True,arguments=False):
    s,p,tref,base,leaf,obj,methods,events=setup();o=s.o
    method=methods[0];o.write_u32(method+4,9 if static else 1)
    # Signature cache produced by407ac0: list of parameter type cells, return last.
    tc=[]
    for ty in (['I','F','I'] if arguments else ['I']):
        cell=o.alloc(4);o.write_u32(cell,TC[ty]);tc.append(cell)
    types=arr(o,tc);sig=o.alloc(12);o.write_u32s(sig,[s.jvm,0,types]);o.write_u32(method+24,sig)
    if arguments:
        s._container_set(s.frames[0]+28,[s._cell({'k':'v','t':'I','w':11}),s._cell({'k':'v','t':'F','w':0x40200000})])
    body=o.alloc(24);o.write_u32s(body,[0x1007,0x1c,0x100b,77,0x2b,0])
    table=arr(o,[body]);o.write_u32(base+24,table)
    rec=o.alloc(16);o.write_u32s(rec,[base,0,method,1])
    caller=s.frames[0];saved=o.read_u32(s.thread+0x24)
    try:
        r=o.call(0x41fbc0,args=[rec,base,0 if static else obj],convention='thiscall',this=s.thread,fpu_cw=CW)
        f=o.read_u32(s.thread+0x20)
        localcells=s._container(f+12)
        before={'return_code':r.eax,'new_frame':f!=caller,'defining_class_is_base':o.read_u32(f+0x34)==base,'frame_receiver_matches':o.read_u32(f+0x30)==(0 if static else obj),'caller_resume_saved':o.read_u32(caller+0x2c)==saved,'locals':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'payload':('receiver' if o.read_u32(v+8)==obj else o.read_u32(v+8))} for v in localcells]}
        s.frames.append(f);s.node_span.append((body,3));s.local_ptrs.append(localcells)
        o.call(RUN,args=[0.],convention='thiscall',this=s.thread,fpu_cw=CW)
        vals=s._container(caller+28)
        return {'operation':'frame_call','static':static,'arguments':arguments,'before_run':before,'after_run':{'back_to_caller':o.read_u32(s.thread+0x20)==caller,'stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'payload':('receiver' if o.read_u32(v+8)==obj else o.read_u32(v+8))} for v in vals]}}
    except Exception as e:return {'operation':'frame_call','static':static,'error':str(e),'before_run':locals().get('before'),'events':events}

def frame_cases():return [dict(static=static,arguments=arguments) for static,arguments in [(True,False),(False,False),(True,True),(False,True)]]
def resolved_call(static=False,arguments=False):
    s,p,tref,base,leaf,obj,methods,events=setup();o=s.o
    o.write_u32(tref+16,0x40000000 if static else 0)
    method=methods[0];o.write_u32(method+4,9 if static else 1)
    tc=[]
    for ty in (['I','F','I'] if arguments else ['I']):
        cell=o.alloc(4);o.write_u32(cell,TC[ty]);tc.append(cell)
    sig=o.alloc(12);o.write_u32s(sig,[s.jvm,0,arr(o,tc)]);o.write_u32(method+24,sig)
    body=o.alloc(24);o.write_u32s(body,[0x1007,0x1c,0x100b,77,0x2b,0]);o.write_u32(base+24,arr(o,[body]))
    args=[s._cell({'k':'v','t':'I','w':11}),s._cell({'k':'v','t':'F','w':0x40200000})] if arguments else []
    args.append(s._cell({'k':'v','t':'I','w':2 if arguments else 0}))
    s._container_set(s.frames[0]+28,args)
    caller=s.frames[0];o.write_u32(caller+0x30,0 if static else obj)
    code=o.alloc(32);o.write_u32s(code,[0x24,0,0x1011,1,0x101a,1,0x10,0]);o.write_u32(caller+44,code);o.write_u32(s.thread+36,code)
    o.call(RUN,args=[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=500000)
    vals=s._container(caller+28)
    return {'back_to_caller':o.read_u32(s.thread+0x20)==caller,'stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'payload':o.read_u32(v+8)} for v in vals]}
def literal(kind,package=True,bad=False,raw_hex=None,wrong_kind=False):
    s=Scenario();o=s.o
    h=_holder(o);o.write_u32(s.jvm+0x68,h)
    o.hook(0x41f210,lambda oc,a:0,'fastcall',0)
    o.hooks.pop(0x406140,None)
    p,ps=pool(o,[(0,'system.rpk',10),(3,0,0),(0,'hello literal',13)])
    if raw_hex is not None:
        raw=bytes.fromhex(raw_hex);data=o.alloc(len(raw)+1);o.write(data,raw+b'\0');o.write_u32(ps[2]+8,data);o.write_u32(ps[2]+12,len(raw))
    # Real constructor maps on-disk (package,index) to +c/+8 respectively.
    o.call(0x407210,args=[0 if package else 0xffffffff,27],convention='thiscall',this=ps[1])
    if bad:o.write_u32(ps[1]+4,1)
    o.write_u32(s.ctx+0x14,p)
    o.write_u32(s.ctx_q+0x24,p)  # read-only error-position class-file pool
    calls=[]
    def rid(oc,a):
        calls.append({'entry':'4122b0','package':oc.read_cstr(a[0]).decode("latin1") if a[0] else None,'local_id':a[1]})
        return 0x12340000|a[1]
    o.hook(0x4122b0,rid,'cdecl',2)
    def string_alloc(oc,a):
        calls.append({'entry':'4174a0','text':oc.read_cstr(a[0]).decode("latin1") if a[0] else None})
        if raw_hex is not None:calls[-1]['bytes_hex']=oc.read_cstr(a[0]).hex() if a[0] else None
        return 0x22220000
    o.hook(0x4174a0,string_alloc,'thiscall',1)
    if wrong_kind:
        o.write_u32(ps[2]+4,4);o.hooks.pop(0x4174a0,None)  # real null fast path
    idx=1 if kind==0x100e else 2
    s.add_frame(0,{'nodes':[[0x1007,1,0x1c],[kind,1,idx],[0x10,1,0]],'entry':0,'locals':[],'stack':[]})
    s.finish_setup()
    o.call(RUN,args=[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=500000)
    slots=s._container(s.frames[0]+0x1c)
    return {'kind':hex(kind),'package':package,'bad_type':bad,'calls':calls,'stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode("latin1"),'payload':o.read_u32(v+8)} for v in slots], 'member_memory':[o.read_u32(ps[1]+i) for i in (4,8,12)],'original_instructions_last_call':o.icount}

def native_call(ret='I',static=False):
    s,p,tr,base,leaf,obj,methods,events=setup();o=s.o
    record=o.read_u32(o.read_u32(p+12)+4*4);o.write_u32(record+8,cstr(o,'Base'))
    holder=_holder(o);o.write_u32(s.jvm+0x68,holder)
    o.hook(0x41f210,lambda oc,a:0,'fastcall',0)
    o.hooks.pop(0x406f60,None)
    mreg=o.alloc(0x62c);o.call(0x41a370,args=[mreg],convention='fastcall');o.write_u32(s.jvm+12,mreg)
    sigtext='(IF)'+ret
    desc=o.read_u32(o.read_u32(p+12)+8);o.write_u32(desc+8,cstr(o,sigtext))
    method=methods[0];o.write_u32(method+4,0x49 if static else 0x41)
    # Let real407ac0/41dc30 lazily parse realdescriptor instead of faking cache.
    impl=0x41b860
    o.call(0x416b00,args=[cstr(o,'Base'),cstr(o,'method'),cstr(o,sigtext),impl],convention='thiscall',this=s.jvm)
    args=[s._cell({'k':'v','t':'I','w':11}),s._cell({'k':'v','t':'F','w':0x40200000})]
    s._container_set(s.frames[0]+28,args)
    def native(oc,a):
        info=a[0];jvm,thread,container,record=oc.read_u32s(info,4)
        cells=s._container(container)
        events.append({'native_callback':{'jvm_matches':jvm==s.jvm,'thread_matches':thread==s.thread,'signature':sigtext,'cells':[{'type':oc.read_cstr(oc.read_u32(v+4)).decode(),'payload':('receiver' if oc.read_u32(v+8)==obj else oc.read_u32(v+8))} for v in cells]}})
        return 2.5 if ret=='F' else 0x11223344
    o.hook(impl,native,'cdecl',1)
    rec=o.alloc(16);o.write_u32s(rec,[base,0,method,1])
    try:
        r=o.call(0x41fbc0,args=[rec,base,0 if static else obj],convention='thiscall',this=s.thread,fpu_cw=CW,limit=500000)
        vals=s._container(s.frames[0]+28)
        return {'return':ret,'static':static,'return_code':r.eax,'events':events,'after_stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'payload':o.read_u32(v+8)} for v in vals]}
    except Exception as e:return {'return':ret,'static':static,'error':str(e),'events':events}


def native_cases():return [dict(ret=ret,static=static) for ret,static in [('I',False),('I',True),('F',False),('V',True),('LProbeNative;',False),('[I',True),('',False),('',True)]]
def string_identity():
    o=_new_oracle(str(REPO/'reference/raw/StreetLegal_Redline.bak.exe'))
    holder=o.alloc(0x30);o.call(E4D0,[],convention='thiscall',this=holder)
    jvm=o.alloc(0x100);o.write_u32(jvm+0x68,holder);cls=o.alloc(0x80);objects={};loads=[]
    def class_lookup(oc,args):loads.append(oc.read_cstr(args[0]).decode('latin1'));return cls
    def allocate(oc,args):obj=oc.alloc(0x30);objects[obj]=oc.alloc(12);return obj
    def ptr_field(oc,args):
        assert oc.read_cstr(args[0])==b'ptr'
        return objects[oc.reg('ecx')]
    o.hook(0x411610,class_lookup,'thiscall',1);o.hook(0x404e20,allocate,'thiscall',0);o.hook(0x408800,ptr_field,'thiscall',1)
    o.hooks.pop(0x41f210,None) # collisions must execute the real chain cursor
    words=[];observations=[]
    for data in [None,b'',b'',b'alpha',b'alpha',b'Alpha',b'alpha\0ignored',b'\xff\x80',b'\xff\x80']:
        pointer=0
        if data is not None:pointer=o.alloc(len(data)+1);o.write(pointer,data+b'\0')
        result=o.call(0x4174a0,[pointer],convention='thiscall',this=jvm,limit=100000);word=result.eax
        record=o.read_u32(objects[word]+8) if word else 0
        observations.append({'input_hex':None if data is None else data.hex(),'null':word==0,'same_as':[i for i,prior in enumerate(words) if prior==word],'allocated_objects':len(objects),'stored_hex':o.read_cstr(record).hex() if record else None,'class_loads':loads.copy()});words.append(word)
    return {'observations':observations,'scope':'real4174a0/41eb70/41e800/41ecd0/41f210; controlled loaded class, object allocation and ptr field; no active GC context'}
def signed(w):return w if w<0x80000000 else w-0x100000000
class Probe:
 def __init__(self):
  self.s=Scenario();self.o=o=self.s.o;self.names={};self.classes={};self.loads=[];self.logs=[]
  self.desc={'I':TC['I'],'F':TC['F'],'L;':TC['L'],'V':TC['V']}
  o.hook(0x406f60,lambda oc,a:a[0],'thiscall',1);o.hook(0x406140,lambda oc,a:a[0],'thiscall',1)
  def load(oc,a):
   text=oc.read_cstr(a[0]).decode('latin1');self.loads.append(text);return self.classes.get(text.removeprefix('L').removesuffix(';'),0)
  o.hook(0x412670,load,'thiscall',1)
  o.hook(0x551140,lambda oc,a:self.logs.append(oc.read_cstr(a[1]).decode('latin1')) or None,'cdecl',3)
  for name,kind,base,interfaces in [('Object',0,None,[]),('Base',0,'Object',[]),('Child',0,'Base',[]),('Other',0,'Object',[]),('I0',1,None,[]),('I1',1,None,['I0']),('I2',1,None,['I1']),('IX',1,None,[]),('Implementer',0,'Base',['I1']),('LeafImpl',0,'Implementer',[]),('java.lang.String',0,'Object',[]),('[I',0,'Object',[])]:self.add_class(name,kind,base,interfaces)
 def name(self,name):
  if name not in self.names:self.names[name]=cstr(self.o,name)
  return self.names[name]
 def descriptor(self,text):
  if text not in self.desc:self.desc[text]=self.name(text)
  p=self.o.alloc(4);self.o.write_u32(p,self.desc[text]);return p
 def add_class(self,name,kind=0,base=None,interfaces=()):
  o=self.o;p=o.alloc(0x80);o.write_u32(p+0xc,self.s.jvm);o.write_u32(p+0x38,kind);o.write_u32(p+0x40,self.name(base) if base else 0xffffffff);o.write_u32(p+0x44,len(interfaces));a=o.alloc(max(4,len(interfaces)*4));o.write_u32s(a,[self.name(n) for n in interfaces]);o.write_u32(p+0x48,a);self.classes[name]=p;return p
 def cost(self,source,target):
  r=self.o.call(0x417c30,[self.descriptor(source),self.descriptor(target)],convention='thiscall',this=self.s.jvm,fpu_cw=CW,limit=100000)
  return {'source':source,'target':target,'score':signed(r.eax),'loads':self.loads,'diagnostic_fragments':self.logs,'instructions':r.icount}
 def distance(self,source,target):
  r=self.o.call(0x405400,[self.classes[target]],convention='thiscall',this=self.classes[source],fpu_cw=CW,limit=100000)
  return {'source':source,'target':target,'distance':signed(r.eax),'loads':self.loads,'diagnostic_fragments':self.logs,'instructions':r.icount}

class Selection(Probe):
 def __init__(self):
  super().__init__();o=self.o;o.write_u32(self.s.jvm+0x68,_holder(o));o.write_u32(self.s.jvm+0x74,self.name('synthetic-overload-fixture'));o.write_u32(self.s.jvm+0x70,1);self.methods=[]
  o.hook(0x406170,lambda oc,a:self.name(oc.read_cstr(a[0]).decode()),'thiscall',1)
  for p in self.classes.values():o.write_u32(p+0x50,arr(o,[],True));o.write_u32(p+0x58,arr(o,[],True))
  # Keep real log generation harmless while retaining ambiguity marker.
  o.hook(0x551220,lambda oc,a:oc.write(a[0],b'0\0') or None,'cdecl',3)
 def owner(self,name,base=None,static=(),instance=()):
  o=self.o;p=self.add_class(name,0,base)
  for kind,sigs,off in [('static',static,0x58),('instance',instance,0x50)]:
   methods=[]
   for i,spec in enumerate(sigs):
    method_name,signature=(spec if isinstance(spec,list) else ('m',spec))
    m=o.alloc(28);o.write_u32s(m,[0,9 if kind=='static' else 1,self.name(method_name),self.name(signature),i,0,0]);methods.append(m);self.methods.append((name,kind,i,signature,m))
   o.write_u32(p+off,arr(o,methods,True))
  return p
 def select(self,actual,static=(),instance=(),parent_static=(),parent_instance=(),name='m'):
  parent=self.owner('Parent',None,parent_static,parent_instance);owner=self.owner('Owner','Parent',static,instance)
  result=self.o.call(0x405920,[self.name(name),self.name(actual)],convention='thiscall',this=owner,fpu_cw=CW,limit=500000)
  idx=result.eax;kind='static' if idx&0x40000000 else 'instance';offset=idx&0xbfffffff;selected=None
  if idx!=0xffffffff:
   candidates=[x for x in self.methods if x[1]==kind]
   # Global indices are base-first, even though search goes derived-first.
   order=[x for n in ['Parent','Owner'] for x in candidates if x[0]==n]
   if offset<len(order):selected=list(order[offset][:4])
  return {'actual':actual,'static':static,'instance':instance,'parent_static':parent_static,'parent_instance':parent_instance,'name':name,'index_hex':hex(idx),'selected':selected,'diagnostic_fragments':self.logs,'loads':self.loads,'instructions':result.icount}
OVERLOAD_CASES=[dict(actual='(I)',instance=['(F)I','(I)I']),dict(actual='(I)',instance=['(I)I','(F)I']),dict(actual='(F)',instance=['(I)I']),dict(actual='(L;)',instance=['(LBase;)I','(LChild;)I']),dict(actual='(L;)',instance=['(LChild;)I','(LBase;)I']),dict(actual='(I)',instance=['(F)I','(F)V']),dict(actual='(I)',instance=['(I)I','(I)V']),dict(actual='(I)',static=['(I)I'],instance=['(I)I']),dict(actual='(I)',static=['(F)I'],instance=['(I)I']),dict(actual='(I)',instance=['(F)I'],parent_instance=['(I)I']),dict(actual='(I)',instance=['(F)I'],parent_instance=['(F)I']),dict(actual='(II)',instance=['(FI)I','(IF)I']),dict(actual='(LChild;)',instance=['(LObject;)I','(LBase;)I']),dict(actual='(LChild;)',instance=['(LBase;)I','(LChild;)I']),dict(actual='(LChild;)',instance=['(LOther;)I']),dict(actual='(I)',instance=['(II)I']),dict(actual='(I)',instance=['(I)I'],name='missing'),dict(actual='(I)',instance=[['<init>','(F)'],['<init>','(I)']],name='<init>'),dict(actual='(LImplementer;)',instance=['(LBase;)I','(LI0;)I']),dict(actual='(LImplementer;)',instance=['(LImplementer;)I','(LI0;)I']),dict(actual='(LImplementer;)',instance=['(LI0;)I','(LImplementer;)I'])]
OVERLOAD_CASES.extend([dict(actual='(I)',static=['(F)I'],instance=['(F)I']),dict(actual='(I)',static=['(I)I'],instance=['(F)I'],parent_instance=['(I)I']),dict(actual='(I)',static=['(I)I'],instance=['(LOther;)I'],parent_instance=['(I)I']),dict(actual='(I)',static=['(I)I'],parent_instance=['(I)I']),dict(actual='(I)',static=['(I)I','(I)V']),dict(actual='(IF)',instance=['(FI)I','(FF)I','(II)I']),dict(actual='(I)',instance=['(F)I'],parent_static=['(F)I'])])

class Runtime(Selection):
 def runtime(self,declared='LBase;',actual='Child',null=False,method_name='m',static=False,no_class=False):
  o=self.o;s=self.s;o.hooks.pop(0x551140,None);self.owner('Parent');self.owner('Owner','Parent',static=['(LBase;)I','(LChild;)I'] if static else [],instance=[] if static else ['(LBase;)I','(LChild;)I']);objects={}
  for name in ['Parent','Owner','Base','Child']:
   cl=o.alloc(0x200);o.write_u32(cl+4,self.classes[name]);o.write_u32(cl+0xc,s.jvm);desc=o.alloc(4);o.write_u32(desc,self.name('L'+name+';'));o.write_u32(cl+8,desc);objects[name]=cl
  o.write_u32(objects['Owner']+0x1c8,objects['Parent'])
  oldfatal=o.hooks[0x4190d0];o.hook(0x4190d0,lambda oc,a:self.logs.append(oc.read_cstr(a[1]).decode('latin1')) or oldfatal[0](oc,a),oldfatal[1],oldfatal[2]);o.hook(0x5513b0,lambda oc,a:self.logs.append(oc.read_cstr(a[0]).decode('latin1')) or None,'cdecl',1)
  o.hook(0x403e80,lambda oc,a:1,'thiscall',0)
  o.hook(0x406010,lambda oc,a:a[0],'thiscall',2)
  obj=o.alloc(32);o.write_u32(obj+0xc,objects[actual]);receiver=o.alloc(32);o.write_u32(receiver+0xc,objects['Owner'])
  frame=s.add_frame(0,{'nodes':[[0x10,1,0]],'entry':0,'locals':[],'stack':[]});s.finish_setup();o.write_u32(frame+0x34,objects['Owner']);o.write_u32(frame+0x30,receiver)
  cell=s._cell({'k':'v','t':'L','w':0 if null else obj});o.write_u32(cell+4,self.name(declared));count=s._cell({'k':'v','t':'I','w':1});s._container_set(frame+0x1c,[cell,count]);calls=[]
  def dispatch(oc,a):
   rec=oc.read_u32s(a[0],4);selected=next((list(x[:4]) for x in self.methods if x[4]==rec[2]),None);calls.append({'selected':selected,'record_owner':next((n for n,p in objects.items() if p==rec[0]),None),'call_class':next((n for n,p in objects.items() if p==a[1]),None),'receiver_matches_expected':a[2]==(0 if static else receiver)});return 0x12345678
  o.hook(0x41fbc0,dispatch,'thiscall',3)
  try:
   r=o.call(0x4207c0,[0 if no_class else objects['Owner'],0 if static else receiver,self.name(method_name)],convention='thiscall',this=s.thread,fpu_cw=CW,limit=500000);result=hex(r.eax);outcome='return';icount=r.icount
  except ScriptFatal:
   result=None;outcome='fatal';icount=o.icount
  return {'declared':declared,'actual':actual,'null':null,'method_name':method_name,'static':static,'no_class':no_class,'outcome':outcome,'return':result,'calls':calls,'remaining_arguments':len(s._container(frame+0x1c)),'diagnostic_fragments':self.logs,'instructions':icount}

def runtime_case(**case):return Runtime().runtime(**case)
def runtime_cases():return [dict(),dict(null=True),dict(declared='L;',null=True),dict(declared='LOther;',actual='Child'),dict(actual='Base'),dict(method_name='missing'),dict(static=True),dict(no_class=True)]
def select_case(**case):return Selection().select(**case)
def new_route(statement=False):
    s,p,tref,base,leaf,obj,methods,events=setup();o=s.o
    # Requested class token is a real pool Str; target class/instance allocation
    # is deliberately supplied at the class-loader/allocator boundaries.
    text=cstr(o,'Leaf');record=o.read_u32(o.read_u32(p+12));o.write_u32(record+8,text)
    def load(oc,a):
        events.append({'seam':'load_class','name':oc.read_cstr(a[0]).decode()});return leaf
    def allocate(oc,a):
        events.append({'seam':'allocate_instance','this_is_leaf':oc.reg('ecx')==leaf});return obj
    o.hook(0x410890,load,'thiscall',1);o.hook(0x404e20,allocate,'thiscall',0)
    body=o.alloc(24);o.write_u32s(body,[0x1008 if statement else 0x1007,0x1e,0x1006,0,0x10,0]);o.write_u32(s.frames[0]+44,body);o.write_u32(s.thread+36,body)
    try:
        o.call(RUN,args=[0.],convention='thiscall',this=s.thread,fpu_cw=CW)
        vals=s._container(s.frames[0]+28)
        return {'operation':'new','events':events,'stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'is_supplied_leaf_instance':o.read_u32(v+8)==obj} for v in vals]}
    except Exception as e:return {'operation':'new','error':str(e),'events':events}

def constructor_route(op=0x21,missing_super=False):
    s,p,tref,base,leaf,obj,methods,events=setup();o=s.o;caller=s.frames[0]
    o.write_u32(caller+0x30,obj)
    if missing_super:o.write_u32(leaf+0x1c8,0)
    def dispatch(oc,a):
        events.append({'thread_matches':oc.reg('ecx')==s.thread,'class':'Leaf' if a[0]==leaf else 'Base' if a[0]==base else hex(a[0]),'receiver_matches':a[1]==obj,'name':oc.read_cstr(a[2]).decode()});return 1
    o.hook(0x4207c0,dispatch,'thiscall',3)
    stack=[s._cell({'k':'v','t':'L','w':obj})] if op==0x21 else []
    stack.append(s._cell({'k':'v','t':'I','w':0}));s._container_set(caller+28,stack)
    code=o.alloc(16);o.write_u32s(code,[op,0,0x10,0]);o.write_u32(caller+44,code);o.write_u32(s.thread+36,code)
    o.call(RUN,[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=100000)
    return {'dispatch':events,'scope':'actual420ff0 constructor opcodes and408a30/408a70/408a90; final4207c0 dispatch recording boundary'}

def named_method_route(op=0x4025,null=False,missing_super=False):
    s,p,tref,base,leaf,obj,methods,events=setup();o=s.o;caller=s.frames[0]
    o.write_u32(caller+0x30,obj)
    if missing_super:o.write_u32(leaf+0x1c8,0)
    def dispatch(oc,a):
        events.append({'class':'Leaf' if a[0]==leaf else 'Base' if a[0]==base else hex(a[0]),'receiver_matches':a[1]==obj,'name':oc.read_cstr(a[2]).decode()});return 1
    if not missing_super:o.hook(0x4207c0,dispatch,'thiscall',3)
    stack=[s._cell({'k':'v','t':'I','w':0})]
    if op==0x4025:stack.append(s._cell({'k':'v','t':'L','w':0 if null else obj}))
    s._container_set(caller+28,stack)
    code=o.alloc(16);o.write_u32s(code,[op,0,0x10,0]);o.write_u32(caller+44,code);o.write_u32(s.thread+36,code)
    try:o.call(RUN,[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=100000);outcome='return'
    except ScriptFatal:outcome='fatal'
    return {'dispatch':events,'outcome':outcome,'remaining_stack':len(s._container(caller+28)),'scope':'actual420ff0/408a30 routing; valid final4207c0 recording boundary, missing-super actual fatal path'}


def array_world(desc):
 s=Scenario();o=s.o;o.hooks.pop(0x406140,None);o.write_u32(s.jvm+0x68,_holder(o))
 p,_=pool(o,[(0,desc,len(desc)),(0,'length',6),(0,'I',1)])
 o.write_u32(s.ctx+0x14,p);o.write_u32(s.ctx_q+0x24,p)
 owner=o.alloc(0x40);head=o.alloc(0x40);o.write_u32(owner+0x18,head);o.write_u32(s.jvm+0x10,owner)
 cl=o.alloc(0x200);mod=o.alloc(0x80);o.write_u32(cl+4,mod);o.write_u32(cl+12,s.jvm);o.write_u32(cl+0x14,p);o.write_u32(cl+0x18,o.alloc(16));o.write_u32(cl+0x1d0,o.alloc(20));o.write_u32(mod+0x24,p)
 typecell=o.alloc(4);o.write_u32(typecell,cstr(o,desc));o.write_u32(cl+8,typecell)
 field=o.alloc(20);o.write_u32s(field,[0,0x21,1,2,0xffffffff]);o.write_u32(mod+0x4c,arr(o,[field],True))
 o.write_u32(0x62dee8,0);o.write_u32(0x62dffc,1)
 o.hook(0x403e80,lambda oc,a:1,'thiscall',0)
 events=[]
 def arrayclass(oc,a):events.append(oc.read_cstr(a[0]).decode());return cl
 o.hook(0x4109e0,arrayclass,'thiscall',1)
 return s,events

def array_construct(desc='[I',length=3):
 s,events=array_world(desc);o=s.o
 s.add_frame(0,{'nodes':[[0x1007,1,0x1f],[0x1006,1,0],[0x10,1,0]],'entry':0,'locals':[],'stack':[{'k':'v','t':'I','w':length}]});s.finish_setup()
 try:o.call(RUN,[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=1000000)
 except ScriptFatal:return {'outcome':'fatal','remaining_stack':len(s._container(s.frames[0]+28)),'scope':'real420ff0 descriptor guard; array class lookup boundary is not reached'}
 value=s._container(s.frames[0]+28)[0];obj=o.read_u32(value+8)
 fields=s._container(o.read_u32(o.read_u32(obj+16)+12))
 container=o.read_u32(fields[1]+8);elements=s._container(container)
 return {'outcome':'return','scope':'real420ff0/404e60/407da0 typed cells; loaded array-class lookup4109e0 and class-init403e80 boundaries','descriptor':o.read_cstr(o.read_u32(value+4)).decode(),'length':o.read_u32(fields[0]+8),'calls':events,'cells':[{'kind':'variable' if o.read_u32(cell)==VAR_VT else 'other','type':o.read_cstr(o.read_u32(cell+4)).decode(),'payload':o.read_u32(cell+8)} for cell in elements]}

def arg_array(desc='[I',items=None,target=True,target_kind='var'):
 items=[] if items is None else items
 s,events=array_world(desc);o=s.o
 frame=s.add_frame(0,{'nodes':[[0x101e,1,len(items)],[0x10,1,0]],'entry':0,'locals':[],'stack':[]});s.finish_setup()
 variable=s._cell({'k':target_kind,'t':'L','w':0}) if target else 0
 if variable:o.call(0x423e10,[s.jvm,cstr(o,desc)],convention='thiscall',this=variable+4,fpu_cw=CW)
 cells=[s._cell({'k':item.get('kind','v'),'t':item['type'],'w':item['payload']}) for item in items]
 s._container_set(frame+28,([variable] if target else [])+cells)
 try:o.call(RUN,[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=1000000);outcome='return'
 except ScriptFatal:outcome='fatal'
 stack=s._container(frame+28)
 result={'outcome':outcome,'target_preserved':bool(target and stack and stack[0]==variable),'remaining_stack':len(stack),'calls':events,'scope':'actual420ff0/101e copy, typed assignment,404e60/407da0; loadedarrayclass4109e0/classinit boundaries'}
 if outcome=='return':
  value=stack[-1];obj=o.read_u32(value+8);result['type']=o.read_cstr(o.read_u32(value+4)).decode();result['null']=obj==0
  if obj:
   fields=s._container(o.read_u32(o.read_u32(obj+16)+12));elements=s._container(o.read_u32(fields[1]+8));result['length']=o.read_u32(fields[0]+8);result['cells']=[{'type':o.read_cstr(o.read_u32(cell+4)).decode(),'payload':o.read_u32(cell+8)} for cell in elements]
 result['variables_after']=[{'index':index,'type':o.read_cstr(o.read_u32(cell+4)).decode(),'payload':o.read_u32(cell+8)} for index,(item,cell) in enumerate(zip(items,cells)) if item.get('kind')=='var']
 return result

def arg_cases():return [dict(target=False),dict(),dict(desc='I'),dict(items=[dict(type='I',payload=11),dict(type='I',payload=22),dict(type='I',payload=33)]),dict(desc='[F',items=[dict(type='I',payload=11,kind='var'),dict(type='F',payload=0x40200000)]),dict(desc='[I',items=[dict(type='F',payload=0x40200000,kind='var'),dict(type='I',payload=11)]),dict(desc='[Ljava.lang.Object;',items=[dict(type='L',payload=0),dict(type='L',payload=0,kind='var')]),dict(target_kind='v',items=[dict(type='I',payload=11)])]

def array_cases():return [dict(desc=desc,length=length) for desc,length in [('[I',3),('[F',3),('[Ljava.lang.Object;',3),('[I',0),('[F',0),('[[I',3),('I',3)]]
def cast_operation(operation='cast',source='LBase;',actual='Child',target='LBase;',null=False,payload=11):
 p=Probe();p.add_class('StringSub',0,'java.lang.String',[]);s=p.s;o=p.o;o.write_u32(s.jvm+0x68,_holder(o));o.hooks.pop(0x41f210,None);o.hooks.pop(0x551220,None)
 runtime={}
 for name,meta in p.classes.items():
  cl=o.alloc(0x200);o.write_u32s(cl+4,[meta,p.descriptor('L'+name+';'),s.jvm,o.read_u32(meta+0x38)]);o.write_u32(cl+0x18,o.alloc(16));runtime[name]=cl
 for name,cl in runtime.items():
  meta=p.classes[name];parent=o.read_u32(meta+0x40)
  if parent!=0xffffffff:o.write_u32(cl+0x1c8,runtime[o.read_cstr(parent).decode()])
  n=o.read_u32(meta+0x44);data=o.read_u32(meta+0x48);o.write_u32(cl+0x1cc,arr(o,[runtime[o.read_cstr(o.read_u32(data+i*4)).decode()] for i in range(n)]))
 calls=[]
 def find(oc,a):
  name=oc.read_cstr(a[0]).decode();calls.append({'find':name});return runtime.get(name.removeprefix('L').removesuffix(';'),0)
 o.hook(0x411610,find,'thiscall',1);o.hook(0x403e80,lambda oc,a:1,'thiscall',0)
 def string(oc,a):calls.append({'string':oc.read_cstr(a[0]).decode('latin1')});return 0x22220000
 o.hook(0x4174a0,string,'thiscall',1)
 obj=o.alloc(32);o.write_u32(obj+12,runtime[actual])
 word=(0 if null else obj) if source[0] in 'L[' else payload
 cell=s._cell({'k':'v','t':source[0] if source[0] in TC else 'L','w':word})
 o.call(0x423e10,[s.jvm,p.name(source)],convention='thiscall',this=cell+4,fpu_cw=CW)
 frame=s.add_frame(0,{'nodes':[[0x1007,1,0x20 if operation=='instanceof' else 0x21],[0x1006,1,p.name(target)],[0x10,1,0]],'entry':0,'locals':[],'stack':[]});s.finish_setup();s._container_set(frame+28,[cell])
 try:o.call(RUN,[0.],convention='thiscall',this=s.thread,fpu_cw=CW,limit=1000000);outcome='return'
 except ScriptFatal:outcome='fatal'
 vals=s._container(frame+28)
 return {'outcome':outcome,'calls':calls,'stack':[{'type':o.read_cstr(o.read_u32(v+4)).decode(),'payload':'object' if o.read_u32(v+8)==obj else o.read_u32(v+8)} for v in vals]}

def cast_cases():return [dict(),dict(target='LChild;'),dict(target='LOther;'),dict(source='LOther;',actual='Child',target='LBase;'),dict(source='LBase;',actual='Other',target='LBase;'),dict(null=True,target='LChild;'),dict(source='I',target='F',payload=11),dict(source='F',target='I',payload=0x40200000),dict(source='I',target='Ljava.lang.String;'),dict(source='F',target='Ljava.lang.String;',payload=0x40200000),dict(operation='instanceof',target='Base'),dict(operation='instanceof',target='Other'),dict(operation='instanceof',actual='Implementer',target='I0'),dict(source='I',target='LObject;'),dict(source='I',target='LStringSub;'),dict(source='I',target='Ljava.lang.String;',payload=0x80000000),dict(source='F',target='Ljava.lang.String;',payload=0x80000000),dict(source='F',target='Ljava.lang.String;',payload=0x7fc00001),dict(source='L;',null=True,target='V'),dict(source='V',target='V',payload=11),dict(source='V',target='Ljava.lang.String;',payload=0)]
def invalid_cases():return [dict(kind=0x100e,bad=True)]
def resource_cases():return [dict(kind=0x100e),dict(kind=0x100e,package=False)]
GROUPS={'wrong_kind_literal':(literal,lambda:[dict(kind=0x4009,wrong_kind=True)]),'raw_literals':(literal,lambda:[dict(kind=0x4009,raw_hex=raw) for raw in ['ff','ff80','ff0041']]),'arg_arrays':(arg_array,arg_cases),'casts':(cast_operation,cast_cases),'arrays':(array_construct,array_cases),'named_methods':(named_method_route,lambda:[dict(op=0x4025),dict(op=0x4026),dict(op=0x4025,null=True),dict(op=0x4026,missing_super=True)]),'constructors':(constructor_route,lambda:[dict(op=0x21),dict(op=0x22),dict(op=0x23),dict(op=0x23,missing_super=True)]),'new':(new_route,lambda:[dict(statement=False),dict(statement=True)]),'runtime':(runtime_case,runtime_cases),'overload':(select_case,lambda:OVERLOAD_CASES),'string_literal':(literal,lambda:[dict(kind=0x4009)]),'string_identity':(string_identity,lambda:[{}]),'native':(native_call,native_cases),'frames':(frame_call,frame_cases),'calls':(resolved_call,frame_cases),'resource_invalid':(literal,invalid_cases),'resource':(literal,resource_cases)}
# Executed paths are group-specific; recording/allocation boundaries remain
# documented in each result's scope and generator callback setup.
ADDRESSES={
 'wrong_kind_literal':'0x00420ff0/0x00406140/0x004173d0/0x004174a0',
 'raw_literals':'0x00420ff0/0x004173d0',
 'arg_arrays':'0x00420ff0/0x00404e60/0x00407da0',
 'casts':'0x00420ff0/0x00417da0/0x004044e0/0x00405400',
 'arrays':'0x00420ff0/0x00404e60/0x00407da0',
 'named_methods':'0x00420ff0/0x00408a30',
 'constructors':'0x00420ff0/0x00408a30/0x00408a70/0x00408a90',
 'new':'0x00420ff0',
 'runtime':'0x004207c0/0x0041dcb0/0x00404910/0x00405920/0x00404d40',
 'overload':'0x00405920/0x00417c30/0x00405400',
 'string_literal':'0x00420ff0/0x004173d0',
 'string_identity':'0x004174a0/0x0041eb70/0x0041e800/0x0041ecd0/0x0041f210',
 'native':'0x0041fbc0/0x00416b00/0x00407ac0',
 'frames':'0x0041fbc0/0x00420ff0',
 'calls':'0x00420ff0/0x004208e0/0x0041fbc0',
 'resource_invalid':'0x00420ff0/0x00406660',
 'resource':'0x00420ff0/0x00406660/0x00407210',
}
def main():
 command=sys.argv[1] if len(sys.argv)>1 else 'gen'
 if command=='verify':
  selected=list(map(Path,sys.argv[2:]));paths=selected or [OUT/f'vmexec_{name}.jsonl' for name in GROUPS];count=0
  for path in paths:
   name=path.stem.removeprefix('vmexec_')
   if name not in GROUPS:raise SystemExit(f'unknown vector suite: {path}')
   records=[json.loads(line) for line in path.read_text().splitlines()]
   if not records:raise SystemExit(f'empty vectors: {path}')
   cases=GROUPS[name][1]();seen=set()
   if not selected and len(records)!=len(cases):raise SystemExit(f'incomplete {name}')
   for row in records:
    index=row['id']
    assert isinstance(index,int) and 0<=index<len(cases) and index not in seen,('bad/duplicate row id',path,index)
    seen.add(index)
    assert row['name']==name and row['input']==cases[index],('manifest mismatch',path,index)
    assert row['oracle']=='unicorn' and row['exe_sha256']==EXE_SHA256 and row['exe_addr']==ADDRESSES[name] and row['cw']==hex(CW),('provenance mismatch',path,index)
    actual=json.loads(json.dumps(GROUPS[name][0](**row['input'])));assert 'error' not in actual,actual;assert actual==row['result'],(path,index,actual,row['result']);count+=1
  if not count:raise SystemExit('no compared rows')
  print(f'verify: {count}/{count} identical')
 else:
  OUT.mkdir(parents=True,exist_ok=True)
  for name in (GROUPS if command=='gen' else [command]):
   rows=[dict(id=i,name=name,input=case,result=GROUPS[name][0](**case),oracle='unicorn',exe_sha256=EXE_SHA256,exe_addr=ADDRESSES[name],cw=hex(CW),date=datetime.date.today().isoformat()) for i,case in enumerate(GROUPS[name][1]())]
   assert all('error' not in row['result'] for row in rows),rows
   path=OUT/f'vmexec_{name}.jsonl';path.write_text(''.join(json.dumps(row,sort_keys=True)+'\n' for row in rows));print(f'{path}: {len(rows)} rows')
if __name__=='__main__':main()
