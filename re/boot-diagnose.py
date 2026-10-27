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
sys.path.insert(0, str(Path(__file__).resolve().parent))
import calllog
import diag_archive as archive
import diag_fixtures as fixtures

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


def git_identity(repository, outputs=()):
    """Source identity: the commit, the tracked diff and every untracked source
    file. `outputs` are paths this tool owns; bytes under them are run products,
    never source, so a run cannot invalidate its own identity by writing them.
    """
    excluded = tuple(Path(path).resolve() for path in outputs)
    def git(*args):
        return subprocess.check_output(["git", "-C", str(repository), *args])
    commit = git("rev-parse", "HEAD").decode().strip()
    # Include untracked source bytes, not only Git's tracked-file diff.
    tracked = git("diff", "HEAD", "--binary")
    untracked = []
    for raw in git("ls-files", "--others", "--exclude-standard", "-z").split(b"\0"):
        if raw:
            path = repository / os.fsdecode(raw)
            resolved = path.resolve()
            if any(resolved == root or resolved.is_relative_to(root) for root in excluded):
                continue
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


def native_prefix(path, scan_bytes=None):
    """Diagnostic only: retain decoded complete records before a crash tail."""
    if not path.exists(): return {"missing":True,"valid_records":0}
    total = path.stat().st_size
    budget = total if scan_bytes is None else min(total, scan_bytes)
    records, count, offset, scanned, error, truncated = [], 0, 8, 0, None, False
    try:
        with path.open("rb") as source:
            header = source.read(min(8, budget))
            scanned = len(header)
            if budget < 8 and total >= 8:
                truncated = True
            else:
                calllog.Reader(header)
                while offset < total:
                    if budget - scanned < 5:
                        truncated = budget < total
                        if not truncated: raise ValueError("truncated record header")
                        break
                    frame = source.read(5)
                    scanned += len(frame)
                    if len(frame) != 5: raise ValueError("truncated record header")
                    _, size = struct.unpack("<BI", frame)
                    if size > calllog.MAX_BODY or offset + 5 + size > total:
                        raise ValueError("truncated or oversized record")
                    if size > budget - scanned:
                        truncated = True
                        break
                    body = source.read(size)
                    scanned += len(body)
                    if len(body) != size: raise ValueError("truncated or oversized record")
                    decoded = calllog.Reader(header + frame + body).records()
                    count += len(decoded)
                    records = (records + decoded)[-12:]
                    offset += 5 + size
    except Exception as exception: error = str(exception)
    return {"valid_records":count,"valid_bytes":offset,"total_bytes":total,
        "tail_error":error,"tail_error_offset":offset if error else None,"recent_records":records,
        "scanned_bytes":scanned,"truncated":truncated,"acceptance_evidence":False}


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


def signal_process_group(pgid, signum):
    try:
        os.killpg(pgid, signum)
    except ProcessLookupError:
        pass
    except PermissionError:
        if sys.platform != "darwin":
            raise
        # XNU killpg1 filters SZOMB members and returns EPERM if none remain
        # signalable. Confirm the group is gone or zombie-only; never hide a
        # genuine denial for a live descendant. Inspection failures fail closed.
        # https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c
        listing = subprocess.run(["ps", "-axo", "pgid=,stat="], check=True,
                                 capture_output=True, text=True, timeout=3).stdout
        for line in listing.splitlines():
            group, state = line.split()
            if int(group) == pgid and not state.startswith("Z"):
                raise


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
            signal_process_group(process.pid, signal.SIGTERM)
            try: process.wait(timeout=3)
            except subprocess.TimeoutExpired: pass
            # The process leader may have exited while grandchildren remain.
            signal_process_group(process.pid, signal.SIGKILL)
            process.wait()


def verify_artifacts(bundle, manifest):
    if manifest.get("status") not in ("completed", "failed", "interrupted"):
        raise ValueError("bundle is running or unfinished; no completed-run acceptance")
    artifacts = manifest.get("artifacts", {})
    if isinstance(artifacts, list):
        artifacts = {item["path"]: item for item in artifacts}
    # The boot runner writes a comparison report; an embedding without native
    # parity (the frontend) has nothing to report and must not be rejected for
    # the absence of a file it could never produce.
    if not artifacts or (manifest.get("embedding", "slr-boot") == "slr-boot" and "report.json" not in artifacts):
        raise ValueError("artifact manifest incomplete")
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


def stream_dir(bundle):
    """Where a bundle keeps its streams.

    The headless runner nests them under `runtime/`; the shared telemetry crate
    writes them at the bundle root. One reader must handle both.
    """
    nested = bundle / "runtime"
    return nested if nested.is_dir() else bundle


def correlate(bundle, comparisons):
    events, states, parse_errors, missing = [], [], [], []
    for filename, output in [("events.jsonl", events), ("states.jsonl", states)]:
        path = stream_dir(bundle) / filename
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
    manifest = bundle_manifest(bundle)
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
    failure = stream_dir(bundle) / "failure.json"
    failure_value = None
    if failure.exists():
        try: failure_value = json.loads(failure.read_text())
        except Exception as error: parse_errors.append({"file":"failure.json","error":str(error)})
    return {"diagnostic_parse_errors":parse_errors,"missing_or_empty_streams":missing,"events_written":len(events),"states_written":len(states),"last_sequence":events[-1]["sequence"] if events else None,"has_capture_end_event":bool(events and events[-1].get("kind",{}).get("category") == "capture-end"), "divergence_context": selected, "nearest_preceding_state": preceding[-1] if preceding else None,
            "failure": failure_value}


def analyze(bundle):
    bundle = Path(bundle).resolve(strict=True)
    manifest = bundle_manifest(bundle)
    if manifest.get("status") == "running" or manifest.get("pinned"):
        context = correlate(bundle,{})
        return {"run_id":manifest.get("run_id"),"status":"unfinished","diagnostic_only":True,"acceptance":False,
            "inventory_verified":False,"inventory_warning":"Wrapper did not finalize; process liveness is unknown and artifact identities are unverified.",
            "report_present":(bundle / "report.json").exists(),"completion_marker_present":(stream_dir(bundle) / "capture-end.json").exists(),
            "native_prefixes":{label:native_prefix(stream_dir(bundle) / name) for label,name in
                (("filtered","runtime.calllog"),("unfiltered","runtime-all.calllog"))},**context}
    # A reconstructed manifest carries sizes but no hashes: there was no
    # writer to take them, so verification is reported as unavailable.
    if not manifest.get("reconstructed"):
        verify_artifacts(bundle, manifest)
    report_path = bundle / "report.json"
    if report_path.exists():
        report = json.loads(report_path.read_text())
    else:
        report = {"status": manifest.get("status"), "error": manifest.get("error"),
                  "comparisons": manifest.get("comparisons", {}),
                  "embedding": manifest.get("embedding"), "final": manifest.get("final")}
    context = correlate(bundle, report.get("comparisons") or {})
    if manifest["status"] == "completed":
        end = json.loads((stream_dir(bundle) / "capture-end.json").read_text())
        if context["diagnostic_parse_errors"] or context["missing_or_empty_streams"] or not context["has_capture_end_event"] or any(end.get(key) != context[key] for key in ("events_written","states_written","last_sequence")) or end.get("run_id") != manifest["run_id"] or end.get("schema_version") != 1:
            raise ValueError("completed bundle causal stream proof invalid")
    return {"run_id": manifest["run_id"], "status": manifest["status"], "result": report, **context}


def archive_root(runs_root):
    """Index and the content store live beside `runs/`, never inside it.

    Tests and `analyze` treat every `runs/` child as a bundle, so nothing else
    may appear there.
    """
    return archive.archive_root(runs_root)


def store_path(runs_root, digest):
    return archive_root(runs_root) / "inputs-store" / digest


def snapshot(path, directory, label, runs_root=None):
    """Freeze an input into the bundle, sharing bytes through the content store.

    The bundle keeps a real file (hard link where the filesystem allows) so
    `verify_artifacts` and `analyze` are unchanged; identical inputs across
    bundles cost no additional disk.
    """
    identity = file_identity(path)
    destination = directory / f"{label}-{Path(path).name}"
    data = Path(identity["path"]).read_bytes()
    if sha(data) != identity["sha256"]: raise ValueError("input changed before snapshot")
    if runs_root is None:
        runs_root = directory.parent.parent
    stored = store_path(runs_root, identity["sha256"])
    if not stored.exists():
        stored.parent.mkdir(parents=True, exist_ok=True)
        temporary = stored.with_name(stored.name + ".tmp")
        with temporary.open("xb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, stored)
    elif file_identity(stored)["sha256"] != identity["sha256"]:
        raise ValueError("content store entry has unexpected bytes")
    # The bundle copy must stay independently mutable: verification hashes
    # each bundle's own bytes, so a hard link would let one bundle invalidate
    # another. Reflink (APFS, btrfs, XFS) keeps that independence at no extra
    # space; every other filesystem gets a plain copy. On macOS `clonefile`
    # is the reflink primitive; the store is on the same volume by construction.
    cloned = False
    if sys.platform == "darwin":
        import ctypes
        libc = ctypes.CDLL(None, use_errno=True)
        cloned = libc.clonefile(os.fsencode(stored), os.fsencode(destination), 0) == 0
    if not cloned:
        shutil.copyfile(stored, destination)
    if file_identity(destination)["sha256"] != identity["sha256"]:
        raise ValueError("snapshotted input differs from its identity")
    return {**identity, "snapshot": destination.name}


def index_path(runs_root):
    return archive.index_path(runs_root)


def resolve_record(record, runs_root):
    """A record's bundle directory (relative, legacy-absolute or moved), or None."""
    return archive.resolve_bundle(record, runs_root, REPO)


def append_index(runs_root, record):
    """Append one line per bundle. Best effort: a full disk must not fail a run."""
    path = index_path(runs_root)
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(record, sort_keys=True, default=str).encode() + b"\n"
    with path.open("ab") as output:
        if output.tell() > 0:
            with path.open("rb") as existing:
                existing.seek(-1, os.SEEK_END)
                if existing.read(1) not in (b"\n",):
                    output.write(b"\n")
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())


def index_record(bundle, manifest, report, runs_root=None):
    """The searchable summary of one bundle.

    With `runs_root`, `bundle` is stored relative to it so the record survives
    the checkout or worktree that produced it; without, it stays absolute.
    """
    comparisons = report.get("comparisons") or {}
    first = next(({"comparison": label, **value} for label, value in comparisons.items()
                  if value.get("equal") is False), None)
    origin = archive.record_provenance(manifest, bundle)
    signature, values_signature = archive.mismatch_signature({**report, "comparisons": comparisons})
    # A frontend bundle has no native comparison: it is neither passing nor
    # failing parity, and must not be read as either.
    artifacts = manifest.get("artifacts", {})
    # The headless runner keys artifacts by relative path; the shared crate
    # writes a list. Accept either so one index covers both embeddings.
    entries = artifacts.values() if isinstance(artifacts, dict) else artifacts
    sizes = [item.get("bytes", 0) for item in entries]
    return {
        "schema_version": 1,
        "run_id": manifest.get("run_id"),
        "bundle": archive.relative_bundle(bundle, runs_root) if runs_root is not None else str(bundle),
        "kind": manifest.get("kind", "run"),
        "embedding": manifest.get("embedding", "slr-boot"),
        "started": manifest.get("started"),
        "finished": manifest.get("finished"),
        "status": manifest.get("status"),
        "error": report.get("error"),
        "source_commit": origin["source_commit"],
        "source_commit_reason": origin["source_commit_reason"],
        "source_commit_basis": origin["source_commit_basis"],
        "branch": origin["branch"],
        "worktree": origin["worktree"],
        "session": origin["session"],
        "source_dirty": origin["source_dirty"],
        "source_diff_sha256": origin["source_diff_sha256"],
        "source_run": (manifest.get("source_run") or {}).get("run_id"),
        "binary_sha256": (manifest.get("binary") or {}).get("sha256"),
        "case_sha256": archive.case_signature(manifest),
        "configuration": {key: (manifest.get("configuration") or {}).get(key) for key in
                          ("frames", "trace_level", "event_ring", "clock_mask", "stock")},
        "comparisons": {label: {"equal": value.get("equal"), "record": value.get("record"),
                                "kind": value.get("kind"), "records": value.get("records"),
                                "expected_records": value.get("expected_records"),
                                "actual_records": value.get("actual_records")}
                        for label, value in comparisons.items()},
        **archive.progress(comparisons),
        "first_mismatch": first,
        "first_mismatch_signature": signature,
        "first_mismatch_values_signature": values_signature,
        "native_call_id": (report.get("divergence_context") or {}).get("event", {}).get("context", {}).get("native", {}).get("call_id"),
        "targeted_rerun": {key: (report.get("targeted_rerun") or {}).get(key)
                           for key in ("identical_full_traces", "snapshot_valid")},
        "flake": manifest.get("flake"),
        # Present only when the wrapper manifest was lost and the crash
        # markers supplied the identity instead.
        "reconstructed": manifest.get("reconstructed"),
        # True when `started`/`finished` came from the directory mtime rather
        # than a writer that recorded them.
        "started_derived": manifest.get("started_derived"),
        "artifacts": len(artifacts) if not isinstance(artifacts, dict) else len(artifacts),
        "bytes": sum(sizes),
    }


def prune(runs_root, keep, dry_run=False, kind="run"):
    """Delete the oldest bundles of one `kind`, newest `keep` retained.

    Only `kind` bundles are candidates (default: runs); hook records share the
    archive but never displace parity runs from retention, and are pruned with
    `--kind hook`.

    A `running` bundle is never removed: its writer may still be appending.
    The index is left intact, so pruning is visible in the history.
    """
    root = Path(runs_root).resolve()
    if keep < 1:
        raise ValueError("--keep must be at least 1")
    bundles = []
    for child in root.iterdir():
        if not child.is_dir():
            continue
        try:
            manifest = json.loads((child / "manifest.json").read_text())
        except Exception:
            continue
        if manifest.get("kind", "run") != kind:
            continue
        bundles.append((manifest.get("started") or "", child, manifest))
    bundles.sort(key=lambda item: item[0])
    removed, retained = [], []
    for started, child, manifest in bundles[:-keep] if len(bundles) > keep else []:
        if manifest.get("status") == "running":
            retained.append(str(child.resolve()))
            continue
        relative = child.name
        if not dry_run:
            shutil.rmtree(child)
        removed.append({"bundle": str(child.resolve()), "name": relative,
                        "started": started, "status": manifest.get("status")})
    if not dry_run:
        # Drop store entries no live bundle links to.
        store = archive_root(runs_root) / "inputs-store"
        if store.is_dir():
            live = set()
            for child in root.iterdir():
                for path in child.rglob("*"):
                    if path.is_file():
                        try: live.add(str(path.stat().st_ino))
                        except OSError: pass
            for entry in store.iterdir():
                if entry.is_file():
                    try:
                        if str(entry.stat().st_ino) not in live: entry.unlink()
                    except OSError: pass
    return {"removed": removed, "retained_running": retained, "kept": keep}


def environment_identity(repository):
    """Host and toolchain identity a bundle was produced on."""
    import platform
    def tool(*command):
        try:
            return subprocess.run(command, cwd=repository, capture_output=True, text=True,
                                  timeout=30).stdout.strip() or None
        except (OSError, subprocess.TimeoutExpired):
            return None
    return {
        "schema_version": 1,
        "captured": utc(),
        "uname": {"system": platform.system(), "release": platform.release(),
                  "machine": platform.machine(), "version": platform.version()},
        "python": platform.python_version(),
        "cpus": os.cpu_count(),
        "rustc": tool("rustc", "--version"),
        "cargo": tool("cargo", "--version"),
        "wine": tool("wine", "--version"),
    }


def flake_summary(statuses, signatures=None, values_signatures=None):
    """Failure rate and first-mismatch agreement over repeated executions of one case.

    `signatures` are the runs' first-mismatch signatures (None for a run with no
    mismatch). Identical failures are deterministic; the case is nondeterministic
    when outcomes are mixed or the runs fail at different places or with
    different values.
    """
    total = len(statuses)
    failed = sum(1 for status in statuses if status != "completed")
    summary = {"iterations": total, "failed": failed, "completed": total - failed,
               "rate": (failed / total) if total else 0.0, "statuses": list(statuses),
               "nondeterministic": 0 < failed < total}
    if signatures is not None:
        distinct = {signature for signature in signatures}
        values = {signature for signature in (values_signatures or signatures)}
        summary.update({"first_mismatch_signatures": list(signatures),
                        "distinct_signatures": len(distinct),
                        "signature_stable": len(distinct) <= 1,
                        "values_stable": len(values) <= 1})
        summary["nondeterministic"] = summary["nondeterministic"] or len(distinct) > 1 or len(values) > 1
    return summary


def file_issue(repository, record, bundle):
    """Create a GitHub issue for a failed bundle; returns the command to run."""
    title = f"boot diagnostics: {record.get('error') or 'divergence'} (run {record['run_id']})"
    mismatch = record.get("first_mismatch") or {}
    body = "\n".join([
        f"Automated diagnostic failure from `{bundle}`.",
        "",
        f"- status: `{record.get('status')}`",
        f"- error: `{record.get('error')}`",
        f"- commit: `{record.get('source_commit')}` (dirty: {record.get('source_dirty')})",
        f"- binary: `{record.get('binary_sha256')}`",
        f"- comparison: `{mismatch.get('comparison')}` record {mismatch.get('record')} ({mismatch.get('kind')})",
        f"- native call id: `{record.get('native_call_id')}`",
        "",
        "Reproduce:",
        "",
        "```sh",
        f"python3 re/boot-diagnose.py --replay-from {bundle}",
        "```",
        "",
        "Verify the recorded frontier:",
        "",
        "```sh",
        f"python3 re/boot-diagnose.py --check-regression {bundle}",
        "```",
    ])
    return ["gh", "issue", "create", "--title", title, "--body", body,
            "--label", "needs-triage"]


def run_once(args, extra):
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
        "event_ring":args.event_ring,"extra":extra,"clock_mask":args.clock_mask,"stock":args.stock,
        "repeat":args.repeat,"wall_clock":args.wall_clock}
    manifest["environment"] = environment_identity(repository)
    # Provenance is recorded before anything can fail, so even a run that dies
    # during setup names its commit (or says why it cannot).
    manifest["provenance"] = archive.provenance(repository)
    dump(bundle / "manifest.json", manifest)
    report, exit_code = {"comparisons": {}}, 2
    try:
        # Only this tool's own products are excluded from source identity.
        outputs = [Path(args.runs_root), index_path(args.runs_root),
                   archive_root(args.runs_root) / "inputs-store"]
        manifest["source"], diff = git_identity(repository, outputs)
        manifest["provenance"] = archive.provenance(repository, manifest["source"])
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
        manifest["inputs"] = {label: snapshot(path if path.is_absolute() else repository / path, inputs, label, args.runs_root)
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
            manifest["inputs"]["replay"] = snapshot(replay, inputs, "replay", args.runs_root)
            # Save the sibling provenance manifests and tapes as an immutable
            # reference bundle. Rust still validates the original sibling set.
            for path in sorted(replay.parent.glob("*.json*")):
                if path != replay and path.is_file():
                    manifest["inputs"]["replay_" + path.name] = snapshot(path, inputs, "replay", args.runs_root)
            source = replay.parent / "observed/original-facts.json"
            if source.exists(): manifest["inputs"]["replay_source"] = snapshot(source, inputs, "replay", args.runs_root)
            # Keep exact sibling names for the replay provider's manifest-based
            # validation, and execute against these frozen bytes.
            companion = replay.parent / "frame-timer-manifest.json"
            if companion.exists():
                authority = json.loads(companion.read_text())
                for field in ("file", "source_facts"):
                    relative = authority[field]
                    if Path(relative).is_absolute(): raise ValueError("absolute replay companion path cannot be relocated")
                    identity = snapshot(replay.parent / relative, inputs, "companion_" + field, args.runs_root)
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
            # A shared target directory can satisfy cargo while another build
            # is mid-flight, linking a stale dependency rlib. Acceptance builds
            # can request a private target directory instead.
            environment = os.environ.copy()
            if args.isolated_build:
                environment["CARGO_TARGET_DIR"] = str(bundle / "target")
            build = child(["cargo", "build", "-p", "slr-boot"], repository, environment, bundle / "build.stdout.txt", bundle / "build.stderr.txt", args.timeout)
            manifest["build"] = build
            if build["timed_out"] or build["returncode"] != 0: raise ValueError("build failed or timed out")
        binary = Path(args.binary)
        if args.isolated_build and not binary.is_absolute():
            binary = bundle / "target/debug" / binary.name
        elif not binary.is_absolute():
            binary = repository / binary
        manifest["binary"] = file_identity(binary)
        manifest["binary_snapshot"] = snapshot(binary, inputs, "binary", args.runs_root)
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
        if args.wall_clock: argv.append("--wall-clock")
        if args.replay: argv.extend(["--clock-replay", manifest["replay_argument"]])
        if any(arg.split("=", 1)[0] in ("--output", "--run-id", "--trace-level", "--event-ring", "--clock-replay", "--wall-clock") for arg in extra):
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
        if git_identity(repository, outputs)[0] != manifest["source"]: raise ValueError("source changed during run")
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
                    stable = stable and git_identity(repository, outputs)[0] == manifest["source"]
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
            prior_prefix = frontier["record"] - 1 if frontier and isinstance(frontier.get("record"), int) else None
            current_prefix, _ = archive.comparison_progress(current) if frontier else (None, None)
            # A shorter matched prefix than the frozen frontier is a regression
            # even though the comparison as a whole fails either way.
            report["regression_check"] = {"source_run_id":expected_regression["run_id"],"passed":fixed,
                "full_parity":manifest["status"] == "completed","prior_frontier":frontier,
                "prior_matched_prefix":prior_prefix,"matched_prefix":current_prefix,
                "regressed":bool(prior_prefix is not None and current_prefix is not None and current_prefix < prior_prefix)}
        if manifest["status"] != "completed":
            report["native_prefixes"] = {label:native_prefix(bundle / "runtime" / name) for label,name in
                (("filtered","runtime.calllog"),("unfiltered","runtime-all.calllog"))}
        report["status"] = manifest["status"]
        dump(bundle / "report.json", report)
        manifest["finished"] = utc()
        manifest["artifacts"] = {path.relative_to(bundle).as_posix(): file_identity(path)
            for path in sorted(bundle.rglob("*")) if path.is_file() and path != bundle / "manifest.json"}
        dump(bundle / "manifest.json", manifest)
    return exit_code, bundle, manifest, report


def run(args, extra):
    """Execute the requested iterations, then index, prune and report once."""
    repository = Path(args.repository).resolve()
    iterations = []
    exit_code = 0
    for iteration in range(args.repeat):
        status, bundle, manifest, report = run_once(args, extra)
        iterations.append((status, bundle, manifest, report))
        exit_code = exit_code or status
    flake = None
    if args.repeat > 1:
        recorded = [index_record(bundle, manifest, report) for _, bundle, manifest, report in iterations]
        flake = flake_summary([manifest.get("status") for _, _, manifest, _ in iterations],
                              [record["first_mismatch_signature"] for record in recorded],
                              [record["first_mismatch_values_signature"] for record in recorded])
        if args.flake_check and flake["nondeterministic"]:
            exit_code = 3
    last = iterations[-1]
    for index, (_, bundle, manifest, report) in enumerate(iterations):
        manifest["flake"] = {**flake, "iteration": index + 1} if flake else manifest.get("flake")
        report["flake"] = manifest["flake"]
        if flake:
            dump(bundle / "report.json", report)
            dump(bundle / "manifest.json", manifest)
        record = index_record(bundle, manifest, report, args.runs_root)
        if flake:
            record["flake"] = manifest["flake"]
        append_index(args.runs_root, record)
        print(json.dumps({"bundle": str(bundle), "status": manifest["status"],
            "error": report.get("error"), "comparisons": report["comparisons"],
            "flake": record["flake"]}, default=str))
    _, bundle, manifest, report = last
    if args.prune_keep:
        report["pruned"] = prune(args.runs_root, args.prune_keep)
        dump(bundle / "report.json", report)
    if exit_code != 0 and args.file_issue:
        record = index_record(bundle, manifest, report, args.runs_root)
        command = file_issue(repository, record, bundle)
        if args.file_issue == "dry-run":
            report["issue_command"] = command
        else:
            try:
                completed = subprocess.run(command, cwd=repository, capture_output=True, text=True)
                report["issue"] = {"returncode": completed.returncode,
                                   "stdout": completed.stdout.strip(),
                                   "stderr": completed.stderr.strip()}
            except OSError as error:
                report["issue"] = {"error": str(error)}
        dump(bundle / "report.json", report)
    print(json.dumps({"bundle": str(bundle), "status": manifest["status"],
        "exit_code": exit_code, "index": str(index_path(args.runs_root)),
        "flake": report.get("flake"), "pruned": report.get("pruned"),
        "issue": report.get("issue_command") or report.get("issue")}, default=str))
    return exit_code


def directory_utc(bundle):
    """The bundle directory's mtime as a UTC timestamp.

    A bundle whose writer was killed has no recorded start. The directory's
    mtime is when its last artifact landed, which is the only clock it has;
    callers mark it as derived so it is never mistaken for a recorded time.
    """
    seconds = Path(bundle).stat().st_mtime
    return dt.datetime.fromtimestamp(seconds, dt.timezone.utc).isoformat()


def bundle_manifest(bundle):
    """The bundle's manifest, reconstructed when the wrapper never wrote one.

    A killed wrapper leaves complete crash-surviving markers and no
    `manifest.json`. Those bundles are the ones most worth reading, so their
    identity comes from the markers instead of failing.
    """
    manifest_path = bundle / "manifest.json"
    if manifest_path.exists():
        return json.loads(manifest_path.read_text())
    streams = stream_dir(bundle)
    capture = json.loads((streams / "capture.json").read_text())
    end = json.loads((streams / "capture-end.json").read_text())
    failure = streams / "failure.json"
    return {
        "schema_version": 1,
        "run_id": capture.get("run_id") or end.get("run_id"),
        "status": end.get("status", "failed"),
        # The markers carry no clock, so the directory mtime stands in and is
        # labelled: `--since` and `status` must see these bundles, but nothing
        # may present a derived time as recorded.
        "started": end.get("started") or directory_utc(bundle),
        "started_derived": not end.get("started"),
        "finished": end.get("finished") or end.get("started") or directory_utc(bundle),
        "error": end.get("error")
        or (json.loads(failure.read_text()).get("failure") if failure.exists() else None),
        "artifacts": {path.relative_to(bundle).as_posix(): {
            "sha256": None, "bytes": path.stat().st_size}
            for path in bundle.rglob("*") if path.is_file()},
        "reconstructed": "wrapper manifest missing; identity from capture markers",
    }


def index_bundle(bundle, runs_root=None):
    """Build an index record from a finished bundle already on disk."""
    bundle = Path(bundle).resolve()
    manifest = bundle_manifest(bundle)
    report_path = bundle / "report.json"
    report = json.loads(report_path.read_text()) if report_path.exists() else {}
    # Older and frontend bundles store their own completion facts.
    end_path = stream_dir(bundle) / "capture-end.json"
    if "error" not in report and end_path.exists():
        report["error"] = json.loads(end_path.read_text()).get("error")
    if "status" not in report:
        report["status"] = manifest.get("status")
    return index_record(bundle, manifest, report, runs_root)


def index_missing(runs_root, rebuild=False, dry_run=False, source=None):
    """Index every bundle that has no record.

    `rebuild` replaces the whole index through `archive.rebuild_index`: the old
    file is backed up, the new one is written and verified beside it, and only
    then swapped in. Bundles are only ever read. `dry_run` stops before the swap.
    """
    root = Path(runs_root).resolve()
    if rebuild:
        result = archive.rebuild_index(root, index_bundle, REPO, apply=not dry_run, source=source)
        if not result["verified"]:
            raise ValueError("index rebuild failed verification; index left unchanged: "
                             + "; ".join(result["problems"]))
        return result
    known = set()
    if index_path(root).exists():
        for record in archive.read_lines(index_path(root)):
            known.add(record.get("run_id"))
            resolved = resolve_record(record, root)
            if resolved is not None:
                known.add(resolved.name)
    added, skipped = [], []
    for child in sorted(root.iterdir()):
        if not child.is_dir():
            continue
        bundle = child.resolve()
        if bundle.name in known:
            continue
        try:
            record = index_bundle(bundle, root)
        except (OSError, ValueError) as error:
            skipped.append({"bundle": str(bundle), "reason": str(error)})
            continue
        added.append(record)
    for record in added:
        append_index(runs_root, record)
    return {"added": len(added), "skipped": skipped, "records": added}


def read_index(runs_root):
    """Every record, oldest first, with `bundle_path` resolved when it exists.

    `bundle_path` is added in memory for the caller; it is never written back.
    """
    records = archive.read_lines(index_path(runs_root))
    # The file is appended in whatever order bundles were discovered. Chronology
    # is what "latest" and "outstanding" mean, so order by start time and fall
    # back to the directory's mtime for bundles whose wrapper never started.
    def started(record):
        return record.get("started") or ""
    def mtime(record):
        path = record.get("bundle_path")
        try:
            return Path(path).stat().st_mtime if path else 0.0
        except OSError:
            return 0.0
    for record in records:
        resolved = resolve_record(record, runs_root)
        record["bundle_path"] = str(resolved) if resolved else None
    records = list({(record.get("run_id") or record.get("bundle")): record for record in records}.values())
    records.sort(key=lambda record: (started(record), mtime(record)))
    return records


def query_index(runs_root, status=None, source_commit=None, since=None, limit=None):
    records = read_index(runs_root)
    if status:
        records = [record for record in records if record.get("status") == status]
    if source_commit:
        records = [record for record in records if (record.get("source_commit") or "").startswith(source_commit)]
    if since:
        records = [record for record in records if (record.get("started") or "") >= since]
    if limit:
        records = records[-limit:]
    return records


def status_lines(status):
    """The status as short text lines: one per commit trend, REGRESSION first."""
    lines = []
    shown = status.get("regressions", [])[-10:]
    if len(status.get("regressions", [])) > len(shown):
        lines.append(f"... {len(status['regressions']) - len(shown)} older regressions omitted (see `regressions` in the JSON)")
    for item in shown:
        lines.append(f"REGRESSION run {item['run_id']} commit {item['commit'][:12]} matched prefix "
                     f"{item['matched_prefix']} < {item['best_prior_prefix']} "
                     f"(best ancestor run {item['best_prior_run']} at {item['best_prior_commit'][:12]}; "
                     f"lost {item['lost_records']} records)")
    for entry in status.get("trend", []):
        lines.append(f"commit {entry['commit'][:12]} runs={entry['runs']} best_prefix={entry['best']} "
                     f"latest_prefix={entry['latest']} (run {entry['latest_run']})")
    if status.get("unknown_commit_runs"):
        lines.append(f"{status['unknown_commit_runs']} runs have an unknown commit and are excluded from the trend")
    return lines


def bundle_status(runs_root):
    """Latest bundle per state, the newest failure, and matched-prefix progress.

    `trend` is the best/latest matched prefix per commit; `regressions` lists runs
    whose prefix is shorter than the best earlier run of the same case at an
    ancestor commit. Hook records are reported apart: they gate commits, not parity.
    """
    everything = read_index(runs_root)
    hooks = [record for record in everything if record.get("kind") == "hook"]
    records = [record for record in everything if record.get("kind") != "hook"]
    hook_failed = [record for record in hooks if record.get("status") != "completed"]
    hook_summary = {"records": len(hooks), "latest": hooks[-1] if hooks else None,
                    "latest_failed": hook_failed[-1] if hook_failed else None}
    if not records:
        return {"records": 0, "latest": None, "latest_failed": None, "outstanding": [],
                "note": "no indexed bundles", "trend": [], "regressions": [], "hooks": hook_summary,
                "lines": []}
    latest = records[-1]
    failed = [record for record in records if record.get("status") != "completed"]
    # A failure is outstanding unless a later completed run exists for the same case.
    outstanding = []
    for record in failed:
        if not any(later.get("status") == "completed"
                   and later.get("source_commit") == record.get("source_commit")
                   and later.get("started", "") > record.get("started", "")
                   for later in records):
            outstanding.append(record)
    progress = archive.trend_and_regressions(records, REPO)
    result = {"records": len(records), "latest": latest,
              "latest_failed": failed[-1] if failed else None,
              "outstanding": outstanding, **progress, "hooks": hook_summary,
              "unknown_commit_runs": sum(1 for record in records
                                         if record.get("source_commit") in (None, archive.UNKNOWN))}
    result["lines"] = status_lines(result)
    return result


DIGEST_SCHEMA = 1
DIGEST_CHAIN = 20
DIGEST_BYTES = 48 * 1024
DIGEST_SCAN_BYTES = 1024 * 1024
DIGEST_STRING = 240


def find_bundle(target, runs_root):
    """A bundle directory from a path, a run ID or a unique run-ID prefix."""
    path = Path(target)
    if path.is_dir():
        return path.resolve()
    root = Path(runs_root)
    roots = [root, *archive.legacy_roots(REPO)]
    if index_path(root).exists():
        matches = [record for record in read_index(root) if (record.get("run_id") or "").startswith(str(target))]
        if len(matches) > 1:
            raise ValueError(f"run ID prefix is ambiguous: {target} ({len(matches)} matches)")
        if matches and matches[0].get("bundle_path"):
            return Path(matches[0]["bundle_path"])
    for candidate_root in roots:
        if (candidate_root / str(target)).is_dir():
            return (candidate_root / str(target)).resolve()
        if candidate_root.is_dir():
            near = [child for child in candidate_root.iterdir() if child.is_dir() and child.name.startswith(str(target))]
            if len(near) == 1:
                return near[0].resolve()
    raise ValueError(f"no bundle or run ID matches {target!r} under {root}")


def clip(value, limit=DIGEST_STRING):
    """Bound any JSON value: long strings are cut and say how much was lost."""
    if isinstance(value, str):
        return value if len(value) <= limit else value[:limit] + f"...[+{len(value) - limit} chars]"
    if isinstance(value, (list, tuple)):
        return [clip(item, limit) for item in value[:16]] + ([f"...[+{len(value) - 16} items]"] if len(value) > 16 else [])
    if isinstance(value, dict):
        return {str(key): clip(item, limit) for key, item in list(value.items())[:24]}
    return value


def event_summary(event):
    kind = event.get("kind") if isinstance(event.get("kind"), dict) else {"event": event.get("kind")}
    context = event.get("context") or {}
    native = context.get("native") or {}
    return {"sequence": event.get("sequence"), "category": kind.get("category") or kind.get("event"),
            "fields": clip(kind.get("fields", [])), "thread": context.get("thread_name"),
            "script": (context.get("location") or {}).get("label"),
            "native": {key: native.get(key) for key in ("call_id", "class", "name")} if native else None}


def tail_events(path, count, scan_bytes):
    """The last `count` complete events of a stream, reading at most `scan_bytes`.

    Returns (events, truncated). A crashed writer leaves a partial final line;
    it is dropped, never guessed at. `truncated` says earlier events exist.
    """
    path = Path(path)
    if not path.is_file():
        return [], False
    size = path.stat().st_size
    with path.open("rb") as source:
        source.seek(max(0, size - scan_bytes))
        data = source.read()
    lines = data.split(b"\n")
    complete = size - len(data) == 0
    if size > scan_bytes:
        lines = lines[1:]  # first line is cut mid-record
        complete = False
    lines = lines[:-1] if lines and lines[-1] != b"" else lines  # partial tail from a killed writer
    events = []
    for line in lines:
        if line.strip():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue  # a torn record is skipped, never repaired
    return events[-count:], (not complete) or len(events) > count


def digest(target, runs_root, chain=DIGEST_CHAIN, max_bytes=DIGEST_BYTES, scan_bytes=DIGEST_SCAN_BYTES):
    """A size-bounded summary of one bundle for an agent. Reads; never executes.

    Works on complete, failed and unfinished bundles: every section that cannot
    be read becomes an entry in `errors` rather than a failure. Every cut
    (events, strings, bytes scanned, the runtime's own ring) sets a flag under
    `truncated`, so a short answer is never mistaken for a complete one.
    """
    bundle = find_bundle(target, runs_root)
    errors, truncated = [], {}
    def guarded(label, function, default=None):
        try:
            return function()
        except Exception as error:
            errors.append({"section": label, "error": f"{type(error).__name__}: {error}"})
            return default
    manifest = guarded("manifest", lambda: bundle_manifest(bundle), {})
    report_path = bundle / "report.json"
    report = {}
    if report_path.is_file():
        if report_path.stat().st_size > 64 * 1024 * 1024:
            truncated["report"] = f"report.json is {report_path.stat().st_size} bytes; not loaded"
        else:
            report = guarded("report", lambda: json.loads(report_path.read_text()), {})
    record = guarded("index_record", lambda: index_record(bundle, manifest, report, Path(runs_root)), {}) or {}
    comparisons = report.get("comparisons") or manifest.get("comparisons") or {}
    mismatch_value = next(((label, value) for label, value in comparisons.items() if value.get("equal") is False), None)
    mapped = (report.get("divergence_context") or {}).get("comparison")
    if mapped in comparisons:
        mismatch_value = (mapped, comparisons[mapped])
    first = None
    if mismatch_value:
        label, value = mismatch_value
        first = {"comparison": label, "kind": value.get("kind"), "record": value.get("record"),
                 "expected": clip(value.get("expected")), "actual": clip(value.get("actual")),
                 "expected_records": value.get("expected_records"), "actual_records": value.get("actual_records"),
                 "signature": record.get("first_mismatch_signature")}
    event = (report.get("divergence_context") or {}).get("event")
    native = ((event or {}).get("context") or {}).get("native")
    streams = stream_dir(bundle)
    if event:
        history = (report["divergence_context"].get("preceding_events") or [])[-(chain - 1):] + [event]
        chain_events, cut = history, len((report["divergence_context"].get("preceding_events") or [])) > chain - 1
    else:
        chain_events, cut = guarded("events", lambda: tail_events(streams / "events.jsonl", chain, scan_bytes), ([], False))
    truncated["causal_chain"] = bool(cut)
    failure = None
    failure_path = streams / "failure.json"
    if failure_path.is_file():
        value = guarded("failure", lambda: json.loads(failure_path.read_text()), None)
        if isinstance(value, dict):
            stats = value.get("diagnostics") or {}
            ring = value.get("recent_events") or []
            failure = {"present": True, "message": clip(value.get("message") or value.get("failure"), 600),
                       "location": value.get("location"), "phase": value.get("phase"),
                       "backtrace": clip(value.get("backtrace"), 1500),
                       "ring": {"capacity": stats.get("capacity"), "events_total": stats.get("events_total"),
                                "retained": stats.get("ring_retained"), "evicted": stats.get("ring_evicted"),
                                "returned": min(len(ring), chain), "ring_truncated": bool(stats.get("ring_evicted")),
                                "output_truncated": len(ring) > chain},
                       "recent_events": [event_summary(item) for item in ring[-chain:]]}
            truncated["failure_ring"] = bool(stats.get("ring_evicted")) or len(ring) > chain
    if failure is None:
        failure = {"present": False}
    crash_path = streams / "crash.json"
    crash = guarded("crash", lambda: json.loads(crash_path.read_text()), None) if crash_path.is_file() else None
    prefixes = None
    if manifest.get("status") in (None, "running") or not comparisons:
        prefixes = {label: {key: value for key, value in native_prefix(streams / name, scan_bytes).items() if key != "recent_records"}
                    for label, name in (("filtered", "runtime.calllog"), ("unfiltered", "runtime-all.calllog"))}
        truncated["native_prefixes"] = any(value.get("truncated") for value in prefixes.values())
    runs = Path(runs_root)
    quoted = lambda path: json.dumps(str(path))
    commands = {"replay_from": f"python3 re/boot-diagnose.py --runs-root {runs} --replay-from {bundle}",
                "check_regression": f"python3 re/boot-diagnose.py --runs-root {runs} --check-regression {bundle}",
                "analyze": f"python3 re/boot-diagnose.py analyze {bundle}",
                "digest": f"python3 re/boot-diagnose.py digest {bundle} --json"}
    result = {"schema_version": DIGEST_SCHEMA, "run_id": manifest.get("run_id"), "bundle": str(bundle),
              "embedding": manifest.get("embedding", "slr-boot"), "kind": record.get("kind", "run"),
              "status": manifest.get("status"), "finished": manifest.get("finished"),
              "reconstructed": manifest.get("reconstructed"), "error": clip(report.get("error") or manifest.get("error"), 600),
              "provenance": {key: record.get(key) for key in ("source_commit", "source_commit_reason",
                             "source_commit_basis", "branch", "worktree", "session", "source_dirty",
                             "source_diff_sha256", "binary_sha256")},
              "progress": {"matched_prefix": record.get("matched_prefix"), "records_total": record.get("records_total"),
                           "first_mismatch_signature": record.get("first_mismatch_signature")},
              "first_mismatch": first,
              "native_call": ({key: native.get(key) for key in ("call_id", "class", "name", "signature")} if native else None),
              "causal_chain": [event_summary(item) for item in chain_events],
              "failure": failure,
              "crash": clip(crash, 400) if crash else None,
              "native_prefixes": prefixes,
              "targeted_rerun": record.get("targeted_rerun"),
              "commands": commands, "errors": errors,
              "limits": {"chain_events": chain, "max_bytes": max_bytes, "string_chars": DIGEST_STRING,
                         "scan_bytes": scan_bytes},
              "truncated": truncated}
    result["bytes"] = 0
    def size():
        # Include the bytes field itself and the CLI's trailing newline.
        while result["bytes"] != len(digest_json(result).encode()):
            result["bytes"] = len(digest_json(result).encode())
        return result["bytes"]
    # Last resort for a pathological bundle: shed the oldest chain entries,
    # then the ring, until the answer fits; each cut is declared.
    while size() > max_bytes and result["causal_chain"]:
        result["causal_chain"] = result["causal_chain"][len(result["causal_chain"]) // 4 + 1:]
        result["truncated"]["causal_chain"] = True
        result["truncated"]["output"] = True
    while size() > max_bytes and result["failure"].get("recent_events"):
        result["failure"]["recent_events"] = result["failure"]["recent_events"][len(result["failure"]["recent_events"]) // 4 + 1:]
        result["failure"]["ring"]["output_truncated"] = True
        result["failure"]["ring"]["returned"] = len(result["failure"]["recent_events"])
        result["truncated"]["failure_ring"] = True
        result["truncated"]["output"] = True
    # Small budgets may not hold even an empty chain and ring. Explicitly
    # omit optional sections rather than return an oversized digest.
    for section in ("commands", "crash", "targeted_rerun", "native_prefixes", "provenance",
                    "native_call", "first_mismatch", "failure", "error", "errors", "bundle",
                    "run_id", "progress", "reconstructed", "finished", "embedding", "kind", "status"):
        if size() <= max_bytes:
            break
        result[section] = None
        result["truncated"]["output"] = True
        result["truncated"].setdefault("sections", []).append(section)
    size()
    return result


def digest_json(result):
    """The exact serialization whose UTF-8 size the digest reports."""
    return json.dumps(result, indent=2, sort_keys=True, default=str) + "\n"


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] in (["index"], ["list"], ["query"], ["status"], ["prune"]):
        command, rest = argv[0], argv[1:]
        parser = argparse.ArgumentParser(prog=f"boot-diagnose.py {command}")
        parser.add_argument("--runs-root", default=None,
                            help="runs directory (default: $SLR_RUNS_ROOT/boot/runs, else the permanent "
                                 "archive, else a repository-local re/out/boot/runs that already has history)")
        parser.add_argument("--status")
        parser.add_argument("--commit")
        parser.add_argument("--since")
        parser.add_argument("--limit", type=int)
        parser.add_argument("--keep", type=int)
        parser.add_argument("--dry-run", action="store_true")
        parser.add_argument("--source", help="index --rebuild: read bundles (and the old index) from this existing "
                                             "runs directory, read-only, and write the new index under --runs-root")
        parser.add_argument("--kind", default="run", choices=["run", "hook"], help="prune: which records to retire")
        parser.add_argument("--rebuild", action="store_true",
                            help="index: one-time safe rebuild from the bundles on disk (backup, verify "
                                 "in a new file, then atomic swap; bundles are never modified)")
        parser.add_argument("--json", action="store_true")
        parser.add_argument("--text", action="store_true", help="status: print the trend lines, not JSON")
        options = parser.parse_args(rest)
        options.runs_root = str(archive.reader_runs_root(options.runs_root, REPO) if command != "prune"
                                else (options.runs_root or archive.default_runs_root()))
        if command == "prune":
            if options.keep is None:
                parser.error("prune needs --keep N")
            result = prune(options.runs_root, options.keep, options.dry_run, options.kind)
        elif command == "index":
            result = index_missing(options.runs_root, options.rebuild, options.dry_run, options.source)
        elif command == "status":
            result = bundle_status(options.runs_root)
            if options.text:
                print("\n".join(result["lines"]) or "no matched-prefix history")
                return 0
        else:
            result = query_index(options.runs_root, options.status, options.commit,
                                 options.since, options.limit)
        print(json.dumps(result, indent=2, sort_keys=True, default=str))
        return 0
    if argv[:1] == ["fixtures"]:
        parser = argparse.ArgumentParser(prog="boot-diagnose.py fixtures",
                                         description="Provenance and staleness of the local re/out test fixtures.")
        parser.add_argument("action", choices=["status", "lock", "snapshot"])
        parser.add_argument("--repository", default=str(REPO))
        parser.add_argument("--manifest")
        parser.add_argument("--lock")
        parser.add_argument("--json", action="store_true")
        options = parser.parse_args(argv[1:])
        registry = fixtures.load_manifest(options.repository, options.manifest)
        lock_file = fixtures.lock_path(options.repository, registry, options.lock)
        if options.action == "lock":
            written = fixtures.lock(options.repository, registry, lock_file)
            print(json.dumps({"lock": str(lock_file), "locked": len(written)}) if options.json
                  else f"locked {len(written)} fixtures in {lock_file}")
            return 0
        if options.action == "snapshot":
            print(json.dumps(fixtures.snapshot(options.repository, registry), indent=2, sort_keys=True))
            return 0
        results = fixtures.evaluate(options.repository, registry, fixtures.load_lock(lock_file))
        if options.json:
            print(json.dumps({"fixtures": results, "lines": fixtures.status_lines(results)}, indent=2))
        else:
            print("\n".join(fixtures.status_lines(results)))
        return 1 if fixtures.fatal(results) else 0
    if argv[:1] == ["digest"]:
        parser = argparse.ArgumentParser(prog="boot-diagnose.py digest",
                                         description="Bounded JSON summary of one bundle; never runs the game.")
        parser.add_argument("target", help="bundle directory, run ID or unique run-ID prefix")
        parser.add_argument("--runs-root", default=None)
        parser.add_argument("--json", action="store_true", help="accepted for clarity; output is always JSON")
        parser.add_argument("--chain", type=int, default=DIGEST_CHAIN)
        parser.add_argument("--max-bytes", type=int, default=DIGEST_BYTES)
        options = parser.parse_args(argv[1:])
        if options.chain < 1 or options.max_bytes < 2048:
            parser.error("--chain must be positive and --max-bytes at least 2048")
        runs_root = archive.reader_runs_root(options.runs_root, REPO)
        print(digest_json(digest(options.target, runs_root, options.chain, options.max_bytes)), end="")
        return 0
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
    parser.add_argument("--runs-root", default=str(archive.default_runs_root("boot")),
                        help="where new bundles go (default: $SLR_RUNS_ROOT/boot/runs, else "
                             "~/.local/share/street-legal/boot/runs)")
    parser.add_argument("--binary", default="target/debug/slr-boot")
    parser.add_argument("--timeout", type=float, default=float(os.environ.get("SLR_BOOT_SECONDS", "120")))
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--isolated-build", action="store_true")
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
    parser.add_argument("--wall-clock", action="store_true",
                        help="add a monotonic wall-clock field to every event")
    parser.add_argument("--repeat", type=int, default=1,
                        help="run the same case N times and record the flake rate")
    parser.add_argument("--flake-check", type=int, metavar="N", default=None,
                        help="opt-in determinism check: run the case N times, compare first-mismatch "
                             "signatures, record the flake summary; exit 3 when runs disagree")
    parser.add_argument("--keep", dest="prune_keep", type=int, default=None,
                        help="after the run, retain only the newest N bundles")
    parser.add_argument("--file-issue", nargs="?", const="create", choices=["create", "dry-run"],
                        default=None, help="open a GitHub issue when the run fails")
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
        for name in ("frames", "trace", "trace_level", "event_ring", "clock_mask", "stock", "wall_clock"):
            if name in configuration:
                setattr(args, name, configuration[name])
        # Repetition and retention describe this invocation, not the frozen case.
        args.repeat, args.prune_keep, args.file_issue = 1, args.prune_keep, args.file_issue
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
    if args.flake_check is not None:
        if args.flake_check < 2:
            parser.error("--flake-check needs at least 2 runs")
        args.repeat = args.flake_check
    if args.repeat < 1:
        parser.error("--repeat must be at least 1")
    if args.prune_keep is not None and args.prune_keep < 1:
        parser.error("--keep must be at least 1")
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
