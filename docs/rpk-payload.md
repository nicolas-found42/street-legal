# Map RPK binary payload survey (M2-03)

The four stock map packages keep a serialized object tree in the gap between the
root entry's descriptor and the tail descriptors. The reader lives in
`crates/slr-fmt-rpak/src/payload.rs`; the corpus test
(`crates/slr-fmt-rpak/tests/payload.rs`) walks all four maps with complete byte
accounting. Dense working notes: `slr-notes/m2-03-work.md` (outside the repo).

All layouts below mirror the exe's map loader (`FUN_0053dee0` walks chunks,
`FUN_00544010` walks records; `re/out/all_pseudo.c:147786-148121` and
`:152043-152111`). Field meanings are claimed only where the exe proves them.

## 1. Container

```text
range  ::= u32 zero(=0)  u32 count  (count x record)  chunk*
record ::= u32 k1  u32 k2  u8 a(=1)  u8 b  u32 z/f1  u32 x  u32 y
           u8 n  u8 name[n]  f32 bbox[6]  u8 pad[24]      (stride 71+n)
chunk  ::= u32 tag  u32 size  u8 payload[size]
```

- A record's `(x, y)` is the absolute file range of a child range. Every payload
  byte belongs to exactly one range; ranges tile the gap with zero overlaps
  (asserted per range: records tile `[offset, offset+header_len)`, chunks tile
  the tail exactly).
- `b == 0x01`: `z` is `u32 0` (inner link). `b == 0x21`: `z` is `f32 1.0` and the
  record carries a placement name. The exe takes the same branch for both
  (`b & 1`), so no semantic difference is claimed.
- `n` includes the trailing NUL (`road320\0` is `n == 8`); the pad is always
  24 zeros, hence stride `71 + n`.
- `k1` of a child equals the parent record's `k2` (chain observed, meaning open).
- A range with `count == 0` holds only chunks (the 8-zero "prefix" seen before
  leaf chunk streams is the empty header, not a chunk).
- The root entry's descriptor has the same shape: `count` records, then an `RSD`
  chunk (and, in `city` only, one `EXTP` chunk of one id).
- `a` is 1 everywhere in the corpus; `x`/`y` of every visited record land
  inside the payload (no out-of-payload links on any map).

## 2. Tag inventory (payload tails; root-descriptor chunks excluded)

| tag     | city         | city2        | ROC_track   | test_track  | understood          |
|---------|--------------|--------------|-------------|-------------|---------------------|
| `PHYS`  | 23,976       | 19,040       | 5,147       | 6,420       | structure exact     |
| `RSD\0` | 12,722       | 15,653       | 1,791       | 1,413       | structure exact     |
| `POLY`  | 624          | 748          | 155         | 242         | structure exact     |
| `EXTP`  | 0 (+1 root)  | 0            | 0           | 0           | structure exact     |

No other tag occurs in any payload; the reader keeps unknown tags as opaque
bytes with offsets (zero occur in the corpus).

Byte totals walked (records + chunks, `node + tail = payload`, exact):

| map        | ranges | node bytes | tail bytes | payload    |
|------------|--------|------------|------------|------------|
| city       | 58,209 | 4,837,176  | 27,349,143 | 32,186,319 |
| city2      | 56,688 | 4,719,962  | 33,255,082 | 37,975,044 |
| ROC_track  | 11,265 | 930,240    | 6,588,482  | 7,518,722  |
| test_track | 12,223 | 1,006,555  | 8,802,866  | 9,809,421  |

## 3. Per-chunk layout status

- `RSD\0` — **understood (structure)**. Payload is text lines: `gametype ...`
  then `params ...` (sometimes a third `flags ...` line; every text RSD has
  exactly 2 lines except the flagged root ones). 1,799 chunks of gametype
  `0x352` and 755 of `0x15` carry opaque token bytes instead of text
  (`;`-separated tokens, NULs); kept as raw bytes with `lines == None`.
  160 distinct text gametypes. Evidence: `FUN_0053dee0` line-parse loop.
- `PHYS` — **understood (structure)**. `u32 nplanes`, then `nplanes` x 7 dwords
  (`u32 w0`, `u32 i0/i1/i2`, `f32 nx/ny/nz`), then `u32 nverts`, then `nverts`
  x `f32 x/y/z`. All 54,583 chunks tile exactly. `w0`/`i*` meanings are open
  (small ids dominate: 1, 2, 3, 4, 5, 0, 6, 7 ...). Evidence: `FUN_0053dee0`
  PHYS branch (`n1` x 7 dwords, `n2` x 3 dwords); the 7-dword stride (not 4)
  follows from the exe advancing past `puVar8 + 4` after consuming indices 1-7.
- `POLY` — **understood (structure)**. `u32 1`, `u32 ngroups` (always 1;
  1,769 chunks), then per group `u32 f0` (always 1), `u32 nvert`, `u32 nidx`,
  `nvert` x 56-byte vertices, `nidx` x 12-byte index triples. All tile exactly
  (738 distinct `nvert`, 664 distinct `nidx`). Vertex attribute split is
  **opaque** (first 6 floats plausibly position+normal); index semantics open.
  Evidence: `FUN_0053dee0` POLY branch (groups of 14-dword verts, 3-dword idx).
- `EXTP` — **understood (structure)**. `size/4` x `u32` exterior-package type
  ids (`city` root: `(3,)`). Evidence: `FUN_0053dee0` EXTP branch via
  `FUN_00544370`.
- Records — **understood (structure)**. Offsets, stride, name/bbox placement
  mirror `FUN_00544010` exactly. `k1`/`k2` id-space meaning open; `z`/`f1`
  naming (`b`-dependent zero vs 1.0) is positional, not semantic. Bounding
  boxes are finite everywhere (centimetre-ish scale; axes open).

## 4. Open questions (for M2-04)

1. POLY 56-byte vertex split (pos/normal clear; remaining 8 dwords: colors,
   uvs, second set?) — correlate with the D3D9 FVF/stride paths.
2. PHYS plane `w0`/`i0-i2` semantics (kind + vertex indices?) and how planes
   bind to POLY geometry.
3. RSD gametype registry (160 ids -> exe classes via `FUN_00544590`?); binary
   `0x352`/`0x15` token encoding.
4. `k1`/`k2` id-space meaning; named-placement (`b == 0x21`) role beyond the
   name string (exe treats both branches alike at the walker level).
5. `area_*.scx` `mesh 0x...` offsets into the loose `.SCY` (INVO v3, undecoded)
   and `.scm` materials — the geometry side of M2-04.
