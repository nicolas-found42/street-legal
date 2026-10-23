#!/usr/bin/env python3
"""Capture original memory at a function entry during the strict headless boot.

The headless run executes build 606 from its PE entry (re/headless/startup.py).
When execution reaches `--function` for the `--nth` time at or after the
native-call index `--after-native`, every mapped nonzero page, the registers,
the stack arguments and the emulator heap pointer are written to
`re/out/snapshots/<name>.{pages,json}` and the run stops.

`snapshot_replay.py` loads such a snapshot into a single-function Oracle so an
original function graph can be re-executed on genuine game state with declared
VM boundaries, and Rust tests load the same pages into `Space`.

Page file: repeated records of `u32 address` + 4096 bytes, little endian.
"""
import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "re/headless"))
OUT = REPO / "re/out/snapshots"
PAGE = 4096
REGISTERS = ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "eip")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def producer_files():
    return [Path(__file__).resolve(), REPO / "re/oracle/oracle.py", REPO / "acceptance/validators.py", *sorted((REPO / "re/headless").glob("*.py"))]


def stock_identity():
    # Reuse the canonical strict verifier, including every stock file hash.
    import importlib.util
    spec = importlib.util.spec_from_file_location("snapshot_acceptance_validators", REPO / "acceptance/validators.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    verified = module.stock_identity(REPO / "stock")
    return {"manifest_sha256": verified["manifest_sha256"], "files": len(verified["files"])}


def capture_identity(host):
    profiles = ["re/out/headless/hypothetical-cd-toc.json", "re/out/headless/boot/graphics-profile.json",
                "re/out/headless/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
                "re/out/headless/boot/platform-profile.json"]
    return {"exe_sha256": host.exe_sha256, "stock": stock_identity(),
            "sources": {p.relative_to(REPO).as_posix(): digest(p) for p in producer_files()},
            "profiles": {p: digest(REPO / p) for p in profiles}}


def validate_identity(meta, pages):
    if meta.get("schema_version") != 2:
        raise ValueError("legacy snapshot lacks original executable/stock/producer provenance; recapture, do not relabel")
    identity = meta["provenance"]
    if identity["exe_sha256"] != digest(REPO / "reference/raw/StreetLegal_Redline.bak.exe"):
        raise ValueError("snapshot original executable changed")
    if identity["stock"] != stock_identity():
        raise ValueError("snapshot stock identity changed")
    expected = {p.relative_to(REPO).as_posix(): digest(p) for p in producer_files()}
    if identity["sources"] != expected:
        raise ValueError("snapshot capture producer changed; recapture required")
    if set(identity["profiles"]) != {"re/out/headless/hypothetical-cd-toc.json", "re/out/headless/boot/graphics-profile.json",
        "re/out/headless/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
        "re/out/headless/boot/platform-profile.json"}: raise ValueError("snapshot profile inventory incomplete")
    for path, expected in identity["profiles"].items():
        if digest(REPO / path) != expected: raise ValueError(f"snapshot profile changed: {path}")
    if hashlib.sha256(pages).hexdigest() != meta["pages_sha256"]:
        raise ValueError("snapshot page digest mismatch")
    if meta["fpu_cw"] != meta["fpu"]["control"] or meta["fpu_cw"] & 0xf00:
        raise ValueError("snapshot float control state is unsupported by device-thread port")


def dump(host, uc, name, function, args_count):
    from unicorn import x86_const as X

    OUT.mkdir(parents=True, exist_ok=True)
    pages = 0
    with open(OUT / f"{name}.pages", "wb") as out:
        for begin, end, _ in uc.mem_regions():
            for address in range(begin, end + 1, PAGE):
                data = bytes(uc.mem_read(address, PAGE))
                if data.count(0) != PAGE:
                    out.write(struct.pack("<I", address) + data)
                    pages += 1
    registers = {r: uc.reg_read(getattr(X, "UC_X86_REG_" + r.upper())) for r in REGISTERS}
    esp = registers["esp"]
    stack = list(struct.unpack(f"<{args_count + 1}I", bytes(uc.mem_read(esp, 4 * (args_count + 1)))))
    stack_ranges = [[0x7fe00000, 0x7ff00000]]
    for thread in host.threads:
        if thread != 1:
            low = 0x40000000 + thread * 0x200000
            stack_ranges.append([low, low + 0x100000])
    meta = {
        "schema_version": 2,
        "provenance": capture_identity(host),
        "pages_sha256": digest(OUT / f"{name}.pages"),
        "comparison_policy": {"id": "retained-native-memory-v1", "thread_stack_ranges": stack_ranges,
            "oracle_vm_scratch": [0x30000000, 0x30100000],
            "native_stack_bookkeeping": [0x7ffdf000, 0x7ffe0000], "retained_word_exclusions": []},
        "fpu": {"control": uc.reg_read(X.UC_X86_REG_FPCW), "status": uc.reg_read(X.UC_X86_REG_FPSW),
            "tag": uc.reg_read(X.UC_X86_REG_FPTAG),
            "registers": [list(uc.reg_read(X.UC_X86_REG_FP0 + i)) for i in range(8)]},
        "function": function,
        "registers": registers,
        "return_address": stack[0],
        "stack_args": stack[1:],
        "heap_ptr": host.heap_ptr,
        "icount": host.icount,
        "native_calls": host.observer.stats["native_calls"] if host.observer else None,
        "pages": pages,
        "fpu_cw": uc.reg_read(X.UC_X86_REG_FPCW),
    }
    (OUT / f"{name}.json").write_text(json.dumps(meta, indent=1, sort_keys=True))
    return meta


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("name")
    ap.add_argument("--function", type=lambda v: int(v, 0), required=True)
    ap.add_argument("--nth", type=int, default=1)
    ap.add_argument("--after-native", type=int, default=0)
    ap.add_argument("--args", type=int, default=6, help="stack words to record")
    ap.add_argument("--limit", type=int, default=3_000_000_000)
    a = ap.parse_args()

    import startup as S

    state = {"count": 0}
    original_block = S.Startup._on_block

    def block(self, uc, address, size, user):
        if address == a.function and self.observer is not None:
            if self.observer.stats["native_calls"] >= a.after_native:
                state["count"] += 1
                if state["count"] == a.nth:
                    state["meta"] = dump(self, uc, a.name, a.function, a.args)
                    self._err = S.Boundary("snapshot_taken")
                    uc.emu_stop()
                    return None
        return original_block(self, uc, address, size, user)

    S.Startup._on_block = block
    headless = REPO / "re/out/headless"
    sys.argv = [
        sys.argv[0],
        "--media-profile", str(headless / "hypothetical-cd-toc.json"),
        "--graphics-profile", str(headless / "boot/graphics-profile.json"),
        "--desktop-profile", str(headless / "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json"),
        "--platform-profile", str(headless / "boot/platform-profile.json"),
        "--observe-dir", str(headless / "snapshot-run/observed"),
        "--out", str(headless / "snapshot-run/boot.json"),
        "--event-limit", "100000", "--event-ring", "5000", "--fast-count",
        "--limit", str(a.limit),
    ]
    try:
        S.main()
    except SystemExit:
        pass
    if "meta" not in state:
        raise SystemExit("function entry not reached")
    print(json.dumps(state["meta"], sort_keys=True))


if __name__ == "__main__":
    main()
