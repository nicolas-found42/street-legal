#!/usr/bin/env python3
"""Original native-class payload query slot +0x3c for car and part templates.

Dumps the class constructor name table at 0x60c848, locates each class payload
vtable (car known 0x5f0f18; part recovered from its constructor's vtable
store), then calls vtable+0x3c for register_created's kinds 8, 9 and 0x1c over
the declared controlled payload layout (resource+0x50 = Config 0xffff0080).
No hooks replace observed execution; allocations use the original heap.
"""
import json, hashlib
from pathlib import Path
from oracle import Oracle, DEFAULT_EXE, CW_D3D9

CLASS_TABLE = 0x60C848
CLASSES = ["ground", "camera", "object", "car", "part", "trigger", "player",
           "button", "cursor", "bot", "road", "walk", "trlight", "ped", "building"]
KINDS = [8, 9, 0x1C]


def read_table(o):
    entries = []
    for i in range(len(CLASSES)):
        base = CLASS_TABLE + i * 8
        name_ptr = o.read_u32(base)
        ctor = o.read_u32(base + 4)
        try:
            name = o.read_cstr(name_ptr, 32).decode("ascii", "replace")
        except Exception:
            name = None
        entries.append(dict(index=i, name_ptr=name_ptr, name=name, ctor=ctor))
    return entries


def run():
    o = Oracle(fpu_cw=CW_D3D9)
    o.hook_alloc()
    table = read_table(o)
    by_name = {e["name"]: e for e in table if e["name"]}
    out = dict(scope=__doc__, exe_sha256=hashlib.sha256(
        Path(DEFAULT_EXE).read_bytes()).hexdigest(),
        class_table=table, vtables={}, queries=[], words=[])

    # car: known vtable; part: recover from constructor bytes with capstone
    import capstone  # available in the oracle venv; detail scan may stop at
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    recovered = {}
    for cls in ["car", "part", "cursor", "player", "trigger", "button"]:
        ctor = by_name[cls]["ctor"]
        code = o.read(ctor, 0x400)
        # mov dword ptr [reg], imm32 == C7 /0 with modrm mod=00, reg=000
        vts = []
        for i in range(len(code) - 6):
            if code[i] == 0xC7 and (code[i + 1] & 0xC0) == 0x00 \
                    and (code[i + 1] & 0x38) == 0x00 \
                    and 0x5F0000 <= int.from_bytes(code[i + 2:i + 6], "little") < 0x600000:
                vtable = int.from_bytes(code[i + 2:i + 6], "little")
                vts.append(vtable)
                # corroborate by disassembling at the store instruction
                text = next(md.disasm(code[i:i + 8], ctor + i), None)
                recovered.setdefault(cls, []).append(
                    dict(offset=i, vtable=vtable,
                         at=ctor + i, disasm=str(text)))
                if len(vts) >= 1:
                    break
            if len(vts) >= 1:
                break
    out["vtables"] = recovered

    for cls in ["car", "part", "cursor", "player", "trigger", "button"]:
        entry = (recovered.get(cls) or [None])[0]
        vtable = entry["vtable"] if entry else (0x5F0F18 if cls == "car" else None)
        if not vtable:
            continue
        slot = o.read_u32(vtable + 0x3C)
        if cls == "part":
            # The part payload needs its real constructor state (lazy +0x120
            # collaborator). The part-template probe supplies a constructed
            # payload; without it this probe records the recovered vtable/slot
            # only and reports the query rows as unavailable.
            out.setdefault("unavailable", {})["part_queries"] = (
                "requires constructed part payload (see part template probe)")
            continue
        for kind in KINDS:
            k = o.alloc(0x2E98)
            obj = o.alloc(0x212C)
            resource = o.alloc(0x100)
            o.write_u32(k, vtable)
            o.write_u32(k + 0xC, 0x1A000006)
            o.write_u32(k + 0x10, resource)
            o.write_u32(resource + 0x50, 0xFFFF0080)
            try:
                result = o.call(slot, [obj, kind, 0], convention="thiscall", this=k)
                out["queries"].append(dict(
                    cls=cls, vtable=vtable, slot=slot, kind=kind,
                    eax=result.eax, edx=result.edx))
            except Exception as error:
                out.setdefault("unavailable", {})[f"{cls}_query{kind}"] = (
                    f"execution boundary: {type(error).__name__}: {error}")
    return out


if __name__ == "__main__":
    print(json.dumps(run(), sort_keys=True))
