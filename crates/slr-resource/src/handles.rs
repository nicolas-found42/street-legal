//! The handle table and the 40 handle-native ports (`M1-11`, issue #38).
//!
//! Layout facts below are machine-code reads of the entry points listed in
//! `re/out/native_api.tsv` (class `java.util.resource.{RenderRef,
//! PhysicsRef,Animation,ParticleSystem,SfxRef}`); oracle notes name the
//! Unicorn oracle (`re/oracle/res_vectors.py`, local only) that pins them.

use std::collections::HashMap;

/// A handle-table entry: the live state behind one `u32` handle id.
///
/// The port's stand-in for the envelope object `Animation.<init>` allocates
/// and `finalize` releases; `words` mirrors the envelope words the natives
/// read and write off-object.
///
/// exe: LAB_0047ea10
/// exe: LAB_0047eba0
#[derive(Clone, Debug, PartialEq)]
pub struct HandleEntry {
    /// Which native class owns this handle (`Animation`, `RenderRef`, ...).
    pub kind: HandleKind,
    /// Words the exe keeps off-object: the envelope/body pointer analogue.
    /// `Animation`: `[queue_buf_lo, queue_buf_hi_or_unused, count,
    /// capacity]` mirrors `env+0x9c/+0xa0/+0xa4` (the buffer lives inline in
    /// [`HandleEntry::queue`] instead of a separate allocation).
    pub words: Vec<u32>,
    /// `Animation` command queue: appended `[tag, payload]` entries.
    pub queue: Vec<Command>,
    /// `ParticleSystem` action list, in link order.
    pub actions: Vec<Action>,
    /// `modePermanent` flag word analogue (`env+0x6c`).
    pub flags: u32,
    /// `setFreq` rate analogue (`env+0xc`).
    pub freq_bits: u32,
}

/// The five handle classes of this ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandleKind {
    /// `java.util.resource.RenderRef`.
    RenderRef,
    /// `java.util.resource.PhysicsRef`.
    PhysicsRef,
    /// `java.util.resource.Animation`.
    Animation,
    /// `java.util.resource.ParticleSystem`.
    ParticleSystem,
    /// `java.util.resource.SfxRef`.
    SfxRef,
}

/// One animation command-queue entry: 8 bytes `[tag, payload]`.
///
/// Set by `setSpeed` (tag 5), `setFade` (tag 6), `play` (tag 0),
/// `loopPlay` (tag 1), `pause` (tag 3), `seek` (tag 4); the payload is
/// float bits for seek/speed/fade, else 0.
///
/// exe: LAB_0047ec00
/// exe: LAB_0047ecc0
/// exe: LAB_0047ed80
/// exe: LAB_0047ee30
/// exe: LAB_0047eee0
/// exe: LAB_0047ef90
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// Command tag (0, 1, 3, 4, 5, 6).
    pub tag: u32,
    /// Payload word (float bits or 0).
    pub payload: u32,
}

/// One particle action-list node.
///
/// Walked by `delAction` (`FUN_0047fdb0`) and `getCounter`
/// (`FUN_0047fcd0`), appended by `setCounter` (`LAB_0047fa70`); the source
/// setters link their own nodes.
///
/// exe: FUN_0047fdb0
/// exe: FUN_0047fcd0
/// exe: LAB_0047fa70
/// exe: LAB_0047f270
/// exe: LAB_0047f660
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// Node type id (`+0x10`): 10 source, 11 direct-source, 13 counter, ...
    pub type_id: u32,
    /// Action label (`+0x14`, NUL-terminated in the exe).
    pub label: String,
    /// Counter value (`+0x58`, type 13 only).
    pub counter: u32,
    /// Rate word (`+0x54`). `setCounter` zeroes it when it appends a node
    /// and leaves it untouched when it reuses one.
    pub rate: u32,
    /// Domain extents (`+0x10/+0x14` of the `0x20`-byte domain object);
    /// `+0x60` on counter (type `0xd`) nodes.
    pub domain: Option<Domain>,
    /// First extent domain (`+0x58`, source nodes only — the same word the
    /// counter nodes use for [`Action::counter`]; a node carries one or the
    /// other depending on its type).
    pub domain1: Option<Domain>,
    /// Second domain (`+0x5c`, source nodes only): the velocity extents the
    /// source setters build from their second `Vector3`/float pair.
    pub domain2: Option<Domain>,
    /// Bone id (`+0x60`, source nodes only): the `0x48bc40` lookup of the
    /// bone-name argument, `0` when the argument was null.
    pub bone: u32,
}

/// One `0x20`-byte domain object (vtable `0x5f113c`).
///
/// exe: LAB_0047f270
/// exe: LAB_0047f660
/// exe: LAB_0047fa70
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domain {
    /// Extent words (`+0x10`, `+0x14`): the selected `flds` operand bits,
    /// or the swapped pair when `fcomp` reports `ST(0) > ST(1)`.
    pub ext: [u32; 2],
    /// Squared extents (`+0x18`, `+0x1c`): `ext[i] * ext[i]` at x87 PC=64.
    pub ext_sq: [u32; 2],
}

/// The live handle table: `id -> entry`, mirroring the `DAT_0062e008` int
/// field the exe threads through `FUN_0042ab50`.
///
/// exe: FUN_0042ab50
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandleTable {
    entries: HashMap<u32, HandleEntry>,
    next: u32,
}

impl HandleTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live handles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no handle is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Allocate a handle of `kind` with `words` engine words, returning its
    /// nonzero id (ids start at 1; id 0 is the exe's null handle).
    pub fn alloc(&mut self, kind: HandleKind, words: Vec<u32>) -> u32 {
        self.next = self.next.wrapping_add(1);
        if self.next == 0 {
            self.next = 1;
        }
        let id = self.next;
        self.entries.insert(
            id,
            HandleEntry {
                kind,
                words,
                queue: Vec::new(),
                actions: Vec::new(),
                flags: 0,
                freq_bits: 0,
            },
        );
        id
    }

    /// Look up a live handle.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&HandleEntry> {
        self.entries.get(&id)
    }

    /// Look up a live handle mutably.
    #[must_use]
    pub fn get_mut(&mut self, id: u32) -> Option<&mut HandleEntry> {
        self.entries.get_mut(&id)
    }

    /// Free a handle; returns `false` when `id` was not live.
    ///
    /// `Animation.finalize` releases the envelope.
    ///
    /// exe: LAB_0047eba0
    pub fn free(&mut self, id: u32) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// Insert a handle under an explicit id (the runtime keys RenderRef and
    /// ParticleSystem envelopes by the receiver's `ptr` word, which the exe
    /// resolves through `FUN_0042ab50` instead of allocating). Replaces an
    /// existing entry with the same id, like a re-`init` overwrites the
    /// exe's particle object.
    pub fn admit(&mut self, id: u32, kind: HandleKind, words: Vec<u32>) {
        if id == 0 {
            return;
        }
        self.entries.insert(
            id,
            HandleEntry {
                kind,
                words,
                queue: Vec::new(),
                actions: Vec::new(),
                flags: 0,
                freq_bits: 0,
            },
        );
    }
}

/// Which command tag an `Animation` setter appends.
///
/// exe: LAB_0047ec00
/// exe: LAB_0047ecc0
/// exe: LAB_0047ed80
/// exe: LAB_0047ee30
/// exe: LAB_0047eee0
/// exe: LAB_0047ef90
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimCommand {
    /// `play()`: tag 0, payload 0.
    Play,
    /// `loopPlay()`: tag 1, payload 0.
    Loop,
    /// `pause()`: tag 3, payload 0.
    Pause,
    /// `seek(float)`: tag 4, payload float bits.
    Seek,
    /// `setSpeed(float)`: tag 5, payload float bits.
    SetSpeed,
    /// `setFade(float)`: tag 6, payload float bits.
    SetFade,
}

impl AnimCommand {
    /// The queue tag word.
    #[must_use]
    pub const fn tag(self) -> u32 {
        match self {
            Self::Play => 0,
            Self::Loop => 1,
            Self::Pause => 3,
            Self::Seek => 4,
            Self::SetSpeed => 5,
            Self::SetFade => 6,
        }
    }

    /// `true` for the commands carrying float bits (seek/speed/fade).
    #[must_use]
    pub const fn has_float_payload(self) -> bool {
        matches!(self, Self::Seek | Self::SetSpeed | Self::SetFade)
    }
}

/// Append one command to an `Animation` handle's queue.
///
/// The exe grows the `+0x9c` buffer by `0x20` entries when
/// `count == capacity` (`FUN_0054f580` realloc) and stores `[tag, payload]`
/// at `count`, then `count += 1`. `Vec` growth is the port's analogue;
/// only the appended words are compared.
///
/// exe: LAB_0047ec00
/// exe: LAB_0047ecc0
/// exe: LAB_0047ed80
/// exe: LAB_0047ee30
/// exe: LAB_0047eee0
/// exe: LAB_0047ef90
/// oracle: Unicorn `re/oracle/res_vectors.py::anim_queue` (local only).
pub fn anim_push(table: &mut HandleTable, id: u32, cmd: AnimCommand, payload: u32) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::Animation {
        return false;
    }
    e.queue.push(Command {
        tag: cmd.tag(),
        payload,
    });
    true
}

/// `ParticleSystem.setFreq(float)`: store the rate word.
///
/// exe: FUN_0047f1f0
/// Writes `*(env+0xc) = bits`.
/// oracle: Unicorn `re/oracle/res_vectors.py::psys_freq` (local only).
pub fn psys_set_freq(table: &mut HandleTable, id: u32, rate_bits: u32) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::ParticleSystem {
        return false;
    }
    e.freq_bits = rate_bits;
    true
}

/// `ParticleSystem.modePermanent(int)`: clear/set bit `0x20000000`.
///
/// The exe branches on the *unboxed int arg*: 0 clears
/// `*(env+0x6c) &= ~0x20000000`, 1 sets `|= 0x20000000`; any other value
/// leaves the flags word untouched.
///
/// exe: FUN_0047f150
/// oracle: Unicorn `re/oracle/res_vectors.py::psys_perm` (local only).
pub fn psys_mode_permanent(table: &mut HandleTable, id: u32, mode: u32) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::ParticleSystem {
        return false;
    }
    match mode {
        0 => e.flags &= !0x2000_0000,
        1 => e.flags |= 0x2000_0000,
        _ => {}
    }
    true
}

/// `ParticleSystem.delAction(String)`: drop the first action whose label
/// compares equal (`FUN_005d7190` case-insensitive), freeing it
/// (`vtable[0](1)`); a null list or a miss frees nothing.
///
/// exe: FUN_0047fdb0
/// oracle: Unicorn `re/oracle/res_vectors.py::psys_del` (local only).
pub fn psys_del_action(table: &mut HandleTable, id: u32, label: &str) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::ParticleSystem {
        return false;
    }
    let Some(i) = e
        .actions
        .iter()
        .position(|a| a.label.eq_ignore_ascii_case(label))
    else {
        return false;
    };
    e.actions.remove(i);
    true
}

/// `ParticleSystem.getCounter(String)`: walk to the *first* node whose
/// label matches (`FUN_005d7190` case-insensitive) — the type is not part of
/// the comparison — and return its `+0x58` word, zeroing it. A match whose
/// node is not type `0xd`, a miss and an empty list all return 0 and zero
/// nothing (`ebx` stays 0 on every early-out).
///
/// exe: FUN_0047fcd0
/// oracle: Unicorn `re/oracle/res_vectors.py::psys_counter` (local only).
#[must_use]
pub fn psys_get_counter(table: &mut HandleTable, id: u32, label: &str) -> u32 {
    let Some(e) = table.get_mut(id) else {
        return 0;
    };
    if e.kind != HandleKind::ParticleSystem {
        return 0;
    }
    let Some(a) = e
        .actions
        .iter_mut()
        .find(|a| a.label.eq_ignore_ascii_case(label))
    else {
        return 0;
    };
    if a.type_id != 0xd {
        return 0;
    }
    let v = a.counter;
    a.counter = 0;
    v
}

/// `ParticleSystem.setCounter(String, Vector3, float)`: walk the action list
/// to the first node whose label matches (case-insensitive) and replace only
/// its `+0x60` domain — `+0x54` and `+0x58` keep their values. A match whose
/// node is not type `0xd` aborts the call without building anything. When no
/// label matches, a fresh type-`0xd` node is appended with `+0x54`/`+0x58`
/// zeroed and the new domain at `+0x60`.
///
/// `domain` is the caller-built [`build_domain`] of the float argument and
/// `0.0`, which is the pair the exe itself builds (`fld rate; fld
/// [0x5e73cc]`); the port does not model the `Vector3` words the exe also
/// stores in the domain.
///
/// exe: LAB_0047fa70
/// oracle: Unicorn `re/oracle/res_vectors.py::psys_set_counter` (local only).
pub fn psys_set_counter(table: &mut HandleTable, id: u32, label: String, domain: Domain) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::ParticleSystem {
        return false;
    }
    let matched = e
        .actions
        .iter()
        .position(|a| a.label.eq_ignore_ascii_case(&label));
    let Some(i) = matched else {
        e.actions.push(Action {
            type_id: 0xd,
            label,
            counter: 0,
            rate: 0,
            domain: Some(domain),
            domain1: None,
            domain2: None,
            bone: 0,
        });
        return true;
    };
    if e.actions[i].type_id != 0xd {
        return false;
    }
    e.actions[i].domain = Some(domain);
    true
}

/// `ParticleSystem.setSource`/`setDirectSource` node link
/// (`LAB_0047f660`/`LAB_0047f270` after their gate chain): walk the action
/// list to the first node whose label matches (case-insensitive,
/// `FUN_005d7190`) and, when its type equals `type_id`, replace its
/// `+0x54` rate, `+0x58`/`+0x5c` domains and `+0x60` bone. A first match of
/// another type aborts the call. With no label match a fresh node of
/// `type_id` is appended carrying all four words.
///
/// exe: LAB_0047f270 (direct source, type `0xb`)
/// exe: LAB_0047f660 (source, type `0xa`)
#[allow(clippy::too_many_arguments)] // The original call site passes all eight.
pub fn psys_link_source(
    table: &mut HandleTable,
    id: u32,
    type_id: u32,
    label: String,
    rate: u32,
    domain: Domain,
    domain2: Domain,
    bone: u32,
) -> bool {
    let Some(e) = table.get_mut(id) else {
        return false;
    };
    if e.kind != HandleKind::ParticleSystem {
        return false;
    }
    let matched = e
        .actions
        .iter()
        .position(|a| a.label.eq_ignore_ascii_case(&label));
    let Some(i) = matched else {
        e.actions.push(Action {
            type_id,
            label,
            counter: 0,
            rate,
            domain: None,
            domain1: Some(domain),
            domain2: Some(domain2),
            bone,
        });
        return true;
    };
    if e.actions[i].type_id != type_id {
        return false;
    }
    let node = &mut e.actions[i];
    node.rate = rate;
    node.domain1 = Some(domain);
    node.domain2 = Some(domain2);
    node.bone = bone;
    true
}

/// Build one `0x20`-byte domain object from two extent operands.
///
/// The exe loads the first operand, then the second as `ST(0)`, and compares
/// with `fcomp st(1)` + `fnstsw`; `test ah,0x41` is nonzero for C0
/// (`ST(0) < ST(1)`) and C3 (equal) — and for unordered (NaN) — so the
/// swapped store runs only on the ordered `ST(0) > ST(1)`. Everything else
/// stores `(a, b)`. `setCounter` feeds `(rate, 0.0f)`: the extents are
/// `(rate, 0)` for `rate >= 0` or NaN and `(0, rate)` for `rate < 0`; the
/// squared extents are `ext[i] * ext[i]` computed at x87 PC=64.
///
/// exe: LAB_0047f270
/// exe: LAB_0047f660
/// exe: LAB_0047fa70
/// oracle: Unicorn `re/oracle/res_vectors.py::domain` (local only).
#[must_use]
pub fn build_domain(a_bits: u32, b_bits: u32) -> Domain {
    let a = f32::from_bits(a_bits);
    let b = f32::from_bits(b_bits);
    // Ordered `ST(0) > ST(1)` is the only swap case; Rust's `>` is already
    // false for equal and for NaN operands.
    let swap = b > a;
    let ext = if swap {
        [b_bits, a_bits]
    } else {
        [a_bits, b_bits]
    };
    // Squared at PC=64: f64-rounded `ext*ext`, narrowed to f32. f32
    // multiply is correctly rounded and exact here (same value twice), so
    // the f64 detour is unobservable; keep the direct product.
    let sq = |w: u32| {
        let v = f32::from_bits(w);
        (v * v).to_bits()
    };
    Domain {
        ext,
        ext_sq: [sq(ext[0]), sq(ext[1])],
    }
}

/// `PhysicsRef` body-slot address: `base + slot*0xd4`.
///
/// The exe computes `(slot*3, *9, *2, -slot)` i.e. `slot*53*4 = slot*0xd4`
/// off `engine+0x84`, indexed by the word at `engine+0x20c`; position
/// fields live at `+0x64`, orientation at `+0xa4` (via `FUN_00551c90`).
///
/// exe: FUN_00480b00
/// exe: LAB_00480c10
/// exe: LAB_00480920
/// oracle: Unicorn `re/oracle/res_vectors.py::phys_slot` (local only).
#[must_use]
pub const fn phys_slot_addr(base: u32, slot: u32) -> u32 {
    base.wrapping_add(slot.wrapping_mul(0xd4))
}

/// `SfxRef.nplay` volume encoding: `DAT_00612c58 * vol`.
///
/// exe: FUN_00480d40
/// `fmuls` at `0x480dee`.
/// oracle: Unicorn `re/oracle/res_vectors.py::sfx_vol` (local only).
#[must_use]
pub fn sfx_scaled_volume(unit_vol_bits: u32, vol: f32) -> u32 {
    (f32::from_bits(unit_vol_bits) * vol).to_bits()
}

/// `RenderRef.getDetail()` detail-word rule.
///
/// `env+0x4c` must be 1 or 3 (else the error path yields the float default);
/// then the object word at `env+0x8` must be nonzero (else default); the
/// result is the float at `env+0x6c`.
///
/// exe: LAB_00480440
/// oracle: Unicorn `re/oracle/res_vectors.py::render_detail` (local only).
#[must_use]
pub const fn render_detail(mode: u32, obj_word: u32, detail_bits: u32, default_bits: u32) -> u32 {
    if mode != 1 && mode != 3 {
        return default_bits;
    }
    if obj_word == 0 {
        return default_bits;
    }
    detail_bits
}

/// `RenderRef.getTypeID()` gate: the object word `[obj+0xc]` must be
/// non-null, its mode word `[env+0x4c]` must be 3, and the `0x419860`
/// envelope fetch (`0xa0000001`, `1.0f`, `0`, `10.0f`) must return non-null;
/// only then does the native read the result `[envelope+0x80]`.
///
/// The exe tests the env word *ahead of* the mode compare
/// (`0x486c07` `mov ecx,[eax+0xc]` / `test ecx,ecx` / `je 0x486c74`
/// precede `cmp dword [ecx+0x4c],3` at `0x486c0e`), and the fetch is only
/// reached when both pass (`0x486c25` / `test eax,eax` / `je 0x486c74`).
/// Every miss edge lands on `0x486c74` = `xor eax,eax; ret` (the null-`obj`
/// edge at `0x486c35` additionally logs and then falls into the same zero
/// return), so a null env short-circuits regardless of `mode` and a null
/// envelope returns 0.
///
/// exe: LAB_00486be0
/// oracle: Unicorn `re/oracle/res_vectors.py::render_gettypeid` (local only).
#[must_use]
pub const fn render_type_gate(env: u32, mode: u32, envelope: u32) -> bool {
    if env == 0 {
        return false;
    }
    if mode != 3 {
        return false;
    }
    envelope != 0
}

/// `SfxRef.nplay` Vector3 gate: the argument copy of the `Vector3` is
/// forwarded to the engine tail `0x48cfb0` only when the argument pointer is
/// non-null.
///
/// The `id & 0xffff` window compare at `0x480dd3` is dead: `jb`/`jae` both
/// jump to the join at `0x480de2`, and the only instruction skipped
/// (`0x480ddf mov eax,[ebp+8]`) re-loads the very word the fall-through path
/// already holds (`0x480d89`/`0x480dc4`), so no value of the object word at
/// `obj+8` changes what the tail's `neg`/`sbb`/`and` turns into
/// `vector ? &copy : 0` at `0x480df6`.
///
/// exe: FUN_00480d40
///
/// The gate is `0x480d8c` (`test eax,eax` / `je 0x480dca`) and the
/// `vector ? &copy : 0` select `0x480df6`
/// (`neg`/`sbb eax,eax`/`and eax,edx`).
/// oracle: Unicorn `re/oracle/res_vectors.py::sfx_nplay` (local only).
#[must_use]
pub const fn sfx_target_in_range(vec_ptr: u32) -> bool {
    vec_ptr != 0
}

/// The engine-call sink: every native tail that needs the render, physics,
/// particle or audio engine reports its call here instead of executing it.
/// Headless backends (M1-12) implement this as call-log records; tests use
/// [`Recorder`].
pub trait EngineSink {
    /// `RenderRef.create(res, parent, name)`.
    ///
    /// exe: LAB_00480ee0
    /// Engine `FUN_00539880`.
    fn render_create(&mut self, res: u32, parent: u32);
    /// `RenderRef.changeResource(old, new)`.
    ///
    /// exe: LAB_00480220
    fn render_change_resource(&mut self, old: u32, nu: u32);
    /// `RenderRef.setColor(argb)`.
    ///
    /// exe: FUN_00480310
    /// Engine `FUN_0048c8c0`.
    fn render_set_color(&mut self, handle: u32, argb: u32);
    /// `RenderRef.setLight(r, g, b)` after default-fill (`0xffffff`).
    ///
    /// **Argument order is the exe's, not the script's.** The three pushed
    /// words reach `FUN_0048c9d0` reversed and with a trailing zero:
    /// `setLight(1, 2, 3)` arrives as `[3, 2, 1, 0]`, and
    /// `setLight(255, 128, 64)` as `[64, 128, 255, 0]`
    /// (`re/out/res/res_handles_render_light.jsonl` rows 1 and 2, Unicorn).
    /// A backend that forwards `rgb` in script order diverges.
    ///
    /// exe: LAB_00486ab0
    /// Engine `FUN_0048c9d0`.
    fn render_set_light(&mut self, handle: u32, rgb: [u32; 3]);
    /// `RenderRef.setFlare(res, a, b, c, d, e)`.
    ///
    /// exe: LAB_00486b20
    /// Engine `FUN_0048cb40`.
    fn render_set_flare(&mut self, handle: u32, res: u32, args: [u32; 5]);
    /// `RenderRef.setType(res)`.
    ///
    /// exe: FUN_00486ba0
    /// Engine `FUN_0048c970`.
    fn render_set_type(&mut self, handle: u32, res: u32);
    /// `RenderRef.setMatrix` / `PhysicsRef.setMatrix` resolved matrix write.
    ///
    /// exe: LAB_004810b0
    /// exe: LAB_00481240
    /// exe: LAB_00480920
    /// Matrix via `FUN_00551c70`, write via `FUN_0048bf50` / physics twin.
    fn set_matrix(&mut self, handle: u32, matrix_words: [u32; 16]);
    /// `RenderRef.lineCreate(a, b) -> line id`.
    ///
    /// exe: LAB_0047fe70
    fn line_create(&mut self, a: u32, b: u32) -> u32;
    /// `RenderRef.lineAdd(p0, p1, mode, freq)`.
    ///
    /// exe: LAB_0047fee0
    fn line_add(&mut self, p0: [u32; 3], p1: [u32; 3], mode: u32, freq_bits: u32);
    /// `RenderRef.plotRoute(a, b, mode, freq, pos) -> route id`.
    ///
    /// exe: LAB_00483960
    fn plot_route(&mut self, a: u32, b: u32, mode: u32, freq_bits: u32, pos: [u32; 3]) -> u32;
    /// `RenderRef.getBoneId(name) -> bone id` (engine `FUN_0048bc40`).
    ///
    /// exe: FUN_00481020
    fn get_bone_id(&mut self, handle: u32, name: &str) -> u32;
    /// `PhysicsRef.create/createBox/createSphere` body creation.
    ///
    /// exe: LAB_004807f0
    /// exe: LAB_004805b0
    /// exe: LAB_004806f0
    /// Engine `FUN_0048a1e0`.
    fn phys_create(&mut self, res: u32, extents: [u32; 3], name: &str);
    /// `Animation.<init>` envelope construction.
    ///
    /// exe: LAB_0047ea10
    /// Engine `0x541400/0x541600/0x541810/0x541860`.
    fn anim_create(&mut self, render: u32, res: u32);
    /// `Animation.finalize` envelope release (`vtable[0](1)`).
    ///
    /// exe: LAB_0047eba0
    fn anim_release(&mut self, handle: u32);
    /// `Animation.getPos` engine sample (`FUN_0053fe70`).
    ///
    /// exe: LAB_0047f050
    fn anim_sample(&mut self, handle: u32) -> u32;
    /// `ParticleSystem.init` (`FUN_0048a490`).
    ///
    /// exe: LAB_0047f0b0
    fn psys_init(&mut self, handle: u32, res: u32, render: u32);
    /// `ParticleSystem.stop` (`FUN_0048a610`).
    ///
    /// exe: LAB_0047f120
    fn psys_stop(&mut self, handle: u32);
    /// `ParticleSystem.setDirectSource/setSource` node link
    /// (`FUN_0045f5a0` + `FUN_0045faf0`, type `0x5f1128`/`0x5f138c`).
    ///
    /// exe: LAB_0047f270
    /// exe: LAB_0047f660
    fn psys_link_source(&mut self, handle: u32, type_id: u32);
    /// `PhysicsRef.getPos/getOri` engine pointer (`FUN_0048aea0`).
    ///
    /// exe: FUN_00480b00
    /// exe: LAB_00480c10
    fn phys_body_ptr(&mut self, handle: u32) -> u32;
    /// `SfxRef.nplay` play (`FUN_0048cfb0`, 8 engine args).
    ///
    /// exe: FUN_00480d40
    fn sfx_play(&mut self, handle: u32, args: [u32; 8]) -> u32;
    /// `SfxRef.stop` (`FUN_0048d180`).
    ///
    /// exe: FUN_00480e60
    fn sfx_stop(&mut self, handle: u32, channel: u32);
}

/// Test recorder implementing [`EngineSink`]: captures every engine call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorder {
    /// Captured calls, in order, as `name(arg, ...)` strings.
    pub calls: Vec<String>,
}

impl Recorder {
    /// A fresh recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineSink for Recorder {
    fn render_create(&mut self, res: u32, parent: u32) {
        self.calls.push(format!("render_create({res},{parent})"));
    }
    fn render_change_resource(&mut self, old: u32, nu: u32) {
        self.calls.push(format!("render_change({old},{nu})"));
    }
    fn render_set_color(&mut self, handle: u32, argb: u32) {
        self.calls.push(format!("set_color({handle},{argb:#x})"));
    }
    fn render_set_light(&mut self, handle: u32, rgb: [u32; 3]) {
        self.calls.push(format!(
            "set_light({handle},{},{},{})",
            rgb[0], rgb[1], rgb[2]
        ));
    }
    fn render_set_flare(&mut self, handle: u32, res: u32, args: [u32; 5]) {
        self.calls.push(format!(
            "set_flare({handle},{res},{},{},{},{},{})",
            args[0], args[1], args[2], args[3], args[4]
        ));
    }
    fn render_set_type(&mut self, handle: u32, res: u32) {
        self.calls.push(format!("set_type({handle},{res})"));
    }
    fn set_matrix(&mut self, handle: u32, matrix_words: [u32; 16]) {
        self.calls
            .push(format!("set_matrix({handle},{matrix_words:08x?})"));
    }
    fn line_create(&mut self, a: u32, b: u32) -> u32 {
        self.calls.push(format!("line_create({a},{b})"));
        0
    }
    fn line_add(&mut self, p0: [u32; 3], p1: [u32; 3], mode: u32, freq_bits: u32) {
        self.calls.push(format!(
            "line_add({p0:08x?},{p1:08x?},{mode},{freq_bits:#x})"
        ));
    }
    fn plot_route(&mut self, a: u32, b: u32, mode: u32, freq_bits: u32, pos: [u32; 3]) -> u32 {
        self.calls.push(format!(
            "plot_route({a},{b},{mode},{freq_bits:#x},{pos:08x?})"
        ));
        0
    }
    fn get_bone_id(&mut self, handle: u32, name: &str) -> u32 {
        self.calls.push(format!("get_bone_id({handle},{name})"));
        0
    }
    fn phys_create(&mut self, res: u32, extents: [u32; 3], name: &str) {
        self.calls
            .push(format!("phys_create({res},{extents:08x?},{name})"));
    }
    fn anim_create(&mut self, render: u32, res: u32) {
        self.calls.push(format!("anim_create({render},{res})"));
    }
    fn anim_release(&mut self, handle: u32) {
        self.calls.push(format!("anim_release({handle})"));
    }
    fn anim_sample(&mut self, handle: u32) -> u32 {
        self.calls.push(format!("anim_sample({handle})"));
        0
    }
    fn psys_init(&mut self, handle: u32, res: u32, render: u32) {
        self.calls
            .push(format!("psys_init({handle},{res},{render})"));
    }
    fn psys_stop(&mut self, handle: u32) {
        self.calls.push(format!("psys_stop({handle})"));
    }
    fn psys_link_source(&mut self, handle: u32, type_id: u32) {
        self.calls.push(format!("psys_link({handle},{type_id:#x})"));
    }
    fn phys_body_ptr(&mut self, handle: u32) -> u32 {
        self.calls.push(format!("phys_body_ptr({handle})"));
        0
    }
    fn sfx_play(&mut self, handle: u32, args: [u32; 8]) -> u32 {
        self.calls.push(format!("sfx_play({handle},{args:08x?})"));
        0
    }
    fn sfx_stop(&mut self, handle: u32, channel: u32) {
        self.calls.push(format!("sfx_stop({handle},{channel})"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_alloc_ids_start_at_one() {
        let mut t = HandleTable::new();
        assert!(t.is_empty());
        let a = t.alloc(HandleKind::Animation, vec![]);
        let b = t.alloc(HandleKind::RenderRef, vec![1, 2, 3]);
        assert_eq!((a, b), (1, 2));
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(a).unwrap().kind, HandleKind::Animation);
        assert_eq!(t.get(b).unwrap().words, vec![1, 2, 3]);
        assert!(t.free(a));
        assert!(!t.free(a));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn anim_queue_tags_match_exe() {
        assert_eq!(AnimCommand::Play.tag(), 0);
        assert_eq!(AnimCommand::Loop.tag(), 1);
        assert_eq!(AnimCommand::Pause.tag(), 3);
        assert_eq!(AnimCommand::Seek.tag(), 4);
        assert_eq!(AnimCommand::SetSpeed.tag(), 5);
        assert_eq!(AnimCommand::SetFade.tag(), 6);
        assert!(AnimCommand::SetSpeed.has_float_payload());
        assert!(!AnimCommand::Play.has_float_payload());
    }

    #[test]
    fn anim_push_appends_in_order() {
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::Animation, vec![]);
        assert!(anim_push(&mut t, id, AnimCommand::Play, 0));
        assert!(anim_push(&mut t, id, AnimCommand::SetSpeed, 0x3f80_0000));
        assert_eq!(
            t.get(id).unwrap().queue,
            vec![
                Command { tag: 0, payload: 0 },
                Command {
                    tag: 5,
                    payload: 0x3f80_0000
                },
            ]
        );
        // Wrong kind / dead id are silent misses (the exe's gate returns).
        let r = t.alloc(HandleKind::RenderRef, vec![]);
        assert!(!anim_push(&mut t, r, AnimCommand::Play, 0));
        assert!(!anim_push(&mut t, 999, AnimCommand::Play, 0));
    }

    #[test]
    fn perm_bits_follow_exe_branches() {
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        assert!(psys_mode_permanent(&mut t, id, 1));
        assert_eq!(t.get(id).unwrap().flags & 0x2000_0000, 0x2000_0000);
        assert!(psys_mode_permanent(&mut t, id, 0));
        assert_eq!(t.get(id).unwrap().flags & 0x2000_0000, 0);
        // Other values leave the word untouched.
        t.get_mut(id).unwrap().flags = 0xffff_ffff;
        assert!(psys_mode_permanent(&mut t, id, 7));
        assert_eq!(t.get(id).unwrap().flags, 0xffff_ffff);
    }

    /// One pre-existing action node for the particle-list tests.
    fn act(type_id: u32, label: &str, counter: u32, rate: u32) -> Action {
        Action {
            type_id,
            label: label.to_owned(),
            counter,
            rate,
            domain: None,
            domain1: None,
            domain2: None,
            bone: 0,
        }
    }

    #[test]
    fn counter_get_stops_at_first_label_match() {
        // The walk compares labels only, so a first match of another type
        // returns 0 and must not zero that node or any later counter node.
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        t.get_mut(id).unwrap().actions = vec![
            act(0xa, "counter_a", 5, 0),
            act(0xd, "counter_a", 7, 0),
            act(0xd, "counter_b", 9, 0),
        ];
        assert_eq!(psys_get_counter(&mut t, id, "counter_a"), 0);
        assert_eq!(psys_get_counter(&mut t, id, "counter_b"), 9);
        assert_eq!(psys_get_counter(&mut t, id, "COUNTER_B"), 0); // case-insens., now zero
        assert_eq!(psys_get_counter(&mut t, id, "missing"), 0);
        let acts = &t.get(id).unwrap().actions;
        assert_eq!((acts[0].counter, acts[1].counter), (5, 7));
        // The exe's every early-out returns the type default, so a dead id,
        // a wrong kind and an empty list all answer 0.
        let r = t.alloc(HandleKind::RenderRef, vec![]);
        assert_eq!(psys_get_counter(&mut t, r, "counter_a"), 0);
        assert_eq!(psys_get_counter(&mut t, 999, "counter_a"), 0);
        let e = t.alloc(HandleKind::ParticleSystem, vec![]);
        assert_eq!(psys_get_counter(&mut t, e, "counter_a"), 0);
    }

    #[test]
    fn set_counter_reuses_or_appends_per_node_type() {
        let dom = Domain {
            ext: [0x4080_0000, 0],
            ext_sq: [0x4180_0000, 0],
        };
        // Reuse: only +0x60 is replaced, +0x54/+0x58 keep their words.
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        t.get_mut(id).unwrap().actions = vec![act(0xd, "counter_a", 7, 0x1111)];
        assert!(psys_set_counter(&mut t, id, "COUNTER_A".to_owned(), dom));
        let a = &t.get(id).unwrap().actions;
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].counter, a[0].rate), (7, 0x1111));
        assert_eq!(a[0].domain, Some(dom));
        // Append: a fresh type-0xd node with +0x54/+0x58 zero.
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        t.get_mut(id).unwrap().actions = vec![act(0xa, "source_a", 0, 0x1111)];
        assert!(psys_set_counter(&mut t, id, "counter_a".to_owned(), dom));
        let a = &t.get(id).unwrap().actions;
        assert_eq!(a.len(), 2);
        assert_eq!((a[1].type_id, a[1].label.as_str()), (0xd, "counter_a"));
        assert_eq!((a[1].counter, a[1].rate), (0, 0));
        assert_eq!(a[1].domain, Some(dom));
        assert_eq!((a[0].counter, a[0].rate), (0, 0x1111)); // source untouched
                                                            // First label match of another type aborts: nothing appended, no
                                                            // domain built for it.
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        t.get_mut(id).unwrap().actions = vec![act(0xa, "counter_a", 5, 0)];
        assert!(!psys_set_counter(&mut t, id, "counter_a".to_owned(), dom));
        let a = &t.get(id).unwrap().actions;
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].type_id, a[0].domain), (0xa, None));
        assert!(!psys_set_counter(&mut t, 999, "x".to_owned(), dom));
    }

    /// H3b: `FUN_00480d40`'s `id & 0xffff` compare at `0x480dd3` is dead
    /// (both edges re-load `eax` with `[ebp+8]`), so the forwarded word is
    /// decided by the `Vector3` argument pointer alone. The oracle rows
    /// `res_gate_sfx_nplay` confirm it: targets `0x25`, `0x2c`, `0x10025`,
    /// `0x12345678` (outside the `0x26..0x2c` window) still forward the
    /// copy, and only the null-`Vector3` row forwards 0.
    #[test]
    fn sfx_target_in_range_tests_the_vector_pointer_only() {
        for vec_ptr in [1u32, 0x1000, 0x7fff_f000, u32::MAX] {
            assert!(sfx_target_in_range(vec_ptr), "{vec_ptr:#x}");
        }
        assert!(!sfx_target_in_range(0));
    }

    /// H3b: `LAB_00486be0` order — null env word `[obj+0xc]` misses *before*
    /// the `cmp [env+0x4c],3`, and a null `0x419860` envelope misses after
    /// it; all three edges return 0 (oracle `res_gate_render_gettypeid` rows
    /// 7 and 8: `env_null` never fetches, `fetch_null` fetches and returns 0).
    #[test]
    fn render_type_gate_orders_env_null_before_mode_and_gates_the_envelope() {
        assert!(render_type_gate(0x2000, 3, 0x3000));
        // Null env: the mode word is never read, so even mode 3 misses.
        assert!(!render_type_gate(0, 3, 0x3000));
        assert!(!render_type_gate(0, 0x1234, 0x3000));
        assert!(!render_type_gate(0, 3, 0));
        // Live env, any mode but 3.
        for mode in [0u32, 1, 2, 4, 0xffff_ffff] {
            assert!(!render_type_gate(0x2000, mode, 0x3000), "{mode}");
        }
        // Live env and mode 3, but the envelope fetch returned null.
        assert!(!render_type_gate(0x2000, 3, 0));
    }

    #[test]
    fn gates_match_exe_constants() {
        assert_eq!(render_detail(1, 9, 0x3f00_0000, 0), 0x3f00_0000);
        assert_eq!(render_detail(3, 9, 0x3f00_0000, 0), 0x3f00_0000);
        assert_eq!(render_detail(0, 9, 0x3f00_0000, 0x5e73cc), 0x5e73cc);
        assert_eq!(render_detail(1, 0, 0x3f00_0000, 0x5e73cc), 0x5e73cc);
        assert_eq!(phys_slot_addr(0x1000, 3), 0x1000 + 3 * 0xd4);
        assert_eq!(sfx_scaled_volume(0x3f80_0000, 0.5), 0x3f00_0000);
    }

    #[test]
    fn del_action_drops_first_case_insensitive_match() {
        let mut t = HandleTable::new();
        let id = t.alloc(HandleKind::ParticleSystem, vec![]);
        t.get_mut(id).unwrap().actions =
            vec![act(0xa, "counter_a", 5, 0), act(0xd, "counter_a", 7, 0)];
        // Label-only walk, case-insensitive: the type-0xa node goes first.
        assert!(psys_del_action(&mut t, id, "COUNTER_A"));
        let a = &t.get(id).unwrap().actions;
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].type_id, 0xd);
        assert!(!psys_del_action(&mut t, id, "counter_b"));
        assert!(psys_del_action(&mut t, id, "counter_a"));
        assert!(t.get(id).unwrap().actions.is_empty());
    }

    #[test]
    fn domain_extent_order_matches_fcomp() {
        // setCounter feeds (rate, 0.0f) and the exe stores (a, b) unless the
        // ordered ST(0) > ST(1): the extents are (rate, 0) for rate >= 0 or
        // NaN and (0, rate) for rate < 0.
        let zero = 0.0f32.to_bits();
        for rate in [
            0.0f32,
            -0.0,
            1.0,
            1e-45,
            3.4028235e38,
            f32::INFINITY,
            f32::NAN,
        ] {
            assert_eq!(build_domain(rate.to_bits(), zero).ext, [rate.to_bits(), 0]);
        }
        for rate in [-1.0f32, -2.5, f32::NEG_INFINITY] {
            assert_eq!(build_domain(rate.to_bits(), zero).ext, [0, rate.to_bits()]);
        }
        // Both squares are ext[i]*ext[i] at x87 PC=64.
        let d = build_domain(0x4000_0000, 0x3f80_0000); // (2.0, 1.0)
        assert_eq!(d.ext, [0x4000_0000, 0x3f80_0000]);
        assert_eq!(d.ext_sq, [0x4080_0000, 0x3f80_0000]);
        // 3.4028235e38 squares past the f32 range; 1e-45 squares to zero.
        assert_eq!(build_domain(0x7f7f_ffff, zero).ext_sq, [0x7f80_0000, 0]);
        assert_eq!(build_domain(1, zero).ext_sq, [0, 0]);
    }
}
