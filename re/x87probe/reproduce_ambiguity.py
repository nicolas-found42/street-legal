"""Fresh exact examples of scalar target ambiguity; no runtime/contract edits.

True cosine is independently enclosed by exact rational alternating-series
bounds, not mpmath or a libm referee. Executes original54ecd0 without substituted
math, pinned independent Bochs standalone model, and x86instructions onRosetta.
"""
from pathlib import Path
from fractions import Fraction as F
from math import factorial
import sys,subprocess,json,hashlib,struct,platform,argparse
ROOT=Path(__file__).resolve().parents[2];OUT=Path(__file__).parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out',type=Path,required=True)
parser.add_argument('--bochs-build',type=Path,required=True)
parser.add_argument('--rosetta-probe',type=Path,required=True)
parser.add_argument('--rosetta-source',type=Path,required=True)
parser.add_argument('--exe',type=Path,required=True)
parser.add_argument('--primary',type=Path,required=True)
args=parser.parse_args()
assert args.out.resolve().is_relative_to(ROOT/'re/out'), 'raw evidence must stay local and ignored'
assert not args.out.exists(), 'preserve earlier attempts; choose a fresh --out'

sys.path.insert(0,str(ROOT/'re/oracle'))
from oracle import Oracle,CW_D3D9
from unicorn import UC_HOOK_CODE,__version__ as unicorn_version
from unicorn.x86_const import UC_X86_REG_FPCW,UC_X86_REG_FPSW
import unicorn
BUILD=args.bochs_build.resolve()
INPUTS=[0x3e800000,0x3f300000]
def h(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(argv,text=None):
 p=subprocess.run([str(x) for x in argv],input=text,text=True,capture_output=True,check=True)
 return dict(command=[str(x) for x in argv],returncode=p.returncode,stdout=p.stdout,stderr=p.stderr)
def ext80(s):
 b=bytes.fromhex(s);sig=int.from_bytes(b[:8],'little');se=int.from_bytes(b[8:],'little')
 assert (se&0x7fff) not in (0,0x7fff)
 return (-1 if se>>15 else 1)*F(sig)*F(2)**((se&0x7fff)-16383-63)
def cosine_bounds(x,n=32):
 lower=sum((F((-1)**k)*x**(2*k)/factorial(2*k) for k in range(n)),F(0))
 upper=lower+x**(2*n)/factorial(2*n)
 return lower,upper
primary=args.primary.resolve();primary_text=primary.with_suffix('.txt');model=json.loads((BUILD/'receipt.json').read_text())
assert model['upstream_commit']=='d245c751e6a6fab4bf68e72823598a48001adb3c'
assert model['mismatches']==[] and model['original_parity'] is False
for name,digest in model['rust_sources'].items(): assert h(ROOT/name)==digest, 'candidate source changed'
assert h(BUILD/'bochs-trig')==model['oracle_sha256']
for name,digest in model['observations'].items(): assert h(BUILD/name)==digest
assert h(BUILD/'harness.cc')==model['harness_sha256']
assert h(primary)=='7aca6a3e847520c4a1b2f1230da74ae3837757bc49288f5c895a6b003371d051'

files=[Path(__file__),args.rosetta_source.resolve(),args.rosetta_probe.resolve(),BUILD/'bochs-trig',BUILD/'harness.cc',BUILD/'receipt.json',ROOT/'re/oracle/oracle.py',args.exe.resolve(),primary,primary_text,Path(unicorn.__file__).parent/'lib/libunicorn.2.dylib',Path('/usr/libexec/rosetta/runtime'),Path('/usr/libexec/rosetta/translate_tool'),Path('/Library/Apple/usr/libexec/oah/libRosettaRuntime')]
before={str(p):h(p) for p in files}

text=''.join(f'{w:08x}\n' for w in INPUTS)
b=run([BUILD/'bochs-trig'],text);r=run([args.rosetta_probe],text)
assert json.loads(r['stderr'])==dict(sysctl_proc_translated=1,sysctl_status=0)
both={key:[line.split() for line in result['stdout'].splitlines()] for key,result in [('bochs',b),('rosetta',r)]}
rows=[]
for i,word in enumerate(INPUTS):
 o=Oracle(exe=str(args.exe.resolve()),fpu_cw=CW_D3D9);buf=o.alloc(0x90);dest=buf+0x10;source=buf+0x60;o.write_u32s(buf,[0x5a010000+j*137 for j in range(36)]);o.write_u32s(source,[word,0,0]);observed=[]
 def observe(uc,address,size,user):
  if address==0x54ecdf:observed.append(dict(raw=o._read_st0_raw().hex(),fpcw=uc.reg_read(UC_X86_REG_FPCW),fpsw=uc.reg_read(UC_X86_REG_FPSW)))
 o.uc.hook_add(UC_HOOK_CODE,observe);result=o.call(0x54ecd0,[source],convention='thiscall',this=dest,fpu_cw=CW_D3D9)
 assert len(observed)==1 and observed[0]['fpcw']==127
 x=F(struct.unpack('<f',struct.pack('<I',word))[0]);lo,hi=cosine_bounds(x);ulp=F(1,1<<64)
 assert F(1,2)<lo<hi<1 and hi-lo<F(1,1<<300)
 values={'bochs':both['bochs'][i][2],'rosetta':both['rosetta'][i][2],'unicorn_original':observed[0]['raw']};assert all(int(both[name][i][0],16)==word for name in both)
 images={key:ext80(image) for key,image in values.items()};errors={key:dict(lower=str((value-hi)/ulp),upper=str((value-lo)/ulp),midpoint_ulp=float((value-(lo+hi)/2)/ulp)) for key,value in images.items()}
 for name in ['bochs','rosetta']:
  assert abs(images[name]-lo)<F(82,100)*ulp and abs(images[name]-hi)<F(82,100)*ulp
 assert abs(images['bochs']-images['rosetta'])==ulp
 assert abs(images['unicorn_original']-lo)>ulp and abs(images['unicorn_original']-hi)>ulp
 # Verify the mathematicalm32 conversion is unambiguous, not merely a
 # potentiallydouble-rounded Python conversion. Allimages share onebin.
 f32=struct.unpack('<I',struct.pack('<f',float(images['bochs'])))[0]
 def fword(w):return F(struct.unpack('<f',struct.pack('<I',w))[0])
 lower_mid=(fword(f32-1)+fword(f32))/2;upper_mid=(fword(f32)+fword(f32+1))/2
 assert all(lower_mid<v<upper_mid for v in images.values())
 rows.append(dict(input_word=f'{word:08x}',input_exact=str(x),operation='standalone FCOS',original_entry='54ecd0',original_observation='54ecdf',fpcw=127,rosetta_status_words=both['rosetta'][i][4:6],raw_ext80_little_endian=values,cosine_rational_lower=str(lo),cosine_rational_upper=str(hi),ulp=str(ulp),signed_error_ulp=errors,all_images_round_to_m32=f'{f32:08x}',original_memory=o.uc.mem_read(dest,0x40).hex(),original_fpsw=observed[0]['fpsw'],original_instructions=result.icount))
assert before=={str(p):h(p) for p in files}, 'input changed during capture'
receipt=dict(schema='x87-target-ambiguity-rational-v1',scope='two exact dyadic FCOS inputs; notphysicalIntelexecution; notG11acceptance',producer_sha256=h(Path(__file__)),environment=dict(platform=platform.platform(),os_build=run(['sw_vers','-buildVersion'])['stdout'].strip(),compiler=run(['clang','--version'])['stdout'],unicorn_version=unicorn_version,translated=json.loads(r['stderr'])),upstream_commit=model['upstream_commit'],upstream_sources=model['upstream_sources'],observations=rows,commands=dict(bochs=b,rosetta=r),original_parity=False,production_provider=False,files={str(p):dict(sha256=h(p),bytes=p.stat().st_size) for p in files})
path=args.out.resolve();path.write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(dict(examples=len(rows),both_models_admissible_within_082_ulp=True,unicorn_outside_1_ulp=True,physical_hardware_ran=False),indent=2))
