#!/usr/bin/env python3
"""Observe resource destruction without changing the standard startup scenario."""
import startup
from resource_lifetime_history import ResourceLifetimeHistory

class ResourceLifetimeObserver(startup.Observer):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.resource_lifetime = ResourceLifetimeHistory(self)

    def sidecar(self):
        return {**super().sidecar(), "resource_lifetime": self.resource_lifetime.sidecar()}

if __name__ == "__main__":
    startup.Observer = ResourceLifetimeObserver
    startup.main()
