#!/usr/bin/env python3
"""Run the pre-commit Rust checks and keep their evidence as an indexed run.

The checks are exactly the hook's, in the same order, with the same flags:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

What this adds, without weakening or skipping any of them:

* a fixture preflight: if a local `re/out` fixture is stale or changed, the hook
  stops in seconds with the one-line reason instead of failing 22 minutes in;
* a fixture guard: the watched fixtures are hashed before and after the run, and a
  change mid-run fails the hook and names the files;
* a durable record: each step's combined output and exit status, the commit,
  branch, duration and fixture state are written as a `kind: hook` bundle to the
  run archive and appended to the same `runs-index.jsonl` the boot runs use.

Recording is best effort and can never change a check's verdict; the exit status
is the first failing step's own, or 3 for a stale fixture, or 4 for a fixture
that changed during the run.
"""
from __future__ import annotations
import argparse
import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))
import diag_archive as archive
import diag_fixtures as fixtures

STEPS = (
    ("fmt", ["cargo", "fmt", "--all", "--", "--check"]),
    ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
    ("test", ["cargo", "test", "--workspace"]),
)
EXIT_STALE, EXIT_CHANGED = 3, 4


def load_boot():
    spec = importlib.util.spec_from_file_location("boot_diagnostics", HERE / "boot-diagnose.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def run_step(name, command, directory, repository):
    """Run one check, streaming its output to the terminal and to a file."""
    began = time.monotonic()
    path = directory / f"{name}.txt"
    with path.open("wb") as log:
        process = subprocess.Popen(command, cwd=repository, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        for chunk in iter(lambda: process.stdout.read1(65536), b""):
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            log.write(chunk)
        code = process.wait()
    return {"name": name, "command": command, "exit_code": code, "seconds": round(time.monotonic() - began, 3),
            "output": path.name, "output_bytes": path.stat().st_size}


def tail(path, limit=2000):
    try:
        data = Path(path).read_bytes()[-limit:]
    except OSError:
        return ""
    return data.decode(errors="replace")


def record(boot, root, directory, manifest, report):
    """Finalize the bundle and index it. Never raises: recording is best effort."""
    try:
        manifest["finished"] = utc()
        report["status"] = manifest["status"]
        boot.dump(directory / "report.json", report)
        manifest["artifacts"] = {path.relative_to(directory).as_posix(): boot.file_identity(path)
                                 for path in sorted(directory.rglob("*")) if path.is_file() and path.name != "manifest.json"}
        boot.dump(directory / "manifest.json", manifest)
        entry = boot.index_record(directory, manifest, report, root)
        entry.update({key: manifest[key] for key in ("steps", "duration_seconds", "fixtures", "index_tree") if key in manifest})
        boot.append_index(root, entry)
        print(f"hook-run: recorded {manifest['status']} run {manifest['run_id']} in {boot.index_path(root)}", file=sys.stderr)
    except Exception as error:  # noqa: BLE001 - the verdict must not depend on bookkeeping
        print(f"hook-run: could not record this run ({type(error).__name__}: {error}); checks are unaffected", file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repository", default=str(REPO))
    parser.add_argument("--runs-root", default=None, help="default: the boot runs directory of the archive")
    parser.add_argument("--manifest", default=None, help="fixtures manifest (default re/fixtures-manifest.json)")
    parser.add_argument("--lock", default=None)
    parser.add_argument("--step", action="append", metavar="NAME=CMD",
                        help="test hook: replace the steps (repeatable), CMD is split on spaces")
    options = parser.parse_args(argv)
    repository = Path(options.repository).resolve()
    boot = load_boot()
    root = Path(options.runs_root) if options.runs_root else archive.default_runs_root("boot")
    steps = STEPS
    if options.step:
        steps = tuple((item.split("=", 1)[0], item.split("=", 1)[1].split(" ")) for item in options.step)
    run_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ") + "-hook-" + uuid.uuid4().hex[:8]
    began = time.monotonic()
    manifest = {"schema_version": 1, "kind": "hook", "embedding": "hook", "run_id": run_id, "status": "running",
                "started": utc(), "arguments": sys.argv[1:], "steps": [], "artifacts": {}}
    report = {"comparisons": {}}
    directory = None
    try:
        directory = root / run_id
        directory.mkdir(parents=True)
        manifest["provenance"] = archive.provenance(repository)
        try:
            manifest["index_tree"] = subprocess.run(["git", "-C", str(repository), "write-tree"], capture_output=True,
                                                    text=True, timeout=60).stdout.strip() or None
        except (OSError, subprocess.TimeoutExpired):
            manifest["index_tree"] = None
        boot.dump(directory / "manifest.json", manifest)
    except Exception as error:  # noqa: BLE001
        print(f"hook-run: archive unavailable ({error}); running checks without a record", file=sys.stderr)
        directory = None

    def finish(code, status, error=None):
        manifest["status"], manifest["exit_code"] = status, code
        manifest["duration_seconds"] = round(time.monotonic() - began, 3)
        if error:
            report["error"] = error
        if directory is not None:
            record(boot, root, directory, manifest, report)
        return code

    # Fixture preflight and baseline digest. A broken registry must not block
    # commits on a tooling bug, but a stale fixture must.
    manifest_path = Path(options.manifest) if options.manifest else repository / fixtures.MANIFEST
    registry, baseline = None, {}
    if manifest_path.is_file():
        try:
            registry = fixtures.load_manifest(repository, manifest_path)
            lock = fixtures.load_lock(fixtures.lock_path(repository, registry, options.lock))
            results = fixtures.evaluate(repository, registry, lock)
            lines = fixtures.status_lines(results)
            manifest["fixtures"] = {"states": {state: sum(1 for row in results if row["state"] == state)
                                               for state in sorted({row["state"] for row in results})},
                                    "problems": [line for line in lines if not line.startswith("fixtures:")]}
            bad = fixtures.fatal(results)
            if bad:
                print("hook-run: refusing to start the test run; local fixtures are inconsistent:", file=sys.stderr)
                for line in lines:
                    print("  " + line, file=sys.stderr)
                print("  regenerate with the command shown, then `python3 re/boot-diagnose.py fixtures lock`.", file=sys.stderr)
                return finish(EXIT_STALE, "failed", "stale fixtures: " + "; ".join(row["path"] for row in bad))
            baseline = fixtures.snapshot(repository, registry)
        except Exception as error:  # noqa: BLE001
            print(f"hook-run: fixture check unavailable ({type(error).__name__}: {error}); continuing", file=sys.stderr)
            registry = None
    failed = None
    for name, command in steps:
        result = (run_step(name, command, directory, repository) if directory is not None
                  else run_step_untracked(name, command, repository))
        manifest["steps"].append(result)
        if result["exit_code"] != 0:
            failed = result
            break
    exit_code = failed["exit_code"] if failed else 0
    error = None
    if failed:
        error = f"{failed['name']} exited {failed['exit_code']}"
        report["step_tail"] = tail(directory / failed["output"]) if directory is not None else ""
    # Fixture guard: bytes that changed mid-run make a green test run meaningless
    # and a red one misleading, so say so whichever way the checks went.
    if registry is not None:
        try:
            after = fixtures.snapshot(repository, registry, baseline)
            moved = fixtures.changed_between(baseline, after)
            if moved:
                report["fixtures_changed_during_run"] = moved
                print("hook-run: fixtures changed while the checks ran; the result cannot be trusted:", file=sys.stderr)
                for line in moved:
                    print("  " + line, file=sys.stderr)
                if failed:
                    print("  the failure above may be caused by this change.", file=sys.stderr)
                error = (error + "; " if error else "") + "fixtures changed during the run: " + "; ".join(moved)
                exit_code = exit_code or EXIT_CHANGED
        except Exception as problem:  # noqa: BLE001
            print(f"hook-run: end-of-run fixture check failed ({problem}); continuing", file=sys.stderr)
    return finish(exit_code, "completed" if exit_code == 0 else "failed", error)


def run_step_untracked(name, command, repository):
    began = time.monotonic()
    code = subprocess.call(command, cwd=repository)
    return {"name": name, "command": command, "exit_code": code,
            "seconds": round(time.monotonic() - began, 3), "output": None, "output_bytes": 0}


if __name__ == "__main__":
    sys.exit(main())
