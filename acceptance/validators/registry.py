"""Obligation validator registry.

Each obligation row has at most one validator function, registered from its own
module under ``validators/rows/`` (``rows/s1.py``, ``rows/g3.py``, ...). Modules
there are imported automatically, so adding a validator never edits a shared
file. A row with no registered validator stays ``unsupported``.

A validator is ``fn(ctx: Context) -> dict`` and returns ``core.result(...)``.
Fail-closed guarantees enforced here, not by each validator:

* the result must carry a known outcome and a non-empty reason;
* ``id``/``kind``/``title``/``progress``/``supporting_evidence`` always come
  from the inventory row and cannot be supplied by a validator;
* an exception raised by a validator becomes ``failed``, never ``passed``;
* ``requires`` runs first, and a validator whose prerequisites did not pass can
  inspect ``ctx.results`` but a ``passed`` result is still its own to earn;
* ``--execute`` is only accepted for ids registered ``executable=True``.
"""
from __future__ import annotations

import importlib
import pkgutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

OUTCOMES = frozenset({
    "passed", "failed", "stale", "not_run", "replay_required", "invalid_evidence",
    "blocked", "unsupported", "partial",
})
PROTECTED = frozenset({"id", "kind", "title", "progress", "supporting_evidence"})
UNSUPPORTED = "No executable validator for the complete obligation; listed evidence is supporting context only."


@dataclass
class Context:
    """What a validator may see. ``results`` holds already computed results."""

    row: dict
    results: dict[str, dict]
    execute: frozenset[str]
    output_root: Path | None
    boot_bundle: Path | None = None
    live_ids: frozenset[str] = field(default_factory=frozenset)

    options: dict = field(default_factory=dict)

    @property
    def id(self) -> str:
        return self.row["id"]

    def live(self, rid: str | None = None) -> bool:
        """True when ``rid`` (default: this row) must run its live checks, either
        because it was named by ``--execute`` or because an executed row requires it."""
        return (rid or self.id) in self.live_ids


@dataclass(frozen=True)
class Validator:
    id: str
    fn: Callable[[Context], dict]
    executable: bool
    requires: tuple[str, ...]
    module: str


REGISTRY: dict[str, Validator] = {}
_loaded = False


def validator(rid: str, *, executable: bool = False, requires: tuple[str, ...] = ()):
    """Register ``fn`` as the validator of obligation ``rid``."""

    def decorate(fn):
        known = known_ids()
        if rid not in known:
            raise ValueError(f"validator registered for unknown obligation {rid!r}")
        if rid in REGISTRY and REGISTRY[rid].fn is not fn:
            raise ValueError(f"obligation {rid} already has a validator ({REGISTRY[rid].module})")
        for dependency in requires:
            if dependency not in known:
                raise ValueError(f"{rid} requires unknown obligation {dependency!r}")
        REGISTRY[rid] = Validator(rid, fn, executable, tuple(requires), fn.__module__)
        return fn

    return decorate


def known_ids() -> list[str]:
    return [*(f"P{i:02}" for i in range(1, 31)), *(f"G{i}" for i in range(1, 12)), *(f"S{i}" for i in range(1, 14))]


def load_all() -> dict[str, Validator]:
    """Import every module under ``validators.rows`` (and ``scenarios``) once."""
    global _loaded
    if not _loaded:
        base = __package__
        for sub in ("rows", "scenarios"):
            package = importlib.import_module(f"{base}.{sub}")
            for info in pkgutil.iter_modules(package.__path__):
                if not info.name.startswith("_"):
                    importlib.import_module(f"{base}.{sub}.{info.name}")
        _loaded = True
    return REGISTRY


def executable_ids() -> list[str]:
    return sorted(rid for rid, entry in load_all().items() if entry.executable)


def order(ids: list[str]) -> list[str]:
    """Stable topological order of ``ids`` honouring ``requires``; cycles raise."""
    registry = load_all()
    done: list[str] = []
    state: dict[str, int] = {}

    def visit(rid: str):
        if state.get(rid) == 2:
            return
        if state.get(rid) == 1:
            raise ValueError(f"validator dependency cycle through {rid}")
        state[rid] = 1
        for dependency in registry[rid].requires if rid in registry else ():
            visit(dependency)
        state[rid] = 2
        done.append(rid)

    for rid in ids:
        visit(rid)
    return done


def live_closure(execute) -> frozenset[str]:
    """Executed ids plus everything they transitively require."""
    registry = load_all()
    live: set[str] = set()
    pending = [rid for rid in execute]
    while pending:
        rid = pending.pop()
        if rid in live:
            continue
        live.add(rid)
        if rid in registry:
            pending.extend(registry[rid].requires)
    return frozenset(live)


def checked(rid: str, outcome) -> dict:
    """Validate a validator's return value; anything malformed is a failure."""
    if not isinstance(outcome, dict):
        return {"result": "failed", "reason": f"{rid} validator returned {type(outcome).__name__}, not a result"}
    if outcome.get("result") not in OUTCOMES:
        return {"result": "failed", "reason": f"{rid} validator returned unknown outcome {outcome.get('result')!r}"}
    if not isinstance(outcome.get("reason"), str) or not outcome["reason"].strip():
        return {"result": "failed", "reason": f"{rid} validator returned no reason"}
    if overreach := PROTECTED & outcome.keys():
        return {"result": "failed", "reason": f"{rid} validator tried to set inventory fields: {sorted(overreach)}"}
    return outcome


def evaluate(rows: list[dict], *, boot_bundle: Path | None = None, execute=(), output_root: Path | None = None, options: dict | None = None) -> list[dict]:
    """Compute actual results for ``rows``. Only registered validators can pass."""
    registry = load_all()
    execute = frozenset(execute)
    for rid in execute:
        if rid not in registry or not registry[rid].executable:
            raise ValueError(f"{rid} has no executable validator")
    live = live_closure(execute)
    results = [
        {"id": row["id"], "kind": row["kind"], "title": row["title"], "progress": row["status"],
         "result": "unsupported", "reason": UNSUPPORTED, "supporting_evidence": row["evidence"]}
        for row in rows
    ]
    by_id = {entry["id"]: entry for entry in results}
    rows_by_id = {row["id"]: row for row in rows}
    for rid in order([rid for rid in by_id if rid in registry]):
        context = Context(rows_by_id[rid], by_id, execute, output_root, boot_bundle, live, dict(options or {}))
        try:
            outcome = checked(rid, registry[rid].fn(context))
        except Exception as error:  # fail closed: a crashing validator is a failed row
            outcome = {"result": "failed", "reason": f"{rid} validator raised {type(error).__name__}: {error}"}
        by_id[rid].update(outcome)
    return results
