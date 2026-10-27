#!/usr/bin/env python3
"""Re-execute an original function graph on a headless snapshot.

`headless_snapshot.py` captures genuine boot memory at a function entry. This
module maps those pages into a single-function Oracle, continues the emulator
heap from the captured pointer through the bump allocator, stubs only declared
VM boundaries, and reports each boundary event and the word-level memory diff.

Rust tests load the same snapshot and must produce the same events and diff.
Output goes to re/out/snapshots/<snapshot>.<scenario>.json.
"""
import json
import hashlib
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import Oracle  # noqa: E402
from headless_snapshot import digest, validate_identity

SNAPSHOTS = REPO / "re/out/snapshots"
PAGE = 4096
REGION = 0x100000
# Fixed address returned for VM field cells; outside every captured region.
VM_CELL = 0x30000800


def read_pages(name):
    data = (SNAPSHOTS / f"{name}.pages").read_bytes()
    if len(data) % (PAGE + 4): raise ValueError("truncated snapshot page record")
    pages = {}
    for offset in range(0, len(data), PAGE + 4):
        (address,) = struct.unpack_from("<I", data, offset)
        if address % PAGE or address in pages: raise ValueError("unaligned or duplicate snapshot page")
        pages[address] = data[offset + 4 : offset + 4 + PAGE]
    return pages


class Replay(Oracle):
    def __init__(self, name):
        super().__init__()
        self.meta_name = name
        self.meta = json.loads((SNAPSHOTS / f"{name}.json").read_text())
        validate_identity(self.meta, (SNAPSHOTS / f"{name}.pages").read_bytes())
        self.pages = read_pages(name)
        mapped = [(b, e) for b, e, _ in self.uc.mem_regions()]
        for address, page in self.pages.items():
            if not any(b <= address <= e for b, e in mapped):
                base = address & ~(REGION - 1)
                self.uc.mem_map(base, REGION)
                mapped.append((base, base + REGION - 1))
            self.uc.mem_write(address, page)
        self.heap_ptr = self.meta["heap_ptr"]
        self.hook_alloc()
        self.events = []
        self.checkpoints = None

    def checkpoint(self, functions, limit_per_function=None):
        """Record entry/exit states of `functions` (first calls of each).

        A write hook tracks dirty pages; a checkpoint stores every page that
        differs from the snapshot, plus registers, stack words and the heap
        pointer. Page contents are deduplicated when written out."""
        from unicorn import UC_HOOK_BLOCK, UC_HOOK_MEM_WRITE
        from unicorn import x86_const as X

        self.checkpoints = []
        self.dirty = set()
        self.blobs = {}
        pending = []
        seen = {}
        functions = set(functions)
        self.truncated_functions = set()

        def write(uc, access, address, size, value, user):
            self.dirty.add(address & ~(PAGE - 1))
            self.dirty.add((address + size - 1) & ~(PAGE - 1))

        def state():
            pages = {}
            for page in sorted(self.dirty):
                data = bytes(self.uc.mem_read(page, PAGE))
                if data != self.pages.get(page, bytes(PAGE)):
                    self.blobs.setdefault(data, len(self.blobs))
                    pages[page] = self.blobs[data]
            return pages

        def block(uc, address, size, user):
            esp = uc.reg_read(X.UC_X86_REG_ESP)
            while pending and address == pending[-1]["ret"] and esp > pending[-1]["esp"]:
                entry = pending.pop()
                depth = self._fpu_depth()
                self.checkpoints.append(
                    {
                        "leave": entry["index"],
                        "eax": uc.reg_read(X.UC_X86_REG_EAX),
                        "st0": self._read_st0_raw().hex() if depth else None,
                        "heap_ptr": self.heap_ptr,
                        "events": len(self.events),
                        "pages": state(),
                    }
                )
            if address in functions and limit_per_function is not None and seen.get(address, 0) >= limit_per_function:
                self.truncated_functions.add(address)
            if address in functions and (limit_per_function is None or seen.get(address, 0) < limit_per_function):
                seen[address] = seen.get(address, 0) + 1
                index = len(self.checkpoints)
                words = struct.unpack("<9I", bytes(uc.mem_read(esp, 36)))
                regs = {r: uc.reg_read(getattr(X, "UC_X86_REG_" + r.upper())) for r in ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp")}
                self.checkpoints.append(
                    {
                        "enter": address,
                        "index": index,
                        "registers": regs,
                        "stack": list(words[1:]),
                        "fpu_cw": uc.reg_read(X.UC_X86_REG_FPCW),
                        "heap_ptr": self.heap_ptr,
                        "events": len(self.events),
                        "pages": state(),
                    }
                )
                pending.append({"ret": words[0], "esp": esp, "index": index})

        self.uc.hook_add(UC_HOOK_MEM_WRITE, write)
        self.uc.hook_add(UC_HOOK_BLOCK, block)

    def write_checkpoints(self, scenario):
        """`<snapshot>.<scenario>.ckpt`: u32 JSON length, JSON, then blobs in
        the JSON's `blobs` order (4096 bytes each)."""
        order = sorted(self.blobs, key=self.blobs.get)
        rows = [{**c, "pages": sorted(c["pages"].items())} for c in self.checkpoints]
        header = json.dumps({"schema_version": 2, "snapshot_pages_sha256": self.meta["pages_sha256"],
            "snapshot_metadata_sha256": digest(SNAPSHOTS / f"{self.meta_name}.json"),
            "replay_payload_sha256": digest(SNAPSHOTS / f"{self.meta_name}.{scenario}.json"),
            "producers": replay_producers(), "comparison_policy": self.meta["comparison_policy"],
            "truncated_functions": sorted(self.truncated_functions),
            "checkpoints": rows, "blobs": len(order)}).encode()
        path = SNAPSHOTS / f"{self.meta_name}.{scenario}.ckpt"
        with open(path, "wb") as out:
            out.write(struct.pack("<I", len(header)) + header)
            for data in order:
                out.write(data)
        return path

    def resume(self, limit=200_000_000):
        """Continue the captured call exactly: same registers, stack and args;
        only its return address becomes the stop sentinel. Caller frames that
        heap objects may still link to remain intact below the entry ESP."""
        from oracle import SENTINEL, OracleError
        from unicorn import UcError
        from unicorn import x86_const as X

        uc = self.uc
        self.icount = 0
        self.limit = limit
        self._err = None
        self._mem_writes = []
        self.fpu_cw = self.meta["fpu_cw"]
        self._fpu_init()
        fpu = self.meta["fpu"]
        uc.reg_write(X.UC_X86_REG_FPCW, fpu["control"])
        uc.reg_write(X.UC_X86_REG_FPSW, fpu["status"])
        uc.reg_write(X.UC_X86_REG_FPTAG, fpu["tag"])
        for i, value in enumerate(fpu["registers"]): uc.reg_write(X.UC_X86_REG_FP0 + i, tuple(value))
        for register, value in self.meta["registers"].items():
            if register != "eip":
                self.set_reg(register, value)
        esp = self.meta["registers"]["esp"]
        uc.mem_write(esp, struct.pack("<I", SENTINEL))
        try:
            uc.emu_start(self.meta["function"], SENTINEL, count=0)
        except UcError as error:
            if self._err is None:
                eip = uc.reg_read(X.UC_X86_REG_EIP)
                self._err = OracleError(f"unicorn error {error} at eip={eip:#x}")
        if self._err is None and uc.reg_read(X.UC_X86_REG_EIP) != SENTINEL:
            self._err = OracleError(f"stopped early at eip={uc.reg_read(X.UC_X86_REG_EIP):#x}")
        if self._err is not None:
            raise self._err
        return uc.reg_read(X.UC_X86_REG_EAX)

    def record(self, kind, **fields):
        self.events.append({"kind": kind, **fields})

    def diff(self):
        """Every word changed relative to the snapshot, including fresh pages."""
        out = []
        entry_esp = self.meta["registers"]["esp"]
        for begin, end, _ in sorted(self.uc.mem_regions()):
            if 0x7F000000 <= begin <= 0x7FFFFFFF and not begin <= entry_esp <= end:
                continue  # thunks, TEB, GDT and the sentinel page
            if begin == 0x30000000:
                continue  # Oracle scratch arena: VM stub cells, not game state.
            for address in range(begin, end + 1, PAGE):
                now = bytes(self.uc.mem_read(address, PAGE))
                before = self.pages.get(address, bytes(PAGE))
                if now == before:
                    continue
                for i in range(0, PAGE, 4):
                    a = struct.unpack_from("<I", before, i)[0]
                    b = struct.unpack_from("<I", now, i)[0]
                    # Below the entry ESP is callee scratch, not retained state.
                    if a != b and not (begin <= entry_esp <= end and address + i < entry_esp + 4):
                        out.append([address + i, a, b])
        return out


def replay_producers():
    return {p: digest(REPO / p) for p in ["re/oracle/snapshot_replay.py", "re/oracle/part_install_replay.py", "re/oracle/oracle.py"]}


def write(name, scenario, payload):
    path = SNAPSHOTS / f"{name}.{scenario}.json"
    meta = json.loads((SNAPSHOTS / f"{name}.json").read_text())
    payload = {**payload, "schema_version": 2, "snapshot_pages_sha256": meta["pages_sha256"],
        "snapshot_metadata_sha256": digest(SNAPSHOTS / f"{name}.json"),
        "producers": replay_producers(), "comparison_policy": meta["comparison_policy"]}
    path.write_text(json.dumps(payload, sort_keys=True))
    return path
