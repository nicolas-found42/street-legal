#!/usr/bin/env python3
"""Unicorn-based single-function oracle for StreetLegal_Redline.bak.exe (32-bit x86 PE).

Library:
    from oracle import Oracle
    o = Oracle()                                  # loads the PE, sets up stack/heap/TEB
    o.write_u32(addr, 5); o.write_f32s(addr, [1.0, 2.0])
    r = o.call(0x41a5c0, convention="thiscall", this=addr)
    r.eax, r.edx_eax, r.st0, r.icount

CLI:
    python oracle.py call 0x41a5c0 --conv thiscall --this 0x20000000 --args 1 2.5f
(see README.md for details; use run_vectors.py for batch runs)

Never writes to the reference directory; the exe is only read.
"""
from __future__ import annotations

import argparse
import os
import struct
import sys
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional, Sequence, Tuple, Union

import pefile
from capstone import CS_ARCH_X86, CS_MODE_32, Cs
from unicorn import (
    UC_ARCH_X86, UC_HOOK_CODE, UC_HOOK_MEM_FETCH_UNMAPPED, UC_HOOK_MEM_READ_UNMAPPED,
    UC_HOOK_MEM_WRITE_UNMAPPED, UC_MODE_32, UC_PROT_ALL, Uc, UcError,
)
from unicorn import x86_const as X

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_EXE = os.path.join(REPO, "reference", "raw", "StreetLegal_Redline.bak.exe")

# Memory layout (outside the image 0x400000..0x7c0000)
STACK_TOP = 0x7FF00000
STACK_SIZE = 0x100000
TEB_BASE = 0x7FFDE000
GDT_BASE = 0x7FFD0000
SENTINEL = 0x7FFF0000          # return address; reaching it stops emulation
THUNK_BASE = 0x7F000000        # import thunks (one 16-byte slot each)
THUNK_SIZE = 0x10000
HEAP_BASE = 0x20000000
HEAP_SIZE = 0x04000000
SCRATCH_BASE = 0x30000000      # generic scratch arena for tests
SCRATCH_SIZE = 0x00100000

# CW defaults: 0x27f = 53-bit, round-nearest, all exceptions masked (MSVC/Windows process default).
CW_DEFAULT = 0x027F
CW_D3D9 = 0x007F               # 24-bit precision (D3D9 device without FPU_PRESERVE)

Arg = Union[int, float, Tuple[str, Union[int, float]]]

# stdcall arg bytes for imports commonly reached from leaf code (dll-less names). Unknown -> 0.
IMPORT_STDCALL_BYTES = {
    "QueryPerformanceCounter": 4, "QueryPerformanceFrequency": 4, "GetTickCount": 0,
    "timeGetTime": 0, "GetLastError": 0, "SetLastError": 4, "EnterCriticalSection": 4,
    "LeaveCriticalSection": 4, "InitializeCriticalSection": 4, "DeleteCriticalSection": 4,
    "GetCurrentThreadId": 0, "GetProcessHeap": 0, "HeapAlloc": 12, "HeapFree": 12,
    "Sleep": 4, "GetModuleHandleA": 4, "GetProcAddress": 8, "lstrlenA": 4,
}


class OracleError(RuntimeError):
    pass


class UnmappedAccess(OracleError):
    pass


class InstructionLimit(OracleError):
    pass


@dataclass
class CallResult:
    eax: int
    edx: int
    st0: Optional[float]          # float64 view of ST0 (None if FPU stack empty)
    st0_raw: Optional[bytes]      # 10-byte x87 extended value of ST0
    icount: int
    esp_delta: int                # esp after return minus esp before pushing args+retaddr (callee-cleaned bytes)
    fpu_top_depth: int            # number of live FPU stack slots at return
    fpcw: int
    fpsw: int
    mem_writes: List[Tuple[int, int, int]] = field(default_factory=list)

    @property
    def edx_eax(self) -> int:
        return (self.edx << 32) | self.eax

    @property
    def eax_s(self) -> int:
        return self.eax - (1 << 32) if self.eax & 0x80000000 else self.eax

    @property
    def eax_f32(self) -> float:
        return struct.unpack("<f", struct.pack("<I", self.eax))[0]


def _page(n: int) -> int:
    return (n + 0xFFF) & ~0xFFF


class Oracle:
    def __init__(self, exe: str = DEFAULT_EXE, fpu_cw: int = CW_DEFAULT, stub_imports: bool = True,
                 trace: bool = False):
        self.exe = exe
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.fpu_cw = fpu_cw
        self.trace = trace
        self.hooks: Dict[int, Tuple[Callable, str, int]] = {}
        self.import_names: Dict[int, str] = {}      # thunk VA -> "dll!name"
        self.import_log: List[str] = []
        self.heap_ptr = HEAP_BASE
        self.heap_sizes: Dict[int, int] = {}
        self.icount = 0
        self.limit = 0
        self._err: Optional[Exception] = None
        self._cs = Cs(CS_ARCH_X86, CS_MODE_32)
        self._load_pe(stub_imports)
        self._setup_regions()
        self._setup_teb()
        self.uc.hook_add(UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED | UC_HOOK_MEM_FETCH_UNMAPPED,
                         self._on_unmapped)
        self.uc.hook_add(UC_HOOK_CODE, self._on_code)

    # ---------------------------------------------------------------- loading
    def _load_pe(self, stub_imports: bool):
        data = open(self.exe, "rb").read()          # read only
        pe = pefile.PE(data=data, fast_load=False)
        self.pe = pe
        self.base = pe.OPTIONAL_HEADER.ImageBase
        self.image_size = _page(pe.OPTIONAL_HEADER.SizeOfImage)
        self.uc.mem_map(self.base, self.image_size, UC_PROT_ALL)   # zero-filled (covers .bss tails)
        hdr = pe.OPTIONAL_HEADER.SizeOfHeaders
        self.uc.mem_write(self.base, data[:hdr])
        self.sections = []
        for s in pe.sections:
            va = self.base + s.VirtualAddress
            raw = s.get_data()[: s.Misc_VirtualSize] if s.SizeOfRawData else b""
            raw = data[s.PointerToRawData: s.PointerToRawData + min(s.SizeOfRawData, s.Misc_VirtualSize or s.SizeOfRawData)]
            if raw:
                self.uc.mem_write(va, raw)
            self.sections.append((s.Name.rstrip(b"\0").decode(), va, s.Misc_VirtualSize, s.SizeOfRawData))
        self.text_range = None
        for n, va, vs, rs in self.sections:
            if n == ".text":
                self.text_range = (va, va + vs)
        self.uc.mem_map(THUNK_BASE, THUNK_SIZE, UC_PROT_ALL)
        if stub_imports:
            self._stub_imports()

    def _stub_imports(self):
        """Point every IAT slot at a private thunk `xor eax,eax; ret N` (return 0)."""
        slot = 0
        for ent in getattr(self.pe, "DIRECTORY_ENTRY_IMPORT", []):
            dll = ent.dll.decode().lower()
            for imp in ent.imports:
                name = imp.name.decode() if imp.name else f"ord{imp.ordinal}"
                n = IMPORT_STDCALL_BYTES.get(name, 0)
                tva = THUNK_BASE + slot * 16
                code = b"\x31\xc0" + (b"\xc2" + struct.pack("<H", n) if n else b"\xc3")
                self.uc.mem_write(tva, code)
                self.uc.mem_write(imp.address, struct.pack("<I", tva))   # imp.address is a VA
                self.import_names[tva] = f"{dll}!{name}"
                slot += 1
        self._thunk_slots = slot

    def _setup_regions(self):
        self.uc.mem_map(STACK_TOP - STACK_SIZE, STACK_SIZE + 0x1000, UC_PROT_ALL)
        self.uc.mem_map(HEAP_BASE, HEAP_SIZE, UC_PROT_ALL)
        self.uc.mem_map(SCRATCH_BASE, SCRATCH_SIZE, UC_PROT_ALL)
        self.uc.mem_map(SENTINEL, 0x1000, UC_PROT_ALL)
        self.uc.mem_write(SENTINEL, b"\xf4")        # hlt (never reached; we stop at SENTINEL)
        self.scratch_ptr = SCRATCH_BASE

    def _setup_teb(self):
        uc = self.uc
        uc.mem_map(TEB_BASE, 0x2000, UC_PROT_ALL)
        uc.mem_map(GDT_BASE, 0x1000, UC_PROT_ALL)
        # TEB: [0]=SEH chain head (-1), [4]=stack base, [8]=stack limit, [0x18]=self
        uc.mem_write(TEB_BASE, struct.pack("<IIII", 0xFFFFFFFF, STACK_TOP, STACK_TOP - STACK_SIZE, 0))
        uc.mem_write(TEB_BASE + 0x18, struct.pack("<I", TEB_BASE))
        # GDT: null, flat code (0x08), flat data (0x10), fs (0x18, base=TEB)
        def ent(base, limit, access, flags):
            return struct.pack("<HHBBBB", limit & 0xFFFF, base & 0xFFFF, (base >> 16) & 0xFF, access,
                               ((flags & 0xF) << 4) | ((limit >> 16) & 0xF), (base >> 24) & 0xFF)
        gdt = b"\0" * 8 + ent(0, 0xFFFFF, 0x9B, 0xC) + ent(0, 0xFFFFF, 0x93, 0xC) + ent(TEB_BASE, 0xFFF, 0x93, 0x4)
        uc.mem_write(GDT_BASE, gdt)
        uc.reg_write(X.UC_X86_REG_GDTR, (0, GDT_BASE, len(gdt) - 1, 0))
        uc.reg_write(X.UC_X86_REG_CS, 0x08)
        uc.reg_write(X.UC_X86_REG_SS, 0x10)
        uc.reg_write(X.UC_X86_REG_DS, 0x10)
        uc.reg_write(X.UC_X86_REG_ES, 0x10)
        uc.reg_write(X.UC_X86_REG_GS, 0x10)
        uc.reg_write(X.UC_X86_REG_FS, 0x18)

    # ---------------------------------------------------------------- memory
    def read(self, addr: int, n: int) -> bytes:
        try:
            return bytes(self.uc.mem_read(addr, n))
        except UcError as e:
            raise UnmappedAccess(f"read of {n} bytes at {addr:#x} unmapped") from e

    def write(self, addr: int, data: bytes):
        try:
            self.uc.mem_write(addr, bytes(data))
        except UcError as e:
            raise UnmappedAccess(f"write of {len(data)} bytes at {addr:#x} unmapped") from e

    def read_u8(self, a): return self.read(a, 1)[0]
    def read_u16(self, a): return struct.unpack("<H", self.read(a, 2))[0]
    def read_u32(self, a): return struct.unpack("<I", self.read(a, 4))[0]
    def read_i32(self, a): return struct.unpack("<i", self.read(a, 4))[0]
    def read_f32(self, a): return struct.unpack("<f", self.read(a, 4))[0]
    def read_f64(self, a): return struct.unpack("<d", self.read(a, 8))[0]
    def read_u32s(self, a, n): return list(struct.unpack(f"<{n}I", self.read(a, 4 * n)))
    def read_f32s(self, a, n): return list(struct.unpack(f"<{n}f", self.read(a, 4 * n)))
    def read_f64s(self, a, n): return list(struct.unpack(f"<{n}d", self.read(a, 8 * n)))
    def read_cstr(self, a, maxn=4096):
        out = b""
        while len(out) < maxn:
            c = self.read(a + len(out), 1)
            if c == b"\0":
                break
            out += c
        return out

    def write_u8(self, a, v): self.write(a, struct.pack("<B", v & 0xFF))
    def write_u16(self, a, v): self.write(a, struct.pack("<H", v & 0xFFFF))
    def write_u32(self, a, v): self.write(a, struct.pack("<I", v & 0xFFFFFFFF))
    def write_i32(self, a, v): self.write(a, struct.pack("<i", v))
    def write_f32(self, a, v): self.write(a, struct.pack("<f", v))
    def write_f64(self, a, v): self.write(a, struct.pack("<d", v))
    def write_u32s(self, a, vs): self.write(a, struct.pack(f"<{len(vs)}I", *[v & 0xFFFFFFFF for v in vs]))
    def write_f32s(self, a, vs): self.write(a, struct.pack(f"<{len(vs)}f", *vs))
    def write_f64s(self, a, vs): self.write(a, struct.pack(f"<{len(vs)}d", *vs))

    def alloc(self, n: int, align: int = 16) -> int:
        """Heap bump allocation (zeroed, never reused)."""
        p = (self.heap_ptr + align - 1) & ~(align - 1)
        if p + n > HEAP_BASE + HEAP_SIZE:
            raise OracleError("oracle heap exhausted")
        self.heap_ptr = p + n
        self.heap_sizes[p] = n
        return p

    def scratch(self, n: int, align: int = 16) -> int:
        """Allocate from the scratch arena (for test inputs)."""
        p = (self.scratch_ptr + align - 1) & ~(align - 1)
        if p + n > SCRATCH_BASE + SCRATCH_SIZE:
            raise OracleError("scratch exhausted")
        self.scratch_ptr = p + n
        return p

    def disasm(self, va: int, n: int = 16):
        return list(self._cs.disasm(self.read(va, n * 8), va))[:n]

    # ---------------------------------------------------------------- hooks
    def hook(self, va: int, fn: Callable, convention: str = "cdecl", nargs: int = 0):
        """Replace the function at `va` by a Python callback.

        fn(oracle, args) -> int | float | None ; args = list of `nargs` u32 stack words (thiscall: args exclude
        this; use oracle.reg('ecx')). Integer results go to EAX (negative ok); a Python float is returned in
        ST0 (pushed on the FPU stack); None leaves EAX unchanged. Stack is cleaned for stdcall/thiscall/fastcall(+stack args).
        """
        self.hooks[va] = (fn, convention, nargs)

    def hook_alloc(self, malloc_va=0x54F560, free_va=0x54F5B0):
        """Install the game's allocator wrappers as bump-allocator hooks (zeroed memory)."""
        self.hook(malloc_va, lambda o, a: (o.alloc(a[0]) if a[0] >= 1 else 0), "cdecl", 1)
        self.hook(free_va, lambda o, a: None, "cdecl", 1)

    def reg(self, name: str) -> int:
        return self.uc.reg_read(getattr(X, "UC_X86_REG_" + name.upper()))

    def set_reg(self, name: str, v: int):
        self.uc.reg_write(getattr(X, "UC_X86_REG_" + name.upper()), v & 0xFFFFFFFF)

    def _on_unmapped(self, uc, access, addr, size, value, ud):
        from unicorn import UC_MEM_FETCH_UNMAPPED, UC_MEM_READ_UNMAPPED, UC_MEM_WRITE_UNMAPPED
        k = {UC_MEM_READ_UNMAPPED: "read", UC_MEM_WRITE_UNMAPPED: "write", UC_MEM_FETCH_UNMAPPED: "fetch"}.get(access, "?")
        eip = uc.reg_read(X.UC_X86_REG_EIP)
        self._err = UnmappedAccess(f"unmapped {k} of {size} bytes at {addr:#x} (eip={eip:#x}, "
                                   f"esp={uc.reg_read(X.UC_X86_REG_ESP):#x})")
        uc.emu_stop()
        return False

    def _on_code(self, uc, address, size, ud):
        self.icount += 1
        if self.limit and self.icount > self.limit:
            self._err = InstructionLimit(f"instruction limit {self.limit} exceeded at eip={address:#x}")
            uc.emu_stop()
            return
        if self.trace:
            for i in self._cs.disasm(bytes(uc.mem_read(address, size)), address):
                print(f"  {i.address:#x}: {i.mnemonic} {i.op_str}")
        if address in self.hooks:
            self._run_hook(address)
        elif address in self.import_names:
            self.import_log.append(self.import_names[address])

    def _run_hook(self, va):
        uc = self.uc
        fn, conv, nargs = self.hooks[va]
        esp = uc.reg_read(X.UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        words = list(struct.unpack(f"<{nargs}I", bytes(uc.mem_read(esp + 4, 4 * nargs)))) if nargs else []
        res = fn(self, words)
        cleanup = 0 if conv == "cdecl" else 4 * nargs
        if conv == "thiscall" or conv == "stdcall":
            cleanup = 4 * nargs
        uc.reg_write(X.UC_X86_REG_ESP, esp + 4 + cleanup)
        if isinstance(res, float):
            self._fpu_push(res)
        elif res is not None:
            uc.reg_write(X.UC_X86_REG_EAX, res & 0xFFFFFFFF)
        uc.reg_write(X.UC_X86_REG_EIP, ret)

    # ---------------------------------------------------------------- FPU helpers
    def _fpu_init(self):
        uc = self.uc
        uc.reg_write(X.UC_X86_REG_FPCW, self.fpu_cw)
        uc.reg_write(X.UC_X86_REG_FPSW, 0)
        uc.reg_write(X.UC_X86_REG_FPTAG, 0xFFFF)

    def _fpu_depth(self) -> int:
        tag = self.uc.reg_read(X.UC_X86_REG_FPTAG) & 0xFFFF
        return sum(1 for i in range(8) if ((tag >> (2 * i)) & 3) != 3)

    def _fpu_top(self) -> int:
        return (self.uc.reg_read(X.UC_X86_REG_FPSW) >> 11) & 7

    def _fpu_push(self, v: float):
        """Push a float64 on the x87 stack by direct register writes."""
        import math
        uc = self.uc
        top = (self._fpu_top() - 1) & 7
        sw = (uc.reg_read(X.UC_X86_REG_FPSW) & ~0x3800) | (top << 11)
        uc.reg_write(X.UC_X86_REG_FPSW, sw)
        tag = uc.reg_read(X.UC_X86_REG_FPTAG) & 0xFFFF & ~(3 << (2 * top))
        uc.reg_write(X.UC_X86_REG_FPTAG, tag)
        if v == 0:
            mant, exp = 0, (0x8000 if math.copysign(1, v) < 0 else 0)
        else:
            m, e = math.frexp(abs(v))
            mant = int(m * 2**64) & ((1 << 64) - 1) | (1 << 63)
            exp = (e - 1 + 16383) | (0x8000 if v < 0 else 0)
        uc.reg_write(X.UC_X86_REG_FP0 + top, (mant, exp))

    # ---------------------------------------------------------------- calling
    def call(self, va: int, args: Sequence[Arg] = (), convention: str = "cdecl", this: Optional[int] = None,
             limit: int = 5_000_000, fpu_cw: Optional[int] = None, regs: Optional[Dict[str, int]] = None,
             float_ret: bool = False) -> CallResult:
        """Call function at `va`.

        args: ints (pushed as u32, negative ok), floats (f32, 4 bytes), ("f32"|"f64"|"u32"|"i32", v).
        convention: cdecl | stdcall | thiscall (ECX=this) | fastcall (first two int args in ECX, EDX).
        """
        uc = self.uc
        self.icount = 0
        self.limit = limit
        self._err = None
        if fpu_cw is not None:
            self.fpu_cw = fpu_cw
        self._fpu_init()
        words: List[bytes] = []
        reg_args: List[int] = []
        for a in args:
            if isinstance(a, tuple):
                kind, v = a
            elif isinstance(a, float):
                kind, v = "f32", a
            else:
                kind, v = "u32", a
            if kind == "f32":
                b = struct.pack("<f", v)
            elif kind == "f64":
                b = struct.pack("<d", v)
            elif kind in ("u32", "i32", "int"):
                b = struct.pack("<I", v & 0xFFFFFFFF)
            else:
                raise OracleError(f"bad arg kind {kind}")
            if convention == "fastcall" and kind in ("u32", "i32", "int") and len(reg_args) < 2:
                reg_args.append(v & 0xFFFFFFFF)
            else:
                words.append(b)
        for r in ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp"):
            self.set_reg(r, 0)
        if regs:
            for k, v in regs.items():
                self.set_reg(k, v)
        if convention == "thiscall":
            if this is None:
                raise OracleError("thiscall needs this=")
            self.set_reg("ecx", this)
        elif convention == "fastcall":
            if reg_args:
                self.set_reg("ecx", reg_args[0])
            if len(reg_args) > 1:
                self.set_reg("edx", reg_args[1])
        blob = b"".join(words)
        esp0 = STACK_TOP - 0x100
        esp = (esp0 - len(blob) - 4) & ~0xF
        esp -= 4  # keep (esp+4) 16-aligned at entry like a real call
        uc.mem_write(esp, struct.pack("<I", SENTINEL) + blob)
        uc.reg_write(X.UC_X86_REG_ESP, esp)
        uc.reg_write(X.UC_X86_REG_EBP, 0)
        entry_esp = esp
        try:
            uc.emu_start(va, SENTINEL, count=0)
        except UcError as e:
            if self._err is None:
                eip = uc.reg_read(X.UC_X86_REG_EIP)
                self._err = OracleError(f"unicorn error {e} at eip={eip:#x}")
        if self._err is None and uc.reg_read(X.UC_X86_REG_EIP) != SENTINEL:
            self._err = OracleError(f"emulation stopped early at eip={uc.reg_read(X.UC_X86_REG_EIP):#x}")
        if self._err is not None:
            raise self._err
        esp_end = uc.reg_read(X.UC_X86_REG_ESP)
        depth = self._fpu_depth()
        st0 = raw = None
        if depth:
            raw = self._read_st0_raw()
            st0 = _ext_to_float(raw)
        return CallResult(
            eax=uc.reg_read(X.UC_X86_REG_EAX), edx=uc.reg_read(X.UC_X86_REG_EDX), st0=st0, st0_raw=raw,
            icount=self.icount, esp_delta=esp_end - (entry_esp + 4), fpu_top_depth=depth,
            fpcw=uc.reg_read(X.UC_X86_REG_FPCW), fpsw=uc.reg_read(X.UC_X86_REG_FPSW))

    def _read_st0_raw(self) -> bytes:
        mant, exp = self.uc.reg_read(X.UC_X86_REG_FP0 + self._fpu_top())   # (mantissa:u64, sign|exp:u16)
        return struct.pack("<QH", mant, exp)


def _ext_to_float(raw: bytes) -> float:
    mant, se = struct.unpack("<QH", raw)
    sign = -1.0 if se & 0x8000 else 1.0
    e = se & 0x7FFF
    if e == 0 and mant == 0:
        return sign * 0.0
    if e == 0x7FFF:
        return sign * float("inf") if (mant << 1) & ((1 << 64) - 1) == 0 else float("nan")
    import math
    return sign * math.ldexp(mant, e - 16383 - 63)


def ext_bytes_to_fraction(raw: bytes):
    from fractions import Fraction
    mant, se = struct.unpack("<QH", raw)
    sign = -1 if se & 0x8000 else 1
    e = se & 0x7FFF
    if e == 0x7FFF:
        return None
    if e == 0:
        e = 1
    return sign * Fraction(mant) * Fraction(2) ** (e - 16383 - 63)


# ------------------------------------------------------------------ CLI
def _parse_arg(s: str) -> Arg:
    if s.endswith("f") and not s.startswith("0x"):
        return float(s[:-1])
    if s.endswith("d"):
        return ("f64", float(s[:-1]))
    return int(s, 0)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("call", help="call a function once")
    c.add_argument("va", type=lambda s: int(s, 0))
    c.add_argument("--conv", default="cdecl", choices=["cdecl", "stdcall", "thiscall", "fastcall"])
    c.add_argument("--this", type=lambda s: int(s, 0))
    c.add_argument("--args", nargs="*", default=[], help="ints, 1.5f (f32), 1.5d (f64)")
    c.add_argument("--cw", type=lambda s: int(s, 0), default=CW_DEFAULT)
    c.add_argument("--limit", type=int, default=5_000_000)
    c.add_argument("--exe", default=DEFAULT_EXE)
    c.add_argument("--alloc-hooks", action="store_true")
    d = sub.add_parser("disasm", help="disassemble at VA")
    d.add_argument("va", type=lambda s: int(s, 0))
    d.add_argument("-n", type=int, default=20)
    d.add_argument("--exe", default=DEFAULT_EXE)
    a = ap.parse_args(argv)
    if a.cmd == "disasm":
        o = Oracle(a.exe)
        for i in o.disasm(a.va, a.n):
            print(f"{i.address:#x}: {i.mnemonic} {i.op_str}")
        return 0
    o = Oracle(a.exe, fpu_cw=a.cw)
    if a.alloc_hooks:
        o.hook_alloc()
    r = o.call(a.va, [_parse_arg(x) for x in a.args], a.conv, this=a.this, limit=a.limit)
    print(f"eax={r.eax:#010x} edx={r.edx:#010x} st0={r.st0!r} icount={r.icount} esp_delta={r.esp_delta}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
