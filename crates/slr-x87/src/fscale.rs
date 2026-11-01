//! `FSCALE`: exact exponent scaling, `ST0 · 2^trunc(ST1)`.
//!
//! No transcendental is involved: the result is the operand's significand with the exponent
//! moved by the truncated scale, rounded once under RC at the full 64-bit significand (the
//! control word's PC does not apply; both the Rosetta and Bochs legs return identical bytes at
//! PC24/53/64). Operand classes follow the SDM table for `FSCALE`; unnormal/pseudo-denormal
//! operands are not supported (no game path produces them), matching the rest of the crate.
//!
//! Evidence: `re/x87probe/g11` stream corpus, all four RC cells, Bochs and Rosetta legs agree
//! bit for bit on 120,552 vectors and this implementation equals both.

use crate::{Rounding, X87};

const QUIET: u64 = 0xC000_0000_0000_0000;

fn pack(sign: bool, exp: u16, sig: u64) -> X87 {
    let mut bytes = [0u8; 10];
    bytes[..8].copy_from_slice(&sig.to_le_bytes());
    bytes[8..].copy_from_slice(&(((sign as u16) << 15) | (exp & 0x7FFF)).to_le_bytes());
    X87::from_bytes(bytes)
}

fn is_zero_class(exp: u16, sig: u64) -> bool {
    exp == 0 && sig == 0
}

/// x87 NaN propagation for two operands, at least one of which is a NaN.
pub(crate) fn propagate_nan(a: X87, b: X87) -> X87 {
    let (sa, ea) = a.sign_exp();
    let (sb, eb) = b.sign_exp();
    let (ga, gb) = (a.significand(), b.significand());
    let take_b = if a.is_signaling_nan() || b.is_signaling_nan() {
        if a.is_signaling_nan() {
            if b.is_signaling_nan() {
                larger_mag(ea, ga, sa, eb, gb, sb)
            } else {
                b.is_nan()
            }
        } else {
            !a.is_nan()
        }
    } else {
        larger_mag(ea, ga, sa, eb, gb, sb)
    };
    let (s, e, g) = if take_b { (sb, eb, gb) } else { (sa, ea, ga) };
    pack(s, e, g | QUIET)
}

/// True when B wins the "larger magnitude" tie-break of SoftFloat's 8086 NaN rule.
fn larger_mag(ea: u16, ga: u64, sa: bool, eb: u16, gb: u64, sb: bool) -> bool {
    if ea != eb {
        return ea < eb;
    }
    if ga != gb {
        return ga < gb;
    }
    // equal magnitude: A wins only when its sign word is the smaller one (positive A)
    !(((sa as u16) << 15 | ea) < ((sb as u16) << 15 | eb))
}

impl X87 {
    /// `FSCALE`: `self · 2^trunc(scale)`, rounded under `rc` (see the module notes).
    pub fn fscale(self, scale: X87, rc: Rounding) -> X87 {
        let (sign_a, exp_a_raw) = self.sign_exp();
        let mut exp_a = exp_a_raw as i32;
        let mut sig_a = self.significand();
        let (sign_b, exp_b_raw) = scale.sign_exp();
        let mut exp_b = exp_b_raw as i32;
        let mut sig_b = scale.significand();
        if self.is_nan() || scale.is_nan() {
            return propagate_nan(self, scale);
        }
        if exp_a == 0x7FFF {
            // infinity: -inf scale is invalid (default NaN), otherwise unchanged
            return if exp_b == 0x7FFF && sign_b {
                default_nan()
            } else {
                self
            };
        }
        if exp_b == 0x7FFF {
            if is_zero_class(exp_a as u16, sig_a) {
                return if sign_b { self } else { default_nan() };
            }
            return if sign_b {
                pack(sign_a, 0, 0)
            } else {
                pack(sign_a, 0x7FFF, 1 << 63)
            };
        }
        if exp_a == 0 {
            if sig_a == 0 {
                return self;
            }
            // denormal operand: normalise (biased exponent 1 scale)
            let lz = sig_a.leading_zeros() as i32;
            sig_a <<= lz;
            exp_a = 1 - lz;
            if exp_b < 0x3FFF {
                return round_pack(sign_a, exp_a, sig_a, rc);
            }
        }
        if exp_b == 0 {
            if sig_b == 0 {
                return self;
            }
            let lz = sig_b.leading_zeros() as i32;
            sig_b <<= lz;
            exp_b = 1 - lz;
        }
        if exp_b > 0x400E {
            let scale_exp: i32 = if sign_b { -0x10000 } else { 0x10000 };
            return round_pack(sign_a, exp_a + scale_exp, sig_a, rc);
        }
        if exp_b < 0x3FFF {
            return self;
        }
        let shift = 0x403E - exp_b;
        let magnitude = (sig_b >> shift) as i32;
        let delta = if sign_b { -magnitude } else { magnitude };
        round_pack(sign_a, exp_a + delta, sig_a, rc)
    }
}

fn default_nan() -> X87 {
    pack(true, 0x7FFF, QUIET)
}

/// Round and pack a normalised significand at biased exponent `exp` (masked exceptions, 64-bit
/// significand): overflow to infinity or the largest finite per RC, gradual underflow rounded once.
fn round_pack(sign: bool, exp: i32, sig: u64, rc: Rounding) -> X87 {
    let toward_inf = match rc {
        Rounding::NearestEven => true,
        Rounding::Zero => false,
        Rounding::Up => !sign,
        Rounding::Down => sign,
    };
    if exp >= 0x7FFF {
        return if toward_inf {
            pack(sign, 0x7FFF, 1 << 63)
        } else {
            pack(sign, 0x7FFE, u64::MAX)
        };
    }
    if exp >= 1 {
        return pack(sign, exp as u16, sig);
    }
    // denormal result: shift right by (1 - exp), keeping a sticky bit, round once
    let shift = (1 - exp) as u32;
    let (kept, round_bit, sticky) = if shift >= 65 {
        (0u64, false, sig != 0)
    } else if shift == 64 {
        (0, sig >> 63 == 1, sig << 1 != 0)
    } else {
        let kept = sig >> shift;
        let rest = sig << (64 - shift);
        (kept, rest >> 63 == 1, rest << 1 != 0)
    };
    let inexact = round_bit || sticky;
    let up = match rc {
        Rounding::NearestEven => round_bit && (sticky || kept & 1 == 1),
        Rounding::Zero => false,
        Rounding::Up => !sign && inexact,
        Rounding::Down => sign && inexact,
    };
    let rounded = kept + up as u64;
    // a carry into the integer bit makes the smallest normal
    pack(sign, (rounded >> 63) as u16, rounded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(sign: bool, exp: u16, sig: u64) -> X87 {
        pack(sign, exp, sig)
    }

    #[test]
    fn scales_exactly_by_truncated_power_of_two() {
        let one = X87::from_f64(1.0);
        assert_eq!(
            one.fscale(X87::from_f64(3.9), Rounding::NearestEven),
            X87::from_f64(8.0)
        );
        assert_eq!(
            one.fscale(X87::from_f64(-3.9), Rounding::NearestEven),
            X87::from_f64(0.125)
        );
        // |scale| < 1 truncates to zero: operand returned unchanged
        assert_eq!(one.fscale(X87::from_f64(0.75), Rounding::Zero), one);
        assert_eq!(
            X87::from_f64(-0.0).fscale(X87::from_f64(5.0), Rounding::Up),
            X87::from_f64(-0.0)
        );
    }

    #[test]
    fn overflow_follows_rounding_control() {
        let big = x(false, 0x7FFE, u64::MAX);
        let two = X87::from_f64(1.0);
        assert!(big.fscale(two, Rounding::NearestEven).is_inf());
        assert_eq!(big.fscale(two, Rounding::Zero), big);
        let neg = x(true, 0x7FFE, u64::MAX);
        assert_eq!(neg.fscale(two, Rounding::Up), neg);
        assert!(neg.fscale(two, Rounding::Down).is_inf());
    }

    #[test]
    fn underflow_rounds_once_into_the_denormal_range() {
        // 1.5 * 2^-16383 sits in the denormal range; 0b11 << 62 shifted right by 1 is exact
        let a = x(false, 1, 0xC000_0000_0000_0000);
        let r = a.fscale(X87::from_f64(-1.0), Rounding::NearestEven);
        assert_eq!(r, x(false, 0, 0x6000_0000_0000_0000));
        // a tie rounds to even, other modes round directionally
        let tie =
            x(false, 1, 0x8000_0000_0000_0003).fscale(X87::from_f64(-1.0), Rounding::NearestEven);
        assert_eq!(tie, x(false, 0, 0x4000_0000_0000_0002));
        let up = x(false, 1, 0x8000_0000_0000_0003).fscale(X87::from_f64(-1.0), Rounding::Up);
        assert_eq!(up, x(false, 0, 0x4000_0000_0000_0002));
        let down = x(false, 1, 0x8000_0000_0000_0003).fscale(X87::from_f64(-1.0), Rounding::Down);
        assert_eq!(down, x(false, 0, 0x4000_0000_0000_0001));
        // far below the smallest denormal: zero (nearest), smallest denormal (round up)
        let tiny = X87::from_f64(1.0).fscale(
            x(true, 0x400F, 0x8000_0000_0000_0000),
            Rounding::NearestEven,
        );
        assert_eq!(tiny, x(false, 0, 0));
        let tiny_up =
            X87::from_f64(1.0).fscale(x(true, 0x400F, 0x8000_0000_0000_0000), Rounding::Up);
        assert_eq!(tiny_up, x(false, 0, 1));
    }

    #[test]
    fn special_operand_classes() {
        let inf = x(false, 0x7FFF, 1 << 63);
        let neg_inf = x(true, 0x7FFF, 1 << 63);
        let zero = x(false, 0, 0);
        let two = X87::from_f64(2.0);
        assert_eq!(two.fscale(inf, Rounding::NearestEven), inf);
        assert_eq!(two.fscale(neg_inf, Rounding::NearestEven), zero);
        assert_eq!(inf.fscale(two, Rounding::NearestEven), inf);
        // zero scaled by +inf and inf scaled by -inf are invalid (default NaN)
        assert!(zero.fscale(inf, Rounding::NearestEven).is_nan());
        assert!(inf.fscale(neg_inf, Rounding::NearestEven).is_nan());
        assert_eq!(zero.fscale(neg_inf, Rounding::NearestEven), zero);
        // a signalling NaN is quieted and propagated
        let snan = x(false, 0x7FFF, 0xA000_0000_0000_0000);
        assert_eq!(
            two.fscale(snan, Rounding::Zero),
            x(false, 0x7FFF, 0xE000_0000_0000_0000)
        );
    }
}
