#!/usr/bin/env python3
"""Adversarial data-free regressions for the whole-game acceptance gate.

Runs every ``acceptance/tests/test_*.py``. A validator ships with its own
``test_<row>.py`` there proving it rejects stale, relabelled and missing
evidence; nothing in this runner needs the stock install or the oracle.
"""
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

if __name__ == "__main__":
    suite = unittest.defaultTestLoader.discover(str(HERE / "tests"), top_level_dir=str(HERE))
    outcome = unittest.TextTestRunner(verbosity=1).run(suite)
    sys.exit(0 if outcome.wasSuccessful() else 1)
