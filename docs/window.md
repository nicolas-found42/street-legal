# Native macOS window and display

S9 covers the shared production `slr-d3d9` window/display backend on macOS
arm64. It is separate from stock-scene rendering (G6), physical input parity
(G8) and in-game Prime playback (P30).

The game renders into a logical back buffer (800×600 by default). The native
window presents that texture inside the largest centered rectangle with its
aspect ratio, with opaque black bars outside. Pointer coordinates are mapped
through that rectangle. `SLR_LOGICAL_SIZE=640x480` selects the logical size;
`SLR_FULLSCREEN=1` starts fullscreen. F11, Alt+Enter and Cmd+Enter switch modes.
A shortcut keeps ownership of its physical key through repeats and its matching
release; focus loss clears that ownership.

macOS uses winit's simple fullscreen mode: the window covers the current monitor
and restores its prior content geometry without a Spaces transition. The earlier
Spaces probes reverted to windowed; their failed observations remain retained.
The display journal records actual layouts and observed modes, not just requests.
Drawable readback is submitted before presenting the acquired surface texture.

## Live S9 verification

```sh
python3 acceptance/check.py --execute S9 --output-root re/out/window-proof
```

A linked worktree also supplies its primary checkout with `--evidence-root`.
The runner builds the production backend for `aarch64-apple-darwin` in a dedicated
`target-platform/` cache. It binds source revision, dirty contents, executable
hash, arguments, authored inputs, full journals and PNGs to each fresh bundle.
It never accepts an imported receipt as a pass.

Two runs use logical sizes 640×480 and 960×540, starting windowed and fullscreen
respectively. Both switch mode and restore it, completing 220 presentations.
The actual Metal window, monitor geometry, windowed scale, fullscreen coverage,
logical aspect rectangle and three captures per run must agree. The authored
back buffer is opaque red below frame ID 60 and blue thereafter. The native
probe decodes every PNG and checks **every pixel**: the authored color inside
the expected rectangle and opaque black outside. Missing captures, GPU errors,
unmet transitions, incomplete scripts, timeouts, image changes and source/binary
drift fail. Failed attempts remain in their own bundles.

These runs use no stock or original assets, no oracle, no RNG and no headless
device shim. They prove S9's host path. They do not establish original-scene
screenshot equivalence, a human visual receipt, physical input parity or P30.

## Video component

`slr-fmt-video::player::Player` decodes the shipped Cinepak AVI timeline through
all dependent intermediate frames and supports stopping/repeating. Its stock
Prime test requires readable fixtures in strict mode. `Frame.video` exposes an
opaque video pass beneath the world and OSD; cache updates include pixel content
so replacing a clip with a reused serial cannot retain old pixels. Stream
emission includes that pass and actual logical dimensions.

Successful native `GfxEngine.openVideo` integration remains unfinished. The
existing boot projection still models its declared unregistered DirectShow graph;
the original also has WINMM movie paths. A decoded Player and a functioning
video surface do not complete the stock feature. P30 remains open.
