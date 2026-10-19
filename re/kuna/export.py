#!/usr/bin/env python3
"""Bounded, resumable second-opinion export; Ghidra remains authoritative.

All generated definitions, C, errors, and manifests live under ignored re/out.
Emitted C is an analysis observation, never evidence of behavioral parity.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]


def address(value):
    if isinstance(value, int):
        return value
    return int(re.sub(r"^(?:FUN_|LAB_|sub_|dat_|DAT_)", "", value), 16)


def read_index(path):
    entries = {}
    for line in path.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-fA-F]+) (\S+) (\d+)", line)
        if not match:
            raise ValueError(f"invalid Ghidra index row: {line!r}")
        start = int(match[1], 16)
        if start in entries:
            raise ValueError(f"duplicate Ghidra entry {start:#x}")
        entries[start] = match[2]
    return entries


def definitions(entries):
    # ExportAll reports body address *count*, not a contiguous end address.
    return "".join(f"{a:#x}={name}\n" for a, name in entries.items())


def atomic_json(path, value):
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(value, indent=2) + "\n")
    tmp.replace(path)


def read_inventory(path):
    document = json.loads(path.read_text())
    if (not isinstance(document, dict) or document.get("error")
            or not isinstance(document.get("functions"), list) or not document["functions"]):
        raise ValueError("invalid default inventory")
    if document.get("count") != len(document["functions"]):
        raise ValueError("default inventory count mismatch")
    for row in document["functions"]:
        address(row.get("address", row.get("address_hex")))
    return document


def group_state(pgid):
    """Observe the owned process group, including orphaned worker processes."""
    result = subprocess.run(["ps", "-axo", "pgid=,rss=,stat="], capture_output=True, text=True, check=True)
    living = []
    for line in result.stdout.splitlines():
        group, rss, state = line.split()
        if int(group) == pgid and not state.startswith("Z"):
            living.append(int(rss))
    return sum(living), bool(living)


def run_bounded(command, output, seconds, memory_mib):
    error = None
    start = time.monotonic()
    peak = 0
    with output.open("w") as stdout, output.with_suffix(".stderr").open("w") as stderr:
        child = subprocess.Popen(command, stdout=stdout, stderr=stderr, start_new_session=True)
        try:
            while True:
                exited = child.poll() is not None
                try:
                    rss, living = group_state(child.pid)
                    peak = max(peak, rss)
                except (OSError, ValueError, subprocess.CalledProcessError) as exc:
                    error = f"memory watchdog unavailable: {exc}"
                    living = True
                if exited and not living:
                    break
                if time.monotonic() - start > seconds:
                    error = f"batch wall time exceeded {seconds}s"
                if peak > memory_mib * 1024:
                    error = f"batch RSS exceeded {memory_mib} MiB"
                if error:
                    break
                time.sleep(0.25)
        finally:
            # The leader can exit before its workers. Always reap the owned
            # group on failure or interruption, not only a living leader.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
    if child.returncode and not error:
        error = f"kuna exited {child.returncode}; see {output.with_suffix('.stderr')}"
    return {"command": command, "exit_code": child.returncode, "error": error,
            "elapsed_seconds": round(time.monotonic() - start, 3), "peak_rss_kib": peak}


def normalize(document, targets, run_error=None):
    found = {}
    for row in document.get("functions", []):
        a = address(row.get("address", row.get("address_hex")))
        if a not in targets or a in found:
            raise ValueError(f"unexpected or duplicate output entry {a:#x}")
        found[a] = row
    rows = []
    for a, symbol in targets.items():
        row = found.get(a, {})
        error = run_error or document.get("error") or row.get("error")
        code = row.get("code")
        if not error and not (isinstance(code, str) and code.strip()):
            error = "missing function output or empty C body"
        warnings = [line.strip() for line in (code or "").splitlines()
                    if re.search(r"\bwarn(?:ing)?:|// jump-as-call\b", line, re.I)]
        rows.append({"address": f"0x{a:08x}", "ghidra_symbol": symbol,
                     "kuna_name": row.get("name"), "status": "error" if error else
                     ("c_with_warnings" if warnings else "c_emitted"),
                     "error": error, "warnings": warnings, "code": code})
    return rows


def summarize(out, entries, baseline, rows, requested=None):
    ghidra = set(entries)
    requested = ghidra if requested is None else set(requested)
    discovered = {address(r.get("address", r.get("address_hex"))) for r in baseline["functions"]}
    summary = {"ghidra_entries": len(ghidra), "kuna_default_entries": len(discovered),
               "overlap": len(ghidra & discovered), "ghidra_only": len(ghidra - discovered),
               "kuna_only": len(discovered - ghidra), "attempted": len(rows),
               "requested_entries": len(requested),
               "pending": len(requested - {address(r["address"]) for r in rows}),
               "c_emitted": sum(r["status"] != "error" for r in rows),
               "c_without_recognized_diagnostics": sum(r["status"] == "c_emitted" for r in rows),
               "c_with_warnings": sum(r["status"] == "c_with_warnings" for r in rows),
               "errors": [{k: r[k] for k in ("address", "ghidra_symbol", "error")} for r in rows if r["status"] == "error"],
               "warning_functions": [r["address"] for r in rows if r["warnings"]],
               "limitations": ["Entry overlap, not instruction/body equivalence.",
                   "C emission does not prove correctness; use the original under Unicorn.",
                   "Declared entries only: Ghidra body counts are not contiguous bounds.",
                   "External watchdog kills a whole batch; its affected entries remain errors."]}
    atomic_json(out / "summary.json", summary)
    atomic_json(out / "by-address.json", {r["address"]: r for r in rows})
    return summary


def do_export(args):
    out = args.out.resolve()
    if not out.is_relative_to(ROOT / "re/out"):
        raise ValueError("generated exports must stay under ignored re/out")
    out.mkdir(parents=True, exist_ok=True)
    entries = read_index(args.index)
    selected = {address(a) for a in args.addr} if args.addr else set(entries)
    if selected - entries.keys():
        raise ValueError("--addr is not an entry in the Ghidra index; use kuna decompile directly for interior labels")
    binary_hash = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    version = subprocess.run([args.kuna, "--version"], check=True, capture_output=True, text=True).stdout.strip()
    manifest = {"binary": str(args.binary.resolve()), "sha256": binary_hash, "kuna": version,
                "index_sha256": hashlib.sha256(args.index.read_bytes()).hexdigest(),
                "batch_size": args.batch_size, "fn_seconds": args.fn_seconds,
                "memory_mib": args.memory_mib, "mode": args.mode}
    if args.addr:
        manifest["selected_addresses"] = sorted(selected)
    manifest_path = out / "manifest.json"
    if manifest_path.exists() and json.loads(manifest_path.read_text()) != manifest:
        raise ValueError("export profile changed: choose a fresh --out directory")
    atomic_json(manifest_path, manifest)
    defs = out / "ghidra-entries.txt"
    defs.write_text(definitions(entries))
    baseline_path = out / "default-functions.json"
    baseline = None
    if baseline_path.exists():
        try:
            baseline = read_inventory(baseline_path)
        except (ValueError, TypeError, KeyError):
            baseline_path.rename(out / f"default-functions.invalid-{time.time_ns()}.json")
    if baseline is None:
        pending = out / "default-functions.pending.json"
        run = run_bounded([args.kuna, "functions", str(args.binary), "--json"], pending, 60, args.memory_mib)
        if run["error"]:
            raise ValueError(run["error"])
        baseline = read_inventory(pending)
        pending.replace(baseline_path)
    all_rows = []
    starts = [a for a in entries if a in selected]
    for offset in range(0, len(starts), args.batch_size):
        targets = {a: entries[a] for a in starts[offset:offset + args.batch_size]}
        path = out / f"batch-{offset:05d}.json"
        result_path = out / f"batch-{offset:05d}.result.json"
        if not result_path.exists():
            command = [args.kuna, "decompile-all", str(args.binary), "--json", "--no-vars",
                       "--mode", args.mode, "--max-fn-seconds", str(args.fn_seconds),
                       "--define-function", f"@{defs}"]
            for a in targets:
                command.extend(["--addr", hex(a)])
            run = run_bounded(command, path, max(60, args.batch_size * (args.fn_seconds + 1)), args.memory_mib)
            try:
                document = json.loads(path.read_text())
            except (ValueError, OSError):
                document = {}
                run["error"] = run["error"] or "invalid JSON output"
            rows = normalize(document, targets, run["error"])
            atomic_json(result_path, {"run": run, "rows": rows})
        cached = json.loads(result_path.read_text())
        # Re-evaluate diagnostic markers when this reader improves, without
        # rerunning or losing the original command/budget evidence.
        batch = normalize(json.loads(path.read_text()) if not cached["run"]["error"] else {},
                          targets, cached["run"]["error"])
        atomic_json(result_path, {"run": cached["run"], "rows": batch})
        if {address(r["address"]) for r in batch} != set(targets):
            raise ValueError(f"stale or incomplete result: {result_path}")
        all_rows.extend(batch)
        summary = summarize(out, entries, baseline, all_rows, selected)
        print(f"{len(all_rows)}/{len(starts)} attempted; {summary['c_emitted']} C, "
              f"{summary['c_with_warnings']} warned, {len(summary['errors'])} errors", flush=True)
    if hashlib.sha256(args.binary.read_bytes()).hexdigest() != binary_hash:
        raise ValueError("input executable changed during export")
    return 1 if summary["errors"] or summary["pending"] else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    exp = sub.add_parser("export")
    exp.add_argument("--binary", type=Path, default=ROOT / "reference/raw/StreetLegal_Redline.bak.exe")
    exp.add_argument("--index", type=Path, default=ROOT / "re/out/index.txt")
    exp.add_argument("--out", type=Path, default=ROOT / "re/out/kuna/ghidra-entries")
    exp.add_argument("--kuna", default="kuna")
    exp.add_argument("--batch-size", type=int, default=32)
    exp.add_argument("--fn-seconds", type=int, default=3)
    exp.add_argument("--memory-mib", type=int, default=768)
    exp.add_argument("--mode", choices=["auto", "reliable", "fast", "aggressive"], default="auto")
    exp.add_argument("--addr", action="append", default=[], help="select a Ghidra entry (repeatable); all entries remain declared")
    lookup = sub.add_parser("lookup")
    lookup.add_argument("symbol")
    lookup.add_argument("--out", type=Path, default=ROOT / "re/out/kuna/ghidra-entries")
    args = parser.parse_args()
    try:
        if args.action == "lookup":
            rows = json.loads((args.out / "by-address.json").read_text())
            row = rows[f"0x{address(args.symbol):08x}"]
            print(json.dumps({k: v for k, v in row.items() if k != "code"}))
            if row["code"]:
                print(row["code"])
            return 1 if row["status"] == "error" else 0
        if min(args.batch_size, args.fn_seconds, args.memory_mib) <= 0:
            parser.error("budgets must be positive")
        return do_export(args)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
