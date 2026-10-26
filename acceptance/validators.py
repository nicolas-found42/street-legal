"""Live obligation validators. A receipt JSON is never an executable result."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import uuid

ROOT = Path(__file__).resolve().parents[1]
# Extraction-verified build-606 install identity, retained by existing receipts.
STOCK_MANIFEST = "671a67c0dfee0a546d1e873096d4df68a5791fe14acccd4da5ac99cd72adccce"
ORIGINAL_EXE = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
BOOT_ORACLE = {
    "oracle_filtered": "af2b27add7ff707cd174a98e5e574a0371074627b29a95823e584179b7895858",
    "oracle_unfiltered": "bce2916908eb6b6de2f318f868997a9ebdee1453cdd944e50ac360f6e2760749",
    "oracle_facts": "a30c13a964f3cce0f999d12c108360089cbadd967b50f5ab096bca9f71fa1f83",
}
BOOT_SOURCE_RUN = "8f86e0d69f80198499fd7a725a6d3b5d4cca599791fbb60c0f2e2129f6aeffde"
BOOT_NODES = "671d3ce8d3331e13136a0bc8628c879034d42b9fa0ffcd544338e098ec536c7a"
BOOT_ENDPOINT_ICOUNT = 1759995995


def boot_tools():
    spec = importlib.util.spec_from_file_location("acceptance_boot_diagnose", ROOT / "re/boot-diagnose.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def stock_identity(root: Path) -> dict:
    manifest = root / ".slr-manifest"
    if digest(manifest) != STOCK_MANIFEST:
        raise ValueError("stock manifest differs from extraction-verified stock media")
    lines = manifest.read_text().splitlines()
    if lines[0] != "# slr-stock-manifest v1" or len(lines) != 7859:
        raise ValueError("stock manifest count/header mismatch")
    entries = []
    for line in lines[1:]:
        expected, size, name = line.split("\t")
        relative = Path(name)
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("stock manifest path escapes install")
        path = root / relative
        if path.is_symlink() or not path.is_file() or path.stat().st_size != int(size) or digest(path) != expected:
            raise ValueError(f"stock file missing or changed: {name}")
        entries.append({"path": name, "bytes": int(size), "sha256": expected})
    actual = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    if actual != {entry["path"] for entry in entries} | {".slr-manifest"}:
        raise ValueError("stock install has unexplained additional or missing files")
    if digest(root / "StreetLegal_Redline.bak.exe") != ORIGINAL_EXE:
        raise ValueError("original executable identity mismatch")
    return {"manifest_sha256": STOCK_MANIFEST, "original_exe_sha256": ORIGINAL_EXE, "files": entries}


def result(outcome: str, reason: str, **details) -> dict:
    if "result" in details:
        raise ValueError("details cannot replace the checked validator result")
    return {"result": outcome, "reason": reason, **details}


def candidate_source_matches(expected: dict) -> bool:
    return boot_tools().git_identity(ROOT)[0] == expected


def native_registration_rows(text: str) -> list[list[str]]:
    rows = [line.split("\t") for line in text.splitlines()]
    if len(rows) != 353 or any(len(row) != 4 for row in rows):
        raise ValueError("native registration fixture must contain exactly 353 complete rows")
    if len({tuple(row[:3]) for row in rows}) != 353:
        raise ValueError("native registration fixture has duplicate signatures")
    if any(not re.fullmatch(r"(?:FUN|LAB)_[0-9a-fA-F]{8}", row[3]) for row in rows):
        raise ValueError("native registration fixture has unresolved original addresses")
    return rows


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
        return result("not_run", "Supply --boot-bundle from a candidate-bound isolated boot replay.")
    try:
        tool = boot_tools()
        manifest = tool.bundle_manifest(bundle)
        if manifest.get("reconstructed") or not manifest.get("source"):
            raise ValueError("boot bundle lacks original source provenance")
        tool.verify_artifacts(bundle, manifest)
        known_boot_oracle(manifest.get("inputs", {}))
        oracle_run = ROOT / "re/out/headless/menu-continuation/menu-final/accepted/source-run.json"
        if digest(oracle_run) != BOOT_SOURCE_RUN:
            raise ValueError("original oracle run provenance missing or changed")
        original_run = json.loads(oracle_run.read_text())
        if original_run.get("exe_sha256") != ORIGINAL_EXE or original_run.get("oracle") != "unicorn":
            raise ValueError("oracle executable identity/kind mismatch")
        if not candidate_source_matches(manifest["source"]):
            return result("stale", "Boot source revision/dirty source differs from current candidate.", bundle=str(bundle))
        stock = tool.tree_identity(ROOT / manifest["configuration"]["stock"])
        if stock["sha256"] != manifest["stock_inputs_sha256"]:
            raise ValueError("boot stock input tree differs from captured inputs")
        stock_identity(ROOT / manifest["configuration"]["stock"])
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
            if not path.resolve().is_relative_to(inputs.resolve()) or digest(path) != identity["sha256"]:
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
            return result("failed", "Boot execution or complete native-log comparison failed.", comparisons=comparisons, bundle=str(bundle), status=manifest["status"])
        # Runtime prints this only after checking actual MainMenu state and 300
        # consecutive idle frames. It is checked alongside complete stream and
        # causal completion validation, never accepted as a standalone marker.
        stdout = (bundle / "runtime.stdout.txt").read_text()
        match = re.search(r"BOOT MainMenu dialog complete at frame (\d+); idle frames=(\d+)", stdout)
        if not match or int(match[2]) < 300 or not analysis.get("has_capture_end_event"):
            raise ValueError("completed MainMenu plus 300 idle frames not demonstrated")
        if "profile0" not in named or digest(named["profile0"]) != BOOT_NODES:
            raise ValueError("complete original executed-node provenance missing or changed")
        nodes = node_observation(bundle / "runtime.stderr.txt", named["profile0"], BOOT_ENDPOINT_ICOUNT)
        if not candidate_source_matches(manifest["source"]):
            return result("stale", "Candidate changed during boot evidence verification.")
        if not live_execution:
            return result("replay_required", "Raw boot evidence compares equal; run --execute S1 to rebuild and replay this candidate before accepting it.", comparisons=comparisons, node_observation=nodes, bundle=str(bundle))
        return result("passed", "Fresh candidate-bound complete boot streams/MainMenu endpoint verified; node differences explicitly recorded under S1's narrow contract.", comparisons=comparisons, node_observation=nodes, bundle=str(bundle))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return result("invalid_evidence", str(error), bundle=str(bundle))


def run_boot(output_root: Path) -> dict:
    runs = output_root / ("boot-" + uuid.uuid4().hex[:12]) / "runs"
    runs.parent.mkdir(parents=True)
    command = [sys.executable, str(ROOT / "re/boot-diagnose.py"),
        "re/out/headless/menu-continuation/menu-final/accepted/script-natives.slrlog",
        "--frames", "331", "--replay", "re/out/headless/clock-investigation/menu1800M/clock-inputs.jsonl",
        "--trace-level", "full", "--profile", "re/out/headless/menu-continuation/menu-final/observed/script-steps.jsonl",
        "--profile", "re/out/headless/menu-continuation/menu-final/accepted/source-run.json",
        "--isolated-build", "--runs-root", str(runs)]
    with (runs.parent / "runner.stdout").open("w") as stdout, (runs.parent / "runner.stderr").open("w") as stderr:
        process = subprocess.run(command, cwd=ROOT, stdout=stdout, stderr=stderr)
    (runs.parent / "command.json").write_text(json.dumps({"command": command, "returncode": process.returncode}, indent=2) + "\n")
    bundles = list(runs.iterdir()) if runs.exists() else []
    if len(bundles) != 1:
        return result("invalid_evidence", "Boot runner did not retain exactly one complete run bundle.", bundle=str(runs.parent))
    observed = boot_result(bundles[0], live_execution=True)
    if observed["result"] == "passed" and process.returncode != 0:
        return result("failed", f"Boot runner exited {process.returncode}.", bundle=str(bundles[0]))
    return observed


def retirement_shape(root: Path) -> list[str]:
    """Check retirement, allowing slr-game to be reused as the production host.

    This is only a structural prerequisite. Cargo's resolved graph, a fresh
    boot of the replacement host and repository checks are required to pass.
    """
    errors = []
    forbidden = {"slr-core", "macroquad", "miniquad"}
    if (root / "crates/slr-core").exists() or (root / "crates/slr-core").is_symlink():
        errors.append("Old approximate slice remains: crates/slr-core")
    manifests = [root / "Cargo.toml", *sorted((root / "crates").rglob("Cargo.toml"))]
    def inspect(value, path):
        if isinstance(value, dict):
            for key, child in value.items():
                if key in forbidden or (key in ("name", "package") and isinstance(child, str) and child in forbidden):
                    errors.append(f"Retired package/dependency {key}={child!r}: {path}")
                inspect(child, path)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, str) and Path(child).name in forbidden:
                    errors.append(f"Retired workspace member {child}: {path}")
                inspect(child, path)
    for path in manifests:
        inspect(tomllib.loads(path.read_text()), path.relative_to(root))
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    for package in lock.get("package", []):
        if package.get("name") in forbidden:
            errors.append(f"Retired package remains in Cargo.lock: {package['name']}")
    host = root / "crates/slr-game/Cargo.toml"
    if not host.is_file() or tomllib.loads(host.read_text()).get("package", {}).get("name") != "slr-game":
        errors.append("Replacement production slr-game host is missing")
    for path in (root / "crates").rglob("*.rs"):
        if re.search(r"\b(?:slr_core|macroquad|miniquad)\s*::|\bextern\s+crate\s+(?:slr_core|macroquad|miniquad)\b", path.read_text()):
            errors.append(f"Retired runtime import remains: {path.relative_to(root)}")
    return errors


def retirement_result(output_root: Path | None, fresh_boot: dict | None = None, *, live_execution=False) -> dict:
    """S10: retire approximation, exercise its replacement and recheck integration.

    Only evaluate's live S10 dispatch supplies fresh_boot, computed by run_boot
    in this invocation. No user-supplied receipt can assert the prerequisite.
    """
    try:
        errors = retirement_shape(ROOT)
        if errors:
            return result("failed", "Approximation retirement prerequisites failed.", errors=errors)
        if not live_execution:
            return result("not_run", "Run --execute S10 for fresh S1, replacement-host boot and workspace integration checks.")
        if not fresh_boot or fresh_boot.get("result") != "passed":
            return result("blocked", "S10 requires a freshly computed passing S1 in this invocation.", prerequisite=fresh_boot)
        tool = boot_tools()
        boot_bundle = Path(fresh_boot["bundle"])
        manifest = tool.bundle_manifest(boot_bundle)
        # Recheck raw streams, inputs, source and artifact identities. This
        # recomparison cannot itself replace the live prerequisite above.
        observed = boot_result(boot_bundle)
        if observed["result"] != "replay_required":
            return result("blocked", "Fresh S1 evidence failed revalidation.", prerequisite=observed)
        source = manifest["source"]
        def unchanged_stock():
            stock_identity(ROOT / manifest["configuration"]["stock"])
            if tool.tree_identity(ROOT / manifest["configuration"]["stock"])["sha256"] != manifest["stock_inputs_sha256"]:
                raise ValueError("Stock inputs changed after fresh S1")
        unchanged_stock()
        bundle = output_root / ("retirement-" + uuid.uuid4().hex[:12])
        bundle.mkdir(parents=True)
        context = {"schema": 1, "obligation": "S10", "source": source,
            "s1_bundle": str(boot_bundle), "tools": tool.environment_identity(ROOT), "commands": []}
        verdict = result("failed", "Retirement validation did not complete.", bundle=str(bundle))
        try:
            environment = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
            environment["CARGO_TARGET_DIR"] = str(bundle / "target")
            context["environment"] = {key: environment[key] for key in ("CARGO_TARGET_DIR", "RUSTFLAGS", "RUSTUP_TOOLCHAIN") if key in environment}
            def execute(label, command, *, tests=False):
                stdout, stderr = bundle / (label + ".stdout"), bundle / (label + ".stderr")
                with stdout.open("w") as out, stderr.open("w") as err:
                    process = subprocess.run(command, cwd=ROOT, env=environment, stdout=out, stderr=err, timeout=1800)
                entry = {"command": command, "returncode": process.returncode,
                    "stdout": stdout.name, "stderr": stderr.name}
                context["commands"].append(entry)
                if process.returncode:
                    raise ValueError(f"{label} exited {process.returncode}")
                if tests:
                    from observations import rust_test_observation
                    entry["observation"] = rust_test_observation(process.returncode, stdout.read_text(), stderr.read_text())
                    if not entry["observation"]["valid_component_observation"]:
                        raise ValueError("Workspace tests were incomplete or skipped oracle fixtures")
                return stdout
            metadata = json.loads(execute("metadata", ["cargo", "metadata", "--locked", "--format-version", "1"]).read_text())
            packages = {row["id"]: row for row in metadata["packages"]}
            if any(row["name"] in {"slr-core", "macroquad", "miniquad"} for row in packages.values()):
                raise ValueError("Resolved Cargo graph retains the approximation or its renderer")
            hosts = [row for key, row in packages.items() if key in metadata["workspace_members"] and row["name"] == "slr-game"]
            if len(hosts) != 1:
                raise ValueError("Cargo workspace lacks exactly one production slr-game package")
            host = hosts[0]
            graph = {row["id"]: row["dependencies"] for row in metadata["resolve"]["nodes"]}
            reachable, pending = set(), [host["id"]]
            while pending:
                key = pending.pop()
                if key not in reachable:
                    reachable.add(key)
                    pending.extend(graph.get(key, []))
            if not {"slr-vm", "slr-io", "slr-calllog"} <= {packages[key]["name"] for key in reachable}:
                raise ValueError("Production host does not link the original-script VM/runtime")
            binaries = [row["name"] for row in host["targets"] if "bin" in row["kind"]]
            binary_name = host.get("default_run") or (binaries[0] if len(binaries) == 1 else None)
            if binary_name not in binaries:
                raise ValueError("Production host binary is missing or ambiguous")
            execute("format", ["cargo", "fmt", "--all", "--", "--check"])
            execute("clippy", ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"])
            execute("workspace-tests", ["cargo", "test", "--locked", "--workspace", "--all-targets", "--", "--nocapture"], tests=True)
            execute("host-build", ["cargo", "build", "--locked", "-p", "slr-game", "--bin", binary_name])
            binary = bundle / "target/debug" / binary_name
            context["binary"] = tool.file_identity(binary)
            # Reuse the actually executed S1 input/clock tape and arguments;
            # change only binary and isolated outputs/run identity.
            args = list(manifest["child_arguments"])
            args[0] = str(binary)
            for flag, value in (("--output", str(bundle / "runtime")), ("--run-id", bundle.name)):
                args[args.index(flag) + 1] = value
            environment.update(manifest.get("trace", {}).get("environment", {}))
            context["trace_environment"] = manifest.get("trace", {}).get("environment", {})
            stdout = execute("host-replay", args)
            match = re.search(r"BOOT MainMenu dialog complete at frame (\d+); idle frames=(\d+)", stdout.read_text())
            if not match or int(match[2]) < 300:
                raise ValueError("Replacement host did not complete MainMenu plus 300 idle frames")
            context["comparisons"] = {}
            # S1 already validated descriptor authority and both original
            # snapshots. Compare the replacement's entire streams to the fresh
            # S1's parsed records without masking; never accept a common prefix.
            for label, name in (("filtered", "runtime.calllog"), ("unfiltered", "runtime-all.calllog")):
                original = tool.load_records(boot_bundle / "runtime" / name)
                actual = tool.load_records(bundle / "runtime" / name)
                mismatch = next((index for index, pair in enumerate(zip(original, actual)) if pair[0] != pair[1]), None)
                if mismatch is None and len(original) != len(actual):
                    mismatch = min(len(original), len(actual))
                context["comparisons"][label] = {"equal": original == actual, "s1_records": len(original), "host_records": len(actual),
                    "first_difference": None if mismatch is None else {"index": mismatch,
                        "s1": original[mismatch] if mismatch < len(original) else None,
                        "host": actual[mismatch] if mismatch < len(actual) else None}}
                if not original or original != actual:
                    raise ValueError(f"Replacement host {label} stream differs from fresh S1")
            causal = tool.correlate(bundle, context["comparisons"])
            context["causal_observation"] = causal
            end = json.loads((bundle / "runtime/capture-end.json").read_text())
            if (causal["diagnostic_parse_errors"] or causal["missing_or_empty_streams"] or not causal["has_capture_end_event"]
                or any(end.get(key) != causal[key] for key in ("events_written", "states_written", "last_sequence"))
                or end.get("run_id") != bundle.name or end.get("schema_version") != 1 or end.get("status") != "completed"
                or end.get("truncated") is not False or end.get("pending_records") != 0
                or end.get("filtered_records") != context["comparisons"]["filtered"]["host_records"]
                or end.get("all_records") != context["comparisons"]["unfiltered"]["host_records"]):
                raise ValueError("Replacement host lacks complete causal capture evidence")
            if digest(binary) != context["binary"]["sha256"]:
                raise ValueError("Production host binary changed during replay")
            tool.verify_artifacts(boot_bundle, manifest)
            unchanged_stock()
            if retirement_shape(ROOT) or not candidate_source_matches(source):
                verdict = result("stale", "Candidate changed during retirement verification.", bundle=str(bundle))
            else:
                verdict = result("passed", "Fresh S1, approximation removal, replacement-host boot and workspace integration verified.", bundle=str(bundle))
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            verdict = result("failed", str(error), bundle=str(bundle))
        finally:
            context["verdict"] = verdict
            context["artifacts"] = {p.relative_to(bundle).as_posix(): {"sha256": digest(p), "bytes": p.stat().st_size}
                for p in bundle.rglob("*") if p.is_file() and "target" not in p.relative_to(bundle).parts}
            (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
        return verdict
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return result("failed", str(error))


def fixture_sources(root: Path) -> list[Path]:
    """Frozen inputs made available to the isolated ledger collector.

    Whole-game run products and the explicitly named media/research archives
    below are excluded. Boot's root vector files and the snapshot corpus are
    included. Unknown directories remain included, including hidden inputs;
    do not replace this with an extension allowlist or a research-name glob.
    A test requiring anything else must fail or emit the skip marker, which
    the observing Cargo wrapper rejects.
    """
    native_registration_rows((root / "re/out/native_api.tsv").read_text())
    required_snapshot = root / "re/out/snapshots/install-part-queue"
    for suffix in (".json", ".pages", ".install.json", ".install.ckpt"):
        path = required_snapshot.with_name(required_snapshot.name + suffix)
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"required original snapshot missing or empty: {path}")
    output = []
    excluded = {
        "checkpoints", "handoff-2026-10-20", "headless",
        # Video downloads, an isolated approximation capture/build, and static
        # critique research; none is an input to the production test collector.
        # Keep exclusions exact so new oracle fixtures are retained by default.
        "gameplay-comparison-2026-10-22",
        "implementation-showcase-2026-10-22",
        "critique-acceptance-2026-10-22",
    }
    for child in (root / "re/out").iterdir():
        if child.name in excluded or child.name == "test-results.json":
            continue
        if child.name == "boot":
            output.extend(p for p in child.iterdir() if p.is_file() and p.suffix in {".json", ".jsonl", ".bin", ".slrlog", ".ckpt", ".pages"})
        elif child.is_dir():
            output.extend(p for p in child.rglob("*") if p.is_file())
        elif child.suffix in {".json", ".jsonl", ".tsv"}:
            output.append(child)
    # Narrow original captures used by registered library/integration tests;
    # do not copy hundreds of GiB of unrelated headless run products.
    headless = root / "re/out/headless"
    clock = headless / "clock-investigation/menu1800M"
    output.extend(p for p in clock.iterdir() if p.is_file() and p.suffix in {".json", ".jsonl"})
    for relative in (
        "hypothetical-cd-toc.json",
        "boot/graphics-profile.json",
        "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
        "boot/platform-profile.json",
        "clock-investigation/menu1800M/observed/original-facts.json",
        "finalizer-investigation/gc-probe.json",
        "finalizer-investigation/string-add.json",
        "finalizer-investigation/long-observed/original-facts.json",
        "finalizer-investigation/callers-observed/original-facts.json",
        "finalizer-investigation/frame-long-observed/frame-timer-inputs.jsonl",
        "finalizer-investigation/frame-long-observed/original-facts.json",
        "oracle-33d8aa8/observed/script-natives.slrlog",
        "package-investigation/full-observed/original-facts.json",
        "allocation-history/long-observed/original-facts.json",
    ):
        path = headless / relative
        if not path.is_file():
            raise ValueError(f"required original capture missing: {relative}")
        output.append(path)
    # Complete original Euler/CFG consumer fixtures used by the default bin
    # tests. Checkpoint storage does not exempt these inputs from freezing.
    for relative in (
        "car-command/euler-specials-v2.json",
        "car-command/cfg-euler-v2.json",
    ):
        path = root / "re/out/checkpoints/20261021-campaign" / relative
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"required original consumer fixture missing: {relative}")
        output.append(path)
    return sorted(set(output))


def ledger_result(output_root: Path) -> dict:
    """Run collection/regeneration against retained source and frozen fixtures.

    Live execution only: there is no load-passed-receipt entry point.
    """
    tool = boot_tools()
    bundle = output_root / ("ledger-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    candidate = bundle / "candidate"
    candidate.mkdir()
    context = {"schema": 1, "obligation": "S11", "commands": []}
    verdict = result("failed", "Ledger validation did not complete.", bundle=str(bundle))
    try:
        context["source"], patch = tool.git_identity(ROOT)
        (bundle / "source.diff").write_bytes(patch)
        context["source_untracked"] = tool.archive_untracked_sources(ROOT, bundle, context["source"])
        names = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "--cached", "--others", "--exclude-standard", "-z"]).split(b"\0")
        for name in names:
            if not name:
                continue
            path = ROOT / os.fsdecode(name)
            if not path.exists():  # retained in the binary source diff
                continue
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"source is not an ordinary file: {path}")
            destination = candidate / path.relative_to(ROOT)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
        if not candidate_source_matches(context["source"]):
            raise ValueError("source changed while preserving candidate")
        context["stock"] = stock_identity(ROOT / "stock")
        context["compatibility"] = tool.tree_identity(ROOT / "reference/raw")
        context["fixtures"] = []
        for path in fixture_sources(ROOT):
            identity = tool.file_identity(path)
            relative = path.relative_to(ROOT)
            destination = candidate / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
            if digest(destination) != identity["sha256"]:
                raise ValueError(f"fixture changed while freezing: {relative}")
            context["fixtures"].append({**identity, "snapshot": relative.as_posix()})
        (candidate / "stock").symlink_to(ROOT / "stock", target_is_directory=True)
        (candidate / "reference").symlink_to(ROOT / "reference", target_is_directory=True)
        (bundle / "commands").mkdir()
        (bundle / "bin").mkdir()
        cargo = shutil.which("cargo")
        if not cargo:
            raise ValueError("Cargo unavailable")
        # Every child cargo test is observed, including output collect hides.
        wrapper = bundle / "bin/cargo"
        wrapper.write_text(f'''#!{sys.executable}
import json, pathlib, subprocess, sys, uuid
sys.path.insert(0, {str(candidate / "acceptance")!r})
from observations import rust_test_observation
command = [{cargo!r}, *sys.argv[1:]]
r = subprocess.run(command, capture_output=True, text=True)
base = pathlib.Path({str(bundle / "commands")!r}) / uuid.uuid4().hex
base.with_suffix('.stdout').write_text(r.stdout)
base.with_suffix('.stderr').write_text(r.stderr)
observation = rust_test_observation(r.returncode, r.stdout, r.stderr) if 'test' in sys.argv[1:] else None
base.with_suffix('.json').write_text(json.dumps({{'command': command, 'returncode': r.returncode, 'observation': observation}}, indent=2))
sys.stdout.write(r.stdout); sys.stderr.write(r.stderr)
sys.exit(r.returncode if observation is None or observation['valid_component_observation'] else 2)
''')
        wrapper.chmod(0o755)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
        environment.update({"PATH": str(bundle / "bin") + os.pathsep + os.environ.get("PATH", ""), "CARGO_TARGET_DIR": str(bundle / "target"), "SLR_STOCK": str(ROOT / "stock"), "SLR_REFERENCE": str(ROOT / "reference/raw")})
        context["configuration"] = {key: environment[key] for key in ("CARGO_TARGET_DIR", "SLR_STOCK", "SLR_REFERENCE", "RUSTFLAGS", "RUSTUP_TOOLCHAIN") if key in environment}
        context["tools"] = {"python": sys.version, "cargo": subprocess.check_output([cargo, "--version"], text=True).strip(), "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()}
        context["status"] = "running"
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
        def execute(label, args):
            command = [cargo, "run", "--locked", "-p", "slr-ledger", "--", *args]
            with (bundle / (label + ".stdout")).open("w") as stdout, (bundle / (label + ".stderr")).open("w") as stderr:
                process = subprocess.run(command, cwd=candidate, env=environment, stdout=stdout, stderr=stderr)
            context["commands"].append({"command": command, "cwd": str(candidate), "returncode": process.returncode, "stdout": label + ".stdout", "stderr": label + ".stderr"})
            if process.returncode:
                raise ValueError(f"{label} exited {process.returncode}; see retained outputs")
        expected_doc = (candidate / "docs/fidelity.md").read_bytes()
        execute("collect", ["collect"])
        observations = [json.loads(path.read_text()) for path in (bundle / "commands").glob("*.json")]
        if not observations or any(not row.get("observation", {}).get("valid_component_observation") for row in observations):
            raise ValueError("fresh collector did not produce valid observed test runs")
        rows = json.loads((candidate / "re/out/test-results.json").read_text())
        if not rows or any(row.get("status") != "pass" or not row.get("test") or not row.get("oracle") for row in rows):
            raise ValueError("fresh collection contains missing or failing evidence rows")
        context["evidence_rows"] = len(rows)
        execute("regen", ["regen"])
        generated = (candidate / "docs/fidelity.md").read_bytes()
        execute("regen-idempotence", ["regen"])
        if generated != (candidate / "docs/fidelity.md").read_bytes():
            raise ValueError("ledger regeneration is not idempotent")
        if generated != expected_doc:
            raise ValueError("freshly regenerated ledger differs from candidate fidelity.md")
        if context["stock"] != stock_identity(ROOT / "stock") or context["compatibility"] != tool.tree_identity(ROOT / "reference/raw"):
            raise ValueError("stock or compatibility corpus changed during validation")
        for identity in context["fixtures"]:
            if digest(Path(identity["path"])) != identity["sha256"]:
                raise ValueError(f"oracle fixture changed during validation: {identity['path']}")
        if not candidate_source_matches(context["source"]):
            verdict = result("stale", "Candidate source changed during isolated S11 verification.", bundle=str(bundle))
        else:
            verdict = result("passed", "Fresh observed collector, current-source ledger equality and idempotent regeneration verified.", bundle=str(bundle), evidence_rows=len(rows))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["status"] = "completed"
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
    return verdict
