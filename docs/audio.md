# Audio: DirectSound-equivalent call stream (G7, S6, P28)

Authority: ADR 0009 (tier D audio), `docs/fidelity.md` (gate D: audio), plan M6.
Obligations: **G7** (call streams equal the original's, human listening per item),
**S6** (the stream tooling and the listening protocol), **P28** (engine and world
sound, music). This file records the current tooling and remaining work. Executable S6/G7
scenario validation is not yet integrated; neither obligation is accepted.

## What is compared

The original's sound code is a small DirectSound wrapper in the exe at
`0x559880`–`0x55c250` (device init `FUN_005598c0`, sample load `FUN_0055b490`,
voice play `FUN_0055b9a0`, voice update `FUN_0055bb20`/`FUN_0055c120`, music
streaming `FUN_00559ed0`/`FUN_0055a1c0`/`FUN_0055a820`, a pump thread
`0x559bc0`, a 50 ms multimedia timer callback `0x55ad10`). It is the only code
that calls `IDirectSound*`. The `slr-audio` crate currently provides a software DirectSound device and
stream comparator. Porting the original wrapper/library, connecting live game
natives and implementing a CoreAudio sink remain unfinished. ADR 0009: **output samples are not compared**; the call stream is.

One canonical event per `IDirectSound*` call (plus `DirectSoundCreate`):

```json
{"i": 3, "call": "IDirectSound.CreateSoundBuffer", "this": "ds",
 "args": {"desc": {"bytes": 92674, "flags": 232, "format": {...}, ...}, "out_given": true, "outer": false},
 "ret": 0, "out": {"object": "buf#1"}, "meta": {...}}
```

`call`, `this`, `args`, `ret`, `out` are compared **exactly** (JSON equality; no
numeric tolerance, no reordering, no dropped or coalesced calls). `meta`
(instruction count, virtual clock, exe call site, script-native count, digest of
the bytes written between `Lock` and `Unlock`) is evidence for humans and is
never compared: decoded sample bytes are tier D (ADR 0009) and the call site is
an exe address a port cannot have.

### Canonicalisation (an identity relabelling, not a tolerance)

- COM objects are named by creation order: `ds`; `buf#k` (every primary,
  secondary and duplicate buffer, `k` from 0 in creation order); `listener`;
  `3d#k` (the 3D interface of `buf#k`).
- Window handles are `window#j` by first appearance; a null handle is `null`.
- Pointers are never recorded, only nullness. `Lock` records `len1`, `len2` and
  whether each pointer was returned; `Unlock` records `matches_lock` (it passed
  back exactly the regions `Lock` returned).
- Floats are exact bit patterns (`"f32:0x3f800000"`); every other word is a
  decimal integer (`LONG`s signed); GUIDs are named when known, hex otherwise.
- `AddRef`/`Release` are recorded (stricter than the hook layer, which skips
  them), so a leaked or double-released object diverges.
- An unlisted method or struct stops the capture (`Boundary`); the recorder
  never summarises. Failed calls claim no `out` values.

The device-side inputs of a scenario (`GetCaps` words, the play/write-cursor
lead, the cooperative-level window) are **declared platform-profile inputs**
(`re/headless/make_boot_profile.py`: software-only driver, no hardware mixing or
3D, 10 ms write lead). The play cursor is a pure function of the virtual clock
and the call history, so both sides produce the same `GetCurrentPosition`
answers only if the port runs under the same clock replay.

## The tiers of evidence

| Layer | What runs | Judged by |
|---|---|---|
| Device model | the Rust `DirectSound` software device vs the Python `dsound_platform.py` model, on shared method-level vectors | `slr-audio` device-vector tests (bounded component) |
| Library functions | the exe's `0x559xxx` entry points under Unicorn (with the Python device and declared inputs) vs the Rust library port, per function over vectors | `slr-audio` device-vector tests (bounded component) |
| Scenario streams | the headless original's full-boot stream vs the port's stream from its own natives and scripts, deterministic replay | future live G7 validator |
| Human listening | a person runs the listening protocol on the native build and records receipts | G7, S6, P28 |

A lower layer never substitutes for a higher one. Only the scenario layer plus
listening receipts can pass G7; device and function layers can only fail or be
partial.

## Oracle capture

```sh
~/slr-oracle-venv/bin/python re/oracle/dsound_capture.py --label boot-2100M --limit 2100000000
```

Writes `re/out/headless/audio/<label>/{dsound.jsonl,manifest.json}` (header line
= provenance: exe SHA-256, platform/graphics/desktop/media profile SHA-256,
instruction limit, scenario). The manifest adds the source identity (git HEAD,
tracked-diff list, SHA-256 of the tracing and oracle sources) and the stop
reason. A run that hits a `Boundary` keeps what it recorded and says so. In a
worktree, make `re/out/headless` worktree-local first
(`LOCAL_OUT="boot headless" tools/link-worktree.sh <main>`): the oracle writes
there and must never write through a symlink into the main checkout.

Compare two streams (first divergence, exit 1; unreadable input, exit 2):

```sh
cargo run -q -p slr-audio --bin slr-audio-diff -- expected.jsonl got.jsonl
```

## Human listening protocol (S6, G7, P28)

Receipts are a human step: Codex prepares the checklist, the build and the
capture of what is running; it never writes a receipt. `acceptance/listening/`
holds the protocol and the receipt schema:

- One row per applicable parity item (`P28` engine/world sound, music, plus the
  items whose rows list audio: menu clicks, race start, collisions, horn,
  nitro). Each row names the scenario, what to listen for, and the original
  reference (the user's own run of build 606, or a described expectation).
- A receipt records the reviewer, date, the candidate source revision (git HEAD
  plus dirty state), the binary's SHA-256, the output device, and per row
  `pass`/`fail`/`not-heard` with notes. `not-heard` is a failure.
- The validator accepts a receipt only if its revision and binary match the
  candidate under test and every required row is present.

## Current device evidence

The deterministic model corpus uses 180 cases across write leads 0, 10 and
37 ms, with 2,231 exact events and 154 matching stop operations. This executes
the Python COM/device model; it executes no original sound-library code. The
fixture manifest binds the producer sources, vector bytes and declared profiles.
Unknown descriptor layouts stop before event emission. Unlock preserves null
pointer/length pairs, and the test supplies the oracle's declared remaining
256 MiB resource arena explicitly; duplicates share storage. This evidence does
not establish scenario parity, audible output, or a human listening receipt.
