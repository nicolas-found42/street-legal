#!/usr/bin/env python3
"""Compile the pinned Bochs softfloat3e + fpu transcendentals with bochs_stream.cc (tool only)."""
import re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

CONFIG = """#pragma once
#include <cstdint>
#define BX_LITTLE_ENDIAN 1
#define BX_CPP_INLINE inline
#define BX_CONST64(x) UINT64_C(x)
typedef uint8_t Bit8u; typedef int8_t Bit8s; typedef uint16_t Bit16u; typedef int16_t Bit16s;
typedef uint32_t Bit32u; typedef int32_t Bit32s; typedef uint64_t Bit64u; typedef int64_t Bit64s;
"""
up, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
sf, fpu = up / "bochs/cpu/softfloat3e", up / "bochs/cpu/fpu"
(out / "config.h").write_text(CONFIG)
names = re.findall(r"([a-zA-Z0-9_]+)\$\(OBJ\)", (sf / "Makefile.in").read_text())
srcs = []
for n in dict.fromkeys(names):
    s = sf / (n + ".cc")
    srcs.append(s if s.exists() else sf / "8086-SSE" / (n + ".cc"))
srcs += [fpu / f for f in ("fsincos.cc", "f128_67.cc", "fpatan.cc", "f2xm1.cc", "fyl2x.cc", "fprem.cc")]
srcs.append(Path(__file__).with_name("bochs_stream.cc"))
flags = ["-std=c++17", "-O2", "-DSOFTFLOAT_FAST_INT64", "-DINLINE_LEVEL=5", "-DSOFTFLOAT_FAST_DIV32TO16", "-DSOFTFLOAT_FAST_DIV64TO32"]
for d in (out, sf / "include", sf / "8086-SSE", fpu, up / "bochs/cpu"):
    flags += ["-I", str(d)]
objdir = out / "obj"
objdir.mkdir(exist_ok=True)
def one(item):
    i, s = item
    o = objdir / f"{i}.o"
    r = subprocess.run(["clang++", *flags, "-c", str(s), "-o", str(o)], capture_output=True, text=True)
    if r.returncode:
        sys.exit(f"{s}: {r.stderr[:2000]}")
    return str(o)
with ThreadPoolExecutor(8) as pool:
    objs = list(pool.map(one, enumerate(srcs)))
subprocess.run(["clang++", *objs, "-o", str(out / "bochs-stream")], check=True)
