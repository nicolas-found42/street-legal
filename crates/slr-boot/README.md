# Stock bootstrap and interactive main menu

From the repository root on macOS:

```sh
cargo run -p slr-boot -- stock --window
```

This opens an 800×600 window through winit/wgpu (Metal). The unchanged stock
scripts build MainMenu; the presentation adapter reads their live rectangle,
text, camera and group state and renders the stock textures and font meshes.
The menu appears after bootstrap and keeps updating until the window closes or
an original script calls `System.exit`.

Use the mouse to hover and click buttons, arrow keys to navigate, Enter/Space
to select, and Escape for the focused script's cancel/back action. Name entry
accepts Latin-1 characters and Backspace through the original StringInput code.
Dialogs retain their stock behavior; Escape is not universally a Cancel button.
Input goes through the original Hotkey/Group callbacks and focused OSD, with
picking based on the script-created UI physics boxes. Menu commands, disabled
buttons, text editing and modal decisions remain in unchanged TUFA scripts.

This platform adapter does not establish complete DirectInput/controller or
remapped-control parity. The animated 3D background, audio, full D3D9 emulator
and gameplay beyond the menu remain unfinished. It does not establish screenshot
parity or make the exact port fully playable.

Capture the same rendering path without opening a window:

```sh
cargo run -p slr-boot -- stock --menu-frame re/out/menu.png
```

`--menu-frame PATH` also works with `--window`. The PNG is 800×600; the run's
`presentation.json` lists the script labels, their bounds and source textures.
Both presentation options default the Config/Init instance IDs to the same
controlled `0xffff0080`/`0xffff007f` profile as the existing menu regression.
Explicit instance-ID arguments override those defaults. These remain controlled
inputs, not recovered original startup allocation history. Without either
presentation option the headless defaults and logging behavior are unchanged.

For deterministic input replay, pass `--input-script PATH` with a JSON array:

```json
[
  {"tick":5,"event":{"type":"key","code":39,"pressed":true}},
  {"tick":6,"event":{"type":"key","code":39,"pressed":false},"capture":true},
  {"tick":10,"event":{"type":"pointer","x":0.65,"y":0.41}},
  {"tick":11,"event":{"type":"button","pressed":true}},
  {"tick":12,"event":{"type":"button","pressed":false}}
]
```

Ticks begin at the first complete main menu. Pointer coordinates are normalized
to the drawable window; key codes use Windows virtual-key values. Other events
are `text` (with `text`), `focus_lost`, and `close`. Optional `capture` writes
`input-{tick+1}.png` after the frame; `input-state.json` records up to 512 input
snapshots. Replay enables presentation and uses the diagnostic frame budget
without a window. Unconsumed events fail the run.

`cargo test -p slr-boot --test stock_menu_input` exercises actual stock keyboard
navigation, Options, mouse clicks, disabled buttons, drag/focus cancellation,
modal isolation, repeated text/backspace, Cancel and script-driven exit.
`cargo test -p slr-boot --test stock_menu_presentation` checks the real stock
menu image, title/column bounds and hidden-panel exclusion, and compares every
byte of the native-call log between no-input presented and headless executions.
These tests require the local verified stock install and a Metal device.
Generated captures and stock assets remain under ignored paths.

## Headless bootstrap history

The following describes the earlier bootstrap frontier; current integration
status is maintained in `docs/boot-status.md`.

Run from the repository root:

```sh
cargo run -p slr-boot -- stock
cargo run -p slr-boot -- stock --config-instance-id 123
cargo run -p slr-boot -- stock --config-instance-id 0xffff0080 --game-init-instance-id 0xffff007f
```

The default stops at `ResourceCreationBoundary`. The second command is a
**controlled script replay**: 123 is an explicit external instance-ID input,
matching the controlled constructor bridge oracle's input. It is not the ID
observed during original startup. The third command supplies a second created
ID and enters unchanged `java.game.Init`. These profiles do not establish
original startup allocation history, MainMenu or stock single-player parity.

The driver verifies the stock manifest, loads system.rpk, initializes the loader,
requests its two eager classes, installs available native implementations, finds
the Config RPK entry, and allocates its unchanged script class. With the explicit
instance-ID input it runs the unchanged Config constructor, then applies
Config's settings through the port of `FUN_00427bf0`, preserving ordered raw
global words and optional absent-field state. It declares an absent renderer
backend and takes original `FUN_004fc590`'s measured no-op branch. It then reads
`Config.init`, resolves that resource's script class, and stops at
`GameInitCreationBoundary` unless a GameInit instance ID is also supplied.
Both Config and GameInit receive a type1 native resource, Native.ptr link and
script back-reference before their constructors. Consistent relinking and
`ResourceRef.newNative`/`set` on the same ID preserve that node. Creation also
keeps the original temporary retained successor while a constructor runs, then
releases it after returning. A stopped constructor retains its inspectable
pending state. Global package
indices are a replay-local namespace; their equality to original startup is not
established.

With both IDs the stock run completes `Sound.init`: it materialises
`system.rpk:0x1b` using its `script java.util.resource.Dummy` descriptor after
root1's `lod_amp 1.000`, requests the actual retained class, and creates the
native instance with the existing script and Native.ptr. Its compiled engine
constructor remains null. `Frontend.defLoadingPic` now enters its unchanged
`ResourceRef(I)` constructor with mapped resource ID `0x000100a8` and retains
the allocated object through constructor delegation. It stops at the named
`ResourceRef.set(I)V` resource-loading boundary: that nonzero mapped ID has no
registered container. The initializer field has not been committed. The guard
preserves zero and same-ID behavior, including a same-ID hit with an unavailable
lookup. It does not claim the resource is actually missing from stock.
Unwired `Thread.start` also remains guarded before it could drop a pending frame.

Initializer constructor support is limited to zero arguments or one integer
literal/resource literal. Other argument forms and the existing unsupported
initializer families remain explicit boundaries. Four original Unicorn rows
exercise static/instance initializer allocation and null-body `()`/`(I)`
constructor dispatch with real argument collection; decoded metadata/allocation
are supplied. A separate Rust regression executes a real constructor script
body and verifies integer assignment and object retention. Resolved constructor
class IDs/names, receivers, arguments and frame-entry outcomes are inspectable
in `re/out/boot/initializer-constructors.txt`.

Script file operations use a fresh complete copy under ignored
`re/out/boot/files/`, never stock. The controlled allocator starts at seed1 with supplied Config/Init IDs reserved; this is not recovered original startup history. Factory operations are written to `re/out/boot/factory-operations.txt`.

Native calls are written in the existing binary
call-log format to `re/out/boot/runtime.calllog`. Standard output reports the
ordered driver boundaries and standard error reports exact execution stops;
redirect both to `re/out/boot/runtime.txt` to inspect a replay. Missing native
stops include class, method and signature. Graphics/audio/input remain explicit
headless logging hosts. Config reads, settings writes and the final refresh
request are inspectable in `re/out/boot/settings-operations.txt`. This includes
PC24 x87 gamma reciprocal, texture quality multiplication/truncation and FFB
scaling. Controlled original settings rows cover both optional-field branches;
refresh rows establish only the absent-backend branch used by this profile.

Original oracles used by the VM fixes: original `FUN_0041fbc0` null-body dispatch
under Unicorn (`re/oracle/vm_empty_dispatch_probe.py`, six cases), and static
field flags checked against original field-reference behavior. The boot bridge
oracle (`re/oracle/boot_bridge_probe.py`) establishes constructor dispatch shape
with controlled resources; it does not run original script bodies. The separate
headless-original startup still stops before script execution. Consequently
there is no original script-call prefix available for an execution comparison.

Pending boot work includes native registration order/full surface, actual
original allocation history, general resource materialisation,
present-backend graphics refresh, scheduler/native-thread wiring and monotonic
clock/pacing. Native link validation uses
`re/oracle/boot_native_link_probe.py` (original creation/link/field-store
instructions, supplied resources and script-dispatch boundary). The current
frontier deliberately exits with a nonzero status.

Default factory evidence: original `FUN_0053a1a0` (three supplied-boundary
cases), `FUN_0053a2a0` (three controlled cases plus one fresh original type1
constructor and empty lifecycle case), and `FUN_00536ed0`/`FUN_00536f90`
(two 130-allocation/recycle sequences) under Unicorn. Fresh instance flags are
0x221, dynamic default-template flags2 (stock script templates keep descriptor flags0), and gametype words start at 1.0f/0. The class cache uses
retained ClassId identity. Tests preserve the receiver's existing Native.ptr
while attaching the new container and script back-reference. Parented native
creation accepts only the declared loaded fresh-empty type1 profile, with
zero inherited range words; other parent branches stop before allocation.
Config, Init and factory children append to the game-root child list. Typed
head/end sentinels preserve order, previous/next links and tail behavior.
Original `FUN_00536a10` rows cover explicit-parent registration of three
sequential children for each type1/type8; manager lookup remains unverified.
The original type8 parser row compares two supplied parser phases over actual
stock text; it does not prove package-manager loading/inheritance. The driver's
RPK reader supplies that phase order, logs it, and accepts only a direct child
of library root1. Deeper/other-root hierarchies, unknown descriptor commands,
flags and packages stop explicitly. The fresh child factory row supplies a declared
already loaded empty parent and virtual/event boundaries. None of these probes
executes original script bodies or establishes full original startup parity.
