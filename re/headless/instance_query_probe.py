#!/usr/bin/env python3
"""Passive instance registration/query observations on unchanged startup."""
import startup


class InstanceQueryObserver(startup.Observer):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.instance_queries = []
        self.instance_queries_total = 0
        for address in (0x427210, 0x426470, 0x425a90, 0x43e320, 0x448430):
            self.hook(address, self.observe_instance_query)

    def observe_instance_query(self, uc, address, size, user):
        self.instance_queries_total += 1
        if len(self.instance_queries) >= 20000:
            return
        sp = self.reg('esp')
        arg = self.try_u32(sp + 4)
        data = None
        if address in (0x427210, 0x426470):
            container = self.try_u32(arg + 12) if arg else 0
            kind = self.try_u32(container + 0x4c) if container else 0
            if kind == 1:
                data = container + 0xd8
        elif address == 0x425a90:
            data = arg
        else:
            container, kind = None, None
        self.instance_queries.append(dict(address=address, icount=self.host.icount,
            frame=self.loop_iterations, natives=self.stats['native_calls'],
            caller=self.try_u32(sp), argument=arg, kind=kind if address != 0x425a90 else None,
            words=[self.try_u32(sp + 4 + 4 * i) for i in range(6)],
            data=data, fields={hex(offset): self.try_u32(data + offset)
                              for offset in (0xc, 0x44, 0x4c, 0x50)} if data else {}))

    def sidecar(self):
        return {**super().sidecar(), 'instance_queries': dict(alters_execution=False,
            total=self.instance_queries_total, events=self.instance_queries,
            truncated=self.instance_queries_total > len(self.instance_queries))}


if __name__ == '__main__':
    startup.Observer = InstanceQueryObserver
    startup.main()
