# re/oracle — Unicorn function oracle (M0-08)

Ticket: issue #9 (plan id M0-08). Python tooling lives here, **outside the
Cargo workspace** (ADR 0008: uncommon/GPL-licensed code is never linked into
Rust crates; Unicorn via the Python venv is fine).

## Setup

```sh
re/oracle/setup_venv.sh            # default venv dir: ~/slr-oracle-venv
re/oracle/setup_venv.sh /tmp/v     # custom location
```

Pins: `unicorn==2.1.4`, `capstone==5.0.7`, `pefile==2024.8.26`
(verified 2026-10-17; Unicorn native lib `uc_version()` = `(2, 1, ...)`).

## Usage

Single call at an exe address (sections mapped at their VAs, image base
`0x400000`; stack + heap + scratch arenas; TEB/GDT so FS-relative code works;
imports stubbed to `xor eax,eax; ret N` returning 0):

```sh
V=~/slr-oracle-venv/bin/python
# integer helper (cdecl): ceil_div(7, 3) -> 3
$V re/oracle/oracle.py call 0x570330 --args 7 3
# 2-vector float dot (thiscall): needs two float[2] buffers, so the batch
# driver (run_vectors.py) or the library form below is the usual path; the
# CLI offers --this plus --f32p/--u32p scratch-pointer args for quick probes
$V re/oracle/oracle.py disasm 0x436950 -n 8
```

Batch vectors (generate + round-trip verify):

```sh
$V re/oracle/run_vectors.py gen --suite leaf-float    # 64 rows, FUN_00436950
$V re/oracle/run_vectors.py verify re/out/oracle/dot2_f32x2.jsonl
$V re/oracle/run_vectors.py gen --suite int-smoke     # 64 rows, FUN_00570330
$V re/oracle/run_vectors.py verify re/out/oracle/ceil_div_i32.jsonl
```

Library use:

```python
from oracle import Oracle
o = Oracle()                                   # default: reference/raw/*.bak.exe
pa = o.scratch(8); o.write_f32s(pa, [1.0, 2.0])
pb = o.scratch(8); o.write_f32s(pb, [3.0, 4.0])
r = o.call(0x436950, args=[pb], convention="thiscall", this=pa)
r.st0          # float64 view of ST0
r.st0_raw      # 10-byte x87 extended value of ST0
r.mem_writes   # [(addr, size, value)] writes during the call
```

## Calling conventions

`cdecl` (caller cleans), `stdcall` (callee cleans `4*nargs`),
`thiscall` (`ECX=this`, callee cleans), `fastcall` (first two integer args
in `ECX`/`EDX`). Args: ints (pushed as u32), Python floats (f32),
`("f32"|"f64"|"u32"|"i32", v)` tuples. Float results return in ST0;
`mem_writes` captures every guest write (the hook value is authoritative —
Unicorn's `MEM_WRITE` callback fires *before* the write lands).

## Control-word handling

- `--cw` sets the x87 control word before each call (`Oracle(fpu_cw=...)` /
  `call(..., fpu_cw=...)` do the same); the value is recorded per vector row.
- Game default: **`0x007F`** = PC 24-bit, round-nearest, all exceptions
  masked — the D3D9 device-thread word (no `FPU_PRESERVE`; CreateDevice flags
  `0x84`/fallback `0x24`; see `docs/plan.md`, ADR 0007). `CW_D3D9 = 0x007F`.
- Worker-thread variant: **`0x027F`** = PC 53-bit, the MSVC/Windows process
  default (`CW_DEFAULT`). Pass `--cw 0x27f` for worker-thread functions.
- Note the naming trap: `CW_DEFAULT` (0x027F, 53-bit) is the *process*
  default, **not** the game's device-thread word. Vectors for game float
  code use `0x007F`.

## Unicorn PC limitation (M0-07 finding)

Unicorn 2.1.4 honors RC but **silently ignores PC** (x87 precision control):
the same vector returns bit-identical results at PC 24/53/64. Full evidence:
`docs/x87.md` ("Precision-control emulation (a finding in itself)" and the
trusted-operation table).

Consequences for this oracle:

- **Trusted from Unicorn alone:** PC-insensitive ops — moves/stores/compares
  (`fld`/`fst`/`fistp`/`fcom*`, modulo the documented −0/`unnormal` quirks),
  `fprem`, and functions whose instruction mix is PC-insensitive
  (`fadd`/`fsub`/`fmul`/`fdiv` on inputs that cannot diverge across PC, plus
  integer-only code). The M0-08 leaf function qualifies: `f32` inputs through
  `fmul`/`fadd` return the identical ST0 byte-string at `0x007F` and `0x027F`
  (checked during development).
- **NOT trustworthy from Unicorn alone:** any result that can differ across
  PC — basic arithmetic near a 24-bit rounding boundary, and all
  transcendentals (`fsin`/`fcos`/`fpatan`/`f2xm1`/`fyl2x`, which are APPROX
  even at PC=64). These need the SoftFloat leg (`docs/x87.md`) before a
  bit-exact claim.
- The runner sets the game's control word on the virtual FPU anyway (so RC
  and the recorded `cw` are honest), but a matching `cw` field does **not**
  imply PC-correctness. Differential vectors must flag the oracle leg used:
  every row carries `"oracle": "unicorn"` + the Unicorn version, so a future
  multi-leg referee can require a `"softfloat"` leg wherever `docs/x87.md`
  marks the op PC-sensitive.

## Vector format

JSONL, one object per row, under `re/out/oracle/` (git-ignored via `/re/out`).
Field names follow `re/x87probe` conventions where they overlap (`id`,
`result`, `cw`) and add `exe_addr`/`oracle` for the Rust consumer:

```json
{"id": 2, "exe_addr": "0x436950", "name": "dot2_F32x2",
 "convention": "thiscall",
 "args": [{"ptr": "f32", "values": [1.0, 2.0]},
           {"ptr": "f32", "values": [3.0, 4.0]}],
 "cw": "0x7f", "result": {"st0": "0x00000000000000b00240"},
 "writes": [], "oracle": "unicorn", "unicorn": "2.1.4", "date": "2026-10-17"}
```

- `args`: `{"int": n}` stack words; `{"ptr": "f32"|"u32", "values": [...]}` =
  one scratch allocation passed by address (thiscall `this` = first entry).
- `result`: only *defined* outputs — float functions record `st0` (10-byte
  hex) and integer functions record `eax`/`edx` (8-digit hex). Scratch
  leftovers (EAX after a float return, EDX after `ceil_div`) are excluded;
  they vary with arena addresses and are not function outputs.
- `verify` re-runs each row from the encoded inputs on a fresh `Oracle` and
  requires byte-identical `result` bytes (`struct.pack('<f')` round-trip for
  floats; the 2×64-row suites below pass 64/64).

## Acceptance evidence (2026-10-17, this Mac)

Leaf float: `FUN_00436950` (`re/out/all_pseudo.c:34297-34303`)

```c
float10 __thiscall FUN_00436950(float *param_1,float *param_2)
{
  return (float10)*param_2 * (float10)*param_1 + (float10)param_2[1] * (float10)param_1[1];
}
```

Machine code: `fmul`/`fadd` leaf (`0x436950: fld; fmul; fld; fmul; faddp;
ret 4`), no calls, no globals. 64 vectors
(`re/out/oracle/dot2_f32x2.jsonl`, git-ignored): zeros, unit axes,
`[1,2]·[3,4]=11.0`, signs, f32-inexact tenths, `1e10/1e-10`, f32-max,
f32-min-normal/subnormal, ±0, plus a deterministic LCG fill — verify:
**64/64 byte-identical**, generation deterministic (re-`gen` → `cmp`
identical).

Smoke integer: `FUN_00570330` (`re/out/all_pseudo.c:176046-176052`)

```c
int FUN_00570330(int param_1,int param_2)
{
  return (param_1 + -1 + param_2) / param_2;
}
```

(`lea; cdq; idiv; ret`, no FPU.) 64 vectors
(`re/out/oracle/ceil_div_i32.jsonl`): divide edges incl. `INT_MAX`,
zero dividend, plus LCG fill — verify: **64/64 byte-identical**.

Nothing derived is committed: vectors live under `re/out/` (ignored); only
`oracle.py`, `run_vectors.py`, `setup_venv.sh`, and this README are tracked.
