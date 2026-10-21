# NativesRenderAnim progress

Scope: 35 rows (RenderRef 14, Animation 9, ParticleSystem 9, Painter 3; assignment's "5 Animation" corrected to the 9 rows present in missing_rows.rs).

## Evidence read (addresses)
- All 35 FUN_/LAB_ bodies disassembled from build-606 `.bak.exe` into `re/out/renderanim-disasm.txt` (all_pseudo.c lacks LAB_ bodies for most; doPaint/xPaint present as FUN_00482960/00482c00).
- Existing pinned captures: `re/out/res/res_anim_*.jsonl` (6 Animation setters), `res_psys_*.jsonl` (setFreq/modePermanent/delAction/getCounter/setCounter), `res_handles_render_{color,light,type,change}.jsonl`, `res_handles_{line_create,bone_id}.jsonl`, `res_gate_render_{getdetail,gettypeid}.jsonl`, `res_gate_sfx_*.jsonl` — helpers + facts already pinned in `crates/slr-resource/src/handles.rs` + `tests/res_oracle.rs`.
- setLight reversal confirmed by vectors: script (1,2,3) -> engine 0x48c9d0 args [3,2,1,0].
- setDirectSource/setSource (0x47f270/0x47f660): gates this.ptr -> [+0xc] -> 0x419860(0xa0000001,1.0,0,0) -> [+0x18] particle; case-insensitive label walk (0x5d7190); reuse requires node type 0xb (direct) resp. 0xa (source); nodes carry +0x54 rate, +0x58 domain1, +0x5c domain2, +0x60 bone (0x48bc40 lookup when bone-name arg nonzero); append via 0x45f5a0/0x45faf0 with node vtable 0x5f1128.
- Animation.<init> 0x47ea10: materialise/resolve chain on `render`, engine 0x541400/0x541600/0x541810/0x541860/0x5417f0, envelope stored to `this.obj`; setters gate [env+8] and push [tag,payload] (tags 0/1/3/4/5/6); getPos 0x47f050 gates [env+8] then samples 0x53fe70 (headless 0.0f edge = [0x5e73cc]); finalize 0x47eba0 gates [env+8] then vtable[0](1).
- Painter: paintPart 0x4827d0 + doPaint 0x482960 call paint-engine collaborator FUN_0048d450 (and 0x45fb80/0x551220/0x426800 on success) -> fail closed with that name. xPaint 0x482c00 = FUN_00426800(this,0,0x10,"bigdecal 0 0 0 0 0 0 0 0 0 0 1 0",0) only.
- plotRoute 0x483960 gates on route-engine global [0x6408d0]==0 -> answers 0 (exe's own no-engine path).
- Boot frontier baseline: run 20261021T041614 (filtered 614, unfiltered 903; port stops at MainMenuDialog.show() Unsupported(36), not my rows; oracle trace calls only create/changeResource/setLight/setMatrix, all void, before the frontier).

## Done / next
- [done] recon + disassembly + design
- [next] module native_render_anim.rs, Action domain2/bone extension, probes, tests, registration, MISSING removal
