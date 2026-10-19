//! The exe's custom `sscanf` (`FUN_00553890`, reached through
//! `FUN_00551240`), restricted to the conversions the skeleton reader uses:
//! `%f`, `%d` and literal characters.
//!
//! It is not the C runtime scanner. `%f` accepts `[ws] ['-'] digit+ ['.' digit*]`
//! only (no `+`, no leading `.`, no exponent, no `inf`/`nan`) and stops at the
//! first other byte. The value is accumulated in x87 registers, not parsed
//! correctly-rounded: the integer part is loaded with `FILD` (a 32-bit
//! wrapping accumulator), then for each fractional digit `d`
//! `acc += d * scale; scale *= 0.1`, with `scale` starting at the `f32`
//! constant `0.1f` (`.rdata 0x005f08e8`) and the multiplier the `f64`
//! constant `0.1` (`.rdata 0x005f0d20`). Every `FMUL`/`FADD` rounds to the
//! x87 precision control; the result is stored once as `f32` (`FSTP m32`).
//! Whitespace is bytes `9..=13` and space.

use slr_x87::{Precision, Rounding, X87};

fn is_space(b: u8) -> bool {
    (9..=13).contains(&b) || b == b' '
}

/// Skip whitespace in `s` starting at `*pos`.
pub(crate) fn skip_space(s: &[u8], pos: &mut usize) {
    while s.get(*pos).copied().is_some_and(is_space) {
        *pos += 1;
    }
}

/// Literal format character: must match the input byte exactly.
pub(crate) fn expect(s: &[u8], pos: &mut usize, c: u8) -> bool {
    if s.get(*pos) == Some(&c) {
        *pos += 1;
        true
    } else {
        false
    }
}

/// `%d`: whitespace, optional `-`, at least one digit; 32-bit wrapping.
/// exe: `FUN_00553890` case `'d'`.
pub(crate) fn scan_int(s: &[u8], pos: &mut usize) -> Option<i32> {
    let mut p = *pos;
    while s.get(p).copied().is_some_and(is_space) {
        p += 1;
    }
    let neg = s.get(p) == Some(&b'-');
    if neg {
        p += 1;
    }
    if !s.get(p).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut v: i32 = 0;
    while let Some(d) = s.get(p).filter(|b| b.is_ascii_digit()) {
        v = v.wrapping_mul(10).wrapping_add(i32::from(d - b'0'));
        p += 1;
    }
    *pos = p;
    Some(if neg { v.wrapping_neg() } else { v })
}

/// exe: FUN_00553890
///
/// `%f` at x87 precision control `pc` (case `'f'` of the scanner;
/// machine code at `0x00553b80..0x00553c47`; the Ghidra pseudocode shows
/// `f32` temporaries but the registers are never spilled).
pub(crate) fn scan_float(s: &[u8], pos: &mut usize, pc: Precision) -> Option<f32> {
    let mut p = *pos;
    while s.get(p).copied().is_some_and(is_space) {
        p += 1;
    }
    let neg = s.get(p) == Some(&b'-');
    if neg {
        p += 1;
    }
    if !s.get(p).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut int: u32 = 0;
    while let Some(d) = s.get(p).filter(|b| b.is_ascii_digit()) {
        int = int.wrapping_mul(10).wrapping_add(u32::from(d - b'0'));
        p += 1;
    }
    let mut acc = X87::from_i64(i64::from(int));
    if s.get(p) == Some(&b'.') {
        p += 1;
        let mut scale = X87::from_f32(0.1);
        let ten_th = X87::from_f64(0.1);
        while let Some(d) = s.get(p).filter(|b| b.is_ascii_digit()) {
            let term = X87::from_i32(i32::from(d - b'0')).mul(scale, pc, Rounding::NearestEven);
            acc = acc.add(term, pc, Rounding::NearestEven);
            scale = scale.mul(ten_th, pc, Rounding::NearestEven);
            p += 1;
        }
    }
    let v = acc.to_f32(pc, Rounding::NearestEven);
    *pos = p;
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_x87::Precision;

    fn f(s: &str) -> Option<f32> {
        let mut pos = 0;
        scan_float(s.as_bytes(), &mut pos, Precision::F32)
    }

    #[test]
    fn plain_decimals() {
        assert_eq!(f("1.5"), Some(1.5));
        assert_eq!(f("-0.25"), Some(-0.25));
        assert_eq!(f("42"), Some(42.0));
        assert_eq!(f("  7.0"), Some(7.0));
    }

    #[test]
    fn exe_grammar_is_narrower_than_strtod() {
        // Probed under the Unicorn oracle (FUN_00553890 `%f`): no leading
        // '+', no leading '.', no exponent, no inf/nan.
        assert_eq!(f("+1"), None);
        assert_eq!(f(".5"), None);
        assert_eq!(f("inf"), None);
        // The exponent is not consumed: the token stops at 'e'.
        let mut pos = 0;
        assert_eq!(scan_float(b"1e-3", &mut pos, Precision::F32), Some(1.0));
        assert_eq!(pos, 1);
    }

    #[test]
    fn negative_zero_survives() {
        assert_eq!(f("-0.000000").map(f32::to_bits), Some((-0.0f32).to_bits()));
    }
}
