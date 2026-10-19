//! Keywords that write vehicle parameters: the engine / drivetrain / driver / steering numbers
//! in the parameter block `P = *(K+0x2e48)` and the scalar fields of `K`.

use super::*;

const ONE: u32 = 0x3f80_0000;

/// `enginepower a b c d [e f]` (stock: `135.0 6.5 0.00006 0.003`). Starts from the current
/// values, so absent trailing numbers keep the defaults; `a` is stored twice.
///
/// exe: FUN_00443f90
pub fn enginepower(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let l = [0xd0, 0xe0, 0xf0, 0xf4, 0xe4, 0xe8].map(|o| c.r(p + o));
    let s = scan(line, "%f %f %f %f %f %f");
    let v: Vec<u32> = (0..6).map(|i| s.fb(i).unwrap_or(l[i])).collect();
    c.w(p + 0xd0, v[0]);
    c.w(p + 0xd4, v[0]);
    c.w(p + 0xe0, v[1]);
    c.w(p + 0xf0, v[2]);
    c.w(p + 0xf4, v[3]);
    c.w(p + 0xe4, v[4]);
    c.w(p + 0xe8, v[5]);
}

/// `ratios n r1..r7`: gear count at `P+0x1c`, ratios at `P+0x24..0x3c`.
///
/// exe: FUN_00444080
pub fn ratios(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%d %f %f %f %f %f %f %f");
    c.put(p + 0x1c, s.db(0));
    for (i, o) in [0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c]
        .into_iter()
        .enumerate()
    {
        c.put(p + o, s.fb(i + 1));
    }
}

/// `torque n (rpm:torque)*n`: count at `P[0]`, pairs into the array at `P[1]`. `%#` makes
/// `sscanf` return the bytes consumed, which is how the exe walks the line.
///
/// exe: FUN_004440e0
pub fn torque(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let mut text = rest(line);
    let s = slr_fmt_cfg::scan::sscanf(text, b"%#%d", slr_fmt_cfg::Precision::F32);
    c.put(p, s.db(0));
    text = &text[(s.ret.max(0) as usize).min(text.len())..];
    let n = c.r(p) as i32;
    for i in 0..n.max(0) as u32 {
        let dst = c.r(p + 4).wrapping_add(i * 8);
        let s = slr_fmt_cfg::scan::sscanf(text, b"%#%f:%f", slr_fmt_cfg::Precision::F32);
        c.put(dst, s.fb(0));
        c.put(dst + 4, s.fb(1));
        text = &text[(s.ret.max(0) as usize).min(text.len())..];
    }
}

/// `rearendratio a`: final drive ratio at `P+0x40`.
///
/// exe: FUN_00444150
pub fn rearendratio(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%f");
    c.put(p + 0x40, s.fb(0));
}

/// `brakepower front rear`: `P+0xd8`, `P+0xdc`.
///
/// exe: FUN_00444190
pub fn brakepower(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let (a, b) = (c.r(p + 0xd8), c.r(p + 0xdc));
    let s = scan(line, "%f %f");
    c.w(p + 0xd8, s.fb(0).unwrap_or(a));
    c.w(p + 0xdc, s.fb(1).unwrap_or(b));
}

/// `clutch a`: `P+0xc8`.
///
/// exe: FUN_00444200
pub fn clutch(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let a = c.r(p + 0xc8);
    let s = scan(line, "%f");
    c.w(p + 0xc8, s.fb(0).unwrap_or(a));
}

/// `driver` / `controller` (one handler; the keyword decides): appends a 0x90-byte driver
/// record at `K+0x1784+n*0x90`, at most four.
///
/// exe: FUN_004445f0
pub fn driver(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && (c.kr(0x177c) as i32) < 4) {
        return;
    }
    if c.kr(0x177c) == 0 {
        c.kw(0x1780, 0);
    }
    let n = c.kr(0x177c);
    if stricmp_eq(line, b"driver") {
        c.kw(0x1780, n.wrapping_sub(1));
    }
    let d = c.k.wrapping_add(n.wrapping_mul(0x90)).wrapping_add(0x1784);
    c.kw(0x177c, n.wrapping_add(1));
    let s = scan(line, "%d %d %d %d %d %d %d %d");
    for (i, o) in [0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40]
        .into_iter()
        .enumerate()
    {
        c.put(d + o, s.db(i));
    }
    c.w(d + 0x20, s.ret as u32);
    let f126c = c.kf(0x126c);
    c.w(d + 0x70, 0);
    c.w(d + 0x74, 0);
    c.wf(d + 0x78, -f126c);
    c.w(d + 0x64, 0);
    c.w(d + 0x68, 0);
    c.w(d + 0x6c, 0);
    let v1278 = c.kr(0x1278);
    c.w(d + 0x88, v1278);
    let f3 = c.kf(0x1270) + 10.0f32; // 0x5e7334
    let f1 = c.kf(0x1274);
    c.w(d + 0x7c, 0);
    c.wf(d + 0x80, -(f1 + 1.0f32)); // 0x5f0fb8
    c.wf(d + 0x84, -f3);
    c.w(d + 0x8c, 0x42c8_0000);
}

/// `ext_camera`: exterior camera record at `K+0x16f4+driver*0x90`; the first and last numbers
/// are scaled by `K+0x1278` / `K+0x127c` whether or not they parsed.
///
/// exe: FUN_00443d40
pub fn ext_camera(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let r =
        c.k.wrapping_add(c.kr(0x177c).wrapping_mul(0x90))
            .wrapping_add(0x16f4);
    let s = scan(line, "%f %f,%f,%f %f,%f,%f %f,%f,%f %f");
    let dst = [
        0x8c, 0x64, 0x68, 0x6c, 0x70, 0x74, 0x78, 0x7c, 0x80, 0x84, 0x88,
    ];
    for (i, o) in dst.into_iter().enumerate() {
        c.put(r + o, s.fb(i));
    }
    let (a, b) = (c.kf(0x1278), c.kf(0x127c));
    let x = c.rf(r + 0x88);
    c.wf(r + 0x88, a * x);
    let y = c.rf(r + 0x8c);
    c.wf(r + 0x8c, b * y);
}

/// `nodriver`: no current driver.
///
/// exe: FUN_00444710
pub fn nodriver(c: &mut Ctx, _line: &[u8]) {
    if c.guard() {
        c.kw(0x1780, 0xffff_ffff);
    }
}

/// `name`: accepted and ignored.
///
/// exe: FUN_00444730
pub fn name(_c: &mut Ctx, _line: &[u8]) {}

/// Six floats into two vec3 locals (the second is zeroed by `FUN_00551c70`).
fn six(line: &[u8]) -> [u32; 6] {
    let s = scan(line, "%f %f %f %f %f %f");
    [0, 1, 2, 3, 4, 5].map(|i| s.fb(i).unwrap_or(0))
}

/// `seat x y z a b c`: `P+0x91c..0x930`.
///
/// exe: FUN_00444740
pub fn seat(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let v = six(line);
    let p = c.p();
    for (i, x) in v.into_iter().enumerate() {
        c.w(p + 0x91c + 4 * i as u32, x);
    }
}

/// `pedals x y z a b c`: three copies at `P+0x934`, `P+0x94c`, `P+0x964`; the first copy's x
/// is moved by -0.1 and the third's by +0.1.
///
/// exe: FUN_00444810
pub fn pedals(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let v = six(line);
    let p = c.p();
    for base in [0x934, 0x94c, 0x964] {
        for (i, x) in v.into_iter().enumerate() {
            c.w(p + base + 4 * i as u32, x);
        }
    }
    let a = c.rf(p + 0x934) - 0.1f32; // 0x5f08e8
    c.wf(p + 0x934, a);
    let b = c.rf(p + 0x964) + 0.1f32;
    c.wf(p + 0x964, b);
}

/// `shifter x y z a b c`: `P+0x9ac..0x9c0`.
///
/// exe: FUN_00444990
pub fn shifter(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let v = six(line);
    let p = c.p();
    for (i, x) in v.into_iter().enumerate() {
        c.w(p + 0x9ac + 4 * i as u32, x);
    }
}

/// `key`: parsed into locals and dropped.
///
/// exe: FUN_00444a60
pub fn key(_c: &mut Ctx, _line: &[u8]) {}

/// `handbrake`: parsed into locals and dropped.
///
/// exe: FUN_00444ae0
pub fn handbrake(_c: &mut Ctx, _line: &[u8]) {}

/// `mass a`: `K+0x1ec4`.
///
/// exe: FUN_00443f50
pub fn mass(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f");
    c.put(c.k + 0x1ec4, s.fb(0));
}

/// `maxsteer a b`: `P+0xa0c`, `P+0xa1c`.
///
/// exe: FUN_00444260
pub fn maxsteer(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%f %f");
    c.put(p + 0xa0c, s.fb(0));
    c.put(p + 0xa1c, s.fb(1));
}

/// `rpm_idle rpm` (default 800): idle speed in rad/s at `P+0xec`.
///
/// exe: FUN_004442b0
pub fn rpm_idle(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%f");
    let rpm = s.f(0).unwrap_or(800.0);
    c.wf(p + 0xec, rpm * f32::from_bits(0x3dd6_7750)); // 0x5f0ef0
}

/// `drag a x y z` (default 0.3): `P+0x908` scaled by 1.116, vector at `P+0x8fc`.
///
/// exe: FUN_00444310
pub fn drag(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f %f");
    let a = s.f(0).unwrap_or(f32::from_bits(0x3e99_999a));
    let v = [1, 2, 3].map(|i| s.fb(i).unwrap_or(0));
    let p = c.p();
    c.wf(p + 0x908, a * f32::from_bits(0x3f8e_d917)); // 0x5f0fb4
    c.w(p + 0x8fc, v[0]);
    c.w(p + 0x900, v[1]);
    c.w(p + 0x904, v[2]);
}

/// `wing`: appends a wing record (`K+0x2bc8 + n*0x28`, at most 17).
///
/// exe: FUN_004443c0
pub fn wing(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let n = c.kr(0x2bc4);
    if (n as i32) >= 0x11 {
        return;
    }
    let r = c.k.wrapping_add(0x2bc8).wrapping_add(n.wrapping_mul(0x28));
    c.kw(0x2bc4, n.wrapping_add(1));
    let s = scan(line, "%d %f %f %f %f %f %f %f %f %f %f %f %f %f");
    let id = s.db(0).unwrap_or(0);
    let pos = [1, 2, 3].map(|i| s.fb(i).unwrap_or(0));
    let dir = [4, 5, 6].map(|i| s.f(i).unwrap_or(0.0));
    let f34 = s.fb(7).unwrap_or(0);
    let f30 = s.fb(8).unwrap_or(0);
    let u2c = s.fb(9).unwrap_or(ONE);
    c.w(r, id);
    c.w(r + 4, pos[0]);
    c.w(r + 8, pos[1]);
    c.w(r + 12, pos[2]);
    // The exe (0x4444ec-0x44450b) folds the squares as (dir[1]^2 + dir[2]^2) + dir[0]^2, i.e. the
    // last two dir floats first; Rust's left-to-right `a + b + c` must follow that association or
    // `len` (and with it all three normalised components) shifts by one ULP.
    let len = (dir[1] * dir[1] + dir[2] * dir[2] + dir[0] * dir[0]).sqrt();
    if len != 0.0 {
        let inv = 1.0f32 / len;
        c.wf(r + 16, dir[0] * inv);
        c.wf(r + 20, inv * dir[1]);
        c.wf(r + 24, dir[2] * inv);
    }
    c.w(r + 28, f34 | 0x8000_0000);
    c.w(r + 32, f30 & 0x7fff_ffff);
    c.w(r + 36, u2c);
}

/// `center x y z`: `K+0x1eac..0x1eb4`.
///
/// exe: FUN_00444570
pub fn center(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f");
    for i in 0..3 {
        c.kw(0x1eac + 4 * i as u32, s.fb(i).unwrap_or(0));
    }
}

/// `rollbar a b c d`: appends a 16-byte entry at `P+0x48+n*0x10` (at most 8).
///
/// exe: FUN_00443a60
pub fn rollbar(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    if (c.r(p + 0x44) as i32) >= 8 {
        return;
    }
    let s = scan(line, "%d %d %f %f");
    let v = [
        s.db(0).unwrap_or(0),
        s.db(1).unwrap_or(0),
        s.fb(2).unwrap_or(0x461c_4000),
        s.fb(3).unwrap_or(0x43fa_0000),
    ];
    let n = c.r(p + 0x44);
    let e = p.wrapping_add(n.wrapping_mul(0x10)).wrapping_add(0x48);
    c.w(p + 0x44, n.wrapping_add(1));
    for (i, x) in v.into_iter().enumerate() {
        c.w(e + 4 * i as u32, x);
    }
}

/// `sw_radius` / `steeringradius` (one handler): `P+0xa14`, `P+0xa18`, both scaled by 0.01
/// whether or not they parsed.
///
/// exe: FUN_00443ef0
pub fn sw_radius(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    let s = scan(line, "%f %f");
    c.put(p + 0xa14, s.fb(0));
    c.put(p + 0xa18, s.fb(1));
    let k = f32::from_bits(0x3c23_d70a); // 0x5f0c20
    let a = c.rf(p + 0xa14) * k;
    c.wf(p + 0xa14, a);
    let b = c.rf(p + 0xa18) * k;
    c.wf(p + 0xa18, b);
}

/// `steerhelp`: six floats at `K+0x12bc..0x12d0`.
///
/// exe: FUN_004460a0
pub fn steerhelp(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f %f %f %f");
    for i in 0..6 {
        c.put(c.k + 0x12bc + 4 * i as u32, s.fb(i));
    }
}

/// `cheat_control`: `K+0x12dc, 0x12e8, 0x12e0, 0x12e4, 0x12b8`.
///
/// exe: FUN_00446100
pub fn cheat_control(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f %f %f");
    for (i, o) in [0x12dc, 0x12e8, 0x12e0, 0x12e4, 0x12b8]
        .into_iter()
        .enumerate()
    {
        c.put(c.k + o, s.fb(i));
    }
}

/// `notify`: sets flag 2 in `K+0x2e4c`.
///
/// exe: FUN_00446160
pub fn notify(c: &mut Ctx, _line: &[u8]) {
    if c.guard() {
        let v = c.kr(0x2e4c) | 2;
        c.kw(0x2e4c, v);
    }
}

/// `velfactor a b`: `K+0x1298`, `K+0x129c`.
///
/// exe: FUN_00445fc0
pub fn velfactor(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f");
    c.put(c.k + 0x1298, s.fb(0));
    c.put(c.k + 0x129c, s.fb(1));
}

/// `turn x y z`: the current `K+0x12ec..0x12f4` are negated, replaced by the parsed numbers
/// (if any) and negated again.
///
/// exe: FUN_00445f00
pub fn turn(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let m1 = -1.0f32; // 0x5f0c70
    let cur = [0x12ec, 0x12f0, 0x12f4].map(|o| c.kf(o) * m1);
    let s = scan(line, "%f %f %f");
    for (i, o) in [0x12ec, 0x12f0, 0x12f4].into_iter().enumerate() {
        let v = s.f(i).unwrap_or(cur[i]) * m1;
        c.kw(o, v.to_bits());
    }
}

/// `roll a b c`: `K+0x12b4, 0x12a8, 0x12ac`.
///
/// exe: FUN_00446000
pub fn roll(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f");
    for (i, o) in [0x12b4, 0x12a8, 0x12ac].into_iter().enumerate() {
        c.put(c.k + o, s.fb(i));
    }
}

/// `pitch a b c`: `K+0x12b0, 0x12a0, 0x12a4`.
///
/// exe: FUN_00446050
pub fn pitch(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f");
    for (i, o) in [0x12b0, 0x12a0, 0x12a4].into_iter().enumerate() {
        c.put(c.k + o, s.fb(i));
    }
}

/// `damage a b`: only the first number is kept, at `K+0x11d8`.
///
/// exe: FUN_0046cf50
pub fn damage(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f");
    c.kw(0x11d8, s.fb(0).unwrap_or(0));
}

/// `damagedef`: accepted and ignored.
///
/// exe: FUN_00445ef0
pub fn damagedef(_c: &mut Ctx, _line: &[u8]) {}

/// `material id a b weight`: appends a 0x48-byte record at `K+0x12fc+n*0x48` (at most 16) and
/// binds its three resource slots.
///
/// exe: FUN_00445e00
pub fn material(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let lookup = c.call(
        EXT_LOOKUP,
        This::None,
        vec![Arg::S("system.rpk".to_string()), Arg::U(3)],
    );
    let n = c.kr(0x12f8);
    if (n as i32) >= 0x10 {
        return;
    }
    let r = c.k.wrapping_add(0x12fc).wrapping_add(n.wrapping_mul(0x48));
    c.kw(0x12f8, n.wrapping_add(1));
    let s = scan(line, "%d %d %d %f");
    let id = s.db(0).unwrap_or(lookup);
    let a = s.db(1).unwrap_or(0);
    let b = s.db(2).unwrap_or(0);
    c.put(r + 0x44, s.fb(3));
    c.bind(r + 0x20, id, 0xe, 0);
    let h = c.resolve(a);
    c.bind(r, h, 6, 0);
    let h = c.resolve(b);
    c.bind(r + 0x30, h, 6, 0);
}
