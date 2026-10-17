# Reverse-engineering progress (stock build 606)

Status: **early.** The exact port described in ADR 0004 is not started beyond tooling and the findings below.
The playable slice in `slr-core`/`slr-game` is still the old approximation.

## Tooling (reproducible)
- `re/ghidra_scripts/ExportAll.java`: headless Ghidra export of every function's pseudocode to `re/out/` (git-ignored, derived from the exe).
  4,356 functions exported from `StreetLegal_Redline.bak.exe`.
- Oracle: a Python venv with `unicorn`, `capstone`, `pefile` (kept outside the repo) for differential tests. No differential test exists yet.

## TUFA (script VM) findings
| Fact | Evidence |
|---|---|
| Class loader is `FUN_00411b20` (`JVM::addClass`); chunks `TREE` `DHTM`(MTHD) `DLIF`(FILD) `CONS` `CLSS` `TUFA` | pseudocode in `re/out/all_pseudo.c` |
| Serialized tree node = 1 opcode byte + u16 operand + (u32 immediate iff op class is 0x1000/0x2000/0x4000); size 3 or 7 | `FUN_0041a5f0`, `FUN_0041a5c0` |
| Opcode byte indexes a u16 table at VA 0x5f0914; 47 ops (0x00-0x2e); high nibble = operand class | bytes at 0x5f0914 |
| In memory a node is a C++ object, vtable 0x5f0974, opword at +0x14, immediate at +0x18, children list at +0x10 | `FUN_0041a700` |
| The exe also contains a Java-like source compiler (`JVM::compileSource`, yacc parser, type checks, a ~14.7k-line `compileHook`) | strings 0x60b4f8.. |
| 339 native methods are registered through `FUN_00416b00(class, name, signature, impl)`; the table is extracted to `re/out/native_api.tsv` (largest groups: WheelRef 29, Part 26, GroundRef 25, Chassis 23, System 22, ResourceRef 20, GameRef 18) | `FUN_004427e0` and siblings |
| Package roots map to script dirs (`java.lang`->system/scripts/lang, `java.game`->sl/scripts/game, parts, accessories) | `FUN_0041cfe0` calls near line 10034 |
| `tufa::Module::trees` decodes every stock class exactly: 1,546 modules, 5,816 bodies, 320,522 nodes, zero trailing bytes (test `every_tufa_tree_decodes_exactly`). The 11 modules in `render/osd/Dialog.class` are version 0x108f5, which the exe cannot decode | `crates/slr-formats` |
| Executor is `Thread::run` = `FUN_00420ff0`: postfix stack machine over ref-counted values; opcode 0x1007 carries a sub-op (0..0x1a) selecting a binary/unary operator helper | `re/out/threadrun.c` |
| Constant pool (`CONS`) is `u32 count` + entries `u32 type` + payload: 0 string (`u32 len`, bytes, NUL), 3 `(name, extra)`, 4 class (string idx), 5 and 7 two-dword refs; JVM-like (7 = name+descriptor, 5 = class+member). No int/float pool types occur in the corpus. `tufa::Module::pool` parses all 1,546 modules (102,711 entries) with zero leftover bytes | disassembly of `0x407660` and per-type readers; test `every_tufa_constant_pool_parses_exactly` |
| `MTHD` = two arrays `u32 n` + n x 5 dwords; `FILD` = two arrays of n x 4 dwords; `CLSS` = 5 dwords + `head[4]` tail dwords. Method record on the shipped Drag_Track class reads `(kind=1, name idx, signature idx, TREE body idx, extra)`; the last field's meaning is open. All 1,538 MTHD / 206 FILD / 1,546 CLSS chunks parse exactly | `0x4195b0`, `0x419b60`, loader `FUN_00411b20`; test `every_tufa_method_field_and_class_chunk_parses_exactly` |
| Not yet decoded: the semantics of the remaining ops and the value-object helpers (`FUN_0041bfb0`.. etc.) | open |

## What is exact vs open (honest status)
- Exact and corpus-tested: RPAK, SCX v4, cfg, SDAT header, and now the whole TUFA class file (pool, fields, methods, class record, node trees).
- Open: opcode semantics and value helpers of `Thread::run`; method/field record meaning beyond the above; the 339 natives; every physics function; D3D9 rendering; audio.
- The playable game is still the earlier approximation. No exact-port function has a differential (Unicorn) test yet.

## Open work (ordered)
1. Locate the executor and decode the 47 ops; implement them in a Rust VM; differential-test against Unicorn.
2. Port native method registrations (`java.game.*`, `java.render.*`, ...) with oracle tests.
3. Vehicle simulation, collision/damage, game flow, rendering, audio (ADR 0004 order).
