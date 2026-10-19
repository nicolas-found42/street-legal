#!/usr/bin/env python3
"""Execute language native wrappers with recorded engine ABI calls.

GameType wrappers, System compiler/package/config/reset/array operations,
and Thread lifecycle entry points execute from the read-only build-606 image.
Engine collaborators are recorded at their ABI boundary; thread construction,
queue state, array payload copies and native wrapper control flow execute for real.
Derived vectors stay under ignored re/out and must never be committed."""

# Oracle hooks execute synchronously before their scenario loop advances.
# ruff: noqa: B023
import json
import os
import sys
from pathlib import Path

from langobj_vectors import TYPE_CELLS, Probe, default_exe

OUT = "re/out/langobj/engine.jsonl"


def rows(exe):
    out = []
    for name, addr, sig, params, replies in [
        ("compileAll", 0x47C080, "(Ljava.lang.String;)I", [0x1234], [0]),
        ("compileAll", 0x47C080, "(Ljava.lang.String;)I", [0x5678], [0xFFFFFFFE]),
        ("getConfigOptions", 0x47C500, "()V", [], [0]),
        (
            "openLib",
            0x487BE0,
            "(Ljava.lang.String;)I",
            [0x1234],
            [7, 0x22000000, 0x22000000, 2],
        ),
        ("openLib", 0x487BE0, "(Ljava.lang.String;)I", [0x1234], [9, 0, 0]),
        (
            "openLib",
            0x487BE0,
            "(Ljava.lang.String;)I",
            [0x1234],
            [3, 0x22000000, 0x22000000, 1, 0],
        ),
    ]:
        p = Probe(exe)
        calls = []
        q = list(replies)
        for a, conv, n in [
            (0x418EB0, "thiscall", 1),
            (0x427BF0, "thiscall", 0),
            (0x538380, "thiscall", 1),
            (0x5383F0, "thiscall", 1),
            (0x544000, "thiscall", 0),
            (0x545FC0, "thiscall", 1),
        ]:

            def hook(o, args, a=a, conv=conv):
                this = o.reg("ecx") if conv == "thiscall" else 0
                # temporary link pointers are represented by a stable fixture address
                if a == 0x545FC0:
                    this = 0x21000000
                calls.append([a, this, list(args)])
                return q.pop(0)

            p.o.hook(a, hook, conv, n)
        p.o.write_u32(0x618D48, 0x23000000)
        ci = p.call_info(0, sig.encode(), params=params)
        p.o.write_u32(ci, 0x24000000)
        r = p.call(addr, ci)
        out.append(
            dict(
                id=f"{name}-{len(out)}",
                name=name,
                exe_addr=addr,
                sig=sig,
                args=params + [0],
                jvm=0x24000000,
                memory={"6393160": 0x23000000},
                replies=replies,
                calls=calls,
                after={},
                **{"return": r.eax},
            )
        )
    for seed in (0, 1, 0x80000001, 0xFFFFFFFF):
        p = Probe(exe)
        p.o.write_u32(0x618D6C, seed)
        p.call(0x47C3A0, p.call_info(0, b"()V"))
        addresses = [0x765E8C + i * 4 for i in range(32)] + [0x765F24]
        out.append(
            {
                "id": f"isLoadingReset-{seed}",
                "name": "isLoadingReset",
                "exe_addr": 0x47C3A0,
                "sig": "()V",
                "args": [0],
                "jvm": 0,
                "memory": {str(0x618D6C): seed},
                "replies": [],
                "calls": [],
                "after": {str(a): p.o.read_u32(a) for a in addresses},
            }
        )
    return out


def game_rows(exe):
    out = []
    table = [
        line.strip().split("\t")
        for line in Path("re/out/native_api.tsv").read_text().splitlines()
        if line.startswith("java.lang.GameType\t")
    ]
    hooks = [
        (0x42AB50, "cdecl", 2),
        (0x48B670, "stdcall", 2),
        (0x48B500, "stdcall", 5),
        (0x48B750, "thiscall", 4),
        (0x48B8C0, "thiscall", 0),
        (0x48D3D0, "cdecl", 3),
        (0x48D420, "cdecl", 2),
        (0x427370, "thiscall", 2),
        (0x4274E0, "thiscall", 2),
        (0x4274D0, "thiscall", 1),
        (0x404EA0, "thiscall", 0),
        (0x53A1A0, "cdecl", 3),
        (0x546070, "thiscall", 3),
        (0x53A2A0, "cdecl", 5),
        (0x4290F0, "thiscall", 1),
        (0x551140, "cdecl", 3),
        (0x5513B0, "cdecl", 1),
    ]
    receiver = 0x22001000
    obj = 0x22002000
    cls = 0x22003000
    for _, name, sig, symbol in table:
        for variant in (
            (
                "live",
                "null",
                "zero",
                "lookup",
                "class-default",
                "native-zero",
                "same-owner",
                "unlinked",
                "previous",
            )
            if name == "createNativeInstance"
            else ("live", "null", "zero")
        ):
            p = Probe(exe)
            calls = []
            replies = []
            effects = []
            scratch_real = [0]
            memory = {
                0x62E008: 0x3333,
                receiver + 12: cls,
                cls + 0x1D4: 0x789,
                0x22004008: 1,
                obj + 12: 0x22005000,
                obj: 0x22006000,
                obj + 4: 0x22007000,
                0x22008050: 0x9999,
                0x62E018: 0xABCDEF01,
            }
            if variant == "lookup":
                memory[cls + 0x1D4] = 0
            if variant == "same-owner":
                memory[obj + 12] = 0x22008000
            if variant == "unlinked":
                memory[obj + 12] = 0
            if variant == "previous":
                memory[obj] = 0
            for a, v in memory.items():
                p.o.write_u32(a, v)
            returns = {
                0x42AB50: obj if variant != "null" else 0,
                0x404EA0: 0x5555,
                0x53A1A0: 0x22008000,
                0x53A2A0: 0 if variant == "native-zero" else 0x22008000,
            }

            def norm(a):
                return (
                    0x21000000 + (a - scratch_real[0])
                    if scratch_real[0] and scratch_real[0] <= a < scratch_real[0] + 16
                    else a
                )

            for a, conv, n in hooks:

                def hook(o, args, a=a, conv=conv):
                    this = o.reg("ecx") if conv == "thiscall" else 0
                    if a == 0x546070:
                        scratch_real[0] = this
                    recorded = [norm(v) for v in args]
                    calls.append([a, norm(this), recorded])
                    ret = returns.get(a, 0)
                    replies.append(ret)
                    effect = {}
                    if a == 0x546070:
                        # A live temporary resource link, requiring cleanup on return.
                        for off, value in [
                            (0, 0),
                            (4, 0x2200A000),
                            (8, 0),
                            (12, 0x2200B000),
                        ]:
                            o.write_u32(this + off, value)
                            effect[str(0x21000000 + off)] = value
                    effects.append(effect)
                    return ret

                p.o.hook(a, hook, conv, n)
            if name == "remNotification":
                params = [0x123, 0x45]
            elif name == "addNotification":
                params = [0x123, 0x45, 0x67, 0x89] + (
                    [0xAB] if sig.count("String;") == 2 else []
                )
            elif name == "addTimer":
                params = [0x45, 0x3F000000]  # reverse VM stack: callback, delay=0.5
            elif name == "createNativeInstance":
                params = [0x456, 0x5555, 0x789, 0x22004000]
            elif sig == "()V":
                params = []
            else:
                params = [0x45]
            if variant == "zero" and params:
                params[-1] = 0
            if name == "createNativeInstance":
                if variant == "lookup":
                    params[2] = 0
                if variant == "class-default":
                    params[0] = 0
                if variant == "unlinked":
                    params[-1] = 0
            ci = p.call_info(receiver, sig.encode(), params=params)
            p.o.write_f32s(p.o.read_u32(0x617650), [2.0])
            p.call(int(symbol[-8:], 16), ci)
            watched = [obj + x for x in (0, 4, 8, 12)] + [
                0x22006004,
                0x22007000,
                0x22005048,
                0x2200B048,
                0x2200A000,
                0x62E018,
                cls + 0x1D4,
            ]
            out.append(
                {
                    "id": f"{symbol}-{variant}",
                    "family": "gametype",
                    "name": name,
                    "exe_addr": int(symbol[-8:], 16),
                    "sig": sig,
                    "args": params + [receiver],
                    "jvm": 0,
                    "memory": {str(a): v for a, v in memory.items()},
                    "replies": replies,
                    "effects": effects,
                    "calls": calls,
                    "after": {str(a): p.o.read_u32(a) for a in watched},
                    "sim_time": 2.0,
                }
            )
    return out


def array_rows(exe):
    out = []
    for same, srcpos, dstpos, count in [
        (False, 0, 1, 3),
        (False, 1, 0, 2),
        (True, 0, 1, 3),
        (True, 1, 0, 3),
        (False, 0, 0, 0),
        (False, -1, 0, 0),
        (False, 0, -1, 0),
        (False, 3, 0, 2),
        (False, 0, 4, 1),
    ]:
        p = Probe(exe)
        calls = []

        def log(o, a):
            calls.append([0x551140, 0, list(a)])
            p.logs.append(o.read_cstr(a[1]).decode("latin1"))

        def flush(o, a):
            calls.append([0x5513B0, 0, list(a)])
            return 0

        p.o.hook(0x551140, log, "cdecl", 3)
        p.o.hook(0x5513B0, flush, "cdecl", 1)
        source = [11, 22, 33, 44]
        dest = source if same else [90, 80, 70, 60]
        objects = []
        containers = []
        cells = []
        type_names = {TYPE_CELLS[tag]: tag for tag in ("I", "F", "L")}
        type_patterns = [("I", "F", "L", "I"), ("F", "L", "I", "F")]
        for array_index, values in enumerate([source] if same else [source, dest]):
            obj = p.o.alloc(32)
            context = p.o.alloc(16)
            record = p.o.alloc(12)
            data = p.o.alloc(len(values) * 4)
            cellwords = [p.cell(v) for v in values]
            for i, c in enumerate(cellwords):
                p.o.write_u32(c + 4, TYPE_CELLS[type_patterns[array_index][i]])
                p.o.write_u32(data + 4 * i, c)
            container = p.o.alloc(16)
            p.o.write_u32s(container, [len(values), data, len(values), 0])
            p.o.write_u32(record + 8, container)
            p.o.write_u32(obj + 16, context)
            objects.append(obj)
            containers.append((context, record))
            cells.append(cellwords)
        if same:
            objects.append(objects[0])
            cells.append(cells[0])
        p.o.hook(0x42A430, lambda o, a: 4, "cdecl", 2)
        p.o.hook(0x403820, lambda o, a: dict(containers)[o.reg("ecx")], "thiscall", 1)
        before_types = [
            [type_names[p.o.read_u32(c + 4)] for c in group] for group in cells
        ]
        params = [count, dstpos, objects[1], srcpos, objects[0]]
        ci = p.call_info(
            0, b"(Ljava.lang.Object;ILjava.lang.Object;II)V", params=params
        )
        p.call(0x487660, ci)
        out.append(
            {
                "id": f"array-{len(out)}",
                "family": "array",
                "name": "arraycopy",
                "sig": "(Ljava.lang.Object;ILjava.lang.Object;II)V",
                "same": same,
                "src": source,
                "dst": dest,
                "srcpos": srcpos,
                "dstpos": dstpos,
                "count": count,
                "src_types": before_types[0],
                "dst_types": before_types[1],
                "after_src_types": [type_names[p.o.read_u32(c + 4)] for c in cells[0]],
                "after_dst_types": [type_names[p.o.read_u32(c + 4)] for c in cells[1]],
                "after_src": [p.o.read_u32(c + 8) for c in cells[0]],
                "after_dst": [p.o.read_u32(c + 8) for c in cells[1]],
                "calls": calls,
            }
        )
    return out


def lifecycle_rows(exe):
    from thread_vectors import Scenario

    out = []
    for mode in ("init", "start-ok", "start-fail", "start-twice", "stop", "stop-twice"):
        scenario = Scenario(exe)
        p = Probe(exe)
        p.o = scenario.o
        receiver = 0x22001000
        stored = [0]
        created = [0]
        steps = []
        runs = []
        owner = p.o.read_u32(scenario.jvm + 0x18)
        sentinel = p.o.read_u32(scenario.thread + 8)

        def set_handle(o, a):
            stored[0] = a[2]
            return 0

        p.o.hook(0x42A9E0, set_handle, "cdecl", 3)
        p.o.hook(0x42AB50, lambda o, a: stored[0], "cdecl", 2)
        p.o.hook(0x42A690, lambda o, a: 0x1234, "cdecl", 3)

        def run(o, a):
            runs.append([o.reg("ecx"), o.read_cstr(a[1]).decode("latin1")])
            return int(mode != "start-fail")

        p.o.hook(0x408A30, run, "thiscall", 2)
        name = p.o.alloc(32)
        p.o.write(name, b"oracle-worker\0")
        sequence = ["init"] + (
            ["start"] * (2 if mode == "start-twice" else 1)
            if mode.startswith("start")
            else ["stop"] * (2 if mode == "stop-twice" else 1)
            if mode.startswith("stop")
            else []
        )
        for op in sequence:
            ci = p.call_info(
                receiver,
                b"(Ljava.lang.String;)V" if op == "init" else b"()V",
                params=[name] if op == "init" else [],
            )
            p.o.write_u32(ci, scenario.jvm)
            p.call({"init": 0x47C510, "start": 0x47C570, "stop": 0x47C600}[op], ci)
            if op == "init":
                created[0] = stored[0]
            t = created[0]
            cur = p.o.read_u32(owner + 0x18)
            count = 0
            while cur and cur != sentinel:
                count += 1
                cur = p.o.read_u32(cur + 8)
            steps.append(
                {
                    "op": op,
                    "flags": p.o.read_u32(t + 0x2C),
                    "priority": p.o.read_u32(t + 0x28),
                    "script": p.o.read_u32(t + 0x18),
                    "handle": stored[0] != 0,
                    "name": p.o.read_cstr(p.o.read_u32(t + 0x1C)).decode("latin1"),
                    "queue_count": count,
                    "runs": list(runs),
                }
            )
        out.append({"id": mode, "family": "lifecycle", "steps": steps})
    return out


def open_fallback_rows(exe):
    out = []
    for prev, next_ in [
        (0, 0),
        (0, 0x2200A000),
        (0x2200C000, 0x2200A000),
        (0x2200C000, 0),
    ]:
        p = Probe(exe)
        calls = []
        replies = []
        effects = []
        p.o.write_u32(0x618D48, 0x23000000)
        for a, conv, n, ret in [
            (0x538380, "thiscall", 1, 13),
            (0x5383F0, "thiscall", 1, 0),
            (0x545FC0, "thiscall", 1, 0),
        ]:

            def hook(o, args, a=a, ret=ret):
                this = o.reg("ecx")
                effect = {}
                if a == 0x545FC0:
                    for off, v in [(0, prev), (4, next_), (8, 0), (12, 0x2200B000)]:
                        o.write_u32(this + off, v)
                        effect[str(0x21000000 + off)] = v
                    this = 0x21000000
                calls.append([a, this, list(args)])
                replies.append(ret)
                effects.append(effect)
                return ret

            p.o.hook(a, hook, conv, n)
        ci = p.call_info(0, b"(Ljava.lang.String;)I", params=[0x1234])
        r = p.call(0x487BE0, ci)
        out.append(
            dict(
                id=f"open-loaded-{len(out)}",
                name="openLib",
                exe_addr=0x487BE0,
                sig="(Ljava.lang.String;)I",
                args=[0x1234, 0],
                jvm=0,
                memory={str(0x618D48): 0x23000000},
                replies=replies,
                effects=effects,
                calls=calls,
                after={
                    str(a): p.o.read_u32(a)
                    for a in [0x2200B048, 0x2200C004, 0x2200A000]
                },
                **{"return": r.eax},
            )
        )
    return out


def main():
    fresh = (
        rows(default_exe())
        + game_rows(default_exe())
        + array_rows(default_exe())
        + lifecycle_rows(default_exe())
        + open_fallback_rows(default_exe())
    )
    if len(sys.argv) > 1 and sys.argv[1] == "verify":
        old = [json.loads(x) for x in Path(OUT).read_text().splitlines()]
        assert old == fresh
        print(f"verify {len(old)}/{len(old)} engine rows")
    else:
        os.makedirs(os.path.dirname(OUT), exist_ok=True)
        with open(OUT, "w") as f:
            f.writelines(json.dumps(row) + "\n" for row in fresh)
        print(f"{len(fresh)} engine rows")


if __name__ == "__main__":
    main()
