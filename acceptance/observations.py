"""Strict component-run observations for future obligation-specific validators.

A successful observation is only bounded component evidence. It never promotes
an acceptance row by itself. Validators must also prove their complete scenario,
input provenance, comparison policy, and required human reviews.
"""
from __future__ import annotations

import re

SUMMARY = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
    r" (\d+) measured; (\d+) filtered out;", re.MULTILINE
)


def rust_test_observation(returncode: int, stdout: str, stderr: str, expected_tests=()) -> dict:
    combined = stdout + "\n" + stderr
    errors = []
    if returncode != 0:
        errors.append(f"test process exited {returncode}")
    if "SLR_ORACLE_SKIPPED:" in combined:
        errors.append("oracle fixture was skipped")
    if re.search(r"^\s*SKIP(?::|\s)", combined, re.MULTILINE):
        errors.append("legacy skipped observation")
    summaries = SUMMARY.findall(combined)
    if not summaries:
        errors.append("no complete Rust test summary")
    totals = {"passed": 0, "failed": 0, "ignored": 0, "measured": 0, "filtered": 0}
    for status, *counts in summaries:
        for key, count in zip(totals, counts):
            totals[key] += int(count)
        if status != "ok":
            errors.append("test summary failed")
    if totals["passed"] == 0:
        errors.append("no tests executed successfully")
    for key in ("failed", "ignored", "measured"):
        if totals[key]:
            errors.append(f"{totals[key]} {key} tests")
    statuses = dict(re.findall(r"^test (\S+) \.\.\. (ok|FAILED|ignored)$", combined, re.MULTILINE))
    if totals["filtered"] and not expected_tests:
        errors.append("filtered suite lacks explicit expected test identities")
    for test in expected_tests:
        if statuses.get(test) != "ok":
            errors.append(f"required test did not execute successfully: {test}")
    return {"valid_component_observation": not errors, "errors": errors, **totals}
