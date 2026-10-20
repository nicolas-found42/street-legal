#!/usr/bin/env python3
"""Controlled original4255e0 callback ABI/name/lifetime probe; no script replay."""
import hashlib,json,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,CW_D3D9
EXE=Path(__file__).resolve().parents[2]/'reference/raw/StreetLegal_Redline.bak.exe'
assert hashlib.sha256(EXE.read_bytes()).hexdigest()=='7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
def run(request_return,dispatch_result=0):
 o=Oracle(exe=str(EXE),fpu_cw=CW_D3D9);o.hook_alloc();events=[]
 def mem(n):return o.alloc(n)
 def text(s):
  p=mem(len(s)+1);o.write(p,s.encode()+b'\0');return p
 def cstr(p):return o.read(p,100).split(b'\0',1)[0].decode()
 owner=mem(128);handle=mem(128);vtable=mem(32);thread=mem(64);tv=mem(16);cell=mem(12);cv=mem(12)
 o.write_u32(owner+0x50,0x11111111);o.write_u32(owner+0x44,handle);o.write_u32(handle,vtable);o.write_u32(handle+0x4c,1);o.write_u32(vtable+0xc,0x300f0010)
 o.write_u32(thread,tv);o.write_u32(tv,0x300f0020);o.write_u32(cell,cv);o.write_u32(cell+4,text('I'));o.write_u32(cell+8,17);o.write_u32(cv+4,0x300f0030);o.write_u32(0x63641c,0x12345678)
 def hook(addr,n,result=0,conv='cdecl',name=None):
  def call(oc,args):
   e={'call':name or hex(addr),'this':oc.reg('ecx'),'args':args};events.append(e)
   if addr==0x41f340:e['name']=cstr(args[3])
   return result(oc,args) if callable(result) else result
  o.hook(addr,call,conv,n)
 hook(0x5447d0,3,0,'thiscall');hook(0x300f0010,1,owner,'thiscall','resource.data');hook(0x48adb0,0,text('Ljava.render.Osd;'),'thiscall')
 hook(0x41f340,4,thread,'thiscall','thread.create');hook(0x425240,3,0,name='arguments');hook(0x408a30,2,dispatch_result,'thiscall','dispatch');hook(0x420ff0,1,0.0,'thiscall','run');hook(0x41f7d0,0,cell,'thiscall','return.pop');hook(0x300f0030,1,0,'thiscall','cell.release');hook(0x41f780,0,0,'thiscall','thread.die');hook(0x300f0020,1,0,'thiscall','thread.destroy')
 method=text('getInfo' if request_return else 'animate');result=o.call(0x425a90,[owner,method,request_return,0x4f],limit=100000)
 create=next(e for e in events if e['call']=='thread.create');assert create['args'][1:3]==[10,request_return];assert create['name']=='THRD-RUNVMI Ljava.render.Osd;.'+('getInfo' if request_return else 'animate')
 assert any(e['call']=='thread.destroy' for e in events)==bool(request_return or dispatch_result)
 assert any(e['call']=='run' for e in events)==(dispatch_result==0)
 assert result.eax==(17 if request_return or dispatch_result else 0)
 return {'request_return':request_return,'dispatch_result':dispatch_result,'result':result.eax,'events':events}
def player_empty_frames():
 o=Oracle(exe=str(EXE),fpu_cw=CW_D3D9);o.hook_alloc();rows=[]
 for fill in (0,0xa5):
  body=o.alloc(0x2800);o.write(body,bytes([fill])*0x2800);o.write_u32(0x63c648,1)
  o.call(0x436370,convention='thiscall',this=body,limit=1000000)
  fields={hex(offset):o.read_u32(body+offset) for offset in (0x21ac,0x230c,0x2424,0x250c)}
  refs=[o.read_u32(body+0x2468+i*16) for i in range(8)]
  assert fields['0x21ac']==0 and refs==[0]*8
  assert fields['0x230c']==fill*0x01010101
  rows.append({'initial_fill_byte':fill,'constructor':0x436370,'fields':fields,'render_refs':refs})
  if fill==0:
   o.write_u32(0x63c874,0x12345678);o.write_u32(0x6439c0,0x11223344)
   o.call(0x47a5f0,[body,0x3dcccccd],convention='stdcall',limit=10000)
   assert o.read_u32(0x63c874)==0x12345678
   o.call(0x47a780,[body,0x80000000],convention='stdcall',limit=10000)
   assert o.read_u32(0x63c874)==0x80000000 and o.read_u32(0x6439c0)==0x11223344
   rows[-1]['empty_frame_result']={'animation_dt_bits':o.read_u32(0x63c874),'physics_counter_untouched':o.read_u32(0x6439c0)}
 return rows

def player_registration():
 o=Oracle(exe=str(EXE),fpu_cw=CW_D3D9);o.hook_alloc();target=o.read_u32(0x5f12f0+0x3c);assert target==0x47b220;native=o.alloc(0x2600)
 rows=[]
 for kind in [8,9,28]:
  result=o.call(target,[native,kind,0],convention='stdcall',limit=10000)
  rows.append({'kind':kind,'result':result.eax,'dispatch_byte':o.read_u8(0x47b428+kind-8)})
 assert [row['result'] for row in rows]==[1,0,1]
 return {'vtable':0x5f12f0,'getInfo':target,'rows':rows,'scope':'original native switch constants; no script or input samples'}
if __name__=='__main__':print(json.dumps({'exe_sha256':hashlib.sha256(EXE.read_bytes()).hexdigest(),'scope':'controlled original wrapper, original string appender; recorded dispatch/interpreter boundaries','rows':[run(0),run(1),run(0,1)],'native_player_registration':player_registration(),'native_player_empty_frames':player_empty_frames()},indent=2))
