"""Passive build606 clock/scheduler investigation; never changes emulated state.

Captures original FUN_005516c0 x87 return bits alongside its exact external
QPC/frequency/baseline words, and surrounding scheduler/Thread::run contexts.
Default observation window is 99M..110M block-counted instructions. Context
tracking starts earlier so a function spanning the window remains attributable.
This is evidence for a clock-input replay design, not a replay or scenario edit.
"""
import struct
from unicorn import x86_const as X

CLOCK = 0x5516C0
CLOCK_QPC_COMPLETE = 0x5516D6
CLOCK_RETURN = 0x5516F2
ENTRIES = {0x418D10: ('tick', 1), 0x416940: ('drive', 2), 0x420FF0: ('thread', 1)}
RETURNS = {0x418E9E: 'tick', 0x416AEC: 'drive', 0x416AF8: 'drive', 0x423966: 'thread'}


class ClockHistory:
    def __init__(self, observer, start=99_000_000, end=110_000_000, limit=50000):
        self.o = observer
        self.start, self.end, self.limit = start, end, limit
        self.events = []
        self.total = 0
        self.pending_clock = {}
        self.contexts = {}
        self.next_id = 0
        self.errors = []
        for address in ENTRIES:
            observer.hook(address, self.on_enter)
        for address in RETURNS:
            observer.hook(address, self.on_return)
        observer.hook(CLOCK, self.on_clock)
        observer.hook(CLOCK_QPC_COMPLETE, self.on_qpc)
        observer.hook(CLOCK_RETURN, self.on_clock_return)

    def key(self):
        return (self.o.host.thread_id, self.o.reg('esp'))

    def visible(self):
        return self.start <= self.o.host.icount <= self.end

    def record(self, event):
        if not self.visible():
            return
        self.total += 1
        if len(self.events) < self.limit:
            self.events.append({'ordinal': self.total, 'icount': self.o.host.icount,
                                'os_thread': self.o.host.thread_id, **event})

    def u64(self, address):
        low, high = self.o.try_u32(address), self.o.try_u32(address + 4)
        return None if low is None or high is None else low | high << 32

    def thread(self, address):
        o = self.o
        if not address:
            return None
        name = o.try_u32(address + 0x1C)
        frame = o.try_u32(address + 0x20)
        cls = o.try_u32(frame + 0x34) if frame else None
        method = o.try_u32(frame + 0x38) if cls else None
        node = o.try_u32(address + 0x24)
        entry = getattr(o, 'frame_entries', {}).get(frame)
        node_index = (node - entry) // 8 if node and entry and node >= entry else None
        return {'address': address, 'name': o.cstr(name) if name else None,
                'jvm': o.try_u32(address + 0x10),
                'priority': o.try_u32(address + 0x28), 'flags': o.try_u32(address + 0x2C),
                'sleep_bits': o.try_u32(address + 0x30), 'node': node, 'node_index': node_index,
                'frame': frame, 'class': o.class_name(cls) if cls else None,
                'method': o.cstr(method) if method else None}

    def driver(self, address):
        o = self.o
        cursor = o.try_u32(address + 0x78)
        queues = {}
        for name, offset in (('ready', 0x18), ('waiting', 0x1C)):
            items, valid = o.menu_history.chain(o.try_u32(address + offset), 4096)
            queues[name] = {'valid': valid, 'threads': [self.thread(t) for t in items]}
        return {'address': address, 'cursor': self.thread(cursor), 'queues': queues,
                'ticks': o.try_u32(address + 0x40)}

    def active(self):
        return self.contexts.setdefault(self.o.host.thread_id, [])

    def on_enter(self, uc, address, size, user):
        o = self.o
        kind, nargs = ENTRIES[address]
        self.next_id += 1
        context = {'id': self.next_id, 'kind': kind, 'stack': o.reg('esp'),
                   'receiver': o.reg('ecx'), 'caller': o.try_u32(o.reg('esp')),
                   'arguments': [o.try_u32(o.reg('esp') + 4 * (i + 1)) for i in range(nargs)]}
        self.active().append(context)
        if self.visible():
            state = self.thread(context['receiver']) if kind == 'thread' else self.driver(context['receiver'])
            self.record({'kind': kind + '_enter', 'context': dict(context), 'state': state})

    def on_return(self, uc, address, size, user):
        kind, stack = RETURNS[address], self.o.reg('esp')
        active = self.active()
        if not active or active[-1]['kind'] != kind or active[-1]['stack'] != stack:
            if self.visible():
                self.errors.append({'kind': kind, 'stack': stack, 'icount': self.o.host.icount})
            return
        context = active.pop()
        if self.visible():
            state = self.thread(context['receiver']) if kind == 'thread' else self.driver(context['receiver'])
            self.record({'kind': kind + '_return', 'context_id': context['id'],
                         'eax': self.o.reg('eax'), 'state': state})

    def on_clock(self, uc, address, size, user):
        if not self.visible():
            return
        o = self.o
        self.pending_clock[self.key()] = {'kind': 'clock', 'caller': o.try_u32(o.reg('esp')),
            'entry_icount': o.host.icount, 'contexts': [dict(c) for c in self.active()]}
        threads = [c for c in self.active() if c['kind'] == 'thread']
        if threads:
            self.pending_clock[self.key()]['thread_state'] = self.thread(threads[-1]['receiver'])
        drives = [c for c in self.active() if c['kind'] == 'drive']
        if drives:
            self.pending_clock[self.key()]['driver_cursor'] = self.thread(o.try_u32(drives[-1]['receiver'] + 0x78))

    def on_qpc(self, uc, address, size, user):
        event = self.pending_clock.get(self.key())
        if event is not None:
            event['external_input'] = {'counter': self.u64(0x76A290),
                'frequency': self.u64(0x76A280), 'baseline': self.u64(0x76A288),
                'completed_icount': self.o.host.icount, 'elapsed_ms': self.o.host.elapsed_ms}

    def on_clock_return(self, uc, address, size, user):
        event = self.pending_clock.pop(self.key(), None)
        if event is None:
            return
        o = self.o
        top = (o.reg('fpsw') >> 11) & 7
        mantissa, exponent = o.uc.reg_read(X.UC_X86_REG_FP0 + top)
        event.update(return_st0_le=struct.pack('<QH', mantissa, exponent).hex(),
                     fpcw=o.reg('fpcw'), fpsw=o.reg('fpsw'))
        self.record(event)

    def sidecar(self):
        return {'alters_execution': False, 'window': [self.start, self.end],
                'events': self.events, 'total': self.total,
                'truncated': self.total > len(self.events),
                'pending_clocks': len(self.pending_clock), 'context_errors': self.errors,
                'active_contexts': self.contexts}
