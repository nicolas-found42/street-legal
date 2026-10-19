//! Keywords that build cameras, HUD gauges, lights and sound records: the camera
//! table (`P+0xa54`), cockpit needles, the `osd_*` gauge blocks and the `sfx_*`
//! sound records in the parameter block `P = *(K+0x2e48)`, plus the `render` /
//! `light` shared body-record logic.

use super::*;
/// `render ...` / `light ...`: the exe shares one handler with `deformable`, so this is the
/// exe's `render` wrapper around it: the same body-record append, resource bind and
/// `K+0x1108` owner chain, followed by two flag fixups on the just-appended record
/// (`cur + 0x58`) selected by the `K+0xffc` cascade word.
///
/// exe: FUN_00443de0
///
/// (`FUN_0046bfa0` is the shared body, ported as `part::deformable`.)
pub fn render(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    part::deformable(c, line);
    // Flag fixup on the just-appended record (`K + nbody*0x74 + 0x10c` == `cur + 0x58`).
    let f = c.kr(0xffc);
    let r2 = super::part::cur(c) + 0x58;
    if (f as i32) >> 24 == 0 {
        if f & 0xff == c.kr(0xff4) {
            c.w(r2, c.r(r2) | (c.kr(0xff4) << 24) | 0x8000);
        }
    } else if (f as i32) >> 24 == 0x10 && f & 0xff == c.kr(0x1f40).wrapping_sub(1) {
        c.w(
            r2,
            c.r(r2) | (c.kr(0x1f40).wrapping_add(0xff) << 24) | 0x1000,
        );
    }
}

/// `camera x y z a b c fov`: appends a 0x1c-byte camera record at
/// `P+0xa58+n*0x1c` (at most 16); the 7th number is scaled by `K+0x127c`
/// whether or not it parsed. The first record also seeds the seat/pedal
/// vectors and two shifter vec3s.
///
/// exe: FUN_00443b00
pub fn camera(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let p = c.p();
    if (c.r(p + 0xa54) as i32) >= 0x10 {
        return;
    }
    // `%f %f %f %f %f %f %f` (0x60f5c4).
    let s = scan(line, "%f %f %f %f %f %f %f");
    let n = c.r(p + 0xa54);
    let r = p.wrapping_add(0xa58).wrapping_add(n.wrapping_mul(0x1c));
    c.w(p + 0xa54, n.wrapping_add(1));
    for (i, o) in [0, 4, 8, 0xc, 0x10, 0x14, 0x18].into_iter().enumerate() {
        c.put(r + o, s.fb(i));
    }
    // The last number is scaled by `K+0x127c` whether or not it parsed.
    let b = c.kf(0x127c);
    c.wf(r + 0x18, c.rf(r + 0x18) * b);
    if c.r(p + 0xa54) != 1 {
        return;
    }
    c.w(p + 0x91c, c.r(p + 0xa58));
    c.wf(p + 0x920, c.rf(p + 0xa5c) - 0.55f32); // 0x5f0fb0
    c.wf(p + 0x924, c.rf(p + 0xa60) - 0.2f32); // 0x5f0cd4
    vec3_set(c.m, p + 0x928, 0, 0x3e99_999au32, 0);
    c.wf(p + 0x934, c.rf(p + 0x91c) - 0.1f32); // 0x5f08e8
    c.wf(p + 0x938, c.rf(p + 0x920) - 0.1f32);
    c.wf(p + 0x93c, c.rf(p + 0x924) - 0.6f32); // 0x5f0f00
    c.w(p + 0x94c, c.r(p + 0x91c));
    c.wf(p + 0x950, c.rf(p + 0x920) - 0.1f32);
    c.wf(p + 0x954, c.rf(p + 0x924) - 0.6f32);
    c.wf(p + 0x964, c.rf(p + 0x91c) + 0.1f32);
    c.wf(p + 0x968, c.rf(p + 0x920) - 0.1f32);
    c.wf(p + 0x96c, c.rf(p + 0x924) - 0.6f32);
    vec3_zero(c.m, p + 0x940);
    vec3_zero(c.m, p + 0x958);
    vec3_zero(c.m, p + 0x970);
    c.w(p + 0x9b0, 0xbdcc_cccd);
    c.w(p + 0x9b4, 0xbdcc_cccd);
    c.w(p + 0x9ac, 0);
    vec3_zero(c.m, p + 0x9b8);
}

/// `cockpit_rpm ...`: needle record at `K+0x19d4..0x19f0` plus the euler pair at
/// `K+0x1a0c..0x1a18`; the resource binds at `K+0x19f0`.
///
/// exe: FUN_00444b60
pub fn cockpit_rpm(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%f %f %f %f %f %f %d %f %f %f %f` (0x60f72c).
    let s = scan(line, "%f %f %f %f %f %f %d %f %f %f %f");
    for (i, o) in [0x19d4, 0x19d8, 0x19dc, 0x19e0, 0x19e4, 0x19e8]
        .into_iter()
        .enumerate()
    {
        c.put(c.k + o, s.fb(i));
    }
    let id = s.db(6).unwrap_or(0);
    for (i, o) in [0x1a0c, 0x1a10, 0x1a14, 0x1a18].into_iter().enumerate() {
        c.put(c.k + o, s.fb(7 + i));
    }
    c.load(c.k + 0x19f0, id, 0xe, 0);
}

/// `cockpit_speed ...`: needle record at `K+0x1a1c..0x1a38` plus the euler pair
/// at `K+0x1a54..0x1a60`; the resource binds at `K+0x1a38`.
///
/// exe: FUN_00444cb0
pub fn cockpit_speed(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%f %f %f %f %f %f %d %f %f %f %f` (0x60f750).
    let s = scan(line, "%f %f %f %f %f %f %d %f %f %f %f");
    for (i, o) in [0x1a1c, 0x1a20, 0x1a24, 0x1a28, 0x1a2c, 0x1a30]
        .into_iter()
        .enumerate()
    {
        c.put(c.k + o, s.fb(i));
    }
    let id = s.db(6).unwrap_or(0);
    for (i, o) in [0x1a54, 0x1a58, 0x1a5c, 0x1a60].into_iter().enumerate() {
        c.put(c.k + o, s.fb(7 + i));
    }
    c.load(c.k + 0x1a38, id, 0xe, 0);
}

/// `osd_gauge id x y z`: floats at `K+0x1b84..0x1b8c`, always bound at `K+0x1ba0`.
///
/// exe: FUN_00444e00
pub fn osd_gauge(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f` (0x60f774).
    let s = scan(line, "%d %f %f %f");
    let id = s.db(0).unwrap_or(0);
    for (i, o) in [0x1b84, 0x1b88, 0x1b8c].into_iter().enumerate() {
        c.put(c.k + o, s.fb(1 + i));
    }
    c.load(c.k + 0x1ba0, id, 0xe, 0);
}

/// `osd_rpmpin id x y z a b c d`: floats at `K+0x1a64..0x1a80`, always bound at
/// `K+0x1a80`.
///
/// exe: FUN_00444e80
pub fn osd_rpmpin(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f %f %f` (0x60f780).
    let s = scan(line, "%d %f %f %f %f %f %f %f");
    osd8(c, &s, 0x1a64, 0x1a80);
}

/// `osd_spdpin id x y z a b c d`: floats at `K+0x1aac..0x1ac8`, always bound at
/// `K+0x1ac8`.
///
/// exe: FUN_00444f20
pub fn osd_spdpin(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f %f %f` (0x60f798).
    let s = scan(line, "%d %f %f %f %f %f %f %f");
    osd8(c, &s, 0x1aac, 0x1ac8);
}

/// `osd_nospin id x y z a b c d`: floats at `K+0x1af4..0x1b10`, always bound at
/// `K+0x1b10`.
///
/// exe: FUN_00444fc0
pub fn osd_nospin(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f %f %f` (0x60f7b0).
    let s = scan(line, "%d %f %f %f %f %f %f %f");
    osd8(c, &s, 0x1af4, 0x1b10);
}

/// Shared tail of the always-bound eight-float gauges (`osd_rpmpin`,
/// `osd_spdpin`, `osd_nospin`): the id and seven floats go to `base..`,
/// the resource binds at `slot`.
fn osd8(c: &mut Ctx, s: &Scan, base: u32, slot: u32) {
    let id = s.db(0).unwrap_or(0);
    for (i, o) in [0, 4, 8, 0xc, 0x10, 0x14, 0x18].into_iter().enumerate() {
        c.put(c.k + base + o, s.fb(1 + i));
    }
    c.load(c.k + slot, id, 0xe, 0);
}

/// `osd_damage id x y z`: floats at `K+0x1b3c..0x1b44`, always bound at
/// `K+0x1b58`.
///
/// exe: FUN_00445060
pub fn osd_damage(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f` (0x60f7c8).
    let s = scan(line, "%d %f %f %f");
    let id = s.db(0).unwrap_or(0);
    for (i, o) in [0x1b3c, 0x1b40, 0x1b44].into_iter().enumerate() {
        c.put(c.k + o, s.fb(1 + i));
    }
    c.load(c.k + 0x1b58, id, 0xe, 0);
}

/// `osd_damage_slot c`: accepted and ignored. The exe masks its car-object
/// argument down to a byte (`K & 0xffffff00`) and uses that stack scratch as
/// the `%c` destination, so the parsed character is never stored.
///
/// exe: FUN_004450e0
pub fn osd_damage_slot(_c: &mut Ctx, _line: &[u8]) {}

/// Shared tail of the conditionally-bound `osd_*` gauges: the id and three
/// floats go to `f0..`, the `%d` goes to `i0`, the `%8s` text (when present) is
/// copied to `t0`, and the resource binds at `slot` only when `sscanf`
/// converted something (`ret > 0`; `osd_gear` needs `ret > 1`).
#[allow(clippy::too_many_arguments)]
fn osd_cond(
    c: &mut Ctx,
    s: &Scan,
    fs: [u32; 3],
    int: u32,
    text: Option<u32>,
    more: &[(u32, usize)],
    slot: u32,
    ret_gt_one: bool,
) {
    let id = s.db(0).unwrap_or(0);
    for (i, o) in fs.into_iter().enumerate() {
        c.put(c.k + o, s.fb(1 + i));
    }
    c.put(c.k + int, s.db(4));
    if let Some(t0) = text {
        if let Some(t) = s.s(5) {
            for (i, b) in t.iter().take(8).enumerate() {
                c.m.w8(c.k + t0 + i as u32, *b);
            }
        }
    }
    for (o, i) in more.iter() {
        c.put(c.k + *o, s.fb(*i));
    }
    let need = if ret_gt_one { 1 } else { 0 };
    if s.ret > need {
        c.load(c.k + slot, id, 0xe, 0);
    }
}

/// `osd_gear id x y z n text`: floats at `K+0x1c5c..0x1c64`, `%d` at `K+0x1c88`,
/// `%8s` text at `K+0x19c8`; bound at `K+0x1c78` only when `sscanf` converted
/// more than one item.
///
/// exe: FUN_00445120
pub fn osd_gear(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f7d8).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1c5c, 0x1c60, 0x1c64],
        0x1c88,
        Some(0x19c8),
        &[],
        0x1c78,
        true,
    );
}

/// `osd_speed id x y z n text`: floats at `K+0x1c14..0x1c1c`, `%d` at `K+0x1c40`,
/// `%8s` text at `K+0x1c44`; bound at `K+0x1c30` when anything converted.
///
/// exe: FUN_004451b0
pub fn osd_speed(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f7ec).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1c14, 0x1c18, 0x1c1c],
        0x1c40,
        Some(0x1c44),
        &[],
        0x1c30,
        false,
    );
}

/// `osd_rpm id x y z n text`: floats at `K+0x1bcc..0x1bd4`, `%d` at `K+0x1bf8`,
/// `%8s` text at `K+0x1bfc`; bound at `K+0x1be8` when anything converted.
///
/// exe: FUN_00445260
pub fn osd_rpm(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f800).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1bcc, 0x1bd0, 0x1bd4],
        0x1bf8,
        Some(0x1bfc),
        &[],
        0x1be8,
        false,
    );
}

/// `osd_nos id x y z n text a b c d`: floats at `K+0x1ca4..0x1cac`, `%d` at
/// `K+0x1cd0`, `%8s` text at `K+0x1cd4`, more floats at `K+0x1cdc..0x1ce8`;
/// bound at `K+0x1cc0` when anything converted.
///
/// exe: FUN_00445310
pub fn osd_nos(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s %f %f %f %f` (0x60f814).
    let s = scan(line, "%d %f %f %f %d %8s %f %f %f %f");
    osd_cond(
        c,
        &s,
        [0x1ca4, 0x1ca8, 0x1cac],
        0x1cd0,
        Some(0x1cd4),
        &[(0x1cdc, 6), (0x1ce0, 7), (0x1ce4, 8), (0x1ce8, 9)],
        0x1cc0,
        false,
    );
}

/// `osd_turbo id x y z n text a b c d`: floats at `K+0x1cec..0x1cf4`, `%d` at
/// `K+0x1d18`, `%8s` text at `K+0x1d1c`, more floats at `K+0x1d24..0x1d30`;
/// bound at `K+0x1d08` when anything converted.
///
/// exe: FUN_004453c0
pub fn osd_turbo(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s %f %f %f %f` (0x60f834).
    let s = scan(line, "%d %f %f %f %d %8s %f %f %f %f");
    osd_cond(
        c,
        &s,
        [0x1cec, 0x1cf0, 0x1cf4],
        0x1d18,
        Some(0x1d1c),
        &[(0x1d24, 6), (0x1d28, 7), (0x1d2c, 8), (0x1d30, 9)],
        0x1d08,
        false,
    );
}

/// `osd_mileage id x y z n text a b c d`: floats at `K+0x1d34..0x1d3c`, `%d` at
/// `K+0x1d60`, `%8s` text at `K+0x1d64`, more floats at `K+0x1d6c..0x1d78`;
/// bound at `K+0x1d50` when anything converted.
///
/// exe: FUN_00445470
pub fn osd_mileage(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s %f %f %f %f` (0x60f854).
    let s = scan(line, "%d %f %f %f %d %8s %f %f %f %f");
    osd_cond(
        c,
        &s,
        [0x1d34, 0x1d38, 0x1d3c],
        0x1d60,
        Some(0x1d64),
        &[(0x1d6c, 6), (0x1d70, 7), (0x1d74, 8), (0x1d78, 9)],
        0x1d50,
        false,
    );
}

/// `osd_abs id x y z n`: floats at `K+0x1d7c..0x1d84`, `%d` at `K+0x1da8`;
/// bound at `K+0x1d98` when anything converted. The `%8s` conversion has no
/// destination in the exe and is dropped.
///
/// exe: FUN_00445520
pub fn osd_abs(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f874).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1d7c, 0x1d80, 0x1d84],
        0x1da8,
        None,
        &[],
        0x1d98,
        false,
    );
}

/// `osd_asr id x y z n`: floats at `K+0x1dc4..0x1dcc`, `%d` at `K+0x1df0`;
/// bound at `K+0x1de0` when anything converted. The `%8s` conversion has no
/// destination in the exe and is dropped.
///
/// exe: FUN_004455b0
pub fn osd_asr(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f894).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1dc4, 0x1dc8, 0x1dcc],
        0x1df0,
        None,
        &[],
        0x1de0,
        false,
    );
}

/// `osd_hbrake id x y z n`: floats at `K+0x1e0c..0x1e14`, `%d` at `K+0x1e38`;
/// bound at `K+0x1e28` when anything converted. The `%8s` conversion has no
/// destination in the exe and is dropped.
///
/// exe: FUN_00445640
pub fn osd_hbrake(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f8b4).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1e0c, 0x1e10, 0x1e14],
        0x1e38,
        None,
        &[],
        0x1e28,
        false,
    );
}

/// `osd_emergency id x y z n`: floats at `K+0x1e54..0x1e5c`, `%d` at `K+0x1e80`;
/// bound at `K+0x1e70` when anything converted. The `%8s` conversion has no
/// destination in the exe and is dropped.
///
/// exe: FUN_004456d0
pub fn osd_emergency(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %d %8s` (0x60f8d4).
    let s = scan(line, "%d %f %f %f %d %8s");
    osd_cond(
        c,
        &s,
        [0x1e54, 0x1e58, 0x1e5c],
        0x1e80,
        None,
        &[],
        0x1e70,
        false,
    );
}

/// Shared tail of the triple-`sfx_*` handlers: appends a 0x24-byte record at
/// `P+base+n*0x24` (at most 16), resolves + binds the sound resource and
/// stores `1.0/f` plus the four parameters.
///
/// exe: FUN_00445760
/// exe: FUN_00445890
/// exe: FUN_004459c0
fn sfx3(c: &mut Ctx, line: &[u8], fmt: &str, base: u32, count: u32) {
    let p = c.p();
    if (c.r(p + count) as i32) >= 0x10 {
        return;
    }
    let s = scan(line, fmt);
    let id = s.db(0).unwrap_or(0);
    let f = s.f(1).unwrap_or(1000.0);
    let v = [
        s.fb(2).unwrap_or(0x3f80_0000),
        s.fb(3).unwrap_or(0x4974_2400),
        s.fb(4).unwrap_or(0x3f80_0000),
        s.fb(5).unwrap_or(0x3f80_0000),
    ];
    let n = c.r(p + count);
    let r = p.wrapping_add(base).wrapping_add(n.wrapping_mul(0x24));
    c.w(p + count, n.wrapping_add(1));
    c.load(r, id, 6, 0);
    c.wf(r + 0x10, 1.0f32 / f);
    for (i, o) in [0x14, 0x18, 0x1c, 0x20].into_iter().enumerate() {
        c.w(r + o, v[i]);
    }
}

/// `sfx_engine_up id f a b c d`: engine-up record (`P+0xf8`, count `P+0x338`).
///
/// exe: FUN_00445760
pub fn sfx_engine_up(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f` (0x60f8f4).
    sfx3(c, line, "%d %f %f %f %f %f", 0xf8, 0x338);
}

/// `sfx_engine_down id f a b c d`: engine-down record (`P+0x340`, count
/// `P+0x580`).
///
/// exe: FUN_00445890
pub fn sfx_engine_down(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f` (0x60f904).
    sfx3(c, line, "%d %f %f %f %f %f", 0x340, 0x580);
}

/// `sfx_exhaust id f a b c d`: exhaust record (`P+0x588`, count `P+0x7c8`).
///
/// exe: FUN_004459c0
pub fn sfx_exhaust(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %f %f %f %f` (0x60f918).
    sfx3(c, line, "%d %f %f %f %f %f", 0x588, 0x7c8);
}

/// `sfx_gearbox_fwd id f`: float at `P+0x804`, resource bound at `P+0x7d4`.
///
/// exe: FUN_00445af0
pub fn sfx_gearbox_fwd(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f` (0x60f92c).
    let s = scan(line, "%d %f");
    let id = s.db(0).unwrap_or(0);
    let p = c.p();
    c.load(p + 0x7d4, id, 6, 0);
    c.put(p + 0x804, s.fb(1));
}

/// `sfx_gearbox_rev id f`: float at `P+0x808`, resource bound at `P+0x7e4`.
///
/// exe: FUN_00445b80
pub fn sfx_gearbox_rev(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f` (0x60f934).
    let s = scan(line, "%d %f");
    let id = s.db(0).unwrap_or(0);
    let p = c.p();
    c.load(p + 0x7e4, id, 6, 0);
    c.put(p + 0x808, s.fb(1));
}

/// `sfx_wreck id`: resource bound at `P+0x884`.
///
/// exe: FUN_00445c10
pub fn sfx_wreck(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d` (0x60f93c).
    let s = scan(line, "%d");
    let p = c.p();
    c.load(p + 0x884, s.db(0).unwrap_or(0), 6, 0);
}

/// `sfx_horn id f n`: resource `id` bound at `P+0x834+(n&3)*0x10`, float `f`
/// stored at `P+0x874+(n&3)*4`.
///
/// exe: FUN_00445c80
pub fn sfx_horn(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f %d` (0x60f940).
    let s = scan(line, "%d %f %d");
    let id = s.db(0).unwrap_or(0);
    let p = c.p();
    // `(n & 0x80000003)`, sign-fixed like the exe's `idiv` remainder path:
    // negative values take `(n - 1 | !3) + 1`.
    let n = s.d(2).unwrap_or(1);
    let n = (if n < 0 { ((n - 1) | -4) + 1 } else { n } as u32) & 0x8000_0003;
    let slot = p.wrapping_add(0x834).wrapping_add(n.wrapping_mul(0x10));
    c.load(slot, id, 6, 0);
    c.put(
        p.wrapping_add(0x874).wrapping_add(n.wrapping_mul(4)),
        s.fb(1),
    );
}

/// `sfx_gearchange a b`: resources bound at `P+0x894` and `P+0x8a4`.
///
/// exe: FUN_00445d50
pub fn sfx_gearchange(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %d` (0x60f94c).
    let s = scan(line, "%d %d");
    let p = c.p();
    c.load(p + 0x894, s.db(0).unwrap_or(0), 6, 0);
    c.load(p + 0x8a4, s.db(1).unwrap_or(0), 6, 0);
}
