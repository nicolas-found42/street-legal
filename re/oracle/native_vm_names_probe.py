#!/usr/bin/env python3
"""Original canonical-name table memory, including real collision helpers.

Scenario allocator collaborators only;41f210 is NOT replaced. Full heap bytes
and every ordered CPU heap store are preserved. Stack/register projection and
stock loader event order are outside this bounded memory comparison.
"""
from pathlib import Path
import hashlib,json,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from thread_vectors import Scenario,CW
from oracle import DEFAULT_EXE,HEAP_BASE
from unicorn import UC_HOOK_MEM_WRITE

def row(name,strings):
    s=Scenario();o=s.o;holder=o.alloc(8)
    inputs=[]
    for text in strings:
        data=text.encode();p=o.alloc(len(data)+1);o.write(p,data+b'\0')
        inputs.append({'source':p,'length':len(data),'text':text})
    def heap():return {'heap_ptr':o.heap_ptr,'bytes':list(o.read(HEAP_BASE,o.heap_ptr-HEAP_BASE))}
    before=heap();stores=[]
    def observe(uc,access,address,size,value,_):
        if HEAP_BASE<=address<o.heap_ptr:
            # Unicorn can report the full source register for a byte write.
            # Preserve that report separately; value is exactly the written
            # size, matching Oracle._on_write's byte-width definition.
            stores.append({'address':address,'size':size,'value':value&((1<<(size*8))-1),'reported_value':value})
    o.uc.hook_add(UC_HOOK_MEM_WRITE,observe)
    o.call(0x41e4d0,[],convention='thiscall',this=holder,fpu_cw=CW)
    for item in inputs:
        item['result']=o.call(0x41e610,[item['source'],item['length']],convention='thiscall',this=holder,fpu_cw=CW).eax
    return {'name':name,'holder':holder,'inputs':inputs,'before':before,'after':heap(),'stores':stores}

def main():
    sequences=[('primitive_and_repeat',['I','F','V','I','[I','','[I']),
               ('same_hash_same_length',['ab','ac','ad','ab','ac','ad']),
               ('same_hash_different_length',['abc','bc','abc','bc']),
               ('stock_names',['Ljava.lang.String;','Ljava.game.GameRef;','[I','this','param','Ljava.lang.String;']),
               ('both_pool_growth',['abcX'+str(i).zfill(5)+'zzzz' for i in range(1030)])]
    sources=[Path(__file__),Path(__file__).with_name('oracle.py'),Path(__file__).with_name('thread_vectors.py')]
    print(json.dumps({'schema':1,'scope':__doc__,'fpu_cw':CW,
                      'exe_sha256':hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),
                      'producer_sources':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},
                      'rows':[row(name,strings)for name,strings in sequences]},sort_keys=True,separators=(',',':')))
if __name__=='__main__':main()
