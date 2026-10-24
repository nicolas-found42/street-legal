#!/usr/bin/env python3
"""Original initializer Thread/frame lifecycle, controlled canonical metadata.

World supplies decoded class/type/pool lookups and Scenario alloc/log/time/error
collaborators.403e80/407da0/41f340/407b10/420ff0 and retirement execute original
instructions. Passive entries retain raw identities/args/flags, not runtime answers.
"""
from pathlib import Path
import hashlib,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from vminit_vectors import World,n,STOP
from oracle import DEFAULT_EXE
from unicorn import UC_HOOK_CODE

def capture(mode,count):
    w=World();o=w.o;events=[]
    fields=[(f'f{i}','I',i)for i in range(count)]
    cls=w.klass('Lifecycle',statics=fields if mode=='static' else [],instance=fields if mode=='instance' else [],bodies=[[n(0x100b,i+11),STOP]for i in range(count)])
    def observe(uc,pc,size,_):
        if pc not in [0x41f340,0x407b10,0x420ff0,0x41f780,0x41f4d0,0x41f8b0]:return
        this=o.reg('ecx');sp=o.reg('esp');args=[]
        if pc==0x41f340:args=o.read_u32s(sp+4,4)
        if pc==0x407b10:args=o.read_u32s(sp+4,5)
        item=dict(pc=pc,this=this,args=args,return_pc=o.read_u32(sp))
        if pc==0x41f340:item['name']=o.read_cstr(args[3]).decode('ascii')
        if pc in [0x420ff0,0x41f780,0x41f4d0]:item.update(frame=o.read_u32(this+0x20),flags=o.read_u32(this+0x2c),pc_word=o.read_u32(this+0x24))
        events.append(item)
    o.uc.hook_add(UC_HOOK_CODE,observe);o.uc.ctl_remove_cache(0x400000,0x600000)
    obj=0 if mode=='static'else o.alloc(32)
    outcome=w.call(0x403e80 if mode=='static'else 0x407da0,cls if mode=='static'else obj,[]if mode=='static'else[cls,0])
    assert outcome=='ok',outcome
    return dict(mode=mode,count=count,outcome=outcome,events=events,fields=w.owner(o.read_u32(cls+0x1d0)if mode=='static'else o.read_u32(obj+16)))
def main():
    sources=[Path(__file__),*[Path(__file__).with_name(n)for n in ['vminit_vectors.py','thread_vectors.py','vmexec_vectors.py','oracle.py']]]
    print(json.dumps(dict(schema=1,scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),producer_sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in sources},rows=[capture(mode,count)for mode in ['static','instance']for count in [0,1,3]]),sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
