#!/usr/bin/env python3
"""Unicorn vectors for the cooperative scheduler driver `FUN_00416940` and the
per-frame tick `FUN_00418d10` (M1-05 T5 / S4, issue #32).

Every row builds a synthetic scheduler object plus a synthetic thread list in
emulator memory, hooks `Thread::run` (`FUN_00420ff0`) to log `(thread index,
slice bits)` and return `0.0`, and drives the real exe function. The row
records the initial priorities/flags/sleep deadlines, the advancing clock
trace the driver consumed, the ordered run trace, the final cursor, each
thread's flags, the threads reaped through the deleting destructor's vtable
slot, and the driver's own return value (`eax`). Tick rows add the finalizer
drain: the quota (`[driver+0x3c]`), the records popped off `[driver+0x44]`,
the splice of `[driver+0x30]` onto `[driver+0x2c]`, the tick counter and the
smoothed frame time.

Scenarios (handoff-M1-05.md S4): empty list, one runnable thread, priority
ordering (0, 5, -3), a sleeping thread that holds and one that wakes, a
suspended thread, stopped-to-dead with and without the guard bit, time
exhaustion, the budget-10 immediate timeout, minPriority filtering, and the
finalizer-queue drain (quota update, clamps, splice boundaries, zero quota).

Harness notes learned in M1-05:
  * the run queue's sentinel list, the thread objects and the finalizer
    records all live in the oracle's own heap arena (`Oracle.alloc`), never
    in the image or the exe's own heap range;
  * the clock hook (`FUN_005516c0`) must advance — a constant clock makes
    `FUN_00416940` spin (a row whose trace runs out raises instead);
  * `Thread::run` must be hooked: the un-hooked game body would interpret
    `[thread+0x0c]` (the frame list) and fault;
  * the deleting destructor (`0x423b60`, the thread vtable's slot 0) is
    hooked so the driver's unlink of a dead thread is observable and does
    not free synthetic memory: the hook performs the list surgery of
    `FUN_0041f4d0` and records which thread was reaped;
  * the drain's `FUN_00408560` is hooked for the same reason (it would run a
    synthetic record's `finalize`), performing the front-of-`+0x44` unlink;
  * the vectors are generated at the D3D9 24-bit control word
    (`CW_D3D9`), which is the precision the port's `SCRIPT_PRECISION` uses,
    so the recorded slice bits are the port's.

Usage:
    python sched_vectors.py gen [--exe PATH] [--only SUBSTR]
    python sched_vectors.py verify [FILE ...] [--exe PATH]

Vectors land in the git-ignored `re/out/sched/sched_vectors.jsonl` as one JSON
object per row. Every float the replay needs (budget, sleep deadlines, the
clock program and the consumed clock trace, each run's slice) is stored as its
raw f32 bit pattern in a `0x…` hex string, the way the differential replay
(`crates/slr-vm/tests/sched_oracle.rs`) compares them.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from oracle import CW_D3D9, Oracle, OracleError  # noqa: E402
import unicorn  # noqa: F401,E402

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "re", "out", "sched")
MAIN_CHECKOUT = "/Users/nicolas/Documents/github/games/street-legal"
UC_VER = "2.1.4"
CW = CW_D3D9

# The exe under test.
DRIVE = 0x416940            # FUN_00416940(driver, float budget, int minPriority)
TICK = 0x418D10             # FUN_00418d10(driver, float budget)
INVOKE = 0x408560           # FUN_00408560(record): unlink + run the finalize
CLOCK = 0x5516C0            # FUN_005516c0: the frame-delta clock
RUN = 0x420FF0              # Thread::run(thread, float slice)
DTOR = 0x423B60             # the thread vtable's slot 0 (deleting destructor)

# Synthetic object tags (values are arbitrary; the driver never dereferences
# them): the sentinel list's own word 0 and the thread object's vtable.
LIST_VT = 0x5F0874
RUNQ_TAG = 0x5F087C
LIST_TAG = 0x5E7418
THREAD_VT = 0x5F098C
REC_TAG = 0x5E7400          # a finalizer record's word 0

# Thread flags (`slr_vm::interp::flags` / handoff-M1-05.md).
RUNNING = 0x04
SLEEPING = 0x08
NOTIFIED = 0x20
STOPPED = 0x40
DEAD = 0x80
GUARD = 0x01

RUN_LIMIT = 2_000_000
LIST_WALK_CAP = 300_000


def _today() -> str:
    return datetime.date.today().isoformat()


def _default_exe() -> str:
    for root in (REPO, MAIN_CHECKOUT, os.environ.get("SLR_REFERENCE", "")):
        if not root:
            continue
        p = os.path.join(root, "reference", "raw", "StreetLegal_Redline.bak.exe")
        if os.path.exists(p):
            return p
    return os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe")


def fbits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def f32(f: float) -> float:
    return struct.unpack("<f", struct.pack("<f", f))[0]


def bhex(v: int) -> str:
    return f"0x{v & 0xFFFFFFFF:08x}"


def unb(s: str) -> int:
    return int(s, 16)


def from_bhex(s: str) -> float:
    return struct.unpack("<f", struct.pack("<I", unb(s)))[0]


class Harness:
    """One fresh emulator instance, one synthetic scheduler object."""

    def __init__(self, row: dict, exe=None, clock_trace=None):
        self.row = row
        self.o = Oracle(exe or _default_exe(), fpu_cw=CW)
        self.o.hook_alloc()
        self.log: dict = {"runs": [], "clock": [], "reaped": [], "pops": []}
        self.index: dict = {}
        self.thread_nodes: list = []
        self.queued_index: dict = {}
        # The clock: either an explicit trace (verify) or an arithmetic
        # program `start + step * read` (gen), both recorded per read.
        self.clock_feed = list(clock_trace) if clock_trace is not None else None
        clk = row["clock"]
        self.clock_start = from_bhex(clk["start"])
        self.clock_step = from_bhex(clk["step"])

    # ------------------------------------------------------------ memory
    def _make_list(self, tag: int) -> int:
        o = self.o
        base = o.alloc(0x20)
        o.write_u32(base, LIST_VT)
        o.write_u32(base + 0x04, tag)
        o.write_u32(base + 0x0C, 0)
        o.write_u32(base + 0x10, tag)
        o.write_u32(base + 0x14, 0)
        o.write_u32(base + 0x18, base + 4)
        o.write_u32(base + 0x08, base + 0x10)
        return base

    def _bodies(self, base: int, n: int, stride: int, tag: int) -> list:
        """Attach `n` nodes to the sentinel list `base`, in list order."""
        o = self.o
        if n:
            arena = o.alloc(n * stride, 16)
            nodes = [arena + stride * i for i in range(n)]
        else:
            nodes = []
        head, tail = base + 4, base + 0x10
        for i, nd in enumerate(nodes):
            o.write_u32(nd, tag)
            o.write_u32(nd + 4, nodes[i + 1] if i + 1 < n else tail)
            o.write_u32(nd + 8, nodes[i - 1] if i > 0 else head)
        o.write_u32(head + 4, nodes[0] if nodes else tail)
        o.write_u32(tail + 8, nodes[-1] if n else head)
        o.write_u32(base + 0x08, nodes[0] if nodes else tail)
        o.write_u32(base + 0x18, nodes[-1] if n else head)
        return nodes

    def _read_list(self, base: int) -> list:
        """The node addresses of the sentinel list `base`, head to tail."""
        o = self.o
        stop = base + 0x10
        out, cur = [], o.read_u32(base + 0x08)
        while cur != stop:
            out.append(cur)
            cur = o.read_u32(cur + 4)
            if len(out) > LIST_WALK_CAP:
                raise OracleError(f"list {base:#x} walk exceeded {LIST_WALK_CAP}")
        return out

    def _unlink_from(self, base: int, node: int) -> None:
        """The list surgery of `FUN_0041f4d0` for one node of `base`."""
        o = self.o
        prev, nxt = o.read_u32(node + 8), o.read_u32(node + 4)
        o.write_u32(prev + 4, nxt)
        o.write_u32(nxt + 8, prev)
        if o.read_u32(base + 0x08) == node:
            o.write_u32(base + 0x08, nxt)
        if o.read_u32(base + 0x18) == node:
            o.write_u32(base + 0x18, prev)

    # ------------------------------------------------------------ hooks
    def _clock(self, o: Oracle, a: list) -> float:
        if self.clock_feed is not None:
            if not self.clock_feed:
                raise OracleError("clock trace exhausted")
            bits = unb(self.clock_feed.pop(0))
        else:
            i = len(self.log["clock"])
            bits = fbits(f32(self.clock_start + self.clock_step * i))
        self.log["clock"].append(bits)
        return struct.unpack("<f", struct.pack("<I", bits))[0]

    def _run(self, o: Oracle, a: list) -> float:
        th = o.reg("ecx")
        i = self.index.get(th)
        if i is None:
            raise OracleError(f"Thread::run on unknown thread {th:#x}")
        self.log["runs"].append([i, a[0]])
        mask = unb(self.row["threads"][i]["run_mask"])
        if mask:
            o.write_u32(th + 0x2C, o.read_u32(th + 0x2C) | mask)
        return 0.0

    def _dtor(self, o: Oracle, a: list) -> None:
        th = o.reg("ecx")
        i = self.index.get(th)
        if i is None:
            raise OracleError(f"deleting destructor on unknown thread {th:#x}")
        self.log["reaped"].append(i)
        self._unlink_from(self.runq, th)

    def _invoke(self, o: Oracle, a: list) -> int:
        rec = o.reg("ecx")
        i = self.queued_index.get(rec)
        if i is None:
            raise OracleError(f"finalizer run on unknown record {rec:#x}")
        self.log["pops"].append(i)
        self._unlink_from(self.L44, rec)
        return 0

    # ------------------------------------------------------------ build
    def build(self) -> None:
        o, row = self.o, self.row
        d = o.alloc(0x100)
        self.driver = d
        threads = row["threads"]
        self.runq = self._make_list(RUNQ_TAG)
        self.thread_nodes = self._bodies(self.runq, len(threads), 0x40, THREAD_VT)
        self.index = {nd: i for i, nd in enumerate(self.thread_nodes)}
        for i, spec in enumerate(threads):
            th = self.thread_nodes[i]
            o.write_i32(th + 0x28, spec["priority"])
            o.write_u32(th + 0x2C, unb(spec["flags"]))
            o.write_u32(th + 0x30, unb(spec["sleep_deadline"]))
        o.write_u32(d + 0x18, self.runq)
        cur = row["cursor_in"]
        o.write_u32(d + 0x78, self.thread_nodes[cur] if cur is not None else 0)
        if row["kind"] == "tick":
            f = row["finalizers"]
            self.L44 = self._make_list(LIST_TAG)
            self.L30 = self._make_list(LIST_TAG)
            self.L2C = self._make_list(LIST_TAG)
            self.queued_index = {
                nd: i
                for i, nd in enumerate(self._bodies(self.L44, f["queued"], 0x20, REC_TAG))
            }
            self._bodies(self.L30, f["spare"], 0x20, REC_TAG)
            self._bodies(self.L2C, f["held"], 0x20, REC_TAG)
            o.write_u32(d + 0x44, self.L44)
            o.write_u32(d + 0x30, self.L30)
            o.write_u32(d + 0x2C, self.L2C)
            o.write_i32(d + 0x3C, f["divisor"])
            o.write_u32(d + 0x40, f["ticks"])
            o.write_i32(d + 0x24, 0)
            o.write_i32(d + 0x34, f["callback_ticks"])
            o.write_i32(d + 0x38, f["callback_period"])
            # The callback block's frame table (`+0x08`) and argument buffer
            # (`+0x20`) are left zero (and the gate is closed by
            # `callback_ticks <= callback_period`): that block is S7's.
            arg = o.alloc(0x10)
            o.write_u32(arg, 0x40)
            o.write_u32(arg + 4, o.alloc(0x100))
            o.write_u32(arg + 8, 0)
            o.write_u32(d + 0x10, self._make_list(LIST_TAG))
            o.write_u32(d + 0x20, arg)
        o.hook(CLOCK, self._clock, "cdecl", 0)
        o.hook(RUN, self._run, "thiscall", 1)
        o.hook(DTOR, self._dtor, "thiscall", 1)
        if row["kind"] == "tick":
            o.hook(INVOKE, self._invoke, "thiscall", 0)

    # ------------------------------------------------------------ run
    def call(self):
        row, d = self.row, self.driver
        budget = from_bhex(row["budget"])
        if row["kind"] == "tick":
            args = [("f32", budget)]
            va = TICK
        else:
            args = [("f32", budget), ("i32", row["min_priority"])]
            va = DRIVE
        return self.o.call(va, args=args, convention="thiscall", this=d,
                           fpu_cw=CW, limit=RUN_LIMIT)

    def snapshot(self, res) -> dict:
        o, d, row = self.o, self.driver, self.row
        cur = o.read_u32(d + 0x78)
        if cur == 0:
            cursor = None
        elif cur in self.index:
            cursor = self.index[cur]
        else:
            raise OracleError(f"cursor {cur:#x} is not a synthetic thread")
        exp = {
            "runs": [[i, bhex(bits)] for i, bits in self.log["runs"]],
            "clock_reads": len(self.log["clock"]),
            "flags_after": [bhex(o.read_u32(th + 0x2C)) for th in self.thread_nodes],
            "sleep_after": [bhex(o.read_u32(th + 0x30)) for th in self.thread_nodes],
            "cursor_after": cursor,
            "reaped": list(self.log["reaped"]),
            "return": res.eax & 0xFF,
        }
        if row["kind"] == "tick":
            exp["ticks"] = o.read_u32(d + 0x40)
            exp["divisor"] = o.read_i32(d + 0x3C)
            exp["elapsed_ms"] = o.read_i32(d + 0x24)
            exp["callback_ticks_after"] = o.read_i32(d + 0x34)
            exp["finalizer_pops"] = list(self.log["pops"])
            exp["finalizer_after"] = {
                "queued": len(self._read_list(self.L44)),
                "spare": len(self._read_list(self.L30)),
                "held": len(self._read_list(self.L2C)),
            }
        return exp


def run_row(row: dict, exe=None) -> dict:
    """Replay one row with a fresh emulator; returns the row plus evidence."""
    h = Harness(row, exe, clock_trace=row.get("clock_trace"))
    h.build()
    res = h.call()
    out = dict(row)
    out["clock_trace"] = [bhex(b) for b in h.log["clock"]]
    out["expect"] = h.snapshot(res)
    return out


# ---------------------------------------------------------------- rows

def _thread(priority=0, flags=0, deadline=0.0, run_mask=0x40) -> dict:
    return {
        "priority": priority,
        "flags": bhex(flags),
        "sleep_deadline": bhex(fbits(deadline)),
        "run_mask": bhex(run_mask),
    }


def _row(name, threads, kind="drive", budget=0.0, min_priority=0, cursor_in=None,
         clock=(0.0, 1.0), finalizers=None, notes="") -> dict:
    return {
        "name": name,
        "kind": kind,
        "exe_addr": bhex(TICK if kind == "tick" else DRIVE),
        "budget": bhex(fbits(budget)),
        "min_priority": min_priority,
        "cursor_in": cursor_in,
        "clock": {"start": bhex(fbits(clock[0])), "step": bhex(fbits(clock[1]))},
        "threads": threads,
        "finalizers": finalizers,
        "notes": notes,
    }


def _fin(ticks=0, divisor=4, queued=0, spare=0, held=0,
         callback_ticks=0, callback_period=1000) -> dict:
    return {
        "ticks": ticks,
        "divisor": divisor,
        "queued": queued,
        "spare": spare,
        "held": held,
        "callback_ticks": callback_ticks,
        "callback_period": callback_period,
    }


def build_rows() -> list:
    P0 = _thread(0)
    rows = []

    # -- drive: list emptiness and the walk's exits
    rows.append(_row("drive_empty_untimed", [],
                     notes="no deadline: one clock read (entry), ListEnd=0"))
    rows.append(_row("drive_empty_timed", [], budget=5.0, clock=(0.0, 10.0),
                     notes="deadline 5.0, first loop read 10.0 -> TimeUp=1"))
    rows.append(_row("drive_one_runnable", [dict(P0)],
                     notes="priority 0, flags 0, slice 10.0, stops and is reaped"))
    rows.append(_row("drive_priorities_0_5_m3",
                     [_thread(0), _thread(5), _thread(-3)], min_priority=-5,
                     notes="walk is list order; slices 10.0 / 505.0 / 7.3"))
    rows.append(_row("drive_cursor_resume", [_thread(0), _thread(0)], cursor_in=1,
                     notes="cursor [driver+0x78] resumes at index 1"))

    # -- drive: sleep
    rows.append(_row("drive_sleeping_holds", [_thread(0, SLEEPING, 100.0)],
                     notes="clock 1.0 < deadline 100.0: no run, TimeUp=1"))
    rows.append(_row("drive_sleeping_wakes", [_thread(0, SLEEPING, 0.5)],
                     notes="clock 1.0 > deadline 0.5: bit 8 cleared, then runs"))
    rows.append(_row("drive_sleeping_holds_then_runs",
                     [_thread(0, SLEEPING, 100.0), _thread(0)],
                     notes="the walk continues past a held sleeping thread"))

    # -- drive: suspend / stop / dead
    rows.append(_row("drive_suspended", [_thread(0, NOTIFIED)],
                     notes="0x20 is not a skip; a suspended thread in the queue runs"))
    rows.append(_row("drive_suspended_sleeping_wakes", [_thread(0, NOTIFIED | SLEEPING, 0.5)],
                     notes="0x28: sleep bit cleared, 0x20 kept"))
    rows.append(_row("drive_stopped_dead", [_thread(0, STOPPED)],
                     notes="0x40 without the 0x01 guard -> 0x80, reaped, ListEnd=0"))
    rows.append(_row("drive_stopped_guarded", [_thread(0, STOPPED | GUARD)],
                     notes="0x41 keeps the thread alive; no run -> TimeUp=1"))
    rows.append(_row("drive_stopped_dead_mixed",
                     [_thread(0), _thread(0, STOPPED), _thread(0)],
                     notes="run/skip/run walk order"))
    rows.append(_row("drive_dead_entry", [_thread(0, DEAD)],
                     notes="0x80 blocks at 0x82: no run, reaped, TimeUp=1"))
    rows.append(_row("drive_running_skipped", [_thread(0, RUNNING)],
                     notes="0x04 is a skip mask (0x44): no run, no death, TimeUp=1"))
    rows.append(_row("drive_created_skipped", [_thread(0, 0x02)],
                     notes="0x02 is a skip mask (0x82): no run, no death, TimeUp=1"))

    # -- drive: time
    rows.append(_row("drive_time_exhaustion",
                     [_thread(0, run_mask=0), _thread(0, run_mask=0)],
                     budget=5.0, clock=(0.0, 1.0),
                     notes="budget 5.0 (< 10.0) becomes slice_unit; TimeUp at 5.0"))
    rows.append(_row("drive_budget10_immediate_timeup", [_thread(0)],
                     budget=10.0, clock=(0.0, 10.0),
                     notes="clock 0 -> 10 with budget 10.0: TimeUp with no run"))

    # -- drive: the priority floor
    rows.append(_row("drive_minpriority_filter", [_thread(0), _thread(-1)], min_priority=0,
                     notes="-1 is under the floor; skipped, never reaped"))
    rows.append(_row("drive_minpriority_floor", [_thread(0), _thread(-1)], min_priority=-5,
                     notes="floor -5 admits both"))

    # -- tick: the finalizer drain
    rows.append(_row("tick_fairness_quota_low_clamp", [dict(P0), dict(P0)],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=255, divisor=17, queued=1, spare=1000),
                     notes="tick 256 updates the quota: (1000/17+1)>>6 = 0 -> clamp 4; "
                           "1 < 3 pops -> splice 1000 onto +0x2c"))
    rows.append(_row("tick_fairness_quota_high_clamp", [dict(P0), dict(P0)],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=255, divisor=4, queued=1, spare=20000),
                     notes="(20000/4+1)>>6 = 78 -> clamp 0x40; pops 1 < 63 -> splice"))
    rows.append(_row("tick_fairness_no_update", [],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=0, divisor=4, queued=4, spare=3),
                     notes="tick 1: low byte != 0, divisor stays 4; pops 4 == quota: "
                           "no splice"))
    rows.append(_row("tick_fairness_all_but_one", [],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=0, divisor=4, queued=3, spare=2),
                     notes="pops 3 == quota-1: no splice"))
    rows.append(_row("tick_fairness_splice", [],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=0, divisor=4, queued=2, spare=2),
                     notes="pops 2 < 3: whole +0x30 spliced onto the +0x2c tail"))
    rows.append(_row("tick_fairness_quota_zero", [],
                     kind="tick", budget=0.0,
                     finalizers=_fin(ticks=0, divisor=0, queued=0, spare=3),
                     notes="quota 0 with no update tick: pops nothing, still splices"))
    rows.append(_row("tick_budget10", [dict(P0), dict(P0)],
                     kind="tick", budget=10.0,
                     finalizers=_fin(ticks=0, divisor=4),
                     notes="the frame loop's own budget: slice_unit stays 10.0"))
    return rows


# ---------------------------------------------------------------- gen/verify

def _path_for(files):
    if files:
        return list(files)
    return [os.path.join(OUT_DIR, f) for f in sorted(os.listdir(OUT_DIR))
            if f.endswith(".jsonl")]


def gen(exe=None, only=None):
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = build_rows()
    if only:
        rows = [r for r in rows if only in r["name"]]
    if not rows:
        raise SystemExit(f"no row matches {only!r}")
    path = os.path.join(OUT_DIR, "sched_vectors.jsonl")
    out = []
    for i, r in enumerate(rows):
        got = run_row(r, exe)
        got.update({
            "id": i,
            "oracle": "unicorn",
            "unicorn": UC_VER,
            "cw": f"0x{CW:04x}",
            "date": _today(),
        })
        out.append(got)
    with open(path, "w") as f:
        for r in out:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    runs = sum(len(r["expect"]["runs"]) for r in out)
    print(f"{len(out)} rows, {runs} thread runs -> {path}")


def _diff(a, b, path=""):
    out = []
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append(f"{path}.{k}: missing in row")
            elif k not in b:
                out.append(f"{path}.{k}: extra in replay ({b[k]!r})")
            else:
                out += _diff(a[k], b[k], f"{path}.{k}")
    elif a != b:
        out.append(f"{path}: row {a!r} != replay {b!r}")
    return out


def verify(files, exe=None):
    bad = n = 0
    for path in _path_for(files):
        with open(path) as f:
            for line in f:
                if not line.strip():
                    continue
                row = json.loads(line)
                n += 1
                try:
                    got = run_row(row, exe)
                except (OracleError, KeyError, IndexError, struct.error) as e:
                    bad += 1
                    print(f"FAIL {row['name']}: {type(e).__name__}: {e}")
                    continue
                if got["clock_trace"] != row.get("clock_trace"):
                    bad += 1
                    print(f"FAIL {row['name']}: clock trace {got['clock_trace']} "
                          f"!= {row.get('clock_trace')}")
                    continue
                d = _diff(row["expect"], got["expect"])
                if d:
                    bad += 1
                    print(f"FAIL {row['name']}:")
                    for ln in d:
                        print(f"  {ln}")
    print(f"{n - bad}/{n} rows verified (unicorn {UC_VER})")
    return 1 if bad else 0


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--exe", default=None)
    g.add_argument("--only", default=None, help="substring filter on row names")
    v = sub.add_parser("verify")
    v.add_argument("files", nargs="*")
    v.add_argument("--exe", default=None)
    a = p.parse_args(argv)
    if a.cmd == "gen":
        gen(a.exe, a.only)
        return 0
    return verify(a.files, a.exe)


if __name__ == "__main__":
    sys.exit(main())
