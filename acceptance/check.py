#!/usr/bin/env python3
"""Executable, fail-closed inventory of all 54 whole-game obligations.

Inventory progress and evidence paths are context, never passing results. A
passing result must come from a checked-in obligation validator. Validators
must rerun their comparison and validate candidate/input/policy provenance;
there is deliberately no generic JSON receipt or command supplied by the
matrix that can claim success. Unsupported obligations remain unsupported.

Exit 0: all 54 executable results passed; 1: honest incomplete inventory;
2: malformed inventory or unsupported passing claim. --table only reports.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

from validators import boot_result, ledger_result, result, retirement_result, run_boot, stock_identity

ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "acceptance/matrix.json"
STATUSES = ["open", "partial", "passed"]
EXPECTED = {
    **{f"P{i:02}": "feature" for i in range(1, 31)},
    **{f"G{i}": "gate" for i in range(1, 12)},
    **{f"S{i}": "supporting" for i in range(1, 14)},
}
KINDS = ("feature", "gate", "supporting")


def local_file(entry: object) -> Path:
    if not isinstance(entry, str) or not entry:
        raise ValueError("evidence must name a repository-relative regular file")
    relative = Path(entry)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"evidence escapes repository: {entry!r}")
    path = ROOT / relative
    if not path.resolve().is_relative_to(ROOT.resolve()):
        raise ValueError(f"evidence escapes repository through symlink: {entry!r}")
    if not path.is_file():
        raise ValueError(f"evidence file missing: {entry!r}")
    return path


def feature_tiers() -> dict[str, list[str]]:
    text = (ROOT / "docs/fidelity.md").read_text()
    return {
        rid: re.findall(r"\b[A-E]\b", tiers)
        for rid, tiers in re.findall(
            r"^\| (P\d{2}) \| [^|]+ \| ([^|]+) \|", text, re.MULTILINE
        )
    }


def load(path: Path) -> tuple[dict, list[str], list[str]]:
    problems: list[str] = []
    absent: list[str] = []
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        return {}, [f"{path}: {error}"], []
    if not isinstance(data, dict):
        return {}, ["matrix must be an object"], []
    if data.get("schema") != 2:
        problems.append("unsupported matrix schema (expected 2)")
    if data.get("authority") != "docs/fidelity.md#whole-game-completion-contract":
        problems.append("authority must name the whole-game completion contract")
    if data.get("statuses") != STATUSES:
        problems.append("status vocabulary must be open, partial, passed")
    rows = data.get("rows")
    if not isinstance(rows, list):
        return data, problems + ["matrix rows must be a list"], absent
    tiers = feature_tiers()
    if set(tiers) != {rid for rid in EXPECTED if rid.startswith("P")}:
        problems.append("fidelity feature contract is missing or ambiguous")
    seen: set[str] = set()
    for index, row in enumerate(rows):
        where = f"row {index}"
        if not isinstance(row, dict):
            problems.append(f"{where}: row must be an object")
            continue
        rid = row.get("id")
        if not isinstance(rid, str) or rid not in EXPECTED:
            problems.append(f"{where}: unknown obligation {rid!r}")
            continue
        where += f" ({rid})"
        if rid in seen:
            problems.append(f"{where}: duplicate id")
        seen.add(rid)
        if row.get("kind") != EXPECTED[rid]:
            problems.append(f"{where}: kind must be {EXPECTED[rid]}")
        if not isinstance(row.get("title"), str) or not row["title"].strip():
            problems.append(f"{where}: missing title")
        status = row.get("status")
        if status not in STATUSES:
            problems.append(f"{where}: unknown progress status {status!r}")
        if rid.startswith("P") and row.get("tiers") != tiers.get(rid):
            problems.append(f"{where}: tiers differ from fidelity contract")
        evidence = row.get("evidence")
        if not isinstance(evidence, list):
            problems.append(f"{where}: evidence must be a list")
            continue
        for entry in evidence:
            try:
                local_file(entry)
            except ValueError as error:
                absent.append(f"{where}: {error}")
        # Even where a live validator exists, an inventory claim cannot replace
        # executing it. Existing checkpoint receipts cover bounded components.
        if status == "passed":
            problems.append(f"{where}: inventory progress cannot assert passed; execute its live validator")
        if any(key in row for key in ("command", "validator", "result", "passed")):
            problems.append(f"{where}: executable results/commands cannot be supplied by inventory")
    if missing := set(EXPECTED) - seen:
        problems.append(f"missing obligations: {', '.join(sorted(missing))}")
    return data, problems, absent


def evaluate(rows: list[dict], boot_bundle=None, execute=(), output_root=None) -> list[dict]:
    """Actual results. Adding a pass requires implementing its complete validator.

    Do not infer parity from path existence, receipt counters, a test's return
    code, or a manually supplied status. Only registered live validators can
    promote a result; unsupported scenarios remain explicit.
    """
    results = [
        {
            "id": row["id"], "kind": row["kind"], "title": row["title"],
            "progress": row["status"], "result": "unsupported",
            "reason": "No executable validator for the complete obligation; listed evidence is supporting context only.",
            "supporting_evidence": row["evidence"],
        }
        for row in rows
    ]
    by_id = {row["id"]: row for row in results}
    live_boot = "S1" in execute or "S10" in execute
    by_id["S1"].update(run_boot(output_root) if live_boot else boot_result(boot_bundle))
    by_id["S11"].update(
        ledger_result(output_root) if "S11" in execute else
        result("not_run", "Run --execute S11 for fresh isolated ledger collection/regeneration; receipts cannot assert pass.")
    )
    by_id["S10"].update(retirement_result(output_root, by_id["S1"] if live_boot else None,
        live_execution="S10" in execute))
    if "G1" in execute:
        try:
            stock = stock_identity(ROOT / "stock")
            by_id["G1"].update(result("partial", "Stock checksums verified; native window/fullscreen, unchanged-script runtime and approximation-removal gates remain incomplete.", stock_files=len(stock["files"]), original_exe_sha256=stock["original_exe_sha256"]))
        except (OSError, ValueError) as error:
            by_id["G1"].update(result("failed", str(error)))
    return results


def render(results: list[dict]) -> str:
    lines = []
    for kind in KINDS:
        group = [row for row in results if row["kind"] == kind]
        lines.append(f"\n{kind.upper()} ({len(group)})")
        for row in group:
            lines.append(f"  {row['id']:<3}  {row['result'].upper():<11}  [{row['progress']}]  {row['title'][:78]}")
    progress = Counter(row["progress"] for row in results)
    actual = Counter(row["result"] for row in results)
    lines.append(f"\n{len(results)} obligations: " + ", ".join(f"{actual[key]} {key}" for key in sorted(actual)))
    lines.append("Inventory progress (not acceptance): " + ", ".join(f"{progress[key]} {key}" for key in STATUSES))
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", type=Path, default=MATRIX)
    parser.add_argument("--table", action="store_true", help="report only; cannot establish acceptance")
    parser.add_argument("--json", action="store_true", help="emit computed results, never inventory claims")
    parser.add_argument("--boot-bundle", type=Path, help="recompare a source/input-bound isolated boot bundle")
    parser.add_argument("--execute", action="append", choices=("S1", "S10", "S11", "G1"), default=[], help="run a live validator; S10 also executes S1; may be repeated")
    parser.add_argument("--output-root", type=Path, default=ROOT / "re/out/acceptance", help="retained isolated validation bundles")
    args = parser.parse_args(argv)
    data, problems, absent = load(args.matrix)
    if problems or absent:
        errors = problems + absent
        if args.json:
            print(json.dumps({"schema": 2, "accepted": False, "errors": errors}, indent=2))
        else:
            print("acceptance inventory is not trustworthy:\n" + "\n".join(f"  - {error}" for error in errors), file=sys.stderr)
        return 2
    results = evaluate(data["rows"], args.boot_bundle.resolve() if args.boot_bundle else None, args.execute, args.output_root.resolve())
    accepted = len(results) == 54 and all(row["result"] == "passed" for row in results)
    if args.json:
        print(json.dumps({"schema": 2, "accepted": accepted, "report_only": args.table, "results": results}, indent=2))
    else:
        print(render(results))
        if not args.table:
            print(f"\nACCEPTANCE {'MET' if accepted else 'NOT MET'}: {sum(row['result'] == 'passed' for row in results)} of 54 executable results passed", file=sys.stderr)
    return 0 if args.table or accepted else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
