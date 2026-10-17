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

## 3. Per-chunk layout status (M2-04: geometry/physics decoded to field level)

Reader: `crates/slr-fmt-rpak/src/payload.rs` (`PolyVertex::decode`,
`PhysPlane::edge_lengths`, `Chunk::{rsd_gametype, rsd_point}`); corpus gates in
`crates/slr-fmt-rpak/tests/payload.rs` (`walk_file` asserts every invariant
below on all four maps, both profiles). Dense notes: `slr-notes/rpk-geometry.md`
(outside the repo).

- `RSD\0` — **understood (fields)**. Text payloads are `gametype 0xHHHHLLLL`
  + `params x,y,z,yaw,0,0` (two `\r\n` lines; no third `flags` line exists in any
  payload — the M2-03 "flagged root" remark was a line-split miscount: the file
  ends with a trailing empty split). `HHHH` is the map id (2 = city/city2,
  3 = ROC_track, 6 = test_track; low word never 0). `params x,y,z` is the
  placement point (finite everywhere; 7 city sky-prop and 213 city2 far-field
  points lie past the streamed-geometry edge, so only finiteness is gated).
  Binary gametypes `0x352` (1,799, city only) and `0x15` (755, city only) keep
  token bytes (`lines == None`): `0x352` is `params <n>;<ALPHA...>` (300-1011B),
  `0x15` is `params <i>;<a,b>` + NUL (36-38B). 160 distinct text gametypes.
  Evidence: `FUN_0053dee0` RSD branch (line loop via `FUN_005511c0`, key match
  `gametype`/`params` via `FUN_005d7190`, id+args via `FUN_00551240`).
- `PHYS` — **understood (fields)**. `u32 nplanes`, then `nplanes` x 7 dwords
  (`u32 w0`, `u32 i0/i1/i2` indices into the chunk's own vertex array,
  `f32 nx/ny/nz` unit face normal), then `u32 nverts`, then `nverts`
  x `f32 x/y/z` (centimetres). All 54,583 chunks tile exactly. Per corpus:
  every ref triple lands in-range; every normal has length 1 (±1e-4) and is
  perpendicular to both triangle edges (rel. 1e-3); winding is uniformly
  CCW-into-normal except two degenerate city2 `w0=7` skirt planes with an index
  repeat (`(12,13,12)`, `(4,1,4)`, zero area, valid up normal). `w0` stays
  **open**: a small material-ish id (0-8 dominate; 43-107 distinct per map) that
  varies independently of plane/vertex counts (exe copies it opaquely into the
  `0x20`-byte collision record). Evidence: `FUN_0053dee0` PHYS branch
  (allocates `n1<<5` bytes = 32B/plane, then `n2*0xc` vertex bytes); the 7-dword
  stride follows from the exe advancing past `puVar8 + 4` after indices 1-7.
- `POLY` — **understood (fields, exe-unproven split)**. `u32 1`, `u32 ngroups`
  (always 1; 1,769 chunks), then per group `u32 mat` (zero-based material id:
  spans `0..ngroups-in-file` per map — city 0..107, city2 0..74, ROC 0..41,
  test 0..85; the M2-03 "always 1" remark misread the first chunk's value),
  `u32 nvert`, `u32 ntri`, `nvert` x 56-byte vertices, `ntri` x 12-byte triangle
  index triples (every index in-range; no base offsets — `walk_file` asserts
  `< nvert` directly). Vertex split (`PolyVertex`):
  `pos[3f]` cm + `nrm[3f]` unit + `dif[u32]` diffuse `A8R8G8B8` (near-white
  tints; alpha 255 on 88-100%) + `spc[u32]` remainder (`0xFFFFFFFF` on flat
  faces) + `uv[2f]` + `aux[8B]` + `q[2f]` second coords. Corpus: positions land
  in per-map world bounds; normals unit (±1e-3; exact-zero billboard normals
  exempt: 2 city, 253 city2, 27 test_track); triangle winding agrees with the
  vertex normals (198,521/198,574 city; negatives are isolated flipped quads);
  `aux` is zeros except finite float pairs on road skirts/sign quads and
  small-int tuples (`(0,1)`) on city2; 5 verts carry sentinel uv words
  (`0x00000130/0xFFFFFFFF`, `0x7FFFFFFF/0x3F800000`) on untextured faces.
  Evidence: geometric validation only — the exe's POLY branch copies 14-dword
  verts and 3-dword index triples opaquely (`iVar11*3+3+iVar5*0xe` size pass,
  then verbatim dword copy); no FVF/stride consumer has been traced, so the
  color/uv/aux/q roles are inferred, and `pos`/`nrm` mirror the SCX v3 vertex
  head (pos 3f + normal 3f + uv 2f; `slr-fmt-scx` crate).
- `EXTP` — **understood (structure)**. `size/4` x `u32` exterior-package type
  ids (`city` root: `(3,)`). Evidence: `FUN_0053dee0` EXTP branch via
  `FUN_00544370`.
- Records — **understood (fields)**. Offsets, stride, name/bbox placement
  mirror `FUN_00544010` exactly. Chain solved: the tree is strict (no range is
  linked twice on any map) and every child range's records carry
  `k1 == parent record's k2`; `k2` values are globally unique per map (city
  58,209/58,209; city2 56,688/56,688; ROC 11,265/11,265; test 12,223/12,223).
  Root `k1` is 1 on all four maps. Named-placement role solved: every
  `b == 0x21` record owns exactly one child holding exactly one text `RSD`
  (city 10,168/10,168; city2 15,653/15,653; ROC 1,791/1,791; test 1,413/1,413 —
  asserted 1:1:1 in `walk_file`); the record's 6-float bbox is centre+halfsize
  around the placement (halves sane <1000cm on all named records; the point
  itself is often the prop's ground anchor, not the box centre). `b == 0x01`
  children hold `PHYS` (collision), `POLY` (render), binary-`RSD`, or empty
  (grouping) ranges. `z`/`f1` naming (`b`-dependent zero vs 1.0) is positional,
  not semantic. Bounding boxes are finite everywhere (centimetre scale).

## 4. Open questions (M2-03 list → M2-04 status)

1. POLY 56-byte vertex split — **solved (exe-unproven)**. `PolyVertex::decode`
   + corpus gates above. Remains: trace the D3D9 FVF/stride consumer to promote
   the color/uv/aux/q roles from inferred to exe-proven.
2. PHYS plane `w0`/`i0-i2` semantics — **half-solved**. `i*` are own-array
   vertex indices and the normal is the unit face normal (proven by corpus
   geometry + exe stride match). `w0`'s meaning (material/friction id?) and how
   PHYS planes bind to POLY geometry stay open.
3. RSD gametype registry — **half-solved**. Id shape (`HHHH` = map) and the
   `params` placement-point layout are proven; binary `0x352`/`0x15` shapes are
   catalogued but their token encoding stays opaque, and the 160 text ids still
   need mapping to exe classes (candidate dispatcher: `FUN_00544590` id
   remap + `FUN_00546070` attach; class constructors `FUN_0053xxx` via
   `FUN_00536a10` switch — not traced per gametype).
4. `k1`/`k2` id-space; named-placement role — **solved**. Strict tree + `k1 ==
   parent k2` + globally unique `k2` (asserted); named records 1:1:1 own their
   text `RSD`; bbox is centre+halfsize.
5. `area_*.scx` `mesh 0x...` offsets into loose `.SCY`/`.scm` — **out of scope**
   (geometry side belongs to the SCX ticket, not RPK payload).
