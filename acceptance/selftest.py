#!/usr/bin/env python3
"""Self-tests for acceptance/check.py.

A gate that cannot reject is not a gate. Each case writes a scratch matrix and
asserts the exit code the checker must produce:

  0  every row passed and every claim evidenced
  1  matrix honest, contract unmet
  2  matrix untrustworthy (unsupported claim, missing evidence, bad schema)
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHECK = ROOT / "acceptance" / "check.py"
REAL_EVIDENCE = "docs/fidelity.md"


def run(rows: list[dict], statuses=None, schema=1) -> int:
    matrix = {
        "schema": schema,
        "authority": "docs/fidelity.md#whole-game-completion-contract",
        "statuses": statuses or ["open", "partial", "passed"],
        "rows": rows,
    }
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "matrix.json"
        path.write_text(json.dumps(matrix))
        return subprocess.run(
            [sys.executable, str(CHECK), "--matrix", str(path)],
            capture_output=True,
            text=True,
        ).returncode


def row(rid="P01", status="open", evidence=None, kind="feature"):
    return {
        "id": rid,
        "kind": kind,
        "title": f"t-{rid}",
        "status": status,
        "evidence": [] if evidence is None else evidence,
    }


CASES: list[tuple[str, dict, int]] = [
    ("honest incomplete row", {"rows": [row()]}, 1),
    ("all rows passed with real evidence", {"rows": [row(status="passed", evidence=[REAL_EVIDENCE])]}, 0),
    ("passed without evidence", {"rows": [row(status="passed")]}, 2),
    ("passed with missing evidence", {"rows": [row(status="passed", evidence=["docs/not-there.md"])]}, 2),
    ("unknown status", {"rows": [row(status="done")]}, 2),
    ("duplicate id", {"rows": [row("P01"), row("P01")]}, 2),
    ("kind outside the vocabulary", {"rows": [row(kind="nice-to-have")]}, 2),
    ("empty rows", {"rows": []}, 2),
    ("wrong schema", {"rows": [row()], "schema": 2}, 2),
    ("mixed passed and open", {"rows": [row("P01", "passed", [REAL_EVIDENCE]), row("P02")]}, 1),
]


def main() -> int:
    failures = 0
    for name, spec, expected in CASES:
        got = run(**spec)
        ok = got == expected
        failures += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {name}: expected {expected}, got {got}")
    if failures:
        print(f"\n{failures} of {len(CASES)} self-tests failed", file=sys.stderr)
        return 1
    print(f"\n{len(CASES)} of {len(CASES)} self-tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
