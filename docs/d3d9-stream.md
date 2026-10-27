# D3D9 stream comparison

`slr-d3d9-stream` canonicalizes the declared headless original's call/state/draw
stream and the calls implied by the current wgpu translator. The producer can
supply modeled device calls explicitly through `Frame.calls`. An empty call
list describes the translator's actual passes, including separate alpha
blending; it does not copy an original trace.

Run the live comparison through the acceptance registry:

```sh
python3 acceptance/check.py --execute G6 --json
```

In a linked worktree, declare the main checkout with `--evidence-root PATH` for
inventory input links. `tools/link-worktree.sh` keeps headless capture outputs,
D3D9 stream captures and snapshot runs local, alongside the other mutable run
roots. Existing shared output links are rejected and must be preserved before
converting the worker layout. Captures and derived assets remain ignored.

The G6 dependency executes S5 once and retains the same observation. Source
identity includes dirty contents and untracked sources; a source change during
the run invalidates its result. Capture output confinement checks the lexical
worktree boundary, so a symlinked headless output root cannot authorize writes
into another checkout.

## Current proof and limits

The initial integration run freshly executes the original for 1.8 billion
instructions, reproduces its pinned stream digest, and produces two repeatable
port streams and a port PNG. The selected original menu frame 349 and first
emitted port frame diverge in clear parameters and device state. This selected
pair is an explicit scene assumption, not established temporal alignment or
whole-run equality. G6 is failed; S5 is partial. The generated image displays
the menu, but no original pixel comparison is available.

The headless original records call arguments, not vertex/index/texture payloads
or rendered pixels. Garage, racing, free ride, sky/fog and gameplay HUD/cameras
remain blocked scenes. The stream tool reports unsupported methods and first
differences; component tests or self-equality of an original capture establish
no port parity. Missing an explicitly requested oracle capture cannot silently
fall back to an archived file.

Finish the native device/state model and remaining scene input paths, capture
payloads needed to compare actual draws, and obtain original screenshots and
real reviewer observations. No screenshot-review receipt is produced by this
harness or by its data-free tests.
