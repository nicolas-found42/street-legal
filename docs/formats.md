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
  the `Part.save` part-record run (interior slot grammar open), then the footer
  the exe writes in `File.close()` (below): slot-count word, JPEG-slot run
  (`[index, 7, 0]` triples), resource table (`u32` count + `(index, len, path)`
  with dense 1-based indices, `RPK` paths for `resID` words) and the byte total.
Numbered companions (`<basename>.<i>`, exe `sprintf "%s.%d"`, store `FUN_00484fa0`
refs `0x485141`/`0x4850f6`) are JFIF JPEGs (`FF D8 FF E0`), one per JPEG slot
(stock 131 files; e.g. `ROC_GT2_1` has 19 for 19 slots).

### Exe store path (differentially tested, `crates/slr-fmt-sdat/src/store.rs`)
- `File` natives `FUN_00485b20` (write int), `FUN_00485bc0` (write float),
  `FUN_00485b70` (readInt), `FUN_00485c10` (readFloat): each pops its frame
  (`0x45d910`), resolves the object's `handle` int field to a native file struct
  (`0x42ab50`; a NULL field or other field type makes the native a no-op / return
  0), then moves exactly 4 raw little-endian bytes through the file layer
  (`0x54cf00` write / `0x54cec0` read). A closed handle (slot word 0) skips the
  I/O; the write natives pass the file layer's return value through. `readInt` /
  `readFloat` on a closed handle or after a short read return an uninitialised
  stack word (undefined; the port returns `None`), and on a missing field the
  constant 0 / 0.0.
- JPEG-slot run writer `FUN_00484fa0`: for each slot node (id, kind, object)
  writes `id`, `kind` (4 bytes each); kind 5 then writes a zero word and saves the
  companion `<name>.<id>` (`0x48daa0`); kind 7 writes the object's blob inline
  (`len`, bytes) only if `sscanf(blob, "sourcefile %5s")` yields something other
  than `save\` / `save/`, else a zero word and the companion (`0x48da30`, the
  shipped case: every stock triple is `[index, 7, 0]`); any other kind writes only
  `(id, kind)`. Slots with an object get a prepare call (`0x419860`) first. The
  return value is the number of bytes written. A blob not starting with
  `sourcefile` leaves the exe's `sscanf` buffer uninitialised (undefined; the port
  errors).
- `File.close()` `0x485870`: slot 0 skips straight to cleanup; mode 0 (read)
  closes; mode 1 (write) writes the footer then closes; other modes do not close.
  Footer by file-format major version (byte at `0x612c76`, 2 in the shipped
  `0x00020100`): version 2 =
  `[slot_count][slot run][resource_count]{[id][len][path NUL]}*[total]` with
  `total = 4 + slot-run bytes + 4 + sum(8 + len)` (so the vehicle "trailing u32"
  is a byte total, not an opaque value); version 1 = entries, their byte sum,
  `resource_count`; other versions write no footer. `slot_count` /
  `resource_count` are words kept by the script side (`file+0x28` / `file+8`) and
  are not recomputed. Cleanup frees the helper lists, the struct and clears the
  handle field.
- Evidence: 79 Unicorn rows (`re/oracle/sdat_vectors.py`, consumer
  `crates/slr-fmt-sdat/tests/sdat_oracle.rs`) compare results and the ordered
  file-layer event log; the oracle replaces only the file layer, the companion
  savers and the list plumbing (the real exe code runs `0x45d910`, `0x42ab50`,
  strcmp, sprintf and sscanf). All 8 stock vehicle saves (and the 9 modded) end
  with exactly the version-2 footer the port produces.
- Read path: `File.open` (mode 0) scans the footer and rebuilds both tables
  (`[slot count][slot triples (id, kind, size[, blob])][resource count]{(id, len,
  path)}*`, `0x485190` / `0x485655`); kind 5 requests the companion `<name>.<id>`,
  kind 7 an inline blob or `sourcefile <name>.<id>`. Whole-session evidence
  (original natives vs port, cross-load): `docs/saves-e2e.md`.
Not covered: `File.readString` (`0x486060`), `readResID` (`FUN_00485ea0`),
`write(String)` / `write(ResourceRef)` (`0x485fd0`, `FUN_00485c60`), the CRT
`fwrite` slot table under the file layer, and the SDAT header writer / open path.

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
`next = cur + *cur + 0x14`; `texture_size` clamp at `DAT_006188b4`, live value 4:
`dropped = max(1, mip_count - texture_size)` compared **signed**, and unless
`dropped >= mip_count` the base dimensions are right-shifted by `dropped` — entries are
stored smallest-first, so the clamp keeps that many smallest mips and drops the largest),
per-mip slices feed the JPEG decode request `FUN_00564b50` from `FUN_00506ad0`
(`payload+0x14`, `rgb_size`) with the alpha branch in `FUN_00506c70`
(`alpha_size != 0`). JPEG decode is tier D via `zune-jpeg`: dimensions asserted only.
Differential evidence (M2 follow-up #105): the Unicorn harness
`re/oracle/ptx_vectors.py` runs the init path as real exe code (only the `[ebx+0xec]`
device-allocation slot is a scratch sentry, and the two `FUN_004f6230` checksum calls are
hooked) plus the walk path with `FUN_00506ad0`/`FUN_00506c70` stubbed as event-log
stubs: 225 rows (185 stock, 9 reference, 31 synthetic/clamp), **225/225 byte-identical**,
replayed by `crates/slr-fmt-texture/tests/ptx_oracle.rs` (`SLR_PTX_VECTORS=$PWD/re/out/ptx
cargo test -p slr-fmt-texture --test ptx_oracle`). The walk assertion covers the loader's
advance/finish rule, not the JPEG decode: `FUN_00506ad0`/`FUN_00506c70` bottom out in the
exe's static libjpeg (`FUN_00564b50` et al.) and need a booted device heap.

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

## Particle scripts (`particles/scripts/**/*.cfg`)
Two dialects, 14 files in the stock install (modded overlay: the same 14).
**Emitter scripts** (`particle_clouds/*.cfg`, 10 files, 40 `[ACTION]` lines, 11
`;`-commented lines): `[STATE]` then keyword lines (`type`, `velscale`, `dimension`,
`source_size`/`dest_size`/`source_color`/`dest_color`/`source_alpha`/`dest_alpha`/
`source_angle`/`dest_angle`/`texture_offset`/`angular_velocity`/`lifetime` each followed by
a *domain*), then `[ACTION]` lines (`source`, `gravity`, `bounce`, `move`, `damping`,
`explosion`, `counter`, `directsource`; label as 2nd field). Domains (`FUN_005195e0`):
`point x y z`, `line a b`, `triangle p q r`, `plane p n`, `rectangle p u v`, `box a b`,
`sphere c r1 r2`. Parser `FUN_0051ac20` (file via `FUN_0054c880/0054cdd0/0054cec0`, lines
via `FUN_005511c0`, markers by `strstr` so `[ACTION]` matches anywhere in a line, even a
`;` comment; state lines `FUN_00519110`, action lines `FUN_0051a140`) builds a 0xbc-byte
object: header (`1000, 0, 1.0f`), an intrusive action list, and a 0x34-word state block
with eleven domain pointers (defaults from `FUN_0051bbc0`). **Skidmark scripts**
(`skids/{sliding,frontend}/*.cfg`, 4 files): no sections, `maxcount`, `texture`, `width`
(stored halved), `height`, `uv_normal`, `uv_end`, `fade`; `FUN_00521e10`, 0xb0-byte object.
Tokens split on ` ,\t\r\n`, keywords case-insensitive (`FUN_005d7190`), unknown lines (and
every `;` comment) skipped silently. Numbers use the game's own `sscanf` `FUN_00553890`, not
the CRT: `%f` takes `[-]digits[.digits]` only (`+3`, `.5`, `1e5` do not parse as written),
fraction digits are summed on the x87 stack, `%d` accepts a `0x` prefix, `%x` *requires* it
(`texture 0` converts nothing and leaves id 0). Derived domain fields (triangle/plane/
rectangle normals and offsets) follow the exe's x87 evaluation order at PC=24.
Crate `slr-fmt-cfg` (`particle`, `skid`, `scan`; facade `slr_formats::cfg`).
Verified: stock and modded 10 + 4 files, zero skipped lines apart from comments;
differential test vs FUN_0051ac20/FUN_00521e10 in Unicorn (155 + 48 vectors incl. all 14
cfgs of both installs), see `docs/fidelity.md`.
## Skeletons and animations (`.bon`, `.skl`, `.matrixdata`)
Stock: 19 `.bon` (433 bones), 17 `.skl` (429 bones), 17 `.matrixdata` (9,337 keys); modded
21/18/18 (455/449 bones, 9,437 keys). All in `humans/{drivers,nicelady,pedestrians}` plus
`parts/running_gear/meshes/{shock_absorber,spring}.bon` (two bones, both roots). The extensions
are not literals in the exe: the paths come from model/animation descriptor scripts.

**`.bon` / `.skl` (text, CRLF).** Same format, one reader. Per bone nine lines: `name`, `parent`
(`%d`, index in file order, `-1` root; the parent may come later in the file, `vin.bon` lists
`Bip01 Spine` with parent 3), three rotation rows `%f,%f,%f`, a translation row (stored x 0.1,
`.rdata 0x005f08e8`), three `%f, %f` joint-limit lines (min, max radians). `.bon` limits are
`-pi..pi` everywhere; `.skl` carries the animator limits (a min may exceed its max, stored
verbatim) and differs slightly in bind pose and parent table. Reader `FUN_00531e90`
(`thiscall(skeleton, buf, size)`, from the `skeleton %s` line of a model descriptor in
`FUN_00549720`): lines by `FUN_005511c0`, fields by the exe's own scanner `FUN_00553890`. Bone
record `0xe4` bytes: `+0` parent, `+4` thread link, `+8` local 4x4 (rows 0..2 as written with
column 3 zero, row 3 = translation x 0.1 with w = 1), `+0x48` name (line capped at 255 bytes, then
the last **two** bytes cut: a CRLF assumption, so an LF-only file loses a name character),
`+0x4c` mins, `+0x58` maxs (all three limit lines must scan to two numbers, else every axis
falls back to `-pi`/`+pi`). `FUN_00530cd0` threads the bones parent-before-child (`next`, `head`).
World/inverse matrices (`+0x64..`, `FUN_0054f3d0`) are derived state and not ported.

**The scanner is not `strtod`.** `FUN_00553890` `%f` accepts `[ws] ['-'] digit+ ['.' digit*]`
only (no `+`, no leading `.`, no exponent: `1e-3` scans as `1`), accumulates in x87 registers
(integer part by `FILD`, then `acc += d*scale; scale *= 0.1` with `scale` starting at `0.1f` and
the multiplier the `f64` `0.1`), and stores one `f32` (`FSTP m32`). The result depends on the x87
precision control and is often one ulp from the correctly rounded `f32`: of 4,422 decimal tokens in
the stock `.spl` files 1,229 differ at PC=24 (`slr-fmt-spline` uses `str::parse::<f32>`; see the
follow-up in the M2-09 close notes). The port reproduces it with `slr-x87` (PC=24 production,
ADR 0007; the oracle vectors also cover PC=53).

**`.matrixdata` (binary, little endian).** `u32 bones`, then per bone `u32 key_count` and
`key_count` x 32 bytes (`f32 time`, `f32 q[4]`, `f32 d[3]`), then one `\n`-terminated name per
bone in bone order (the exe drops the last byte, then caps at 255). Reader `FUN_00532e70`
(`thiscall(object, buf, size)`, from the `sourcefile` line, `FUN_00538840`): sizes everything from
the **first** bone's key count and skips the later count words unread (all shipped files agree),
takes the times from bone 0, and stores keys key-major (`(key * bones + bone) * 7` floats).
Names equal the sibling `.bon`/`.skl` bone names, in order (corpus test, every model). `q` is a
unit quaternion on every shipped key (`x y z w` order inferred from `w ~ 1`); `d` is near unit
length on most keys and looks like the bone translation over its bind length (inference; the exe
only copies the floats). Shipped files have 5 to 84 keys per bone.

Decoder: `crates/slr-fmt-skeleton` (facade `slr_formats::skeleton`). Verification:

- corpus: every file parses; per file the parent-before-child thread visits each bone once, all
  limits scan, `.bon` limits are `+-pi`, quaternions are unit, times strictly increase, and the
  names of each model's `.bon`, `.skl` and `.matrixdata` agree (stock and modded).
- Unicorn function oracle (`re/oracle/skel_vectors.py`, vectors in git-ignored `re/out/skel/`):
  `FUN_00531e90` and `FUN_00532e70` run on the corpus bytes plus synthetic edge cases and compared
  bit for bit (skeleton: 35 distinct corpus files + 11 synthetic rows at control word `0x007F` and again at
  `0x027F`; matrixdata: 23 rows). Unicorn honours the control word's precision bits for
  `FADD`/`FMUL` here, and a guard test proves the PC=24 and PC=53 vectors differ and that the port
  at the wrong precision fails.
- Not covered: which thread runs the loader (so PC=24 vs 53 in the real game is the ADR 0007
  default, not observed), the `%f` grammar beyond plain decimals (probed, unit-tested only), and
  the world-matrix pass.

## Standard media (DDS/PNG/TGA, WAV/MP3; M2-11)
No exe-side decoder is ported: the exe statically links libpng/libjpeg/zlib, its DDS path is D3DX
(error string "Could not get level Description of D3DX texture"), and its WAV/MP3 loading path is
not yet located (strings `.wav`, `.mp3`, `DSOUND.dll`, `mciSendCommandA`; follow-up). Everything goes through
maintained crates (ADR 0008 licence rule: MIT/Apache-compatible only), decoded pixels/samples are tier D
(ADR 0009), and the tests assert dimensions, formats and frame counts against headers re-read from the raw
bytes in the test (never against the crate's own header parse).
- `slr-fmt-texture`: `dds` (`ddsfile` 0.6 MIT + `texture2ddecoder` 0.1 MIT/Apache; DXT1/3/5, payload must
  equal the header-implied mip chain exactly, cube/volume rejected), `pngdec` (`png` 0.18, to RGBA8),
  `tga` (`tinytga` 0.5, types 1/2/3 uncompressed; header bytes 2/12/14/16; origin normalised to top-first).
  Stock: DDS 1,462 (DXT1 1,138 / DXT3 122 / DXT5 202; 8,076 mips) all decode, PNG 370 (8-bit RGB 103 /
  RGBA 257 / gray+alpha 10), TGA 12 (type 1 8-bit palette 24-bit x7, type 2 32-bit x2, type 3 gray8 x3).
  Modded profile: 1,559 DDS (2 uncompressed A8R8G8B8 cubemaps are out of scope, reported `unsupported`),
  506 PNG (adds 2 indexed), 13 TGA (adds one 24-bit).
- `slr-fmt-sound`: `wav` (`hound` 3.5 Apache-2.0; 8/16-bit integer PCM, 8-bit widened `i8 << 8`), `mp3`
  (`nanomp3` 0.2 MIT/Apache, a bit-exact safe-Rust port of minimp3; Layer III). Stock: WAV 147 (11025/
  22050/32000/44100 Hz, mono/stereo, 8 and 16 bit), MP3 15 (12 MPEG-1 44.1 kHz, 3 MPEG-2 22.05 kHz, all
  stereo CBR); decoded samples per channel equal `frames * samples_per_frame` from an independent header
  walk for all 15. Cinepak/AVI is covered in the AVI + Cinepak section below (M2-12).

## AVI + Cinepak (`Data/Fmv/*.avi`)
Three stock files, byte-identical in the modded profile: `Activision.avi`
(640x480, 304 frames, 33367 us/frame), `Invictus.avi` (640x400, 373 frames,
40000 us), `Prime.avi` (160x120, 834 frames, 41667 us). All carry one `cvid`
(Cinepak) 24-bit video stream; Activision and Invictus add a PCM audio stream
(`auds`, 2 ch, 44100 Hz, 16 bit; one audio chunk more than video chunks) and
an OpenDML `LIST odml`/`dmlh`, plus `idx1`; `Prime.avi` is video only, plain
AVI 1.0 with `idx1`. Layout: `RIFF AVI` { `hdrl` { `avih`, `strl` per stream,
`odml`/`dmlh` }, `JUNK`, `LIST movi`, `idx1` }, then (Activision, Invictus) a
`JUNK` outside the RIFF. `idx1` offsets are relative to the `movi` list type
fourcc (resolved 1:1 against the walked chunks in all three files). `Prime.avi`
stores its video frames under `00db` chunk ids (the "uncompressed DIB" id)
although the stream is `cvid`; the other two use `00dc`/`01wb`. A zero-length
frame chunk is a dropped frame (the previous picture stays). The exe does not
decode video itself: it plays through WINMM `mciSendCommandA` (natives
`openVideo`/`closeVideo`/`isPlayingVideo`), i.e. Windows' own Cinepak codec.

Cinepak bitstream (big-endian; own implementation, public format): frame =
`u8 flags, u24 size, u16 w, u16 h, u16 strips`; strip = `u8 id (0x10 key / 0x11
inter), u24 size (incl. the 12-byte header), u16 y1, x1, y2, x2` (`y1 == 0`:
below the previous strip, `y2` = height); chunk = `u8 id, u24 size (incl. 4)`.
Chunks `0x20/0x21` V4 codebook (full / selective update by 32-bit flag words,
MSB first), `0x22/0x23` V1 codebook, `0x24..0x27` grayscale (4-byte entries),
`0x30` intra vectors, `0x31` inter vectors (flag bit = coded, next bit = V4),
`0x32` V1-only. Codebook entry = `y0 y1 y2 y3 u v` for a 2x2 block; V1 index =
entry doubled to 4x4, V4 = four indices, one per quadrant. Colour `r = y+2v`,
`g = y - trunc(u/2) - v`, `b = y+2u`, clipped. Codebooks persist per strip
across frames; with frame flag bit 0 clear, strip n starts from strip n-1's.

Verified: stock 3/3 and modded 3/3 decode every frame (1511 frames total) and
the decoded count equals the `idx1` video record count, `avih.dwTotalFrames`,
`strh.dwLength` and `dmlh.dwTotalFrames`; all 1511 frames are byte-identical
RGB24 to FFmpeg 9.0.2's Cinepak decoder (independent decoder, run locally, no
exe oracle exists; the test skips without `ffmpeg` on `PATH`). The OpenDML
super index (`indx`) and `ixNN` chunk bodies are not interpreted (none in the
shipped files). The exe's string table also names `Data\FMV\StreetLegal.avi`,
which is not shipped (stock has `Prime.avi` instead); decoder-independent, for
the parity tickets (P01, P30). Decoder: `crates/slr-fmt-video`.
