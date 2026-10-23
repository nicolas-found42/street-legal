#!/usr/bin/env python3
"""Compare the diagnostic Rust FSIN/FCOS model with pinned upstream Bochs.

No Bochs code is linked into the Cargo workspace. Requires an existing sparse
checkout with bochs/cpu/{fpu,softfloat3e} at PIN. Builds the independent C++
implementation under ignored re/out and retains every observation and identity.
This comparison validates the Rust adaptation, not x87 hardware/original parity.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import random
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PIN = 'd245c751e6a6fab4bf68e72823598a48001adb3c'

CONFIG = '''#pragma once
#include <cstdint>
#define BX_LITTLE_ENDIAN 1
#define BX_CPP_INLINE inline
#define BX_CONST64(x) UINT64_C(x)
typedef uint8_t Bit8u;
typedef int8_t Bit8s;
typedef uint16_t Bit16u;
typedef int16_t Bit16s;
typedef uint32_t Bit32u;
typedef int32_t Bit32s;
typedef uint64_t Bit64u;
typedef int64_t Bit64s;
'''

HARNESS = r'''#include <iostream>
#include <iomanip>
#include "fpu_trans.h"
static void emit(floatx80 x) {
    for (int i=0; i<8; ++i) std::cout << std::setw(2) << ((x.signif >> (i*8)) & 255);
    std::cout << std::setw(2) << (x.signExp & 255) << std::setw(2) << (x.signExp >> 8);
}
int main() {
    uint32_t bits;
    while (std::cin >> std::hex >> bits) {
        softfloat_status_t status = {};
        status.extF80_roundingPrecision = 32;
        status.softfloat_exceptionMasks = 0x3f;
        floatx80 a = f32_to_extF80(bits, &status);
        floatx80 s=a, c=a;
        int sc2 = fsin(s, status), cc2 = fcos(c, status);
        std::cout << std::hex << std::setfill('0') << std::setw(8) << bits << " ";
        emit(s); std::cout << " "; emit(c);
        std::cout << " " << ((sc2 || cc2) ? 1 : 0) << "\n";
    }
}
'''


def run(argv, **kwargs):
    command = [str(a) for a in argv]
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, **kwargs)
    if result.returncode:
        raise RuntimeError(f'{command!r} exited {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rust_sources():
    crate = ROOT / 'crates/slr-x87'
    paths = list(crate.rglob('*.rs')) + list((crate / 'vendor').rglob('*'))
    paths += [crate / 'Cargo.toml', crate / 'src/slr_softfloat_shim.c', ROOT / 'Cargo.lock', ROOT / 'Cargo.toml']
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(set(paths)) if p.is_file()}


def corpus():
    words = [0, 0x80000000, 1, 0x007fffff, 0x00800000, 0x1d000000, 0x2f000000,
             0x3dcccccd, 0xbe20c49c, 0x3e800000, 0x3f000000, 0x3f800000,
             0x3fc90fdb, 0x40490fdb, 0x40c90fdb, 0x5effffff, 0x5f000000,
             0x5f000001, 0x7f7fffff]
    # Include both sides of exponent/table edges and every sign.
    words += [(exp << 23) + delta for exp in range(1, 255) for delta in [-1, 0, 1]]
    words += [0x3e800000 + k * 0x100000 + d for k in range(16) for d in [-1, 0, 1]]
    rng = random.Random(0x54ecd0)
    words += [rng.randrange(0x7f800000) for _ in range(4096)]
    return sorted({w | sign for w in words for sign in [0, 0x80000000]})


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--upstream', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    upstream, out = args.upstream.resolve(), args.out.resolve()
    if not out.is_relative_to(ROOT / 're/out'):
        ap.error('outputs must stay under ignored re/out')
    if out.exists():
        ap.error('preserve prior capture; choose a fresh output directory')
    if run(['git', '-C', upstream, 'rev-parse', 'HEAD']).strip() != PIN:
        ap.error('upstream revision differs from PIN')
    if run(['git', '-C', upstream, 'status', '--porcelain']).strip():
        ap.error('upstream checkout is dirty')
    candidate_inputs = rust_sources()
    producer = digest(Path(__file__))
    out.mkdir(parents=True)
    (out / 'config.h').write_text(CONFIG)
    (out / 'harness.cc').write_text(HARNESS)
    sf, fpu = upstream / 'bochs/cpu/softfloat3e', upstream / 'bochs/cpu/fpu'
    def upstream_sources():
        paths = list(sf.rglob('*.cc')) + list(sf.rglob('*.h')) + list(fpu.glob('*.cc')) + list(fpu.glob('*.h'))
        return {str(p.relative_to(upstream)): digest(p) for p in sorted(paths)}
    inputs = upstream_sources()
    makefile = (sf / 'Makefile.in').read_text()
    names = re.findall(r'([a-zA-Z0-9_]+)\$\(OBJ\)', makefile)
    sources = []
    for name in dict.fromkeys(names):
        source = sf / (name + '.cc')
        if not source.exists():
            source = sf / '8086-SSE' / (name + '.cc')
        if not source.is_file():
            raise RuntimeError(f'missing upstream translation unit: {name}')
        sources.append(source)
    sources += [fpu / 'fsincos.cc', fpu / 'f128_67.cc', out / 'harness.cc']
    flags = ['-std=c++17', '-O2', '-DSOFTFLOAT_FAST_INT64', '-DINLINE_LEVEL=5',
             '-DSOFTFLOAT_FAST_DIV32TO16', '-DSOFTFLOAT_FAST_DIV64TO32']
    for directory in [out, sf / 'include', sf / '8086-SSE', fpu, upstream / 'bochs/cpu']:
        flags += ['-I', str(directory)]
    def compile_one(item):
        index, source = item
        obj = out / f'{index}.o'
        run(['clang++', *flags, '-c', source, '-o', obj])
        return obj
    with ThreadPoolExecutor(max_workers=8) as pool:
        objects = list(pool.map(compile_one, enumerate(sources)))
    oracle = out / 'bochs-trig'
    run(['clang++', *objects, '-o', oracle])
    words = corpus()
    text = ''.join(f'{w:08x}\n' for w in words)
    (out / 'inputs.txt').write_text(text)
    expected = run([oracle], input=text)
    (out / 'bochs.txt').write_text(expected)
    run(['cargo', 'build', '-p', 'slr-x87', '--features', 'experimental-intel-trig',
         '--example', 'intel_trig_candidate'], cwd=ROOT)
    candidate = ROOT / 'target/debug/examples/intel_trig_candidate'
    actual = run([candidate], input=text)
    (out / 'rust.txt').write_text(actual)
    left, right = expected.splitlines(), actual.splitlines()
    assert len(left) == len(right) == len(words)
    mismatches = [dict(input=f'{word:08x}', bochs=a, rust=b)
                  for word, a, b in zip(words, left, right) if a != b]
    if rust_sources() != candidate_inputs or upstream_sources() != inputs or digest(Path(__file__)) != producer:
        raise RuntimeError('source changed during comparison; observations retained but no valid receipt')
    receipt = dict(schema=1, upstream_commit=PIN, rows=len(words), mismatches=mismatches,
                   scope=__doc__, original_parity=False, production_provider=False,
                   upstream_sources=inputs, compile_flags=flags,
                   compiler=run(['clang++', '--version']),
                   rustc=run(['rustc', '--version', '--verbose']),
                   compiled_sources=[str(p) for p in sources],
                   harness_sha256=digest(out/'harness.cc'), config_sha256=digest(out/'config.h'),
                   candidate_sha256=digest(candidate), oracle_sha256=digest(oracle),
                   producer_sha256=producer, rust_sources=candidate_inputs,
                   observations={name:digest(out/name) for name in ['inputs.txt','bochs.txt','rust.txt']})
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(f'{len(words)} upstream comparisons; {len(mismatches)} mismatches; {out}')
    if mismatches:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
