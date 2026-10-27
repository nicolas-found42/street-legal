"""Archive layout, provenance, progress and index maintenance for diagnostics.

Shared by `boot-diagnose.py`, `menu-diagnose.py` and the hook recorder. Nothing
here executes the game or touches licensed data; it only reads and writes the
small JSON records that describe a run.

Layout. `SLR_RUNS_ROOT` names the permanent archive *base*; it defaults to
`$XDG_DATA_HOME/street-legal` (else `~/.local/share/street-legal`), which is
outside every worktree. Under it each embedding keeps `<kind>/runs/<run_id>/`
bundles, with `<kind>/runs-index.jsonl` and `<kind>/inputs-store/` beside them
(that is `archive_root`, the parent of a runs directory). `--runs-root` still
selects a runs directory directly, and bundles in the repository-local
`re/out/<kind>/runs` (this checkout or the main checkout) stay readable.
"""
from __future__ import annotations
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ENV_ROOT = "SLR_RUNS_ROOT"
UNKNOWN = "unknown"
SESSION_ENVIRONMENT = ("CLAUDE_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID",
                       "CODEX_SESSION_ID", "SLR_SESSION_ORIGIN")


def sha(data):
    return hashlib.sha256(data).hexdigest()


# --------------------------------------------------------------------- roots

def archive_base():
    explicit = os.environ.get(ENV_ROOT)
    if explicit:
        return Path(explicit).expanduser()
    data_home = os.environ.get("XDG_DATA_HOME")
    base = Path(data_home).expanduser() if data_home else Path.home() / ".local/share"
    return base / "street-legal"


def default_runs_root(kind="boot"):
    """Where new bundles of `kind` (boot, menu, frontend, hook) are written."""
    return archive_base() / kind / "runs"


def main_checkout(repository):
    """The primary working tree, even when `repository` is a linked worktree."""
    try:
        common = subprocess.run(["git", "-C", str(repository), "rev-parse", "--path-format=absolute",
                                 "--git-common-dir"], capture_output=True, text=True, timeout=30)
        if common.returncode == 0 and common.stdout.strip():
            return Path(common.stdout.strip()).parent
    except (OSError, subprocess.TimeoutExpired):
        pass
    return Path(repository)


def legacy_roots(repository, kind="boot"):
    """Repository-local run directories that predate the permanent archive."""
    found = []
    for base in (Path(repository), main_checkout(repository)):
        candidate = base / "re/out" / kind / "runs"
        if candidate not in found:
            found.append(candidate)
    return found


def reader_runs_root(explicit, repository, kind="boot"):
    """The runs directory a read-only command should use.

    An explicit `--runs-root` always wins. Otherwise the permanent archive is
    used, unless it holds nothing yet and a repository-local archive does, so
    existing local bundles stay discoverable without any flag.
    """
    if explicit:
        return Path(explicit)
    permanent = default_runs_root(kind)
    if has_history(permanent):
        return permanent
    for candidate in legacy_roots(repository, kind):
        if has_history(candidate):
            return candidate
    return permanent


def has_history(runs_root):
    """True when the archive beside `runs_root` indexes at least one non-hook run.

    The commit hook writes its own records into the same index; those alone must
    not hide an older repository-local archive that still holds the real runs.
    """
    index = Path(runs_root).resolve().parent / "runs-index.jsonl"
    try:
        for line in index.read_text().splitlines():
            if line.strip() and json.loads(line).get("kind") != "hook":
                return True
    except (OSError, ValueError):
        pass
    return False


def archive_root(runs_root):
    return Path(runs_root).resolve().parent


def index_path(runs_root):
    return archive_root(runs_root) / "runs-index.jsonl"


def relative_bundle(bundle, runs_root):
    """`bundle` as stored in an index record: relative to the runs root when it
    lives there, otherwise absolute (an externally chosen `--output`)."""
    bundle, root = Path(bundle).resolve(), Path(runs_root).resolve()
    try:
        return bundle.relative_to(root).as_posix()
    except ValueError:
        return str(bundle)


def resolve_bundle(record, runs_root, repository=None, kind="boot"):
    """The directory of a record's bundle, or None when it cannot be found.

    Understands relative records and legacy absolute ones. A legacy absolute
    path that no longer exists (a deleted worktree) falls back to
    `<runs-root>/<run_id>`, then to the repository-local roots.
    """
    value = record.get("bundle")
    root = Path(runs_root).resolve()
    candidates = []
    if value:
        path = Path(value)
        candidates.append(path if path.is_absolute() else root / path)
    run_id = record.get("run_id")
    if run_id:
        candidates.append(root / run_id)
        if repository is not None:
            candidates.extend(legacy / run_id for legacy in legacy_roots(repository, kind))
    for candidate in candidates:
        if candidate.is_dir():
            return candidate.resolve()
    return None


# ---------------------------------------------------------------- provenance

def _git(repository, *args):
    result = subprocess.run(["git", "-C", str(repository), *args], capture_output=True, text=True, timeout=60)
    if result.returncode != 0:
        raise RuntimeError((result.stderr or result.stdout).strip() or f"git {args[0]} failed")
    return result.stdout.strip()


def session_origin():
    """Which agent/session started this run, from the environment; else None.

    Only the variable's name and value are kept. Hostnames and user names are
    deliberately absent so records compare across machines.
    """
    for name in SESSION_ENVIRONMENT:
        value = os.environ.get(name)
        if value:
            return {"env": name, "id": value}
    return None


def provenance(repository, source=None):
    """Where a run's code came from. Never raises and never leaves the commit null.

    `source` is the manifest's `source` block when the caller already computed
    it, which supplies the dirty flag and diff hash without a second walk.
    """
    result = {"schema_version": 1, "source_commit": UNKNOWN, "source_commit_reason": None,
              "branch": None, "worktree": None, "session": session_origin(),
              "dirty": None, "source_diff_sha256": None, "source_dirty_sha256": None}
    try:
        result["source_commit"] = _git(repository, "rev-parse", "HEAD")
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        result["source_commit_reason"] = f"git could not resolve HEAD: {error}"
    for field, args in (("branch", ("rev-parse", "--abbrev-ref", "HEAD")),
                        ("worktree", ("rev-parse", "--show-toplevel"))):
        try:
            result[field] = _git(repository, *args)
        except (OSError, RuntimeError, subprocess.TimeoutExpired):
            pass
    if result["branch"] == "HEAD":
        result["branch"] = None  # detached
    if source:
        result["dirty"] = source.get("dirty")
        result["source_diff_sha256"] = source.get("tracked_diff_sha256")
        result["source_dirty_sha256"] = source.get("dirty_diff_sha256")
        if source.get("commit") and result["source_commit"] == UNKNOWN:
            result["source_commit"], result["source_commit_reason"] = source["commit"], None
    elif result["source_commit"] != UNKNOWN:
        try:
            tracked = subprocess.run(["git", "-C", str(repository), "diff", "HEAD", "--binary"],
                                     capture_output=True, timeout=120).stdout
            untracked = _git(repository, "ls-files", "--others", "--exclude-standard")
            result["dirty"] = bool(tracked or untracked)
            result["source_diff_sha256"] = sha(tracked)
        except (OSError, RuntimeError, subprocess.TimeoutExpired):
            pass
    return result


def record_provenance(manifest, bundle=None):
    """Provenance for an index record, backfilled only from reliable evidence.

    Order: the manifest's own `provenance`, its `source.commit`, and a replay's
    source commit when the replay recorded that its source did not change.
    Anything else stays `unknown` with the reason, never a guess; in particular
    `source.diff` alone cannot name a commit, so it is not used to invent one.
    """
    recorded = manifest.get("provenance") or {}
    source = manifest.get("source") or {}
    commit, basis, reason = None, None, None
    if recorded.get("source_commit") and recorded["source_commit"] != UNKNOWN:
        commit, basis = recorded["source_commit"], "manifest.provenance"
    elif source.get("commit"):
        commit, basis = source["commit"], "manifest.source"
    else:
        prior = manifest.get("source_run") or {}
        if prior.get("source_changed") is False and (prior.get("source") or {}).get("commit"):
            commit, basis = prior["source"]["commit"], "replay-source-unchanged"
    if commit is None:
        reason = recorded.get("source_commit_reason") or (
            "manifest records no commit and none is derivable from source.diff"
            if source or manifest.get("source_run") or bundle is not None else "manifest records no source identity")
    dirty = recorded.get("dirty") if recorded.get("dirty") is not None else source.get("dirty")
    return {"source_commit": commit or UNKNOWN, "source_commit_reason": reason,
            "source_commit_basis": basis, "branch": recorded.get("branch"),
            "worktree": recorded.get("worktree"), "session": recorded.get("session"),
            "source_dirty": dirty,
            "source_diff_sha256": recorded.get("source_diff_sha256") or source.get("tracked_diff_sha256"),
            "source_dirty_sha256": recorded.get("source_dirty_sha256") or source.get("dirty_diff_sha256")}


# ------------------------------------------------------------------ progress

def comparison_progress(value):
    """(matched_prefix, total_expected) for one comparison, or (None, None)."""
    if not isinstance(value, dict) or value.get("equal") is None:
        return None, None
    if value["equal"]:
        records = value.get("records")
        return records, records
    record = value.get("record")
    matched = record - 1 if isinstance(record, int) and record > 0 else None
    return matched, value.get("expected_records")


def progress(comparisons):
    """Matched-prefix counts for both comparisons; None for a frontend bundle."""
    if not comparisons:
        return {"matched_prefix": None, "records_total": None}
    matched, total = {}, {}
    for label in ("filtered", "unfiltered"):
        matched[label], total[label] = comparison_progress(comparisons.get(label))
    return {"matched_prefix": matched, "records_total": total}


def mismatch_signature(report):
    """Stable identity of the first mismatch, independent of run ID and clocks.

    Hashes where it happened (comparison, kind, ordinal) and the native/script
    identity on both sides plus the interpreter thread and script location. The
    native call ID, sequence numbers and the compared values are excluded: the
    first is a per-run counter, the others are what nondeterminism changes.
    Returns (signature, values_signature) or (None, None) without a mismatch.
    """
    comparisons = report.get("comparisons") or {}
    mapped = (report.get("divergence_context") or {}).get("comparison")
    label = mapped if mapped in comparisons else next(
        (name for name, value in comparisons.items() if value.get("equal") is False), None)
    if label is None or comparisons[label].get("equal") is not False:
        return None, None
    value = comparisons[label]

    def native(record):
        return [record[1], record[2]] if isinstance(record, (list, tuple)) and len(record) > 2 else None

    context = ((report.get("divergence_context") or {}).get("event") or {}).get("context") or {}
    identity = (context.get("native") or {})
    document = {"comparison": label, "kind": value.get("kind"), "ordinal": value.get("record"),
                "expected": native(value.get("expected")), "actual": native(value.get("actual")),
                "thread": context.get("thread_name"),
                "script": (context.get("location") or {}).get("label"),
                "native": {key: identity.get(key) for key in ("class", "name", "signature")} if identity else None}
    encode = lambda item: json.dumps(item, sort_keys=True, separators=(",", ":"), default=str).encode()
    values = {**document, "expected_record": value.get("expected"), "actual_record": value.get("actual")}
    return sha(encode(document)), sha(encode(values))


# --------------------------------------------------------------------- index

def read_lines(path):
    records = []
    if not Path(path).exists():
        return records
    for number, line in enumerate(Path(path).read_text().splitlines(), 1):
        if line.strip():
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise ValueError(f"{path}:{number}: malformed index record: {error}")
    return records


def write_lines(path, records):
    with Path(path).open("xb") as output:
        for record in records:
            output.write(json.dumps(record, sort_keys=True, default=str).encode() + b"\n")
        output.flush()
        os.fsync(output.fileno())


def timestamp():
    return dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")


def rebuild_index(runs_root, build_record, repository=None, apply=True, source=None):
    """One-time safe rebuild of `runs-index.jsonl` from the bundles on disk.

    1. The current index is copied to `runs-index.jsonl.bak-<UTC>` (kept).
    2. The rebuilt index is written to a *new* file beside it and verified:
       the counts add up, every record resolves to an existing bundle, and every
       run the old index named is still present.
    3. Only then is the new file moved over the old one with `os.replace`.

    Bundles are only read: never deleted, moved or modified. Old records whose
    bundle is gone are carried over unchanged (history survives pruning) and
    flagged `bundle_missing`. With `apply=False` everything but the swap runs.
    `build_record(bundle)` produces the record for one bundle directory.

    `source` names an existing runs directory to read bundles (and its old index)
    from, read-only, while the new index is written under `runs_root`'s archive:
    adopt a legacy archive into a new permanent root without writing to it.
    """
    root = Path(runs_root).resolve()
    path = index_path(root)
    path.parent.mkdir(parents=True, exist_ok=True)
    origin = Path(source).resolve() if source else root
    old = read_lines(path)
    if origin != root:
        known = {record.get("run_id") for record in old}
        old += [record for record in read_lines(index_path(origin)) if record.get("run_id") not in known]
    stamp = timestamp()
    backup = None
    if path.exists():
        backup = path.with_name(path.name + f".bak-{stamp}")
        data = path.read_bytes()
        with backup.open("xb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        if backup.read_bytes() != data:
            raise ValueError("index backup does not match the index")
    rebuilt, skipped, directories = [], [], 0
    for child in sorted(origin.iterdir()) if origin.is_dir() else []:
        if not child.is_dir():
            continue
        directories += 1
        try:
            rebuilt.append(build_record(child.resolve(), root))
        except (OSError, ValueError, KeyError) as error:
            skipped.append({"bundle": child.name, "reason": f"{type(error).__name__}: {error}"})
    by_run = {record.get("run_id") for record in rebuilt}
    carried = []
    for record in old:
        if record.get("run_id") not in by_run:
            # Gone (or unreadable): keep the history line, honestly labelled.
            carried.append({**record, "bundle_missing": resolve_bundle(record, root, repository) is None})
    records = rebuilt + carried
    candidate = path.with_name(path.name + f".new-{stamp}")
    write_lines(candidate, records)
    problems = []
    if len(rebuilt) + len(skipped) != directories:
        problems.append(f"counts: {len(rebuilt)} indexed + {len(skipped)} skipped != {directories} bundle directories")
    written = read_lines(candidate)
    if len(written) != len(records):
        problems.append(f"counts: wrote {len(written)} of {len(records)} records")
    unresolved = [record.get("run_id") for record in written
                  if not record.get("bundle_missing") and resolve_bundle(record, root, repository) is None
                  and resolve_bundle(record, origin, repository) is None]
    if unresolved:
        problems.append(f"{len(unresolved)} records do not resolve to a bundle: {unresolved[:5]}")
    lost = {record.get("run_id") for record in old} - {record.get("run_id") for record in written}
    if lost:
        problems.append(f"{len(lost)} previously indexed runs are absent from the rebuild: {sorted(lost)[:5]}")
    result = {"runs_root": str(root), "source": str(origin), "backup": str(backup) if backup else None, "candidate": str(candidate),
              "previous_records": len(old), "bundle_directories": directories, "indexed": len(rebuilt),
              "skipped": skipped, "carried_over": len(carried), "records": len(written),
              "verified": not problems, "problems": problems, "swapped": False}
    if problems or not apply:
        return result
    os.replace(candidate, path)
    result["swapped"] = True
    return result


# ------------------------------------------------------------------- regress

def ancestors(repository, commit):
    """Commits reachable from `commit`, or None when git cannot say."""
    if not commit or commit == UNKNOWN:
        return None
    try:
        output = _git(repository, "rev-list", commit)
    except (OSError, RuntimeError, subprocess.TimeoutExpired):
        return None
    return set(output.split())


def prefix_total(record):
    """A single comparable number: the shorter matched prefix of both streams.

    Parity needs both streams to agree, so progress is the smaller of the two.
    A missing comparison (frontend, early failure) has no prefix at all.
    """
    matched = record.get("matched_prefix")
    if not isinstance(matched, dict):
        return None
    values = [matched.get(label) for label in ("filtered", "unfiltered")]
    return None if any(value is None for value in values) else min(values)


def case_signature(manifest):
    """Identity of the case a run exercised: its configuration and input hashes.

    Two runs are only comparable (a shorter prefix is only a regression) when
    they ran the same oracle logs, signatures, profiles and replay under the same
    configuration. The commit and binary are deliberately not part of it.
    """
    inputs = {label: (item or {}).get("sha256") for label, item in (manifest.get("inputs") or {}).items()}
    document = {"configuration": {key: value for key, value in (manifest.get("configuration") or {}).items()
                                  if key not in ("repeat", "wall_clock")}, "inputs": inputs}
    return sha(json.dumps(document, sort_keys=True, separators=(",", ":"), default=str).encode())


def default_case_key(record):
    return record.get("case_sha256") or json.dumps(record.get("configuration"), sort_keys=True)


def trend_and_regressions(records, repository, case_key=default_case_key):
    """Matched-prefix trend per commit, and runs shorter than an ancestor's best.

    Only runs of the same case (configuration) are comparable. A run regresses
    when its prefix is shorter than the best prefix of any *strictly earlier*
    run whose commit is an ancestor of (or equal to, for a different run) its
    own commit (same-commit comparisons need both runs clean). Records with an unknown commit or prefix are never flagged and
    never used as a baseline: there is nothing reliable to compare.
    """
    usable = [record for record in records if record.get("embedding", "slr-boot") == "slr-boot"
              and record.get("source_commit") not in (None, UNKNOWN) and prefix_total(record) is not None]
    cache = {}
    def reachable(commit):
        if commit not in cache:
            cache[commit] = ancestors(repository, commit)
        return cache[commit]
    trend = {}
    for record in usable:
        entry = trend.setdefault(record["source_commit"], {"commit": record["source_commit"], "runs": 0,
                                                            "best": None, "latest": None, "latest_run": None})
        total = prefix_total(record)
        entry["runs"] += 1
        entry["best"] = total if entry["best"] is None else max(entry["best"], total)
        entry["latest"], entry["latest_run"] = total, record.get("run_id")
    regressions = []
    for index, record in enumerate(usable):
        total, ancestry = prefix_total(record), reachable(record["source_commit"])
        if ancestry is None:
            continue
        best = None
        for prior in usable[:index]:
            if case_key(prior) != case_key(record) or prior["source_commit"] not in ancestry:
                continue
            # At the same commit only clean runs are comparable: two dirty runs
            # differ by uncommitted edits the commit cannot name.
            if prior["source_commit"] == record["source_commit"] and (
                    prior.get("source_dirty") is not False or record.get("source_dirty") is not False):
                continue
            candidate = prefix_total(prior)
            if best is None or candidate > best[0]:
                best = (candidate, prior)
        if best and total < best[0]:
            regressions.append({"run_id": record.get("run_id"), "commit": record["source_commit"],
                                "matched_prefix": total, "best_prior_prefix": best[0],
                                "best_prior_run": best[1].get("run_id"), "best_prior_commit": best[1]["source_commit"],
                                "lost_records": best[0] - total})
    ordered = sorted(trend.values(), key=lambda entry: entry["commit"])
    return {"trend": ordered, "regressions": regressions}


def atomic_write(path, data):
    """Write `data` (bytes) to `path` so a reader never sees a partial file."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(handle, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise
