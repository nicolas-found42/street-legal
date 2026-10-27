//! Data-free unit tests for the x87 value type (issue #15 / M0-14).
//!
//! These run on any machine: they need no stock install, no reference
//! install and no `re/out` vectors. Expectations come from three sources
//! that are independent of the `X87` wrapper under test:
//!
//! 1. **Raw SoftFloat** (`reference` below): the vendored Berkeley SoftFloat
//!    entry points are declared again here with their own `extern "C"` block
//!    and 80-bit struct, and driven with the SoftFloat enum codes written out
//!    as literals (`softfloat.h`: near_even=0, minMag=1, min=2, max=3;
//!    precision 32/64/80). Neither `ffi.rs` nor any `X87` method is used to
//!    compute an expectation, so a wrong RC/PC mapping in the wrapper cannot
//!    cancel itself out.
//! 2. **Host IEEE arithmetic**: at PC=24/53 and round-to-nearest the x87
//!    result equals native `f32`/`f64` arithmetic; `f64 % f64` (fmod) is an
//!    exact truncated-quotient remainder, i.e. FPREM; `round_ties_even`,
//!    `floor`, `ceil`, `trunc` give the four FISTP/FRNDINT modes.
//! 3. **Hand-derived values** (Intel SDM Vol 1 §8.1.5.2, §4.8.3, Table 8-x
//!    compare codes, §8.5 integer indefinite) written as literal bit
//!    patterns, plus `mpmath`-pinned transcendental literals.
//!
//! The probe-corpus test (`tests/probe_trusted.rs`) additionally checks the
//! Unicorn/Rosetta legs, but only where the git-ignored corpus exists; this
//! module is what CI runs.

use crate::trans::{self, tolerance};
use crate::{Arithmetic, ConditionCodes, Precision, Rounding, SfFlags, X87};
use std::sync::{Mutex, MutexGuard};

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

/// Serialises every test in this module: `X87::last_flags` reads global
/// SoftFloat state, so flag assertions must not interleave with other ops.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const PCS: [Precision; 3] = [Precision::F32, Precision::F64, Precision::Ext];
const RCS: [Rounding; 4] = [
    Rounding::NearestEven,
    Rounding::Down,
    Rounding::Up,
    Rounding::Zero,
];

/// Collects mismatches so one run reports every failing vector.
#[derive(Default)]
struct Failures {
    total: usize,
    shown: Vec<String>,
}

impl Failures {
    fn check(&mut self, ok: bool, msg: impl FnOnce() -> String) {
        self.total += 1;
        if !ok && self.shown.len() < 25 {
            self.shown.push(msg());
        }
    }
    fn finish(self, what: &str) {
        assert!(
            self.shown.is_empty(),
            "{what}: {} of {} vectors diverged; first {}:\n{}",
            self.shown.len(),
            self.total,
            self.shown.len(),
            self.shown.join("\n")
        );
        assert!(self.total > 0, "{what}: no vectors ran");
        eprintln!("{what}: {} vectors OK", self.total);
    }
}

/// Deterministic xorshift64* generator (no data files, no external crates).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Raw 80-bit image from its parts (sign, biased exponent, significand).
fn x(sign: bool, exp: u16, sig: u64) -> X87 {
    let mut b = [0u8; 10];
    b[0..8].copy_from_slice(&sig.to_le_bytes());
    b[8..10].copy_from_slice(&(((sign as u16) << 15) | exp).to_le_bytes());
    X87::from_bytes(b)
}

const INT_BIT: u64 = 1 << 63;

/// 1 + 2^-n (exact, n <= 63).
fn one_plus(n: u32) -> X87 {
    x(false, 0x3FFF, INT_BIT | (1 << (63 - n)))
}
/// 2^-n as a normal extended value.
fn pow2m(n: u32) -> X87 {
    x(false, 0x3FFF - n as u16, INT_BIT)
}
const ONE: X87 = X87 {
    bits: [0, 0, 0, 0, 0, 0, 0, 0x80, 0xFF, 0x3F],
};
const TWO: X87 = X87 {
    bits: [0, 0, 0, 0, 0, 0, 0, 0x80, 0x00, 0x40],
};

fn num(v: f64) -> X87 {
    reference::from_f64(v)
}

// ---------------------------------------------------------------------------
// Independent SoftFloat reference (raw FFI, no ffi.rs, no X87 methods)
// ---------------------------------------------------------------------------

mod reference {
    use super::{Precision, Rounding, X87};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct R80 {
        signif: u64,
        sign_exp: u16,
    }

    unsafe extern "C" {
        // Global-state setters from the crate's C shim (plain stores into the
        // SoftFloat globals; no arithmetic).
        fn slr_sf_set_rounding(mode: u8);
        fn slr_sf_set_precision(prec: u8);
        fn slr_sf_set_tininess(t: u8);
        fn slr_sf_clear_flags();

        fn f32_to_extF80(a: u32) -> R80;
        fn f64_to_extF80(a: u64) -> R80;
        fn i32_to_extF80(a: i32) -> R80;
        fn i64_to_extF80(a: i64) -> R80;
        fn extF80_to_f32(a: R80) -> u32;
        fn extF80_to_f64(a: R80) -> u64;
        fn extF80_to_i32(a: R80, mode: u8, exact: bool) -> i32;
        fn extF80_to_i64(a: R80, mode: u8, exact: bool) -> i64;
        fn extF80_add(a: R80, b: R80) -> R80;
        fn extF80_sub(a: R80, b: R80) -> R80;
        fn extF80_mul(a: R80, b: R80) -> R80;
        fn extF80_div(a: R80, b: R80) -> R80;
        fn extF80_sqrt(a: R80) -> R80;
        fn extF80_rem(a: R80, b: R80) -> R80;
        fn extF80_roundToInt(a: R80, mode: u8, exact: bool) -> R80;
        fn extF80_eq(a: R80, b: R80) -> bool;
        fn extF80_lt(a: R80, b: R80) -> bool;
    }

    // softfloat.h: softfloat_round_near_even=0, _minMag=1, _min=2, _max=3.
    // x87 RC: 0 nearest, 1 down, 2 up, 3 zero.
    fn sf_mode(rc: Rounding) -> u8 {
        match rc {
            Rounding::NearestEven => 0,
            Rounding::Down => 2,
            Rounding::Up => 3,
            Rounding::Zero => 1,
        }
    }

    // extF80_roundingPrecision: 32 / 64 / 80 select 24 / 53 / 64-bit sigs.
    fn sf_prec(pc: Precision) -> u8 {
        match pc {
            Precision::F32 => 32,
            Precision::F64 => 64,
            Precision::Ext => 80,
        }
    }

    fn r(v: X87) -> R80 {
        let b = v.to_bytes();
        R80 {
            signif: u64::from_le_bytes(b[0..8].try_into().unwrap()),
            sign_exp: u16::from_le_bytes(b[8..10].try_into().unwrap()),
        }
    }

    fn w(v: R80) -> X87 {
        let mut b = [0u8; 10];
        b[0..8].copy_from_slice(&v.signif.to_le_bytes());
        b[8..10].copy_from_slice(&v.sign_exp.to_le_bytes());
        X87::from_bytes(b)
    }

    /// Run `f` with the SoftFloat globals programmed, holding the crate-wide
    /// SoftFloat lock (the same one `X87` ops take; it is released before
    /// the caller invokes the wrapper, so there is no re-entrancy).
    fn with<T>(pc: Precision, rc: Rounding, f: impl FnOnce() -> T) -> T {
        let _g = crate::lock();
        unsafe {
            slr_sf_set_rounding(sf_mode(rc));
            slr_sf_set_precision(sf_prec(pc));
            slr_sf_set_tininess(0); // softfloat_tininess_beforeRounding
            slr_sf_clear_flags();
        }
        f()
    }

    #[derive(Clone, Copy, Debug)]
    pub enum Op {
        Add,
        Sub,
        Mul,
        Div,
    }

    pub fn bin(op: Op, a: X87, b: X87, pc: Precision, rc: Rounding) -> X87 {
        with(pc, rc, || unsafe {
            w(match op {
                Op::Add => extF80_add(r(a), r(b)),
                Op::Sub => extF80_sub(r(a), r(b)),
                Op::Mul => extF80_mul(r(a), r(b)),
                Op::Div => extF80_div(r(a), r(b)),
            })
        })
    }

    pub fn sqrt(a: X87, pc: Precision, rc: Rounding) -> X87 {
        with(pc, rc, || unsafe { w(extF80_sqrt(r(a))) })
    }

    pub fn from_f32(v: f32) -> X87 {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            w(f32_to_extF80(v.to_bits()))
        })
    }
    pub fn from_f64(v: f64) -> X87 {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            w(f64_to_extF80(v.to_bits()))
        })
    }
    pub fn from_i32(v: i32) -> X87 {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            w(i32_to_extF80(v))
        })
    }
    pub fn from_i64(v: i64) -> X87 {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            w(i64_to_extF80(v))
        })
    }
    pub fn to_f32(a: X87, pc: Precision, rc: Rounding) -> f32 {
        with(pc, rc, || unsafe { f32::from_bits(extF80_to_f32(r(a))) })
    }
    pub fn to_f64(a: X87, pc: Precision, rc: Rounding) -> f64 {
        with(pc, rc, || unsafe { f64::from_bits(extF80_to_f64(r(a))) })
    }
    pub fn to_i32(a: X87, rc: Rounding) -> i32 {
        with(Precision::Ext, rc, || unsafe {
            extF80_to_i32(r(a), sf_mode(rc), true)
        })
    }
    pub fn to_i64(a: X87, rc: Rounding) -> i64 {
        with(Precision::Ext, rc, || unsafe {
            extF80_to_i64(r(a), sf_mode(rc), true)
        })
    }
    pub fn round_to_int(a: X87, rc: Rounding) -> X87 {
        with(Precision::Ext, rc, || unsafe {
            w(extF80_roundToInt(r(a), sf_mode(rc), true))
        })
    }
    /// SoftFloat `extF80_rem` is IEEE-754 remainder (nearest quotient), i.e.
    /// FPREM1, not FPREM; used only for the congruence relation below.
    pub fn rem_nearest(a: X87, b: X87) -> X87 {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            w(extF80_rem(r(a), r(b)))
        })
    }
    pub fn lt_eq(a: X87, b: X87) -> (bool, bool) {
        with(Precision::Ext, Rounding::NearestEven, || unsafe {
            (extF80_lt(r(a), r(b)), extF80_eq(r(a), r(b)))
        })
    }
}

use reference::Op;

// ---------------------------------------------------------------------------
// Operand grids (deterministic, seeded)
// ---------------------------------------------------------------------------

/// f64 operands: hand-picked boundary values plus a seeded random tail.
fn f64_grid() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        2.0,
        -2.0,
        3.0,
        0.5,
        -0.5,
        1.5,
        2.5,
        -2.5,
        3.5,
        10.0,
        0.1,
        -0.3,
        1.0 + 2f64.powi(-23),
        1.0 + 2f64.powi(-24),
        1.0 + 3.0 * 2f64.powi(-24),
        1.0 + 2f64.powi(-30),
        1.0 + 2f64.powi(-52),
        2f64.powi(-24),
        2f64.powi(-30),
        2f64.powi(-53),
        f32::MIN_POSITIVE as f64,
        2f64.powi(-149), // smallest f32 denormal
        f32::MAX as f64,
        2f64.powi(128), // just past f32 range
        f64::MIN_POSITIVE,
        5e-324, // smallest f64 denormal
        f64::MAX,
        1e30,
        1e-30,
        3.0e9,
        2147483647.5,
        -2147483648.5,
        2147483648.0,
        2f64.powi(63),
        -2f64.powi(63),
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..16 {
        // moderate exponent, random mantissa, random sign
        let m = rng.next() & 0x000F_FFFF_FFFF_FFFF;
        let e = 1023 - 40 + (rng.next() % 81);
        let s = rng.next() & 1;
        v.push(f64::from_bits((s << 63) | (e << 52) | m));
    }
    for _ in 0..8 {
        v.push(f64::from_bits(rng.next())); // any class, incl. NaN/inf
    }
    v
}

/// Raw extended images that no f64/f32 can express (full 64-bit significands,
/// extended denormals, extreme exponents, NaN payloads, signalling NaN).
fn ext_specials() -> Vec<X87> {
    vec![
        x(false, 0x3FFF, u64::MAX),
        x(false, 0x3FFF, INT_BIT | 1),
        x(true, 0x3FFF, 0xC000_0000_0000_0001),
        x(false, 0x4000, 0xD555_5555_5555_5555),
        x(false, 0x0000, 1),                    // smallest extended denormal
        x(true, 0x0000, 0x4000_0000_0000_0000), // largest-ish denormal, neg
        x(false, 0x0001, INT_BIT),              // smallest normal
        x(false, 0x7FFE, u64::MAX),             // largest finite
        x(true, 0x7FFE, INT_BIT),
        x(false, 0x7FFF, INT_BIT),               // +inf
        x(true, 0x7FFF, INT_BIT),                // -inf
        x(true, 0x7FFF, 0xC000_0000_0000_0000),  // real indefinite
        x(false, 0x7FFF, 0xC000_0000_0000_1234), // QNaN with payload
        x(false, 0x7FFF, 0xA000_0000_0000_0000), // SNaN
    ]
}

fn full_grid() -> Vec<X87> {
    let mut g: Vec<X87> = f64_grid().into_iter().map(num).collect();
    g.extend(ext_specials());
    g
}

// ---------------------------------------------------------------------------
// Slice 1: hand-derived basics (Intel SDM values)
// ---------------------------------------------------------------------------

#[test]
fn one_plus_one_is_two_at_every_pc_and_rc() {
    let _t = serial();
    for pc in PCS {
        for rc in RCS {
            assert_eq!(ONE.add(ONE, pc, rc), TWO, "1+1 at {pc:?}/{rc:?}");
            assert_eq!(TWO.sub(ONE, pc, rc), ONE, "2-1 at {pc:?}/{rc:?}");
            assert_eq!(TWO.mul(ONE, pc, rc), TWO, "2*1 at {pc:?}/{rc:?}");
            assert_eq!(TWO.div(TWO, pc, rc), ONE, "2/2 at {pc:?}/{rc:?}");
        }
    }
    assert_eq!(
        x(false, 0x4001, INT_BIT).sqrt(Precision::F32, Rounding::NearestEven),
        TWO
    );
    // 4.0 = 0x4001:8000.., sqrt = 2.0
    assert_eq!(
        x(false, 0x4001, INT_BIT).sqrt(Precision::Ext, Rounding::Zero),
        TWO
    );
    assert_eq!(X87::last_flags(), SfFlags(0), "exact sqrt raises nothing");
}

#[test]
fn hand_derived_register_images() {
    let _t = serial();
    // FLD1 image: 3FFF:8000000000000000.
    assert_eq!(X87::from_i32(1).to_bytes(), ONE.to_bytes());
    assert_eq!(ONE.to_bytes(), [0, 0, 0, 0, 0, 0, 0, 0x80, 0xFF, 0x3F]);
    // 0.5 = 3FFE:8000.., -2.0 = C000:8000.., 3.0 = 4000:C000..
    assert_eq!(X87::from_f32(0.5), x(false, 0x3FFE, INT_BIT));
    assert_eq!(X87::from_f64(-2.0), x(true, 0x4000, INT_BIT));
    assert_eq!(X87::from_i32(3), x(false, 0x4000, 0xC000_0000_0000_0000));
    // f32 denormal 2^-149 widens to a NORMAL extended value (exp 16383-149).
    assert_eq!(X87::from_f32(f32::from_bits(1)), x(false, 0x3F6A, INT_BIT));
    // f64 denormal 2^-1074 likewise (exp 16383-1074 = 0x3BCD).
    assert_eq!(X87::from_f64(f64::from_bits(1)), x(false, 0x3BCD, INT_BIT));
    // FLD preserves the sign of zero (the Unicorn -0 quirk is NOT replicated).
    assert_eq!(X87::from_f32(-0.0), x(true, 0, 0));
    assert_eq!(X87::from_f64(-0.0), x(true, 0, 0));
    assert!(X87::from_f64(-0.0).is_zero() && X87::from_f64(-0.0).sign());
    // inf / NaN classification.
    assert!(X87::from_f32(f32::INFINITY).is_inf());
    assert!(X87::from_f64(f64::NAN).is_nan());
    assert!(!X87::from_f64(f64::NAN).is_signaling_nan());
    assert!(x(false, 0x7FFF, 0xA000_0000_0000_0000).is_signaling_nan());
    assert!(!x(false, 0x7FFF, 0xE000_0000_0000_0000).is_signaling_nan());
    assert_eq!(ONE.biased_exp(), 0x3FFF);
    assert_eq!(ONE.significand(), INT_BIT);
    assert!(!ONE.sign());
}

/// `a ⊕ b` at (pc, rc) for the expectation rows `[N, Down, Up, Zero]`.
fn check_rows(
    what: &str,
    op: fn(X87, X87, Precision, Rounding) -> X87,
    a: X87,
    b: X87,
    pc: Precision,
    expect: [X87; 4],
) {
    for (rc, want) in RCS.iter().zip(expect) {
        let got = op(a, b, pc, *rc);
        assert_eq!(
            got, want,
            "{what} at {pc:?}/{rc:?}: got {got:?} want {want:?}"
        );
    }
}

#[test]
fn pc_rc_rounding_of_one_plus_small() {
    let _t = serial();
    use Precision::{Ext, F32, F64};
    let add = X87::add;
    let one = ONE;
    let up24 = one_plus(23); // 1 + ulp24
                             // Operand 2^-30: below half an ulp24 (2^-24). Exact at PC53/PC64.
    let b30 = pow2m(30);
    check_rows("1+2^-30", add, one, b30, F32, [one, one, up24, one]);
    for pc in [F64, Ext] {
        let e = one_plus(30);
        check_rows("1+2^-30", add, one, b30, pc, [e; 4]);
    }
    // 2^-24: exactly half an ulp24 -> ties-to-even keeps 1.0.
    let b24 = pow2m(24);
    check_rows("1+2^-24", add, one, b24, F32, [one, one, up24, one]);
    check_rows("1+2^-24", add, one, b24, F64, [one_plus(24); 4]);
    // 2^-52: exact at PC53 (that is exactly ulp53) and PC64.
    let b52 = pow2m(52);
    check_rows("1+2^-52", add, one, b52, F32, [one, one, up24, one]);
    for pc in [F64, Ext] {
        check_rows("1+2^-52", add, one, b52, pc, [one_plus(52); 4]);
    }
    // 2^-53: half ulp53 -> tie to even at PC53; exact at PC64.
    let b53 = pow2m(53);
    check_rows("1+2^-53", add, one, b53, F32, [one, one, up24, one]);
    check_rows("1+2^-53", add, one, b53, F64, [one, one, one_plus(52), one]);
    check_rows("1+2^-53", add, one, b53, Ext, [one_plus(53); 4]);
    // 2^-64: half ulp64 (= 2^-63) -> tie to even at PC64.
    let b64 = pow2m(64);
    check_rows("1+2^-64", add, one, b64, Ext, [one, one, one_plus(63), one]);
    // Just above the PC24 halfway point: 2^-24 + 2^-30 rounds UP under RC0.
    let above = x(false, 0x3FFF - 24, INT_BIT | (1 << 57));
    check_rows(
        "1+(2^-24+2^-30)",
        add,
        one,
        above,
        F32,
        [up24, one, up24, one],
    );
    // Tie between an odd and an even neighbour: (1+2^-23) + 2^-24 -> even.
    let even = one_plus(22);
    check_rows(
        "(1+2^-23)+2^-24",
        add,
        up24,
        b24,
        F32,
        [even, up24, even, up24],
    );
    // Negative mirror of 1+2^-24: directions flip.
    let m1 = x(true, 0x3FFF, INT_BIT);
    let mb24 = x(true, 0x3FFF - 24, INT_BIT);
    let mup24 = x(true, 0x3FFF, INT_BIT | (1 << 40));
    check_rows("-1-2^-24", add, m1, mb24, F32, [m1, mup24, m1, m1]);
    // Same sum written as a subtraction.
    check_rows(
        "1-(-2^-24)",
        X87::sub,
        one,
        mb24,
        F32,
        [one, one, up24, one],
    );
}

#[test]
fn pc_rc_rounding_of_mul_and_div() {
    let _t = serial();
    use Precision::{Ext, F32, F64};
    // (1+2^-12)^2 = 1 + 2^-11 + 2^-24: the 2^-24 term is a PC24 tie.
    let a = one_plus(12);
    let down = one_plus(11);
    let up = x(false, 0x3FFF, INT_BIT | (1 << 52) | (1 << 40));
    check_rows("(1+2^-12)^2", X87::mul, a, a, F32, [down, down, up, down]);
    let exact = x(false, 0x3FFF, INT_BIT | (1 << 52) | (1 << 39));
    for pc in [F64, Ext] {
        check_rows("(1+2^-12)^2", X87::mul, a, a, pc, [exact; 4]);
    }
    // 1/3 (the classic FLD1; FDIV): 3FFD:AAAA...AB at every PC under RC0.
    let three = x(false, 0x4000, 0xC000_0000_0000_0000);
    let third = |sig: u64| x(false, 0x3FFD, sig);
    check_rows(
        "1/3 PC24",
        X87::div,
        ONE,
        three,
        F32,
        [
            third(0xAAAA_AB00_0000_0000),
            third(0xAAAA_AA00_0000_0000),
            third(0xAAAA_AB00_0000_0000),
            third(0xAAAA_AA00_0000_0000),
        ],
    );
    check_rows(
        "1/3 PC53",
        X87::div,
        ONE,
        three,
        F64,
        [
            third(0xAAAA_AAAA_AAAA_A800),
            third(0xAAAA_AAAA_AAAA_A800),
            third(0xAAAA_AAAA_AAAA_B000),
            third(0xAAAA_AAAA_AAAA_A800),
        ],
    );
    check_rows(
        "1/3 PC64",
        X87::div,
        ONE,
        three,
        Ext,
        [
            third(0xAAAA_AAAA_AAAA_AAAB),
            third(0xAAAA_AAAA_AAAA_AAAA),
            third(0xAAAA_AAAA_AAAA_AAAB),
            third(0xAAAA_AAAA_AAAA_AAAA),
        ],
    );
    // 1/3 at PC24 is the f32 0x3EAAAAAB once stored.
    let q = ONE.div(three, F32, Rounding::NearestEven);
    assert_eq!(q.to_f32(F32, Rounding::NearestEven).to_bits(), 0x3EAA_AAAB);
    // sqrt(2) at PC24/RC0 = f32 0x3FB504F3.
    let s2 = TWO.sqrt(F32, Rounding::NearestEven);
    assert_eq!(s2.to_f32(F32, Rounding::NearestEven).to_bits(), 0x3FB5_04F3);
    // sqrt(2) at PC64/RC0 = 3FFF:B504F333F9DE6484 (known 64-bit value).
    assert_eq!(
        TWO.sqrt(Ext, Rounding::NearestEven),
        x(false, 0x3FFF, 0xB504_F333_F9DE_6484)
    );
}

// ---------------------------------------------------------------------------
// Slice 2: raw-SoftFloat differential grids
// ---------------------------------------------------------------------------

#[test]
fn arithmetic_matches_raw_softfloat_over_grid() {
    let _t = serial();
    let grid = full_grid();
    let mut f = Failures::default();
    for &a in &grid {
        for &b in &grid {
            for pc in PCS {
                for rc in RCS {
                    for (name, op, wrapper) in [
                        (
                            "add",
                            Op::Add,
                            X87::add as fn(X87, X87, Precision, Rounding) -> X87,
                        ),
                        ("sub", Op::Sub, X87::sub),
                        ("mul", Op::Mul, X87::mul),
                        ("div", Op::Div, X87::div),
                    ] {
                        let want = reference::bin(op, a, b, pc, rc);
                        let got = wrapper(a, b, pc, rc);
                        f.check(got == want, || {
                            format!("{name}({a:?}, {b:?}) {pc:?}/{rc:?}: got {got:?} want {want:?}")
                        });
                    }
                }
            }
        }
    }
    f.finish("add/sub/mul/div vs raw SoftFloat");
}

#[test]
fn sqrt_matches_raw_softfloat_over_grid() {
    let _t = serial();
    let mut f = Failures::default();
    for &a in &full_grid() {
        for pc in PCS {
            for rc in RCS {
                let want = reference::sqrt(a, pc, rc);
                let got = a.sqrt(pc, rc);
                f.check(got == want, || {
                    format!("sqrt({a:?}) {pc:?}/{rc:?}: got {got:?} want {want:?}")
                });
            }
        }
    }
    f.finish("sqrt vs raw SoftFloat");
}

/// At PC=24 (resp. 53) and round-to-nearest the register result stored with
/// FST m32 (m64) is exactly native IEEE `f32` (`f64`) arithmetic, provided
/// the result is not a subnormal (the x87 register has a wider exponent range,
/// so a subnormal would round twice).
#[test]
fn pc24_and_pc53_match_host_ieee_arithmetic() {
    let _t = serial();
    let n = Rounding::NearestEven;
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let mut f32s: Vec<f32> = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        3.0,
        0.1,
        1.0 + f32::EPSILON,
        f32::MAX,
        f32::MIN_POSITIVE,
        1e-20,
        1e20,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    for _ in 0..40 {
        f32s.push(f32::from_bits(rng.next() as u32));
    }
    let mut f = Failures::default();
    let ok32 = |got: f32, want: f32| -> bool {
        if want.is_nan() {
            got.is_nan()
        } else {
            got.to_bits() == want.to_bits()
        }
    };
    for &a in &f32s {
        let xa = reference::from_f32(a);
        for &b in &f32s {
            let xb = reference::from_f32(b);
            for (name, got, want) in [
                ("add", xa.add(xb, Precision::F32, n), a + b),
                ("sub", xa.sub(xb, Precision::F32, n), a - b),
                ("mul", xa.mul(xb, Precision::F32, n), a * b),
                ("div", xa.div(xb, Precision::F32, n), a / b),
            ] {
                let got = got.to_f32(Precision::F32, n);
                let denorm = want != 0.0 && want.abs() < f32::MIN_POSITIVE;
                f.check(denorm || ok32(got, want), || {
                    format!("f32 {name}({a:e}, {b:e}): got {got:e} want {want:e}")
                });
            }
        }
        let got = xa.sqrt(Precision::F32, n).to_f32(Precision::F32, n);
        f.check(ok32(got, a.sqrt()), || {
            format!("f32 sqrt({a:e}): got {got:e} want {:e}", a.sqrt())
        });
    }
    f.finish("PC24 vs host f32");

    let mut f64s: Vec<f64> = f64_grid();
    for _ in 0..40 {
        f64s.push(f64::from_bits(rng.next()));
    }
    let mut f = Failures::default();
    let ok64 = |got: f64, want: f64| -> bool {
        if want.is_nan() {
            got.is_nan()
        } else {
            got.to_bits() == want.to_bits()
        }
    };
    for &a in &f64s {
        let xa = reference::from_f64(a);
        for &b in &f64s {
            let xb = reference::from_f64(b);
            for (name, got, want) in [
                ("add", xa.add(xb, Precision::F64, n), a + b),
                ("sub", xa.sub(xb, Precision::F64, n), a - b),
                ("mul", xa.mul(xb, Precision::F64, n), a * b),
                ("div", xa.div(xb, Precision::F64, n), a / b),
            ] {
                let got = got.to_f64(Precision::F64, n);
                let denorm = want != 0.0 && want.abs() < f64::MIN_POSITIVE;
                f.check(denorm || ok64(got, want), || {
                    format!("f64 {name}({a:e}, {b:e}): got {got:e} want {want:e}")
                });
            }
        }
        let got = xa.sqrt(Precision::F64, n).to_f64(Precision::F64, n);
        f.check(ok64(got, a.sqrt()), || {
            format!("f64 sqrt({a:e}): got {got:e} want {:e}", a.sqrt())
        });
    }
    f.finish("PC53 vs host f64");
}

// ---------------------------------------------------------------------------
// Slice 3: conversions (FLD/FST/FILD/FISTP/FRNDINT)
// ---------------------------------------------------------------------------

#[test]
fn f32_and_f64_loads_are_exact_widenings() {
    let _t = serial();
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut f32s: Vec<f32> = vec![
        0.0,
        -0.0,
        1.0,
        f32::MAX,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(0x007F_FFFF),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::from_bits(0x7F80_0001), // signalling
    ];
    for _ in 0..64 {
        f32s.push(f32::from_bits(rng.next() as u32));
    }
    let mut f = Failures::default();
    for &v in &f32s {
        let got = X87::from_f32(v);
        let want = reference::from_f32(v);
        f.check(got == want, || {
            format!("from_f32({:#010x}): got {got:?} want {want:?}", v.to_bits())
        });
        if !v.is_nan() {
            // Host check: widening back to f64 is exact.
            let back = got.to_f64(Precision::Ext, Rounding::NearestEven);
            f.check(back.to_bits() == (v as f64).to_bits(), || {
                format!("from_f32({v:e}) -> f64: got {back:e}")
            });
        }
    }
    for &v in &f64_grid() {
        let got = X87::from_f64(v);
        let want = reference::from_f64(v);
        f.check(got == want, || {
            format!("from_f64({:#018x}): got {got:?} want {want:?}", v.to_bits())
        });
    }
    f.finish("FLD m32/m64");
}

#[test]
fn f32_f64_stores_match_softfloat_and_host() {
    let _t = serial();
    let mut f = Failures::default();
    let grid = full_grid();
    for &a in &grid {
        for pc in PCS {
            for rc in RCS {
                let w32 = reference::to_f32(a, pc, rc);
                let g32 = a.to_f32(pc, rc);
                f.check(g32.to_bits() == w32.to_bits(), || {
                    format!(
                        "to_f32({a:?}) {pc:?}/{rc:?}: got {:#010x} want {:#010x}",
                        g32.to_bits(),
                        w32.to_bits()
                    )
                });
                let w64 = reference::to_f64(a, pc, rc);
                let g64 = a.to_f64(pc, rc);
                f.check(g64.to_bits() == w64.to_bits(), || {
                    format!(
                        "to_f64({a:?}) {pc:?}/{rc:?}: got {:#018x} want {:#018x}",
                        g64.to_bits(),
                        w64.to_bits()
                    )
                });
            }
        }
    }
    // Host cross-check (round-to-nearest-even): f64 -> ext -> f32 is `as f32`.
    for &v in &f64_grid() {
        if v.is_nan() {
            continue;
        }
        let got = X87::from_f64(v).to_f32(Precision::F32, Rounding::NearestEven);
        f.check(got.to_bits() == (v as f32).to_bits(), || {
            format!(
                "host: {v:e} as f32 = {:#010x}, got {:#010x}",
                (v as f32).to_bits(),
                got.to_bits()
            )
        });
    }
    f.finish("FST m32/m64 vs SoftFloat+host");
}

#[test]
fn f32_store_directed_rounding_hand_derived() {
    let _t = serial();
    // 1 + 2^-24 lies between f32 1.0 (0x3F800000) and 1+2^-23 (0x3F800001).
    let v = one_plus(24);
    let want = [0x3F80_0000u32, 0x3F80_0000, 0x3F80_0001, 0x3F80_0000];
    for pc in PCS {
        for (rc, w) in RCS.iter().zip(want) {
            assert_eq!(v.to_f32(pc, *rc).to_bits(), w, "{pc:?}/{rc:?}");
        }
    }
    // Negative: Down moves away from zero, Up toward it.
    let nv = x(true, 0x3FFF, INT_BIT | (1 << 39));
    let want = [0xBF80_0000u32, 0xBF80_0001, 0xBF80_0000, 0xBF80_0000];
    for (rc, w) in RCS.iter().zip(want) {
        assert_eq!(nv.to_f32(Precision::F32, *rc).to_bits(), w, "{rc:?}");
    }
    // Overflow: 2^128 is not an f32. N/Up -> +inf, Down/Zero -> f32::MAX.
    let big = x(false, 0x3FFF + 128, INT_BIT);
    let want = [f32::INFINITY, f32::MAX, f32::INFINITY, f32::MAX];
    for (rc, w) in RCS.iter().zip(want) {
        assert_eq!(big.to_f32(Precision::F32, *rc), w, "{rc:?}");
    }
    // Underflow to the f32 denormal range: 2^-150 is half the smallest
    // denormal -> N ties to even (0), Up -> 2^-149, Down/Zero -> 0.
    let tiny = x(false, 0x3FFF - 150, INT_BIT);
    let want = [0u32, 0, 1, 0];
    for (rc, w) in RCS.iter().zip(want) {
        assert_eq!(tiny.to_f32(Precision::F32, *rc).to_bits(), w, "{rc:?}");
    }
    // f64 store: 1 + 2^-53 is the f64 tie.
    let v = one_plus(53);
    let want = [1.0f64, 1.0, 1.0 + f64::EPSILON, 1.0];
    for (rc, w) in RCS.iter().zip(want) {
        assert_eq!(v.to_f64(Precision::Ext, *rc), w, "{rc:?}");
    }
}

#[test]
fn integer_loads_are_exact_and_roundtrip_through_fistp() {
    let _t = serial();
    let mut rng = Rng(0xA5A5_5A5A_1357_9BDF);
    let mut i32s: Vec<i32> = vec![0, 1, -1, 2, i32::MAX, i32::MIN, 0x0100_0001, -0x0100_0001];
    let mut i64s: Vec<i64> = vec![
        0,
        1,
        -1,
        i64::MAX,
        i64::MIN,
        i64::MIN + 1,
        (1 << 53) + 1,
        -((1 << 53) + 1),
    ];
    for _ in 0..32 {
        i32s.push(rng.next() as i32);
        i64s.push(rng.next() as i64);
    }
    let mut f = Failures::default();
    for &i in &i32s {
        let got = X87::from_i32(i);
        let want = reference::from_i32(i);
        f.check(got == want, || {
            format!("from_i32({i}): got {got:?} want {want:?}")
        });
        for rc in RCS {
            f.check(got.to_i32(rc) == i, || format!("i32 round trip {i} {rc:?}"));
            f.check(got.to_i64(rc) == i as i64, || {
                format!("i32->i64 {i} {rc:?}")
            });
        }
        let h = got.to_f64(Precision::Ext, Rounding::NearestEven);
        f.check(h == i as f64, || format!("from_i32({i}) as f64 = {h}"));
    }
    for &i in &i64s {
        let got = X87::from_i64(i);
        let want = reference::from_i64(i);
        f.check(got == want, || {
            format!("from_i64({i}): got {got:?} want {want:?}")
        });
        for rc in RCS {
            f.check(got.to_i64(rc) == i, || format!("i64 round trip {i} {rc:?}"));
        }
        // A 64-bit significand holds every i64 exactly; the f64 store rounds
        // like Rust's `as f64` (nearest-even).
        let h = got.to_f64(Precision::Ext, Rounding::NearestEven);
        f.check(h.to_bits() == (i as f64).to_bits(), || {
            format!("from_i64({i}) as f64 = {h}")
        });
    }
    f.finish("FILD / round trip");
}

/// Host model of FISTP/FRNDINT for one RC on an f64 operand.
fn host_round(v: f64, rc: Rounding) -> f64 {
    match rc {
        Rounding::NearestEven => v.round_ties_even(),
        Rounding::Down => v.floor(),
        Rounding::Up => v.ceil(),
        Rounding::Zero => v.trunc(),
    }
}

fn host_i32(v: f64, rc: Rounding) -> i32 {
    let r = host_round(v, rc);
    if !v.is_nan() && (-2147483648.0..=2147483647.0).contains(&r) {
        r as i32
    } else {
        i32::MIN // integer indefinite
    }
}

fn host_i64(v: f64, rc: Rounding) -> i64 {
    let r = host_round(v, rc);
    if !v.is_nan() && (-(2f64.powi(63))..2f64.powi(63)).contains(&r) {
        r as i64
    } else {
        i64::MIN
    }
}

#[test]
fn fistp_rounding_hand_derived() {
    let _t = serial();
    // (value, [N, Down, Up, Zero]) — SDM: 2.5 -> 2,2,3,2.
    let table: [(f64, [i32; 4]); 12] = [
        (2.5, [2, 2, 3, 2]),
        (-2.5, [-2, -3, -2, -2]),
        (1.5, [2, 1, 2, 1]),
        (-1.5, [-2, -2, -1, -1]),
        (0.5, [0, 0, 1, 0]),
        (-0.5, [0, -1, 0, 0]),
        (3.5, [4, 3, 4, 3]),
        (0.75, [1, 0, 1, 0]),
        (-0.25, [0, -1, 0, 0]),
        (7.0, [7, 7, 7, 7]),
        (2147483647.0, [i32::MAX; 4]),
        (-2147483648.0, [i32::MIN; 4]),
    ];
    for (v, want) in table {
        let xv = reference::from_f64(v);
        for (rc, w) in RCS.iter().zip(want) {
            assert_eq!(xv.to_i32(*rc), w, "fistp32({v}) {rc:?}");
            assert_eq!(xv.to_i64(*rc), w as i64, "fistp64({v}) {rc:?}");
        }
    }
    // Integer indefinite (0x80000000 / 0x8000000000000000).
    let indefinite32 = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        2147483648.0,
        -2147483649.0,
        1e30,
    ];
    for v in indefinite32 {
        for rc in RCS {
            assert_eq!(
                reference::from_f64(v).to_i32(rc),
                i32::MIN,
                "fistp32({v}) {rc:?}"
            );
        }
    }
    // 2147483647.5: nearest -> 2^31 (overflow -> indefinite); truncate fits.
    let v = reference::from_f64(2147483647.5);
    assert_eq!(v.to_i32(Rounding::NearestEven), i32::MIN);
    assert_eq!(v.to_i32(Rounding::Up), i32::MIN);
    assert_eq!(v.to_i32(Rounding::Down), i32::MAX);
    assert_eq!(v.to_i32(Rounding::Zero), i32::MAX);
    // -2147483648.5: nearest -> -2^31 (fits), Down -> -2^31-1 (overflow).
    let v = reference::from_f64(-2147483648.5);
    assert_eq!(v.to_i32(Rounding::NearestEven), i32::MIN);
    assert_eq!(v.to_i32(Rounding::Zero), i32::MIN);
    assert_eq!(v.to_i32(Rounding::Up), i32::MIN);
    assert_eq!(v.to_i32(Rounding::Down), i32::MIN); // overflow -> indefinite == MIN
                                                    // 64-bit: 2^63 is out of range; -2^63 is representable.
    let p63 = reference::from_f64(2f64.powi(63));
    let m63 = reference::from_f64(-(2f64.powi(63)));
    for rc in RCS {
        assert_eq!(p63.to_i64(rc), i64::MIN, "2^63 {rc:?}");
        assert_eq!(m63.to_i64(rc), i64::MIN, "-2^63 {rc:?}");
        assert_eq!(X87::from_f64(f64::NAN).to_i64(rc), i64::MIN);
        assert_eq!(X87::from_f64(f64::INFINITY).to_i64(rc), i64::MIN);
    }
    // Extended-only value: 2^63 - 0.5 (64-bit significand, not an f64).
    let v = x(false, 0x3FFF + 62, u64::MAX); // 2^63 - 0.5
    assert_eq!(v.to_i64(Rounding::Zero), i64::MAX);
    assert_eq!(v.to_i64(Rounding::Down), i64::MAX);
    assert_eq!(v.to_i64(Rounding::NearestEven), i64::MIN); // rounds to 2^63
    assert_eq!(v.to_i64(Rounding::Up), i64::MIN);
}

#[test]
fn fistp_and_frndint_match_host_and_softfloat() {
    let _t = serial();
    let mut f = Failures::default();
    let mut grid = f64_grid();
    for k in -6..=6 {
        grid.push(k as f64 + 0.5);
        grid.push(k as f64 + 0.25);
        grid.push(k as f64 - 0.75);
    }
    for &v in &grid {
        let xv = reference::from_f64(v);
        for rc in RCS {
            let g32 = xv.to_i32(rc);
            f.check(g32 == host_i32(v, rc), || {
                format!("fistp32({v:e}) {rc:?}: got {g32} host {}", host_i32(v, rc))
            });
            f.check(g32 == reference::to_i32(xv, rc), || {
                format!(
                    "fistp32({v:e}) {rc:?}: got {g32} softfloat {}",
                    reference::to_i32(xv, rc)
                )
            });
            let g64 = xv.to_i64(rc);
            f.check(g64 == host_i64(v, rc), || {
                format!("fistp64({v:e}) {rc:?}: got {g64} host {}", host_i64(v, rc))
            });
            f.check(g64 == reference::to_i64(xv, rc), || {
                format!(
                    "fistp64({v:e}) {rc:?}: got {g64} softfloat {}",
                    reference::to_i64(xv, rc)
                )
            });
            // FRNDINT: SoftFloat bits, and host value when not NaN.
            let gr = xv.frndint(rc);
            let wr = reference::round_to_int(xv, rc);
            f.check(gr == wr, || {
                format!("frndint({v:e}) {rc:?}: got {gr:?} softfloat {wr:?}")
            });
            if !v.is_nan() {
                let h = gr.to_f64(Precision::Ext, Rounding::NearestEven);
                f.check(h.to_bits() == host_round(v, rc).to_bits(), || {
                    format!(
                        "frndint({v:e}) {rc:?}: got {h:e} host {:e}",
                        host_round(v, rc)
                    )
                });
            }
        }
    }
    for &a in &ext_specials() {
        for rc in RCS {
            f.check(a.to_i32(rc) == reference::to_i32(a, rc), || {
                format!("fistp32 {a:?} {rc:?}")
            });
            f.check(a.to_i64(rc) == reference::to_i64(a, rc), || {
                format!("fistp64 {a:?} {rc:?}")
            });
            f.check(a.frndint(rc) == reference::round_to_int(a, rc), || {
                format!("frndint {a:?} {rc:?}")
            });
        }
    }
    f.finish("FISTP/FRNDINT");
}

// ---------------------------------------------------------------------------
// Slice 4: FPREM (truncated quotient)
// ---------------------------------------------------------------------------

#[test]
fn fprem_hand_derived_truncated_quotient() {
    let _t = serial();
    let f = |v: f64| reference::from_f64(v);
    // (a, b, remainder). 5.5 rem 2.0 = +1.5 (q=2, truncated); IEEE
    // remainder (FPREM1) would give -0.5 (q=3).
    let table = [
        (5.5, 2.0, 1.5),
        (-5.5, 2.0, -1.5),
        (5.5, -2.0, 1.5),
        (-5.5, -2.0, -1.5),
        (7.0, 4.0, 3.0),
        (7.0, 2.0, 1.0),
        (1.0, 4.0, 1.0),
        (0.75, 1.0, 0.75),
        (10.0, 3.0, 1.0),
        (1e10, 7.0, 1e10 % 7.0),
    ];
    for (a, b, want) in table {
        let (r, c2) = f(a).fprem(f(b));
        assert_eq!(r, f(want), "{a} rem {b}");
        assert!(!c2, "{a} rem {b}: C2 must be clear (gap < 64)");
    }
    // Exact multiples give a zero carrying the dividend's sign.
    let (r, _) = f(4.0).fprem(f(2.0));
    assert_eq!(r, x(false, 0, 0));
    let (r, _) = f(-4.0).fprem(f(2.0));
    assert_eq!(r, x(true, 0, 0));
    // Exponent gap thresholds: 2^63 rem 1 has gap 63 (complete), 2^64 rem 1
    // has gap 64 (partial in hardware -> C2 set; the exact remainder is 0).
    let (r, c2) = x(false, 0x3FFF + 63, INT_BIT).fprem(ONE);
    assert!(!c2 && r.is_zero());
    let (r, c2) = x(false, 0x3FFF + 64, INT_BIT).fprem(ONE);
    assert!(c2 && r.is_zero());
    // 2^100 rem 3 = 1 (4 = 1 mod 3 => 2^100 = (2^2)^50 = 1 mod 3), exact
    // even though hardware needs several passes; C2 reports the partial.
    let (r, c2) = x(false, 0x3FFF + 100, INT_BIT).fprem(f(3.0));
    assert!(c2);
    assert_eq!(r, ONE);
    // Dividend smaller than divisor: unchanged, no partial.
    let (r, c2) = f(0.125).fprem(f(8.0));
    assert!(!c2);
    assert_eq!(r, f(0.125));
    // 64-bit significand operand: (2^64-1)*2^-63 rem 3 computed exactly.
    // a = 1.999..9 (all ones) * 2^0 ~ 2 - 2^-63; rem 0.75 =
    //   a - 2*0.75 = 0.5 - 2^-63 (q = 2).
    let a = x(false, 0x3FFF, u64::MAX);
    let (r, c2) = a.fprem(f(0.75));
    assert!(!c2);
    assert_eq!(r, x(false, 0x3FFD, 0xFFFF_FFFF_FFFF_FFFC)); // 0.5 - 2^-63
}

#[test]
fn fprem_matches_host_fmod_and_softfloat_remainder() {
    let _t = serial();
    let mut f = Failures::default();
    let vals: Vec<f64> = f64_grid();
    for &a in &vals {
        for &b in &vals {
            if !a.is_finite() || !b.is_finite() || b == 0.0 {
                continue;
            }
            let xa = reference::from_f64(a);
            let xb = reference::from_f64(b);
            let (r, c2) = xa.fprem(xb);
            // 1. Exact host fmod (truncated quotient, sign of dividend).
            let want = reference::from_f64(a % b);
            f.check(r == want, || {
                format!("fprem({a:e}, {b:e}): got {r:?} fmod {want:?}")
            });
            // 2. C2 = (exponent gap >= 64), operands are normal extended.
            let gap = xa.biased_exp() as i32 - xb.biased_exp() as i32;
            let want_c2 = a != 0.0 && gap >= 64;
            f.check(c2 == want_c2, || {
                format!("fprem({a:e}, {b:e}): C2 got {c2} want {want_c2} (gap {gap})")
            });
            // 3. Relation to SoftFloat's nearest-quotient remainder: they
            //    agree unless the nearest remainder has the opposite sign of
            //    the dividend, where FPREM = FPREM1 + |b| carrying a's sign.
            let r1 = reference::rem_nearest(xa, xb);
            let expect = if !r1.is_zero() && r1.sign() != xa.sign() {
                let bs = x(xa.sign(), xb.biased_exp(), xb.significand());
                reference::bin(Op::Add, r1, bs, Precision::Ext, Rounding::NearestEven)
            } else {
                r1
            };
            // zero remainders carry the dividend's sign in both
            f.check(r == expect || (r.is_zero() && expect.is_zero()), || {
                format!("fprem({a:e}, {b:e}): got {r:?}, SoftFloat-derived {expect:?}")
            });
        }
    }
    f.finish("FPREM vs fmod + SoftFloat relation");
}

// ---------------------------------------------------------------------------
// Slice 5: FCOM / FUCOM condition bits
// ---------------------------------------------------------------------------

fn cc(c3: bool, c2: bool, c0: bool) -> ConditionCodes {
    ConditionCodes { c3, c2, c0 }
}

#[test]
fn fcom_condition_bits_hand_derived() {
    let _t = serial();
    let (gt, lt, eq, un) = (
        cc(false, false, false),
        cc(false, false, true),
        cc(true, false, false),
        cc(true, true, true),
    );
    let one = ONE;
    let two = TWO;
    let neg1 = x(true, 0x3FFF, INT_BIT);
    let pz = x(false, 0, 0);
    let nz = x(true, 0, 0);
    let inf = x(false, 0x7FFF, INT_BIT);
    let ninf = x(true, 0x7FFF, INT_BIT);
    let den = x(false, 0, 1);
    let qnan = x(false, 0x7FFF, 0xC000_0000_0000_0000);
    let snan = x(false, 0x7FFF, 0xA000_0000_0000_0000);
    let indef = x(true, 0x7FFF, 0xC000_0000_0000_0000);
    // Ordered pairs: (a, b, codes, same for fcom and fucom, no invalid).
    let ordered = [
        (two, one, gt),
        (one, two, lt),
        (one, one, eq),
        (neg1, one, lt),
        (one, neg1, gt),
        (pz, nz, eq), // +0 == -0
        (nz, pz, eq),
        (den, pz, gt),
        (nz, den, lt),
        (inf, one, gt),
        (ninf, one, lt),
        (inf, inf, eq),
        (ninf, ninf, eq),
        (ninf, inf, lt),
        (inf, ninf, gt),
    ];
    for (a, b, want) in ordered {
        assert_eq!(a.fcom(b), (want, false), "fcom({a:?}, {b:?})");
        assert_eq!(a.fucom(b), (want, false), "fucom({a:?}, {b:?})");
    }
    // A QNaN is unordered; FCOM raises #IA, FUCOM does not.
    for (a, b) in [(qnan, one), (one, qnan), (qnan, qnan), (indef, pz)] {
        assert_eq!(a.fcom(b), (un, true), "fcom({a:?}, {b:?})");
        assert_eq!(a.fucom(b), (un, false), "fucom({a:?}, {b:?})");
    }
    // A signalling NaN raises #IA under both.
    for (a, b) in [(snan, one), (one, snan), (snan, qnan)] {
        assert_eq!(a.fcom(b), (un, true), "fcom({a:?}, {b:?})");
        assert_eq!(a.fucom(b), (un, true), "fucom({a:?}, {b:?})");
    }
    // The invalid flag is observable on the SoftFloat side after an unordered
    // FCOM.
    let _ = qnan.fcom(one);
    assert!(X87::last_flags().invalid());
}

#[test]
fn fcom_matches_host_ordering_over_grid() {
    let _t = serial();
    let mut f = Failures::default();
    let vals = f64_grid();
    for &a in &vals {
        for &b in &vals {
            let want = match a.partial_cmp(&b) {
                Some(std::cmp::Ordering::Greater) => cc(false, false, false),
                Some(std::cmp::Ordering::Less) => cc(false, false, true),
                Some(std::cmp::Ordering::Equal) => cc(true, false, false),
                None => cc(true, true, true),
            };
            let nan = a.is_nan() || b.is_nan();
            let xa = reference::from_f64(a);
            let xb = reference::from_f64(b);
            // Note: the f64 NaNs in the grid are quiet, except random-bit
            // ones which `from_f64` quiets (SNaN -> QNaN, invalid raised in
            // the conversion, not in the compare).
            let (g, inv) = xa.fcom(xb);
            f.check(g == want && inv == nan, || {
                format!("fcom({a:e}, {b:e}): got {g:?}/{inv} want {want:?}/{nan}")
            });
            let (g, _) = xa.fucom(xb);
            f.check(g == want, || {
                format!("fucom({a:e}, {b:e}): got {g:?} want {want:?}")
            });
        }
    }
    // Extended-only operands against SoftFloat's lt/eq predicates.
    let ext = ext_specials();
    for &a in &ext {
        for &b in &ext {
            if a.is_nan() || b.is_nan() {
                continue;
            }
            let (lt, eq) = reference::lt_eq(a, b);
            let want = if eq {
                cc(true, false, false)
            } else if lt {
                cc(false, false, true)
            } else {
                cc(false, false, false)
            };
            f.check(a.fcom(b).0 == want && a.fucom(b).0 == want, || {
                format!("fcom({a:?}, {b:?}) vs SoftFloat lt/eq {want:?}")
            });
        }
    }
    f.finish("FCOM/FUCOM");
}

/// Regression for a bug this suite found: `X87::is_signaling_nan` used to test
/// only the significand, so any finite value whose significand looks like
/// `10xxxx...` (nonzero payload, quiet bit clear, e.g. 2.5 =
/// 4000:A000000000000000) was reported as a signalling NaN and
/// `fucom(2.5, QNaN)` returned `invalid = true`. The SDM, and SoftFloat's
/// `extF80_isSignalingNaN` (which checks exp == 0x7FFF), say FUCOM raises #IA
/// only for an SNaN.
#[test]
fn fucom_invalid_only_for_signaling_nans() {
    let _t = serial();
    let mut f = Failures::default();
    // Finite values that look like an SNaN significand must not be one.
    for v in [2.5, -2.5, 10.0, -0.3, 1.0 + 2f64.powi(-23)] {
        let xv = reference::from_f64(v);
        f.check(!xv.is_signaling_nan(), || {
            format!("is_signaling_nan({v:e}) = true for finite {xv:?}")
        });
    }
    let qnan = x(false, 0x7FFF, 0xC000_0000_0000_0000);
    for &a in &f64_grid() {
        let xa = reference::from_f64(a);
        let (_, inv) = xa.fucom(qnan);
        f.check(!inv, || {
            format!("fucom({a:e}, QNaN): invalid raised, want clear")
        });
        let (_, inv) = qnan.fucom(xa);
        f.check(!inv, || {
            format!("fucom(QNaN, {a:e}): invalid raised, want clear")
        });
    }
    f.finish("FUCOM invalid flag");
}

// ---------------------------------------------------------------------------
// Slice 6: special values
// ---------------------------------------------------------------------------

const INF: X87 = X87 {
    bits: [0, 0, 0, 0, 0, 0, 0, 0x80, 0xFF, 0x7F],
};

#[test]
fn special_values_hand_derived() {
    let _t = serial();
    use Precision::{Ext, F32};
    let n = Rounding::NearestEven;
    let indef = x(true, 0x7FFF, 0xC000_0000_0000_0000);
    let ninf = x(true, 0x7FFF, INT_BIT);
    let pz = x(false, 0, 0);
    let nz = x(true, 0, 0);
    let three = x(false, 0x4000, 0xC000_0000_0000_0000);
    let neg3 = x(true, 0x4000, 0xC000_0000_0000_0000);

    // Invalid operations produce the real indefinite (negative QNaN) and
    // raise #IA.
    let invalid_cases: [(&str, X87); 8] = [
        ("inf-inf", INF.sub(INF, F32, n)),
        ("inf+(-inf)", INF.add(ninf, F32, n)),
        ("0*inf", pz.mul(INF, F32, n)),
        ("inf*0", INF.mul(pz, Ext, n)),
        ("0/0", pz.div(pz, F32, n)),
        ("inf/inf", INF.div(INF, F32, n)),
        ("sqrt(-1)", x(true, 0x3FFF, INT_BIT).sqrt(F32, n)),
        ("sqrt(-inf)", ninf.sqrt(F32, n)),
    ];
    for (name, got) in invalid_cases {
        assert_eq!(got, indef, "{name}");
    }
    let _ = INF.sub(INF, F32, n);
    assert!(X87::last_flags().invalid());

    // Division by zero gives a signed infinity and the divide-by-zero flag.
    assert_eq!(ONE.div(pz, F32, n), INF);
    assert_eq!(ONE.div(nz, F32, n), ninf);
    assert_eq!(x(true, 0x3FFF, INT_BIT).div(pz, F32, n), ninf);
    assert_eq!(X87::last_flags(), SfFlags(SfFlags::INFINITE));

    // Infinity arithmetic with finite operands.
    assert_eq!(INF.add(three, F32, n), INF);
    assert_eq!(three.sub(INF, F32, n), ninf);
    assert_eq!(INF.mul(neg3, F32, n), ninf);
    assert_eq!(three.div(INF, F32, n), pz);
    assert_eq!(neg3.div(INF, F32, n), nz);
    assert_eq!(INF.sqrt(F32, n), INF);

    // Signed zeros.
    assert_eq!(pz.add(nz, Ext, n), pz);
    assert_eq!(pz.add(nz, Ext, Rounding::Down), nz);
    assert_eq!(pz.add(nz, Ext, Rounding::Up), pz);
    assert_eq!(pz.add(nz, Ext, Rounding::Zero), pz);
    assert_eq!(nz.add(nz, Ext, Rounding::Up), nz);
    assert_eq!(nz.add(nz, Ext, n), nz);
    assert_eq!(three.sub(three, Ext, n), pz);
    assert_eq!(three.sub(three, Ext, Rounding::Down), nz);
    assert_eq!(pz.mul(neg3, F32, n), nz);
    assert_eq!(nz.div(three, F32, n), nz);
    assert_eq!(pz.sqrt(F32, n), pz);
    assert_eq!(nz.sqrt(F32, n), nz);

    // NaN propagation: a QNaN operand propagates (payload intact), a
    // signalling NaN is quieted (quiet bit set) and raises #IA.
    let qn = x(false, 0x7FFF, 0xC000_0000_0000_1234);
    assert_eq!(qn.add(ONE, F32, n), qn);
    assert_eq!(ONE.mul(qn, F32, n), qn);
    assert_eq!(qn.sqrt(F32, n), qn);
    let _ = qn.add(ONE, F32, n);
    assert_eq!(X87::last_flags(), SfFlags(0), "QNaN propagation is silent");
    let sn = x(false, 0x7FFF, 0xA000_0000_0000_0000);
    let quieted = x(false, 0x7FFF, 0xE000_0000_0000_0000);
    assert_eq!(sn.add(ONE, F32, n), quieted);
    assert!(X87::last_flags().invalid());
    assert_eq!(ONE.div(sn, F32, n), quieted);

    // Denormals (extended): exact arithmetic stays exact.
    let d1 = x(false, 0, 1);
    let d2 = x(false, 0, 2);
    assert_eq!(d1.add(d1, Ext, n), d2);
    assert_eq!(d1.mul(TWO, Ext, n), d2);
    assert_eq!(d2.div(TWO, Ext, n), d1);
    // Smallest normal / 2 = a denormal with the integer bit clear.
    let min_normal = x(false, 1, INT_BIT);
    assert_eq!(
        min_normal.div(TWO, Ext, n),
        x(false, 0, 0x4000_0000_0000_0000)
    );
    // Largest denormal + smallest denormal = smallest normal.
    let max_denorm = x(false, 0, 0x7FFF_FFFF_FFFF_FFFF);
    assert_eq!(max_denorm.add(d1, Ext, n), min_normal);
    // Underflow to zero is silent-inexact for a product of tiny normals.
    let tiny = x(false, 1, INT_BIT);
    assert_eq!(tiny.mul(tiny, Ext, n), pz);
    assert_eq!(tiny.mul(tiny, Ext, Rounding::Up), d1);

    // Overflow at PC64: the rounding mode picks inf vs the largest finite.
    let max = x(false, 0x7FFE, u64::MAX);
    assert_eq!(max.add(max, Ext, n), INF);
    assert_eq!(max.add(max, Ext, Rounding::Up), INF);
    assert_eq!(max.add(max, Ext, Rounding::Down), max);
    assert_eq!(max.add(max, Ext, Rounding::Zero), max);
    let nmax = x(true, 0x7FFE, u64::MAX);
    assert_eq!(nmax.add(nmax, Ext, Rounding::Up), nmax);
    assert_eq!(nmax.add(nmax, Ext, Rounding::Down), ninf);

    // Flags: exact ops are clean, inexact ops raise inexact.
    let _ = ONE.add(ONE, F32, n);
    assert_eq!(X87::last_flags(), SfFlags(0));
    let _ = ONE.div(three, F32, n);
    assert!(X87::last_flags().inexact() && !X87::last_flags().invalid());
}

// ---------------------------------------------------------------------------
// Slice 7: transcendentals (APPROX, pinned literals from mpmath)
// ---------------------------------------------------------------------------

/// Reference images rounded from 400-bit `mpmath` results, nearest-even.
/// Regenerate: evaluate the function on the exact f64 operand with
/// `mpmath.mp.prec = 400` and round to 64 significand bits.
#[test]
fn approx_transcendentals_stay_within_contract_of_pinned_literals() {
    let _t = serial();
    let f = |v: f64| reference::from_f64(v);
    // (name, approximate result, contract ulps, expected image)
    let cases: Vec<(&str, trans::Approx, (bool, u16, u64))> = vec![
        (
            "sin(1)",
            trans::approx_sin(f(1.0)),
            (false, 0x3ffe, 0xd76aa47848677021),
        ),
        (
            "cos(1)",
            trans::approx_cos(f(1.0)),
            (false, 0x3ffe, 0x8a51407da8345c92),
        ),
        (
            "sin(2)",
            trans::approx_sin(f(2.0)),
            (false, 0x3ffe, 0xe8c7b7568da22efd),
        ),
        (
            "cos(100)",
            trans::approx_cos(f(100.0)),
            (false, 0x3ffe, 0xdcc0edfb32fefb20),
        ),
        (
            "sin(1e10)",
            trans::approx_sin(f(1e10)),
            (true, 0x3ffd, 0xf99a63c4b5271a66),
        ),
        (
            "cos(-0.25)",
            trans::approx_cos(f(-0.25)),
            (false, 0x3ffe, 0xf80aa4fbef750ba8),
        ),
        (
            "f2xm1(0.25)",
            trans::approx_f2xm1(f(0.25)),
            (false, 0x3ffc, 0xc1bf828c6dc54b7a),
        ),
        (
            "f2xm1(-0.75)",
            trans::approx_f2xm1(f(-0.75)),
            (true, 0x3ffd, 0xcf901f5ce48ead21),
        ),
        (
            "f2xm1(1)",
            trans::approx_f2xm1(f(1.0)),
            (false, 0x3fff, 0x8000000000000000),
        ),
        (
            "fyl2x(x=10,y=1)",
            trans::approx_fyl2x(f(10.0), f(1.0)),
            (false, 0x4000, 0xd49a784bcd1b8afe),
        ),
        (
            "fyl2x(x=0.1,y=-3)",
            trans::approx_fyl2x(f(0.1), f(-3.0)),
            (false, 0x4002, 0x9f73da38d9d4a72a),
        ),
        (
            "fyl2x(x=3,y=0.5)",
            trans::approx_fyl2x(f(3.0), f(0.5)),
            (false, 0x3ffe, 0xcae00d1cfdeb43d0),
        ),
        (
            "fpatan(y=1,x=2)",
            trans::approx_patan(f(1.0), f(2.0)),
            (false, 0x3ffd, 0xed63382b0dda7b45),
        ),
        (
            "fpatan(y=-3,x=-5)",
            trans::approx_patan(f(-3.0), f(-5.0)),
            (true, 0x4000, 0xa6799ef62f4f0059),
        ),
        // pi, the FLDPI value 4000:C90FDAA22168C235 (Intel SDM).
        (
            "fpatan(y=0,x=-1)",
            trans::approx_patan(f(0.0), f(-1.0)),
            (false, 0x4000, 0xc90fdaa22168c235),
        ),
        (
            "fpatan(y=1,x=-1)",
            trans::approx_patan(f(1.0), f(-1.0)),
            (false, 0x4000, 0x96cbe3f9990e91a8),
        ),
    ];
    let mut f2 = Failures::default();
    for (name, approx, (s, e, g)) in cases {
        let reference = x(s, e, g);
        f2.check(approx.within_tolerance_of(reference), || {
            format!(
                "{name}: got {:?} outside contract {} of {reference:?}",
                approx.value, approx.ulp_err_64
            )
        });
    }
    f2.finish("pinned transcendental literals");
    assert_eq!(tolerance::SIN_COS_ULP, 1 << 14);
    assert_eq!(tolerance::ATAN_ULP, 1 << 14);
    assert_eq!(tolerance::F2XM1_ULP, 1 << 14);
    assert_eq!(tolerance::FYL2X_ULP, 1 << 14);
}

#[test]
fn approx_transcendentals_exact_special_cases() {
    let _t = serial();
    let pz = x(false, 0, 0);
    let nz = x(true, 0, 0);
    // sin(±0) = ±0, cos(±0) = 1, exact.
    assert_eq!(trans::approx_sin(pz).value, pz);
    assert_eq!(trans::approx_sin(nz).value, nz);
    assert_eq!(trans::approx_cos(pz).value, ONE);
    assert_eq!(trans::approx_cos(nz).value, ONE);
    // f2xm1(0) = 0, f2xm1(1) = 1 (2^1 - 1).
    assert_eq!(trans::approx_f2xm1(pz).value, pz);
    // fyl2x: y*log2(x) with exact power-of-two x is exact; log2(1) = 0 with
    // the product's sign (SDM: -3*log2(1) = -0).
    assert_eq!(
        trans::approx_fyl2x(ONE, x(false, 0x4000, 0xC000_0000_0000_0000)).value,
        pz
    );
    assert_eq!(
        trans::approx_fyl2x(ONE, x(true, 0x4000, 0xC000_0000_0000_0000)).value,
        nz
    );
    // 3 * log2(8) = 9 = 4002:9000000000000000 (log2 of a power of two is exact).
    assert_eq!(
        trans::approx_fyl2x(
            x(false, 0x4002, INT_BIT),
            x(false, 0x4000, 0xC000_0000_0000_0000)
        )
        .value,
        x(false, 0x4002, 0x9000_0000_0000_0000)
    );
    // fpatan(+0, +1) = +0 and pi/4 for (1, 1).
    assert_eq!(trans::approx_patan(pz, ONE).value, pz);
    assert!(trans::approx_patan(ONE, ONE).within_tolerance_of(x(
        false,
        0x3FFE,
        0xC90F_DAA2_2168_C235
    )));
    // NaN / inf inputs make no tolerance claim and never panic.
    assert!(trans::approx_sin(x(false, 0x7FFF, INT_BIT)).value.is_nan());
    assert!(trans::approx_cos(x(false, 0x7FFF, 0xC000_0000_0000_0000))
        .value
        .is_nan());
    // Tolerance predicate semantics.
    let a = trans::approx_sin(ONE);
    assert!(
        !a.within_tolerance_of(x(false, 0x7FFF, 0xC000_0000_0000_0000)),
        "NaN reference"
    );
    assert!(
        !a.within_tolerance_of(x(true, 0x3FFE, 0xD76A_A478_4867_7021)),
        "sign differs"
    );
}

#[test]
fn fucom_invalid_regression_vectors() {
    let _t = serial();
    let qnan = x(false, 0x7FFF, 0xC000_0000_0000_0000);
    let qnan2 = x(true, 0x7FFF, 0xC000_0000_0000_1234);
    let snan = x(false, 0x7FFF, 0xA000_0000_0000_0000);
    let unordered = cc(true, true, true);
    // The vectors that used to report invalid = true: finite operands whose
    // significand pattern mimics an SNaN, against a QNaN. FUCOM: no #IA.
    for v in [2.5, -2.5, 1e1, -3e-1] {
        let xv = reference::from_f64(v);
        assert!(!xv.is_signaling_nan(), "{v} is finite, not an SNaN");
        assert_eq!(xv.fucom(qnan), (unordered, false), "fucom({v}, QNaN)");
        assert_eq!(qnan.fucom(xv), (unordered, false), "fucom(QNaN, {v})");
        // FCOM raises #IA on any NaN.
        assert_eq!(xv.fcom(qnan), (unordered, true), "fcom({v}, QNaN)");
        assert_eq!(qnan.fcom(xv), (unordered, true), "fcom(QNaN, {v})");
    }
    // A true signalling NaN still raises #IA under FUCOM, in either position
    // and against finite operands that look like SNaN significands.
    let f25 = reference::from_f64(2.5);
    assert!(snan.is_signaling_nan());
    assert_eq!(snan.fucom(f25), (unordered, true));
    assert_eq!(f25.fucom(snan), (unordered, true));
    assert_eq!(snan.fucom(qnan), (unordered, true));
    assert_eq!(qnan.fucom(snan), (unordered, true));
    // QNaN vs QNaN (incl. one with a payload and the sign set): unordered,
    // no #IA under FUCOM; FCOM raises it.
    assert!(!qnan.is_signaling_nan() && !qnan2.is_signaling_nan());
    assert_eq!(qnan.fucom(qnan2), (unordered, false));
    assert_eq!(qnan2.fucom(qnan), (unordered, false));
    assert_eq!(qnan.fucom(qnan), (unordered, false));
    assert_eq!(qnan.fcom(qnan2), (unordered, true));
    // The SoftFloat-side flag agrees: FUCOM of finite-vs-QNaN leaves it clear.
    let _ = f25.fucom(qnan);
    assert!(!X87::last_flags().invalid());
    // Classification straight from the encoding (SoftFloat rule: exp 0x7FFF,
    // quiet bit clear, payload nonzero).
    assert!(!x(false, 0x4000, 0xA000_0000_0000_0000).is_signaling_nan()); // 2.5
    assert!(!x(false, 0x7FFF, 0x8000_0000_0000_0000).is_signaling_nan()); // +inf
    assert!(x(true, 0x7FFF, 0x8000_0000_0000_0001).is_signaling_nan());
    assert!(!x(false, 0x7FFE, 0xA000_0000_0000_0000).is_signaling_nan());
    assert!(!x(false, 0x0000, 0x2000_0000_0000_0000).is_signaling_nan());
}

#[test]
fn atomic_results_keep_their_own_exception_bits_under_contention() {
    let _serial = serial();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                for iteration in 0..200 {
                    let inexact = (iteration + worker) % 2 == 0;
                    let (result, flags) = X87::from_f32(1.).arithmetic_with_exceptions(
                        X87::from_f32(if inexact { 3. } else { 2. }),
                        if inexact {
                            Arithmetic::Divide
                        } else {
                            Arithmetic::Add
                        },
                        Precision::F32,
                        Rounding::NearestEven,
                    );
                    assert_eq!(flags, u8::from(inexact));
                    assert_eq!(
                        result
                            .to_f32(Precision::F32, Rounding::NearestEven)
                            .to_bits(),
                        if inexact { 0x3eaaaaab } else { 3f32.to_bits() }
                    );
                    // Binary32 represents 2^24 exactly, but cannot represent
                    // the adjacent integer 2^24+1 (ties-to-even rounds down).
                    let (result, flags) =
                        X87::from_f64(if inexact { 16777217. } else { 16777216. })
                            .to_f32_with_exceptions(Precision::F32, Rounding::NearestEven);
                    assert_eq!(result.to_bits(), 16777216f32.to_bits());
                    assert_eq!(flags, u8::from(inexact));
                }
            });
        }
    });
}
