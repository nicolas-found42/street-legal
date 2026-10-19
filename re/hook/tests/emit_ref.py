"""Reference emitter from docs/call-log.md section 5 (independent of slrlog.h).

Writes the exact same logical log as emit_test.c; run_tests.sh requires
byte equality and that the Rust reader accepts both.
"""
import struct, sys

out = open(sys.argv[1], "wb")
out.write(b"SLRLOG" + struct.pack("<H", 1))


def record(tag, body):
    out.write(struct.pack("B", tag) + struct.pack("<I", len(body)) + body)


def cstr(s):
    b = s.encode("utf-8")
    return struct.pack("<I", len(b)) + b


def v_int(x): return b"\x00" + struct.pack("<i", x)
def v_long(x): return b"\x01" + struct.pack("<q", x)
def v_f32(bits): return b"\x02" + struct.pack("<I", bits)
def v_f64(bits): return b"\x03" + struct.pack("<Q", bits)
def v_str(s): return b"\x04" + cstr(s)
def v_bytes(b): return b"\x05" + struct.pack("<I", len(b)) + b
def v_bool(x): return b"\x06" + (b"\x01" if x else b"\x00")


body = cstr("java.game.GameType") + cstr("addCall") + struct.pack("<I", 5)
body += v_int(1) + v_f32(0x40A00000) + v_f32(0x80000000) + v_str("café") + v_bool(True)
body += b"\x01" + v_long(-2)
record(0x00, body)
for tag, kind, m in ((0x01, 1, "DrawIndexedPrimitive"), (0x02, 0, "Play")):
    record(tag, bytes([kind]) + cstr(m) + struct.pack("<I", 2)
           + v_f64(0x3FF8000000000000) + v_bytes(b"\x01\x02\x03"))
record(0x03, b"\x00" + struct.pack("<I", 4) + b"\x00\x01\x00\xff")
record(0x04, b"\x00" + cstr("sl/Scripts/game/Part.class") + struct.pack("<I", 2)
       + v_str("r") + v_bool(True))
record(0x05, struct.pack("<QIII", 7, 0x401000, 0x7F, 1) + v_int(3))
record(0x06, struct.pack("<Q", 7)
       + b"".join(struct.pack("<I", 0x3F800000 + i) for i in range(9))
       + struct.pack("<I", 0x45FA0000) + struct.pack("<i", -1) + struct.pack("<H", 0))
out.close()
