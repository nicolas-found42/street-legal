#!/usr/bin/env python3
"""Build the native registration table (re/out/native_api.tsv, git-ignored).

Reads the reference exe (default: reference/raw/StreetLegal_Redline.bak.exe)
and the Ghidra headless export (default: re/out/all_pseudo.c), and writes a
353-row TSV: class<TAB>name<TAB>sig<TAB>impl, where impl is the Ghidra label
(FUN_/LAB_) of the native implementation.

Method (2026-10-17, ticket M0-04):
- Primary extraction is a byte-level scan of .text: every `E8 <rel32>`
  targeting FUN_00416b00 (the registrar, __thiscall with ecx = VM ctx loaded
  from [0x63641c]) is decoded by walking back over exactly
  `mov ecx,[0x63641c]` / `mov dword [0x63c86c],1` prefixes and the 4
  `push imm32` args (class, name, sig, impl). This is independent of Ghidra's
  formatting and finds all 360 call sites, of which 7 are java.io.Controller
  registrations Ghidra never attributed to a function (gap 0x477ca6-0x477e00).
- The committed TSV keeps the 353 rows visible in the pseudocode export
  (all_pseudo.c): the 360 binary sites minus those 7 Controller rows, so the
  table cross-checks 1:1 against the export. The Controller rows are listed
  in --controller-dump (and in docs/re-progress.md) but excluded because
  their owner function is unknown to Ghidra.
- A regex pass over all_pseudo.c (r')\\s*;') must yield the same 353 rows;
  the generator asserts this and fails loudly on any delta.

Usage:
  python3 re/build_native_table.py [exe] [all_pseudo.c] [out.tsv]

Defaults assume CWD = repo root.
"""

import re
import struct
import sys

EXE = "reference/raw/StreetLegal_Redline.bak.exe"
PSEUDO = "re/out/all_pseudo.c"
OUT = "re/out/native_api.tsv"

REGISTRAR = 0x00416B00
VM_CTX = 0x0063641C
MOV_ECX = bytes([0x8B, 0x0D, 0x1C, 0x64, 0x63, 0x00])  # mov ecx,[0x63641c]
MOV_FLAG = bytes([0xC7, 0x05, 0x6C, 0xC8, 0x63, 0x00,
                  0x01, 0x00, 0x00, 0x00])  # mov dword [0x63c86c],1

# 7 Controller registrations live in the Ghidra function gap 0x477ca6-0x477e00.
CONTROLLER_SITES = {0x477D22, 0x477D41, 0x477D60, 0x477D7F,
                    0x477D9E, 0x477DBD, 0x477DDC}


def parse_pe(exe: bytes):
    pe_off = struct.unpack_from("<I", exe, 0x3C)[0]
    n_sec = struct.unpack_from("<H", exe, pe_off + 6)[0]
    opt_size = struct.unpack_from("<H", exe, pe_off + 20)[0]
    image_base = struct.unpack_from("<I", exe, pe_off + 24 + 28)[0]
    text = None
    secs = []
    o = pe_off + 24 + opt_size
    for i in range(n_sec):
        name, vs, va, rs, rp = struct.unpack_from("<8sIIII", exe, o + i * 40)
        secs.append((va, max(vs, rs), rp))
        if name.startswith(b".text"):
            text = (va, rp, rs)
    assert text, "no .text section"
    return image_base, text, secs


def cstr(exe: bytes, secs, image_base: int, va: int) -> str:
    rva = va - image_base
    for a, s, p in secs:
        if a <= rva < a + s:
            off = p + rva - a
            end = exe.index(b"\0", off)
            return exe[off:end].decode("latin1")
    raise ValueError(f"VA {va:#x} not in any section")


def binary_rows(exe: bytes):
    """All registrar call sites from raw bytes: (site, class, name, sig, impl_va)."""
    image_base, (text_va, text_raw, text_size), secs = parse_pe(exe)
    tva = image_base + text_va
    code = exe[text_raw:text_raw + text_size]
    rows = []
    for off in range(len(code) - 5):
        if code[off] != 0xE8:
            continue
        rel = struct.unpack_from("<i", code, off + 1)[0]
        if tva + off + 5 + rel != REGISTRAR:
            continue
        p = off
        pushes = []
        while len(pushes) < 4:
            if code[p - 5] == 0x68:
                pushes.append(struct.unpack_from("<I", code, p - 4)[0])
                p -= 5
            elif code[p - 6:p] == MOV_ECX:
                p -= 6
            elif code[p - 10:p] == MOV_FLAG:
                p -= 10
            else:
                raise ValueError(f"undecodable prologue at call {tva + off:#x}")
        cls, name, sig, impl = pushes
        rows.append((tva + off, cls, name, sig, impl))
    assert len(rows) == 360, f"expected 360 call sites, got {len(rows)}"
    return rows


def pseudo_rows(pseudo: str):
    """Registrations visible in the Ghidra export: (class, name, sig, impl_label)."""
    pat = re.compile(r"FUN_00416b00\(((?:s_|&DAT_|DAT_)[^;]*?)\)\s*;", re.S)
    va_of = re.compile(r"_([0-9a-f]{8})$")
    out = []
    for m in pat.finditer(pseudo):
        args = [a.strip() for a in m.group(1).split(",")]
        assert len(args) == 4, m.group(1)[:120]
        imp = args[3][1:] if args[3].startswith("&") else args[3]
        out.append(tuple(imp if i == 3 else args[i] for i in range(4)))
    return out, va_of


def main(argv):
    exe_path, pseudo_path, out_path = (argv + [EXE, PSEUDO, OUT])[:3]
    exe = open(exe_path, "rb").read()
    pseudo = open(pseudo_path, errors="replace").read()

    image_base, _, secs = parse_pe(exe)

    def sym(symname: str) -> str:
        m = re.search(r"_([0-9a-f]{8})$", symname)
        assert m, symname
        return cstr(exe, secs, image_base, int(m.group(1), 16))

    # 1. Byte-level ground truth.
    bin_rows = binary_rows(exe)
    table = {}
    for site, cls, name, sig, impl in bin_rows:
        if site in CONTROLLER_SITES:
            continue
        key = (cstr(exe, secs, image_base, cls),
               cstr(exe, secs, image_base, name),
               cstr(exe, secs, image_base, sig))
        assert key not in table, f"duplicate registration: {key}"
        table[key] = (site, impl)

    # 2. Pseudocode cross-check: same row set, same order.
    pre_rows, _ = pseudo_rows(pseudo)
    assert len(pre_rows) == 353, f"pseudo rows: {len(pre_rows)}"
    resolved = [(sym(c), sym(n), sym(s), imp) for c, n, s, imp in pre_rows]
    assert [(c, n, s) for c, n, s, _ in resolved] == \
        [(c, n, s) for c, n, s in table], "binary/pseudo row mismatch"
    for (c, n, s, imp), (key, (site, impl_va)) in zip(resolved, table.items()):
        assert (c, n, s) == key
        assert imp in (f"FUN_{impl_va:08x}", f"LAB_{impl_va:08x}"), \
            f"{key}: pseudo {imp} vs binary {impl_va:#x}"

    # 11 overloaded (class,name) pairs (14 extra rows: File.write x4,
    # System.analyze x3, 9 more x2) are genuine exe overloads, kept as-is.
    assert len({(c, n) for c, n, s in table}) == 339, \
        "expected 11 overloaded (class,name) pairs"
    assert len({(c, n, s) for c, n, s in table}) == 353

    with open(out_path, "w") as f:
        for (c, n, s), (_, impl_va) in table.items():
            label = next(imp for (cc, nn, ss, imp) in resolved
                         if (cc, nn, ss) == (c, n, s))
            f.write(f"{c}\t{n}\t{s}\t{label}\n")
    print(f"{len(table)} rows -> {out_path}")


if __name__ == "__main__":
    main(sys.argv[1:])
