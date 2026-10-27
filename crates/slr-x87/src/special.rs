//! Exact special-operand results for the transcendentals.
//!
//! Zeros, infinities and NaNs are not approximations: the SDM tables fix the result, and the
//! pinned Bochs model and Rosetta 2 agree bit for bit on every such operand pair in the G11
//! corpus (`re/x87probe/g11`, `docs/research/x87-g11-decision.md`). Finite non-zero operands still
//! go through the `APPROX` paths in [`crate::trans`]. Results are for round-to-nearest, the
//! game's rounding control; the π constants are the 64-bit nearest values.

use crate::fscale::propagate_nan;
use crate::X87;

const ONE: u64 = 1 << 63;
/// The 64-bit significand of π, π/2 and π/4 (nearest) and of 3π/4 (exponent 0x4000).
const PI_SIG: u64 = 0xC90F_DAA2_2168_C235;
const THREE_PI_4_SIG: u64 = 0x96CB_E3F9_990E_91A8;

fn pack(sign: bool, exp: u16, sig: u64) -> X87 {
    let mut bytes = [0u8; 10];
    bytes[..8].copy_from_slice(&sig.to_le_bytes());
    bytes[8..].copy_from_slice(&(((sign as u16) << 15) | exp).to_le_bytes());
    X87::from_bytes(bytes)
}

/// The "real indefinite" QNaN an invalid operation returns.
fn indefinite() -> X87 {
    pack(true, 0x7FFF, 0xC000_0000_0000_0000)
}

fn inf(sign: bool) -> X87 {
    pack(sign, 0x7FFF, ONE)
}

fn zero(sign: bool) -> X87 {
    pack(sign, 0, 0)
}

fn quiet(a: X87) -> X87 {
    let (s, e) = a.sign_exp();
    pack(s, e, a.significand() | 0xC000_0000_0000_0000)
}

fn is_zero(a: X87) -> bool {
    a.is_zero()
}

/// `FSIN`/`FCOS` of a NaN, an infinity or a zero.
pub(crate) fn sin_cos(a: X87, cos: bool) -> Option<X87> {
    if a.is_nan() {
        return Some(quiet(a));
    }
    if a.is_inf() {
        return Some(indefinite());
    }
    if is_zero(a) {
        return Some(if cos { X87::from_f32(1.0) } else { a });
    }
    None
}

/// `F2XM1`: NaN and ±0. (`|x| > 1`, infinities included, is outside the SDM domain: Rosetta and
/// Bochs disagree on −∞, so no claim is made.)
pub(crate) fn f2xm1(a: X87) -> Option<X87> {
    if a.is_nan() {
        return Some(quiet(a));
    }
    if is_zero(a) {
        return Some(a);
    }
    None
}

/// `FPATAN` (`y` = ST(1), `x` = ST(0)) when either operand is a NaN, an infinity or a zero.
pub(crate) fn patan(y: X87, x: X87) -> Option<X87> {
    if x.is_nan() || y.is_nan() {
        return Some(propagate_nan(x, y));
    }
    let (ys, xs) = (y.sign(), x.sign());
    let pi = |exp| Some(pack(ys, exp, PI_SIG));
    if is_zero(y) {
        // ±0 with a positive x (including +∞) stays ±0; a negative x gives ±π
        return if xs { pi(0x4000) } else { Some(zero(ys)) };
    }
    if y.is_inf() {
        return if x.is_inf() {
            if xs {
                Some(pack(ys, 0x4000, THREE_PI_4_SIG))
            } else {
                pi(0x3FFE)
            }
        } else {
            pi(0x3FFF)
        };
    }
    if is_zero(x) {
        return pi(0x3FFF);
    }
    if x.is_inf() {
        return if xs { pi(0x4000) } else { Some(zero(ys)) };
    }
    None
}

/// `FYL2X` (`x` = ST(0), `y` = ST(1)) for NaN, zero and infinite operands, `x < 0`, and `x = 1`.
pub(crate) fn fyl2x(x: X87, y: X87) -> Option<X87> {
    if x.is_nan() || y.is_nan() {
        return Some(propagate_nan(x, y));
    }
    let x_neg_nonzero = x.sign() && !is_zero(x);
    if x_neg_nonzero {
        return Some(indefinite());
    }
    let ys = y.sign();
    if is_zero(x) {
        // log2(±0) = -∞: the product is -y·∞, invalid for y = 0
        return Some(if is_zero(y) { indefinite() } else { inf(!ys) });
    }
    let one = x.sign_exp() == (false, 0x3FFF) && x.significand() == ONE;
    if y.is_inf() {
        if x.is_inf() {
            return Some(inf(ys));
        }
        if one {
            return Some(indefinite());
        }
        let below_one = x.sign_exp().1 < 0x3FFF;
        return Some(inf(ys != below_one));
    }
    if x.is_inf() {
        return Some(if is_zero(y) { indefinite() } else { inf(ys) });
    }
    if one {
        return Some(zero(ys));
    }
    if is_zero(y) {
        // ±0 · log2(x): the sign flips when log2(x) < 0, i.e. x < 1 (denormals included)
        return Some(zero(ys != (x.sign_exp().1 < 0x3FFF)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(sign: bool, exp: u16, sig: u64) -> X87 {
        pack(sign, exp, sig)
    }

    #[test]
    fn sin_cos_of_zero_infinity_and_nan() {
        assert_eq!(sin_cos(zero(true), false), Some(zero(true)));
        assert_eq!(sin_cos(zero(true), true), Some(X87::from_f32(1.0)));
        assert_eq!(sin_cos(inf(false), false), Some(indefinite()));
        assert_eq!(sin_cos(inf(true), true), Some(indefinite()));
        // a signalling NaN is quieted with its payload kept
        let snan = v(false, 0x7FFF, 0xA000_0000_0000_0000);
        assert_eq!(
            sin_cos(snan, false),
            Some(v(false, 0x7FFF, 0xE000_0000_0000_0000))
        );
        assert_eq!(sin_cos(X87::from_f32(0.5), false), None);
    }

    #[test]
    fn patan_follows_the_sdm_table() {
        let pos = X87::from_f32(2.0);
        let neg = X87::from_f32(-2.0);
        assert_eq!(patan(zero(false), pos), Some(zero(false)));
        assert_eq!(patan(zero(true), pos), Some(zero(true)));
        assert_eq!(patan(zero(false), neg), Some(v(false, 0x4000, PI_SIG)));
        assert_eq!(patan(zero(true), zero(true)), Some(v(true, 0x4000, PI_SIG)));
        assert_eq!(patan(pos, zero(false)), Some(v(false, 0x3FFF, PI_SIG)));
        assert_eq!(patan(neg, zero(true)), Some(v(true, 0x3FFF, PI_SIG)));
        assert_eq!(patan(inf(false), pos), Some(v(false, 0x3FFF, PI_SIG)));
        assert_eq!(patan(inf(true), inf(false)), Some(v(true, 0x3FFE, PI_SIG)));
        assert_eq!(
            patan(inf(false), inf(true)),
            Some(v(false, 0x4000, THREE_PI_4_SIG))
        );
        assert_eq!(patan(pos, inf(false)), Some(zero(false)));
        assert_eq!(patan(neg, inf(true)), Some(v(true, 0x4000, PI_SIG)));
        assert_eq!(patan(pos, pos), None);
    }

    #[test]
    fn fyl2x_special_operands() {
        let two = X87::from_f32(2.0);
        let half = X87::from_f32(0.5);
        let one = X87::from_f32(1.0);
        assert_eq!(fyl2x(zero(false), two), Some(inf(true)));
        assert_eq!(fyl2x(zero(true), X87::from_f32(-2.0)), Some(inf(false)));
        assert_eq!(fyl2x(zero(false), zero(false)), Some(indefinite()));
        assert_eq!(fyl2x(X87::from_f32(-3.0), two), Some(indefinite()));
        assert_eq!(fyl2x(inf(true), two), Some(indefinite()));
        assert_eq!(fyl2x(inf(false), two), Some(inf(false)));
        assert_eq!(fyl2x(inf(false), X87::from_f32(-2.0)), Some(inf(true)));
        assert_eq!(fyl2x(two, inf(true)), Some(inf(true)));
        assert_eq!(fyl2x(half, inf(true)), Some(inf(false)));
        assert_eq!(fyl2x(one, inf(false)), Some(indefinite()));
        // log2(1) = +0, so the product takes the sign of y: -3 * +0 = -0
        assert_eq!(fyl2x(one, X87::from_f32(-3.0)), Some(zero(true)));
        assert_eq!(fyl2x(half, zero(false)), Some(zero(true)));
        assert_eq!(fyl2x(two, zero(false)), Some(zero(false)));
        assert_eq!(fyl2x(two, two), None);
    }

    #[test]
    fn f2xm1_keeps_zero_and_quiets_nan_but_claims_nothing_outside_the_domain() {
        assert_eq!(f2xm1(zero(true)), Some(zero(true)));
        assert_eq!(f2xm1(inf(true)), None);
        assert_eq!(f2xm1(X87::from_f32(0.25)), None);
    }
}
