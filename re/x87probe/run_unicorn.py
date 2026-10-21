"""Unicorn oracle runner for the M0-07 x87 probe corpus.

Executes each vector as a tiny 32-bit x86 stub under Unicorn 2.1.4:
sets the FPU control word from the vector's (PC, RC), loads operands as
exact 80-bit values via FLD tbyte, runs the probed instruction, reads back
ST(0) bytes / integer memory / flags exactly.

Stub layout (all addresses fixed):
  CODE  = 0x1000  instruction bytes under test
  LDSTUB= 0x0000  FLD tbyte [MEM80] (6 bytes: DB 2D 00 00 02 00)
  MEM80 = 0x20000 operand/result scratch (16 bytes)
  MEM32 = 0x21000 f32/fst/fistp scratch
  MEM64 = 0x22000 f64 scratch

Control word: mask 0x037F | PC<<8 | RC<<10 with all exceptions masked
(0x037F base = CW_DEFAULT-compatible; mirrors re/oracle/oracle.py).

Per-op stubs (loads push a-then-b so ST0=b(top), ST1=a unless noted):
  fadd/fsub/fmul/fdiv: D8 C1 (FADD ST0,ST1), D8 E9 (FSUBR ST0,ST1 = a-b),
    D8 C9 (FMUL ST0,ST1), D8 F9 (FDIVR ST0,ST1 = a/b). Result in ST0, no pop.
  fsqrt : D9 FA (ST0=sqrt(ST0), one operand)
  fsin  : D9 FE   fcos: D9 FF   fpatan: D9 F3 (ST1=atan2(ST1,ST0), pop;
    corpus fpatan(a=y, b=x): load a first then b => ST1=y, ST0=x)
  f2xm1 : D9 F0   fyl2x: D9 F1 (ST1=y*log2(x), pop; corpus fyl2x(a=x, b=y):
    load b first then a => ST1=y, ST0=x)
  fprem : D9 F8 + FSTSW AX captured (also C2 partial bit from FPSW;
    corpus (dividend a, divisor b): load b first then a => ST0=a, ST1=b)
  fcom  : D8 D1 (FCOM ST1) -> FNSTSW AX; fcomp adds pop (D8 D9)
  fucom : DD E1 (FUCOM ST1) -> FNSTSW AX
  fucomi: DB F1 (FCOMI ST0,ST1) / DF F1 (FUCOMI ST0,ST1) -> EFLAGS read directly
    (CF=C0, PF=C2, ZF=C3); corpus (a,b): load b first then a => ST0=a, ST1=b
  fld   : load only, no extra stub (result = loaded value)
  fst   : FST m64 [MEM64] (DD 15, no pop) + FSTP tbyte [MEM80] (DB 3D);
    reports m64 bytes + ST0
  fistp : FISTP m32 [MEM32] (DB 1D) + FISTP m64 [MEM64] (DF 3D); reports
    i32, i64 raw values.
"""
from __future__ import annotations

import json
import os
import struct
import sys

from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_PROT_ALL
from unicorn import x86_const as X

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.abspath(os.path.join(HERE, "..", "out", "x87"))
CORPUS = os.path.join(OUT_DIR, "corpus.jsonl")

CODE = 0x1000
LDSTUB = 0x0000
MEM80 = 0x20000
MEM32 = 0x21000
MEM64 = 0x22000

FLD_TBYTE = bytes([0xDB, 0x2D, 0x00, 0x00, 0x02, 0x00])  # fld tbyte [0x20000]
FNSTSW_AX = bytes([0xDF, 0xE0])

BIAS = 16383


def op_to_f80_bytes(o: dict) -> bytes:
    sign = o["sign"]
    sig = o["sig"] & 0xFFFFFFFFFFFFFFFF
    if isinstance(o.get("exp"), str):
        k = o["exp"]
        if k == "zero":
            return struct.pack("<Q", 0) + struct.pack("<H", sign << 15) + b"\x00" * 6
        if k == "inf":
            return struct.pack("<Q", 0x8000000000000000) + struct.pack("<H", 0x7FFF | (sign << 15)) + b"\x00" * 6
        # nan
        return struct.pack("<Q", sig) + struct.pack("<H", 0x7FFF | (sign << 15)) + b"\x00" * 6
    if "biased" in o:
        return struct.pack("<Q", sig) + struct.pack("<H", (o["biased"] & 0x7FFF) | (sign << 15)) + b"\x00" * 6
    biased = (o["exp"] + BIAS) & 0x7FFF
    return struct.pack("<Q", sig) + struct.pack("<H", biased | (sign << 15)) + b"\x00" * 6


STUBS: dict[str, bytes] = {
    # D8-group arithmetic (mod=11): result in ST0, no pop. reg selects the
    # operation direction; rm selects ST(i). With ST0=b, ST1=a after loading
    # a-then-b: C1=FADD ST0,ST1 (b+a); E9=FSUBR ST0,ST1 (a-b); C9=FMUL
    # ST0,ST1 (b*a); F9=FDIVR ST0,ST1 (a/b). All commutative-safe choices
    # give a OP b in ST0.
    "fadd": bytes([0xD8, 0xC1]),   # FADD ST0,ST1: ST0=ST0+ST1 (no pop)
    "fsub": bytes([0xD8, 0xE9]),   # FSUBR ST0,ST1: ST0=ST1-ST0 = a-b (no pop)
    "fmul": bytes([0xD8, 0xC9]),   # FMUL ST1,ST0: ST1=ST1*ST0 (no pop)
    "fdiv": bytes([0xD8, 0xF9]),   # FDIVR ST0,ST1: ST0=ST1/ST0 = a/b (no pop)
    "fsqrt": bytes([0xD9, 0xFA]),
    "fsin": bytes([0xD9, 0xFE]),
    "fcos": bytes([0xD9, 0xFF]),
    "fpatan": bytes([0xD9, 0xF3]),
    "f2xm1": bytes([0xD9, 0xF0]),
    "fyl2x": bytes([0xD9, 0xF1]),
    "fprem": bytes([0xD9, 0xF8]) + FNSTSW_AX,
    "fcom": bytes([0xD8, 0xD1]) + FNSTSW_AX,
    "fcomp": bytes([0xD8, 0xD9]) + FNSTSW_AX,
    "fucom": bytes([0xDD, 0xE1]) + FNSTSW_AX,
    "fucomi_st": bytes([0xDB, 0xF1]),   # FCOMI ST0,ST1
    "fucomi_st1": bytes([0xDF, 0xF1]),  # FUCOMI ST0,ST1
    "fld": b"",
    "fst_m64": bytes([0xDD, 0x15, 0x00, 0x20, 0x02, 0x00]),   # fst m64 [MEM64] (no pop)
    "fstp_m80": bytes([0xDB, 0x3D, 0x00, 0x00, 0x02, 0x00]),  # fstp tbyte [MEM80]
    "fistp_m32": bytes([0xDB, 0x1D, 0x00, 0x10, 0x02, 0x00]),  # fistp m32 [MEM32]
    "fistp_m64": bytes([0xDF, 0x3D, 0x00, 0x20, 0x02, 0x00]),  # fistp m64 [MEM64] (DF /3, disp32 MEM64)
}

PC_BITS = {24: 0b00, 53: 0b10, 64: 0b11}


def make_uc(pc: int, rc: int) -> Uc:
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    uc.mem_map(0x0, 0x1000, UC_PROT_ALL)
    uc.mem_map(CODE, 0x1000, UC_PROT_ALL)
    uc.mem_map(MEM80, 0x1000, UC_PROT_ALL)
    uc.mem_map(MEM32, 0x1000, UC_PROT_ALL)
    uc.mem_map(MEM64, 0x1000, UC_PROT_ALL)
    cw = 0x037F | (PC_BITS[pc] << 8) | ((rc & 3) << 10)
    uc.reg_write(X.UC_X86_REG_FPCW, cw)
    uc.reg_write(X.UC_X86_REG_FPSW, 0)
    uc.reg_write(X.UC_X86_REG_FPTAG, 0xFFFF)
    uc.reg_write(X.UC_X86_REG_ESP, MEM64 + 0x800)
    uc.reg_write(X.UC_X86_REG_EAX, 0)
    uc.mem_write(LDSTUB, FLD_TBYTE)
    return uc


def push80(uc: Uc, data: bytes) -> None:
    uc.mem_write(MEM80, data + b"\x00" * (16 - len(data)))
    uc.reg_write(X.UC_X86_REG_EIP, LDSTUB)
    uc.emu_start(LDSTUB, LDSTUB + len(FLD_TBYTE))


def run_stub(uc: Uc, stub: bytes) -> None:
    if not stub:
        return
    uc.mem_write(CODE, stub)
    uc.reg_write(X.UC_X86_REG_EIP, CODE)
    uc.emu_start(CODE, CODE + len(stub))


def read_st0(uc: Uc):
    top = (uc.reg_read(X.UC_X86_REG_FPSW) >> 11) & 7
    mant, exp = uc.reg_read(X.UC_X86_REG_FP0 + top)
    return top, mant & 0xFFFFFFFFFFFFFFFF, exp & 0xFFFF


def st_depth(uc: Uc) -> int:
    tag = uc.reg_read(X.UC_X86_REG_FPTAG) & 0xFFFF
    fpsw = uc.reg_read(X.UC_X86_REG_FPSW)
    top = (fpsw >> 11) & 7
    # count non-empty tags; unicorn maintains FPTAG on push/pop
    return sum(1 for i in range(8) if ((tag >> (2 * i)) & 3) != 3)


# ops whose x87 semantics are ST1 OP ST0 with the vector's a in ST1:
# load a first, then b (ST0=b). fpatan/fyl2x/fprem/compares: SDM order is
# (ST1, ST0) = (y/dividend/a?, ...) -- we define corpus (a,b) so that the
# natural reading holds: fpatan(a= y-part? ...). Document precisely:
#   fpatan: ST1 = ST1 + atan(ST1/ST0)? No: FPATAN computes
#     ST1 = atan2(ST1, ST0) i.e. atan(ST1/ST0) with quadrant of (ST1,ST0),
#     then pops. Corpus emits fpatan(y, x): load y first then x => ST1=y,
#     ST0=x => atan2(y,x). Natural.
#   fyl2x: ST1 = ST1 * log2(ST0): corpus emits (x, y) with a=x (log arg),
#     b=y: load b first then a => ST1=y, ST0=x.
#   fprem: ST0 = ST0 rem ST1: corpus (dividend a, divisor b): load a then b
#     gives ST0=b -- WRONG. Load b first then a => ST0=a, ST1=b. Reversed too.
#   compares FCOM ST1: compares ST0 with ST1 (ST0 - ST1 sign). Corpus (a,b):
#     to get "a vs b" load b first then a => ST0=a, ST1=b.


def run_vector(uc: Uc, v: dict) -> dict:
    op = v["op"]
    a = op_to_f80_bytes(v["a"])
    b = op_to_f80_bytes(v["b"]) if v.get("b") is not None else None
    out: dict = {"id": v["id"], "op": op, "pc": v["pc"], "rc": v["rc"]}
    try:
        if op in ("fadd", "fsub", "fmul", "fdiv"):
            push80(uc, a)
            push80(uc, b)
            run_stub(uc, STUBS[op])
            # D8-group result lands in ST0: physical register TOP.
            top = (uc.reg_read(X.UC_X86_REG_FPSW) >> 11) & 7
            mant, exp = uc.reg_read(X.UC_X86_REG_FP0 + top)
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op in ("fsqrt", "fsin", "fcos", "f2xm1"):
            push80(uc, a)
            run_stub(uc, STUBS[op])
            top, mant, exp = read_st0(uc)
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fyl2x":
            # ST1=y=b, ST0=x=a -> ST1 = y*log2(x). Load b first, then a.
            push80(uc, b)
            push80(uc, a)
            run_stub(uc, STUBS[op])
            top, mant, exp = read_st0(uc)
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fpatan":
            # ST1=y=a, ST0=x=b -> ST1 = atan2(y, x). Load a first, then b.
            push80(uc, a)
            push80(uc, b)
            run_stub(uc, STUBS[op])
            top, mant, exp = read_st0(uc)
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fprem":
            push80(uc, b)
            push80(uc, a)
            run_stub(uc, STUBS[op])
            top, mant, exp = read_st0(uc)
            fpsw = uc.reg_read(X.UC_X86_REG_FPSW)
            ax = uc.reg_read(X.UC_X86_REG_EAX) & 0xFFFF
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{fpsw:04x}"
            out["ax"] = f"0x{ax:04x}"
            out["C2"] = (fpsw >> 10) & 1
            out["C0"] = (fpsw >> 8) & 1
            out["C3"] = (fpsw >> 14) & 1
            out["C1"] = (fpsw >> 9) & 1
        elif op in ("fcom", "fcomp", "fucom"):
            push80(uc, b)
            push80(uc, a)
            run_stub(uc, STUBS[op])
            ax = uc.reg_read(X.UC_X86_REG_EAX) & 0xFFFF
            out["ax"] = f"0x{ax:04x}"
            out["cc"] = {"C3": (ax >> 14) & 1, "C2": (ax >> 10) & 1, "C0": (ax >> 8) & 1}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fucomi":
            push80(uc, b)
            push80(uc, a)
            # try FUCOMI (DF F1); fall back to FCOMI (DB F1) if it faults
            try:
                run_stub(uc, STUBS["fucomi_st1"])
                out["variant"] = "fucomi"
            except Exception:
                uc2 = make_uc(v["pc"], v["rc"])
                push80(uc2, b)
                push80(uc2, a)
                run_stub(uc2, STUBS["fucomi_st"])
                uc = uc2
                out["variant"] = "fcomi-fallback"
            ef = uc.reg_read(X.UC_X86_REG_EFLAGS)
            out["eflags"] = f"0x{ef:08x}"
            out["cc"] = {"CF": (ef >> 0) & 1, "PF": (ef >> 2) & 1, "ZF": (ef >> 6) & 1}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fld":
            push80(uc, a)
            top, mant, exp = read_st0(uc)
            out["st"] = {"sign": (exp >> 15) & 1, "biased": exp & 0x7FFF, "sig": f"0x{mant:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fst":
            push80(uc, a)
            run_stub(uc, STUBS["fst_m64"])
            m64 = bytes(uc.mem_read(MEM64, 8))
            # reload for the m80 store (fst m64 does not pop)
            run_stub(uc, STUBS["fstp_m80"])
            m80 = bytes(uc.mem_read(MEM80, 10))
            mant80 = struct.unpack("<Q", m80[:8])[0]
            exp80 = struct.unpack("<H", m80[8:10])[0]
            out["m64"] = "0x" + m64.hex()
            out["st"] = {"sign": (exp80 >> 15) & 1, "biased": exp80 & 0x7FFF, "sig": f"0x{mant80:016x}"}
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        elif op == "fistp":
            push80(uc, a)
            run_stub(uc, STUBS["fistp_m32"])
            i32 = bytes(uc.mem_read(MEM32, 4))
            # fresh instance for the m64 store (fistp pops)
            uc2 = make_uc(v["pc"], v["rc"])
            push80(uc2, a)
            run_stub(uc2, STUBS["fistp_m64"])
            i64 = bytes(uc2.mem_read(MEM64, 8))
            out["i32"] = struct.unpack("<i", i32)[0]
            out["i32_hex"] = "0x" + i32.hex()
            out["i64"] = struct.unpack("<q", i64)[0]
            out["i64_hex"] = "0x" + i64.hex()
            out["fpsw"] = f"0x{uc.reg_read(X.UC_X86_REG_FPSW):04x}"
        else:
            out["error"] = f"unknown op {op}"
    except Exception as e:  # noqa: BLE001 - record emulator faults as data
        out["uc_fault"] = f"{type(e).__name__}: {e}"
    return out


def main() -> None:
    corpus_path = sys.argv[1] if len(sys.argv) > 1 else CORPUS
    out_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(OUT_DIR, "uc_results.jsonl")
    n = 0
    with open(corpus_path) as fin, open(out_path, "w") as fout:
        for line in fin:
            v = json.loads(line)
            uc = make_uc(v["pc"], v["rc"])
            fout.write(json.dumps(run_vector(uc, v)) + "\n")
            n += 1
            if n % 2000 == 0:
                print(f"  {n} ...", flush=True)
    print(f"wrote {n} results -> {out_path}")


if __name__ == "__main__":
    main()
