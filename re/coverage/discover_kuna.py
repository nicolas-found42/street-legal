#!/usr/bin/env python3
"""Reproduce entry discovery only, retaining binary/tool/command/content identity."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from inventory import ROOT, DEFAULT_OUT, validated_kuna


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--binary", type=Path, default=ROOT / "reference/raw/StreetLegal_Redline.bak.exe")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--kuna", default="kuna")
    args = ap.parse_args()
    out, binary = args.out.resolve(), args.binary.resolve()
    if not out.is_relative_to(ROOT / "re/out"):
        ap.error("generated discovery must stay under ignored re/out")
    out.mkdir(parents=True, exist_ok=True)
    command = [args.kuna, "functions", str(binary), "--json"]
    version = subprocess.check_output([args.kuna, "--version"], text=True).strip()
    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
    # Preserve previous successful observations rather than overwrite a failure.
    pending = out / "kuna-functions.pending.json"
    manifest = out / "kuna-functions.pending.manifest.json"
    pending.write_text(result.stdout)
    stderr = out / "kuna-functions.pending.stderr"
    stderr.write_text(result.stderr)
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest.write_text(json.dumps({"command": command, "exit_code": result.returncode,
                                   "binary_sha256": binary_hash, "tool": version,
                                   "inventory_sha256": hashlib.sha256(pending.read_bytes()).hexdigest()}, indent=2) + "\n")
    rows, identity = validated_kuna(pending, manifest, binary_hash)
    target, target_manifest = out / "kuna-functions.json", out / "kuna-functions.manifest.json"
    if target.exists() or target_manifest.exists():
        old_rows, old_identity = validated_kuna(target, target_manifest, binary_hash)
        if identity != old_identity:
            raise ValueError("discovery profile changed; select a fresh --out directory")
        if rows != old_rows:
            raise ValueError("matching discovery profile produced changed entries; select a fresh --out directory")
    pending.replace(target)
    manifest.replace(target_manifest)
    stderr.replace(out / "kuna-functions.stderr")
    print(f"{len(rows)} discovered entries; no C export or parity claim")


if __name__ == "__main__":
    main()
