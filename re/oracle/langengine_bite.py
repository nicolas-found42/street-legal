#!/usr/bin/env python3
"""Perturb every consumed oracle expectation, require Rust replay failure.

Uses a temporary vector file under ignored re/out; the original is never edited.
Builds the focused Rust integration test once, then invokes that binary per case.
"""

import copy
import json
import os
import subprocess
import tempfile
from pathlib import Path


def leaves(value, prefix=()):
    if isinstance(value, dict):
        for key, item in value.items():
            yield from leaves(item, prefix + (key,))
    elif isinstance(value, list) and value:
        for key, item in enumerate(value):
            yield from leaves(item, prefix + (key,))
    else:
        yield prefix


def run():
    built = subprocess.run(
        [
            "cargo",
            "test",
            "-q",
            "-p",
            "slr-lang",
            "--test",
            "engine_oracle",
            "--no-run",
            "--message-format=json",
        ],
        text=True,
        capture_output=True,
        check=True,
    )
    executable = next(
        json.loads(line)["executable"]
        for line in built.stdout.splitlines()
        if json.loads(line).get("executable")
    )
    original = Path("re/out/langobj/engine.jsonl")
    rows = [json.loads(line) for line in original.read_text().splitlines()]
    path = Path("re/out/langobj/bite.jsonl").resolve()
    env = dict(os.environ, SLR_ENGINE_VECTORS=str(path))
    binary = [executable, "--exact", "engine_natives_match_oracle"]
    checked = 0
    with tempfile.TemporaryDirectory(dir="re/out") as missing_dir:
        missing_env = dict(
            env, SLR_ENGINE_VECTORS=str(Path(missing_dir) / "absent.jsonl")
        )
        skipped = subprocess.run(
            binary + ["--nocapture"],
            env=missing_env,
            text=True,
            capture_output=True,
            check=True,
        )
        marker = "SLR_ORACLE_SKIPPED: engine_natives_match_oracle"
        if marker not in skipped.stdout + skipped.stderr:
            raise RuntimeError("missing oracle input did not emit ledger skip marker")
        print("missing-input check: standard ledger skip marker observed")
    try:
        path.write_text(original.read_text())
        subprocess.run(binary, env=env, capture_output=True, check=True)
        for row_index, row in enumerate(rows):
            if row.get("family") == "lifecycle":
                keys = ["steps"]
                # `op` selects the input operation; it is not an expectation.
                paths = [p for p in leaves(row["steps"], ("steps",)) if p[-1] != "op"]
            elif row.get("family") == "array":
                keys = [
                    "after_src",
                    "after_dst",
                    "after_src_types",
                    "after_dst_types",
                    "calls",
                ]
                paths = [p for k in keys for p in leaves(row[k], (k,))]
            else:
                keys = ["calls", "after", "exe_addr"] + (
                    ["return"] if row["sig"].endswith("I") else []
                )
                paths = [p for k in keys for p in leaves(row[k], (k,))]
            for cursor in paths:
                changed = copy.deepcopy(row)
                parent = changed
                for key in cursor[:-1]:
                    parent = parent[key]
                key = cursor[-1]
                old = parent[key]
                if isinstance(old, bool):
                    parent[key] = not old
                elif isinstance(old, int):
                    parent[key] = old ^ 1
                elif isinstance(old, str):
                    parent[key] = old + "!"
                elif isinstance(old, list):
                    parent[key] = [[1, 2, []]] if key in ("calls", "runs") else [1]
                else:
                    raise TypeError((cursor, old))
                corpus = list(rows)
                corpus[row_index] = changed
                path.write_text("".join(json.dumps(r) + "\n" for r in corpus))
                result = subprocess.run(
                    binary, env=env, capture_output=True, check=False
                )
                if result.returncode == 0:
                    raise RuntimeError(f"assertion did not bite: {row['id']} {cursor}")
                checked += 1
        print(
            f"{checked}/{checked} expectation perturbations failed as required; {len(rows)} rows"
        )
    finally:
        path.unlink(missing_ok=True)
    # Re-run untouched vectors after all perturbations.
    env["SLR_ENGINE_VECTORS"] = str(original.resolve())
    subprocess.run(binary, env=env, check=True)


if __name__ == "__main__":
    run()
