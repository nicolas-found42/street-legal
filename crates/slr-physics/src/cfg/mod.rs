//! The native cfg keyword handlers of the car-class loader (M3-09, issue #50).
//!
//! The exe parses every vehicle, part and click cfg through one keyword table (`0x60e0f8`, 107
//! rows, 104 distinct handlers) driven by `FUN_004b6340`. Each handler is called as
//! `handler(line, K)` where `line` is the text line with its first delimiter after the keyword
//! overwritten by NUL (the handler reads its arguments from just past that NUL) and `K` is the
//! car-class object (`0x2e98` bytes, built by the factory `FUN_00442500`). Handlers write into
//! `K`, the default parameter block `P = *(K+0x2e48)` and heap nodes they allocate.
//!
//! Port model: objects live in a [`Space`] at their original layout (see its docs); handlers are
//! transliterations of the exe code with raw offsets, which is what makes whole-memory
//! differential tests possible. Offsets are bytes from `K` unless a comment says `P`.
//!
//! Calls the handlers make outside their own logic go through [`Host`]: the physics world (body
//! factories `FUN_0049ab60`/`FUN_0049a990`), the resource manager (`FUN_00544590`,
//! `FUN_00546070`, `FUN_00538440`, `FUN_005383f0`), the diagnostics log (`FUN_005513b0`) and the
//! x87 matrix helpers (`FUN_0054ecd0`, `FUN_00551c90`). Everything else the handlers call (the
//! vec3 helpers, list-node constructors, `stricmp`, `memcpy`, `sscanf`) is ported here.

use crate::space::Space;
use slr_fmt_cfg::scan::{sscanf, Scan};
use slr_fmt_cfg::Precision;

mod hud;
mod part;
mod vehicle;
mod wheel;

pub use hud::*;
pub use part::*;
pub use vehicle::*;
pub use wheel::*;
// `damage` (FUN_0046cf50) is defined identically in `part` and `vehicle`; the table uses the
// `vehicle` copy and the explicit re-export below pins it for glob importers.
pub use vehicle::damage;

/// An argument of a [`Host`] call.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    U(u32),
    S(String),
}

/// The `this` (ECX) of a thiscall to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum This {
    /// Not recorded (calls whose receiver is a global).
    None,
    /// A stack temporary of the handler (not vehicle state).
    Local,
    Addr(u32),
}

/// One call from a handler to code that is not part of the cfg handlers.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtCall {
    /// Address of the exe function.
    pub f: u32,
    pub this: This,
    pub a: Vec<Arg>,
}

/// Exe functions outside the handlers (see the module docs). `call` performs the call, applies
/// its effects to `m` and returns EAX.
pub trait Host {
    fn call(&mut self, m: &mut Space, c: &ExtCall) -> u32;
}

/// `FUN_00544590`: resource id -> handle.
pub const EXT_RESOLVE: u32 = 0x544590;
/// `FUN_00546070`: bind a resource reference slot (`this`) to (handle, kind, flag).
pub const EXT_BIND: u32 = 0x546070;
/// `FUN_00538440`: (resource package name, index) -> resource id.
pub const EXT_LOOKUP: u32 = 0x538440;
/// `FUN_005383f0`: resource id -> name (diagnostics).
pub const EXT_RESNAME: u32 = 0x5383f0;
/// `FUN_005513b0`: printf-style diagnostics.
pub const EXT_LOG: u32 = 0x5513b0;
/// `FUN_0049ab60`: physics world, create a primitive body.
pub const EXT_BODY: u32 = 0x49ab60;
/// `FUN_0049a990`: physics world, create a body from a click definition.
pub const EXT_BODYDEF: u32 = 0x49a990;
/// `FUN_0054ecd0`: euler angles -> 3x3 matrix (rows of stride 16 bytes) at `this`.
pub const EXT_EULER_TO_MATRIX: u32 = 0x54ecd0;
/// `FUN_00551c90`: matrix -> euler angles at `this`.
pub const EXT_MATRIX_TO_EULER: u32 = 0x551c90;
/// The physics world object the body factories are called on (`PTR_DAT_00617650` -> `0x6409c4`).
pub const PHYSICS_WORLD: u32 = 0x6409c4;

/// Handler context: the memory, the host and the car-class object `K`.
pub struct Ctx<'a> {
    pub m: &'a mut Space,
    pub host: &'a mut dyn Host,
    pub k: u32,
}

/// Signature of a keyword handler: `(line, K)`.
pub type Handler = fn(&mut Ctx, &[u8]);

/// Every ported keyword handler, in exe table order (`0x60e0f8`).
///
/// Aliases share their handler (`driver`/`controller`, `sw_radius`/`steeringradius`;
/// `render`/`light` and `slotflags`/`slottype` land with their modules).
pub fn table() -> Vec<(&'static str, Handler)> {
    vec![
        ("click", part::click),
        ("body", part::body),
        ("bone", part::bone),
        ("noclick", part::noclick),
        ("nocollision", part::nocollision),
        ("buoy", part::buoy),
        ("render", hud::render),
        ("light", hud::render),
        ("deformable", part::deformable),
        ("shadow", part::shadow),
        ("flags", part::flags),
        ("lod", part::lod),
        ("lods", part::lods),
        ("colors", part::colors),
        ("category", part::category),
        ("slot", part::slot),
        ("bolt", part::bolt),
        ("attach", part::attach),
        ("compatible", part::compatible),
        ("slotdmgmode", part::slotdmgmode),
        ("slotdeform", part::slotdeform),
        ("slottype", part::slottype),
        ("slotflags", part::slottype),
        ("slotmove", part::slotmove),
        ("deform", part::deform),
        ("mesh", part::mesh),
        ("texture", part::texture),
        ("logo", part::logo),
        ("flap", part::flap),
        ("flapsfx", part::flapsfx),
        ("camera", hud::camera),
        ("type", wheel::type_),
        ("friction", wheel::friction),
        ("spring", wheel::spring),
        ("wheel", wheel::wheel),
        ("wheelbones", wheel::wheelbones),
        ("bonepos", wheel::bonepos),
        ("wheelpars", wheel::wheelpars),
        ("skid", wheel::skid),
        ("wheeldeform", wheel::wheeldeform),
        ("arm", wheel::arm),
        ("hub", wheel::hub),
        ("instant_center", wheel::instant_center),
        ("steering", wheel::steering),
        ("sw_radius", vehicle::sw_radius),
        ("steeringradius", vehicle::sw_radius),
        ("maxsteer", vehicle::maxsteer),
        ("rpm_idle", vehicle::rpm_idle),
        ("drag", vehicle::drag),
        ("wing", vehicle::wing),
        ("center", vehicle::center),
        ("rollbar", vehicle::rollbar),
        ("linked", part::linked),
        ("sfx_engine_up", hud::sfx_engine_up),
        ("sfx_engine_down", hud::sfx_engine_down),
        ("sfx_exhaust", hud::sfx_exhaust),
        ("sfx_gearbox_fwd", hud::sfx_gearbox_fwd),
        ("sfx_gearbox_rev", hud::sfx_gearbox_rev),
        ("sfx_wreck", hud::sfx_wreck),
        ("sfx_horn", hud::sfx_horn),
        ("sfx_gearchange", hud::sfx_gearchange),
        ("mass", vehicle::mass),
        ("enginepower", vehicle::enginepower),
        ("ratios", vehicle::ratios),
        ("torque", vehicle::torque),
        ("rearendratio", vehicle::rearendratio),
        ("brakepower", vehicle::brakepower),
        ("clutch", vehicle::clutch),
        ("driver", vehicle::driver),
        ("controller", vehicle::driver),
        ("ext_camera", vehicle::ext_camera),
        ("nodriver", vehicle::nodriver),
        ("name", vehicle::name),
        ("seat", vehicle::seat),
        ("pedals", vehicle::pedals),
        ("shifter", vehicle::shifter),
        ("key", vehicle::key),
        ("handbrake", vehicle::handbrake),
        ("cockpit_rpm", hud::cockpit_rpm),
        ("cockpit_speed", hud::cockpit_speed),
        ("osd_gauge", hud::osd_gauge),
        ("osd_rpmpin", hud::osd_rpmpin),
        ("osd_spdpin", hud::osd_spdpin),
        ("osd_nospin", hud::osd_nospin),
        ("osd_damage", hud::osd_damage),
        ("osd_gear", hud::osd_gear),
        ("osd_speed", hud::osd_speed),
        ("osd_rpm", hud::osd_rpm),
        ("osd_nos", hud::osd_nos),
        ("osd_turbo", hud::osd_turbo),
        ("osd_mileage", hud::osd_mileage),
        ("osd_abs", hud::osd_abs),
        ("osd_asr", hud::osd_asr),
        ("osd_hbrake", hud::osd_hbrake),
        ("osd_emergency", hud::osd_emergency),
        ("osd_damage_slot", hud::osd_damage_slot),
        ("stockpart", part::stockpart),
        ("material", vehicle::material),
        ("damage", vehicle::damage),
        ("damagedef", vehicle::damagedef),
        ("velfactor", vehicle::velfactor),
        ("turn", vehicle::turn),
        ("roll", vehicle::roll),
        ("pitch", vehicle::pitch),
        ("steerhelp", vehicle::steerhelp),
        ("cheat_control", vehicle::cheat_control),
        ("notify", vehicle::notify),
    ]
}

impl<'a> Ctx<'a> {
    pub fn new(m: &'a mut Space, host: &'a mut dyn Host, k: u32) -> Ctx<'a> {
        Ctx { m, host, k }
    }

    /// Read the word at `K + off`.
    pub fn kr(&self, off: u32) -> u32 {
        self.m.r32(self.k.wrapping_add(off))
    }

    pub fn kw(&mut self, off: u32, v: u32) {
        self.m.w32(self.k.wrapping_add(off), v);
    }

    pub fn kf(&self, off: u32) -> f32 {
        f32::from_bits(self.kr(off))
    }

    /// The default parameter block `P = *(K+0x2e48)`.
    pub fn p(&self) -> u32 {
        self.kr(0x2e48)
    }

    /// The "already configured" guard every handler tests first (`K+0x120 == 0`).
    pub fn guard(&self) -> bool {
        self.kr(0x120) == 0
    }

    /// Body-slot count `K+0xfa8` (signed in the exe's compares).
    pub fn nbody(&self) -> i32 {
        self.kr(0xfa8) as i32
    }

    /// Wheel count `K+8000` (`0x1f40`).
    pub fn nwheel(&self) -> i32 {
        self.kr(0x1f40) as i32
    }

    pub fn r(&self, a: u32) -> u32 {
        self.m.r32(a)
    }

    pub fn w(&mut self, a: u32, v: u32) {
        self.m.w32(a, v);
    }

    pub fn rf(&self, a: u32) -> f32 {
        self.m.rf(a)
    }

    pub fn wf(&mut self, a: u32, v: f32) {
        self.m.wf(a, v);
    }

    /// Store `v` at `a` if the conversion ran (the exe leaves unconverted destinations alone).
    pub fn put(&mut self, a: u32, v: Option<u32>) {
        if let Some(v) = v {
            self.m.w32(a, v);
        }
    }

    pub fn call(&mut self, f: u32, this: This, a: Vec<Arg>) -> u32 {
        let c = ExtCall { f, this, a };
        self.host.call(self.m, &c)
    }

    /// `FUN_00544590` with the resource manager `*(K+0x124)` as `this` (the exe's `ECX`).
    ///
    /// Every `FUN_00544590` call the cfg handlers make sets `ECX = *(K+0x124)` immediately
    /// before the call; there is no receiver-less site. Verified site by site by Unicorn
    /// disassembly of `reference/raw/StreetLegal_Redline.bak.exe` (absolute `call 0x544590`
    /// preceded in the same block by `mov ecx, [base+0x124]`, `base` = the handler's `K`,
    /// `P`-owner or `ebx`/`esi`/`eax` holding it):
    ///
    /// | exe function | `ECX` load | resolve sites |
    /// |---|---|---|
    /// | `FUN_0046b950` click | `0x46ba37 [esi+0x124]` | 1 |
    /// | `FUN_0046bb80` body | `0x46bc6c [esi+0x124]` | 1 (`%d` shape branch only) |
    /// | `FUN_0046bfa0` deformable | `0x46c07c`/`0x46c09c`/`0x46c0c7 [esi+0x124]` | 3 |
    /// | `FUN_0046c1a0` shadow | `0x46c252 [edi+0x124]` | 1 |
    /// | `FUN_0046c4e0` bolt | `0x46c5e4 [ebx+0x124]` | 1 |
    /// | `FUN_0046c6a0` attach | `[K+0x124]` (`attach_node`) | 1 |
    /// | `FUN_0046c770` compatible | `[K+0x124]` (`attach_node`) | 1 |
    /// | `FUN_0046ca70` flapsfx | `0x46caf6`/`0x46cb0e [ebx+0x124]` | 2 |
    /// | `FUN_0046cdf0` texture | `[K+0x124]` | 1 |
    /// | `FUN_0046ce60` mesh | `[K+0x124]` | 1 |
    /// | `FUN_0046cff0` material2 | `0x46d05c`/`0x46d07c`/`0x46d09d [ebx+0x124]` | 3 |
    /// | `FUN_0046d400` logo | `0x46d440 [esi+0x124]` | 1 |
    /// | `FUN_0046d970` stockpart | `0x46d9ec [ebx+0x124]` | 1 |
    /// | `FUN_0046da30` stockpart2 | `0x46da7e [eax+0x124]` | 1 |
    /// | `FUN_00444b60` cockpit_rpm | `0x444c20 [esi+0x124]` | 1 |
    /// | `FUN_00444cb0` cockpit_speed | `0x444d70 [esi+0x124]` | 1 |
    /// | `FUN_00444e00` osd_gauge | `0x444e55 [esi+0x124]` | 1 |
    /// | `FUN_00444e80`/`0x444f20`/`0x444fc0` osd8 | `[esi+0x124]` | 1 each |
    /// | `FUN_00445060` osd_damage | `0x4450b5 [esi+0x124]` | 1 |
    /// | `FUN_00445120`..`FUN_004456d0` osd_cond | `[esi+0x124]` | 1 each |
    /// | `FUN_00445760`/`0x445890`/`0x4459c0` sfx3 | `[esi+0x124]` | 1 each |
    /// | `FUN_00445af0`/`0x445b80`/`0x445c10`/`0x445c80`/`0x445d50` sfx | `[esi+0x124]` | 1/1/1/1/2 |
    /// | `FUN_00445e00` material | `0x445ea8`/`0x445ec4 [ebx+0x124]` | 2 |
    ///
    /// 21 exe functions, 30 sites, all `ECX = *(K+0x124)`.
    ///
    /// Port side, before this row — the four receiver-less funnels and their call sites:
    ///
    /// | port site (pre-fix) | exe counterpart | port `this` | oracle reachability |
    /// |---|---|---|---|
    /// | `bind_res` x12: `deformable` x3, `shadow`, `bolt`, `logo`, `flapsfx` x2, `material2` x3, `stockpart2` | `46bfa0`/`46c1a0`/`46c4e0`/`46d400`/`46ca70`/`46cff0`/`46da30` | `None` | `deformable` reachable (4085 `render` + 79 `deformable` rows); the other six keywords are absent from the corpus |
    /// | `body` (`FUN_0046bb80`) | `46bb80` | `None` | keyword exercised (2966 rows) but no row takes the `%d`-shape branch: 0 recorded resolve calls |
    /// | `material` x2 (`FUN_00445e00`) | `445e00` | `None` | reachable (85 `material` rows) |
    /// | `load` x13: `cockpit_rpm`, `cockpit_speed`, `sfx_horn`, `osd_gauge`, `osd8`, `osd_damage`, `osd_cond`, `sfx3`, `sfx_gearbox_fwd`/`_rev`, `sfx_wreck`, `sfx_gearchange` | `444b60`..`445d50` (see above) | `None` | `cockpit_rpm`/`cockpit_speed`/`sfx_horn` reachable (48/48/12 rows); the rest absent from the corpus |
    ///
    /// Every one of the 30 exe sites passes the manager, so each of these was a divergence. The
    /// oracle could not see any of them: its recorded `this` is the scratch manager `0x30000040`
    /// (from `init.json`'s `*(K+0x124)`), and `test_support::this_match` accepts it against
    /// `This::None` because it lies outside the heap and stack ranges. The divergence is real
    /// game state nevertheless — the exe's `FUN_00544590` reads its receiver (`[ecx+8]`), so a
    /// receiver-less call yields a different handle — hence `resolve` now always passes
    /// `*(K+0x124)` and the separate `resolve_mgr` variant is gone.
    pub fn resolve(&mut self, id: u32) -> u32 {
        let mgr = self.kr(0x124);
        self.call(EXT_RESOLVE, This::Addr(mgr), vec![Arg::U(id)])
    }

    /// `FUN_00546070` on the reference slot at `slot` (inside the object graph).
    pub fn bind(&mut self, slot: u32, handle: u32, kind: u32, flag: u32) {
        self.call(
            EXT_BIND,
            This::Addr(slot),
            vec![Arg::U(handle), Arg::U(kind), Arg::U(flag)],
        );
    }

    /// `FUN_00546070` on a stack temporary: returns the resulting `(slot[2], slot[3])`.
    pub fn bind_local(&mut self, handle: u32, kind: u32, flag: u32) -> (u32, u32) {
        let r = self.call(
            EXT_BIND,
            This::Local,
            vec![Arg::U(handle), Arg::U(kind), Arg::U(flag)],
        );
        (handle, r)
    }

    /// `resolve(id)` then `bind(slot, ..)`, the pair every resource keyword issues.
    pub fn load(&mut self, slot: u32, id: u32, kind: u32, flag: u32) {
        let h = self.resolve(id);
        self.bind(slot, h, kind, flag);
    }
}

/// `param_1 + strlen(param_1) + 1`: where the handlers read their arguments.
pub fn rest(line: &[u8]) -> &[u8] {
    match line.iter().position(|&b| b == 0) {
        Some(n) => line.get(n + 1..).unwrap_or(&[]),
        None => &[],
    }
}

/// The exe's `sscanf` over the handler's argument text, at the game's x87 precision.
pub fn scan(line: &[u8], fmt: &str) -> Scan {
    sscanf(rest(line), fmt.as_bytes(), Precision::F32)
}

/// Stored-word views of a [`Scan`].
pub trait ScanBits {
    /// The i-th conversion as the 32-bit pattern the exe stores (`%f` -> f32 bits).
    fn fb(&self, i: usize) -> Option<u32>;
    /// The i-th `%d`/`%x` conversion as the stored word.
    fn db(&self, i: usize) -> Option<u32>;
}

impl ScanBits for Scan {
    fn fb(&self, i: usize) -> Option<u32> {
        self.f(i).map(f32::to_bits)
    }

    fn db(&self, i: usize) -> Option<u32> {
        self.d(i).map(|v| v as u32)
    }
}

/// `FUN_005d7190`: case-insensitive compare of two C strings; true when equal.
///
/// exe: FUN_005d7190
pub fn stricmp_eq(a: &[u8], b: &[u8]) -> bool {
    let a = a.split(|&c| c == 0).next().unwrap_or(&[]);
    let b = b.split(|&c| c == 0).next().unwrap_or(&[]);
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// `FUN_00551c60`: zero a vec3 at `a`.
///
/// exe: FUN_00551c60
pub fn vec3_zero(m: &mut Space, a: u32) {
    m.w32(a, 0);
    m.w32(a + 4, 0);
    m.w32(a + 8, 0);
}

/// `FUN_00551c70`: set a vec3 at `a`.
///
/// exe: FUN_00551c70
pub fn vec3_set(m: &mut Space, a: u32, x: u32, y: u32, z: u32) {
    m.w32(a, x);
    m.w32(a + 4, y);
    m.w32(a + 8, z);
}

/// `FUN_00428fd0`: construct the 7-word list head at `a`.
///
/// exe: FUN_00428fd0
pub fn list_head_ctor(m: &mut Space, a: u32) {
    m.w32(a + 4, 0x5f09ac);
    m.w32(a + 12, 0);
    m.w32(a + 8, 0);
    m.w32(a + 16, 0x5f09ac);
    m.w32(a + 24, 0);
    m.w32(a + 20, 0);
    m.w32(a + 8, a + 16);
    m.w32(a + 12, 0);
    m.w32(a + 20, 0);
    m.w32(a + 24, a + 4);
    m.w32(a, 0x5f09a8);
}

/// `FUN_00429130`: construct a list node base at `a`.
///
/// exe: FUN_00429130
pub fn list_node_ctor(m: &mut Space, a: u32) {
    m.w32(a + 8, 0);
    m.w32(a + 4, 0);
    m.w32(a, 0x5f09bc);
}

/// `FUN_004776f0` (fastcall, `ECX = slot`): drop the object `*slot` from its owner's reference
/// chain (`owner+0xc` list of slots, owner refcount at `+0x10`), then clear the slot.
///
/// exe: FUN_004776f0
pub fn ref_release(m: &mut Space, slot: u32) {
    let owner = m.r32(slot);
    if owner == 0 {
        return;
    }
    let mut cur = m.r32(owner + 0xc);
    let mut prev = 0u32;
    if cur == 0 {
        return;
    }
    while cur != slot {
        let next = m.r32(cur + 4);
        prev = cur;
        cur = next;
        if next == 0 {
            return;
        }
    }
    let next = m.r32(cur + 4);
    if prev == 0 {
        m.w32(owner + 0xc, next);
    } else {
        m.w32(prev + 4, next);
    }
    let o = m.r32(slot);
    let base = if o == 0 { 0 } else { o + 0xc };
    let n = m.r32(base.wrapping_add(4)).wrapping_sub(1);
    m.w32(base.wrapping_add(4), n);
    m.w32(slot, 0);
    m.w32(slot + 4, 0);
}

/// The tail every body-creating keyword shares: replace the body in the record slot at
/// `slot` (`rec+0x40`) by `new` and link the slot into the new body's reference chain.
pub(crate) fn attach_body(m: &mut Space, slot: u32, new: u32) {
    if m.r32(slot) != 0 {
        ref_release(m, slot);
    }
    m.w32(slot, new);
    if new == 0 {
        m.w32(slot + 4, 0);
    } else {
        let head = m.r32(new + 0xc);
        m.w32(slot + 4, head);
        m.w32(new + 0xc, slot);
        let o = m.r32(slot);
        let rc = m.r32(o + 0x10).wrapping_add(1);
        m.w32(o + 0x10, rc);
    }
}

/// Test-only replay of the Unicorn function oracle (`re/oracle/cfg_handler_vectors.py`):
/// [`ReplayHost`] feeds one vector row's recorded host calls back to the handler under test.
pub mod test_support {
    use super::*;

    /// A recorded host call: function, `this`, args, return value and memory effects.
    #[derive(Debug, Clone)]
    pub struct RecordedCall {
        pub f: u32,
        pub this: This,
        pub a: Vec<Arg>,
        pub r: Option<u32>,
        pub w: Vec<(u32, u32)>,
    }

    /// Replays a row's recorded [`Host`] calls: asserts the handler issues the same calls
    /// with the same arguments in order, applies the recorded `w` effects to the [`Space`],
    /// bump-allocates exactly the recorded heap-returning calls and returns the recorded `r`.
    pub struct ReplayHost {
        calls: Vec<RecordedCall>,
        next: usize,
    }

    fn expect_u32(v: &serde_json::Value) -> u32 {
        match v {
            serde_json::Value::Number(n) => n.as_u64().expect("call arg must fit u32") as u32,
            other => panic!("expected u32 call arg, got {other}"),
        }
    }

    fn expect_arg(v: &serde_json::Value) -> Arg {
        match v {
            serde_json::Value::Number(_) => Arg::U(expect_u32(v)),
            serde_json::Value::String(s) => Arg::S(s.clone()),
            other => panic!("expected u32 or string call arg, got {other}"),
        }
    }

    fn expect_this(v: &serde_json::Value) -> This {
        match v {
            serde_json::Value::Null => This::None,
            serde_json::Value::String(s) if s == "local" => This::Local,
            serde_json::Value::Number(_) => This::Addr(expect_u32(v)),
            other => panic!("expected null/\"local\"/addr this, got {other}"),
        }
    }

    /// Parse one oracle `calls` entry (`{f, this, a, r, w}`; `f` is a `"0x..."` string).
    pub fn parse_call(v: &serde_json::Value) -> RecordedCall {
        let f = u32::from_str_radix(
            v["f"]
                .as_str()
                .expect("call f must be a hex string")
                .trim_start_matches("0x"),
            16,
        )
        .expect("call f must parse as hex");
        let w = v["w"]
            .as_array()
            .expect("call w must be an array")
            .iter()
            .map(|e| {
                let pair = e.as_array().expect("w entry must be [addr, u32]");
                (expect_u32(&pair[0]), expect_u32(&pair[1]))
            })
            .collect();
        RecordedCall {
            f,
            this: expect_this(&v["this"]),
            a: v["a"]
                .as_array()
                .expect("call a must be an array")
                .iter()
                .map(expect_arg)
                .collect(),
            r: v["r"].as_u64().map(|r| r as u32),
            w,
        }
    }

    /// Lenient argument equality for oracle replay. The oracle hooks normalize stack
    /// addresses away: a stack C string argument (e.g. the sscanf `%s` shape name the
    /// exe passes from `&local_80` to `EXT_BODY`) is recorded as the string, while the
    /// port passes the same bytes from the tokenized line buffer as `Arg::S`. A recorded
    /// `U` that falls in the oracle stack range therefore matches any port arg: the
    /// execution environments' stack layouts are not part of vehicle state. All other
    /// arguments (heap pointers, floats, ids, resource handles) compare exactly.
    fn args_lenient(got: &[Arg], want: &[Arg]) -> bool {
        const STACK_LO: u32 = 0x7FE0_0000;
        const STACK_HI: u32 = 0x7FF0_1000;
        if got.len() != want.len() {
            return false;
        }
        got.iter().zip(want.iter()).all(|(g, w)| match (g, w) {
            (Arg::U(a), Arg::U(b)) => a == b || (STACK_LO..STACK_HI).contains(b),
            (Arg::S(a), Arg::S(b)) => a == b,
            (Arg::S(_), Arg::U(b)) => (STACK_LO..STACK_HI).contains(b),
            (Arg::U(_), Arg::S(_)) => true,
        })
    }

    /// `this` equality for oracle replay: exact, except a receiver-less (`This::None`)
    /// port call matches any recorded address outside the heap and stack ranges (the
    /// oracle's `cdecl` hooks log whatever dead value ECX holds; only `This::Addr`
    /// heap/stack receivers are vehicle state).
    fn this_match(got: &This, want: &This) -> bool {
        match (got, want) {
            (This::None, This::None) | (This::Local, This::Local) => true,
            (This::Addr(a), This::Addr(b)) => a == b,
            (This::None, This::Addr(b)) | (This::Addr(b), This::None) => {
                !((0x2000_0000..0x2400_0000).contains(b) || (0x7FE0_0000..0x7FF0_1000).contains(b))
            }
            _ => false,
        }
    }

    impl ReplayHost {
        /// Replays a row's recorded [`Host`] calls.
        pub fn new(calls: Vec<RecordedCall>) -> ReplayHost {
            ReplayHost { calls, next: 0 }
        }

        /// True when every recorded call was consumed.
        pub fn exhausted(&self) -> bool {
            self.next == self.calls.len()
        }

        fn alloc_like_exe(m: &mut Space, n: u32) -> u32 {
            // The exe wrapper (`FUN_0054f560`, hooked as 16-byte aligned zeroed bump alloc).
            let p = (m.heap_ptr() + 15) & !15;
            m.set_heap_ptr(p + n);
            for a in (p..p + n).step_by(4) {
                m.w32(a, 0);
            }
            p
        }
    }

    impl Host for ReplayHost {
        fn call(&mut self, m: &mut Space, c: &ExtCall) -> u32 {
            assert!(
                self.next < self.calls.len(),
                "handler issued unexpected call #{next}: f=0x{f:x} this={this:?} a={a:?}",
                next = self.next,
                f = c.f,
                this = c.this,
                a = c.a,
            );
            let want = &self.calls[self.next];
            self.next += 1;
            assert_eq!(
                c.f,
                want.f,
                "call #{}: function mismatch (got 0x{:x}, want 0x{:x})",
                self.next - 1,
                c.f,
                want.f,
            );
            // `this` equality, except the oracle's `cdecl` hooks record whatever dead
            // value ECX holds on entry (call() zeroes registers, so usually a small
            // constant like 0x30000000 or 0x65efa0) while the port uses `This::None`
            // for receiver-less calls. Heap/stack addresses still compare exactly.
            assert!(
                this_match(&c.this, &want.this),
                "call #{} f=0x{:x}: this mismatch (got {:?}, want {:?})",
                self.next - 1,
                c.f,
                c.this,
                want.this,
            );
            assert!(
                args_lenient(&c.a, &want.a),
                "call #{} f=0x{:x}: args mismatch (got {:?}, want {:?})",
                self.next - 1,
                c.f,
                c.a,
                want.a,
            );
            for &(a, v) in &want.w {
                m.w32(a, v);
            }
            match (c.f, want.r) {
                (EXT_BODY, Some(r)) | (EXT_BODYDEF, Some(r)) => {
                    let p = Self::alloc_like_exe(m, 0xa0);
                    assert_eq!(p, r, "allocator lockstep broke: heap diverged");
                    p
                }
                (_, Some(r)) => r,
                (_, None) => 0,
            }
        }
    }

    /// Write the `init.json` nonzero heap words into `m` at their addresses and set the heap
    /// pointer; returns `(k, p)`.
    pub fn load_init(m: &mut Space, init: &serde_json::Value) -> (u32, u32) {
        let words = init["words"]
            .as_array()
            .expect("init.words must be an array");
        for e in words {
            let pair = e.as_array().expect("init word must be [addr, u32]");
            let (a, v) = (
                pair[0].as_u64().expect("init addr must fit u32") as u32,
                pair[1].as_u64().expect("init word must fit u32") as u32,
            );
            m.w32(a, v);
        }
        m.set_heap_ptr(init["hp"].as_u64().expect("init.hp must fit u32") as u32);
        (
            init["k"].as_u64().expect("init.k must fit u32") as u32,
            init["p"].as_u64().expect("init.p must fit u32") as u32,
        )
    }
}
