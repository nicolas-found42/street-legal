"""Bounded passive build-606 resource ID history, attached to Observer.

Entry/return hooks read the original allocator's arguments, result and free
stack counters. Registration hooks associate runtime IDs with descriptor types.
No registers, memory, original return values or instruction pointers are changed.
"""

ALLOCATE = 0x536ED0
ALLOCATE_RETURNS = (0x536F59, 0x536F86)
RECYCLE = 0x536F90
RECYCLE_RETURN = 0x536FF3
REGISTER_ROOT = 0x536890
REGISTER_CHILD = 0x536900


class ResourceHistory:
    def __init__(self, observer, limit=50000):
        self.observer = observer
        self.limit = limit
        self.events = []
        self.total = 0
        self.pending = {}
        observer.hook(ALLOCATE, self.on_enter)
        observer.hook(RECYCLE, self.on_enter)
        for address in (*ALLOCATE_RETURNS, RECYCLE_RETURN):
            observer.hook(address, self.on_return)
        observer.hook(REGISTER_ROOT, self.on_register)
        observer.hook(REGISTER_CHILD, self.on_register)

    def record(self, event):
        self.total += 1
        if len(self.events) < self.limit:
            self.events.append(event)

    def context(self):
        o = self.observer
        native = o.native_pending[-1] if o.native_pending else None
        return {
            "icount": o.host.icount,
            "native": None if native is None else f'{native["class"]}.{native["name"]}',
            "native_ordinal": o.stats["native_calls"] + 1 if native else None,
        }

    def on_enter(self, uc, address, size, user):
        o = self.observer
        stack, manager = o.reg("esp"), o.reg("ecx")
        self.pending[stack] = {
            **self.context(),
            "operation": "allocate" if address == ALLOCATE else "recycle",
            "caller": o.u32(stack),
            "manager": manager,
            "before": o.u32(manager + 0xDA4),
            "id": None if address == ALLOCATE else o.u32(stack + 4),
        }

    def on_return(self, uc, address, size, user):
        o = self.observer
        event = self.pending.pop(o.reg("esp"))
        if event["operation"] == "allocate":
            event["id"] = o.reg("eax")
        event["after"] = o.u32(event["manager"] + 0xDA4)
        event["return_icount"] = o.host.icount
        self.record(event)

    def on_register(self, uc, address, size, user):
        o = self.observer
        stack = o.reg("esp")
        resource_id = o.u32(stack + 4)
        if resource_id >> 16 != 0xFFFF:
            return
        descriptor = o.u32(stack + 12)
        self.record({
            **self.context(), "operation": "register", "id": resource_id,
            "caller": o.u32(stack), "owner_id": o.u32(stack + 8),
            "descriptor": [o.u32(descriptor + i * 4) for i in range(5)],
            "parent": o.u32(stack + 16) if address == REGISTER_CHILD else 0,
        })

    def sidecar(self):
        return {
            "alters_execution": False, "events": self.events,
            "total": self.total, "truncated": self.total > len(self.events),
            "pending": len(self.pending),
        }
