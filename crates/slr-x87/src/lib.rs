//! x87 80-bit floating-point value type with precision control and rounding.
//!
//! Bit-exact replica of the x87 register model at the game's configuration
//! (precision control PC=24, round-to-nearest), implemented on vendored
//! Berkeley SoftFloat 3e (`extF80`, 8086-SSE specialize). Trusted-operation
//! table: `docs/x87.md` (M0-07 probe suite, issue #8).
//!
//! # Wrapper strategy (from docs/x87.md)
//!
//! SoftFloat always rounds the 64-bit significand unless the global
//! `extF80_roundingPrecision` is set to 32/64. The crate maps PC 24/53/64 to
//! precision 32/64/80 for `fadd`/`fsub`/`fmul`/`fdiv`/`fsqrt` only; every
//! other op runs at precision 80. This matches Intel SDM Vol 1 §8.1.5.2: PC
//! shortens only the significand (low bits zeroed, so a later `FST m32`
//! cannot double-round); the exponent keeps its full 15-bit range.
//!
//! Rounding modes RC 0..3 map to SoftFloat
//! near-even/min(max)/minMag. Tininess is detected before rounding (the x87
//! choice). All SoftFloat calls are serialized through one process-wide mutex
//! because the rounding globals are shared.
//!
//! # Transcendentals
//!
//! `fsin`/`fcos`/`fpatan`/`f2xm1`/`fyl2x` are **APPROX** per docs/x87.md
//! (Unicorn tracks the correctly-rounded reference to ~1 ulp; neither oracle
//! is the real Intel microcode). They live behind explicitly-named
//! `approx_*` methods with a documented tolerance contract, and must NEVER be
//! used where bit-exactness is required. `fprem` is TRUSTED via Unicorn
//! (SDM: exact result, RC ignored) and is bit-exact through the
//! truncated-quotient helper in `src/slr_softfloat_shim.c`.
//!
//! # Licensing
//!
//! The `vendor/softfloat3e` sources are BSD-3-Clause (J. Hauser / UC Regents);
//! see `vendor/softfloat3e/ATTRIBUTION.md` and the preserved `COPYING.txt`.

mod ffi;
pub mod trans;

#[cfg(test)]
mod tests;

use ffi::{ExtF80, SfFlags, SfMode, SfPrec, SfTiny};
use std::sync::{Mutex, MutexGuard};

// Process-wide SoftFloat global-state lock. SoftFloat's rounding mode,
// precision, tininess and exception flags are shared globals; every op sets
// them under this mutex so concurrent `X87` users observe sequential
// x87-FPU semantics.
static SF_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    SF_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// x87 precision control (FPCW bits 8–9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Precision {
    /// 24-bit significand (single precision). The game's configuration.
    F32 = 24,
    /// 53-bit significand (double precision).
    F64 = 53,
    /// 64-bit significand (extended precision).
    Ext = 64,
}

impl Precision {
    fn to_sf(self) -> SfPrec {
        match self {
            Precision::F32 => SfPrec::P32,
            Precision::F64 => SfPrec::P64,
            Precision::Ext => SfPrec::P80,
        }
    }
}

/// x87 rounding control (FPCW bits 10–11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rounding {
    /// Round to nearest, ties to even.
    #[default]
    NearestEven = 0,
    /// Round down (toward −∞).
    Down = 1,
    /// Round up (toward +∞).
    Up = 2,
    /// Round toward zero (truncate).
    Zero = 3,
}

impl Rounding {
    fn to_sf(self) -> SfMode {
        match self {
            Rounding::NearestEven => SfMode::NearEven,
            Rounding::Down => SfMode::Min,
            Rounding::Up => SfMode::Max,
            Rounding::Zero => SfMode::MinMag,
        }
    }
}

/// Condition codes from `fcom`/`fucom` (C3, C2, C0 as a packed triple).
///
/// | ST0 vs ST1 | C3 | C2 | C0 |
/// |------------|----|----|----|
/// | greater    | 0  | 0  | 0  |
/// | less       | 0  | 0  | 1  |
/// | equal      | 1  | 0  | 0  |
/// | unordered  | 1  | 1  | 1  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConditionCodes {
    pub c3: bool,
    pub c2: bool,
    pub c0: bool,
}

impl ConditionCodes {
    fn greater() -> Self {
        Self {
            c3: false,
            c2: false,
            c0: false,
        }
    }
    fn less() -> Self {
        Self {
            c3: false,
            c2: false,
            c0: true,
        }
    }
    fn equal() -> Self {
        Self {
            c3: true,
            c2: false,
            c0: false,
        }
    }
    fn unordered() -> Self {
        Self {
            c3: true,
            c2: true,
            c0: true,
        }
    }
}

/// An x87 80-bit extended-precision register value.
///
/// Stored as the raw 10-byte image: 64-bit significand followed by
/// sign+biased-exponent (little-endian layout, matching `FLD tbyte`).
/// Unnormal encodings (biased exponent nonzero without the integer bit) are
/// representable as images but are NOT supported as arithmetic inputs:
/// real hardware raises `#IA` on them and the two M0-07 oracles disagree
/// there (docs/x87.md "Unnormal note"). Game paths never produce them
/// (`FLD m32/m64`/`FILD` yield normal/denormal/zero/inf/NaN only).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct X87 {
    bits: [u8; 10],
}

impl X87 {
    /// Wrap the raw 10-byte register image (significand ++ signExp, LE).
    #[inline]
    pub fn from_bytes(bytes: [u8; 10]) -> Self {
        Self { bits: bytes }
    }

    /// The raw 10-byte register image.
    #[inline]
    pub fn to_bytes(self) -> [u8; 10] {
        self.bits
    }

    /// Significand (low 64 bits of the image).
    #[inline]
    pub fn significand(self) -> u64 {
        u64::from_le_bytes(self.bits[0..8].try_into().expect("len 8"))
    }

    /// Sign (bit 15 of the exponent word) and biased exponent (bits 0–14).
    #[inline]
    pub fn sign_exp(self) -> (bool, u16) {
        let w = u16::from_le_bytes(self.bits[8..10].try_into().expect("len 2"));
        (w & 0x8000 != 0, w & 0x7FFF)
    }

    /// Sign bit.
    #[inline]
    pub fn sign(self) -> bool {
        self.sign_exp().0
    }

    /// Biased exponent (15 bits).
    #[inline]
    pub fn biased_exp(self) -> u16 {
        self.sign_exp().1
    }

    fn from_ext(v: ExtF80) -> Self {
        Self { bits: v.to_bytes() }
    }

    fn to_ext(self) -> ExtF80 {
        ExtF80::from_bytes(self.bits)
    }

    /// Whether the value is a NaN of any kind.
    pub fn is_nan(self) -> bool {
        let (_, exp) = self.sign_exp();
        exp == 0x7FFF && (self.significand() & 0x7FFF_FFFF_FFFF_FFFF) != 0
    }

    /// Whether the value is a signaling NaN: biased exponent 0x7FFF, quiet bit
    /// (bit 62) clear, and a nonzero payload below it (bits 0-61). This is
    /// SoftFloat's `extF80_isSignalingNaN`, which Intel SDM Vol 1 §4.8.3 agrees
    /// with. The exponent check matters: a finite value such as 2.5
    /// (4000:A000000000000000) has an SNaN-looking significand but is not a NaN.
    pub fn is_signaling_nan(self) -> bool {
        let (_, exp) = self.sign_exp();
        let sig = self.significand();
        exp == 0x7FFF && (sig & 0x4000_0000_0000_0000) == 0 && (sig & 0x3FFF_FFFF_FFFF_FFFF) != 0
    }

    /// Whether the value is infinite.
    pub fn is_inf(self) -> bool {
        let (_, exp) = self.sign_exp();
        exp == 0x7FFF && self.significand() == 0x8000_0000_0000_0000
    }

    /// Whether the value is zero (either sign).
    pub fn is_zero(self) -> bool {
        let (_, exp) = self.sign_exp();
        exp == 0 && self.significand() == 0
    }

    // -- constructors: fld/fild semantics ---------------------------------

    /// `FLD m32`: load an f32 operand exactly (widened, never rounded).
    ///
    /// TRUSTED per docs/x87.md (320/325 agree; the 5 mismatches are the −0.0
    /// operand, which Unicorn's `FLD` normalizes to +0 while SoftFloat
    /// preserves — this constructor preserves −0 like SoftFloat/hardware).
    pub fn from_f32(v: f32) -> Self {
        let _g = lock();
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(SfMode::NearEven);
        Self::from_ext(ffi::f32_to_ext(v))
    }

    /// `FLD m64`: load an f64 operand exactly.
    ///
    /// TRUSTED (same table row as `from_f32`).
    pub fn from_f64(v: f64) -> Self {
        let _g = lock();
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(SfMode::NearEven);
        Self::from_ext(ffi::f64_to_ext(v))
    }

    /// `FILD m32`: load an i32 operand exactly.
    ///
    /// Integer loads are always exact (no rounding possible).
    pub fn from_i32(v: i32) -> Self {
        let _g = lock();
        Self::from_ext(ffi::i32_to_ext(v))
    }

    /// `FILD m64`: load an i64 operand exactly.
    pub fn from_i64(v: i64) -> Self {
        let _g = lock();
        Self::from_ext(ffi::i64_to_ext(v))
    }

    // -- stores: fst/fistp semantics ---------------------------------------

    /// `FST m32`: round to f32 under (PC, RC).
    ///
    /// At PC=24/RC=0 (the game config) register results already carry zeroed
    /// low significand bits, so this store cannot double-round (verified on
    /// the M0-07 double-rounding boundary pairs; docs/x87.md).
    pub fn to_f32(self, pc: Precision, rc: Rounding) -> f32 {
        let _g = lock();
        setup(pc, rc);
        ffi::ext_to_f32(self.to_ext())
    }

    /// `FST m64`: round to f64 under (PC, RC).
    pub fn to_f64(self, pc: Precision, rc: Rounding) -> f64 {
        let _g = lock();
        setup(pc, rc);
        ffi::ext_to_f64(self.to_ext())
    }

    /// `FISTP m32`: round to i32 under RC (PC ignored per SDM).
    ///
    /// TRUSTED per docs/x87.md (284/285 agree; RC honored on both oracles —
    /// 2.5 → 2,2,3,2 across RC0–3; indefinite `0x80000000` on invalid; the
    /// sole mismatch is a flag-surfacing difference on a tiny-subnormal→1
    /// vector, values agree). Out-of-range/NaN/inf yield `0x80000000`
    /// (the indefinite value), matching both oracles.
    pub fn to_i32(self, rc: Rounding) -> i32 {
        let _g = lock();
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(rc.to_sf());
        ffi::set_tininess(SfTiny::Before);
        ffi::clear_flags();
        ffi::ext_to_i32(self.to_ext(), rc.to_sf())
    }

    /// `FISTP m64`: round to i64 under RC (PC ignored per SDM).
    ///
    /// Same trusted row as [`X87::to_i32`]; the i64 store is cross-checked in
    /// the acceptance test against an independent Python-side reference
    /// (mirroring `compare.py::expected_i64`).
    pub fn to_i64(self, rc: Rounding) -> i64 {
        let _g = lock();
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(rc.to_sf());
        ffi::set_tininess(SfTiny::Before);
        ffi::clear_flags();
        ffi::ext_to_i64(self.to_ext(), rc.to_sf())
    }

    // -- basic arithmetic + sqrt: TRUSTED at every (PC, RC) -----------------

    /// `FADD`: `self + other` rounded under (PC, RC). TRUSTED bit-exact.
    pub fn add(self, other: Self, pc: Precision, rc: Rounding) -> Self {
        let _g = lock();
        setup(pc, rc);
        Self::from_ext(ffi::ext_add(self.to_ext(), other.to_ext()))
    }

    /// `FSUB`: `self − other` rounded under (PC, RC). TRUSTED bit-exact.
    pub fn sub(self, other: Self, pc: Precision, rc: Rounding) -> Self {
        let _g = lock();
        setup(pc, rc);
        Self::from_ext(ffi::ext_sub(self.to_ext(), other.to_ext()))
    }

    /// `FMUL`: `self × other` rounded under (PC, RC). TRUSTED bit-exact.
    pub fn mul(self, other: Self, pc: Precision, rc: Rounding) -> Self {
        let _g = lock();
        setup(pc, rc);
        Self::from_ext(ffi::ext_mul(self.to_ext(), other.to_ext()))
    }

    /// `FDIV`: `self ÷ other` rounded under (PC, RC). TRUSTED bit-exact.
    pub fn div(self, other: Self, pc: Precision, rc: Rounding) -> Self {
        let _g = lock();
        setup(pc, rc);
        Self::from_ext(ffi::ext_div(self.to_ext(), other.to_ext()))
    }

    /// `FSQRT`: `√self` rounded under (PC, RC). TRUSTED bit-exact.
    pub fn sqrt(self, pc: Precision, rc: Rounding) -> Self {
        let _g = lock();
        setup(pc, rc);
        Self::from_ext(ffi::ext_sqrt(self.to_ext()))
    }

    /// `FRNDINT`: round to integer under RC (PC ignored per SDM).
    ///
    /// Shares the `FISTP`-trusted SoftFloat primitive (`extF80` round-to-int,
    /// RC-honored: 2.5 → 2,2,3,2 across RC0–3); pinned by the same integer
    /// exactness the `fistp` acceptance cells assert, plus the
    /// `fprem_congruent` partial-cell invariant in `tests/probe_trusted.rs`.
    pub fn frndint(self, rc: Rounding) -> Self {
        let _g = lock();
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(rc.to_sf());
        ffi::set_tininess(SfTiny::Before);
        ffi::clear_flags();
        Self::from_ext(ffi::ext_round_to_int(self.to_ext(), rc.to_sf()))
    }

    // -- remainder: TRUSTED via Unicorn ------------------------------------

    /// `FPREM`: exact remainder with truncated quotient (SDM semantics).
    ///
    /// TRUSTED via Unicorn per docs/x87.md (35/70 raw-agree with SoftFloat's
    /// `extF80_rem`, every mismatch consistent with the FPREM-vs-FPREM1
    /// quotient-rule split; exemplar id 14197: 5.5 rem 2.0 → +1.5). RC is
    /// ignored (exact result); PC is ignored. Returns `(remainder, c2)`.
    ///
    /// Two completion modes, both exact (proven-congruent: `self − remainder`
    /// is always an exact multiple of `divisor`):
    ///
    /// - Gap < 64 (C2 = 0): the complete remainder, bit-exact with hardware.
    /// - Gap ≥ 64 (C2 = 1): hardware performs a partial reduction (one 64-bit
    ///   quotient step) and requires further `FPREM`s; the historical
    ///   QEMU/Unicorn partial *values* at huge gaps are float-rounding
    ///   artifacts (e.g. `2^100 rem 3` surfaces `2^64`, which is not even
    ///   congruent to the dividend), so this method instead returns the
    ///   complete exact remainder with C2 set — callers must still iterate
    ///   like hardware when C2 is set, and must not compare partial *values*
    ///   against QEMU bit-for-bit. The acceptance test pins C2 parity plus
    ///   the congruence invariant for these cells.
    pub fn fprem(self, divisor: Self) -> (Self, bool) {
        let _g = lock();
        // FPREM is exact: force precision-80/near-even so the quotient
        // selection inside the shim never perturbs the exact remainder.
        ffi::set_precision(SfPrec::P80);
        ffi::set_rounding(SfMode::NearEven);
        ffi::set_tininess(SfTiny::Before);
        ffi::clear_flags();
        let (r, c2) = ffi::ext_fprem_trunc(self.to_ext(), divisor.to_ext());
        (Self::from_ext(r), c2)
    }

    // -- comparisons: TRUSTED for all normal inputs -------------------------

    /// `FCOM`: compare with signaling-NaN semantics (any NaN unordered,
    /// SNaN raises invalid). RC/PC have no effect per SDM.
    ///
    /// TRUSTED for all normal inputs incl. NaN/±0/±inf per docs/x87.md
    /// (1565/1625 agree each for fcom/fcomp; the 60 mismatches are
    /// unnormal-operand pairs, unsupported — see [`X87`] type docs).
    /// Returns `(codes, invalid_raised)`.
    pub fn fcom(self, other: Self) -> (ConditionCodes, bool) {
        compare_impl(self, other, true)
    }

    /// `FUCOM`: compare with quiet-NaN semantics (unordered on any NaN,
    /// invalid raised only for SNaN). RC/PC have no effect per SDM.
    ///
    /// Same trusted row as [`X87::fcom`] (`FUCOMI` mapping CF=C0, PF=C2,
    /// ZF=C3 verified in the probe suite).
    pub fn fucom(self, other: Self) -> (ConditionCodes, bool) {
        compare_impl(self, other, false)
    }

    /// Exception flags raised by the last SoftFloat operation on this thread.
    ///
    /// SoftFloat-side only: the Unicorn leg does not report x87 flags, so
    /// flag parity with hardware is NOT covered by the M0-07 probes
    /// (docs/x87.md open item 2). Provided for diagnostics, not for
    /// fidelity claims.
    pub fn last_flags() -> SfFlags {
        let _g = lock();
        ffi::get_flags()
    }
}

/// Program the SoftFloat globals for one trusting op: PC selects precision
/// (basic ops only — callers of PC-independent ops set P80 directly),
/// RC selects the rounding mode, tininess-before-rounding is the x87 choice,
/// flags cleared.
fn setup(pc: Precision, rc: Rounding) {
    ffi::set_precision(pc.to_sf());
    ffi::set_rounding(rc.to_sf());
    ffi::set_tininess(SfTiny::Before);
    ffi::clear_flags();
}

/// Shared compare core. `signaling`: FCOM family (invalid on any NaN) vs
/// FUCOM family (invalid only on SNaN). Mirrors the oracle harness
/// (`softfloat_harness.c` fcom/fucom arms) so bit parity with the SoftFloat
/// leg holds by construction; the acceptance test additionally checks
/// C-bits against the Unicorn leg.
fn compare_impl(a: X87, b: X87, signaling: bool) -> (ConditionCodes, bool) {
    let _g = lock();
    ffi::set_precision(SfPrec::P80);
    ffi::set_rounding(SfMode::NearEven);
    ffi::set_tininess(SfTiny::Before);
    ffi::clear_flags();
    let ae = a.to_ext();
    let be = b.to_ext();
    if a.is_nan() || b.is_nan() {
        // SDM: FCOM raises #IA (invalid) on ANY NaN (QNaN or SNaN);
        // FUCOM raises only on SNaN. Mirrors softfloat_harness.c arms.
        let invalid = signaling || a.is_signaling_nan() || b.is_signaling_nan();
        if invalid {
            ffi::raise_invalid();
        }
        return (ConditionCodes::unordered(), invalid);
    }
    // FCOM uses signaling comparisons (QNaN impossible here, but SNaN bit
    // patterns with cleared payload edge cases route through SoftFloat's
    // raising ops); FUCOM uses quiet comparisons. For finite operands both
    // reduce to the same ordered result.
    let (lt, eq) = if signaling {
        (ffi::ext_lt(ae, be), ffi::ext_eq_signaling(ae, be))
    } else {
        (ffi::ext_lt_quiet(ae, be), ffi::ext_eq(ae, be))
    };
    ffi::clear_flags();
    if eq {
        (ConditionCodes::equal(), false)
    } else if lt {
        (ConditionCodes::less(), false)
    } else {
        (ConditionCodes::greater(), false)
    }
}

impl std::fmt::Debug for X87 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (sign, exp) = self.sign_exp();
        write!(
            f,
            "X87({}{:#06x}:0x{:016x})",
            if sign { "-" } else { "+" },
            exp,
            self.significand()
        )
    }
}

impl std::fmt::Display for X87 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
