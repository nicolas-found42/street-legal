# 0009: Acceptance per tier, oracle fallback, media decoding, CD check

Amends the tier table of 0004 (grilling round 3, 2026-10-17).

| Tier | Acceptance |
|---|---|
| A formats | every stock file parses; the exe's own loader logic (e.g. PTX mip parsing) is ported exactly and differential-tested. Standard media (DDS, PNG, TGA, JPEG, WAV, MP3, Cinepak) is decoded with Rust crates (Cinepak by our own small decoder); decoded pixels and samples are tier D |
| B VM + natives | per-op and per-native differential tests; headless boot call log equals the oracle's |
| C physics | level 1: every physics native passes per-function differential tests, 0 ulp on trusted operations, over random and edge vectors. Level 2: scenario traces (drag, braking, collision; every stock car) in deterministic replay match the oracle's per-frame vehicle state bit for bit on trusted operations. Mismatches are investigated, never toleranced away |
| D render | primary: per-frame D3D9-equivalent call stream (states, texture stages, transforms, draws) equals the oracle's hooked D3D9 log in deterministic replay. Secondary: perceptual screenshot diffs for named scenes, failures reviewed by a human, so the oracle's own renderer bugs do not define correct |
| D audio | DirectSound-equivalent call stream (buffers, play/stop, volume, pan, frequency, 3D) equals the oracle's hooked log; output not sample-compared; a human listening check per parity item |
| D input | keyboard, mouse, gamepad through the original ControlSet scripts; no force feedback |
| E saves | the same scripted session produces byte-identical save and config files in port and oracle; saves cross-load both ways |

- **Oracle fallback:** M0 time-boxes a macOS Wine spike. If the stock exe will not run, the "headless original"
  (the exe's code under Unicorn with logging Win32/D3D9/DirectSound/DirectInput stubs) is built far enough
  to run boot, physics and scripted sessions. Every acceptance claim names the oracle that actually ran.
- **CD check** (`FUN_00401480`, `texture.dat`) is not ported; it is a ledgered intentional divergence. The
  port instead checks the clean stock install by checksum.
