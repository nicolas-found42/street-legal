"""Declared winmm timers: API semantics and the platform timer thread."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from platform_fixtures import EXE, Boundary, make_startup, profile_data
from platform_profile import PlatformProfile


def api(startup, name, *args):
    return startup.apis[name][1](list(args))


class TimerProfileTests(unittest.TestCase):
    def test_period_limits_are_validated(self):
        for value in (
            {"period_min_ms": 0, "period_max_ms": 10},
            {"period_min_ms": 5, "period_max_ms": 4},
            {"period_min_ms": 1, "period_max_ms": 2000000},
            {"period_min_ms": 1},
            {"period_min_ms": "1", "period_max_ms": 2},
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                PlatformProfile(profile_data(timer=value))


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class TimerTests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()

    def find_callback(self):
        """Any original `ret 0x14` serves as a harmless stdcall(5 args) callback."""
        start, end = self.startup.text_range
        code = bytes(self.startup.uc.mem_read(start, end - start))
        return start + code.index(b"\xc2\x14\x00")

    def test_begin_and_end_period_follow_declared_range_and_pairing(self):
        s = self.startup
        self.assertEqual(api(s, "timeBeginPeriod", 1), 0)
        self.assertEqual(api(s, "timeBeginPeriod", 0), 97)
        self.assertEqual(api(s, "timeBeginPeriod", 2000000), 97)
        self.assertEqual(api(s, "timeEndPeriod", 1), 0)
        self.assertEqual(api(s, "timeEndPeriod", 1), 97)  # unmatched

    def test_time_get_time_is_the_virtual_clock(self):
        s = self.startup
        s.icount = 5000
        self.assertEqual(api(s, "timeGetTime"), 5)
        s.elapsed_ms = 100
        self.assertEqual(api(s, "timeGetTime"), 105)

    def test_set_event_validation_and_kill(self):
        s = self.startup
        callback = self.find_callback()
        self.assertEqual(api(s, "timeSetEvent", 0, 1, callback, 0, 0), 0)
        self.assertEqual(api(s, "timeSetEvent", 10, 1, 0, 0, 0), 0)
        with self.assertRaises(Boundary):
            api(s, "timeSetEvent", 10, 1, callback, 0, 0x100)  # unsupported callback type
        with self.assertRaises(Boundary):
            api(s, "timeSetEvent", 10, 1, 0x7F000000, 0, 0)  # not original code
        first = api(s, "timeSetEvent", 10, 1, callback, 7, 1)
        second = api(s, "timeSetEvent", 10, 1, callback, 7, 0)
        self.assertEqual((first, second), (1, 2))
        self.assertEqual(api(s, "timeKillEvent", first), 0)
        self.assertEqual(api(s, "timeKillEvent", first), 11)
        self.assertEqual(len(s.threads), 2)  # one platform timer thread for all timers

    def run_busy_loop(self, startup, iterations):
        # mov ecx, N ; L: dec ecx ; jnz L ; ret
        code = b"\xb9" + iterations.to_bytes(4, "little") + b"\x49\x75\xfd\xc3"
        address = startup.scratch(len(code))
        startup.write(address, code)
        return startup.call(address, [], limit=50_000_000)

    def test_periodic_timer_fires_on_the_timer_thread_at_the_declared_rate(self):
        s = self.startup
        callback = self.find_callback()
        self.assertEqual(api(s, "timeSetEvent", 10, 1, callback, 0xBEEF, 1), 1)
        self.run_busy_loop(s, 100_000)  # 200k instructions: about 200 ms of virtual time at 1 ms per 1000 instructions
        self.assertGreaterEqual(s.timers.fired, 15)
        self.assertLessEqual(s.timers.fired, 21)
        events = [e for e in s.events if "schedule" in e]
        self.assertTrue(any(e["schedule"]["to"] == s.timers.thread for e in events))

    def test_one_shot_fires_once_and_timer_thread_sleeps_without_timers(self):
        s = self.startup
        callback = self.find_callback()
        api(s, "timeSetEvent", 5, 1, callback, 1, 0)
        self.run_busy_loop(s, 100_000)
        self.assertEqual(s.timers.fired, 1)
        self.assertEqual(s.threads[s.timers.thread]["status"], "sleeping")
        self.assertIsNone(s.threads[s.timers.thread]["sleep_until"])

    def test_killed_timer_never_fires(self):
        s = self.startup
        callback = self.find_callback()
        ident = api(s, "timeSetEvent", 5, 1, callback, 1, 1)
        api(s, "timeKillEvent", ident)
        self.run_busy_loop(s, 100_000)
        self.assertEqual(s.timers.fired, 0)


if __name__ == "__main__":
    unittest.main()
