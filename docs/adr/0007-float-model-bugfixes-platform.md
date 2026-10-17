# 0007: x87 float model, original bugs fixed in core, macOS only

Amends 0004 and 0006 (grilling round 2, 2026-10-17).

- **Float model.** The stock exe creates its D3D9 device with flags 0x84 (fallback 0x24), without
  `D3DCREATE_FPU_PRESERVE`, and no game code changes x87 precision afterwards (static analysis; CRT and
  D3DX only save/restore). So game float code runs at x87 precision control 24-bit, round-to-nearest; game
  math is x87-only (VC6). The port uses plain `f32` with the original evaluation order by default. A
  function moves to a soft `X87` type (f80 value with precision control) only where a differential test
  shows divergence (register exponent range, double rounding). Transcendentals (`fsin`, `fsqrt`, ...) are
  always bit-exact x87 implementations. This settles the open x87 risk in 0004.
- **Oracle FPU state.** Before any float vector is trusted, an x87 probe suite checks Unicorn's precision
  control against real x87. Function oracles then run with the control word the game actually has on that
  thread (24-bit on the device thread, 53-bit on worker threads).
- **Original bugs are fixed in the port**, not reproduced and not behind switches. Each fix must first be
  confirmed in build 606 (Proofix documents build 936), and is recorded as an intentional divergence in the
  fidelity ledger with its trigger inputs; differential tests exclude exactly those inputs. Trade-off:
  "exact" now has documented exceptions, accepted so that players do not get the original's bugs.
- **Platform: macOS only** (arm64). Windows and Linux are out of scope. CI runs only data-free tests;
  tests needing the clean stock install or oracle vectors run locally.
- **Clean stock install** is produced with Homebrew `msitools` + `cabextract` into git-ignored `stock/`;
  all acceptance tests use it; `reference/raw` is only a compatibility check.
- **Script compiler** (`JVM::compileSource`) is out of scope: stock ships no `.java` sources, so a stock
  boot never recompiles.
- **Randomness and time** go through seams; the exe's MSVC `rand` LCG (per-thread state) and its frame
  timing are ported exactly; all tests run in deterministic replay.
- **Licence:** the project is MIT OR Apache-2.0. Oracle vectors stay git-ignored under `re/out`. Unicorn
  and other GPL tooling live under `re/`, outside the Cargo workspace, and are never linked.
- **Old approximate slice** (`slr-core`, `slr-game`) is deleted in one commit once headless boot works.
