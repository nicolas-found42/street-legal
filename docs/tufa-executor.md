# TUFA executor (`Thread::run`, exe `FUN_00420ff0`)

Status: **oracle-verified at the operator/assign-helper level; protocol-level
elsewhere.** The 27 operator/assignment helpers (`0x41b860` etc.) that
implement sub-ops `0x1007/0x1008` `0x01–0x2e` have Unicorn vectors under
`re/out/vm/` (git-ignored, 281 rows, 281/281 byte-identical round-trip via
`re/oracle/vm_vectors.py verify`). Everything else is decoded from Ghidra
pseudocode (`re/out/threadrun.c`, `re/out/all_pseudo.c`) plus a corpus survey
of 1,044 shipped `.class` files (`reference/raw/{sl/Scripts,system/scripts,parts,accessories}`).
Per-claim provenance is marked **[oracle]**, **[static]** (machine code read),
or **[corpus]** (emitted by the stock compiler). Open items are explicit.

## Machine model

- A thread steps an 8-byte node pointer (`thread+0x24`) through a method body
  (the decoded `TREE` nodes). The default advance is `pc += 8` (one node).
  Jumps add `n * 8` **[static]**.
- Flat node layout (8 bytes): `+0` opword u16, `+2` line-number u16 (statement
  groups in the corpus share it; `run` copies it to `thread+0x70` for error
  reports), `+4` immediate/child-count u32 **[static + corpus]**.
  The high nibble of the opword is the operand class (`0x1000/0x2000/0x4000`
  carry the u32 immediate) **[static]**.
- Operand stack: `frame+0x28` (growable array of value pointers).
  Locals: `frame+0x18` (inline `T_Container`: `+0xc` capacity, `+0x10` data,
  `+0x14` count, `+0x18` self-pointer) **[static]**.
- A value is a ref-counted object (vtable `0x5e734c`): `+0` vtable,
  `+4` type cell, `+8` payload. Slot 0 returns a kind: 2 means "variable
  slot" (an lvalue, vtable `0x5e7344`, real payload at `+8`, cell at `+0xc`),
  otherwise a plain value **[static]**.
  Helpers: `FUN_00423d40` raw pop (underflow is a fatal log),
  `FUN_0041f7d0` dereferencing pop (copies a value out of a variable slot),
  `FUN_00423c00` cell allocator (freelist `DAT_0062f130[4]` or pooled growth),
  `FUN_00423e30` value copy, `FUN_00423ba0` push, and vtable slot 1 releases
  **[static]**.
- Type identity is the **first char** of the type record: `I` int, `F` float,
  `L`/`[` reference/array, plus `V` and the `0x100d` literal marker (all `I`)
  **[static]**. Built-in descriptors are created at start-up (`FUN_0040fe70`)
  from strings at `0x60ab50..` (`DAT_0062def0`/`DAT_0062e00c` = `I`,
  `DAT_0062deec` = `F`, `DAT_0062e000` = `L;`, `DAT_0062e010` = `V`)
  **[static]**.
- `JVM::numericPromotion` (`FUN_00417570`): `F`+`F` stays, `I`+`I` stays,
  mixed `I`/`F` converts the `I` side to `F` (`FUN_00417da0`, exact for small
  ints; `F`→`I` truncates via `__ftol`); `L`+`L` passes; anything else logs
  `cannot promote` **[static]**. Boolean conversion (`FUN_00417840`):
  `F` compares against `+0.0` (`_DAT_005e73cc`), `L`/`[` test the payload
  against null, `I` passes through (integers are already truth values)
  **[static]**.
- Time slicing: `run(budget)` returns the leftover time. The loop stops on a
  status flag (`thread+0x2c & 0x78`), when the scheduler check
  `FUN_00423b90` (`(thread+0x2c & 0x82) == 0`) fails, or when the budget
  (`float10` param, compared via `FUN_005516c0` clock) runs out **[static]**.

## Operand order (settled)

`run` pops the stack top first (`T`, the RIGHT operand) and then the deeper
slot (`N`, the LEFT operand), and calls binary helpers as
`(dst=N, src=T)`; the result overwrites the LEFT cell and the RIGHT cell is
released. So with left-first (standard postfix) codegen, `[A][B][op]`
computes `A op B` **[static + corpus]**. Evidence: the call sequence is
`push edi(=first pop); push esi(=second pop)` with `edi` holding the
first-pop deref-copy and `esi` the second-pop, hence callee `param_2` is the
second pop = LEFT (**[static]**, `0x422300–0x42235c` and the identical
`0x4229d1–0x4229ed` compare sequence); corpus statement shapes
(`X = f(0) * 100` etc.) only simulate with this order (**[corpus]**).
The shift helpers (`0x41aff0` `>>`, `0x41b0e0` `<<`) are the exception: `run`
calls them as `(count=T, value=N)` and the result overwrites the VALUE cell
**[static]**. The vectors record this per-helper in the `dst` field.

## Opcodes (opword → behaviour)

`VA` = handler code reached for the op (the `Thread::run` dispatch itself has
no VA per op; the sub-op helpers do — see the sub-op table). Effect notation:
`S` = operand stack, `L` = locals, `pc` = `thread+0x24`.

| Opword | VA / helper | Stack effect | Semantics |
|---|---|---|---|
| 0x0000, 0x0013 (`0x13` = byte `0x13`), 0x0042, 0x0048, 0x004d, 0x004a | `FUN_004190d0` fatal | — (aborts) | `fatal script error`. 0x42/0x48/0x4d/0x4a are not in the opcode table (`0x5f0914`); they are defensive Ghidra jump-table cases sharing the abort **[static]**. Never emitted **[corpus]** (0x13 absent; 0x0000 absent). |
| 0x1001 | — | `S.push(L[imm] or null)` | Push local `imm` (null when out of range) **[static]**. |
| 0x1002 | `FUN_004050b0/00408d60` | `L.push(new var)`; if next node is `0x002c`, also `S.push(var)` | Declare a local of type `pool[imm]`, with an optional `0x402e` initialiser-type node; the `0x002c` follower marks a declaration-with-value **[static + corpus]**. Never in corpus as a standalone op (always the declare position) — see 0x402e/0x002c rows. |
| 0x1003 | — | `L.pop` (release) | Drop the innermost local; underflow logs `T_Container__underflow` **[static]**. |
| 0x0004 / 0x0005 | `FUN_0041bd90` | net zero (peek, re-push, maybe skip) | Short-circuit: raw-peek the top (no deref), boolean-convert, push it back; `0x0004` skips the next node when true (`\|\|`), `0x0005` when false (`&&`) **[static]**. |
| 0x1007 | sub-op table | per sub-op | Operator, selected by `imm` (see below). Pops operands, pushes exactly one result (except no-op sub-ops) **[static]**. |
| 0x1008 | sub-op table | per sub-op, net pop | Same sub-ops as 0x1007, but the result is discarded (expression statement): binary/unary cases pop and release everything; assignment cases store without pushing; `0x1e new` still pushes (as in 0x1007); `0x22`/`0x1d`/`0xc` are silent no-ops **[static]**. |
| 0x1011 | `FUN_00420d20` | `S.push(resolve(pc children))` | Resolve a name (consumes `imm` child nodes inline) and push it; `name cannot be parsed` is fatal **[static]**. |
| 0x1014 | `FUN_00423b80` (= node `+4`) | — | `pc += imm * 8` (unconditional jump; `imm` is a node count) **[static]**. |
| 0x1015 / 0x1016 | `FUN_0041f7d0` + `FUN_00417840` | `S.pop` | Pop the condition (dereferencing), boolean-convert; jump when false / true **[static]**. |
| 0x1017 | `FUN_0041c5c0` (eq) | `S.pop × 2` | Pop two values (dereferencing copies) and compare; on match jump (`pc += imm*8`), releasing the case value and pushing the switch value back; on mismatch release the switch value and continue **[static]**. |
| 0x101b | `FUN_004117a0/00406bb0` | `S.push(fieldref)` | Push a field reference (static when the pool entry has bit 30 set, otherwise instance; instance with null `frame+0x30` is `JT_FIELD_REF__inst…` fatal) **[static]**. |
| 0x101e | `FUN_00404e60/004109e0` | `S.pop × imm → S.push(array)` | Pop `imm` values and pack them into an argument array (elementwise `0x23`-assign), or push null when `imm` = 0 **[static]**. |
| 0x101f | — | `S.push × imm` | Push `imm` int zeros **[static]**. Never emitted **[corpus]** (constant-fill is done with literals). |
| 0x1027 | — | `S.push(int imm)` | Push int literal `imm` (payload = the u32 immediate, type `I`) **[static]**. |
| 0x401c | `FUN_00408800` etc. | `S.pop → S.push` | Pop an object and push its member (`illegal fieldaccess` on null; missing member is fatal) **[static]**. |
| 0x4025 / 0x4026 | `FUN_00408a30` / `FUN_004207c0` | pops per callee | Instance / static method call through `callMethod` (below); null receiver is `illegal methodcall` fatal **[static]**. |
| 0x0010 | — | — (stops) | Stop the thread (sets flag `0x40`; the loop then exits via `0x2a/0x2b`-style epilogue when `pc` is null) **[static]**. |
| 0x0018 | — | `S.pop` (release) | Pop and release (dereferences lvalues via `FUN_0041f7d0` first) **[static]**. |
| 0x001d | `FUN_00404820/00408800` | `S.push(field)` | Push a field by name (global lookup when `frame+0x30` is 0, else instance-field lookup) **[static]**. Never emitted **[corpus]** (field access goes through 0x101b/0x401c). |
| 0x0020 | `FUN_00423ea0` | `S.pop × 2 → S.push` | Array element access: pops index (`I`) then array (`[`); negative index, non-array, or non-`I` index is fatal; null array is fatal **[static]**. |
| 0x0021 / 0x0022 / 0x0023 / 0x0024 | `FUN_00408a30/00408a70/00408a90` → `FUN_0041fbc0` | pop callee args; `0x0024` pushes one return slot | Invoke `<init>` / super-constructor / outer-constructor / general call. `0x0024` resolves the target with `FUN_004208e0` over the following name nodes (a `0x1011`-shaped inline name) and pushes a return-value placeholder; `0x0021–0x0023` push nothing (their wrappers return status flags). Native targets (record flag `-2`) go through the native registry instead of a script frame **[static + corpus]** (return-slot use verified in corpus statement shapes). |
| 0x0028 | `FUN_00423ba0` | `S.pop` | Pop and set the return value slot **[static]**. |
| 0x0029 | — | `S.push(int 0)` | Push void (an int-zero cell from the value pool) **[static]**. |
| 0x002a / 0x002b | `FUN_0041f7b0/0041f7c0` | frame pop | Return / return-with-value (restore `pc` from `frame+0x2c`, pop the frame); `0x002a/0x002b` with null `pc` set flag `0x40` and exit **[static]**. |
| 0x002d | `FUN_0041acb0(…, 0x23)` | `S.pop × 2` | Plain assignment: pop value, pop lvalue (must be a variable slot, else `stack access…` fatal), store **[static]**. |
| 0x1006 (class name) | — (structural) | — | Consumed inline: the class operand of `0x1e new` / the `0x1021` cast check. Stepping on it standalone silently advances **[static + corpus]** (emitted 2,065×). |
| 0x100b (int literal) | — (structural) | — | Literal payload of a preceding `0x1007/0x1c` (type `I`, value = next node's `imm`) **[static + corpus]** (12,871×). |
| 0x100c (`0x000c`, null literal) | — (structural) | — | Literal payload of `0x1007/0x1c`: pushes a null (`L;`, payload 0) **[static + corpus]** (1,537×). |
| 0x100d (`I` literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (type `I`) **[static]**. Never emitted **[corpus]**. |
| 0x100e (string literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (resolves `pool[imm]` to a string) **[static + corpus]** (14,404×). |
| 0x100f | — (structural) | — | Never emitted **[corpus]**; no standalone case (silent advance) **[static]**. |
| 0x200a (float literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (type `F`, payload = next node's `imm` bits) **[static + corpus]** (12,819×). |
| 0x4009 (class literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (`FUN_004173d0` interns `pool[imm]`) **[static + corpus]** (1,352×). |
| 0x1019 (name part) | — (structural) | — | Name leaf consumed inline by `0x1011`/`0x0024`/`0x1021`-family resolution **[static + corpus]** (13,024×). |
| 0x101a (name marker) | — (structural) | — | Structural name leaf (same consumption family as `0x1019`); standalone = silent advance **[static + corpus]** (13,502×). |
| 0x4012 (name signature) | — (structural) | — | Pairs with `0x1019` inside `0x1011(2)` names (method name + signature); standalone = silent advance (`0x4012` has no dispatch case) **[static + corpus]** (12,540×). |
| 0x402e (declarator tag) | — (structural) | — | Initialiser-type child of `0x1002` declare **[static]**. Never emitted **[corpus]**. |
| 0x002c | — (structural) | — | Declaration-with-value marker consumed by `0x1002` **[static + corpus]** (2,257×). |
| 0x1007 sub `0x1b` | — | net zero | **Silent no-op** (`pc += 8` only, stack untouched): missing from the `0x1007` sub-switch. Emitted 4,876×, always right after an inline call name / name leaf (`0x101a`, `0x4012`, `0x1011`) — a compiler barrier/marker whose codegen purpose is open **[static + corpus]**. |
| 0x1007 sub `0x1d`, `0xc`, `0x22` | — | net zero | Silent no-ops (missing from the switch) **[static]**; never emitted **[corpus]**. |
| 0x1008 sub `0x22` | — | net zero | Silent no-op (missing from the `0x1008` switch). Emitted 1,167×, always after the non-pushing calls `0x0022` (34×) / `0x0023` (1,133×): a statement-boundary marker after void calls **[static + corpus]**. |
| 0x1008 sub `0x1b` | — | `S.pop` (release) | Pop one value (dereferencing) and release: statement-leftover cleanup. Emitted 9,290× **[static + corpus]**. |

## Operator sub-ops (`0x1007` / `0x1008` `imm`)

Each helper is identified by the operator named in its own error string.
`L`/`R` = LEFT (deeper stack slot, result cell) and RIGHT (stack top);
`dst` names the recorded result cell (`0` = LEFT, `1` = RIGHT — only the
shifts use `1`). All rows below are **[oracle]** (`re/out/vm/vm_<name>.jsonl`,
counts = generated + verified rows).

| imm | op | helper VA | dst | Semantics (oracle-verified) |
|---|---|---|---|---|
| 1 | `\|\|` | `0x41bfb0` | 0 | `FUN_0041bfb0(L,R)`: boolean-convert both (int-only after conversion); `R = (L != 0) \|\| (R != 0)`. 6 vectors. |
| 2 | `&&` | `0x41bea0` | 0 | `FUN_0041bea0(L,R)`: `R = (L != 0) && (R != 0)`. 6 vectors. |
| 3 | `\|` | `0x41c290` | 0 | `FUN_0041c290(L,R)`: promote, int-only; `L \|= R`. 5 vectors. |
| 4 | `^` | `0x41c380` | 0 | `FUN_0041c380(L,R)`: promote, int-only; `L ^= R`. 5 vectors. |
| 5 | `&` | `0x41c1a0` | 0 | `FUN_0041c1a0(L,R)`: promote, int-only; `L &= R`. 5 vectors. |
| 6 | `!=` | `0x41c6c0` | 0 | `FUN_0041c6c0(L,R)`: **logical NOT of the `==` helper** (`bVar1 = (L == R); R = !bVar1`) — the negation is inside the helper, confirmed **[static + oracle]**. Int/ref compare payload words; float via x87 (see NaN). 13 vectors. |
| 7 | `==` | `0x41c5c0` | 0 | `FUN_0041c5c0(L,R)`: int/ref: `R = (L == R)`; float: `fld/fcomp/fnstsw/test ah,0x40` — C3 set means equal. 13 vectors. |
| 8 | `>=` | `0x41c860` | 0 | `FUN_0041c860(L,R)`: `R = (L == R) \|\| (L > R)` (eq-or-gt, not negated). 13 vectors. |
| 9 | `<=` | `0x41cb60` | 0 | `FUN_0041cb60(L,R)`: **NOT of gt** (`bVar1 = (R < L); R = !bVar1`, i.e. `R = !(R < L)`) — the negation is inside the helper, confirmed **[static + oracle]**. 13 vectors. |
| 0xa | `>` | `0x41c790` | 0 | `FUN_0041c790(L,R)`: `R = (R < L)` (int and float). 13 vectors. |
| 0xb | `<` | `0x41c9e0` | 0 | `FUN_0041c9e0(L,R)`: **NOT of (eq-or-gt)**: returns 0 when `L == R` or `R < L`, else 1 (i.e. `R = !(L == R \|\| R < L)`) — the negation is inside the helper, confirmed **[static + oracle]**. 13 vectors. |
| 0xc | `>>>` | — | — | **Unsupported**: no case in either switch (silent no-op); never emitted **[static + corpus]**. |
| 0xd | `>>` | `0x41aff0` | 1 | `FUN_0041aff0(count=T, value=N)`: promote, int-only; `value >>= (count & 31)` (arithmetic). Result lands in the VALUE cell (`dst 1`). 8 vectors. |
| 0xe | `<<` | `0x41b0e0` | 1 | `FUN_0041b0e0(count=T, value=N)`: `value <<= (count & 31)`. `dst 1`. 8 vectors. |
| 0xf | `-` | `0x41b760` | 0 | `FUN_0041b760(L,R)`: negate R in place (`F`/`I`, else fatal), then `L += (-R)` via the add helper. So `L = L - R` with float/int promotion. 7 vectors. |
| 0x10 | `+` | `0x41b4d0` | 0 | `FUN_0041b4d0(L,R)`: string-concatenation check first (`FUN_00417a70`, converts one side when exactly one side is `Ljava.lang.String;`), then promote; `F`: `L += R`; `I`: `L += R`; `L`-String: concatenate (malloc + copy, interns via `FUN_004174a0`); other classes are fatal. The `L`-String path needs a booted string pool and has **no vectors (open)**; int/float paths have 7 vectors **[oracle for I/F; static for L]**. |
| 0x11 | `%` | `0x41bbf0` | 0 | `FUN_0041bbf0(L,R)`: int-only (`%` via `idiv`; returns quotient in `eax` too, ignored by `run`); logs `division by zero` then traps (`#DE`) on zero divisor — zero-divisor vectors excluded by construction, behaviour documented **[static]**; 5 vectors **[oracle]**. |
| 0x12 | `/` | `0x41b970` | 0 | `FUN_0041b970(L,R)`: `F`: zero divisor logs then yields `±inf` (no trap — 4 vectors incl. `±1.5/±0.0` **[oracle]**); `I`: truncating `idiv`, logs then traps on zero divisor (excluded) **[static]**; 9 vectors. |
| 0x13 | `*` | `0x41b860` | 0 | `FUN_0041b860(L,R)`: promote; `F`/`I` multiply (wraps for int). 11 vectors. |
| 0x14 | `~` | `0x41c0c0` | 0 | `FUN_0041c0c0(V)`: unary int (`FUN_00417830` is a no-op deref guard); `V = ~V`. 8 vectors. |
| 0x15 | `!` | `0x41bdb0` | 0 | `FUN_0041bdb0(V)`: boolean-convert, int-only; `V = (V == 0)`. 8 vectors. |
| 0x16 | unary `-` | `0x41b1d0` | 0 | `FUN_0041b1d0(V)`: `F`/`I` negate in place. 8 vectors. |
| 0x17 / 0x19 | `++` pre/post | `0x41b2d0` | 0 | `FUN_0041b2d0(V)`: V must be an lvalue (kind 2, else `stack access…` fatal); pushes a copy, then increments in place: pre pushes the copy **before** incrementing (new value), post increments **before** copying (old value). `F` adds `_DAT_005f08f0` (`1.0f`), `I` adds 1. 8 vectors each (operands are plain values; the lvalue gate is **[static]**). |
| 0x18 / 0x1a | `--` pre/post | `0x41b3d0` | 0 | Mirror of `++` with `-1.0f`/`-1`. 8 vectors each. |
| 0x1c | literal | — | push | Read the next node as the literal (`0x100e` string, `0x000c` null, `0x100b` int, `0x100d` `I`, `0x200a` float, `0x4009` class); anything else is `unhandled literal` fatal **[static + corpus]**. |
| 0x1e | `new` | `FUN_0041acb0`-adjacent | push | Allocate the object (`FUN_00410890` class init, `FUN_00404e20` sizing, `FUN_00423c00` + `FUN_00423e60`), store the size word at `+8`, push it. Consumes the following `0x1006` class-name node **[static + corpus]** (1,949×). No vectors (needs class loading; open). |
| 0x1f | `new T[]` | — | push | One-dimensional array only (`multi dimensional array` is fatal; element type from the `[...` signature; sizes popped from the stack). No vectors (needs class loading; open) **[static + corpus]** (116×). |
| 0x20 | `instanceof` | `FUN_00423ea0/00408800` | push | Pops the value (dereferencing), advances `pc` past the check node, pushes int 1/0 (`I` index, `[` array; bad index type, non-array, or negative index are fatal) **[static + corpus]** (80×). No vectors (needs class hierarchy; open). |
| 0x21 | cast | `FUN_00417da0/00417d80` | push | Pops the value; the following node must be `0x1006` (class) or `0x1019` (name); failed casts are `cannot cast` fatal **[static + corpus]** (42×). No vectors (needs class hierarchy; open). |
| 0x23–0x2e | assign | `0x41acb0` | 0 (0x1007) / drop (0x1008) | `FUN_0041acb0(dst, src, kind)`: `0x23` plain (`FUN_0041ab60`, with assignment conversion + `L`/`[` refcount/GC hook), `0x24` `*=`, `0x25` `/=`, `0x26` `%=`, `0x27` `+=`, `0x28` `-=`, `0x2c` `&=`, `0x2d` `^=`, `0x2e` `\|=` (compound ops reuse the arithmetic helpers on a temp). `0x1007` pushes the stored value back; `0x1008` releases it. 8–11 vectors per kind (85 total) **[oracle]**. |

### NaN compares (settled, oracle-verified)

The float equality helper uses `fld L; fcomp R; fnstsw ax; test ah,0x40`
(C3 = `ST0 == ST1`). An **unordered** comparison (either side NaN) sets
C3+C2+C0, so C3 reads *set*. Micro-verified with a raw `fcomp(nan, 1.0)`
under Unicorn (`fnstsw ax = 0x4500`) **[oracle]**. Consequences, all in the
vectors:

- `==` treats NaN as equal: `nan == nan`, `nan == 1.0`, `1.0 == nan` → 1.
- `!=` (its negation) → 0 for any NaN side.
- `>=` (eq-or-gt) → 1 for any NaN side; `>` (gt) → 0.
- `<=` (not-gt) → 1 for any NaN side; `<` (not-(eq-or-gt)) → 0.

`+0.0 == -0.0` → 1 (x87 equality) **[oracle]**. Note the Unicorn PC
limitation (`docs/x87.md`): these results are PC-insensitive by construction
(flag test after `fcomp`), so the Unicorn leg is trustworthy here.

## Frames and calls

- `Thread::callMethod` = `FUN_0041fbc0` (this = thread). Entry: `param_2` =
  call record (`[0]` class, `[2]` method), plus receiver. Native methods
  (record flag `0x40`) skip frame creation; script methods create a frame
  with `FUN_00407b10` (pool allocator `FUN_00408d90` over `DAT_0062ddf0`)
  **[static]**.
- `FUN_00407b10(this=frame, a1..a5)` stores (machine-code verified with
  sentinel args under Unicorn **[oracle]**): `frame+0x3c = a1`,
  `frame+0x34 = a2`, `frame+0x2c = a3`, `frame+0x30 = a4`, local count =
  `a5` (locals container `+0xc/+0x10/+0x14/+0x18`, stack container at
  `+0x28` = `&frame+0x1c`, vtable `0x5e7410`) **[oracle + static]**.
  Call-site push order (`push edx; push ecx; push esi; push ebp; push edi`)
  gives `a5=edx … a1=edi` **[static]**.
- Roles during `run`:
  - `frame+0x30` = **receiver** (`a4` = `callMethod` receiver; 0 for
    static). `0x101b`/`0x001d` branch on nullness: null → global/static
    path, non-null → instance path **[static]**.
  - `frame+0x2c` = **saved pc**: initialised to the body entry (`a3` = body
    node pointer — `run` starts with `thread pc = frame+0x2c`), saved by
    `FUN_0041f7c0` on call, restored by `FUN_0041f7b0` on return
    **[static]**.
  - `frame+0x34` = **class context** (`a2`): `+0x14` is the constant pool
    (static name resolution in `0x1011`/`0x101b`/casts), `+0x4` feeds the
    loop-header error file/line lookup, `+0x1c8` chains to the superclass
    for `0x4026`/`0x0023` resolution **[static]**. The exact heap identity
    of the object passed as `a2` is narrowed but **open** (candidate: the
    class object produced by the `+0x14` pool lookup slot; the writer slot
    is statically ambiguous — see below).
  - `(frame+0x34)+0x1c8` = **superclass link**: the `+0x1c8` walkers
    (`FUN_00404500/00404730/004047c0/00404840/00404d40`) are class-hierarchy
    walks, and `FUN_00403e80` writes `class+0x1c8` from the superclass
    lookup (null when the base index is negative) **[static]**.
  - `frame+0x38` = method signature string (written directly in `callMethod`
    from the resolved name, not via the ctor) **[static]**.
  - `frame+0x3c` = `a1` (class-ish record word; linked into the thread's
    frame list at `thread+0xc`) **[static]**.
  - Thread flags (`thread+0x2c`): 4 = running, `0x10` = waiting, `0x20` =
    notified, `0x40` = stopped, `0x80` = dead; `0x78` aborts the loop,
    `0x82` blocks the scheduler check **[static]** (`FUN_0041f650/0041f6f0/
    0041f780`, `Thread::wait/notify`).
- Calls by name (`FUN_004207c0`, used by `0x4026` and `<init>`/super/outer
  wrappers `FUN_00408a30/00408a70/00408a90`) resolve `(class, name, sig)`
  through `FUN_00404910`; a miss is `Thread::callMethod: %s.%s%s not found`
  fatal **[static]**.
- **Open (narrowed):** which heap object the script path passes as ctor
  `a2` (`frame+0x34`). The `+0x18`-local slot feeding it is written on some
  paths but never on the straight script path in the static disassembly, so
  the writer is ambiguous between the class object and the name-lookup
  result; all *reads* treat it as the class context above. Resolving it
  needs a booted JVM (class loading + a live `callMethod`), which is beyond
  the helper-level oracle.

## Vectors (`re/out/vm/`, git-ignored)

`re/oracle/vm_vectors.py gen` builds minimal value cells directly in
emulator memory (no JVM boot): each cell is
`[vtable 0x5e734c][cstr I/F/L][payload word]`; `DAT_0062e00c` etc. point at
heap cells holding the static type cstrings (production shape —
`byte [[v+4]]` reads observe the type char either way). `this` is a zeroed
scratch whose `[0]` points at a fake thread carrying `+0x70/+0x74` cstrings
so diagnostic paths stay mapped; the log sinks (`0x551140/0x551220/0x5513b0`)
are stubbed (error-reporting only — `FUN_004190d0` fatal is *not* stubbed,
so reaching it fails loudly). `cw 0x7f` (game word); float operands stay
exactly representable (halves/small ints) so results are PC-insensitive per
`docs/x87.md`.

`verify` re-runs every row on a fresh `Oracle` and requires byte-identical
`(type, payload)` words: **281/281** across 32 files (one per helper; the
assign helper has 9 kind files). Recorded per row: `id/exe_addr/name/
convention/args/dst/result/oracle/unicorn/cw/date` (M0-08 conventions).

| File(s) | Helper(s) | Rows | Notes |
|---|---|---|---|
| `vm_mul/add/sub/div/mod` | `0x41b860/0x41b4d0/0x41b760/0x41b970/0x41bbf0` | 11/7/7/9/5 | int wrap (`2³¹−1 × 2³¹−1`), `INT_MIN` edges, float `±inf` via `/0.0`; int `/0`, `%0`, `INT_MIN/-1` excluded (log-then-`#DE` trap **[static]**). |
| `vm_bor/band` (`\|\|`,`&&`) | `0x41bfb0/0x41bea0` | 6/6 | int-only after boolean conversion. |
| `vm_bitor/bitxor/bitand` | `0x41c290/0x41c380/0x41c1a0` | 5/5/5 | int-only after promotion. |
| `vm_shr/shl` | `0x41aff0/0x41b0e0` | 8/8 | `dst 1` (value cell); count `& 31` incl. 32/33/−1; arithmetic `>>`. |
| `vm_eq/ne/ge/le/gt/lt` | `0x41c5c0/0x41c6c0/0x41c860/0x41cb60/0x41c790/0x41c9e0` | 13 × 6 | int edges, `±0.0`, `±inf`, NaN matrix (see above), reference identity (`L` payload words, incl. null). |
| `vm_neg/lnot/uminus/inc/dec` | `0x41c0c0/0x41bdb0/0x41b1d0/0x41b2d0/0x41b3d0` | 8 × 5 | unary; `inc/dec` cover `F` (`±1.0f`) and `I` wrap; the lvalue gate (kind 2) is static-only. |
| `vm_assign23/24/25/26/27/28/2c/2d/2e` | `0x41acb0` kinds | 8/11/11/11/8/8/8/8/8 | incl. mixed `I`/`F` (result type follows promotion — the dst cell type can *change*, recorded), `L` word-identity, `-0.0` preservation; kinds `0x24/0x25` skip zero-src rows (log-then-trap). |

Deliberately **without** vectors (open): the `+` string-concatenation path
(needs a booted string pool: `FUN_00408800/004174a0/00417a70`), `new` /
`new[]` / `instanceof` / casts (need class loading), and full call
protocols (`0x0021–0x0024`, `0x4025/0x4026` arg shuffling needs a loaded
method + live frames).

## Open (explicit)

- `frame+0x34` writer identity (narrowed, see above).
- The `0x1007/0x1b` marker's codegen purpose (behaviour: certain no-op).
- `0x4012`'s exact field semantics (pairs with `0x1019` in `0x1011(2)`
  method names; consumed inline, never stepped).
- `0x0021` `<init>` receiver handoff to `FUN_0041fbc0` (record-driven, but
  which record cell carries a freshly `new`ed object is unconfirmed
  without a live call).
- `0x101e` arg-array element order beyond the push loop (elementwise
  `0x23`-assign from the top of a reversed staging area — matches
  left-to-right per the `local_400` index walk, but no oracle coverage).
- String concatenation, `new`/`new[]`, `instanceof`, casts, and the
  `0x0021–0x0024`/`0x4025`/`0x4026` call arg shuffling have no vectors
  (need a booted JVM); their machine-code semantics above are static-only.
