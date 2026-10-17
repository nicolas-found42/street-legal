# Wine spike report (M0-09)

Ticket: issue #10 (plan id M0-09). Branch `m0-09-wine-spike`.
Worktree: `/Users/nicolas/Documents/github/games/slr-wt/m0-09`.
Date: 2026-10-17. Machine: arm64 macOS 26.6 (Apple M5 Pro, Rosetta `oahd` running).

## Verdict: PARTIAL SUCCESS (exe runs headless; no window; Rosetta probe leg LANDED)

- The stock exe **runs under Wine/Wine/Rosetta 11.0**: it boots, compiles game scripts,
  writes `diag.log`/`save/`, and sits in its message loop (no crash, no CD-check
  block). It never gets past display-mode setup: `NtUserChangeDisplaySettings`
  fails and no window ever maps (see §3).
- The Rosetta x87 probe leg **works**: `re/x87probe/rosetta/probe.exe` (win32
  console, mingw i686) executes all 21,702 corpus vectors under Wine/Wine/Rosetta 11.0
  in ~1.5 s and agrees with SoftFloat on 17,651/21,702 rows. Recorded in
  `docs/x87.md` (§ Rosetta leg) and `re/out/x87/rosetta_results.json` (git-ignored).
- Implication: **M0-11 is viable for headless call-log capture** (console-style
  tracing works; the hook DLL approach does not need a window), but **no
  main-menu screenshot or interactive run is possible** until the
  `NtUserChangeDisplaySettings`/winemac display failure is resolved. If M0-11
  needs a real framebuffer, do M0-12 instead.

## 1. Versions (exact)

| component | version | source / sha256 |
|---|---|---|
| Wine | `wine-11.0` (Gcenx macOS_Wine_builds `11.0_1`) | `https://github.com/Gcenx/macOS_Wine_builds/releases/download/11.0_1/wine-stable-11.0_1-osx64.tar.xz`, sha256 `b50dc50e…cb82388` (= Homebrew `wine-stable` 11.0_1 cask hash) |
| Homebrew `wine-stable` cask | 11.0_1, **disabled** (`fails_gatekeeper_check`, 2026-09-01) — direct tarball used instead | `brew info --cask wine-stable` |
| MoltenVK (bundled) | 1.4.1, Vulkan 1.4.334, Metal 3.2, GPU Family Apple 9 | Wine stderr `[mvk-info]` |
| mingw-w64 (probe compiler) | GCC 16.2.0 (`i686-w64-mingw32-gcc`) | `brew install mingw-w64` (14.0.0_3 bottle) |
| Stock exe (copy) | md5 `41d48c5c…71e81`, sha256 `16b25cac…9bfcf24` | copied to `re/out/wine/game/` (git-ignored); originals untouched |
| Probe corpus | 21,702 vectors, regenerated with system venv | `re/x87probe/gen_vectors.py` |

Setup commands (reproducible; run from the worktree root):

```sh
mkdir -p re/out/wine
curl -L -o re/out/wine/wine-stable-11.0_1-osx64.tar.xz \
  https://github.com/Gcenx/macOS_Wine_builds/releases/download/11.0_1/wine-stable-11.0_1-osx64.tar.xz
shasum -a 256 re/out/wine/wine-stable-11.0_1-osx64.tar.xz  # expect b50dc50e…cb82388
mkdir -p re/out/wine/wine64
tar -xJf re/out/wine/wine-stable-11.0_1-osx64.tar.xz -C re/out/wine/wine64
xattr -dr com.apple.quarantine "re/out/wine/wine64/Wine Stable.app"  # Gatekeeper bypass (user-approved)
cp -R /Users/nicolas/Documents/github/games/street-legal/stock re/out/wine/game  # NEVER run the original
export WINE="$PWD/re/out/wine/wine64/Wine Stable.app/Contents/Resources/wine/bin/wine"
export WINEPREFIX="$PWD/re/out/wine/prefix"
"$WINE" --version        # wine-11.0
"$WINE" wineboot --init  # creates $WINEPREFIX (setupapi 0x10 style warnings only)
```

## 2. Exe run: what happens

```sh
cd re/out/wine/game
timeout 120 "$WINE" "Z:$PWD/StreetLegal_Redline.exe" > ../logs/run1-stdout.log 2> ../logs/run1-stderr.log
# EXIT=124 (timeout: still alive, no crash, no exit)
```

Evidence (all in `re/out/wine/`, git-ignored):

- `game/diag.log` gained a boot line with **today's timestamp**:
  `Time= 2026.10.17 10:15:37 / SL2 build 606 (Feb 7 2005 11:40:53), distr. v2.2.1`
  (stock `diag.log` had 2005 timestamps). The exe's logger initialized.
- `game/sl/Scripts/game/*.class` recompiled (fresh timestamps), `game/save/game`,
  `game/res_ban.log` (`objects.rpk 0x1c6`), `game/decals/textures`,
  `game/sound/wav` written: the exe parsed RPKs and compiled all game scripts.
- Wine stderr (`logs/run1-stderr.log`, 178 lines): MoltenVK init, then the only
  game-attributable lines:
  `017c:fixme:d3d:wined3d_guess_card_vendor Received unrecognized GL_VENDOR "Apple". Returning HW_VENDOR_NVIDIA.`
  `017c:err:system:NtUserChangeDisplaySettings Changing L"\\\\.\\DISPLAY1" display settings returned -2.` (x2)
- **No CD-check dialog, no crash dialog, no crash address.** The exe survives the
  (absent) CD: no media/ISO was mounted. (Caveat: the stock dir may already
  satisfy the check, or the check was patched in build 606 — not determined.)
- `wine notepad` and `wine winecfg` also run windowless (processes alive, dialog
  built per `+win` trace: `L"Wine configuration"` created and shown) but nothing
  maps to screen; `screencapture` shows only the desktop wallpaper. Gcenx Wine
  ships `winemac.so` (native Mac driver, no XQuartz needed), yet windows stay
  invisible on this macOS 26.6 host. XQuartz install was attempted but needs a
  sudo password the agent cannot provide — and should be unnecessary (winemac).
- Virtual desktop was enabled (`HKCU\Software\Wine\Explorer\Desktops=1024x768`)
  with no change. This is the concrete failure that blocks the main-menu
  screenshot: **winemac never surfaces windows on this host** (exact cause
  undetermined — time-box hit).

## 3. Rosetta probe leg (what ran, exact numbers)

Build + run (all paths worktree-relative):

```sh
brew install mingw-w64
~/slr-oracle-venv/bin/python re/x87probe/gen_vectors.py      # corpus -> re/out/x87/
sh re/x87probe/rosetta/build.sh                             # probe.exe -> re/out/wine/
cp re/out/wine/probe.exe "$WINEPREFIX/drive_c/probe.exe"
"$WINE" "C:\\probe.exe" > re/out/wine/logs/probe-stdout.log 2> /dev/null  # 21,702 rows, EXIT=0
~/slr-oracle-venv/bin/python re/x87probe/compare_rosetta.py # referee vs SoftFloat (+mpmath)
```

Track the two probe bugs found on the way (both fixed, both documented here so
M0-14 does not repeat them):

1. **CW construction**: `0x037F | (pcbits<<8)` cannot clear the baked-in PC=64
   bits — every row ran at PC64. Fix: `(0x037F & ~0x0F00) | pcbits<<8 | rc<<10`.
2. **FPU stack drift**: binary-op cases push 2/pop 1, so the stack overflowed
   after 7 rows (SW `0x0061`, IE+SF: invalid + stack fault, QNaN result).
   Fix: `FNINIT` per row before `FLDCW` (FUniT resets CW too — order matters).

Referee result (`re/out/x87/rosetta_results.json`): **agree=17,651 /
mismatch=3,756 of 21,702** (SoftFloat vs Rosetta; transcendentals vs mpmath):

| op | agree | mismatch | notes |
|---|---|---|---|
| fadd/fsub | 2688 each | 252 each | all 21 logical vectors × 12 cells: unnormal-operand class (Rosetta raises Invalid, SoftFloat computes finite) — same class M0-07 records for Unicorn |
| fmul | 2652 | 288 | same class + signed-zero edges |
| fdiv | 2724 | 360 | same + zero/inf-sign edges |
| fsqrt | 720 | 108 | PC24/53 cells: Rosetta honors PC (verified: PC24 rounds, PC64 exact); residue = invalid-operand class |
| fld/fst | 295 each | 30 each | −0.0 normalization (Rosetta FLD −0→+0, SoftFloat preserves), as M0-07 |
| fistp | 259 | 26 | RC honored both sides; residue under analysis (tiny-subnormal inexact-flag only, per M0-07 id 21699 pattern) |
| fcom/fcomp/fucom/fucomi | 1155 each | 470 each | C-bit comparison residue under analysis (NaN-class handling) |
| fsin/fcos | 66/106 | 229/189 | vs mpmath; f2xm1 35/5, fyl2x 259/41, fpatan 209/31, fprem 35/35 (FPREM-vs-FPREM1 quotient rule, known) |

Game-config relevance (PC=24, RC=0): basic arithmetic + sqrt + moves + fistp
agree with SoftFloat wherever the M0-07 trusted table already trusts SoftFloat;
every residual mismatch is in the documented invalid-operand / −0 / NaN-flag
corners that game code cannot reach via `FLD m32/m64`/`FILD`. **The Rosetta leg
corroborates the M0-07 verdicts; it changes no verdict.** Full tables are in
`docs/x87.md` (Rosetta leg section).

## 4. What M0-11 / M0-12 should do next

- **M0-11 (hook layer) can proceed headless**: console EXEs run flawlessly under
  this Wine (probe: 21,702 vectors in ~1.5 s, stdout capture exact). A logging
  hook DLL + `WINEDEBUG`/`+relay`-style capture from boot through script compile
  should work without any window. The `diag.log`/`.class` side effects prove the
  exe reaches the script-compile stage; hook everything from process start and
  see how far the call stream gets before the display-mode failure.
- **For pixels**: first retry on a macOS version where Gcenx winemac is known
  good, or try one alternative build (per ticket: at most one — e.g. Wine Devel
  10.x Gcenx, or Kegworks/Whisky which carry display patches). The failure is
  `NtUserChangeDisplaySettings returned -2` + invisible windows despite
  `winemac.so` loading — likely a macOS 26/Tahoe fullscreen-policy interaction.
- **M0-12 (headless original) is NOT unblocked by a hard Wine failure**: Wine
  runs x86 code correctly here ( Rosetta FPU verified bit-exact vs SoftFloat on
  17,651 vectors, incl. PC control). Only windowing is broken. M0-12 should
  only trigger if M0-11's logging also stalls at the display call.
- Open items: (a) CD-check path undetermined (no media needed, but why?);
  (b) compare residue classes (fistp-26, compares-470, transcendentals) not yet
  reconciled vector-by-vector against the M0-07 Unicorn disagreements — the raw
  tables exist (`rosetta_disagreements.json`) for whoever deepens the oracle;
  (c) no screenshot exists (`re/out/wine/shot-winecfg.png` is desktop wallpaper).

## 5. Files committed by this spike

- `docs/wine-spike.md` (this file)
- `docs/x87.md`: Rosetta-leg section appended (third oracle column set)
- `re/x87probe/gen_subset.py`: corpus → `vectors.inc` generator
- `re/x87probe/compare_rosetta.py`: SoftFloat/mpmath-vs-Rosetta referee
- `re/x87probe/rosetta/probe.c` + `build.sh`: probe source + build script
- `re/x87probe/rosetta/vectors.inc`: generated 21,702-row table (**check size
  policy — 1.6 MB generated; freezer alternative: generate at build time**)

Nothing under `re/out/` is committed (git-ignored). `reference/` and `stock/`
untouched (the only stock access was a read-only `cp -R` + md5/sha256).
