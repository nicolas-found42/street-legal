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
`"SDAT" u32 0x00020100 u32 type` (`CTRL` tag, `0xFEDCBA98` options, `0x88664422` vehicle); body
plaintext little-endian records, undecoded. Game saves here are JSON (`saves/slot1.json`).
