"""Boot-endpoint machinery shared by S1, S10 and P01: oracle identity, node
observation, bundle verification and the live isolated replay."""
from __future__ import annotations

import json
import re
import subprocess
import sys
import uuid
from pathlib import Path

from . import core

BOOT_ORACLE = {
    "oracle_filtered": "af2b27add7ff707cd174a98e5e574a0371074627b29a95823e584179b7895858",
    "oracle_unfiltered": "bce2916908eb6b6de2f318f868997a9ebdee1453cdd944e50ac360f6e2760749",
    "oracle_facts": "a30c13a964f3cce0f999d12c108360089cbadd967b50f5ab096bca9f71fa1f83",
}
BOOT_SOURCE_RUN = "8f86e0d69f80198499fd7a725a6d3b5d4cca599791fbb60c0f2e2129f6aeffde"
BOOT_NODES = "671d3ce8d3331e13136a0bc8628c879034d42b9fa0ffcd544338e098ec536c7a"
BOOT_ENDPOINT_ICOUNT = 1759995995


def known_boot_oracle(inputs: dict) -> None:
    for name, expected in BOOT_ORACLE.items():
        if inputs.get(name, {}).get("sha256") != expected:
            raise ValueError(f"boot oracle identity is not the complete declared-profile endpoint: {name}")



def node_observation(port_stderr: Path, original_steps: Path, cutoff: int) -> dict:
    """No dropped calls, no common-prefix pass. FI label normalization is explicit.

    The port's body label lacks static/instance FI distinction; recording this
    observation does not establish parity of that distinction.
    """
    port = []
    pattern = re.compile(r"^STEP (.+) idx=(\d+) op=([0-9a-f]+) imm=(-?\d+) line=\d+$")
    for line in port_stderr.read_text().splitlines():
        if not line.startswith("STEP "):
            continue
        match = pattern.fullmatch(line)
        if not match:
            raise ValueError("malformed executed-node trace line")
        label, index, operation, immediate = match.groups()
        if ".<field init body" in label:
            cls, method = label.split(".<field init body", 1)[0], "FI"
        else:
            cls, _, method = label.split("(", 1)[0].rpartition(".")
        if not cls or not method:
            raise ValueError("executed-node trace lacks class/method identity")
        port.append((cls, method, int(index), int(operation, 16), int(immediate) & 0xFFFFFFFF))
    original = []
    previous = -1
    with original_steps.open() as source:
        for line in source:
            row = json.loads(line)
            if not isinstance(row.get("ic"), int) or row["ic"] < previous:
                raise ValueError("original node trace lacks ordered instruction identity")
            previous = row["ic"]
            if row["ic"] > cutoff:
                break
            original.append((row["c"], "FI" if "field init" in row["m"] else row["m"], row["n"], row["o"], row["i"] & 0xFFFFFFFF))
    if not port or not original:
        raise ValueError("empty executed-node stream cannot establish an observation")
    mismatch = next((index for index, values in enumerate(zip(port, original)) if values[0] != values[1]), None)
    if mismatch is None and len(port) != len(original):
        mismatch = min(len(port), len(original))
    return {"equal": mismatch is None, "port_nodes": len(port), "original_nodes": len(original),
        "first_difference": None if mismatch is None else {"index": mismatch, "port": port[mismatch] if mismatch < len(port) else None, "original": original[mismatch] if mismatch < len(original) else None},
        "dropped_nodes": 0, "field_initializer_labels": "FI (port lacks static/instance body-label distinction)",
        "scope": "Recorded node observation for S1 only; differing or normalized nodes do not establish node parity."}


def boot_result(bundle: Path | None, *, live_execution=False) -> dict:
    if bundle is None:
        return core.result("not_run", "Supply --boot-bundle from a candidate-bound isolated boot replay.")
    try:
        tool = core.boot_tools()
        manifest = tool.bundle_manifest(bundle)
        if manifest.get("reconstructed") or not manifest.get("source"):
            raise ValueError("boot bundle lacks original source provenance")
        tool.verify_artifacts(bundle, manifest)
        known_boot_oracle(manifest.get("inputs", {}))
        oracle_run = core.ROOT / "re/out/headless/menu-continuation/menu-final/accepted/source-run.json"
        if core.digest(oracle_run) != BOOT_SOURCE_RUN:
            raise ValueError("original oracle run provenance missing or changed")
        original_run = json.loads(oracle_run.read_text())
        if original_run.get("exe_sha256") != core.ORIGINAL_EXE or original_run.get("oracle") != "unicorn":
            raise ValueError("oracle executable identity/kind mismatch")
        if not core.candidate_source_matches(manifest["source"]):
            return core.result("stale", "Boot source revision/dirty source differs from current candidate.", bundle=str(bundle))
        stock = tool.tree_identity(core.ROOT / manifest["configuration"]["stock"])
        if stock["sha256"] != manifest["stock_inputs_sha256"]:
            raise ValueError("boot stock input tree differs from captured inputs")
        core.stock_identity(core.ROOT / manifest["configuration"]["stock"])
        if manifest.get("comparison_policy") != {**tool.MASK_POLICY, "masked_clock_results": []}:
            raise ValueError("boot comparison policy is not the strict captured-clock policy")
        if not manifest.get("build") or manifest["build"].get("returncode") != 0:
            raise ValueError("boot bundle lacks successful build")
        if str(bundle.resolve() / "target") not in manifest["binary"]["path"]:
            raise ValueError("boot binary was not built in the bundle's isolated target")
        inputs = bundle / "inputs"
        named = {}
        for key, identity in manifest["inputs"].items():
            path = inputs / identity["snapshot"]
            if not path.resolve().is_relative_to(inputs.resolve()) or core.digest(path) != identity["sha256"]:
                raise ValueError(f"boot input snapshot mismatch: {key}")
            named[key] = path
        # Always recompute comparisons from raw complete streams.
        original_all = tool.load_records(named["oracle_unfiltered"])
        if len(original_all) != 9209 or len(tool.load_records(named["oracle_filtered"])) != 6508:
            raise ValueError("oracle endpoint is truncated or extended")
        def selected(record):
            return not (record[1].startswith(("java.render.", "java.sound.")) or record[1] in ("java.io.Input", "java.io.MouseCursor", "java.io.Controller"))
        if [record for record in original_all if selected(record)] != tool.load_records(named["oracle_filtered"]):
            raise ValueError("oracle filtered/unfiltered boundaries differ")
        if "oracle_facts" not in named:
            raise ValueError("boot oracle descriptor/provenance facts missing")
        authorities = tool.oracle_descriptors(named["oracle_facts"], original_all)
        types = tool.signatures(named["native_api"])
        comparisons = {}
        for label, filename in (("filtered", "runtime.calllog"), ("unfiltered", "runtime-all.calllog")):
            authority = [value for record, value in zip(original_all, authorities) if selected(record)] if label == "filtered" else authorities
            comparisons[label] = tool.compare(named["oracle_" + label], bundle / "runtime" / filename, types, False, authority)
        analysis = tool.analyze(bundle)
        if any(not value["equal"] for value in comparisons.values()) or manifest["status"] != "completed":
            return core.result("failed", "Boot execution or complete native-log comparison failed.", comparisons=comparisons, bundle=str(bundle), status=manifest["status"])
        # Runtime prints this only after checking actual MainMenu state and 300
        # consecutive idle frames. It is checked alongside complete stream and
        # causal completion validation, never accepted as a standalone marker.
        stdout = (bundle / "runtime.stdout.txt").read_text()
        match = re.search(r"BOOT MainMenu dialog complete at frame (\d+); idle frames=(\d+)", stdout)
        if not match or int(match[2]) < 300 or not analysis.get("has_capture_end_event"):
            raise ValueError("completed MainMenu plus 300 idle frames not demonstrated")
        if "profile0" not in named or core.digest(named["profile0"]) != BOOT_NODES:
            raise ValueError("complete original executed-node provenance missing or changed")
        nodes = node_observation(bundle / "runtime.stderr.txt", named["profile0"], BOOT_ENDPOINT_ICOUNT)
        if not core.candidate_source_matches(manifest["source"]):
            return core.result("stale", "Candidate changed during boot evidence verification.")
        if not live_execution:
            return core.result("replay_required", "Raw boot evidence compares equal; run --execute S1 to rebuild and replay this candidate before accepting it.", comparisons=comparisons, node_observation=nodes, bundle=str(bundle))
        return core.result("passed", "Fresh candidate-bound complete boot streams/MainMenu endpoint verified; node differences explicitly recorded under S1's narrow contract.", comparisons=comparisons, node_observation=nodes, bundle=str(bundle))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return core.result("invalid_evidence", str(error), bundle=str(bundle))


def run_boot(output_root: Path) -> dict:
    runs = output_root / ("boot-" + uuid.uuid4().hex[:12]) / "runs"
    runs.parent.mkdir(parents=True)
    command = [sys.executable, str(core.ROOT / "re/boot-diagnose.py"),
        "re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog",
        "--frames", "331", "--replay", "re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl",
        "--trace-level", "full", "--profile", "re/out/headless/menu-continuation/menu-final/observed/script-steps.jsonl",
        "--profile", "re/out/headless/menu-continuation/menu-final/accepted/source-run.json",
        "--isolated-build", "--runs-root", str(runs)]
    with (runs.parent / "runner.stdout").open("w") as stdout, (runs.parent / "runner.stderr").open("w") as stderr:
        process = subprocess.run(command, cwd=core.ROOT, stdout=stdout, stderr=stderr)
    (runs.parent / "command.json").write_text(json.dumps({"command": command, "returncode": process.returncode}, indent=2) + "\n")
    bundles = list(runs.iterdir()) if runs.exists() else []
    if len(bundles) != 1:
        return core.result("invalid_evidence", "Boot runner did not retain exactly one complete run bundle.", bundle=str(runs.parent))
    observed = boot_result(bundles[0], live_execution=True)
    if observed["result"] == "passed" and process.returncode != 0:
        return core.result("failed", f"Boot runner exited {process.returncode}.", bundle=str(bundles[0]))
    return observed
