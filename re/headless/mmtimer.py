"""Declared multimedia timers (winmm): periodic callbacks on a platform timer thread.

`timeSetEvent`, `timeKillEvent`, `timeBeginPeriod`, `timeEndPeriod` and
`timeGetTime` follow the documented winmm contracts: callbacks run on a
separate system thread, periodic timers fire every `delay` milliseconds of the
virtual clock, and the callback receives (uTimerID, uMsg=0, dwUser, 0, 0)
through the stdcall ABI. The timer thread is a platform thread running a small
trampoline in emulator memory that waits for a due timer (a platform thunk) and
then calls the original's callback; no original code is replaced. The virtual
clock advances with instruction counts and, when every thread is blocked, jumps
to the earliest wake time. Timer period limits are declared in the profile.
"""

from __future__ import annotations

import struct

TIMERR_NOCANDO = 97
MMSYSERR_INVALPARAM = 11
TRAMPOLINE = 0x7FFF0400
WAIT_NAME = "__mm_timer_wait"
TIME_PERIODIC = 1


class TimerProfile:
    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"period_min_ms", "period_max_ms"}:
            raise ValueError("timer needs exactly period_min_ms and period_max_ms")
        low, high = data["period_min_ms"], data["period_max_ms"]
        if type(low) is not int or type(high) is not int or not 1 <= low <= high <= 1000000:
            raise ValueError("timer periods must satisfy 1 <= min <= max <= 1000000")
        self.period_min = low
        self.period_max = high


class MultimediaTimers:
    def __init__(self, host, boundary, profile):
        self.host = host
        self.boundary = boundary
        self.profile = profile
        self.timers = {}
        self.next_id = 1
        self.thread = None
        self.periods = {}
        self.fired = 0

    def register(self, startup):
        for name, nargs, handler in (
            ("timeBeginPeriod", 1, self.begin_period),
            ("timeEndPeriod", 1, self.end_period),
            ("timeGetTime", 0, self.get_time),
            ("timeSetEvent", 5, self.set_event),
            ("timeKillEvent", 1, self.kill_event),
            (WAIT_NAME, 0, self.wait),
        ):
            startup.apis[name] = (nargs, handler)
            startup.api_modules[name] = "winmm.dll"
            if startup.enable_all:
                startup.enabled.add(name)
        self.wait_address = startup.allocate_thunk("winmm.dll!" + WAIT_NAME)
        # Trampoline inside the sentinel page, on the timer thread:
        #   L0: call wait ; test eax,eax ; jz L0
        #       push 0 ; push 0 ; push edx(user) ; push 0 ; push eax(id) ; call ecx ; jmp L0
        code = bytearray()
        code += b"\xe8" + struct.pack("<i", self.wait_address - (TRAMPOLINE + 5))
        code += b"\x85\xc0\x74\xf7"
        code += b"\x6a\x00\x6a\x00\x52\x6a\x00\x50\xff\xd1"
        code += b"\xeb" + bytes([(256 - (len(code) + 2)) & 0xFF])
        startup.write(TRAMPOLINE, bytes(code))

    # ------------------------------------------------------------------ API
    def begin_period(self, args):
        period = args[0]
        if not self.profile.period_min <= period <= self.profile.period_max:
            return TIMERR_NOCANDO
        self.periods[period] = self.periods.get(period, 0) + 1
        return 0

    def end_period(self, args):
        period = args[0]
        if not self.profile.period_min <= period <= self.profile.period_max:
            return TIMERR_NOCANDO
        if not self.periods.get(period):
            return TIMERR_NOCANDO
        self.periods[period] -= 1
        return 0

    def get_time(self, args):
        return self.host.clock_ms() & 0xFFFFFFFF

    def set_event(self, args):
        delay, resolution, callback, user, flags = args
        host = self.host
        if flags & ~TIME_PERIODIC:
            raise self.boundary("unsupported_argument", api="timeSetEvent", args=args)
        if not 1 <= delay <= 65535 or resolution > 65535 or not callback:
            return 0
        if not host.text_range[0] <= callback < host.text_range[1]:
            raise self.boundary("unsupported_timer_callback", address=callback)
        if len(self.timers) >= 16:
            raise self.boundary("timer_budget", limit=16)
        ident = self.next_id
        self.next_id += 1
        self.timers[ident] = {
            "delay": delay,
            "callback": callback,
            "user": user,
            "periodic": bool(flags & TIME_PERIODIC),
            "due": host.clock_ms() + delay,
        }
        if self.thread is None:
            self.thread = host.spawn_thread(TRAMPOLINE, 0)
        self.wake_if_due()
        return ident

    def kill_event(self, args):
        if args[0] not in self.timers:
            return MMSYSERR_INVALPARAM
        del self.timers[args[0]]
        return 0

    # ------------------------------------------------------- timer thread
    def wake_if_due(self):
        state = self.host.threads.get(self.thread)
        if state is not None and state["status"] == "sleeping" and self.timers:
            state["sleep_until"] = min(t["due"] for t in self.timers.values())

    def wait(self, args):
        """Platform thunk run on the timer thread; returns a due timer id or sleeps."""
        host = self.host
        now = host.clock_ms()
        due = [(t["due"], i) for i, t in self.timers.items() if t["due"] <= now]
        if due:
            _, ident = min(due)
            timer = self.timers[ident]
            if timer["periodic"]:
                timer["due"] += timer["delay"]
                if timer["due"] <= now:  # never fire a backlog of missed periods
                    timer["due"] = now + timer["delay"]
            else:
                del self.timers[ident]
            host.set_reg("ecx", timer["callback"])
            host.set_reg("edx", timer["user"])
            self.fired += 1
            return ident
        wake = min((t["due"] for t in self.timers.values()), default=None)
        state = host.threads[host.thread_id]
        state["status"] = "sleeping"
        state["sleep_until"] = wake  # None: sleeps until a timer exists
        return 0
