#!/usr/bin/env python3
"""Unicorn vectors for the resource handle natives (M1-11, issue #38).

Runs the exe's `java.util.resource.*` handle natives directly with hand-built
argument frames in emulator memory (no JVM boot needed). This file is the
harness skeleton; the scenario groups are added per ledger row:

  anim  Animation queue setters (H1):
          0x47ec00 setSpeed(F)   0x47ecc0 setFade(F)   0x47ed80 play()
          0x47ee30 loopPlay()    0x47eee0 pause()      0x47ef90 seek(F)

Native shape (machine-code read, `re/out/res_disasm.txt`):

- Argument frame F: [+8] -> operand-stack container {cap, data, count} of
  value-cell pointers; [+0xc] -> method info M with [M+0xc] class name,
  [M+0x10] method name, [M+0x14] call info C; [C+4] -> signature cstr.
  `this` is the TOP cell (index count-1) and the declared arguments sit
  below it (0x45d910 pops them from the signature cstr).
- Value cell V: [V] = 0x5e734c, [V+4] -> type cstr ('I' 0x60ab50, 'F'
  0x60ab58, 'L' 0x60ab5c), [V+8] = payload word. `this` carries the game
  object pointer; float arguments carry their IEEE-754 bits.
- Field chain: the natives call 0x42a690(obj, "obj", <class>) to resolve the
  object's `obj` field, then 0x42ab50(handle, DAT_0062e008=3) to read it;
  both ends are the game's real class tables, which need the JVM. The two
  calls are therefore hooked:
    * 0x42a690 -> 1 (the nested-object wrapper is irrelevant: the next read
      is hooked too);
    * 0x42ab50 -> the fake RenderRef object planted by the scenario.
  The value read from the field is used as `[eax]`/`[eax+8]` by the real
  exe code, so the second 0x42ab50 must return a real POINTER, never the
  literal 1: `mov ecx, [eax+8]` is the "handle exists" flag test (the
  documented `unmapped read at 0x9` blocker was exactly this).
- Animation object A: [A+0x9c] queue buffer, [A+0xa0] count, [A+0xa4]
  capacity. When count == capacity the exe reallocs `(capacity+0x20)*8`
  bytes through 0x54f580 and stores the new pointer back at +0x9c; the new
  entry `[tag, payload]` is written at `buf + count*8` and count is bumped.
  [A+8] = handle-exists flag: zero makes every setter return without a push.

Other boundaries replaced by hooks (everything else is real exe code,
including 0x45d910 argument popping and the setters' own queue logic):
- allocator 0x54f560 / free 0x54f5b0 (bump allocator, zeroed) and realloc
  0x54f580 (copy-on-grow);
- log sinks 0x551140 / 0x551220 / 0x5513b0 (no output modelled);
- envelope gate 0x5447d0 -> 0 (pass);
- `DAT_0062e008`=3 and a heap cell holding the 'I' type cstr at
  `DAT_0062e00c`, exactly as the other oracle harnesses plant them.

Row (JSONL under re/out/res/, git-ignored):
  {"id","exe_addr","name","convention","sig","args","result","oracle",
   "unicorn","cw","date"}
`args` is the scenario: {"pre": [[tag,payload],...], "cap": capacity,
"exists": 0|1, "f32_bits": float bits}. `result` is
{"count", "capacity", "queue": [[tag,payload],...]} read back from the
object's +0xa0/+0xa4/+0x9c after the call. The exe grows the queue by
0x20 entries; capacity is recorded for reference only (the Rust port models
the queue as a `Vec`), while count and queue are the compared facts.

ParticleSystem group (`psys_*`, H2). The natives reach the live particle
object through a fixed chain that the harness models with real pointers:

    this -> [env+0xc] (ref) -> vtable[3] (0xc) -> envelope -> [+0x18] particle
    ref+0x4c == 1 skips the extra vtable[5] (0x14) "materialise" call
    envelope fetch 0x419860 (arg 0xa0000001) -> the same envelope

`env` is what the 0x42ab50 hook returns (the unboxed int); `ref` is its
`+0xc` object with `+0x4c`=1 and a scratch vtable whose slot 3 and slot 5
both point at one stub returning the envelope; the envelope's `+0x18` is
the particle object.

Action list. `particle+0x24` holds the head node, `+4` next, `+8` prev; the
walk in getCounter/delAction starts AT that node (so its own `+0x14` label is
compared first) and only compares a node whose `+4` is nonzero. The exe's
own append splices a circular list around the sentinel embedded at
`particle+0x2c`, which the guard cannot terminate on a miss, so the harness
links `S -> n0 -> ... -> nk -> T` with a trailing terminator `T` (`+4`=0):
every real node has a nonzero `+4` (so all are compared, in link order) and
the miss walk falls out at `T`. Nodes are `+0x10` type id, `+0x14` inline
NUL-terminated label (compared with the exe's case-insensitive `0x5d7190`),
`+0x58` counter word, `+0x54` rate, `+0x60` domain pointer.

Argument cells. The exe's popper 0x45d910 pops the container top-down and
writes `this` first, then the declared arguments, so the container is
`[argN, ..., arg1, this]` (`this` is index `count-1`). Pinning that order
needed both call-site cleanups to be right: the envelope gate 0x5447d0
cleans its 3 stack words and 0x419860 its 4 (the natives read arguments
from esp-relative slots they hold besides the popper outputs, so a
"cdecl" no-cleanup hook shifted every later read by 0xc/0x10 and made
delAction/setCounter read garbage). `setCounter`'s three 0x42a560 field
reads (x/y/z off the Vector3) and its rate are modelled by hooks:
0x42a560 returns the requested component bits as an ST0 float.

Domain words. setCounter builds the `0x20`-byte domain (vtable `0x5f113c`)
by `fld rate; fld dword [0x5e73cc]` (=0.0f); `fcomp st(1)` sets C0 when
ST0<ST1 and C3 when equal/unordered, and the `test ah,0x41` branch stores
the pair swapped when `rate < 0` (ordered) and unswapped otherwise, i.e.
`ext = (rate, 0)` for `rate >= 0`/NaN and `(0, rate)` for `rate < 0`; the
squares are x87 PC=64 products. Append writes a fresh node with `+0x54`
and `+0x58` zero and the new domain at `+0x60`; reuse only swaps the domain
at `+0x60` and leaves `+0x54`/`+0x58` alone; a matched node whose type is
not `0xd` aborts setCounter without appending; getCounter stops at the
first LABEL match and returns 0 unless that node is type `0xd`.

Row `args` for this group is {"env": {"mode": 0|1, "freq_bits": w,
"flags": w}, "pre": [actions], "call": {...}}; `result` is the observed
object/list projection described per scenario (`run_psys`, compared facts
are the port's `HandleEntry.flags`/`freq_bits` and its action list, not the
exe's pointer identities: nodes are reported in link order as
{"type","label","counter","rate","domain":[ext0,ext1,sq0,sq1]}).

Handle ops (`handles`, H4). Six resource handle natives driven through their
real frames, with only the engine entry point hooked (it needs a live D3D9
device), so what the vectors pin is each native's own forwarding: the word
list that reaches the engine and what the native answers.

  0x480220 changeResource  0x480310 setColor   0x486ab0 setLight
  0x486ba0 setType         0x47fe70 lineCreate 0x481020 getBoneId

`changeResource` dispatches through the ref's vtable (+0xc resolve, +0x14
materialise, +8 store) rather than one engine entry, so its rows pin that
call sequence; `+0x4c == 1` skips the materialise call.

Three ops are deliberately NOT in the group: `render_create` (0x480ee0),
`setFlare` (0x486b20) and `phys_create` (0x4807f0). Their arms fault on
vtable slots the fake objects do not carry (`eip=0x0`, or a planted heap
pointer executed), so their ledger rows stay `ported (no test yet)` instead
of being claimed by vectors that never reached the engine path.

Gate/constant group (`gate`, H3). Five natives whose port targets are the
constant helpers, each row recording the exact fact the helper reproduces:

  0x480d40 SfxRef.nplay   -> sfx_target_in_range / sfx_scaled_volume
  0x480e60 SfxRef.stop    -> sfx_stop (handle + channel passthrough)
  0x486be0 getTypeID      -> render_type_gate (mode must be 3)
  0x480440 getDetail      -> render_detail (mode 1|3 and obj word
                             nonzero, else the 0.0f default at 0x5e73cc)
  0x480920 setMatrix      -> phys_slot_addr (body + slot*0xd4; position
                             fields at +0x64, orientation at +0xa4)

nplay's tail 0x48cfb0 is called with the eight pushed words
`[arg3, DAT_00612c58*arg4, arg5, vector?, arg6, arg2, 0, 0]`: the Vector3
word is the caller's Vector3 copy only when that argument is non-null — the
`id & 0xffff` comparison at 0x480dd3 has no effect on it. `unit_bits` in
`args` is planted into DAT_00612c58 so the volume product is exercised.

getTypeID's result is `[envelope+0x80]` (0 when the 0x419860 envelope fetch
returns null); the fetch only runs when the env word `[obj+0xc]` is nonzero
and `[env+0x4c] == 3`. setMatrix writes the same position to two slots
(`slot` and `slot^1`), so slot 2/3 (whose fields alias the index word at
`body+0x20c`) are omitted rather than corrupting the index mid-call.
"""
from __future__ import annotations
import argparse
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import Oracle, CW_D3D9  # noqa: E402
import unicorn  # noqa: F401

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "res")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"

VAL_VT = 0x5E734C
TC = {"I": 0x60AB50, "F": 0x60AB58, "L": 0x60AB5C}
FIELD_IDX = 3          # value planted in DAT_0062e008 (the `handle` field id)
FIELD_CELL_COUNT = 5   # fields in the fake class struct
CW = CW_D3D9
UC_VER = unicorn.__version__


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        p = os.path.join(root, "reference", "raw",
                         "StreetLegal_Redline.bak.exe")
        if p and os.path.exists(p):
            return p
    return os.path.join("reference", "raw", "StreetLegal_Redline.bak.exe")


def _alloc_cstr(o: Oracle, s: bytes) -> int:
    p = o.alloc(len(s) + 16)
    o.write(p, s + b"\0")
    return p


def _cell(o: Oracle, ty: str, w: int) -> int:
    p = o.alloc(12)
    o.write_u32(p, VAL_VT)
    o.write_u32(p + 4, TC[ty])
    o.write_u32(p + 8, w & 0xFFFFFFFF)
    return p


def _container(o: Oracle, words: list) -> int:
    c = o.alloc(12)
    data = o.alloc(max(4 * len(words), 4))
    for i, w in enumerate(words):
        o.write_u32(data + 4 * i, w)
    o.write_u32(c, len(words))
    o.write_u32(c + 4, data)
    o.write_u32(c + 8, len(words))
    return c


def _frame(w, sig: bytes, stack_cells: list, cls: bytes = b"java.util.resource.Animation") -> int:
    """Argument frame: container, method info (class/method/callinfo), frame."""
    o = w.o
    stack = _container(o, stack_cells)
    sigp = _alloc_cstr(o, sig)
    callinfo = o.alloc(0x20)
    o.write_u32(callinfo + 4, sigp)
    method = o.alloc(0x40)
    o.write_u32(method + 0xC, _alloc_cstr(o, cls))
    o.write_u32(method + 0x10, _alloc_cstr(o, b"native"))
    o.write_u32(method + 0x14, callinfo)
    frame = o.alloc(0x40)
    o.write_u32(frame + 8, stack)
    o.write_u32(frame + 0xC, method)
    return frame


def _realloc(oc: Oracle, args: list) -> int:
    old, size = args[0], args[1]
    new = oc.alloc(max(size, 1))
    if old:
        n = oc.heap_sizes.get(old, 0)
        if n:
            oc.write(new, oc.read(old, min(n, size)))
    return new


class World:
    """One fresh Oracle plus the hook state for an Animation or ParticleSystem
    scenario.

    The anim group plants its `Animation` object via `_anim_obj`; the psys
    group drives the real `env -> ref -> envelope -> particle` chain built
    here (`this` = what 0x42ab50 returns).
    """

    def __init__(self, exe=None, exists: int = 1, mode: int = 1):
        self.o = Oracle(exe or _default_exe())
        self.exists = exists
        self.mode = mode
        self.obj = 0
        self.term = 0
        self.field_reads = []
        # Gate/constant group state (inert for the anim/psys groups, whose
        # natives never reach these addresses).
        self.engine = 0
        self.obj_components = {}
        self.fetch_calls = 0
        self.sfx_play_calls = []
        self.sfx_stop_calls = []
        self.copy_dests = []
        o = self.o
        o.hook_alloc()
        o.hook(0x54F580, _realloc, "cdecl", 2)
        o.hook(0x551140, lambda oc, a: None, "cdecl", 3)
        o.hook(0x551220, lambda oc, a: None, "cdecl", 3)
        o.hook(0x5513B0, lambda oc, a: None, "cdecl", 0)
        # The envelope gate cleans its 3 stack words (the natives then read
        # args from esp-relative slots of their own), so the hook must too.
        o.hook(0x5447D0, lambda oc, a: 0, "thiscall", 3)
        o.hook(0x42A690, lambda oc, a: 1, "cdecl", 3)
        o.hook(0x42AB50, self._h_obj_field, "cdecl", 2)
        # Gate group boundaries: the engine tails (sfx play/stop, physics
        # body fetch and the matrix/orientation helpers) are recorded here
        # instead of executing.
        o.hook(0x48CFB0, self._h_sfx_play, "thiscall", 8)
        o.hook(0x48D180, self._h_sfx_stop, "thiscall", 1)
        o.hook(0x48AEA0, self._h_phys_engine, "thiscall", 0)
        o.hook(0x551C70, lambda oc, a: None, "thiscall", 3)
        o.hook(0x54ECD0, self._h_copy, "thiscall", 1)
        o.hook(0x54FF20, lambda oc, a: None, "thiscall", 1)
        o.hook(0x4986F0, lambda oc, a: None, "cdecl", 1)
        # DAT_0062e00c -> an 'I' type cstr cell, DAT_0062e008 = field index
        d = o.alloc(4)
        o.write_u32(d, TC["I"])
        o.write_u32(0x62E00C, d)
        o.write_u32(0x62E008, FIELD_IDX)
        self._build_psys_chain()

    # ------------------------------------------------------- hook bodies
    def _h_obj_field(self, oc: Oracle, args: list) -> int:
        return self.obj

    def _h_sfx_play(self, oc: Oracle, args: list) -> int:
        """0x48cfb0 (SfxRef.nplay tail): record this + the 8 pushed words."""
        self.sfx_play_calls.append({"handle": oc.reg("ecx"), "args": list(args)})
        return 0

    def _h_sfx_stop(self, oc: Oracle, args: list) -> None:
        """0x48d180 (SfxRef.stop tail): record this + the channel word."""
        self.sfx_stop_calls.append({"handle": oc.reg("ecx"), "channel": args[0]})

    def _h_phys_engine(self, oc: Oracle, args: list) -> int:
        """0x48aea0 (physics body fetch): the planted fake engine."""
        return self.engine

    def _h_copy(self, oc: Oracle, args: list) -> None:
        """0x54ecd0 (12-byte orientation copy): record the destination."""
        self.copy_dests.append(oc.reg("ecx"))

    def _h_envelope(self, oc: Oracle, args: list) -> int:
        """Vtable slot 3/5 of the ref, and the 0x419860 fetch: the envelope."""
        self.fetch_calls += 1
        return self.envelope

    def _h_component(self, oc: Oracle, args: list) -> float:
        """0x42a560(obj, "x"|"y"|"z"|...) -> the component as ST0.

        The setMatrix native reads two objects (a Vector3 with x/y/z and a
        Ypr with y/p/r), so a per-object map is consulted before the flat
        psys `components` map.
        """
        name = oc.read_cstr(args[1], 8).decode("latin1")
        self.field_reads.append((args[0], name))
        if args[0] in self.obj_components:
            return self.obj_components[args[0]].get(name, 0.0)
        return self.components.get(name, 0.0)

    def _build_psys_chain(self) -> None:
        o = self.o
        self.components = {"x": 0.0, "y": 0.0, "z": 0.0}
        # One scratch stub: a `ret` byte whose address the code hook
        # intercepts (keep the byte valid in case it ever executes).
        self.stub = o.alloc(0x10)
        o.write(self.stub, b"\xc3")
        o.hook(self.stub, self._h_envelope, "thiscall", 1)
        o.hook(0x419860, self._h_envelope, "stdcall", 4)
        o.hook(0x42A560, self._h_component, "cdecl", 2)
        ref = o.alloc(0x60)
        vt = o.alloc(0x20)
        o.write_u32(vt + 0x0C, self.stub)
        o.write_u32(vt + 0x14, self.stub)
        o.write_u32(ref, vt)
        o.write_u32(ref + 0x4C, self.mode)
        env = o.alloc(0x40)
        o.write_u32(env + 0x0C, ref)
        envl = o.alloc(0x40)
        part = o.alloc(0x100)
        o.write_u32(envl + 0x18, part)
        # Action list: head pointer at +0x24, sentinel node at +0x2c.
        s = part + 0x2C
        o.write_u32(part + 0x24, s)
        o.write_u32(s + 4, s)
        o.write_u32(s + 8, s)
        self.env, self.ref, self.envelope, self.particle = env, ref, envl, part
        self.obj = env

    # ------------------------------------------------------- psys builder
    def particle_obj(self, flags: int = 0, freq_bits: int = 0) -> int:
        """Fresh particle object with the list root reset."""
        o = self.o
        p = o.alloc(0x100)
        s = p + 0x2C
        o.write_u32(p + 0x0C, freq_bits)
        o.write_u32(p + 0x24, s)
        o.write_u32(p + 0x6C, flags)
        o.write_u32(s + 4, s)
        o.write_u32(s + 8, s)
        o.write_u32(self.envelope + 0x18, p)
        self.particle = p
        self.term = 0
        return p

    def node(self, type_id: int, label: str, counter: int = 0,
             rate_bits: int = 0, domain: list | None = None) -> int:
        """One action node; `domain` = [ext0,ext1,sq0,sq1] words or None."""
        o = self.o
        n = o.alloc(0x64)
        o.write_u32(n, 0x5F13A0)          # the exe's counter-node vtable
        o.write_u32(n + 4, 0)
        o.write_u32(n + 8, 0)
        o.write_u32(n + 0x10, type_id)
        o.write(n + 0x14, label.encode() + b"\0")
        o.write_u32(n + 0x54, rate_bits)
        o.write_u32(n + 0x58, counter)
        o.write_u32(n + 0x5C, 0)
        o.write_u32(n + 0x60, self.domain_obj(domain) if domain else 0)
        return n

    def domain_obj(self, words: list) -> int:
        """A 0x20-byte vtable-0x5f113c domain carrying `words`."""
        o = self.o
        d = o.alloc(0x20)
        o.write_u32(d, 0x5F113C)
        for i, w in enumerate(words):
            o.write_u32(d + 0x10 + 4 * i, w)
        return d

    def link(self, nodes: list) -> None:
        """Link `nodes` after the sentinel, tipped by a zero-next terminator.

        The exe walk only compares a node whose `+4` is nonzero, so the
        terminator is what lets a miss fall out instead of wrapping the
        sentinel's circular list. It is not reported and is only ever
        compared after an exe append splices a node behind it, which happens
        in setCounter after its own walk is done.
        """
        o = self.o
        p = self.particle
        s = p + 0x2C
        prev = s
        for n in nodes:
            o.write_u32(prev + 4, n)
            o.write_u32(n + 8, prev)
            prev = n
        term = o.alloc(0x64)
        o.write_u32(term, 0x5F13A0)
        o.write_u32(term + 4, 0)          # zero next: the walk stops here
        o.write_u32(term + 8, prev)
        o.write_u32(term + 0x10, 0)
        o.write(term + 0x14, b"\0")
        o.write_u32(prev + 4, term)
        o.write_u32(s + 8, term)
        self.term = term

    def actions_view(self) -> list:
        """The linked action nodes in link order, as portable dictionaries.

        Nodes the exe itself appended are reported too, so the list is the
        live chain and not just the planted `pre` nodes; only the sentinel
        and the harness terminator are skipped.
        """
        o = self.o
        out = []
        head = o.read_u32(self.particle + 0x24)
        cur = o.read_u32(head + 4)
        seen = set()
        while cur and cur not in seen and cur != head:
            seen.add(cur)
            if cur == self.term:
                # The exe splices an appended node after the terminator, so
                # step over it rather than stopping.
                cur = o.read_u32(cur + 4)
                continue
            dom = o.read_u32(cur + 0x60)
            out.append({
                "type": o.read_u32(cur + 0x10),
                "label": o.read_cstr(cur + 0x14, 0x40).decode("latin1"),
                "counter": o.read_u32(cur + 0x58),
                "rate": o.read_u32(cur + 0x54),
                "domain": None if not dom else
                [o.read_u32(dom + 0x10), o.read_u32(dom + 0x14),
                 o.read_u32(dom + 0x18), o.read_u32(dom + 0x1C)],
            })
            cur = o.read_u32(cur + 4)
        return out


def _anim_obj(w: World, entries: list, cap) -> int:
    """The Animation object: +0x9c buffer, +0xa0 count, +0xa4 capacity."""
    o = w.o
    obj = o.alloc(0x100)
    o.write_u32(obj + 8, w.exists)
    n = len(entries)
    capacity = n if cap is None else cap
    buf = 0
    if capacity or entries:
        buf = o.alloc(max(capacity, 1) * 8)
        for i, (tag, payload) in enumerate(entries):
            o.write_u32(buf + i * 8, tag)
            o.write_u32(buf + i * 8 + 4, payload)
    o.write_u32(obj + 0x9C, buf)
    o.write_u32(obj + 0xA0, n)
    o.write_u32(obj + 0xA4, capacity)
    w.obj = obj
    return obj


# ---------------------------------------------------------------- natives
ANIM_NATIVES = {
    "setSpeed": (0x47EC00, b"(F)V"),
    "setFade": (0x47ECC0, b"(F)V"),
    "play": (0x47ED80, b"()V"),
    "loopPlay": (0x47EE30, b"()V"),
    "pause": (0x47EEE0, b"()V"),
    "seek": (0x47EF90, b"(F)V"),
}


def run_anim(name, case, exe=None):
    addr, sig = ANIM_NATIVES[name]
    w = World(exe, exists=case.get("exists", 1))
    o = w.o
    obj = _anim_obj(w, case.get("pre", []), case.get("cap"))
    cells = []
    if sig == b"(F)V":
        cells.append(_cell(o, "F", case["f32_bits"]))
    cells.append(_cell(o, "L", obj))
    frame = _frame(w, sig, cells)
    o.call(addr, [frame], "cdecl", limit=2_000_000)
    o = w.o
    count = o.read_u32(obj + 0xA0)
    capacity = o.read_u32(obj + 0xA4)
    buf = o.read_u32(obj + 0x9C)
    queue = []
    if buf and count:
        for i in range(count):
            queue.append([o.read_u32(buf + i * 8),
                          o.read_u32(buf + i * 8 + 4)])
    return {"count": count, "capacity": capacity, "queue": queue}, []


# ------------------------------------------------------------- psys natives
PSYS_NATIVES = {
    "modePermanent": (0x47F150, b"(I)V"),
    "setFreq": (0x47F1F0, b"(F)V"),
    "getCounter": (0x47FCD0, b"(Ljava.lang.String;)I"),
    "delAction": (0x47FDB0, b"(Ljava.lang.String;)V"),
    "setCounter": (0x47FA70, b"(Ljava.lang.String;Ljava.lang.Vector3;F)V"),
}
PSYS_CLS = b"java.util.resource.ParticleSystem"


def _f32_of(bits: int) -> float:
    return struct.unpack("<f", struct.pack("<I", bits & 0xFFFFFFFF))[0]


def run_psys(name, case, exe=None):
    """Run one ParticleSystem native and project the particle object.

    `case` = {"env": {"mode", "flags", "freq_bits"}, "pre": [nodes],
    "call": {...}}. The container order is `[argN, ..., arg1, this]`
    (see the module docstring): setCounter's rate cell therefore comes
    first, then the Vector3, then the label, then `this`.
    """
    addr, sig = PSYS_NATIVES[name]
    env = case.get("env") or {}
    call = case.get("call") or {}
    w = World(exe, mode=env.get("mode", 1))
    o = w.o
    w.particle_obj(env.get("flags", 0), env.get("freq_bits", 0))
    vec = call.get("vec", [0, 0, 0])
    w.components = {"x": _f32_of(vec[0]), "y": _f32_of(vec[1]),
                    "z": _f32_of(vec[2])}
    w.link([w.node(a["type"], a["label"], a.get("counter", 0),
                   a.get("rate", 0), a.get("domain"))
            for a in case.get("pre", [])])
    if sig == b"(I)V":
        cells = [_cell(o, "I", call["mode"])]
    elif sig == b"(F)V":
        cells = [_cell(o, "F", call["freq_bits"])]
    elif sig in (b"(Ljava.lang.String;)I", b"(Ljava.lang.String;)V"):
        cells = [_cell(o, "L", _alloc_cstr(o, call["label"].encode()))]
    else:
        label = _alloc_cstr(o, call["label"].encode())
        cells = [_cell(o, "F", call["freq_bits"]),
                 _cell(o, "L", o.alloc(0x20)),
                 _cell(o, "L", label)]
    cells.append(_cell(o, "L", w.env))
    frame = _frame(w, sig, cells, cls=PSYS_CLS)
    res = o.call(addr, [frame], "cdecl", limit=20_000)
    result = {"flags": o.read_u32(w.particle + 0x6C),
              "freq": o.read_u32(w.particle + 0x0C),
              "ret": None,
              "actions": w.actions_view()}
    if name == "getCounter":
        result["ret"] = res.eax
    elif name == "delAction":
        result["freed"] = res.eax != 0
    return result, [r[1] for r in w.field_reads]


# ---------------------------------------------------------------- cases
FLOATS = [0.0, -0.0, 1.0, -1.5, 3.14159274, 1e-45, 3.4028235e38,
          float("inf"), float("-inf"), 0.1]


def _fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def _pre(n: int) -> list:
    """n marker entries [tag, payload] used as pre-existing queue content."""
    return [[0x100 + i, 0x200 + i] for i in range(n)]


def anim_cases(name):
    sig = ANIM_NATIVES[name][1]
    out = []
    if sig == b"(F)V":
        for f in FLOATS:
            out.append({"f32_bits": _fbits(f), "pre": [], "exists": 1})
        # append behind existing entries and behind a full buffer (grow path)
        out.append({"f32_bits": _fbits(2.5), "pre": _pre(3), "exists": 1})
        out.append({"f32_bits": _fbits(2.5), "pre": _pre(0x20),
                    "cap": 0x20, "exists": 1})
        out.append({"f32_bits": _fbits(-7.25), "pre": _pre(0x21),
                    "cap": 0x40, "exists": 1})
        out.append({"f32_bits": _fbits(1.25), "pre": [], "cap": 0x20,
                    "exists": 1})
        # handle-exists flag clear: no push, queue untouched
        out.append({"f32_bits": _fbits(1.25), "pre": _pre(2), "exists": 0})
    else:
        out.append({"pre": [], "exists": 1})
        out.append({"pre": _pre(2), "exists": 1})
        out.append({"pre": _pre(0x20), "cap": 0x20, "exists": 1})
        out.append({"pre": _pre(1), "exists": 0})
    return out


def _act(type_id, label, counter=0, rate=0, domain=None):
    """One pre-existing action node for `pre`."""
    return {"type": type_id, "label": label, "counter": counter,
            "rate": rate, "domain": domain}


def _psys(pre=(), call=None, flags=0, freq_bits=0, mode=1):
    return {"env": {"mode": mode, "flags": flags, "freq_bits": freq_bits},
            "pre": list(pre), "call": call or {}}


def _vec(x, y, z):
    return [_fbits(float(x)), _fbits(float(y)), _fbits(float(z))]


def _sc(label, rate, pre=(), vec=(1.0, 2.0, 3.0), mode=1):
    """One setCounter case at `rate` (a Python float)."""
    return _psys(pre, call={"label": label, "vec": _vec(*vec),
                            "freq_bits": _fbits(rate)}, mode=mode)


def psys_cases(name):
    if name == "modePermanent":
        out = []
        for mode in (0, 1, 2, 0xFFFFFFFF):
            for flags in (0, 0x20000000, 0xFFFFFFFF):
                out.append(_psys(flags=flags, call={"mode": mode}))
        # ref+0x4c != 1 also runs the extra vtable[5] materialise call
        out.append(_psys(flags=0x20000000, call={"mode": 0}, mode=0))
        out.append(_psys(flags=0x20000000, call={"mode": 1}, mode=0))
        return out
    if name == "setFreq":
        out = [_psys(call={"freq_bits": _fbits(f)}) for f in FLOATS]
        out.append(_psys(freq_bits=0x40200000, call={"freq_bits": _fbits(7.5)}))
        out.append(_psys(flags=0x20000000, freq_bits=0x1111,
                         call={"freq_bits": _fbits(-0.5)}))
        return out
    if name == "getCounter":
        return [
            _psys(call={"label": "counter_a"}),                     # empty list
            _psys([_act(0xD, "counter_a", counter=7)],              # value+zero
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=7),
                   _act(0xD, "counter_b", counter=9)],
                  call={"label": "counter_b"}),
            _psys([_act(0xD, "counter_a", counter=7)],              # case-insens.
                  call={"label": "CoUnTeR_A"}),
            _psys([_act(0xD, "counter_a", counter=0xFFFFFFFF)],
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=0)],
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=7),               # first match
                   _act(0xD, "counter_a", counter=9)],
                  call={"label": "counter_a"}),
            _psys([_act(0xA, "counter_a", counter=5),               # type gate
                   _act(0xD, "counter_a", counter=7)],
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=7)],              # miss
                  call={"label": "miss"}),
        ]
    if name == "delAction":
        return [
            _psys(call={"label": "counter_a"}),                     # empty list
            _psys([_act(0xD, "counter_a", counter=7), _act(0xA, "source_a")],
                  call={"label": "counter_a"}),
            _psys([_act(0xA, "source_a"), _act(0xD, "counter_a", counter=7)],
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=7),
                   _act(0xD, "counter_b", counter=9)],
                  call={"label": "COUNTER_B"}),
            _psys([_act(0xD, "counter_a", counter=7),
                   _act(0xD, "counter_a", counter=9)],
                  call={"label": "counter_a"}),
            _psys([_act(0xD, "counter_a", counter=7)], call={"label": "miss"}),
            _psys([_act(0x10, "emitter", rate=0x41200000)],
                  call={"label": "emitter"}),
        ]
    if name == "setCounter":
        out = []
        # reuse: only the +0x60 domain is replaced
        out.append(_sc("counter_a", 4.0, [_act(0xD, "counter_a", counter=7,
                                               rate=0x1111,
                                               domain=[0x3F800000, 0,
                                                       0x3F800000, 0])]))
        out.append(_sc("COUNTER_A", 4.0, [_act(0xD, "counter_a", counter=7,
                                               rate=0x1111)]))
        # append: fresh node with +0x54/+0x58 zero, domain at +0x60
        out.append(_sc("counter_a", 4.0, [_act(0xA, "source_a", rate=0x1111)]))
        out.append(_sc("counter_a", 4.0))
        out.append(_sc("counter_a", 4.0, [_act(0xD, "counter_b", counter=9)]))
        # matched label of another type: no append, nothing built
        out.append(_sc("counter_a", 4.0, [_act(0xA, "counter_a", counter=5)]))
        # rate sign drives the domain extent pair
        for rate in (0.0, -0.0, 1.0, -1.0, 1e-45, 3.4028235e38, -2.5,
                     float("inf"), float("-inf"), float("nan")):
            out.append(_sc("counter_a", rate))
        out.append(_sc("counter_a", 0.5, [_act(0xD, "counter_a", counter=3)],
                       vec=(0.5, -0.25, 0.0)))
        return out
    raise KeyError(name)


# ------------------------------------------------------------- gate natives
# The gate/constant family (H3): five natives whose port targets are the
# constant helpers `sfx_target_in_range` / `sfx_scaled_volume`
# (FUN_00480d40 nplay), `render_type_gate` (LAB_00486be0 getTypeID),
# `render_detail` (LAB_00480440 getDetail) and `phys_slot_addr`
# (LAB_00480920 setMatrix). Each row records the exact fact the port helper
# reproduces, so a mismatch is a direct divergence report.
GATE_NATIVES = {
    "sfx_nplay": (0x480D40, b"(Ljava.lang.Vector3;FFFII)I",
                  b"java.util.resource.SfxRef"),
    "sfx_stop": (0x480E60, b"(I)V", b"java.util.resource.SfxRef"),
    "render_gettypeid": (0x486BE0, b"()I",
                         b"java.util.resource.RenderRef"),
    "render_getdetail": (0x480440, b"()F",
                         b"java.util.resource.RenderRef"),
    "phys_setmatrix": (0x480920,
                       b"(Ljava.lang.Vector3;Ljava.lang.Ypr;)V",
                       b"java.util.resource.PhysicsRef"),
}
UNIT_VOL_ADDR = 0x612C58    # DAT_00612c58, the nplay unit volume
UNIT_VOL_BITS = 0x3F800000  # its shipped value, 1.0f
SLOT_STRIDE = 0xD4          # phys body slot stride
SLOT_POS_OFF = 0x64         # position fields inside a slot
SLOT_ORI_OFF = 0xA4         # orientation fields inside a slot


def _gate_obj(o: Oracle, obj_word: int, mode: int, detail_bits: int) -> int:
    """`this` object + its `[obj+0xc]` environment.

    `obj+8` is the word the getDetail gate tests, `env+0x4c` the mode word
    and `env+0x6c` the detail float the getDetail result reads.
    """
    env = o.alloc(0x80)
    o.write_u32(env + 0x4C, mode)
    o.write_u32(env + 0x6C, detail_bits)
    obj = o.alloc(0x40)
    o.write_u32(obj + 8, obj_word)
    o.write_u32(obj + 0xC, env)
    return obj


# --------------------------------------------------------------- handles
# The resource handle ops (M1-11). Each native is a frame pop, a
# `0x42ab50` field lookup, a null gate and ONE engine call; the engine call
# is the boundary (it needs a live D3D9 device / physics world), so it is
# hooked and recorded. What the vectors pin is the native's own forwarding:
# which words reach the engine, in what order, and what the native answers.
#
# `changeResource` is different: it dispatches through the ref's vtable
# (+0xc resolve, +0x14 materialise, +8 the store), so its rows pin the call
# sequence rather than one engine entry point.
# Ops whose every arm replays in the probe. `render_create`, `render_flare`
# and `phys_create` are omitted on purpose: their arms fault on vtable slots
# the fake objects do not carry (`eip=0x0` / a heap address executed), so
# their ledger rows stay `ported (no test yet)` rather than being claimed by
# vectors that never reached the engine path.
HANDLE_NATIVES = {
    "render_change": (0x480220,
                      b"(Ljava.util.resource.Resource;Ljava.util.resource.Resource;)V",
                      b"java.util.resource.RenderRef", None),
    "render_color": (0x480310, b"(I)V", b"java.util.resource.RenderRef",
                     (0x48C8C0, 1)),
    "render_light": (0x486AB0, b"(III)V", b"java.util.resource.RenderRef",
                     (0x48C9D0, 4)),
    "render_type": (0x486BA0, b"(I)V", b"java.util.resource.RenderRef",
                    (0x48C970, 1)),
    "line_create": (0x47FE70, b"(Ljava.lang.Vector3;Ljava.lang.Vector3;)I",
                    b"java.util.resource.RenderRef", (0x48A670, 5)),
    "bone_id": (0x481020, b"(Ljava.lang.String;)I",
                b"java.util.resource.RenderRef", (0x48BC40, 1)),
}

# `changeResource`'s vtable sequence, in call order.
CHANGE_VTABLE = {0x0C: "resolve", 0x14: "materialise", 0x08: "store"}


def _handle_engine(w, name):
    """A hook body recording one engine entry point's `this` and args.

    The answer is a fresh ref-shaped object: `render_create` compares the
    engine's result against the object's existing `[+0xc]` and then splices
    it through `0x4290f0`, so a scalar would be dereferenced as a pointer.
    """
    def hook(oc, args):
        w.handle_calls.append({"fn": name, "this": oc.reg("ecx"),
                               "args": list(args)})
        return _ref_obj(oc, 1)
    return hook


def _res_obj(o: Oracle, word: int, calls: list | None = None) -> int:
    """A minimal loaded resource: `[+0xc]` -> a body with `[+0x48]`/`[+0x44]`
    and a scratch vtable.

    `render_create` gates on `[[arg]+0xc] != 0` before it calls the engine, so
    a body must always be present. The exe then dispatches through the body's
    vtable (`mov edx,[esi]` / `call [edx+0x14]` at `0x48bc82`), and
    `changeResource` does the same through the outer ref, so both get the same
    `ret`-byte + code-hook stub the psys chain uses. Slot answers:
    0xc -> the body (resolve), 0x14 -> 1 (materialise), 8 -> the word (store).
    """
    rec = calls if calls is not None else []
    body = o.alloc(0x80)
    o.write_u32(body + 0x48, 0)
    o.write_u32(body + 0x50, word)
    vt = o.alloc(0x40)
    for slot, label in ((0x0C, "resolve"), (0x14, "materialise"), 0x08 if False else (0x08, "store")):
        stub = o.alloc(0x10)
        o.write(stub, b"\xc3")

        def h(oc, a, _label=label):
            rec.append({"fn": _label, "this": oc.reg("ecx"), "args": list(a)})
            return body if _label == "resolve" else (word if _label == "store" else 1)
        o.hook(stub, h, "thiscall", 1)
        o.write_u32(vt + slot, stub)
    o.write_u32(body, vt)
    res = o.alloc(0x20)
    o.write_u32(res + 0xC, body)
    o.write_u32(res, vt)
    return res


def _ref_obj(o: Oracle, word: int) -> int:
    """The engine's answer: a fresh ref whose `[+0xc]` is a body."""
    return _res_obj(o, word)


def run_handle(name, case, exe=None):
    addr, sig, cls, boundary = HANDLE_NATIVES[name]
    w = World(exe)
    o = w.o
    obj = o.alloc(0x80)
    w.obj = obj
    o.write_u32(obj + 8, 1)          # the `[obj+8]` handle-exists word
    w.handle_calls = []
    if boundary:
        ep, nargs = boundary
        o.hook(ep, _handle_engine(w, f"0x{ep:06x}"), "thiscall", nargs)
    if name == "render_change":
        # vtable[0xc] -> resolved ref, vtable[0x14] -> materialise,
        # vtable[8] -> the store; `+0x4c == 1` skips the materialise call
        target = o.alloc(0x60)
        o.write_u32(target + 0x4C, 1 if case.get("skip_materialise") else 0)
        vt = o.alloc(0x20)
        for slot, label in CHANGE_VTABLE.items():
            o.write_u32(vt + slot, _make_vt_stub(w, o, label))
        o.write_u32(target, vt)
        w.change_target = target
        o.hook(0x5447D0, lambda oc, a: 0, "thiscall", 3)
    cells = []
    for ty, val in case["args"]:
        if ty == "I":
            cells.append(_cell(o, "I", val))
        elif ty == "L":
            # an `L` argument is a resource: the natives read `[word+0xc]`
            # (the loaded body) and then `[body+0x48]` / `[body+0x44]`, so
            # plant a resource-shaped object (or the null word)
            cells.append(_cell(o, "L", _res_obj(o, val) if val else 0))
    cells.append(_cell(o, "L", obj))
    frame = _frame(w, sig, cells, cls=cls)
    res = o.call(addr, [frame], "cdecl", limit=200_000)
    return _handle_view(w, name, case, res), list(w.handle_calls)


def _make_vt_stub(w, o, label):
    """One vtable slot: a `ret` byte whose address is code-hooked."""
    addr = o.alloc(0x10)
    o.write(addr, b"\xc3")
    def stub(oc, args):
        w.handle_calls.append({"fn": label, "this": oc.reg("ecx"),
                               "args": list(args)})
        return w.change_target if label == "resolve" else 1
    o.hook(addr, stub, "thiscall", 1)
    return addr


def _handle_view(w, name, case, res) -> dict:
    """The comparable facts: the return word and the engine call list.

    A return that is a heap pointer is not comparable across runs, so the
    view reports whether it is the answer the planted object holds (the
    native's `[obj+8]` handle word) rather than the address.
    """
    ret = res.eax & 0xFFFFFFFF
    return {"ret_is_ptr": ret >= 0x20000000, "obj8": 1}


def handle_cases(name):
    """Argument tuples per case; the vectors are the observed call lists."""
    one = 0x11111111
    two = 0x22222222
    if name == "render_change":
        return [{"args": [("I", 0), ("I", 0)]},
                {"args": [("I", 1), ("I", 2)]},
                {"args": [("I", 1), ("I", 2)], "skip_materialise": True}]
    if name == "render_color":
        return [{"args": [("I", v)]} for v in (0, 0xFF00FF, 0xFFFFFFFF)]
    if name == "render_light":
        return [{"args": [("I", a), ("I", b), ("I", c)]}
                for a, b, c in ((0, 0, 0), (1, 2, 3), (255, 128, 64),
                                (0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF))]
    if name == "render_type":
        return [{"args": [("I", v)]} for v in (0, 1, 0x1234)]
    if name == "line_create":
        return [{"args": [("I", 0), ("I", 0)]},
                {"args": [("I", one), ("I", two)]}]
    if name == "bone_id":
        return [{"args": [("I", 0)]}, {"args": [("I", one)]}]
    raise KeyError(name)


def run_gate(name, case, exe=None):
    """Run one gate native and project the engine call / returned word."""
    addr, sig, cls = GATE_NATIVES[name]
    w = World(exe)
    o = w.o
    if name == "sfx_nplay":
        obj = o.alloc(0x40)
        o.write_u32(obj + 8, case["target"])
        w.obj = obj
        o.write_u32(UNIT_VOL_ADDR, case.get("unit_bits", UNIT_VOL_BITS))
        vec = o.alloc(0x20) if case.get("vec", True) else 0
        # container [arg6, arg5, arg4, arg3, arg2, arg1, this]
        cells = [
            _cell(o, "I", case.get("e", 0)),
            _cell(o, "I", case.get("d", 0)),
            _cell(o, "F", case["fc"]),
            _cell(o, "F", case["fb"]),
            _cell(o, "F", case["fa"]),
            _cell(o, "L", vec),
            _cell(o, "L", obj),
        ]
    elif name == "sfx_stop":
        obj = o.alloc(0x40)
        w.obj = obj
        cells = [_cell(o, "I", case["channel"]), _cell(o, "L", obj)]
    elif name == "render_gettypeid":
        obj = _gate_obj(o, case.get("obj_word", 1), case.get("mode", 3), 0)
        if case.get("env_null"):
            o.write_u32(obj + 0xC, 0)
        envl = o.alloc(0x100)
        o.write_u32(envl + 0x80, case.get("type_word", 0x1234))
        w.envelope = 0 if case.get("fetch_null") else envl
        w.obj = obj
        cells = [_cell(o, "L", obj)]
    elif name == "render_getdetail":
        obj = _gate_obj(o, case["obj_word"], case["mode"], case["detail"])
        if case.get("env_null"):
            o.write_u32(obj + 0xC, 0)
        w.obj = obj
        cells = [_cell(o, "L", obj)]
    else:  # phys_setmatrix
        obj = o.alloc(0x40)
        w.obj = obj
        engine = o.alloc(0x200)
        body = o.alloc(0x40000)
        o.write_u32(engine + 0x84, body)
        o.write_u32(body + 0x20C, case["slot"])
        w.engine = engine
        w.body = body
        pos = o.alloc(0x20)
        ori = o.alloc(0x20)
        w.components = {"x": 0.0, "y": 0.0, "z": 0.0}
        w.obj_components = {
            pos: {"x": _f32_of(case["pos"][0]),
                  "y": _f32_of(case["pos"][1]),
                  "z": _f32_of(case["pos"][2])},
            ori: {"y": _f32_of(case["ori"][0]),
                  "p": _f32_of(case["ori"][1]),
                  "r": _f32_of(case["ori"][2])},
        }
        # container [arg2(Ypr), arg1(Vector3), this]
        cells = [_cell(o, "L", ori), _cell(o, "L", pos),
                 _cell(o, "L", obj)]
    frame = _frame(w, sig, cells, cls=cls)
    res = o.call(addr, [frame], "cdecl", limit=200_000)
    return _gate_view(w, name, case, res), [r[1] for r in w.field_reads]


def _gate_view(w, name, case, res) -> dict:
    o = w.o
    if name == "sfx_nplay":
        return {"ret": res.eax,
                "play": w.sfx_play_calls[0] if w.sfx_play_calls else None,
                "target": case["target"],
                "unit": case.get("unit_bits", UNIT_VOL_BITS),
                "vol": case["fc"], "vec": bool(case.get("vec", True))}
    if name == "sfx_stop":
        return {"ret": res.eax,
                "stop": w.sfx_stop_calls[0] if w.sfx_stop_calls else None}
    if name == "render_gettypeid":
        return {"ret": res.eax, "fetched": w.fetch_calls,
                "mode": case.get("mode", 3)}
    if name == "render_getdetail":
        return {"ret": None if res.st0 is None else _fbits(res.st0),
                "mode": case["mode"], "obj_word": case["obj_word"]}
    p = w.body + case["slot"] * SLOT_STRIDE + SLOT_POS_OFF
    q = w.body + case["slot"] * SLOT_STRIDE + SLOT_ORI_OFF
    lo, hi = w.body, w.body + 0x40000
    return {"slot": case["slot"],
            "pos": o.read_u32s(p, 3), "ori": o.read_u32s(q, 3),
            "pos_addr": p, "ori_addr": q,
            "expect_pos": list(case["pos"]), "expect_ori": list(case["ori"]),
            "copy_dests": list(w.copy_dests),
            "writes": [[a, v] for (a, sz, v) in res.mem_writes
                       if sz == 4 and lo <= a < hi]}


def gate_cases(name):
    if name == "sfx_nplay":
        out = []
        # The `id & 0xffff` window 0x26..0x2c, plus values whose low word is
        # out of it; every case carries a live Vector3 so the only variable
        # is the gate.
        for tgt in (0x26, 0x2B, 0x25, 0x2C, 0x00, 0xFFFF, 0x10026, 0x10025,
                    0xFFFF0026, 0x12345678):
            out.append({"target": tgt, "vec": True, "fa": _fbits(1.0),
                        "fb": _fbits(2.0), "fc": _fbits(0.5), "d": 0, "e": 0})
        # A null Vector3 argument: the pointer argument is dropped.
        out.append({"target": 0x26, "vec": False, "fa": _fbits(1.0),
                    "fb": _fbits(2.0), "fc": _fbits(0.5), "d": 0, "e": 0})
        # Volume scaling `DAT_00612c58 * arg4`: distinct words pin the
        # operand order, and the unit word is planted per row.
        for unit, fc in ((0x3F800000, 0x3F000000), (0x40000000, 0x40000000),
                         (0x3F800000, 0xC0000000), (0x3F000000, 0x40400000),
                         (0x7F7FFFFF, 0x40000000), (0x3F800000, 0x7F800000)):
            out.append({"target": 0x26, "vec": True, "fa": _fbits(11.0),
                        "fb": _fbits(22.0), "fc": fc, "d": 0xDDDD0004,
                        "e": 0xEEEE0005, "unit_bits": unit})
        return out
    if name == "sfx_stop":
        return [{"channel": ch} for ch in (0, 1, 0x26, 0xFFFFFFFF)]
    if name == "render_gettypeid":
        out = [{"mode": m, "type_word": 0x12345678, "obj_word": 1}
               for m in (0, 1, 2, 3, 4, 0xFFFFFFFF)]
        out.append({"mode": 3, "type_word": 0, "obj_word": 1})
        out.append({"mode": 3, "obj_word": 1, "env_null": True})
        out.append({"mode": 3, "obj_word": 1, "fetch_null": True})
        return out
    if name == "render_getdetail":
        out = [{"mode": m, "obj_word": 1, "detail": 0x40000000}
               for m in (0, 1, 2, 3, 4, 0xFFFFFFFF)]
        for obj_word in (0, 1, 0xFFFFFFFF):
            for mode in (1, 3):
                out.append({"mode": mode, "obj_word": obj_word,
                            "detail": 0x40490FDB})
        for det in (0x0, 0x80000000, 0x3F800000, 0x7F7FFFFF, 0x7F800000,
                    0xFF800000):
            out.append({"mode": 1, "obj_word": 1, "detail": det})
        out.append({"mode": 1, "obj_word": 1, "detail": 0x40000000,
                    "env_null": True})
        return out
    if name == "phys_setmatrix":
        # Slots 2 and 3 are skipped: the exe reads its slot index from
        # `body+0x20c`, which is exactly slot 2's position field, so those
        # rows would corrupt the index mid-call (a real exe behaviour, not a
        # harness artifact, but not a phys_slot_addr fact).
        return [{"slot": s,
                 "pos": [_fbits(10.0), _fbits(20.0), _fbits(30.0)],
                 "ori": [_fbits(1.0), _fbits(2.0), _fbits(3.0)]}
                for s in (0, 1, 4, 5, 0x10, 0xFF)]
    raise KeyError(name)


# M1-10 GameRef query boundary. The native, argument marshaller, type gates,
# and envelope field reads execute in the real image; JVM field access and
# live engine lookups are the only replacements.
GAME_NATIVES = {
    "create": (0x47D7B0, b"(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)Ljava.lang.GameType;"),
    "create_native": (0x47D900, b"(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)V"),
    "getInfoI": (0x47DDF0, b"(II)I"),
    "getInfoS": (0x47DDF0, b"(ILjava.lang.String;)I"),
    "setPos": (0x47E350, b"(Ljava.lang.Vector3;)V"),
    "setMatrix": (0x47E490, b"(Ljava.lang.Vector3;Ljava.lang.Ypr;)V"),
    "setState": (0x47E630, b"(Ljava.lang.Vector3;Ljava.lang.Ypr;Ljava.lang.Vector3;Ljava.lang.Vector3;)V"),
    "isEmpty": (0x486D10, b"()I"),
    "isScripted": (0x486DA0, b"(Ljava.lang.String;)I"),
    "getScriptInstance": (0x486F30, b"()Ljava.lang.Object;"),
    "getDetail": (0x480440, b"()F"),
    "getPos": (0x47DAD0, b"()Ljava.lang.Vector3;"),
    "getOri": (0x47DBE0, b"()Ljava.lang.Ypr;"),
    "getVel": (0x47DCE0, b"()Ljava.lang.Vector3;"),
    "setParent": (0x47E2D0, b"(Ljava.util.resource.ResourceRef;)V"),
    "queueEvent": (0x47DA30, b"(Ljava.util.resource.ResourceRef;ILjava.lang.String;)V"),
}


def game_cases(name):
    if name in ("setPos", "setMatrix", "setState"):
        return [{"mode": 1, "node": node, "container": True, "envelope": True,
                 "gate": False, "script": 0, "child": False, "member": False,
                 "class_match": False, "child_compiled": False, "name": False,
                 "detail": 0, "node_id": 9, "nulls": mask,
                 "vectors": [[0x3FC00000, 0xC0200000, 0x80000000],
                             [0x41000000,0xC1100000,0x41200000],
                             [0x3E800000,0x3F000000,0x3F400000],
                             [0x41800000,0x42000000,0x42800000]]}
                for node in (True, False) for mask in (0,1,2,4,8,15)]
    base = {"mode": 1, "node": True, "container": True, "envelope": True,
            "gate": False, "script": 0x1234, "child": True,
            "member": True, "class_match": True, "child_compiled": False,
            "name": False, "detail": 0x40400000, "node_id": 9}
    cases = [base]
    for key, values in {
        "mode": (0, 1, 3, 8, 14), "node": (False,),
        "container": (False,), "envelope": (False,), "gate": (True,),
        "script": (0,), "child": (False,), "member": (False,),
        "class_match": (False,), "child_compiled": (True,),
        "node_id": (0,), "detail": (0, 0x80000000, 0xC0200000),
    }.items():
        for value in values:
            cases.append(dict(base, **{key: value}))
    if name in ("create", "create_native"):
        cases += [dict(base,first=False),dict(base,first_bound=False),dict(base,first=False,default_bound=True)]
    if name in ("getInfoI", "getInfoS"):
        cases += [dict(base,secondary_mode=3),dict(base,secondary_gate=True),
                  dict(base,secondary_envelope=False),dict(base,info_id=0)]
    if name == "isScripted":
        cases += [dict(c, name=True) for c in cases]
    if name == "isEmpty":
        cases += [dict(base, mode=8, child=c, member=m)
                  for c in (False, True) for m in (False, True)]
    return cases


def run_game(name, case, exe=None):
    w = World(exe, mode=case["mode"])
    o = w.o
    ref = o.alloc(0x100)
    o.write(ref, o.read(w.ref, 0x60))
    w.ref = ref
    o.write_u32(w.obj + 0xC, ref)
    o.write_u32(w.obj + 8, case["node_id"])
    o.write_u32(w.ref + 0x6C, case["detail"])
    script = o.alloc(0x20)
    o.write_u32(script + 0xC, o.alloc(0x20))
    o.write_u32(w.envelope + 0x50, script if case["script"] else 0)
    o.hook(0x404E20, lambda oc, args: script if case["script"] else 0, "thiscall", 0)
    child = o.alloc(0x20)
    o.write_u32(child + 0x10, int(case["child_compiled"]))
    o.write_u32(w.envelope + 0xC, int(case["member"]))
    o.write_u32(w.envelope + 0x10, child if case["child"] else 0)
    if not case["container"]:
        o.write_u32(w.obj + 0xC, 0)
    if not case["node"]:
        w.obj = 0
    if not case["envelope"]:
        w.envelope = 0
    events = []
    w.secondary = 0
    def resource_identity(word):
        return "primary" if word == w.ref else "secondary" if word == w.secondary else word
    def node_identity(word):
        return "node" if word == w.obj else word
    vt = o.read_u32(w.ref)
    def slot(label):
        def call(oc, args):
            events.append([label,resource_identity(oc.reg("ecx"))])
            return w.envelope
        stub = o.alloc(16)
        o.hook(stub, call, "thiscall", 1)
        return stub
    o.write_u32(vt + 0xC, slot("fetch"))
    o.write_u32(vt + 0x14, slot("materialise"))
    def gate(oc, args):
        target = oc.reg("ecx")
        events.append(["gate",resource_identity(target),*args])
        refused = case.get("secondary_gate",False) if target == w.secondary else case["gate"]
        return 0x80000000 if refused else 0
    o.hook(0x5447D0, gate, "thiscall", 3)
    def class_match(oc, args):
        events.append(["class", oc.read_cstr(args[0], 256).decode("latin1")])
        return int(case["class_match"])
    o.hook(0x4044E0, class_match, "thiscall", 1)
    # isScripted formats the optional name through the exe string helpers;
    # these are plain buffer helpers, so execute them rather than log hooks.
    if name == "isScripted":
        # World's logger hook aliases the append function. Restore its real
        # body by removing just that hook, preserving all executable code.
        o.hooks.pop(0x551140, None)
    vector = [0x3FC00000, 0xC0200000, 0x80000000]
    def fill(oc, args):
        events.append(["read", "vel" if name == "getVel" else "ori" if name == "getOri" else "pos", node_identity(args[0] if name == "getVel" else oc.reg("ecx"))])
        dest = args[2] if name == "getVel" else args[0]
        for i, word in enumerate(vector): oc.write_u32(dest + 4*i, word)
        return 1
    o.hook(0x48B280, fill, "thiscall", 1)
    o.hook(0x48B300, fill, "thiscall", 1)
    o.hook(0x426470, fill, "thiscall", 3)
    o.hook(0x410890, lambda oc,a: 1, "thiscall", 1)
    o.hook(0x407DA0, lambda oc,a: 0, "thiscall", 2)
    fields = []
    o.hook(0x42A040, lambda oc,a: fields.append([oc.read_cstr(a[1], 8).decode(), a[2]]), "cdecl", 3)
    o.hook(0x48ABA0, lambda oc,a: events.append(["parent",node_identity(oc.reg("ecx")),a[0]]), "thiscall", 1)
    o.hook(0x426800, lambda oc,a: events.append(["queue",node_identity(a[0]),*a[1:]]), "thiscall", 5)
    if name in ("getInfoI", "getInfoS"):
        o.write_u32(w.envelope + 0x4C, case.get("info_id",0x333)) if w.envelope else None
        secondary = o.alloc(0x100)
        w.secondary = secondary
        second_vt = o.alloc(0x20)
        second_env = o.alloc(0x60)
        subobject = o.alloc(0x40)
        sub_vt = o.alloc(0x50)
        invoke = o.alloc(16)
        o.hook(invoke, lambda oc,a: events.append(["info", "dispatch" if oc.reg("ecx") == subobject else oc.reg("ecx"), *a]) or 0xBEEF, "thiscall", 3)
        o.write_u32(sub_vt + 0x3C, invoke)
        o.write_u32(subobject, sub_vt)
        o.write_u32(second_env + 0xC, subobject)
        fetch = o.alloc(16)
        o.hook(fetch, lambda oc,a: events.append(["fetch",resource_identity(oc.reg("ecx"))]) or (second_env if case.get("secondary_envelope",True) else 0), "thiscall", 1)
        o.write_u32(second_vt + 0xC, fetch)
        o.write_u32(secondary, second_vt)
        o.write_u32(secondary + 0x4C,case.get("secondary_mode",1))
        o.write_u32(second_vt + 0x14,slot("materialise"))
        if w.envelope: o.write_u32(w.envelope + 0x44, secondary if case["child"] else 0)
    created = 0
    creation_args = []
    if name in ("create", "create_native"):
        created = o.alloc(0x100)
        o.write_u32(created + 0x50, 0x55)
        o.write_u32(0x636460+0xC,int(case.get("default_bound",False)))
        first = o.alloc(0x20)
        o.write_u32(first + 0xC, int(case.get("first_bound",True)))
        creation_args = [first if case.get("first",True) else 0, 0x456, 0x789, 0xABC]
        def create(native):
            def call(oc,a):
                events.append(["create",native,*[0x777 if x == first else x for x in a]])
                return created if case["child"] else 0
            return call
        o.hook(0x53A5E0,create(False),"cdecl",4)
        o.hook(0x53A2A0,create(True),"cdecl",5)
        o.hook(0x419860, lambda oc,a: events.append(["instance", "created" if oc.reg("ecx") == created else oc.reg("ecx")]) or w.envelope, "thiscall", 4)
    input_vectors = []
    if name in ("setPos", "setMatrix", "setState"):
        for index, vec in enumerate(case["vectors"]):
            obj = o.alloc(0x20)
            input_vectors.append(0 if case["nulls"] & (1 << index) else obj)
            w.obj_components[obj] = dict(zip(("x","y","z") if index != 1 else ("y","p","r"), [_f32_of(b) for b in vec]))
        def component(oc,a):
            field = oc.read_cstr(a[1], 8).decode()
            index = input_vectors.index(a[0]) if a[0] else -1
            events.append(["field", index, field])
            return w.obj_components.get(a[0], {}).get(field, 0.0)
        o.hook(0x42A560, component, "cdecl", 2)
        o.hook(0x551FA0, lambda oc,a: 0, "thiscall", 0)
        o.hook(0x551FB0, lambda oc,a: 0, "thiscall", 0)
        def command(oc,a):
            fmt = oc.read_cstr(a[1], 256).decode()
            count = 12 if name == "setState" else 6
            values = list(struct.unpack("<"+"Q"*count, oc.read(oc.reg("esp")+12, 8*count)))
            events.append(["command", fmt, values])
            return 0
        o.hook(0x551FC0, command, "cdecl", 2)
        o.hook(0x48B440, lambda oc,a: events.append(["apply",node_identity(oc.reg("ecx"))]), "thiscall", 1)
    addr, sig = GAME_NATIVES[name]
    params = [_cell(o, "L", 0)] if name == "isScripted" else []
    if name == "isScripted" and case["name"]:
        params = [_cell(o, "L", _alloc_cstr(o, b"Engine"))]
    if name == "setParent": params = [_cell(o, "L", 0x777)]
    if name == "queueEvent": params = [_cell(o,"L",0x999), _cell(o,"I", 0x456), _cell(o,"L",0x777)]
    if input_vectors:
        count = {"setPos": 1, "setMatrix": 2, "setState": 4}[name]
        params = [_cell(o, "L", v) for v in reversed(input_vectors[:count])]
    if creation_args: params = [_cell(o,"L",v) for v in reversed(creation_args)]
    if name in ("getInfoI", "getInfoS"):
        params = [_cell(o,"I" if name == "getInfoI" else "L",0x789), _cell(o,"I",0x456)]
    frame = _frame(w, sig, params + [_cell(o, "L", 1)],
                   cls=b"java.util.resource.GameRef")
    res = o.call(addr, [frame], "cdecl", limit=500_000)
    if name in ("create", "create_native"):
        return {"ret": case["script"] if res.eax == script else 0 if name == "create" else None,
                "id": o.read_u32(w.obj + 8) if w.obj else None,
                "bound": bool(o.read_u32(w.obj + 0xC)) if w.obj else None}, events
    if name in ("getPos", "getOri", "getVel"):
        return {"fields": fields, "present": res.eax != 0}, events
    if name in ("setParent", "queueEvent", "setPos", "setMatrix", "setState"):
        return {}, events
    ret = _fbits(res.st0) if name == "getDetail" else res.eax
    if name == "getScriptInstance" and ret == script:
        ret = case["script"]
    return {"ret": ret}, events


# M1-10 ResourceRef natives, plus the GameRef flag traffic on the same node.
RESOURCE_NATIVES = {
    "newNative": (0x47CEA0, b"()V"), "deleteNative": (0x47CF00,b"()V"),
    "setI": (0x47CF80,b"(I)V"), "setRef": (0x47CFC0,b"(Ljava.util.resource.ResourceRef;)V"),
    "load": (0x47D0A0,b"()V"), "unload": (0x47D170,b"()V"), "destroy": (0x47D1E0,b"()V"),
    "type": (0x47D210,b"()I"), "id": (0x47D290,b"()I"), "getParentID": (0x47D3E0,b"()I"),
    "getParent": (0x47D4B0,b"()Ljava.util.resource.ResourceRef;"),
    "getWTRoot": (0x47D570,b"()Ljava.util.resource.ResourceRef;"),
    "getFirstChild": (0x47D630,b"()Ljava.util.resource.ResourceRef;"),
    "getNextChild": (0x47D6F0,b"()Ljava.util.resource.ResourceRef;"),
    "makeTexture": (0x47FFE0,b"(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V"),
    "makeSound": (0x480100,b"(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V"),
    "cache": (0x47E8D0,b"()V"), "precache": (0x47E950,b"()V"),
    "duplicate": (0x4802C0,b"(Ljava.util.resource.ResourceRef;)V"),
    "scaleMesh": (0x480390,b"(FFF)V"),
    "getFlags": (0x47DF40,b"()I"), "setFlags": (0x486C80,b"(I)V"),
    "clearFlags": (0x486CD0,b"(I)V"), "setActiveCollision": (0x47DF80,b"()V"),
}


def resource_cases(name):
    base = {"node": True, "bound": True, "id": 9, "mode": 3,
            "flags": 0x12345678, "mask": 0x123, "gate": False, "precache": 1,
            "target": True, "arg": True, "text": True, "terminal": False}
    cases = [base]
    for k,v in [("node",False),("bound",False),("id",0),("mode",1),
                ("gate",True),("precache",0),("target",False),
                ("arg",False),("text",False),("terminal",True)]:
        cases.append(dict(base, **{k:v}))
    if name in ("setFlags", "clearFlags"): cases += [dict(base,mask=x) for x in (0,0x10,0xFFFFFFFF)]
    if name == "setI": cases += [dict(base, id=0xBEEF), dict(base, id=0x55)]
    return cases


def run_resource(name, case, exe=None):
    w = World(exe, mode=case["mode"])
    o = w.o
    node = w.obj
    ref = o.alloc(0x100)
    o.write(ref, o.read(w.ref, 0x60))
    w.ref = ref
    target = o.alloc(0x100)
    terminal = o.alloc(0x100)
    o.write_u32(target + 4, 0 if case["terminal"] else terminal)
    o.write_u32(target + 0x50, 0x55)
    o.write_u32(target + 0x4C, 1)
    o.write_u32(ref + 4, target if case["target"] else terminal)
    o.write_u32(ref + 0x14, target if case["target"] else 0)
    o.write_u32(ref + 0x20, target if case["target"] else terminal)
    o.write_u32(ref + 0xCC, target if case["target"] else 0)
    o.write_u32(ref + 0x54, case["flags"])
    o.write_u32(ref + 0x50, case["id"])
    o.write_u32(ref + 0x48, node if case["node"] and case["bound"] else 0)
    o.write_u32(node + 8, case["id"])
    o.write_u32(node + 0xC, ref if case["bound"] else 0)
    if not case["node"]: w.obj = 0
    events = []
    labels = {node:"node", ref:"container", target:"target", terminal:"terminal", 0:"null"}
    def slot(label):
        stub = o.alloc(16)
        o.hook(stub, lambda oc,a: events.append([label]) or w.envelope, "thiscall", 1)
        return stub
    vt = o.read_u32(ref)
    o.write_u32(vt + 0xC, slot("fetch"))
    o.write_u32(vt + 0x14, slot("materialise"))
    o.hook(0x5447D0, lambda oc,a: events.append(["gate",*a]) or (0x80000000 if case["gate"] else 0), "thiscall", 3)
    o.hook(0x426170, lambda oc,a: case["precache"], "cdecl", 1)
    def set_field(oc,a): w.obj = a[2]
    o.hook(0x42A9E0, set_field, "cdecl", 3)
    def lookup(oc,a):
        events.append(["lookup",*a])
        return target if case["target"] else 0
    o.hook(0x536820, lookup, "thiscall", 3)
    o.hook(0x48A8F0, lambda oc,a: events.append(["release","node" if oc.reg("ecx")==w.obj else oc.reg("ecx"),False]), "thiscall", 0)
    o.hook(0x48A8D0, lambda oc,a: events.append(["release","node" if oc.reg("ecx")==w.obj else oc.reg("ecx"),True]), "thiscall", 0)
    o.hook(0x48A920, lambda oc,a: events.append(["cache","node" if oc.reg("ecx")==w.obj else oc.reg("ecx"),*a]) or 1.0, "thiscall", 2)
    o.hook(0x48D860, lambda oc,a: events.append(["duplicate","node" if oc.reg("ecx")==w.obj else oc.reg("ecx"),0x777 if a[0]==argref else a[0]]), "thiscall", 1)
    o.hook(0x48E7F0, lambda oc,a: events.append(["scale","node" if oc.reg("ecx")==w.obj else oc.reg("ecx"),*o.read_u32s(a[0],3)]), "thiscall", 1)
    def wrap(oc,a):
        events.append(["wrap",labels[o.read_u32(a[1]+0xC)]])
        return 0x7777
    o.hook(0x47D300, wrap, "cdecl", 2)
    o.hook(0x544F40, lambda oc,a: events.append(["recolour"]), "thiscall", 1)
    def make(sound):
        def call(oc,a):
            events.append(["make",sound,0x777 if a[0]==argref else a[0],oc.read_cstr(a[1],256).decode(),a[2]])
            return target if case["target"] else 0
        return call
    o.hook(0x539170,make(False),"cdecl",3)
    o.hook(0x539230,make(True),"cdecl",3)
    if name in ("makeTexture","makeSound"): o.hooks.pop(0x551220,None)
    argref = o.alloc(0x20)
    o.write_u32(argref + 0xC, target if case["target"] else 0)
    text = _alloc_cstr(o,b"paint.dds")
    arg = argref if case["arg"] else 0
    params = []
    if name == "setI": params = [_cell(o,"I",0xBEEF)]
    elif name in ("setRef","duplicate"): params = [_cell(o,"L",arg)]
    elif name in ("makeTexture","makeSound"): params = [_cell(o,"L",text if case["text"] else 0), _cell(o,"L",arg)]
    elif name in ("setFlags","clearFlags"): params = [_cell(o,"I",case["mask"])]
    elif name == "scaleMesh": params = [_cell(o,"F",b) for b in reversed([0x3fc00000,0xc0200000,0x80000000])]
    addr,sig = RESOURCE_NATIVES[name]
    frame = _frame(w,sig,params+[_cell(o,"L",1)],cls=b"java.util.resource.ResourceRef" if name not in ("getFlags","setFlags","clearFlags","setActiveCollision") else b"java.util.resource.GameRef")
    try:
        result = o.call(addr,[frame],"cdecl",limit=500_000)
    except Exception as e:
        # Preserve precise fault evidence; only documented original null
        # dereferences may be excluded by the replay.
        return {"fault":str(e)}, events
    state = {"node":w.obj != 0, "id": o.read_u32(w.obj+8) if w.obj else None,
             "bound":bool(o.read_u32(w.obj+0xC)) if w.obj else None,
             "flags":o.read_u32(ref+0x54),
             "source_head": bool(o.read_u32(ref+0x48)),
             "target_head":bool(o.read_u32(target+0x48))}
    if sig.endswith(b")I"): state["ret"] = result.eax
    elif sig.endswith(b";"): state["ret"] = result.eax != 0
    return state, events



def wrapper_cases(name):
    return [{"type": ty,"bound":bound} for ty in (0,1,2,3,8,14,0xFFFFFFFF) for bound in (True,False)]


def run_wrapper(name,case,exe=None):
    w=World(exe)
    o=w.o
    node=o.alloc(0x10)
    container=o.alloc(0x100)
    o.write_u32(container+0x4C,case["type"])
    o.write_u32(node+0xC,container if case["bound"] else 0)
    frame=o.alloc(4)
    events=[]
    def load(oc,args):
        events.append(["load",oc.read_cstr(args[0],128).decode()])
        return 0x7001
    o.hook(0x410890,load,"thiscall",1)
    o.hook(0x407DA0,lambda oc,args:events.append(["construct",*args]),"thiscall",2)
    stores=[]
    o.hook(0x42A9E0,lambda oc,args:stores.append(args),"cdecl",3)
    result=o.call(0x47D300,[frame,node],"cdecl",limit=500_000)
    return {"exists":result.eax!=0,"events":events,
            "stores":[[args[0]==result.eax,args[1],args[2]==node] for args in stores]},[]


SCENARIOS = [
    ("anim", n, ANIM_NATIVES[n][0], "cdecl", ANIM_NATIVES[n][1],
     anim_cases(n)) for n in ANIM_NATIVES
] + [
    ("psys", n, PSYS_NATIVES[n][0], "cdecl", PSYS_NATIVES[n][1],
     psys_cases(n)) for n in PSYS_NATIVES
] + [
    ("gate", n, GATE_NATIVES[n][0], "cdecl", GATE_NATIVES[n][1],
     gate_cases(n)) for n in GATE_NATIVES
] + [
    ("handles", n, HANDLE_NATIVES[n][0], "cdecl", HANDLE_NATIVES[n][1],
     handle_cases(n)) for n in HANDLE_NATIVES
]

SCENARIOS += [("game", n, a, "cdecl", sig, game_cases(n))
              for n, (a, sig) in GAME_NATIVES.items()]

SCENARIOS += [("resource", n, a, "cdecl", sig, resource_cases(n))
              for n, (a, sig) in RESOURCE_NATIVES.items()]

SCENARIOS += [("wrapper","factory",0x47D300,"cdecl",b"(II)I",wrapper_cases("factory"))]

def _run(group, name, case, exe):
    if group == "wrapper":
        return run_wrapper(name,case,exe)
    if group == "resource":
        return run_resource(name,case,exe)
    if group == "game":
        return run_game(name, case, exe)
    if group == "handles":
        return run_handle(name, case, exe)
    if group == "anim":
        return run_anim(name, case, exe)
    if group == "psys":
        return run_psys(name, case, exe)
    if group == "gate":
        return run_gate(name, case, exe)
    raise KeyError(group)


def _write(path: str, rows: list) -> None:
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    groups = [s for s in SCENARIOS if only is None or s[0] == only]
    for group, name, addr, conv, sig, cases in groups:
        rows = []
        for i, case in enumerate(cases):
            result, events = _run(group, name, case, exe)
            rows.append({
                "id": i, "exe_addr": f"0x{addr:08x}", "name": name,
                "convention": conv, "sig": sig.decode(), "args": case,
                "result": result, "events": events,
                "oracle": "unicorn", "unicorn": UC_VER,
                "cw": f"0x{CW:04x}", "date": _today()})
        path = os.path.join(OUT_DIR, f"res_{group}_{name}.jsonl")
        _write(path, rows)
        total += len(rows)
        print(f"{group}: {len(rows)} rows -> {path}")
    print(f"total {total} rows, {len(groups)} groups")


def verify(files, exe=None):
    bad = n = 0
    for path in files:
        with open(path) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        stem = os.path.basename(path)[len("res_"):-len(".jsonl")]
        group = stem if "_" not in stem else stem.split("_")[0]
        for row in rows:
            n += 1
            result, events = _run(group, row["name"], row["args"], exe)
            if result != row["result"] or events != row["events"]:
                bad += 1
                print(f"MISMATCH {path}:{row['id']} {row['name']}:\n"
                      f"  want {(row['result'], row['events'])}\n"
                      f"  got  {(result, events)}")
        print(f"{path}: ok")
    print(f"verify: {n - bad}/{n} identical")
    return 1 if bad else 0


def main(argv=None):
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("gen", "anim", "psys", "gate", "handles", "game", "resource", "wrapper"):
        g = sub.add_parser(name)
        g.add_argument("--exe", default=None)
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    a = ap.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe)
        return 0
    if a.cmd in ("anim", "psys", "gate", "handles", "game", "resource", "wrapper"):
        gen(a.exe, only=a.cmd)
        return 0
    files = a.files or sorted(os.path.join(OUT_DIR, f) for f in
                              os.listdir(OUT_DIR) if f.startswith("res_") and f.endswith(".jsonl"))
    return verify(files, a.exe)


if __name__ == "__main__":
    sys.exit(main())
