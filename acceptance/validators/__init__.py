"""Live obligation validators. A receipt JSON is never an executable result.

Layout: ``core`` (identity/hash/result helpers), ``registry`` (the validator
registry and ``evaluate``), shared machinery (``boot``, ``retirement``,
``ledger``) and one module per obligation under ``rows/`` (and per-feature
scenarios under ``scenarios/``). See ``registry`` for how to add a validator.
"""
from . import boot, core, ledger, retirement  # noqa: F401
from .core import ROOT, digest, result, stock_identity, candidate_source_matches, boot_tools  # noqa: F401
from .registry import Context, evaluate, executable_ids, known_ids, load_all, validator  # noqa: F401
