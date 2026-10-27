"""Shared constants for the data-free acceptance tests."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / "acceptance/check.py"
BASE = json.loads((ROOT / "acceptance/matrix.json").read_text())
RUST_OK = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"
