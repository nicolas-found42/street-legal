"""SLRLOG (`slr-calllog`, docs/call-log.md) writer and reader for the headless oracle.

Byte-compatible with `crates/slr-calllog`: header `SLRLOG` + u16 version 1,
then framed records `tag u8, body length u32 LE, body`. Only the kinds the
original run can produce are written (native call 0x00, D3D9 0x01, DirectSound
0x02, DirectInput 0x03, file I/O 0x04); the reader decodes every documented
kind so tests can verify emitted logs without the Rust tools.

Values are `u8 tag + payload` (int, long, float bits, double bits, str, bytes,
bool). Floats are always exact bit patterns, never decimal text.
"""

from __future__ import annotations

import struct

MAGIC = b"SLRLOG"
VERSION = 1

NATIVE = 0x00
D3D9 = 0x01
DSOUND = 0x02
DINPUT = 0x03
FILE_IO = 0x04
VM_SNAPSHOT = 0x05
VEHICLE = 0x06

INT, LONG, FLOAT, DOUBLE, STR, BYTES, BOOL = range(7)
MAX_VALUES = 64
MAX_STRING = 8 * 1024 * 1024
MAX_BODY = 16 * 1024 * 1024


class Float32(float):
    """A float value encoded as the exact f32 bit pattern of its source."""

    def __new__(cls, value=0.0, *, bits=None):
        obj = super().__new__(cls, struct.unpack("<f", struct.pack("<I", bits))[0] if bits is not None else value)
        obj.bits = (
            bits if bits is not None else struct.unpack("<I", struct.pack("<f", value))[0]
        )
        return obj


class Float64(float):
    def __new__(cls, value=0.0, *, bits=None):
        obj = super().__new__(cls, struct.unpack("<d", struct.pack("<Q", bits))[0] if bits is not None else value)
        obj.bits = (
            bits if bits is not None else struct.unpack("<Q", struct.pack("<d", value))[0]
        )
        return obj


class Long(int):
    pass


def encode_str(text):
    data = text.encode("utf-8")
    if len(data) > MAX_STRING:
        raise ValueError("string exceeds SLRLOG cap")
    return struct.pack("<I", len(data)) + data


def encode_value(value):
    if isinstance(value, bool):
        return struct.pack("<BB", BOOL, 1 if value else 0)
    if isinstance(value, Float32):
        return struct.pack("<BI", FLOAT, value.bits)
    if isinstance(value, Float64):
        return struct.pack("<BQ", DOUBLE, value.bits)
    if isinstance(value, Long):
        return struct.pack("<Bq", LONG, int(value))
    if isinstance(value, int):
        if not -(2**31) <= value < 2**32:
            raise ValueError("integer outside 32-bit SLRLOG int range")
        # unsigned 32-bit words are stored as their two's-complement i32 bits
        return struct.pack("<Bi", INT, value - (1 << 32) if value >= 2**31 else value)
    if isinstance(value, str):
        return bytes([STR]) + encode_str(value)
    if isinstance(value, (bytes, bytearray)):
        if len(value) > MAX_STRING:
            raise ValueError("byte vector exceeds SLRLOG cap")
        return bytes([BYTES]) + struct.pack("<I", len(value)) + bytes(value)
    raise TypeError(f"unsupported SLRLOG value {value!r}")


def encode_values(values):
    values = list(values)
    if len(values) > MAX_VALUES:
        raise ValueError("more than 64 values in one SLRLOG list")
    return struct.pack("<I", len(values)) + b"".join(encode_value(v) for v in values)


def native_call(class_name, name, args, result=None, *, void=False):
    body = encode_str(class_name) + encode_str(name) + encode_values(args)
    body += b"\x00" if void else b"\x01" + encode_value(result)
    return NATIVE, body


def d3d9_call(kind, method, args):
    if not 0 <= kind <= 4:
        raise ValueError("unknown D3D9 kind")
    return D3D9, bytes([kind]) + encode_str(method) + encode_values(args)


def dsound_call(kind, method, args):
    if not 0 <= kind <= 2:
        raise ValueError("unknown DirectSound kind")
    return DSOUND, bytes([kind]) + encode_str(method) + encode_values(args)


def dinput_poll(kind, report):
    if not 0 <= kind <= 2:
        raise ValueError("unknown DirectInput kind")
    return DINPUT, bytes([kind]) + struct.pack("<I", len(report)) + bytes(report)


def file_io(op, path, detail):
    if not 0 <= op <= 4:
        raise ValueError("unknown file I/O op")
    return FILE_IO, bytes([op]) + encode_str(path) + encode_values(detail)


class Writer:
    """Streams records; `close()` flushes. An empty log is exactly 8 bytes."""

    def __init__(self, path):
        self.path = path
        self.stream = open(path, "wb")
        self.stream.write(MAGIC + struct.pack("<H", VERSION))
        self.records = 0

    def record(self, entry):
        tag, body = entry
        if len(body) > MAX_BODY:
            raise ValueError("record body exceeds SLRLOG cap")
        self.stream.write(struct.pack("<BI", tag, len(body)) + body)
        self.records += 1

    def close(self):
        self.stream.close()


class Reader:
    def __init__(self, data):
        if data[:6] != MAGIC or len(data) < 8:
            raise ValueError("bad SLRLOG magic")
        (version,) = struct.unpack("<H", data[6:8])
        if version != VERSION:
            raise ValueError("unsupported SLRLOG version")
        self.data = data
        self.offset = 8

    def _take(self, count):
        if self.offset + count > len(self.data):
            raise ValueError("truncated SLRLOG")
        chunk = self.data[self.offset : self.offset + count]
        self.offset += count
        return chunk

    def string(self):
        (length,) = struct.unpack("<I", self._take(4))
        return self._take(length).decode("utf-8")

    def value(self):
        (tag,) = self._take(1)
        if tag == INT:
            return ("int", struct.unpack("<i", self._take(4))[0])
        if tag == LONG:
            return ("long", struct.unpack("<q", self._take(8))[0])
        if tag == FLOAT:
            return ("float", struct.unpack("<I", self._take(4))[0])
        if tag == DOUBLE:
            return ("double", struct.unpack("<Q", self._take(8))[0])
        if tag == STR:
            return ("str", self.string())
        if tag == BYTES:
            (length,) = struct.unpack("<I", self._take(4))
            return ("bytes", self._take(length))
        if tag == BOOL:
            (flag,) = self._take(1)
            if flag > 1:
                raise ValueError("bad bool")
            return ("bool", bool(flag))
        raise ValueError(f"unknown value tag {tag}")

    def values(self):
        (count,) = struct.unpack("<I", self._take(4))
        return [self.value() for _ in range(count)]

    def records(self):
        out = []
        while self.offset < len(self.data):
            tag, length = struct.unpack("<BI", self._take(5))
            body = self._take(length)
            inner = Reader.__new__(Reader)
            inner.data, inner.offset = body, 0
            if tag == NATIVE:
                cls, name, args = inner.string(), inner.string(), inner.values()
                (has,) = inner._take(1)
                result = inner.value() if has else None
                out.append(("native", cls, name, args, result, bool(has)))
            elif tag in (D3D9, DSOUND):
                (kind,) = inner._take(1)
                out.append(("d3d9" if tag == D3D9 else "dsound", kind, inner.string(), inner.values()))
            elif tag == DINPUT:
                (kind,) = inner._take(1)
                (count,) = struct.unpack("<I", inner._take(4))
                out.append(("dinput", kind, inner._take(count)))
            elif tag == FILE_IO:
                (op,) = inner._take(1)
                out.append(("file", op, inner.string(), inner.values()))
            elif tag in (VM_SNAPSHOT, VEHICLE):
                out.append(("raw", tag, body))
                continue
            else:
                raise ValueError(f"unknown record kind {tag}")
            if inner.offset != len(body):
                raise ValueError("trailing bytes in record body")
        return out
