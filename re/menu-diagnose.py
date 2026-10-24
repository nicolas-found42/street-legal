#!/usr/bin/env python3
"""Interactive exact-port capture, historical queries and behavioral replay.

Independent of boot parity: a menu scenario never claims oracle equality.
Normal `slr-boot --window` launches are supervised here automatically.
"""
from __future__ import annotations
import argparse
import gzip
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tarfile
import time
import uuid

REPO = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("boot_diagnostics", REPO / "re/boot-diagnose.py")
boot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boot)
DEFAULT_ROOT = REPO / "re/out/menu/runs"


def records(bundle, tolerate_tail=False):
    """Stream segmented, optionally compressed evidence in sequence order."""
    root = Path(bundle) / "runtime"
    paths = sorted(root.glob("session-*.jsonl*"))
    for path in paths:
        opener = gzip.open if path.suffix == ".gz" else open
        with opener(path, "rt") as source:
            for number, line in enumerate(source, 1):
                try:
                    yield json.loads(line)
                except json.JSONDecodeError:
                    if tolerate_tail and path == paths[-1] and not line.endswith("\n"):
                        return
                    raise ValueError(f"malformed journal record: {path}:{number}")


def manifest(bundle):
    return json.loads((Path(bundle) / "manifest.json").read_text())


def menu_signature(row):
    state = row["data"]["state"]
    return {key: state.get(key) for key in ("focus", "selected", "pressed", "act", "labels", "exit", "game_state", "pending_state_call") if key in state or key not in ("game_state", "pending_state_call")}


def behavior(bundle):
    """Stable state transitions, not clocks, native IDs, or replay wall time."""
    states, renders, images, errors, pending, applied = [], [], [], [], {}, []
    sequence = 0
    run_id = manifest(bundle)["run_id"]
    for row in records(bundle):
        if row.get("run_id") != run_id or row["sequence"] != sequence + 1:
            raise ValueError("journal identity/sequence gap")
        sequence = row["sequence"]
        kind, data = row["kind"], row["data"]
        if kind == "input-apply":
            pending[sequence] = data
            applied.append({"tick": data["tick"], "event": data["event"]})
        elif kind == "input-result":
            if data["input_id"] not in pending:
                raise ValueError("input result has no recorded application")
            pending.pop(data["input_id"])
            if data.get("error"):
                errors.append(data["error"])
        elif kind == "menu-state":
            value = menu_signature(row)
            if not states or states[-1] != value:
                states.append(value)
        elif kind == "render-state":
            value = data["recipe_sha256"]
            if not renders or renders[-1] != value:
                renders.append(value)
        elif kind == "state-image":
            images.append({"recipe_sha256":data["recipe_sha256"],"png_sha256":data["png_sha256"]})
        elif kind in ("gpu-error", "gpu-device-lost", "surface-error", "worker-stall"):
            errors.append({"kind": kind, "data": data})
    return {"states": states, "renders": renders, "images":images, "errors": errors,
            "pending_input_ids": list(pending), "inputs": applied, "records": sequence}


def inventory(bundle):
    return {p.relative_to(bundle).as_posix(): boot.file_identity(p)
            for p in sorted(bundle.rglob("*")) if p.is_file() and p.name != "manifest.json"}


def compress(bundle):
    for pattern in ("*.jsonl",):
        for path in (bundle / "runtime").glob(pattern):
            if path.stat().st_size < 1024 * 1024:
                continue
            with path.open("rb") as source, gzip.open(str(path) + ".gz", "wb") as target:
                import shutil
                shutil.copyfileobj(source, target)
            path.unlink()


def get_option(arguments, name, default=None):
    if name not in arguments:
        return default
    index = arguments.index(name)
    if index + 1 >= len(arguments):
        raise ValueError(f"{name} needs a value")
    value = arguments[index + 1]
    del arguments[index:index + 2]
    return value


def launch(binary, arguments, runtime, timeout, stall_timeout):
    environment = os.environ.copy()
    environment.update(SLR_SESSION_CAPTURE="1", SLR_SESSION_RUNTIME=str(runtime),
                       SLR_SESSION_RUN_ID=runtime.parent.name)
    offsets, tail = {}, {}
    heartbeat = {"worker": time.monotonic(), "ui": time.monotonic()}
    window = "--window" in arguments
    reason = None
    with (runtime.parent / "stdout.txt").open("xb") as stdout, (runtime.parent / "stderr.txt").open("xb") as stderr:
        process = subprocess.Popen([str(binary), *arguments], cwd=REPO, env=environment,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        began = time.monotonic()
        previous = {}
        def interrupt(signum, frame):
            raise InterruptedError(f"supervisor signal {signum}")
        try:
            for sig in (signal.SIGINT, signal.SIGTERM):
                previous[sig] = signal.signal(sig, interrupt)
            while process.poll() is None:
                now = time.monotonic()
                for path in sorted(runtime.glob("session-*.jsonl")):
                    with path.open("rb") as source:
                        source.seek(offsets.get(path, 0))
                        data = tail.get(path, b"") + source.read()
                        offsets[path] = source.tell()
                    lines = data.split(b"\n")
                    tail[path] = lines.pop()
                    for line in lines:
                        row = json.loads(line)
                        if row["kind"] == "worker-heartbeat": heartbeat["worker"] = now
                        if row["kind"] in ("ui-heartbeat", "window-created"): heartbeat["ui"] = now
                if now - began > timeout:
                    reason = "session-timeout"
                    break
                missing = [k for k, value in heartbeat.items()
                           if (k != "ui" or window) and now - value > stall_timeout]
                if missing:
                    reason = "heartbeat-timeout:" + ",".join(missing)
                    break
                time.sleep(0.1)
        except (InterruptedError, KeyboardInterrupt) as error:
            reason = str(error)
        finally:
            for sig, handler in previous.items(): signal.signal(sig, handler)
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    return process.returncode, reason


def capture(args, arguments, prior=None):
    arguments = list(arguments)
    if arguments[:1] == ["--"]: arguments.pop(0)
    supplied_output = get_option(arguments, "--output")
    get_option(arguments, "--run-id")
    if not arguments: arguments = ["stock", "--window"]
    if args.headless:
        arguments = [arg for arg in arguments if arg != "--window"]
    if args.input_script:
        if "--input-script" in arguments: raise ValueError("duplicate input script")
        arguments.extend(["--input-script", str(args.input_script)])
    run_id = boot.utc().replace(":", "").replace("+", "_") + "-" + uuid.uuid4().hex[:10]
    root = Path(args.runs_root).resolve()
    bundle = Path(supplied_output).resolve() if supplied_output else root / run_id
    for protected in (REPO / "stock", REPO / "reference"):
        if bundle.is_relative_to(protected): raise ValueError("capture cannot write inside stock/reference")
    if bundle.exists(): raise ValueError(f"bundle already exists: {bundle}")
    bundle.mkdir(parents=True)
    runtime = bundle / "runtime"
    runtime.mkdir()
    inputs = bundle / "inputs"
    inputs.mkdir()
    # The directory name identifies both the cross-thread journal and VM capture.
    run_id = bundle.name
    value = {"schema_version": 1, "embedding": "slr-menu", "run_id": run_id,
             "status": "running", "started": boot.utc(), "artifacts": {},
             "configuration": {"arguments": arguments, "wall_clock": True, "compact": not args.full,
                               "timeout": args.timeout, "stall_timeout": args.stall_timeout},
             "environment": boot.environment_identity(REPO), "comparisons": None,
             "supervisor": {"pid":os.getpid(),"identity":process_identity(os.getpid())}}
    if prior: value["source_run"] = {"run_id": manifest(prior)["run_id"], "bundle": str(prior)}
    boot.dump(bundle / "manifest.json", value)
    boot.append_index(root, boot.index_record(bundle, value, {}))
    print(json.dumps({"bundle": str(bundle), "status": "running"}), flush=True)
    report = {"comparisons": {}, "parity": "not-evaluated"}
    exit_code = 1
    try:
        if args.expect:
            expectation=json.loads(Path(args.expect).read_text())
            (inputs / "expect.json").write_text(json.dumps(expectation,indent=2)+"\n")
            value["expectation"]=expectation
        value["source"], diff = boot.git_identity(REPO, (root, bundle, boot.index_path(root), boot.archive_root(root) / "inputs-store"))
        (bundle / "source.diff").write_bytes(diff)
        value["source_untracked"] = boot.archive_untracked_sources(REPO, bundle, value["source"])
        binary = Path(args.binary).resolve(strict=True)
        value["binary"] = boot.file_identity(binary)
        value["source_binding"] = "observed-workspace-not-build-attestation"
        if prior and value["binary"]["sha256"] == manifest(prior)["binary"]["sha256"]:
            value["binary_source_run"] = manifest(prior)["run_id"]
        value["binary_snapshot"] = boot.snapshot(binary, inputs, "binary", root)
        (inputs / value["binary_snapshot"]["snapshot"]).chmod(binary.stat().st_mode & 0o777)
        stock_arg = next((arg for arg in arguments if not arg.startswith("--")), "stock")
        stock = Path(stock_arg)
        if not stock.is_absolute(): stock = REPO / stock
        value["stock"] = boot.tree_identity(stock)
        if prior and value["stock"]["sha256"] != manifest(prior)["stock"]["sha256"]:
            raise ValueError("stock differs from recorded session")
        # Freeze recognized script bytes; replay never depends on a former path.
        script = get_option(arguments, "--input-script")
        if script:
            source = Path(script).resolve(strict=True)
            saved = inputs / "input-script.json"
            saved.write_bytes(source.read_bytes())
            json.loads(saved.read_text())
            value["input_script"] = boot.file_identity(saved)
            arguments.extend(["--input-script", str(saved)])
        value["configuration"]["arguments"] = arguments
        if args.full:
            get_option(arguments,"--trace-level")
            arguments.extend(["--trace-level","full"])
        command = [*arguments, "--output", str(runtime), "--run-id", run_id, "--wall-clock"]
        value["child_arguments"] = command
        boot.dump(bundle / "manifest.json", value)
        returncode, reason = launch(binary, command, runtime, args.timeout, args.stall_timeout)
        report.update(returncode=returncode, termination=reason)
        end = runtime / "capture-end.json"
        report["capture_end"] = json.loads(end.read_text()) if end.exists() else None
        observed = behavior(bundle)
        report["behavior"] = observed
        (inputs / "recorded-input.json").write_text(json.dumps(observed["inputs"], indent=2) + "\n")
        if reason or returncode or not report["capture_end"] or report["capture_end"]["status"] != "completed":
            raise ValueError(reason or f"child exit {returncode}; capture may be incomplete")
        if observed["pending_input_ids"] or observed["errors"]: raise ValueError("interactive evidence contains failed/incomplete operations")
        if not observed["renders"] or not observed["states"]: raise ValueError("no completed interactive menu evidence")
        if "--window" in arguments:
            presented = [r for r in records(bundle) if r["kind"] == "frame-presented"]
            if not presented: raise ValueError("window never presented a correlated frame")
            if not any(r["kind"] == "ui-exit" and r["data"].get("error") is None for r in records(bundle)):
                raise ValueError("UI did not complete cleanly")
        if boot.file_identity(binary)["sha256"] != value["binary"]["sha256"]: raise ValueError("binary changed during session")
        current, _ = boot.git_identity(REPO, (root, bundle, boot.index_path(root), boot.archive_root(root) / "inputs-store"))
        if current != value["source"]: raise ValueError("source changed during session")
        if boot.tree_identity(stock)["sha256"] != value["stock"]["sha256"]: raise ValueError("stock changed during session")
        if prior:
            expected = behavior(prior)
            mismatches = {}
            prefix_only = manifest(prior)["status"] != "completed"
            for key in ("states", "renders", "images"):
                a, b = expected[key], observed[key]
                if key=="states" and a:
                    fields=set().union(*(row.keys() for row in a))
                    projected=[]
                    for row in b:
                        row={field:value for field,value in row.items() if field in fields}
                        if not projected or projected[-1]!=row:projected.append(row)
                    b=projected
                first = next((i for i, pair in enumerate(zip(a, b)) if pair[0] != pair[1]), min(len(a), len(b)))
                if a != (b[:len(a)] if prefix_only else b): mismatches[key] = {"transition": first, "expected": a[first] if first < len(a) else None,
                                            "actual": b[first] if first < len(b) else None,
                                            "expected_count": len(a), "actual_count": len(b)}
            report["verification"] = {"passed": not mismatches, "mismatches": mismatches,
                                      "policy": "prior-successful-prefix-and-clean-completion" if prefix_only else "exact-menu-state-render-recipe-and-png-transitions"}
            if mismatches: raise ValueError("recorded menu behavior did not reproduce")
        if value.get("expectation") is not None:
            expected=value["expectation"]
            if not isinstance(expected,dict) or set(expected)-{"labels_include","selected","exit","game_state","pending_state_call"}:
                raise ValueError("expectation supports labels_include, selected, exit, game_state and pending_state_call")
            final=observed["states"][-1]
            assertions={key:(all(label in final.get("labels",[]) for label in wanted) if key=="labels_include" else final.get(key)==wanted) for key,wanted in expected.items()}
            report["assertions"]={"passed":all(assertions.values()),"results":assertions,"final":final}
            if not report["assertions"]["passed"]:raise ValueError("menu scenario assertions failed")
        value["status"], exit_code = "completed", 0
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        value["status"] = "interrupted" if "signal" in str(error) else "failed"
    finally:
        value["finished"] = boot.utc()
        # Failures stay pinned until explicitly released; never silently prune incidents.
        value["pinned"] = value["status"] != "completed"
        report["status"] = value["status"]
        boot.dump(bundle / "report.json", report)
        compress(bundle)
        value["artifacts"] = inventory(bundle)
        boot.dump(bundle / "manifest.json", value)
        boot.append_index(root, boot.index_record(bundle, value, report))
    return exit_code, bundle


def process_identity(pid):
    try:
        return subprocess.check_output(["ps","-p",str(pid),"-o","lstart="],stderr=subprocess.DEVNULL,text=True).strip() or None
    except (OSError,subprocess.CalledProcessError):return None


def recover(root):
    """A killed supervisor is discoverable without inventing successful evidence."""
    results = []
    root = root.resolve()
    paths = {Path(row["bundle"]).resolve() for row in boot.read_index(root)}
    if root.exists(): paths.update(path.resolve() for path in root.iterdir() if path.is_dir())
    for bundle in paths:
        if not (bundle / "manifest.json").is_file(): continue
        value = manifest(bundle)
        status=value["status"]
        owner=value.get("supervisor",{})
        if status=="running" and owner.get("pid") and process_identity(owner["pid"])!=owner.get("identity"):
            status="interrupted"
        results.append({"bundle": str(bundle), "status": status, "run_id": value["run_id"],
                        "started": value["started"], "pinned": value.get("pinned", status != "completed") or value["status"] == "running",
                        "error": json.loads((bundle / "report.json").read_text()).get("error") if (bundle / "report.json").exists() else None})
    return sorted(results, key=lambda row: row["started"])


def prune(root, keep, max_bytes):
    if keep < 1 or max_bytes < 1: raise ValueError("retention limits must be positive")
    candidates = [row for row in recover(root) if Path(row["bundle"]).is_relative_to(root.resolve())]
    removed = []
    def size(bundle): return sum(v.get("bytes", 0) for v in manifest(bundle).get("artifacts", {}).values())
    total = sum(size(r["bundle"]) for r in candidates)
    remaining = len(candidates)
    for row in candidates:
        if remaining <= keep and total <= max_bytes: break
        if row["pinned"] or row["status"] == "running" or row == candidates[-1]: continue
        bundle = Path(row["bundle"])
        total -= size(bundle)
        import shutil
        shutil.rmtree(bundle)
        remaining -= 1
        removed.append(str(bundle))
    # Reflink copies have independent inodes. Reachability is by content hash.
    if not any(row["status"]=="running" for row in recover(root)):
        live=set()
        for row in recover(root):
            value=manifest(row["bundle"])
            live.update(item.get("sha256") for item in value.get("artifacts",{}).values())
        store=boot.archive_root(root)/"inputs-store"
        if store.exists():
            for path in store.iterdir():
                if path.is_file() and path.name not in live:path.unlink()
    return {"removed": removed, "remaining_bytes": total, "budget_blocked_by_protected_runs": total > max_bytes}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("record", "replay", "verify"):
        p = sub.add_parser(name)
        if name != "record": p.add_argument("bundle", type=Path)
        p.add_argument("--binary", type=Path, default=None)
        p.add_argument("--runs-root", type=Path, default=DEFAULT_ROOT)
        p.add_argument("--headless", action="store_true")
        p.add_argument("--full", action="store_true", help="persist VM stores and instructions; much larger capture")
        p.add_argument("--input-script", type=Path)
        p.add_argument("--expect", type=Path, help="final menu assertions: labels_include, selected, exit")
        p.add_argument("--timeout", type=float, default=3600)
        p.add_argument("--stall-timeout", type=float, default=60)
        p.add_argument("--repeat", type=int, default=1)
        p.add_argument("--keep", type=int, default=20)
        p.add_argument("--max-bytes", type=int, default=2 * 1024**3)
    for name in ("status", "latest-failure", "prune"):
        p = sub.add_parser(name)
        p.add_argument("--runs-root", type=Path, default=DEFAULT_ROOT)
        if name == "prune":
            p.add_argument("--keep", type=int, default=20)
            p.add_argument("--max-bytes", type=int, default=2 * 1024**3)
    for name in ("timeline", "inspect", "export", "unpin"):
        p = sub.add_parser(name)
        p.add_argument("bundle", type=Path)
        if name == "timeline":
            p.add_argument("--event", type=int)
            p.add_argument("--radius", type=int, default=10)
            p.add_argument("--vm", action="store_true", help="include VM events at the selected input frame")
            p.add_argument("--limit", type=int, default=200)
        if name == "export": p.add_argument("destination", type=Path)
    args, extra = parser.parse_known_args(argv)
    if args.command in ("record", "replay", "verify"):
        if min(args.timeout, args.stall_timeout, args.repeat, args.keep, args.max_bytes) <= 0: parser.error("limits must be positive")
        prior = None
        if args.command != "record":
            prior = args.bundle.resolve()
            value = manifest(prior)
            boot.verify_artifacts(prior, value)
            if not (prior / "inputs/recorded-input.json").exists(): raise ValueError("no complete input tape; inspect interrupted evidence instead")
            if args.input_script: raise ValueError("replay must use the frozen tape")
            if args.binary is None and args.command == "replay":
                args.binary = prior / "inputs" / value["binary_snapshot"]["snapshot"]
            if args.expect is None and (prior / "inputs/expect.json").exists():args.expect=prior/"inputs/expect.json"
            args.input_script = prior / "inputs/recorded-input.json"
            extra = list(value["configuration"]["arguments"])
            recorded_frames = get_option(extra, "--frames")
            for key in ("--output", "--run-id", "--input-script"): get_option(extra, key)
            extra = [arg for arg in extra if arg != "--window"]
            tape = json.loads(args.input_script.read_text())
            extra.extend(["--frames", recorded_frames or str(max((e["tick"] for e in tape), default=0) + 1000)])
            args.headless = True
        if args.binary is None: args.binary = REPO / "target/debug/slr-boot"
        results = []
        for _ in range(args.repeat):
            code, bundle = capture(args, extra, prior)
            results.append({"exit_code": code, "bundle": str(bundle)})
        print(json.dumps({"runs": results, "failure_rate": sum(r["exit_code"] != 0 for r in results) / len(results),
                          "retention": prune(args.runs_root, args.keep, args.max_bytes)}))
        return int(any(r["exit_code"] for r in results))
    if extra: parser.error("unrecognized arguments: " + " ".join(extra))
    if args.command in ("status", "latest-failure"):
        rows = recover(args.runs_root)
        if args.command == "latest-failure": rows = [r for r in rows if r["status"] in ("failed", "interrupted")][-1:]
        print(json.dumps(rows, indent=2))
    elif args.command == "prune": print(json.dumps(prune(args.runs_root, args.keep, args.max_bytes)))
    elif args.command == "timeline":
        if args.limit<1 or args.radius<0:raise ValueError("timeline limits must be positive (radius may be zero)")
        selected, session_total = [], 0
        for row in records(args.bundle, True):
            if args.event is None or abs(row["sequence"] - args.event) <= args.radius:
                session_total += 1
                if len(selected)<args.limit:selected.append(row)
        vm, total = [], 0
        if args.vm:
            frames = {r["context"].get("frame") for r in selected if r.get("context", {}).get("frame") is not None}
            for path in sorted((args.bundle / "runtime").glob("events.jsonl*")):
                opener = gzip.open if path.suffix == ".gz" else open
                with opener(path,"rt") as source:
                    for line in source:
                        try: row = json.loads(line)
                        except json.JSONDecodeError: continue
                        if row.get("context", {}).get("frame") in frames:
                            total += 1
                            if len(vm)<args.limit: vm.append(row)
        print(json.dumps({"session":selected[:args.limit],"session_total":session_total,"vm":vm,"vm_total":total,"limit":args.limit}))
    elif args.command == "inspect":
        value = manifest(args.bundle)
        verified = value["status"] != "running"
        if verified: boot.verify_artifacts(args.bundle, value)
        print(json.dumps({"manifest": value, "verified": verified, "behavior": behavior(args.bundle) if verified else None}, indent=2))
    elif args.command == "export":
        boot.verify_artifacts(args.bundle, manifest(args.bundle))
        if args.destination.resolve().is_relative_to(args.bundle.resolve()):raise ValueError("export destination cannot be inside the evidence bundle")
        with args.destination.open("xb") as output, tarfile.open(fileobj=output, mode="w:gz") as archive:
            archive.add(args.bundle, arcname=args.bundle.name)
        print(json.dumps(boot.file_identity(args.destination)))
    elif args.command == "unpin":
        value = manifest(args.bundle)
        value["pinned"] = False
        boot.dump(args.bundle / "manifest.json", value)
    return 0


if __name__ == "__main__":
    try: sys.exit(main())
    except (ValueError, OSError) as error:
        print(json.dumps({"error": str(error)}), file=sys.stderr)
        sys.exit(1)
