#!/usr/bin/env python3
"""Run stock boot into a unique, inspectable diagnostics bundle; fail closed.

Usage: boot-diagnose.py [original.slrlog] [options] [-- Rust arguments]
       boot-diagnose.py analyze BUNDLE [--report NEW_PATH]

Completed means the child exited zero AND both complete native logs match.
Neither a common prefix nor a clean runtime exit establishes parity.
"""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import signal
import shutil
import stat
import struct
import subprocess
import sys
import time
import uuid

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "re/headless"))
import calllog

SCHEMA = 1
MASK_POLICY = {"reference_arguments_and_results": True,
               "masked_clock_results": ["java.lang.System.currentTime", "java.lang.System.simTime"],
               "primitive_ids_masked": False, "ambiguous_overloads":"retain_raw_words"}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def dump(path, value):
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("x") as output:
        json.dump(value, output, indent=2, sort_keys=True, default=lambda value: {"bytes_hex": value.hex()} if isinstance(value, bytes) else str(value))
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, path)


def file_identity(path):
    path = Path(path).resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    after = path.stat()
    if (before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_ino, after.st_size, after.st_mtime_ns):
        raise ValueError(f"input changed while hashing: {path}")
    return {"path": str(path), "bytes": after.st_size, "sha256": digest.hexdigest()}


def tree_identity(root):
    root = Path(root).resolve(strict=True)
    entries = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"input-tree symlink requires explicit provenance: {path}")
        if path.is_file():
            record = file_identity(path)
            record["path"] = path.relative_to(root).as_posix()
            entries.append(record)
    encoded = json.dumps(entries, separators=(",", ":"), sort_keys=True).encode()
    return {"root": str(root), "files": entries, "sha256": sha(encoded)}


def git_identity(repository):
    def git(*args):
        return subprocess.check_output(["git", "-C", str(repository), *args])
    commit = git("rev-parse", "HEAD").decode().strip()
    # Include untracked source bytes, not only Git's tracked-file diff.
    tracked = git("diff", "HEAD", "--binary")
    untracked = []
    for raw in git("ls-files", "--others", "--exclude-standard", "-z").split(b"\0"):
        if raw:
            path = repository / os.fsdecode(raw)
            record = file_identity(path)
            record["path"] = os.fsdecode(raw)
            untracked.append(record)
    body = tracked + b"\0" + json.dumps(untracked, sort_keys=True).encode()
    return {"commit": commit, "dirty": bool(tracked or untracked), "dirty_diff_sha256": sha(body),
            "tracked_diff_sha256": sha(tracked), "untracked": untracked}, tracked


def archive_untracked_sources(repository, bundle, source):
    """Retain the bytes named by git_identity, without changing that identity."""
    archived = []
    for expected in source["untracked"]:
        relative = Path(expected["path"])
        path = repository / relative
        if relative.is_absolute() or ".." in relative.parts:
            raise ValueError("untracked source path escapes repository")
        # A hash of a symlink target does not capture the source tree's link
        # semantics. Reject rather than silently turning it into a regular file.
        if any(candidate.is_symlink() for candidate in (path, *path.parents)
               if candidate != repository and candidate.is_relative_to(repository)):
            raise ValueError(f"untracked source symlink requires explicit provenance: {relative}")
        before = path.stat()
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"untracked source is not a regular file: {relative}")
        destination = bundle / "source-untracked" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        with path.open("rb") as original, destination.open("xb") as saved:
            shutil.copyfileobj(original, saved)
            saved.flush()
            os.fsync(saved.fileno())
        actual = file_identity(destination)
        current = file_identity(path)
        if any(record[key] != expected[key]
               for record in (actual, current) for key in ("sha256", "bytes")) or path.stat().st_mode != before.st_mode:
            raise ValueError(f"untracked source changed while archiving: {relative}")
        mode = stat.S_IMODE(before.st_mode)
        destination.chmod(mode)
        archived.append({**expected, "snapshot": destination.relative_to(bundle).as_posix(), "mode": mode})
    return archived


def load_records(path):
    data = Path(path).read_bytes()
    if len(data) > 256 * 1024 * 1024:
        raise ValueError("SLRLOG exceeds256MiB diagnostic bound")
    reader = calllog.Reader(data)
    offset = 8
    while offset < len(data):
        if offset + 5 > len(data):
            raise ValueError("truncated SLRLOG record header")
        tag, size = struct.unpack_from("<BI", data, offset)
        if tag > 6 or size > calllog.MAX_BODY or offset + 5 + size > len(data):
            raise ValueError("unknown, oversized or truncated SLRLOG record")
        if tag == 0:
            body = calllog.Reader.__new__(calllog.Reader)
            body.data, body.offset = data[offset + 5:offset + 5 + size], 0
            body.string(); body.string(); body.values()
            if body._take(1)[0] > 1:
                raise ValueError("invalid SLRLOG native result discriminator")
        offset += 5 + size
    return reader.records()


def native_prefix(path):
    """Diagnostic only: retain decoded complete records before a crash tail."""
    if not path.exists(): return {"missing":True,"valid_records":0}
    data = path.read_bytes()
    records, offset, error = [], 8, None
    try:
        calllog.Reader(data)
        while offset < len(data):
            if offset + 5 > len(data): raise ValueError("truncated record header")
            tag, size = struct.unpack_from("<BI",data,offset)
            if size > calllog.MAX_BODY or offset + 5 + size > len(data): raise ValueError("truncated or oversized record")
            decoded = calllog.Reader(data[:8] + data[offset:offset+5+size]).records()
            records.extend(decoded)
            offset += 5 + size
    except Exception as exception: error = str(exception)
    return {"valid_records":len(records),"valid_bytes":offset,"total_bytes":len(data),
        "tail_error":error,"tail_error_offset":offset if error else None,"recent_records":records[-12:],"acceptance_evidence":False}


def descriptor(signature):
    if not signature.startswith("(") or ")" not in signature:
        return None
    parameters, result = signature[1:].split(")", 1)
    kinds, index = [], 0
    while index < len(parameters):
        kind = parameters[index]
        kinds.append(kind)
        index += 1
        if kind == "[":
            while index < len(parameters) and parameters[index] == "[": index += 1
            if index >= len(parameters): return None
            kind = parameters[index]
            index += 1
        if kind == "L":
            end = parameters.find(";", index)
            if end < 0: return None
            index = end + 1
    return kinds, result[:1] or "V"


def signatures(path):
    result = {}
    for line in Path(path).read_text().splitlines():
        columns = line.split("\t")
        if len(columns) >= 3 and (types := descriptor(columns[2])):
            args, ret = types
            result.setdefault((columns[0], columns[1], len(args)), []).append((args, ret))
    return result


def masked(record, types, clock_mask=True):
    if record[0] != "native": return record
    _, cls, name, args, result, present = record
    args = list(args)
    candidates = types.get((cls, name, len(args)), [])
    for index in range(len(args)):
        references = [kinds[index] in ("L", "[") for kinds, _ in candidates]
        if references and all(references): args[index] = ("int", 0)
    returns = [ret in ("L", "[") for _, ret in candidates]
    if result is not None and ((returns and all(returns))
            or clock_mask and cls == "java.lang.System" and name in ("currentTime", "simTime")):
        result = ("int", 0)
    return ("native", cls, name, args, result, present)


def oracle_descriptors(path, records):
    facts = json.loads(Path(path).read_text())
    rows = facts.get("native_calls", [])
    if len(rows) < len(records): raise ValueError("oracle descriptor authority truncated before log boundary")
    identities, output = {}, []
    def identity(word):
        if not word: return 0
        if word not in identities: identities[word] = len(identities) + 1
        return identities[word]
    def value(kind, word):
        if kind == "F": return ("float", word)
        if kind in ("L", "["): return ("int", identity(word))
        return ("int", word - (1 << 32) if word >= 1 << 31 else word)
    for ordinal, (row, record) in enumerate(zip(rows, records),1):
        signature = descriptor(row["descriptor"])
        if not signature or row["i"] != ordinal: raise ValueError("oracle descriptor authority ordinal malformed")
        kinds, result_kind = signature
        if len(kinds) != len(row["args"]): raise ValueError("oracle descriptor argument count mismatch")
        args = []
        for kind, cell in zip(kinds,row["args"]):
            cell_kind = (cell.get("kind") or "?")[:1]
            if cell_kind not in "IL[FDJZBCS": raise ValueError("oracle argument cell kind unsupported")
            args.append(value(cell_kind,cell["payload"]))
        # Match observer.py's existing encoding, including its full-return-
        # descriptor comparison: "Ljava...;" returns remain raw signed words.
        # Argument normalization similarly follows the observed cell tag,
        # which can be I even when the declared parameter is a reference.
        raw_return_kind = row["descriptor"].rsplit(")",1)[1]
        result = None if result_kind == "V" else value(raw_return_kind,row["result"])
        reconstructed = ("native",row["class"],row["name"],args,result,result_kind != "V")
        if reconstructed != record: raise ValueError(f"oracle descriptor authority does not match frozen log at {ordinal}")
        output.append(signature)
    return output


def compare(expected, actual, types, clock_mask=True, authorities=None):
    left, right = load_records(expected), load_records(actual)
    common = 0
    for common, (a, b) in enumerate(zip(left, right)):
        selected_types = types
        if authorities:
            selected_types = dict(types)
            selected_types[(a[1],a[2],len(a[3]))] = [authorities[common]]
        if masked(a, selected_types, clock_mask) != masked(b, selected_types, clock_mask):
            return {"equal": False, "kind": "record_mismatch", "record": common + 1,
                    "expected": a, "actual": b, "expected_records": len(left), "actual_records": len(right)}
    common = min(len(left), len(right))
    if len(left) != len(right):
        return {"equal": False, "kind": "length_mismatch", "record": common + 1,
                "expected": left[common] if common < len(left) else None,
                "actual": right[common] if common < len(right) else None,
                "expected_records": len(left), "actual_records": len(right)}
    return {"equal": True, "records": len(left)}


class InterruptedRun(Exception):
    pass


def child(argv, cwd, environment, stdout, stderr, timeout):
    process = None
    try:
        with Path(stdout).open("xb") as out, Path(stderr).open("xb") as err:
            process = subprocess.Popen(argv, cwd=cwd, env=environment, stdout=out, stderr=err, start_new_session=True)
            return {"returncode": process.wait(timeout=timeout), "pid": process.pid, "timed_out": False}
    except subprocess.TimeoutExpired:
        return {"returncode": None, "pid": process.pid, "timed_out": True}
    finally:
        if process is not None:
            try: os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError: pass
            try: process.wait(timeout=3)
            except subprocess.TimeoutExpired: pass
            # The process leader may have exited while grandchildren remain.
            try: os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError: pass
            process.wait()


def verify_artifacts(bundle, manifest):
    if manifest.get("status") not in ("completed", "failed", "interrupted"):
        raise ValueError("bundle is running or unfinished; no completed-run acceptance")
    artifacts = manifest.get("artifacts", {})
    if not artifacts or "report.json" not in artifacts: raise ValueError("artifact manifest incomplete")
    actual_files = {path.relative_to(bundle).as_posix() for path in bundle.rglob("*") if path.is_file() and path != bundle / "manifest.json"}
    if actual_files != set(artifacts): raise ValueError("artifact manifest does not cover bundle files")
    for relative, expected in artifacts.items():
        path = (bundle / relative).resolve()
        if not path.is_relative_to(bundle.resolve()): raise ValueError("artifact path escapes bundle")
        actual = file_identity(path)
        if actual["sha256"] != expected["sha256"] or actual["bytes"] != expected["bytes"]:
            raise ValueError(f"bundle artifact changed: {relative}")
    # Older bundles may have only source hashes; do not invent archived bytes
    # for them. New bundles bind every preserved untracked file to that hash.
    if "source_untracked" in manifest:
        archived = manifest["source_untracked"]
        expected_sources = manifest["source"]["untracked"]
        if [{key: row[key] for key in ("path", "sha256", "bytes")} for row in archived] != expected_sources:
            raise ValueError("untracked source archive does not match source identity")
        for row in archived:
            expected_path = (Path("source-untracked") / row["path"]).as_posix()
            if row["snapshot"] != expected_path or expected_path not in artifacts:
                raise ValueError("untracked source archive is incomplete")
            saved = bundle / row["snapshot"]
            if saved.is_symlink() or stat.S_IMODE(saved.stat().st_mode) != row["mode"]:
                raise ValueError("untracked source archive mode changed")
            if any(artifacts[expected_path][key] != row[key] for key in ("sha256", "bytes")):
                raise ValueError("untracked source archive content differs from source identity")


def correlate(bundle, comparisons):
    events, states, parse_errors, missing = [], [], [], []
    for filename, output in [("events.jsonl", events), ("states.jsonl", states)]:
        path = bundle / "runtime" / filename
        if path.exists():
            offset = 0
            for raw in path.read_bytes().splitlines(keepends=True):
                try:
                    if not raw.endswith(b"\n"): raise ValueError("unterminated JSON line")
                    output.append(json.loads(raw))
                except Exception as error:
                    parse_errors.append({"file":filename,"byte_offset":offset,"error":str(error),"valid_records":len(output)})
                    break
                offset += len(raw)
        if not output: missing.append(filename)
    manifest = json.loads((bundle / "manifest.json").read_text())
    previous = -1
    for event_index, event in enumerate(events):
        sequence = event.get("sequence")
        if event.get("schema_version") != 1 or event.get("run_id") != manifest["run_id"] or not isinstance(sequence, int) or sequence <= previous:
            parse_errors.append({"file":"events.jsonl","error":"event run identity, schema or sequence invalid","valid_records":event_index})
            events = events[:event_index]
            break
        previous = sequence
    prior_state_sequence = -1
    event_sequences = {event["sequence"] for event in events}
    for state_index, state in enumerate(states):
        provenance = state.get("provenance", {})
        if state.get("schema_version") != 1 or provenance.get("run_id") != manifest["run_id"] or not isinstance(state.get("sequence"), int) or state["sequence"] <= prior_state_sequence or state["sequence"] not in event_sequences:
            parse_errors.append({"file":"states.jsonl","error":"state run identity, schema or sequence invalid","valid_records":state_index})
            states = states[:state_index]
            break
        prior_state_sequence = state["sequence"]
    selected, candidates = None, []
    for kind in ("filtered", "unfiltered"):
        divergence = comparisons.get(kind, {})
        if divergence.get("equal") is False and divergence.get("record"):
            field = "filtered_ordinal" if kind == "filtered" else "all_ordinal"
            for index, event in enumerate(events):
                pairs = event.get("kind", {}).get("fields", []) if isinstance(event.get("kind"), dict) else []
                values = dict(pairs) if isinstance(pairs, list) else pairs
                if str(values.get(field)) == str(divergence["record"] - 1):
                    candidates.append({"comparison": kind, "event": event, "preceding_events": events[max(0, index - 12):index]})
                    break
    if candidates: selected = min(candidates, key=lambda candidate:int(dict(candidate["event"]["kind"]["fields"])["all_ordinal"]))
    target_sequence = selected["event"].get("sequence", float("inf")) if selected else float("inf")
    preceding = [state for state in states if state.get("sequence", state.get("event_sequence", 0)) <= target_sequence]
    failure = bundle / "runtime/failure.json"
    failure_value = None
    if failure.exists():
        try: failure_value = json.loads(failure.read_text())
        except Exception as error: parse_errors.append({"file":"failure.json","error":str(error)})
    return {"diagnostic_parse_errors":parse_errors,"missing_or_empty_streams":missing,"events_written":len(events),"states_written":len(states),"last_sequence":events[-1]["sequence"] if events else None,"has_capture_end_event":bool(events and events[-1].get("kind",{}).get("category") == "capture-end"), "divergence_context": selected, "nearest_preceding_state": preceding[-1] if preceding else None,
            "failure": failure_value}


def analyze(bundle):
    bundle = Path(bundle).resolve(strict=True)
    manifest = json.loads((bundle / "manifest.json").read_text())
    if manifest.get("status") == "running":
        context = correlate(bundle,{})
        return {"run_id":manifest.get("run_id"),"status":"unfinished","diagnostic_only":True,"acceptance":False,
            "inventory_verified":False,"inventory_warning":"Wrapper did not finalize; process liveness is unknown and artifact identities are unverified.",
            "report_present":(bundle / "report.json").exists(),"completion_marker_present":(bundle / "runtime/capture-end.json").exists(),
            "native_prefixes":{label:native_prefix(bundle / "runtime" / name) for label,name in
                (("filtered","runtime.calllog"),("unfiltered","runtime-all.calllog"))},**context}
    verify_artifacts(bundle, manifest)
    report = json.loads((bundle / "report.json").read_text())
    context = correlate(bundle, report.get("comparisons", {}))
    if manifest["status"] == "completed":
        end = json.loads((bundle / "runtime/capture-end.json").read_text())
        if context["diagnostic_parse_errors"] or context["missing_or_empty_streams"] or not context["has_capture_end_event"] or any(end.get(key) != context[key] for key in ("events_written","states_written","last_sequence")) or end.get("run_id") != manifest["run_id"] or end.get("schema_version") != 1:
            raise ValueError("completed bundle causal stream proof invalid")
    return {"run_id": manifest["run_id"], "status": manifest["status"], "result": report, **context}


def snapshot(path, directory, label):
    identity = file_identity(path)
    destination = directory / f"{label}-{Path(path).name}"
    data = Path(identity["path"]).read_bytes()
    if sha(data) != identity["sha256"]: raise ValueError("input changed before snapshot")
    with destination.open("xb") as output: output.write(data)
    return {**identity, "snapshot": destination.name}


def run(args, extra):
    repository = Path(args.repository).resolve()
    run_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ") + "-" + uuid.uuid4().hex[:12]
    root = Path(args.runs_root).resolve()
    if root.is_relative_to((repository / "stock").resolve()) or root.is_relative_to((repository / "reference").resolve()):
        raise ValueError("diagnostic output must not be inside stock/reference")
    root.mkdir(parents=True, exist_ok=True)
    bundle = root / run_id
    bundle.mkdir()
    inputs = bundle / "inputs"
    inputs.mkdir()
    manifest = {"schema_version": SCHEMA, "run_id": run_id, "status": "running", "started": utc(),
                "arguments": sys.argv[1:], "comparison_policy": {**MASK_POLICY,
                    "masked_clock_results": MASK_POLICY["masked_clock_results"] if args.clock_mask == "legacy" else []}, "artifacts": {}}
    manifest["configuration"] = {"frames":args.frames,"trace":args.trace,"trace_level":args.trace_level,
        "event_ring":args.event_ring,"extra":extra,"clock_mask":args.clock_mask,"stock":args.stock}
    dump(bundle / "manifest.json", manifest)
    report, exit_code = {"comparisons": {}}, 2
    try:
        manifest["source"], diff = git_identity(repository)
        (bundle / "source.diff").write_bytes(diff)
        manifest["source_untracked"] = archive_untracked_sources(repository, bundle, manifest["source"])
        stock = tree_identity(repository / args.stock)
        dump(bundle / "stock-inputs.json", stock)
        manifest["stock_inputs_sha256"] = stock["sha256"]
        paths = {"oracle_filtered": Path(args.original), "native_api": Path(args.native_api)}
        if not args.oracle_all:
            basename = "accepted-script-natives-all.slrlog" if paths["oracle_filtered"].name.startswith("accepted-") else "script-natives-all.slrlog"
            args.oracle_all = str(paths["oracle_filtered"].parent / basename)
        paths["oracle_unfiltered"] = Path(args.oracle_all)
        if args.oracle_facts:
            paths["oracle_facts"] = Path(args.oracle_facts)
        else:
            for candidate in (paths["oracle_filtered"].parent / "source-original-facts.json", paths["oracle_filtered"].parent / "observed/original-facts.json"):
                if (candidate if candidate.is_absolute() else repository / candidate).exists():
                    paths["oracle_facts"] = candidate
                    break
        paths.update({f"profile{index}": Path(path) for index, path in enumerate(args.profile)})
        manifest["inputs"] = {label: snapshot(path if path.is_absolute() else repository / path, inputs, label)
                              for label, path in paths.items()}
        for label in ("oracle_filtered", "oracle_unfiltered"):
            records = load_records(inputs / manifest["inputs"][label]["snapshot"])
            if not records or any(record[0] != "native" for record in records):
                raise ValueError("oracle must contain a nonempty complete native-call trace")
        expected_all = load_records(inputs / manifest["inputs"]["oracle_unfiltered"]["snapshot"])
        def selected(record):
            cls = record[1]
            return not (cls.startswith(("java.render.", "java.sound.")) or cls in ("java.io.Input", "java.io.MouseCursor", "java.io.Controller"))
        if [record for record in expected_all if selected(record)] != load_records(inputs / manifest["inputs"]["oracle_filtered"]["snapshot"]):
            raise ValueError("filtered and unfiltered oracle traces have mismatched inputs or capture boundaries")
        if args.replay:
            replay = Path(args.replay).resolve(strict=True)
            manifest["inputs"]["replay"] = snapshot(replay, inputs, "replay")
            # Save the sibling provenance manifests and tapes as an immutable
            # reference bundle. Rust still validates the original sibling set.
            for path in sorted(replay.parent.glob("*.json*")):
                if path != replay and path.is_file():
                    manifest["inputs"]["replay_" + path.name] = snapshot(path, inputs, "replay")
            source = replay.parent / "observed/original-facts.json"
            if source.exists(): manifest["inputs"]["replay_source"] = snapshot(source, inputs, "replay")
            # Keep exact sibling names for the replay provider's manifest-based
            # validation, and execute against these frozen bytes.
            companion = replay.parent / "frame-timer-manifest.json"
            if companion.exists():
                authority = json.loads(companion.read_text())
                for field in ("file", "source_facts"):
                    relative = authority[field]
                    if Path(relative).is_absolute(): raise ValueError("absolute replay companion path cannot be relocated")
                    identity = snapshot(replay.parent / relative, inputs, "companion_" + field)
                    identity["replay_relative"] = relative
                    manifest["inputs"]["replay_companion_" + field] = identity
            replay_directory = bundle / "replay/tree/capture"
            replay_directory.mkdir(parents=True)
            for label, identity in manifest["inputs"].items():
                if label == "replay" or label.startswith("replay_"):
                    relative = identity.get("replay_relative", "observed/original-facts.json" if label == "replay_source" else Path(identity["path"]).name)
                    destination = (replay_directory / relative).resolve()
                    if not destination.is_relative_to(bundle / "replay"): raise ValueError("replay companion escapes frozen bundle")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(inputs / identity["snapshot"], destination)
            manifest["replay_argument"] = str(replay_directory / replay.name)
        if args.replay_from:
            prior_bundle = Path(args.replay_from).resolve(strict=True)
            prior = json.loads((prior_bundle / "manifest.json").read_text())
            verify_artifacts(prior_bundle, prior)
            if prior["stock_inputs_sha256"] != stock["sha256"]:
                raise ValueError("replay stock inputs differ from source run")
            manifest["source_run"] = {"path": str(prior_bundle), "run_id": prior["run_id"],
                "source_changed": prior["source"] != manifest["source"], "source": prior["source"]}
            for label, expected in prior["inputs"].items():
                if label not in manifest["inputs"] or manifest["inputs"][label]["sha256"] != expected["sha256"]:
                    raise ValueError(f"replay input mismatch: {label}")
        if not args.no_build:
            build = child(["cargo", "build", "-p", "slr-boot"], repository, os.environ.copy(), bundle / "build.stdout.txt", bundle / "build.stderr.txt", args.timeout)
            manifest["build"] = build
            if build["timed_out"] or build["returncode"] != 0: raise ValueError("build failed or timed out")
        binary = Path(args.binary)
        if not binary.is_absolute(): binary = repository / binary
        manifest["binary"] = file_identity(binary)
        manifest["binary_snapshot"] = snapshot(binary, inputs, "binary")
        (inputs / manifest["binary_snapshot"]["snapshot"]).chmod(binary.stat().st_mode & 0o777)
        if "source_run" in manifest:
            manifest["source_run"]["binary_changed"] = prior["binary"]["sha256"] != manifest["binary"]["sha256"]
        environment = os.environ.copy()
        if args.replay_from:
            for name in list(environment):
                if name.startswith("SLR_TRACE_"): del environment[name]
            environment.update(prior["trace"]["environment"])
        for item in args.trace:
            name, value = item.split("=", 1)
            if not name.startswith("SLR_TRACE_"): raise ValueError("trace names must begin SLR_TRACE_")
            environment[name] = value
        if args.trace_level == "full": environment["SLR_TRACE_STEPS"] = "1"
        manifest["trace"] = {"level": args.trace_level, "event_ring": args.event_ring,
                             "environment": {key: value for key, value in environment.items() if key.startswith("SLR_TRACE_")}}
        argv = [str(binary.resolve()), str((repository / args.stock).resolve()),
                "--config-instance-id", "0xffff0080", "--game-init-instance-id", "0xffff007f",
                "--output", str(bundle / "runtime"), "--run-id", run_id,
                "--trace-level", args.trace_level, "--event-ring", str(args.event_ring)]
        if args.frames is not None: argv.extend(["--frames", str(args.frames)])
        if args.replay: argv.extend(["--clock-replay", manifest["replay_argument"]])
        if any(arg.split("=", 1)[0] in ("--output", "--run-id", "--trace-level", "--event-ring", "--clock-replay") for arg in extra):
            raise ValueError("runner-owned arguments must use their diagnostic options")
        argv.extend(extra)
        manifest["child_arguments"] = argv
        dump(bundle / "manifest.json", manifest)
        began = time.time_ns()
        result = child(argv, repository, environment, bundle / "runtime.stdout.txt", bundle / "runtime.stderr.txt", args.timeout)
        report["runtime"] = result
        if result["timed_out"]:
            report["runtime_error"] = "runtime timeout; child process group terminated"
        elif result["returncode"] != 0:
            detail = (bundle / "runtime.stderr.txt").read_text(errors="replace")[-4000:]
            report["runtime_error"] = f"runtime exited {result['returncode']}: {detail.strip()}"
        if file_identity(binary)["sha256"] != manifest["binary"]["sha256"]: raise ValueError("binary changed during run")
        if tree_identity(repository / args.stock)["sha256"] != stock["sha256"]: raise ValueError("stock inputs changed during run")
        for label, expected in manifest["inputs"].items():
            if file_identity(expected["path"])["sha256"] != expected["sha256"]:
                raise ValueError(f"input changed during run: {label}")
        types = signatures(inputs / manifest["inputs"]["native_api"]["snapshot"])
        authorities = None
        if "oracle_facts" in manifest["inputs"]:
            authorities = oracle_descriptors(inputs / manifest["inputs"]["oracle_facts"]["snapshot"], expected_all)
            report["descriptor_authority"] = {"validated_records":len(authorities),"sha256":manifest["inputs"]["oracle_facts"]["sha256"]}
        unresolved = []
        if not authorities:
            for record in expected_all:
                candidates = types.get((record[1],record[2],len(record[3])),[])
                if candidates and (any(len({kind[index] in ("L","[") for kind,_ in candidates}) > 1 for index in range(len(record[3]))) or len({ret in ("L","[") for _,ret in candidates}) > 1):
                    unresolved.append(record[1]+"."+record[2])
            report["unresolved_overload_authority"] = sorted(set(unresolved))
        for label, oracle_key, filename in [("filtered", "oracle_filtered", "runtime.calllog"),
                                            ("unfiltered", "oracle_unfiltered", "runtime-all.calllog")]:
            output = bundle / "runtime" / filename
            if not output.exists() or output.stat().st_mtime_ns < began:
                raise ValueError(f"missing or stale current-run log: {filename}")
            report["comparisons"][label] = compare(inputs / manifest["inputs"][oracle_key]["snapshot"], output, types, args.clock_mask == "legacy", [authority for record,authority in zip(expected_all,authorities) if selected(record)] if authorities and label == "filtered" else authorities)
        if result["timed_out"]: raise ValueError("runtime timeout; child process group terminated")
        if result["returncode"] != 0:
            detail = (bundle / "runtime.stderr.txt").read_text(errors="replace")[-4000:]
            raise ValueError(f"runtime exited {result['returncode']}: {detail.strip()}")
        marker = json.loads((bundle / "runtime/capture.json").read_text())
        end = json.loads((bundle / "runtime/capture-end.json").read_text())
        if marker.get("run_id") != run_id or marker.get("schema_version") != 1:
            raise ValueError("runtime capture provenance mismatch")
        if end.get("status") != "completed" or end.get("truncated") is not False or end.get("pending_records") != 0:
            raise ValueError("runtime capture unfinished, failed or truncated")
        for label, field in [("filtered", "filtered_records"), ("unfiltered", "all_records")]:
            count = report["comparisons"][label].get("actual_records", report["comparisons"][label].get("records"))
            if end.get(field) != count: raise ValueError("runtime completion record count mismatch")
        if git_identity(repository)[0] != manifest["source"]: raise ValueError("source changed during run")
        if result["timed_out"]: raise ValueError("runtime timeout; child process group terminated")
        if result["returncode"] != 0: raise ValueError(f"runtime exited{result['returncode']}")
        if not all(value.get("equal") for value in report["comparisons"].values()):
            raise ValueError("complete native traces diverge")
        if unresolved: raise ValueError("native overload descriptor authority unresolved; raw-word comparison is diagnostic only")
        manifest["status"], exit_code = "completed", 0
    except (KeyboardInterrupt, InterruptedRun) as error:
        manifest["status"] = "interrupted"
        report["error"] = str(error) or "interrupted"
        exit_code = 130
    except Exception as error:
        manifest["status"] = "failed"
        report["error"] = report.get("runtime_error", f"{type(error).__name__}: {error}")
        if "runtime_error" in report: report["capture_boundary"] = f"{type(error).__name__}: {error}"
    finally:
        try:
            report.update(correlate(bundle, report.get("comparisons", {})))
            if manifest["status"] == "completed" and (report["diagnostic_parse_errors"] or report["missing_or_empty_streams"] or not report["has_capture_end_event"] or
                    any(end.get(key) != report[key] for key in ("events_written","states_written","last_sequence")) or end.get("run_id") != run_id or end.get("schema_version") != 1):
                manifest["status"], exit_code = "failed", 2
                report["error"] = "incomplete or malformed diagnostic timeline"
        except Exception as error:
            manifest["status"], exit_code = "failed", 2
            report["diagnostic_error"] = f"{type(error).__name__}: {error}"
        if not args.no_divergence_rerun and manifest["status"] == "failed" and report.get("error") == "ValueError: complete native traces diverge":
            try:
                event = (report.get("divergence_context") or {}).get("event", {})
                call_id = (event.get("context", {}).get("native") or {}).get("call_id")
                if call_id is not None:
                    rerun = bundle / "divergence"
                    rerun.mkdir()
                    rerun_argv = list(manifest["child_arguments"])
                    rerun_argv[rerun_argv.index("--output") + 1] = str(rerun / "runtime")
                    rerun_argv[rerun_argv.index("--run-id") + 1] = run_id + "-divergence"
                    rerun_argv.extend(["--snapshot-native", str(call_id)])
                    outcome = child(rerun_argv, repository, environment, rerun / "stdout.txt", rerun / "stderr.txt", args.timeout)
                    stable = not outcome["timed_out"] and outcome["returncode"] == report["runtime"]["returncode"]
                    for name in ("runtime.calllog", "runtime-all.calllog"):
                        stable = stable and load_records(rerun / "runtime" / name) == load_records(bundle / "runtime" / name)
                    stable = stable and file_identity(binary)["sha256"] == manifest["binary"]["sha256"]
                    stable = stable and git_identity(repository)[0] == manifest["source"]
                    stable = stable and tree_identity(repository / args.stock)["sha256"] == manifest["stock_inputs_sha256"]
                    for identity in manifest["inputs"].values():
                        stable = stable and file_identity(inputs / identity["snapshot"])["sha256"] == identity["sha256"]
                    if args.replay:
                        for label, identity in manifest["inputs"].items():
                            if label == "replay" or label.startswith("replay_"):
                                relative = identity.get("replay_relative", "observed/original-facts.json" if label == "replay_source" else Path(identity["path"]).name)
                                stable = stable and file_identity(Path(manifest["replay_argument"]).parent / relative)["sha256"] == identity["sha256"]
                    snapshot_path = rerun / "runtime/divergence.json"
                    snapshot_identity = json.loads(snapshot_path.read_text()) if snapshot_path.exists() else {}
                    identity_valid = snapshot_identity.get("run_id") == run_id + "-divergence" and snapshot_identity.get("native_call_id") == call_id and snapshot_identity.get("schema_version") == 1
                    report["targeted_rerun"] = {"arguments":rerun_argv,"runtime":outcome,"native_call_id":call_id,
                        "identical_full_traces":stable,"snapshot_valid":stable and identity_valid,
                        "snapshot":str(snapshot_path.relative_to(bundle)) if snapshot_path.exists() else None}
            except Exception as error:
                report["targeted_rerun_error"] = f"{type(error).__name__}: {error}"
        mapped_comparison = (report.get("divergence_context") or {}).get("comparison")
        divergence = (mapped_comparison, report["comparisons"][mapped_comparison]) if mapped_comparison else next(((label, value) for label, value in report.get("comparisons", {}).items() if value.get("equal") is False), None)
        regression = {"schema_version":1,"run_id":run_id,"input_hashes":{label:item["sha256"] for label,item in manifest.get("inputs",{}).items()},
            "comparison_policy":manifest["comparison_policy"],"source_bundle":str(bundle),
            "reproduction_command":[sys.executable,str(Path(__file__).resolve()),"--replay-from",str(bundle)],
            "verification_command":[sys.executable,str(Path(__file__).resolve()),"--check-regression",str(bundle)],
            "frontier":{ "comparison":divergence[0], **divergence[1]} if divergence else None,
            "native_call_id":((report.get("divergence_context") or {}).get("event",{}).get("context",{}).get("native") or {}).get("call_id"),
            "assertion":"All records through the recorded mismatch must now equal the frozen oracle under the recorded comparison policy."}
        dump(bundle / "regression.json", regression)
        if args.check_regression:
            expected_regression = json.loads((Path(args.check_regression) / "regression.json").read_text())
            frontier = expected_regression.get("frontier")
            current = report.get("comparisons",{}).get(frontier["comparison"],{}) if frontier else {}
            fixed = bool(frontier and (current.get("equal") is True or current.get("record",0) > frontier["record"]))
            report["regression_check"] = {"source_run_id":expected_regression["run_id"],"passed":fixed,
                "full_parity":manifest["status"] == "completed","prior_frontier":frontier}
        if manifest["status"] != "completed":
            report["native_prefixes"] = {label:native_prefix(bundle / "runtime" / name) for label,name in
                (("filtered","runtime.calllog"),("unfiltered","runtime-all.calllog"))}
        report["status"] = manifest["status"]
        dump(bundle / "report.json", report)
        manifest["finished"] = utc()
        manifest["artifacts"] = {path.relative_to(bundle).as_posix(): file_identity(path)
            for path in sorted(bundle.rglob("*")) if path.is_file() and path != bundle / "manifest.json"}
        dump(bundle / "manifest.json", manifest)
    print(json.dumps({"bundle": str(bundle), "status": manifest["status"], "error": report.get("error"), "comparisons": report["comparisons"]}, default=str))
    return exit_code


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ["analyze"]:
        parser = argparse.ArgumentParser()
        parser.add_argument("bundle")
        parser.add_argument("--report")
        args = parser.parse_args(argv[1:])
        result = analyze(args.bundle)
        if args.report:
            path = Path(args.report)
            if path.resolve().is_relative_to(Path(args.bundle).resolve()): raise ValueError("analysis report must be outside the immutable source bundle")
            if path.exists(): raise ValueError("analysis report already exists")
            dump(path, result)
        else: print(json.dumps(result, indent=2, default=str))
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original", nargs="?", default=os.environ.get("SLR_ORIGINAL_LOG", "re/out/headless/boot/observed/script-natives.slrlog"))
    parser.add_argument("--oracle-all")
    parser.add_argument("--oracle-facts")
    parser.add_argument("--repository", default=str(REPO))
    parser.add_argument("--stock", default="stock")
    parser.add_argument("--native-api", default="re/out/native_api.tsv")
    parser.add_argument("--runs-root", default=str(REPO / "re/out/boot/runs"))
    parser.add_argument("--binary", default="target/debug/slr-boot")
    parser.add_argument("--timeout", type=float, default=float(os.environ.get("SLR_BOOT_SECONDS", "120")))
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--frames", type=int)
    parser.add_argument("--replay")
    parser.add_argument("--replay-from")
    parser.add_argument("--check-regression", metavar="PRIOR_BUNDLE")
    parser.add_argument("--clock-mask", choices=["strict", "legacy"])
    parser.add_argument("--no-divergence-rerun", action="store_true")
    parser.add_argument("--profile", action="append", default=[])
    parser.add_argument("--trace", action="append", default=[])
    parser.add_argument("--trace-level", choices=["metadata", "full"], default="metadata")
    parser.add_argument("--event-ring", type=int, default=4096)
    args, extra = parser.parse_known_args(argv)
    if extra[:1] == ["--"]: extra = extra[1:]
    if args.check_regression:
        if args.replay_from: parser.error("choose replay-from or check-regression")
        args.replay_from = args.check_regression
    if args.replay_from:
        prior_bundle = Path(args.replay_from).resolve(strict=True)
        prior = json.loads((prior_bundle / "manifest.json").read_text())
        verify_artifacts(prior_bundle, prior)
        configuration = prior["configuration"]
        for name in ("frames", "trace", "trace_level", "event_ring", "clock_mask", "stock"):
            setattr(args, name, configuration[name])
        extra = configuration["extra"]
        def frozen(label): return str(prior_bundle / "inputs" / prior["inputs"][label]["snapshot"])
        args.original, args.oracle_all, args.native_api = frozen("oracle_filtered"), frozen("oracle_unfiltered"), frozen("native_api")
        args.oracle_facts = frozen("oracle_facts") if "oracle_facts" in prior["inputs"] else None
        args.profile = [frozen(label) for label in prior["inputs"] if label.startswith("profile")]
        if "replay" in prior["inputs"]:
            args.replay = prior["replay_argument"]
    if args.clock_mask is None: args.clock_mask = "strict" if args.replay else "legacy"
    if args.timeout <= 0 or args.event_ring <= 0 or args.frames is not None and args.frames < 0:
        parser.error("timeout/event-ring must be positive and frames nonnegative")
    previous = {}
    def interrupt(signum, frame): raise InterruptedRun(f"signal{signum}")
    for signum in (signal.SIGINT, signal.SIGTERM): previous[signum] = signal.signal(signum, interrupt)
    try: return run(args, extra)
    finally:
        for signum, handler in previous.items(): signal.signal(signum, handler)


if __name__ == "__main__":
    try: raise SystemExit(main())
    except Exception as error:
        print(f"boot-diagnose: {error}", file=sys.stderr)
        raise SystemExit(2)
