"""Shared identity, hashing and result helpers for every obligation validator.

Other modules reference these as ``core.NAME`` so a single patch point exists.
"""
from __future__ import annotations

import hashlib
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# Extraction-verified build-606 install identity, retained by existing receipts.
STOCK_MANIFEST = "671a67c0dfee0a546d1e873096d4df68a5791fe14acccd4da5ac99cd72adccce"
ORIGINAL_EXE = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"


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
