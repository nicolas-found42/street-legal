# TUFA executor (`Thread::run`, exe `FUN_00420ff0`)

Status: **inferred from Ghidra pseudocode only.** Nothing here is oracle-verified yet. Each row needs a
Unicorn differential test before it counts as exact (ADR 0004, tier B).

## Machine model
- A thread steps an 8-byte node pointer (`thread+0x24`) through a method body (the decoded `TREE` nodes).
  The default advance is `pc += 8` (one node). Jumps add `n * 8`.
- Operand stack: `frame+0x28` (growable array of value pointers). Locals: `frame+0x18`.
- A value is a ref-counted object (vtable `0x5e734c`): `+4` = type descriptor, `+8` = payload.
  Helpers: `FUN_00423d40` pops, `FUN_00423c00` allocates, `FUN_00423e30` copies, and vtable slot 1 releases.
  Vtable slot 0 returns a kind, where 2 means "variable slot" (an lvalue) as opposed to a plain value.
- Built-in type descriptors are created at start-up (`FUN_0040fe70`) from strings at `0x60ab50..`:
  `DAT_0062def0` = `I` (used for 0x100d literals), `DAT_0062e00c` = `I` (int), `DAT_0062deec` = `F`
  (float), `DAT_0062e000` = `L;` (null reference), `DAT_0062e010` = `V` (void).
- Time slicing: `run(budget)` returns the leftover time. The loop stops on a status flag
  (`thread+0x2c & 0x78`), when the scheduler check `FUN_00423b90` fails, or when the budget runs out.

## Opcodes (opword → behaviour)
| Opword | Behaviour (inferred) |
|---|---|
| 0x0000, 0x0013 | fatal script error |
| 0x1001 | push local `imm` |
| 0x1002 | declare a local of type `pool[imm]`, with an optional 0x402e initialiser type; pushed if followed by 0x002c |
| 0x1003 | drop the innermost local |
| 0x0004 / 0x0005 | short-circuit: peek the condition; if it is true / false, skip the next node (`||` / `&&`) |
| 0x1007 | operator, selected by `imm` (see the operator table); 0 ternary; 0x17/0x18 pre-inc/dec; 0x19/0x1a post-inc/dec; 0x1c literal (next node 0x100e string, 0x000c null, 0x100b int, 0x100d `I`, 0x200a float, 0x4009 class); 0x1e `new`; 0x1f `new T[]` (one dimension only); 0x20 `instanceof`; 0x21 cast; 0x23–0x2e assignment via `FUN_0041acb0(dst, src, kind)` |
| 0x1008 | same sub-ops as 0x1007, but the result is discarded (expression statement) |
| 0x1011 | resolve a name and push it (`FUN_00420d20`) |
| 0x1014 | jump by `FUN_00423b80()` nodes |
| 0x1015 / 0x1016 | pop the condition; jump if false / true |
| 0x1017 | pop two values and compare (switch case); jump on match |
| 0x101b | push a field reference (static when bit 30 is set, otherwise instance) |
| 0x101e | pop `imm` values and pack them into an argument array, or push null when `imm` = 0 |
| 0x101f | push `imm` int zeros |
| 0x1027 | push int literal `imm` |
| 0x401c | pop an object and push its member (`illegal fieldaccess` on null) |
| 0x4025 / 0x4026 | instance / static method call |
| 0x0010 | stop the thread (flag 0x40) |
| 0x0018 | pop and discard |
| 0x001d | push a field by name (on `this`, or static) |
| 0x0020 | array element access (`I` index, `[` array) |
| 0x0021 / 0x0022 / 0x0023 / 0x0024 | invoke `<init>` / super / outer / general call (native when the record's flag is −2) |
| 0x0028 | pop and set the return value |
| 0x0029 | push void |
| 0x002a / 0x002b | return / return a value (pop the frame) |
| 0x002d | plain assignment (`kind` 0x23) |

## Operator sub-ops (0x1007 / 0x1008 `imm`)
Each helper is identified by the operator named in its own error string (read from the exe).
| imm | op | helper | notes |
|---|---|---|---|
| 1 | `\|\|` | 0x41bfb0 | int only |
| 2 | `&&` | 0x41bea0 | int only |
| 3 | `\|` | 0x41c290 | int only |
| 4 | `^` | 0x41c380 | int only |
| 5 | `&` | 0x41c1a0 | int only |
| 6 | `!=` | 0x41c6c0 | eq helper, negated (negation to confirm) |
| 7 | `==` | 0x41c5c0 | int/float/reference |
| 8 | `>=` | 0x41c860 | eq or gt |
| 9 | `<=` | 0x41cb60 | gt, negated (to confirm) |
| 0xa | `>` | 0x41c790 | `a > b` |
| 0xb | `<` | 0x41c9e0 | eq or gt, negated (to confirm) |
| 0xc | none | none | no case: `>>>` is unsupported |
| 0xd | `>>` | 0x41aff0 | arithmetic, shift count `& 31` |
| 0xe | `<<` | 0x41b0e0 | shift count `& 31` |
| 0xf | `-` | 0x41b760 | int/float |
| 0x10 | `+` | 0x41b4d0 | int/float; string concatenation for `Ljava.lang.String;` |
| 0x11 | `%` | 0x41bbf0 | int only; error on division by zero |
| 0x12 | `/` | 0x41b970 | int/float; error on division by zero |
| 0x13 | `*` | 0x41b860 | int/float |
| 0x14 | `~` | 0x41c0c0 | unary, int |
| 0x15 | `!` | 0x41bdb0 | unary, int: `x == 0` |
| 0x16 | unary `-` | 0x41b1d0 | int/float |
| 0x17 / 0x19 | `++` pre/post | 0x41b2d0 | int/float |
| 0x18 / 0x1a | `--` pre/post | 0x41b3d0 | int/float |

## Frames and calls (from `FUN_0041fbc0` Thread::callMethod, `FUN_004207c0`, `FUN_0041f7b0/7c0`)
- `thread+0x20` = current frame; `thread+0x24` = pc; `frame+0x2c` = saved pc. `FUN_0041f7c0` saves pc into the
  frame, and `FUN_0041f7b0` restores it (used by 0x002a/0x002b return).
- Method record (runtime): `+4` flags (bit 3 = static, so an instance call without a receiver is an error;
  bit 6 = no script body, which is the native path), `+8` name pool idx, `+0x10` index of the body in the class's body table
  (`class+0x18`), `+0x14` passed to frame creation (likely the local count). This matches the file `MTHD`
  record `(kind, name, signature, body, extra)`.
- Call: save pc into the caller frame, allocate a frame (`FUN_00407b10`), link it into the thread's frame list,
  and for instance methods push `this` as local 0 (`FUN_00423ec0` with the receiver's class).
- Calls by name (`FUN_004207c0`) resolve `(class, name, signature)` through `FUN_00404910`; a miss raises
  `Thread::callMethod: %s.%s%s not found`.
- 0x0018 pops and dereferences an lvalue (`FUN_0041f7d0` copies a value out of a variable slot).
- Thread status flags (`thread+0x2c`): 4 = running, 0x40 = stopped by 0x0010; `0x78` aborts the loop and
  `0x82` makes `FUN_00423b90` (can-continue) false.

## Open
- Confirm the negations for 6, 9 and 0xb; the float `==` uses an x87 compare, so check NaN behaviour under the oracle.
- The frame layout fields `+0x30`, `+0x34`, `+0x1c8`, and the call/return protocol. Jump target: `FUN_00423b80` returns the node's own `+4` field, so the immediate is a node-count offset.
- Conversion rules (`JVM::numericPromotion`, `FUN_00417570`): only int↔float promotion exists, plus `L` references.
