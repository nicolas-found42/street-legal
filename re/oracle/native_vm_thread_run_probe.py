#!/usr/bin/env python3
"""Original420ff0 entry through first opcode4210c5, with real5516c0.

Controlled retained caller stacks/GPRs, real module/string lookup, and retained
x87 environment. Only Win32 QPF/QPC leaves are supplied. At every platform leaf
and endpoint compare complete declared heap/stack, counters, eight GPRs, FPCW,
FPSW, tags and physical register values; arithmetic PE is separately
triangulated against the independent Rosetta/native x87 probe. No VM/native result hooks or scripts
are replaced. These captures are test inputs only, never production templates.
"""
from pathlib import Path
import hashlib,itertools,json,random,struct,sys,subprocess,platform
sys.path.insert(0,str(Path(__file__).resolve().parent))
from oracle import Oracle,DEFAULT_EXE,STACK_TOP
from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X
ROOT=Path(__file__).resolve().parents[2]
REGS=('eax','ebx','ecx','edx','esi','edi','ebp','esp')
CALLER=0x30000000;STACK=STACK_TOP-0x800;HEAP=0x2000

def fpu(o):
 return dict(control=o.uc.reg_read(X.UC_X86_REG_FPCW),status=o.uc.reg_read(X.UC_X86_REG_FPSW),tags=o.uc.reg_read(X.UC_X86_REG_FPTAG),registers=[list(o.uc.reg_read(X.UC_X86_REG_FP0+i))for i in range(8)])

def capture(seed,fill,budget,cw,depth,referee):
 o=Oracle();o.write(0x20000000,bytes([fill])*HEAP);o.write(STACK,random.Random(seed).randbytes(0x800))
 thread=o.alloc(0x38);jvm=o.alloc(0x80);frame=o.alloc(0x40);cl=o.alloc(0x1d8);module=o.alloc(0x64);pool=o.alloc(20);table=o.alloc(8);cc=o.alloc(16);sc=o.alloc(16);name=o.alloc(16);pc=o.alloc(8)
 o.write(name,b'test.Run\0');o.write_u32s(pc,[0xfedc101b,17]);o.write_u32(thread+0x10,jvm);o.write_u32(thread+0x20,frame);o.write_u32(thread+0x24,pc+4);o.write_u32(thread+0x2c,seed&0x51)
 o.write_u32(jvm+0x78,0x1234abcd if fill else 0);o.write_u32(frame+0x2c,pc);o.write_u32(frame+0x34,cl);o.write_u32(cl+4,module);o.write_u32(module+0x24,pool);o.write_u32(module+0x3c,0)
 o.write_u32s(pool,[0,0,2,table,2]);o.write_u32s(table,[cc,sc]);o.write_u32s(cc,[0,4,1,0]);o.write_u32s(sc,[0,0,name,8]);o.write_u32(0x5e73cc,0);o.write_u32(0x5f0910,0x447a0000)
 baseline=1000000;frequency=1000000;counter=baseline+int(seed)*43
 o.write(0x76a280,struct.pack('<qqq',frequency,baseline,counter-17))
 budget_bits=struct.unpack('<I',struct.pack('<f',budget))[0]
 rng=random.Random(seed);incoming={r:rng.getrandbits(32)for r in REGS[:-1]};incoming['ecx']=thread
 code=bytearray()
 # Source FLDs establish actual retained physical TOP and contents; there is
 # no post-entry FPU/register patch in the original execution.
 for i in range(depth):
  address=CALLER+0x800+i*4;o.write_u32(address,[0x3fa00000,0xc0600000][i]);code+=b'\xd9\x05'+struct.pack('<I',address)
 for r,opcode in [('eax',0xb8),('ebx',0xbb),('ecx',0xb9),('edx',0xba),('esi',0xbe),('edi',0xbf),('ebp',0xbd)]:code+=bytes([opcode])+struct.pack('<I',incoming[r])
 code+=b'\x68'+struct.pack('<I',budget_bits);at=CALLER+len(code);code+=b'\xe8'+struct.pack('<I',(0x420ff0-at-5)&0xffffffff);ret=CALLER+len(code);o.write(CALLER,code)
 states={};events=[]
 def state():return dict(registers={r:o.reg(r)for r in REGS},fpu=fpu(o),heap_ptr=o.heap_ptr,heap=o.read(0x20000000,HEAP).hex(),stack=o.read(STACK,0x800).hex(),counters=o.read(0x76a280,24).hex())
 def platform(kind,value):
  def leaf(oc,args):
   events.append(dict(kind=kind,args=args,state=state()));oc.write(args[0],struct.pack('<q',value));return 1
  return leaf
 o.hook(o.read_u32(0x5e7050),platform('frequency',frequency),'stdcall',1);o.hook(o.read_u32(0x5e7048),platform('counter',counter),'stdcall',1)
 def observe(uc,address,size,_):
  if address==0x420ff0:states['before']=state()
  if address==0x421061:states['before_clock']=state()
  if address==0x4210c5:states['after']=state();uc.emu_stop()
 o.uc.hook_add(UC_HOOK_CODE,observe)
 try:o.call(CALLER,[],fpu_cw=cw)
 except Exception:
  if 'after'not in states:raise
 assert len(states)==3 and len(events)==2 and not o.import_log
 reference=json.loads(subprocess.check_output([str(referee),hex(cw),str(frequency),str(baseline),str(counter),str(budget),str(depth)]))
 base=states['after']['registers']['esp']-STACK
 assert reference['deadline']==int.from_bytes(bytes.fromhex(states['after']['stack'])[base+0x44:base+0x48],'little')
 assert reference['before']==states['before_clock']['fpu']['status']
 # Independently expose the known arithmetic PE discrepancy, rather than
 # passing an exception-flag mismatch off as original CPU-state parity.
 assert (reference['after']^states['after']['fpu']['status']) in (0,0x20)
 return dict(referee=reference,seed=seed,fill=fill,budget=budget_bits,cw=cw,depth=depth,thread=thread,return_pc=ret,frequency=frequency,counter=counter,events=events,**states)

def main():
 output=Path(sys.argv[1])if len(sys.argv)>1 else ROOT/'re/out/boot/native-vm-thread-run-original.json'
 referee=ROOT/'re/out/native-abi/thread-run-fpu-referee';referee.parent.mkdir(parents=True,exist_ok=True)
 c=Path(__file__).with_name('thread_run_fpu_referee.c')
 command=['clang','-O0']+(['-arch','x86_64']if platform.system()=='Darwin'else[])+['-o',str(referee),str(c)]
 subprocess.run(command,check=True)
 sources=[Path(__file__),Path(__file__).with_name('oracle.py'),c];digests={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources}
 rows=[capture(*spec,referee)for spec in itertools.product([606,20261025],[0,0xa5],[0.,.5,-1.],[0x3f,0x7f],[0,2])]
 assert len(rows)==48
 assert digests=={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources}
 output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(dict(schema=1,profile=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources=digests,rows=rows),separators=(',',':'))+'\n');print(json.dumps(dict(output=str(output),rows=len(rows))))
if __name__=='__main__':main()
