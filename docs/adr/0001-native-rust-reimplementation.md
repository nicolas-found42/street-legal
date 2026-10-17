# 0001: Reimplement natively instead of emulating the original

The original is a 32-bit Windows x86 D3D9 executable that cannot run on macOS arm64 without
emulation, and its logic is split between native code and a custom bytecode (TUFA) VM. We
reimplement the game in Rust and read the original *data* directly. Consequence: behaviour is
only as faithful as the decoded evidence; everything else is explicitly approximated
(see `docs/fidelity.md`).
