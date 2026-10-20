"""Bounded passive build606 object allocation/GC/enqueue/finalization history.

This observer reads original execution only. It does not retain, release, mark,
enqueue or finalize objects, and does not replace original code or results.
"""


class FinalizerHistory:
    def __init__(self, observer, limit=50000):
        self.o = observer
        self.limit = limit
        self.events = []
        self.total = 0
        self.allocations = {}
        self.allocation_total = 0
        self.strings_pending = {}
        for address in (0x407DA0, 0x408500, 0x408560, 0x408720, 0x408780,
                        0x417F80, 0x418110, 0x418290, 0x418970, 0x418C20):
            observer.hook(address, self.observe)
        observer.hook(0x4174A0, self.string_enter)
        for address in (0x417502, 0x41755E, 0x417565):
            observer.hook(address, self.string_return)

    def string_enter(self, uc, address, size, user):
        o = self.o
        stack = o.reg("esp")
        pointer = o.u32(stack + 4)
        self.strings_pending[stack] = {
            "text": o.cstr(pointer, 4096) if pointer else None,
            "caller": o.u32(stack), "icount": o.host.icount,
        }

    def string_return(self, uc, address, size, user):
        o = self.o
        call = self.strings_pending.pop(o.reg("esp"))
        allocation = self.allocations.get(o.reg("eax"))
        if allocation is not None:
            allocation["string_text"] = call["text"]
            allocation.setdefault("first_intern", call)

    def record(self, event):
        self.total += 1
        if len(self.events) < self.limit:
            self.events.append(event)

    def class_spelling(self, cls):
        name = self.o.class_name(cls)
        if name and name != "?":
            return name
        record = self.o.try_u32(cls + 8)
        pointer = self.o.try_u32(record) if record else None
        descriptor = self.o.cstr(pointer) if pointer else None
        return descriptor or name

    def observe(self, uc, address, size, user):
        o = self.o
        this, stack = o.reg("ecx"), o.reg("esp")
        context = {"icount": o.host.icount, "caller": o.u32(stack),
                   "native_count": o.stats["native_calls"],
                   "loop_iterations": o.loop_iterations}
        if address == 0x407DA0:
            self.allocation_total += 1
            record = {**context, "word": this,
                      "class": self.class_spelling(o.u32(stack + 4)),
                      "ordinal": self.allocation_total}
            if len(self.allocations) < self.limit:
                self.allocations[this] = record
            return
        if address in (0x408500, 0x408560, 0x408720, 0x408780):
            self.record({**context, "operation": {0x408500: "enqueue", 0x408560: "finalize",
                          0x408720: "disable_gc", 0x408780: "enable_gc"}[address],
                         "word": this, "class": self.class_spelling(o.u32(this + 12)),
                         "flags": o.u32(this + 24),
                         "allocation": self.allocations.get(this)})
            return
        context.update(operation={0x417F80: "explicit_gc", 0x418110: "native_roots",
                                  0x418290: "thread_roots", 0x418970: "mark_step",
                                  0x418C20: "drain"}[address],
                       jvm=this, gc_context=o.u32(this + 0x20),
                       allocation_counter=o.u32(this + 0x34),
                       collection_threshold=o.u32(this + 0x38),
                       drain_quota=o.u32(this + 0x3C))
        vector = context["gc_context"]
        context["mark_pending"] = o.u32(vector + 8) if vector else None
        self.record(context)

    def sidecar(self):
        return {"alters_execution": False, "total": self.total, "events": self.events,
                "allocation_total": self.allocation_total,
                "allocation_records": len(self.allocations),
                "allocations": sorted(self.allocations.values(), key=lambda record: record["ordinal"]),
                "strings_pending": len(self.strings_pending),
                "truncated": self.total > len(self.events) or self.allocation_total > self.limit}
