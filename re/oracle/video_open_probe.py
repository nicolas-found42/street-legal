#!/usr/bin/env python3
"""Execute original47c330/4f8dd0 for disabled video and COM failure arms.

Only the external CoCreateInstance response is supplied. All native marshalling,
filename/global writes, control flow and HRESULT propagation execute originalx86.
No successful graph/media/render backend is modelled by these rows.
"""
import hashlib
import json
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9
from render_vectors import _cell, _frame


def run(enabled, filename, hresult, windowed, repeat):
    digest = hashlib.sha256(Path(DEFAULT_EXE).read_bytes()).hexdigest()
    assert digest == '7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde'
    o = Oracle(fpu_cw=CW_D3D9)
    o.write(0x64A390, bytes([enabled]))
    o.write_u32(0x64A394, 0)
    o.write_u32(0x64A368, 0x12345678)
    o.write_u32(0x61852C, 0x11111111)
    calls = []
    def create(oc, args):
        clsid, outer, context, iid, out = args
        assert oc.read(clsid, 16).hex() == 'b3eb36e44f52ce119f530020af0ba770'
        assert outer == 0 and context == 1 and out == 0x64A368
        calls.append({'clsid': oc.read(clsid,16).hex(), 'iid': oc.read(iid,16).hex(),
                      'outer': outer, 'context': context, 'out': out, 'hresult': hresult})
        oc.write_u32(out, 0)
        return hresult
    thunk = next(va for va,name in o.import_names.items() if name.lower() == 'ole32.dll!cocreateinstance')
    o.hook(thunk, create, 'stdcall', 5)
    name = 0
    if filename is not None:
        name = o.alloc(len(filename)+1)
        o.write(name, filename.encode()+b'\0')
    cells = [_cell(o,'I',repeat), _cell(o,'I',windowed), _cell(o,'L',name), _cell(o,'L',0)]
    frame = _frame(o,'java.render.GfxEngine','openVideo','(Ljava.lang.String;II)I',cells)
    result = o.call(0x47C330,[frame],fpu_cw=CW_D3D9).eax
    expected = 0xFFFFFFFF if filename is None else (hresult if enabled else 0x80004005)
    assert result == expected, (hex(result),hex(expected))
    assert len(calls) == int(bool(enabled) and filename is not None)
    if calls:
        assert o.read_cstr(0x64A160).decode() == filename
        assert o.read_u32(0x61852C) == windowed
        assert o.read_u32(0x64A368) == 0
    assert o.read_u32(0x64A394) == 0
    return {'exe_sha256': digest, 'enabled': enabled, 'filename': filename,
            'com_hresult': hresult, 'windowed': windowed, 'repeat': repeat,
            'result': result, 'calls': calls, 'playing': o.read_u32(0x64A394),
            'filename_after': o.read_cstr(0x64A160).decode(),
            'windowed_after': o.read_u32(0x61852C), 'graph_after': o.read_u32(0x64A368),
            'boundary': 'CoCreateInstance failure is external; original47c330/4f8dd0 execute'}

if __name__ == '__main__':
    for values in [(1,None,0x80040154,1,1), (0,'data\\fmv\\prime.avi',0x80040154,1,1),
                   (1,'data\\fmv\\prime.avi',0x80040154,1,1),
                   (1,'data\\fmv\\other.avi',0x800401F0,0,0)]:
        print(json.dumps(run(*values),sort_keys=True))
