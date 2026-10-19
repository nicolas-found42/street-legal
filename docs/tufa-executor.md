# TUFA executor (`Thread::run`, exe `FUN_00420ff0`)

Status: **oracle-verified at the operator/assign-helper level and, since
M1-03, for the interpreter loop itself over synthetic programs (see "Rust
port status" at the end); protocol-level elsewhere.** The 27 operator/assignment helpers (`0x41b860` etc.) that
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
| 0x0020 | `FUN_00423ea0` | `S.pop × 2 → S.push` | Array element access: pops array (`[`) then index (`I`); negative index, non-array, or non-`I` index is fatal; null array is fatal **[static]**. |
| 0x0021 / 0x0022 / 0x0023 / 0x0024 | `FUN_00408a30/00408a70/00408a90`, `FUN_004208e0` → `FUN_004207c0/0041fbc0` | method dependent | `0x21` invokes `<init>` on the object below the arguments and count marker; `0x22` invokes the current defining class constructor on `this`; `0x23` invokes its superclass constructor (absent superclass returns without dispatch). `0x24` resolves following inline name records. Native flag is `0x40`; sentinel `-2` means a dynamic method name. **[oracle]** (`vmexec_constructors`: actual420ff0/wrappers with4207c0 recording boundary; `vmexec_calls/frames/native`: controlled metadata and actual dispatch/frame/return). Script void returns push no value; a native `V` return pushes `V:0`. No whole-world boot claim. |
| 0x0028 | `FUN_00423ba0` | `S.pop` | Pop and set the return value slot **[static]**. |
| 0x0029 | — | `S.push(int 0)` | Push void (an int-zero cell from the value pool) **[static]**. |
| 0x002a / 0x002b | `FUN_0041f7b0/0041f7c0` | frame pop | Return / return-with-value (restore `pc` from `frame+0x2c`, pop the frame); `0x002a/0x002b` with null `pc` set flag `0x40` and exit **[static]**. |
| 0x002d | `FUN_0041acb0(…, 0x23)` | `S.pop × 2` | Plain assignment: pop value, pop lvalue (must be a variable slot, else `stack access…` fatal), store **[static]**. |
| 0x1006 (class name) | — (structural) | — | Consumed inline: the class operand of `0x1e new` / the `0x1021` cast check. Stepping on it standalone silently advances **[static + corpus]** (emitted 2,065×). |
| 0x100b (int literal) | — (structural) | — | Literal payload of a preceding `0x1007/0x1c` (type `I`, value = next node's `imm`) **[static + corpus]** (12,871×). |
| 0x100c (`0x000c`, null literal) | — (structural) | — | Literal payload of `0x1007/0x1c`: pushes a null (`L;`, payload 0) **[static + corpus]** (1,537×). |
| 0x100d (`I` literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (type `I`) **[static]**. Never emitted **[corpus]**. |
| 0x100e (resource literal) | `FUN_00406660` → `FUN_004122b0` | `S.push(I)` | Reads a Member record from the defining frame's pool: its on-disk `name` is the package-string index and `extra` the raw resource ID. The resource-world boundary resolves them to the global ID. A non-Member produces `I:0`. **[oracle]** (`vmexec_resource*`; real420ff0/406660/407210, controlled4122b0 result); 14,404 occurrences **[corpus]**. |
| 0x100f | — (structural) | — | Never emitted **[corpus]**; no standalone case (silent advance) **[static]**. |
| 0x200a (float literal) | — (structural) | — | Literal payload of `0x1007/0x1c` (type `F`, payload = next node's `imm` bits) **[static + corpus]** (12,819×). |
| 0x4009 (String literal) | `FUN_00406140` → `FUN_004173d0` → `FUN_004174a0` | `S.push(Ljava.lang.String;)` | Reads pool text from the frame's defining class and returns an interned String object. **[oracle]** (`vmexec_string_literal`: real420ff0/4173d0 with4174a0 allocation boundary; `vmexec_string_identity`: actual4174a0 lookup/insert/collision cursor, controlled class/allocation/ptr field, no active GC context). 1,352 occurrences **[corpus]**. |
| 0x1019 (name part) | — (structural) | — | Name leaf consumed inline by `0x1011`/`0x0024`/`0x1021`-family resolution **[static + corpus]** (13,024×). |
| 0x101a (name marker) | — (structural) | — | Structural name leaf (same consumption family as `0x1019`); standalone = silent advance **[static + corpus]** (13,502×). |
| 0x4012 (method name) | `FUN_004208e0` → `FUN_004207c0` | structural | Pool string naming a method; it is not a signature. General name resolution creates the dynamic sentinel `-2`, then4207c0 consumes the argument-count marker and infers the signature: non-null references use runtime classes, null references retain their declared descriptors. **[oracle]** (`vmexec_runtime`, final dispatch recording boundary; `vmexec_overload`, real405920/417c30 ranking). Standalone silently advances **[static]**; 12,540 occurrences **[corpus]**. |
| 0x402e (declarator tag) | — (structural) | — | Initialiser-type child of `0x1002` declare **[static]**. Never emitted **[corpus]**. |
| 0x002c | — (structural) | — | Declaration-with-value marker consumed by `0x1002` **[static + corpus]** (2,257×). |
| 0x1007 sub `0x1b` | — | net zero | **Silent no-op** (`pc += 8` only, stack untouched): missing from the `0x1007` sub-switch. Emitted 4,876×, always right after an inline call name / name leaf (`0x101a`, `0x4012`, `0x1011`) — a compiler barrier/marker whose codegen purpose is open **[static + corpus]**. |
| 0x1007 sub `0x1d`, `0xc`, `0x22` | — | net zero | Silent no-ops (missing from the switch) **[static]**; never emitted **[corpus]**. |
| 0x1008 sub `0x22` | — | net zero | Silent no-op (missing from the `0x1008` switch). Emitted 1,167×, always after constructor calls `0x0022` (34×) / `0x0023` (1,133×): a statement-boundary marker after parameter-only constructor signatures **[static + corpus]**. |
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
| 0x10 | `+` | `0x41b4d0` | 0 | `FUN_0041b4d0(L,R)`: string-concatenation check first (`FUN_00417a70`, converts one side when exactly one side is `Ljava.lang.String;`), then promote; `F`: `L += R`; `I`: `L += R`; `L`-String: concatenate (malloc + copy, interns via `FUN_004174a0`); other classes are fatal. I/F legs: 7 vectors (`re/out/vm/vm_add.jsonl`). M2 follow-up #105 adds `re/oracle/vmstr_vectors.py` (git-ignored `re/out/vmstr/`, 23 rows across 4 families, **23/23 byte-identical**, replayed by `crates/slr-vm/tests/vmstr_oracle.rs`): `vmstr_add` 7 (`+` on I/F and on null-`String` operands), `vmstr_strcheck` 5 (the `FUN_00417a70` gate never mutates — both cells stay byte-identical; the both-String row logs exactly one `UnsupportedClassType`), `vmstr_concat` 2 (both-String refs at payload 0 → `FUN_004174a0(null)` = 0; the port logs instead of concatenating), `vmstr_conv` 9 (see the `0x23–0x2e` row). Still **open (booted state)**: real `L`-String concatenation (malloc + copy + `FUN_004174a0` interning) needs live String objects (`FUN_00408800` field access) and class state; mixed `String`/`(I\|F)` `+` needs `FUN_00411610` for the I/F→String leg and crashes a bare oracle at `0x411682` (unmapped class table). **[oracle for I/F, the gate and the conversion; static for L-String]** |
| 0x11 | `%` | `0x41bbf0` | 0 | `FUN_0041bbf0(L,R)`: int-only (`%` via `idiv`; returns quotient in `eax` too, ignored by `run`); logs `division by zero` then traps (`#DE`) on zero divisor — zero-divisor vectors excluded by construction, behaviour documented **[static]**; 5 vectors **[oracle]**. |
| 0x12 | `/` | `0x41b970` | 0 | `FUN_0041b970(L,R)`: `F`: zero divisor logs then yields `±inf` (no trap — 4 vectors incl. `±1.5/±0.0` **[oracle]**); `I`: truncating `idiv`, logs then traps on zero divisor (excluded) **[static]**; 9 vectors. |
| 0x13 | `*` | `0x41b860` | 0 | `FUN_0041b860(L,R)`: promote; `F`/`I` multiply (wraps for int). 11 vectors. |
| 0x14 | `~` | `0x41c0c0` | 0 | `FUN_0041c0c0(V)`: unary int (`FUN_00417830` is a no-op deref guard); `V = ~V`. 8 vectors. |
| 0x15 | `!` | `0x41bdb0` | 0 | `FUN_0041bdb0(V)`: boolean-convert, int-only; `V = (V == 0)`. 8 vectors. |
| 0x16 | unary `-` | `0x41b1d0` | 0 | `FUN_0041b1d0(V)`: `F`/`I` negate in place. 8 vectors. |
| 0x17 / 0x19 | `++` pre/post | `0x41b2d0` | 0 | `FUN_0041b2d0(V)`: V must be an lvalue (kind 2, else `stack access…` fatal); pushes a copy, then increments in place: pre pushes the copy **before** incrementing (new value), post increments **before** copying (old value). `F` adds `_DAT_005f08f0` (`1.0f`), `I` adds 1. 8 vectors each (operands are plain values; the lvalue gate is **[static]**). |
| 0x18 / 0x1a | `--` pre/post | `0x41b3d0` | 0 | Mirror of `++` with `-1.0f`/`-1`. 8 vectors each. |
| 0x1c | literal | — | push | Read the next node as the literal (`0x100e` resource Member, `0x000c` null, `0x100b` int, `0x100d` `I`, `0x200a` float, `0x4009` String); anything else is `unhandled literal` fatal **[static + corpus]**. |
| 0x1e | `new` | `FUN_0041acb0`-adjacent | push | Allocate the object (`FUN_00410890` class init, `FUN_00404e20` sizing, `FUN_00423c00` + `FUN_00423e60`), store the size word at `+8`, push it. Consumes the following `0x1006` class-name node **[static + corpus]** (1,949×). No vectors (needs class loading; open). |
| 0x1f | `new T[]` | — | push | One-dimensional array only (`multi dimensional array` is fatal; element type from the `[...` signature; sizes popped from the stack). No vectors (needs class loading; open) **[static + corpus]** (116×). |
| 0x20 | `instanceof` | `FUN_00423ea0/00408800` | push | Pops the value (dereferencing), advances `pc` past the check node, pushes int 1/0 (`I` index, `[` array; bad index type, non-array, or negative index are fatal) **[static + corpus]** (80×). No vectors (needs class hierarchy; open). |
| 0x21 | cast | `FUN_00417da0/00417d80` | push | Pops the value; the following node must be `0x1006` (class) or `0x1019` (name); failed casts are `cannot cast` fatal **[static + corpus]** (42×). No vectors (needs class hierarchy; open); `FUN_00417da0`'s non-cast conversion legs do have vectors — see the `0x23–0x2e` row. |
| 0x23–0x2e | assign | `0x41acb0` | 0 (0x1007) / drop (0x1008) | `FUN_0041acb0(dst, src, kind)`: `0x23` plain (`FUN_0041ab60`, with assignment conversion + `L`/`[` refcount/GC hook), `0x24` `*=`, `0x25` `/=`, `0x26` `%=`, `0x27` `+=`, `0x28` `-=`, `0x2c` `&=`, `0x2d` `^=`, `0x2e` `\|=` (compound ops reuse the arithmetic helpers on a temp). `0x1007` pushes the stored value back; `0x1008` releases it. That assignment conversion (`FUN_00417da0`, `values::assign_convert`) is covered since M2 follow-up #105 by the `vmstr_conv` rows of `re/oracle/vmstr_vectors.py` (`re/out/vmstr/vmstr_conv.jsonl`, git-ignored): 9 rows — I→F, F→I, same-type identity (rows 5/7 are harness pointer-identity artifacts, skipped by name: the harness makes a second copy of the same spelling so the exe compares pointers while the port compares interned names) and null `String` → `Object` descriptor — 7 asserted, all byte-identical, replayed by `crates/slr-vm/tests/vmstr_oracle.rs`. Still **open (booted state)**: the `L`-destination class-loader leg (`FUN_00417da0` → `FUN_00411610`/`FUN_004044e0`/`FUN_004174a0`) needs a live class table and loaded `java.lang.String`; under a bare oracle it crashes at `0x411682` (unmapped class table), so the port keeps the `loader_convert` static port for it. |

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

## Native-call ABI (`CallInfo`, `FUN_0045d910`)

Every registered native is an ordinary cdecl function with **exactly one
stack argument**: a pointer to a 4-word `CallInfo` (`0x41fc20-0x4200b7`
builds it; `Object.hashCode` shows the whole callee side in 6
instructions). Layout **[static]**:

```text
info+0x00  jvm                 info+0x08  arg container
info+0x04  thread              info+0x0c  native record
container  { +0 cap, +4 data ptr, +8 count, +0xc self }   (0x41e2d0/e2e0/e2f0)
record     { +0 vtable 0x5f08fc, +4 jvm, +8 impl addr,
             +0xc class cstr, +0x10 name cstr, +0x14 sig record }
sig rec    { +0 owner, +4 cached descriptor text, +8 type container }
```

- **The container is the caller's operand stack**: `0x420165` reads
  `[frame+0x20]+0x28`, the frame's own stack container, and hands the same
  object to the CallInfo. Its `count` **includes the receiver/return slot**:
  `[count-1]` is that slot, and the declared parameters run downward from
  `[count-2]`, so the *first* declared parameter is at the top.
- **`FUN_0041def0`** returns the signature text: `"("`, each parameter's
  type spelling concatenated with `strcat`, `")"`, then the return type
  (`"V"` when the type container has no return entry). It only *builds*
  the text when `sig+4` is null, caches it there, and returns `sig+4`
  otherwise — so the marshaller's input is `sig+4`. Building from the type
  container is the path that faults in a hand-built harness (the type
  container was never populated): a CallInfo probe must write the cached
  descriptor at `sig+4`.
- **`FUN_0045d910(info, &out0, &out1, ...)`** writes `out0 =
  container[count-1].payload`, then walks the descriptor from index 1
  (`mov cl,[edi+1]; inc edi`) until `')'` **[static + oracle]**:
  - per parameter it first does `remaining -= 1`, then checks the *old*
    value was non-zero (the `je 0x45dabf` sits **before** the type
    switch, so an exhausted container reports "not enough parameters" even
    when the next byte is also bogus);
  - `I` / `F` -> write `container[remaining].payload`, consume one slot;
  - `L` -> write the payload, then `edi` scans to `';'` and steps one past
    it. The type char that then follows (`I`/`F`/`L`) needs **no extra
    slot**: the class name is fully consumed by the `;` scan
    (`(Ljava.lang.String;I)V` reads 2 containers slots for 2 parameters,
    oracle);
  - `[` -> skip every further `[`, write the payload, then the element
    type decides: `I`/`F`/`L` each advance one extra container slot (the
    element is a separate cell, so `[[I` consumes 2 slots), anything else
    is the array-specific unknown-type log (`0x610a1c`, trailing space) —
    **after** the payload was already written;
  - any other byte is the plain unknown-type log (`0x6109e8`).
  On either log path the exe concatenates `"!"`, the message, the record's
  class and method names and the signature text into `0x62e018`, flushes
  it (`0x5513b0`) and returns: no exception, the out slots already written
  stay written.
- **Return handing** (`0x42012f-0x42018b`): the caller takes the top cell
  of the signature record's type container, allocates a value cell for it
  and pushes it on the caller frame's stack. The type-byte switch picks the
  register: `F` -> `(float)ST0` bits, `I`/`L`/`[` -> `eax`, `V` -> a
  **`V:0` cell**. The six real41fbc0 dispatcher vectors in
  `vmexec_native` establish this separately from script0x2a, which pushes
  no return cell.
- **Port projection** (`crates/slr-vm/src/host.rs`): [`CallInfo`] carries
  `args` (the container contents; `len` is the exe's `count`), `class`,
  `name` and `sig` (the cached descriptor text, always present),
  [`marshal_args`] is `FUN_0045d910` over `&[Value]` + `&mut [u32]`,
  [`parameter_slots`] is the same walk returning the count
  [`collect_args`] needs to pop, and [`return_type`] reads the descriptor
  tail (the new resolved-method path preserves the full descriptor, including
  `V`). `VmHost::invoke_method_native` replaces
  the former synthetic `push 0` stub: registry `record+8` -> `native_impls`
  entry, and an address with no ported function is `NoBody` rather than a
  fabricated zero.
- Verified oracle rows (Unicorn on `0x45d910` with a hand-built container
  and a cached `sig+4`, log sinks captured; no row compares the exe's own
  interned strings): `()I` count 1, `(I)V`/`(F)V`/`(Ljava/lang/String;)V`/
  `([[I)V`/`([[Ljava/lang/String;)V` count 2, `(II)V` count 3, `(III)V`
  count 4, `(IIII)V` count 5, `(Ljava.lang.String;I)V` count 3,
  `([[I[I)V` count 3 — all reproduce the slot order above; `(II)V` with
  count 2 and `(X)V` / `([X)V` count 2 produce the three log texts with the
  partial out writes pinned. `System.buildNumber` (`0x47c0b0`, `mov eax,
  0x25e; ret`) is the trivial end-to-end native; `Object.hashCode`
  (`0x47c900`) returns the receiver slot word, which is what
  `invoke_native_pushes_the_return_cell` asserts.

## Resource handle natives (M1-11, issue #38; crate `slr-resource`)

The 40 natives of the five handle classes — `Animation` 9, `ParticleSystem` 9,
`PhysicsRef` 6, `RenderRef` 14, `SfxRef` 2 (`re/out/native_api.tsv`) — take a
`u32` handle the game keeps in the object's `handle` int field
(`DAT_0062e008`) and resolve it through `FUN_0042ab50` (after `FUN_0042a690`
resolves the nested `obj` field). `crates/slr-resource/src/handles.rs` models
that as `HandleTable` (`id -> HandleEntry`); ids start at 1 and 0 is the exe's
null handle.

- `HandleEntry::words` — the off-object word vector: for `Animation` the
  envelope's `+0x9c` queue buffer, `+0xa0` count and `+0xa4` capacity (the
  buffer lives inline in `queue`, not in a separate allocation); for
  `ParticleSystem` the `+0x6c` flag word and the `+0xc` frequency.
- `queue` — the `Animation` command list: 8-byte `[tag, payload]` entries
  (0 play, 1 loopPlay, 3 pause, 4 seek, 5 setSpeed, 6 setFade; the payload is
  float bits for seek/speed/fade, else 0). The exe reallocs `(cap+0x20)*8`
  bytes through `FUN_0054f580` when `count == capacity`; `Vec` growth is the
  port's analogue and only the appended words are compared.
- `actions` — the `ParticleSystem` list in link order: `+0x10` type id,
  `+0x14` label (case-insensitive compare `FUN_005d7190`), `+0x54` rate,
  `+0x58` counter, `+0x60` domain pointer.

Ported values: the six `Animation` queue setters (`anim_push`),
`ParticleSystem` `setFreq`, `modePermanent` (arg 1 sets bit `0x20000000`, 0
clears it, any other value leaves the word), `delAction`, `getCounter`,
`setCounter` (plus `build_domain`: `fcomp` stores `(rate, 0)` for `rate >= 0`
or NaN and `(0, rate)` for `rate < 0`, squares at x87 PC=64), `PhysicsRef`
slot addressing `base + slot*0xd4`, and the `RenderRef`/`SfxRef` gate and
constant helpers (`render_type_gate`, `render_detail`, `sfx_target_in_range`,
`sfx_scaled_volume`).

**Not ported: the engine tails.** 27 of the 40 natives end in a render,
physics, particle or audio engine call. Each is declared on the `EngineSink`
trait (21 methods; `Recorder` captures the calls for tests) and left
unimplemented — the real engine work lands with the headless backends (M1-12)
and M3/M5/M6. That covers `Animation` `<init>`/`finalize`/`getPos`,
`ParticleSystem` `init`/`stop`/`setSource`/`setDirectSource`, all six
`PhysicsRef` natives (`create`/`createBox`/`createSphere`, `getPos`/`getOri`
and `setMatrix`), 12 of the 14 `RenderRef` methods (`create`, both `setMatrix`
overloads, `changeResource`, `setColor`, `setLight`, `setFlare`, `setType`,
`lineCreate`, `lineAdd`, `plotRoute`, `getBoneId`) and both `SfxRef`
methods. `RenderRef.getTypeID` has no sink method at all: only its gate is
modelled, not the final `[envelope+0x80]` read.

Oracle: `re/oracle/res_vectors.py` runs the exe's own entry points under
Unicorn with hand-built argument frames (no JVM boot; the allocator/realloc,
the log sinks, the two field reads and the envelope fetches are hooked, the
rest is real exe code, including the `0x45d910` argument popper).
**171 rows in 16 files** under `re/out/res/` (git-ignored): `anim` 57,
`psys` 59, `gate` 55. `verify` re-runs every row on a fresh emulator and
requires identical `result` + `events`: **171/171 identical**. The rows are
replayed in-tree by `crates/slr-resource/tests/res_oracle.rs` (three suites,
170 rows replayed — the one `env_null` `getDetail` row is counted and skipped
by design; absent vectors skip with a printed note) and are wired into
`slr-ledger collect` as a `unicorn` suite, so the 20 annotated rows for the
20 evidenced natives read `passing` in `docs/fidelity.md`.

### Acceptance (issue #38)

Oracle that ran: **Unicorn function oracle** — `re/oracle/res_vectors.py`
executes each native's own exe entry point under Unicorn with hand-built
argument frames and hooked allocator/log/envelope sinks. **No booted engine**
and no JVM: the recorded facts are the exe's own queue words, particle-object
words, gate outcomes and engine-call arguments.

Of the 40 handle natives, **12 are boundary-complete** — handle semantics
(resolve + object mutation) ported and the values oracle-verified:

| Class | Natives | Rows | Replay suite |
|---|---|---|---|
| `Animation` | `play`, `loopPlay`, `pause`, `seek`, `setSpeed`, `setFade` | 57 | `anim_queue_matches_oracle_vectors` |
| `ParticleSystem` | `setFreq`, `modePermanent`, `delAction`, `getCounter`, `setCounter` | 59 | `particle_actions_match_oracle_vectors` |
| `RenderRef` | `getDetail` | 19 | `handle_gates_match_oracle_vectors` |

The other **28 natives have an unported engine tail**:

- **27 declare an `EngineSink` stub** (unimplemented; owned by M1-12 for the
  headless call log and M3/M5/M6 for the real engine): `Animation`
  `<init>`/`finalize`/`getPos`; `ParticleSystem`
  `init`/`stop`/`setSource`/`setDirectSource`; all six `PhysicsRef`; the 12
  `RenderRef` methods listed above; both `SfxRef`.
- **1 has an unmodelled tail**: `RenderRef.getTypeID` — its gate
  (`render_type_gate`) is ported and oracle-verified, but the final
  `[envelope+0x80]` read has no sink method and is not ported.

Eight of the 28 have their boundary ported and oracle-evidenced even though the
tail is stubbed or unmodelled: `SfxRef.nplay` (the `sfx_target_in_range` /
`sfx_scaled_volume` gates), `SfxRef.stop` (the `sfx_stop` sink passthrough),
`PhysicsRef.setMatrix`/`getPos`/`getOri` (shared `phys_slot_addr`),
`ParticleSystem.setSource`/`setDirectSource` (shared `build_domain`) and
`RenderRef.getTypeID` (`render_type_gate`). So **20 of the 40 natives have a
`passing` ledger row** — 16 as the direct target of a recorded vector suite and
the other four through the shared helpers. The remaining **20 natives are pure
stubs** — no ported boundary beyond the `EngineSink` declaration and the shared
`HandleTable` resolve (`FUN_0042ab50`/`FUN_0042a690`, unit tested, no oracle
vectors): `Animation` `<init>`/`finalize`/`getPos`; `ParticleSystem`
`init`/`stop`; `PhysicsRef` `create`/`createBox`/`createSphere`; `RenderRef`
`create`, `getBoneId`, both `setMatrix` overloads, `changeResource`,
`setColor`, `setLight`, `setFlare`, `setType`, `lineCreate`, `lineAdd`,
`plotRoute`. A `passing` ledger row therefore certifies the annotated port
item, not the whole native.

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
| `vmstr_{add,strcheck,concat,conv}` (own dir `re/out/vmstr/`, M2 follow-up #105) | `0x41b4d0`/`0x417a70`/`0x417da0` | 7/5/2/9 | `re/oracle/vmstr_vectors.py`, replayed by `tests/vmstr_oracle.rs` — 23/23. `+` on I/F and null-`String` operands; the gate (never mutates; the both-String row logs one `UnsupportedClassType`); both-String refs at payload 0 (`FUN_004174a0(null)` = 0, the port logs instead of concatenating); I→F, F→I, null `String` → `Object` descriptor (`vmstr_conv` rows 5/7 are pointer-identity artifacts, skipped by name). |

Deliberately **without** vectors (open): the `L`-String concatenation leg
of `+` (needs a booted string pool: `FUN_00408800/004174a0`), `new` /
`new[]` / `instanceof` / casts (need class loading), and full call
protocols (`0x0021–0x0024`, `0x4025/0x4026` arg shuffling needs a loaded
method + live frames). The `+` I/F legs and the `FUN_00417a70` gate do have
vectors since M2 follow-up #105 — `re/oracle/vmstr_vectors.py`, 23 rows
(`re/out/vmstr/`, git-ignored; `tests/vmstr_oracle.rs`).

## Open (explicit)

- Assigning an `I`/`F` value into an `L`-typed variable (class-loader
  string conversion in the exe, raw-word copy in the port): see "Rust port
  status" and the `docs/fidelity.md` divergence table. Ticket #31.
- `frame+0x34` is the defining class in the supplied Base/Leaf frame vectors;
  whole-world class loading and all call-record construction remain separate.
- The `0x1007/0x1b` marker's codegen purpose (behaviour: certain no-op).
- Dynamic method signatures and supplied interface graphs now have bounded
  original vectors; actual array-loader graphs and conflicting class-file
  selection remain open.
- Construction vectors establish21/22/23 receiver routing and separate
  script-frame entry; complete stock constructor/class initialization is open.
- `0x101e` arg-array element order beyond the push loop (elementwise
  `0x23`-assign from the top of a reversed staging area — matches
  left-to-right per the `local_400` index walk, but no oracle coverage).
- String concatenation, `new`/`new[]`, `instanceof`, casts, and the
  `0x0021–0x0024`/`0x4025`/`0x4026` call arg shuffling have no vectors
  (need a booted JVM); their machine-code semantics above are static-only.

## Rust port status (M1-03, issue #30; M1-04, issue #31; crate `slr-vm`)

Oracles that really ran (Unicorn running the exe's own code; no JVM boot):

- `crates/slr-vm/src/ops.rs` ports the 27 operator/assignment helpers plus
  the assignment store `FUN_0041ab60`; `tests/vm_oracle.rs` replays
  `re/out/vm/` bit for bit (281 rows, 32 files).
- `crates/slr-vm/src/interp.rs` ports `Thread::run` `FUN_00420ff0` and the
  frame destructor `FUN_00407d10`/`FUN_00407ba0`; `tests/thread_oracle.rs`
  replays `re/out/thread/thread_run.jsonl` (`re/oracle/thread_vectors.py`:
  the real exe Thread/frame constructors and `Thread::run` on hand-built
  node arrays; 972 programs: 381 hand-written, 400 seeded random, 191 from
  the independent review pass, of which 14 are known-divergence cases that
  are not compared). It compares outcome (stopped / fatal script error /
  `idiv` trap / crash), thread flags, current frame, pc, the live-cell
  counter `DAT_0062f138` (which pins every release), the raw 10-byte x87
  leftover-time return value, and each frame's operand stack, locals and
  saved pc. For the fatal/trap/crash rows the harness stops at the moment
  the exe enters its error path, and the test compares what exists there
  (flags without the RUNNING bit, current frame, pc, live counter, stacks and
  locals); the leftover time and the saved pcs, which only `run`'s exit
  path writes, are not comparable for them. Hooked, non-oracle: the
  error-position lookups `FUN_00406f60/00406140` (return 0), the clock
  `FUN_005516c0` (fed from the row) and the log sinks.

**Which opwords the oracle covers.** `Thread::run` (`FUN_00420ff0`) is
annotated as one function, but only the opwords the loop executes itself are
oracle-verified: `0x0004 0x0005 0x0010 0x0018 0x0028 0x0029 0x002a 0x002b
0x002d 0x1001 0x1003 0x1007 0x1008 0x1014 0x1015 0x1016 0x1017 0x101f
0x1027`, sub-ops `0..=0x1d` and `0x23..=0x2e` of `0x1007`/`0x1008`
(`0x1007/0x1c` for `I`/`F`/null literals; `0x1008/0x20` and `0x1008/0x21` as
statements), the fatal opwords and the silent no-ops. Delegated to the
`Host` trait and **not verified by the original loop-only suite**: `0x001d 0x0020 0x0021-0x0024 0x1002
0x1011 0x101b 0x101e 0x401c 0x4025 0x4026`, `0x1007/0x1c` with resource Member
(`0x100e`) or String (`0x4009`) literals, `0x1007/0x1e 0x1f 0x20 0x21` and
`0x1008/0x1e 0x1f`. The same loop-only scope is in the doc comment of `Thread::run`. Issue109
adds field/lvalue vectors, and110 adds the separately scoped literal,
construction, method-selection, and dispatch groups above.

Facts the loop vectors settled or corrected:

- A jump's `imm` counts from the jump node itself (`pc += imm`, no extra
  step): skipping one node needs `imm = 2`.
- `0x1007` sub-op `0` is the ternary select `[cond][a][b]` (missing from the
  table above): pops three, booleans the condition, keeps `a` on true and
  `b` on false. `0x1008/0` pops and releases three.
- `0x1008` binary sub-ops pop and release two cells, unary ones one.
- `0x0028` is a dereferencing pop-and-push (a variable slot becomes a value
  copy), not a "return value slot" write.
- `++`/`--` (`0x17`/`0x18`) push the value from *before* the change,
  `0x19`/`0x1a` the value *after* it; the lvalue is not pushed.
- Assignments: `0x1007/0x23..0x2e` take `[value][lvalue]` (lvalue on top,
  non-lvalue is a fatal script error) and push the value back; the plain
  `0x002d` takes `[lvalue][value]`.
- Boolean conversion of a NaN float is 0 (`fcomp` C3 "equal"); the M1-02
  port had it as 1 and the `vmval` vectors did not cover it. Fixed in
  `values::boolean_convert`.
- The time slice reads the clock at entry, after step 1 and then every 256
  steps while `budget != 0`, and at exit; the return value is
  `(float)(t0 + budget) - t_end` at the thread's x87 precision.
- Returning (`0x002a`/`0x002b`) releases value cells left on the callee
  stack (lvalue words are not released) and all its locals; returning into
  a null saved pc stops the thread.

Execution evidence and remaining boundaries:

- `vmexec_frames/calls/native` replay real frame construction, reverse typed
  arguments, defining-owner context, runtime receiver context, return unwinding,
  and native explicit-`V` versus parameter-only returns. Native implementations
  are recording boundaries. The public Rust path composes these with loaded
  stock String/native roundtrips; this is not a whole-world boot proof.
- `vmexec_resource*`, `string_literal/identity`, `overload/runtime`,
  `constructors/named_methods/new`, `arrays/arg_arrays`, and `casts` cover their
  explicitly recorded original helper paths and controlled boundaries. The
  generator records each boundary; its vectors do not certify full helpers.
  Type0 pool literals retain invalid UTF-8 bytes explicitly as `RawStr`; metadata
  names/descriptors requiring UTF-8 reject that variant. `vmexec_raw_literals`
  compares `ff`, `ff80`, and the C-string prefix of `ff0041` through parser,
  loader, and execution; `vmexec_wrong_kind_literal` records a Class entry
  producing a typed null String. Literal byte lookup never follows Class
  metadata indirection. Absent nonnegative pool indices fail explicitly;
  negative indices take the original null-literal branch.
- Field vectors and unchanged stock Vector3 execution cover persistent typed
  cells and lvalue behavior (#109). Construction uses that same storage.
- Bounded initializer execution uses the same authoritative cells: each typed
  zero is appended immediately before its body. Already-loaded base statics,
  declared interfaces, and own statics run before base-first instance fields.
  An installed static owner suppresses reentry while later fields remain absent.
  Interface-kind diagnostics continue; unresolved metadata fails before recursive
  dependency initialization. Own-class static descriptors skip their bodies at
  the current canonical, single-version class boundary; instance and ancestor
  descriptors do not. Versioned descriptor identity remains open.
  `re/oracle/vminit_vectors.py` executes the original initialization drivers with
  explicit metadata, lookup, allocation, logging, and fatal-stop boundaries.
  RID/String/cast/new and static-script-return fields are compared at public
  parsed-TUFA/VM seams. Numeric/reference assignment diagnostics retain the
  original raw-copy continuation, including overwriting prior body writes.
  A lookup of a not-yet-installed field reaches an original invalid-memory
  stop (observed at `41f835`); Rust returns a managed missing-member/VM error.
  Those comparisons assert partial cells and error presence, not physical
  fault or exception-type parity. This differs from explicit fatal-opcode
  controls stopped at `4190d0`.
  Fatal controls stop at `4190d0`: partial cells/writes remain observable, but no
  original unwind, catch, rollback, continuation, or permanent poison policy is
  claimed. The instance recursive-self-new observation retains its supplied
  original allocator and is not counted as a matching Rust allocation replay.
  Rust nested initializer bodies have an explicit configurable depth limit32;
  malformed graph cycles fail explicitly. Neither limit is original-behavior
  evidence. ArgArray, constructor/direct-call forms and array/type-test
  initializer families remain explicitly unsupported in this slice. Their
  guard preflights the entire current body: prior fields and its current zero
  remain, but earlier instructions of a rejected body are not executed. The
  nesting guard counts field bodies, not arbitrary dependency-graph depth.
  General
  scheduler/yield completion, versioned overlapping class identities, complete
  Type5 name/cache resolution, call field chains, machine `CallInfo` context,
  actual array-class graphs, active String GC, and boot acceptance remain open.
- Cell values carry full type names since M1-04 (`TypeTag::named`);
  the helpers still dispatch on the first byte exactly like the exe, while
  the string check and conversions read the whole name.
- Compound assignments `+=` / `-=` (kinds `0x27`/`0x28`, which the exe runs
  through the string-aware `FUN_0041b4d0`) and `+`/`-` offer reference
  operands to `Host::operator_on_refs` (`RefOperator::Add` /
  `AddAssign`; the loop negates the right operand first for the `-` forms).
  No other assignment kind reaches that helper. Unit-tested with a fake
  host, no oracle (needs the string pool).
- **Former divergence, closed by M1-04:** assigning an `I` or `F` value into
  an `L`-typed variable (kinds `0x23 0x26 0x27 0x28 0x2c 0x2d 0x2e` with an
  `L` destination). The exe converts through the class loader
  (`FUN_00417da0` -> `FUN_00411610` / `FUN_004044e0` / `FUN_004174a0`,
  an int-to-string conversion; the older loader-less oracle fixture crashes,
  14 recorded rows; `vmexec_casts` now runs recovered numeric/String conversions). The port now converts through `loader_convert` (the
  `FUN_00417da0` static port: `%d` / `%.3f` + interning) when the
  interpreter's loader is present (`Alu::store_with`); loader-less
  `Alu::store` keeps logging `AssignConversion` and copying the raw word,
  pinned by
  `loaderless_int_or_float_into_a_reference_variable_copies_the_raw_word`.
  The `known_divergence` rows of `thread_run.jsonl` still pin the old
  oracle-vs-port gap for the loader-less replay path.
- M1-04 (`strings.rs`, `vmclass.rs`, `natives.rs`, `host.rs`): the cell
  model carries full type names (`TypeTag::Ref`/`Array` hold the interned
  descriptor; first-byte dispatch unchanged, all oracle suites green); the
  string interner (`FUN_0041e4d0`/`0041e610`/`0041ef10`/`0041e830`), the
  native registry (`FUN_00416b00`, hash `sum(name)*0x49+sum(class)`,
  bucket `&0x7f`), boot (`FUN_0040fe70`: 8 packages, 5 descriptors,
  `java` root), loading/linking (`FUN_00411b20`/`00410890`/`00410630`/
  `00410430`, class initialization `FUN_00403e80`, dispatch `FUN_00404910`/
  `00404d40`/`00405920`, assignability `FUN_00405400`) and a `VmHost`
  implementing all 18 `ExternKind`s with the string-aware `+`
  (`FUN_00417a70`). Vectors (`re/oracle/vmclass_vectors.py`, git-ignored):
  11 interner + 3 registrar + 5 bind rows, all byte-identical
  (`tests/vmclass_oracle.rs`). Corpus: 1,452 stock files, 1,451 link
  (the exclusion is `Dialog.class`: `0x108f5` + pool type 10, unreadable
  to the stock exe itself); registry loads the 353-row table
  (`tests/corpus.rs`). The 7 ownerless `java.io.Controller` natives stay
  excluded and reported (`Natives::CONTROLLER_EXCLUDED`).
- The exe's "log and continue" paths are recorded as `ops::Diagnostic`
  entries (message kind + helper address), read off every helper's
  pseudocode (`tests/ops_diag.rs`); the Unicorn log sinks are stubbed, so
  this is static evidence, not oracle.
- The variable-slot arena recycles freed slots (free list) with a
  generation counter, so a call loop does not grow it.
- `call_script_method` raises a fatal script error for an instance method
  called without a receiver, like the exe's first check.
- M1-08 S0 (`host.rs`): the native-call ABI — `CallInfo`, the marshaller
  `FUN_0045d910` (`marshal_args`), the descriptor walk
  (`parameter_slots`), the return-cell rule (`return_type`) and
  `VmHost::invoke_native` over a new `native_impls: HashMap<u32, NativeFn>`
  keyed by the registry's `record+8` address. Covered by data-free unit
  tests only (the oracle rows above are recorded as expectations, not as a
  replayable vector file).
- Script errors stop the thread with `VmError`; the exe logs and continues
  with garbage, and integer `#DE` traps are not turned into the original's
  SEH behaviour.

## Native acceptance: `slr-lang` Math/Float/Integer/String (M1-06, issue #33)

Oracle that ran: **Unicorn function oracle** — `re/oracle/langmath_vectors.py`
executes each native's own exe entry point under Unicorn with the game's D3D9
control word (`0x007f`, PC=24) on hand-built `CallInfo` frames. No booted
engine, no JVM. **148 rows in 7 families** land in
`re/out/langmath/langmath.jsonl` (git-ignored) and replay in-tree through
`crates/slr-lang/tests/langmath_oracle.rs` (`math_natives_match_oracle_vectors`),
wired into `slr-ledger collect` as a `unicorn` suite. The six
`java.lang.String` natives replay through `langobj_oracle`
(`object_natives_match_oracle_vectors`), whose generator is
`re/oracle/langobj_vectors.py`.

| Native | Rows | Row family | Observable compared |
|---|---|---|---|
| `Math.setrandseed` `(I)V` | 7 | `setrandseed` | the CRT `rand` word at `ptd+0x14` |
| `Math.randomize` `()V` | 6 | `randomize` | the seeded word, with `clock()` hooked to the row |
| `Math.random` `()F` | 10 | `random` | 8–16 draws as `f32` bits, then the word |
| `Math.sqrt` `(F)F` | 40 | `sqrt` | the `f32` store image of ST0 |
| `Integer.toString` `(I)LString;` | 17 | `int_to_string` | the bytes handed the interner |
| `Integer.toHexString` `(I)LString;` | 16 | `int_to_hex` | the bytes handed the interner |
| `Float.toString` `(FLString;)LString;` | 52 | `float_to_string` | the bytes handed the interner |
| `String.intValue/floatValue/length/append/finalize/token` | 22 | `string` | scanned word / `f32` bits / length / interned bytes / release / token |

Two harness facts the vectors pinned, recorded because both first looked like
port bugs:

- **ST0 narrowing.** `CallResult.st0` goes through `_ext_to_float`, which
  renders every NaN as positive `nan`; the sign of x87's indefinite is lost
  there. The native returns `F`, so the observable is the 32-bit store of
  ST0 — the generator decodes the raw 80-bit register (`st0_f32`) and a
  negative `sqrt` operand reads `ffc00000`, which is what the port and the
  M0-07 probe both say. Rows also carry `st0_ext` for the full register.
- **`Float.toString` has no integer argument cell.** It marshals the format
  word and one promoted double; a `%d` conversion therefore reads
  uninitialized caller-frame bytes (`0x60000000` / `0xa0000000` observed).
  Those rows were dropped as undefined-frame reads and replaced with
  `%8.2f` / `%08.3f` width rows, which exercise the same formatter legally.
- **The marshalled `String` word is the char buffer.** `Float.toString`
  pushes slot 0 straight into `sprintf` (`481dff push edx`) with no
  `FUN_0042ab50` resolve, the same shape `String.token` shows for its
  charset. An earlier harness planted an object word there and every row
  rendered `""`.
- **The interner is `thiscall`.** `481e6b mov ecx,[esi] / call 0x4174a0`:
  the JVM word arrives in `ecx` and the char buffer as the stack argument.

Ledger: all 13 registered natives read `passing` with the `unicorn` oracle.
`slr-ledger collect` now **fails** when a registered symbol is one no
`/// exe:` annotation emits, so a spelling drift cannot silently drop the
evidence again (it found two in this pass: `FUN_0041a370` and
`FUN_00423e10`).

## Native acceptance: `slr-lang` Vector3/Ypr (M1-07, issue #34)

Oracle that ran: **Unicorn function oracle** — `re/oracle/langvec_vectors.py`
drives each native's `record+8` entry on hand-built object/argument containers
at the game's control word `0x007f` (PC=24). **707 rows in 13 families** land
in `re/out/langvec/langvec.jsonl` (git-ignored) and replay in-tree through
`crates/slr-lang/tests/vector_oracle.rs` (`vector_natives_match_oracle_vectors`),
registered in `slr-ledger collect`.

| Family | Rows | Compared |
|---|---|---|
| `length` | 50 | `st0` **bit for bit** |
| `distance` | 80 | `st0` **bit for bit** |
| `normalize` | 50 | receiver `x/y/z` **bit for bit** |
| `vec_init` | 48 | receiver fields (tolerant: goes through `fsin`/`fcos`) |
| `rotate_ypr` | 80 | receiver fields (tolerant) |
| `rotate_axis` | 55 | receiver fields (tolerant) |
| `ypr_init` | 50 | receiver fields (tolerant) |
| `ypr_mul` | 72 | receiver fields (tolerant) |
| `vec_to_ypr` | 50 | triple (tolerant) |
| `mat3_from_ypr` | 38 | matrix (tolerant) |
| `mat3_from_axis` | 34 | matrix (tolerant) |
| `mat3_to_ypr` | 46 | matrix (tolerant) |
| `mat3_mul` | 54 | matrix **bit for bit** |

**"0 ulp on trusted ops" is exactly the split above.** Every family whose
arithmetic is only multiply/add (`length`, `distance`, `normalize`, `mat3_mul`)
is compared bit for bit and passes that way. The other eight reach
`fsin`/`fcos`/`fpatan`, which `slr-x87::trans` implements as **APPROX**
(`docs/x87.md`), so they are compared at relative tolerance `1e-5` — the
measured divergence is a few `f32` ulps and all 707 rows pass with a 10x
margin. The tolerant families are **untrusted** operations in the ADR-0009
sense and are reported as such here rather than folded into a tier-C claim.

The natives are driven through `receiver_and_arg`, so each row also replays
that helper's `out[0]`/`out[1]` container shape, and the eight registry rows
(`FUN_00482020`, `FUN_00482110`, `FUN_00482280`, `FUN_004823e0`,
`FUN_00482450`, `FUN_00482550`, `FUN_00482610`, `FUN_004826c0`) plus the six
matrix helpers (`FUN_0054eab0`, `FUN_0054ecd0`, `FUN_0054ee80`, `FUN_00551c90`,
`FUN_00551d20`, and the `mat3_mul` body) read `passing` in the ledger.

## Native acceptance: Threads, timers and callbacks (M1-05, issue #32)

Two oracles ran, both Unicorn on the exe's own entry points at control word
`0x007f` (PC=24):

- **`re/oracle/thread_vectors.py`** drives `Thread::run` (`FUN_00420ff0`) on
  hand-built programs, using the exe's own thread ctor (`FUN_0041f340`), frame
  pool (`FUN_00408d90`), frame ctor (`FUN_00407b10`) and node arrays.
  **958 rows** replay in-tree through `crates/slr-vm/tests/thread_oracle.rs`;
  outcomes compared: `stopped` 896, `trap` 37, `fatal` 20, `crash` 5. The 14
  `known_divergence` rows (`x_assign_0x2[3-8]_l0_[IF]`) pin the loader-less
  replay gap recorded in the divergence table; the live interpreter converts
  through `loader_convert`, so they are excluded from the equality count
  rather than silently passing.
- **`re/oracle/sched_vectors.py`** drives the scheduler driver
  (`FUN_00416940`), the fairness counter (`FUN_00418c20`) and the frame-time
  accumulator block (`FUN_00418d10`). **27 rows / 25 thread runs** replay
  **bit for bit** through `crates/slr-vm/tests/sched_oracle.rs`, including the
  finalizer queues' `push_queued` (`FUN_00418290`), `push_spare`
  (`FUN_00408720`) and `push_held` (`FUN_00408780`).

Timers and callbacks: `GameType.addTimer` and the callback registration rows
are part of issue #35's GameType family; the scheduler they run on is the one
above, and its ordering (queue walk, min-priority skip, divisor gate, slice
rounding) is pinned by the 27 vectors rather than by inspection.

## File path rows (issue #36, I8)

The last four `java.io.File` registry rows are eight-instruction bodies that
take a **raw path pointer** — not a `CallInfo` — marshal it, and return their
wrapper's `eax`:

| row | native | wrapper |
|---|---|---|
| `exists(Ljava.lang.String;)I` | `0x486170` | `0x54c170` → `0x5542d0` (`_access`) or `0x5586b0` (`CreateFileA` + `CloseHandle`) |
| `delete(Ljava.lang.String;)I` | `0x486150` | `0x54c7c0` → close a matching table entry, then `0x558cf0` (`DeleteFileA`, constant 1) |
| `copy(Ljava.lang.String;Ljava.lang.String;)I` | `0x4860f0` | `0x54c550` → `CopyFileA`, answer `0x558a50` = `(src != dst)` |
| `move(Ljava.lang.String;Ljava.lang.String;)I` | `0x486120` | `0x54c650` → `MoveFileA`, same name-compare answer |

`re/oracle/io_vectors.py` gained a `path` group (**7 rows**) that calls each
*wrapper* with cdecl path arguments — exactly the layer the port replaces —
with the CRT primitives (`0x554860`, `0x5542d0`, `0x5586b0`, `0x558cf0`,
`0x558a50`, the table lock and its per-entry compare) hooked.
`crates/slr-io/tests/io_oracle.rs::the_path_rows_replay` replays them, and the
four ledger rows now read `passing` with `unicorn` evidence.

Two corrections the vectors forced:

- **`exists` asks the filesystem, not the path policy.** The first port
  answered `u32::from(resolve(path).is_some())`, so any path the layer would
  accept counted as present; `exists_absent` caught it. The wrapper asks the
  real CRT probe.
- **The probes see the converted name.** Each wrapper rewrites `'/'` to
  `'\\'` before calling out, so an expectation seeded with forward slashes
  never matched; the row would have "passed" against a broken port.

`move` is inspection-verified only. Its wrapper (`0x54c650`) runs two complete
CRT table scans (`0x54c6db`-`0x54c715`, `0x54c74d`-`0x54c778`) and the
emulated table has no live entries, so the second scan reads past its last
entry into unmapped memory — the oracle cannot reach a defined answer. The
port reproduces the same two-scan shape over its own table; the divergence is
in the probe, not the port, and it is recorded here rather than papered over
with a vector.

## System state rows (issue #35, non-timer half)

Five of the ten `java.lang.System` registry rows outside the timer family have
bodies that read or write nothing but globals, so they port exactly:

| row | exe | body |
|---|---|---|
| `timeWarp(F)F` | `FUN_0047bf80` | loads `[0x636448]` *before* the branch, stores the argument through `FUN_004283f0` on the taken arm, returns the pre-call word |
| `setLdPriority(I)V` | `LAB_0047bf30` | zero → `0.1f`/`0` at `[0x60c8c4]`/`[0x640920]`, else `-1.0f`/`1`; then `ret` |
| `syncGameTime(F)V` | `LAB_0047bff0` | `__ftol`, `idiv` by 86400, store `edx` at `[0x640988]`, then a five-band ladder writes `[0x6178d0]` |
| `setMeasure(F)V` | `FUN_0047c4b0` | store the argument at `[0x60e04c]`, `value*[0x5f0ee8]` at `[0x63c770]`, `[0x5e7334]/value` at `[0x63c73c]` |
| `isLoading()I` | `LAB_0047c3b0` | returns `[[0x618d48]+0x106ed4]` |

`re/oracle/langobj_vectors.py` suite `system` drives all five (**21 rows**),
each recording the globals before and after as raw bit patterns, and
`crates/slr-lang/tests/langobj_oracle.rs` replays them; the ledger rows read
`passing` with `unicorn` evidence.

Three exe facts the oracle corrected after the first port attempt, each a case
where inspection had been wrong:

- **`timeWarp` returns the pre-call word.** `mov edx,[0x636448]` sits before
  the `fnstsw`/`test`, so the native returns the old scale even on the arm
  that stores a new one — and the store is not a bare `mov`: it goes through
  `FUN_004283f0(0x636338, argument)`, which also derives `[0x636440]` as
  `clock*0.001 - [0x617650]/scale`. `timeWarp_below`/`_nan` return the old
  word and write nothing; `_at_gate`/`_above` store and derive.
- **`setMeasure` stores a zero argument as zero.** The `1000.0f` at
  `0x47c4bd` writes `[esp+0xc]`, not the marshaller's slot (`lea eax,[esp]`),
  so it is only visible when the container never reaches that word. The
  `setMeasure_zero` row stores `0.0` and the `fdiv` yields `+inf`
  (`0x7f800000`) — the row that pins it.
- **`syncGameTime`'s ladder has five bands, not two.** `>= 72000 → 1`,
  `< 3600 → 1`, `< 18000 → 0`, `< 25200 → 1`, else `0`; the eleven time rows
  bracket every boundary (3599/3600, 17999/18000, 25199/25200, 71999/72000).

The other five registry rows of the same family — `isLoadingReset`,
`getConfigOptions`, `arraycopy`, `openLib`, `compileAll` — are **deliberately
unported**. Each drives an engine subsystem this crate has no stand-in for
(the loader object, the config tree, object-context array copies, the package
loader, the class compiler), so a port would be invention. They keep their
constant and stay out of `NativeTable`, so dispatch reports `NotPorted`.

**Three scheduler annotations were prose, not symbols.** `exe: the append
FUN_00408500 makes at 0x408553` and five like it never scanned, so the
scheduler had no ledger rows at all; they now name the containing analyzed
function (`FUN_00418290`, `FUN_00416940`, `FUN_00418d10`, `FUN_00408720`,
`FUN_00408780`) resolved from `re/out/index.txt`, with the addresses kept in
prose. `sched_oracle` was also absent from `slr-ledger collect`, so its
passing test evidenced nothing; it is registered now.
