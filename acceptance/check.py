#!/usr/bin/env python3
"""Fail-closed gate over acceptance/matrix.json.

Exit 0 only when every row is `passed`. Exit 2 when the matrix itself is
malformed or makes an unsupported claim (a `passed` row with no evidence, or an
evidence path that does not exist). Exit 1 when the matrix is honest but the
contract is not met.

`--table` prints the inventory and exits 0; it is a report, not a claim.
"""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "acceptance" / "matrix.json"
KINDS = ("feature", "gate", "supporting")
ORDER = {"kind": ("feature", "gate", "supporting")}


def matrix_path(argv: list[str]) -> Path:
    """`--matrix PATH` overrides the file under test (used by the self-tests)."""
    if "--matrix" in argv:
        index = argv.index("--matrix")
        if index + 1 >= len(argv):
            raise SystemExit("--matrix needs a path")
        return Path(argv[index + 1])
    return MATRIX


def load(path: Path) -> tuple[dict, list[str], list[str]]:
    """Returns (matrix, structural problems, evidence paths absent here)."""
    problems: list[str] = []
    absent: list[str] = []
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        return {}, [f"{path}: {error}"], []
    if data.get("schema") != 1:
        problems.append(f"unsupported matrix schema {data.get('schema')!r}")
    authority = data.get("authority", "")
    authority_path = authority.split("#", 1)[0]
    if not authority_path or not (ROOT / authority_path).exists():
        problems.append(f"authority document missing: {authority_path!r}")
    statuses = set(data.get("statuses") or ())
    if not statuses:
        problems.append("no status vocabulary declared")
    rows = data.get("rows")
    if not isinstance(rows, list) or not rows:
        return data, problems + ["matrix has no rows"], []

    seen: set[str] = set()
    for index, row in enumerate(rows):
        rid = row.get("id")
        where = f"row {index}" + (f" ({rid})" if rid else "")
        if not rid:
            problems.append(f"{where}: missing id")
            continue
        if rid in seen:
            problems.append(f"{where}: duplicate id")
        seen.add(rid)
        if row.get("kind") not in KINDS:
            problems.append(f"{where}: kind {row.get('kind')!r} not in {KINDS}")
        status = row.get("status")
        if status not in statuses:
            problems.append(f"{where}: status {status!r} not in {sorted(statuses)}")
        evidence = row.get("evidence") or []
        if not isinstance(evidence, list):
            problems.append(f"{where}: evidence must be a list")
            continue
        if status == "passed" and not evidence:
            problems.append(f"{where}: claims `passed` with no evidence")
        for entry in evidence:
            if not isinstance(entry, str) or not entry:
                problems.append(f"{where}: evidence entry {entry!r} is not a path")
            elif not (ROOT / entry).exists():
                absent.append(f"{where}: evidence path missing: {entry}")
    return data, problems, absent


def report(problems: list[str], absent: list[str]) -> int:
    """A claim without its evidence, or a corrupt matrix, is never a pass."""
    if problems:
        print("acceptance matrix is not trustworthy:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
    if absent:
        print(
            f"acceptance matrix references {len(absent)} evidence path(s) that are"
            " absent from this checkout. Evidence cannot be recreated from a hash:"
            " reproduce or restore the bundle before trusting the row.",
            file=sys.stderr,
        )
        for entry in absent[:12]:
            print(f"  - {entry}", file=sys.stderr)
        if len(absent) > 12:
            print(f"  ... and {len(absent) - 12} more", file=sys.stderr)
    return 2


def render(rows: list[dict], statuses: list[str]) -> str:
    width = max(len(row["id"]) for row in rows)
    lines = []
    for kind in ORDER["kind"]:
        group = [row for row in rows if row.get("kind") == kind]
        if not group:
            continue
        lines.append(f"\n{kind.upper()}S ({len(group)})")
        for row in group:
            mark = {"passed": "PASS", "partial": "PART", "open": "OPEN"}.get(
                row.get("status"), "????"
            )
            lines.append(
                f"  {row['id']:<{width}}  {mark}  {row.get('title', '')[:88]}"
            )
    counts = Counter(row.get("status") for row in rows)
    total = len(rows)
    summary = ", ".join(f"{counts[s]} {s}" for s in statuses)
    lines.append(f"\n{total} obligations: {summary}")
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    data, problems, absent = load(matrix_path(argv))
    rows = data.get("rows") or []
    statuses = data.get("statuses") or ["open", "partial", "passed"]

    if "--table" in argv:
        if rows:
            print(render(rows, statuses))
        if problems or absent:
            return report(problems, absent)
        return 0

    if problems or absent:
        return report(problems, absent)

    print(render(rows, statuses))
    unmet = [row["id"] for row in rows if row.get("status") != "passed"]
    if unmet:
        print(
            f"\nACCEPTANCE NOT MET: {len(unmet)} of {len(rows)} obligations open"
            f" ({', '.join(unmet[:12])}{' ...' if len(unmet) > 12 else ''})",
            file=sys.stderr,
        )
        return 1
    print(f"\nACCEPTANCE MET: all {len(rows)} obligations passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
