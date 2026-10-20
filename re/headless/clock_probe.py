#!/usr/bin/env python3
"""Run startup.py unchanged with additional passive clock observation.

Accepts startup.py's arguments and profiles plus --clock-observe-start/end. This launcher changes only
which read-only Observer subclass startup instantiates; no API handlers, original
instructions, clocks, script inputs or scenario values are replaced.
"""
import argparse
import sys

from clock_history import ClockHistory
import startup


class ClockObserver(startup.Observer):
    observation_start = 99_000_000
    observation_end = 350_000_000
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.clock_history = ClockHistory(self, start=self.observation_start, end=self.observation_end)

    def sidecar(self):
        return {**super().sidecar(), 'clock_history': self.clock_history.sidecar()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--clock-observe-start', type=int, default=99_000_000)
    parser.add_argument('--clock-observe-end', type=int, default=350_000_000)
    observation, remaining = parser.parse_known_args()
    if not 0 <= observation.clock_observe_start <= observation.clock_observe_end:
        parser.error('clock observation range must be nonnegative and ordered')
    ClockObserver.observation_start = observation.clock_observe_start
    ClockObserver.observation_end = observation.clock_observe_end
    sys.argv = [sys.argv[0], *remaining]
    startup.Observer = ClockObserver
    startup.main()
