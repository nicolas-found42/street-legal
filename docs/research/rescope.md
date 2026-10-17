# Re-scope research: exact port of SLRR build 606

Scope: prior art, equivalence-verification methods, Rust float/VM patterns, and a phased plan. Every cited claim has a URL. Items marked **(unverified)** are not backed by a fetched source and need checking before use. Web content was read as data only.

## 1. Existing SLRR community work

| Project | What it is | Relevance | Source |
|---|---|---|---|
| EliasFD3S/slrr-reverse (fork: TrentonCh) | C++/MSVC/CMake rewrite of the Invictus `StreetLegal_Redline.exe` host; "boots stock / typical-mod Java via a TREE + VA-backed native table"; has `jvm`, `tree_eval`, `GameRef`, physics, `render_d3d9` files; MIT; 7 commits | Closest prior art. Its README gives no verification method, no native inventory and no format docs. Not a drop-in oracle. | https://github.com/EliasFD3S/slrr-reverse , https://github.com/TrentonCh/slrr-reverse |
| Connor-Moyle/blender-slrr-scx-importer | Dependency-free SCX parser (`scx_format.py`); handles v3/v5 inline and v4/v6 directory layouts, multi-block v5; "INVO" mesh format; MIT | Format notes for SCX live in the parser header comment. Confirms `.rpk` (RPAK) only references loose files. | https://github.com/Connor-Moyle/blender-slrr-scx-importer |
| CookiePLMonster/SLRR-Proofix | Proof-of-concept fix for v2.3.1 (build 936, not 606). Documents off-by-one copies, CFG tokenizer reading past line end (four-char tag read as `uint32_t`), too many D3D calls per frame, unexplained collision failures | Known original bugs the port must either reproduce or flag. Build differs from ours. | https://silentsblog.com/2018/06/15/slrr-proof-of-fix/ , https://github.com/CookiePLMonster/SLRR-Proofix |
| nedobylskiy/SLRRMultiplayer | Frida hook-based multiplayer mod (2016) | Shows runtime hooking works on the exe; useful as a trace-capture precedent. | https://github.com/topics/slrr |
| MethanolFuel/SLRR-LE2MWM-MethanolFuel-Edition | Mod build with many `.rpk` and Java sources; no format docs | Corpus of scripts for compatibility tests. | https://github.com/MethanolFuel/SLRR-LE2MWM-MethanolFuel-Edition |
| bobberdolle1/slrr-macos-resolution-fix | macOS CrossOver/Wine resolution fix | Evidence the game runs under Wine on macOS. | https://github.com/topics/slrr |

Findings and gaps:

- Steam threads say `.class` files are not openable and that modders edit the shipped `.java` in the SRC folder, with the game compiling on its own. TUFA is therefore an in-engine compiler plus VM, not only a loader. https://steamcommunity.com/app/497180/discussions/0/1290690926870178822/
- Wikipedia lists the engine as custom, "Invictus SCX". https://en.wikipedia.org/wiki/Street_Legal_Racing:_Redline
- No public docs found for RDB or PTX, no resconvert source, no TUFA/Invictus spec, and no SL2 modding wiki with format detail. Searches returned nothing; the `rpk/rdb/ptx` notes in `docs/formats.md` are likely the best available. Steam threads mention a "Borat's RPK converter" but no source was found. https://steamcommunity.com/app/497180/discussions/0/
- No complete open reimplementation was found. slrr-reverse is the only attempt.

Legal posture of fan reimplementations:

- Both existing SLRR projects ship no game assets, say assets and the exe belong to their owners, and use MIT licenses. Connor-Moyle adds a non-affiliation notice. https://github.com/EliasFD3S/slrr-reverse , https://github.com/Connor-Moyle/blender-slrr-scx-importer
- Counter-example of risk: Take-Two sent DMCA takedowns against the re3/reVC reverse-engineered GTA projects, got them restored after a counter-notice, then sued and had them removed again. Defenders argued fair use. https://www.pcgamesn.com/gta-3-takedown , https://www.thegamer.com/take-two-gta-reverse-engineering-project-takedown/
- Mitigation consistent with ADR 0004: ship no assets and no original bytes, require the user's install, keep `reference/` private. This is a risk note, not legal advice.

## 2. Verifying x86 logic with provable equivalence

"Provable" in practice means bit-exact agreement on large input sets (differential testing), not formal proof. No source found claims formal equivalence for game code.

Emulation oracle (single function):
- Unicorn: QEMU-based, x86 16/32/64, fine-grained instrumentation, Rust bindings, GPLv2. https://github.com/unicorn-engine/unicorn . Its README says nothing about x87. Issue #1524 "Missing x87 FPU registers" was closed Jan 2022; precision-control fidelity is **(unverified)**. https://github.com/unicorn-engine/unicorn/issues?q=x87+fpu
- Plan to test x87 behavior directly: run a corpus of `fld/fmul/fsin/fistp` cases under Unicorn and compare with real hardware or Wine. QEMU's TCG x87 uses softfloat; whether it honors the 53-bit precision-control setting that D3D9 sets is **(unverified)**. D3D9 lowers x87 precision to 24 bits unless `D3DCREATE_FPU_PRESERVE` is given **(unverified, from general knowledge)**. This likely matters for every float in the game.
- Qiling layers PE loading, DLL and API hooks, and syscall emulation on Unicorn; docs warn Windows needs collected DLLs and registry. https://docs.qiling.io/en/latest/ , https://isc.sans.edu/diary/Qiling+A+true+instrumentable+binary+emulation+framework/27372 . For single leaf functions a plain Unicorn harness with a hand-mapped PE is lighter.
- x87 results differ for reasons other than bugs (extended-precision registers), so triage divergences by hand. https://lists.webkit.org/pipermail/webkit-dev/2019-March/030529.html

Static recompilation:
- N64Recomp: literal per-instruction MIPS-to-C, MIT, needs a runtime (N64ModernRuntime). No formal verification described. https://github.com/N64Recomp/N64Recomp
- XenonRecomp: Xbox 360 PPC-to-C++, inspired by N64Recomp, no runtime, hookable by weak-aliasing. https://cdn.jsdelivr.net/gh/hedge-dev/XenonRecomp@main/README.md
- x86 examples: M-HT/SR (DOS/Windows exes), xboxrecomp (Xbox x86 to C), sp00nznet/nocturne ("pcrecomp" family). https://github.com/M-HT/SR , https://raw.githubusercontent.com/sp00nznet/nocturne/HEAD/README.md
- Takeaway: recompiling the exe gives exactness cheaply but is not a Rust port, and conflicts with the function-level goal. Use it as an oracle or as scaffolding, not the deliverable. Whether it fits ADR 0004 is the owner's call.

Matching decompilation:
- reccmp (isledecomp): annotation comments (`// FUNCTION: LEGO1 0x100b12c0`) map source to addresses and report match percentages; 32-bit x86 with old MSVC; AGPL-3.0. https://github.com/isledecomp/reccmp . It needs C++ source compiled with a period-correct compiler, so it does not apply to a Rust port. Reuse the idea (address-keyed annotations and per-function status) in a Rust test registry.
- objdiff and decomp.me are local and web diffing tools for object files; decomp.me is MIT; its supported x86 compilers are not stated on the pages fetched. https://git.axiodl.com/encounter/objdiff/src/tag/v2.0.0-beta.2 , https://github.com/decompme/decomp.me . Same applicability limit as reccmp.
- Ghidra headless: `analyzeHeadless` with an export script gives bulk decompiled C. The scripts named in sources are third-party wrappers, not official; check the NSA docs. https://skills.cat/skills/trailofbits/skills-curated/ghidra-headless
- This repo's REA/rizin tooling already covers the same ground; use Ghidra output only as a cross-check.

Trace capture on arm64 hosts:
- Box64's own guide recommends arm64 Wine WOW64 (with Hangover) for 32-bit Windows exes, plus Box32, "highly experimental and unstable"; it says WineD3D performs poorly for 32-bit apps and recommends DXVK. https://github.com/ptitseb/box64/blob/main/docs/WINE.md
- DXVK is a Vulkan-based D3D8/9/10/11 layer, Zlib license; headless operation is not documented. https://github.com/doitsujin/dxvk
- Headless d3d9 via wined3d plus Mesa llvmpipe, Rosetta in Docker, and dsound/dinput stubs: nothing citable found, all **(unverified)**. Treat as a spike. A wine-in-docker run may not reproduce x87 precision state exactly, so compare against Unicorn on a few functions first.
- Alternative that avoids all of this: Frida-style in-process hooks (the multiplayer mod's approach) on the original Windows exe inside a VM. https://github.com/topics/slrr

## 3. Rust patterns for a faithful port

- **Floats:** 32-bit x86 returns floats in x87 registers even with SSE2 enabled, and libm functions may use x87 internally; the thread's advice is to avoid i686 if you care about float results. https://users.rust-lang.org/t/floating-point-does-same-formula-with-same-inputs-give-same-result/100398
- `fmuladd` may or may not fuse; explicit `mul_add` is single-rounded and falls back to software without hardware FMA. Software FMA had a subnormal rounding bug in std and musl. https://doc.rust-lang.org/std/intrinsics/fn.fmuladdf32.html , https://tproger.ru/news/rust-std-i-musl-neverno-okruglyayut-fmaf-na-redkih-chislah
- Rust does not contract or reorder `a*b+c` by default; this is from general knowledge and should be confirmed against the Rust reference. **(unverified)**
- **x87 emulation in Rust:** `rustc_apfloat` is a pure-Rust LLVM APFloat port with an x87 80-bit format, though a maintainer called it in limbo, and a 2022 fix covered an x87 `from_bits` off-by-one. https://docs.rs/rustc_apfloat , https://github.com/rust-lang/rust/pull/100685 . `softfloat-sys` binds Berkeley SoftFloat 3, whose history mentions x86-model specialization and an 80-bit sqrt fix in 3d. https://docs.rs/crate/softfloat-sys/0.1.1/source/berkeley-softfloat-3/doc/SoftFloat-history.html . The `softfloat` crate is f32/f64 only. https://docs.rs/softfloat . Precedent: v86 links softfloat f80 into a Rust build. https://remotebranch.eu/Stowage/v86/commit/b96f984963d915e88e7679110afa6338abdd2830
- Plan: model the original's per-function precision mode in a small `X87` type (f80 value, PC 24/53/64, rounding mode) built on softfloat, used only where the oracle shows f32-on-SSE diverges. Keep everything else plain f32/f64 with explicit evaluation order. Transcendentals (`fsin`, `fpatan`, `f2xm1`) need their own bit-exact implementations; softfloat does not provide them. **(unverified for all candidate crates)**
- **VM design:** build the TUFA interpreter as a table-driven opcode loop whose state (stack, locals, heap handles) is plain data, so it can be snapshotted and compared with a trace from the original. Pin every native method behind a trait with a recorded name and address so coverage is countable (see GLOSSARY "Native method"). slrr-reverse's "TREE + native table" split is a useful structural hint. https://github.com/EliasFD3S/slrr-reverse
- **Test harness:** keep oracle vectors as files (inputs, outputs, x87 control word) under `re/out`, with a runner that reports per-function status keyed by exe address, as reccmp does for C++. https://github.com/isledecomp/reccmp
- **Rendering:** wgpu has no fixed-function pipeline; D3D9 fixed-function would need generated shaders on top of it. This is my inference; the search found no existing wgpu D3D9 emulator. Shader generation from pipeline state is a known technique. https://patents.justia.com/patent/10255651 . DXVK implements D3D9 on Vulkan and is a reference for what state must be modeled. https://github.com/doitsujin/dxvk . Macroquad's fixed pipeline is not a faithful match for D3D9 lighting or texture stages **(unverified)**; budget for a custom wgpu layer or approximate.

## 4. Scoping advice

| Area | Exactness | Rationale |
|---|---|---|
| File formats (RPAK, SCX, CFG, PTX, RDB, SDAT reading) | Exact | Pure functions of bytes; parser round trips and cross-checks against the importer above are easy to oracle. |
| TUFA compiler/VM | Exact | Deterministic, integer-heavy; verify via traces of opcodes and native calls. |
| Vehicle physics natives | Exact per function if x87 precision state is modeled; otherwise "approximate" | This is where float fidelity decides. Do few functions first and see how far the oracle agrees. |
| Collision, AI, traffic | Exact where tied to the scripts; known original bugs (collision failures noted by Proofix) should be reproduced or flagged | https://silentsblog.com/2018/06/15/slrr-proof-of-fix/ |
| Rendering | Approximate | Compare by screenshot tolerance, not bits. |
| Audio, input, windowing | Approximate | No gameplay state depends on them. |
| Saves | JSON per ADR 0002 | Not exact by design. |

Tools to speed up TUFA executor discovery:
- Use slrr-reverse's native table and file split as a map to cross-check names against our own RE, without copying code or licenses blindly. https://github.com/EliasFD3S/slrr-reverse
- Hook the original exe at the native-call boundary with Frida (as SLRRMultiplayer does) to log every native name, arguments and return values while a script runs. https://github.com/topics/slrr
- Use the shipped `.java` sources as input corpus: compile them in the original, capture bytecode via a hook, and diff against the port's compiler output. Source ships with the game per the Steam thread above.
- Drive Unicorn from Ghidra or REA exports of the dispatch loop to replay opcode handlers one at a time.

## Recommended plan

1. **Oracle spike (1 week).** Unicorn harness: map PE sections, run one leaf float function, compare with a native run (VM or Wine). Decide whether Unicorn's x87 and precision control are trustworthy; if not, add a softfloat f80 shim. Record the control word D3D9 leaves set.
2. **Formats first.** Port RPAK/SCX/CFG/PTX/RDB parsers with byte-exact round-trip tests plus the importer's results as a second reference. Acceptance: all files in `reference/raw` parse, re-serialize identically, and agree with the oracle on counts and offsets.
3. **TUFA compiler and VM.** Acceptance: for every shipped script, port output equals traced original output (opcodes and native calls) for a fixed seed.
4. **Native inventory with hooks.** Log every native in a real run; classify each as pure, state, or I/O; implement pure ones with differential tests keyed by exe address.
5. **Physics natives.** Per-function differential tests with random plus edge-case inputs; target 0 ulp. Add the f80 shim only where needed.
6. **Headless whole-game trace.** Spike Wine/Box64/llvmpipe capture; if it does not work, use in-VM Frida traces. Acceptance: a 60 s scripted drag race reproduces position/speed time series bit-exact.
7. **Rendering and audio.** Approximate, on wgpu; accept by screenshot tolerance. Do not block earlier phases.

Risks: x87 divergence from the oracle, undocumented natives, GPL/AGPL contamination if oracle tools are linked into the shipped crate (keep Unicorn/reccmp as dev tools only), and takedown risk (see section 1).
