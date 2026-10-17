# Format notes

Each layout below is enforced by code in `crates/slr-formats` and the corpus test
(`crates/slr-formats/tests/corpus.rs`).

## RPAK (`.rpk`)
`"RPAK" u32 0x200, u32 link_count, u32 0`, links `{u32 id, char[60] name}`, then
`u32 headers_size, total, type1_count, other_count`, then entry headers
`{u32 super_id, u32 type_id, u8 typeof, u8 flags, f32 is_parent_compatible, u32 rsd_offset,
u32 rsd_size, u8 alias_len, alias, [12 f32 if typeof==1]}`. `rsd_*` are absolute file ranges of
descriptor text (`sourcefile ...`). Verified: stock 61/61 and modded 80/80 files, header sizes and counts consistent.

Map packages (`maps/city.rpk`, `city2`, `ROC_track`, `test_track`) carry a binary payload
tree in the root entry's range (record list + `RSD\0`/`PHYS`/`POLY`/`EXTP` chunks, fully
tiled; see `docs/rpk-payload.md` and `crates/slr-fmt-rpak/src/payload.rs`).

## INVO (`.scx`, `.scy`)
v4: `"INVO", 4, chunk_count, 0`, `{offset, tag}` table, contiguous chunks. Tag 5 vertex buffer
`{?, len, count, fvf}`; stride = `(len-16)/count` (flags 0x1 pos, 0x40 normal, 0x200 uv; strides
12/24/32/36/40/44/48/56 seen). Tag 0 index buffer `{?, len, count}` + u16 triangle list (decoded,
indices range-checked on all stock 2,531 v4 files; modded 2,587). Tags 1-4 (materials/metadata) kept opaque. Units are
centimetres; car panels are centred on their own origin.
v3 (stock 390 `.scx` + 13 `.scy`; modded 441 + 21): `"INVO", 3`, then material groups
`{u32 material_size (0x38|0x68|0x88, word included), material bytes, u32 vert_count,
vert_count * stride verts, u32 face_count, face_count * 3 u32 indices}` with an optional
trailing zero word. Stride is `0x2c` after a `0x38` block else the u32 at material offset 60
(64 everywhere shipped); each vertex is `pos[3f] normal[3f] uv[2f]` + `stride-32` raw bytes.
Indices range-checked (stock 178,473 verts / 70,247 tris; modded 288,510 / 107,663). Ports
`FUN_004b5c30` (`local_28 == 3` loop); file units are centimetres (exe scales by `0.01`).
`.scy` files share the container with all-empty groups; their payload is the named
surface-type table (`Material::name`: `"aszfalt"`, `"szalagkorlat"`, `"Material #N"`).
Collision geometry is not in these files.

## TUFA (`.class`)
Flat chunk list `tag[4] u32 size payload`, no gaps (stock 1,452/1,452, modded 1,501/1,501). First chunk `TUFA` (4-byte
version), then `CONS` (constant pool), optional `FILD`, `MTHD`, `CLSS`, `TREE`; a few files hold
several modules. First two pool strings are class name and base class
(`java.game.parts.engines.Baiern_Emer.Emer_StreetGT_block` -> `...Block_Inline_OHC`). `TREE` node
encoding differs by version (u16 ops in 0x108f5; 1-byte ops with a lookup in 0x11ae1); not executed.
Stock profile: 1,491 modules / 5,262 bodies / 269,034 nodes / 94,703 pool entries / 3,414 methods /
2,730 fields; modded profile keeps the previously asserted 1,546 / 5,816 / 320,522 / 102,711 / 3,591 / 3,531.

## cfg
Keyword lines. `_main_*.cfg` vehicle files give: `body x y z rx ry rz mass mesh` (sum = shell
mass), `type mass sphere radius` + `spring a b c` + `wheel x y z rx ry rz steer drive brake
handbrake` (flag semantics inferred, consistent over 96 wheels: 48 steered, rear-handbrake),
`ratios N g1..gN reverse`, `rearendratio`, `brakepower`, `maxsteer`, `enginepower` (6 floats),
`slot x y z rx ry rz id`. cfg axes: x right, y up, z backwards (front wheels at z<0).
`enginepower`: native handler at VA 0x00443f90 (found through the keyword table at 0x0060e2e8)
`sscanf`s six floats into the vehicle struct (offsets 0xd0/0xd4, 0xe0, 0xf0, 0xf4, 0xe4, 0xe8).
Their consumers were not traced, so they are **not used** by the simulation. The 5th/6th values
are identical across a maker's variants (500-1100 / 28-75).

## Body panel assembly
A panel cfg's `attach <type> <slotid>` + its own `slot ... <slotid>` line pair with the chassis
slot of the same id in `_main_*.cfg`: panel origin = chassis slot - panel slot. Mesh file is
`meshes/<panel>.scx`. Validated by the corpus test (24 cars, plausible dimensions, 4.4k-7k tris).

## SDAT
`"SDAT" u32 0x00020100 u32 type` (`CTRL` tag, `0xFEDCBA98` options, `0x88664422` vehicle).
Bodies are script-ordered little-endian records through the exe's `File` natives
(`File.write(int)` `FUN_00485b20`, `write(float)` `FUN_00485bc0`,
`write(String)` `LAB_00485fd0` = `u32 len` + bytes incl. NUL,
`write(ResourceRef)` `FUN_00485c60` = `u32`, readers `FUN_00485b70`/`FUN_00485c10`/
`0x486060`/`FUN_00485ea0`):
- options: `Config.saveConfig`/`loadConfig` (`system/Scripts/util/Config.class`):
  `(id=12, ver=0)`, one word per field (stock 37 fields, `windowed` not on disk;
  modded `Config.class` 42 fields), footer `[0, 0, 8]`.
- controls: `ControlSet.save`/`load` (`system/Scripts/io/ControlSet.class`):
  `(id=16, ver=2)`, device-name strings, `u32` count, 78 structs of
  `{group, vaxisID, deviceID, axisID: u32, from_min/max, to_min/max, dead_zone: f32}`,
  `vasp` vector (`u32` count + `{id: u32, 5x f32}`), footer `[0, 0, 8]`.
- vehicle: `Vehicle.save`/`load` (`sl/Scripts/game/Vehicle.class`): `(id=5, ver)`,
  the `Part.save` part-record run (interior slot grammar open), the JPEG-slot run
  (`[index, 7, 0]` triples), the resource table (`u32` count + `(index, len, path)`
  with dense 1-based indices, `RPK` paths for `resID` words), and a trailing `u32`.
Numbered companions (`<basename>.<i>`, exe `sprintf "%s.%d"`, store `FUN_00484fa0`
refs `0x485141`/`0x4850f6`) are JFIF JPEGs (`FF D8 FF E0`), one per JPEG slot
(stock 131 files; e.g. `ROC_GT2_1` has 19 for 19 slots).

## TRC (`.trc` traffic road graph)
`u32 version(=1)`, seven raw header words `(53, 218, 484, 1572, 4, 760, 194816)`,
then an opaque node/link block to the first lane record at `0x62871`, then
1,473 lane records tiling the file to EOF with zero leftover bytes. Each lane
record is `u32 N` + `N` 57-byte lane items + trailer (`89 + 57 * N` bytes on the
1,461 plain records, ending in floats `13.888889, 0.9, NaN`); 12 records carry
extra bytes past the terminator (the last record absorbs the file's final
175 bytes, which open with `N=3` but hold no terminator). Totals: 4,864 lane
items. Section 1 and the header words' exact roles are unproven (the exe's
route/traffic natives at `0x0048310f`, `0x0048329e`, `0x00483754`,
`0x00483b65`, `0x00483d03`, `0x00483e68`, `0x00484050`, `0x00484420`,
`0x00484730` consume the in-memory graph, whose `.trc` reader was not isolated
from the surrounding container code at `0x00484fa0`/`0x00485190`/`0x00485448`/
`0x00485870`); the lane framing above is structural (every record obeys
`len >= 89 + 57 * N` with the terminator present). Decoder:
`crates/slr-fmt-traffic` (facade `slr_formats::traffic`).

## Pedestrian areas (`.walk` / `.rest`)
Line ASCII under `maps/city/ai/`: six comma floats (bbox `x1,y1,z1,x2,y2,z2`,
`x1 <= x2`, `z1 <= z2`), space, 1-based line id, `;`, payload. `rest`
payloads are `1,<index>` (middle word always 1; index dense 0-based per area
id: stock id 1 x484, 2 x53, 3 x218 over 755 lines). `walk` payloads are
nibble-encoded binary (`A`-`P` = 0-15, pairs little-endian): `u32 total_len`
(`== decoded-4`) then `FF FF FF FF`-terminated records plus an unterminated
tail. First record `[f32x4][f32][u32 X][chain]` (`X` 3 on the 278 two-record
lines, else 4); mids are `21` (`[21][quad]`, 32x), `22` spans (1,489x) or
`23` sidewalks (64x); tails are index runs (`[k][k u32]`, k 1..=7, 921x) or
`22` spans (878x). A `22` span is `[22][quad][id]` then a chained record
(`21`/`22`/`23`, next word in 21..=23) or a leaf index run (4..=6 1-based
walk line numbers, max depth 2, 214 nested spans). A `23` sidewalk is
`[23][n]` (`n` 3..=6), `n+1` `(x,y,z,dx,dy,dz)` waypoints, endpoint quad.
Verified: stock `city.walk` 1,799/1,799 lines (ids dense; 3,718 leaf indices
all in 1..=1,799) and `city.rest` 755/755 lines, zero unexplained bytes;
modded files identical. No isolated exe reader exists (extensions absent
from exe strings; pedestrian natives `FUN_00484d30`/`FUN_00484d90`/
`FUN_00484e20`/`LAB_00484c60` tune density only), so field order, separators,
and the nibble alphabet are ported from the files with structural roles
documented as inferences. Decoder: `crates/slr-fmt-pedestrian` (facade
`slr_formats::pedestrian`).

## PTX (`.ptx`)
`u32 0, u32 mip_count, u32 width, u32 height`, then per mip (smallest first, base last)
`{u32 total, u32 rgb_size, u32 alpha_size, u32 0, f32 level=(index+1)/mip_count}` +
`u8[total]` payload (`rgb_size` bytes JPEG + `alpha_size` bytes JPEG). `total ==
rgb_size + alpha_size` everywhere; RGB SOI (`FFD8`) required, alpha SOI required iff
`alpha_size > 0`. `width`/`height` are the base mip's JPEG (SOF0) dimensions; per-mip
JPEGs halve down to 4x4 (non-square bases keep aspect, e.g. 128x256 has 4x8..256x512).
Verified: stock 127/127 and modded 127/127 files, zero leftover (stock 336 mips:
97 with alpha on every mip, 30 RGB-only; mip counts `{1:90, 5:3, 6:12, 7:17, 8:5}`;
modded 339 mips `{1:90, 5:3, 6:9, 7:20, 8:5}`). Container ports `FUN_00507110`
(header words at `res+4/8/0xc`, first entry at `res+0x10`, per-mip advance
`next = cur + *cur + 0x14`; `texture_size` clamp at `DAT_006188b8` drops high mips),
per-mip slices feed the JPEG decode request `FUN_00564b50` from `FUN_00506ad0`
(`payload+0x14`, `rgb_size`) with the alpha branch in `FUN_00506c70`
(`alpha_size != 0`). JPEG decode is tier D via `zune-jpeg`: dimensions asserted only.

## SPL (`.spl` AI spline)
One control point per line: `x y z dx dy dz\twidth\tspeed[\tflag_a[\text_a\text_b\tflag_b]]`
(token counts 8/9/12, plus 11 for the one ROC line whose empty width column
`\t\t` contributes no token; the scanner skips all whitespace). `dx dy dz`
is a unit direction (stock norms `0.988..1.022`). `width` = road half-width,
`speed` = target speed, `flag_a` (default 1) opens a pit/branch segment where
contiguous (`race.spl` lines 21..33), `flag_b` (default 1) marks twisty
braking-profile sections, `ext_a`/`ext_b` (default 10000.0) are per-car tuned
distances (105..3000 in twisty sections, 10000.0 open elsewhere; the seven
ROC files differ only in these two columns and `flag_a`). The file's first
point repeats as the last (`race.spl` re-emits it with the exit speed).
Verified: stock 9/9 and modded 9/9 files, 453 points each, zero unexplained
content. Reader `FUN_004337e0`: file from the `AI_spline` script line
(`"%*s %f %s %d"`, `.rdata 0x0060d974`), lines via `FUN_005511c0`, scan via
`FUN_00551240` with `"%lf %lf %lf %lf %lf %lf %lf %lf %d %lf %lf %d"`
(`.rdata 0x0060db98`, custom `%lf`-as-`f32` scanner `FUN_00553890`,
differential-tested under the Unicorn oracle), kept iff >5 conversions,
tail defaults `(width = script %f, 1.0, 1, 10000.0, 10000.0, 1)`, chained by
`FUN_0057de20` into `0x54`-byte nodes via `FUN_0057c590`. Decoder:
`crates/slr-fmt-spline` (facade `slr_formats::spline`).

## SCM (`.scm` ground material table)
Text lines (`\r\n`; `;`, `#`, `/` start comments, cut in `FUN_004b6340`):
`materials <N>` then `material <i> <name>` per entry (`%d %31s`: name is the
next token, so `1 - Default` stores `1`; `#`-truncation makes
`Material #7678` store `Material`), plus modded-only tuning lines
(`friction`/`rollres`/`collcoeff`/`skid_*`/`params`/`eof`) that apply to the
most recently defined material (each `material` line clears all present
flags, then sets its own). Indices are dense `0..N`; names index the sibling
`.scy` surface table 1:1 in the stock maps (108/108 city, 75/75 city2).
Verified: stock 2/2 (183 materials), modded text 5/5 (213 materials); the 3
binary addon `.SCM` variants (MP1, Physic_Lab, Caynon) are a different
format, out of scope. Reader: ground-body init `FUN_00463220` (`groundbody`
cfg line `"%x,%f,%f,%f,%f,%f,%f,%s,%s"`, `.rdata 0x00610f10`, carries the
`.scm` path), loaded by `FUN_0054c300`, dispatched per line by
`FUN_004b6340` through the keyword table at `.data 0x00610db8`
(`materials`/`material`/`params`/`friction`/`rollres`/`collcoeff`/`skid_PS`/
`skid_SM`/`skid_SFX`/`eof` handlers `0x4627f0`/`0x4629c0`/… with formats
`"%d"`/`"%d %31s"`/`"%31s"`/`"%f"`/`"%x"` at `.rdata 0x00610e10…`; the
`material` store `19 → "1"`, `74 → "Material"` with present flag set was
differential-tested under the Unicorn oracle). Decoder:
`crates/slr-fmt-ground` (facade `slr_formats::ground`).
