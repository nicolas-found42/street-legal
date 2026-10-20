"""Passive raw QPC inputs for original slot7 final-frame elapsed time.

Raw inputs are read after QueryPerformanceCounter and before any x87 loads.
No ST0 result is recorded or replayed as an input. Build606 final frame sample
returns to428ed0; vsync/pre-pacing samples have distinct retained caller counts.
"""
import json
from collections import Counter
from pathlib import Path


class FrameTimerHistory:
    def __init__(self, observer, limit=50000):
        self.o = observer
        self.limit = limit
        self.frame = 0
        self.rows = []
        self.total = 0
        self.pending = {}
        self.errors = []
        self.callers = Counter()
        self.resets = []
        for address, method in [(0x552310, self.on_reset), (0x55232D, self.on_reset_done),
                                (0x552330, self.on_elapsed), (0x55233E, self.on_counter)]:
            observer.hook(address, method)

    def u64(self, address):
        return self.o.u32(address) | self.o.u32(address + 4) << 32

    def key(self):
        return (self.o.host.thread_id, self.o.reg("esp"))

    def on_reset(self, uc, address, size, user):
        o = self.o
        if o.u32(o.reg("esp") + 4) == 7:
            self.frame += 1

    def on_reset_done(self, uc, address, size, user):
        o = self.o
        if o.u32(o.reg("esp") + 4) == 7 and len(self.resets) < self.limit:
            self.resets.append({"frame": self.frame, "frequency": self.u64(0x76A298),
                                "start": self.u64(0x76A3A0 + 7 * 8)})

    def on_elapsed(self, uc, address, size, user):
        o = self.o
        stack = o.reg("esp")
        if o.u32(stack + 4) != 7:
            return
        caller = o.u32(stack)
        self.callers[f"0x{caller:08x}"] += 1
        if caller == 0x428ED0:
            self.pending[(o.host.thread_id, stack - 8)] = {
                "frame": self.frame, "caller": caller, "entry_icount": o.host.icount,
            }

    def on_counter(self, uc, address, size, user):
        o = self.o
        row = self.pending.pop(self.key(), None)
        if row is None:
            return
        row.update(counter=self.u64(o.reg("esp")), start=self.u64(0x76A3A0 + 7 * 8),
                   frequency=self.u64(0x76A298), cw=o.reg("fpcw"),
                   completed_icount=o.host.icount)
        if row["frequency"] == 0 or row["counter"] < row["start"]:
            self.errors.append({"kind": "invalid_counter_input", "frame": row["frame"]})
        if row["frame"] != self.total + 1:
            self.errors.append({"kind": "frame_gap", "frame": row["frame"], "previous": self.total})
        self.total += 1
        if len(self.rows) < self.limit:
            self.rows.append(row)

    def write(self, path):
        Path(path).write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in self.rows))

    def sidecar(self):
        return {"alters_execution": False, "raw_inputs_only": True,
                "rows": len(self.rows), "total": self.total, "reset_total": self.frame,
                "resets": self.resets, "slot7_callers": dict(self.callers),
                "pending": len(self.pending), "errors": self.errors,
                "truncated": self.total > len(self.rows) or self.frame > self.limit}
