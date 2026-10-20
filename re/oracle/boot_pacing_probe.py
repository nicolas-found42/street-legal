#!/usr/bin/env python3
"""Original 551730 -> CRT sleep wrapper -> supplied Sleep import.

The original FLD, __ftol and wrapper execute. Only Sleep is replaced; this is
not a host scheduling or whole-loop timing measurement. Output in re/out/ only.
"""
import hashlib
import json
import struct
from pathlib import Path
from oracle import Oracle, CW_D3D9


def run(bits):
    exe = Path(__file__).resolve().parents[2] / 'reference/raw/StreetLegal_Redline.bak.exe'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(exe=str(exe), fpu_cw=CW_D3D9)
    calls = []
    def sleep(oc, args):
        calls.append({'milliseconds': args[0]})
        return 0
    o.hook(o.read_u32(0x5e705c), sleep, 'stdcall', 1)
    result = o.call(0x551730, [bits], fpu_cw=CW_D3D9, limit=5000)
    value = struct.unpack('<f', struct.pack('<I', bits))[0]
    assert len(calls) == 1
    if bits in [0, 0x41200000, 0x412e6666, 0xbf800000]:
        assert calls[0]['milliseconds'] == int(value) & 0xffffffff
    return {'oracle': 'Unicorn original551730 +5d6750 +55c460; supplied Sleep import only', 'input_bits': bits, 'calls': calls, 'instructions': result.icount, 'scope': 'conversion and API request only; no host wait or loop timing'}


if __name__ == '__main__':
    for bits in [0, 0x41200000, 0x412e6666, 0xbf800000, 0x4f000000, 0x7f800000, 0x7fc00000]:
        print(json.dumps(run(bits), sort_keys=True))
