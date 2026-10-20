//! ResourceRef/GameRef native node and envelope semantics are in [`node`].
//! Resource handles for render, physics, animation, particles and sound are
//! in [`handles`].
//!
//! Handle-typed resource natives (`M1-11`, issue #38): `RenderRef`,
//! `PhysicsRef`, `Animation`, `ParticleSystem` and `SfxRef` as handles over
//! the RPAK resource manager.
//!
//! The behaviour behind the handles (render/physics/particles/audio) lands in
//! M3/M5/M6; this crate ports the *handle semantics* of the 40 native entry
//! points: the `this`-unbox gate, the envelope gate, the animation command
//! queue, the particle action-list walks, the physics body-slot addressing,
//! and the exact engine-call argument encodings. Engine-touching tails go
//! through [`EngineSink`] so headless backends (M1-12) can record them.
//!
//! Machine model (all `[static]`, machine code read; oracle: Unicorn running
//! the exe's own entry points, `re/oracle/res_vectors.py`, local only):
//!
//! - Every native starts with `FUN_0045d910` (arg parse: `this` payload word
//!   plus one word per descriptor char) and `FUN_0042ab50` (read the `I`
//!   field `DAT_0062e008` of `this` — the handle id, 0 on miss). The miss
//!   path logs and returns the type default (0 / `DAT_005e73cc`).
//! - Handle natives then read the envelope pointer at `this+0xc` and take
//!   the engine gate: Animation/ParticleSystem call the envelope vtable
//!   slot `0x14` with `1.0` when `env+0x4c != 1`, require
//!   `FUN_005447d0(...)` bit 31 clear (`0x80000000` for Animation,
//!   `0xa0000001` for ParticleSystem), then `env = vtable[0xc](1.0)` and
//!   require `*(env+0x18) != 0`.
//! - Animation setters append 8-byte `[tag, payload]` commands to the
//!   `+0x9c/+0xa0/+0xa4` (buffer/count/capacity) queue, growing by `0x20`
//!   entries via `FUN_0054f580`. Tags: play 0, loop 1, pause 3, seek 4,
//!   setSpeed 5, setFade 6 (payload: float bits for seek/speed/fade, else 0).
//! - Particle actions form a singly-linked list (`+0x4` next) of nodes with
//!   the label at `+0x14` and type id at `+0x10`; counter nodes are type
//!   `0xd` with the value at `+0x58`. Source nodes carry `0x20`-byte domain
//!   objects (vtable `0x5f113c`, extent words at `+0x10/+0x14`, squared
//!   extents at `+0x18/+0x1c`).
//! - Physics bodies are addressed `base + slot*0xd4`-style off
//!   `engine+0x84` indexed by `engine+0x20c` (`0xd4 = 53*4`, the
//!   `(x*3, x*27, *2, -x)` multiply sequence): position at `+0x64`,
//!   orientation via `FUN_00551c90`.
//! - The port stores live handles in [`HandleTable`], keyed by the same `u32`
//!   id the exe threads through `DAT_0062e008`. The exe's object graph is
//!   not observable through any ported interface.

pub mod factory;
pub mod handles;
pub mod node;
pub mod package;
