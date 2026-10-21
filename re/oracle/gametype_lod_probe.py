#!/usr/bin/env python3
"""Original546ae0 defaults and5461a0 type8 LOD scalar text parsing.
No parser or scalar callbacks are replaced. Descriptor sequences are supplied;
package inheritance, class loading and native constructors are outside scope.
"""
import hashlib,json
from pathlib import Path
from oracle import Oracle,DEFAULT_EXE,CW_D3D9
from gametype_materialize_probe import descriptors

def run():
 root=Path(__file__).resolve().parents[2]
 stock=descriptors(root/'stock/cars/racers/prime.rpk')[0xdb]
 assert stock['kind']==8 and b'lod_amp 3.000\r\n' in stock['text']
 scalar=next(line for line in stock['text'].splitlines(keepends=True)if line.startswith(b'lod_amp'))
 sequences=[[scalar.decode()],['lod_amp 0.7\r\n','lod_amp 2.5\r\n'],['lod_amp -0.0\r\n'],['LOD_AMP -3.25\r\n'],['lod_amp 1e3 trailing\r\n'],['lod_amp .5\r\n'],['lod_amp +3.0\r\n'],['lod_amp -0.25\r\n','lod_amp invalid\r\n'],['lod_amp 4294967296.125\r\n'],['lod_amp 3.000\r\n','lod_bias 9.0\r\n'],['lod_amp\r\n'],['lod_amp 0.1234567890123456789\r\n'],['lod_amp 3.000 # comment\r\n']]
 rows=[]
 for phases in sequences:
  o=Oracle();node=o.alloc(512);descriptor=o.alloc(24);o.write_u32s(descriptor,[8,0xdb,0,0,0,0]);o.call(0x546ae0,[descriptor],convention='thiscall',this=node,fpu_cw=CW_D3D9)
  o.write_u32(0x6200a4,123);before=o.read_u32s(node,0xa0//4);steps=[]
  for text in phases:
   raw=text.encode();buffer=o.alloc(len(raw)+1);o.write(buffer,raw+b'\0')
   r=o.call(0x5461a0,[node,buffer,len(raw),('f32',1.)],convention='thiscall',this=node+0x80,fpu_cw=CW_D3D9)
   steps.append(dict(text=text,words=o.read_u32s(node+0x98,2),node_words=o.read_u32s(node,0xa0//4),generation=o.read_u32(0x6200a4)))
  rows.append(dict(before=before,steps=steps))
 return dict(scope=__doc__,exe_sha256=hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest(),stock=dict(path='stock/cars/racers/prime.rpk',sha256=hashlib.sha256((root/'stock/cars/racers/prime.rpk').read_bytes()).hexdigest(),local_id=0xdb,descriptor=stock['text'].decode()),rows=rows)
if __name__=='__main__':print(json.dumps(run(),sort_keys=True))
