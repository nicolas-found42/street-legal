#!/usr/bin/env python3
"""Original cursor wrappers against explicit controlled user32 API calls.

All original4b7bd0/4b7c50/4b7c90/4b7cb0/4b7cc0 instructions execute.
SetCursor/ShowCursor/ClipCursor return arbitrary declared words, discarded by
461d20's caller; GetCursorPos writes the supplied point. This does not observe
a physical window, native cursor image or attached device.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path
import unicorn
from oracle import Oracle, DEFAULT_EXE, CW_D3D9, HEAP_BASE

ROOT=Path(__file__).resolve().parents[2]
PROFILE=dict(name="controlled-user32-cursor-v1",physical_device_observed=False,
             comparison="entire caller heap, seven cursor globals, ordered API requests and decoded RECT",
             api_results="declared arbitrary return words; native caller ignores them",
             exclusions="CPU return registers and temporary stack pointer identities are unclaimed")
def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def globals_(o):return {str(a):o.read_u32(a) for a in [0x6495a4,0x6495ac,0x6495b0,*range(0x6495b4,0x6495dc,4)]}
def capture():
    rows=[]
    for system in [0,1]:
        for results in [0,0xaabbccdd]:
            o=Oracle(fpu_cw=CW_D3D9);events=[]
            raw=o.alloc(64);o.write(raw,bytes(range(64)))
            o.write_u32(0x6495a4,system);o.write_u32(0x6495b0,0x13579bdf)
            o.write_u32s(0x6495b4,[0x41000010+i*16 for i in range(10)])
            point=[-35,77]
            def set_cursor(oc,a):events.append(["SetCursor",*a]);return results
            def show_cursor(oc,a):events.append(["ShowCursor",*a]);return results
            def clip_cursor(oc,a):
                events.append(["ClipCursor",list(struct.unpack('<4i',oc.read(a[0],16))) if a[0] else None]);return results
            def get_cursor(oc,a):events.append(["GetCursorPos"]);oc.write(a[0],struct.pack('<2i',*point));return 1
            for iat,handler in [(0x5e72a4,set_cursor),(0x5e7260,show_cursor),(0x5e72a0,clip_cursor),(0x5e7298,get_cursor)]:o.hook(o.read_u32(iat),handler,'stdcall',1)
            steps=[];before=o.read_u32s(HEAP_BASE,16);before_globals=globals_(o)
            for op,value in [('mode',v) for v in [0,1,2,3,4,5,6,7,8,9,10,0xffffffff,0x80000000,0x7fffffff]]+[('show',0),('hide',0),('lock',0),('unlock',0)]:
                events.clear();address={'mode':0x4b7bd0,'show':0x4b7cb0,'hide':0x4b7cc0,'lock':0x4b7c50,'unlock':0x4b7c90}[op]
                o.call(address,[value] if op=='mode' else [],fpu_cw=CW_D3D9)
                steps.append(dict(operation=op,value=value,events=list(events),after=o.read_u32s(HEAP_BASE,16),globals=globals_(o)))
            rows.append(dict(system=system,api_result=results,point=point,before=before,before_globals=before_globals,steps=steps))
    return dict(schema=1,exe_sha256=digest(DEFAULT_EXE),stock_exe_sha256=digest(ROOT/'stock/StreetLegal_Redline.exe'),producer_sha256=digest(__file__),oracle_sha256=digest(Path(__file__).with_name('oracle.py')),
                fpu_cw=CW_D3D9,unicorn=unicorn.__version__,profile=PROFILE,profile_sha256=hashlib.sha256(json.dumps(PROFILE,sort_keys=True,separators=(',',':')).encode()).hexdigest(),rows=rows)
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    if args.output.exists():raise SystemExit('Preserve old output; choose another path')
    result=capture();args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,sort_keys=True)+'\n');print(len(result['rows']),'sequences',sum(len(r['steps']) for r in result['rows']),'calls')
