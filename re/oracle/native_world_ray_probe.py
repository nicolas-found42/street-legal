"""Authored original49e3d0 scenarios; tree traversal is an explicit controlled collaborator."""
import argparse,hashlib,itertools,json,struct,sys
from pathlib import Path
sys.path.insert(0,str(Path.cwd()/'re/oracle'))
from oracle import Oracle,CW_D3D9,HEAP_BASE,DEFAULT_EXE
GLOBAL=0x643538
COUNT=(0x6435bc-GLOBAL)//4
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,default=Path("re/out/boot/native-world-ray-original.json"));args=parser.parse_args()
rows=[]
def bits(f):return struct.unpack('<I',struct.pack('<f',f))[0]
rays=[[0,0,0,1,0,0],[1,2,3,5,6,7],[-4,1,2,3,-2,0],[0,0,0,0,0,0]]
rays.append([0,0,0,*[struct.unpack('<f',struct.pack('<I',w))[0] for w in [0xc22f1796,0xc28b1bd4,0xc2afe2d6]]])
for number,(values,mode,old) in enumerate(itertools.product(rays,['empty','miss','hit','hit-body','any-hit'],[False,True])):
 o=Oracle(fpu_cw=CW_D3D9);o.hook_alloc();world=o.alloc(0x400);selection=o.alloc(0x40);tree=o.alloc(0x40);ray=o.alloc(24);limit=o.alloc(4);resource=o.alloc(0x80);body=o.alloc(0x400)
 o.write_u32s(GLOBAL,[0xa5000000+i for i in range(COUNT)])
 for a in [0x6435a4,0x6435a8,0x6435ac,0x6435b0,0x6435b4,0x6435b8]:o.write_u32(a,0)
 if old:
  o.write_u32s(0x6435ac,[0,0,0x1234,resource]);o.write_u32(resource+0x48,0x6435ac)
  o.write_u32(0x6435a4,body);o.write_u32(body,0x6435a4);o.write_u32(body+4,1)
 o.write_u32s(ray,list(map(bits,values)));o.write_u32(limit,0x3f400000);o.write_u32(selection+12,0 if mode=='empty' else tree)
 o.write_u32(body+0x20c,0)
 for off,v in [(0x64,.25),(0x68,.5),(0x6c,.75),(0xa4,1),(0xb8,1),(0xcc,1)]:o.write_f32(body+off,v)
 before=o.read_u32s(HEAP_BASE,(o.heap_ptr-HEAP_BASE)//4);globals_before=o.read_u32s(GLOBAL,COUNT);events=[]
 def traversal(oc,args):
  events.append({'this':oc.reg('ecx'),'args':list(args),'globals':oc.read_u32s(GLOBAL,COUNT),'heap':oc.read_u32s(HEAP_BASE,len(before))})
  oc.write_u32(0x643580,bits(.25) if mode.startswith('hit') else bits(1))
  if mode=='hit-body':
   oc.write_u32(0x6435a4,body);oc.write_u32(body,0x6435a4);oc.write_u32(body+4,1)
  return int(mode=='any-hit')
 o.hook(0x49e710,traversal,'thiscall',1)
 supplied=mode!='any-hit'
 result=o.call(0x49e3d0,[ray,selection,limit if supplied else 0],convention='thiscall',this=world)
 rows.append({'id':number,'mode':mode,'old_refs':old,'world':world,'body':body,'selection':selection,'ray':list(map(bits,values)),'limit_address':limit,'limit_supplied':supplied,'heap_before':before,'globals_before':globals_before,'heap_after':o.read_u32s(HEAP_BASE,len(before)),'globals_after':o.read_u32s(GLOBAL,COUNT),'events':events,'eax':result.eax,'heap_end':o.heap_ptr})
output=args.output;output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps({'schema':1,'oracle':'unhooked original49e3d0; controlled49e710 only; ordinary heap allocator shim','exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),'producer_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'rows':rows},indent=1)+'\n');print('original scenarios',len(rows),'output',output)
