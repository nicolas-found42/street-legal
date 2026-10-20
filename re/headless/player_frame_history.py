"""Bounded passive nativeplayer frame state; never changes original execution."""

GET_INFO = 0x47B220
CONTROL = 0x47A5F0
ANIMATE = 0x47A780
QUERY = 0x426470

class PlayerFrameHistory:
    def __init__(self, observer, limit=4096):
        if limit < 1:
            raise ValueError("player frame history limit must be positive")
        self.o = observer
        self.limit = limit
        self.events = []
        self.total = 0
        self.last_query = None
        self.owners = {}
        observer.hook(QUERY, self.on_query)
        for address in (GET_INFO, CONTROL, ANIMATE):
            observer.hook(address, self.on_entry)

    def on_query(self, uc, address, size, user):
        o = self.o
        stack = o.reg("esp")
        handle = o.try_u32(stack + 4)
        self.last_query = {"handle":handle, "kind":o.try_u32(stack + 8),
            "id":o.try_u32(handle + 8) if handle else None,
            "container":o.try_u32(handle + 12) if handle else None}

    def on_entry(self, uc, address, size, user):
        self.total += 1
        if len(self.events) >= self.limit:
            return
        o = self.o
        stack = o.reg("esp")
        body = o.try_u32(stack + 4)
        if address == GET_INFO:
            self.owners[body] = self.last_query
        self.events.append({"address":address,"icount":o.host.icount,
            "frame":o.loop_iterations,"body":body,"argument":o.try_u32(stack + 8),
            "caller":o.try_u32(stack),"native_count":o.stats["native_calls"],
            "resource":self.last_query if address == GET_INFO else self.owners.get(body),
            "fields":{hex(offset):o.try_u32(body + offset) if body else None
                for offset in (0x230c,0x21ac,0x250c,0x2424)},
            "render_refs":[o.try_u32(body + 0x2468 + index * 16) if body else None
                for index in range(8)]})

    def sidecar(self):
        return {"alters_execution":False,"events":self.events,"total":self.total,
            "truncated":self.total > len(self.events)}
