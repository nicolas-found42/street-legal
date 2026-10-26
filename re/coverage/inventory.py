#!/usr/bin/env python3
"""Read-only, address-keyed RE inventory. Presence is never parity.

Run with the oracle venv (pefile/capstone). Derived JSON stays in ignored re/out.
The instruction graph is a conservative discovery aid, not a recovered CFG:
indirect transfers, overlapping code, callbacks and undecodable paths stay open.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = ROOT / "re/out/checkpoints/20261021-campaign/re-inventory"
SYMBOL = re.compile(r"(?:FUN_|LAB_|sub_|0x)([0-9a-fA-F]{6,8})$")


def addr(value):
    if isinstance(value, int):
        return value
    match = SYMBOL.fullmatch(value)
    if not match:
        raise ValueError(f"invalid original address: {value!r}")
    return int(match[1], 16)


def hx(value):
    return f"0x{value:08x}"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def file_record(path, root=ROOT):
    return {"path": str(path.relative_to(root)) if path.is_relative_to(root) else str(path),
            "sha256": digest(path), "bytes": path.stat().st_size}


def ghidra_index(path):
    rows = {}
    for line in path.read_text().splitlines():
        address, name, count = line.split()
        a = int(address, 16)
        if a in rows:
            raise ValueError(f"duplicate Ghidra entry: {hx(a)}")
        rows[a] = {"symbol": name, "body_address_count": int(count)}
    return rows


def pseudo_blocks(path):
    parts = re.split(r"(?m)^//==== ([0-9a-fA-F]+) ([^\n]+)\n", path.read_text())
    rows = {}
    for i in range(1, len(parts), 3):
        a, name, code = int(parts[i], 16), parts[i + 1], parts[i + 2]
        if a in rows:
            raise ValueError(f"duplicate pseudocode entry: {hx(a)}")
        diagnostics = [s.strip() for s in code.splitlines()
                       if re.search(r"(?:/\*|//)\s*(?:WARNING\b|FAILED\b|warn:|jump-as-call\b)", s, re.I)]
        rows[a] = {"symbol": name, "status": "failed" if "// FAILED" in code
                   else "emitted", "body_sha256": hashlib.sha256(code.encode()).hexdigest(),
                   "diagnostics": diagnostics,
                   "authority": "historical_unbound_export",
                   "semantic_recovery": "unverified"}
    return rows


def validated_kuna(path, manifest_path, binary_hash):
    """Reject wrong-binary or altered discovery caches; don't trust file names."""
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("binary_sha256") != binary_hash:
        raise ValueError("Kuna inventory belongs to another executable")
    if manifest.get("inventory_sha256") != digest(path) or manifest.get("exit_code") != 0:
        raise ValueError("Kuna inventory content/exit status is invalid")
    document = json.loads(path.read_text())
    entries = document.get("functions")
    if not isinstance(entries, list) or not entries or document.get("count") != len(entries):
        raise ValueError("Kuna inventory missing or count mismatch")
    rows = {}
    for row in entries:
        a = addr(row.get("address", row.get("address_hex")))
        if a in rows:
            raise ValueError(f"duplicate Kuna entry {hx(a)}")
        rows[a] = row
    return rows, manifest


def focused_emissions(root):
    """Retain useful small cached exports without granting missing provenance.

    Deliberately shallow: re/out contains capture trees and recovered symlinks;
    recursively walking them is unnecessary for these known project exports.
    """
    observations = defaultdict(list)
    boot = root / "re/out/boot"
    for path in sorted(boot.glob("*.kuna/*.c")):
        code = path.read_text()
        parts = re.split(r"(?m)^// Function: (\S+) @ (0x[0-9a-fA-F]+)\n", code)
        identity = file_record(path, root)
        for i in range(1, len(parts), 3):
            observations[addr(parts[i + 1])].append({"artifact": identity,
                "symbol": parts[i], "body_sha256": hashlib.sha256(parts[i + 2].encode()).hexdigest(),
                "authority": "historical_unbound_export", "semantic_recovery": "unverified",
                "diagnostics": [line.strip() for line in parts[i + 2].splitlines()
                                if re.search(r"//\s*(?:jump-as-call|warn:)", line, re.I)]})
    for path in sorted(boot.glob("*.kuna.json")):
        document = json.loads(path.read_text())
        identity = file_record(path, root)
        for row in document.get("functions", []):
            a = addr(row.get("address", row.get("address_hex")))
            observations[a].append({"artifact": identity, "symbol": row.get("name"),
                "body_sha256": hashlib.sha256((row.get("code") or "").encode()).hexdigest(),
                "emission_error": row.get("error") or document.get("error"),
                "authority": "historical_unbound_export", "semantic_recovery": "unverified"})
    return observations


class Image:
    def __init__(self, path):
        import pefile
        self.pe = pefile.PE(str(path))
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.data = self.pe.get_memory_mapped_image()
        self.code = [(self.base + s.VirtualAddress,
                      self.base + s.VirtualAddress + s.Misc_VirtualSize)
                     for s in self.pe.sections if s.Characteristics & 0x20000000]
        self.imports = {}
        for dll in getattr(self.pe, "DIRECTORY_ENTRY_IMPORT", []):
            for row in dll.imports:
                self.imports[row.address] = (dll.dll.decode(errors="replace") + "!" +
                                            (row.name.decode(errors="replace") if row.name
                                             else f"ordinal:{row.ordinal}"))

    def is_code(self, address):
        return any(start <= address < end for start, end in self.code)

    def roots(self):
        roots = defaultdict(list)
        roots[self.base + self.pe.OPTIONAL_HEADER.AddressOfEntryPoint].append("pe_entrypoint")
        export = getattr(self.pe, "DIRECTORY_ENTRY_EXPORT", None)
        for row in export.symbols if export else []:
            if not row.forwarder:
                roots[self.base + row.address].append("pe_export")
        tls = getattr(self.pe, "DIRECTORY_ENTRY_TLS", None)
        if tls:
            offset = tls.struct.AddressOfCallBacks - self.base
            for index in range(1024):
                a = struct.unpack_from("<I", self.data, offset + index * 4)[0]
                if not a:
                    break
                roots[a].append("tls_callback")
            else:
                raise ValueError("unterminated TLS callback array")
        return roots

    def address_taken(self):
        refs = defaultdict(list)
        for block in getattr(self.pe, "DIRECTORY_ENTRY_BASERELOC", []):
            for row in block.entries:
                if row.type == 3:
                    target = struct.unpack_from("<I", self.data, row.rva)[0]
                    if self.is_code(target):
                        refs[target].append(hx(self.base + row.rva))
        return refs


def graph(image, seeds, instruction_limit=50000):
    """Follow actual branches without inferring contiguous bodies from index counts.

    Reaching another declared entry ends a body with an explicit fallthrough/tail
    edge. Calls discover additional seeds. Relocation pointers are candidates;
    they are not assumed resolved targets of any particular indirect transfer.
    """
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32, CS_GRP_JUMP, CS_GRP_RET, CS_GRP_IRET
    from capstone.x86_const import X86_OP_IMM, X86_OP_MEM, X86_INS_CALL, X86_INS_JMP
    decoder = Cs(CS_ARCH_X86, CS_MODE_32)
    decoder.detail = True
    pending = set(seeds)
    known = set(seeds)
    decoded = {}
    all_edges, indirect, gaps = [], [], []
    processed = set()
    while pending:
        entry = min(pending)
        pending.remove(entry)
        processed.add(entry)
        queue, seen = [entry], set()
        while queue:
            ip = queue.pop()
            while ip not in seen:
                if ip != entry and ip in known:
                    all_edges.append({"from": hx(entry), "site": hx(ip), "to": hx(ip),
                                      "kind": "entry_fallthrough"})
                    break
                if len(seen) >= instruction_limit:
                    gaps.append({"entry": hx(entry), "site": hx(ip), "reason": "instruction_limit"})
                    queue.clear()
                    break
                seen.add(ip)
                if not image.is_code(ip):
                    gaps.append({"entry": hx(entry), "site": hx(ip), "reason": "outside_executable_section"})
                    break
                if ip not in decoded:
                    offset = ip - image.base
                    decoded[ip] = next(decoder.disasm(image.data[offset:offset + 15], ip, count=1), None)
                ins = decoded[ip]
                if ins is None:
                    gaps.append({"entry": hx(entry), "site": hx(ip), "reason": "undecodable"})
                    break
                branch = ins.group(CS_GRP_JUMP)
                call = ins.id == X86_INS_CALL
                if branch or call:
                    op = ins.operands[0] if ins.operands else None
                    if op and op.type == X86_OP_IMM:
                        target = op.imm & 0xffffffff
                        if call or target in known:
                            all_edges.append({"from": hx(entry), "site": hx(ip), "to": hx(target),
                                              "kind": "direct_call" if call else
                                              ("direct_tail" if ins.id == X86_INS_JMP else "conditional_entry_branch")})
                        if call and image.is_code(target) and target not in known:
                            known.add(target)
                            pending.add(target)
                        if branch and target not in known:
                            queue.append(target)
                    else:
                        imported = (image.imports.get(op.mem.disp & 0xffffffff)
                                    if op and op.type == X86_OP_MEM and not op.mem.base and not op.mem.index
                                    else None)
                        indirect.append({"entry": hx(entry), "site": hx(ip),
                                         "kind": "call" if call else "jump",
                                         "instruction": ins.mnemonic + " " + ins.op_str,
                                         "import": imported,
                                         "resolution": "import_slot" if imported else "unresolved"})
                    if ins.id == X86_INS_JMP:
                        break
                if ins.group(CS_GRP_RET) or ins.group(CS_GRP_IRET) or ins.mnemonic in ("int3", "hlt", "ud2"):
                    break
                ip += ins.size
    return {"entries_scanned": sorted(known), "decoded_instruction_addresses": len(decoded),
            "edges": sorted(all_edges, key=lambda r: (r["from"], r["site"], r["to"], r["kind"])),
            "indirect": sorted(indirect, key=lambda r: (r["entry"], r["site"])),
            "gaps": sorted(gaps, key=lambda r: (r["entry"], r["site"], r["reason"]))}


def source_annotations(root):
    rows = defaultdict(list)
    files = sorted((root / "crates").rglob("*.rs"))
    for path in files:
        for number, line in enumerate(path.read_text().splitlines(), 1):
            match = re.fullmatch(r"\s*///\s*exe:\s*(\S+)\s*", line)
            if match and SYMBOL.fullmatch(match[1]):
                rows[addr(match[1])].append({"path": str(path.relative_to(root)), "line": number,
                                            "symbol": match[1], "source_sha256": digest(path)})
    return rows, files


def source_snapshot(root):
    names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                                    cwd=root).decode().split("\0")
    files = [file_record(root / p, root) for p in sorted(set(names)) if p and (root / p).is_file()]
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    return revision, files


def assess(root, binary, kuna_path=None, kuna_manifest=None):
    revision, source_identity = source_snapshot(root)
    index_path, pseudo_path = root / "re/out/index.txt", root / "re/out/all_pseudo.c"
    index, pseudo = ghidra_index(index_path), pseudo_blocks(pseudo_path)
    image, binary_hash = Image(binary), digest(binary)
    kuna, kuna_identity = validated_kuna(kuna_path, kuna_manifest, binary_hash) if kuna_path else ({}, None)
    annotations, source_files = source_annotations(root)
    focused = focused_emissions(root)
    claimed = defaultdict(list)
    test_path = root / "re/out/test-results.json"
    for row in json.loads(test_path.read_text()) if test_path.exists() else []:
        if row.get("exe_symbol") and SYMBOL.fullmatch(row["exe_symbol"]):
            claimed[addr(row["exe_symbol"])].append(row)
    natives = defaultdict(list)
    native_path = root / "re/out/native_api.tsv"
    for line in native_path.read_text().splitlines():
        fields = line.split("\t")
        natives[addr(fields[3])].append({"class": fields[0], "method": fields[1], "signature": fields[2]})
    roots, references = image.roots(), image.address_taken()
    seeds = set(index) | set(kuna) | set(natives) | set(annotations) | set(roots)
    instruction_graph = graph(image, {a for a in seeds if image.is_code(a)})
    all_addresses = seeds | set(references) | set(instruction_graph["entries_scanned"]) | set(focused)
    indirect_by_entry, edge_by_entry = defaultdict(list), defaultdict(list)
    for row in instruction_graph["indirect"]:
        indirect_by_entry[addr(row["entry"])].append(row)
    for row in instruction_graph["edges"]:
        edge_by_entry[addr(row["from"])].append(row)
    records = {}
    for a in sorted(all_addresses):
        records[hx(a)] = {"executable_address": image.is_code(a), "entry_sources": roots.get(a, []) +
                         (["ghidra"] if a in index else []) + (["kuna_default"] if a in kuna else []),
                         "ghidra": index.get(a), "kuna_default": kuna.get(a),
                         "source_emission": pseudo.get(a),
                         "focused_kuna_emission_observations": focused.get(a, []),
                         "semantics": {"status": "unverified", "reason": "no validated semantic dossier"},
                         "rust_annotations": annotations.get(a, []),
                         "differential_claims": claimed.get(a, []),
                         "verified_differential_evidence": [],
                         "differential_evidence_status": "unbound_claims" if a in claimed else "missing",
                         "native_registrations": natives.get(a, []),
                         "address_taken_sites": references.get(a, []),
                         "outgoing_direct": edge_by_entry.get(a, []),
                         "indirect_transfers": indirect_by_entry.get(a, []),
                         "scope": "cd_check_replaced_ADR0009" if a == 0x401480 else "unclassified"}
    if source_snapshot(root) != (revision, source_identity):
        raise ValueError("candidate source changed during inventory; previous report preserved, rerun on stable source")
    source_identity_bytes = json.dumps(source_identity, sort_keys=True, separators=(",", ":")).encode()
    summary = {"address_records": len(records), "ghidra_entries": len(index),
               "kuna_default_entries": len(kuna), "shared_entries": len(set(index) & set(kuna)),
               "ghidra_only_entries": len(set(index) - set(kuna)),
               "kuna_only_unclassified_entries": len(set(kuna) - set(index)),
               "ghidra_c_emitted": sum(r["status"] == "emitted" for r in pseudo.values()),
               "ghidra_diagnostic_bodies": sum(bool(r["diagnostics"]) for r in pseudo.values()),
               "focused_kuna_emission_addresses": len(focused),
               "rust_annotated_addresses": len(annotations),
               "addresses_with_differential_claims": len(claimed),
               "passing_claim_addresses": sum(any(r.get("status") == "pass" for r in rows) for rows in claimed.values()),
               "validated_semantic_dossiers": 0, "validated_differential_addresses": 0,
               "native_registrations": sum(map(len, natives.values())),
               "native_addresses_without_rust_annotation": len(set(natives) - set(annotations)),
               "address_taken_candidates": len(references),
               "address_taken_candidates_outside_declared_entries": len(set(references) - seeds),
               "graph_entries_scanned": len(instruction_graph["entries_scanned"]),
               "graph_direct_edges": len(instruction_graph["edges"]),
               "graph_indirect_transfer_sites": len({r["site"] for r in instruction_graph["indirect"]}),
               "graph_unresolved_indirect_sites": len({r["site"] for r in instruction_graph["indirect"] if r["resolution"] == "unresolved"}),
               "graph_gaps": len(instruction_graph["gaps"])}
    stock_binary = root / "stock/StreetLegal_Redline.exe"
    stock_identity = file_record(stock_binary, root) if stock_binary.exists() else None
    if stock_identity:
        stock_data, oracle_data = stock_binary.read_bytes(), binary.read_bytes()
        changed = [i for i, (a, b) in enumerate(zip(stock_data, oracle_data)) if a != b]
        stock_identity["comparison_to_oracle"] = {
            "same_length": len(stock_data) == len(oracle_data),
            "changed_byte_count": len(changed) + abs(len(stock_data) - len(oracle_data)),
            "changed_file_offsets": [hx(i) for i in changed],
            "changed_original_addresses": [hx(image.base + image.pe.get_rva_from_offset(i)) for i in changed],
            "acceptance": "identity observation only; scope/comparison authorization required separately"}
    return {"schema": "slr-re-coverage-v1", "summary": summary,
            "identity": {"binary": file_record(binary, root), "image_base": hx(image.base),
                         "stock_binary": stock_identity,
                         "source_revision": revision,
                         "candidate_source_manifest_sha256": hashlib.sha256(source_identity_bytes).hexdigest(),
                         "source_files": source_identity,
                         "kuna_inventory": kuna_identity,
                         "ghidra_index": file_record(index_path, root), "ghidra_pseudo": file_record(pseudo_path, root),
                         "native_table": file_record(native_path, root),
                         "legacy_claims": file_record(test_path, root) if test_path.exists() else None},
            "scope_accounting": [
                {"area": "CD check", "address": "0x00401480", "policy": "ADR 0009", "status": "replaced; runtime/evidence obligation retained"},
                {"area": "JVM::compileSource", "address": None, "candidate_address": "0x004165a0", "policy": "ADR 0007", "status": "compiler excluded; candidate also checks class headers, so whole-function exclusion is unproved"},
                {"area": "Win32/D3D9/DirectSound/DirectInput", "policy": "ADR 0009 tier D", "status": "replacement required; no blanket function exclusions"},
                {"area": "Dialog old format", "policy": "ADR 0006", "status": "data exclusion only; excludes no executable addresses"}],
            "limitations": ["No coverage percentage: full supported reachable address denominator is unresolved.",
                "Ghidra source/index lack binary-bound producer receipts; current hashes cannot retrospectively bind them.",
                "Historical Kuna 4,356-emission cache is absent from the checkout; fresh discovery does not reproduce decompilation.",
                "Legacy test-results rows lack candidate/executable/stock/oracle/comparison identities; no parity credits imported.",
                "Annotations identify claimed implementation locations, not implemented semantics or test coverage.",
                "Instruction traversal is an inferred discovery graph, not a complete recovered control-flow graph; candidate seeds can decode data as instructions.",
                "Indirect sites remain unresolved even when some potential targets appear as relocation pointers.",
                "Source-emission diagnostics remain open; no semantic recovery is inferred from clean C.",
                "Unclassified addresses are neither excluded nor accepted; original-address compiler/platform mappings remain work."],
            "graph": instruction_graph, "by_address": records}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", type=Path, default=ROOT)
    ap.add_argument("--binary", type=Path, default=ROOT / "reference/raw/StreetLegal_Redline.bak.exe")
    ap.add_argument("--kuna-inventory", type=Path)
    ap.add_argument("--kuna-manifest", type=Path)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT / "coverage.json")
    args = ap.parse_args()
    root, out = args.root.resolve(), args.out.resolve()
    if not out.is_relative_to(root / "re/out"):
        ap.error("derived report must stay under ignored re/out")
    if bool(args.kuna_inventory) != bool(args.kuna_manifest):
        ap.error("Kuna inventory and manifest are required together")
    report = assess(root, args.binary.resolve(), args.kuna_inventory, args.kuna_manifest)
    out.parent.mkdir(parents=True, exist_ok=True)
    pending = out.with_suffix(".tmp")
    pending.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    pending.replace(out)
    print(json.dumps(report["summary"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
