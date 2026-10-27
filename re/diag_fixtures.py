"""Provenance and staleness of the local `re/out` fixtures the tests consume.

The fixtures are generated on the developer machine by oracle probes (they hold
licensed-derived captures and are never committed). `re/fixtures-manifest.json`
is the tracked, data-free registry: for each fixture its producer script, the
regeneration command and the other fixtures it was built from. `re/out/
fixtures-lock.json` (local, beside the data) records what was true when each
fixture was last known good: its SHA-256, the producer's SHA-256, the inputs'
hashes and when it was generated.

A fixture is

* `missing`  - absent; tests that need it skip (or fail under
  `SLR_REQUIRE_ORACLE_FIXTURES`), so this is reported but never fatal here;
* `stale`    - its producer script changed since it was generated, or an input
  it was built from changed after it (the native-VM dispatch-kernel fixture
  digests `snapshots/install-part-queue.*`; regenerating those silently breaks
  it), or a producer it embeds in `producer_sources` no longer hashes the same;
* `changed`  - the fixture file itself differs from the locked hash;
* `unlocked` - present, nothing recorded to compare against (informational);
* `ok`.

`stale` and `changed` are what the commit hook refuses to start a 22-minute test
run on. The same digest, taken at the start and end of the run, detects a
fixture regenerated mid-run.
"""
from __future__ import annotations
import datetime as dt
import hashlib
import json
import os
from pathlib import Path

MANIFEST = "re/fixtures-manifest.json"
FATAL = ("stale", "changed")


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def stamp(path):
    status = Path(path).stat()
    return {"bytes": status.st_size, "mtime_ns": status.st_mtime_ns}


def utc(timestamp_ns):
    return dt.datetime.fromtimestamp(timestamp_ns / 1e9, dt.timezone.utc).isoformat()


def load_manifest(repository, path=None):
    path = Path(path) if path else Path(repository) / MANIFEST
    value = json.loads(path.read_text())
    if value.get("schema_version") != 1 or not isinstance(value.get("fixtures"), list):
        raise ValueError(f"{path}: not a fixtures manifest (schema_version 1)")
    for entry in value["fixtures"]:
        for key in ("path", "producer", "regenerate"):
            if not entry.get(key):
                raise ValueError(f"{path}: fixture entry lacks {key!r}: {entry}")
    return value


def lock_path(repository, manifest, override=None):
    explicit = override or os.environ.get("SLR_FIXTURES_LOCK")
    if explicit:
        return Path(explicit)
    return Path(repository) / manifest.get("lock", "re/out/fixtures-lock.json")


def load_lock(path):
    try:
        value = json.loads(Path(path).read_text())
    except FileNotFoundError:
        return {"schema_version": 1, "fixtures": {}}
    if value.get("schema_version") != 1:
        raise ValueError(f"{path}: unsupported fixtures lock schema")
    return value


def find_producer(repository, manifest, name):
    """A producer by repo-relative path, or by bare file name in `producer_dirs`."""
    direct = Path(repository) / name
    if "/" in name and direct.is_file():
        return direct
    for directory in manifest.get("producer_dirs", []):
        candidate = Path(repository) / directory / Path(name).name
        if candidate.is_file():
            return candidate
    return None


def embedded_producers(path, limit=32 * 1024 * 1024):
    """`producer_sources` a fixture recorded about itself, if it is JSON that has them."""
    try:
        if path.suffix != ".json" or path.stat().st_size > limit:
            return {}
        value = json.loads(path.read_text())
    except (OSError, ValueError):
        return {}
    sources = value.get("producer_sources") if isinstance(value, dict) else None
    return sources if isinstance(sources, dict) else {}


def evaluate(repository, manifest, lock):
    """One result per fixture: path, state, one-line detail and how to regenerate."""
    repository, results = Path(repository), []
    for entry in manifest["fixtures"]:
        path = repository / entry["path"]
        regenerate = entry["regenerate"]
        row = {"path": entry["path"], "state": "ok", "detail": "", "regenerate": regenerate}
        if not path.exists():
            row.update(state="missing", detail="not generated on this machine")
            results.append(row)
            continue
        problems = []
        locked = (lock.get("fixtures") or {}).get(entry["path"])
        producer = find_producer(repository, manifest, entry["producer"])
        if producer is None:
            problems.append(("stale", f"producer {entry['producer']} not found"))
        for name, recorded in embedded_producers(path).items():
            source = find_producer(repository, manifest, name)
            if source is None:
                problems.append(("stale", f"embedded producer {name} not found"))
            elif sha256_file(source) != recorded:
                problems.append(("stale", f"producer {name} changed since generation"))
        if locked:
            if producer is not None and locked.get("producer_sha256") not in (None, sha256_file(producer)):
                problems.append(("stale", f"producer {entry['producer']} changed since generation"))
            for name, recorded in (locked.get("inputs") or {}).items():
                source = repository / name
                if not source.exists():
                    problems.append(("stale", f"input {name} is gone"))
                elif sha256_file(source) != recorded:
                    problems.append(("stale", f"input {name} was regenerated after this fixture"))
            if locked.get("sha256") and sha256_file(path) != locked["sha256"]:
                problems.append(("changed", "file differs from the locked hash"))
        elif not problems:
            row.update(state="unlocked", detail="no lock record; run `fixtures lock` after a known-good generation")
        if problems:
            order = {"changed": 0, "stale": 1}
            problems.sort(key=lambda item: order[item[0]])
            row.update(state=problems[0][0], detail="; ".join(dict.fromkeys(text for _, text in problems)))
        if locked and locked.get("generated_at"):
            row["generated_at"] = locked["generated_at"]
        else:
            row["generated_at"] = utc(path.stat().st_mtime_ns)
        results.append(row)
    return results


def status_lines(results):
    """One line per fixture that is not simply fine; `ok` fixtures are counted."""
    lines = []
    for row in results:
        if row["state"] == "ok":
            continue
        lines.append(f"{row['state'].upper():8} {row['path']}: {row['detail']} -> {row['regenerate']}")
    counts = {}
    for row in results:
        counts[row["state"]] = counts.get(row["state"], 0) + 1
    lines.append("fixtures: " + ", ".join(f"{count} {state}" for state, count in sorted(counts.items())))
    return lines


def fatal(results):
    return [row for row in results if row["state"] in FATAL]


def watched(repository, manifest):
    """Every file whose bytes a test run depends on: fixtures and their inputs."""
    names = []
    for entry in manifest["fixtures"]:
        names.append(entry["path"])
        names.extend(entry.get("inputs", []))
    seen, files = set(), []
    for name in names:
        path = Path(repository) / name
        # An input may name a family (`snapshots/install-part-queue.*`).
        matches = sorted(path.parent.glob(path.name)) if any(ch in path.name for ch in "*?[") else [path]
        for match in matches:
            if match.is_file() and match not in seen:
                seen.add(match)
                files.append(match)
    return files


def snapshot(repository, manifest, previous=None):
    """path -> {sha256, bytes, mtime_ns}. Unchanged (size, mtime) files reuse a prior hash."""
    repository, digest = Path(repository), {}
    for path in watched(repository, manifest):
        relative = path.relative_to(repository).as_posix() if path.is_relative_to(repository) else str(path)
        current = stamp(path)
        before = (previous or {}).get(relative)
        if before and before["bytes"] == current["bytes"] and before["mtime_ns"] == current["mtime_ns"]:
            digest[relative] = before
        else:
            digest[relative] = {**current, "sha256": sha256_file(path)}
    return digest


def changed_between(start, end):
    """Human lines for every file added, removed or altered between two snapshots."""
    lines = []
    for name in sorted(set(start) | set(end)):
        if name not in end:
            lines.append(f"{name}: removed during the run")
        elif name not in start:
            lines.append(f"{name}: created during the run")
        elif start[name]["sha256"] != end[name]["sha256"]:
            lines.append(f"{name}: modified during the run ({utc(end[name]['mtime_ns'])})")
    return lines


def lock(repository, manifest, path):
    """Record the current state as known good. Run it after regenerating, never before."""
    repository = Path(repository)
    record = load_lock(path)
    record["fixtures"] = record.get("fixtures") or {}
    now = dt.datetime.now(dt.timezone.utc).isoformat()
    written = []
    for entry in manifest["fixtures"]:
        fixture = repository / entry["path"]
        if not fixture.is_file():
            continue
        producer = find_producer(repository, manifest, entry["producer"])
        inputs = {}
        for name in entry.get("inputs", []):
            target = repository / name
            for match in sorted(target.parent.glob(target.name)) if any(ch in target.name for ch in "*?[") else [target]:
                if match.is_file():
                    inputs[match.relative_to(repository).as_posix()] = sha256_file(match)
        record["fixtures"][entry["path"]] = {
            "sha256": sha256_file(fixture), "generated_at": utc(fixture.stat().st_mtime_ns), "locked_at": now,
            "producer": entry["producer"], "producer_sha256": sha256_file(producer) if producer else None,
            "regenerate": entry["regenerate"], "inputs": inputs}
        written.append(entry["path"])
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(target.name + ".tmp")
    temporary.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
    os.replace(temporary, target)
    return written
