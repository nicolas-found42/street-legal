# Call-log trace format (`slr-calllog`, M0-10)

One versioned schema for the seven trace sources the hook layer (M0-11) and
the headless backends (M1-12) both emit. The Rust implementation in
`crates/slr-calllog` is normative: a Python or C emitter that follows this
document byte-for-byte produces logs the Rust reader and diff tool accept.

## 1. File layout

```text
offset  size  content
0       6     magic: ASCII "SLRLOG" (bytes 53 4c 52 4c 4f 47)
6       2     format version, u16 little-endian (this document: 1)
8       *     records, back to back, until EOF
```

Each record:

```text
1 byte   record tag (0x00..=0x06)
u32 LE   body length N (bytes, NOT including tag+length)
N bytes  body (layout depends on the tag)
```

EOF is only valid on a record boundary: a truncated tag, length or body is
a hard error, as is a log whose magic or version does not match. There is no
checksum and no index; writers stream and readers verify by re-reading.

## 2. Record kinds

Tag bytes `0x07..=0xFF` are unassigned. A reader that meets one MUST fail
with `UnknownRecordKind` — never skip, never guess. New kinds claim the next
free tag and bump the format version; see §6.

| Tag  | Kind                | Body                                                        |
|------|---------------------|-------------------------------------------------------------|
| 0x00 | native call         | `str class, str name, u32 argc + values, option<value> result` |
| 0x01 | D3D9 call           | `u8 kind, str method, u32 argc + values`                    |
| 0x02 | DirectSound call    | `u8 kind, str method, u32 argc + values`                    |
| 0x03 | DirectInput poll    | `u8 kind, bytes report`                                     |
| 0x04 | file I/O            | `u8 op, str path, u32 n + values detail`                    |
| 0x05 | VM state snapshot   | `u64 frame, u32 pc, u32 control_word, u32 n + values stack` |
| 0x06 | per-frame vehicle   | fixed 56-byte struct (§4)                                   |

Kind tags and family/op bytes:

- native call `class`/`name`: the TUFA declaring class and method
  (`java.game.GameType`, `addCall`). `result` is absent (`00`) for `void`
  natives, present (`01` + value) otherwise.
- D3D9 `kind`: `0x00` device state, `0x01` draw, `0x02` resource,
  `0x03` vertex format, `0x04` present. The full entry-point name
  (`DrawIndexedPrimitive`) travels in `method`.
- DirectSound `kind`: `0x00` buffer, `0x01` spatial, `0x02` capture.
- DirectInput `kind`: `0x00` keyboard, `0x01` mouse, `0x02` gamepad.
  `report` is the raw device report bytes.
- file I/O `op`: `0x00` open, `0x01` read, `0x02` write, `0x03` close,
  `0x04` find. `path` is the requested path (or the handle's file for
  reads/writes); `detail` is op-specific (mode string, byte counts and
  payload bytes, enumeration hits).

## 3. Value encoding

Every value is `u8 tag` + payload:

| Tag  | Type   | Payload                          |
|------|--------|----------------------------------|
| 0x00 | int    | i32 LE                           |
| 0x01 | long   | i64 LE                           |
| 0x02 | float  | u32 LE = exact `f32` bit pattern |
| 0x03 | double | u64 LE = exact `f64` bit pattern |
| 0x04 | str    | `u32 LE length + UTF-8 bytes`    |
| 0x05 | bytes  | `u32 LE length + raw bytes`      |
| 0x06 | bool   | `0x00` false, `0x01` true        |

Floats are bit patterns, never decimal text: `5.0f` is `00 00 a0 40`, and
`-0.0`, NaN payloads and subnormals survive untouched. Tag `0x07` is the
first free value tag. Unknown value tags, file-op bytes and family `kind`
bytes are hard errors. A bool payload other than `0x00`/`0x01` is a hard
error.

Primitives used above:

- `str`: `u32 LE byte length` + UTF-8 bytes (no NUL terminator).
- `bytes`: `u32 LE byte length` + raw bytes.
- `option<value>`: `0x00`, or `0x01` followed by a value.

Sanity caps (fail loudly past them): 64 values per list, 8 MiB per
string/byte-vector, 16 MiB per record body.

## 4. Per-frame vehicle state (tag 0x06, 56 bytes)

All floats are exact `f32` bit patterns, u32 LE:

```text
u64 frame            monotonic frame counter (shared with snapshots)
u32 pos[3]           world position x/y/z, metres (cfg axes: x right, y up, z back)
u32 ypr[3]           yaw/pitch/roll, radians
u32 vel[3]           velocity vx/vy/vz, m/s
u32 rpm_bits         engine RPM
i32 gear             selected gear (-1 reverse, 0 neutral)
u16 reserved         MUST be zero (nonzero = newer writer → fail loudly)
```

Total: 8 + 12 + 12 + 12 + 4 + 4 + 2 = 54 bytes, padded by the 2-byte
reserved word to 56.

## 5. Emitting from Python and C

The framing is deliberately primitive so the hook layer (M0-11, Python/C)
needs no serialization library:

```python
import struct
out = open("run.slrlog", "wb")
out.write(b"SLRLOG" + struct.pack("<H", 1))          # header
def record(tag: int, body: bytes) -> None:           # one record
    out.write(struct.pack("B", tag) + struct.pack("<I", len(body)) + body)
def cstr(s: str) -> bytes:                            # str primitive
    b = s.encode("utf-8")
    return struct.pack("<I", len(b)) + b
# native call "GameType.addCall(1) -> void":
body = cstr("java.game.GameType") + cstr("addCall")
body += struct.pack("<I", 1) + struct.pack("B", 0x00) + struct.pack("<i", 1)
body += struct.pack("B", 0x00)                        # result: absent (void)
record(0x00, body)
```

```c
/* Same framing in C: < = little-endian, so put helpers emit LE directly. */
static void put_u32(uint8_t **p, uint32_t v) {
    (*p)[0] = v; (*p)[1] = v >> 8; (*p)[2] = v >> 16; (*p)[3] = v >> 24; *p += 4;
}
```

Bit-exactness rules for foreign emitters: floats via `struct.pack("<f")` /
`memcpy` of the 4 bytes (never `repr`/`printf %f`); strings UTF-8 without
NUL; records in call order with no reordering, coalescing or dropping.

## 6. Versioning and extension

- Format version `1` covers exactly the tags in §2–§3.
- A new record kind, value type, family kind or file op bumps the version
  and claims the next free tag/byte; the new layout is documented here
  first, then implemented.
- Readers reject unknown versions, tags, ops and nonzero reserved words
  loudly. The one exception inside a version: trailing bytes after a
  decoded record body are a hard error (encoder/decoder drift), not
  forward-compatible padding.

## 7. Diff tool (`slr-calllog-diff`)

`slr-calllog-diff <expected.slrlog> <got.slrlog>` walks both logs in
lockstep and prints the first divergence:

```text
DIVERGENCE at record #1 (native_call):
  field args[1]: expected f32:0x40800000, got f32:0x40800001
```

Exit 0 = identical (`OK: N records identical`), 1 = diverged, 2 = unreadable
input. Kind mismatches print `expected X, got Y` at the ordinal; a common
equal prefix followed by exhaustion prints a length mismatch
(`DIVERGENCE after record #3: length mismatch: expected 4 records, got 3
records`). There is no resynchronization: a dropped record is the
divergence. Floats print as hex bit patterns so 1-ulp diffs are visible.
```

```text
Field paths: `class`, `name`, `method`, `kind`, `op`, `path`, `report`,
`result`, `args[i]`, `detail[i]`, `stack[i]`, `frame`, `pc`,
`control_word`, `pos[i]`, `ypr[i]`, `vel[i]`, `rpm`, `gear`.
```
