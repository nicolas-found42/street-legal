# Headless stock bootstrap frontier

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

With both IDs the current stock run enters Init, then stops in
`Sound.init → Dummy.<init> → GameType.createNativeInstance` at a named
`NativeBackendBoundary`. The GameType engine collaborator is not installed.
The driver also guards unwired `Thread.start` before it could silently lose a
pending script frame. A rejected native call produces no fabricated result.

Script file operations use a fresh complete copy under ignored
`re/out/boot/files/`, never stock. Native calls are written in the existing binary
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
original allocation history, native GameType factory/resource loading,
present-backend graphics refresh, scheduler/native-thread wiring and monotonic
clock/pacing. Native link validation uses
`re/oracle/boot_native_link_probe.py` (original creation/link/field-store
instructions, supplied resources and script-dispatch boundary). The current
frontier deliberately exits with a nonzero status.
