"""S10 helpers: approximation retirement shape and replacement-host checks."""
from __future__ import annotations

import json
import os
import re
import subprocess
import tomllib
import uuid
from pathlib import Path

from . import boot, core

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
        errors = retirement_shape(core.ROOT)
        if errors:
            return core.result("failed", "Approximation retirement prerequisites failed.", errors=errors)
        if not live_execution:
            return core.result("not_run", "Run --execute S10 for fresh S1, replacement-host boot and workspace integration checks.")
        if not fresh_boot or fresh_boot.get("result") != "passed":
            return core.result("blocked", "S10 requires a freshly computed passing S1 in this invocation.", prerequisite=fresh_boot)
        tool = core.boot_tools()
        boot_bundle = Path(fresh_boot["bundle"])
        manifest = tool.bundle_manifest(boot_bundle)
        # Recheck raw streams, inputs, source and artifact identities. This
        # recomparison cannot itself replace the live prerequisite above.
        observed = boot.boot_result(boot_bundle)
        if observed["result"] != "replay_required":
            return core.result("blocked", "Fresh S1 evidence failed revalidation.", prerequisite=observed)
        source = manifest["source"]
        def unchanged_stock():
            core.stock_identity(core.ROOT / manifest["configuration"]["stock"])
            if tool.tree_identity(core.ROOT / manifest["configuration"]["stock"])["sha256"] != manifest["stock_inputs_sha256"]:
                raise ValueError("Stock inputs changed after fresh S1")
        unchanged_stock()
        bundle = output_root / ("retirement-" + uuid.uuid4().hex[:12])
        bundle.mkdir(parents=True)
        context = {"schema": 1, "obligation": "S10", "source": source,
            "s1_bundle": str(boot_bundle), "tools": tool.environment_identity(core.ROOT), "commands": []}
        verdict = core.result("failed", "Retirement validation did not complete.", bundle=str(bundle))
        try:
            environment = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
            environment["CARGO_TARGET_DIR"] = str(bundle / "target")
            context["environment"] = {key: environment[key] for key in ("CARGO_TARGET_DIR", "RUSTFLAGS", "RUSTUP_TOOLCHAIN") if key in environment}
            def execute(label, command, *, tests=False):
                stdout, stderr = bundle / (label + ".stdout"), bundle / (label + ".stderr")
                with stdout.open("w") as out, stderr.open("w") as err:
                    process = subprocess.run(command, cwd=core.ROOT, env=environment, stdout=out, stderr=err, timeout=1800)
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
            if core.digest(binary) != context["binary"]["sha256"]:
                raise ValueError("Production host binary changed during replay")
            tool.verify_artifacts(boot_bundle, manifest)
            unchanged_stock()
            if retirement_shape(core.ROOT) or not core.candidate_source_matches(source):
                verdict = core.result("stale", "Candidate changed during retirement verification.", bundle=str(bundle))
            else:
                verdict = core.result("passed", "Fresh S1, approximation removal, replacement-host boot and workspace integration verified.", bundle=str(bundle))
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            verdict = core.result("failed", str(error), bundle=str(bundle))
        finally:
            context["verdict"] = verdict
            context["artifacts"] = {p.relative_to(bundle).as_posix(): {"sha256": core.digest(p), "bytes": p.stat().st_size}
                for p in bundle.rglob("*") if p.is_file() and "target" not in p.relative_to(bundle).parts}
            (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
        return verdict
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        return core.result("failed", str(error))
