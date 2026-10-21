#!/usr/bin/env python3
"""Original47d7b0/47d900 marshalling of distinct constructor text and name.

Only receiver-field lookup and53a5e0/53a2a0 construction are controlled. The
original45d910 marshaller and native wrapper execute; constructor hooks record
arguments then return null so no unimplemented script graph is fabricated.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from render_vectors import _cell,_frame

SIG='(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)'
def run(native=False,default_owner=False,parameters='ctor=3.125; paint=0x1234',name='named-instance-distinct'):
 o=Oracle(fpu_cw=CW_D3D9)
 receiver,handle,owner,template,owner_resource=[o.alloc(0x100) for _ in range(5)]
 o.write_u32(owner+12,owner_resource)
 o.write_u32(0x636460+12,owner_resource)
 def text(value):
  if value is None:return 0
  p=o.alloc(len(value)+1);o.write(p,value.encode()+b'\0');return p
 parameter_word=text(parameters);name_word=text(name)
 o.hook(0x42ab50,lambda oc,a:handle if a[0]==receiver else 0,'cdecl',2)
 calls=[]
 def construct(oc,args):
  calls.append(args);return 0
 o.hook(0x53a2a0 if native else 0x53a5e0,construct,'cdecl',5 if native else 4)
 values=[0 if default_owner else owner,template,parameter_word,name_word]
 cells=[_cell(o,'L',v) for v in reversed(values)]+[_cell(o,'L',receiver)]
 method='create_native' if native else 'create'
 sig=SIG+('V' if native else 'Ljava.lang.GameType;')
 result=o.call(0x47d900 if native else 0x47d7b0,[_frame(o,'java.util.resource.GameRef',method,sig,cells)],fpu_cw=CW_D3D9).eax
 expected=[0x636460 if default_owner else owner,template]+([0] if native else [])+[parameter_word,name_word]
 assert calls==[expected],(method,calls,expected)
 assert o.read_u32(handle+12)==0
 return dict(method=method,default_owner=default_owner,parameters=parameters,name=name,call_args=calls[0],parameter_argument=3 if native else 2,name_argument=4 if native else 3,parameters_forwarded=calls[0][-2]==parameter_word,name_forwarded=calls[0][-1]==name_word,script_argument_zero=not native or calls[0][2]==0,result=result)
def main():
 digest=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()
 assert digest=='7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
 rows=[]
 for native in [False,True]:
  for owner in [False,True]:
   for parameters,name in [('ctor=3.125; paint=0x1234','named-instance-distinct'),(None,'name-only'),('ctor-only',None)]:rows.append(run(native,owner,parameters,name))
 print(json.dumps(dict(exe_sha256=digest,scope=__doc__,rows=rows),indent=2))
if __name__=='__main__':main()
