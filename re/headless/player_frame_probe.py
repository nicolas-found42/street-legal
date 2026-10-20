#!/usr/bin/env python3
"""Run unchanged startup profiles with passive nativeplayer state observation."""
import startup
from player_frame_history import PlayerFrameHistory

class PlayerFrameObserver(startup.Observer):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.player_frames = PlayerFrameHistory(self)

    def sidecar(self):
        return {**super().sidecar(), "player_frames": self.player_frames.sidecar()}

if __name__ == "__main__":
    startup.Observer = PlayerFrameObserver
    startup.main()
