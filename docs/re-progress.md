# Reverse-engineering progress (stock build 606)

Status: **early.** The exact port described in ADR 0004 is not started beyond tooling and the findings below.
The playable slice in `slr-core`/`slr-game` is still the old approximation.

## Tooling (reproducible)
- `re/ghidra_scripts/ExportAll.java`: headless Ghidra export of every function's pseudocode to `re/out/` (git-ignored, derived from the exe).
  4,356 functions exported from `StreetLegal_Redline.bak.exe`.
- `re/build_native_table.py`: regenerates `re/out/native_api.tsv` (git-ignored) from the exe plus
  `re/out/all_pseudo.c`. Byte-level scan of `.text` for `E8` calls to the registrar `FUN_00416b00`
  (asserts 360 sites), minus the 7 Controller rows Ghidra never attributed, cross-checked 1:1 against
  a `\)\s*;` regex pass over the export (asserts 353 rows, impl labels agree). Exe SHA-256
  `7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde` (capstone 5.0.7 + pefile 2024.8.26).

## TUFA (script VM) findings
| Fact | Evidence |
|---|---|
| Class loader is `FUN_00411b20` (`JVM::addClass`); chunks `TREE` `DHTM`(MTHD) `DLIF`(FILD) `CONS` `CLSS` `TUFA` | pseudocode in `re/out/all_pseudo.c` |
| Serialized tree node = 1 opcode byte + u16 operand + (u32 immediate iff op class is 0x1000/0x2000/0x4000); size 3 or 7 | `FUN_0041a5f0`, `FUN_0041a5c0` |
| Opcode byte indexes a u16 table at VA 0x5f0914; 47 ops (0x00-0x2e); high nibble = operand class | bytes at 0x5f0914 |
| In memory a node is a C++ object, vtable 0x5f0974, opword at +0x14, immediate at +0x18, children list at +0x10 | `FUN_0041a700` |
| The exe also contains a Java-like source compiler (`JVM::compileSource`, yacc parser, type checks, a ~14.7k-line `compileHook`) | strings 0x60b4f8.. |
| 353 native methods are registered through `FUN_00416b00(class, name, signature, impl)`; the table is extracted by `re/build_native_table.py` to `re/out/native_api.tsv` (largest groups: WheelRef 29, Part 26, GroundRef 25, Chassis 23, System 22, ResourceRef 20, GameRef 18). 11 `(class,name)` pairs overload (14 extra rows; genuine exe overloads, kept as-is). The prior 338-row table missed 15 registrations whose decompiled call ends `)` newline `;` (old regex `'\);'`); the fixed regex `'\)\s*;'` recovers them: WheelRef getRadius/getHBrake/setRadius/setRollRes/setBearing/setMaxLoad, Part isSlotDisabled/getRenderType/setRenderType, DynoData newNative, Animation pause, GameType removeAllTimers/registerCallback, GameRef setFlags/isEmpty | registrar `FUN_00416b00` (ecx = VM ctx from `[0x63641c]`); `re/out/all_pseudo.c` |
| `java.io.Controller`'s 7 natives (`user_Add (IIIFFFF)I` @0x477740, `user_Del (III)I` @0x477810, `user_SetAxisSmooth (IFFFFF)I` @0x4778b0, `user_SetAxisSpeed (IF)V` @0x477970, `user_Reset ()V` @0x477a00, `user_GetAxisVal (I)F` @0x477a80, `user_SetAxisForce (IFF)V` @0x477b20) register at binary call sites 0x477d22/41/60/7f/9e/bd/dc inside an un-attributed function at 0x477cb0 (vtable slot `0x5f12f4`, installed by `FUN_00477bc0`), in Ghidra's function gap 0x477ca6-0x477e00; the 7 name/sig/class strings sit at 0x61263c-0x612770. Recorded as open: the table's 353 rows exclude them pending a Ghidra-side owner for 0x477cb0 | exe bytes (pefile+capstone scan, 360 `E8→0x416b00` sites, impls agree on all 353 shared rows); zero hits for `Controller`/`user_Add`/etc. and for VAs 0x612654/0x477d2x in `all_pseudo.c` |
| Package roots map to script dirs (`java.lang`->system/scripts/lang, `java.game`->sl/scripts/game, parts, accessories) | `FUN_0041cfe0` calls near line 10034 |
| `tufa::Module::trees` decodes every stock class exactly: 1,546 modules, 5,816 bodies, 320,522 nodes, zero trailing bytes (test `every_tufa_tree_decodes_exactly`). The 11 modules in `render/osd/Dialog.class` are version 0x108f5, which the exe cannot decode | `crates/slr-formats` |
| Executor is `Thread::run` = `FUN_00420ff0`: postfix stack machine over ref-counted values; opcode 0x1007 carries a sub-op (0..0x1a) selecting a binary/unary operator helper | `re/out/threadrun.c` |
| Constant pool (`CONS`) is `u32 count` + entries `u32 type` + payload: 0 string (`u32 len`, bytes, NUL), 3 `(name, extra)`, 4 class (string idx), 5 and 7 two-dword refs; JVM-like (7 = name+descriptor, 5 = class+member). No int/float pool types occur in the corpus. `tufa::Module::pool` parses all 1,546 modules (102,711 entries) with zero leftover bytes | disassembly of `0x407660` and per-type readers; test `every_tufa_constant_pool_parses_exactly` |
| `MTHD` = two arrays `u32 n` + n x 5 dwords; `FILD` = two arrays of n x 4 dwords; `CLSS` = 5 dwords + `head[4]` tail dwords. Method record on the shipped Drag_Track class reads `(kind=1, name idx, signature idx, TREE body idx, extra)`; the last field's meaning is open. All 1,538 MTHD / 206 FILD / 1,546 CLSS chunks parse exactly | `0x4195b0`, `0x419b60`, loader `FUN_00411b20`; test `every_tufa_method_field_and_class_chunk_parses_exactly` |
| Not yet decoded: the semantics of the remaining ops and the value-object helpers (`FUN_0041bfb0`.. etc.); the owner/callers of the Controller registrar at 0x477cb0 (reachable only via vtable `0x5f12f0+4`; no `E8`/`E9` into 0x477cb0-0x477de1 in `.text`) | open |

## What is exact vs open (honest status)
- Exact and corpus-tested: RPAK, SCX v4, cfg, SDAT header, and now the whole TUFA class file (pool, fields, methods, class record, node trees).
- Open: opcode semantics and value helpers of `Thread::run`; method/field record meaning beyond the above; the 353 natives (+7 Controller, open); every physics function; D3D9 rendering; audio.
- The playable game is still the earlier approximation. No exact-port function has a differential (Unicorn) test yet.

## Open work (ordered)
1. Locate the executor and decode the 47 ops; implement them in a Rust VM; differential-test against Unicorn.
2. Port native method registrations (`java.game.*`, `java.render.*`, ...) with oracle tests.
3. Vehicle simulation, collision/damage, game flow, rendering, audio (ADR 0004 order).
