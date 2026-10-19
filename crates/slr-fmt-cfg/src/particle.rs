//! Particle emitter scripts (`particles/scripts/particle_clouds/*.cfg`): the `[STATE]` /
//! `[ACTION]` dialect parsed by `FUN_0051ac20`.
//!
//! ```text
//! [STATE]
//! type            1
//! velscale        2.00
//! source_size     line   0.01 0.00 0.00   0.04 0.00 0.00
//! lifetime        point  3.00 0.00 0.00
//!
//! [ACTION]
//! source  s0 300  line -0.10 0 0 0.10 0 0   sphere 0 0 0 0.10 2.50
//! gravity g0      0.00 -9.81 0.00
//! move    m0      1 0
//! damping d0      0.70 0.70 0.70
//! ```
//!
//! The file is split into lines; a line containing `[ACTION]` or `[STATE]` anywhere (even
//! inside a `;` comment) switches section, every other line is handed to the state handler
//! (`FUN_00519110`) or the action handler (`FUN_0051a140`) of the current section and lines
//! before the first marker are ignored. Fields split on ` ,\t\r\n`; keywords compare
//! case-insensitively; unknown keywords (which includes every `;comment` line) are skipped
//! silently. Numbers are read with the game's own `sscanf` (see [`crate::scan`]).
//!
//! The action model is McAllister's Particle System API (`pSource`, `pGravity`, `pBounce`,
//! `pDamping`, `pExplosion`, `pMove`, `pCopyVertexB`-free subset); `domain` arguments are the
//! API's `pDomain` shapes.
//!
//! Original behaviours not reproduced (ADR 0007, see `docs/fidelity.md`): an unknown or
//! truncated domain keyword makes the exe store a pointer into its own stack frame as the
//! domain (and, in a state line, free the previous domain first); the port keeps the previous
//! value / `None` instead. A cfg line longer than 1023 bytes, or a `%s` token longer than 63
//! bytes, overflows the exe's stack buffers; the port has no such limit (labels truncate at 63
//! bytes). A final line without newline reads into heap garbage in the exe; the port stops at
//! the end of the data.

use crate::scan::{scan_f32, scan_i32, scan_str, Tokens};
use slr_fmt_core::Result;
use slr_x87::{Precision, Rounding, X87};

const RC: Rounding = Rounding::NearestEven;

/// A `pDomain` shape (vtable / words as the exe lays them out after the vtable).
#[derive(Debug, Clone, PartialEq)]
pub enum Domain {
    Point {
        p: [f32; 3],
    },
    /// `line start end`: stores the start and `end - start`.
    Line {
        p: [f32; 3],
        d: [f32; 3],
    },
    /// `triangle p0 p1 p2`: stores `p0`, the two edges, their lengths and the unit normal of
    /// the plane through `p0` (`d = -n.p0`).
    Triangle {
        p: [f32; 3],
        e1: [f32; 3],
        e2: [f32; 3],
        len1: f32,
        len2: f32,
        normal: [f32; 3],
        d: f32,
    },
    /// `plane point normal`: normal is normalised in place, `d = -n.p`.
    Plane {
        p: [f32; 3],
        normal: [f32; 3],
        d: f32,
    },
    /// `rectangle origin u v`.
    Rectangle {
        p: [f32; 3],
        u: [f32; 3],
        v: [f32; 3],
        len_u: f32,
        len_v: f32,
        normal: [f32; 3],
        d: f32,
    },
    /// `box a b`: component-wise min and max of the two corners.
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    /// `sphere center r1 r2`: outer/inner radius (larger first) and their squares.
    Sphere {
        center: [f32; 3],
        r_max: f32,
        r_min: f32,
        r_max2: f32,
        r_min2: f32,
    },
}

impl Domain {
    /// The keyword that produced this shape.
    pub fn kind(&self) -> &'static str {
        match self {
            Domain::Point { .. } => "point",
            Domain::Line { .. } => "line",
            Domain::Triangle { .. } => "triangle",
            Domain::Plane { .. } => "plane",
            Domain::Rectangle { .. } => "rectangle",
            Domain::Box { .. } => "box",
            Domain::Sphere { .. } => "sphere",
        }
    }

    /// The f32 words that follow the vtable in the exe's object (bit patterns).
    pub fn words(&self) -> Vec<u32> {
        let mut w: Vec<f32> = Vec::new();
        match self {
            Domain::Point { p } => w.extend(p),
            Domain::Line { p, d } => {
                w.extend(p);
                w.extend(d);
            }
            Domain::Triangle {
                p,
                e1,
                e2,
                len1,
                len2,
                normal,
                d,
            } => {
                w.extend(p);
                w.extend(e1);
                w.extend(e2);
                w.push(*len1);
                w.push(*len2);
                w.extend(normal);
                w.push(*d);
            }
            Domain::Plane { p, normal, d } => {
                w.extend(p);
                w.extend(normal);
                w.push(*d);
            }
            Domain::Rectangle {
                p,
                u,
                v,
                len_u,
                len_v,
                normal,
                d,
            } => {
                w.extend(p);
                w.extend(u);
                w.extend(v);
                w.push(*len_u);
                w.push(*len_v);
                w.extend(normal);
                w.push(*d);
            }
            Domain::Box { min, max } => {
                w.extend(min);
                w.extend(max);
            }
            Domain::Sphere {
                center,
                r_max,
                r_min,
                r_max2,
                r_min2,
            } => {
                w.extend(center);
                w.push(*r_max);
                w.push(*r_min);
                w.push(*r_max2);
                w.push(*r_min2);
            }
        }
        w.into_iter().map(f32::to_bits).collect()
    }
}

/// One `[ACTION]` line. The `label` is the second field (`s0`, `g0`, ...), at most 63 bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub label: String,
    pub kind: ActionKind,
}

/// Domains are `None` only when the cfg line named an unknown shape or ran out of fields
/// (the exe stores a dangling stack pointer there, see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// `bounce <label> friction resilience cutoff <domain>`
    Bounce {
        friction: f32,
        resilience: f32,
        cutoff: f32,
        domain: Option<Domain>,
    },
    /// `damping <label> x y z [vlow [vhigh]]` (`vlow` defaults to 0, `vhigh` to 1e6).
    Damping {
        damping: [f32; 3],
        vlow: f32,
        vhigh: f32,
    },
    /// `explosion <label> cx cy cz velocity magnitude stdev [epsilon [lifetime]]`
    /// (`epsilon` defaults to 1e-6, `lifetime` to 0).
    Explosion {
        center: [f32; 3],
        velocity: f32,
        magnitude: f32,
        stdev: f32,
        epsilon: f32,
        lifetime: f32,
    },
    /// `gravity <label> x y z`
    Gravity { gravity: [f32; 3] },
    /// `move <label> a b`: bit 0 = `a == 1`, bit 1 = `a == -1`, bit 2 = `b == 1`,
    /// bit 3 = `b == -1` (velocity / position move direction).
    Move { flags: u32 },
    /// `source <label> rate <position domain> <velocity domain>`
    Source {
        rate: f32,
        position: Option<Domain>,
        velocity: Option<Domain>,
    },
    /// `directsource <label> rate <position domain> <velocity domain>` (no shipped use).
    DirectSource {
        rate: f32,
        position: Option<Domain>,
        velocity: Option<Domain>,
    },
    /// `counter <label> count <domain>` (no shipped use).
    Counter { count: i32, domain: Option<Domain> },
}

impl ActionKind {
    /// True when a domain argument is missing (unknown shape keyword or truncated line).
    pub fn has_missing_domain(&self) -> bool {
        match self {
            ActionKind::Bounce { domain, .. } | ActionKind::Counter { domain, .. } => {
                domain.is_none()
            }
            ActionKind::Source {
                position, velocity, ..
            }
            | ActionKind::DirectSource {
                position, velocity, ..
            } => position.is_none() || velocity.is_none(),
            _ => false,
        }
    }

    /// The action-type id the exe stores at `+0x10` of the node.
    pub fn type_id(&self) -> u32 {
        match self {
            ActionKind::Bounce { .. } => 2,
            ActionKind::Damping { .. } => 3,
            ActionKind::Explosion { .. } => 4,
            ActionKind::Gravity { .. } => 5,
            ActionKind::Move { .. } => 7,
            ActionKind::Source { .. } => 10,
            ActionKind::DirectSource { .. } => 11,
            ActionKind::Counter { .. } => 13,
        }
    }
}

/// The `[STATE]` block: per-particle attribute ranges (each a [`Domain`], interpolated from the
/// `source_*` to the `dest_*` value over the particle's life) plus scalar settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleState {
    /// First word of the state block (0.1); no parser line touches it.
    pub param0: f32,
    pub source_size: Domain,
    pub dest_size: Domain,
    pub source_color: Domain,
    pub dest_color: Domain,
    pub source_alpha: Domain,
    pub dest_alpha: Domain,
    pub source_angle: Domain,
    pub dest_angle: Domain,
    pub texture_offset: Domain,
    pub angular_velocity: Domain,
    pub lifetime: Domain,
    /// `velscale` (default 1.0).
    pub velscale: f32,
    /// `type`: 1 -> 0x10, 2 -> 0x20, anything else (and the default) 0.
    pub kind: u32,
    /// OR of: `source_size` 4, `source_color` 2, `source_alpha` 2, `source_angle` 0x20.
    pub flags: u32,
    /// `dimension cols rows` (default 1 1).
    pub dimension: [i32; 2],
    /// Texture atlas origin (always 0,0).
    pub tex_origin: [f32; 2],
    /// `[1/dimension.y, 1/dimension.x]` (default 1,1).
    pub tex_step: [f32; 2],
}

/// Fields the exe copies from the state block into the object after the script loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Initial {
    pub tex_origin: [f32; 2],
    pub tex_step: [f32; 2],
    pub flags: u32,
}

/// The constant object header the exe writes after parsing (`+0x04`, `+0x08`, `+0x0c`, `+0xb0`).
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// 1000 (role inferred: particle cap).
    pub limit: u32,
    /// 0 (role inferred: live count).
    pub count: u32,
    /// 1.0 (role inferred: emission-rate scale).
    pub rate_scale: f32,
    /// 0.
    pub tail: u32,
}

/// A parsed particle script.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleCfg {
    pub header: Header,
    pub state: ParticleState,
    pub initial: Initial,
    pub actions: Vec<Action>,
}

fn p3(v: [f32; 3]) -> Domain {
    Domain::Point { p: v }
}

impl ParticleState {
    /// The defaults `FUN_0051bbc0` builds.
    fn defaults() -> Self {
        ParticleState {
            param0: f32::from_bits(0x3dcc_cccd),
            source_size: p3([f32::from_bits(0x3dcc_cccd); 3]),
            dest_size: p3([0.5; 3]),
            source_color: Domain::Line {
                p: [0.0, 1.0, 0.0],
                d: [1.0, -1.0, 0.0],
            },
            dest_color: Domain::Line {
                p: [0.0, 1.0, 0.0],
                d: [1.0, -1.0, 0.0],
            },
            source_alpha: p3([1.0; 3]),
            dest_alpha: p3([0.0; 3]),
            source_angle: p3([0.0; 3]),
            dest_angle: p3([1.0; 3]),
            texture_offset: p3([0.0; 3]),
            angular_velocity: p3([f32::from_bits(0x3fcc_cccd); 3]),
            lifetime: p3([2.0; 3]),
            velscale: 1.0,
            kind: 0,
            flags: 0,
            dimension: [1, 1],
            tex_origin: [0.0, 0.0],
            tex_step: [1.0, 1.0],
        }
    }
}

// ----------------------------------------------------------------------- scanning helpers

pub(crate) struct Fields<'a> {
    toks: Tokens<'a>,
    pc: Precision,
}

impl<'a> Fields<'a> {
    fn new(line: &'a [u8], pc: Precision) -> Self {
        Fields {
            toks: Tokens::new(line),
            pc,
        }
    }

    fn word(&mut self) -> Option<&'a [u8]> {
        self.toks.next_token()
    }

    /// `strtok` + `sscanf("%f")` into `dst` (left alone when absent or unparsable).
    fn f(&mut self, dst: &mut f32) {
        if let Some(v) = self.word().and_then(|t| scan_f32(t, self.pc)) {
            *dst = v;
        }
    }

    fn d(&mut self, dst: &mut i32) {
        if let Some(v) = self.word().and_then(scan_i32) {
            *dst = v;
        }
    }

    fn f3(&mut self) -> [f32; 3] {
        let mut v = [0.0; 3];
        for x in &mut v {
            self.f(x);
        }
        v
    }

    fn label(&mut self) -> String {
        match self.word() {
            Some(t) => {
                let s = scan_str(t);
                s[..s.len().min(63)].iter().map(|&b| b as char).collect()
            }
            None => String::new(),
        }
    }
}

// ------------------------------------------------------------------------- domain maths
//
// Every multi-operation expression below mirrors the exe's x87 instruction order (see the
// disassembly of FUN_005195e0 / FUN_004369b0 / FUN_0048f3f0 / FUN_0045dc80 / FUN_004944a0)
// and runs on the soft X87 type so the register-held intermediates behave as they do under
// the game's precision control (`pc`).

fn x(v: f32) -> X87 {
    X87::from_f32(v)
}

fn sub(a: f32, b: f32, pc: Precision) -> f32 {
    x(a).sub(x(b), pc, RC).to_f32(pc, RC)
}

fn sub3(a: [f32; 3], b: [f32; 3], pc: Precision) -> [f32; 3] {
    [
        sub(a[0], b[0], pc),
        sub(a[1], b[1], pc),
        sub(a[2], b[2], pc),
    ]
}

/// `FUN_004369b0`: `sqrt((x*x + y*y) + z*z)`, left in the register (not stored).
///
/// exe: FUN_004369b0
pub(crate) fn length(v: [f32; 3], pc: Precision) -> X87 {
    let xx = x(v[0]).mul(x(v[0]), pc, RC);
    let yy = x(v[1]).mul(x(v[1]), pc, RC);
    let zz = x(v[2]).mul(x(v[2]), pc, RC);
    xx.add(yy, pc, RC).add(zz, pc, RC).sqrt(pc, RC)
}

fn is_zero(v: X87) -> bool {
    // FCOM with 0.0f then `test ah,0x40` (C3): equal or unordered.
    v.fcom(X87::from_f32(0.0)).0.c3
}

/// `FUN_0045dc80`: cross product `a x b`, each component stored as f32.
///
/// exe: FUN_0045dc80
pub(crate) fn cross(a: [f32; 3], b: [f32; 3], pc: Precision) -> [f32; 3] {
    let m = |p: f32, q: f32| x(p).mul(x(q), pc, RC);
    let fin = |v: X87| v.to_f32(pc, RC);
    [
        fin(m(b[2], a[1]).sub(m(a[2], b[1]), pc, RC)),
        fin(m(a[2], b[0]).sub(m(a[0], b[2]), pc, RC)),
        fin(m(a[0], b[1]).sub(m(b[0], a[1]), pc, RC)),
    ]
}

/// `FUN_0048f3f0`: normalise in place by `1.0 / length` (register-held reciprocal); a zero
/// (or NaN) length leaves the vector alone.
///
/// exe: FUN_0048f3f0
pub(crate) fn normalize(v: [f32; 3], pc: Precision) -> [f32; 3] {
    let len = length(v, pc);
    if is_zero(len) {
        return v;
    }
    let r = x(1.0).div(len, pc, RC);
    [
        r.mul(x(v[0]), pc, RC).to_f32(pc, RC),
        r.mul(x(v[1]), pc, RC).to_f32(pc, RC),
        r.mul(x(v[2]), pc, RC).to_f32(pc, RC),
    ]
}

/// The plane constant `-(n.z*p.z + n.y*p.y + n.x*p.x)` (`FUN_004944a0` then `fchs`).
///
/// exe: FUN_004944a0
pub(crate) fn plane_d(n: [f32; 3], p: [f32; 3], pc: Precision) -> f32 {
    let m = |a: f32, b: f32| x(a).mul(x(b), pc, RC);
    let s = m(n[2], p[2])
        .add(m(n[1], p[1]), pc, RC)
        .add(m(n[0], p[0]), pc, RC);
    let mut b = s.to_bytes();
    b[9] ^= 0x80; // FCHS
    X87::from_bytes(b).to_f32(pc, RC)
}

/// `v * r` per component with the f32 factor `r` (`FUN_004367a0` with stored operands).
fn scale_f32(v: [f32; 3], r: f32, pc: Precision) -> [f32; 3] {
    let f = |c: f32| x(r).mul(x(c), pc, RC).to_f32(pc, RC);
    [f(v[0]), f(v[1]), f(v[2])]
}

/// `1.0f / len` stored to an f32 (the triangle/plane paths round the reciprocal).
fn recip_f32(len: X87, pc: Precision) -> f32 {
    x(1.0).div(len, pc, RC).to_f32(pc, RC)
}

/// One `pDomain` (`FUN_005195e0`): the shape keyword is the next field of the line.
///
/// exe: FUN_005195e0
pub(crate) fn parse_domain(f: &mut Fields, pc: Precision) -> Option<Domain> {
    let kw = f.word()?;
    let is = |name: &str| kw.eq_ignore_ascii_case(name.as_bytes());
    if is("point") {
        Some(Domain::Point { p: f.f3() })
    } else if is("line") {
        let p = f.f3();
        let end = f.f3();
        Some(Domain::Line {
            p,
            d: sub3(end, p, pc),
        })
    } else if is("triangle") {
        let p = f.f3();
        let q1 = f.f3();
        let q2 = f.f3();
        let e1 = sub3(q1, p, pc);
        let e2 = sub3(q2, p, pc);
        let l1 = length(e1, pc);
        let len1 = l1.to_f32(pc, RC);
        let len2 = length(e2, pc).to_f32(pc, RC);
        let a = scale_f32(e1, recip_f32(x(len1), pc), pc);
        let b = scale_f32(e2, recip_f32(x(len2), pc), pc);
        let normal = normalize(cross(a, b, pc), pc);
        Some(Domain::Triangle {
            p,
            e1,
            e2,
            len1,
            len2,
            normal,
            d: plane_d(normal, p, pc),
        })
    } else if is("plane") {
        let p = f.f3();
        let mut n = f.f3();
        let len = length(n, pc);
        if !is_zero(len) {
            n = scale_f32(n, recip_f32(len, pc), pc);
        }
        Some(Domain::Plane {
            p,
            normal: n,
            d: plane_d(n, p, pc),
        })
    } else if is("rectangle") {
        let p = f.f3();
        let u = f.f3();
        let v = f.f3();
        let len_u = length(u, pc).to_f32(pc, RC);
        let len_v_reg = length(v, pc);
        let len_v = len_v_reg.to_f32(pc, RC);
        // r_u divides the *stored* f32 length; r_v divides the register-held one.
        let r_u = x(1.0).div(x(len_u), pc, RC);
        let r_v = x(1.0).div(len_v_reg, pc, RC);
        let fin = |r: X87, c: f32| r.mul(x(c), pc, RC).to_f32(pc, RC);
        let a = [fin(r_u, u[0]), fin(r_u, u[1]), fin(r_u, u[2])];
        let b = [fin(r_v, v[0]), fin(r_v, v[1]), fin(r_v, v[2])];
        let normal = normalize(cross(a, b, pc), pc);
        Some(Domain::Rectangle {
            p,
            u,
            v,
            len_u,
            len_v,
            normal,
            d: plane_d(normal, p, pc),
        })
    } else if is("box") {
        let a = f.f3();
        let b = f.f3();
        let le = |l: f32, r: f32| if l <= r { (l, r) } else { (r, l) };
        let (x0, x1) = le(b[0], a[0]);
        let (y0, y1) = le(b[1], a[1]);
        // the z pair uses a strict `<` in the exe (identical except for NaN)
        let (z0, z1) = if a[2] < b[2] {
            (a[2], b[2])
        } else {
            (b[2], a[2])
        };
        Some(Domain::Box {
            min: [x0, y0, z0],
            max: [x1, y1, z1],
        })
    } else if is("sphere") {
        let center = f.f3();
        let (mut r1, mut r2) = (0.0f32, 0.0f32);
        f.f(&mut r1);
        f.f(&mut r2);
        let (r_max, r_min) = if r1 <= r2 { (r2, r1) } else { (r1, r2) };
        Some(Domain::Sphere {
            center,
            r_max,
            r_min,
            r_max2: r_max * r_max,
            r_min2: r_min * r_min,
        })
    } else {
        None
    }
}

// ------------------------------------------------------------------------- line handlers

/// What a handler made of one line (diagnostics only; the exe skips silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStatus {
    Ok,
    /// Unknown keyword, which is also what a `;` comment looks like to the exe.
    Unknown,
    /// Known keyword whose domain argument is unknown or truncated (the exe keeps a dangling
    /// stack pointer there, see the module docs).
    BadDomain,
}

/// One `[STATE]` line.
///
/// exe: FUN_00519110
pub(crate) fn parse_state_line(st: &mut ParticleState, line: &[u8], pc: Precision) -> LineStatus {
    let mut f = Fields::new(line, pc);
    let Some(kw) = f.word() else {
        return LineStatus::Ok;
    };
    let is = |name: &str| kw.eq_ignore_ascii_case(name.as_bytes());
    if is("velscale") {
        f.f(&mut st.velscale);
        LineStatus::Ok
    } else if is("dimension") {
        f.d(&mut st.dimension[0]);
        f.d(&mut st.dimension[1]);
        let recip = |n: i32| {
            x(1.0)
                .div(X87::from_i64(n as u32 as i64), pc, RC)
                .to_f32(pc, RC)
        };
        st.tex_origin = [0.0, 0.0];
        st.tex_step = [recip(st.dimension[1]), recip(st.dimension[0])];
        LineStatus::Ok
    } else if is("type") {
        let mut v = i32::MIN; // the exe's slot holds a stack pointer when no number converts
        f.d(&mut v);
        st.kind = match v {
            1 => 0x10,
            2 => 0x20,
            _ => 0,
        };
        LineStatus::Ok
    } else {
        // (field, flag bits OR-ed in even when the domain does not parse)
        let slot: Option<(&mut Domain, u32)> = if is("source_size") {
            Some((&mut st.source_size, 4))
        } else if is("dest_size") {
            Some((&mut st.dest_size, 0))
        } else if is("source_color") {
            Some((&mut st.source_color, 2))
        } else if is("dest_color") {
            Some((&mut st.dest_color, 0))
        } else if is("source_alpha") {
            Some((&mut st.source_alpha, 2))
        } else if is("dest_alpha") {
            Some((&mut st.dest_alpha, 0))
        } else if is("source_angle") {
            Some((&mut st.source_angle, 0x20))
        } else if is("dest_angle") {
            Some((&mut st.dest_angle, 0))
        } else if is("texture_offset") {
            Some((&mut st.texture_offset, 0))
        } else if is("angular_velocity") {
            Some((&mut st.angular_velocity, 0))
        } else if is("lifetime") {
            Some((&mut st.lifetime, 0))
        } else {
            None
        };
        match slot {
            Some((slot, bits)) => {
                let d = parse_domain(&mut f, pc);
                st.flags |= bits;
                match d {
                    Some(d) => {
                        *slot = d;
                        LineStatus::Ok
                    }
                    None => LineStatus::BadDomain,
                }
            }
            None => LineStatus::Unknown,
        }
    }
}

/// One `[ACTION]` line; `None` for unknown keywords.
///
/// exe: FUN_0051a140
pub(crate) fn parse_action_line(line: &[u8], pc: Precision) -> Option<Action> {
    let mut f = Fields::new(line, pc);
    let kw = f.word()?;
    let is = |name: &str| kw.eq_ignore_ascii_case(name.as_bytes());
    let kind = if is("bounce") {
        let label = f.label();
        let (mut friction, mut resilience, mut cutoff) = (0.0, 0.0, 0.0);
        f.f(&mut friction);
        f.f(&mut resilience);
        f.f(&mut cutoff);
        let domain = parse_domain(&mut f, pc);
        (
            label,
            ActionKind::Bounce {
                friction,
                resilience,
                cutoff,
                domain,
            },
        )
    } else if is("damping") {
        let label = f.label();
        let damping = f.f3();
        let (mut vlow, mut vhigh) = (0.0, f32::from_bits(0x4974_2400));
        f.f(&mut vlow);
        f.f(&mut vhigh);
        (
            label,
            ActionKind::Damping {
                damping,
                vlow,
                vhigh,
            },
        )
    } else if is("explosion") {
        let label = f.label();
        let center = f.f3();
        let (mut velocity, mut magnitude, mut stdev) = (0.0, 0.0, 0.0);
        let (mut epsilon, mut lifetime) = (f32::from_bits(0x3586_37bd), 0.0);
        f.f(&mut velocity);
        f.f(&mut magnitude);
        f.f(&mut stdev);
        f.f(&mut epsilon);
        f.f(&mut lifetime);
        (
            label,
            ActionKind::Explosion {
                center,
                velocity,
                magnitude,
                stdev,
                epsilon,
                lifetime,
            },
        )
    } else if is("gravity") {
        let label = f.label();
        (label, ActionKind::Gravity { gravity: f.f3() })
    } else if is("counter") {
        let label = f.label();
        let mut count = 0;
        f.d(&mut count);
        let domain = parse_domain(&mut f, pc);
        (label, ActionKind::Counter { count, domain })
    } else if is("move") {
        let label = f.label();
        let (mut a, mut b) = (0, 0);
        f.d(&mut a);
        f.d(&mut b);
        let mut flags = match a {
            -1 => 2,
            1 => 1,
            _ => 0,
        };
        match b {
            -1 => flags |= 8,
            1 => flags |= 4,
            _ => {}
        }
        (label, ActionKind::Move { flags })
    } else if is("source") || is("directsource") {
        let direct = is("directsource");
        let label = f.label();
        let mut rate = 0.0;
        f.f(&mut rate);
        let position = parse_domain(&mut f, pc);
        let velocity = parse_domain(&mut f, pc);
        let k = if direct {
            ActionKind::DirectSource {
                rate,
                position,
                velocity,
            }
        } else {
            ActionKind::Source {
                rate,
                position,
                velocity,
            }
        };
        (label, k)
    } else {
        return None;
    };
    Some(Action {
        label: kind.0,
        kind: kind.1,
    })
}

/// Split into lines the way `FUN_005511c0` does: a line ends after `\n` or NUL, the terminator
/// is not part of the content.
///
/// exe: FUN_005511c0
pub(crate) fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        if pos >= bytes.len() {
            return None;
        }
        let rest = &bytes[pos..];
        let n = rest
            .iter()
            .position(|&b| b == b'\n' || b == 0)
            .unwrap_or(rest.len());
        let line = &rest[..n];
        pos += n + 1;
        Some(line)
    })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// A line the parser skipped or only partly understood (diagnostics; the exe is silent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignored {
    /// 1-based line number.
    pub line: usize,
    pub why: &'static str,
}

/// Lines the exe would silently skip, split into `;` comments (the dialect's convention, which
/// the exe handles as an unknown keyword) and genuinely unrecognised content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub comments: usize,
    pub ignored: Vec<Ignored>,
}

impl ParticleCfg {
    /// Parse at the game's x87 precision (24-bit, ADR 0007).
    pub fn parse(bytes: &[u8]) -> ParticleCfg {
        Self::parse_with(bytes, Precision::F32)
    }

    /// Parse with an explicit x87 precision control. Only the derived domain fields and the
    /// `%f` conversion depend on it.
    ///
    /// exe: FUN_0051ac20
    pub fn parse_with(bytes: &[u8], pc: Precision) -> ParticleCfg {
        Self::parse_diagnosed(bytes, pc).0
    }

    /// [`ParticleCfg::parse_with`] plus a report of every line that was skipped.
    pub fn parse_diagnosed(bytes: &[u8], pc: Precision) -> (ParticleCfg, Diagnostics) {
        let mut state = ParticleState::defaults();
        let mut actions = Vec::new();
        let mut diag = Diagnostics::default();
        let mut section = 0u8;
        for (i, line) in lines(bytes).enumerate() {
            let first = line.iter().find(|b| !b" ,\t\r\n".contains(b)).copied();
            let Some(first) = first else { continue };
            let mut note = |status: LineStatus| match status {
                LineStatus::Ok => {}
                LineStatus::Unknown if first == b';' => diag.comments += 1,
                LineStatus::Unknown => diag.ignored.push(Ignored {
                    line: i + 1,
                    why: "unknown keyword",
                }),
                LineStatus::BadDomain => diag.ignored.push(Ignored {
                    line: i + 1,
                    why: "unknown or truncated domain",
                }),
            };
            if contains(line, b"[ACTION]") {
                section = 1;
            } else if contains(line, b"[STATE]") {
                section = 2;
            } else if section == 1 {
                match parse_action_line(line, pc) {
                    Some(a) => {
                        let bad = a.kind.has_missing_domain();
                        actions.push(a);
                        note(if bad {
                            LineStatus::BadDomain
                        } else {
                            LineStatus::Ok
                        });
                    }
                    None => note(LineStatus::Unknown),
                }
            } else if section == 2 {
                note(parse_state_line(&mut state, line, pc));
            } else if first == b';' {
                diag.comments += 1;
            } else {
                diag.ignored.push(Ignored {
                    line: i + 1,
                    why: "before the first [STATE]/[ACTION] marker",
                });
            }
        }
        let initial = Initial {
            tex_origin: state.tex_origin,
            tex_step: state.tex_step,
            flags: state.flags,
        };
        (
            ParticleCfg {
                header: Header {
                    limit: 1000,
                    count: 0,
                    rate_scale: 1.0,
                    tail: 0,
                },
                state,
                initial,
                actions,
            },
            diag,
        )
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<ParticleCfg> {
        Ok(Self::parse(&std::fs::read(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_script_has_the_exes_defaults() {
        let c = ParticleCfg::parse(b"");
        assert_eq!(c.state.velscale, 1.0);
        assert_eq!(c.state.lifetime, Domain::Point { p: [2.0; 3] });
        assert_eq!(c.state.dimension, [1, 1]);
        assert_eq!(c.header.limit, 1000);
        assert!(c.actions.is_empty());
    }

    #[test]
    fn section_markers_match_anywhere_in_the_line() {
        // a `;` comment that mentions [ACTION] still switches section (strstr in the exe)
        let c = ParticleCfg::parse(b";[ACTION]\r\ngravity g0 0 -9.5 0\r\n");
        assert_eq!(c.actions.len(), 1);
        // lines before any marker are ignored
        let c = ParticleCfg::parse(b"gravity g0 0 -9.5 0\r\n[ACTION]\r\n");
        assert!(c.actions.is_empty());
    }

    #[test]
    fn spark_like_script() {
        let text = b"[STATE]\r\ntype\t\t\t1\r\nvelscale\t1.00\r\nsource_size\tline\t0.5 0 0\t1.5 0 0\r\n\
            lifetime\tpoint\t3 0 0\r\n\r\n[ACTION]\r\nsource s0 300 point 0 0 0 sphere 0 0 0 0.5 2\r\n\
            ;bounce b0 0.01 0.35 0 plane 0 5 0 0 1 0\r\nmove m0 1 -1\r\ndamping d0 0.5 0.5 0.5\r\n";
        let c = ParticleCfg::parse(text);
        assert_eq!(c.state.kind, 0x10);
        assert_eq!(c.state.flags, 4);
        assert_eq!(
            c.state.source_size,
            Domain::Line {
                p: [0.5, 0.0, 0.0],
                d: [1.0, 0.0, 0.0]
            }
        );
        assert_eq!(c.state.lifetime, Domain::Point { p: [3.0, 0.0, 0.0] });
        let kinds: Vec<u32> = c.actions.iter().map(|a| a.kind.type_id()).collect();
        assert_eq!(kinds, [10, 7, 3]);
        assert_eq!(c.actions[1].kind, ActionKind::Move { flags: 1 | 8 });
        match &c.actions[0].kind {
            ActionKind::Source { rate, velocity, .. } => {
                assert_eq!(*rate, 300.0);
                assert_eq!(
                    velocity.as_ref().unwrap(),
                    &Domain::Sphere {
                        center: [0.0; 3],
                        r_max: 2.0,
                        r_min: 0.5,
                        r_max2: 4.0,
                        r_min2: 0.25
                    }
                );
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(
            c.actions[2].kind,
            ActionKind::Damping {
                damping: [0.5; 3],
                vlow: 0.0,
                vhigh: 1.0e6
            }
        );
    }

    #[test]
    fn unknown_domain_keeps_the_previous_state_value() {
        let c = ParticleCfg::parse(b"[STATE]\nsource_size blob 1 2 3\n");
        assert_eq!(c.state.source_size, ParticleState::defaults().source_size);
        assert_eq!(c.state.flags, 4); // the flag is set even so, as in the exe
    }
}
