//! Vehicle physics leaf functions: the engine torque curve lookup, the Pacejka tyre force and the
//! tyre slip blend (docs/physics.md sections 8-9).
//!
//! The exe computes these on the x87 stack at precision control 24-bit, round to nearest (ADR 0007,
//! `0x007F`), so every function here evaluates in the original's instruction order through
//! [`X87`] at that control word and keeps each intermediate where the exe keeps it: in a register
//! (a 24-bit significand with the full exponent range) or stored to memory as an `f32`
//! (`fst`/`fstp m32`, which rounds the exponent range too). Results the exe returns in `ST0` are
//! returned as [`X87`]; results it writes through pointers are returned as `u32` words.
//!
//! The Pacejka force reaches `f2xm1`, `fpatan` and `fsin`: those are pinned as `APPROX` until G11
//! (docs/research/x87-g11-decision.md), so [`pacejka_force`] is level-1 evidence only for the
//! operations around them and is reported `blocked_by_g11` by `acceptance/g4.py`.

use slr_x87::trans::{approx_f2xm1, approx_patan, approx_sin};
use slr_x87::{Precision, Rounding, X87};

const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;

/// `FLDL2E` at round-to-nearest: log2(e) rounded to 64 significant bits.
const L2E: [u8; 10] = [0xBC, 0xF0, 0x17, 0x5C, 0x29, 0x3B, 0xAA, 0xB8, 0xFF, 0x3F];

fn ld(word: u32) -> X87 {
    X87::from_f32(f32::from_bits(word))
}

/// `fst`/`fstp m32`: round to f32 (significand and exponent range).
fn st32(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}

fn add(a: X87, b: X87) -> X87 {
    a.add(b, PC, RC)
}

fn sub(a: X87, b: X87) -> X87 {
    a.sub(b, PC, RC)
}

fn mul(a: X87, b: X87) -> X87 {
    a.mul(b, PC, RC)
}

fn div(a: X87, b: X87) -> X87 {
    a.div(b, PC, RC)
}

fn zero() -> X87 {
    X87::from_f32(0.0)
}

fn one() -> X87 {
    X87::from_f32(1.0)
}

/// `FABS`.
fn fabs(a: X87) -> X87 {
    let mut bytes = a.to_bytes();
    bytes[9] &= 0x7F;
    X87::from_bytes(bytes)
}

/// `FCOM`: condition codes as `(c0, c3)`: C0 = less (or unordered), C3 = equal (or unordered).
fn cc(a: X87, b: X87) -> (bool, bool) {
    let (codes, _invalid) = a.fcom(b);
    (codes.c0, codes.c3)
}

/// Piecewise-linear lookup in a `{count, pairs}` torque curve (`FUN_0043be70`, `cdecl`, result in
/// `ST0`). `pairs[i] = [rpm, torque]` as `f32` words; the exe always reads `pairs[0]`, so a curve
/// holds at least one pair. Below the first knot, at or beyond the last knot, and for a
/// single-knot curve the result is `+0.0`.
///
/// exe: FUN_0043be70
pub fn torque_curve(x: u32, pairs: &[[u32; 2]], count: i32) -> X87 {
    curve_lookup(x, pairs, count)
}

/// The same lookup over a curve embedded in an object (`FUN_0043bee0`: count at `+8`, pairs
/// pointer at `+0xc`); the arithmetic is identical.
///
/// exe: FUN_0043bee0
pub fn torque_curve_obj(x: u32, pairs: &[[u32; 2]], count: i32) -> X87 {
    curve_lookup(x, pairs, count)
}

fn curve_lookup(x: u32, pairs: &[[u32; 2]], count: i32) -> X87 {
    if cc(ld(x), ld(pairs[0][0])).0 {
        return zero();
    }
    let mut i = 1usize;
    if count <= 1 {
        return zero();
    }
    loop {
        if cc(ld(x), ld(pairs[i][0])).0 {
            break;
        }
        i += 1;
        if i as i32 >= count {
            return zero();
        }
    }
    let rise = sub(ld(pairs[i][1]), ld(pairs[i - 1][1]));
    let run_x = sub(ld(x), ld(pairs[i - 1][0]));
    let product = mul(rise, run_x);
    let span = sub(ld(pairs[i][0]), ld(pairs[i - 1][0]));
    add(div(product, span), ld(pairs[i - 1][1]))
}

/// Pacejka magic formula, `coef` = the eleven `f32` words at the start of the wheel's coefficient
/// block, `slip` and `fz` (vertical load) as `f32` words. `clamped` selects `FUN_0043ba80`, which
/// clamps a negative force to `+0.0`; the other (`FUN_0043bb30`) does not. Result in `ST0`.
///
/// exe: FUN_0043ba80
/// exe: FUN_0043bb30
pub fn pacejka_force(coef: &[u32; 11], slip: u32, fz: u32, clamped: bool) -> X87 {
    let p = |i: usize| ld(coef[i]);
    let fz = ld(fz);
    let d = add(mul(fz, p(1)), p(2));
    let q = mul(d, p(0));
    if cc(q, zero()).1 {
        return zero();
    }
    // exp(-fz*p5) = 2^(x), x = -fz*p5*log2(e), built as 2^round(x) * (1 + (2^(x-round(x)) - 1))
    let x = mul(X87::from_bytes(L2E), fchs(mul(fz, p(5))));
    let r = x.frndint(RC);
    let frac = sub(x, r);
    let e = approx_f2xm1(frac).value;
    let s = add(one(), e).fscale(r, RC);
    let a = add(mul(fz, p(3)), p(4));
    let b = div(mul(a, s), q);
    let shifted = add(add(mul(fz, p(9)), p(10)), ld(slip));
    let xx = mul(shifted, b);
    let a1 = approx_patan(xx, one()).value;
    let diff = sub(a1, xx);
    let curv = add(add(mul(mul(fz, fz), p(6)), mul(fz, p(7))), p(8));
    let inner = add(mul(curv, diff), xx);
    let a2 = approx_patan(inner, one()).value;
    let angle = mul(a2, p(0));
    let sine = fsin_value(angle);
    let force = mul(mul(d, fz), sine);
    if clamped && cc(force, zero()).0 {
        return zero();
    }
    force
}

// FSIN leaves finite inputs with |x| >= 2^63 unchanged and sets C2.
fn fsin_value(angle: X87) -> X87 {
    if (0x403e..0x7fff).contains(&angle.sign_exp().1) {
        angle
    } else {
        approx_sin(angle).value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsin_preserves_out_of_range_input() {
        let angle = X87::from_f64(2.0_f64.powi(63));
        assert_eq!(fsin_value(angle), angle);
        assert_ne!(fsin_value(X87::from_f64(1.0)), X87::from_f64(1.0));
    }
}

/// `FCHS`.
fn fchs(a: X87) -> X87 {
    let mut bytes = a.to_bytes();
    bytes[9] ^= 0x80;
    X87::from_bytes(bytes)
}

/// Runtime tuning words the slip blend reads (`_DAT_0060e02c..0x60e048`, set from the game config
/// at start-up) plus its two fixed constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlipConsts {
    /// `0x60e02c`: clamp of the longitudinal relaxed slip.
    pub max_long: u32,
    /// `0x60e030`: clamp of the lateral relaxed slip.
    pub max_lat: u32,
    /// `0x60e034`: longitudinal relaxation weight.
    pub long_weight: u32,
    /// `0x60e038`: lateral relaxation weight.
    pub lat_weight: u32,
    /// `0x60e03c`: blend gain.
    pub blend_gain: u32,
    /// `0x60e040`: low-speed threshold.
    pub threshold: u32,
    /// `0x60e044`: longitudinal relaxation length factor.
    pub long_len: u32,
    /// `0x60e048`: lateral relaxation length factor.
    pub lat_len: u32,
}

/// Tyre slip with relaxation length and low-speed blending (`FUN_0043bbd0`, `cdecl`). `out_long`
/// and `out_lat` are the words at `*param_5` / `*param_6` on entry; the returned pair is what the
/// exe leaves there.
///
/// exe: FUN_0043bbd0
pub fn slip(args: [u32; 4], consts: &SlipConsts, out_long: u32, out_lat: u32) -> (u32, u32) {
    let [p1, p2, p3, p4] = args;
    let k = |w: u32| ld(w);
    let speed = fabs(ld(p2));
    let mut angle_word = st32(speed);
    let old_long = out_long;
    let old_lat = out_lat;
    let t = mul(sub(speed, k(consts.threshold)), k(consts.blend_gain));
    // blend = clamp(t, 0, 1), keeping NaN
    let (c0, c3) = cc(zero(), t);
    let blend_word = if !(c0 || c3) {
        0
    } else {
        let below_one = cc(t, one()).0;
        if !below_one {
            1.0f32.to_bits()
        } else {
            let (c0, c3) = cc(zero(), t);
            if c0 || c3 {
                st32(t)
            } else {
                0
            }
        }
    };
    // `blend` is re-read from memory below
    let s1 = if cc(k(angle_word), k(consts.threshold)).0 || cc(k(angle_word), k(consts.threshold)).1
    {
        angle_word = 0;
        zero()
    } else {
        let speed_mem = k(angle_word);
        let s = div(sub(ld(p1), ld(p2)), speed_mem);
        let ratio = div(ld(p3), speed_mem);
        angle_word = st32(approx_patan(ratio, one()).value);
        s
    };
    if !cc(k(blend_word), one()).0 {
        return (st32(s1), angle_word);
    }
    let m = st32(mul(ld(p4), X87::from_f32(50.0)));
    let num1 = add(
        mul(sub(ld(p1), ld(p2)), ld(p4)),
        mul(k(consts.long_weight), k(old_long)),
    );
    let den1 = mul(
        add(mul(k(m), k(consts.long_len)), one()),
        k(consts.long_weight),
    );
    let mut f3 = div(num1, den1);
    let abs3 = fabs(f3);
    let (c0, c3) = cc(abs3, k(consts.max_long));
    if !(c0 || c3) {
        f3 = mul(f3, div(k(consts.max_long), abs3));
    }
    let num2 = add(mul(k(consts.lat_weight), k(old_lat)), mul(ld(p3), ld(p4)));
    let den2 = mul(
        add(mul(k(m), k(consts.lat_len)), one()),
        k(consts.lat_weight),
    );
    let f5 = div(num2, den2);
    let mut f5_word = st32(f5);
    let abs5 = fabs(f5);
    let (c0, c3) = cc(abs5, k(consts.max_lat));
    if !(c0 || c3) {
        f5_word = st32(mul(div(k(consts.max_lat), abs5), k(f5_word)));
    }
    let omb = sub(one(), k(blend_word));
    let omb_word = st32(omb);
    let r1 = add(mul(omb, f3), mul(s1, k(blend_word)));
    let r2 = add(
        mul(k(omb_word), k(f5_word)),
        mul(k(angle_word), k(blend_word)),
    );
    (st32(r1), st32(r2))
}
