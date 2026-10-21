#!/usr/bin/env python3
"""Execute build-606 malformed particle/skid inputs without normalizing results.

Only allocator and file/texture boundaries from particle_vectors are supplied.
The free shim records the original calls, retaining the existing no-op allocator
semantics. Two skid cases explicitly declare zero/0xa5 allocator fill to expose
an uninitialized maxcount; this is a scenario input, not an original constant.
Outputs stay ignored. Generation never overwrites an existing capture.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import particle_vectors
import oracle as oracle_runtime
from oracle import HEAP_BASE, STACK_SIZE, STACK_TOP


CASES = [
    ("unknown_state_default", "state", b"[STATE]\nsource_size blob 1 2 3\n", 0),
    ("unknown_state_after_point", "state", b"[STATE]\nsource_size point 7 8 9\nsource_size blob 1 2 3\n", 0),
    ("truncated_state_domain", "state", b"[STATE]\nsource_size\n", 0),
    ("truncated_source_velocity", "source", b"[ACTION]\nsource s0 5 point 0 0 0\n", 0),
    *[("invalid_gravity_" + token, "gravity", f"[ACTION]\ngravity first 1 2 3\ngravity second {token} {token} {token}\n".encode(), 0)
      for token in ("abc", "+3", ".5")],
    ("skid_missing_maxcount_zero", "skid", b"width 2 4\nheight 1\n", 0),
    ("skid_missing_maxcount_poison", "skid", b"width 2 4\nheight 1\n", 0xA5),
]


def capture_case(case, exe):
    name, kind, cfg, fill = case
    oracle, textures = particle_vectors._new_oracle(cfg, exe)
    freed = []
    oracle.hook(0x54F5B0, lambda _oracle, args: freed.append(args[0]), "cdecl", 1)

    def allocate(memory, args):
        pointer = memory.alloc(args[0])
        memory.write(pointer, bytes([fill]) * args[0])
        return pointer

    oracle.hook(0x54F560, allocate, "cdecl", 1)
    address = 0x521E10 if kind == "skid" else 0x51AC20
    result = oracle.call(address, [0x1234], "cdecl", fpu_cw=particle_vectors.CW_D3D9,
                         limit=200_000_000)
    if not result.eax:
        raise RuntimeError(f"{name}: original parser returned null")
    if kind in ("state", "source"):
        slot = result.eax + 0x3C if kind == "state" else oracle.read_u32(result.eax + 0x24) + 0x5C
        pointer = oracle.read_u32(slot)
        if not STACK_TOP - STACK_SIZE <= pointer < STACK_TOP:
            raise RuntimeError(f"{name}: original no longer publishes a stack domain")
        observed = {"slot": slot, "domain_pointer": pointer,
                    "domain_first_word": oracle.read_u32(pointer), "freed": freed}
        if kind == "state":
            observed["state_flags"] = oracle.read_u32(result.eax + 0x70)
    elif kind == "gravity":
        observed = {"actions": particle_vectors.dump_particle(oracle, result.eax)["actions"]}
    else:
        observed = particle_vectors.dump_skid(oracle, result.eax, textures)
    return {"case": name, "kind": kind, "cfg_hex": cfg.hex(), "allocator_fill": fill,
            "entry": address, "object": result.eax, "instructions": result.icount,
            "original": observed}


def capture(exe):
    digest = hashlib.sha256(Path(exe).read_bytes()).hexdigest()
    sources = [Path(__file__), Path(particle_vectors.__file__), Path(oracle_runtime.__file__)]
    source_hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}
    rows = [capture_case(case, str(exe)) for case in CASES]
    if source_hashes != {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}:
        raise RuntimeError("oracle sources changed during capture")
    if hashlib.sha256(Path(exe).read_bytes()).hexdigest() != digest:
        raise RuntimeError("original executable changed during capture")
    return {"schema_version": 1, "oracle": "build606-unicorn-original-functions",
            "exe_sha256": digest, "oracle_source_sha256": source_hashes, "unicorn_version": particle_vectors.UC_VER,
            "profile": {"cw": particle_vectors.CW_D3D9, "stack_top": STACK_TOP,
                        "stack_size": STACK_SIZE, "heap_base": HEAP_BASE,
                        "allocator": "bump allocation, declared fill, observed no-op free",
                        "file_texture_shims": "particle_vectors._new_oracle"}, "rows": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--exe", type=Path, default=Path(particle_vectors._default_exe()))
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    if not args.verify and args.output.exists():
        raise FileExistsError("preserve the previous capture; choose a fresh output")
    result = capture(args.exe)
    if args.verify:
        if json.loads(args.output.read_text()) != result:
            raise RuntimeError("original exception capture does not reproduce exactly")
        print(f"verified {len(result['rows'])} raw original exception cases")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("x") as stream:
            json.dump(result, stream, indent=2, sort_keys=True)
            stream.write("\n")
        print(f"captured {len(result['rows'])} raw original exception cases: {args.output}")


if __name__ == "__main__":
    main()
