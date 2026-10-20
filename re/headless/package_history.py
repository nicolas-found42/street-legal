"""Passive build-606 package registration order; no emulated state changes.

FUN_00538380 registers names independently of loading package files. Observe
its original entry and RET, preserving names, first-use indices and callers.
"""

REGISTER_PACKAGE = 0x538380
REGISTER_RETURN = 0x5383EC
CONSTRUCT_PACKAGE = 0x543EB0
LOAD_HEADERS = 0x544170
PACKAGE_COUNT = 0xFFE20


class PackageHistory:
    def __init__(self, observer, limit=50000):
        self.observer = observer
        self.limit = limit
        self.events = []
        self.total = 0
        self.pending = {}
        self.creations = []
        self.creation_total = 0
        self.header_loads = []
        self.header_total = 0
        observer.hook(REGISTER_PACKAGE, self.on_enter)
        observer.hook(REGISTER_RETURN, self.on_return)
        observer.hook(CONSTRUCT_PACKAGE, self.on_construct)
        observer.hook(LOAD_HEADERS, self.on_headers)

    def on_headers(self, uc, address, size, user):
        o = self.observer
        package = o.reg("ecx")
        self.header_total += 1
        if len(self.header_loads) < self.limit:
            self.header_loads.append({"icount": o.host.icount, "caller": o.u32(o.reg("esp")),
                                      "index": o.u32(package + 8) & 0xFFFF,
                                      "name": o.cstr(package + 0xC, 1024),
                                      "state": o.u32(package + 0x50)})

    def on_construct(self, uc, address, size, user):
        o = self.observer
        stack = o.reg("esp")
        self.creation_total += 1
        if len(self.creations) < self.limit:
            self.creations.append({"icount": o.host.icount, "caller": o.u32(stack),
                                   "index": o.u32(stack + 4),
                                   "name": o.cstr(o.u32(stack + 8), 1024)})

    def on_enter(self, uc, address, size, user):
        o = self.observer
        stack, manager = o.reg("esp"), o.reg("ecx")
        native = o.native_pending[-1] if o.native_pending else None
        self.pending[stack] = {
            "icount": o.host.icount,
            "caller": o.u32(stack),
            "manager": manager,
            "name": o.cstr(o.u32(stack + 4), 1024),
            "before": o.u32(manager + PACKAGE_COUNT),
            "native": None if native is None else f'{native["class"]}.{native["name"]}',
            "native_ordinal": o.stats["native_calls"] + 1 if native else None,
        }

    def on_return(self, uc, address, size, user):
        o = self.observer
        event = self.pending.pop(o.reg("esp"))
        event.update(index=o.reg("eax"), after=o.u32(event["manager"] + PACKAGE_COUNT),
                     return_icount=o.host.icount)
        self.total += 1
        if len(self.events) < self.limit:
            self.events.append(event)

    def sidecar(self):
        return {"alters_execution": False, "events": self.events,
                "creations": self.creations, "creation_total": self.creation_total,
                "header_loads": self.header_loads, "header_total": self.header_total,
                "total": self.total, "truncated": (self.total > len(self.events)
                    or self.creation_total > len(self.creations)
                    or self.header_total > len(self.header_loads)),
                "pending": len(self.pending)}
