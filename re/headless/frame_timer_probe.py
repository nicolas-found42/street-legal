#!/usr/bin/env python3
"""Run the existing ClockObserver plus passive frame-timer input capture.

Accepts startup.py arguments; clock observation spans the whole instruction
budget. Outputs frame-timer-inputs.jsonl in the requested observe directory.
Does not modify profiles, API handlers, native calls or original clocks.
"""
from pathlib import Path
import startup
from clock_probe import ClockObserver
from frame_timer_history import FrameTimerHistory


class FrameTimerObserver(ClockObserver):
    observation_start = 0
    observation_end = 10_000_000_000

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.frame_timer_history = FrameTimerHistory(self)

    def finish(self):
        super().finish()
        self.frame_timer_history.write(Path(self.host.observe_dir) / "frame-timer-inputs.jsonl")

    def sidecar(self):
        return {**super().sidecar(), "frame_timer_history": self.frame_timer_history.sidecar()}


if __name__ == '__main__':
    startup.Observer = FrameTimerObserver
    startup.main()
