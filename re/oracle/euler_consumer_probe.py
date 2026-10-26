"""Fresh original fullheaps/rawpoints for every retained accepted/unsupportedcase.

Uses unchanged original193-byte function, no substituted math. Inputs combine
all160specialfixturecases,330existingreducedcube and1025independentcross-check
inputs. One oracle image is reused because verifiedbody hasno globalwrites;
allcallerheapwords and FPU/registerstate reset for everycall.
"""
from pathlib import Path
import json,sys,hashlib,subprocess,platform,ctypes,struct,random,argparse
ROOT=Path(__file__).resolve().parents[2]
def provider_identity():
 class DlInfo(ctypes.Structure):_fields_=[('dli_fname',ctypes.c_char_p),('dli_fbase',ctypes.c_void_p),('dli_sname',ctypes.c_char_p),('dli_saddr',ctypes.c_void_p)]
 lib=ctypes.CDLL('/usr/lib/libSystem.B.dylib');lib.dladdr.argtypes=[ctypes.c_void_p,ctypes.POINTER(DlInfo)];lib.dladdr.restype=ctypes.c_int
 symbols={}
 for name in ['sin','cos']:
  info=DlInfo();assert lib.dladdr(ctypes.cast(getattr(lib,name),ctypes.c_void_p),ctypes.byref(info))
  header=struct.unpack('<8I',ctypes.string_at(info.dli_fbase,32));assert header[0]==0xfeedfacf
  position=info.dli_fbase+32;uuid=None
  for _ in range(header[4]):
   cmd,size=struct.unpack('<2I',ctypes.string_at(position,8));assert size>=8
   if cmd==0x1b:uuid=ctypes.string_at(position+8,16).hex()
   position+=size
  assert uuid is not None
  symbols[name]=dict(image=info.dli_fname.decode(),symbol=info.dli_sname.decode(),image_uuid=uuid,symbol_prefix64_sha256=hashlib.sha256(ctypes.string_at(info.dli_saddr,64)).hexdigest())
 return dict(os_build=subprocess.check_output(['sw_vers','-buildVersion'],text=True).strip(),symbols=symbols)

if '--provider-identity' in sys.argv:
 print(json.dumps(provider_identity(),sort_keys=True));raise SystemExit(0)

sys.path.insert(0,str(ROOT/'re/oracle'))
from oracle import Oracle,CW_D3D9
from unicorn import UC_HOOK_CODE,__version__ as unicorn_version
from unicorn.x86_const import UC_X86_REG_FPSW,UC_X86_REG_FPCW
paths=[ROOT/'re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json',ROOT/'re/out/boot/certified-euler-original.json']
inputs=[]
for path in paths:
 data=json.loads(path.read_text())
 for row in data['rows']:inputs.append((str(path.relative_to(ROOT)),row.get('angles',row.get('input'))))
rng=random.Random(0x54ecd0)
def word(value):return struct.unpack('<I',struct.pack('<f',value))[0]
generated=[[0xbe20c49c,0x40491687,0]]+[[word(rng.uniform(-6.3,6.3)) for _ in range(3)] for _ in range(1024)]
inputs.extend(('seeded-mixed-1025',angles) for angles in generated)
assert len(inputs)==1515
sites={0x54ecdb:(0,'sin'),0x54ecdf:(0,'cos'),0x54ece8:(1,'sin'),0x54eced:(1,'cos'),0x54ecf6:(2,'sin'),0x54ecff:(2,'cos')}
o=Oracle(fpu_cw=CW_D3D9);p=o.alloc(0x90);out=p+0x10;source=p+0x60;obs=[];current=[]
def observe(uc,address,size,user):
 if address in sites:
  axis,op=sites[address];obs.append(dict(site=address,axis=axis,op=op,input=current[axis],raw=o._read_st0_raw().hex(),fpcw=uc.reg_read(UC_X86_REG_FPCW),fpsw=uc.reg_read(UC_X86_REG_FPSW)))
o.uc.hook_add(UC_HOOK_CODE,observe)
rows=[]
for i,(provenance,current) in enumerate(inputs):
 o.write_u32s(p,[0x5a010000+j*137 for j in range(36)]);o.write_u32s(source,current);before=o.read_u32s(p,36);obs=[]
 result=o.call(0x54ecd0,[source],convention='thiscall',this=out,fpu_cw=CW_D3D9)
 assert len(obs)==6
 rows.append(dict(index=i,provenance=provenance,angles=current,out=out,source=source,before=before,after=o.read_u32s(p,36),trig=obs,fpcw=result.fpcw,fpsw=result.fpsw,depth=result.fpu_top_depth,icount=result.icount))
identity=provider_identity()
result=dict(schema='original-euler-complete-corpus-v1',entry=0x54ecd0,original_sha256=hashlib.sha256(Path(o.exe).read_bytes()).hexdigest(),producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),oracle_sha256=hashlib.sha256((ROOT/'re/oracle/oracle.py').read_bytes()).hexdigest(),generated_input_sha256=hashlib.sha256(json.dumps(generated).encode()).hexdigest(),input_inventories={str(x.relative_to(ROOT)):hashlib.sha256(x.read_bytes()).hexdigest() for x in paths},oracle_profile=dict(unicorn=unicorn_version,fpu_cw=127,substituted_math=False,observations='readonly postinstruction hooks',host=platform.platform(),os_build=identity['os_build'],symbols=identity['symbols'],libunicorn_sha256=hashlib.sha256(Path('/Users/nicolas/slr-oracle-venv/lib/python3.13/site-packages/unicorn/lib/libunicorn.2.dylib').read_bytes()).hexdigest()),rows=rows)
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,default=ROOT/'re/out/boot/euler-consumer-original.json');args=parser.parse_args();path=args.out.resolve();assert path.is_relative_to(ROOT/'re/out'), 'output must stay ignored'
assert not path.exists(), 'preserve existing attempt'
path.write_text(json.dumps(result,indent=2)+'\n');print(len(rows),'original fullbuffers/rawobservations')
