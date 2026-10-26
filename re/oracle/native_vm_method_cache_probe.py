#!/usr/bin/env python3
"""Original404910 cached method lookup, including equal-key memo chains.

Setup loads authored TUFA with original411b20 and warms all three methods with
original404910. The measured driver supplies randomized retained caller GPRs
and stack bytes. No method/native/script result hook is installed. Every heap,
stack, global byte, heap cursor and GPR is captured; derived data stays local.
"""
from pathlib import Path
import hashlib,json,random,struct,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,STACK_TOP,HEAP_BASE
from unicorn import UC_HOOK_CODE,UC_HOOK_MEM_WRITE
ROOT=Path(__file__).resolve().parents[2]
STACK=STACK_TOP-0x1000;CALLER=0x30000000
REGS=('eax','ebx','ecx','edx','esi','edi','ebp','esp')
def words(values):return struct.pack('<'+'I'*len(values),*values)
def capture(seed,selected):
 s=Scenario();o=s.o
 for address in [0x406f60,0x406140,0x551140]:o.hooks.pop(address,None)
 o.hook(0x54be80,lambda oc,args:0,'thiscall',1)
 o.call(0x40fb50,[],convention='thiscall',this=s.jvm,fpu_cw=CW);o.hooks.pop(0x54be80)
 holder=o.alloc(8);o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW);o.write_u32(s.jvm+0x68,holder)
 strings=['Root','methodA','methodB','()I','(I)'];pool=words([6])
 for i,text in enumerate(strings):
  data=text.encode();pool+=words([0,len(data)])+data+b'\0'
  if i==0:pool+=words([4,0])
 # Root=0,Class(0)=1,A=2,B=3,()I=4,(I)=5. All three keys are7*3.
 methods=words([3])+words([0x49,2,4,0xffffffff,0])+words([0x49,3,4,0xffffffff,0])+words([0x49,2,5,0xffffffff,0])+words([0])
 source=b'TUFA'+words([4,0x11ae1])
 for tag,data in [(b'CONS',pool),(b'FILD',words([0,0])),(b'MTHD',methods),(b'CLSS',words([0xffffffff,0,1,0xffffffff,0])),(b'TREE',words([0]))]:source+=tag+words([len(data)])+data
 p=o.alloc(len(source));o.write(p,source);cl=o.call(0x411b20,[p,len(source)],convention='thiscall',this=s.jvm,fpu_cw=CW).eax
 module=o.read_u32(cl+4);pool=o.read_u32(module+0x24);table=o.read_u32(pool+12)
 text=lambda i:o.read_u32(o.read_u32(table+i*4)+8)
 pairs=[(text(2),text(4)),(text(3),text(4)),(text(2),text(5))]
 expected=[o.call(0x404910,list(args),convention='thiscall',this=cl,fpu_cw=CW).eax for args in pairs]
 name,descriptor=pairs[selected];rng=random.Random(seed);o.write(STACK,rng.randbytes(0x1000))
 incoming={r:rng.getrandbits(32)for r in REGS[:-1]};incoming['ecx']=cl;code=b''
 for r,opcode in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)]:code+=bytes([opcode])+words([incoming[r]])
 code+=b'\x68'+words([descriptor])+b'\x68'+words([name]);at=CALLER+len(code);code+=b'\xe8'+words([(0x404910-at-5)&0xffffffff]);ret=CALLER+len(code);o.write(CALLER,code)
 areas=[('heap',HEAP_BASE,(o.heap_ptr+0xfff&~0xfff)-HEAP_BASE),('stack',STACK,0x1000),('globals',0x62ddf0,0x1350)]
 states={};started=False
 def state():return dict(registers={r:o.reg(r)for r in REGS},heap_ptr=o.heap_ptr,**{key:o.read(address,n).hex()for key,address,n in areas})
 def observe(uc,pc,size,_):
  nonlocal started
  if pc==0x404910 and not started:started=True;states['before']=state()
  if started and pc==ret:states['after']=state();uc.emu_stop()
 def write(uc,access,address,n,value,_):
  if started:assert any(base<=address and address+n<=base+size for _,base,size in areas),f'out of profile write {address:x}'
 o.uc.hook_add(UC_HOOK_CODE,observe);o.uc.hook_add(UC_HOOK_MEM_WRITE,write);o.uc.ctl_remove_cache(0x400000,0x600000)
 try:o.call(CALLER,[],fpu_cw=CW)
 except Exception:
  if 'after'not in states:raise
 assert len(states)==2 and states['after']['registers']['eax']==expected[selected] and not o.import_log
 return dict(seed=seed,selected=selected,cl=cl,name=name,descriptor=descriptor,result=expected[selected],return_pc=ret,**states)
def main():
 output=Path(sys.argv[1]);sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
 rows=[capture(seed,selected)for seed in [606,20261025]for selected in range(3)]
 output.write_text(json.dumps(dict(schema=1,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(output),rows=len(rows))))
if __name__=='__main__':main()
