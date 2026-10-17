//! Raw FFI bindings to the vendored SoftFloat 3e `extF80` API plus the
//! `slr_softfloat_shim.c` helpers. All functions are `unsafe` and must be
//! called with [`crate::SF_LOCK`] held (via `lock()` in lib.rs), with the
//! rounding globals programmed first.

/// 80-bit extended float as SoftFloat sees it: 64-bit significand + 16-bit
/// sign/exponent word. `#[repr(C)]` matches `struct extFloat80M` on
/// little-endian (signif first).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtF80 {
    pub signif: u64,
    pub sign_exp: u16,
}

impl ExtF80 {
    /// Decode from the 10-byte little-endian register image.
    pub fn from_bytes(b: [u8; 10]) -> Self {
        Self {
            signif: u64::from_le_bytes(b[0..8].try_into().expect("len 8")),
            sign_exp: u16::from_le_bytes(b[8..10].try_into().expect("len 2")),
        }
    }

    /// Encode to the 10-byte little-endian register image.
    pub fn to_bytes(self) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0..8].copy_from_slice(&self.signif.to_le_bytes());
        out[8..10].copy_from_slice(&self.sign_exp.to_le_bytes());
        out
    }
}

/// SoftFloat rounding modes (subset used by this crate).
#[derive(Clone, Copy)]
pub enum SfMode {
    NearEven = 0,
    MinMag = 1,
    Min = 2,
    Max = 3,
}

/// SoftFloat rounding-precision codes: 32/64/80 select 24/53/64-bit
/// significand rounding for `extF80` arithmetic.
pub enum SfPrec {
    P32 = 32,
    P64 = 64,
    P80 = 80,
}

/// Tininess detection: x87 detects before rounding.
pub enum SfTiny {
    Before = 0,
}

/// Exception flags raised by the last op (SoftFloat-side only; the Unicorn
/// leg does not report x87 flags — docs/x87.md open item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfFlags(pub u8);

impl SfFlags {
    pub const INEXACT: u8 = 1;
    pub const UNDERFLOW: u8 = 2;
    pub const OVERFLOW: u8 = 4;
    pub const INFINITE: u8 = 8;
    pub const INVALID: u8 = 16;

    pub fn invalid(self) -> bool {
        self.0 & Self::INVALID != 0
    }
    pub fn inexact(self) -> bool {
        self.0 & Self::INEXACT != 0
    }
}

unsafe extern "C" {
    fn slr_sf_set_rounding(mode: u8);
    fn slr_sf_set_precision(prec: u8);
    fn slr_sf_set_tininess(t: u8);
    fn slr_sf_clear_flags();
    fn slr_sf_get_flags() -> u8;
    fn softfloat_raiseFlags(flags: u8);

    fn f32_to_extF80(a: u32) -> ExtF80;
    fn f64_to_extF80(a: u64) -> ExtF80;
    fn i32_to_extF80(a: i32) -> ExtF80;
    fn i64_to_extF80(a: i64) -> ExtF80;
    fn extF80_to_f32(a: ExtF80) -> u32;
    fn extF80_to_f64(a: ExtF80) -> u64;
    fn extF80_to_i32(a: ExtF80, mode: u8, exact: bool) -> i32;
    fn extF80_to_i64(a: ExtF80, mode: u8, exact: bool) -> i64;
    fn extF80_add(a: ExtF80, b: ExtF80) -> ExtF80;
    fn extF80_sub(a: ExtF80, b: ExtF80) -> ExtF80;
    fn extF80_mul(a: ExtF80, b: ExtF80) -> ExtF80;
    fn extF80_div(a: ExtF80, b: ExtF80) -> ExtF80;
    fn extF80_sqrt(a: ExtF80) -> ExtF80;
    fn extF80_roundToInt(a: ExtF80, mode: u8, exact: bool) -> ExtF80;
    fn extF80_eq(a: ExtF80, b: ExtF80) -> bool;
    fn extF80_eq_signaling(a: ExtF80, b: ExtF80) -> bool;
    fn extF80_lt(a: ExtF80, b: ExtF80) -> bool;
    fn extF80_lt_quiet(a: ExtF80, b: ExtF80) -> bool;
    fn slr_extF80_fprem_trunc(a: ExtF80, b: ExtF80, c2: *mut bool) -> ExtF80;
}

/// Program the SoftFloat rounding mode (call with the crate lock held).
pub fn set_rounding(m: SfMode) {
    unsafe { slr_sf_set_rounding(m as u8) }
}

/// Program the SoftFloat rounding precision (call with the lock held).
pub fn set_precision(p: SfPrec) {
    unsafe { slr_sf_set_precision(p as u8) }
}

/// Program tininess detection (call with the lock held).
pub fn set_tininess(t: SfTiny) {
    unsafe { slr_sf_set_tininess(t as u8) }
}

/// Clear exception flags (call with the lock held).
pub fn clear_flags() {
    unsafe { slr_sf_clear_flags() }
}

/// Read exception flags (call with the lock held).
pub fn get_flags() -> SfFlags {
    SfFlags(unsafe { slr_sf_get_flags() })
}

/// Raise the invalid flag (call with the lock held).
pub fn raise_invalid() {
    unsafe { softfloat_raiseFlags(SfFlags::INVALID) }
}

pub fn f32_to_ext(v: f32) -> ExtF80 {
    unsafe { f32_to_extF80(v.to_bits()) }
}

pub fn f64_to_ext(v: f64) -> ExtF80 {
    unsafe { f64_to_extF80(v.to_bits()) }
}

pub fn i32_to_ext(v: i32) -> ExtF80 {
    unsafe { i32_to_extF80(v) }
}

pub fn i64_to_ext(v: i64) -> ExtF80 {
    unsafe { i64_to_extF80(v) }
}

pub fn ext_to_f32(a: ExtF80) -> f32 {
    f32::from_bits(unsafe { extF80_to_f32(a) })
}

pub fn ext_to_f64(a: ExtF80) -> f64 {
    f64::from_bits(unsafe { extF80_to_f64(a) })
}

pub fn ext_to_i32(a: ExtF80, mode: SfMode) -> i32 {
    unsafe { extF80_to_i32(a, mode as u8, true) }
}

pub fn ext_to_i64(a: ExtF80, mode: SfMode) -> i64 {
    unsafe { extF80_to_i64(a, mode as u8, true) }
}

pub fn ext_add(a: ExtF80, b: ExtF80) -> ExtF80 {
    unsafe { extF80_add(a, b) }
}

pub fn ext_sub(a: ExtF80, b: ExtF80) -> ExtF80 {
    unsafe { extF80_sub(a, b) }
}

pub fn ext_mul(a: ExtF80, b: ExtF80) -> ExtF80 {
    unsafe { extF80_mul(a, b) }
}

pub fn ext_div(a: ExtF80, b: ExtF80) -> ExtF80 {
    unsafe { extF80_div(a, b) }
}

pub fn ext_sqrt(a: ExtF80) -> ExtF80 {
    unsafe { extF80_sqrt(a) }
}

pub fn ext_round_to_int(a: ExtF80, mode: SfMode) -> ExtF80 {
    unsafe { extF80_roundToInt(a, mode as u8, true) }
}

pub fn ext_eq(a: ExtF80, b: ExtF80) -> bool {
    unsafe { extF80_eq(a, b) }
}

pub fn ext_eq_signaling(a: ExtF80, b: ExtF80) -> bool {
    unsafe { extF80_eq_signaling(a, b) }
}

pub fn ext_lt(a: ExtF80, b: ExtF80) -> bool {
    unsafe { extF80_lt(a, b) }
}

pub fn ext_lt_quiet(a: ExtF80, b: ExtF80) -> bool {
    unsafe { extF80_lt_quiet(a, b) }
}

/// Truncated-quotient FPREM from the C shim. Returns `(remainder, c2)`.
pub fn ext_fprem_trunc(a: ExtF80, b: ExtF80) -> (ExtF80, bool) {
    let mut c2 = false;
    let r = unsafe { slr_extF80_fprem_trunc(a, b, &mut c2) };
    (r, c2)
}
