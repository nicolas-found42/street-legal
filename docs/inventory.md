# Reference inventory

Identity: Street Legal Racing: Redline `SL2` build 606 (Feb 7 2005), distribution v2.2.1
(`diag.log`). `StreetLegal_Redline.bak.exe` SHA-256 `7d77466a...e8edde` is the stock binary;
`StreetLegal_Redline.exe` differs by 2 code bytes (CD-check bypass at VA 0x0040150a,
`JNZ 0x4016fe` -> `JNZ 0x401720`), a version-banner string, and `RT_VERSION`/icon resources
(Miran Wichur Mod v0.9b banner). Installed mods: `MW_Mod.rpk`, `DestructionTools.rpk`, 12
addon maps, modified `parts.rpk`/`particles.rpk`.

8,411 files (corpus test asserts this exact number, excluding `.DS_Store`):

| ext | count | parsed by this repo |
|---|---|---|
| scx | 3028 (2587 v4 / 441 v3) | v4 geometry (4,435 meshes, 1,053,152 triangles); v3 header only |
| dds / png / ptx / jpg / gif / tga / bmp | 1559 / 506 / 127 / 26 / 14 / 13 / 9 | not decoded (textures not used yet) |
| class | 1501 (1500 v0x11ae1, 1 v0x108f5) | chunk walk + constant-pool strings (1,557 modules) |
| cfg | 1053 | line parser; 24 vehicle cfgs typed |
| wav / mp3 | 165 / 15 | not used |
| rpk | 80 | all parsed (11,367 entries) |
| SDAT saves | 11 (+1 empty marker) | header + kind only |
| scy bon skl matrixdata spl scm trc walk rest db | 21 21 18 18 9 8 1 1 1 20 | not decoded |

Evidence artifacts from the reverse-engineering pass (Ghidra via REA, disassembly) were kept
outside the repo in `$TMPDIR/street-legal-re-artifacts`; the conclusions that matter are
restated in `docs/formats.md` and `docs/fidelity.md`.
