//! Candidate standalone Intel FSIN/FCOS value model; NOT yet trusted for production.
//!
//! Rust adaptation of Bochs fsincos.cc and f128_67.cc at
//! d245c751e6a6fab4bf68e72823598a48001adb3c. Preserves the 66-bit
//! reduction constant, asymmetric multiplier and internal 67/64-bit rounding.
//! This API handles FLD m32 inputs at round-to-nearest with masked exceptions.
//! It does not implement status flags or arbitrary extF80 operands. See
//! docs/research/native-runtime-gaps.md for the unresolved oracle comparison.

/*============================================================================
This source file is an extension to the SoftFloat IEC/IEEE Floating-point
Arithmetic Package, Release 2b, written for Bochs (x86 achitecture simulator)
floating point emulation.

THIS SOFTWARE IS DISTRIBUTED AS IS, FOR FREE.  Although reasonable effort has
been made to avoid it, THIS SOFTWARE MAY CONTAIN FAULTS THAT WILL AT TIMES
RESULT IN INCORRECT BEHAVIOR.  USE OF THIS SOFTWARE IS RESTRICTED TO PERSONS
AND ORGANIZATIONS WHO CAN AND WILL TAKE FULL RESPONSIBILITY FOR ALL LOSSES,
COSTS, OR OTHER PROBLEMS THEY INCUR DUE TO THE SOFTWARE, AND WHO FURTHERMORE
EFFECTIVELY INDEMNIFY JOHN HAUSER AND THE INTERNATIONAL COMPUTER SCIENCE
INSTITUTE (possibly via similar legal warning) AGAINST ALL LOSSES, COSTS, OR
OTHER PROBLEMS INCURRED BY THEIR CUSTOMERS AND CLIENTS DUE TO THE SOFTWARE.

Derivative works are acceptable, even for commercial purposes, so long as
(1) the source code for the derivative work includes prominent notice that
the work is derivative, and (2) the source code includes prominent notice with
these four paragraphs for those parts of this code that are retained.
=============================================================================*/

use crate::{
    ffi::{self, Quad, SfMode, SfPrec},
    lock, X87,
};

const ONE: Quad = Quad {
    hi: 0x3fff000000000000,
    lo: 0,
};
const SIGN: u64 = 1 << 63;

fn sign(mut a: Quad, negative: bool) -> Quad {
    a.hi = (a.hi & !SIGN) | if negative { SIGN } else { 0 };
    a
}
fn negate(mut a: Quad) -> Quad {
    a.hi ^= SIGN;
    a
}
fn chop(mut a: Quad, precision: u32) -> Quad {
    a.lo &= !((1u64 << (113 - precision)) - 1);
    a
}
fn add67(a: Quad, b: Quad) -> Quad {
    ffi::set_rounding(SfMode::MinMag);
    chop(unsafe { ffi::f128_add(a, b) }, 67)
}
fn mul67(a: Quad, b: Quad) -> Quad {
    ffi::set_rounding(SfMode::MinMag);
    chop(unsafe { ffi::f128_mul(a, b) }, 67)
}
fn mul_e67(a: Quad, b: Quad) -> Quad {
    mul67(a, chop(b, 64))
}
fn round64(a: Quad) -> Quad {
    ffi::set_rounding(SfMode::NearEven);
    unsafe { ffi::extF80_to_f128(ffi::f128_to_extF80(a)) }
}
fn add64(a: Quad, b: Quad) -> Quad {
    ffi::set_rounding(SfMode::NearEven);
    round64(unsafe { ffi::f128_add(a, b) })
}
fn mul64(a: Quad, b: Quad) -> Quad {
    ffi::set_rounding(SfMode::NearEven);
    round64(unsafe { ffi::f128_mul(a, chop(b, 64)) })
}
fn finish(head: Quad, tail: Quad) -> X87 {
    ffi::set_rounding(SfMode::NearEven);
    let result = unsafe { ffi::f128_to_extF80(ffi::f128_add(head, tail)) };
    X87::from_bytes(result.to_bytes())
}

// All callers hold SF_LOCK throughout: SoftFloat global modes cannot leak
// between polynomial steps, and no public X87 arithmetic is called inside.
fn reduce(value: ffi::ExtF80) -> (Quad, bool, u32) {
    let exp = (value.sign_exp & 0x7fff) as i32 - 0x3fff;
    if exp < -1 {
        let mut positive = value;
        positive.sign_exp &= 0x7fff;
        return (unsafe { ffi::extF80_to_f128(positive) }, false, 0);
    }
    const PI2: u128 = 0xc90fdaa22168c234c000000000000000;
    let (mut remainder, mut quadrant) = if exp <= 0 {
        let x = ((value.signif as u128) << 64) >> -exp;
        if x >= PI2 {
            (x - PI2, 1u32)
        } else {
            (x, 0u32)
        }
    } else {
        let dividend = (value.signif as u128) << exp;
        let mut remainder = 0u128;
        let mut q = 0u32;
        for index in 0..192 {
            let bit = if index < 128 {
                (dividend >> (127 - index)) & 1
            } else {
                0
            };
            let carry = remainder >> 127;
            remainder = (remainder << 1) | bit;
            q = q.wrapping_shl(1);
            if carry != 0 || remainder >= PI2 {
                remainder = remainder.wrapping_sub(PI2);
                q |= 1;
            }
        }
        (remainder, q)
    };
    let negative = remainder > PI2 / 2;
    if negative {
        remainder = PI2 - remainder;
        quadrant = quadrant.wrapping_add(1);
    }
    let r = if remainder == 0 {
        Quad { hi: 0, lo: 0 }
    } else {
        // FLD m32 and the 66-bit divisor make this fixed point remainder
        // exactly representable in binary128: its low 62 bits are zero.
        debug_assert_eq!(remainder & ((1u128 << 62) - 1), 0);
        let shift = remainder.leading_zeros();
        let significand = (remainder << shift) >> 15;
        Quad {
            hi: ((0x3fff - shift as u64) << 48) | ((significand >> 64) as u64 & 0x0000ffffffffffff),
            lo: significand as u64,
        }
    };
    (r, negative, quadrant & 3)
}

fn polynomial(r: Quad, r_negative: bool, quadrant: u32, cosine: bool, input_negative: bool) -> X87 {
    let n = (quadrant + u32::from(cosine)) & 3;
    let cosine_lane = n & 1 != 0;
    let negative = (!cosine && input_negative) ^ (n & 2 != 0);
    if (r.hi >> 48) & 0x7fff >= 0x3ffd {
        let exponent = (r.hi >> 48) & 0x7fff;
        let index = (((exponent & 1) << 2) | ((r.hi >> 46) & 3)) as usize;
        let c = Quad {
            hi: (r.hi & !((1u64 << 45) - 1)) | (1 << 45),
            lo: 0,
        };
        let d = add67(r, negate(c));
        let e = mul67(d, d);
        let mut p = U[3];
        let mut q = V[3];
        for i in (0..3).rev() {
            p = add64(mul67(e, p), U[i]);
            q = add64(mul67(e, q), V[i]);
        }
        p = add64(d, mul67(mul67(e, p), d));
        q = mul64(e, q);
        if cosine_lane {
            let sigma = sign(SIN[index], negative);
            let gamma = sign(COS[index], negative);
            finish(gamma, add67(mul67(sigma, negate(p)), mul67(gamma, q)))
        } else {
            let sigma = sign(SIN[index], negative ^ r_negative);
            let gamma = sign(COS[index], negative ^ r_negative);
            finish(sigma, add67(mul67(sigma, q), mul67(gamma, p)))
        }
    } else {
        let s = mul_e67(r, r);
        let t = mul_e67(s, s);
        let coefficients = if cosine_lane { B } else { A };
        let mut p = mul67(t, coefficients[4]);
        p = add64(p, coefficients[2]);
        p = mul67(t, p);
        p = add64(p, coefficients[0]);
        p = mul67(s, p);
        let mut q = mul67(t, coefficients[5]);
        q = add64(q, coefficients[3]);
        q = mul67(t, q);
        q = add64(q, coefficients[1]);
        q = mul67(t, q);
        if cosine_lane {
            let f = add67(p, q);
            finish(sign(ONE, negative), if negative { negate(f) } else { f })
        } else {
            let rr = sign(r, negative ^ r_negative);
            finish(rr, mul67(rr, add64(p, q)))
        }
    }
}

/// Candidate *separate* FSIN and FCOS results, plus the out-of-range C2 bit.
/// Finite f32 operands only. This value model has no exception/status contract.
pub fn candidate_f32(bits: u32) -> Result<(X87, X87, bool), &'static str> {
    let abs = bits & 0x7fff_ffff;
    if abs >= 0x7f80_0000 {
        return Err("candidate Intel trig: nonfinite input is not validated");
    }
    let input = X87::from_f32(f32::from_bits(bits));
    let one = X87::from_i32(1);
    if abs >= 0x5f00_0000 {
        return Ok((input, input, true));
    }
    if abs <= 0x1d00_0000 {
        return Ok((input, one, false));
    }
    let _guard = lock();
    ffi::set_precision(SfPrec::P80);
    ffi::set_rounding(SfMode::NearEven);
    let (r, negative, quadrant) = reduce(ffi::ExtF80::from_bytes(input.to_bytes()));
    Ok((
        polynomial(r, negative, quadrant, false, bits & 0x8000_0000 != 0),
        polynomial(r, negative, quadrant, true, false),
        false,
    ))
}

const A: [Quad; 6] = [
    Quad {
        hi: 0xBFFC555555555555,
        lo: 0x5555400000000000,
    },
    Quad {
        hi: 0x3FF8111111111111,
        lo: 0x0F8D400000000000,
    },
    Quad {
        hi: 0xBFF2A01A01A019DC,
        lo: 0xF1DD000000000000,
    },
    Quad {
        hi: 0x3FEC71DE3A53D425,
        lo: 0x5B5C000000000000,
    },
    Quad {
        hi: 0xBFE5AE64448BBE81,
        lo: 0x4CBC000000000000,
    },
    Quad {
        hi: 0x3FDE60C0FC098535,
        lo: 0x7936000000000000,
    },
];

const B: [Quad; 6] = [
    Quad {
        hi: 0xBFFDFFFFFFFFFFFF,
        lo: 0xFFFF800000000000,
    },
    Quad {
        hi: 0x3FFA555555555555,
        lo: 0x509DC00000000000,
    },
    Quad {
        hi: 0xBFF56C16C16C1686,
        lo: 0x286E800000000000,
    },
    Quad {
        hi: 0x3FEFA01A019D6D56,
        lo: 0x7CB3C00000000000,
    },
    Quad {
        hi: 0xBFE927E4EBD87D4D,
        lo: 0x24C0000000000000,
    },
    Quad {
        hi: 0x3FE21E93C920D453,
        lo: 0x06BE000000000000,
    },
];

const U: [Quad; 4] = [
    Quad {
        hi: 0xBFFC555555555555,
        lo: 0x5511400000000000,
    },
    Quad {
        hi: 0x3FF81111111110E8,
        lo: 0xFF6D800000000000,
    },
    Quad {
        hi: 0xBFF2A01A0182C811,
        lo: 0x3A6B800000000000,
    },
    Quad {
        hi: 0x3FEC71D5C5B983CC,
        lo: 0x8490000000000000,
    },
];

const V: [Quad; 4] = [
    Quad {
        hi: 0xBFFDFFFFFFFFFFFF,
        lo: 0xFE8A000000000000,
    },
    Quad {
        hi: 0x3FFA5555555554E7,
        lo: 0x3EB9800000000000,
    },
    Quad {
        hi: 0xBFF56C16C143CC6C,
        lo: 0xB9C4C00000000000,
    },
    Quad {
        hi: 0x3FEFA00E62355B8E,
        lo: 0xFFC4000000000000,
    },
];

const SIN: [Quad; 8] = [
    Quad {
        hi: 0x3FFE110D0C4B69C3,
        lo: 0xB764800000000000,
    },
    Quad {
        hi: 0x3FFE44EB381CF386,
        lo: 0xAB04C00000000000,
    },
    Quad {
        hi: 0x3FFE73B7680DEA57,
        lo: 0x7DBB800000000000,
    },
    Quad {
        hi: 0x3FFE9CB6A9BBCE64,
        lo: 0xAD61800000000000,
    },
    Quad {
        hi: 0x3FFD1C37D64C6B87,
        lo: 0x6518000000000000,
    },
    Quad {
        hi: 0x3FFD591BC9FA2F59,
        lo: 0x72F9000000000000,
    },
    Quad {
        hi: 0x3FFD94A6BE9F546C,
        lo: 0x4A58C00000000000,
    },
    Quad {
        hi: 0x3FFDCE9D2E3D4A51,
        lo: 0xEDA0800000000000,
    },
];

const COS: [Quad; 8] = [
    Quad {
        hi: 0x3FFEB11D04162A4C,
        lo: 0x623BC00000000000,
    },
    Quad {
        hi: 0x3FFE8BB105A5DC90,
        lo: 0x0619000000000000,
    },
    Quad {
        hi: 0x3FFE6018526F563D,
        lo: 0xF51B400000000000,
    },
    Quad {
        hi: 0x3FFE2F011326420E,
        lo: 0x431C800000000000,
    },
    Quad {
        hi: 0x3FFEEBE214F76EFA,
        lo: 0x7BF4000000000000,
    },
    Quad {
        hi: 0x3FFEE20BF49ACD6C,
        lo: 0x0F4D000000000000,
    },
    Quad {
        hi: 0x3FFED653F073E403,
        lo: 0xFA27800000000000,
    },
    Quad {
        hi: 0x3FFEC8C5BF8CE1A8,
        lo: 0x4356800000000000,
    },
];
