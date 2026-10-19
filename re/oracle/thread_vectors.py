#!/usr/bin/env python3
"""Unicorn vectors for the TUFA interpreter loop `Thread::run` (M1-03, #30).

Runs the REAL exe function `FUN_00420ff0` (`Thread::run`) on hand-built
programs in emulator memory, no JVM boot and no game data. Per program the
harness builds, with the exe's own constructors where they are
self-contained:

- a thread via `FUN_0041f340` (the real Thread ctor: allocates the frame
  list `thread+0xc` and the base frame, links the thread into the JVM
  thread list); the JVM object is a zeroed stub that carries only the
  `+0x18` list the ctor appends to;
- further frames via `FUN_00408d90` (frame pool) + `FUN_00407b10` (frame
  ctor), linked into the frame list exactly like `Thread::callMethod`
  (`FUN_0041fbc0`) does, so `thread+0x20` bookkeeping on return is the
  exe's;
- an 8-byte node array per frame (`+0` opword u16, `+2` line u16, `+4`
  imm u32);
- initial locals (variable slots, vtable `0x5e7344`) and operand-stack
  contents (value cells, vtable `0x5e734c`, or pointers to locals);
- the allocator pools `DAT_0062f130` (values), `DAT_0062f134` (variables),
  `DAT_0062ddf0` (frames), the live counter `DAT_0062f138` and the type
  descriptor globals `DAT_0062e00c/def0/deec/e000/e010` exactly like
  `vm_vectors.py` (production shape).

Hooked (non-oracle) callees, all error-position or time plumbing that does
not influence value semantics: `FUN_00406f60` / `FUN_00406140` (class-file
and constant-pool string lookups feeding `thread+0x70/0x74`, return 0),
`FUN_005516c0` (the clock; fed from the row's `clock` list), and the log
sinks. `FUN_004190d0` (fatal script error) is hooked to stop the emulation
and the row records `outcome: "fatal"`; an `idiv` trap is recorded as
`outcome: "trap"`.

Row (JSONL under re/out/thread/, git-ignored):
  {"id","name","exe_addr":"0x00420ff0","budget":f32,"clock":[f64..],
   "frames":[{"nodes":[[op,line,imm]..],"entry":i,"locals":[cell..],
              "stack":[cell..]}..]   # bottom frame first, last = current
   "result":{"outcome","flags","frame","pc","frames":[{"stack","locals",
             "saved_pc"}..],"live","leftover"},
   "oracle":"unicorn","unicorn","cw","date"}
cell = {"k":"v","t":"I","w":n}      value cell
     | {"k":"var","t":"I","w":n}    variable slot (locals; or orphan on stack)
     | {"k":"lv","i":n}             stack entry pointing at local n
     | {"k":"null"}                 null stack word
`pc` / `saved_pc` are node indices into the frame's node array (`pc` is
`[frame, index]`). `leftover` is the raw 10-byte x87 return value as hex.

Usage: thread_vectors.py gen | verify [files]   (output re/out/thread/)
"""
from __future__ import annotations
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9, OracleError, UnmappedAccess  # noqa: E402
import unicorn  # noqa: F401

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "thread")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
VAR_VT = 0x5E7344
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C, "V": 0x60AB60}
CW = CW_D3D9
UC_VER = "2.1.4"

RUN = 0x420FF0
THREAD_CTOR = 0x41F340
FRAME_CTOR = 0x407B10
FRAME_POOL_ALLOC = 0x408D90
CLOCK = 0x5516C0
FATAL = 0x4190D0


class ScriptFatal(OracleError):
    pass


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe():
    p = os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe")
    if os.path.exists(p):
        return p
    return os.path.join(MAIN_CHECKOUT, "reference", "raw", "StreetLegal_Redline.bak.exe")


# ---------------------------------------------------------------- scenario


class Scenario:
    """One emulator instance holding one thread and its frames."""

    def __init__(self, exe=None, clock=()):
        self.o = o = Oracle(exe or _default_exe())
        o.hook_alloc()

        def _realloc(oc, args):
            old, size = args[0], args[1]
            new = oc.alloc(max(size, 1))
            if old:
                n = oc.heap_sizes.get(old, 0)
                if n:
                    oc.write(new, oc.read(old, min(n, size)))
            return new

        o.hook(0x54F580, _realloc, "cdecl", 2)
        o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
        o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
        o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
        # error-position plumbing (see module docstring)
        o.hook(0x406F60, lambda oc, a: 0, "thiscall", 1)
        o.hook(0x406140, lambda oc, a: 0, "thiscall", 1)
        self._clock = list(clock)

        def _clock_hook(oc, a):
            return float(self._clock.pop(0)) if self._clock else 0.0

        o.hook(CLOCK, _clock_hook, "cdecl", 0)

        def _fatal(oc, a):
            msg = ""
            try:
                msg = oc.read_cstr(a[1]) if len(a) > 1 else ""
            except Exception:
                pass
            oc._err = ScriptFatal(msg)
            oc.uc.emu_stop()
            return 0

        o.hook(FATAL, _fatal, "cdecl", 3)
        for cell, ch in ((0x62E00C, "I"), (0x62DEF0, "I"), (0x62DEEC, "F"),
                         (0x62E000, "L"), (0x62E010, "V")):
            d = o.alloc(4)
            o.write_u32(d, TC[ch])
            o.write_u32(cell, d)
        for g in (0x62F130, 0x62F134, 0x62DDF0):  # value / var / frame pools
            o.write_u32(g, o.alloc(20))
        o.write_u32(0x62F138, 0)  # live counter

        jvm = o.alloc(0x200)
        # helpers run with this = [thread+0x14] = [jvm+4], a cell holding the
        # jvm pointer (their error paths read [this]+0x70/0x74)
        jcell = o.alloc(16)
        o.write_u32(jcell, jvm)
        o.write_u32(jvm + 4, jcell)
        owner, head = o.alloc(0x40), o.alloc(0x40)
        o.write_u32(jvm + 0x18, owner)
        o.write_u32(owner + 0x18, head)
        self.thread = o.alloc(0x100)
        name = o.alloc(16)
        o.write(name, b"t\0")
        o.call(THREAD_CTOR, args=[jvm, 0, 0, name], convention="thiscall",
               this=self.thread, fpu_cw=CW)
        self.jvm = jvm
        self.frames = [o.read_u32(self.thread + 0x20)]  # base frame
        self.ctx = o.alloc(0x100)
        self.ctx_q = o.alloc(0x100)
        o.write_u32(self.ctx + 4, self.ctx_q)
        self.node_span = []  # per frame: (node array address, node count)
        self.local_ptrs = []  # per frame: local pointers

    # -- builders
    def _cell(self, spec):
        o = self.o
        k = spec["k"]
        if k == "null":
            return 0
        p = o.alloc(16 if k == "var" else 12)
        o.write_u32(p, VAR_VT if k == "var" else VAL_VT)
        o.write_u32(p + 4, TC[spec["t"]])
        o.write_u32(p + 8, spec["w"] & 0xFFFFFFFF)
        if k == "var":
            o.write_u32(p + 12, 0)
        return p

    def _container_set(self, base, words):
        o = self.o
        cap = len(words) + 2
        data = o.alloc(4 * cap)
        if words:
            o.write_u32s(data, words)
        o.write_u32(base, cap)
        o.write_u32(base + 4, data)
        o.write_u32(base + 8, len(words))

    def add_frame(self, idx, fspec):
        """Build frame `idx` (0 = the thread ctor's base frame)."""
        o = self.o
        nodes = fspec["nodes"]
        arr = o.alloc(8 * len(nodes) + 8)
        for i, (op, line, imm) in enumerate(nodes):
            o.write_u16(arr + 8 * i, op)
            o.write_u16(arr + 8 * i + 2, line)
            o.write_u32(arr + 8 * i + 4, imm)
        self.node_span.append((arr, len(nodes)))
        entry = 0 if fspec.get("null_pc") else arr + 8 * fspec["entry"]
        if idx == 0:
            f = self.frames[0]
            o.write_u32(f + 0x34, self.ctx)
            o.write_u32(f + 0x2C, entry)
        else:
            pool = o.read_u32(0x62DDF0)
            f = o.call(FRAME_POOL_ALLOC, convention="fastcall",
                       args=[pool], fpu_cw=CW).eax
            o.call(FRAME_CTOR, args=[self.thread, self.ctx, entry, 0,
                                     len(fspec["locals"])],
                   convention="thiscall", this=f, fpu_cw=CW)
            lst = o.read_u32(self.thread + 0xC)
            tail = o.read_u32(lst + 8)
            o.write_u32(f + 4, tail)
            o.write_u32(f + 8, o.read_u32(tail + 8))
            o.write_u32(tail + 8, f)
            o.write_u32(lst + 8, f)
            o.write_u32(f + 0x38, 0)
            self.frames.append(f)
        # locals: variable slots pushed into the frame's local container
        lptrs = [self._cell(c) for c in fspec["locals"]]
        self._container_set(f + 0xC, lptrs)
        self.local_ptrs.append(lptrs)
        stack = []
        for c in fspec["stack"]:
            if c["k"] == "lv":
                stack.append(lptrs[c["i"]])
            else:
                stack.append(self._cell(c))
        self._container_set(f + 0x1C, stack)
        return f

    def finish_setup(self):
        o = self.o
        # thread points at the last (current) frame
        o.write_u32(self.thread + 0x20, self.frames[-1])
        # run() refuses an "empty thread" (pc 0) and then reloads pc from
        # frame+0x2c, so any non-null pc will do.
        o.write_u32(self.thread + 0x24, o.read_u32(self.frames[-1] + 0x2C) or 1)

    # -- observation
    def read_cell(self, p, lptrs):
        o = self.o
        if p == 0:
            return {"k": "null"}
        vt = o.read_u32(p)
        if p in lptrs:
            return {"k": "lv", "i": lptrs.index(p)}
        t = chr(o.read_u8(o.read_u32(p + 4)))
        w = o.read_u32(p + 8)
        return {"k": "var" if vt == VAR_VT else "v", "t": t, "w": w}

    def _container(self, base):
        o = self.o
        data, count = o.read_u32(base + 4), o.read_u32(base + 8)
        return [o.read_u32(data + 4 * i) for i in range(count)]

    def node_index(self, ptr):
        """Pointer into a frame's node array -> [frame, node index] or None."""
        for fi, (base, count) in enumerate(self.node_span):
            if base <= ptr <= base + 8 * count and (ptr - base) % 8 == 0:
                return [fi, (ptr - base) // 8]
        return None

    def snapshot(self, outcome, st0_raw):
        o = self.o
        cur = o.read_u32(self.thread + 0x20)
        cur_idx = self.frames.index(cur) if cur in self.frames else -1
        pc = o.read_u32(self.thread + 0x24)
        res = {
            "outcome": outcome,
            "flags": o.read_u32(self.thread + 0x2C),
            "frame": cur_idx,
            "pc": self.node_index(pc) if pc else None,
            "live": o.read_i32(0x62F138),
            "leftover": st0_raw.hex() if st0_raw else None,
            "frames": [],
        }
        n_alive = cur_idx + 1 if cur_idx >= 0 else 0
        for fi in range(n_alive):
            f = self.frames[fi]
            lptrs = self._container(f + 0xC)
            saved = o.read_u32(f + 0x2C)
            res["frames"].append({
                "locals": [self.read_cell(p, []) for p in lptrs],
                "stack": [self.read_cell(p, lptrs) for p in self._container(f + 0x1C)],
                "saved_pc": (self.node_index(saved)[1] if saved and
                             self.node_index(saved) else None),
            })
        return res


def run_program(prog, exe=None):
    """prog = {"name","frames":[..],"budget":f,"clock":[..]} -> result dict."""
    sc = Scenario(exe, prog.get("clock", ()))
    for i, fs in enumerate(prog["frames"]):
        sc.add_frame(i, fs)
    sc.finish_setup()
    o = sc.o
    budget = prog.get("budget", 0.0)
    outcome, raw = "stopped", None
    try:
        r = o.call(RUN, args=[("f32", budget)], convention="thiscall",
                   this=sc.thread, fpu_cw=CW, limit=2_000_000)
        raw = r.st0_raw
    except ScriptFatal as e:
        outcome = "fatal"
        if os.environ.get("THREAD_VECTORS_DEBUG"):
            print(f"  fatal in {prog['name']}: {e}")
    except UnmappedAccess:
        outcome = "crash"  # the exe dereferenced a null/garbage cell
    except OracleError as e:
        if "exception" in str(e).lower() or "Unicorn error" in str(e) or "unicorn error" in str(e):
            outcome = "trap"
        else:
            raise
    return sc.snapshot(outcome, raw)


# ---------------------------------------------------------------- programs


def n(op, imm=0, line=1):
    return [op, line, imm]


def V(t, w):
    return {"k": "v", "t": t, "w": w & 0xFFFFFFFF}


def VAR(t, w):
    return {"k": "var", "t": t, "w": w & 0xFFFFFFFF}


def LV(i):
    return {"k": "lv", "i": i}


def fbits(f):
    return struct.unpack("<I", struct.pack("<f", f))[0]


def lit_i(v):
    return [n(0x1007, 0x1C), n(0x100B, v & 0xFFFFFFFF)]


def lit_f(v):
    return [n(0x1007, 0x1C), n(0x200A, fbits(v))]


def lit_null():
    return [n(0x1007, 0x1C), n(0x000C, 0)]


def frame(nodes, entry=0, locals_=(), stack=()):
    return {"nodes": nodes, "entry": entry, "locals": list(locals_),
            "stack": list(stack)}


def prog(name, nodes, locals_=(), stack=(), budget=0.0, clock=()):
    return {"name": name, "budget": budget, "clock": list(clock),
            "frames": [frame(nodes, 0, locals_, stack)]}


STOP = n(0x0010)
FUZZ_SEEDS = 400

INT_OPS = [3, 4, 5, 0xD, 0xE, 0x11]
NUM_OPS = [1, 2, 6, 7, 8, 9, 0xA, 0xB, 0xF, 0x10, 0x12, 0x13]
OPERANDS = [
    ("ii", lit_i(7), lit_i(3)),
    ("ineg", lit_i(-9), lit_i(4)),
    ("ff", lit_f(1.5), lit_f(0.5)),
    ("if", lit_i(2), lit_f(2.5)),
    ("fi", lit_f(2.5), lit_i(2)),
    ("fnan", lit_f(float("nan")), lit_f(1.0)),
    ("ll", lit_null(), lit_null()),
]
UNARY_OPS = [0x14, 0x15, 0x16]


def programs():
    P = []
    # literals and pushes
    P.append(prog("lit_int", lit_i(5) + [STOP]))
    P.append(prog("lit_float", lit_f(-2.25) + [STOP]))
    P.append(prog("lit_null", lit_null() + [STOP]))
    P.append(prog("lit_class_i", [n(0x1007, 0x1C), n(0x100D, 9), STOP]))
    P.append(prog("push_int_1027", [n(0x1027, 42), n(0x1027, -1 & 0xFFFFFFFF), STOP]))
    P.append(prog("push_zeros_101f", [n(0x101F, 3), STOP]))
    P.append(prog("push_void_0029", [n(0x0029), STOP]))
    # locals
    L2 = [VAR("I", 11), VAR("F", fbits(0.75))]
    P.append(prog("local_push", [n(0x1001, 0), n(0x1001, 1), STOP], L2))
    P.append(prog("local_push_oob", [n(0x1001, 5), STOP], L2))
    P.append(prog("local_pop_1003", [n(0x1003), STOP], L2))
    P.append(prog("local_pop_twice", [n(0x1003), n(0x1003), STOP], L2))
    # pops
    P.append(prog("pop_0018_value", lit_i(1) + [n(0x0018), STOP]))
    P.append(prog("pop_0018_lvalue", [n(0x1001, 0), n(0x0018), STOP], L2))
    P.append(prog("pop_1008_1b", lit_i(1) + [n(0x1008, 0x1B), STOP]))
    P.append(prog("pop_1008_1b_lv", [n(0x1001, 1), n(0x1008, 0x1B), STOP], L2))
    P.append(prog("deref_0028_lv", [n(0x1001, 0), n(0x0028), STOP], L2))
    P.append(prog("deref_0028_value", lit_f(3.0) + [n(0x0028), STOP]))
    P.append(prog("noop_1007_1b", lit_i(8) + [n(0x1007, 0x1B), STOP]))
    P.append(prog("noop_1007_1d", lit_i(8) + [n(0x1007, 0x1D), STOP]))
    P.append(prog("noop_1007_22", lit_i(8) + [n(0x1007, 0x22), STOP]))
    P.append(prog("noop_1007_c", lit_i(8) + [n(0x1007, 0x0C), STOP]))
    P.append(prog("noop_1008_22", lit_i(8) + [n(0x1008, 0x22), STOP]))
    # binary operators, 0x1007 (push result) and 0x1008 (discard)
    for op in INT_OPS + NUM_OPS:
        for tag, a, b in OPERANDS:
            body = a + b + [n(0x1007, op), STOP]
            P.append(prog(f"binop_{op:#x}_{tag}", body))
        P.append(prog(f"binop_{op:#x}_discard",
                      lit_i(6) + lit_i(2) + [n(0x1008, op), STOP]))
        P.append(prog(f"binop_{op:#x}_lvalue_left",
                      [n(0x1001, 0)] + lit_i(2) + [n(0x1007, op), STOP], L2))
        P.append(prog(f"binop_{op:#x}_lvalue_right",
                      lit_i(2) + [n(0x1001, 0), n(0x1007, op), STOP], L2))
    # unary operators
    for op in UNARY_OPS:
        for tag, a in (("i", lit_i(5)), ("f", lit_f(1.5)), ("z", lit_i(0)),
                       ("n", lit_null())):
            P.append(prog(f"unop_{op:#x}_{tag}", a + [n(0x1007, op), STOP]))
        P.append(prog(f"unop_{op:#x}_lv", [n(0x1001, 0), n(0x1007, op), STOP], L2))
        P.append(prog(f"unop_{op:#x}_discard", lit_i(5) + [n(0x1008, op), STOP]))
    # ternary select (sub-op 0): [cond][a][b]
    for cond in (0, 1):
        P.append(prog(f"ternary_{cond}",
                      lit_i(cond) + lit_i(100) + lit_i(200) + [n(0x1007, 0), STOP]))
    P.append(prog("ternary_float_cond",
                  lit_f(0.0) + lit_i(100) + lit_i(200) + [n(0x1007, 0), STOP]))
    P.append(prog("ternary_discard",
                  lit_i(1) + lit_i(100) + lit_i(200) + [n(0x1008, 0), STOP]))
    # ++ / -- on locals, both opwords
    for op in (0x17, 0x18, 0x19, 0x1A):
        for li, tag in ((0, "i"), (1, "f")):
            P.append(prog(f"incdec_{op:#x}_{tag}",
                          [n(0x1001, li), n(0x1007, op), STOP], L2))
            P.append(prog(f"incdec_{op:#x}_{tag}_discard",
                          [n(0x1001, li), n(0x1008, op), STOP], L2))
        P.append(prog(f"incdec_{op:#x}_nonlvalue", lit_i(3) + [n(0x1007, op), STOP]))
    # assignment: 0x1007 (value below, lvalue on top) and 0x1008
    LA = [VAR("I", 5), VAR("F", fbits(2.5)), VAR("L", 0)]
    for kind in (0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E):
        for li in (0, 1):
            for tag, v in (("i", lit_i(3)), ("f", lit_f(1.5))):
                P.append(prog(f"assign_{kind:#x}_l{li}_{tag}",
                              v + [n(0x1001, li), n(0x1007, kind), STOP], LA))
        P.append(prog(f"assign_{kind:#x}_discard",
                      lit_i(3) + [n(0x1001, 0), n(0x1008, kind), STOP], LA))
        P.append(prog(f"assign_{kind:#x}_nonlvalue",
                      lit_i(3) + lit_i(4) + [n(0x1007, kind), STOP], LA))
    P.append(prog("assign_ref_null", lit_null() + [n(0x1001, 2), n(0x1007, 0x23), STOP], LA))
    # plain assignment opword 0x002d: [lvalue][value]
    for li in (0, 1):
        for tag, v in (("i", lit_i(9)), ("f", lit_f(-0.5))):
            P.append(prog(f"assign_002d_l{li}_{tag}",
                          [n(0x1001, li)] + v + [n(0x002D), STOP], LA))
    P.append(prog("assign_002d_value_lvalue",
                  [n(0x1001, 1), n(0x1001, 0), n(0x002D), STOP], LA))
    P.append(prog("assign_002d_nonlvalue", lit_i(1) + lit_i(2) + [n(0x002D), STOP], LA))
    # jumps and conditional jumps
    P.append(prog("jump_fwd", [n(0x1014, 2)] + lit_i(1) + lit_i(2) + [STOP]))
    P.append(prog("jump_skip_one", [n(0x1014, 1), n(0x1027, 7), n(0x1027, 8), STOP]))
    for tag, cond in (("i0", lit_i(0)), ("i1", lit_i(1)), ("f0", lit_f(0.0)),
                      ("f1", lit_f(0.5)), ("null", lit_null()),
                      ("nzero", lit_f(-0.0))):
        for op in (0x1015, 0x1016):
            P.append(prog(f"condjump_{op:#x}_{tag}",
                          cond + [n(op, 1), n(0x1027, 1), n(0x1027, 2), STOP]))
    P.append(prog("condjump_lvalue", [n(0x1001, 0), n(0x1015, 1), n(0x1027, 1), n(0x1027, 2), STOP], L2))
    # switch compare 0x1017: [switch][case]
    for tag, a, b in (("match", lit_i(4), lit_i(4)), ("miss", lit_i(4), lit_i(5)),
                      ("float", lit_f(1.0), lit_f(1.0)), ("mixed", lit_i(1), lit_f(1.0))):
        P.append(prog(f"switch_{tag}", a + b + [n(0x1017, 1), n(0x1027, 77), n(0x1027, 78), STOP]))
    # short circuit
    for op in (0x0004, 0x0005):
        for tag, a in (("i0", lit_i(0)), ("i1", lit_i(1)), ("i5", lit_i(5)),
                       ("f0", lit_f(0.0)), ("f2", lit_f(2.0)), ("null", lit_null())):
            P.append(prog(f"shortcircuit_{op:#x}_{tag}",
                          a + [n(op), n(0x1027, 1), n(0x1027, 2), STOP]))
    P.append(prog("shortcircuit_lvalue", [n(0x1001, 1), n(0x0004), n(0x1027, 1), n(0x1027, 2), STOP], L2))
    # fatal
    P.append(prog("fatal_op0", [n(0x0000), STOP]))
    P.append(prog("fatal_op13", [n(0x0013), STOP]))
    P.append(prog("fatal_literal_bad", [n(0x1007, 0x1C), n(0x1027, 1), STOP]))
    # integer traps (logged, then idiv #DE)
    P.append(prog("trap_div0", lit_i(5) + lit_i(0) + [n(0x1007, 0x12), STOP]))
    P.append(prog("trap_mod0", lit_i(5) + lit_i(0) + [n(0x1007, 0x11), STOP]))
    # float / 0.0 does not trap
    P.append(prog("fdiv0", lit_f(1.0) + lit_f(0.0) + [n(0x1007, 0x12), STOP]))
    # stop leaves pc on the stop node; nodes after it never run
    P.append(prog("stop_only", [STOP, n(0x1027, 1)]))
    P.append(prog("stop_mid", [n(0x1027, 5), STOP, n(0x1027, 6)]))
    # structural opwords are silent advances
    for op in (0x1006, 0x100B, 0x100C, 0x100F, 0x4012, 0x101A, 0x1019, 0x002C, 0x402E):
        P.append(prog(f"structural_{op:#x}", [n(op, 3), n(0x1027, 1), STOP]))
    # long straight-line program (more than 256 iterations)
    P.append(prog("long_run", [n(0x1027, 1), n(0x1008, 0x1B)] * 300 + [STOP]))
    # budgeted runs (clock hook: start, then each periodic check)
    P.append(prog("budget_expires", [n(0x1027, 1), n(0x1008, 0x1B)] * 20 + [STOP],
                  budget=5.0, clock=[0.0, 10.0, 99.0]))
    P.append(prog("budget_not_expired", [n(0x1027, 1), n(0x1008, 0x1B)] * 20 + [STOP],
                  budget=5.0, clock=[0.0, 1.0, 99.0]))
    P.append(prog("budget_long", [n(0x1027, 1), n(0x1008, 0x1B)] * 300 + [STOP],
                  budget=5.0, clock=[0.0, 1.0, 2.0, 3.0, 9.0, 99.0]))
    # returns with two frames: base frame (caller) + callee on top
    P.extend(call_programs())
    for seed in range(FUZZ_SEEDS):
        P.append(fuzz_program(seed))
    P.extend(extra_programs())
    return P


def call_programs():
    out = []
    callee_nodes = [n(0x1027, 5), n(0x002A), n(0x1027, 99)]
    caller_nodes = [n(0x0029), n(0x1027, 1), n(0x1027, 2), STOP]
    # caller saved pc = node 0 (the "call" node); returning resumes at node 1
    out.append({"name": "return_void", "budget": 0.0, "clock": [],
                "frames": [frame(caller_nodes, 0, [VAR("I", 1)], [V("I", 7)]),
                           frame(callee_nodes, 0, [VAR("F", fbits(2.0))], [])]})
    callee_val = [n(0x1027, 5), n(0x002B), n(0x1027, 99)]
    out.append({"name": "return_value", "budget": 0.0, "clock": [],
                "frames": [frame(caller_nodes, 0, [], []),
                           frame(callee_val, 0, [VAR("I", 3)], [])]})
    callee_lv = [n(0x1001, 0), n(0x002B), n(0x1027, 99)]
    out.append({"name": "return_lvalue_value", "budget": 0.0, "clock": [],
                "frames": [frame(caller_nodes, 0, [], []),
                           frame(callee_lv, 0, [VAR("F", fbits(4.5))], [V("I", 2), V("F", fbits(1.0))])]})
    # return from the last frame: caller pc null -> thread stops (flag 0x40)
    out.append({"name": "return_to_null_pc", "budget": 0.0, "clock": [],
                "frames": [{"nodes": [STOP], "entry": 0, "locals": [], "stack": [],
                            "null_pc": True},
                           frame([n(0x1027, 1), n(0x002A)], 0, [VAR("I", 3)], [V("I", 9)])]})
    # three frames, return twice
    out.append({"name": "return_twice", "budget": 0.0, "clock": [],
                "frames": [frame([n(0x1027, 1), n(0x1027, 2), STOP], 0, [], []),
                           frame([n(0x1027, 3), n(0x1027, 0), n(0x002A), n(0x1027, 5)], 1, [], []),
                           frame([n(0x1027, 6), n(0x002A)], 0, [VAR("I", 8)], [])]})
    return out



# ---------------------------------------------------------------- fuzz


def fuzz_program(seed, steps=24):
    """Random straight-line / forward-jump program with a simulated operand
    stack so that pops never underflow. Stack kinds: I, F, L, lv(local)."""
    import random
    r = random.Random(seed)
    locs = [VAR("I", r.choice([0, 1, -5, 7, 2**31 - 1])),
            VAR("F", fbits(r.choice([0.0, 1.5, -2.25, 100.0]))),
            VAR("L", 0)]
    lkind = ["I", "F", "L"]
    nodes, st = [], []  # st: kinds on the stack

    def push_lit():
        k = r.choice("IIIFFL")
        if k == "I":
            nodes.extend(lit_i(r.choice([0, 1, 2, 3, -1, 7, 100, 2**31 - 1, -2**31, 31, 32])))
        elif k == "F":
            nodes.extend(lit_f(r.choice([0.0, -0.0, 0.5, 1.5, -2.0, 3.25, 1e10, float("nan"), float("inf")])))
        else:
            nodes.extend(lit_null())
        st.append(k)

    def push_local():
        i = r.randrange(3)
        nodes.append(n(0x1001, i))
        st.append("lv%d" % i)

    def kind(x):
        return lkind[int(x[2:])] if x.startswith("lv") else x

    for _ in range(steps):
        c = r.random()
        d = len(st)
        if d < 2 or c < 0.25:
            (push_lit if r.random() < 0.6 else push_local)()
        elif c < 0.55:
            # binary operator on the top two entries
            a, b = kind(st[-2]), kind(st[-1])
            ops = [1, 2, 6, 7]
            if a in "IF" and b in "IF":
                ops += [3, 4, 5, 8, 9, 0xA, 0xB, 0xD, 0xE, 0xF, 0x10, 0x11, 0x12, 0x13]
            elif a == b == "L":
                ops += [8, 9, 0xA, 0xB]
            op = r.choice(ops)
            disc = r.random() < 0.2
            nodes.append(n(0x1008 if disc else 0x1007, op))
            st[-2:] = [] if disc else [("I" if op in (1, 2, 6, 7, 8, 9, 0xA, 0xB) else a)]
            if not disc and op in (0xD, 0xE):
                st[-1] = b if False else st[-1]
        elif c < 0.65:
            # unary on the top entry
            op = r.choice([0x14, 0x15, 0x16])
            if kind(st[-1]) == "L" and op != 0x15:
                op = 0x15
            nodes.append(n(0x1007, op))
            st[-1] = "I" if op == 0x15 else kind(st[-1])
        elif c < 0.78:
            # assignment: value below, lvalue on top
            i = r.randrange(2)  # never assign into the L local with arithmetic
            if r.random() < 0.5:
                nodes.extend(lit_i(r.choice([0, 3, -4, 9])))
                vk = "I"
            else:
                nodes.extend(lit_f(r.choice([0.5, 2.0, -1.25])))
                vk = "F"
            nodes.append(n(0x1001, i))
            kinds = [0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2C, 0x2D, 0x2E]
            k = r.choice(kinds)
            disc = r.random() < 0.3
            nodes.append(n(0x1008 if disc else 0x1007, k))
            if not disc:
                st.append(vk)
        elif c < 0.85:
            # ++/-- on a numeric local
            i = r.randrange(2)
            op = r.choice([0x17, 0x18, 0x19, 0x1A])
            disc = r.random() < 0.3
            nodes.append(n(0x1001, i))
            nodes.append(n(0x1008 if disc else 0x1007, op))
            if not disc:
                st.append(lkind[i])
        elif c < 0.9:
            # pop one
            if r.random() < 0.5:
                nodes.append(n(0x0018))
            else:
                nodes.append(n(0x1008, 0x1B))
            st.pop()
        elif c < 0.95:
            # conditional skip over a balanced [push, pop] pair
            cond = r.choice([lit_i(0), lit_i(1), lit_f(0.0), lit_f(2.5), lit_null()])
            nodes.extend(cond)
            nodes.append(n(r.choice([0x1015, 0x1016]), 3))
            nodes.append(n(0x1027, 5))
            nodes.append(n(0x0018))
        else:
            # ternary
            nodes.extend(lit_i(r.choice([0, 1])))
            nodes.extend(lit_i(r.randrange(10)))
            nodes.extend(lit_f(1.5) if r.random() < 0.5 else lit_i(4))
            nodes.append(n(0x1007, 0))
            st.append("I")
    nodes.append(STOP)
    return prog(f"fuzz_{seed}", nodes, locs)



# ---------------------------------------------------------------- extra


KNOWN_DIVERGENCE = (
    "assigning an I/F value into an L-typed variable: the exe converts "
    "through the class loader (FUN_00417da0 -> FUN_00411610/4044e0/4174a0) "
    "and crashes under Unicorn; the port logs and copies the raw word "
    "(ticket #31; docs/fidelity.md)")


def extra_programs():
    """Second review pass (177 oracle-compared programs + 14 known-divergence
    cases): references, underflow and null paths, lvalue ternaries, kinds x
    source types x destination types for assignments, backward jumps, budgets
    (negative, tiny, infinite loop), float-to-int edge conversions, shifts on
    non-integers. Rows whose name matches a known-divergence case carry the
    `known_divergence` marker and are not compared by the Rust test."""
    P = []
    L2 = [VAR("I", 11), VAR("F", fbits(0.75))]
    LR = [VAR("L", 0), VAR("I", 3), VAR("F", fbits(1.0))]
    P.append(prog("x_1008_1c_stmt_literal", [n(0x1008,0x1C), n(0x100B,5), n(0x1027,1), STOP]))
    P.append(prog("x_1008_20", lit_i(1)+[n(0x1008,0x20), n(0x1006,0), n(0x1027,9), STOP]))
    P.append(prog("x_1008_21", lit_i(1)+[n(0x1008,0x21), n(0x1006,0), n(0x1027,9), STOP]))
    P.append(prog("x_pop_empty_0018", [n(0x0018), STOP]))
    P.append(prog("x_pop_local_x3", [n(0x1003)]*3+[STOP], L2))
    P.append(prog("x_binop_underflow", lit_i(1)+[n(0x1007,0x10), STOP]))
    P.append(prog("x_ret_only_frame", [n(0x1027,1), n(0x002A), STOP]))
    P.append(prog("x_ret_value_only_frame", [n(0x1027,1), n(0x002B), STOP]))
    # ternary with lvalues
    P.append(prog("x_ternary_lv_all", [n(0x1001,0), n(0x1001,1), n(0x1001,0), n(0x1007,0), STOP], L2))
    P.append(prog("x_ternary_lv_cond", [n(0x1001,0)]+lit_i(5)+lit_f(2.0)+[n(0x1007,0), STOP], L2))
    P.append(prog("x_ternary_discard_lv", [n(0x1001,0), n(0x1001,1), n(0x1001,0), n(0x1008,0), STOP], L2))
    # refs nonzero
    for op in (6,7,8,9,0xA,0xB,1,2,0x10,0xF,0x12,3):
        for a,b in ((5,5),(5,6)):
            P.append({"name":f"x_refcmp_{op:#x}_{a}_{b}","budget":0.0,"clock":[],
                "frames":[frame([n(0x1007,op), STOP],0,[],[V("L",a),V("L",b)])]})
        P.append({"name":f"x_ref_int_{op:#x}","budget":0.0,"clock":[],
                "frames":[frame([n(0x1007,op), STOP],0,[],[V("L",5),V("I",5)])]})
        P.append({"name":f"x_int_ref_{op:#x}","budget":0.0,"clock":[],
                "frames":[frame([n(0x1007,op), STOP],0,[],[V("I",5),V("L",5)])]})
        P.append({"name":f"x_ref_float_{op:#x}","budget":0.0,"clock":[],
                "frames":[frame([n(0x1007,op), STOP],0,[],[V("L",5),V("F",fbits(2.0))])]})
    for k in (0x23,0x24,0x25,0x26,0x27,0x28,0x2c,0x2d,0x2e):
        for src in (V("L",0x1234), V("I",7), V("F",fbits(2.5))):
            for li in (0,1,2):
                P.append({"name":f"x_assign_{k:#x}_l{li}_{src['t']}","budget":0.0,"clock":[],
                  "frames":[frame([n(0x1001,li), n(0x1007,k), STOP],0,LR,[src])]})
    for op in (0x0004,0x0005,0x1015,0x1016,0x14,0x15,0x16):
        P.append({"name":f"x_ref_unary_{op:#x}","budget":0.0,"clock":[],
            "frames":[frame([n(op if op>0xff else op,0) if op!=0x14 else n(0x1007,0x14), n(0x1027,1), n(0x1027,2), STOP],0,[],[V("L",9)])]})
    for op in (0x14,0x15,0x16):
        P.append({"name":f"x_ref_unop_{op:#x}","budget":0.0,"clock":[],
            "frames":[frame([n(0x1007,op), STOP],0,[],[V("L",9)])]})
    # backward-jump loop: i=local0; loop: i++ ; if i<5 jump back
    loop=[n(0x1001,0), n(0x1008,0x17), n(0x1001,0), n(0x1027,5), n(0x1007,0xB), n(0x1016,(-5)&0xFFFFFFFF), STOP]
    P.append(prog("x_loop_back", loop, [VAR("I",0)]))
    inf=[n(0x1027,1), n(0x1008,0x1B), n(0x1014,(-2)&0xFFFFFFFF), STOP]
    P.append(prog("x_inf_loop_budget", inf, budget=5.0, clock=[0.0,1.0,2.0,3.0,99.0]))
    P.append(prog("x_budget_negative", inf, budget=-1.0, clock=[0.0,0.0,5.0]))
    P.append(prog("x_budget_tiny_f32", inf, budget=1e-30, clock=[1e9,1e9+1e-9,1e9]))
    P.append(prog("x_101f_zero", [n(0x101F,0), STOP]))
    P.append(prog("x_switch_ref", [n(0x1017,1), n(0x1027,1), n(0x1027,2), STOP]))
    P[-1]["frames"][0]["stack"]=[V("L",5),V("L",5)]
    P.append(prog("x_cond_float_nan", lit_f(float('nan'))+[n(0x1015,1), n(0x1027,1), STOP]))
    # shifts with float / L
    for op in (0xD,0xE):
        P.append(prog(f"x_shift_{op:#x}_fi", lit_f(2.5)+lit_i(3)+[n(0x1007,op), STOP]))
        P.append(prog(f"x_shift_{op:#x}_if", lit_i(2)+lit_f(3.5)+[n(0x1007,op), STOP]))
        P.append(prog(f"x_shift_{op:#x}_neg", lit_i(-1)+lit_i(0x80000000)+[n(0x1007,op), STOP]))
        P.append(prog(f"x_shift_{op:#x}_big", lit_i(100)+lit_i(1)+[n(0x1007,op), STOP]))
    # int/float conversions to int via assign of huge floats
    for fv in (1e10, -1e10, 3e9, float('nan'), float('inf'), 2147483648.0, -2147483649.0):
        P.append({"name":f"x_assign_f2i_{fv}","budget":0.0,"clock":[],
            "frames":[frame(lit_f(fv)+[n(0x1001,0), n(0x1007,0x23), STOP],0,[VAR("I",1)],[])]})
        P.append(prog(f"x_promote_i_f_{fv}", lit_i(7)+lit_f(fv)+[n(0x1007,0x10), STOP]))
    for p in P:
        # kinds that store through the L-typed local 0 from an I or F source
        n_ = p["name"]
        if n_.startswith("x_assign_0x") and "_l0_" in n_ and n_.endswith(("_I", "_F")):
            kind = int(n_.split("_")[2], 16)
            if kind not in (0x24, 0x25):
                p["known_divergence"] = KNOWN_DIVERGENCE
    return P


# ---------------------------------------------------------------- driver


def gen(exe=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = []
    for i, p in enumerate(programs()):
        res = run_program(p, exe)
        rows.append({
            "id": i, "name": p["name"], "exe_addr": f"0x{RUN:08x}",
            "budget": p.get("budget", 0.0), "clock": p.get("clock", []),
            "frames": p["frames"], "result": res,
            **({"known_divergence": p["known_divergence"]}
               if "known_divergence" in p else {}),
            "oracle": "unicorn", "unicorn": UC_VER, "cw": f"0x{CW:04x}",
            "date": _today()})
    path = os.path.join(OUT_DIR, "thread_run.jsonl")
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    outcomes = {}
    for r in rows:
        outcomes[r["result"]["outcome"]] = outcomes.get(r["result"]["outcome"], 0) + 1
    print(f"{len(rows)} programs -> {path} outcomes={outcomes}")


def verify(paths, exe=None):
    bad = n_ = 0
    for path in paths:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        for row in rows:
            n_ += 1
            got = run_program({"name": row["name"], "frames": row["frames"],
                               "budget": row["budget"], "clock": row["clock"]}, exe)
            if got != row["result"]:
                bad += 1
                print(f"MISMATCH {row['name']}: want {row['result']} got {got}")
    print(f"verify: {n_ - bad}/{n_} byte-identical")
    return 1 if bad else 0


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe)
        return 0
    files = a.files or sorted(os.path.join(OUT_DIR, f) for f in os.listdir(OUT_DIR)
                              if f.endswith(".jsonl"))
    return verify(files, a.exe)


if __name__ == "__main__":
    sys.exit(main())
