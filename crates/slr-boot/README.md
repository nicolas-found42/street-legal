# Headless stock bootstrap frontier

Run from the repository root:

```sh
cargo run -p slr-boot -- stock
cargo run -p slr-boot -- stock --config-instance-id 123
```

The default stops at `ResourceCreationBoundary`. The second command is a
**controlled script replay**: 123 is an explicit external instance-ID input,
matching the controlled constructor bridge oracle's input. It is not the ID
observed during original startup. Neither command claims MainMenu or stock
single-player parity.

The driver verifies the stock manifest, loads system.rpk, initializes the loader,
requests its two eager classes, installs available native implementations, finds
the Config RPK entry, and allocates its unchanged script class. With the explicit
instance-ID input it runs the unchanged Config constructor, then applies
Config's settings through the port of `FUN_00427bf0`, preserving ordered raw
global words and optional absent-field state. It declares an absent renderer
backend and takes original `FUN_004fc590`'s measured no-op branch. It then reads
`Config.init`, resolves that resource's script class, and stops at
`GameInitCreationBoundary` before creating or executing GameInit. Global package
indices are a replay-local
namespace; their equality to original startup is not established.

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
resource creation and native/script linking, present-backend graphics refresh,
GameInit creation, scheduler/native-thread wiring and monotonic clock/pacing. The current
frontier deliberately exits with a nonzero status.
