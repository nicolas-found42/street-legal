#!/usr/bin/env python3
"""Recount exact-port progress from the local ledger and reference inventory.

This measures annotation/evidence presence, not complete function behavior.
Output is local assessment data; no executable bytes or vectors are emitted.
"""
from collections import Counter
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def address(symbol):
    match = re.fullmatch(r"(?:FUN_|LAB_|0x)([0-9a-fA-F]+)", symbol)
    return int(match[1], 16) if match else None


def assess(root=ROOT):
    fidelity = (root / "docs/fidelity.md").read_text()
    generated = fidelity.split("<!-- ledger:start -->", 1)[1].split("<!-- ledger:end -->", 1)[0]
    ledger = []
    for line in generated.splitlines():
        if line.startswith("| `"):
            fields = [v.strip() for v in line.split("|")[1:-1]]
            ledger.append({"symbol": fields[0].strip("`"), "status": fields[2]})
    by_symbol = {r["symbol"]: r["status"] for r in ledger}
    by_address = {}
    for r in ledger:
        a = address(r["symbol"])
        if a is not None:
            by_address.setdefault(a, set()).add(r["status"])
    native = [line.split("\t") for line in (root / "re/out/native_api.tsv").read_text().splitlines()]
    missing_symbols = [r for r in native if r[3] not in by_symbol]
    missing_addresses = [r for r in native if address(r[3]) not in by_address]
    passing_native_symbols = [r for r in native if by_symbol.get(r[3]) == "passing"]
    entries = {int(line.split()[0], 16) for line in (root / "re/out/index.txt").read_text().splitlines()}
    parity = [line.split("|")[1:-1] for line in fidelity.splitlines() if re.match(r"^\| P\d+ \|", line)]
    return {
        "ledger": {"symbol_rows": len(ledger), "statuses": dict(Counter(r["status"] for r in ledger)),
                   "unique_addresses": len(by_address),
                   "ghidra_entries_with_annotation": len(entries & by_address.keys()),
                   "ghidra_entries_with_passing_evidence": sum("passing" in by_address[a] for a in entries & by_address.keys())},
        "natives": {"registrations": len(native), "exact_symbol_present": len(native) - len(missing_symbols),
                    "exact_symbol_with_passing_evidence": len(passing_native_symbols),
                    "exact_symbol_missing": len(missing_symbols), "address_present": len(native) - len(missing_addresses),
                    "address_missing": len(missing_addresses),
                    "missing_symbols_by_class": dict(sorted(Counter(r[0] for r in missing_symbols).items())),
                    "missing_addresses_by_class": dict(sorted(Counter(r[0] for r in missing_addresses).items()))},
        "parity": {"rows": len(parity), "open": sum(r[-1].strip() == "open" for r in parity),
                   "passing": sum(r[-1].strip().startswith("passing") for r in parity)},
        "ghidra_export_entries": len(entries),
        "limitations": ["Exact-symbol and canonical-address joins are separate measures.",
                        "A LAB address can be interior to a Ghidra function; counts are not additive.",
                        "Passing rows evidence only the cases asserted by their registered tests.",
                        "Annotation presence and C emission do not prove behavior or stock parity."]}


if __name__ == "__main__":
    print(json.dumps(assess(), indent=2))
