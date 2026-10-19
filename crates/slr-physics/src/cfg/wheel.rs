//! Keywords that build the wheel table: wheel records at `K+0x1e7c+(i+1)*200` (count at
//! `K+8000`, at most 16) and the shared tyre / skid parameters.

use super::*;

/// Base of the current (last) wheel record: `K + 0x1e7c + count*200`.
fn wrec(c: &Ctx) -> u32 {
    c.k.wrapping_add(0x1e7c)
        .wrapping_add((c.nwheel() as u32).wrapping_mul(200))
}

/// `type mass shape r..` (shape `sphere`, `box` or a named mesh): appends a wheel record and
/// creates its body through the physics world.
///
/// exe: FUN_00442f90
pub fn type_(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let n = c.nwheel();
    if n > 0xf {
        return;
    }
    let r =
        c.k.wrapping_add(0x1f44)
            .wrapping_add((n as u32).wrapping_mul(200));
    c.kw(0x1f40, (n as u32).wrapping_add(1));
    let s = scan(line, "%f %s %f %f %f");
    let mass = s.fb(0).unwrap_or(0);
    let shape = s.s(1).map(|v| v.to_vec()).unwrap_or_default();
    let (a, b, d) = (
        s.fb(2).unwrap_or(0),
        s.fb(3).unwrap_or(0),
        s.fb(4).unwrap_or(0),
    );
    let body = if stricmp_eq(&shape, b"sphere") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(mass),
                Arg::U(1),
                Arg::U(a),
                Arg::U(0),
                Arg::U(0),
                Arg::U(0),
            ],
        )
    } else if stricmp_eq(&shape, b"box") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(mass),
                Arg::U(5),
                Arg::U(a),
                Arg::U(b),
                Arg::U(d),
                Arg::U(0),
            ],
        )
    } else {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(mass),
                Arg::U(6),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::S(String::from_utf8_lossy(&shape).into_owned()),
            ],
        )
    };
    attach_body(c.m, r + 0x48, body);
    let o = c.r(r + 0x48);
    let fl = c.r(o + 0x9c) & !1;
    c.w(o + 0x9c, fl);
    c.w(r + 0x30, ONE);
    c.w(r + 0x34, ONE);
    c.w(r + 0x44, a);
}

const ONE: u32 = 0x3f80_0000;

/// `friction a b`: `rec+0x30`, `rec+0x34`.
///
/// exe: FUN_004432b0
pub fn friction(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f");
    c.put(r + 0x30, s.fb(0));
    c.put(r + 0x34, s.fb(1));
}

/// `spring a b c`: `rec+0x38/0x3c/0x40`; a non-positive value is logged and replaced by
/// 0.1 / 0.2 / 0.05.
///
/// exe: FUN_004431c0
pub fn spring(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f");
    c.put(r + 0x38, s.fb(0));
    c.put(r + 0x3c, s.fb(1));
    c.put(r + 0x40, s.fb(2));
    const MSG: [&str; 3] = [
        "Invalid spring parameter (parameter 0 = %f)!",
        "Invalid spring parameter (parameter 1 = %f)!",
        "Invalid spring parameter (parameter 2 = %f)!",
    ];
    for (i, (o, repl)) in [
        (0x38u32, 0x3dcc_cccdu32),
        (0x3c, 0x3e4c_cccd),
        (0x40, 0x3d4c_cccd),
    ]
    .into_iter()
    .enumerate()
    {
        let v = c.rf(r + o);
        if v <= 0.0 {
            let bits = (v as f64).to_bits();
            c.call(
                EXT_LOG,
                This::None,
                vec![
                    Arg::S(MSG[i].to_string()),
                    Arg::U(bits as u32),
                    Arg::U((bits >> 32) as u32),
                ],
            );
            c.w(r + o, repl);
        }
    }
}

/// `wheel x y z a b c [steer drive brake hbrake]`: the main wheel line.
///
/// exe: FUN_004433e0
pub fn wheel(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f %f %f %f %f %f %f %f");
    let v6 = [0, 1, 2, 3, 4, 5].map(|i| s.fb(i).unwrap_or(0));
    let f24 = s.f(6).unwrap_or(0.0);
    let u1c = s.fb(7).unwrap_or(0);
    let mut p2 = s.f(8).unwrap_or(-1.0);
    let mut f28 = s.f(9).unwrap_or(-1.0);
    if p2 < 0.0 {
        p2 = 1.0;
    }
    if f28 < 0.0 {
        f28 = if f24 == 0.0 { 1.0 } else { 0.0 };
    }
    for (i, x) in v6.into_iter().enumerate() {
        c.w(r + 4 * i as u32, x);
    }
    c.w(r + 0x28, ONE);
    c.wf(r + 0x18, f24);
    c.w(r + 0x1c, u1c);
    c.w(r + 0x20, p2.to_bits() & 0x7fff_ffff);
    c.w(r + 0x2c, 0x41d8_0000);
    c.w(r + 0x64, ONE);
    c.w(r + 0x84, 0xffff_ffff);
    c.w(r + 0x68, 0xbf80_0000);
    c.w(r + 0x88, 0xbf80_0000);
    c.w(r + 0x24, f28.to_bits() & 0x7fff_ffff);
    c.w(r + 0xb0, 0);
    c.w(r + 0xb4, 0);
    c.w(r + 0xb8, 0x461c_4000);
    c.w(r + 0xbc, 0x461c_4000);
    c.w(r + 0xc0, 0);
    c.w(r + 0xc4, 0);
    c.w(r + 0x58, 0);
    c.w(r + 0x5c, 0x3ecc_cccd);
    c.w(r + 0x60, 0);
    let v = (c.nwheel() as u32).wrapping_sub(1) | 0x1000_0000;
    c.kw(0xffc, v);
}

/// `wheelbones`: gives the current wheel a unit-mass 0.1 sphere body (a bone anchor).
///
/// exe: FUN_004438d0
pub fn wheelbones(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let base = c.k.wrapping_add((c.nwheel() as u32).wrapping_mul(200));
    let body = c.call(
        EXT_BODY,
        This::Addr(PHYSICS_WORLD),
        vec![
            Arg::U(ONE),
            Arg::U(1),
            Arg::U(0x3dcc_cccd),
            Arg::U(0),
            Arg::U(0),
            Arg::U(0),
        ],
    );
    let slot = base + 0x1ecc;
    attach_body(c.m, slot, body);
    let o = c.r(slot);
    let f = c.r(o + 0x9c) | 8;
    c.w(o + 0x9c, f);
    let f = c.r(o + 0x9c) & !1;
    c.w(o + 0x9c, f);
}

/// `bonepos x y z`: `rec+0x58..0x60`.
///
/// exe: FUN_00443a00
pub fn bonepos(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f");
    for i in 0..3 {
        c.put(r + 0x58 + 4 * i as u32, s.fb(i));
    }
}

/// `wheelpars`: nine tyre coefficients at `K+0x2e50..0x2e90` (the default Pacejka block).
///
/// exe: FUN_00443310
pub fn wheelpars(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f %f %f %f %f %f %f");
    let dst = [
        0x2e50, 0x2e58, 0x2e60, 0x2e7c, 0x2e80, 0x2e84, 0x2e88, 0x2e8c, 0x2e90,
    ];
    for (i, o) in dst.into_iter().enumerate() {
        c.put(c.k + o, s.fb(i));
    }
}

/// `skid a b c`: `P+0x90c`, `P+0x914`, `P+0x918`.
///
/// exe: FUN_00443390
pub fn skid(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%f %f %f");
    c.put(p + 0x90c, s.fb(0));
    c.put(p + 0x914, s.fb(1));
    c.put(p + 0x918, s.fb(2));
}

/// `wheeldeform a`: `K+0x1ee0+count*200`.
///
/// exe: FUN_004439b0
pub fn wheeldeform(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let a =
        c.k.wrapping_add(0x1ee0)
            .wrapping_add((c.nwheel() as u32).wrapping_mul(200));
    let s = scan(line, "%f");
    c.put(a, s.fb(0));
}

/// `arm id x y z a b c` (suspension arm): `rec+0x68`, `rec+0x78..0x80` (offset by the wheel
/// position) and `rec+0x6c..0x74`. A missing `id` stores the car-class pointer, which is what
/// the exe's reused parameter slot holds.
///
/// exe: FUN_004435b0
pub fn arm(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f %f %f %f %f");
    let p2 = s.fb(0).unwrap_or(c.k);
    let v: Vec<f32> = (1..7).map(|i| s.f(i).unwrap_or(0.0)).collect();
    c.w(r + 0x68, p2);
    let (x, y, z) = (c.rf(r), c.rf(r + 4), c.rf(r + 8));
    c.wf(r + 0x78, v[0] + x);
    c.wf(r + 0x7c, v[1] + y);
    c.wf(r + 0x80, v[2] + z);
    c.wf(r + 0x6c, v[3]);
    c.wf(r + 0x70, v[4]);
    c.wf(r + 0x74, v[5]);
}

/// `hub id x y z a b c u v w`: `rec+0x88..`, with the last three offset by the wheel position.
/// A missing `id` stores the car-class pointer (see [`arm`]).
///
/// exe: FUN_00443690
pub fn hub(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f %f %f %f %f %f %f %f");
    let p2 = s.fb(0).unwrap_or(c.k);
    let v: Vec<u32> = (1..7).map(|i| s.fb(i).unwrap_or(0)).collect();
    let w: Vec<f32> = (7..10).map(|i| s.f(i).unwrap_or(0.0)).collect();
    c.w(r + 0x88, p2);
    for (i, x) in v.into_iter().enumerate() {
        c.w(r + 0x8c + 4 * i as u32, x);
    }
    let (x, y, z) = (c.rf(r), c.rf(r + 4), c.rf(r + 8));
    c.wf(r + 0xa4, w[0] + x);
    c.wf(r + 0xa8, w[1] + y);
    c.wf(r + 0xac, w[2] + z);
}

/// `instant_center a b c d e f`: two points offset by the wheel position at `rec+0xb0..0xc4`.
/// (The exe's format string has ten `%f` but only six destinations; lines with more than six
/// numbers would store through stack garbage and are not modelled.)
///
/// exe: FUN_004437a0
pub fn instant_center(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nwheel() > 0) {
        return;
    }
    let r = wrec(c);
    let s = scan(line, "%f %f %f %f %f %f %f %f %f %f");
    let v: Vec<f32> = (0..6).map(|i| s.f(i).unwrap_or(0.0)).collect();
    let (x, y, z) = (c.rf(r), c.rf(r + 4), c.rf(r + 8));
    c.wf(r + 0xb0, v[0] + x);
    c.wf(r + 0xb4, v[1] + y);
    c.wf(r + 0xb8, v[2] + z);
    c.wf(r + 0xbc, v[3] + x);
    c.wf(r + 0xc0, v[4] + y);
    c.wf(r + 0xc4, v[5] + z);
}

/// `steering ...`: a `body` line that also marks the body as the steering body and derives
/// its euler angles into `K+0x11c8`.
///
/// exe: FUN_00443ea0
pub fn steering(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    super::part::body(c, line);
    let n = c.kr(0xfa8);
    c.kw(0xff4, n.wrapping_sub(1));
    let mat = c.k.wrapping_add(0xc0).wrapping_add(n.wrapping_mul(0x74));
    let m: Vec<Arg> = (0..11).map(|i| Arg::U(c.r(mat + 4 * i))).collect();
    c.call(EXT_MATRIX_TO_EULER, This::Addr(c.k + 0x11c8), m);
}
