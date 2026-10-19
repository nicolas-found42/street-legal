#!/usr/bin/env python3
"""Unicorn vectors for the `java.lang.Object` natives (M1-08 rows O1..O4).

Calls the real exe code (build 606) with a hand-built `CallInfo`, exactly the
shape `FUN_0041fbc0` builds and `FUN_0045d910` consumes:

    info+0  jvm   info+4  thread
    info+8  arg container {cap,data,count,self}
    info+0xc native record {vtable, jvm, impl, class, name, sig}
    sig+0   owner  sig+4 cached descriptor text  sig+8 type container

Hooked callees (test sinks only; the natives' own code is the real thing):

- `0x4174a0` (`FUN_004174a0`, thiscall intern): records the buffer text and
  returns a sentinel word, so `toString`'s `sprintf` output is observed
  without a booted JVM (which `FUN_004174a0` needs for its class table).
- `0x551140` (log append) / `0x5513b0` (flush): diagnostic sinks.
  `0x551220` (the `sprintf` `toString` calls) is NOT hooked: the real CRT
  conversion is what proves `%0x` is lowercase hex with no width.

Suites:

- `hash` — `0x47c900`: `eax` is the container's receiver/return word.
- `tostr` — `0x47c870`: the text handed to the interner for several class
  names (proves `"%s<%0x>"`: lowercase, no width, no padding).
- `gc` — `0x47c8c0`/`0x47c8e0`: after every call the object's link words
  (`+4`/`+8`), its flag word (`+0x18`), the two class-data head cells and the
  sentinel nodes' links, over a six-step move sequence on two objects.
- `exit` — `0x47be50`/`0x47be60` (M1-08 row O5): the exit request word
  `[0x63c520]` before and after each call (plus the diagnostic-log sink,
  which the two well-formed shapes never touch). Proves `exit()V` sets the
  word, and that `exit(Ljava.lang.String;)V` marshals and changes nothing
  (the handoff's "exit code ignored" quirk).
- `analyze` — `0x47bed0`/`0x47bf10`/`0x47bef0` (M1-08 row O8): the word left
  in the native's own caller-frame argument slot (ESP at return is that
  slot), `eax`, whether `st0` stayed empty, the instruction count (the
  `Object` overload's one-`nop` `if` costs one instruction when the word is
  non-zero) and the diagnostic-log sink. Proves all three marshals land in
  the slot — the `F` payload as a raw bit pattern, with no FPU use — and that
  a malformed container leaves the `info` pointer there.
- `info` — `0x47c0c0` (M1-08 row O9): the text the native hands to the
  interner over a hand-built JVM object, plus the instruction count. Pins the
  six-id mapping (three `+0x04` chain counts at `[[jvm+0x18/0x1c/0x10]+8]`,
  then `[jvm+0x24]`, `[jvm+0x28]`, `[0x62f138]`), the signed `%d`, the
  `[count-2]` marshalled parameter, and the id>5 path's unformatted scratch
  residue (`""` on a fresh stack, the primed call's text otherwise).
- `sysgc` — `0x47c060`/`0x47c070` (M1-08 row O10): per scenario, the JVM
  word `[jvm+0x20]` and the mark-size word `[jvm+0x28]` before/after every
  call, `eax`, the instruction count, the FPU state, every write that lands
  inside the JVM object (as an offset, so the rows are arena-independent) and
  whether the collector's own JVM global names the fixture. Pins `gc`'s two
  passes (`-1` from the pass that allocates the mark context, the collected
  count — 0 with nothing to free — from the pass that drains and frees it,
  which is also the pass that stores `cap << 2` at `[jvm+0x28]`) and
  `runFinalization`'s `xor eax,eax; ret`. The `gc-cycle-info4` row reads
  `System.info(4)` on the same JVM before and after the cycle, which is what
  makes the `[jvm+0x28]` write visible through an already-ported native
  (`"0"` → `"256"`).

Rows land in `re/out/langobj/objects.jsonl` (git-ignored, never committed).
Object words are arena-dependent, so `verify` compares the *text rule*
(`"<class><<word in lowercase hex>>"` over the freshly allocated word) and the
GC rows' link **relationships** (which fixture word each link names), not raw
addresses.

Usage (repo root, venv at ~/slr-oracle-venv):

    ~/slr-oracle-venv/bin/python re/oracle/langobj_vectors.py gen \\
        --exe reference/raw/StreetLegal_Redline.bak.exe
    ~/slr-oracle-venv/bin/python re/oracle/langobj_vectors.py verify

Never writes to `reference/` or `stock/`; the exe is only read.
"""
from __future__ import annotations

import argparse
import datetime
import json
import struct
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import CW_D3D9, Oracle  # noqa: E402

MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
OUT_DIR = os.path.join("re", "out", "langobj")
OUT_FILE = "objects.jsonl"

TO_STRING = 0x47C870
DISABLE_GC = 0x47C8C0
ENABLE_GC = 0x47C8E0
HASH_CODE = 0x47C900
EXIT = 0x47BE50
EXIT_STRING = 0x47BE60
ANALYZE_OBJECT = 0x47BED0
ANALYZE_FLOAT = 0x47BEF0
ANALYZE_INT = 0x47BF10
EXIT_FLAG_ADDR = 0x63C520
STRING_INT_VALUE = 0x481ED0
STRING_FLOAT_VALUE = 0x481F10
STRING_LENGTH = 0x481FE0
STRING_APPEND = 0x481F50
STRING_FINALIZE = 0x486190
STRING_TOKEN = 0x481CF0
STRING_FIELD_IDX = 0
INTERN = 0x4174A0
LOG = 0x551140
FLUSH = 0x5513B0

VAL_VT = 0x5E734C
TYPE_CELLS = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C, "V": 0x60AB60}
INTERN_SENTINEL = 0x77777777
FIXTURE_KEYS = ("ctx", "normal", "disabled", "hn", "hd", "o1", "o2")


def default_exe() -> str:
    for root in (
        os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")),
        MAIN_CHECKOUT,
        os.environ.get("SLR_REFERENCE", ""),
    ):
        path = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if path and os.path.exists(path):
            return path
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


class Probe:
    """One emulator with the CallInfo helpers and the log/intern sinks hooked."""

    def __init__(self, exe: str):
        self.o = Oracle(exe)
        self.o.hook_alloc()
        self.logs: list[str] = []
        self.interned: list[bytes] = []

        def log(oc, args):
            addr = args[1]
            text = "<null>" if addr == 0 else oc.read_cstr(addr, 0x200)
            self.logs.append(text.decode("latin1"))

        def intern(oc, args):
            # A null char pointer (`FUN_004174a0`'s `test ecx, ecx` guard)
            # returns 0 and interns nothing; `token` reaches it after a failed
            # `strtok`.
            if args[0] == 0:
                self.interned.append(b"")
                return 0
            self.interned.append(oc.read_cstr(args[0], 0x200))
            return INTERN_SENTINEL

        self.o.hook(LOG, log, "cdecl", 3)
        self.o.hook(FLUSH, lambda oc, args: None, "cdecl", 0)
        self.o.hook(INTERN, intern, "thiscall", 1)
        # `FUN_0042ab50(obj, [0x62e008])` for a `java.lang.String`: the char
        # buffer. The row plants the bytes and points this at them, which is
        # exactly what the exe's own field read returns for a String.
        self.buffers: dict[int, int] = {}

        def string_field(oc, args):
            # The receiver and each String parameter are separate objects; a
            # row plants their buffers by object word.
            return self.buffers.get(args[0], 0)

        self.o.hook(0x42AB50, string_field, "cdecl", 2)
        # The interner's release pair `finalize` calls: recorded, not run (they
        # walk the JVM's string table, which a headless probe has no reason to
        # build). `thiscall` = the interner in `ecx`.
        self.releases: list[tuple[int, int]] = []

        def release0(oc, args):
            self.releases.append((args[0], 0))
            return 0

        def release1(oc, args):
            self.releases.append((args[0], 1))
            return 0

        self.o.hook(0x41ECD0, release0, "thiscall", 2)
        self.o.hook(0x41E830, release1, "thiscall", 1)
        # The CRT string helpers the String natives call, left real:
        # 0x551240 (sscanf) and 0x551220 (sprintf) are the exe's own code.
        for _tag, cell in TYPE_CELLS.items():
            d = self.o.alloc(4)
            self.o.write_u32(d, cell)
            self.o.write_u32(cell, d)

    # -- object-model fixtures ------------------------------------------
    def zeroed(self, cls: int = 0, next_: int = 0, prev: int = 0, flags: int = 0) -> int:
        p = self.o.alloc(0x20)
        w = [0] * 8
        w[1], w[2], w[3], w[6] = next_, prev, cls, flags
        self.o.write_u32s(p, w)
        return p

    def object_of(self, name: bytes) -> int:
        """A live object: `+0xc` points at a class descriptor whose `+8`
        points at the name cstring (`FUN_00404ea0`: `[[ecx+8]]`)."""
        nm = self.o.alloc(64)
        self.o.write(nm, name + b"\0")
        desc = self.o.alloc(8)
        self.o.write_u32(desc, nm)
        ctx = self.o.alloc(0x20)
        self.o.write_u32(ctx + 8, desc)
        return self.zeroed(cls=ctx)

    def plant(self, text: bytes, obj: int) -> int:
        """Plant `text` as the `java.lang.String` bytes of object `obj`.

        Returns the cstr pointer. The field hook answers by object word, so a
        row can plant the receiver and a charset parameter separately.
        """
        buf = self.o.alloc(max(len(text) + 1, 4))
        self.o.write(buf, text + b"\0")
        self.buffers[obj] = buf
        return buf

    def cell(self, word: int) -> int:
        """One value cell (`+0` vtable, `+4` type, `+8` payload word)."""
        cell = self.o.alloc(12)
        self.o.write_u32s(cell, [VAL_VT, TYPE_CELLS["I"], word & 0xFFFFFFFF])
        return cell

    def call_info(self, word: int, sig_text: bytes, params=(), record_names=(b"", b"")) -> int:
        """A `CallInfo` for `(word, sig_text)`.

        `params` are the declared parameters in declaration order; the
        container holds them *before* the receiver/return slot, which is where
        `FUN_0045d910` reads (`[count-1]` is the receiver, then downward).
        `record_names` fills `record+0xc`/`+0x10` with class/name cstrings, so
        a diagnostic-log row has real text to concatenate.
        """
        cls = self.o.alloc(64)
        self.o.write(cls, record_names[0] + b"\0")
        name = self.o.alloc(64)
        self.o.write(name, record_names[1] + b"\0")
        sig = self.o.alloc(16)
        text = self.o.alloc(max(64, len(sig_text) + 1))
        self.o.write(text, sig_text + b"\0")
        self.o.write_u32s(sig, [0, text, 0, 0])
        rec = self.o.alloc(0x20)
        self.o.write_u32s(rec, [0x5F08FC, 0, 0, cls, name, sig])
        cells = [self.cell(w) for w in params] + [self.cell(word)]
        data = self.o.alloc(4 * len(cells))
        for i, c in enumerate(cells):
            self.o.write_u32(data + 4 * i, c)
        cont = self.o.alloc(16)
        self.o.write_u32s(cont, [len(cells), data, len(cells), 0])
        info = self.o.alloc(32)
        self.o.write_u32s(info, [0, 0, cont, rec])
        return info

    def call(self, addr: int, info: int):
        return self.o.call(addr, args=[info], convention="cdecl", fpu_cw=CW_D3D9)


def gc_fixture(exe: str):
    """The two-head class data plus two objects, as the exe reads it:
    `obj+0xc = ctx`, `ctx+0xc = class data`, `classdata+0x10 = normal list
    object` (head node `+8`), `classdata+0x14 = disabled list object`
    (head node `+0x18`)."""
    p = Probe(exe)
    ctx = p.zeroed()
    data = p.zeroed()
    normal = p.zeroed()
    disabled = p.zeroed()
    hn = p.zeroed()
    hd = p.zeroed()
    p.o.write_u32(ctx + 0xC, data)
    p.o.write_u32(data + 0x10, normal)
    p.o.write_u32(data + 0x14, disabled)
    p.o.write_u32(normal + 8, hn)
    p.o.write_u32(disabled + 0x18, hd)
    fx = {
        "ctx": ctx,
        "normal": normal,
        "disabled": disabled,
        "hn": hn,
        "hd": hd,
        "o1": p.zeroed(cls=ctx),
        "o2": p.zeroed(cls=ctx),
    }
    return p, fx


def rel(value: int, fx: dict) -> str:
    """Name a fixture word, or the raw hex when it is outside the fixture."""
    for key in FIXTURE_KEYS:
        if fx[key] == value:
            return key
    return f"0x{value:x}"


def snapshot(p: Probe, fx: dict) -> dict:
    def w(a: int) -> int:
        return p.o.read_u32(a)

    def node(word: int) -> dict:
        return {"next": rel(w(word + 4), fx), "prev": rel(w(word + 8), fx)}

    return {
        "normal_head_cell": rel(w(fx["normal"] + 8), fx),
        "disabled_head_cell": rel(w(fx["disabled"] + 0x18), fx),
        "normal_sentinel": node(fx["hn"]),
        "disabled_sentinel": node(fx["hd"]),
        "o1": {**node(fx["o1"]), "flags": w(fx["o1"] + 0x18)},
        "o2": {**node(fx["o2"]), "flags": w(fx["o2"] + 0x18)},
    }


def run_hash(exe: str, words):
    rows = []
    for word in words:
        p = Probe(exe)
        eax = p.call(HASH_CODE, p.call_info(word, b"()I")).eax
        rows.append(
            {
                "suite": "hash",
                "id": f"hash-{word:08x}",
                "exe_addr": HASH_CODE,
                "name": "hashCode",
                "receiver_word": word,
                "eax": eax,
            }
        )
    return rows


def run_tostr(exe: str, names):
    rows = []
    for name in names:
        p = Probe(exe)
        obj = p.object_of(name)
        p.interned.clear()
        r = p.call(TO_STRING, p.call_info(obj, b"()Ljava.lang.String;"))
        rows.append(
            {
                "suite": "tostr",
                "id": f"tostr-{name.decode('latin1')}",
                "exe_addr": TO_STRING,
                "name": "toString",
                "class": name.decode("latin1"),
                "object_word": obj,
                "text": p.interned[0].decode("latin1") if p.interned else None,
                "eax": r.eax,
                "intern_sentinel": INTERN_SENTINEL,
                "logs": p.logs,
            }
        )
    return rows


def run_gc(exe: str):
    steps = [
        ("disable-o1", DISABLE_GC, "o1"),
        ("disable-o2", DISABLE_GC, "o2"),
        ("enable-o2", ENABLE_GC, "o2"),
        ("enable-o1", ENABLE_GC, "o1"),
        ("disable-o1-again", DISABLE_GC, "o1"),
        ("enable-o2-again", ENABLE_GC, "o2"),
    ]
    p, fx = gc_fixture(exe)
    rows = []
    for step, addr, key in steps:
        p.call(addr, p.call_info(fx[key], b"()V"))
        rows.append(
            {
                "suite": "gc",
                "id": f"gc-{step}",
                "exe_addr": addr,
                "name": "disableGC" if addr == DISABLE_GC else "enableGC",
                "step": step,
                "target": key,
                "fixture": {k: fx[k] for k in FIXTURE_KEYS},
                "state": snapshot(p, fx),
            }
        )
    return rows


def run_exit(exe: str):
    """`System.exit()V` / `System.exit(Ljava.lang.String;)V` (row O5).

    Each row gets its own image: the exit request word is a process global,
    so rows that share a probe would not show which call wrote it. A row with
    `prime_exit` calls `exit()V` first, then the row's own call — the shape
    the message overload is reached with in a script that already exited.
    """
    rows = []

    def probe_call(p: Probe, addr: int, info: int = 0):
        if addr == EXIT:
            # `exit()V` takes no parameter: the body never reads `info`.
            return p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
        return p.call(addr, info)

    def row(row_id: str, addr: int, sig: bytes, receiver: int, params, prime: bool):
        p = Probe(exe)
        if prime:
            p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
        before = p.o.read_u32(EXIT_FLAG_ADDR)
        p.logs.clear()
        ci = p.call_info(
            receiver,
            sig,
            params=params,
            record_names=(b"java.lang.System", b"exit"),
        )
        r = probe_call(p, addr, ci)
        rows.append(
            {
                "suite": "exit",
                "id": row_id,
                "exe_addr": addr,
                "name": "exit",
                "sig": sig.decode("latin1"),
                "receiver": receiver,
                "params": list(params),
                "prime_exit": prime,
                "flag_before": before,
                "flag_after": p.o.read_u32(EXIT_FLAG_ADDR),
                "eax": r.eax,
                "logs": list(p.logs),
            }
        )

    # The only writer of `[0x63c520]`.
    row("exit-noargs", EXIT, b"()V", 0, [], False)
    # The message overload on a fresh image: marshals, writes nothing.
    row("exit-string", EXIT_STRING, b"(Ljava.lang.String;)V", 0x200000C0, [0x200000C0], False)
    # ... and it neither sets nor clears a request already made.
    row("exit-string-after-exit", EXIT_STRING, b"(Ljava.lang.String;)V", 0x200000C0, [0x200000C0], True)
    return rows


def run_analyze(exe: str):
    """`System.analyze` trio (row O8): `0x47bed0`/`0x47bf10`/`0x47bef0`.

    Each overload marshals its declared parameter through `FUN_0045d910` into
    its **own caller-frame argument slot** (the word `info` arrived in: the
    exe pushes a null receiver out and its own `[ebp+8]`/`[esp+4]` as the
    parameter out) and does nothing else with it:

    - `analyze(Ljava.lang.Object;)V` (`0x47bed0`) then reads the slot back
      (`mov eax, [ebp+8]`) and `test`s it; the `if (arg != 0)` body is one
      `nop`, so a non-zero word costs exactly one extra instruction.
    - `analyze(I)V` (`0x47bf10`) and `analyze(F)V` (`0x47bef0`) never read the
      slot back; they push nothing to the FPU, so the `F` payload stays the
      raw f32 bit pattern.

    The observable per row is therefore the word left in the slot after the
    call (ESP at return is that slot's address: the native's `ret` has popped
    its frame), plus `eax`, whether `st0` stayed empty, the instruction count
    (the `nop` branch), and whether the `CallInfo.parseParameters`
    diagnostics fired. A malformed container (fewer cells than the descriptor
    needs) leaves the slot holding the `info` **pointer** — the marshaller
    never wrote — and logs; the full repeated diagnostic series is ledger row
    O5b's, so only the first entries are recorded here.
    """
    rows = []

    def row(row_id: str, addr: int, sig: bytes, params):
        p = Probe(exe)
        ci = p.call_info(
            0, sig, params=params, record_names=(b"java.lang.System", b"analyze")
        )
        p.logs.clear()
        r = p.call(addr, ci)
        # At return the native has popped its frame, so ESP is the address of
        # the argument slot it was handed `info` in.
        slot = p.o.read_u32(p.o.reg("esp"))
        malformed = not params
        # `analyze(Object)` returns the slot word (its `mov eax, [ebp+8]`);
        # the other two return whatever the marshaller left in `eax`, which is
        # an arena word the row records but does not pin.
        if addr == ANALYZE_OBJECT:
            eax_rule = "info" if malformed else "slot"
        else:
            eax_rule = "unstable"
        row_out = {
            "suite": "analyze",
            "id": row_id,
            "exe_addr": addr,
            "name": "analyze",
            "sig": sig.decode("latin1"),
            "receiver": 0,
            "params": list(params),
            "slot_word": slot,
            "slot_source": "info" if malformed else "param",
            "eax": r.eax,
            "eax_rule": eax_rule,
            "st0_empty": r.st0 is None,
            "icount": r.icount,
            "logged": bool(p.logs),
            "logs_head": list(p.logs[:5]),
        }
        rows.append(row_out)

    # The Object overload: a live object word, a null, and a container whose
    # top-of-stack word is what the descriptor reads (`[count-2]`).
    row("analyze-object", ANALYZE_OBJECT, b"(Ljava.lang.Object;)V", [0x200000C0])
    row("analyze-object-null", ANALYZE_OBJECT, b"(Ljava.lang.Object;)V", [0])
    row("analyze-object-container-top", ANALYZE_OBJECT, b"(Ljava.lang.Object;)V", [0xAAAA, 0xBBBB])
    # The int overload: the payload word verbatim, sign bit included.
    row("analyze-int", ANALYZE_INT, b"(I)V", [0x12345678])
    row("analyze-int-negative", ANALYZE_INT, b"(I)V", [0xFFFFFFFF])
    # The float overload: raw bit patterns (1.5f, -1.5f, the smallest
    # subnormal word 1) — the native never converts or rounds them.
    row("analyze-float", ANALYZE_FLOAT, b"(F)V", [0x3FC00000])
    row("analyze-float-negative", ANALYZE_FLOAT, b"(F)V", [0xBFC00000])
    row("analyze-float-subnormal", ANALYZE_FLOAT, b"(F)V", [0x00000001])
    # Malformed containers: the marshaller stops, the slot keeps `info`.
    row("analyze-object-malformed", ANALYZE_OBJECT, b"(Ljava.lang.Object;)V", [])
    row("analyze-int-malformed", ANALYZE_INT, b"(I)V", [])
    row("analyze-float-malformed", ANALYZE_FLOAT, b"(F)V", [])
    return rows


INFO = 0x47C0C0
GC = 0x47C060
RUN_FINALIZATION = 0x47C070
GC_JVM_ADDR = 0x62E004
LIVE_CELLS = 0x62F138

# The six `info` ids, in jump-table order: the `%d` source of each.
INFO_KEYS = ("list_18", "list_1c", "list_10", "word_24", "word_28", "live_cells")


def info_signed(word: int) -> int:
    """A 32-bit word as the signed int the exe's `%d` reads."""
    word &= 0xFFFFFFFF
    return word - 0x100000000 if word > 0x7FFFFFFF else word


def info_decimal(word: int) -> str:
    """The exe's own `%d`, `FUN_00554170`, reached from the formatter
    `FUN_00553d70`'s `d`/`i` case (`0x551220` is the exe's `sprintf`, not the
    CRT's).

    Reproduces the body including its `INT_MIN` overflow: the exe negates the
    value before the digit loop, so `0x80000000` comes back negative and every
    `v % 10` (C truncation) is negative, leaving characters `48 - |rem|`.
    """
    v = info_signed(word)
    negative = v < 0
    if negative:
        v = info_signed(-v & 0xFFFFFFFF)
    digits = []
    while True:
        q = (abs(v) // 10) * (1 if v >= 0 else -1)  # C truncation
        digits.append(chr(48 + (v - q * 10)))
        v = q
        if v == 0:
            break
    return ("-" if negative else "") + "".join(reversed(digits))


def info_chain(p, links: int) -> int:
    """A head node whose `+0x04` chain holds `links` following nodes.

    The exe's count loop stops at the first null `+0x04`, so a head with
    `links` successors counts to `links` (the head itself is never counted).
    """
    nodes = [p.o.alloc(0x20) for _ in range(links + 1)]
    for i, node in enumerate(nodes):
        nxt = nodes[i + 1] if i + 1 < len(nodes) else 0
        p.o.write_u32s(node, [0, nxt, 0])
    return nodes[0]


def info_fixture(p, readings, null_heads=()):
    """The JVM object `info` walks (`info+0x00`), from a row's `readings`.

    `+0x18`/`+0x1c`/`+0x10` hold a holder whose `+0x08` is the chain head, the
    three ids 0..=2 count; `+0x24`/`+0x28` are pushed whole; `[0x62f138]` (the
    live-cell counter, a process global) is `readings["live_cells"]`.
    `null_heads` names roots whose head cell is 0 (the count's early-out).
    """
    jvm = p.o.alloc(0x40)
    p.o.write_u32s(jvm, [0] * 16)
    for off, key in ((0x18, "list_18"), (0x1C, "list_1c"), (0x10, "list_10")):
        holder = p.o.alloc(0x20)
        p.o.write_u32s(holder, [0] * 8)
        head = 0 if key in null_heads else info_chain(p, readings[key])
        p.o.write_u32(holder + 8, head)
        p.o.write_u32(jvm + off, holder)
    p.o.write_u32(jvm + 0x24, readings["word_24"] & 0xFFFFFFFF)
    p.o.write_u32(jvm + 0x28, readings["word_28"] & 0xFFFFFFFF)
    p.o.write_u32(LIVE_CELLS, readings["live_cells"] & 0xFFFFFFFF)
    return jvm


def run_info(exe: str):
    """`System.info(I)Ljava.lang.String;` (row O9): `0x47c0c0`.

    The native marshals its `I` parameter into its own frame's scratch word
    (`FUN_0045d910(info, 0, &buffer)`), switches on it (`cmp eax,5; ja`, jump
    table `0x47c1b4`) and interns `sprintf(buf, "%d", value)` through
    `FUN_004174a0`. The JVM object the switch reads through `info+0x00` is
    hand-built per row (`info_fixture`), so the mapping is pinned:

    - 0/1/2 count the `+0x04` chain at `[[jvm+0x18]+0x08]` /
      `[[jvm+0x1c]+0x08]` / `[[jvm+0x10]+0x08]` (head excluded, null -> 0);
    - 3/4/5 push `[jvm+0x24]` / `[jvm+0x28]` / `[0x62f138]` whole;
    - ids above 5 (unsigned `ja`) skip the `sprintf` and intern the scratch
      word's *residue* (the `%d` target is `buffer+4`; the parameter went to
      `buffer+0`, so nothing on that path wrote it). A fresh (zeroed) stack
      reads `""`; `info-residue` primes the same frame with `info(3)` so the
      residue is that call's text. The port interns `""` there and records
      the divergence in `system::info`.

    The observable per row is the text handed to the interner (`""` never
    reaches it in the exe — `FUN_004174a0` is called either way) and the
    instruction count, which separates the formatted branches (249-334) from
    the no-format one (90).
    """
    rows = []
    base = {
        "list_18": 2,
        "list_1c": 0,
        "list_10": 3,
        "word_24": 12345,
        "word_28": 0xFFFFFFFF,
        "live_cells": 42,
    }

    def row(row_id, id_word, readings=None, params=None, prime=None, null_heads=()):
        readings = dict(base if readings is None else readings)
        p = Probe(exe)
        jvm = info_fixture(p, readings, null_heads)
        params = [id_word] if params is None else list(params)

        def call(words):
            ci = p.call_info(0, b"(I)Ljava.lang.String;", params=words)
            p.o.write_u32(ci, jvm)
            p.interned.clear()
            return p.call(INFO, ci)

        if prime is not None:
            call([prime])
        r = call(params)
        text = p.interned[0].decode("latin1") if p.interned else None
        # The descriptor declares one parameter, and the marshaller writes the
        # container's `[count-2]` word — the last of `params` here, since the
        # probe appends the receiver cell after them.
        marshalled = params[-1] if params else 0
        formatted = 0 <= marshalled <= 5
        if formatted:
            want = info_decimal(readings[INFO_KEYS[marshalled]])
            assert text == want, f"{row_id}: exe interned {text!r}, rule says {want!r}"
            residue = None
        elif prime is None:
            # A fresh frame leaves the `%d` target zeroed: the no-format path
            # interns the empty string.
            assert text == "", f"{row_id}: fresh frame interned {text!r}"
            residue = "fresh"
        else:
            # The id>5 path interns whatever the frame already held: a
            # preceding formatted call from the same depth leaves its text.
            assert 0 <= prime <= 5, row_id
            want = info_decimal(readings[INFO_KEYS[prime]])
            assert text == want, f"{row_id}: residue {text!r} != primed text {want!r}"
            residue = "primed"
        rows.append(
            {
                "suite": "info",
                "id": row_id,
                "exe_addr": INFO,
                "name": "info",
                "sig": "(I)Ljava.lang.String;",
                "receiver": 0,
                "params": params,
                "id_word": id_word,
                "primed_id": prime,
                "readings": readings,
                "null_heads": list(null_heads),
                "text": text,
                "formatted": formatted,
                "residue": residue,
                "intern_count": len(p.interned),
                "icount": r.icount,
                "logged": bool(p.logs),
            }
        )

    # The six mapped ids, over distinct readings.
    for i, key in enumerate(
        ("list_18", "list_1c", "list_10", "word_24", "word_28", "live_cells")
    ):
        row(f"info-{i}", i)
    # The `%d` conversion is signed: the top bit is a `-`, not a big positive.
    # Only the pushed fields (ids 3/4/5) can carry the top bit — the chain
    # counts are loop counters, and 0x80000000 links would not emulate.
    signed = dict(base, word_24=0xFFFFFFFE, word_28=0x80000000, live_cells=0x7FFFFFFF)
    row("info-word-24-negative", 3, readings=signed)
    row("info-word-28-negative", 4, readings=signed)
    row("info-live-cells-max", 5, readings=signed)
    # A head cell of 0 takes the count's early-out (`je` before the loop).
    row("info-null-head", 1, null_heads=("list_1c",))
    # The switch reads the marshalled parameter (`[count-2]` — the last cell
    # of a 2+-cell container), not the receiver at `[count-1]` and not the
    # container's first cell: with `[0xAAAA, 0x1000, 3]` (+ the receiver)
    # only id 3 formats.
    row("info-container-top", 0xBBBB, params=[0xAAAA, 0x1000, 3])
    # Ids outside 0..=5 have no format: the exe interns the scratch word's
    # residue, which a fresh stack leaves empty and a primed frame keeps.
    row("info-id-6", 6)
    row("info-id-7", 7)
    row("info-id-negative", 0xFFFFFFFF)
    row("info-id-large", 0x41414141)
    row("info-residue", 9, prime=3)
    return rows


def sysgc_fixture(p, ctx=0, mark_bytes=0):
    """The JVM object `System.gc`'s collector walks (`*(info+0x00)`).

    `+0x08` is the root-record array the initializing pass scans (512 records
    of 12 bytes, `mov dword ptr [esp+0x10], 0x200` at `0x417fef`), `+0x10`,
    `+0x18` and `+0x1c` are holders whose `+0x08` is an empty chain node (the
    drain pass's sweep and the two finalizer walkers stop at the first null
    `+0x04`), `+0x20` is the mark-context word the row starts from and
    `+0x28` the mark-size word `System.info(4)` reports.
    """
    jvm = p.o.alloc(0x40)
    p.o.write_u32s(jvm, [0] * 16)
    p.o.write_u32(jvm + 8, p.o.alloc(512 * 12))
    for off in (0x10, 0x18, 0x1C):
        holder = p.zeroed()
        node = p.zeroed()
        p.o.write_u32(holder + 8, node)
        p.o.write_u32(jvm + off, holder)
    p.o.write_u32(jvm + 0x20, ctx)
    p.o.write_u32(jvm + 0x28, mark_bytes & 0xFFFFFFFF)
    return jvm


def run_sysgc(exe: str):
    """`System.gc()I` / `System.runFinalization()I` (M1-08 row O10).

    `gc` (`0x47c060`) is `mov eax, [esp+4]; mov ecx, [eax]; jmp FUN_00417f80`:
    the collector's whole body, tail-called with the JVM word, and its `eax`
    is the native's return. The collector's return word is a **pass
    discriminator**:

    - `[jvm+0x20] == 0` (no mark context): this pass allocates one
      (`0x417f97`-`0x417fd7`), publishes the JVM at `0x62e004` and returns
      `-1` (`or eax, 0xffffffff` at `0x418058`). It leaves `[jvm+0x28]`
      alone.
    - `[jvm+0x20] != 0`: `FUN_00418970` drains and frees that context and
      clears the word (`0x418b5b`), and the pass returns the number of
      objects its sweep freed (`mov eax, ebp` at `0x4180b2`; 0 over an empty
      heap). On the way it stores the freed container's byte size at
      `[jvm+0x28]` (`mov edx, [esi]; shl edx, 2; mov dword ptr [ebp+0x28],
      edx` at `0x418b2b`) — so a cycle is visible through `System.info(4)`,
      which the `gc-cycle-info4` row reads on the same JVM before and after.

    `runFinalization` (`0x47c070`) is `xor eax, eax; ret`: two instructions,
    no argument, no call, no write. Its rows prime `[0x63c520]` with the real
    `exit()V` first, so the empty write list proves it clears no exit request
    either (the `exit-string-after-exit` shape).

    A step's writes are recorded as the *offsets* they touch inside the JVM
    object (arena-independent), the whole write count, `eax`, the instruction
    count and the FPU state; the `[0x62e004]` global is recorded as a
    predicate (it names the fixture JVM, an address no row can pin).
    """
    rows = []

    def scenario(row_id, steps, info_id=None, ctx=0, mark_bytes=0, prime_exit=False):
        p = Probe(exe)
        jvm = sysgc_fixture(p, ctx=ctx, mark_bytes=mark_bytes)
        ci = p.call_info(0, b"()I", record_names=(b"java.lang.System", steps[0].encode()))
        # The natives read their JVM word out of `*info` (`mov ecx, [eax]`),
        # not out of the argument container.
        p.o.write_u32(ci, jvm)

        def info4():
            info_ci = p.call_info(0, b"(I)Ljava.lang.String;", params=[info_id])
            p.o.write_u32(info_ci, jvm)
            p.interned.clear()
            p.call(INFO, info_ci)
            return p.interned[0].decode("latin1") if p.interned else None

        if prime_exit:
            p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
        flag_before = p.o.read_u32(EXIT_FLAG_ADDR)
        text_before = info4() if info_id is not None else None
        per_step = []
        for name in steps:
            p.logs.clear()
            r = p.call(GC if name == "gc" else RUN_FINALIZATION, ci)
            per_step.append(
                {
                    "call": name,
                    "eax": r.eax,
                    "icount": r.icount,
                    "esp_delta": r.esp_delta,
                    "st0_empty": r.st0 is None,
                    "writes_total": len(r.mem_writes),
                    "jvm_write_offsets": sorted(
                        a - jvm for a, _s, _v in r.mem_writes if jvm <= a < jvm + 0x40
                    ),
                    "ctx": bool(p.o.read_u32(jvm + 0x20)),
                    "mark_bytes": p.o.read_u32(jvm + 0x28),
                    "gc_jvm_is_fixture": p.o.read_u32(GC_JVM_ADDR) == jvm,
                    "logged": bool(p.logs),
                }
            )
        rows.append(
            {
                "suite": "sysgc",
                "id": row_id,
                "exe_addr": GC if steps[0] == "gc" else RUN_FINALIZATION,
                "name": steps[0],
                "sig": "()I",
                "steps": list(steps),
                "ctx_before": bool(ctx),
                "mark_bytes_before": mark_bytes,
                "prime_exit": prime_exit,
                "flag_before": flag_before,
                "flag_after": p.o.read_u32(EXIT_FLAG_ADDR),
                "per_step": per_step,
                "info_id": info_id,
                "info_text_before": text_before,
                "info_text_after": info4() if info_id is not None else None,
            }
        )

    # The initializing pass: -1, the context stored, no mark size published.
    scenario("gc-init", ["gc"])
    # ... and it leaves a mark size a previous cycle published for `info(4)`.
    scenario("gc-init-keeps-mark-size", ["gc"], mark_bytes=0xDEADBEEF)
    # The draining pass: 0, the context cleared and freed, `cap << 2` stored.
    scenario("gc-cycle", ["gc", "gc"])
    # A third call finds no context again and initializes a new cycle.
    scenario("gc-third-pass", ["gc", "gc", "gc"])
    # The drain pass's `[jvm+0x28]` is the word `System.info(4)` reports.
    scenario("gc-cycle-info4", ["gc", "gc"], info_id=4)
    # `runFinalization` returns 0 and writes nothing at all.
    scenario("runfinalization", ["runFinalization"], prime_exit=True)
    # ... not even after a collector pass in the same image.
    scenario("runfinalization-after-gc", ["gc", "runFinalization"], prime_exit=True)
    return rows


def run_string_natives(exe: str):
    """`java.lang.String` natives (rows M10/M11..M16): `0x481ed0` .. `0x481cf0`.

    Each row plants the receiver's char buffer (and a charset's, for `token`)
    by object word, calls the real native with a hand-built `CallInfo`, and
    records what the exe computed. The intern sink returns a sentinel, so
    `append`/`token` rows record the bytes handed to the interner; `length`,
    `intValue`, `floatValue` and `finalize` record `eax`/`st0` directly.
    """
    rows = []
    STR = 0x2000_0000

    def probe_row(name, native, sig, params, receiver=b"", charset=None, text=None):
        p = Probe(exe)
        obj = STR + 0x40  # any word; the hook maps it to the planted buffer
        p.plant(receiver, obj)
        declared = []
        if charset is not None:
            # `481d6b..481d6e` pushes the marshalled charset word straight into
            # `strtok`, with no `FUN_0042ab50` resolve: the word must be the
            # `java.lang.String` *char buffer*, exactly like the receiver the
            # field hook maps.
            cbuf = p.plant(charset, STR + 0x41)
            declared.append((cbuf, "L"))
        for kind, value in params:
            declared.append((value, kind))
        cells = p.call_info(obj, sig, params=[v for v, _k in declared],
                            record_names=(b"java.lang.String", name.encode()))
        # `finalize` reads the interner out of `[CallInfo+0]` (the JVM word)
        # and then `[jvm+0x68]`; plant both so the hook has a this to record.
        jvm = p.o.alloc(0x80)
        p.o.write_u32(jvm + 0x68, 0x5E7A00)
        p.o.write_u32(cells, jvm)
        p.interned.clear()
        p.logs.clear()
        r = p.call(native, cells)
        rows.append(
            {
                "suite": "string",
                "id": text or name,
                "exe_addr": native,
                "name": name,
                "sig": sig.decode(),
                "receiver": receiver.decode("latin1"),
                "arg": next((v for k, v in params if k == "I"), None),
                "index": next((v for k, v in params if k == "I"), None),
                "charset": None if charset is None else charset.decode("latin1"),
                "eax": r.eax,
                "st0_empty": r.st0 is None,
                "st0_bits": None if r.st0 is None else struct.unpack("<I", struct.pack("<f", r.st0))[0],
                "interned": [b.decode("latin1") for b in p.interned],
                "releases": [list(r) for r in p.releases],
                "logged": list(p.logs),
                "icount": r.icount,
            }
        )

    probe_row("intValue", STRING_INT_VALUE, b"()I", [], receiver=b"  -42abc")
    probe_row("intValue", STRING_INT_VALUE, b"()I", [], receiver=b"", text="int-empty")
    probe_row("intValue", STRING_INT_VALUE, b"()I", [], receiver=b"zzz", text="int-nomatch")
    probe_row("floatValue", STRING_FLOAT_VALUE, b"()F", [], receiver=b"3.25")
    probe_row("floatValue", STRING_FLOAT_VALUE, b"()F", [], receiver=b"0.1", text="float-0.1")
    probe_row("length", STRING_LENGTH, b"()I", [], receiver=b"hello")
    probe_row("length", STRING_LENGTH, b"()I", [], receiver=b"", text="length-empty")
    probe_row("append", STRING_APPEND, b"(I)Ljava.lang.String;", [("I", ord("c"))],
              receiver=b"ab")
    probe_row("append", STRING_APPEND, b"(I)Ljava.lang.String;", [("I", 8)],
              receiver=b"abc", text="append-backspace")
    probe_row("append", STRING_APPEND, b"(I)Ljava.lang.String;", [("I", 7)],
              receiver=b"ab", text="append-below-space")
    probe_row("append", STRING_APPEND, b"(I)Ljava.lang.String;", [("I", ord("!"))],
              receiver=b"x" * 300, text="append-truncates")
    probe_row("finalize", STRING_FINALIZE, b"()V", [], receiver=b"finalize-me")
    # The index sweep pins the loop: the same text, one row per index, so the
    # token the exe returns for each count is recorded rather than inferred.
    for idx in (0, 1, 2, 3):
        probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
                  [("I", idx)], receiver=b"a,b,c", charset=b",",
                  text=f"token-index-{idx}")
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 1)], receiver=b"abc", charset=b",", text="token-nosep-index-1")
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 1)], receiver=b"a,,b", charset=b",", text="token-doublesep-index-1")
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 0)], receiver=b"abc", charset=b",", text="token-nosep-index-0")
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 0)], receiver=b"a,b c", charset=b", ", text="token-two-seps")
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 5)], receiver=b"a", charset=b",", text="token-past-end")
    # `481d3a cmp esi,0x100`: a receiver whose length+1 does not fit the frame
    # takes the `481d42` malloc path. The whole string must still copy.
    probe_row("token", STRING_TOKEN, b"(ILjava.lang.String;)Ljava.lang.String;",
              [("I", 0)], receiver=b"y" * 300, charset=b",", text="token-heap-copy")
    return rows


# Globals the #35 non-timer System rows write or read. A row records the word
# before and after its call so the port's seam can be compared directly.
SYSTEM_GLOBALS = {
    "0x60c8c4": ("ld_scale", "u32"),
    "0x640920": ("ld_flag", "u32"),
    "0x636448": ("time_warp", "u32"),
    "0x636440": ("time_warp_rate", "u32"),
    "0x640988": ("game_time", "i32"),
    "0x6178d0": ("daylight", "u32"),
    "0x60e04c": ("measure", "u32"),
    "0x63c770": ("measure_scaled", "u32"),
    "0x63c73c": ("measure_recip", "u32"),
    # `simTime` reads `[[0x617650]]` (the physics world's +0x00), so the
    # sample dereferences: the pointer itself is not the fact
    "0x617650": ("sim_time", "indirect"),
}


def run_system_rows(exe: str):
    """The ten `java.lang.System` rows outside exit/log/analyze/info/gc/gc2.

    Each row calls the real body with a hand-built `CallInfo` and records the
    globals it touches, before and after. The two scalar rows that marshal no
    argument (`isLoading`) pass a null container, which is what the exe's
    marshaller receives for a `()I` row.
    """
    import struct

    rows = []

    def read_word(oc, addr):
        raw = bytes(oc.read(addr, 4))
        return raw

    def decode(raw, kind):
        # f32 words are recorded as their raw bit patterns: JSON has no
        # spelling for the infinity `setMeasure(F)` produces on a zero
        # argument, and bit equality is the stronger claim anyway.
        if kind == "i32":
            return struct.unpack("<i", raw)[0]
        return struct.unpack("<I", raw)[0]

    def sample(oc):
        out = {}
        for addr_text, (name, kind) in SYSTEM_GLOBALS.items():
            addr = int(addr_text, 16)
            if kind == "indirect":
                # one dereference: the word the pointer names
                ptr = int.from_bytes(bytes(oc.read(addr, 4)), "little")
                out[name] = decode(read_word(oc, ptr), "u32") if ptr else 0
            else:
                out[name] = decode(read_word(oc, addr), kind)
        return out

    def seed_timer(oc, freq=1000, t1=0, t2=500):
        """`FUN_005516c0` reads three qwords: `[0x76a280]` (frequency),
        `[0x76a288]` (start) and `[0x76a290]` (now), and returns
        `(now - start) / freq * 1000.0` milliseconds. The emulator leaves them
        zero, which makes the routine return NaN and hides `FUN_004283f0`'s
        arithmetic, so each row seeds a defined 500 ms reading.
        """
        for addr, value in ((0x76A280, freq), (0x76A288, t1), (0x76A290, t2)):
            oc.write_u32(addr, value & 0xFFFFFFFF)
            oc.write_u32(addr + 4, (value >> 32) & 0xFFFFFFFF)

    def row(name, addr, sig, args, eax_kind="u32"):
        p = Probe(exe)
        seed_timer(p.o)
        info = p.call_info(0, sig, params=args)
        before = sample(p.o)
        res = p.call(addr, info)
        after = sample(p.o)
        if eax_kind == "f32":
            # a float return rides ST0, not EAX (the x87 convention these
            # natives use); record the f64 the FPU handed back
            eax = res.st0 if res.st0 is not None else None
        else:
            eax = res.eax & 0xFFFFFFFF
        rows.append(
            {
                "id": f"system-{name}",
                "suite": "system",
                "name": name,
                "exe_addr": f"0x{addr:08x}",
                "sig": sig.decode("latin1"),
                "args": [a if isinstance(a, int) else a[0] for a in args],
                "before": before,
                "after": after,
                "eax": eax,
            }
        )

    # timeWarp(F)F: below the 0.0 gate, at it, above it, and a NaN (unordered
    # takes the same arm as `<`).
    for label, word in [
        ("below", struct.unpack("<I", struct.pack("<f", -1.5))[0]),
        ("at_gate", 0),
        ("above", struct.unpack("<I", struct.pack("<f", 2.5))[0]),
        ("nan", 0x7FC00000),
    ]:
        row(f"timeWarp_{label}", 0x47BF80, b"(F)F", [word], eax_kind="f32")

    # setLdPriority(I)V: the zero and non-zero arms.
    row("setLdPriority_zero", 0x47BF30, b"(I)V", [0])
    row("setLdPriority_nonzero", 0x47BF30, b"(I)V", [7])

    # syncGameTime(F)V across the day/night boundaries the compares name, plus
    # a negative argument (the signed idiv keeps its sign).
    for label, secs in [
        ("midnight", 0),
        ("before_one", 3599),
        ("one", 3600),
        ("before_five", 17999),
        ("five", 18000),
        ("before_seven", 25199),
        ("seven", 25200),
        ("before_twenty", 71999),
        ("twenty", 72000),
        ("negative", -3600),
        ("wrapped", 86400 + 3600),
    ]:
        word = struct.unpack("<I", struct.pack("<f", float(secs)))[0]
        row(f"syncGameTime_{label}", 0x47BFF0, b"(F)V", [word])

    # setMeasure(F)V: a normal value and zero (the exe's seeded-local path).
    for label, v in [("normal", 500.0), ("zero", 0.0), ("negative", -250.0)]:
        word = struct.unpack("<I", struct.pack("<f", v))[0]
        row(f"setMeasure_{label}", 0x47C4B0, b"(F)V", [word])

    # buildNumber()I: a constant, no marshalling at all.
    row("buildNumber", 0x47C0B0, b"()I", [])

    # currentTime()F: the clock seam scaled by the f32 0.001. `seed_timer`
    # makes the reading a defined 500 ms, so the row pins the multiply.
    row("currentTime", 0x47BFE0, b"()F", [], eax_kind="f32")

    # simTime()F: `fld dword [[0x617650]]`, the physics world's time word.
    # The probe plants a world and seeds its +0x00 so the read is defined.
    p = Probe(exe)
    seed_timer(p.o)
    world = p.o.alloc(0x40)
    p.o.write_u32(world, struct.unpack("<I", struct.pack("<f", 12.5))[0])
    p.o.write_u32(0x617650, world)
    before = sample(p.o)
    res = p.call(0x47BFD0, 0)
    rows.append(
        {
            "id": "system-simTime",
            "suite": "system",
            "name": "simTime",
            "exe_addr": "0x0047bfd0",
            "sig": "()F",
            "args": [],
            "before": before,
            "after": sample(p.o),
            "eax": res.st0,
            "sim_word": 12.5,
        }
    )

    # isLoading()I reads `[[0x618d48]+0x106ed4]`. The oracle leaves
    # `[0x618d48]` at its emulator default, so the row is only meaningful once
    # the loader global is seeded; record it with the pointer planted.
    p = Probe(exe)
    seed_timer(p.o)
    loader = p.o.alloc(0x107000)
    p.o.write_u32(loader + 0x106ED4, 7)
    p.o.write_u32(0x618D48, loader)
    before = sample(p.o)
    res = p.call(0x47C3B0, 0)
    rows.append(
        {
            "id": "system-isLoading",
            "suite": "system",
            "name": "isLoading",
            "exe_addr": "0x0047c3b0",
            "sig": "()I",
            "args": [],
            "before": before,
            "after": sample(p.o),
            "eax": res.eax & 0xFFFFFFFF,
            "loader_word": 7,
        }
    )

    return rows


def run_thread_rows(exe: str):
    """The `java.lang.Thread` field-write natives.

    `setPriority(I)V` (`0x47c790`) and `setDaemon(I)V` (`0x47c6e0`) are a frame
    pop, a `0x42ab50` field lookup and one store into the thread object
    (`+0x28` the priority word, `+0x2c` the flags with bit `0x100`). Nothing
    else runs, so the row can record the whole object before and after.

    `init`/`start`/`stop` are NOT here: they call `0x41f340` (spawn a real
    thread), `0x41f9f0` (run it) and `0x41f780` (die), which need the live JVM
    thread list and scheduler, not a single CallInfo frame.
    """
    rows = []

    def thread_row(name, addr, sig, arg_words, fields):
        p = Probe(exe)
        obj = p.o.alloc(0x60)
        # the field lookup answers this object for any receiver
        p.o.hook(0x42AB50, lambda oc, a: obj, "cdecl", 2)
        for off, value in fields.get("before", {}).items():
            p.o.write_u32(obj + int(off, 16), value)
        info = p.call_info(obj, sig, params=arg_words)
        res = p.call(addr, info)
        after = {
            off: p.o.read_u32(obj + int(off, 16))
            for off in fields["watch"]
        }
        rows.append({
            "id": f"thread-{name}",
            "suite": "thread",
            "name": name,
            "exe_addr": f"0x{addr:08x}",
            "sig": sig.decode("latin1"),
            "args": list(arg_words),
            "object_word": obj,
            "before": fields.get("before", {}),
            "after": after,
            "eax": res.eax & 0xFFFFFFFF,
        })

    # setPriority(I)V: three priorities, on an object whose word starts at 0.
    for prio in (0, 5, 10):
        thread_row(
            f"setPriority_{prio}", 0x47C790, b"(I)V", [prio],
            {"before": {"0x28": 0}, "watch": ["0x28"]},
        )
    # a nonzero starting word, so the store is the only writer
    thread_row(
        "setPriority_overwrite", 0x47C790, b"(I)V", [3],
        {"before": {"0x28": 0xDEADBEEF}, "watch": ["0x28"]},
    )

    # setDaemon(I)V: nonzero sets bit 0x100, zero clears it, and the other
    # bits of +0x2c survive either way.
    for label, arg, before in [
        ("set", 1, 0),
        ("clear", 0, 0x100),
        ("keep_other_bits_set", 1, 0x0F),
        ("keep_other_bits_clear", 0, 0xFFFFFEFF),
        ("bit_already_set", 1, 0x100),
        ("zero_on_zero", 0, 0),
    ]:
        thread_row(
            f"setDaemon_{label}", 0x47C6E0, b"(I)V", [arg],
            {"before": {"0x2c": before}, "watch": ["0x2c"]},
        )

    # isAlive()I and getPriority()I are reads of the same lookup.
    def getter_row(name, addr, sig, priority):
        p = Probe(exe)
        obj = p.o.alloc(0x60)
        p.o.hook(0x42AB50, lambda oc, a: obj, "cdecl", 2)
        p.o.write_u32(obj + 0x28, priority)
        info = p.call_info(obj, sig)
        res = p.call(addr, info)
        return {
            "id": f"thread-{name}",
            "suite": "thread",
            "name": name,
            "exe_addr": f"0x{addr:08x}",
            "sig": sig.decode("latin1"),
            "args": [],
            "object_word": obj,
            "before": {"0x28": priority},
            "after": {"0x28": p.o.read_u32(obj + 0x28)},
            "eax": res.eax & 0xFFFFFFFF,
        }

    # isAlive always answers 0 in build 606 (`xor eax,eax` after the lookup),
    # so the rows differ only in the priority word they start from.
    rows.append(getter_row("isAlive_zero", 0x47C730, b"()I", 0))
    rows.append(getter_row("isAlive_live_thread", 0x47C730, b"()I", 7))
    # getPriority returns [thread+0x28].
    rows.append(getter_row("getPriority_zero", 0x47C760, b"()I", 0))
    rows.append(getter_row("getPriority_five", 0x47C760, b"()I", 5))
    rows.append(getter_row("getPriority_high", 0x47C760, b"()I", 0x7FFFFFFF))
    return rows


def gen(exe: str) -> int:
    rows = []
    rows += run_hash(exe, [0xDEADBEEF, 0x00000001, 0x200000C0, 0xFFFFFFFF])
    rows += run_tostr(
        exe,
        [b"java.lang.Object", b"java.game.Car", b"X", b"a.b.C$Inner"],
    )
    rows += run_gc(exe)
    rows += run_exit(exe)
    rows += run_analyze(exe)
    rows += run_info(exe)
    rows += run_sysgc(exe)
    rows += run_string_natives(exe)
    rows += run_thread_rows(exe)
    rows += run_system_rows(exe)
    repo = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    path = os.path.join(repo, OUT_DIR, OUT_FILE)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    now = datetime.datetime.now().isoformat(timespec="seconds")
    with open(path, "w", encoding="utf-8") as fh:
        for row in rows:
            row["oracle"] = "unicorn"
            row["cw"] = "0x007f"
            row["date"] = now
            fh.write(json.dumps(row) + "\n")
    print(f"# {len(rows)} rows -> {os.path.normpath(path)}", file=sys.stderr)
    return 0


def verify(paths, exe: str) -> int:
    repo = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    files = paths or [os.path.join(repo, OUT_DIR, OUT_FILE)]
    checked = 0
    failed = 0
    regenerated = {}
    for path in files:
        with open(path, encoding="utf-8") as fh:
            rows = [json.loads(line) for line in fh if line.strip()]
        gc_state = None
        for row in rows:
            suite = row["suite"]
            ok = True
            detail = ""
            if suite == "hash":
                p = Probe(exe)
                got = p.call(row["exe_addr"], p.call_info(row["receiver_word"], b"()I")).eax
                ok = got == row["eax"]
                detail = f"eax {got:#x} != {row['eax']:#x}"
            elif suite == "tostr":
                p = Probe(exe)
                name = row["class"].encode("latin1")
                obj = p.object_of(name)
                p.interned.clear()
                p.call(row["exe_addr"], p.call_info(obj, b"()Ljava.lang.String;"))
                got = p.interned[0].decode("latin1") if p.interned else None
                want = f"{row['class']}<{obj:x}>"
                ok = got == want
                detail = f"text {got!r} != {want!r}"
            elif suite == "gc":
                if gc_state is None:
                    gc_state = gc_fixture(exe)
                p, fx = gc_state
                p.call(row["exe_addr"], p.call_info(fx[row["target"]], b"()V"))
                got = snapshot(p, fx)
                ok = got == row["state"]
                detail = f"state {got!r} != {row['state']!r}"
            elif suite == "exit":
                p = Probe(exe)
                if row["prime_exit"]:
                    p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
                before = p.o.read_u32(EXIT_FLAG_ADDR)
                ci = p.call_info(
                    row["receiver"],
                    row["sig"].encode("latin1"),
                    params=row["params"],
                    record_names=(b"java.lang.System", b"exit"),
                )
                if row["exe_addr"] == EXIT:
                    p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
                else:
                    p.call(row["exe_addr"], ci)
                got = (before, p.o.read_u32(EXIT_FLAG_ADDR), list(p.logs))
                want = (row["flag_before"], row["flag_after"], row["logs"])
                ok = got == want
                detail = f"(before, after, logs) {got!r} != {want!r}"
            elif suite == "analyze":
                p = Probe(exe)
                ci = p.call_info(
                    row["receiver"],
                    row["sig"].encode("latin1"),
                    params=row["params"],
                    record_names=(b"java.lang.System", b"analyze"),
                )
                p.logs.clear()
                r = p.call(row["exe_addr"], ci)
                slot = p.o.read_u32(p.o.reg("esp"))
                # `info` slots and `eax`-as-slot are arena words, so they are
                # re-derived from this run instead of compared raw.
                want_slot = ci if row["slot_source"] == "info" else row["params"][-1]
                if row["eax_rule"] == "slot":
                    want_eax = slot
                elif row["eax_rule"] == "info":
                    want_eax = ci
                else:
                    want_eax = r.eax
                got = (slot, r.eax, r.st0 is None, bool(p.logs), list(p.logs[:5]))
                want = (want_slot, want_eax, row["st0_empty"], row["logged"], row["logs_head"])
                # A malformed container makes the exe's marshaller walk on
                # into memory and log repeatedly, so its instruction count and
                # the tail of that series are not pinned (row O5b owns them).
                if not row["logged"]:
                    got += (r.icount,)
                    want += (row["icount"],)
                ok = got == want
                detail = f"(slot, eax, st0_empty, logged, logs_head[, icount]) {got!r} != {want!r}"
            elif suite == "info":
                p = Probe(exe)
                jvm = info_fixture(
                    p,
                    row["readings"],
                    row.get("null_heads", ()),
                )

                def call(words):
                    ci = p.call_info(0, b"(I)Ljava.lang.String;", params=words)
                    p.o.write_u32(ci, jvm)
                    p.interned.clear()
                    return p.call(INFO, ci)

                if row["primed_id"] is not None:
                    call([row["primed_id"]])
                r = call(row["params"])
                got = p.interned[0].decode("latin1") if p.interned else None
                ok = got == row["text"] and r.icount == row["icount"]
                detail = f"(text, icount) {(got, r.icount)!r} != {(row['text'], row['icount'])!r}"
            elif suite == "sysgc":
                p = Probe(exe)
                jvm = sysgc_fixture(
                    p,
                    ctx=row["ctx_before"],
                    mark_bytes=row["mark_bytes_before"],
                )
                ci = p.call_info(
                    0, b"()I", record_names=(b"java.lang.System", row["name"].encode())
                )
                p.o.write_u32(ci, jvm)

                def info4():
                    info_ci = p.call_info(0, b"(I)Ljava.lang.String;", params=[row["info_id"]])
                    p.o.write_u32(info_ci, jvm)
                    p.interned.clear()
                    p.call(INFO, info_ci)
                    return p.interned[0].decode("latin1") if p.interned else None

                if row["prime_exit"]:
                    p.o.call(EXIT, args=[], convention="cdecl", fpu_cw=CW_D3D9)
                flag_before = p.o.read_u32(EXIT_FLAG_ADDR)
                text_before = info4() if row["info_id"] is not None else None
                per_step = []
                for name in row["steps"]:
                    p.logs.clear()
                    r = p.call(GC if name == "gc" else RUN_FINALIZATION, ci)
                    per_step.append(
                        {
                            "call": name,
                            "eax": r.eax,
                            "icount": r.icount,
                            "esp_delta": r.esp_delta,
                            "st0_empty": r.st0 is None,
                            "writes_total": len(r.mem_writes),
                            "jvm_write_offsets": sorted(
                                a - jvm for a, _s, _v in r.mem_writes if jvm <= a < jvm + 0x40
                            ),
                            "ctx": bool(p.o.read_u32(jvm + 0x20)),
                            "mark_bytes": p.o.read_u32(jvm + 0x28),
                            "gc_jvm_is_fixture": p.o.read_u32(GC_JVM_ADDR) == jvm,
                            "logged": bool(p.logs),
                        }
                    )
                got = (
                    per_step,
                    flag_before,
                    p.o.read_u32(EXIT_FLAG_ADDR),
                    text_before,
                    info4() if row["info_id"] is not None else None,
                )
                want = (
                    row["per_step"],
                    row["flag_before"],
                    row["flag_after"],
                    row["info_text_before"],
                    row["info_text_after"],
                )
                ok = got == want
                detail = f"(per_step, flag, info texts) {got!r} != {want!r}"
            elif suite in ("system", "thread", "string"):
                # these two suites are re-derivable wholesale: rerun the
                # generator over a fresh probe and compare every recorded fact
                if suite not in regenerated:
                    generator = {"system": run_system_rows, "thread": run_thread_rows,
                                 "string": run_string_natives}[suite]
                    regenerated[suite] = {r["id"]: r for r in generator(exe)}
                fresh = regenerated[suite]
                want = fresh.get(row["id"])
                if want is None:
                    ok, detail = False, f"row {row['id']} not regenerated"
                else:
                    comparable = ("eax", "after", "before", "args", "sig")
                    if suite == "string":
                        comparable = ("eax", "st0_empty", "st0_bits", "interned",
                                      "releases", "logged", "icount", "receiver",
                                      "arg", "index", "charset")
                    got = {k: row.get(k) for k in comparable}
                    exp = {k: want.get(k) for k in comparable}
                    ok = got == exp
                    detail = f"{got!r} != {exp!r}"
            else:
                ok = False
                detail = f"unknown suite {suite!r}"
            checked += 1
            if not ok:
                failed += 1
                print(f"FAIL {row['id']}: {detail}", file=sys.stderr)
    print(f"# verify {checked - failed}/{checked} rows", file=sys.stderr)
    return 1 if failed else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    args = ap.parse_args(argv)
    exe = args.exe or default_exe()
    if args.cmd == "gen":
        return gen(exe)
    return verify(args.files, exe)


if __name__ == "__main__":
    sys.exit(main())
